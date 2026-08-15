//! Platform data refresh and disk cache (shared by profile HTTP and Tapp scheduler).
//!
//! Extracted from `api::profile` so `services::tapp_scheduler` does not depend on the HTTP layer.

use crate::services::fetcher::PlatformFetcher;
use crate::services::library_items::invalidate_library_assembly_cache;
use crate::services::site_owner::site_owner_user_id;
use chrono::{DateTime, Duration, Utc};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlatformDataCache {
    pub data: Value,
    pub fetched_at: DateTime<Utc>,
}

pub const PLATFORM_CACHE_HOURS: i64 = 12; // 数据缓存12小时

/// 从磁盘加载平台数据缓存
pub fn load_platform_data_cache() -> Option<PlatformDataCache> {
    // 优先从分平台数据目录加载
    let raw_dir = PathBuf::from("./cache/raw");
    if raw_dir.exists() {
        let mut all_data = serde_json::Map::new();
        let mut latest_time = std::time::SystemTime::UNIX_EPOCH;
        let mut found_any = false;

        if let Ok(entries) = fs::read_dir(&raw_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if let Ok(content) = fs::read_to_string(&path) {
                            if let Ok(json) = serde_json::from_str(&content) {
                                all_data.insert(stem.to_string(), json);
                                found_any = true;

                                if let Ok(metadata) = fs::metadata(&path) {
                                    if let Ok(modified) = metadata.modified() {
                                        if modified > latest_time {
                                            latest_time = modified;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if found_any {
            let fetched_at: DateTime<Utc> = latest_time.into();
            let age = Utc::now() - fetched_at;

            if age < Duration::hours(PLATFORM_CACHE_HOURS) {
                tracing::info!(
                    "✓ Loaded platform data from split raw files (age: {}h)",
                    age.num_hours()
                );
                return Some(PlatformDataCache {
                    data: Value::Object(all_data),
                    fetched_at,
                });
            } else {
                tracing::info!(
                    "⏰ Split platform data cache expired (age: {}h)",
                    age.num_hours()
                );
                // 虽然过期，但如果没有其他数据源，也许可以考虑返回？
                // 目前逻辑是过期就返回 None，触发重新获取
                return None;
            }
        }
    }

    None
}

/// 保存平台数据缓存到磁盘（优化：只保存分平台数据，不再保存完整大文件）
pub fn save_platform_data_cache(data: &Value) -> Result<(), Box<dyn std::error::Error>> {
    // 保存分平台的原始数据
    save_split_raw_data(data)?;
    invalidate_library_assembly_cache();
    Ok(())
}

/// 保存分平台的原始数据（避免读取大文件）
/// 优化：添加错误容错和大文件分块写入
pub fn save_split_raw_data(all_data: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let raw_dir = PathBuf::from("./cache/raw");
    if !raw_dir.exists() {
        fs::create_dir_all(&raw_dir)?;
    }

    if let Some(obj) = all_data.as_object() {
        for (platform, data) in obj {
            // 保存所有平台的数据，不仅仅是主要平台
            let file_path = raw_dir.join(format!("{}.json", platform));

            // 优化：先写入临时文件，然后原子性重命名，避免写入中断导致文件损坏
            let temp_path = raw_dir.join(format!("{}.json.tmp", platform));

            match std::fs::File::create(&temp_path) {
                Ok(file) => {
                    // 使用更大的缓冲区处理大文件（512KB）
                    let mut writer = std::io::BufWriter::with_capacity(524288, file);

                    match serde_json::to_writer(&mut writer, data) {
                        Ok(_) => {
                            use std::io::Write;
                            if let Err(e) = writer.flush() {
                                tracing::warn!("⚠️ Failed to flush {} data: {}", platform, e);
                                // 继续处理其他平台
                                continue;
                            }

                            // 原子性重命名
                            if let Err(e) = std::fs::rename(&temp_path, &file_path) {
                                tracing::warn!(
                                    "⚠️ Failed to rename temp file for {}: {}",
                                    platform,
                                    e
                                );
                                // 尝试直接复制
                                if let Err(e2) = std::fs::copy(&temp_path, &file_path) {
                                    tracing::error!(
                                        "❌ Failed to copy temp file for {}: {}",
                                        platform,
                                        e2
                                    );
                                }
                                let _ = std::fs::remove_file(&temp_path);
                            }

                            tracing::info!("💾 Saved raw data for {} to {:?}", platform, file_path);
                        }
                        Err(e) => {
                            tracing::error!("❌ Failed to serialize {} data: {}", platform, e);
                            let _ = std::fs::remove_file(&temp_path);
                            // 继续处理其他平台，不返回错误
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("❌ Failed to create temp file for {}: {}", platform, e);
                    // 继续处理其他平台
                }
            }
        }
    }
    Ok(())
}

/// 一次抓取的结果：合并后的平台数据 + 各平台远程错误（保留旧缓存时也会记录）。
#[derive(Debug, Clone)]
pub struct FreshPlatformData {
    pub data: Value,
    /// platform id → raw error string from the remote fetcher
    pub errors: std::collections::HashMap<String, String>,
}

/// 记录某平台抓取错误（主/副接口均可）；同平台多次失败会拼接，避免覆盖。
fn note_fetch_error(
    errors: &mut std::collections::HashMap<String, String>,
    platform: &str,
    stage: &str,
    error: impl ToString,
) {
    let detail = {
        let raw = error.to_string();
        if stage.is_empty() {
            raw
        } else {
            format!("{stage}: {raw}")
        }
    };
    errors
        .entry(platform.to_string())
        .and_modify(|existing| {
            if !existing.contains(&detail) {
                existing.push_str("; ");
                existing.push_str(&detail);
            }
        })
        .or_insert(detail);
}

/// 将底层抓取错误转成面向用户的说明（含 X 402、通用鉴权/限流等）。
pub fn humanize_platform_fetch_error(platform: &str, error: &str) -> String {
    let lower = error.to_ascii_lowercase();
    let label = match platform {
        "github" => "GitHub",
        "bilibili" => "Bilibili",
        "steam" => "Steam",
        "netease" => "网易云音乐",
        "bangumi" => "Bangumi",
        "x" => "X",
        "discord" => "Discord",
        "mal" => "MyAnimeList",
        "xbox" => "Xbox",
        "psn" => "PSN",
        "youtube" => "YouTube",
        other => other,
    };

    // X 按量计费额度
    if platform.eq_ignore_ascii_case("x")
        && (lower.contains("402")
            || lower.contains("credits depleted")
            || lower.contains("payment required")
            || lower.contains("creditsdepleted"))
    {
        return "X API 额度已耗尽（HTTP 402 Credits Depleted）。请到 developer.x.com 充值/开通按量计费后再刷新；Bearer Token 本身可能仍有效。".to_string();
    }

    if lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("quota")
    {
        return format!(
            "{label} 请求过于频繁或额度/配额不足：{error}。请稍后再试，或检查 API 配额。"
        );
    }

    if lower.contains("401")
        || lower.contains("unauthorized")
        || lower.contains("invalid token")
        || lower.contains("bad credentials")
        || lower.contains("invalid_grant")
    {
        return format!(
            "{label} 鉴权失败：{error}。请检查 Token / API Key / Cookie 是否有效或已过期。"
        );
    }

    if lower.contains("403") || lower.contains("forbidden") || lower.contains("access denied") {
        return format!(
            "{label} 拒绝访问（403）：{error}。常见原因：资料未公开、权限 scope 不足、或 IP/风控拦截。"
        );
    }

    if lower.contains("404") || lower.contains("not found") {
        return format!("{label} 未找到目标资源：{error}。请确认用户名 / ID 配置正确。");
    }

    if lower.contains("timeout") || lower.contains("timed out") || lower.contains("connect") {
        return format!("{label} 网络/超时：{error}。请稍后重试。");
    }

    format!("{label} 抓取失败：{error}")
}

/// 综合「远程错误」与「数据是否为空」给出最终用户提示。
/// - 远程失败且无可用数据 → 优先展示真实错误（如 402）
/// - 远程部分失败但仍有可用数据 → 提示失败点，并说明仍有可用数据
/// - 无远程错误但数据空 → 原有 platform_data_warning
pub fn resolve_platform_fetch_message(
    platform: &str,
    data: Option<&Value>,
    remote_error: Option<&str>,
) -> Option<String> {
    let has_usable = platform_data_warning(platform, data).is_none();

    match (remote_error, has_usable) {
        (Some(err), false) => Some(humanize_platform_fetch_error(platform, err)),
        (Some(err), true) => Some(format!(
            "{}（仍有部分可用数据，请查看详情后重试失败项）",
            humanize_platform_fetch_error(platform, err)
        )),
        (None, false) => platform_data_warning(platform, data),
        (None, true) => None,
    }
}

/// 一键获取所有平台数据（带缓存）

pub fn platform_data_warning(platform: &str, data: Option<&Value>) -> Option<String> {
    let Some(data) = data.filter(|v| !v.is_null()) else {
        return Some(format!(
            "{} 未返回任何数据。请确认该平台已启用且账号/令牌配置正确。",
            platform
        ));
    };

    // 各平台核心数组为空时给出针对性提示
    let is_empty_array = |key: &str| {
        data.get(key)
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true)
    };

    match platform {
        "bangumi" => is_empty_array("collections").then(|| {
            "Bangumi 收藏为空。可能是收藏设为私密、用户名/访问令牌不正确，或该账号确实没有收藏。".to_string()
        }),
        "mal" => {
            let anime_empty = is_empty_array("anime_list");
            let manga_empty = is_empty_array("manga_list");
            (anime_empty && manga_empty).then(|| {
                "MyAnimeList 列表为空。请确认用户名正确；公开列表模式需将列表设为公开，或配置可选 Client ID 使用官方 API。".to_string()
            })
        }
        "steam" => is_empty_array("games")
            .then(|| "Steam 未返回游戏数据。请确认 API Key、SteamID 正确且个人资料设为公开。".to_string()),
        "bilibili" => {
            let no_user = data
                .get("user")
                .or_else(|| data.get("user_info"))
                .filter(|v| !v.is_null())
                .is_none();
            let no_content = is_empty_array("favorites") && is_empty_array("bangumi");
            if no_user && no_content {
                Some(
                    "Bilibili 未返回用户与内容数据。请确认 UID 正确；用户接口受风控时请稍后重试。"
                        .to_string(),
                )
            } else if no_user {
                Some(
                    "Bilibili 用户信息未取到（追番/收藏可能仍有数据）。常见原因：space/acc/info 风控；请重新刷新。"
                        .to_string(),
                )
            } else {
                None
            }
        }
        "github" => data
            .get("user")
            .filter(|v| !v.is_null())
            .is_none()
            .then(|| "GitHub 未返回用户数据。请检查用户名与令牌。".to_string()),
        "x" => data
            .get("user")
            .filter(|v| !v.is_null())
            .is_none()
            .then(|| {
                "X 未返回用户数据。请检查用户名、Bearer Token 以及 API 套餐权限。".to_string()
            }),
        "discord" => data
            .get("user")
            .filter(|v| !v.is_null())
            .is_none()
            .then(|| {
                "Discord 未返回用户数据。请检查 Access Token 是否有效，且 scope 含 identify / guilds / connections。".to_string()
            }),
        "xbox" => data
            .pointer("/achievements/titles")
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true)
            .then(|| {
                "Xbox 未返回成就数据。请确认 Gamertag、OpenXBL API Key 正确且资料设为公开。".to_string()
            }),
        "psn" => is_empty_array("trophy_titles").then(|| {
            "PSN 未返回奖杯数据。请确认 Online ID、NPSSO 有效且奖杯设为公开。".to_string()
        }),
        // YouTube: channel present with 0 videos is a valid empty public channel —
        // never treat as fetch failure. Only warn when channel object is missing.
        "youtube" => data
            .get("channel")
            .filter(|v| !v.is_null() && v.get("id").is_some())
            .is_none()
            .then(|| {
                "YouTube 未返回频道数据。请确认 API Key 有效，且 Channel ID / @handle 正确。"
                    .to_string()
            }),
        "netease" => data
            .get("profile")
            .filter(|v| !v.is_null())
            .is_none()
            .then(|| {
                "网易云未返回用户资料。请确认用户 ID 正确；接口受风控时请稍后重试。".to_string()
            }),
        _ => None,
    }
}

/// 刷新单个平台数据

pub async fn refresh_platform_for_scheduler(
    db: &DatabaseConnection,
    platform: &str,
) -> Result<Value, String> {
    let outcome = fetch_fresh_platform_data(db, Some(platform))
        .await
        .map_err(|error| error.to_string())?;
    if let Some(platform_data) = outcome.data.get(platform) {
        save_platform_data_cache(&json!({ (platform): platform_data }))
            .map_err(|error| error.to_string())?;
    }
    let remote_err = outcome.errors.get(platform).map(String::as_str);
    if let Some(msg) =
        resolve_platform_fetch_message(platform, outcome.data.get(platform), remote_err)
    {
        // 无可用数据时调度器记失败；有旧数据则仅告警并继续返回
        if platform_data_warning(platform, outcome.data.get(platform)).is_some() {
            return Err(msg);
        }
        tracing::warn!(
            "Platform {} refresh warning (stale kept): {}",
            platform,
            msg
        );
    }
    Ok(outcome.data.get(platform).cloned().unwrap_or(Value::Null))
}

pub async fn fetch_fresh_platform_data(
    db: &DatabaseConnection,
    target_platform: Option<&str>,
) -> Result<FreshPlatformData, Box<dyn std::error::Error>> {
    tracing::info!(
        "Starting fetch platform data (target: {:?})...",
        target_platform
    );

    let fetcher = PlatformFetcher::new().await;
    let user_id = site_owner_user_id(db)
        .await
        .map_err(std::io::Error::other)?;

    // 获取动态配置
    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;

    // 1. 如果是增量更新，先加载现有数据
    let mut all_data = if target_platform.is_some() {
        load_platform_data_cache()
            .map(|c| c.data)
            .unwrap_or(json!({}))
    } else {
        json!({})
    };
    let mut fetch_errors: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    // 辅助闭包：判断是否应该获取该平台
    let should_fetch = |p: &str| target_platform.is_none() || target_platform == Some(p);
    // 数据抓取/刷新只要求「已配置」凭证，不要求报告页开关 enabled。
    // `*_enabled` 仅控制报告页是否展示该平台卡片。
    let has_cfg = |v: &Option<String>| v.as_ref().is_some_and(|s| !s.trim().is_empty());
    let is_platform_configured = |p: &str| match p {
        "github" => has_cfg(&config.github_username),
        "bilibili" => has_cfg(&config.bilibili_uid),
        "steam" => has_cfg(&config.steam_api_key) && has_cfg(&config.steam_id),
        "youtube" => has_cfg(&config.youtube_api_key) && has_cfg(&config.youtube_channel_id),
        "netease" => has_cfg(&config.netease_user_id),
        "bangumi" => has_cfg(&config.bangumi_username) || has_cfg(&config.bangumi_access_token),
        "x" => has_cfg(&config.x_username) && has_cfg(&config.x_bearer_token),
        "discord" => has_cfg(&config.discord_access_token),
        "mal" => has_cfg(&config.mal_username),
        "xbox" => {
            let has_gamertag =
                has_cfg(&config.xbox_gamertag) || std::env::var("XBOX_GAMERTAG").is_ok();
            let has_key = has_cfg(&config.openxbl_api_key)
                || std::env::var("OPENXBL_API_KEY").is_ok()
                || std::env::var("XBL_API_KEY").is_ok();
            has_gamertag && has_key
        }
        "psn" => {
            let has_id = has_cfg(&config.psn_online_id) || std::env::var("PSN_ONLINE_ID").is_ok();
            let has_npsso = has_cfg(&config.psn_npsso) || std::env::var("PSN_NPSSO").is_ok();
            has_id && has_npsso
        }
        _ => false,
    };

    // 创建元数据服务
    let metadata_service = crate::services::metadata_service::MetadataService::new(db.clone());

    // 获取GitHub数据（包含仓库信息）
    if should_fetch("github") && is_platform_configured("github") {
        if let Some(github_username) = &config.github_username {
            let github_token = config.github_token.as_deref();

            // 获取用户基本信息
            match fetcher
                .fetch_github_user(github_username, github_token)
                .await
            {
                Ok(user_data) => {
                    all_data["github"]["user"] = user_data;
                    tracing::info!("✓ GitHub user data fetched");
                }
                Err(e) => {
                    tracing::warn!("GitHub user fetch failed: {}", e);
                    note_fetch_error(&mut fetch_errors, "github", "user", e);
                }
            }

            // 获取仓库列表
            match fetcher
                .fetch_github_repos(github_username, github_token)
                .await
            {
                Ok(repos) => {
                    all_data["github"]["repos"] = json!(repos);
                    tracing::info!("✓ GitHub repos fetched: {} repositories", repos.len());
                }
                Err(e) => {
                    tracing::warn!("GitHub repos fetch failed: {}", e);
                    note_fetch_error(&mut fetch_errors, "github", "repos", e);
                }
            }

            // 获取贡献历史
            match fetcher
                .fetch_github_contributions(github_username, github_token)
                .await
            {
                Ok(contributions) => {
                    tracing::info!(
                        "✓ GitHub contributions fetched: {} days",
                        contributions.len()
                    );
                    if !contributions.is_empty() {
                        tracing::debug!("First contribution: {:?}", contributions.first());
                        tracing::debug!("Last contribution: {:?}", contributions.last());
                    }
                    all_data["github"]["contribution_calendar"] = json!(contributions);
                }
                Err(e) => {
                    // 贡献图为增强项：失败不阻断刷新成功，仅记日志
                    tracing::warn!("⚠ GitHub contributions fetch failed: {}", e);
                }
            }

            // 保存GitHub数据到数据库
            if !all_data["github"].is_null() {
                if let Err(e) = metadata_service
                    .save_platform_metadata(user_id, "github", all_data["github"].clone())
                    .await
                {
                    tracing::error!("Failed to save GitHub metadata to database: {}", e);
                }
            }
        }
    }

    // 获取Bilibili数据
    if should_fetch("bilibili") && is_platform_configured("bilibili") {
        if let Some(uid_str) = &config.bilibili_uid {
            if let Ok(uid) = uid_str.parse::<i64>() {
                match fetcher.fetch_bilibili_user(uid).await {
                    Ok(user_data) => {
                        // 与 Steam/GitHub 一致用 `user`；smart_filter / get_user_info 都读这个键
                        // （旧版曾写成 user_info，导致过滤与资料页读不到用户信息）
                        all_data["bilibili"]["user"] = json!(user_data);
                        tracing::info!(
                            "✓ Bilibili user data fetched: {} (mid={}, lv{}, {} followers)",
                            user_data.name,
                            user_data.mid,
                            user_data.level,
                            user_data.follower
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Bilibili user fetch failed: {}", e);
                        note_fetch_error(&mut fetch_errors, "bilibili", "user", e);
                    }
                }

                // 获取追番/追剧数据
                match fetcher.fetch_all_bilibili_bangumi(uid).await {
                    Ok(bangumi_data) => {
                        all_data["bilibili"]["bangumi"] = json!(bangumi_data);
                        tracing::info!(
                            "✓ Bilibili bangumi data fetched: {} items",
                            bangumi_data.len()
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Bilibili bangumi fetch failed: {}", e);
                        note_fetch_error(&mut fetch_errors, "bilibili", "bangumi", e);
                    }
                }

                // 获取收藏夹
                match fetcher.fetch_bilibili_favorites(uid).await {
                    Ok(favorites) => {
                        all_data["bilibili"]["favorites"] = json!(favorites);
                        tracing::info!("✓ Bilibili favorites fetched: {} items", favorites.len());
                    }
                    Err(e) => {
                        tracing::warn!("Bilibili favorites fetch failed: {}", e);
                        note_fetch_error(&mut fetch_errors, "bilibili", "favorites", e);
                    }
                }

                // 保存Bilibili数据到数据库
                if !all_data["bilibili"].is_null() {
                    if let Err(e) = metadata_service
                        .save_platform_metadata(user_id, "bilibili", all_data["bilibili"].clone())
                        .await
                    {
                        tracing::error!("Failed to save Bilibili metadata to database: {}", e);
                    }
                }
            }
        }
    }

    // 获取Steam数据（只保留游玩时间>=3小时的游戏）
    if should_fetch("steam") && is_platform_configured("steam") {
        if let (Some(api_key), Some(steam_id)) = (&config.steam_api_key, &config.steam_id) {
            match fetcher.fetch_steam_user(api_key, steam_id).await {
                Ok(user_data) => {
                    all_data["steam"]["user"] = json!(user_data);
                    tracing::info!("✓ Steam user data fetched");
                }
                Err(e) => {
                    tracing::warn!("Steam user fetch failed: {}", e);
                    note_fetch_error(&mut fetch_errors, "steam", "user", e);
                }
            }

            match fetcher.fetch_steam_games(api_key, steam_id).await {
                Ok(games_data) => {
                    // 过滤：只保留游玩时间>=180分钟(3小时)的游戏
                    let filtered_games: Vec<_> = games_data
                        .into_iter()
                        .filter(|game| game.playtime_forever >= 180)
                        .collect();

                    let total_count = filtered_games.len();
                    all_data["steam"]["games"] = json!(filtered_games);
                    tracing::info!(
                        "✓ Steam games fetched: {} games (filtered >=3h)",
                        total_count
                    );
                }
                Err(e) => {
                    tracing::warn!("Steam games fetch failed: {}", e);
                    note_fetch_error(&mut fetch_errors, "steam", "games", e);
                }
            }

            // 保存Steam数据到数据库
            if !all_data["steam"].is_null() {
                if let Err(e) = metadata_service
                    .save_platform_metadata(user_id, "steam", all_data["steam"].clone())
                    .await
                {
                    tracing::error!("Failed to save Steam metadata to database: {}", e);
                }
            }
        }
    }

    // 获取网易云音乐数据
    if should_fetch("netease") && is_platform_configured("netease") {
        tracing::info!("🎵 Should fetch netease: checking config...");
        tracing::info!("🎵 Config netease_user_id: {:?}", config.netease_user_id);

        if let Some(user_id_str) = &config.netease_user_id {
            tracing::info!("🎵 Netease user_id found in config: {}", user_id_str);
            if let Ok(netease_user_id) = user_id_str.parse::<i64>() {
                tracing::info!("🎵 Parsed netease_user_id: {}", netease_user_id);
                // 获取用户信息
                match fetcher.fetch_netease_user(netease_user_id).await {
                    Ok(user_data) => {
                        // 提取 profile 字段（API 返回格式：{ "code": 200, "profile": {...} }）
                        if let Some(profile) = user_data.get("profile") {
                            all_data["netease"]["profile"] = profile.clone();
                            tracing::info!("✓ Netease user data fetched");
                        } else {
                            // 如果没有 profile 字段，使用整个响应（兼容旧版本）
                            all_data["netease"]["profile"] = user_data;
                            tracing::warn!("⚠️ Netease API response missing 'profile' field, using full response");
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Netease user fetch failed: {}", e);
                        note_fetch_error(&mut fetch_errors, "netease", "user", e);
                    }
                }

                // 获取喜欢的歌曲（分批处理，避免内存占用过大）
                tracing::info!("🎵 Fetching Netease liked songs...");
                match fetcher.fetch_netease_liked_songs(netease_user_id).await {
                    Ok(songs) => {
                        let total_songs = songs.len();
                        tracing::info!("🎵 Total songs fetched: {}", total_songs);

                        // 直接保存完整歌曲列表
                        all_data["netease"]["liked_songs"] = json!(songs);
                        tracing::info!(
                            "✓ Netease Cloud Music liked songs stored: {} songs",
                            total_songs
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Netease Cloud Music fetch failed: {}", e);
                        note_fetch_error(&mut fetch_errors, "netease", "liked_songs", e);
                    }
                }

                // 保存网易云音乐数据到数据库
                if !all_data["netease"].is_null() {
                    if let Err(e) = metadata_service
                        .save_platform_metadata(user_id, "netease", all_data["netease"].clone())
                        .await
                    {
                        tracing::error!("Failed to save Netease metadata to database: {}", e);
                    }
                }
            }
        }
    }

    // 获取 Bangumi 收藏数据
    if should_fetch("bangumi") && is_platform_configured("bangumi") {
        let access_token = config.bangumi_access_token.as_deref();
        let user_agent = config.bangumi_user_agent.as_deref();
        let configured_username = config
            .bangumi_username
            .as_deref()
            .filter(|username| !username.trim().is_empty());

        let user_result = if let Some(username) = configured_username {
            fetcher
                .fetch_bangumi_user(username, access_token, user_agent)
                .await
        } else if let Some(token) = access_token {
            fetcher.fetch_bangumi_me(token, user_agent).await
        } else {
            Err(anyhow::anyhow!(
                "Bangumi username or access token is required"
            ))
        };

        match user_result {
            Ok(user_data) => {
                let resolved_username = user_data
                    .get("username")
                    .and_then(|v| v.as_str())
                    .or(configured_username)
                    .map(str::to_string);
                all_data["bangumi"]["user"] = user_data;
                tracing::info!("✓ Bangumi user data fetched");

                if let Some(username) = resolved_username.as_deref() {
                    match fetcher
                        .fetch_bangumi_collections(username, access_token, user_agent)
                        .await
                    {
                        Ok(collections) => {
                            let total_count = collections.len();
                            all_data["bangumi"]["collections"] = json!(collections);
                            tracing::info!("✓ Bangumi collections fetched: {} items", total_count);
                        }
                        Err(e) => {
                            tracing::warn!("Bangumi collections fetch failed: {}", e);
                            note_fetch_error(&mut fetch_errors, "bangumi", "collections", e);
                        }
                    }
                } else {
                    tracing::warn!(
                        "Bangumi user data did not include username; skipping collections"
                    );
                    note_fetch_error(
                        &mut fetch_errors,
                        "bangumi",
                        "collections",
                        "user data missing username",
                    );
                }
            }
            Err(e) => {
                tracing::warn!("Bangumi user fetch failed: {}", e);
                note_fetch_error(&mut fetch_errors, "bangumi", "user", e);
            }
        }

        if !all_data["bangumi"].is_null() {
            if let Err(e) = metadata_service
                .save_platform_metadata(user_id, "bangumi", all_data["bangumi"].clone())
                .await
            {
                tracing::error!("Failed to save Bangumi metadata to database: {}", e);
            }
        }
    }

    // 获取 X (Twitter) 数据
    if should_fetch("x") && is_platform_configured("x") {
        if let (Some(username), Some(bearer_token)) = (&config.x_username, &config.x_bearer_token) {
            match fetcher.fetch_x_profile_bundle(username, bearer_token).await {
                Ok(bundle) => {
                    all_data["x"] = bundle;
                    let tweet_count = all_data["x"]["tweets"]
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0);
                    tracing::info!("✓ X data fetched: {} tweets", tweet_count);
                }
                Err(e) => {
                    tracing::warn!("X fetch failed: {}", e);
                    note_fetch_error(&mut fetch_errors, "x", "", e);
                }
            }

            if !all_data["x"].is_null() {
                if let Err(e) = metadata_service
                    .save_platform_metadata(user_id, "x", all_data["x"].clone())
                    .await
                {
                    tracing::error!("Failed to save X metadata to database: {}", e);
                }
            }
        } else {
            tracing::warn!("X enabled but username or bearer_token missing");
        }
    }

    // 获取 Discord 数据（用户 OAuth：画像 + 服务器 + 连接）
    if should_fetch("discord") && is_platform_configured("discord") {
        if let Some(access_token_cfg) = config.discord_access_token.as_deref() {
            let expires_at = config
                .discord_token_expires_at
                .as_deref()
                .and_then(|s| s.parse::<i64>().ok());

            // 若配置了 Discord OAuth App（登录用 provider），可用于 refresh
            let (oauth_client_id, oauth_client_secret) = config
                .oauth_providers
                .iter()
                .find(|p| {
                    p.enabled
                        && (p.slug.eq_ignore_ascii_case("discord")
                            || p.display_name.eq_ignore_ascii_case("discord")
                            || p.discovery_url
                                .as_deref()
                                .map(|u| u.contains("discord.com"))
                                .unwrap_or(false))
                })
                .map(|p| (p.client_id.as_str(), p.client_secret.as_str()))
                .unwrap_or(("", ""));

            let (access_token, new_refresh, new_expires, did_refresh) = match fetcher
                .ensure_discord_access_token(
                    access_token_cfg,
                    config.discord_refresh_token.as_deref(),
                    expires_at,
                    if oauth_client_id.is_empty() {
                        None
                    } else {
                        Some(oauth_client_id)
                    },
                    if oauth_client_secret.is_empty() {
                        None
                    } else {
                        Some(oauth_client_secret)
                    },
                )
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("Discord token ensure failed: {}", e);
                    (
                        access_token_cfg.to_string(),
                        config.discord_refresh_token.clone(),
                        expires_at,
                        false,
                    )
                }
            };

            if did_refresh {
                let mut token_updates = std::collections::HashMap::new();
                token_updates.insert(
                    "discord_access_token".to_string(),
                    json!(access_token.clone()),
                );
                if let Some(ref rt) = new_refresh {
                    token_updates.insert("discord_refresh_token".to_string(), json!(rt));
                }
                if let Some(exp) = new_expires {
                    token_updates.insert(
                        "discord_token_expires_at".to_string(),
                        json!(exp.to_string()),
                    );
                }
                if let Err(e) = crate::services::config_service::ConfigService::new(db.clone())
                    .update_configs(token_updates)
                    .await
                {
                    tracing::warn!("Failed to persist refreshed Discord tokens: {}", e);
                } else {
                    tracing::info!("✓ Discord access token refreshed and saved");
                }
            }

            match fetcher.fetch_discord_profile_bundle(&access_token).await {
                Ok(mut bundle) => {
                    // 注入 Myriad 侧配置，供 smart_filter 交叉校验
                    if let Some(obj) = bundle.as_object_mut() {
                        obj.insert(
                            "myriad_cross_refs".to_string(),
                            json!({
                                "steam_id": config.steam_id,
                                "github_username": config.github_username,
                            }),
                        );
                    }

                    if let Some(uid) = bundle
                        .pointer("/user/id")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        let mut id_update = std::collections::HashMap::new();
                        id_update.insert("discord_user_id".to_string(), json!(uid));
                        let _ = crate::services::config_service::ConfigService::new(db.clone())
                            .update_configs(id_update)
                            .await;
                    }

                    all_data["discord"] = bundle;
                    let guild_count = all_data["discord"]["guilds"]
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0);
                    let conn_count = all_data["discord"]["connections"]
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0);
                    tracing::info!(
                        "✓ Discord data fetched: {} guilds, {} connections",
                        guild_count,
                        conn_count
                    );
                }
                Err(e) => {
                    tracing::warn!("Discord fetch failed: {}", e);
                    note_fetch_error(&mut fetch_errors, "discord", "", e);
                }
            }

            if !all_data["discord"].is_null() {
                if let Err(e) = metadata_service
                    .save_platform_metadata(user_id, "discord", all_data["discord"].clone())
                    .await
                {
                    tracing::error!("Failed to save Discord metadata to database: {}", e);
                }
            }
        } else {
            tracing::warn!("Discord enabled but access_token missing");
        }
    }

    // 获取 MyAnimeList 数据（双模式：有 client_id 走官方 API，否则公开 load.json）
    if should_fetch("mal") && is_platform_configured("mal") {
        if let Some(username) = config
            .mal_username
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            let client_id = config
                .mal_client_id
                .as_ref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            match fetcher.fetch_mal_profile_bundle(username, client_id).await {
                Ok(bundle) => {
                    all_data["mal"] = bundle;
                    let anime_count = all_data["mal"]["anime_list"]
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0);
                    let manga_count = all_data["mal"]["manga_list"]
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0);
                    tracing::info!(
                        "✓ MyAnimeList data fetched: {} anime, {} manga ({})",
                        anime_count,
                        manga_count,
                        if client_id.is_some() {
                            "official API"
                        } else {
                            "load.json"
                        }
                    );
                }
                Err(e) => {
                    tracing::warn!("MyAnimeList fetch failed: {}", e);
                    note_fetch_error(&mut fetch_errors, "mal", "", e);
                }
            }

            if !all_data["mal"].is_null() {
                if let Err(e) = metadata_service
                    .save_platform_metadata(user_id, "mal", all_data["mal"].clone())
                    .await
                {
                    tracing::error!("Failed to save MyAnimeList metadata to database: {}", e);
                }
            }
        } else {
            tracing::warn!("MyAnimeList enabled but username missing");
        }
    }

    // 获取 Xbox 数据（成就向：Gamerscore + 各游戏成就进度）
    // 凭据：DB 优先，env 回退（与 game_presence / 配置页展示一致）
    if should_fetch("xbox") && is_platform_configured("xbox") {
        let gamertag = config
            .xbox_gamertag
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| std::env::var("XBOX_GAMERTAG").ok())
            .unwrap_or_default();
        let api_key = config
            .openxbl_api_key
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| std::env::var("OPENXBL_API_KEY").ok())
            .or_else(|| std::env::var("XBL_API_KEY").ok())
            .unwrap_or_default();

        if !gamertag.trim().is_empty() && !api_key.trim().is_empty() {
            match fetcher.fetch_xbox_profile_bundle(&gamertag, &api_key).await {
                Ok(bundle) => {
                    all_data["xbox"] = bundle;
                    let titles_count = all_data["xbox"]["achievements"]["titles"]
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0);
                    tracing::info!("✓ Xbox data fetched: {} titles", titles_count);
                }
                Err(e) => {
                    tracing::warn!("Xbox fetch failed: {}", e);
                    note_fetch_error(&mut fetch_errors, "xbox", "", e);
                }
            }

            if !all_data["xbox"].is_null() {
                if let Err(e) = metadata_service
                    .save_platform_metadata(user_id, "xbox", all_data["xbox"].clone())
                    .await
                {
                    tracing::error!("Failed to save Xbox metadata to database: {}", e);
                }
            }
        } else {
            tracing::warn!("Xbox enabled but gamertag or OpenXBL API key missing");
        }
    }

    // 获取 PSN 数据（奖杯向：奖杯等级 + 各游戏奖杯完成度）
    if should_fetch("psn") && is_platform_configured("psn") {
        let online_id = config
            .psn_online_id
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| std::env::var("PSN_ONLINE_ID").ok())
            .unwrap_or_default();
        let npsso = config
            .psn_npsso
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| std::env::var("PSN_NPSSO").ok())
            .unwrap_or_default();

        if !online_id.trim().is_empty() && !npsso.trim().is_empty() {
            match fetcher.fetch_psn_profile_bundle(&online_id, &npsso).await {
                Ok(bundle) => {
                    all_data["psn"] = bundle;
                    let titles_count = all_data["psn"]["trophy_titles"]
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0);
                    tracing::info!("✓ PSN data fetched: {} trophy titles", titles_count);
                }
                Err(e) => {
                    tracing::warn!("PSN fetch failed: {}", e);
                    note_fetch_error(&mut fetch_errors, "psn", "", e);
                }
            }

            if !all_data["psn"].is_null() {
                if let Err(e) = metadata_service
                    .save_platform_metadata(user_id, "psn", all_data["psn"].clone())
                    .await
                {
                    tracing::error!("Failed to save PSN metadata to database: {}", e);
                }
            }
        } else {
            tracing::warn!("PSN enabled but online_id or NPSSO missing");
        }
    }

    // YouTube（公开频道；API key + channel id/handle，无 OAuth）
    if should_fetch("youtube") && is_platform_configured("youtube") {
        if let (Some(api_key), Some(channel_id)) =
            (&config.youtube_api_key, &config.youtube_channel_id)
        {
            match fetcher
                .fetch_youtube_channel_bundle(api_key, channel_id)
                .await
            {
                Ok(bundle) => {
                    all_data["youtube"] = bundle;
                    let video_n = all_data["youtube"]["videos"]
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0);
                    tracing::info!("✓ YouTube channel data fetched: {} sample videos", video_n);
                }
                Err(e) => {
                    tracing::warn!("YouTube fetch failed: {}", e);
                    note_fetch_error(&mut fetch_errors, "youtube", "", e);
                }
            }

            if !all_data["youtube"].is_null() {
                if let Err(e) = metadata_service
                    .save_platform_metadata(user_id, "youtube", all_data["youtube"].clone())
                    .await
                {
                    tracing::error!("Failed to save YouTube metadata to database: {}", e);
                }
            }
        }
    }

    // 数据清洗：移除无用信息，保留核心5W1H信息
    clean_platform_data(&mut all_data);

    // 更新智能过滤缓存：单平台刷新只处理该平台，避免重写全部平台缓存
    if let Some(platform) = target_platform {
        if let Some(platform_data) = all_data.get(platform) {
            if let Err(e) = crate::services::smart_filter::SmartFilter::process_and_save_single(
                platform,
                platform_data,
            ) {
                tracing::error!(
                    "Failed to update smart filter cache for {}: {}",
                    platform,
                    e
                );
            }
        } else {
            tracing::warn!(
                "No data for platform {} after fetch; skipping smart filter update",
                platform
            );
        }
    } else if let Err(e) =
        crate::services::smart_filter::SmartFilter::process_and_save_all(&all_data)
    {
        tracing::error!("Failed to update smart filter cache: {}", e);
    }

    // 平台画像可能换了（站长在 B站换了头像）——重算站长快照，让 /api/auth/me 等
    // 单查询出口也能跟着变；平台画像藏在 platform_metadata 的 JSON 里，SQL 阶梯够不到。
    crate::services::avatar::refresh_avatar_snapshot(db, user_id).await;

    Ok(FreshPlatformData {
        data: all_data,
        errors: fetch_errors,
    })
}

