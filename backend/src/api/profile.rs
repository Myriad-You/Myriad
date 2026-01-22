use crate::services::fetcher::PlatformFetcher;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use sea_orm::DatabaseConnection;
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PersonalReport {
    pub cards: Vec<ReportCard>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_cards: Option<Vec<ReportCard>>,
    pub generated_at: String,
    pub expires_at: String,
    pub selected_topics: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReportCard {
    pub topic_id: u8,
    pub title: String,
    pub category: String,
    pub icon: String,
    pub color: String,
    pub content: CardContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<String>, // 用于区分不同批次生成的卡片
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CardContent {
    pub summary: String,
    pub details: Vec<String>,
    pub highlight: Option<String>,
    pub tags: Vec<String>,
}

// 持久化缓存（保存到磁盘，重启后依然有效）
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const CACHE_FILE_PATH: &str = "./cache/reports.json";
const MAX_CACHE_ENTRIES: usize = 20;

type ReportCacheEntry = (PersonalReport, DateTime<Utc>);
type ReportCache = Arc<Mutex<HashMap<String, ReportCacheEntry>>>;

lazy_static::lazy_static! {
    static ref REPORT_CACHE: ReportCache = {
        let cache = load_cache_from_disk().unwrap_or_else(|_| HashMap::new());
        Arc::new(Mutex::new(cache))
    };
}

/// 从磁盘加载缓存
fn load_cache_from_disk() -> Result<HashMap<String, ReportCacheEntry>, Box<dyn std::error::Error>> {
    let path = PathBuf::from(CACHE_FILE_PATH);
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let content = fs::read_to_string(path)?;
    let cache: HashMap<String, ReportCacheEntry> = serde_json::from_str(&content)?;

    // 清理过期缓存（7天前）
    let now = Utc::now();
    let valid_cache: HashMap<String, ReportCacheEntry> = cache
        .into_iter()
        .filter(|(_, (_, created_at))| now - *created_at < Duration::days(7))
        .collect();

    tracing::info!("✓ Loaded {} cached reports from disk", valid_cache.len());
    Ok(valid_cache)
}

/// 保存缓存到磁盘
fn save_cache_to_disk() -> Result<(), Box<dyn std::error::Error>> {
    let cache = REPORT_CACHE.lock().unwrap();
    let path = PathBuf::from(CACHE_FILE_PATH);

    tracing::info!("💾 Saving cache to disk: {}", path.display());
    tracing::info!("   Total entries: {}", cache.len());

    // 确保目录存在
    if let Some(parent) = path.parent() {
        tracing::info!("   Creating directory: {}", parent.display());
        fs::create_dir_all(parent)?;
    }

    let content = serde_json::to_string_pretty(&*cache)?;
    tracing::info!("   Writing {} bytes", content.len());
    fs::write(&path, content)?;
    tracing::info!("✅ Cache saved successfully");
    Ok(())
}

/// 检查缓存是否有效（7天内）- 返回最新的报告，并合并所有历史卡片
fn get_cached_report(user_id: i32, force_refresh: bool) -> Option<PersonalReport> {
    if force_refresh {
        tracing::info!("⚡ Force refresh requested, skipping cache");
        return None;
    }

    let cache = REPORT_CACHE.lock().unwrap();
    let now = Utc::now();
    let user_id_str = user_id.to_string();

    // 查找该用户的所有有效缓存
    let mut valid_reports: Vec<(String, PersonalReport, DateTime<Utc>)> = cache
        .iter()
        .filter(|(key, (_, created_at))| {
            key.starts_with(&user_id_str) && now - *created_at < Duration::days(7)
        })
        .map(|(key, (report, created_at))| (key.clone(), report.clone(), *created_at))
        .collect();

    // 按时间排序，最新的在前
    valid_reports.sort_by(|a, b| b.2.cmp(&a.2));

    if let Some((_, latest_report, _)) = valid_reports.first() {
        tracing::info!(
            "✓ Found {} valid cached reports for {}",
            valid_reports.len(),
            user_id
        );

        // 合并所有历史报告的卡片（使用组合键避免覆盖）
        let mut all_cards_map: std::collections::HashMap<(String, u8), ReportCard> =
            std::collections::HashMap::new();

        for (_, report, created_at) in valid_reports.iter() {
            let timestamp = created_at.to_rfc3339();
            for card in &report.cards {
                // 使用 (生成时间, topic_id) 作为组合key，确保不同批次的卡片不会被覆盖
                let mut card_with_time = card.clone();
                card_with_time.generated_at = Some(timestamp.clone());

                all_cards_map
                    .entry((timestamp.clone(), card.topic_id))
                    .or_insert_with(|| card_with_time);
            }
        }

        // 转换为 Vec 并按时间倒序排序（最新的在前）
        let mut all_cards: Vec<ReportCard> = all_cards_map.into_values().collect();
        all_cards.sort_by(|a, b| {
            // 按生成时间倒序，时间相同则按 topic_id 排序
            match (b.generated_at.as_ref(), a.generated_at.as_ref()) {
                (Some(t1), Some(t2)) => t1.cmp(t2).then(a.topic_id.cmp(&b.topic_id)),
                _ => a.topic_id.cmp(&b.topic_id),
            }
        });

        tracing::info!(
            "📦 Merged cards: {} unique cards from {} reports",
            all_cards.len(),
            valid_reports.len()
        );

        // 创建新的报告，包含最新报告的 cards 和所有历史的 all_cards
        let report_with_all = PersonalReport {
            cards: latest_report.cards.clone(),
            all_cards: Some(all_cards),
            generated_at: latest_report.generated_at.clone(),
            expires_at: latest_report.expires_at.clone(),
            selected_topics: latest_report.selected_topics.clone(),
        };

        return Some(report_with_all);
    }

    None
}

/// 保存报告到缓存（限制最多20个）- 使用唯一ID
fn cache_report(user_id: &str, report: PersonalReport) {
    let mut cache = REPORT_CACHE.lock().unwrap();

    // 使用时间戳生成唯一的缓存key
    let timestamp = Utc::now().timestamp();
    let cache_key = format!("{}_{}", user_id, timestamp);

    // 如果缓存已满，删除最旧的条目
    if cache.len() >= MAX_CACHE_ENTRIES {
        if let Some(oldest_key) = cache
            .iter()
            .min_by_key(|(_, (_, created_at))| created_at)
            .map(|(k, _)| k.clone())
        {
            cache.remove(&oldest_key);
            tracing::info!("🗑️ Cache full, removed oldest entry: {}", oldest_key);
        }
    }

    cache.insert(cache_key.clone(), (report, Utc::now()));
    let total_cache = cache.len();
    let user_cache_count = cache.iter().filter(|(k, _)| k.starts_with(user_id)).count();

    tracing::info!("💾 Cached report with key: {}", cache_key);
    tracing::info!(
        "📊 Cache stats - Total: {}, User: {}",
        total_cache,
        user_cache_count
    );

    // 列出所有缓存 key
    tracing::info!("📋 All cache keys:");
    for key in cache.keys() {
        tracing::info!("   - {}", key);
    }

    drop(cache); // 释放锁

    // 保存到磁盘
    tracing::info!("💿 Attempting to save cache to disk...");
    match save_cache_to_disk() {
        Ok(_) => tracing::info!("✅ Cache successfully saved to disk"),
        Err(e) => tracing::error!("❌ Failed to save cache to disk: {}", e),
    }
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
    let user_id = 1; // TODO: 从认证中获取真实用户ID

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

    // 创建元数据服务
    let metadata_service = crate::services::metadata_service::MetadataService::new(db.clone());

    // 获取GitHub数据（包含仓库信息）
    if should_fetch("github") {
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
    if should_fetch("bilibili") {
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
    if should_fetch("steam") {
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
    if should_fetch("netease") {
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

    // 数据清洗：移除无用信息，保留核心5W1H信息
    clean_platform_data(&mut all_data);

    // 更新智能过滤缓存
    if let Err(e) = crate::services::smart_filter::SmartFilter::process_and_save_all(&all_data) {
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

    tracing::info!("✓ Platform data cleaned (removed unnecessary fields)");
}

/// 为 AI 分析筛选数据：应用 5W 原则（Who, What, When, Where, Why）
/// 只保留最关键的信息，减少 token 消耗并提升 AI 分析质量
pub fn filter_data_for_ai(data: &Value) -> Value {
    let mut filtered = json!({});

    // ===== Steam 数据筛选 =====
    if let Some(steam) = data.get("steam") {
        let mut steam_filtered = json!({});

        // Who: 用户信息（保留核心身份）
        if let Some(user) = steam.get("user") {
            steam_filtered["user"] = json!({
                "personaname": user.get("personaname"),      // 谁
                "timecreated": user.get("timecreated"),      // 何时注册
            });
        }

        // What: 游戏列表（只保留关键数据）
        if let Some(games) = steam.get("games").and_then(|g| g.as_array()) {
            let filtered_games: Vec<Value> = games
                .iter()
                .map(|game| {
                    json!({
                        "name": game.get("name"),                          // 什么游戏
                        "playtime_forever": game.get("playtime_forever"),  // 玩了多久
                        "playtime_2weeks": game.get("playtime_2weeks"),    // 最近活跃度
                    })
                })
                .collect();
            steam_filtered["games"] = json!(filtered_games);
            steam_filtered["total_games"] = json!(filtered_games.len());
        }

        filtered["steam"] = steam_filtered;
    }

    // ===== Bilibili 数据筛选 =====
    if let Some(bilibili) = data.get("bilibili") {
        let mut bilibili_filtered = json!({});

        // What + When: 追番/追剧
        if let Some(bangumi) = bilibili.get("bangumi").and_then(|b| b.as_array()) {
            let filtered_bangumi: Vec<Value> = bangumi
                .iter()
                .map(|item| {
                    json!({
                        "title": item.get("title"),              // 什么番剧
                        "progress": item.get("progress"),        // 看到哪里
                        "new_ep": item.get("new_ep"),            // 更新状态
                        "badge": item.get("badge"),              // 类型标签
                    })
                })
                .collect();
            bilibili_filtered["bangumi"] = json!(filtered_bangumi);
            bilibili_filtered["total_bangumi"] = json!(filtered_bangumi.len());
        }

        // What: 收藏的视频
        if let Some(favorites) = bilibili.get("favorites").and_then(|f| f.as_array()) {
            let mut total_videos = 0;
            let filtered_favorites: Vec<Value> = favorites
                .iter()
                .filter_map(|folder| {
                    if let Some(videos) = folder.get("videos").and_then(|v| v.as_array()) {
                        total_videos += videos.len();
                        let filtered_videos: Vec<Value> = videos
                            .iter()
                            .map(|video| {
                                json!({
                                    "title": video.get("title"),        // 什么视频
                                    "duration": video.get("duration"),  // 时长
                                })
                            })
                            .collect();
                        Some(json!({
                            "title": folder.get("title"),      // 收藏夹名称
                            "videos": filtered_videos,
                            "video_count": videos.len(),
                        }))
                    } else {
                        None
                    }
                })
                .collect();
            bilibili_filtered["favorites"] = json!(filtered_favorites);
            bilibili_filtered["total_videos"] = json!(total_videos);
        }

        filtered["bilibili"] = bilibili_filtered;
    }

    // ===== 网易云音乐数据筛选 =====
    if let Some(netease) = data.get("netease") {
        let mut netease_filtered = json!({});

        // What: 喜欢的歌曲
        if let Some(songs) = netease.get("liked_songs").and_then(|s| s.as_array()) {
            let filtered_songs: Vec<Value> = songs
                .iter()
                .map(|song| {
                    json!({
                        "name": song.get("name"),                // 什么歌
                        "ar": song.get("ar"),                    // 谁唱的（艺术家）
                        "al": song.get("al").and_then(|al| al.get("name")), // 专辑
                    })
                })
                .collect();
            netease_filtered["liked_songs"] = json!(filtered_songs);
            netease_filtered["total_songs"] = json!(filtered_songs.len());
        }

        filtered["netease"] = netease_filtered;
    }

    // ===== GitHub 数据筛选 =====
    if let Some(github) = data.get("github") {
        let mut github_filtered = json!({});

        // Who: 用户信息
        if let Some(user) = github.get("user") {
            github_filtered["user"] = json!({
                "login": user.get("login"),              // 谁
                "name": user.get("name"),
                "bio": user.get("bio"),                  // 为什么（个人简介）
                "location": user.get("location"),        // 哪里
                "created_at": user.get("created_at"),    // 何时
            });
        }

        // What: 仓库列表
        if let Some(repos) = github.get("repos").and_then(|r| r.as_array()) {
            let filtered_repos: Vec<Value> = repos
                .iter()
                .map(|repo| {
                    json!({
                        "name": repo.get("name"),                    // 什么项目
                        "description": repo.get("description"),      // 为什么（项目描述）
                        "language": repo.get("language"),            // 什么语言
                        "stargazers_count": repo.get("stargazers_count"), // 影响力
                        "topics": repo.get("topics"),                // 主题标签
                    })
                })
                .collect();
            github_filtered["repos"] = json!(filtered_repos);
            github_filtered["total_repos"] = json!(filtered_repos.len());
        }

        filtered["github"] = github_filtered;
    }

    // ===== Pixiv 数据筛选 =====
    if let Some(pixiv) = data.get("pixiv") {
        let mut pixiv_filtered = json!({});

        // Who: 用户信息
        if let Some(user) = pixiv.get("user") {
            pixiv_filtered["user"] = json!({
                "name": user.get("name"),            // 谁
                "account": user.get("account"),
            });
        }

        // What: 收藏作品
        if let Some(bookmarks) = pixiv.get("bookmarks").and_then(|b| b.as_array()) {
            pixiv_filtered["total_bookmarks"] = json!(bookmarks.len());
            // 只保留数量统计，不传递具体作品详情（减少token）
        }

        filtered["pixiv"] = pixiv_filtered;
    }

    tracing::debug!(
        "Original data size: ~{} bytes",
        serde_json::to_string(data).unwrap_or_default().len()
    );
    tracing::debug!(
        "Filtered data size: ~{} bytes",
        serde_json::to_string(&filtered).unwrap_or_default().len()
    );

    filtered
}

/// 生成个人报告
#[derive(Deserialize)]
pub struct GenerateReportQuery {
    #[serde(default)]
    force: bool,
}

pub async fn generate_report(
    State(_db): State<DatabaseConnection>,
    Query(query): Query<GenerateReportQuery>,
    Json(profile_data): Json<Value>,
) -> (StatusCode, Json<Value>) {
    tracing::info!("Generating personal report (force={})...", query.force);

    let user_id = 1; // TODO: 从认证中获取真实用户ID

    // 检查缓存
    if let Some(cached_report) = get_cached_report(user_id, query.force) {
        tracing::info!("✓ Returning cached report");
        return (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "report": cached_report,
                "from_cache": true
            })),
        );
    }

    // 获取AI配置 - 从数据库读取（已在启动时从环境变量迁移）
    let dynamic_config = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();

    let provider = crate::services::analyzer::AiProvider::from_str(&dynamic_config.ai_provider);

    let (api_key, model, base_url) = match provider {
        crate::services::analyzer::AiProvider::Gemini => match &dynamic_config.gemini_api_key {
            Some(key) if !key.is_empty() => {
                (key.clone(), dynamic_config.gemini_model.clone(), None)
            }
            _ => {
                tracing::error!("Gemini API key not configured in database");
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "message": "Gemini API key not configured. Please set it in the configuration page."
                    })),
                );
            }
        },
        crate::services::analyzer::AiProvider::OpenAI => match &dynamic_config.openai_api_key {
            Some(key) if !key.is_empty() => (
                key.clone(),
                dynamic_config.openai_model.clone(),
                Some(dynamic_config.openai_base_url.clone()),
            ),
            _ => {
                tracing::error!("OpenAI API key not configured in database");
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "message": "OpenAI API key not configured. Please set it in the configuration page."
                    })),
                );
            }
        },
    };

    let analyzer =
        crate::services::analyzer::AiAnalyzer::new(provider, api_key, model, base_url).await;

    // 获取话题风格配置
    let topic_style = dynamic_config.topic_style.clone();
    let style_instruction = get_style_instruction(&topic_style);

    tracing::info!("Using topic style: {}", topic_style);

    // 🔹 对数据进行5W筛选：只保留最关键的信息供AI分析
    let filtered_data = filter_data_for_ai(&profile_data);
    tracing::info!("✓ Data filtered for AI analysis (5W principle applied)");

    // 第一步：让AI生成6个维度话题
    let topics_prompt = format!(
        r#"你是一位专业的数据分析师。请基于用户的真实数据，为他们设计6个不同维度的分析话题。

用户数据：
{}

风格偏好：{}

要求：
1. **必须生成恰好6个话题**
2. **话题必须基于实际数据**：确保用户数据中有足够信息支持这个话题
3. **话题要有创意和深度**：避免千篇一律，要能引发思考
4. **覆盖不同维度**：6个话题应该从不同角度分析用户
5. **标题简洁有趣**：每个标题控制在15字以内

返回JSON格式（只返回JSON，不要其他内容）：
{{
  "topics": [
    {{
      "id": 1,
      "title": "话题标题",
      "category": "类别名称",
      "icon": "emoji图标",
      "color": "from-blue-400 to-cyan-400",
      "analysis_focus": "这个话题要分析什么（给后续分析用的指引）"
    }},
    // ... 共6个话题
  ]
}}

可用的渐变色值：
- from-blue-400 to-cyan-400
- from-purple-400 to-pink-400
- from-green-400 to-teal-400
- from-orange-400 to-red-400
- from-pink-400 to-rose-400
- from-yellow-400 to-amber-400
- from-indigo-400 to-purple-400
- from-emerald-400 to-green-400

图标示例：🌈 🎮 🔬 🗺️ 🎭 🍜 🌌 🔮 🎪 📊 🎁 ✨ 🎨 💡 🚀 🎯 🌟"#,
        serde_json::to_string_pretty(&filtered_data).unwrap_or_else(|_| "{}".to_string()),
        style_instruction
    );

    // 调用AI生成话题
    let topics_json = match analyzer
        .analyze_profile(&json!({"prompt": topics_prompt}))
        .await
    {
        Ok(response) => {
            let cleaned = response
                .trim()
                .trim_start_matches("```json")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim();
            cleaned.to_string()
        }
        Err(e) => {
            tracing::error!("Failed to generate topics: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Failed to generate topics: {}", e)
                })),
            );
        }
    };

    // 解析生成的话题
    #[derive(Debug, Deserialize)]
    struct GeneratedTopic {
        id: u8,
        title: String,
        category: String,
        icon: String,
        color: String,
        analysis_focus: String,
    }

    #[derive(Debug, Deserialize)]
    struct TopicsResponse {
        topics: Vec<GeneratedTopic>,
    }

    let generated_topics: TopicsResponse = match serde_json::from_str(&topics_json) {
        Ok(topics) => topics,
        Err(e) => {
            tracing::error!(
                "Failed to parse generated topics: {}. Response: {}",
                e,
                topics_json
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Failed to parse AI response: {}", e)
                })),
            );
        }
    };

    if generated_topics.topics.len() != 6 {
        tracing::error!(
            "AI generated {} topics instead of 6",
            generated_topics.topics.len()
        );
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": format!("AI generated {} topics instead of 6", generated_topics.topics.len())
            })),
        );
    }

    tracing::info!(
        "Generated topics: {:?}",
        generated_topics
            .topics
            .iter()
            .map(|t| &t.title)
            .collect::<Vec<_>>()
    );

    // 第二步：为每个话题生成详细内容
    let mut cards = Vec::new();
    let topic_ids: Vec<u8> = generated_topics.topics.iter().map(|t| t.id).collect();

    for (index, topic) in generated_topics.topics.iter().enumerate() {
        let content_prompt = format!(
            r#"你是一位专业的数据分析师。请基于用户的真实数据进行深度分析。

话题：{}
分析重点：{}

用户数据：
{}

要求：
1. **必须基于实际数据**：所有结论必须能从用户数据中找到证据支撑
2. **具体量化**：使用具体数字、时间、频率等可量化信息
3. **避免空洞夸奖**：不要使用"你很棒"、"继续加油"等无意义话语
4. **真实关联**：确保分析内容与用户实际行为强相关
5. **简洁专业**：语言简练，直击要点

返回JSON格式（只返回JSON，不要其他内容）：
{{
  "summary": "基于数据的核心发现（15字内，必须包含具体信息）",
  "details": ["数据点1（含具体数值/事实）", "数据点2（含具体数值/事实）", "数据点3（含具体数值/事实）"],
  "highlight": "最值得关注的数据趋势或异常（可选，需有数据支撑）",
  "tags": ["数据特征1", "数据特征2", "数据特征3"]
}}

示例（好的回答）：
- summary: "近30天提交47次，集中在深夜"
- details: ["最活跃时段：23:00-01:00", "周末贡献占比62%", "主要语言：Python 73%"]
- tags: ["夜猫子程序员", "周末战士", "Python专家"]

示例（避免的回答）：
- summary: "你是一个很努力的开发者" ❌
- details: ["你很有天赋", "继续保持", "未来可期"] ❌
- tags: ["优秀", "努力", "加油"] ❌"#,
            topic.title,
            topic.analysis_focus,
            serde_json::to_string_pretty(&filtered_data).unwrap_or_else(|_| "{}".to_string())
        );

        match analyzer
            .analyze_profile(&json!({"prompt": content_prompt}))
            .await
        {
            Ok(analysis) => {
                // 清理AI返回的文本
                let cleaned = analysis
                    .trim()
                    .trim_start_matches("```json")
                    .trim_start_matches("```")
                    .trim_end_matches("```")
                    .trim();

                // 解析JSON
                if let Ok(content) = serde_json::from_str::<CardContent>(cleaned) {
                    cards.push(ReportCard {
                        topic_id: topic.id,
                        title: topic.title.clone(),
                        category: topic.category.clone(),
                        icon: topic.icon.clone(),
                        color: topic.color.clone(),
                        content,
                        generated_at: None, // 新生成的卡片暂不设置时间
                    });

                    tracing::info!("✓ Generated card {}/6: {}", index + 1, topic.title);
                } else {
                    tracing::warn!("Failed to parse AI response for topic: {}", topic.title);
                }
            }
            Err(e) => {
                tracing::error!("AI analysis failed for topic {}: {}", topic.title, e);
            }
        }
    }

    if cards.is_empty() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": "Failed to generate any cards"
            })),
        );
    }

    // 创建报告
    let now = Utc::now();
    let expires_at = now + Duration::days(7);

    let report = PersonalReport {
        cards,
        all_cards: None, // 新生成的报告不包含历史卡片
        generated_at: now.to_rfc3339(),
        expires_at: expires_at.to_rfc3339(),
        selected_topics: topic_ids,
    };

    // 缓存报告
    cache_report(&user_id.to_string(), report.clone());

    // 获取合并后的报告（包含历史all_cards）
    let final_report = get_cached_report(user_id, false).unwrap_or(report);

    tracing::info!(
        "✓ Personal report generated successfully with {} cards (all_cards: {})",
        final_report.cards.len(),
        final_report
            .all_cards
            .as_ref()
            .map(|ac| ac.len())
            .unwrap_or(0)
    );

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "report": final_report,
            "from_cache": false
        })),
    )
}

