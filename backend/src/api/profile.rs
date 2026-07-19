use crate::services::fetcher::PlatformFetcher;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize)]
pub struct FetchPlatformRequest {
    pub platform: String,
}

// 数据缓存结构
#[derive(Debug, Serialize, Deserialize, Clone)]
struct PlatformDataCache {
    data: Value,
    fetched_at: DateTime<Utc>,
}

const PLATFORM_CACHE_HOURS: i64 = 12; // 数据缓存12小时

use std::collections::{HashMap, HashSet};

/// Resolve the site owner's user id for public surfaces (home dashboard, profile,
/// library, `/api/reports/latest`, …).
///
/// Prefer durable `users.is_owner` (the account that owns platform reports and
/// dashboard content). Falling back to the lowest admin id keeps pre-`is_owner`
/// databases working.
pub(crate) async fn site_owner_user_id(db: &DatabaseConnection) -> Result<i32, String> {
    // 1) Durable site owner flag
    if let Ok(Some(row)) = db
        .query_one(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id FROM users WHERE is_owner = true ORDER BY id ASC LIMIT 1".to_string(),
        ))
        .await
    {
        if let Ok(id) = row.try_get::<i32>("", "id") {
            return Ok(id);
        }
    }

    // 2) Legacy: first admin (pre-is_owner installs / column missing)
    let row = db
        .query_one(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id FROM users WHERE is_admin = true ORDER BY id ASC LIMIT 1".to_string(),
        ))
        .await
        .map_err(|error| format!("Failed to resolve site owner: {error}"))?;
    row.and_then(|row| row.try_get::<i32>("", "id").ok())
        .ok_or_else(|| "No administrator is configured as the site owner".to_string())
}

fn site_owner_error(error: String) -> (StatusCode, Json<Value>) {
    tracing::warn!(%error, "Site owner lookup failed");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "success": false,
            "message": "Site owner is not configured"
        })),
    )
}

/// 从磁盘加载平台数据缓存
fn load_platform_data_cache() -> Option<PlatformDataCache> {
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
fn save_platform_data_cache(data: &Value) -> Result<(), Box<dyn std::error::Error>> {
    // 保存分平台的原始数据
    save_split_raw_data(data)
}

/// 保存分平台的原始数据（避免读取大文件）
/// 🚀 优化：添加错误容错和大文件分块写入
fn save_split_raw_data(all_data: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let raw_dir = PathBuf::from("./cache/raw");
    if !raw_dir.exists() {
        fs::create_dir_all(&raw_dir)?;
    }

    if let Some(obj) = all_data.as_object() {
        for (platform, data) in obj {
            // 保存所有平台的数据，不仅仅是主要平台
            let file_path = raw_dir.join(format!("{}.json", platform));

            // 🚀 优化：先写入临时文件，然后原子性重命名，避免写入中断导致文件损坏
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

/// 一键获取所有平台数据（带缓存）
pub async fn fetch_all_data(State(db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    tracing::info!("Starting fetch all data...");

    // 检查缓存
    if let Some(cache) = load_platform_data_cache() {
        tracing::info!("📦 Returning cached platform data");
        return (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "message": "Data loaded from cache",
                "data": cache.data,
                "fetched_at": cache.fetched_at.to_rfc3339(),
                "from_cache": true
            })),
        );
    }

    // 缓存不存在或已过期，重新获取
    tracing::info!("🔄 Fetching fresh platform data...");
    match fetch_fresh_platform_data(&db, None).await {
        Ok(data) => {
            // 保存到缓存
            if let Err(e) = save_platform_data_cache(&data) {
                tracing::error!("Failed to save platform cache: {}", e);
            }

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "message": "Data fetched successfully",
                    "data": data,
                    "fetched_at": chrono::Utc::now().to_rfc3339(),
                    "from_cache": false
                })),
            )
        }
        Err(e) => {
            tracing::error!("Failed to fetch platform data: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Failed to fetch data: {}", e)
                })),
            )
        }
    }
}

/// 手动刷新平台数据
#[derive(Deserialize)]
pub struct RefreshQuery {
    #[serde(default)]
    force: bool,
}

pub async fn refresh_platform_data(
    State(db): State<DatabaseConnection>,
    Query(query): Query<RefreshQuery>,
) -> (StatusCode, Json<Value>) {
    if !query.force {
        // 如果不是强制刷新，检查缓存
        if let Some(cache) = load_platform_data_cache() {
            let age = Utc::now() - cache.fetched_at;
            if age < Duration::hours(PLATFORM_CACHE_HOURS) {
                return (
                    StatusCode::OK,
                    Json(json!({
                        "success": true,
                        "message": "Data still fresh, use force=true to refresh anyway",
                        "data": cache.data,
                        "fetched_at": cache.fetched_at.to_rfc3339(),
                        "age_hours": age.num_hours()
                    })),
                );
            }
        }
    }

    tracing::info!("🔄 Force refreshing platform data...");
    match fetch_fresh_platform_data(&db, None).await {
        Ok(data) => {
            // 保存到缓存
            if let Err(e) = save_platform_data_cache(&data) {
                tracing::error!("Failed to save platform cache: {}", e);
            }

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "message": "Data refreshed successfully",
                    "data": data,
                    "fetched_at": chrono::Utc::now().to_rfc3339()
                })),
            )
        }
        Err(e) => {
            tracing::error!("Failed to refresh platform data: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Failed to refresh data: {}", e)
                })),
            )
        }
    }
}

/// 检查抓取回来的平台数据是否为空/缺失，返回给用户的可读提示。
/// 返回 None 表示数据看起来正常。
fn platform_data_warning(platform: &str, data: Option<&Value>) -> Option<String> {
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
        _ => None,
    }
}

/// 刷新单个平台数据
pub async fn fetch_single_platform_data(
    State(db): State<DatabaseConnection>,
    Json(req): Json<FetchPlatformRequest>,
) -> (StatusCode, Json<Value>) {
    tracing::info!("🔄 Fetching data for platform: {}...", req.platform);

    match fetch_fresh_platform_data(&db, Some(&req.platform)).await {
        Ok(data) => {
            // 只保存请求的平台数据，而不是所有平台
            if let Some(platform_data) = data.get(&req.platform) {
                let single_platform_data = json!({
                    &req.platform: platform_data
                });
                if let Err(e) = save_platform_data_cache(&single_platform_data) {
                    tracing::error!("Failed to save platform cache: {}", e);
                }
            }

            // 检测该平台是否真的取到可用数据，空数据不再伪装成成功
            let warning = platform_data_warning(&req.platform, data.get(&req.platform));
            if let Some(warning) = warning {
                tracing::warn!(
                    "⚠️ {} fetched but data looks empty: {}",
                    req.platform,
                    warning
                );
                return (
                    StatusCode::OK,
                    Json(json!({
                        "success": false,
                        "message": warning,
                        "data": data,
                        "fetched_at": chrono::Utc::now().to_rfc3339()
                    })),
                );
            }

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "message": format!("Data for {} fetched successfully", req.platform),
                    "data": data,
                    "fetched_at": chrono::Utc::now().to_rfc3339()
                })),
            )
        }
        Err(e) => {
            tracing::error!("Failed to fetch platform data: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Failed to fetch data: {}", e)
                })),
            )
        }
    }
}

