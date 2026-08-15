//! 游戏平台公开状态 API（无用户 Cookie）
//!
//! 仅使用公开标识（UID / Gamertag / Online ID）拉取：
//! - Hoyoverse 展柜：Enka.Network（原神 / 星铁 / 绝区零）
//! - Xbox：OpenXBL（需服务端 OPENXBL_API_KEY）
//! - PlayStation：PSN 非公开 API（需服务端 PSN_NPSSO，只读他人公开资料）
//!
//! 标识符存在前端小组件 config 中，不在全局配置页新增设置项。

use crate::config::DynamicConfig;
use crate::error::HttpError;
use axum::{extract::Query, extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::services::outbound_security::build_public_http_client;

// Response types

#[derive(Debug, Serialize, Clone)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub message: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct GamePresenceData {
    pub platform: String,
    pub identity: GameIdentity,
    pub score: Option<GameScore>,
    pub presence: Option<GamePresenceInfo>,
    pub highlights: Vec<GameHighlight>,
    pub showcase: Vec<ShowcaseItem>,
    pub profile_url: Option<String>,
    pub fetched_at: String,
    /// 数据是否因服务端密钥缺失而降级
    pub degraded: bool,
    pub degrade_reason: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct GameIdentity {
    pub id: String,
    pub name: String,
    pub avatar: Option<String>,
    pub subtitle: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct GameScore {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct GamePresenceInfo {
    pub status: String,
    pub title: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct GameHighlight {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct ShowcaseItem {
    pub name: String,
    pub level: Option<i64>,
    pub icon: Option<String>,
    /// 大幅立绘（聚焦展示用）
    pub art: Option<String>,
    pub rarity: Option<i64>,
}

// Query

#[derive(Debug, Deserialize)]
pub struct PresenceQuery {
    /// hoyolab | xbox | psn
    pub platform: String,
    /// UID / Gamertag / Online ID
    pub id: String,
    /// hoyolab 子游戏：genshin | hsr | zzz（默认 genshin）
    pub game: Option<String>,
    /// 角色名本地化语言（zh / en / ja，默认 zh）
    pub lang: Option<String>,
}

// Cache (per platform+id+game, stale-while-revalidate style)

#[derive(Clone)]
enum CachedResult {
    Ok(Box<GamePresenceData>),
    Err(String),
}

struct CacheEntry {
    result: CachedResult,
    fetched_at: Instant,
    /// live presence（在线状态/正在玩）上次刷新时间。
    /// Xbox / PSN 报告卡把这个接口当实时状态用，presence 部分单独走短 TTL；
    /// 其余平台（Enka 展柜）没有 live 数据，该字段恒等于 fetched_at。
    presence_refreshed_at: Instant,
}

static PRESENCE_CACHE: OnceLock<Mutex<HashMap<String, CacheEntry>>> = OnceLock::new();

fn cache_map() -> &'static Mutex<HashMap<String, CacheEntry>> {
    PRESENCE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 展柜/成就类数据变化以天计，6 小时一次足够新鲜
const CACHE_TTL: Duration = Duration::from_secs(6 * 3600);
/// Xbox / PSN 的 live presence 短 TTL：前端报告卡 120s 轮询在线状态，
/// 必须明显短于轮询间隔缓存才有意义。刷新只花 1 次上游调用
/// （身份/GS/奖杯沿用 6h 快照），配额由 credential-spend guard 兜底。
const PRESENCE_TTL: Duration = Duration::from_secs(90);
/// 失败结果（无效 id / 上游拒绝等）缓存更短，避免一直被当活的打上游，
/// 但也不会因为长期缓存把后来纠正过的 id 也一直判定失败。
const ERROR_CACHE_TTL: Duration = Duration::from_secs(30);

fn cache_key(platform: &str, id: &str, game: &str, lang: &str) -> String {
    format!("{}:{}:{}:{}", platform.to_ascii_lowercase(), id, game, lang)
}

enum CacheLookup {
    Hit(CachedResult),
    /// 快照仍新鲜，但 live presence 过期：由本次请求负责单独刷新。
    /// 返回前已抢占更新时间戳，并发请求只有第一个会打上游，其余先用旧 presence。
    RefreshPresence(Box<GamePresenceData>),
    Miss,
}

fn read_cache(key: &str, has_live_presence: bool) -> CacheLookup {
    let Ok(mut guard) = cache_map().lock() else {
        return CacheLookup::Miss;
    };
    let Some(entry) = guard.get_mut(key) else {
        return CacheLookup::Miss;
    };
    match &entry.result {
        CachedResult::Err(_) => {
            if entry.fetched_at.elapsed() < ERROR_CACHE_TTL {
                CacheLookup::Hit(entry.result.clone())
            } else {
                CacheLookup::Miss
            }
        }
        CachedResult::Ok(data) => {
            if entry.fetched_at.elapsed() >= CACHE_TTL {
                return CacheLookup::Miss;
            }
            if has_live_presence
                && !data.degraded
                && entry.presence_refreshed_at.elapsed() >= PRESENCE_TTL
            {
                entry.presence_refreshed_at = Instant::now();
                return CacheLookup::RefreshPresence(data.clone());
            }
            CacheLookup::Hit(entry.result.clone())
        }
    }
}

/// presence 单独刷新成功后回写缓存（不动 fetched_at，快照 6h 过期节奏不变）
fn update_cached_presence(key: &str, presence: &GamePresenceInfo) {
    if let Ok(mut guard) = cache_map().lock() {
        if let Some(entry) = guard.get_mut(key) {
            if let CachedResult::Ok(data) = &mut entry.result {
                data.presence = Some(presence.clone());
            }
        }
    }
}

fn store_cache(key: &str, result: CachedResult) {
    if let Ok(mut guard) = cache_map().lock() {
        let now = Instant::now();
        guard.insert(
            key.to_string(),
            CacheEntry {
                result,
                fetched_at: now,
                presence_refreshed_at: now,
            },
        );
        // 简单上限，避免无限增长
        if guard.len() > 256 {
            let oldest: Vec<String> = guard
                .iter()
                .filter(|(_, e)| e.fetched_at.elapsed() > CACHE_TTL * 2)
                .map(|(k, _)| k.clone())
                .collect();
            for k in oldest {
                guard.remove(&k);
            }
        }
    }
}

// Credential-spend guard (Xbox OpenXBL / PSN NPSSO)
//
// 这个接口本身必须公开（无 Cookie 的展示型小组件，访客不登录也要能看到），
// 不能像 /api/x/user 那样直接挂 auth_middleware。但 Xbox / PSN 分支花的是
// 服务端自己的第三方凭据（OPENXBL_API_KEY / PSN_NPSSO），换 IP 或换 id 就能绕开
// 按 IP 算的全局限流。这里单独给"真正花凭据的上游请求"加一个和调用方身份无关的
// 全局节流，兜底防止配额被刷爆或触发 Sony/Xbox 的异常访问检测。

#[derive(Clone, Copy)]
enum Platform {
    Xbox,
    Psn,
}

struct SpendWindow {
    window_start: Instant,
    count: usize,
}

static XBOX_SPEND: OnceLock<Mutex<SpendWindow>> = OnceLock::new();
static PSN_SPEND: OnceLock<Mutex<SpendWindow>> = OnceLock::new();

const SPEND_WINDOW: Duration = Duration::from_secs(60);
const SPEND_MAX: usize = 20;

fn try_spend_credential_call(platform: Platform) -> bool {
    let lock = match platform {
        Platform::Xbox => XBOX_SPEND.get_or_init(|| {
            Mutex::new(SpendWindow {
                window_start: Instant::now(),
                count: 0,
            })
        }),
        Platform::Psn => PSN_SPEND.get_or_init(|| {
            Mutex::new(SpendWindow {
                window_start: Instant::now(),
                count: 0,
            })
        }),
    };
    let Ok(mut state) = lock.lock() else {
        return true;
    };
    if state.window_start.elapsed() > SPEND_WINDOW {
        state.window_start = Instant::now();
        state.count = 0;
    }
    if state.count >= SPEND_MAX {
        false
    } else {
        state.count += 1;
        true
    }
}

// Media normalize at response edge (identity with profile::proxy_image_url)
// Enka / Xbox / PSN 当前不在 needs_image_proxy 窄名单内，多为恒等变换；
// 保留出口统一处理，避免日后某 CDN 变防盗链时漏改。

fn proxy_presence_media(mut data: GamePresenceData) -> GamePresenceData {
    use crate::api::profile::proxy_image_url;
    if let Some(av) = data.identity.avatar.take() {
        data.identity.avatar = Some(proxy_image_url(&av));
    }
    for item in &mut data.showcase {
        if let Some(icon) = item.icon.take() {
            item.icon = Some(proxy_image_url(&icon));
        }
        if let Some(art) = item.art.take() {
            item.art = Some(proxy_image_url(&art));
        }
    }
    data
}

// Handler

pub async fn get_game_presence(
    State(dynamic_config): State<Arc<RwLock<DynamicConfig>>>,
    Query(q): Query<PresenceQuery>,
) -> Result<Json<ApiResponse<GamePresenceData>>, HttpError> {
    let platform = q.platform.trim().to_ascii_lowercase();
    let account_id = q.id.trim().to_string();
    let game = q
        .game
        .as_deref()
        .unwrap_or("genshin")
        .trim()
        .to_ascii_lowercase();

    if account_id.is_empty() {
        return Ok(Json(ApiResponse {
            success: false,
            data: None,
            message: "Missing account id".to_string(),
        }));
    }

    // 基础校验，防滥用。放开到 Unicode 字母数字 + `#`——
    // 现代 Xbox gamertag 是"名字#四位数字"格式，且不少地区的 gamertag/在线 ID 本身就带非 ASCII 字符；
    // 实际拼上游 URL 时 urlencoding_simple 按字节 percent-encode，本来就能正确处理这些字符。
    if account_id.len() > 64
        || !account_id
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' ' | '@' | '.' | '#'))
    {
        return Ok(Json(ApiResponse {
            success: false,
            data: None,
            message: "Invalid account id format".to_string(),
        }));
    }

    let lang = match q.lang.as_deref().map(str::trim) {
        Some(l) if l.starts_with("en") => "en",
        Some(l) if l.starts_with("ja") => "ja",
        _ => "zh",
    };

    let key = cache_key(&platform, &account_id, &game, lang);
    let has_live_presence = matches!(platform.as_str(), "xbox" | "psn" | "playstation");
    match read_cache(&key, has_live_presence) {
        CacheLookup::Hit(cached) => {
            return Ok(Json(match cached {
                CachedResult::Ok(data) => ApiResponse {
                    success: true,
                    // 缓存里存原始 URL；出口统一代理，兼容旧缓存直链
                    data: Some(proxy_presence_media(*data)),
                    message: "ok (cache)".to_string(),
                },
                CachedResult::Err(msg) => ApiResponse {
                    success: false,
                    data: None,
                    message: msg,
                },
            }));
        }
        CacheLookup::RefreshPresence(mut data) => {
            let refreshed = match platform.as_str() {
                "xbox" => refresh_xbox_presence(&data, &dynamic_config).await,
                _ => refresh_psn_presence(&data, &dynamic_config).await,
            };
            match refreshed {
                Ok(presence) => {
                    update_cached_presence(&key, &presence);
                    data.presence = Some(presence);
                }
                // 刷新失败就先给旧 presence，下个 PRESENCE_TTL 窗口再试
                Err(e) => tracing::debug!("presence refresh failed ({key}): {e}"),
            }
            return Ok(Json(ApiResponse {
                success: true,
                data: Some(proxy_presence_media(*data)),
                message: "ok (cache+presence)".to_string(),
            }));
        }
        CacheLookup::Miss => {}
    }

    let result = match platform.as_str() {
        "hoyolab" | "hoyoverse" | "miyoushe" | "enka" => fetch_enka(&account_id, &game, lang).await,
        "xbox" => fetch_xbox(&account_id, &dynamic_config).await,
        "psn" | "playstation" => fetch_psn(&account_id, &dynamic_config).await,
        _ => Err(format!("Unsupported platform: {platform}")),
    };

    match result {
        Ok(data) => {
            // 缓存存未代理 URL，避免代理路径随部署 base 变化后失效
            store_cache(&key, CachedResult::Ok(Box::new(data.clone())));
            Ok(Json(ApiResponse {
                success: true,
                data: Some(proxy_presence_media(data)),
                message: "ok".to_string(),
            }))
        }
        Err(msg) => {
            tracing::warn!("game presence fetch failed ({platform}/{account_id}): {msg}");
            store_cache(&key, CachedResult::Err(msg.clone()));
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: msg,
            }))
        }
    }
}