/// 根据话题风格获取AI生成指引
fn get_style_instruction(style: &str) -> String {
    // 检查是否是自定义风格
    if style.starts_with("custom:") {
        let custom_style = style.strip_prefix("custom:").unwrap_or("").trim();
        if !custom_style.is_empty() {
            return format!("风格：自定义 - {}", custom_style);
        }
        // 如果自定义内容为空，使用默认平衡风格
        return "风格：平衡多元、兼具深度与趣味。既有数据支撑，又有创意表达。".to_string();
    }

    match style {
        "playful" => {
            "风格：轻松活泼、趣味十足。使用游戏化、拟人化、隐喻等创意手法。话题要像玩游戏一样有趣，让用户会心一笑。
            示例：你是哪种天气？你的背包里有什么？如果你是一道菜".to_string()
        },
        "professional" => {
            "风格：专业严谨、数据驱动。使用量化分析、对比研究、趋势预测等专业方法。话题要有深度和洞察力。
            示例：技能矩阵分析、成长曲线对比、效率分布图、时间投资回报率".to_string()
        },
        "artistic" => {
            "风格：文艺感性、富有诗意。使用隐喻、意象、哲学思考等艺术手法。话题要引发深层思考和情感共鸣。
            示例：你的灵魂色彩、内心的生态系统、时间的河流、精神的原住民".to_string()
        },
        "balanced" => {
            "风格：平衡多元、兼具深度与趣味。既有数据支撑，又有创意表达。话题覆盖行为分析、兴趣洞察、成长轨迹等多个维度。
            示例：你的数字人格、兴趣光谱分析、注意力地图、隐藏的超能力、一年前vs现在的你".to_string()
        },
        "experimental" => {
            "风格：前卫大胆、打破常规。使用科幻、玄学、未来学等实验性概念。话题要让用户感到新奇和意外。
            示例：平行宇宙的你、量子纠缠的兴趣、时间旅行护照、数字考古发现、能量光谱".to_string()
        },
        _ => {
            "风格：平衡多元、兼具深度与趣味。既有数据支撑，又有创意表达。".to_string()
        }
    }
}

