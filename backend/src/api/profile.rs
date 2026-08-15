use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{Duration, Utc};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, Statement,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

// Platform refresh / site owner live in services (scheduler must not depend on HTTP).
use crate::services::platform_refresh::{
    fetch_fresh_platform_data, load_platform_data_cache, platform_data_warning,
    resolve_platform_fetch_message, save_platform_data_cache, PLATFORM_CACHE_HOURS,
};
pub use crate::services::site_owner::site_owner_user_id;

#[derive(Deserialize)]
pub struct FetchPlatformRequest {
    pub platform: String,
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
        Ok(outcome) => {
            // 保存到缓存
            if let Err(e) = save_platform_data_cache(&outcome.data) {
                tracing::error!("Failed to save platform cache: {}", e);
            }

            let message = if outcome.errors.is_empty() {
                "Data fetched successfully".to_string()
            } else {
                format!(
                    "Data fetched with {} platform error(s)",
                    outcome.errors.len()
                )
            };

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "message": message,
                    "data": outcome.data,
                    "errors": outcome.errors,
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
                    "message": "Failed to fetch data"
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
        Ok(outcome) => {
            // 保存到缓存
            if let Err(e) = save_platform_data_cache(&outcome.data) {
                tracing::error!("Failed to save platform cache: {}", e);
            }

            let message = if outcome.errors.is_empty() {
                "Data refreshed successfully".to_string()
            } else {
                format!(
                    "Data refreshed with {} platform error(s)",
                    outcome.errors.len()
                )
            };

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "message": message,
                    "data": outcome.data,
                    "errors": outcome.errors,
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
                    "message": "Failed to refresh data"
                })),
            )
        }
    }
}

/// 检查抓取回来的平台数据是否为空/缺失，返回给用户的可读提示。
/// 返回 None 表示数据看起来正常。
pub async fn fetch_single_platform_data(
    State(db): State<DatabaseConnection>,
    Json(req): Json<FetchPlatformRequest>,
) -> (StatusCode, Json<Value>) {
    tracing::info!("🔄 Fetching data for platform: {}...", req.platform);

    match fetch_fresh_platform_data(&db, Some(&req.platform)).await {
        Ok(outcome) => {
            // 只保存请求的平台数据，而不是所有平台
            if let Some(platform_data) = outcome.data.get(&req.platform) {
                let single_platform_data = json!({
                    &req.platform: platform_data
                });
                if let Err(e) = save_platform_data_cache(&single_platform_data) {
                    tracing::error!("Failed to save platform cache: {}", e);
                }
            }

            let remote_err = outcome.errors.get(&req.platform).map(String::as_str);
            // 远程失败或数据为空：优先透传真实错误（如 X 402 额度耗尽）
            if let Some(message) = resolve_platform_fetch_message(
                &req.platform,
                outcome.data.get(&req.platform),
                remote_err,
            ) {
                let empty =
                    platform_data_warning(&req.platform, outcome.data.get(&req.platform)).is_some();
                tracing::warn!(
                    "⚠️ {} fetch issue (empty={}): {}",
                    req.platform,
                    empty,
                    message
                );
                return (
                    StatusCode::OK,
                    Json(json!({
                        "success": false,
                        "message": message,
                        "data": outcome.data,
                        "error": remote_err,
                        "fetched_at": chrono::Utc::now().to_rfc3339()
                    })),
                );
            }

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "message": format!("Data for {} fetched successfully", req.platform),
                    "data": outcome.data,
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
                    "message": "Failed to fetch data"
                })),
            )
        }
    }
}

/// Admin-only debug dump of the in-process platform data cache (`cache/raw` shape).
///
/// Contains full platform raw JSON for the site — never expose without auth.
/// Route: `GET /api/profile/metadata` (admin middleware).
pub async fn get_raw_metadata(
    crate::extract::Db(_db): crate::extract::Db,
) -> (StatusCode, Json<Value>) {
    tracing::info!("📊 Reading cached platform metadata for debugging (admin)...");

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

/// 获取单个平台持久化原始数据的状态。
///
/// 设置页必须以数据库中的 `platform_metadata` 为准，不能依赖只用于调试的
/// 全局文件缓存；文件缓存可能在进程启动或轮换期间暂时不存在。
pub async fn get_platform_metadata_status(
    Path(platform): Path<String>,
    crate::extract::Db(db): crate::extract::Db,
) -> (StatusCode, Json<Value>) {
    use crate::models::entities::platform_metadata;

    let user_id = match site_owner_user_id(&db).await {
        Ok(user_id) => user_id,
        Err(error) => return site_owner_error(error),
    };

    match platform_metadata::Entity::find()
        .filter(platform_metadata::Column::UserId.eq(user_id))
        .filter(platform_metadata::Column::PlatformName.eq(&platform))
        .order_by_desc(platform_metadata::Column::FetchedAt)
        .one(&db)
        .await
    {
        Ok(metadata) => {
            let raw_data_size = metadata
                .as_ref()
                .and_then(|item| serde_json::to_vec(&item.raw_data).ok())
                .map(|bytes| bytes.len())
                .unwrap_or(0);
            let raw_fetched_at = metadata
                .as_ref()
                .map(|item| item.fetched_at.and_utc().to_rfc3339());

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "platform": platform,
                    "has_raw_data": metadata.is_some(),
                    "raw_data_size": raw_data_size,
                    "raw_fetched_at": raw_fetched_at
                })),
            )
        }
        Err(error) => {
            tracing::error!(
                platform = %platform,
                %error,
                "Failed to load persisted platform metadata status"
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": "Failed to load platform metadata status"
                })),
            )
        }
    }
}