/// 清洗平台数据，只保留核心信息（符合5W1H原则）
/// 优化：原地修改减少内存峰值，添加数据量限制

fn clean_platform_data(data: &mut Value) {
    // 内存保护：各平台最大数据量限制
    const MAX_GITHUB_REPOS: usize = 200;
    const MAX_STEAM_GAMES: usize = 500;
    const MAX_BILIBILI_VIDEOS: usize = 100;
    const MAX_BILIBILI_BANGUMI: usize = 100;
    const MAX_SONGS_TO_CLEAN: usize = 5000;
    const MAX_BANGUMI_COLLECTIONS: usize = 1000;
    const MAX_X_TWEETS: usize = 100;

    // 清洗 GitHub 仓库数据 - 原地修改
    if let Some(repos) = data["github"]["repos"].as_array_mut() {
        // 限制仓库数量
        if repos.len() > MAX_GITHUB_REPOS {
            tracing::warn!(
                "⚠️ Truncating GitHub repos from {} to {}",
                repos.len(),
                MAX_GITHUB_REPOS
            );
            repos.truncate(MAX_GITHUB_REPOS);
        }

        for repo in repos.iter_mut() {
            if let Some(obj) = repo.as_object_mut() {
                // 保留的字段
                let name = obj.get("name").cloned();
                let description = obj.get("description").cloned();
                let language = obj.get("language").cloned();
                let stargazers_count = obj.get("stargazers_count").cloned();
                let forks_count = obj.get("forks_count").cloned();
                let created_at = obj.get("created_at").cloned();
                let updated_at = obj.get("updated_at").cloned();
                let topics = obj.get("topics").cloned();
                let html_url = obj.get("html_url").cloned();

                // 清空对象并只保留必要字段
                obj.clear();

                if let Some(v) = name {
                    obj.insert("name".to_string(), v);
                }
                if let Some(v) = description {
                    obj.insert("description".to_string(), v);
                }
                if let Some(v) = language {
                    obj.insert("language".to_string(), v);
                }
                if let Some(v) = stargazers_count {
                    obj.insert("stargazers_count".to_string(), v);
                }
                if let Some(v) = forks_count {
                    obj.insert("forks_count".to_string(), v);
                }
                if let Some(v) = created_at {
                    obj.insert("created_at".to_string(), v);
                }
                if let Some(v) = updated_at {
                    obj.insert("updated_at".to_string(), v);
                }
                if let Some(v) = topics {
                    obj.insert("topics".to_string(), v);
                }
                if let Some(v) = html_url {
                    obj.insert("html_url".to_string(), v);
                }
            }
        }
    }

    // 清洗 GitHub 用户信息 - 原地修改
    if let Some(user) = data["github"]["user"].as_object_mut() {
        let id = user.get("id").cloned();
        let login = user.get("login").cloned();
        let name = user.get("name").cloned();
        let bio = user.get("bio").cloned();
        let avatar_url = user.get("avatar_url").cloned();
        let company = user.get("company").cloned();
        let location = user.get("location").cloned();
        let public_repos = user.get("public_repos").cloned();
        let followers = user.get("followers").cloned();
        let following = user.get("following").cloned();
        let created_at = user.get("created_at").cloned();

        user.clear();

        if let Some(v) = id {
            user.insert("id".to_string(), v);
        }
        if let Some(v) = login {
            user.insert("login".to_string(), v);
        }
        if let Some(v) = name {
            user.insert("name".to_string(), v);
        }
        if let Some(v) = bio {
            user.insert("bio".to_string(), v);
        }
        if let Some(v) = avatar_url {
            user.insert("avatar_url".to_string(), v);
        }
        if let Some(v) = company {
            user.insert("company".to_string(), v);
        }
        if let Some(v) = location {
            user.insert("location".to_string(), v);
        }
        if let Some(v) = public_repos {
            user.insert("public_repos".to_string(), v);
        }
        if let Some(v) = followers {
            user.insert("followers".to_string(), v);
        }
        if let Some(v) = following {
            user.insert("following".to_string(), v);
        }
        if let Some(v) = created_at {
            user.insert("created_at".to_string(), v);
        }
    }

    // 清洗 Steam 游戏数据 - 原地修改
    if let Some(games) = data["steam"]["games"].as_array_mut() {
        // 限制游戏数量
        if games.len() > MAX_STEAM_GAMES {
            tracing::warn!(
                "⚠️ Truncating Steam games from {} to {}",
                games.len(),
                MAX_STEAM_GAMES
            );
            games.truncate(MAX_STEAM_GAMES);
        }

        for game in games.iter_mut() {
            if let Some(obj) = game.as_object_mut() {
                let appid = obj.get("appid").cloned();
                let name = obj.get("name").cloned();
                let playtime_forever = obj.get("playtime_forever").cloned();
                let playtime_2weeks = obj.get("playtime_2weeks").cloned();

                obj.clear();

                if let Some(v) = appid {
                    obj.insert("appid".to_string(), v);
                }
                if let Some(v) = name {
                    obj.insert("name".to_string(), v);
                }
                if let Some(v) = playtime_forever {
                    obj.insert("playtime_forever".to_string(), v);
                }
                if let Some(v) = playtime_2weeks {
                    obj.insert("playtime_2weeks".to_string(), v);
                }
            }
        }
    }

    // 清洗 Steam 用户信息 - 原地修改
    if let Some(user) = data["steam"]["user"].as_object_mut() {
        let steamid = user.get("steamid").cloned();
        let personaname = user.get("personaname").cloned();
        let avatar = user.get("avatar").cloned();
        let avatarfull = user.get("avatarfull").cloned();
        let profileurl = user.get("profileurl").cloned();
        let timecreated = user.get("timecreated").cloned();
        let communityvisibilitystate = user.get("communityvisibilitystate").cloned();
        let personastate = user.get("personastate").cloned();
        let personastate_label = user.get("personastate_label").cloned();
        let lastlogoff = user.get("lastlogoff").cloned();
        let gameid = user.get("gameid").cloned();
        let gameextrainfo = user.get("gameextrainfo").cloned();

        user.clear();

        if let Some(v) = steamid {
            user.insert("steamid".to_string(), v);
        }
        if let Some(v) = personaname {
            user.insert("personaname".to_string(), v);
        }
        if let Some(v) = avatar {
            user.insert("avatar".to_string(), v);
        }
        if let Some(v) = avatarfull {
            user.insert("avatarfull".to_string(), v);
        }
        if let Some(v) = profileurl {
            user.insert("profileurl".to_string(), v);
        }
        if let Some(v) = timecreated {
            user.insert("timecreated".to_string(), v);
        }
        if let Some(v) = communityvisibilitystate {
            user.insert("communityvisibilitystate".to_string(), v);
        }
        if let Some(v) = personastate {
            user.insert("personastate".to_string(), v);
        }
        if let Some(v) = personastate_label {
            user.insert("personastate_label".to_string(), v);
        }
        if let Some(v) = lastlogoff {
            user.insert("lastlogoff".to_string(), v);
        }
        if let Some(v) = gameid {
            user.insert("gameid".to_string(), v);
        }
        if let Some(v) = gameextrainfo {
            user.insert("gameextrainfo".to_string(), v);
        }
    }

    // 清洗 Bilibili 数据 - 原地修改，添加数量限制
    if let Some(bilibili) = data.get_mut("bilibili") {
        // 清洗收藏夹视频
        if let Some(favorites) = bilibili.get_mut("favorites") {
            if let Some(fav_array) = favorites.as_array_mut() {
                for fav in fav_array.iter_mut() {
                    if let Some(videos) = fav.get_mut("videos") {
                        if let Some(videos_array) = videos.as_array_mut() {
                            if videos_array.len() > MAX_BILIBILI_VIDEOS {
                                tracing::debug!(
                                    "⚠️ Truncating Bilibili videos from {} to {}",
                                    videos_array.len(),
                                    MAX_BILIBILI_VIDEOS
                                );
                                videos_array.truncate(MAX_BILIBILI_VIDEOS);
                            }
                        }
                    }
                }
            }
        }

        // 清洗追番数据
        if let Some(bangumi) = bilibili.get_mut("bangumi") {
            if let Some(bangumi_array) = bangumi.as_array_mut() {
                if bangumi_array.len() > MAX_BILIBILI_BANGUMI {
                    tracing::debug!(
                        "⚠️ Truncating Bilibili bangumi from {} to {}",
                        bangumi_array.len(),
                        MAX_BILIBILI_BANGUMI
                    );
                    bangumi_array.truncate(MAX_BILIBILI_BANGUMI);
                }
            }
        }
    }

    // 清洗网易云音乐数据 - 保留核心字段（优化内存使用）
    if let Some(netease) = data.get_mut("netease") {
        // 优化：原地修改而不是创建新数组，减少内存峰值
        if let Some(songs_value) = netease.get_mut("liked_songs") {
            if let Some(songs_array) = songs_value.as_array_mut() {
                let total_songs = songs_array.len();
                tracing::debug!("🧹 Cleaning {} netease songs in-place...", total_songs);

                // 限制歌曲数量，避免处理过多数据
                if songs_array.len() > MAX_SONGS_TO_CLEAN {
                    tracing::warn!(
                        "⚠️ Truncating songs from {} to {} to prevent memory issues",
                        songs_array.len(),
                        MAX_SONGS_TO_CLEAN
                    );
                    songs_array.truncate(MAX_SONGS_TO_CLEAN);
                }

                // 原地清洗每首歌曲，只保留必要字段（含 fee/isVip 供资料库 VIP 角标）
                for song in songs_array.iter_mut() {
                    if let Some(obj) = song.as_object_mut() {
                        // 保留的字段
                        let id = obj.get("id").cloned();
                        let name = obj.get("name").cloned();
                        let ar = obj.get("ar").cloned();
                        let artists = obj.get("artists").cloned();
                        let al = obj.get("al").cloned();
                        let pic_url = obj.get("picUrl").cloned();
                        let dt = obj.get("dt").cloned();
                        let fee = obj
                            .get("fee")
                            .cloned()
                            .or_else(|| obj.get("privilege").and_then(|p| p.get("fee")).cloned());
                        let is_vip = obj.get("isVip").or_else(|| obj.get("is_vip")).cloned();

                        // 清空对象并只保留必要字段
                        obj.clear();

                        if let Some(v) = id {
                            obj.insert("id".to_string(), v);
                        }
                        if let Some(v) = name {
                            obj.insert("name".to_string(), v);
                        }
                        if let Some(v) = ar {
                            obj.insert("ar".to_string(), v);
                        }
                        if let Some(v) = artists {
                            obj.insert("artists".to_string(), v);
                        }
                        if let Some(mut al_val) = al {
                            // 清洗专辑信息 - 原地修改避免额外分配
                            if let Some(al_obj) = al_val.as_object_mut() {
                                let id = al_obj.get("id").cloned();
                                let name = al_obj.get("name").cloned();
                                let pic_url = al_obj.get("picUrl").cloned();

                                al_obj.clear();

                                if let Some(v) = id {
                                    al_obj.insert("id".to_string(), v);
                                }
                                if let Some(v) = name {
                                    al_obj.insert("name".to_string(), v);
                                }
                                if let Some(v) = pic_url {
                                    al_obj.insert("picUrl".to_string(), v);
                                }
                            }
                            obj.insert("al".to_string(), al_val);
                        }
                        if let Some(v) = pic_url {
                            obj.insert("picUrl".to_string(), v);
                        }
                        if let Some(v) = dt {
                            obj.insert("dt".to_string(), v);
                        }
                        if let Some(v) = fee {
                            let fee_n = v.as_i64().unwrap_or(0);
                            obj.insert("fee".to_string(), v);
                            let vip = is_vip
                                .as_ref()
                                .and_then(|b| b.as_bool())
                                .unwrap_or(fee_n == 1 || fee_n == 4);
                            obj.insert("isVip".to_string(), json!(vip));
                        } else if let Some(v) = is_vip {
                            obj.insert("isVip".to_string(), v);
                        }
                    }
                }

                tracing::debug!("✅ Cleaned {} songs in-place", songs_array.len());
            }
        }

        // 清洗 profile 信息
        if let Some(profile) = netease.get("profile").cloned() {
            if let Some(profile_obj) = profile.as_object() {
                let cleaned_profile = json!({
                    "userId": profile_obj.get("userId"),
                    "nickname": profile_obj.get("nickname"),
                    "avatarUrl": profile_obj.get("avatarUrl"),
                    "backgroundUrl": profile_obj.get("backgroundUrl"),
                    "signature": profile_obj.get("signature"),
                    "gender": profile_obj.get("gender"),
                    "birthday": profile_obj.get("birthday"),
                    "province": profile_obj.get("province"),
                    "city": profile_obj.get("city"),
                    "followeds": profile_obj.get("followeds"),
                    "follows": profile_obj.get("follows"),
                    "eventCount": profile_obj.get("eventCount"),
                    "playlistCount": profile_obj.get("playlistCount"),
                    "level": profile_obj.get("level"),
                });
                if let Some(obj) = netease.as_object_mut() {
                    obj.insert("profile".to_string(), cleaned_profile);
                }
            }
        }
    }

    // 清洗 Bangumi 收藏数据 - 保留资料库、报告和分析需要的核心字段
    if let Some(bangumi) = data.get_mut("bangumi") {
        if let Some(collections) = bangumi
            .get_mut("collections")
            .and_then(|value| value.as_array_mut())
        {
            if collections.len() > MAX_BANGUMI_COLLECTIONS {
                tracing::warn!(
                    "⚠️ Truncating Bangumi collections from {} to {}",
                    collections.len(),
                    MAX_BANGUMI_COLLECTIONS
                );
                collections.truncate(MAX_BANGUMI_COLLECTIONS);
            }

            for collection in collections.iter_mut() {
                if let Some(obj) = collection.as_object_mut() {
                    let subject_id = obj.get("subject_id").cloned();
                    let subject_type = obj.get("subject_type").cloned();
                    let rate = obj.get("rate").cloned();
                    let collection_type = obj.get("type").cloned();
                    let comment = obj.get("comment").cloned();
                    let tags = obj.get("tags").cloned();
                    let ep_status = obj.get("ep_status").cloned();
                    let vol_status = obj.get("vol_status").cloned();
                    let updated_at = obj.get("updated_at").cloned();
                    let private = obj.get("private").cloned();
                    let subject = obj.get("subject").cloned();

                    obj.clear();

                    if let Some(v) = subject_id {
                        obj.insert("subject_id".to_string(), v);
                    }
                    if let Some(v) = subject_type {
                        obj.insert("subject_type".to_string(), v);
                    }
                    if let Some(v) = rate {
                        obj.insert("rate".to_string(), v);
                    }
                    if let Some(v) = collection_type {
                        obj.insert("type".to_string(), v);
                    }
                    if let Some(v) = comment {
                        obj.insert("comment".to_string(), v);
                    }
                    if let Some(v) = tags {
                        obj.insert("tags".to_string(), v);
                    }
                    if let Some(v) = ep_status {
                        obj.insert("ep_status".to_string(), v);
                    }
                    if let Some(v) = vol_status {
                        obj.insert("vol_status".to_string(), v);
                    }
                    if let Some(v) = updated_at {
                        obj.insert("updated_at".to_string(), v);
                    }
                    if let Some(v) = private {
                        obj.insert("private".to_string(), v);
                    }
                    if let Some(mut subject_value) = subject {
                        if let Some(subject_obj) = subject_value.as_object_mut() {
                            let id = subject_obj.get("id").cloned();
                            let subject_type = subject_obj.get("type").cloned();
                            let name = subject_obj.get("name").cloned();
                            let name_cn = subject_obj.get("name_cn").cloned();
                            let images = subject_obj.get("images").cloned();
                            let date = subject_obj.get("date").cloned();
                            let platform = subject_obj.get("platform").cloned();
                            let score = subject_obj.get("score").cloned();
                            let rank = subject_obj.get("rank").cloned();
                            let tags = subject_obj.get("tags").cloned();

                            subject_obj.clear();
                            if let Some(v) = id {
                                subject_obj.insert("id".to_string(), v);
                            }
                            if let Some(v) = subject_type {
                                subject_obj.insert("type".to_string(), v);
                            }
                            if let Some(v) = name {
                                subject_obj.insert("name".to_string(), v);
                            }
                            if let Some(v) = name_cn {
                                subject_obj.insert("name_cn".to_string(), v);
                            }
                            if let Some(v) = images {
                                subject_obj.insert("images".to_string(), v);
                            }
                            if let Some(v) = date {
                                subject_obj.insert("date".to_string(), v);
                            }
                            if let Some(v) = platform {
                                subject_obj.insert("platform".to_string(), v);
                            }
                            if let Some(v) = score {
                                subject_obj.insert("score".to_string(), v);
                            }
                            if let Some(v) = rank {
                                subject_obj.insert("rank".to_string(), v);
                            }
                            if let Some(v) = tags {
                                subject_obj.insert("tags".to_string(), v);
                            }
                        }
                        obj.insert("subject".to_string(), subject_value);
                    }
                }
            }
        }
    }

    // 清洗 X (Twitter) 数据
    if let Some(x_data) = data.get_mut("x") {
        // 用户字段精简
        if let Some(user) = x_data.get_mut("user").and_then(|v| v.as_object_mut()) {
            let id = user.get("id").cloned();
            let username = user.get("username").cloned();
            let name = user.get("name").cloned();
            let description = user.get("description").cloned();
            let profile_image_url = user.get("profile_image_url").cloned();
            let public_metrics = user.get("public_metrics").cloned();
            let verified = user.get("verified").cloned();
            let verified_type = user.get("verified_type").cloned();
            let created_at = user.get("created_at").cloned();
            let location = user.get("location").cloned();
            let url = user.get("url").cloned();
            let protected = user.get("protected").cloned();

            user.clear();
            if let Some(v) = id {
                user.insert("id".to_string(), v);
            }
            if let Some(v) = username {
                user.insert("username".to_string(), v);
            }
            if let Some(v) = name {
                user.insert("name".to_string(), v);
            }
            if let Some(v) = description {
                user.insert("description".to_string(), v);
            }
            if let Some(v) = profile_image_url {
                user.insert("profile_image_url".to_string(), v);
            }
            if let Some(v) = public_metrics {
                user.insert("public_metrics".to_string(), v);
            }
            if let Some(v) = verified {
                user.insert("verified".to_string(), v);
            }
            if let Some(v) = verified_type {
                user.insert("verified_type".to_string(), v);
            }
            if let Some(v) = created_at {
                user.insert("created_at".to_string(), v);
            }
            if let Some(v) = location {
                user.insert("location".to_string(), v);
            }
            if let Some(v) = url {
                user.insert("url".to_string(), v);
            }
            if let Some(v) = protected {
                user.insert("protected".to_string(), v);
            }
        }

        // 推文字段精简
        let clean_tweet = |tweet: &mut Value| {
            if let Some(obj) = tweet.as_object_mut() {
                let id = obj.get("id").cloned();
                let text = obj.get("text").cloned();
                let created_at = obj.get("created_at").cloned();
                let public_metrics = obj.get("public_metrics").cloned();
                let lang = obj.get("lang").cloned();
                let author = obj.get("author").cloned();
                let author_id = obj.get("author_id").cloned();

                obj.clear();
                if let Some(v) = id {
                    obj.insert("id".to_string(), v);
                }
                if let Some(v) = text {
                    obj.insert("text".to_string(), v);
                }
                if let Some(v) = created_at {
                    obj.insert("created_at".to_string(), v);
                }
                if let Some(v) = public_metrics {
                    obj.insert("public_metrics".to_string(), v);
                }
                if let Some(v) = lang {
                    obj.insert("lang".to_string(), v);
                }
                if let Some(v) = author {
                    obj.insert("author".to_string(), v);
                }
                if let Some(v) = author_id {
                    obj.insert("author_id".to_string(), v);
                }
            }
        };

        if let Some(tweets) = x_data.get_mut("tweets").and_then(|v| v.as_array_mut()) {
            if tweets.len() > MAX_X_TWEETS {
                tweets.truncate(MAX_X_TWEETS);
            }
            for tweet in tweets.iter_mut() {
                clean_tweet(tweet);
            }
        }

        // 关注列表字段精简：只保留 SmartFilter 消费的字段（entities 等全量字段体积很大）
        if let Some(following) = x_data.get_mut("following").and_then(|v| v.as_array_mut()) {
            for account in following.iter_mut() {
                if let Some(obj) = account.as_object_mut() {
                    obj.retain(|key, _| {
                        matches!(
                            key.as_str(),
                            "id" | "username"
                                | "name"
                                | "description"
                                | "verified"
                                | "public_metrics"
                                | "profile_image_url"
                        )
                    });
                }
            }
        }

        // 不再同步 likes；清理历史字段
        if let Some(obj) = x_data.as_object_mut() {
            obj.remove("liked_tweets");
        }
    }

    tracing::info!("✓ Platform data cleaned (removed unnecessary fields)");
}