// Enka.Network (Hoyoverse showcase)

/// 展柜条目上限（前端 3x2 网格）
const SHOWCASE_LIMIT: usize = 6;

/// 标签本地化：展柜数据面向访客展示，跟随前端语言
fn hl(lang: &str, zh: &str, en: &str, ja: &str) -> String {
    match lang {
        "en" => en.to_string(),
        "ja" => ja.to_string(),
        _ => zh.to_string(),
    }
}

async fn fetch_enka(uid: &str, game: &str, lang: &str) -> Result<GamePresenceData, String> {
    if !uid.chars().all(|c| c.is_ascii_digit()) || uid.len() < 5 || uid.len() > 12 {
        return Err("UID must be 5–12 digits".to_string());
    }

    // 注意：路径不能带尾斜杠。Enka 会把 `/api/uid/{uid}/?info` 308 重定向到
    // `/api/uid/{uid}?info`，而我们的 HTTP 客户端为防 SSRF 禁用了重定向，
    // 带斜杠的写法会直接拿到空 body 的 308 报错。
    // 原神用 `?info` 精简变体（保留 showAvatarInfoList 摘要）；
    // 星铁 / 绝区零的 info 变体不保证带展柜列表，用完整响应。
    let ua = "Myriad/1.0 (game-presence; +https://github.com)";
    match game {
        "hsr" | "starrail" | "star_rail" => {
            let body =
                http_get_json(&format!("https://enka.network/api/hsr/uid/{uid}"), ua).await?;
            parse_enka_hsr(uid, lang, &body).await
        }
        "zzz" | "zenless" => {
            let body =
                http_get_json(&format!("https://enka.network/api/zzz/uid/{uid}"), ua).await?;
            parse_enka_zzz(uid, lang, &body).await
        }
        _ => {
            let body =
                http_get_json(&format!("https://enka.network/api/uid/{uid}?info"), ua).await?;
            parse_enka_gi(uid, lang, &body).await
        }
    }
}