/// 站长公开资料（首页信息条 / SEO）。
///
/// - **头像** ← [`crate::services::avatar::resolve_avatar`]（`avatar_source_*`）
/// - **名称/简介/平台标签** ← [`crate::services::profile_text::resolve_profile_text`]
///   （`profile_text_source_*`，与画像源独立）
///
/// 两套来源可分别选定，例如脸用 GitHub、简介仍用 B 站。文案 auto 时保留历史
/// `PLATFORM_ORDER` 首个平台；显式选定后严格跟该源。
async fn build_user_info(db: &DatabaseConnection) -> (StatusCode, Value) {
    use crate::services::avatar::resolve_avatar;
    use crate::services::profile_text::resolve_profile_text;

    let user_id = match site_owner_user_id(db).await {
        Ok(user_id) => user_id,
        Err(error) => {
            tracing::warn!(%error, "Site owner lookup failed");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                json!({ "success": false, "message": "Site owner is not configured" }),
            );
        }
    };

    let avatar = resolve_avatar(db, user_id).await;
    let text = match resolve_profile_text(db, user_id).await {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(%error, user_id, "Profile text resolve failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "success": false, "message": "Failed to resolve profile text" }),
            );
        }
    };

    if text.name.is_none() && avatar.is_none() {
        return (
            StatusCode::NOT_FOUND,
            json!({
                "success": false,
                "message": "No user info found in database or cache. Please fetch platform data first."
            }),
        );
    }

    (
        StatusCode::OK,
        json!({
            "success": true,
            "user_info": {
                "name": text.name,
                "avatar": avatar,
                "bio": text.bio,
                "platform": text.platform,
            },
            "source": text.source,
        }),
    )
}

/// 内容哈希 ETag：任何字段变化都会变，因此 304 不会把陈旧名称/简介锁死。
///
/// Uses SHA-256 of canonical JSON bytes so ETags are stable across process
/// restarts (unlike `DefaultHasher`, which is not portable).
fn weak_etag(value: &Value) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(value.to_string().as_bytes());
    format!("W/\"{}\"", hex::encode(digest))
}

/// GET /api/profile/user-info
///
/// 带 `Cache-Control` + 内容 ETag，取代前端那套 30 分钟 localStorage 缓存
/// （后者只在登录/登出时失效，站长换了画像源要等半小时才生效）。
pub async fn get_user_info(
    crate::extract::Db(db): crate::extract::Db,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let (status, body) = build_user_info(&db).await;
    if status != StatusCode::OK {
        return (status, Json(body)).into_response();
    }

    let etag = weak_etag(&body);
    let cache_headers = [
        (axum::http::header::CACHE_CONTROL, "public, max-age=60"),
        (axum::http::header::ETAG, etag.as_str()),
    ];

    let matches = headers
        .get(axum::http::header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|candidate| candidate.trim() == etag));
    if matches {
        return (StatusCode::NOT_MODIFIED, cache_headers).into_response();
    }

    (status, cache_headers, Json(body)).into_response()
}