/// 计划器使用的单平台刷新入口。
/// 复用与手动刷新相同的抓取、缓存合并和空数据校验，避免 scheduler 维护一套假实现。
pub(crate) async fn refresh_platform_for_scheduler(
    db: &DatabaseConnection,
    platform: &str,
) -> Result<Value, String> {
    let data = fetch_fresh_platform_data(db, Some(platform))
        .await
        .map_err(|error| error.to_string())?;
    if let Some(platform_data) = data.get(platform) {
        save_platform_data_cache(&json!({ (platform): platform_data }))
            .map_err(|error| error.to_string())?;
    }
    if let Some(warning) = platform_data_warning(platform, data.get(platform)) {
        return Err(warning);
    }
    Ok(data.get(platform).cloned().unwrap_or(Value::Null))
}

/// 获取新鲜的平台数据（实际执行API调用）
async fn fetch_fresh_platform_data(
    db: &DatabaseConnection,
    target_platform: Option<&str>,
) -> Result<Value, Box<dyn std::error::Error>> {
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

    // 辅助闭包：判断是否应该获取该平台
    let should_fetch = |p: &str| target_platform.is_none() || target_platform == Some(p);
    let is_platform_enabled = |p: &str| match p {
        "github" => config
            .github_enabled
            .unwrap_or(config.github_username.as_ref().is_some()),
        "bilibili" => config
            .bilibili_enabled
            .unwrap_or(config.bilibili_uid.as_ref().is_some()),
        "steam" => config
            .steam_enabled
            .unwrap_or(config.steam_api_key.as_ref().is_some()),
        "netease" => config
            .netease_enabled
            .unwrap_or(config.netease_user_id.as_ref().is_some()),
        "bangumi" => config.bangumi_enabled.unwrap_or(
            config.bangumi_username.as_ref().is_some()
                || config.bangumi_access_token.as_ref().is_some(),
        ),
        "x" => config.x_enabled.unwrap_or(
            config.x_username.as_ref().is_some() && config.x_bearer_token.as_ref().is_some(),
        ),
        "discord" => config
            .discord_enabled
            .unwrap_or(config.discord_access_token.as_ref().is_some()),
        "mal" => config
            .mal_enabled
            .unwrap_or(config.mal_username.as_ref().is_some()),
        "xbox" => {
            let has_gamertag = config
                .xbox_gamertag
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
                || std::env::var("XBOX_GAMERTAG").is_ok();
            let has_key = config
                .openxbl_api_key
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
                || std::env::var("OPENXBL_API_KEY").is_ok()
                || std::env::var("XBL_API_KEY").is_ok();
            config.xbox_enabled.unwrap_or(has_gamertag && has_key)
        }
        "psn" => {
            let has_id = config
                .psn_online_id
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
                || std::env::var("PSN_ONLINE_ID").is_ok();
            let has_npsso = config
                .psn_npsso
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
                || std::env::var("PSN_NPSSO").is_ok();
            config.psn_enabled.unwrap_or(has_id && has_npsso)
        }
        _ => false,
    };

    // 创建元数据服务
    let metadata_service = crate::services::metadata_service::MetadataService::new(db.clone());

    // 获取GitHub数据（包含仓库信息）
    if should_fetch("github") && is_platform_enabled("github") {
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
                Err(e) => tracing::warn!("GitHub user fetch failed: {}", e),
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
                Err(e) => tracing::warn!("GitHub repos fetch failed: {}", e),
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
                Err(e) => tracing::warn!("⚠ GitHub contributions fetch failed: {}", e),
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
    if should_fetch("bilibili") && is_platform_enabled("bilibili") {
        if let Some(uid_str) = &config.bilibili_uid {
            if let Ok(uid) = uid_str.parse::<i64>() {
                match fetcher.fetch_bilibili_user(uid).await {
                    Ok(user_data) => {
                        // 使用 user_info 字段名以匹配 SmartFilter 的期待
                        all_data["bilibili"]["user_info"] = json!(user_data);
                        tracing::info!("✓ Bilibili user data fetched");
                    }
                    Err(e) => tracing::warn!("Bilibili user fetch failed: {}", e),
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
                    Err(e) => tracing::warn!("Bilibili bangumi fetch failed: {}", e),
                }

                // 获取收藏夹
                match fetcher.fetch_bilibili_favorites(uid).await {
                    Ok(favorites) => {
                        all_data["bilibili"]["favorites"] = json!(favorites);
                        tracing::info!("✓ Bilibili favorites fetched: {} items", favorites.len());
                    }
                    Err(e) => tracing::warn!("Bilibili favorites fetch failed: {}", e),
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
    if should_fetch("steam") && is_platform_enabled("steam") {
        if let (Some(api_key), Some(steam_id)) = (&config.steam_api_key, &config.steam_id) {
            match fetcher.fetch_steam_user(api_key, steam_id).await {
                Ok(user_data) => {
                    all_data["steam"]["user"] = json!(user_data);
                    tracing::info!("✓ Steam user data fetched");
                }
                Err(e) => tracing::warn!("Steam user fetch failed: {}", e),
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
                Err(e) => tracing::warn!("Steam games fetch failed: {}", e),
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
    if should_fetch("netease") && is_platform_enabled("netease") {
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
                    Err(e) => tracing::warn!("Netease user fetch failed: {}", e),
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
                    Err(e) => tracing::warn!("Netease Cloud Music fetch failed: {}", e),
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
    if should_fetch("bangumi") && is_platform_enabled("bangumi") {
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
                        Err(e) => tracing::warn!("Bangumi collections fetch failed: {}", e),
                    }
                } else {
                    tracing::warn!(
                        "Bangumi user data did not include username; skipping collections"
                    );
                }
            }
            Err(e) => tracing::warn!("Bangumi user fetch failed: {}", e),
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
    if should_fetch("x") && is_platform_enabled("x") {
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
                Err(e) => tracing::warn!("X fetch failed: {}", e),
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
    if should_fetch("discord") && is_platform_enabled("discord") {
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
                Err(e) => tracing::warn!("Discord fetch failed: {}", e),
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
    if should_fetch("mal") && is_platform_enabled("mal") {
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
            match fetcher
                .fetch_mal_profile_bundle(username, client_id)
                .await
            {
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
                Err(e) => tracing::warn!("MyAnimeList fetch failed: {}", e),
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
    if should_fetch("xbox") && is_platform_enabled("xbox") {
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
                Err(e) => tracing::warn!("Xbox fetch failed: {}", e),
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
    if should_fetch("psn") && is_platform_enabled("psn") {
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
                Err(e) => tracing::warn!("PSN fetch failed: {}", e),
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

    Ok(all_data)
}

/// 清洗平台数据，只保留核心信息（符合5W1H原则）
/// 🚀 优化：原地修改减少内存峰值，添加数据量限制
fn clean_platform_data(data: &mut Value) {
    // 🚀 内存保护：各平台最大数据量限制
    const MAX_GITHUB_REPOS: usize = 200;
    const MAX_STEAM_GAMES: usize = 500;
    const MAX_BILIBILI_VIDEOS: usize = 100;
    const MAX_BILIBILI_BANGUMI: usize = 100;
    const MAX_SONGS_TO_CLEAN: usize = 5000;
    const MAX_BANGUMI_COLLECTIONS: usize = 1000;
    const MAX_X_TWEETS: usize = 100;

    // 清洗 GitHub 仓库数据 - 原地修改
    if let Some(repos) = data["github"]["repos"].as_array_mut() {
        // 🚀 限制仓库数量
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
        // 🚀 限制游戏数量
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
        // 🚀 优化：原地修改而不是创建新数组，减少内存峰值
        if let Some(songs_value) = netease.get_mut("liked_songs") {
            if let Some(songs_array) = songs_value.as_array_mut() {
                let total_songs = songs_array.len();
                tracing::debug!("🧹 Cleaning {} netease songs in-place...", total_songs);

                // 🚀 限制歌曲数量，避免处理过多数据
                if songs_array.len() > MAX_SONGS_TO_CLEAN {
                    tracing::warn!(
                        "⚠️ Truncating songs from {} to {} to prevent memory issues",
                        songs_array.len(),
                        MAX_SONGS_TO_CLEAN
                    );
                    songs_array.truncate(MAX_SONGS_TO_CLEAN);
                }

                // 原地清洗每首歌曲，只保留必要字段
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
pub async fn get_raw_metadata(State(_db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    tracing::info!("📊 Reading cached platform metadata for debugging...");

    // 尝试从缓存文件读取
    if let Some(cache) = load_platform_data_cache() {
        let age = Utc::now() - cache.fetched_at;
        let age_hours = age.num_hours();

        tracing::info!("✅ Found cached platform data (age: {}h)", age_hours);

        return (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "data": cache.data,
                "fetched_at": cache.fetched_at.to_rfc3339(),
                "cache_age_hours": age_hours,
                "is_fresh": age_hours < PLATFORM_CACHE_HOURS
            })),
        );
    }

    // 如果没有缓存，返回提示信息
    tracing::warn!("⚠️ No cached platform data found");
    (
        StatusCode::OK,
        Json(json!({
            "success": false,
            "message": "No cached platform data found. Please fetch data first or generate a report.",
            "data": {}
        })),
    )
}

/// 从数据库或缓存中获取用户信息（支持多平台）
/// 优先从数据库获取，若数据库无数据则从缓存获取
pub async fn get_user_info(State(db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    let user_id = match site_owner_user_id(&db).await {
        Ok(user_id) => user_id,
        Err(error) => return site_owner_error(error),
    };

    // 创建元数据服务
    let metadata_service = crate::services::metadata_service::MetadataService::new(db.clone());

    // 1. 优先从数据库获取最新数据
    match metadata_service.get_all_latest_metadata(user_id).await {
        Ok(db_data) if !db_data.is_empty() => {
            tracing::info!("📊 Returning user info from database");

            // 优先从 Bilibili 获取
            if let Some(bilibili_data) = db_data.get("bilibili") {
                if let Some(bilibili_user) = bilibili_data.get("user") {
                    return (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "user_info": {
                                "name": bilibili_user.get("name"),
                                "avatar": bilibili_user.get("face"),
                                "bio": bilibili_user.get("sign").and_then(|s| s.as_str()).filter(|s| !s.is_empty()).unwrap_or("这家伙很懒，没有介绍呢"),
                                "platform": "Bilibili"
                            },
                            "source": "database"
                        })),
                    );
                }
            }

            // 其次从 GitHub 获取
            if let Some(github_data) = db_data.get("github") {
                if let Some(github_user) = github_data.get("user") {
                    return (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "user_info": {
                                "name": github_user.get("name").and_then(|n| n.as_str()).or_else(|| github_user.get("login").and_then(|l| l.as_str())),
                                "avatar": github_user.get("avatar_url"),
                                "bio": github_user.get("bio").and_then(|b| b.as_str()).filter(|s| !s.is_empty()).unwrap_or("这家伙很懒，没有介绍呢"),
                                "platform": "GitHub"
                            },
                            "source": "database"
                        })),
                    );
                }
            }

            // 最后从 Steam 获取
            if let Some(steam_data) = db_data.get("steam") {
                if let Some(steam_user) = steam_data.get("user") {
                    return (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "user_info": {
                                "name": steam_user.get("personaname"),
                                "avatar": steam_user.get("avatarfull").or_else(|| steam_user.get("avatar")),
                                "bio": "Steam 玩家",
                                "platform": "Steam"
                            },
                            "source": "database"
                        })),
                    );
                }
            }
        }
        Ok(_) => {
            tracing::info!("📊 Database is empty, falling back to cache");
        }
        Err(e) => {
            tracing::warn!(
                "Failed to fetch from database: {}, falling back to cache",
                e
            );
        }
    }

    // 2. 降级：从缓存文件获取数据
    if let Some(cache) = load_platform_data_cache() {
        tracing::info!("📦 Returning user info from cache file");
        let data = &cache.data;

        // 优先从 Bilibili 获取
        if let Some(bilibili_user) = data.get("bilibili").and_then(|b| b.get("user")) {
            return (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "user_info": {
                        "name": bilibili_user.get("name"),
                        "avatar": bilibili_user.get("face"),
                        "bio": bilibili_user.get("sign").and_then(|s| s.as_str()).filter(|s| !s.is_empty()).unwrap_or("这家伙很懒，没有介绍呢"),
                        "platform": "Bilibili"
                    },
                    "source": "cache"
                })),
            );
        }

        // 其次从 GitHub 获取
        if let Some(github_user) = data.get("github").and_then(|g| g.get("user")) {
            return (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "user_info": {
                        "name": github_user.get("name").and_then(|n| n.as_str()).or_else(|| github_user.get("login").and_then(|l| l.as_str())),
                        "avatar": github_user.get("avatar_url"),
                        "bio": github_user.get("bio").and_then(|b| b.as_str()).filter(|s| !s.is_empty()).unwrap_or("这家伙很懒，没有介绍呢"),
                        "platform": "GitHub"
                    },
                    "source": "cache"
                })),
            );
        }

        // 最后从 Steam 获取
        if let Some(steam_user) = data.get("steam").and_then(|s| s.get("user")) {
            return (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "user_info": {
                        "name": steam_user.get("personaname"),
                        "avatar": steam_user.get("avatarfull").or_else(|| steam_user.get("avatar")),
                        "bio": "Steam 玩家",
                        "platform": "Steam"
                    },
                    "source": "cache"
                })),
            );
        }
    }

    // 3. 数据库和缓存都没有数据
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "success": false,
            "message": "No user info found in database or cache. Please fetch platform data first."
        })),
    )
}

