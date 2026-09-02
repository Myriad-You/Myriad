//! 头像（画像）单一解析处。
//!
//! ## 为什么集中在这里
//!
//! 头像有四类来源，且都要保留：
//!
//! | 来源 | 存放 | 写入方 |
//! |------|------|--------|
//! | 账号 | `users.avatar_url` | 本地注册播种 / OAuth 登录同步 |
//! | OAuth 身份 | `user_identities.avatar_url` | 登录时 upsert（每个 provider 一份快照） |
//! | 平台画像 | `platform_metadata` | 站长抓取 B站 / GitHub / YouTube / Steam |
//! | 生成兜底 | 无 | 前端 `localFallback`（不再依赖 ui-avatars.com 外链） |
//!
//! 选择器列表里，OAuth 与同站平台抓取会**合并为一行**（见
//! [`list_avatar_sources`]），避免 GitHub 出现两次；存储仍用既有
//! `identity` / `platform` kind，不新增 DB 类型。
//!
//! 出口散落在 `/api/auth/me`、`/api/tapp/context/user`、`/api/auth/identities`、
//! `/api/admin/users`、`/api/profile/user-info` 五处。此前每处各写一遍优先级与
//! 代理判断，导致同一个人在首页和控制面板可能是两张脸。本模块是唯一解析处：
//! 出口只允许读这里，且**一律经 `proxy_image_url`** —— hdslb 等 CDN 有防盗链，
//! 直链在浏览器里必裂。

use std::collections::HashMap;

use myriad_image_proxy::proxy_image_url;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait,
    Value as SeaValue,
};
use serde_json::{json, Value};

/// 站长平台画像的固定考察顺序（auto 模式下的兜底优先级，与历史行为一致）。
pub const PLATFORM_ORDER: [&str; 4] = ["bilibili", "github", "youtube", "steam"];

/// 未选择画像源（auto）时的隐式阶梯，`{alias}` 为 `users` 表别名。
///
/// `users.avatar_url`（排除历史播种的 ui-avatars 占位外链）→ 该用户最优的一条
/// identity 快照（primary 优先，其次最近登录 / 最近绑定）→ `users.avatar_url` 原值。
const IMPLICIT_LADDER_TEMPLATE: &str = r#"COALESCE(
    NULLIF(
        CASE
            WHEN {alias}.avatar_url LIKE 'https://ui-avatars.com/%'
                 OR {alias}.avatar_url LIKE 'http://ui-avatars.com/%'
            THEN NULL
            ELSE {alias}.avatar_url
        END,
        ''
    ),
    (
        SELECT NULLIF(ui.avatar_url, '')
        FROM user_identities ui
        WHERE ui.user_id = {alias}.id
          AND ui.avatar_url IS NOT NULL
          AND ui.avatar_url <> ''
        ORDER BY ui.is_primary DESC, ui.last_login_at DESC NULLS LAST, ui.linked_at DESC
        LIMIT 1
    ),
    NULLIF({alias}.avatar_url, '')
)"#;

/// 解析器专用：只要隐式阶梯，不含显式快照那一层。
///
/// `resolve_avatar` 要重新算出 auto 的结果，若读进 `avatar_resolved_url` 就会
/// 自我循环——刷新一次便把旧快照当输入固化下来。
fn implicit_ladder_expr(alias: &str) -> String {
    IMPLICIT_LADDER_TEMPLATE.replace("{alias}", alias)
}

/// 头像的 SELECT 表达式片段，`alias` 是 `users` 表在该查询里的别名。
///
/// 优先级：`avatar_resolved_url`（用户显式选定画像源后的解析快照）→ 隐式阶梯。
/// 显式快照是"改一处、处处同步"的落点：所有出口读的是同一个字段。
///
/// 全 NULL 时返回 NULL，由调用方决定兜底 —— 后端不再编造 `ui-avatars.com`
/// 或 `github.com/ghost.png`，那会让"没有头像"和"头像就是这张"无法区分。
///
/// **这段 SQL 曾在七处逐字抄写**（`/auth/me`、`/tapp/context/user`、联邦 actor 四处、
/// 房间成员、发帖署名、社交时间线…），改一处漏六处：联邦那边就一直没跟上用户
/// 选定的画像源。所有需要它的查询都必须调本函数，不要再抄。
pub fn avatar_snapshot_expr(alias: &str) -> String {
    format!(
        "COALESCE(NULLIF({alias}.avatar_resolved_url, ''), {ladder})",
        ladder = implicit_ladder_expr(alias),
    )
}

/// 布尔谓词：该本地用户**是否有可展示的头像**，`alias` 为 `users` 表别名。
///
/// 与 [`avatar_snapshot_expr`] 配套但不同用途：联邦时间线要先判断"这人有没有脸"，
/// 才决定要不要输出 `/users/{name}/avatar` 链接。必须把显式选定的快照算进来 ——
/// 只看 `avatar_url` / identity 的话，画像源选了平台画像的用户会被判成"没有头像"，
/// 联邦那边就不给他出头像链接。
pub fn avatar_presence_expr(alias: &str) -> String {
    format!(
        "(({a}.avatar_resolved_url IS NOT NULL AND {a}.avatar_resolved_url <> '') \
         OR ({a}.avatar_url IS NOT NULL AND {a}.avatar_url <> '' \
             AND {a}.avatar_url NOT LIKE 'https://ui-avatars.com/%' \
             AND {a}.avatar_url NOT LIKE 'http://ui-avatars.com/%') \
         OR EXISTS (SELECT 1 FROM user_identities ui \
                    WHERE ui.user_id = {a}.id \
                      AND ui.avatar_url IS NOT NULL AND ui.avatar_url <> ''))",
        a = alias
    )
}

/// 历史播种的 ui-avatars.com 占位地址（`name=Admin` 渲染出来就是那张 "Ad"）。
///
/// 占位头像是**显示时的兜底**，不是账号数据。注册处已不再播种，但存量库里还留着；
/// 这里统一当「没有头像」处理，于是不用数据迁移也能退到前端本地生成的兜底图。
/// SQL 侧的同名判断在 [`avatar_snapshot_expr`] 的阶梯里，两边必须同进退。
pub fn is_placeholder_avatar(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();
    u.starts_with("https://ui-avatars.com/") || u.starts_with("http://ui-avatars.com/")
}