/// 删除平台数据缓存
pub async fn delete_platform_cache(
    State(_db): State<DatabaseConnection>,
) -> (StatusCode, Json<Value>) {
    tracing::info!("🗑️ Deleting platform data cache...");
    invalidate_library_assembly_cache();

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
                messages.push("Failed to delete platform cache".to_string());
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

// Media URL rewrite (pure) — implementation in services so schedulers/export can share it.
pub use crate::services::image_proxy_urls::{normalize_json_media_urls, proxy_image_url};

// Library item shaping (pure) — DB I/O stays in this module.
pub use crate::services::library_items::{
    append_bangumi_library_items, append_mal_library_items, apply_library_source_preferences,
    cached_library_items, collect_library_source_options, invalidate_library_assembly_cache,
    paginate_library_items, store_library_items, CachedLibraryItems, LibraryItem,
    LibrarySourcePreferences, LIBRARY_SOURCE_PREFERENCES_KEY,
};
async fn load_library_source_preferences(db: &DatabaseConnection) -> LibrarySourcePreferences {
    let sql = "SELECT value FROM configurations WHERE key = $1";
    let result = db
        .query_one_raw(Statement::from_sql_and_values(
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

/// 获取资料库数据（游戏、视频、音乐）
#[derive(Debug, Default, Deserialize)]
pub struct LibraryPageQuery {
    offset: Option<usize>,
    limit: Option<usize>,
    #[serde(rename = "type")]
    item_type: Option<String>,
}

async fn library_page_response(
    db: &DatabaseConnection,
    raw_items: CachedLibraryItems,
    query: LibraryPageQuery,
    user_id: i32,
) -> (StatusCode, Json<Value>) {
    let preferences = load_library_source_preferences(db).await;
    let raw_total = raw_items.len();
    let available_sources = collect_library_source_options(&raw_items);
    let page = match paginate_library_items(
        raw_items.as_slice(),
        Some(&preferences),
        query.item_type.as_deref(),
        query.offset,
        query.limit,
    ) {
        Ok(page) => page,
        Err(message) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "success": false, "message": message })),
            )
        }
    };

    if page.total == 0 {
        tracing::info!("📚 Library empty for user {user_id} — returning 200 + []");
    } else {
        tracing::info!(
            "✅ Returning {} of {} library items ({} raw before source filtering)",
            page.returned,
            page.total,
            raw_total
        );
    }

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "items": page.items,
            "total": page.total,
            "returned": page.returned,
            "offset": page.offset,
            "limit": page.limit,
            "type": query.item_type,
            "has_more": page.has_more,
            "next_offset": page.next_offset,
            "raw_total": raw_total,
            "preferences": preferences,
            "available_sources": available_sources,
            "empty": page.total == 0,
            "message": if page.total == 0 {
                "No library data yet. Fetch platform data when ready."
            } else {
                ""
            }
        })),
    )
}

pub async fn get_library_data(
    State(db): State<DatabaseConnection>,
    Query(query): Query<LibraryPageQuery>,
) -> (StatusCode, Json<Value>) {
    let user_id = match site_owner_user_id(&db).await {
        Ok(user_id) => user_id,
        Err(error) => return site_owner_error(error),
    };

    tracing::info!("📚 Fetching library data for user: {}", user_id);

    // Consecutive typed/page requests reuse the assembled library; preferences and
    // pagination are still reapplied per request, and refresh paths invalidate it.
    if let Some(items) = cached_library_items(user_id) {
        return library_page_response(&db, items, query, user_id).await;
    }

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

    // Empty library is valid. Cache the assembled shape, then reapply current
    // preferences/type/pagination for every response.
    let library_items = store_library_items(user_id, library_items);
    library_page_response(&db, library_items, query, user_id).await
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
    crate::extract::Db(db): crate::extract::Db,
) -> (StatusCode, Json<Value>) {
    tracing::info!("📦 Fetching batch site-owner information");

    let mut response = BatchUserInfoResponse {
        user_info: None,
        config: None,
    };

    // 1. 获取用户基本信息
    let (status, body) = build_user_info(&db).await;
    if status == StatusCode::OK {
        response.user_info = Some(body);
    } else {
        response.user_info = Some(json!({
            "success": false,
            "message": "Failed to fetch user info"
        }));
    }

    // 2. 获取配置信息 - 仅返回平台启用状态，不返回敏感数据
    let (config_status, config_json) =
        crate::api::config::get_config(crate::extract::Db(db.clone())).await;
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
                        // RFC3339 (NaiveDateTime treated as UTC) so FE Date parses reliably.
                        change_date: event.occurred_at.and_utc().to_rfc3339(),
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
                let change_iso = record.change_date.and_utc().to_rfc3339();
                legacy_groups
                    .entry(key)
                    .and_modify(|group| {
                        group.change_count = group.change_count.saturating_add(field_count);
                        if change_iso > group.change_date {
                            group.change_date = change_iso.clone();
                        }
                    })
                    .or_insert_with(|| LegacyActivityGroup {
                        platform_name: record.platform_name,
                        change_count: field_count,
                        change_date: change_iso,
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
                    "message": "Failed to fetch activities"
                })),
            )
        }
    }
}