/// 删除平台数据缓存
pub async fn delete_platform_cache(
    State(_db): State<DatabaseConnection>,
) -> (StatusCode, Json<Value>) {
    tracing::info!("🗑️ Deleting platform data cache...");

    let mut success = true;
    let mut messages = Vec::new();

    // 1. 删除分平台数据
    let raw_dir = PathBuf::from("./cache/raw");
    if raw_dir.exists() {
        match fs::remove_dir_all(&raw_dir) {
            Ok(_) => {
                messages.push("Split raw data deleted".to_string());
            }
            Err(e) => {
                success = false;
                messages.push(format!("Failed to delete split raw data: {}", e));
                tracing::error!("❌ Failed to delete split raw data: {}", e);
            }
        }
    }

    if success {
        tracing::info!("✓ Platform cache deleted successfully");
        (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "message": if messages.is_empty() { "Cache already empty".to_string() } else { messages.join(", ") }
            })),
        )
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": messages.join(", ")
            })),
        )
    }
}

/// 将图片URL转换为代理URL（用于处理防盗链）
pub fn proxy_image_url(url: &str) -> String {
    // 检查是否需要代理（平台图片防盗链/跨域）
    if url.contains("hdslb.com")
        || url.contains("bilibili.com")
        || url.contains("bgm.tv")
        || url.contains("bangumi.tv")
        || url.contains("chii.in")
    {
        format!("/api/proxy/image?url={}", urlencoding::encode(url))
    } else {
        url.to_string()
    }
}