/// 出口规范化：去空白、空串归 `None`、需要时包一层站内代理。
///
/// 所有返回头像的 HTTP 出口都必须过这里，前端才不用各自补 `proxyImageUrl`
/// （漏补的地方就是防盗链裂图现场）。
pub fn proxied_avatar(url: Option<String>) -> Option<String> {
    let trimmed = url?.trim().to_string();
    if trimmed.is_empty() {
        return None;
    }
    let proxied = proxy_image_url(&trimmed);
    if proxied.is_empty() {
        None
    } else {
        Some(proxied)
    }
}

/// `proxied_avatar` 的 JSON 版本：`None` → `Value::Null`。
pub fn proxied_avatar_value(url: Option<String>) -> serde_json::Value {
    match proxied_avatar(url) {
        Some(url) => serde_json::Value::String(url),
        None => serde_json::Value::Null,
    }
}

/// 画像源类型。`Auto` 表示未选择，沿用 [`avatar_snapshot_expr`] 的隐式阶梯。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvatarSourceKind {
    Auto,
    Account,
    Identity,
    Platform,
}

impl AvatarSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Account => "account",
            Self::Identity => "identity",
            Self::Platform => "platform",
        }
    }

    /// 未知/空值一律归为 `Auto` —— 库里存了脏值时退回隐式阶梯，而不是让用户没头像。
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim).unwrap_or_default() {
            "account" => Self::Account,
            "identity" => Self::Identity,
            "platform" => Self::Platform,
            _ => Self::Auto,
        }
    }
}

/// 一个可选画像源（给选择器用）。`avatar_url` 已过站内代理。
#[derive(Debug, Clone)]
pub struct AvatarSource {
    pub kind: AvatarSourceKind,
    pub source_ref: String,
    pub label: String,
    pub sublabel: Option<String>,
    pub avatar_url: Option<String>,
    pub is_current: bool,
}

impl AvatarSource {
    pub fn to_json(&self) -> Value {
        json!({
            "kind": self.kind.as_str(),
            "ref": self.source_ref,
            "label": self.label,
            "sublabel": self.sublabel,
            "avatar_url": self.avatar_url,
            "is_current": self.is_current,
        })
    }
}

/// 平台公开画像（站长的 B站 / GitHub / YouTube / Steam 资料）。
#[derive(Debug, Clone)]
pub struct PlatformProfile {
    /// 展示用平台名（"Bilibili" / "GitHub" / …）
    pub platform: &'static str,
    pub name: Option<String>,
    /// **原始上游地址**（未代理）：要落库当快照、要给联邦抓图，都必须是绝对地址。
    /// 代理只发生在 HTTP 出口，见 [`proxied_avatar`]。
    pub avatar: Option<String>,
    pub bio: String,
}

fn str_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| value.get(*k).and_then(Value::as_str))
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
}