/// 获取已保存的报告
pub async fn get_report(State(_db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    let user_id = 1; // TODO: 从认证中获取真实用户ID

    // 检查缓存（不强制刷新）
    if let Some(cached_report) = get_cached_report(user_id, false) {
        tracing::info!("✓ Retrieved cached report");
        return (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "report": cached_report,
                "from_cache": true
            })),
        );
    }

    // 没有缓存
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": "No cached report found. Please generate a new report.",
            "report": null
        })),
    )
}

/// 获取所有缓存的报告列表
pub async fn list_reports(State(_db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    let user_id = 1; // TODO: 从认证中获取真实用户ID

    let cache = REPORT_CACHE.lock().unwrap();
    let now = Utc::now();

    // 获取该用户所有有效的报告
    let mut reports: Vec<Value> = cache
        .iter()
        .filter(|(key, (_, created_at))| {
            key.starts_with(&user_id.to_string()) && now - *created_at < Duration::days(7)
        })
        .map(|(key, (report, created_at))| {
            json!({
                "id": key,
                "generated_at": report.generated_at,
                "expires_at": report.expires_at,
                "card_count": report.cards.len(),
                "selected_topics": report.selected_topics,
                "cached_at": created_at.to_rfc3339()
            })
        })
        .collect();

    // 按时间倒序排序
    reports.sort_by(|a, b| {
        let time_a = a["generated_at"].as_str().unwrap_or("");
        let time_b = b["generated_at"].as_str().unwrap_or("");
        time_b.cmp(time_a)
    });

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "reports": reports,
            "total": reports.len()
        })),
    )
}