/// 资料库数据项
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LibraryItem {
    pub id: String,
    pub item_type: String, // "game", "video", "music"
    pub title: String,
    pub cover: Option<String>,
    pub platform: String,
    pub metadata: Value,
}

const LIBRARY_SOURCE_PREFERENCES_KEY: &str = "library_source_preferences";
const LIBRARY_ITEM_TYPES: [&str; 6] = ["game", "video", "music", "anime", "tv_series", "book"];
const LIBRARY_PLATFORMS: [&str; 5] = ["Steam", "Bilibili", "Bangumi", "Netease", "MyAnimeList"];

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LibrarySourcePreferences {
    #[serde(default = "default_library_source_categories")]
    pub categories: HashMap<String, Vec<String>>,
}

#[derive(Debug, Serialize)]
struct LibrarySourceOption {
    source: String,
    count: usize,
}

fn default_library_source_categories() -> HashMap<String, Vec<String>> {
    HashMap::from([
        (
            "game".to_string(),
            vec!["Steam".to_string(), "Bangumi".to_string()],
        ),
        (
            "video".to_string(),
            vec!["Bilibili".to_string(), "Bangumi".to_string()],
        ),
        (
            "music".to_string(),
            vec!["Netease".to_string(), "Bangumi".to_string()],
        ),
        (
            "anime".to_string(),
            vec![
                "Bangumi".to_string(),
                "Bilibili".to_string(),
                "MyAnimeList".to_string(),
            ],
        ),
        (
            "tv_series".to_string(),
            vec!["Bangumi".to_string(), "Bilibili".to_string()],
        ),
        (
            "book".to_string(),
            vec!["Bangumi".to_string(), "MyAnimeList".to_string()],
        ),
    ])
}

impl Default for LibrarySourcePreferences {
    fn default() -> Self {
        Self {
            categories: default_library_source_categories(),
        }
    }
}

impl LibrarySourcePreferences {
    pub(crate) fn normalized(mut self) -> Self {
        let defaults = default_library_source_categories();
        let mut normalized = HashMap::new();

        for item_type in LIBRARY_ITEM_TYPES {
            let sources = self
                .categories
                .remove(item_type)
                .unwrap_or_else(|| defaults.get(item_type).cloned().unwrap_or_default());
            normalized.insert(item_type.to_string(), normalize_platform_list(sources));
        }

        self.categories = normalized;
        self
    }

    fn enabled_sources_for(&self, item_type: &str) -> Vec<String> {
        self.categories.get(item_type).cloned().unwrap_or_else(|| {
            default_library_source_categories()
                .get(item_type)
                .cloned()
                .unwrap_or_default()
        })
    }

    fn source_enabled(&self, item_type: &str, platform: &str) -> bool {
        let platform = canonical_library_platform(platform);
        self.enabled_sources_for(item_type).contains(&platform)
    }
}

fn canonical_library_platform(platform: &str) -> String {
    let trimmed = platform.trim();
    let key = platform
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect::<String>();

    match key.as_str() {
        "steam" => "Steam".to_string(),
        "bilibili" | "bili" => "Bilibili".to_string(),
        "bangumi" | "bgm" => "Bangumi".to_string(),
        "x" | "twitter" | "xtwitter" => "X".to_string(),
        "netease" | "neteasemusic" | "neteasecloudmusic" => "Netease".to_string(),
        "mal" | "myanimelist" => "MyAnimeList".to_string(),
        "xbox" => "Xbox".to_string(),
        "psn" | "playstation" => "PlayStation".to_string(),
        _ => trimmed.to_string(),
    }
}

fn normalize_platform_list(sources: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for source in sources {
        let platform = canonical_library_platform(&source);
        if !platform.is_empty() && seen.insert(platform.clone()) {
            normalized.push(platform);
        }
    }

    normalized
}

async fn load_library_source_preferences(db: &DatabaseConnection) -> LibrarySourcePreferences {
    let sql = "SELECT value FROM configurations WHERE key = $1";
    let result = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![LIBRARY_SOURCE_PREFERENCES_KEY.into()],
        ))
        .await;

    match result {
        Ok(Some(row)) => match row.try_get::<Value>("", "value") {
            Ok(value) => serde_json::from_value::<LibrarySourcePreferences>(value)
                .map(LibrarySourcePreferences::normalized)
                .unwrap_or_else(|e| {
                    tracing::warn!("Invalid library source preferences, using defaults: {}", e);
                    LibrarySourcePreferences::default()
                }),
            Err(e) => {
                tracing::warn!("Failed to read library source preferences: {}", e);
                LibrarySourcePreferences::default()
            }
        },
        Ok(None) => LibrarySourcePreferences::default(),
        Err(e) => {
            tracing::warn!("Failed to load library source preferences: {}", e);
            LibrarySourcePreferences::default()
        }
    }
}

fn collect_library_source_options(
    items: &[LibraryItem],
) -> HashMap<String, Vec<LibrarySourceOption>> {
    let mut counts: HashMap<String, HashMap<String, usize>> = HashMap::new();
    for item in items {
        let item_type = item.item_type.clone();
        let platform = canonical_library_platform(&item.platform);
        *counts
            .entry(item_type)
            .or_default()
            .entry(platform)
            .or_insert(0) += 1;
    }

    let platform_order = |source: &str| {
        LIBRARY_PLATFORMS
            .iter()
            .position(|candidate| candidate == &source)
            .unwrap_or(usize::MAX)
    };

    let mut options = HashMap::new();
    for item_type in LIBRARY_ITEM_TYPES {
        let mut source_options = counts
            .remove(item_type)
            .unwrap_or_default()
            .into_iter()
            .map(|(source, count)| LibrarySourceOption { source, count })
            .collect::<Vec<_>>();
        source_options.sort_by_key(|option| platform_order(&option.source));
        options.insert(item_type.to_string(), source_options);
    }

    options
}