/// 获取最近一次获取的原始元数据（用于调试）

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn humanize_x_402_credits_depleted() {
        let msg = humanize_platform_fetch_error(
            "x",
            "X API error (402 Payment Required): credits depleted",
        );
        assert!(msg.contains("额度"), "{msg}");
        assert!(msg.contains("402") || msg.contains("Credits"), "{msg}");
    }

    #[test]
    fn resolve_prefers_remote_error_when_empty() {
        let msg = resolve_platform_fetch_message(
            "x",
            None,
            Some("X API error (402 Payment Required): credits depleted"),
        )
        .unwrap();
        assert!(msg.contains("额度"), "{msg}");
        assert!(!msg.contains("未返回任何数据"), "{msg}");
    }

    #[test]
    fn resolve_keeps_usable_note_when_data_present() {
        let data = json!({"user": {"id": "1", "username": "hitomi"}, "tweets": []});
        let msg = resolve_platform_fetch_message(
            "x",
            Some(&data),
            Some("X API error (402 Payment Required): credits depleted"),
        )
        .unwrap();
        assert!(msg.contains("可用数据"), "{msg}");
    }

    #[test]
    fn humanize_rate_limit_and_auth_generic() {
        let r = humanize_platform_fetch_error("steam", "HTTP 429 Too Many Requests");
        assert!(r.contains("频繁") || r.contains("配额"), "{r}");
        let a = humanize_platform_fetch_error("github", "401 Unauthorized: Bad credentials");
        assert!(a.contains("鉴权"), "{a}");
    }

    #[test]
    fn note_fetch_error_appends_stages() {
        let mut map = std::collections::HashMap::new();
        note_fetch_error(&mut map, "steam", "user", "boom");
        note_fetch_error(&mut map, "steam", "games", "nope");
        let v = map.get("steam").unwrap();
        assert!(v.contains("user: boom"), "{v}");
        assert!(v.contains("games: nope"), "{v}");
    }

    #[test]
    fn platform_data_warning_none_when_payload_present() {
        let data = json!({"games": [{"appid": 1}], "user": {"name": "x"}});
        assert!(platform_data_warning("steam", Some(&data)).is_none());
    }

    #[test]
    fn platform_data_warning_when_missing_or_null() {
        assert!(platform_data_warning("steam", None)
            .unwrap()
            .contains("未返回"));
        assert!(platform_data_warning("steam", Some(&Value::Null))
            .unwrap()
            .contains("未返回"));
    }

    #[test]
    fn platform_data_warning_steam_empty_games() {
        let data = json!({"games": []});
        let w = platform_data_warning("steam", Some(&data)).unwrap();
        assert!(w.contains("Steam"), "{w}");
    }

    #[test]
    fn platform_data_warning_bangumi_empty_collections() {
        let data = json!({"collections": []});
        let w = platform_data_warning("bangumi", Some(&data)).unwrap();
        assert!(w.contains("Bangumi"), "{w}");
    }

    #[test]
    fn platform_data_warning_mal_empty_lists() {
        let data = json!({"anime_list": [], "manga_list": []});
        let w = platform_data_warning("mal", Some(&data)).unwrap();
        assert!(w.contains("MyAnimeList"), "{w}");
    }
}