/// 根据ID获取特定报告
pub async fn get_report_by_id(
    State(_db): State<DatabaseConnection>,
    axum::extract::Path(report_id): axum::extract::Path<String>,
) -> (StatusCode, Json<Value>) {
    let cache = REPORT_CACHE.lock().unwrap();

    if let Some((report, created_at)) = cache.get(&report_id) {
        let now = Utc::now();
        if now - *created_at < Duration::days(7) {
            return (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "report": report,
                    "from_cache": true
                })),
            );
        }
    }

    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "success": false,
            "message": "Report not found or expired"
        })),
    )
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

/// 获取所有缓存数据的详细信息（用于调试）
pub async fn get_cache_debug_info(
    State(_db): State<DatabaseConnection>,
) -> (StatusCode, Json<Value>) {
    let cache = REPORT_CACHE.lock().unwrap();
    let now = Utc::now();

    // 加载平台数据缓存以获取原始数据
    let platform_cache = load_platform_data_cache();
    let raw_data = platform_cache.as_ref().map(|c| c.data.clone());

    let cache_info: Vec<Value> = cache
        .iter()
        .map(|(key, (report, created_at))| {
            let age_seconds = (now - *created_at).num_seconds();
            let age_hours = age_seconds / 3600;
            let age_days = age_seconds / 86400;

            json!({
                "cache_key": key,
                "card_count": report.cards.len(),
                "selected_topics": report.selected_topics,
                "generated_at": report.generated_at,
                "expires_at": report.expires_at,
                "cached_at": created_at.to_rfc3339(),
                "age": {
                    "seconds": age_seconds,
                    "hours": age_hours,
                    "days": age_days
                },
                "is_valid": age_days < 7,
                "raw_data": raw_data.as_ref()
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "total_entries": cache_info.len(),
            "cache_entries": cache_info,
            "max_entries": MAX_CACHE_ENTRIES,
            "cache_file": CACHE_FILE_PATH
        })),
    )
}