fn apply_library_source_preferences(
    items: Vec<LibraryItem>,
    preferences: &LibrarySourcePreferences,
) -> Vec<LibraryItem> {
    items
        .into_iter()
        .filter(|item| preferences.source_enabled(&item.item_type, &item.platform))
        .collect()
}

pub async fn get_library_source_preferences(
    State(db): State<DatabaseConnection>,
) -> (StatusCode, Json<Value>) {
    let preferences = load_library_source_preferences(&db).await;
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "preferences": preferences
        })),
    )
}

pub async fn update_library_source_preferences(
    State(db): State<DatabaseConnection>,
    Json(payload): Json<LibrarySourcePreferences>,
) -> (StatusCode, Json<Value>) {
    let preferences = payload.normalized();
    let config_service = crate::services::config_service::ConfigService::new(db);

    match config_service
        .update_config(
            LIBRARY_SOURCE_PREFERENCES_KEY,
            serde_json::to_value(&preferences).unwrap_or_else(|_| json!({})),
        )
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "preferences": preferences
            })),
        ),
        Err(e) => {
            tracing::error!("Failed to save library source preferences: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": "Failed to save library source preferences"
                })),
            )
        }
    }
}

fn bangumi_library_item_type(subject_type: i64, platform: Option<&str>) -> &'static str {
    match subject_type {
        1 => "book",
        2 => "anime",
        3 => "music",
        4 => "game",
        6 => {
            let platform = platform.unwrap_or_default();
            if platform.contains("TV")
                || platform.contains("剧")
                || platform.contains("Drama")
                || platform.contains("电视剧")
            {
                "tv_series"
            } else {
                "video"
            }
        }
        _ => "video",
    }
}

fn append_bangumi_library_items(library_items: &mut Vec<LibraryItem>, bangumi_data: &Value) {
    let Some(collections) = bangumi_data.get("collections").and_then(|c| c.as_array()) else {
        return;
    };

    let mut added = 0usize;
    for collection in collections {
        let subject = collection.get("subject").unwrap_or(collection);
        let subject_id = collection
            .get("subject_id")
            .and_then(|v| v.as_i64())
            .or_else(|| subject.get("id").and_then(|v| v.as_i64()));
        let Some(subject_id) = subject_id else {
            continue;
        };

        let title = subject
            .get("name_cn")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| subject.get("name").and_then(|v| v.as_str()))
            .unwrap_or("Unknown");
        let subject_type = collection
            .get("subject_type")
            .and_then(|v| v.as_i64())
            .or_else(|| subject.get("type").and_then(|v| v.as_i64()))
            .unwrap_or(0);
        let subject_platform = subject.get("platform").and_then(|v| v.as_str());
        let item_type = bangumi_library_item_type(subject_type, subject_platform);
        let cover = subject
            .get("images")
            .and_then(|images| {
                images
                    .get("large")
                    .or_else(|| images.get("common"))
                    .or_else(|| images.get("medium"))
                    .or_else(|| images.get("small"))
            })
            .and_then(|v| v.as_str())
            .map(proxy_image_url);

        let mut metadata = collection.clone();
        if let Some(obj) = metadata.as_object_mut() {
            obj.insert(
                "url".to_string(),
                json!(format!("https://bgm.tv/subject/{}", subject_id)),
            );
            obj.insert(
                "platform".to_string(),
                json!(subject_platform.unwrap_or("Bangumi")),
            );
        }

        library_items.push(LibraryItem {
            id: format!("bangumi_subject_{}", subject_id),
            item_type: item_type.to_string(),
            title: title.to_string(),
            cover,
            platform: "Bangumi".to_string(),
            metadata,
        });
        added += 1;
    }

    tracing::info!("✓ Loaded {} Bangumi collection items", added);
}