/// 从 YouTube channel 载荷提取公开资料。
///
/// 存储形态是 Data API `channels.list` 条目（`snippet.*`）；同时兼容
/// smart_filter / 旧版扁平键。
fn youtube_profile(yt_user: &Value) -> Option<PlatformProfile> {
    let snip = yt_user.get("snippet");
    let name = snip
        .and_then(|s| s.get("title"))
        .or_else(|| yt_user.get("title"))
        .or_else(|| yt_user.get("name"))
        .or_else(|| snip.and_then(|s| s.get("customUrl")))
        .or_else(|| yt_user.get("customUrl"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let avatar = snip
        .and_then(|s| {
            s.pointer("/thumbnails/high/url")
                .or_else(|| s.pointer("/thumbnails/medium/url"))
                .or_else(|| s.pointer("/thumbnails/default/url"))
        })
        .or_else(|| {
            yt_user
                .get("thumbnails")
                .and_then(|t| t.get("high").or_else(|| t.get("default")))
                .and_then(|t| t.get("url"))
        })
        .or_else(|| yt_user.get("avatar"))
        .or_else(|| yt_user.get("face"))
        .and_then(Value::as_str)
        .map(str::to_string);
    if name.is_none() && avatar.is_none() {
        return None;
    }
    let bio = snip
        .and_then(|s| s.get("description"))
        .or_else(|| yt_user.get("description"))
        .or_else(|| yt_user.get("bio"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("YouTube")
        .to_string();
    Some(PlatformProfile {
        platform: "YouTube",
        name,
        avatar,
        bio,
    })
}

const LAZY_BIO: &str = "这家伙很懒，没有介绍呢";

/// 从单个平台的原始载荷提取公开画像。
///
/// `/api/profile/user-info` 的 DB 路径与缓存路径此前各抄了一遍这四个分支
/// （共八份），任何字段调整都要改八处；两边现在都走这里。
pub fn platform_profile(platform: &str, data: &Value) -> Option<PlatformProfile> {
    match platform {
        "bilibili" => {
            let user = data.get("user").or_else(|| data.get("user_info"))?;
            Some(PlatformProfile {
                platform: "Bilibili",
                name: str_field(user, &["name"]),
                avatar: user.get("face").and_then(Value::as_str).map(str::to_string),
                bio: str_field(user, &["sign"]).unwrap_or_else(|| LAZY_BIO.to_string()),
            })
        }
        "github" => {
            let user = data.get("user")?;
            Some(PlatformProfile {
                platform: "GitHub",
                name: str_field(user, &["name", "login"]),
                avatar: user
                    .get("avatar_url")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                bio: str_field(user, &["bio"]).unwrap_or_else(|| LAZY_BIO.to_string()),
            })
        }
        "youtube" => {
            let user = data
                .get("user")
                .or_else(|| data.get("channel"))
                .or_else(|| data.get("user_info"))?;
            youtube_profile(user)
        }
        "steam" => {
            let user = data.get("user")?;
            Some(PlatformProfile {
                platform: "Steam",
                name: str_field(user, &["personaname"]),
                avatar: user
                    .get("avatarfull")
                    .or_else(|| user.get("avatar"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                bio: "Steam 玩家".to_string(),
            })
        }
        _ => None,
    }
}

/// Whether disk-cache fallback may be used for platform data under `user_id`.
///
/// The filtered platform cache is the **site owner's** snapshot only. Serving it
/// under another `user_id` makes platform source refs validate for non-owners
/// and can corrupt stored `avatar_source_kind` / profile-text platform choices.
#[inline]
pub(crate) fn allow_platform_disk_cache_for_user(is_owner: bool) -> bool {
    is_owner
}

async fn load_user_is_owner(db: &DatabaseConnection, user_id: i32) -> bool {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT COALESCE(is_owner, false) AS is_owner FROM users WHERE id = $1",
        vec![SeaValue::Int(Some(user_id))],
    ))
    .await
    .ok()
    .flatten()
    .and_then(|row| row.try_get::<bool>("", "is_owner").ok())
    .unwrap_or(false)
}

/// 站长的全部平台原始数据：优先数据库，空则回落磁盘缓存（与 profile 的历史行为一致）。
/// 第二个返回值是数据来源标记（`"database"` / `"cache"` / `"none"`），出口要透出。
///
/// Disk-cache fallback is **site-owner only**. For non-owners, empty DB → empty map
/// (never the site-owner cache under another user_id).
pub async fn owner_platform_data(
    db: &DatabaseConnection,
    owner_id: i32,
) -> (HashMap<String, Value>, &'static str) {
    let service = crate::services::metadata_service::MetadataService::new(db.clone());
    match service.get_all_latest_metadata(owner_id).await {
        Ok(data) if !data.is_empty() => return (data, "database"),
        Ok(_) => {}
        Err(e) => tracing::warn!("Avatar: platform metadata unavailable ({e}), trying cache"),
    }

    // Never serve site-owner disk cache under a non-owner user_id.
    let is_owner = load_user_is_owner(db, owner_id).await;
    if !allow_platform_disk_cache_for_user(is_owner) {
        return (HashMap::new(), "none");
    }

    crate::services::platform_refresh::load_platform_data_cache()
        .and_then(|cache| cache.data.as_object().cloned())
        .map(|map| (map.into_iter().collect(), "cache"))
        .unwrap_or_else(|| (HashMap::new(), "none"))
}

/// 站长的平台画像（按 [`PLATFORM_ORDER`] 顺序，仅保留真正解析出内容的）+ 数据来源。
pub async fn owner_platform_snapshot(
    db: &DatabaseConnection,
    owner_id: i32,
) -> (Vec<(String, PlatformProfile)>, &'static str) {
    let (data, source) = owner_platform_data(db, owner_id).await;
    let profiles = PLATFORM_ORDER
        .iter()
        .filter_map(|platform| {
            let profile = platform_profile(platform, data.get(*platform)?)?;
            Some((platform.to_string(), profile))
        })
        .collect();
    (profiles, source)
}

/// [`owner_platform_snapshot`] 去掉来源标记。
pub async fn owner_platform_profiles(
    db: &DatabaseConnection,
    owner_id: i32,
) -> Vec<(String, PlatformProfile)> {
    owner_platform_snapshot(db, owner_id).await.0
}

struct UserAvatarRow {
    kind: AvatarSourceKind,
    source_ref: Option<String>,
    account_avatar: Option<String>,
    /// 未选择时的隐式阶梯结果
    implicit: Option<String>,
    is_owner: bool,
}

async fn load_user_avatar_row<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
) -> Result<Option<UserAvatarRow>, String> {
    // 这里刻意不取 avatar_resolved_url：解析时要重新算，读快照会自我循环
    let sql = format!(
        r#"SELECT u.avatar_source_kind, u.avatar_source_ref, u.avatar_url,
                  COALESCE(u.is_owner, false) AS is_owner,
                  {implicit} AS implicit_avatar
           FROM users u
           WHERE u.id = $1"#,
        implicit = implicit_ladder_expr("u"),
    );
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to load avatar row");
            "Failed to load avatar sources".to_string()
        })?;

    Ok(row.map(|row| UserAvatarRow {
        kind: AvatarSourceKind::parse(
            row.try_get::<Option<String>>("", "avatar_source_kind")
                .ok()
                .flatten()
                .as_deref(),
        ),
        source_ref: row
            .try_get::<Option<String>>("", "avatar_source_ref")
            .ok()
            .flatten(),
        // 存量库里的 ui-avatars 占位值在这里就滤掉：选「账号头像」不该把一张
        // 外部图床的字母图当成真头像用（SQL 阶梯里同样滤，两边同进退）
        account_avatar: row
            .try_get::<Option<String>>("", "avatar_url")
            .ok()
            .flatten()
            .filter(|url| !is_placeholder_avatar(url)),
        implicit: row
            .try_get::<Option<String>>("", "implicit_avatar")
            .ok()
            .flatten(),
        is_owner: row.try_get::<bool>("", "is_owner").unwrap_or(false),
    }))
}

async fn identity_avatar<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    identity_id: i32,
) -> Option<String> {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT avatar_url FROM user_identities WHERE id = $1 AND user_id = $2",
        vec![
            SeaValue::Int(Some(identity_id)),
            SeaValue::Int(Some(user_id)),
        ],
    ))
    .await
    .ok()
    .flatten()
    .and_then(|row| {
        row.try_get::<Option<String>>("", "avatar_url")
            .ok()
            .flatten()
    })
}

/// 解析结果 + 它是否来自 SQL 阶梯。
///
/// 区分这一点是为了决定要不要写快照：SQL 能自己算出的结果写进
/// `avatar_resolved_url` 只会固化成陈旧值，而平台画像 SQL 够不到（它藏在
/// `platform_metadata` 的 JSON 里，各平台字段还不同形），必须落快照，
/// `/api/auth/me` 那种单查询出口才看得见。
struct ResolvedAvatar {
    url: Option<String>,
    from_sql_ladder: bool,
}