/// 原神：playerInfo（camelCase）
async fn parse_enka_gi(uid: &str, lang: &str, body: &Value) -> Result<GamePresenceData, String> {
    let player = body
        .get("playerInfo")
        .ok_or_else(|| "Enka response missing playerInfo".to_string())?;

    let nickname = player
        .get("nickname")
        .and_then(|v| v.as_str())
        .unwrap_or(uid)
        .to_string();
    let level = player.get("level").and_then(|v| v.as_i64());
    let signature = player
        .get("signature")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // 资料头像：新版接口给 pfp id，旧版给 avatarId
    let avatar = crate::services::enka_assets::gi_profile_picture(
        player
            .pointer("/profilePicture/id")
            .and_then(|v| v.as_i64()),
        player
            .pointer("/profilePicture/avatarId")
            .and_then(|v| v.as_i64()),
    )
    .await;

    let mut showcase = Vec::new();
    if let Some(arr) = player.get("showAvatarInfoList").and_then(|v| v.as_array()) {
        for item in arr.iter().take(SHOWCASE_LIMIT) {
            let Some(avatar_id) = item.get("avatarId").and_then(|v| v.as_i64()) else {
                continue;
            };
            let meta = crate::services::enka_assets::gi_character(avatar_id, lang).await;
            showcase.push(ShowcaseItem {
                name: meta.name.unwrap_or_else(|| format!("#{avatar_id}")),
                level: item.get("level").and_then(|v| v.as_i64()),
                icon: meta.icon,
                art: meta.art,
                rarity: meta.rarity,
            });
        }
    }

    let mut highlights = Vec::new();
    if let Some(a) = player.get("finishAchievementNum").and_then(|v| v.as_i64()) {
        highlights.push(GameHighlight {
            label: hl(lang, "成就", "Achievements", "アチーブメント"),
            value: a.to_string(),
        });
    }
    if let (Some(f), Some(l)) = (
        player.get("towerFloorIndex").and_then(|v| v.as_i64()),
        player.get("towerLevelIndex").and_then(|v| v.as_i64()),
    ) {
        if f > 0 {
            highlights.push(GameHighlight {
                label: hl(lang, "深渊", "Abyss", "深境螺旋"),
                value: format!("{f}-{l}"),
            });
        }
    }

    Ok(GamePresenceData {
        platform: "hoyolab".to_string(),
        identity: GameIdentity {
            id: uid.to_string(),
            name: nickname,
            avatar,
            subtitle: signature,
        },
        score: level.map(|l| GameScore {
            label: hl(lang, "冒险等阶", "AR", "冒険ランク"),
            value: l.to_string(),
        }),
        presence: None,
        highlights,
        showcase,
        profile_url: Some(format!("https://enka.network/u/{uid}")),
        fetched_at: chrono::Utc::now().to_rfc3339(),
        degraded: false,
        degrade_reason: None,
    })
}