fn append_mal_library_items(library_items: &mut Vec<LibraryItem>, mal_data: &Value) {
    let mut added = 0usize;

    let mut append_list = |list_key: &str, path_kind: &str, item_type: &str| {
        let Some(list) = mal_data.get(list_key).and_then(|v| v.as_array()) else {
            return;
        };
        for entry in list {
            let node = entry.get("node").unwrap_or(entry);
            let subject_id = node.get("id").and_then(|v| v.as_i64());
            let Some(subject_id) = subject_id else {
                continue;
            };
            let title = node
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            let cover = node
                .pointer("/main_picture/large")
                .or_else(|| node.pointer("/main_picture/medium"))
                .and_then(|v| v.as_str())
                .map(proxy_image_url);

            let list_status = entry.get("list_status");
            // Flatten fields used by LibraryGrid (parity with Bangumi `rate` / `progress`)
            let rate = list_status
                .and_then(|s| s.get("score"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let status = list_status
                .and_then(|s| s.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let progress = if path_kind == "anime" {
                list_status
                    .and_then(|s| s.get("num_episodes_watched"))
                    .and_then(|v| v.as_i64())
                    .map(|n| {
                        let total = node
                            .get("num_episodes")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        if total > 0 {
                            format!("{}/{}", n, total)
                        } else if n > 0 {
                            format!("{}", n)
                        } else {
                            String::new()
                        }
                    })
                    .unwrap_or_default()
            } else {
                list_status
                    .and_then(|s| s.get("num_chapters_read"))
                    .and_then(|v| v.as_i64())
                    .map(|chapters| {
                        let volumes = list_status
                            .and_then(|s| s.get("num_volumes_read"))
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        if volumes > 0 {
                            format!("{}/{}", chapters, volumes)
                        } else if chapters > 0 {
                            format!("{}", chapters)
                        } else {
                            String::new()
                        }
                    })
                    .unwrap_or_default()
            };

            let mut metadata = entry.clone();
            if let Some(obj) = metadata.as_object_mut() {
                obj.insert(
                    "url".to_string(),
                    json!(format!(
                        "https://myanimelist.net/{}/{}",
                        path_kind, subject_id
                    )),
                );
                obj.insert("platform".to_string(), json!("MyAnimeList"));
                obj.insert("rate".to_string(), json!(rate));
                if !status.is_empty() {
                    obj.insert("status".to_string(), json!(status));
                }
                if !progress.is_empty() {
                    obj.insert("progress".to_string(), json!(progress));
                    // Book cards also read ep_status/vol_status (Bangumi shape)
                    if path_kind == "manga" {
                        if let Some(chapters) = list_status
                            .and_then(|s| s.get("num_chapters_read"))
                            .and_then(|v| v.as_i64())
                        {
                            obj.insert("ep_status".to_string(), json!(chapters));
                        }
                        if let Some(volumes) = list_status
                            .and_then(|s| s.get("num_volumes_read"))
                            .and_then(|v| v.as_i64())
                        {
                            obj.insert("vol_status".to_string(), json!(volumes));
                        }
                    }
                }
            }

            library_items.push(LibraryItem {
                id: format!("mal_{}_{}", path_kind, subject_id),
                item_type: item_type.to_string(),
                title: title.to_string(),
                cover,
                platform: "MyAnimeList".to_string(),
                metadata,
            });
            added += 1;
        }
    };

    append_list("anime_list", "anime", "anime");
    append_list("manga_list", "manga", "book");

    tracing::info!("✓ Loaded {} MyAnimeList list items", added);
}

/// 获取资料库数据（游戏、视频、音乐）
pub async fn get_library_data(State(db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    let user_id = match site_owner_user_id(&db).await {
        Ok(user_id) => user_id,
        Err(error) => return site_owner_error(error),
    };

    tracing::info!("📚 Fetching library data for user: {}", user_id);

    // 创建元数据服务
    let metadata_service = crate::services::metadata_service::MetadataService::new(db.clone());

    let mut library_items: Vec<LibraryItem> = Vec::new();

    // 1. 优先从数据库获取数据
    match metadata_service.get_all_latest_metadata(user_id).await {
        Ok(db_data) if !db_data.is_empty() => {
            tracing::info!("📊 Loading library data from database");

            // 处理 Steam 游戏数据
            if let Some(steam_data) = db_data.get("steam") {
                if let Some(games) = steam_data.get("games").and_then(|g| g.as_array()) {
                    for game in games {
                        if let (Some(appid), Some(name)) = (
                            game.get("appid").and_then(|a| a.as_i64()),
                            game.get("name").and_then(|n| n.as_str()),
                        ) {
                            library_items.push(LibraryItem {
                                id: format!("steam_game_{}", appid),
                                item_type: "game".to_string(),
                                title: name.to_string(),
                                cover: Some(format!(
                                    "https://cdn.cloudflare.steamstatic.com/steam/apps/{}/header.jpg",
                                    appid
                                )),
                                platform: "Steam".to_string(),
                                metadata: game.clone(),
                            });
                        }
                    }
                    tracing::info!("✓ Loaded {} Steam games", games.len());
                }
            }

            // 处理 Bilibili 视频数据（追番/追剧）
            if let Some(bilibili_data) = db_data.get("bilibili") {
                if let Some(bangumi) = bilibili_data.get("bangumi").and_then(|b| b.as_array()) {
                    tracing::info!("📺 Processing {} bangumi items", bangumi.len());
                    for item in bangumi {
                        tracing::debug!("Bangumi item: {:?}", item);
                        if let (Some(season_id), Some(title), Some(cover)) = (
                            item.get("season_id").and_then(|s| s.as_i64()),
                            item.get("title").and_then(|t| t.as_str()),
                            item.get("cover").and_then(|c| c.as_str()),
                        ) {
                            // 根据season_type判断类型
                            // 1=番剧(动画), 2=电视剧, 3=纪录片, 4=国创, 5=电影
                            let season_type = item
                                .get("season_type")
                                .and_then(|s| s.as_i64())
                                .unwrap_or(1);
                            let item_type = match season_type {
                                1 | 4 => "anime", // 番剧和国创归类为anime
                                2 => "tv_series", // 电视剧
                                3 | 5 => "video", // 纪录片和电影保持为video
                                _ => "anime",     // 默认为anime
                            };

                            // 创建包含链接信息的metadata
                            let mut metadata = item.clone();
                            if let Some(obj) = metadata.as_object_mut() {
                                obj.insert(
                                    "url".to_string(),
                                    json!(format!(
                                        "https://www.bilibili.com/bangumi/play/ss{}",
                                        season_id
                                    )),
                                );
                            }

                            library_items.push(LibraryItem {
                                id: format!("bilibili_bangumi_{}", season_id),
                                item_type: item_type.to_string(),
                                title: title.to_string(),
                                cover: Some(proxy_image_url(cover)),
                                platform: "Bilibili".to_string(),
                                metadata,
                            });
                        }
                    }
                    tracing::info!("✓ Loaded {} Bilibili bangumi", bangumi.len());
                }

                // 处理收藏的视频
                if let Some(favorites) = bilibili_data.get("favorites").and_then(|f| f.as_array()) {
                    tracing::info!("📁 Processing {} favorite folders", favorites.len());
                    for fav_folder in favorites {
                        if let Some(videos) = fav_folder.get("videos").and_then(|v| v.as_array()) {
                            tracing::info!("📹 Processing {} videos in folder", videos.len());
                            for video in videos {
                                if let (Some(bvid), Some(title), Some(cover)) = (
                                    video.get("bvid").and_then(|b| b.as_str()),
                                    video.get("title").and_then(|t| t.as_str()),
                                    video.get("cover").and_then(|c| c.as_str()),
                                ) {
                                    // 创建包含链接信息的metadata
                                    let mut metadata = video.clone();
                                    if let Some(obj) = metadata.as_object_mut() {
                                        obj.insert(
                                            "url".to_string(),
                                            json!(format!(
                                                "https://www.bilibili.com/video/{}",
                                                bvid
                                            )),
                                        );
                                    }

                                    library_items.push(LibraryItem {
                                        id: format!("bilibili_video_{}", bvid),
                                        item_type: "video".to_string(),
                                        title: title.to_string(),
                                        cover: Some(proxy_image_url(cover)),
                                        platform: "Bilibili".to_string(),
                                        metadata,
                                    });
                                }
                            }
                        }
                    }
                    tracing::info!("✓ Loaded {} Bilibili favorite videos", favorites.len());
                }
            }

            // 处理网易云音乐数据（支持从临时文件加载完整数据）
            if let Some(netease_data) = db_data.get("netease") {
                // 直接从 liked_songs 读取完整数据
                let songs_vec: Vec<Value> = netease_data
                    .get("liked_songs")
                    .and_then(|s| s.as_array())
                    .cloned()
                    .unwrap_or_default();

                tracing::info!(
                    "🎵 Processing {} netease songs for library",
                    songs_vec.len()
                );

                // 使用 HashSet 去重，防止分片合并时产生重复歌曲
                let mut seen_song_ids = std::collections::HashSet::new();
                let mut added_count = 0;

                for song in &songs_vec {
                    if let (Some(id), Some(name)) = (
                        song.get("id").and_then(|i| i.as_i64()),
                        song.get("name").and_then(|n| n.as_str()),
                    ) {
                        // 跳过已处理的歌曲ID
                        if !seen_song_ids.insert(id) {
                            continue;
                        }

                        // 提取封面 - 支持多种字段格式，并通过代理
                        let cover = song
                            .get("al")
                            .or_else(|| song.get("album"))
                            .and_then(|al| {
                                al.get("picUrl")
                                    .or_else(|| al.get("pic_url"))
                                    .or_else(|| al.get("cover"))
                            })
                            .and_then(|p| p.as_str())
                            .map(proxy_image_url);

                        // 规范化metadata确保包含所有必要字段
                        let mut normalized_metadata = song.clone();
                        if let Some(obj) = normalized_metadata.as_object_mut() {
                            // 确保有ar字段（艺术家数组）
                            if !obj.contains_key("ar") && !obj.contains_key("artists") {
                                obj.insert("ar".to_string(), json!([]));
                            }
                            // 确保有al字段（专辑信息）
                            if !obj.contains_key("al") && !obj.contains_key("album") {
                                obj.insert("al".to_string(), json!({"name": "未知专辑"}));
                            }
                            // 确保有dt字段（时长毫秒）
                            if !obj.contains_key("dt") && !obj.contains_key("duration") {
                                obj.insert("dt".to_string(), json!(0));
                            }
                        }

                        library_items.push(LibraryItem {
                            id: format!("netease_song_{}", id),
                            item_type: "music".to_string(),
                            title: name.to_string(),
                            cover,
                            platform: "Netease".to_string(),
                            metadata: normalized_metadata,
                        });
                        added_count += 1;
                    }
                }
                tracing::info!(
                    "✓ Loaded {} Netease songs (deduplicated from {})",
                    added_count,
                    songs_vec.len()
                );
            }

            if let Some(bangumi_data) = db_data.get("bangumi") {
                append_bangumi_library_items(&mut library_items, bangumi_data);
            }

            if let Some(mal_data) = db_data.get("mal") {
                append_mal_library_items(&mut library_items, mal_data);
            }
        }
        Ok(_) => {
            tracing::info!("📊 Database is empty, falling back to cache");
        }
        Err(e) => {
            tracing::warn!(
                "Failed to fetch from database: {}, falling back to cache",
                e
            );
        }
    }

    // 2. 如果数据库没有数据，从缓存获取
    if library_items.is_empty() {
        if let Some(cache) = load_platform_data_cache() {
            tracing::info!("📦 Loading library data from cache file");
            let data = &cache.data;

            // 处理 Steam 游戏
            if let Some(games) = data
                .get("steam")
                .and_then(|s| s.get("games"))
                .and_then(|g| g.as_array())
            {
                for game in games {
                    if let (Some(appid), Some(name)) = (
                        game.get("appid").and_then(|a| a.as_i64()),
                        game.get("name").and_then(|n| n.as_str()),
                    ) {
                        library_items.push(LibraryItem {
                            id: format!("steam_game_{}", appid),
                            item_type: "game".to_string(),
                            title: name.to_string(),
                            cover: Some(format!(
                                "https://cdn.cloudflare.steamstatic.com/steam/apps/{}/header.jpg",
                                appid
                            )),
                            platform: "Steam".to_string(),
                            metadata: game.clone(),
                        });
                    }
                }
            }

            // 处理 Bilibili 番剧
            if let Some(bangumi) = data
                .get("bilibili")
                .and_then(|b| b.get("bangumi"))
                .and_then(|b| b.as_array())
            {
                for item in bangumi {
                    if let (Some(season_id), Some(title), Some(cover)) = (
                        item.get("season_id").and_then(|s| s.as_i64()),
                        item.get("title").and_then(|t| t.as_str()),
                        item.get("cover").and_then(|c| c.as_str()),
                    ) {
                        // 根据season_type判断类型
                        let season_type = item
                            .get("season_type")
                            .and_then(|s| s.as_i64())
                            .unwrap_or(1);
                        let item_type = match season_type {
                            1 | 4 => "anime",
                            2 => "tv_series",
                            3 | 5 => "video",
                            _ => "anime",
                        };

                        // 创建包含链接信息的metadata
                        let mut metadata = item.clone();
                        if let Some(obj) = metadata.as_object_mut() {
                            obj.insert(
                                "url".to_string(),
                                json!(format!(
                                    "https://www.bilibili.com/bangumi/play/ss{}",
                                    season_id
                                )),
                            );
                        }

                        library_items.push(LibraryItem {
                            id: format!("bilibili_bangumi_{}", season_id),
                            item_type: item_type.to_string(),
                            title: title.to_string(),
                            cover: Some(proxy_image_url(cover)),
                            platform: "Bilibili".to_string(),
                            metadata,
                        });
                    }
                }
            }

            // 处理 Bilibili 收藏
            if let Some(favorites) = data
                .get("bilibili")
                .and_then(|b| b.get("favorites"))
                .and_then(|f| f.as_array())
            {
                for fav_folder in favorites {
                    if let Some(videos) = fav_folder.get("videos").and_then(|v| v.as_array()) {
                        for video in videos {
                            if let (Some(bvid), Some(title), Some(cover)) = (
                                video.get("bvid").and_then(|b| b.as_str()),
                                video.get("title").and_then(|t| t.as_str()),
                                video.get("cover").and_then(|c| c.as_str()),
                            ) {
                                // 创建包含链接信息的metadata
                                let mut metadata = video.clone();
                                if let Some(obj) = metadata.as_object_mut() {
                                    obj.insert(
                                        "url".to_string(),
                                        json!(format!("https://www.bilibili.com/video/{}", bvid)),
                                    );
                                }

                                library_items.push(LibraryItem {
                                    id: format!("bilibili_video_{}", bvid),
                                    item_type: "video".to_string(),
                                    title: title.to_string(),
                                    cover: Some(proxy_image_url(cover)),
                                    platform: "Bilibili".to_string(),
                                    metadata,
                                });
                            }
                        }
                    }
                }
            }

            // 处理网易云音乐
            if let Some(songs) = data
                .get("netease")
                .and_then(|n| n.get("liked_songs"))
                .and_then(|s| s.as_array())
            {
                // 使用 HashSet 去重
                let mut seen_song_ids = std::collections::HashSet::new();

                for song in songs {
                    if let (Some(id), Some(name)) = (
                        song.get("id").and_then(|i| i.as_i64()),
                        song.get("name").and_then(|n| n.as_str()),
                    ) {
                        // 跳过已处理的歌曲ID
                        if !seen_song_ids.insert(id) {
                            continue;
                        }

                        // 提取封面 - 支持多种字段格式，并通过代理
                        let cover = song
                            .get("al")
                            .or_else(|| song.get("album"))
                            .and_then(|al| {
                                al.get("picUrl")
                                    .or_else(|| al.get("pic_url"))
                                    .or_else(|| al.get("cover"))
                            })
                            .and_then(|p| p.as_str())
                            .map(proxy_image_url);

                        // 规范化metadata确保包含所有必要字段
                        let mut normalized_metadata = song.clone();
                        if let Some(obj) = normalized_metadata.as_object_mut() {
                            // 确保有ar字段（艺术家数组）
                            if !obj.contains_key("ar") && !obj.contains_key("artists") {
                                obj.insert("ar".to_string(), json!([]));
                            }
                            // 确保有al字段（专辑信息）
                            if !obj.contains_key("al") && !obj.contains_key("album") {
                                obj.insert("al".to_string(), json!({"name": "未知专辑"}));
                            }
                            // 确保有dt字段（时长毫秒）
                            if !obj.contains_key("dt") && !obj.contains_key("duration") {
                                obj.insert("dt".to_string(), json!(0));
                            }
                        }

                        library_items.push(LibraryItem {
                            id: format!("netease_song_{}", id),
                            item_type: "music".to_string(),
                            title: name.to_string(),
                            cover,
                            platform: "Netease".to_string(),
                            metadata: normalized_metadata,
                        });
                    }
                }
            }

            if let Some(bangumi_data) = data.get("bangumi") {
                append_bangumi_library_items(&mut library_items, bangumi_data);
            }

            if let Some(mal_data) = data.get("mal") {
                append_mal_library_items(&mut library_items, mal_data);
            }
        }
    }

    if library_items.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "success": false,
                "message": "No library data found. Please fetch platform data first."
            })),
        );
    }

    let preferences = load_library_source_preferences(&db).await;
    let raw_total = library_items.len();
    let available_sources = collect_library_source_options(&library_items);
    let library_items = apply_library_source_preferences(library_items, &preferences);

    tracing::info!(
        "✅ Loaded {} library items in total ({} raw before source filtering)",
        library_items.len(),
        raw_total
    );

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "items": library_items,
            "total": library_items.len(),
            "raw_total": raw_total,
            "preferences": preferences,
            "available_sources": available_sources
        })),
    )
}