/// 从数据库或缓存中获取用户信息（支持多平台）
/// 优先从数据库获取，若数据库无数据则从缓存获取
pub async fn get_user_info(State(db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    let user_id = 1; // TODO: 从认证中获取真实用户ID

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

/// 删除指定的报告
pub async fn delete_report_by_id(
    State(_db): State<DatabaseConnection>,
    axum::extract::Path(report_id): axum::extract::Path<String>,
) -> (StatusCode, Json<Value>) {
    tracing::info!("🗑️ Deleting report: {}", report_id);

    let mut cache = REPORT_CACHE.lock().unwrap();

    if cache.remove(&report_id).is_some() {
        drop(cache); // 释放锁

        // 保存到磁盘
        if let Err(e) = save_cache_to_disk() {
            tracing::error!("❌ Failed to save cache after deletion: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Report deleted but failed to save: {}", e)
                })),
            );
        }

        tracing::info!("✓ Report deleted successfully");
        (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "message": "Report deleted successfully"
            })),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "success": false,
                "message": "Report not found"
            })),
        )
    }
}

/// 删除所有报告
/// ⚠️ DANGEROUS: This operation deletes all reports and requires admin privileges
pub async fn delete_all_reports(
    State(_db): State<DatabaseConnection>,
) -> (StatusCode, Json<Value>) {
    tracing::warn!("⚠️ ADMIN OPERATION: Deleting all reports...");

    let mut cache = REPORT_CACHE.lock().unwrap();
    let count = cache.len();
    cache.clear();
    drop(cache); // 释放锁

    // 保存到磁盘
    if let Err(e) = save_cache_to_disk() {
        tracing::error!("❌ Failed to save cache after clearing: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": format!("Reports cleared but failed to save: {}", e)
            })),
        );
    }

    tracing::info!("✓ All {} reports deleted successfully", count);
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": format!("All {} reports deleted successfully", count),
            "deleted_count": count
        })),
    )
}