/// 星铁：detailInfo（camelCase），成就等在 recordInfo
async fn parse_enka_hsr(uid: &str, lang: &str, body: &Value) -> Result<GamePresenceData, String> {
    let player = body
        .get("detailInfo")
        .ok_or_else(|| "Enka response missing detailInfo".to_string())?;

    let nickname = player
        .get("nickname")
        .and_then(|v| v.as_str())
        .unwrap_or(uid)
        .to_string();
    let level = player.get("level").and_then(|v| v.as_i64());
    let signature = player
        .get("signature")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut showcase = Vec::new();
    if let Some(arr) = player.get("avatarDetailList").and_then(|v| v.as_array()) {
        for item in arr.iter().take(SHOWCASE_LIMIT) {
            let Some(avatar_id) = item.get("avatarId").and_then(|v| v.as_i64()) else {
                continue;
            };
            let meta = crate::services::enka_assets::hsr_character(avatar_id, lang).await;
            showcase.push(ShowcaseItem {
                name: meta.name.unwrap_or_else(|| format!("#{avatar_id}")),
                level: item.get("level").and_then(|v| v.as_i64()),
                icon: meta.icon,
                art: meta.art,
                rarity: meta.rarity,
            });
        }
    }

    let record = player.get("recordInfo");
    let mut highlights = Vec::new();
    if let Some(a) = record
        .and_then(|r| r.get("achievementCount"))
        .and_then(|v| v.as_i64())
    {
        highlights.push(GameHighlight {
            label: hl(lang, "成就", "Achievements", "アチーブメント"),
            value: a.to_string(),
        });
    }
    if let Some(c) = record
        .and_then(|r| r.get("avatarCount"))
        .and_then(|v| v.as_i64())
    {
        highlights.push(GameHighlight {
            label: hl(lang, "角色", "Characters", "キャラ"),
            value: c.to_string(),
        });
    }

    Ok(GamePresenceData {
        platform: "hoyolab".to_string(),
        identity: GameIdentity {
            id: uid.to_string(),
            name: nickname,
            avatar: None,
            subtitle: signature,
        },
        score: level.map(|l| GameScore {
            label: hl(lang, "开拓等级", "Trailblaze", "開拓レベル"),
            value: l.to_string(),
        }),
        presence: None,
        highlights,
        showcase,
        profile_url: Some(format!("https://enka.network/hsr/{uid}")),
        fetched_at: chrono::Utc::now().to_rfc3339(),
        degraded: false,
        degrade_reason: None,
    })
}

/// 绝区零：PlayerInfo（PascalCase），资料在 SocialDetail，展柜在 ShowcaseDetail
async fn parse_enka_zzz(uid: &str, lang: &str, body: &Value) -> Result<GamePresenceData, String> {
    let player = body
        .get("PlayerInfo")
        .ok_or_else(|| "Enka response missing PlayerInfo".to_string())?;
    let profile = player.pointer("/SocialDetail/ProfileDetail");

    let nickname = profile
        .and_then(|p| p.get("Nickname"))
        .and_then(|v| v.as_str())
        .unwrap_or(uid)
        .to_string();
    let level = profile
        .and_then(|p| p.get("Level"))
        .and_then(|v| v.as_i64());
    let signature = player
        .pointer("/SocialDetail/Desc")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // 资料头像：ProfileDetail.AvatarId 指向展示的角色
    let avatar = match profile
        .and_then(|p| p.get("AvatarId"))
        .and_then(|v| v.as_i64())
    {
        Some(id) => {
            crate::services::enka_assets::zzz_character(id, lang)
                .await
                .icon
        }
        None => None,
    };

    let mut showcase = Vec::new();
    if let Some(arr) = player
        .pointer("/ShowcaseDetail/AvatarList")
        .and_then(|v| v.as_array())
    {
        for item in arr.iter().take(SHOWCASE_LIMIT) {
            let Some(avatar_id) = item.get("Id").and_then(|v| v.as_i64()) else {
                continue;
            };
            let meta = crate::services::enka_assets::zzz_character(avatar_id, lang).await;
            showcase.push(ShowcaseItem {
                name: meta.name.unwrap_or_else(|| format!("#{avatar_id}")),
                level: item.get("Level").and_then(|v| v.as_i64()),
                icon: meta.icon,
                art: meta.art,
                // ZZZ：4 = S 级、3 = A 级，映射到通用五星制方便前端统一判断
                rarity: meta.rarity.map(|r| if r >= 4 { 5 } else { 4 }),
            });
        }
    }

    let mut highlights = Vec::new();
    if let Some(medals) = player
        .pointer("/SocialDetail/MedalList")
        .and_then(|v| v.as_array())
    {
        if !medals.is_empty() {
            highlights.push(GameHighlight {
                label: hl(lang, "勋章", "Medals", "メダル"),
                value: medals.len().to_string(),
            });
        }
    }
    if let Some(title) = profile
        .and_then(|p| p.pointer("/Title/Title"))
        .and_then(|v| v.as_i64())
    {
        // 有称号 id 但没有本地化表，先不展示具体称号文本
        let _ = title;
    }

    Ok(GamePresenceData {
        platform: "hoyolab".to_string(),
        identity: GameIdentity {
            id: uid.to_string(),
            name: nickname,
            avatar,
            subtitle: signature,
        },
        score: level.map(|l| GameScore {
            label: hl(lang, "绳网等级", "Inter-Knot", "インターノット"),
            value: l.to_string(),
        }),
        presence: None,
        highlights,
        showcase,
        profile_url: Some(format!("https://enka.network/zzz/{uid}")),
        fetched_at: chrono::Utc::now().to_rfc3339(),
        degraded: false,
        degrade_reason: None,
    })
}