/// 批量获取用户信息 - 优化性能，减少前端API调用次数
///
/// 这个端点将多个独立的API调用合并为一个请求，显著提升前端加载速度
#[derive(Debug, Serialize)]
pub struct BatchUserInfoResponse {
    pub user_info: Option<Value>,
    pub config: Option<Value>,
}

pub async fn get_batch_user_info(
    State(db): State<DatabaseConnection>,
) -> (StatusCode, Json<Value>) {
    tracing::info!("📦 Fetching batch site-owner information");

    let mut response = BatchUserInfoResponse {
        user_info: None,
        config: None,
    };

    // 1. 获取用户基本信息
    let (status, json) = get_user_info(State(db.clone())).await;
    if status == StatusCode::OK {
        response.user_info = Some(json.0);
    } else {
        response.user_info = Some(json!({
            "success": false,
            "message": "Failed to fetch user info"
        }));
    }

    // 2. 获取配置信息 - 仅返回平台启用状态，不返回敏感数据
    let (config_status, config_json) = crate::api::config::get_config(State(db.clone())).await;
    if config_status == StatusCode::OK {
        let full_config = config_json.0;
        // 只提取平台启用状态和图标，移除所有配置字段
        if let Some(platforms) = full_config.get("platforms").and_then(|p| p.as_array()) {
            let safe_platforms: Vec<_> = platforms
                .iter()
                .map(|platform| {
                    json!({
                        "name": platform.get("name"),
                        "enabled": platform.get("enabled"),
                        "has_token": platform.get("has_token"),
                        "icon": platform.get("icon"),
                        "description": platform.get("description"),
                        // 移除 config_fields - 不返回任何配置值
                    })
                })
                .collect();

            response.config = Some(json!({
                "platforms": safe_platforms,
                // 不返回其他配置部分（ai_config, ui_config 等）
            }));
        } else {
            response.config = Some(json!({
                "success": false,
                "message": "Failed to parse config"
            }));
        }
    } else {
        response.config = Some(json!({
            "success": false,
            "message": "Failed to fetch config"
        }));
    }

    tracing::info!("✓ Batch user info fetched successfully (sanitized)");

    (
        StatusCode::OK,
        Json(serde_json::to_value(&response).unwrap_or_else(|_| {
            json!({
                "success": false,
                "message": "Failed to serialize response"
            })
        })),
    )
}