/// `user_row_db`: connection that can see the latest `users` / identity rows
/// (the open transaction when switching source). `platform_db`: always the
/// pool connection for platform_metadata (unchanged by avatar source writes).
async fn resolve_detail<C: ConnectionTrait>(
    user_row_db: &C,
    platform_db: &DatabaseConnection,
    user_id: i32,
) -> ResolvedAvatar {
    let row = match load_user_avatar_row(user_row_db, user_id).await {
        Ok(Some(row)) => row,
        Ok(None) => {
            return ResolvedAvatar {
                url: None,
                from_sql_ladder: true,
            }
        }
        Err(e) => {
            tracing::warn!("Avatar resolve failed for user {user_id}: {e}");
            return ResolvedAvatar {
                url: None,
                from_sql_ladder: true,
            };
        }
    };

    let owner_platform_avatar = |platform: Option<String>| async {
        if !row.is_owner {
            return None;
        }
        let profiles = owner_platform_profiles(platform_db, user_id).await;
        match platform {
            // 指定平台
            Some(want) => profiles
                .into_iter()
                .find(|(name, _)| *name == want)
                .and_then(|(_, profile)| profile.avatar),
            // auto：按 PLATFORM_ORDER 取第一个有画像的
            None => profiles.into_iter().find_map(|(_, profile)| profile.avatar),
        }
    };

    let selected = match row.kind {
        // 站长未选择时保持历史行为：站点形象优先用平台画像（首页信息条与控制面板
        // 此前都是这张脸），平台没有数据才回落账号阶梯。普通用户没有平台数据，
        // 直接走阶梯。
        AvatarSourceKind::Auto => owner_platform_avatar(None).await,
        AvatarSourceKind::Account => row.account_avatar.clone(),
        AvatarSourceKind::Identity => {
            match row.source_ref.as_deref().and_then(|r| r.parse().ok()) {
                Some(identity_id) => identity_avatar(user_row_db, user_id, identity_id).await,
                None => None,
            }
        }
        AvatarSourceKind::Platform => owner_platform_avatar(row.source_ref.clone()).await,
    };

    // 选中的源失效时（平台数据被清、identity 解绑）回落隐式阶梯，而不是变成
    // 没头像 —— 来源消失是常态，不该让人脸没了。
    let from_sql_ladder = selected.is_none();
    ResolvedAvatar {
        url: selected
            .or(row.implicit)
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty()),
        from_sql_ladder,
    }
}

/// 按用户选定的画像源解析出最终头像（已过站内代理）。
pub async fn resolve_avatar(db: &DatabaseConnection, user_id: i32) -> Option<String> {
    proxied_avatar(resolve_detail(db, db, user_id).await.url)
}

/// 解析并写回 `avatar_resolved_url` 快照，让所有出口一次查询就拿到同一张脸。
///
/// 调用点：切换画像源、OAuth 登录刷新 identity 快照后、站长平台数据抓取完成后、
/// 启动时为站长兜一次底。写失败只记日志（后台刷新路径）；切换画像源走
/// [`refresh_avatar_snapshot_on`] 并纳入同一事务。
///
/// 结果来自 SQL 阶梯时写 NULL —— 让阶梯每次现算，不留陈旧快照。
///
/// **存的是原始上游地址，不是代理地址**：站内代理 URL 是相对路径
/// （`/api/proxy/image?...`），而联邦要拿这个值去真正抓图
/// （`federation::actor::get_avatar` → `cache_image`），相对路径抓不动；
/// 且 ActivityPub 文档面向外站，相对路径也没有意义。代理只发生在 HTTP 出口
/// （[`proxied_avatar`]），与阶梯里其余几列的语义保持一致。
pub async fn refresh_avatar_snapshot(db: &DatabaseConnection, user_id: i32) -> Option<String> {
    match refresh_avatar_snapshot_on(db, db, user_id).await {
        Ok(url) => url,
        Err(e) => {
            tracing::warn!("Avatar snapshot write failed for user {user_id}: {e}");
            // Best-effort resolve for callers that only need a display URL.
            proxied_avatar(resolve_detail(db, db, user_id).await.url)
        }
    }
}

/// Resolve + persist snapshot on `write_db` (may be an open transaction).
///
/// `platform_db` is the pool connection for platform metadata reads.
/// Returns `Err` if the snapshot `UPDATE` fails so callers can abort the
/// surrounding transaction instead of committing a source change without a
/// matching `avatar_resolved_url`.
pub async fn refresh_avatar_snapshot_on<C: ConnectionTrait>(
    write_db: &C,
    platform_db: &DatabaseConnection,
    user_id: i32,
) -> Result<Option<String>, String> {
    let resolved = resolve_detail(write_db, platform_db, user_id).await;

    let value = match resolved.url.clone().filter(|_| !resolved.from_sql_ladder) {
        Some(url) => SeaValue::String(Some(url)),
        None => SeaValue::String(None),
    };
    write_db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE users SET avatar_resolved_url = $1, avatar_updated_at = NOW() WHERE id = $2",
            vec![value, SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to write avatar snapshot");
            "Failed to save avatar source".to_string()
        })?;

    Ok(proxied_avatar(resolved.url))
}

/// OAuth provider slug → `platform_metadata` 平台键（仅 1:1 同站映射）。
///
/// 自定义 OIDC slug（如 `company-sso`）不在此表，始终作为独立 identity 行。
/// 大小写不敏感；未知 slug 返回 `None`。
pub fn identity_provider_platform_key(provider: &str) -> Option<&'static str> {
    match provider.trim().to_ascii_lowercase().as_str() {
        "github" => Some("github"),
        "bilibili" => Some("bilibili"),
        "youtube" => Some("youtube"),
        "steam" => Some("steam"),
        _ => None,
    }
}

/// 平台键 → 展示名（与 [`platform_profile`] 一致）。
pub fn platform_display_label(platform_key: &str) -> String {
    match platform_key {
        "bilibili" => "Bilibili".to_string(),
        "github" => "GitHub".to_string(),
        "youtube" => "YouTube".to_string(),
        "steam" => "Steam".to_string(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
                None => other.to_string(),
            }
        }
    }
}

/// 合并同平台两路头像 URL：优先非空，且**优先平台抓取**（常更高清，如 Steam
/// `avatarfull`、YouTube `thumbnails.high`）。
fn prefer_merged_avatar_url(
    platform_avatar: Option<String>,
    identity_avatar: Option<String>,
) -> Option<String> {
    let platform = platform_avatar
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let identity = identity_avatar
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    platform.or(identity)
}