/// 从报告中删除单个卡片
#[derive(Deserialize)]
pub struct DeleteCardRequest {
    pub card_index: usize,
}

pub async fn delete_card_from_report(
    State(_db): State<DatabaseConnection>,
    axum::extract::Path(report_id): axum::extract::Path<String>,
    Json(payload): Json<DeleteCardRequest>,
) -> (StatusCode, Json<Value>) {
    tracing::info!(
        "🗑️ Deleting card {} from report: {}",
        payload.card_index,
        report_id
    );

    let mut cache = REPORT_CACHE.lock().unwrap();

    if let Some((report, _created_at)) = cache.get_mut(&report_id) {
        if payload.card_index >= report.cards.len() {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "success": false,
                    "message": format!("Card index {} out of range (total: {})", payload.card_index, report.cards.len())
                })),
            );
        }

        // 删除指定索引的卡片
        let deleted_card = report.cards.remove(payload.card_index);
        tracing::info!("✓ Deleted card: {}", deleted_card.title);

        // 如果报告中没有卡片了，删除整个报告
        if report.cards.is_empty() {
            tracing::info!(
                "Report {} has no cards left, deleting entire report",
                report_id
            );
            cache.remove(&report_id);
        }

        drop(cache); // 释放锁

        // 保存到磁盘
        if let Err(e) = save_cache_to_disk() {
            tracing::error!("❌ Failed to save cache after card deletion: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Card deleted but failed to save: {}", e)
                })),
            );
        }

        tracing::info!("✓ Card deleted successfully");
        (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "message": "Card deleted successfully",
                "deleted_card_title": deleted_card.title
            })),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "success": false,
                "message": "Report not found"
            })),
        )
    }
}