/// 获取最近活动记录
#[derive(Deserialize)]
pub struct ActivityQuery {
    pub limit: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ActivityItem {
    pub platform_name: String,
    pub event_type: String,
    pub title: String,
    pub changes: Value,
    pub change_count: i32,
    pub change_date: String,
    pub legacy: bool,
}

struct LegacyActivityGroup {
    platform_name: String,
    change_count: i32,
    change_date: String,
}

pub async fn get_recent_activities(
    Query(params): Query<ActivityQuery>,
    State(db): State<DatabaseConnection>,
) -> (StatusCode, Json<Value>) {
    use crate::models::entities::{activity_events, metadata_history};
    use crate::services::activity_event_service::{platform_label, public_activity_changes};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

    let limit = params.limit.unwrap_or(10).clamp(1, 50);
    let user_id = match site_owner_user_id(&db).await {
        Ok(user_id) => user_id,
        Err(error) => return site_owner_error(error),
    };

    // Legacy rows are collapsed by platform/day, so fetch extra audit records
    // before applying the user-facing limit.
    let audit_limit = (limit * 10).min(500);

    match metadata_history::Entity::find()
        .filter(metadata_history::Column::UserId.eq(user_id))
        .order_by_desc(metadata_history::Column::ChangeDate)
        .limit(audit_limit)
        .all(&db)
        .await
    {
        Ok(records) => {
            let history_ids: Vec<i32> = records.iter().map(|record| record.id).collect();
            let normalized = if history_ids.is_empty() {
                Vec::new()
            } else {
                match activity_events::Entity::find()
                    .filter(activity_events::Column::UserId.eq(user_id))
                    .filter(activity_events::Column::MetadataHistoryId.is_in(history_ids))
                    .all(&db)
                    .await
                {
                    Ok(events) => events,
                    Err(error) => {
                        // During a rolling deploy an old replica may serve before
                        // activity_events (001 + schema_check) is visible. Legacy summaries remain usable.
                        tracing::warn!("Failed to load normalized activity events: {}", error);
                        Vec::new()
                    }
                }
            };
            let mut normalized_by_history: HashMap<i32, activity_events::Model> = normalized
                .into_iter()
                .map(|event| (event.metadata_history_id, event))
                .collect();

            let mut activities = Vec::new();
            let mut legacy_groups: HashMap<String, LegacyActivityGroup> = HashMap::new();

            for record in records {
                if let Some(event) = normalized_by_history.remove(&record.id) {
                    if event.event_type == "suppressed" {
                        continue;
                    }
                    activities.push(ActivityItem {
                        platform_name: event.platform_name,
                        event_type: event.event_type,
                        title: event.title,
                        changes: public_activity_changes(&event.changes),
                        change_count: event.change_count,
                        change_date: event.occurred_at.to_string(),
                        legacy: false,
                    });
                    continue;
                }

                let day = record.change_date.date().to_string();
                let key = format!("{}:{}", record.platform_name, day);
                let field_count = record
                    .changed_fields
                    .as_array()
                    .map(|fields| i32::try_from(fields.len()).unwrap_or(i32::MAX))
                    .unwrap_or(1);
                legacy_groups
                    .entry(key)
                    .and_modify(|group| {
                        group.change_count = group.change_count.saturating_add(field_count);
                        if record.change_date.to_string() > group.change_date {
                            group.change_date = record.change_date.to_string();
                        }
                    })
                    .or_insert_with(|| LegacyActivityGroup {
                        platform_name: record.platform_name,
                        change_count: field_count,
                        change_date: record.change_date.to_string(),
                    });
            }

            activities.extend(legacy_groups.into_values().map(|group| ActivityItem {
                title: platform_label(&group.platform_name).to_string(),
                platform_name: group.platform_name,
                event_type: "legacy_updated".to_string(),
                changes: json!([{
                    "kind": "legacy_summary",
                    "metric": "data_changes",
                    "new": group.change_count
                }]),
                change_count: group.change_count,
                change_date: group.change_date,
                legacy: true,
            }));
            activities.sort_by(|a, b| b.change_date.cmp(&a.change_date));
            activities.truncate(limit as usize);

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "activities": activities,
                    "count": activities.len()
                })),
            )
        }
        Err(e) => {
            tracing::error!("Failed to fetch activities: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Failed to fetch activities: {}", e)
                })),
            )
        }
    }
}