/// 合并 sublabel：两边用户名/昵称，相同或互相包含则只留一份，避免 "octocat · octocat"。
fn merge_platform_sublabel(
    identity_username: Option<String>,
    platform_name: Option<String>,
) -> Option<String> {
    let identity = identity_username
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let platform = platform_name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    match (identity, platform) {
        (Some(u), Some(n)) if u.eq_ignore_ascii_case(&n) => Some(u),
        (Some(u), Some(n)) => {
            let ul = u.to_ascii_lowercase();
            let nl = n.to_ascii_lowercase();
            if nl.contains(&ul) || ul.contains(&nl) {
                // 更长的一侧通常信息量更大（如 "The Octocat" vs "octocat"）
                if n.len() >= u.len() {
                    Some(n)
                } else {
                    Some(u)
                }
            } else {
                Some(format!("{u} · {n}"))
            }
        }
        (Some(u), None) => Some(u),
        (None, Some(n)) => Some(n),
        (None, None) => None,
    }
}

/// 列表合并用的 OAuth 身份快照（未代理）。
#[derive(Debug, Clone)]
struct IdentitySourceRaw {
    id: i32,
    provider: String,
    username: Option<String>,
    avatar_url: Option<String>,
}

/// 将 OAuth 身份与站长平台画像按同站键合并成选择器行。
///
/// 规则（与产品约定一致）：
/// - `account` 不参与合并，由调用方先推入。
/// - 同一 `platform_key`（github / bilibili / youtube / steam）上若 identity 与
///   platform 都存在 → **一行**，`kind=platform`、`ref=平台键`（抓取已配置时
///   选中写 platform，稳定且校验走既有 ownership 路径）。
/// - `is_current`：当前选择是该 platform，**或**是已并入的那条 identity。
/// - 仅有一侧时保持原样（identity 行或 platform 行）。
/// - 自定义 OIDC 等无法映射的 provider 始终 identity-only。
///
/// 顺序：按 identity 绑定顺序输出（合并行替换对应 identity）；未匹配的
/// platform 再按 [`PLATFORM_ORDER`] 追加。
fn build_merged_identity_platform_sources(
    identities: &[IdentitySourceRaw],
    platforms: &[(String, PlatformProfile)],
    current_kind: AvatarSourceKind,
    current_ref: &str,
) -> Vec<AvatarSource> {
    let platform_by_key: HashMap<&str, &PlatformProfile> =
        platforms.iter().map(|(k, p)| (k.as_str(), p)).collect();

    let mut consumed_platforms: HashMap<&str, bool> = HashMap::new();
    let mut out = Vec::new();

    for identity in identities {
        if identity.provider.is_empty() {
            continue;
        }
        let id_str = identity.id.to_string();
        let identity_is_current =
            current_kind == AvatarSourceKind::Identity && current_ref == id_str;

        if let Some(platform_key) = identity_provider_platform_key(&identity.provider) {
            if let Some(profile) = platform_by_key.get(platform_key) {
                consumed_platforms.insert(platform_key, true);
                let platform_is_current =
                    current_kind == AvatarSourceKind::Platform && current_ref == platform_key;
                let raw_avatar =
                    prefer_merged_avatar_url(profile.avatar.clone(), identity.avatar_url.clone());
                out.push(AvatarSource {
                    kind: AvatarSourceKind::Platform,
                    source_ref: platform_key.to_string(),
                    label: platform_display_label(platform_key),
                    sublabel: merge_platform_sublabel(
                        identity.username.clone(),
                        profile.name.clone(),
                    ),
                    avatar_url: proxied_avatar(raw_avatar),
                    is_current: platform_is_current || identity_is_current,
                });
                continue;
            }
        }

        out.push(AvatarSource {
            kind: AvatarSourceKind::Identity,
            source_ref: id_str.clone(),
            label: identity.provider.clone(),
            sublabel: identity.username.clone(),
            avatar_url: proxied_avatar(identity.avatar_url.clone()),
            is_current: identity_is_current,
        });
    }

    // 未并入任何 identity 的平台画像（站长抓了但没绑对应 OAuth）
    for platform_key in PLATFORM_ORDER {
        if consumed_platforms.contains_key(platform_key) {
            continue;
        }
        let Some(profile) = platform_by_key.get(platform_key) else {
            continue;
        };
        out.push(AvatarSource {
            kind: AvatarSourceKind::Platform,
            source_ref: platform_key.to_string(),
            label: profile.platform.to_string(),
            sublabel: profile.name.clone(),
            avatar_url: proxied_avatar(profile.avatar.clone()),
            is_current: current_kind == AvatarSourceKind::Platform && current_ref == platform_key,
        });
    }

    out
}

/// 列出该用户全部可选画像源：账号 + 每个已绑定 OAuth 身份 +（站长）每个平台画像。
///
/// **同站合并**：OAuth identity 与站长 `platform_metadata` 若映射到同一平台键
/// （如 github），只返回一行（`kind=platform`）。详见
/// [`build_merged_identity_platform_sources`]。`auto` / `account` 不参与合并。
pub async fn list_avatar_sources(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Vec<AvatarSource>, String> {
    let row = load_user_avatar_row(db, user_id)
        .await?
        .ok_or_else(|| "User not found".to_string())?;
    let current_ref = row.source_ref.clone().unwrap_or_default();
    let mut sources = Vec::new();

    sources.push(AvatarSource {
        kind: AvatarSourceKind::Account,
        source_ref: String::new(),
        label: "account".to_string(),
        sublabel: None,
        avatar_url: proxied_avatar(row.account_avatar.clone()),
        is_current: row.kind == AvatarSourceKind::Account,
    });

    let identity_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, provider, provider_username, avatar_url \
             FROM user_identities WHERE user_id = $1 ORDER BY linked_at ASC",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to list avatar identities");
            "Failed to load avatar sources".to_string()
        })?;

    let mut identities = Vec::new();
    for identity in identity_rows {
        let id = identity.try_get::<i32>("", "id").unwrap_or(0);
        let provider = identity
            .try_get::<String>("", "provider")
            .unwrap_or_default();
        if provider.is_empty() {
            continue;
        }
        identities.push(IdentitySourceRaw {
            id,
            provider,
            username: identity
                .try_get::<Option<String>>("", "provider_username")
                .ok()
                .flatten(),
            avatar_url: identity
                .try_get::<Option<String>>("", "avatar_url")
                .ok()
                .flatten(),
        });
    }

    // 平台画像只属于站长：普通用户没有 platform_metadata，列出来也是空的
    let platforms = if row.is_owner {
        owner_platform_profiles(db, user_id).await
    } else {
        Vec::new()
    };

    sources.extend(build_merged_identity_platform_sources(
        &identities,
        &platforms,
        row.kind,
        &current_ref,
    ));

    Ok(sources)
}