/// 将图片URL转换为代理URL（用于处理防盗链）
pub fn proxy_image_url(url: &str) -> String {
    // 检查是否需要代理（Bilibili图片）
    if url.contains("hdslb.com") || url.contains("bilibili.com") {
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

/// 获取资料库数据（游戏、视频、音乐）
pub async fn get_library_data(State(db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    let user_id = 1; // TODO: 从认证中获取真实用户ID

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

    tracing::info!("✅ Loaded {} library items in total", library_items.len());

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "items": library_items,
            "total": library_items.len()
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
    pub cache_debug: Option<Value>,
}

pub async fn get_batch_user_info(
    State(db): State<DatabaseConnection>,
) -> (StatusCode, Json<Value>) {
    let user_id = 1; // TODO: 从认证中获取真实用户ID

    tracing::info!("📦 Fetching batch user info for: {}", user_id);

    let mut response = BatchUserInfoResponse {
        user_info: None,
        config: None,
        cache_debug: None,
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

    // 3. 获取缓存调试信息
    let (cache_status, cache_json) = get_cache_debug_info(State(db.clone())).await;
    if cache_status == StatusCode::OK {
        response.cache_debug = Some(cache_json.0);
    } else {
        response.cache_debug = Some(json!({
            "success": false,
            "message": "Failed to fetch cache info"
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
    pub id: i32,
    pub platform_name: String,
    pub changed_fields: Value,
    pub change_date: String,
    pub item_type: Option<String>,
    pub item_title: Option<String>,
}

pub async fn get_recent_activities(
    Query(params): Query<ActivityQuery>,
    State(db): State<DatabaseConnection>,
) -> (StatusCode, Json<Value>) {
    use crate::models::entities::metadata_history;
    use sea_orm::{EntityTrait, QueryOrder, QuerySelect};

    let limit = params.limit.unwrap_or(10).min(50); // 最多50条

    match metadata_history::Entity::find()
        .order_by_desc(metadata_history::Column::ChangeDate)
        .limit(limit)
        .all(&db)
        .await
    {
        Ok(records) => {
            let activities: Vec<ActivityItem> = records
                .into_iter()
                .map(|record| {
                    let changed_fields = record.changed_fields.clone();

                    // 尝试从 new_data 或 old_data 中提取标题和类型
                    let (item_title, item_type) = if let Some(new_data) = &record.new_data {
                        let title = new_data
                            .get("title")
                            .and_then(|v| v.as_str())
                            .or_else(|| new_data.get("name").and_then(|v| v.as_str()))
                            .or_else(|| new_data.get("full_name").and_then(|v| v.as_str()))
                            .map(String::from);

                        let item_type = new_data
                            .get("type")
                            .and_then(|v| v.as_str())
                            .map(String::from);

                        (title, item_type)
                    } else if let Some(old_data) = &record.old_data {
                        let title = old_data
                            .get("title")
                            .and_then(|v| v.as_str())
                            .or_else(|| old_data.get("name").and_then(|v| v.as_str()))
                            .or_else(|| old_data.get("full_name").and_then(|v| v.as_str()))
                            .map(String::from);

                        let item_type = old_data
                            .get("type")
                            .and_then(|v| v.as_str())
                            .map(String::from);

                        (title, item_type)
                    } else {
                        (None, None)
                    };

                    // 如果 new_data/old_data 中没有标题，尝试使用平台名称作为后备
                    let final_title =
                        item_title.or_else(|| Some(format!("{} 数据", record.platform_name)));

                    ActivityItem {
                        id: record.id,
                        platform_name: record.platform_name,
                        changed_fields,
                        change_date: record.change_date.to_string(),
                        item_type,
                        item_title: final_title,
                    }
                })
                .collect();

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