// Xbox via OpenXBL

/// 优先读 DB 配置（配置页保存后即时生效），env 作为回退
async fn xbox_api_key(dynamic_config: &Arc<RwLock<DynamicConfig>>) -> String {
    let config = dynamic_config.read().await;
    config
        .openxbl_api_key
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("OPENXBL_API_KEY").ok())
        .or_else(|| std::env::var("XBL_API_KEY").ok())
        .unwrap_or_default()
}

/// 解析 OpenXBL presence 响应（数组 / 对象两种形态），返回 (state, 正在玩的标题)
fn parse_xbox_presence(pres_raw: Value) -> (Option<String>, Option<String>) {
    let pres = openxbl_unwrap_content(pres_raw);
    let node = pres
        .as_array()
        .and_then(|a| a.first())
        .cloned()
        .unwrap_or(pres);
    let status = node
        .get("state")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let mut title: Option<String> = None;
    if let Some(devices) = node.get("devices").and_then(|v| v.as_array()) {
        // 多设备同时在线时（比如手机开着 Xbox App、主机在玩游戏），
        // 一旦某个设备给出 Full/Fill 占位的 title 就认定是"正在玩"，
        // 不能让后面设备的 title 再覆盖掉——所以命中后要跳出外层循环，
        // 而不只是内层的 titles 循环。
        'devices: for dev in devices {
            if let Some(titles) = dev.get("titles").and_then(|v| v.as_array()) {
                for t in titles {
                    let name = t.get("name").and_then(|v| v.as_str());
                    let placement = t.get("placement").and_then(|v| v.as_str());
                    if placement == Some("Full") || placement == Some("Fill") {
                        if let Some(n) = name {
                            title = Some(n.to_string());
                            break 'devices;
                        }
                    }
                    if title.is_none() {
                        if let Some(n) = name {
                            if n != "Home" {
                                title = Some(n.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    (status, title)
}

/// 仅刷新 Xbox live presence：1 次上游调用，身份 / GS 沿用 6h 快照
async fn refresh_xbox_presence(
    data: &GamePresenceData,
    dynamic_config: &Arc<RwLock<DynamicConfig>>,
) -> Result<GamePresenceInfo, String> {
    let xuid = data.identity.id.trim();
    // 快照没解析出 xuid 时 identity.id 是 gamertag，无法走 presence 端点
    if xuid.is_empty() || !xuid.chars().all(|c| c.is_ascii_digit()) {
        return Err("no xuid in cached snapshot".to_string());
    }
    let api_key = xbox_api_key(dynamic_config).await;
    if api_key.trim().is_empty() {
        return Err("OPENXBL_API_KEY not configured".to_string());
    }
    if !try_spend_credential_call(Platform::Xbox) {
        return Err("Xbox credential budget exhausted".to_string());
    }
    let presence_url = format!("https://xbl.io/api/v2/presence/{xuid}");
    let pres_raw = http_get_json_with_header(
        &presence_url,
        "Myriad/1.0 (game-presence)",
        &[("X-Authorization", api_key.as_str())],
    )
    .await?;
    let (status, title) = parse_xbox_presence(pres_raw);
    Ok(GamePresenceInfo {
        status: status.unwrap_or_else(|| "Unknown".into()),
        title,
        detail: None,
    })
}

async fn fetch_xbox(
    gamertag: &str,
    dynamic_config: &Arc<RwLock<DynamicConfig>>,
) -> Result<GamePresenceData, String> {
    let api_key = xbox_api_key(dynamic_config).await;

    if api_key.trim().is_empty() {
        // 降级：仅返回标识 + 公开主页链接
        return Ok(GamePresenceData {
            platform: "xbox".to_string(),
            identity: GameIdentity {
                id: gamertag.to_string(),
                name: gamertag.to_string(),
                avatar: None,
                subtitle: Some("OpenXBL API key not configured".into()),
            },
            score: None,
            presence: None,
            highlights: vec![],
            showcase: vec![],
            profile_url: Some(format!(
                "https://www.xbox.com/play/user/{}",
                urlencoding_simple(gamertag)
            )),
            fetched_at: chrono::Utc::now().to_rfc3339(),
            degraded: true,
            degrade_reason: Some(
                "Set OPENXBL_API_KEY on the server to load Gamerscore and presence".into(),
            ),
        });
    }

    if !try_spend_credential_call(Platform::Xbox) {
        return Err("Too many Xbox lookups right now, try again in a bit".to_string());
    }

    // Search player（OpenXBL 返回 { content: {...}, code }，先解包）
    // 现代 gamertag 可含 #suffix（如 染川瞳#6234），搜索时去掉后缀
    let search_term = gamertag.split('#').next().unwrap_or(gamertag).trim();
    let search_url = format!(
        "https://xbl.io/api/v2/search/{}",
        urlencoding_simple(search_term)
    );
    let search_raw = http_get_json_with_header(
        &search_url,
        "Myriad/1.0 (game-presence)",
        &[("X-Authorization", api_key.as_str())],
    )
    .await?;
    let search = openxbl_unwrap_content(search_raw);

    // OpenXBL search shapes vary; try common paths
    let person = search
        .get("people")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .cloned()
        .or_else(|| {
            search
                .get("profileUsers")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .cloned()
        })
        .unwrap_or(search.clone());

    let xuid = person
        .get("xuid")
        .or_else(|| person.get("id"))
        .and_then(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        })
        .unwrap_or_default();

    let display_name = person
        .get("gamertag")
        .or_else(|| person.get("modernGamertag"))
        .or_else(|| person.get("uniqueModernGamertag"))
        .and_then(|v| v.as_str())
        .unwrap_or(gamertag)
        .to_string();

    let mut gamerscore: Option<String> = person
        .get("gamerScore")
        .or_else(|| person.get("gamerscore"))
        .and_then(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .or_else(|| v.as_i64().map(|n| n.to_string()))
        });

    let mut avatar = person
        .get("displayPicRaw")
        .or_else(|| person.get("displayPicUri"))
        .or_else(|| person.get("gamerpic"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut presence_status: Option<String> = None;
    let mut presence_title: Option<String> = None;

    if !xuid.is_empty() {
        // Presence
        let presence_url = format!("https://xbl.io/api/v2/presence/{xuid}");
        if let Ok(pres_raw) = http_get_json_with_header(
            &presence_url,
            "Myriad/1.0 (game-presence)",
            &[("X-Authorization", api_key.as_str())],
        )
        .await
        {
            let (status, title) = parse_xbox_presence(pres_raw);
            presence_status = status;
            presence_title = title;
        }

        // Account details for gamerscore / avatar if missing
        if gamerscore.is_none() || avatar.is_none() {
            let acc_url = format!("https://xbl.io/api/v2/account/{xuid}");
            if let Ok(acc_raw) = http_get_json_with_header(
                &acc_url,
                "Myriad/1.0 (game-presence)",
                &[("X-Authorization", api_key.as_str())],
            )
            .await
            {
                let acc = openxbl_unwrap_content(acc_raw);
                let settings = acc
                    .get("profileUsers")
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.first())
                    .and_then(|u| u.get("settings"))
                    .and_then(|s| s.as_array());
                if let Some(settings) = settings {
                    for s in settings {
                        let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let val = s.get("value").and_then(|v| v.as_str()).unwrap_or("");
                        match id {
                            "Gamerscore" if gamerscore.is_none() => {
                                gamerscore = Some(val.to_string());
                            }
                            "GameDisplayPicRaw" | "PublicGamerpic" if avatar.is_none() => {
                                avatar = Some(val.to_string());
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    let mut highlights = Vec::new();
    if let Some(ref gs) = gamerscore {
        highlights.push(GameHighlight {
            label: "Gamerscore".into(),
            value: gs.clone(),
        });
    }

    Ok(GamePresenceData {
        platform: "xbox".to_string(),
        identity: GameIdentity {
            id: if xuid.is_empty() {
                gamertag.to_string()
            } else {
                xuid
            },
            name: display_name,
            avatar,
            subtitle: None,
        },
        score: gamerscore.map(|v| GameScore {
            label: "GS".into(),
            value: v,
        }),
        presence: Some(GamePresenceInfo {
            status: presence_status.unwrap_or_else(|| "Unknown".into()),
            title: presence_title,
            detail: None,
        }),
        highlights,
        showcase: vec![],
        profile_url: Some(format!(
            "https://www.xbox.com/play/user/{}",
            urlencoding_simple(gamertag)
        )),
        fetched_at: chrono::Utc::now().to_rfc3339(),
        degraded: false,
        degrade_reason: None,
    })
}

// PlayStation (optional server NPSSO)

/// 优先读 DB 配置（配置页保存后即时生效），env 作为回退
async fn psn_npsso(dynamic_config: &Arc<RwLock<DynamicConfig>>) -> String {
    let config = dynamic_config.read().await;
    config
        .psn_npsso
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("PSN_NPSSO").ok())
        .unwrap_or_default()
}

/// legacy profile2 的 presences[0] → (onlineStatus, titleName)
fn parse_psn_legacy_presence(profile: &Value) -> (Option<String>, Option<String>) {
    let pres = profile
        .get("presences")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first());
    // Prefer onlineStatus; fall back to availability, then primaryOnlineStatus on profile root.
    let status = pres
        .and_then(|p| {
            p.get("onlineStatus")
                .and_then(|v| v.as_str())
                .or_else(|| p.get("availability").and_then(|v| v.as_str()))
        })
        .or_else(|| {
            profile
                .get("primaryOnlineStatus")
                .and_then(|v| v.as_str())
        })
        .map(normalize_psn_online_status);
    let title = pres
        .and_then(|p| p.get("titleName"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    (status, title)
}

/// Map PSN availability / onlineStatus strings to FE-friendly status labels.
/// `availableToPlay` is online but not always mirrored under primaryPlatformInfo.onlineStatus.
fn normalize_psn_online_status(raw: &str) -> String {
    let lower = raw.trim().to_ascii_lowercase();
    // availableToPlay / available — treat as online for presence widgets
    if lower == "availabletoplay"
        || lower == "available"
        || (lower.contains("available") && !lower.contains("unavailable"))
    {
        return "online".to_string();
    }
    raw.to_string()
}

/// basicPresences 响应 → (onlineStatus, titleName)
fn parse_psn_basic_presence(pres: &Value) -> (Option<String>, Option<String>) {
    let Some(bp) = pres.get("basicPresence") else {
        return (None, None);
    };
    let status = bp
        .get("primaryPlatformInfo")
        .and_then(|p| p.get("onlineStatus"))
        .and_then(|v| v.as_str())
        // Some payloads put onlineStatus on basicPresence itself
        .or_else(|| bp.get("onlineStatus").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
        .or_else(|| {
            bp.get("availability")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .map(|s| normalize_psn_online_status(&s));
    let title = bp
        .get("gameTitleInfoList")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|t| t.get("titleName"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    (status, title)
}

/// 仅刷新 PSN live presence：1 次上游调用，奖杯 / 头像沿用 6h 快照
async fn refresh_psn_presence(
    data: &GamePresenceData,
    dynamic_config: &Arc<RwLock<DynamicConfig>>,
) -> Result<GamePresenceInfo, String> {
    let npsso = psn_npsso(dynamic_config).await;
    if npsso.trim().is_empty() {
        return Err("PSN_NPSSO not configured".to_string());
    }
    if !try_spend_credential_call(Platform::Psn) {
        return Err("PSN credential budget exhausted".to_string());
    }
    let access_token = get_psn_access_token(&npsso).await?;
    let auth = format!("Bearer {access_token}");

    // 快照走过 search 回退路径时 identity.id 是纯数字 accountId
    // （online ID 必须以字母开头，不会与之混淆），直接查 basicPresences
    let account_id = data.identity.id.trim();
    if !account_id.is_empty() && account_id.chars().all(|c| c.is_ascii_digit()) {
        let url = format!(
            "https://m.np.playstation.com/api/userProfile/v1/internal/users/{account_id}/basicPresences?type=primary"
        );
        let pres = http_get_json_with_header(
            &url,
            "Myriad/1.0 (game-presence)",
            &[("Authorization", &auth)],
        )
        .await?;
        let (status, title) = parse_psn_basic_presence(&pres);
        return Ok(GamePresenceInfo {
            status: status.unwrap_or_else(|| "Unknown".into()),
            title,
            detail: None,
        });
    }

    // 否则快照来自 legacy profile2：identity.name 就是 onlineId，同一端点一次调用带回 presence
    let url = format!(
        "https://us-prof.np.community.playstation.net/userProfile/v1/users/{}/profile2?fields=onlineId,primaryOnlineStatus,presences(@titleInfo,hasBroadcastData)",
        urlencoding_simple(&data.identity.name)
    );
    let legacy = http_get_json_with_header(
        &url,
        "Myriad/1.0 (game-presence)",
        &[("Authorization", &auth)],
    )
    .await?;
    let profile = legacy
        .get("profile")
        .ok_or_else(|| "profile2 response missing profile".to_string())?;
    let (status, title) = parse_psn_legacy_presence(profile);
    Ok(GamePresenceInfo {
        status: status.unwrap_or_else(|| "Unknown".into()),
        title,
        detail: None,
    })
}

async fn fetch_psn(
    online_id: &str,
    dynamic_config: &Arc<RwLock<DynamicConfig>>,
) -> Result<GamePresenceData, String> {
    let npsso = psn_npsso(dynamic_config).await;

    if npsso.trim().is_empty() {
        return Ok(GamePresenceData {
            platform: "psn".to_string(),
            identity: GameIdentity {
                id: online_id.to_string(),
                name: online_id.to_string(),
                avatar: None,
                subtitle: Some("PSN_NPSSO not configured".into()),
            },
            score: None,
            presence: None,
            highlights: vec![],
            showcase: vec![],
            profile_url: Some(format!(
                "https://profile.playstation.com/{}",
                urlencoding_simple(online_id)
            )),
            fetched_at: chrono::Utc::now().to_rfc3339(),
            degraded: true,
            degrade_reason: Some(
                "Set PSN_NPSSO on the server to load trophies and presence (service account, not user cookie)".into(),
            ),
        });
    }

    if !try_spend_credential_call(Platform::Psn) {
        return Err("Too many PSN lookups right now, try again in a bit".to_string());
    }

    // NPSSO → access token（缓存 token 本身，不要每次 120s 缓存 miss 都重新走一遍 OAuth）
    let access_token = get_psn_access_token(&npsso).await?;

    // Resolve accountId by onlineId
    let profile_url = format!(
        "https://us-prof.np.community.playstation.net/userProfile/v1/users/{}/profile2?fields=onlineId,aboutMe,languagesUsed,plus,trophySummary(@default,progress,earnedTrophies),isOfficiallyVerified,personalDetail(@default,profilePictureUrls),personalDetailSharing,personalDetailSharingRequestMessageFlag,primaryOnlineStatus,presences(@titleInfo,hasBroadcastData),friendRelation,requestMessageFlag,blocking,mutualFriendsCount,following,followerCount,friendsCount,followingUsersCount&avatarSizes=s,m,l,xl&profilePictureSizes=s,m,l,xl&languagesUsedLanguageSet=set4&psVitaSupport=true&friendStatusSummary=true&npIdHash=true",
        urlencoding_simple(online_id)
    );

    // account search
    let search_url = format!(
        "https://m.np.playstation.com/api/search/v1/users?searchTerm={}",
        urlencoding_simple(online_id)
    );

    let mut identity_name = online_id.to_string();
    let mut avatar: Option<String> = None;
    let mut trophy_level: Option<String> = None;
    let mut platinum: Option<String> = None;
    let mut presence_status: Option<String> = None;
    let mut presence_title: Option<String> = None;
    let mut account_id: Option<String> = None;

    // Try legacy profile endpoint (still works with some tokens)
    if let Ok(legacy) = http_get_json_with_header(
        &profile_url,
        "Myriad/1.0 (game-presence)",
        &[("Authorization", &format!("Bearer {access_token}"))],
    )
    .await
    {
        if let Some(p) = legacy.get("profile") {
            identity_name = p
                .get("onlineId")
                .and_then(|v| v.as_str())
                .unwrap_or(online_id)
                .to_string();
            avatar = p
                .get("avatarUrls")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|u| u.get("avatarUrl"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            if let Some(ts) = p.get("trophySummary") {
                trophy_level = ts.get("level").and_then(|v| {
                    v.as_i64()
                        .map(|n| n.to_string())
                        .or_else(|| v.as_str().map(|s| s.to_string()))
                });
                platinum = ts
                    .get("earnedTrophies")
                    .and_then(|e| e.get("platinum"))
                    .and_then(|v| v.as_i64().map(|n| n.to_string()));
            }
            let (status, title) = parse_psn_legacy_presence(p);
            presence_status = status;
            presence_title = title;
        }
    } else {
        // Fallback: search users
        if let Ok(search) = http_get_json_with_header(
            &search_url,
            "Myriad/1.0 (game-presence)",
            &[("Authorization", &format!("Bearer {access_token}"))],
        )
        .await
        {
            if let Some(domain) = search
                .get("domains")
                .and_then(|v| v.as_array())
                .and_then(|a| {
                    a.iter().find(|d| {
                        d.get("domain").and_then(|x| x.as_str()) == Some("SocialAllAccounts")
                    })
                })
            {
                if let Some(result) = domain
                    .get("results")
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.first())
                    .and_then(|r| r.get("socialMetadata"))
                {
                    identity_name = result
                        .get("onlineId")
                        .and_then(|v| v.as_str())
                        .unwrap_or(online_id)
                        .to_string();
                    account_id = result
                        .get("accountId")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    avatar = result
                        .get("avatarUrl")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                }
            }
        }

        if let Some(aid) = &account_id {
            // Trophy summary v2
            let trophy_url =
                format!("https://m.np.playstation.com/api/trophy/v1/users/{aid}/trophySummary");
            if let Ok(ts) = http_get_json_with_header(
                &trophy_url,
                "Myriad/1.0 (game-presence)",
                &[("Authorization", &format!("Bearer {access_token}"))],
            )
            .await
            {
                trophy_level = ts.get("trophyLevel").and_then(|v| {
                    v.as_str()
                        .map(|s| s.to_string())
                        .or_else(|| v.as_i64().map(|n| n.to_string()))
                });
                platinum = ts
                    .get("earnedTrophies")
                    .and_then(|e| e.get("platinum"))
                    .and_then(|v| v.as_i64().map(|n| n.to_string()));
            }

            let basic_presence_url = format!(
                "https://m.np.playstation.com/api/userProfile/v1/internal/users/{aid}/basicPresences?type=primary"
            );
            if let Ok(pres) = http_get_json_with_header(
                &basic_presence_url,
                "Myriad/1.0 (game-presence)",
                &[("Authorization", &format!("Bearer {access_token}"))],
            )
            .await
            {
                let (status, title) = parse_psn_basic_presence(&pres);
                presence_status = status;
                presence_title = title;
            }
        }
    }

    let mut highlights = Vec::new();
    if let Some(ref p) = platinum {
        highlights.push(GameHighlight {
            label: "Platinum".into(),
            value: p.clone(),
        });
    }

    Ok(GamePresenceData {
        platform: "psn".to_string(),
        identity: GameIdentity {
            id: account_id.unwrap_or_else(|| online_id.to_string()),
            name: identity_name,
            avatar,
            subtitle: None,
        },
        score: trophy_level.map(|v| GameScore {
            label: "Trophy Lv".into(),
            value: v,
        }),
        presence: Some(GamePresenceInfo {
            status: presence_status.unwrap_or_else(|| "Unknown".into()),
            title: presence_title,
            detail: None,
        }),
        highlights,
        showcase: vec![],
        profile_url: Some(format!(
            "https://profile.playstation.com/{}",
            urlencoding_simple(online_id)
        )),
        fetched_at: chrono::Utc::now().to_rfc3339(),
        degraded: false,
        degrade_reason: None,
    })
}

// PSN access token — workspace crate `myriad-psn-auth` (shared with fetcher).
//
// Sony 的 mobile access token 一般有效期在 1 小时左右；crate 内做 ~50min 缓存，
// 并按 NPSSO fingerprint 隔离，避免换 cookie 后复用旧 token。

/// Re-export for game_presence handlers; fetcher should call the crate directly.
pub use myriad_psn_auth::get_psn_access_token;

// HTTP helpers

/// OpenXBL 统一把业务载荷包在 `{ content: {...}, code: 200 }` 里；没有 content 时原样返回。
fn openxbl_unwrap_content(body: Value) -> Value {
    body.get("content").cloned().unwrap_or(body)
}

async fn http_get_json(url: &str, ua: &str) -> Result<Value, String> {
    http_get_json_with_header(url, ua, &[]).await
}

async fn http_get_json_with_header(
    url: &str,
    ua: &str,
    headers: &[(&str, &str)],
) -> Result<Value, String> {
    let (_, client) = build_public_http_client(url, Duration::from_secs(20), Some(ua))
        .await
        .map_err(|e| e.to_string())?;

    let mut req = client.get(url).header("User-Agent", ua);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let status = resp.status();
    if status.as_u16() == 404 {
        return Err("Player not found".to_string());
    }
    if status.as_u16() == 429 {
        return Err("Rate limited by upstream, try again later".to_string());
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "Upstream HTTP {status}: {}",
            body.chars().take(200).collect::<String>()
        ));
    }

    resp.json::<Value>()
        .await
        .map_err(|e| format!("JSON parse failed: {e}"))
}

fn urlencoding_simple(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Lightweight health/capabilities for the widget settings UI
pub async fn get_game_presence_capabilities(
    axum::extract::State(dynamic_config): axum::extract::State<
        std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>,
    >,
) -> Json<Value> {
    let (db_openxbl, db_psn) = {
        let config = dynamic_config.read().await;
        (
            config
                .openxbl_api_key
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty()),
            config
                .psn_npsso
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty()),
        )
    };
    let openxbl = db_openxbl
        || std::env::var("OPENXBL_API_KEY")
            .or_else(|_| std::env::var("XBL_API_KEY"))
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
    let psn = db_psn
        || std::env::var("PSN_NPSSO")
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);

    Json(json!({
        "platforms": {
            "hoyolab": {
                "public": true,
                "needs_server_key": false,
                "games": ["genshin", "hsr", "zzz"],
                "id_label": "UID"
            },
            "xbox": {
                "public": true,
                "needs_server_key": true,
                "server_key_ready": openxbl,
                "id_label": "Gamertag"
            },
            "psn": {
                "public": true,
                "needs_server_key": true,
                "server_key_ready": psn,
                "id_label": "Online ID"
            }
        }
    }))
}