/// 当前选择（供 GET 回显）。
pub async fn current_avatar_source(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<(AvatarSourceKind, Option<String>), String> {
    let row = load_user_avatar_row(db, user_id)
        .await?
        .ok_or_else(|| "User not found".to_string())?;
    Ok((row.kind, row.source_ref))
}

/// 写入画像源选择并立即刷新快照。
///
/// 校验：来源必须真实属于该用户 —— 管理员替他人切换时也只能在**对方已有的**
/// 来源里选，不能塞任意 URL。
///
/// Multi-step writes (`avatar_source_*`, optional `is_primary`, optional
/// `linked_github_id`) run in a single DB transaction.
pub async fn set_avatar_source(
    db: &DatabaseConnection,
    user_id: i32,
    kind: AvatarSourceKind,
    source_ref: Option<&str>,
) -> Result<Option<String>, String> {
    set_avatar_source_txn(db, user_id, kind, source_ref, None).await
}

/// OAuth `POST /identities/:id/primary`: set identity as avatar source and
/// optionally backfill `users.linked_github_id` in the same transaction.
pub async fn set_primary_identity_source(
    db: &DatabaseConnection,
    user_id: i32,
    identity_id: i32,
    linked_github_id: Option<i64>,
) -> Result<Option<String>, String> {
    set_avatar_source_txn(
        db,
        user_id,
        AvatarSourceKind::Identity,
        Some(&identity_id.to_string()),
        linked_github_id,
    )
    .await
}

async fn set_avatar_source_txn(
    db: &DatabaseConnection,
    user_id: i32,
    kind: AvatarSourceKind,
    source_ref: Option<&str>,
    linked_github_id: Option<i64>,
) -> Result<Option<String>, String> {
    let source_ref = source_ref.map(str::trim).filter(|s| !s.is_empty());

    let stored_ref: Option<String> = match kind {
        AvatarSourceKind::Auto | AvatarSourceKind::Account => None,
        AvatarSourceKind::Identity => {
            let raw = source_ref.ok_or_else(|| "identity source requires a ref".to_string())?;
            let identity_id: i32 = raw
                .parse()
                .map_err(|_| "identity ref must be an identity id".to_string())?;
            identity_avatar(db, user_id, identity_id)
                .await
                .ok_or_else(|| "Identity not found for this user".to_string())?;
            Some(identity_id.to_string())
        }
        AvatarSourceKind::Platform => {
            let platform =
                source_ref.ok_or_else(|| "platform source requires a ref".to_string())?;
            // Platform avatar is site-owner only. Check is_owner before profiles so a
            // disk-cache fallback cannot make platform refs validate for non-owners.
            let row = load_user_avatar_row(db, user_id)
                .await?
                .ok_or_else(|| "User not found".to_string())?;
            if !row.is_owner {
                return Err(
                    "Platform avatar source is only available for the site owner".to_string(),
                );
            }
            let available = owner_platform_profiles(db, user_id).await;
            if !available.iter().any(|(name, _)| name == platform) {
                return Err("Platform profile is not available for this user".to_string());
            }
            Some(platform.to_string())
        }
    };

    let txn = db.begin().await.map_err(|error| {
        tracing::error!(%error, "failed to begin avatar source transaction");
        "Failed to save avatar source".to_string()
    })?;

    if let Some(github_id) = linked_github_id {
        txn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE users SET linked_github_id = COALESCE(linked_github_id, $1), \
             updated_at = NOW() WHERE id = $2",
            vec![
                SeaValue::BigInt(Some(github_id)),
                SeaValue::Int(Some(user_id)),
            ],
        ))
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to backfill linked_github_id");
            "Failed to save avatar source".to_string()
        })?;
    }

    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE users SET avatar_source_kind = $1, avatar_source_ref = $2, updated_at = NOW() \
         WHERE id = $3",
        vec![
            SeaValue::String(Some(kind.as_str().to_string())),
            match stored_ref.clone() {
                Some(r) => SeaValue::String(Some(r)),
                None => SeaValue::String(None),
            },
            SeaValue::Int(Some(user_id)),
        ],
    ))
    .await
    .map_err(|error| {
        tracing::error!(%error, "failed to save avatar source");
        "Failed to save avatar source".to_string()
    })?;

    // identity 源同步 is_primary，保持与既有 /identities/{id}/primary 语义一致
    if kind == AvatarSourceKind::Identity {
        if let Some(identity_id) = stored_ref.as_deref().and_then(|r| r.parse::<i32>().ok()) {
            txn.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE user_identities SET is_primary = (id = $1) WHERE user_id = $2",
                vec![
                    SeaValue::Int(Some(identity_id)),
                    SeaValue::Int(Some(user_id)),
                ],
            ))
            .await
            .map_err(|error| {
                tracing::error!(%error, "failed to set primary identity");
                "Failed to save avatar source".to_string()
            })?;
        }
    }

    // Snapshot must land in the same transaction as the source change so
    // readers never observe a new kind with a stale avatar_resolved_url.
    // Platform metadata is unchanged here — resolve it via the pool connection.
    let avatar_url = refresh_avatar_snapshot_on(&txn, db, user_id).await?;

    txn.commit().await.map_err(|error| {
        tracing::error!(%error, "failed to commit avatar source transaction");
        "Failed to save avatar source".to_string()
    })?;

    Ok(avatar_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_expr_binds_to_the_requested_alias() {
        // 联邦那边的表别名各不相同（u / users / peer / author），别名替换漏一处
        // 就会静默取到别的表的列
        for alias in ["u", "users", "author", "peer"] {
            let sql = avatar_snapshot_expr(alias);
            assert!(sql.contains(&format!("{alias}.avatar_resolved_url")));
            assert!(sql.contains(&format!("{alias}.avatar_url")));
            assert!(sql.contains(&format!("ui.user_id = {alias}.id")));
            assert!(!sql.contains("{alias}"), "模板占位符未被替换");
        }
        // primary 优先是画像源选择的落点，顺序不能被改
        assert!(avatar_snapshot_expr("u").contains("ORDER BY ui.is_primary DESC"));
    }

    #[test]
    fn explicit_selection_outranks_implicit_ladder() {
        // 显式选择的快照必须排在阶梯最前，否则"选了不生效"
        let sql = avatar_snapshot_expr("u");
        assert!(sql.find("avatar_resolved_url").unwrap() < sql.find("ui-avatars.com").unwrap());
    }

    #[test]
    fn presence_expr_counts_the_selected_snapshot() {
        // 只看 avatar_url/identity 的话，画像源选了平台画像的用户会被联邦判成
        // "没有头像"，时间线里就不给他出头像链接
        let sql = avatar_presence_expr("peer");
        assert!(sql.contains("peer.avatar_resolved_url IS NOT NULL"));
        assert!(sql.contains("peer.avatar_url NOT LIKE 'https://ui-avatars.com/%'"));
        assert!(sql.contains("ui.user_id = peer.id"));
    }

    #[test]
    fn placeholder_seed_reads_as_no_avatar() {
        // 注册时播下的占位图不是账号数据：选「账号头像」不该把它当真头像用
        assert!(is_placeholder_avatar(
            "https://ui-avatars.com/api/?name=Admin&background=4f46e5&color=fff"
        ));
        assert!(is_placeholder_avatar(
            "http://UI-Avatars.com/api/?name=User"
        ));
        assert!(!is_placeholder_avatar(
            "https://avatars.githubusercontent.com/u/1"
        ));
        assert!(!is_placeholder_avatar(
            "https://i0.hdslb.com/bfs/face/a.jpg"
        ));
    }

    #[test]
    fn proxies_hotlink_cdn_avatars() {
        let out = proxied_avatar(Some("https://i0.hdslb.com/bfs/face/a.jpg".into()));
        assert!(out.unwrap().starts_with("/api/proxy/image?url="));
    }

    #[test]
    fn leaves_healthy_cdn_avatars_untouched() {
        let raw = "https://avatars.githubusercontent.com/u/1?v=4";
        assert_eq!(proxied_avatar(Some(raw.into())), Some(raw.to_string()));
    }

    #[test]
    fn blank_and_missing_collapse_to_none() {
        assert_eq!(proxied_avatar(None), None);
        assert_eq!(proxied_avatar(Some(String::new())), None);
        assert_eq!(proxied_avatar(Some("   ".into())), None);
        assert_eq!(proxied_avatar_value(None), serde_json::Value::Null);
    }

    #[test]
    fn platform_disk_cache_is_owner_only() {
        assert!(allow_platform_disk_cache_for_user(true));
        assert!(!allow_platform_disk_cache_for_user(false));
    }

    #[test]
    fn extracts_each_platform_profile_shape() {
        let bilibili = platform_profile(
            "bilibili",
            &json!({"user": {"name": "阿绫", "face": "https://i0.hdslb.com/f.jpg", "sign": "hi"}}),
        )
        .unwrap();
        assert_eq!(bilibili.platform, "Bilibili");
        assert_eq!(bilibili.name.as_deref(), Some("阿绫"));
        // 平台画像返回原始地址：代理只在 HTTP 出口发生（落库当快照、给联邦抓图
        // 都必须是绝对地址，相对的 /api/proxy/image 抓不动）
        assert_eq!(
            bilibili.avatar.as_deref(),
            Some("https://i0.hdslb.com/f.jpg")
        );
        assert_eq!(bilibili.bio, "hi");

        // 旧缓存用 user_info 而非 user
        assert!(platform_profile("bilibili", &json!({"user_info": {"name": "x"}})).is_some());

        // GitHub 无 name 时回落 login；空 bio 用默认文案
        let github = platform_profile(
            "github",
            &json!({"user": {"login": "octocat", "avatar_url": "https://avatars.githubusercontent.com/u/1", "bio": ""}}),
        )
        .unwrap();
        assert_eq!(github.name.as_deref(), Some("octocat"));
        assert_eq!(github.bio, LAZY_BIO);

        // YouTube 存的是 channels.list 条目形态
        let youtube = platform_profile(
            "youtube",
            &json!({"channel": {"snippet": {
                "title": "Chan",
                "thumbnails": {"high": {"url": "https://yt3.example/a.jpg"}}
            }}}),
        )
        .unwrap();
        assert_eq!(youtube.name.as_deref(), Some("Chan"));
        assert_eq!(youtube.bio, "YouTube");

        let steam = platform_profile(
            "steam",
            &json!({"user": {"personaname": "gaben", "avatarfull": "https://avatars.steamstatic.com/x.jpg"}}),
        )
        .unwrap();
        assert_eq!(steam.name.as_deref(), Some("gaben"));
        assert_eq!(
            steam.avatar.as_deref(),
            Some("https://avatars.steamstatic.com/x.jpg")
        );
    }

    #[test]
    fn unknown_or_empty_platform_payload_yields_nothing() {
        assert!(platform_profile("mastodon", &json!({"user": {}})).is_none());
        assert!(platform_profile("bilibili", &json!({})).is_none());
        // name/avatar 都缺时不算一个可选画像源
        assert!(platform_profile("youtube", &json!({"user": {"snippet": {}}})).is_none());
    }

    #[test]
    fn source_kind_round_trips_and_falls_back_to_auto() {
        for kind in [
            AvatarSourceKind::Auto,
            AvatarSourceKind::Account,
            AvatarSourceKind::Identity,
            AvatarSourceKind::Platform,
        ] {
            assert_eq!(AvatarSourceKind::parse(Some(kind.as_str())), kind);
        }
        // 库里存了脏值时退回 auto，而不是让用户没头像
        assert_eq!(AvatarSourceKind::parse(None), AvatarSourceKind::Auto);
        assert_eq!(
            AvatarSourceKind::parse(Some("nope")),
            AvatarSourceKind::Auto
        );
    }

    #[test]
    fn does_not_double_proxy_already_proxied() {
        let once = proxied_avatar(Some("https://i0.hdslb.com/bfs/face/a.jpg".into())).unwrap();
        assert_eq!(proxied_avatar(Some(once.clone())), Some(once));
    }

    #[test]
    fn maps_known_oauth_slugs_to_platform_keys() {
        assert_eq!(identity_provider_platform_key("github"), Some("github"));
        assert_eq!(identity_provider_platform_key("GitHub"), Some("github"));
        assert_eq!(identity_provider_platform_key("bilibili"), Some("bilibili"));
        assert_eq!(identity_provider_platform_key("youtube"), Some("youtube"));
        assert_eq!(identity_provider_platform_key("steam"), Some("steam"));
        // 自定义 OIDC / 未建站抓取的 slug 不映射
        assert_eq!(identity_provider_platform_key("company-sso"), None);
        assert_eq!(identity_provider_platform_key("google"), None);
        assert_eq!(identity_provider_platform_key(""), None);
    }

    #[test]
    fn merge_sublabel_dedupes_and_prefers_richer_name() {
        assert_eq!(
            merge_platform_sublabel(Some("octocat".into()), Some("octocat".into())).as_deref(),
            Some("octocat")
        );
        assert_eq!(
            merge_platform_sublabel(Some("octocat".into()), Some("The Octocat".into())).as_deref(),
            Some("The Octocat")
        );
        assert_eq!(
            merge_platform_sublabel(Some("alice".into()), Some("bob".into())).as_deref(),
            Some("alice · bob")
        );
        assert_eq!(
            merge_platform_sublabel(Some("only-id".into()), None).as_deref(),
            Some("only-id")
        );
        assert_eq!(
            merge_platform_sublabel(None, Some("only-plat".into())).as_deref(),
            Some("only-plat")
        );
    }

    #[test]
    fn merged_avatar_prefers_platform_then_identity() {
        assert_eq!(
            prefer_merged_avatar_url(
                Some("https://platform.example/a.jpg".into()),
                Some("https://identity.example/b.jpg".into()),
            )
            .as_deref(),
            Some("https://platform.example/a.jpg")
        );
        assert_eq!(
            prefer_merged_avatar_url(None, Some("https://identity.example/b.jpg".into()))
                .as_deref(),
            Some("https://identity.example/b.jpg")
        );
        assert_eq!(
            prefer_merged_avatar_url(Some("  ".into()), Some("https://i.example/x.jpg".into()))
                .as_deref(),
            Some("https://i.example/x.jpg")
        );
        assert_eq!(prefer_merged_avatar_url(None, None), None);
    }

    #[test]
    fn merges_github_identity_and_platform_into_one_row() {
        let identities = vec![IdentitySourceRaw {
            id: 42,
            provider: "github".into(),
            username: Some("octocat".into()),
            avatar_url: Some("https://avatars.githubusercontent.com/u/1?v=4".into()),
        }];
        let platforms = vec![(
            "github".into(),
            PlatformProfile {
                platform: "GitHub",
                name: Some("The Octocat".into()),
                avatar: Some("https://avatars.githubusercontent.com/u/1".into()),
                bio: LAZY_BIO.into(),
            },
        )];

        // 当前选的是 identity → 合并行仍 is_current
        let merged = build_merged_identity_platform_sources(
            &identities,
            &platforms,
            AvatarSourceKind::Identity,
            "42",
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].kind, AvatarSourceKind::Platform);
        assert_eq!(merged[0].source_ref, "github");
        assert_eq!(merged[0].label, "GitHub");
        assert_eq!(merged[0].sublabel.as_deref(), Some("The Octocat"));
        assert!(merged[0].is_current);
        assert!(merged[0].avatar_url.is_some());

        // 当前选的是 platform → 同样 is_current
        let merged_plat = build_merged_identity_platform_sources(
            &identities,
            &platforms,
            AvatarSourceKind::Platform,
            "github",
        );
        assert!(merged_plat[0].is_current);

        // 当前选的是 account → 合并行非 current
        let merged_other = build_merged_identity_platform_sources(
            &identities,
            &platforms,
            AvatarSourceKind::Account,
            "",
        );
        assert!(!merged_other[0].is_current);
    }

    #[test]
    fn does_not_merge_when_only_one_side_exists() {
        let identity_only = build_merged_identity_platform_sources(
            &[IdentitySourceRaw {
                id: 7,
                provider: "github".into(),
                username: Some("octocat".into()),
                avatar_url: None,
            }],
            &[],
            AvatarSourceKind::Identity,
            "7",
        );
        assert_eq!(identity_only.len(), 1);
        assert_eq!(identity_only[0].kind, AvatarSourceKind::Identity);
        assert_eq!(identity_only[0].source_ref, "7");
        assert!(identity_only[0].is_current);

        let platform_only = build_merged_identity_platform_sources(
            &[],
            &[(
                "bilibili".into(),
                PlatformProfile {
                    platform: "Bilibili",
                    name: Some("阿绫".into()),
                    avatar: Some("https://i0.hdslb.com/f.jpg".into()),
                    bio: "hi".into(),
                },
            )],
            AvatarSourceKind::Platform,
            "bilibili",
        );
        assert_eq!(platform_only.len(), 1);
        assert_eq!(platform_only[0].kind, AvatarSourceKind::Platform);
        assert_eq!(platform_only[0].source_ref, "bilibili");
        assert!(platform_only[0].is_current);
    }

    #[test]
    fn custom_oidc_stays_identity_and_platforms_append() {
        let rows = build_merged_identity_platform_sources(
            &[
                IdentitySourceRaw {
                    id: 1,
                    provider: "company-sso".into(),
                    username: Some("alice".into()),
                    avatar_url: None,
                },
                IdentitySourceRaw {
                    id: 2,
                    provider: "github".into(),
                    username: Some("alice-gh".into()),
                    avatar_url: None,
                },
            ],
            &[
                (
                    "github".into(),
                    PlatformProfile {
                        platform: "GitHub",
                        name: Some("alice-gh".into()),
                        avatar: Some("https://avatars.githubusercontent.com/u/9".into()),
                        bio: LAZY_BIO.into(),
                    },
                ),
                (
                    "steam".into(),
                    PlatformProfile {
                        platform: "Steam",
                        name: Some("gaben".into()),
                        avatar: None,
                        bio: "Steam 玩家".into(),
                    },
                ),
            ],
            AvatarSourceKind::Auto,
            "",
        );
        // company-sso identity, merged github, leftover steam
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].kind, AvatarSourceKind::Identity);
        assert_eq!(rows[0].label, "company-sso");
        assert_eq!(rows[1].kind, AvatarSourceKind::Platform);
        assert_eq!(rows[1].source_ref, "github");
        assert_eq!(rows[2].kind, AvatarSourceKind::Platform);
        assert_eq!(rows[2].source_ref, "steam");
    }
}
