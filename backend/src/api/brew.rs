//! Brew 阅读 API 端点
//!
//! 提供 RSS/Atom 订阅管理、文章获取、阅读状态同步等功能

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    middleware::from_fn,
    response::IntoResponse,
    routing::{get, post, put},
    Extension, Json, Router,
};
use chrono::Utc;
use futures::StreamExt;
use reqwest::Url;
use sea_orm::{
    sea_query::Expr, ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait,
    DatabaseBackend, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, QueryTrait, Statement, Value as SeaValue,
};
use serde::Deserialize;
use serde_json::json;

use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::middleware::auth::Claims;
use crate::middleware::auth::{
    ensure_current_admin, verify_current_admin_from_headers, verify_jwt_token,
};
use crate::models::entities::{
    brew_annotations, brew_categories, brew_comments, brew_items, brew_podcasts, brew_sources,
    brew_user_states, rsshub_instances,
};
use crate::services::brew_parser::{FeedParser, ParsedFeed};
use crate::services::brew_scheduler::get_brew_scheduler;
use crate::services::data_paths::paths;
use crate::services::icon_service::IconService;
use crate::services::rsshub_service::RsshubService;

/// 创建 Brew API 路由
pub fn create_brew_routes() -> Router<DatabaseConnection> {
    Router::new()
        // 订阅源管理
        .route("/sources", get(list_sources).post(add_source))
        .route(
            "/sources/{id}",
            get(get_source).put(update_source).delete(delete_source),
        )
        .route("/sources/{id}/refresh", post(refresh_source))
        .route("/sources/discover", post(discover_source))
        // OPML 导入导出
        .route("/import-opml", post(import_opml))
        .route("/export-opml", get(export_opml))
        // 分类管理
        .route("/categories", get(list_categories).post(create_category))
        .route(
            "/categories/{id}",
            put(update_category).delete(delete_category),
        )
        // 文章获取
        .route("/items", get(list_items))
        .route("/items/{id}", get(get_item))
        .route("/items/{id}/fulltext", get(fetch_fulltext))
        // 阅读状态
        .route("/items/{id}/read", post(mark_read))
        .route("/items/{id}/unread", post(mark_unread))
        .route("/items/{id}/star", post(star_item))
        .route("/items/{id}/unstar", post(unstar_item))
        .route("/mark-all-read", post(mark_all_read))
        // 用户评论（批注）
        .route(
            "/items/{id}/comments",
            get(list_comments).post(create_comment),
        )
        .route("/comments/{id}", put(update_comment).delete(delete_comment))
        .route("/comments/{id}/replies", get(list_comment_replies))
        // 离线同步
        .route("/sync-states", post(sync_states))
        // 统计信息
        .route("/stats", get(get_stats))
        // WebSocket（通知）
        .route(
            "/ws",
            get(brew_websocket).route_layer(from_fn(crate::middleware::auth::auth_middleware)),
        )
        // RSSHub 实例管理
        .route(
            "/rsshub/instances",
            get(list_rsshub_instances).post(add_rsshub_instance),
        )
        .route(
            "/rsshub/instances/{id}",
            put(update_rsshub_instance).delete(delete_rsshub_instance),
        )
        .route(
            "/rsshub/instances/{id}/health-check",
            post(health_check_rsshub_instance),
        )
        .route("/rsshub/instances/{id}/reset", post(reset_rsshub_instance))
        .route(
            "/rsshub/health-check-all",
            post(health_check_all_rsshub_instances),
        )
        // 图标静态文件服务（带缓存头和压缩支持）
        .nest_service(
            "/icons",
            tower::ServiceBuilder::new()
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::CACHE_CONTROL,
                    header::HeaderValue::from_static("public, max-age=86400, immutable"),
                ))
                .service(ServeDir::new(&paths().brew_icons)),
        )
        // 图片缓存服务（Notion 临时 URL 等）
        .nest_service(
            "/image-cache",
            tower::ServiceBuilder::new()
                .layer(SetResponseHeaderLayer::if_not_present(
                    header::CACHE_CONTROL,
                    header::HeaderValue::from_static("public, max-age=604800, immutable"),
                ))
                .service(ServeDir::new(&paths().cache_images)),
        )
        // Tapp 运行时携带 Grant 头时做服务端归因与权限强制；宿主 UI 请求不受影响
        .route_layer(from_fn(crate::api::tapp_runtime::brew_host_attribution))
}

// ==================== 订阅源管理 ====================

/// 最新文章预览
#[derive(Clone, Debug, serde::Serialize)]
struct ItemPreview {
    id: i32,
    title: String,
    summary: Option<String>,
    image: Option<String>,
    published_at: Option<i64>,
    is_read: bool,
}

/// 带最新文章的订阅源响应
#[derive(Clone, Debug, serde::Serialize)]
struct SourceWithRecentItems {
    #[serde(flatten)]
    source: brew_sources::SourceResponse,
    recent_items: Vec<ItemPreview>,
}

/// 获取订阅源列表（带最新文章预览）
/// 游客可访问（只读），但不会计算已读状态以节约计算
/// 非管理员用户看不到 admin_only=true 的订阅源
async fn list_sources(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // 获取用户 ID 和管理员状态
    let (user_id, is_admin) = get_user_and_admin_status(&headers).await;

    // 获取订阅源（非管理员过滤掉 admin_only=true 的源）
    let mut query = brew_sources::Entity::find().order_by_asc(brew_sources::Column::Name);

    if !is_admin {
        query = query.filter(brew_sources::Column::AdminOnly.eq(false));
    }

    let sources = match query.all(&db).await {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": e.to_string() })),
            )
                .into_response();
        }
    };

    // 获取所有订阅源的最新文章（每个源最多3篇）
    let source_ids: Vec<i32> = sources.iter().map(|s| s.id).collect();

    // 并行执行两个 SQL 查询，均只传输必要字段：
    // (a) 每源未读数：SQL 聚合，避免把所有 item_id 拉到内存再过滤
    // (b) 每源最新3篇预览：窗口函数精确返回3条，仅加载预览字段，不加载 content 等大字段
    let (source_unread_counts, mut items_by_source) = tokio::join!(
        // (a) 未读数：LEFT JOIN brew_user_states，统计无已读状态的文章数
        async {
            let mut counts: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();
            if let Some(uid) = user_id {
                if !source_ids.is_empty() {
                    let src_ph = source_ids
                        .iter()
                        .enumerate()
                        .map(|(i, _)| format!("${}", i + 2))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let sql = format!(
                        "SELECT i.source_id, COUNT(*)::int AS unread_count \
                         FROM brew_items i \
                         LEFT JOIN brew_user_states s \
                           ON s.item_id = i.id AND s.user_id = $1 AND s.is_read = TRUE \
                         WHERE i.source_id IN ({src_ph}) AND s.item_id IS NULL \
                         GROUP BY i.source_id"
                    );
                    let mut values: Vec<sea_orm::Value> = vec![uid.into()];
                    values.extend(source_ids.iter().map(|&id| sea_orm::Value::Int(Some(id))));
                    let stmt =
                        Statement::from_sql_and_values(DatabaseBackend::Postgres, &sql, values);
                    if let Ok(rows) = db.query_all(stmt).await {
                        for row in &rows {
                            let src: i32 = row.try_get("", "source_id").unwrap_or(0);
                            let cnt: i32 = row.try_get("", "unread_count").unwrap_or(0);
                            counts.insert(src, cnt);
                        }
                    }
                }
            }
            counts
        },
        // (b) 每源最新3篇预览：ROW_NUMBER() OVER PARTITION，只选预览字段
        async {
            let mut map: std::collections::HashMap<i32, Vec<ItemPreview>> =
                std::collections::HashMap::new();
            if !source_ids.is_empty() {
                let src_ph = source_ids
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("${}", i + 2))
                    .collect::<Vec<_>>()
                    .join(", ");
                // $1 = user_id（游客传 -1，不存在的 ID，LEFT JOIN 不会匹配任何行）
                let sql = format!(
                    "SELECT id, source_id, title, summary, image, published_at, \
                            COALESCE(is_read, false) AS is_read \
                     FROM ( \
                       SELECT i.id, i.source_id, i.title, i.summary, i.image, \
                              i.published_at, s.is_read, \
                              ROW_NUMBER() OVER \
                                (PARTITION BY i.source_id ORDER BY i.published_at DESC NULLS LAST) AS rn \
                       FROM brew_items i \
                       LEFT JOIN brew_user_states s \
                         ON s.item_id = i.id AND s.user_id = $1 \
                       WHERE i.source_id IN ({src_ph}) \
                     ) ranked \
                     WHERE rn <= 3"
                );
                let uid_val: i32 = user_id.unwrap_or(-1);
                let mut values: Vec<sea_orm::Value> = vec![uid_val.into()];
                values.extend(source_ids.iter().map(|&id| sea_orm::Value::Int(Some(id))));
                let stmt = Statement::from_sql_and_values(DatabaseBackend::Postgres, &sql, values);
                if let Ok(rows) = db.query_all(stmt).await {
                    for row in &rows {
                        let id: i32 = row.try_get("", "id").unwrap_or(0);
                        let source_id: i32 = row.try_get("", "source_id").unwrap_or(0);
                        let title: String = row.try_get("", "title").unwrap_or_default();
                        let summary: Option<String> = row.try_get("", "summary").ok().flatten();
                        let image: Option<String> = row.try_get("", "image").ok().flatten();
                        let published_at: Option<sea_orm::entity::prelude::DateTimeWithTimeZone> =
                            row.try_get("", "published_at").ok();
                        let is_read: bool = row.try_get("", "is_read").unwrap_or(false);
                        map.entry(source_id).or_default().push(ItemPreview {
                            id,
                            title,
                            summary,
                            image,
                            published_at: published_at.map(|dt| dt.timestamp_millis()),
                            is_read,
                        });
                    }
                }
            }
            map
        }
    );

    // 构建响应，使用 SQL 计算的真实未读数
    let responses: Vec<SourceWithRecentItems> = sources
        .into_iter()
        .map(|s| {
            let source_id = s.id;
            let real_unread_count = source_unread_counts.get(&source_id).copied().unwrap_or(0);
            let mut response: brew_sources::SourceResponse = s.into();
            response.unread_count = real_unread_count;
            SourceWithRecentItems {
                source: response,
                recent_items: items_by_source.remove(&source_id).unwrap_or_default(),
            }
        })
        .collect();

    (
        StatusCode::OK,
        Json(json!({ "success": true, "sources": responses })),
    )
        .into_response()
}

/// 添加订阅源
#[derive(Debug, Deserialize)]
struct AddSourceRequest {
    url: String,
    name: Option<String>,
    category: Option<String>,
    update_interval: Option<i32>,
    /// 来源类型: link, rss, brewlia
    source_type: Option<String>,
    /// Feed 类型: rss, atom, json_feed, notion, rsshub
    feed_type: Option<String>,
    /// RSSHub 路由路径（仅当 feed_type = rsshub 时使用）
    rsshub_route: Option<String>,
    /// 额外配置（用于 Notion token 等敏感信息）
    extra_config: Option<serde_json::Value>,
    /// 仅管理员可见
    admin_only: Option<bool>,
}

async fn add_source(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<AddSourceRequest>,
) -> impl IntoResponse {
    // 添加订阅源需要管理员权限
    let user_id = match get_admin_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    // 验证 URL
    let url = req.url.trim();
    if url.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "URL is required" })),
        )
            .into_response();
    }

    // 检查是否已订阅
    let existing = brew_sources::Entity::find()
        .filter(brew_sources::Column::UserId.eq(user_id))
        .filter(brew_sources::Column::Url.eq(url))
        .one(&db)
        .await;

    if let Ok(Some(_)) = existing {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "success": false, "error": "Already subscribed to this feed" })),
        )
            .into_response();
    }

    // 解析 source_type
    let source_type = match req.source_type.as_deref() {
        Some("link") => brew_sources::SourceType::Link,
        Some("brewlia") => brew_sources::SourceType::Brewlia,
        _ => brew_sources::SourceType::Rss,
    };

    // 检查是否是 Notion 类型
    let is_notion = req.feed_type.as_deref() == Some("notion")
        || crate::services::notion_service::NotionService::parse_notion_url(url).is_ok();

    // 获取源信息
    let (name, description, icon, site_url, feed_type, extra_config) =
        if source_type == brew_sources::SourceType::Link {
            // 纯链接类型不需要解析，直接添加
            (
                req.name.unwrap_or_else(|| url.to_string()),
                None,
                None,
                Some(url.to_string()),
                brew_sources::FeedType::Rss,
                None,
            )
        } else if is_notion {
            // Notion 类型需要 token
            let extra_config = req.extra_config.clone();
            let token = extra_config
                .as_ref()
                .and_then(|config| config.get("token"))
                .and_then(|token| token.as_str())
                .map(str::trim)
                .filter(|token| !token.is_empty());
            let Some(token) = token else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "error": "Notion source requires extra_config with token"
                    })),
                )
                    .into_response();
            };

            // 尝试验证 Notion 源
            let notion_service = crate::services::notion_service::NotionService::new();

            // 解析 Notion URL
            let (resource_type, resource_id) =
                match crate::services::notion_service::NotionService::parse_notion_url(url) {
                    Ok(r) => r,
                    Err(e) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({ "success": false, "error": e.to_string() })),
                        )
                            .into_response();
                    }
                };

            let config = crate::services::notion_service::NotionConfig {
                token: token.to_string(),
                resource_id,
                resource_type,
                filter: extra_config.as_ref().and_then(|c| c.get("filter").cloned()),
                sort: extra_config.as_ref().and_then(|c| c.get("sort").cloned()),
            };

            // 尝试获取 Notion 信息
            match notion_service.fetch(&config).await {
                Ok(feed) => (
                    req.name.unwrap_or(feed.title),
                    feed.description,
                    feed.icon,
                    feed.site_url,
                    brew_sources::FeedType::Notion,
                    extra_config,
                ),
                Err(e) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "success": false,
                            "error": format!("Failed to fetch Notion: {}", e)
                        })),
                    )
                        .into_response();
                }
            }
        } else {
            // 标准 RSS/Atom/JSON Feed 或 RSSHub
            // 检查是否是 RSSHub 类型（由前端传入）
            let is_rsshub = req.feed_type.as_deref() == Some("rsshub");

            let parser = FeedParser::new();
            match parser.fetch_and_parse(url).await {
                Ok(feed) => {
                    // 如果前端指定了 rsshub，使用 rsshub 类型，否则使用解析器返回的类型
                    let final_feed_type = if is_rsshub {
                        brew_sources::FeedType::RssHub
                    } else {
                        feed.feed_type
                    };
                    (
                        req.name.unwrap_or(feed.title),
                        feed.description,
                        feed.icon,
                        feed.site_url,
                        final_feed_type,
                        None,
                    )
                }
                Err(e) => {
                    // 即使解析失败也允许添加，使用用户提供的名称
                    if req.name.is_none() {
                        return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "success": false,
                            "error": format!("Failed to parse feed: {}. Please provide a name.", e)
                        })),
                    )
                        .into_response();
                    }
                    // 如果前端指定了 rsshub，使用 rsshub 类型
                    let final_feed_type = if is_rsshub {
                        brew_sources::FeedType::RssHub
                    } else {
                        brew_sources::FeedType::Rss
                    };
                    (req.name.unwrap(), None, None, None, final_feed_type, None)
                }
            }
        };

    let now = Utc::now();

    // 对于 RSSHub 类型，提取路由路径
    let rsshub_route = if feed_type == brew_sources::FeedType::RssHub {
        // 优先使用前端传入的路由
        req.rsshub_route.clone().or_else(|| {
            // 否则从 URL 中提取
            Url::parse(url).ok().map(|u| u.path().to_string())
        })
    } else {
        None
    };

    let new_source = brew_sources::ActiveModel {
        user_id: Set(user_id),
        name: Set(name),
        url: Set(url.to_string()),
        feed_type: Set(feed_type),
        source_type: Set(source_type.clone()),
        category: Set(req.category),
        icon: Set(icon.clone()),
        description: Set(description),
        site_url: Set(site_url),
        update_interval: Set(if source_type == brew_sources::SourceType::Link {
            0
        } else {
            req.update_interval.unwrap_or(30)
        }),
        enabled: Set(true),
        error_count: Set(0),
        item_count: Set(0),
        unread_count: Set(0),
        extra_config: Set(extra_config),
        rsshub_route: Set(rsshub_route),
        admin_only: Set(req.admin_only.unwrap_or(false)),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        ..Default::default()
    };

    match new_source.insert(&db).await {
        Ok(source) => {
            // 下载图标到本地
            if let Some(icon_url) = &icon {
                let icon_service = IconService::new();
                if let Ok(Some(icon_info)) = icon_service.download_icon(source.id, icon_url).await {
                    // 更新图标为本地路径
                    let mut active: brew_sources::ActiveModel = source.clone().into();
                    active.icon = Set(Some(icon_info.local_path));
                    if let Err(e) = active.update(&db).await {
                        tracing::warn!(
                            "Failed to update icon path for source {}: {}",
                            source.id,
                            e
                        );
                    }
                }
            }

            // 触发首次抓取（纯链接类型不需要抓取）
            if source_type != brew_sources::SourceType::Link {
                if let Some(scheduler) = get_brew_scheduler() {
                    let _ = scheduler.refresh_source(source.id).await;
                }
            }

            // 重新获取最新的 source 数据
            let updated_source = brew_sources::Entity::find_by_id(source.id)
                .one(&db)
                .await
                .ok()
                .flatten()
                .unwrap_or(source);

            let response: brew_sources::SourceResponse = updated_source.into();
            (
                StatusCode::CREATED,
                Json(json!({ "success": true, "source": response })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// 获取单个订阅源（游客可访问；admin_only 源仅管理员可见）
async fn get_source(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    let (_, is_admin) = get_user_and_admin_status(&headers).await;

    let source = brew_sources::Entity::find_by_id(id).one(&db).await;

    match source {
        Ok(Some(source)) => {
            if source.admin_only && !is_admin {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "success": false, "error": "Source not found" })),
                )
                    .into_response();
            }
            let response: brew_sources::SourceResponse = source.into();
            (
                StatusCode::OK,
                Json(json!({ "success": true, "source": response })),
            )
                .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Source not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// 更新订阅源（需要管理员权限）
async fn update_source(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
    Json(req): Json<brew_sources::UpdateSourceRequest>,
) -> impl IntoResponse {
    // 更新订阅源需要管理员权限
    let user_id = match get_admin_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let source = brew_sources::Entity::find_by_id(id)
        .filter(brew_sources::Column::UserId.eq(user_id))
        .one(&db)
        .await;

    match source {
        Ok(Some(source)) => {
            let mut active: brew_sources::ActiveModel = source.into();

            if let Some(name) = req.name {
                active.name = Set(name);
            }
            if let Some(category) = req.category {
                active.category = Set(Some(category));
            }
            if let Some(interval) = req.update_interval {
                active.update_interval = Set(interval);
            }
            if let Some(enabled) = req.enabled {
                active.enabled = Set(enabled);
            }
            if let Some(card_size) = req.card_size {
                active.card_size = Set(Some(card_size));
            }
            if let Some(theme_color) = req.theme_color {
                // 支持空字符串清除主题色
                active.theme_color = Set(if theme_color.is_empty() {
                    None
                } else {
                    Some(theme_color)
                });
            }
            if let Some(icon) = req.icon {
                // 支持空字符串清除图标
                active.icon = Set(if icon.is_empty() { None } else { Some(icon) });
            }
            if let Some(sort_order) = req.sort_order {
                active.sort_order = Set(Some(sort_order));
            }
            if let Some(ref source_type_str) = req.source_type {
                let st = match source_type_str.as_str() {
                    "link" => brew_sources::SourceType::Link,
                    "brewlia" => brew_sources::SourceType::Brewlia,
                    _ => brew_sources::SourceType::Rss,
                };
                active.source_type = Set(st);
            }
            if let Some(ref feed_type_str) = req.feed_type {
                let ft = match feed_type_str.as_str() {
                    "notion" => brew_sources::FeedType::Notion,
                    "atom" => brew_sources::FeedType::Atom,
                    "json" => brew_sources::FeedType::JsonFeed,
                    _ => brew_sources::FeedType::Rss,
                };
                active.feed_type = Set(ft);
            }
            if let Some(ref extra_config) = req.extra_config {
                active.extra_config = Set(Some(extra_config.clone()));
            }
            // 处理 AI 风格标签（用户自定义或 AI 生成）
            if let Some(ref tags) = req.ai_style_tags {
                let tags_json = serde_json::to_value(tags).unwrap_or(serde_json::Value::Null);
                active.ai_style_tags = Set(if tags.is_empty() {
                    None
                } else {
                    Some(tags_json)
                });
            }
            // 处理仅管理员可见选项
            if let Some(admin_only) = req.admin_only {
                active.admin_only = Set(admin_only);
            }
            active.updated_at = Set(Utc::now().into());

            match active.update(&db).await {
                Ok(updated) => {
                    let response: brew_sources::SourceResponse = updated.into();
                    (
                        StatusCode::OK,
                        Json(json!({ "success": true, "source": response })),
                    )
                        .into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "success": false, "error": e.to_string() })),
                )
                    .into_response(),
            }
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Source not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// 删除订阅源（需要管理员权限）
async fn delete_source(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    // 删除订阅源需要管理员权限
    let user_id = match get_admin_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    // 验证所有权
    let source = brew_sources::Entity::find_by_id(id)
        .filter(brew_sources::Column::UserId.eq(user_id))
        .one(&db)
        .await;

    match source {
        Ok(Some(_)) => {
            // 删除本地缓存的图标
            let icon_service = IconService::new();
            if let Err(e) = icon_service.delete_icon(id).await {
                tracing::warn!("Failed to delete icon for source {}: {}", id, e);
            }

            // 删除订阅源（级联删除文章和状态）
            match brew_sources::Entity::delete_by_id(id).exec(&db).await {
                Ok(_) => (StatusCode::OK, Json(json!({ "success": true }))).into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "success": false, "error": e.to_string() })),
                )
                    .into_response(),
            }
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Source not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// 手动刷新订阅源（需要管理员权限）
async fn refresh_source(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    // 刷新订阅源需要管理员权限
    let user_id = match get_admin_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    // 验证所有权
    let source = brew_sources::Entity::find_by_id(id)
        .filter(brew_sources::Column::UserId.eq(user_id))
        .one(&db)
        .await;

    match source {
        Ok(Some(_)) => {
            if let Some(scheduler) = get_brew_scheduler() {
                match scheduler.refresh_source(id).await {
                    Ok(new_count) => (
                        StatusCode::OK,
                        Json(json!({ "success": true, "new_items": new_count })),
                    )
                        .into_response(),
                    Err(e) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "success": false, "error": e })),
                    )
                        .into_response(),
                }
            } else {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({ "success": false, "error": "Scheduler not available" })),
                )
                    .into_response()
            }
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Source not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// 探测订阅源信息
#[derive(Debug, Deserialize)]
struct DiscoverRequest {
    url: String,
}

const RSS_DISCOVERY_SUFFIXES: &[&str] = &[
    "feed",
    "feed/",
    "feed.xml",
    "rss",
    "rss/",
    "rss.xml",
    "atom.xml",
    "index.xml",
];

/// 生成 RSS 自动发现候选：原地址优先，再尝试当前路径和站点根路径。
fn build_feed_discovery_candidates(raw_url: &str) -> Result<Vec<String>, String> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return Err("URL is required".to_string());
    }

    let normalized = if Url::parse(trimmed).is_ok() {
        trimmed.to_string()
    } else if !trimmed.contains("://") {
        format!("https://{}", trimmed)
    } else {
        return Err("Invalid URL".to_string());
    };

    let parsed = Url::parse(&normalized).map_err(|_| "Invalid URL".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only HTTP and HTTPS URLs are supported".to_string());
    }

    let mut candidates = vec![parsed.to_string()];
    let mut bases = Vec::new();

    let mut path_base = parsed.clone();
    path_base.set_query(None);
    path_base.set_fragment(None);
    if !path_base.path().ends_with('/') {
        path_base.set_path(&format!("{}/", path_base.path()));
    }
    bases.push(path_base);

    let mut root_base = parsed;
    root_base.set_path("/");
    root_base.set_query(None);
    root_base.set_fragment(None);
    bases.push(root_base);

    for base in bases {
        for suffix in RSS_DISCOVERY_SUFFIXES {
            if let Ok(candidate) = base.join(suffix) {
                let candidate = candidate.to_string();
                if !candidates.contains(&candidate) {
                    candidates.push(candidate);
                }
            }
        }
    }

    Ok(candidates)
}

fn discover_success_response(
    requested_url: &str,
    discovered_url: String,
    feed: ParsedFeed,
) -> axum::response::Response {
    let autocompleted = requested_url != discovered_url;
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "feed": {
                "url": discovered_url,
                "autocompleted": autocompleted,
                "title": feed.title,
                "description": feed.description,
                "site_url": feed.site_url,
                "icon": feed.icon,
                "feed_type": match feed.feed_type {
                    brew_sources::FeedType::Rss => "rss",
                    brew_sources::FeedType::Atom => "atom",
                    brew_sources::FeedType::JsonFeed => "json_feed",
                    brew_sources::FeedType::Notion => "notion",
                    brew_sources::FeedType::RssHub => "rsshub",
                },
                "item_count": feed.items.len(),
            }
        })),
    )
        .into_response()
}

/// 探测订阅源信息（需管理员；出站经 FeedParser SSRF 防护）
async fn discover_source(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<DiscoverRequest>,
) -> impl IntoResponse {
    if let Err(resp) = get_admin_user_id_from_headers(&headers, &db).await {
        return resp;
    }

    let mut candidates = match build_feed_discovery_candidates(&req.url) {
        Ok(candidates) => candidates,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "success": false, "error": error })),
            )
                .into_response()
        }
    };
    let requested_url = candidates.remove(0);
    let parser = FeedParser::new();

    // 用户输入本身已经是 Feed 时立即返回，不额外请求候选地址。
    let direct_error = match parser.fetch_and_parse(&requested_url).await {
        Ok(feed) => return discover_success_response(&requested_url, requested_url.clone(), feed),
        Err(error) => error.to_string(),
    };

    // 常见后缀最多 4 个并发探测；每个请求仍经过 FeedParser 的 SSRF 防护。
    let mut attempts = futures::stream::iter(candidates.into_iter().map(|candidate| {
        let parser = &parser;
        async move {
            let result = parser.fetch_and_parse(&candidate).await;
            (candidate, result)
        }
    }))
    .buffer_unordered(4);

    while let Some((candidate, result)) = attempts.next().await {
        if let Ok(feed) = result {
            return discover_success_response(&requested_url, candidate, feed);
        }
    }

    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "success": false,
            "error": format!("Unable to discover RSS/Atom feed: {}", direct_error)
        })),
    )
        .into_response()
}

// ==================== OPML 导入导出 ====================

/// 导入 OPML
#[derive(Debug, Deserialize)]
struct ImportOpmlRequest {
    opml: String,
}

async fn import_opml(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ImportOpmlRequest>,
) -> impl IntoResponse {
    // 导入 OPML 需要管理员权限
    let user_id = match get_admin_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    // 解析 OPML
    let feeds = parse_opml(&req.opml);
    if feeds.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "No feeds found in OPML" })),
        )
            .into_response();
    }

    // 批量查询已存在的 URL（避免 N+1）
    let feed_urls: Vec<String> = feeds.iter().map(|f| f.url.clone()).collect();
    let existing_urls: std::collections::HashSet<String> = brew_sources::Entity::find()
        .filter(brew_sources::Column::UserId.eq(user_id))
        .filter(brew_sources::Column::Url.is_in(&feed_urls))
        .all(&db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.url)
        .collect();

    let now = Utc::now();

    // 收集需要插入的新订阅源
    let new_sources: Vec<brew_sources::ActiveModel> = feeds
        .into_iter()
        .filter(|feed| !existing_urls.contains(&feed.url))
        .map(|feed| brew_sources::ActiveModel {
            user_id: Set(user_id),
            name: Set(feed.title),
            url: Set(feed.url),
            feed_type: Set(brew_sources::FeedType::Rss),
            category: Set(feed.category),
            enabled: Set(true),
            error_count: Set(0),
            item_count: Set(0),
            unread_count: Set(0),
            update_interval: Set(30),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            ..Default::default()
        })
        .collect();

    let imported = new_sources.len();
    let skipped = feed_urls.len() - imported;

    // 批量插入新订阅源
    if !new_sources.is_empty() {
        if let Err(e) = brew_sources::Entity::insert_many(new_sources)
            .exec(&db)
            .await
        {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": format!("Failed to import: {}", e) })),
            )
                .into_response();
        }
    }

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "imported": imported,
            "skipped": skipped,
        })),
    )
        .into_response()
}

/// 导出 OPML（游客可访问；非管理员不导出 admin_only 源）
async fn export_opml(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let (_, is_admin) = get_user_and_admin_status(&headers).await;

    let mut query = brew_sources::Entity::find()
        .order_by_asc(brew_sources::Column::Category)
        .order_by_asc(brew_sources::Column::Name);

    if !is_admin {
        query = query.filter(brew_sources::Column::AdminOnly.eq(false));
    }

    let sources = query.all(&db).await;

    match sources {
        Ok(sources) => {
            let opml = generate_opml(&sources);
            (StatusCode::OK, [("Content-Type", "application/xml")], opml).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ==================== 分类管理 ====================

/// 获取分类列表（游客可访问）
async fn list_categories(
    State(db): State<DatabaseConnection>,
    _headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // 游客可访问，获取所有分类
    let categories = brew_categories::Entity::find()
        .order_by_asc(brew_categories::Column::SortOrder)
        .all(&db)
        .await;

    match categories {
        Ok(cats) => (
            StatusCode::OK,
            Json(json!({ "success": true, "categories": cats })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn create_category(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<brew_categories::CreateCategoryRequest>,
) -> impl IntoResponse {
    // 创建分类需要管理员权限
    let user_id = match get_admin_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let now = Utc::now();
    let new_cat = brew_categories::ActiveModel {
        user_id: Set(user_id),
        name: Set(req.name),
        icon: Set(req.icon),
        color: Set(req.color),
        sort_order: Set(0),
        created_at: Set(now.into()),
        ..Default::default()
    };

    match new_cat.insert(&db).await {
        Ok(cat) => (
            StatusCode::CREATED,
            Json(json!({ "success": true, "category": cat })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn update_category(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
    Json(req): Json<brew_categories::UpdateCategoryRequest>,
) -> impl IntoResponse {
    // 更新分类需要管理员权限
    let user_id = match get_admin_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let cat = brew_categories::Entity::find_by_id(id)
        .filter(brew_categories::Column::UserId.eq(user_id))
        .one(&db)
        .await;

    match cat {
        Ok(Some(cat)) => {
            let mut active: brew_categories::ActiveModel = cat.into();
            if let Some(name) = req.name {
                active.name = Set(name);
            }
            if let Some(icon) = req.icon {
                active.icon = Set(Some(icon));
            }
            if let Some(color) = req.color {
                active.color = Set(Some(color));
            }
            if let Some(order) = req.sort_order {
                active.sort_order = Set(order);
            }

            match active.update(&db).await {
                Ok(updated) => (
                    StatusCode::OK,
                    Json(json!({ "success": true, "category": updated })),
                )
                    .into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "success": false, "error": e.to_string() })),
                )
                    .into_response(),
            }
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Category not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn delete_category(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    // 删除分类需要管理员权限
    let user_id = match get_admin_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let cat = brew_categories::Entity::find_by_id(id)
        .filter(brew_categories::Column::UserId.eq(user_id))
        .one(&db)
        .await;

    match cat {
        Ok(Some(_)) => match brew_categories::Entity::delete_by_id(id).exec(&db).await {
            Ok(_) => (StatusCode::OK, Json(json!({ "success": true }))).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": e.to_string() })),
            )
                .into_response(),
        },
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Category not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ==================== 文章获取 ====================

/// 获取文章列表（游客可访问）
/// 游客不计算已读/收藏状态以节约计算
/// 非管理员看不到 admin_only 源下的文章
async fn list_items(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Query(query): Query<brew_items::ItemsQuery>,
) -> impl IntoResponse {
    // 获取可选用户 ID 与管理员状态
    let (user_id, is_admin) = get_user_and_admin_status(&headers).await;

    // 可见订阅源 ID（非管理员过滤 admin_only）
    let mut sources_q = brew_sources::Entity::find()
        .select_only()
        .column(brew_sources::Column::Id);
    if !is_admin {
        sources_q = sources_q.filter(brew_sources::Column::AdminOnly.eq(false));
    }
    let all_sources: Vec<i32> = sources_q.into_tuple().all(&db).await.unwrap_or_default();

    if all_sources.is_empty() {
        return (
            StatusCode::OK,
            Json(json!({ "success": true, "items": [], "total": 0, "page": 1, "per_page": 20 })),
        )
            .into_response();
    }

    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(20).min(100);
    let filter_type = query.filter.as_deref().unwrap_or("all");

    // 游客不支持 starred 和 unread 过滤（需要登录才能使用这些过滤）
    // 对于登录用户，处理 starred 和 unread 过滤
    let filtered_item_ids: Option<Vec<i32>> = if let Some(uid) = user_id {
        match filter_type {
            "starred" => {
                // 获取用户 starred 的 item_ids
                let ids: Vec<i32> = brew_user_states::Entity::find()
                    .filter(brew_user_states::Column::UserId.eq(uid))
                    .filter(brew_user_states::Column::IsStarred.eq(true))
                    .select_only()
                    .column(brew_user_states::Column::ItemId)
                    .into_tuple()
                    .all(&db)
                    .await
                    .unwrap_or_default();

                if ids.is_empty() {
                    return (
                        StatusCode::OK,
                        Json(json!({ "success": true, "items": [], "total": 0, "page": page, "per_page": per_page })),
                    )
                        .into_response();
                }
                Some(ids)
            }
            "unread" => {
                // 在后续 items_query 构建阶段使用子查询过滤，此处仅标记（返回 None）
                None
            }
            _ => None,
        }
    } else {
        // 游客不支持 starred/unread 过滤
        None
    };

    // 构建查询
    let mut items_query =
        brew_items::Entity::find().filter(brew_items::Column::SourceId.is_in(all_sources.clone()));

    // 只有登录用户才应用 starred/unread 过滤
    if user_id.is_some() {
        match filter_type {
            "starred" => {
                if let Some(ref ids) = filtered_item_ids {
                    items_query = items_query.filter(brew_items::Column::Id.is_in(ids.clone()));
                }
            }
            "unread" => {
                // 用子查询替代 NOT IN (ids)，避免已读文章数万条时生成巨型参数列表
                if let Some(uid) = user_id {
                    let read_subquery = brew_user_states::Entity::find()
                        .filter(brew_user_states::Column::UserId.eq(uid))
                        .filter(brew_user_states::Column::IsRead.eq(true))
                        .select_only()
                        .column(brew_user_states::Column::ItemId)
                        .into_query(); // QueryTrait::into_query() 消耗 Select 返回 SelectStatement
                    items_query =
                        items_query.filter(brew_items::Column::Id.not_in_subquery(read_subquery));
                }
            }
            _ => {}
        }
    }

    // 按订阅源筛选
    if let Some(source_id) = query.source_id {
        items_query = items_query.filter(brew_items::Column::SourceId.eq(source_id));
    }

    // 按分类筛选（支持多分类：category 字段可能是逗号分隔的多个分类）
    if let Some(ref category) = query.category {
        // 获取该分类下的订阅源
        // 使用 LIKE 匹配来支持多分类场景（如 "友情链接, 技术" 包含 "技术"）
        let cat_sources: Vec<i32> = brew_sources::Entity::find()
            .filter(
                sea_orm::Condition::any()
                    .add(brew_sources::Column::Category.eq(category.clone()))
                    .add(brew_sources::Column::Category.starts_with(format!("{}, ", category)))
                    .add(brew_sources::Column::Category.ends_with(format!(", {}", category)))
                    .add(brew_sources::Column::Category.contains(format!(", {}, ", category))),
            )
            .select_only()
            .column(brew_sources::Column::Id)
            .into_tuple()
            .all(&db)
            .await
            .unwrap_or_default();

        if cat_sources.is_empty() {
            return (
                StatusCode::OK,
                Json(json!({ "success": true, "items": [], "total": 0, "page": page, "per_page": per_page })),
            )
                .into_response();
        }
        items_query = items_query.filter(brew_items::Column::SourceId.is_in(cat_sources));
    }

    // 排序
    let sort_order = query.sort_order.as_deref().unwrap_or("desc");
    items_query = if sort_order == "asc" {
        items_query.order_by_asc(brew_items::Column::PublishedAt)
    } else {
        items_query.order_by_desc(brew_items::Column::PublishedAt)
    };

    // 获取总数
    let total = items_query.clone().count(&db).await.unwrap_or(0);

    // 分页
    let items = items_query
        .offset(((page - 1) * per_page) as u64)
        .limit(per_page as u64)
        .all(&db)
        .await;

    match items {
        Ok(items) => {
            let item_ids: Vec<i32> = items.iter().map(|i| i.id).collect();

            // 性能优化：并行执行多个独立查询
            let (states_result, sources_result, annotations_result, podcast_result) = tokio::join!(
                // 查询用户状态（仅登录用户）
                async {
                    if let Some(uid) = user_id {
                        brew_user_states::Entity::find()
                            .filter(brew_user_states::Column::UserId.eq(uid))
                            .filter(brew_user_states::Column::ItemId.is_in(item_ids.clone()))
                            .all(&db)
                            .await
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    }
                },
                // 查询订阅源信息
                brew_sources::Entity::find()
                    .filter(brew_sources::Column::Id.is_in(all_sources))
                    .all(&db),
                // 查询哪些文章有 AI 注释
                brew_annotations::Entity::find()
                    .filter(brew_annotations::Column::ItemId.is_in(item_ids.clone()))
                    .select_only()
                    .column(brew_annotations::Column::ItemId)
                    .distinct()
                    .into_tuple::<i32>()
                    .all(&db),
                // 查询哪些文章有 AI 播客
                brew_podcasts::Entity::find()
                    .filter(brew_podcasts::Column::ItemId.is_in(item_ids.clone()))
                    .select_only()
                    .column(brew_podcasts::Column::ItemId)
                    .into_tuple::<i32>()
                    .all(&db)
            );

            let states_map: std::collections::HashMap<i32, brew_user_states::Model> =
                states_result.into_iter().map(|s| (s.item_id, s)).collect();

            let sources_map: std::collections::HashMap<i32, brew_sources::Model> = sources_result
                .unwrap_or_default()
                .into_iter()
                .map(|s| (s.id, s))
                .collect();

            let items_with_annotations: std::collections::HashSet<i32> =
                annotations_result.unwrap_or_default().into_iter().collect();

            let items_with_podcast: std::collections::HashSet<i32> =
                podcast_result.unwrap_or_default().into_iter().collect();

            // 构建响应
            let response_items: Vec<brew_items::ItemResponse> = items
                .into_iter()
                .map(|item| {
                    // 游客所有文章都是未读、未收藏
                    let state = states_map.get(&item.id);
                    let is_read = user_id.is_some() && state.map(|s| s.is_read).unwrap_or(false);
                    let is_starred =
                        user_id.is_some() && state.map(|s| s.is_starred).unwrap_or(false);
                    let read_progress = if user_id.is_some() {
                        state.and_then(|s| s.read_progress)
                    } else {
                        None
                    };

                    let source = sources_map.get(&item.source_id);
                    let source_name = source.map(|s| s.name.clone());
                    let source_icon = source.and_then(|s| s.icon.clone());

                    // 获取 AI 状态
                    let has_ai_annotations = items_with_annotations.contains(&item.id);
                    let has_ai_podcast = items_with_podcast.contains(&item.id);

                    brew_items::ItemResponse::from_model_with_ai(
                        item,
                        source_name,
                        source_icon,
                        is_read,
                        is_starred,
                        read_progress,
                        has_ai_annotations,
                        has_ai_podcast,
                    )
                })
                .collect();

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "items": response_items,
                    "total": total,
                    "page": page,
                    "per_page": per_page,
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// 获取单篇文章详情（游客可访问）
/// 游客不查询已读/收藏状态以节约计算
/// 性能优化：并行查询 AI 状态
/// admin_only 源下的文章仅管理员可见
async fn get_item(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    // 获取可选用户 ID 与管理员状态
    let (user_id, is_admin) = get_user_and_admin_status(&headers).await;

    // 获取文章
    let item = brew_items::Entity::find_by_id(id).one(&db).await;

    match item {
        Ok(Some(item)) => {
            // 获取订阅源信息
            let source = brew_sources::Entity::find_by_id(item.source_id)
                .one(&db)
                .await;

            if let Ok(Some(source)) = source {
                if source.admin_only && !is_admin {
                    return (
                        StatusCode::NOT_FOUND,
                        Json(json!({ "success": false, "error": "Item not found" })),
                    )
                        .into_response();
                }
                // 只有登录用户才查询已读/收藏状态，游客跳过以节约计算
                let (is_read, is_starred, read_progress) = if let Some(uid) = user_id {
                    let state = brew_user_states::Entity::find()
                        .filter(brew_user_states::Column::UserId.eq(uid))
                        .filter(brew_user_states::Column::ItemId.eq(id))
                        .one(&db)
                        .await
                        .ok()
                        .flatten();

                    (
                        state.as_ref().map(|s| s.is_read).unwrap_or(false),
                        state.as_ref().map(|s| s.is_starred).unwrap_or(false),
                        state.and_then(|s| s.read_progress),
                    )
                } else {
                    // 游客不需要查询状态
                    (false, false, None)
                };

                // 性能优化：并行查询 AI 状态（注释和播客）
                let (annotations_result, podcast_result) = tokio::join!(
                    brew_annotations::Entity::find()
                        .filter(brew_annotations::Column::ItemId.eq(id))
                        .count(&db),
                    brew_podcasts::Entity::find()
                        .filter(brew_podcasts::Column::ItemId.eq(id))
                        .count(&db)
                );

                let has_ai_annotations = annotations_result.unwrap_or(0) > 0;
                let has_ai_podcast = podcast_result.unwrap_or(0) > 0;

                let response = brew_items::ItemResponse::from_model_with_ai(
                    item,
                    Some(source.name),
                    source.icon,
                    is_read,
                    is_starred,
                    read_progress,
                    has_ai_annotations,
                    has_ai_podcast,
                );

                (
                    StatusCode::OK,
                    Json(json!({ "success": true, "item": response })),
                )
                    .into_response()
            } else {
                (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "success": false, "error": "Item not found" })),
                )
                    .into_response()
            }
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Item not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn fetch_fulltext(
    State(_db): State<DatabaseConnection>,
    _headers: axum::http::HeaderMap,
    Path(_id): Path<i32>,
) -> impl IntoResponse {
    // TODO: 实现全文抓取
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "success": false, "error": "Fulltext fetching not yet implemented" })),
    )
        .into_response()
}

// ==================== 阅读状态 ====================

async fn mark_read(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(item_id): Path<i32>,
) -> impl IntoResponse {
    update_item_state(&db, &headers, item_id, Some(true), None).await
}

async fn mark_unread(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(item_id): Path<i32>,
) -> impl IntoResponse {
    update_item_state(&db, &headers, item_id, Some(false), None).await
}

async fn star_item(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(item_id): Path<i32>,
) -> impl IntoResponse {
    update_item_state(&db, &headers, item_id, None, Some(true)).await
}

async fn unstar_item(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(item_id): Path<i32>,
) -> impl IntoResponse {
    update_item_state(&db, &headers, item_id, None, Some(false)).await
}

async fn update_item_state(
    db: &DatabaseConnection,
    headers: &axum::http::HeaderMap,
    item_id: i32,
    is_read: Option<bool>,
    is_starred: Option<bool>,
) -> impl IntoResponse {
    let user_id = match get_user_id_from_headers(headers, db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let now = Utc::now();

    // 性能优化：并行查询文章信息和用户状态
    let (item_result, existing) = tokio::join!(
        brew_items::Entity::find_by_id(item_id).one(db),
        brew_user_states::Entity::find()
            .filter(brew_user_states::Column::UserId.eq(user_id))
            .filter(brew_user_states::Column::ItemId.eq(item_id))
            .one(db)
    );

    // 获取文章所属的 source_id（用于更新 unread_count）
    let item = match item_result {
        Ok(Some(item)) => item,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "success": false, "error": "Item not found" })),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": e.to_string() })),
            )
                .into_response();
        }
    };
    let source_id = item.source_id;

    // 记录之前的已读状态，用于计算 unread_count 变化
    let was_read = match &existing {
        Ok(Some(state)) => state.is_read,
        _ => false,
    };

    match existing {
        Ok(Some(state)) => {
            let mut active: brew_user_states::ActiveModel = state.into();

            if let Some(read) = is_read {
                active.is_read = Set(read);
                if read {
                    active.read_at = Set(Some(now.into()));
                }
            }
            if let Some(starred) = is_starred {
                active.is_starred = Set(starred);
                if starred {
                    active.starred_at = Set(Some(now.into()));
                }
            }
            active.updated_at = Set(now.into());

            match active.update(db).await {
                Ok(_) => {
                    // 更新 source 的 unread_count
                    if let Some(read) = is_read {
                        if read != was_read {
                            let _ = update_source_unread_count(
                                db,
                                source_id,
                                if read { -1 } else { 1 },
                            )
                            .await;
                        }
                    }
                    (StatusCode::OK, Json(json!({ "success": true }))).into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "success": false, "error": e.to_string() })),
                )
                    .into_response(),
            }
        }
        Ok(None) => {
            // 创建新记录
            let new_state = brew_user_states::ActiveModel {
                user_id: Set(user_id),
                item_id: Set(item_id),
                is_read: Set(is_read.unwrap_or(false)),
                is_starred: Set(is_starred.unwrap_or(false)),
                read_at: Set(if is_read == Some(true) {
                    Some(now.into())
                } else {
                    None
                }),
                starred_at: Set(if is_starred == Some(true) {
                    Some(now.into())
                } else {
                    None
                }),
                updated_at: Set(now.into()),
                ..Default::default()
            };

            match new_state.insert(db).await {
                Ok(_) => {
                    // 新记录：如果标记为已读，减少 unread_count
                    if is_read == Some(true) {
                        let _ = update_source_unread_count(db, source_id, -1).await;
                    }
                    (StatusCode::OK, Json(json!({ "success": true }))).into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "success": false, "error": e.to_string() })),
                )
                    .into_response(),
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// 更新订阅源的未读计数
/// 使用原子 SQL 避免并发读改写竞态（两个请求同时读取相同值后各自写回导致数据丢失）
async fn update_source_unread_count(
    db: &DatabaseConnection,
    source_id: i32,
    delta: i32,
) -> Result<(), sea_orm::DbErr> {
    let stmt = Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE brew_sources SET unread_count = GREATEST(0, unread_count + $1) WHERE id = $2",
        [delta.into(), source_id.into()],
    );
    db.execute(stmt).await?;
    Ok(())
}

async fn mark_all_read(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<brew_user_states::MarkAllReadRequest>,
) -> impl IntoResponse {
    let user_id = match get_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    // 共享订阅库：按当前用户可见源标记，而非「我创建的源」
    let (_, is_admin) = get_user_and_admin_status(&headers).await;

    // 获取要标记的文章
    let mut query = brew_items::Entity::find();

    // 可见订阅源（非管理员排除 admin_only）
    let mut source_filter = brew_sources::Entity::find();
    if !is_admin {
        source_filter = source_filter.filter(brew_sources::Column::AdminOnly.eq(false));
    }

    if let Some(source_id) = req.source_id {
        source_filter = source_filter.filter(brew_sources::Column::Id.eq(source_id));
    }
    if let Some(ref category) = req.category {
        source_filter = source_filter.filter(brew_sources::Column::Category.eq(category));
    }

    let sources: Vec<brew_sources::Model> = source_filter.all(&db).await.unwrap_or_default();

    let source_ids: Vec<i32> = sources.iter().map(|s| s.id).collect();

    if source_ids.is_empty() {
        return (
            StatusCode::OK,
            Json(json!({ "success": true, "marked": 0 })),
        )
            .into_response();
    }

    query = query.filter(brew_items::Column::SourceId.is_in(source_ids.clone()));

    if let Some(before) = req.before {
        if let Some(dt) = chrono::DateTime::from_timestamp_millis(before) {
            query = query.filter(brew_items::Column::PublishedAt.lt(dt));
        }
    }

    // 获取文章 ID 和对应的 source_id
    let items: Vec<(i32, i32)> = query
        .select_only()
        .column(brew_items::Column::Id)
        .column(brew_items::Column::SourceId)
        .into_tuple()
        .all(&db)
        .await
        .unwrap_or_default();

    if items.is_empty() {
        return (
            StatusCode::OK,
            Json(json!({ "success": true, "marked": 0 })),
        )
            .into_response();
    }

    let now = Utc::now();

    // 收集所有 item_id
    let item_ids: Vec<i32> = items.iter().map(|(id, _)| *id).collect();

    // 构建 item_id -> source_id 映射
    let item_source_map: std::collections::HashMap<i32, i32> = items.into_iter().collect();

    // 批量查询所有已存在的状态
    let existing_states: Vec<brew_user_states::Model> = brew_user_states::Entity::find()
        .filter(brew_user_states::Column::UserId.eq(user_id))
        .filter(brew_user_states::Column::ItemId.is_in(item_ids.clone()))
        .all(&db)
        .await
        .unwrap_or_default();

    let existing_item_ids: std::collections::HashSet<i32> =
        existing_states.iter().map(|s| s.item_id).collect();

    // 找出需要更新的（已存在但未读的）
    let unread_state_ids: Vec<i32> = existing_states
        .iter()
        .filter(|s| !s.is_read)
        .map(|s| s.id)
        .collect();

    // 找出需要插入的（不存在的）
    let missing_item_ids: Vec<i32> = item_ids
        .iter()
        .filter(|id| !existing_item_ids.contains(id))
        .copied()
        .collect();

    let mut marked = 0;
    let mut source_marked_counts: std::collections::HashMap<i32, i32> =
        std::collections::HashMap::new();

    // 批量更新已存在的未读状态
    if !unread_state_ids.is_empty() {
        let update_result = brew_user_states::Entity::update_many()
            .col_expr(brew_user_states::Column::IsRead, Expr::value(true))
            .col_expr(brew_user_states::Column::ReadAt, Expr::value(now))
            .col_expr(brew_user_states::Column::UpdatedAt, Expr::value(now))
            .filter(brew_user_states::Column::Id.is_in(unread_state_ids))
            .exec(&db)
            .await;

        if let Ok(result) = update_result {
            marked += result.rows_affected as i32;
            // 统计每个 source 被标记的数量
            for state in existing_states.iter().filter(|s| !s.is_read) {
                if let Some(&source_id) = item_source_map.get(&state.item_id) {
                    *source_marked_counts.entry(source_id).or_insert(0) += 1;
                }
            }
        }
    }

    // 批量插入不存在的状态
    if !missing_item_ids.is_empty() {
        let new_states: Vec<brew_user_states::ActiveModel> = missing_item_ids
            .iter()
            .map(|&item_id| brew_user_states::ActiveModel {
                user_id: Set(user_id),
                item_id: Set(item_id),
                is_read: Set(true),
                is_starred: Set(false),
                read_at: Set(Some(now.into())),
                updated_at: Set(now.into()),
                ..Default::default()
            })
            .collect();

        let insert_count = new_states.len() as i32;
        if brew_user_states::Entity::insert_many(new_states)
            .exec(&db)
            .await
            .is_ok()
        {
            marked += insert_count;
            // 统计每个 source 被标记的数量
            for &item_id in &missing_item_ids {
                if let Some(&source_id) = item_source_map.get(&item_id) {
                    *source_marked_counts.entry(source_id).or_insert(0) += 1;
                }
            }
        }
    }

    // 更新每个 source 的 unread_count
    for (source_id, count) in source_marked_counts {
        let _ = update_source_unread_count(&db, source_id, -count).await;
    }

    (
        StatusCode::OK,
        Json(json!({ "success": true, "marked": marked })),
    )
        .into_response()
}

// ==================== 离线同步 ====================

async fn sync_states(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<brew_user_states::SyncStatesRequest>,
) -> impl IntoResponse {
    let user_id = match get_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let now = Utc::now();
    let mut synced = 0;
    let mut conflicts = Vec::new();
    // source_id -> unread_count delta（与 update_item_state 一致维护全局缓存列）
    let mut source_unread_deltas: std::collections::HashMap<i32, i32> =
        std::collections::HashMap::new();

    // 性能优化：批量查询所有相关状态与文章 source_id，避免 N+1
    let item_ids: Vec<i32> = req.states.iter().map(|s| s.item_id).collect();
    let existing_states: std::collections::HashMap<i32, brew_user_states::Model> =
        if !item_ids.is_empty() {
            brew_user_states::Entity::find()
                .filter(brew_user_states::Column::UserId.eq(user_id))
                .filter(brew_user_states::Column::ItemId.is_in(item_ids.clone()))
                .all(&db)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|s| (s.item_id, s))
                .collect()
        } else {
            std::collections::HashMap::new()
        };

    let item_source_map: std::collections::HashMap<i32, i32> = if !item_ids.is_empty() {
        brew_items::Entity::find()
            .filter(brew_items::Column::Id.is_in(item_ids))
            .select_only()
            .column(brew_items::Column::Id)
            .column(brew_items::Column::SourceId)
            .into_tuple::<(i32, i32)>()
            .all(&db)
            .await
            .unwrap_or_default()
            .into_iter()
            .collect()
    } else {
        std::collections::HashMap::new()
    };

    for state_item in req.states {
        if let Some(server_state) = existing_states.get(&state_item.item_id) {
            let server_ts = server_state.updated_at.timestamp_millis();

            // 检查冲突：服务器更新时间比客户端新
            if server_ts > state_item.updated_at {
                conflicts.push(brew_user_states::SyncConflict {
                    item_id: state_item.item_id,
                    server_updated_at: server_ts,
                    client_updated_at: state_item.updated_at,
                });
                continue;
            }

            let was_read = server_state.is_read;

            // 应用客户端更新
            let mut active: brew_user_states::ActiveModel = server_state.clone().into();
            if let Some(is_read) = state_item.is_read {
                active.is_read = Set(is_read);
                if is_read {
                    active.read_at = Set(Some(now.into()));
                }
            }
            if let Some(is_starred) = state_item.is_starred {
                active.is_starred = Set(is_starred);
                if is_starred {
                    active.starred_at = Set(Some(now.into()));
                }
            }
            if let Some(progress) = state_item.read_progress {
                active.read_progress = Set(Some(progress));
            }
            active.updated_at = Set(now.into());

            if active.update(&db).await.is_ok() {
                synced += 1;
                if let Some(is_read) = state_item.is_read {
                    if is_read != was_read {
                        if let Some(&source_id) = item_source_map.get(&state_item.item_id) {
                            let delta = if is_read { -1 } else { 1 };
                            *source_unread_deltas.entry(source_id).or_insert(0) += delta;
                        }
                    }
                }
            }
        } else {
            // 跳过不存在的文章，避免为任意 item_id 写状态
            if !item_source_map.contains_key(&state_item.item_id) {
                continue;
            }

            let is_read = state_item.is_read.unwrap_or(false);
            // 创建新记录
            let new_state = brew_user_states::ActiveModel {
                user_id: Set(user_id),
                item_id: Set(state_item.item_id),
                is_read: Set(is_read),
                is_starred: Set(state_item.is_starred.unwrap_or(false)),
                read_progress: Set(state_item.read_progress),
                read_at: Set(if is_read { Some(now.into()) } else { None }),
                starred_at: Set(if state_item.is_starred == Some(true) {
                    Some(now.into())
                } else {
                    None
                }),
                updated_at: Set(now.into()),
                ..Default::default()
            };
            if new_state.insert(&db).await.is_ok() {
                synced += 1;
                if is_read {
                    if let Some(&source_id) = item_source_map.get(&state_item.item_id) {
                        *source_unread_deltas.entry(source_id).or_insert(0) -= 1;
                    }
                }
            }
        }
    }

    for (source_id, delta) in source_unread_deltas {
        if delta != 0 {
            let _ = update_source_unread_count(&db, source_id, delta).await;
        }
    }

    (
        StatusCode::OK,
        Json(json!(brew_user_states::SyncStatesResponse {
            synced,
            conflicts
        })),
    )
        .into_response()
}

// ==================== 统计信息 ====================

/// 获取统计信息（游客可访问）
/// 游客不计算已读/收藏统计以节约计算
async fn get_stats(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // 获取可选用户 ID 与管理员状态
    let (user_id, is_admin) = get_user_and_admin_status(&headers).await;

    // 可见订阅源统计（非管理员排除 admin_only）
    let mut sources_q = brew_sources::Entity::find();
    if !is_admin {
        sources_q = sources_q.filter(brew_sources::Column::AdminOnly.eq(false));
    }
    let sources = sources_q.all(&db).await.unwrap_or_default();

    let total_sources = sources.len();
    let total_items: i32 = sources.iter().map(|s| s.item_count).sum();

    // 只有登录用户才计算未读数和收藏数，游客跳过以节约计算
    let (total_unread, starred_count) = if let Some(uid) = user_id {
        let source_ids: Vec<i32> = sources.iter().map(|s| s.id).collect();

        // 获取所有文章 ID
        let all_item_ids: Vec<i32> = if !source_ids.is_empty() {
            brew_items::Entity::find()
                .filter(brew_items::Column::SourceId.is_in(source_ids))
                .select_only()
                .column(brew_items::Column::Id)
                .into_tuple()
                .all(&db)
                .await
                .unwrap_or_default()
        } else {
            vec![]
        };

        // 性能优化：并行查询已读数和收藏数
        let (read_count, starred) = if !all_item_ids.is_empty() {
            tokio::join!(
                // 获取已读文章数
                brew_user_states::Entity::find()
                    .filter(brew_user_states::Column::UserId.eq(uid))
                    .filter(brew_user_states::Column::ItemId.is_in(all_item_ids.clone()))
                    .filter(brew_user_states::Column::IsRead.eq(true))
                    .count(&db),
                // 获取收藏数
                brew_user_states::Entity::find()
                    .filter(brew_user_states::Column::UserId.eq(uid))
                    .filter(brew_user_states::Column::IsStarred.eq(true))
                    .count(&db)
            )
        } else {
            // 空列表时仍需查询收藏数
            let starred = brew_user_states::Entity::find()
                .filter(brew_user_states::Column::UserId.eq(uid))
                .filter(brew_user_states::Column::IsStarred.eq(true))
                .count(&db)
                .await;
            (Ok(0), starred)
        };

        let unread = (all_item_ids.len() as i32) - read_count.unwrap_or(0) as i32;
        (unread, starred.unwrap_or(0) as i64)
    } else {
        // 游客不计算未读和收藏
        (0, 0)
    };

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "stats": {
                "total_sources": total_sources,
                "total_items": total_items,
                "total_unread": total_unread,
                "total_starred": starred_count,
            }
        })),
    )
        .into_response()
}

// ==================== WebSocket ====================

async fn brew_websocket(
    ws: axum::extract::ws::WebSocketUpgrade,
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<impl IntoResponse, StatusCode> {
    let user_id = claims
        .sub
        .parse::<i32>()
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    Ok(ws.on_upgrade(move |socket| handle_brew_websocket(socket, db, user_id)))
}

async fn handle_brew_websocket(
    mut socket: axum::extract::ws::WebSocket,
    _db: DatabaseConnection,
    user_id: i32,
) {
    use axum::extract::ws::Message;

    // 订阅通知
    if let Some(scheduler) = get_brew_scheduler() {
        let mut rx = scheduler.subscribe_notifications();

        loop {
            tokio::select! {
                // 接收来自调度器的通知
                Ok(notification) = rx.recv() => {
                    if notification.user_id != user_id {
                        continue;
                    }
                    let msg = serde_json::to_string(&notification).unwrap_or_default();
                    if socket.send(Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                }
                // 接收来自客户端的消息（心跳等）
                Some(msg) = socket.recv() => {
                    match msg {
                        Ok(Message::Ping(data)) => {
                            if socket.send(Message::Pong(data)).await.is_err() {
                                break;
                            }
                        }
                        Ok(Message::Close(_)) => break,
                        Err(_) => break,
                        _ => {}
                    }
                }
                else => break,
            }
        }
    }
}

// ==================== 辅助函数 ====================

/// 从请求头获取用户 ID
async fn get_user_id_from_headers(
    headers: &axum::http::HeaderMap,
    _db: &DatabaseConnection,
) -> Result<i32, axum::response::Response> {
    match verify_jwt_token(headers) {
        Ok(claims) => {
            // 从 claims.sub (String) 解析为 i32
            claims.sub.parse::<i32>().map_err(|_| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "success": false, "error": "Invalid user ID" })),
                )
                    .into_response()
            })
        }
        Err(_) => Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "success": false, "error": "Unauthorized" })),
        )
            .into_response()),
    }
}

/// 从请求头获取可选用户 ID（用于游客访问）
/// 游客返回 None，登录用户返回 Some(user_id)
fn get_optional_user_id_from_headers(headers: &axum::http::HeaderMap) -> Option<i32> {
    verify_jwt_token(headers)
        .ok()
        .and_then(|claims| claims.sub.parse::<i32>().ok())
}

/// 检查请求头中的用户是否为管理员
/// 返回 (Option<user_id>, is_admin)
async fn get_user_and_admin_status(headers: &axum::http::HeaderMap) -> (Option<i32>, bool) {
    match verify_jwt_token(headers) {
        Ok(claims) => {
            let user_id = claims.sub.parse::<i32>().ok();
            let is_admin = ensure_current_admin(&claims).await.is_ok();
            (user_id, is_admin)
        }
        Err(_) => (None, false),
    }
}

/// 从请求头获取管理员用户 ID（用于管理功能）
/// 非管理员返回 403 Forbidden
async fn get_admin_user_id_from_headers(
    headers: &axum::http::HeaderMap,
    _db: &DatabaseConnection,
) -> Result<i32, axum::response::Response> {
    let claims = verify_current_admin_from_headers(headers)
        .await
        .map_err(|(status, body)| (status, body).into_response())?;

    claims.sub.parse::<i32>().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "success": false, "error": "Invalid user ID" })),
        )
            .into_response()
    })
}

/// 解析 OPML 文件
fn parse_opml(opml: &str) -> Vec<OpmlFeed> {
    let mut feeds = Vec::new();

    // 简单解析，提取 outline 元素
    let re = regex::Regex::new(
        r#"<outline[^>]*text=["']([^"']+)["'][^>]*xmlUrl=["']([^"']+)["'][^>]*/?"#,
    )
    .ok();

    if let Some(re) = re {
        for caps in re.captures_iter(opml) {
            if let (Some(title), Some(url)) = (caps.get(1), caps.get(2)) {
                feeds.push(OpmlFeed {
                    title: title.as_str().to_string(),
                    url: url.as_str().to_string(),
                    category: None, // TODO: 解析分类
                });
            }
        }
    }

    // 也尝试 xmlUrl 在前的格式
    let re2 = regex::Regex::new(
        r#"<outline[^>]*xmlUrl=["']([^"']+)["'][^>]*text=["']([^"']+)["'][^>]*/?"#,
    )
    .ok();

    if let Some(re) = re2 {
        for caps in re.captures_iter(opml) {
            if let (Some(url), Some(title)) = (caps.get(1), caps.get(2)) {
                // 避免重复
                let url_str = url.as_str().to_string();
                if !feeds.iter().any(|f| f.url == url_str) {
                    feeds.push(OpmlFeed {
                        title: title.as_str().to_string(),
                        url: url_str,
                        category: None,
                    });
                }
            }
        }
    }

    feeds
}

struct OpmlFeed {
    title: String,
    url: String,
    category: Option<String>,
}

/// 生成 OPML 文件
fn generate_opml(sources: &[brew_sources::Model]) -> String {
    let mut opml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
  <head>
    <title>Myriad Brew 订阅导出</title>
  </head>
  <body>
"#,
    );

    // 按分类分组
    let mut by_category: std::collections::HashMap<String, Vec<&brew_sources::Model>> =
        std::collections::HashMap::new();

    for source in sources {
        let cat = source
            .category
            .clone()
            .unwrap_or_else(|| "未分类".to_string());
        by_category.entry(cat).or_default().push(source);
    }

    for (category, sources) in by_category {
        opml.push_str(&format!(
            r#"    <outline text="{}" title="{}">"#,
            escape_xml(&category),
            escape_xml(&category)
        ));
        opml.push('\n');

        for source in sources {
            opml.push_str(&format!(
                r#"      <outline type="rss" text="{}" title="{}" xmlUrl="{}"{}/>
"#,
                escape_xml(&source.name),
                escape_xml(&source.name),
                escape_xml(&source.url),
                source
                    .site_url
                    .as_ref()
                    .map(|u| format!(r#" htmlUrl="{}""#, escape_xml(u)))
                    .unwrap_or_default()
            ));
        }

        opml.push_str("    </outline>\n");
    }

    opml.push_str(
        r#"  </body>
</opml>"#,
    );

    opml
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// ==================== 用户评论（批注）====================

/// 获取文章的用户评论列表
/// 登录用户可以看到自己的评论
/// 性能优化：批量查询回复数量和用户信息，避免 N+1 问题
async fn list_comments(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(item_id): Path<i32>,
) -> impl IntoResponse {
    // 获取可选用户 ID（游客为 None）
    let user_id = get_optional_user_id_from_headers(&headers);

    // 游客无法查看评论
    let uid = match user_id {
        Some(id) => id,
        None => {
            return (
                StatusCode::OK,
                Json(json!({ "success": true, "comments": [], "has_comments": false })),
            )
                .into_response();
        }
    };

    // 获取用户在该文章的顶级评论（parent_id 为 NULL）
    let comments = brew_comments::Entity::find()
        .filter(brew_comments::Column::ItemId.eq(item_id))
        .filter(brew_comments::Column::UserId.eq(uid))
        .filter(brew_comments::Column::ParentId.is_null())
        .order_by_asc(brew_comments::Column::StartOffset)
        .all(&db)
        .await;

    match comments {
        Ok(comments) => {
            let has_comments = !comments.is_empty();

            if comments.is_empty() {
                return (
                    StatusCode::OK,
                    Json(json!({ "success": true, "comments": [], "has_comments": false })),
                )
                    .into_response();
            }

            // 性能优化：批量查询所有评论的回复数量
            let comment_ids: Vec<i32> = comments.iter().map(|c| c.id).collect();
            let reply_counts: std::collections::HashMap<i32, i32> = brew_comments::Entity::find()
                .filter(brew_comments::Column::ParentId.is_in(comment_ids.clone()))
                .select_only()
                .column(brew_comments::Column::ParentId)
                .column_as(brew_comments::Column::Id.count(), "count")
                .group_by(brew_comments::Column::ParentId)
                .into_tuple::<(i32, i64)>()
                .all(&db)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|(parent_id, count)| (parent_id, count as i32))
                .collect();

            // 性能优化：由于所有评论都属于同一用户，只需查询一次用户信息
            let user_info = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT username, display_name, avatar_url FROM users WHERE id = $1",
                    vec![SeaValue::Int(Some(uid))],
                ))
                .await
                .ok()
                .flatten();

            let (user_name, user_display_name, user_avatar) = user_info
                .map(|row| {
                    (
                        row.try_get::<String>("", "username").ok(),
                        row.try_get::<String>("", "display_name").ok(),
                        row.try_get::<String>("", "avatar_url").ok(),
                    )
                })
                .unwrap_or((None, None, None));

            // 构建响应
            let responses: Vec<brew_comments::CommentResponse> = comments
                .into_iter()
                .map(|comment| {
                    let mut response: brew_comments::CommentResponse = comment.clone().into();
                    response.reply_count = Some(*reply_counts.get(&comment.id).unwrap_or(&0));
                    response.user_name = user_name.clone();
                    response.user_display_name = user_display_name.clone();
                    response.user_avatar = user_avatar.clone();
                    response
                })
                .collect();

            (
                StatusCode::OK,
                Json(
                    json!({ "success": true, "comments": responses, "has_comments": has_comments }),
                ),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// 创建评论
/// 仅登录用户可用
async fn create_comment(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(item_id): Path<i32>,
    Json(req): Json<brew_comments::CreateCommentRequest>,
) -> impl IntoResponse {
    // 验证用户身份
    let user_id = match get_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    // 验证文章是否存在且对当前用户可见（admin_only 源需管理员）
    let (_, is_admin) = get_user_and_admin_status(&headers).await;
    let item_visible = match brew_items::Entity::find_by_id(item_id).one(&db).await {
        Ok(Some(item)) => match brew_sources::Entity::find_by_id(item.source_id)
            .one(&db)
            .await
        {
            Ok(Some(source)) => !source.admin_only || is_admin,
            _ => false,
        },
        _ => false,
    };

    if !item_visible {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Article not found" })),
        )
            .into_response();
    }

    // 如果是回复，验证父评论属于同一用户、同一文章
    if let Some(parent_id) = req.parent_id {
        let parent_exists = brew_comments::Entity::find_by_id(parent_id)
            .filter(brew_comments::Column::ItemId.eq(item_id))
            .filter(brew_comments::Column::UserId.eq(user_id))
            .one(&db)
            .await
            .ok()
            .flatten()
            .is_some();

        if !parent_exists {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "success": false, "error": "Parent comment not found" })),
            )
                .into_response();
        }
    }

    // 验证 color 格式（仅允许十六进制颜色）
    let validated_color = req.color.and_then(|c| {
        let color_regex =
            regex::Regex::new(r"^#([0-9A-Fa-f]{3}|[0-9A-Fa-f]{6}|[0-9A-Fa-f]{8})$").ok()?;
        if color_regex.is_match(&c) {
            Some(c)
        } else {
            None
        }
    });

    // 验证输入长度限制
    if req.comment.len() > 2000 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "Comment too long (max 2000 chars)" })),
        )
            .into_response();
    }
    if req.selected_text.len() > 5000 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "Selected text too long (max 5000 chars)" })),
        )
            .into_response();
    }

    let now = Utc::now();
    let new_comment = brew_comments::ActiveModel {
        item_id: Set(item_id),
        user_id: Set(user_id),
        selected_text: Set(req.selected_text),
        comment: Set(req.comment),
        start_offset: Set(req.start_offset),
        end_offset: Set(req.end_offset),
        context_before: Set(req.context_before),
        context_after: Set(req.context_after),
        color: Set(validated_color),
        is_public: Set(false),
        parent_id: Set(req.parent_id),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        ..Default::default()
    };

    match new_comment.insert(&db).await {
        Ok(comment) => {
            let mut response: brew_comments::CommentResponse = comment.clone().into();

            // 查询用户信息
            if let Ok(Some(user_row)) = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT username, display_name, avatar_url FROM users WHERE id = $1",
                    vec![SeaValue::Int(Some(comment.user_id))],
                ))
                .await
            {
                response.user_name = user_row.try_get::<String>("", "username").ok();
                response.user_display_name = user_row.try_get::<String>("", "display_name").ok();
                response.user_avatar = user_row.try_get::<String>("", "avatar_url").ok();
            }

            (
                StatusCode::CREATED,
                Json(json!({ "success": true, "comment": response })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// 更新评论
/// 仅评论作者可用
async fn update_comment(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(comment_id): Path<i32>,
    Json(req): Json<brew_comments::UpdateCommentRequest>,
) -> impl IntoResponse {
    // 验证用户身份
    let user_id = match get_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    // 获取评论并验证所有权
    let comment = brew_comments::Entity::find_by_id(comment_id)
        .filter(brew_comments::Column::UserId.eq(user_id))
        .one(&db)
        .await;

    match comment {
        Ok(Some(comment)) => {
            let mut active: brew_comments::ActiveModel = comment.clone().into();

            if let Some(comment_text) = req.comment {
                if comment_text.len() > 2000 {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "success": false, "error": "Comment too long (max 2000 chars)" })),
                    )
                        .into_response();
                }
                active.comment = Set(comment_text);
            }
            if let Some(color) = req.color {
                // 验证 color 格式（仅允许十六进制颜色）
                let color_regex =
                    regex::Regex::new(r"^#([0-9A-Fa-f]{3}|[0-9A-Fa-f]{6}|[0-9A-Fa-f]{8})$").ok();
                if color_regex.is_some_and(|r| r.is_match(&color)) {
                    active.color = Set(Some(color));
                }
            }
            active.updated_at = Set(Utc::now().into());

            match active.update(&db).await {
                Ok(updated) => {
                    let mut response: brew_comments::CommentResponse = updated.clone().into();

                    // 查询用户信息
                    if let Ok(Some(user_row)) = db
                        .query_one(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            "SELECT username, display_name, avatar_url FROM users WHERE id = $1",
                            vec![SeaValue::Int(Some(updated.user_id))],
                        ))
                        .await
                    {
                        response.user_name = user_row.try_get::<String>("", "username").ok();
                        response.user_display_name =
                            user_row.try_get::<String>("", "display_name").ok();
                        response.user_avatar = user_row.try_get::<String>("", "avatar_url").ok();
                    }

                    (
                        StatusCode::OK,
                        Json(json!({ "success": true, "comment": response })),
                    )
                        .into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "success": false, "error": e.to_string() })),
                )
                    .into_response(),
            }
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Comment not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// 删除评论
/// 仅评论作者可用
async fn delete_comment(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(comment_id): Path<i32>,
) -> impl IntoResponse {
    // 验证用户身份
    let user_id = match get_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    // 获取评论并验证所有权
    let comment = brew_comments::Entity::find_by_id(comment_id)
        .filter(brew_comments::Column::UserId.eq(user_id))
        .one(&db)
        .await;

    match comment {
        Ok(Some(_)) => {
            match brew_comments::Entity::delete_by_id(comment_id)
                .exec(&db)
                .await
            {
                Ok(_) => (StatusCode::OK, Json(json!({ "success": true }))).into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "success": false, "error": e.to_string() })),
                )
                    .into_response(),
            }
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "success": false, "error": "Comment not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// 获取评论的回复列表
/// 性能优化：由于所有回复都属于同一用户，只查询一次用户信息
async fn list_comment_replies(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(comment_id): Path<i32>,
) -> impl IntoResponse {
    // 验证用户身份（可选，用于获取用户 ID）
    let uid = get_optional_user_id_from_headers(&headers);

    // 如果未登录，返回空列表
    let uid = match uid {
        Some(id) => id,
        None => {
            return (
                StatusCode::OK,
                Json(json!({ "success": true, "replies": [] })),
            )
                .into_response();
        }
    };

    // 获取评论的回复（属于当前用户的）
    let replies = brew_comments::Entity::find()
        .filter(brew_comments::Column::ParentId.eq(comment_id))
        .filter(brew_comments::Column::UserId.eq(uid))
        .order_by_asc(brew_comments::Column::CreatedAt)
        .all(&db)
        .await;

    match replies {
        Ok(replies) => {
            if replies.is_empty() {
                return (
                    StatusCode::OK,
                    Json(json!({ "success": true, "replies": [] })),
                )
                    .into_response();
            }

            // 性能优化：由于所有回复都属于同一用户，只需查询一次用户信息
            let user_info = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT username, display_name, avatar_url FROM users WHERE id = $1",
                    vec![SeaValue::Int(Some(uid))],
                ))
                .await
                .ok()
                .flatten();

            let (user_name, user_display_name, user_avatar) = user_info
                .map(|row| {
                    (
                        row.try_get::<String>("", "username").ok(),
                        row.try_get::<String>("", "display_name").ok(),
                        row.try_get::<String>("", "avatar_url").ok(),
                    )
                })
                .unwrap_or((None, None, None));

            // 构建响应
            let responses: Vec<brew_comments::CommentResponse> = replies
                .into_iter()
                .map(|reply| {
                    let mut response: brew_comments::CommentResponse = reply.into();
                    response.user_name = user_name.clone();
                    response.user_display_name = user_display_name.clone();
                    response.user_avatar = user_avatar.clone();
                    response
                })
                .collect();

            (
                StatusCode::OK,
                Json(json!({ "success": true, "replies": responses })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

// ==================== RSSHub 实例管理 ====================

/// 获取 RSSHub 实例列表
async fn list_rsshub_instances(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // 验证用户身份
    let user_id = match get_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let rsshub_service = RsshubService::new(db);

    // 确保默认实例存在
    if let Err(e) = rsshub_service.ensure_default_instances().await {
        tracing::warn!("[RSSHub] Failed to ensure default instances: {}", e);
    }

    match rsshub_service.get_instances(Some(user_id)).await {
        Ok(instances) => {
            let responses: Vec<rsshub_instances::InstanceResponse> =
                instances.into_iter().map(|i| i.into()).collect();

            (
                StatusCode::OK,
                Json(json!({ "success": true, "instances": responses })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e })),
        )
            .into_response(),
    }
}

/// 添加 RSSHub 实例
#[derive(Deserialize)]
struct AddRsshubInstanceRequest {
    name: String,
    url: String,
    access_key: Option<String>,
    priority: Option<i32>,
}

async fn add_rsshub_instance(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<AddRsshubInstanceRequest>,
) -> impl IntoResponse {
    // 验证用户身份
    let user_id = match get_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let (_, is_admin) = get_user_and_admin_status(&headers).await;

    let rsshub_service = RsshubService::new(db);

    match rsshub_service
        .add_instance(
            Some(user_id),
            req.name,
            req.url,
            req.access_key,
            req.priority,
            is_admin,
        )
        .await
    {
        Ok(instance) => {
            let response: rsshub_instances::InstanceResponse = instance.into();
            (
                StatusCode::CREATED,
                Json(json!({ "success": true, "instance": response })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": e })),
        )
            .into_response(),
    }
}

/// 更新 RSSHub 实例
#[derive(Deserialize)]
struct UpdateRsshubInstanceRequest {
    name: Option<String>,
    url: Option<String>,
    access_key: Option<String>,
    priority: Option<i32>,
    enabled: Option<bool>,
}

async fn update_rsshub_instance(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
    Json(req): Json<UpdateRsshubInstanceRequest>,
) -> impl IntoResponse {
    // 验证用户身份
    let user_id = match get_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let (_, is_admin) = get_user_and_admin_status(&headers).await;

    let rsshub_service = RsshubService::new(db);

    match rsshub_service
        .update_instance(
            id,
            Some(user_id),
            req.name,
            req.url,
            req.access_key,
            req.priority,
            req.enabled,
            is_admin,
        )
        .await
    {
        Ok(instance) => {
            let response: rsshub_instances::InstanceResponse = instance.into();
            (
                StatusCode::OK,
                Json(json!({ "success": true, "instance": response })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": e })),
        )
            .into_response(),
    }
}

/// 删除 RSSHub 实例
async fn delete_rsshub_instance(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    // 验证用户身份
    let user_id = match get_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let (_, is_admin) = get_user_and_admin_status(&headers).await;

    let rsshub_service = RsshubService::new(db);

    match rsshub_service
        .delete_instance(id, Some(user_id), is_admin)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(json!({ "success": true }))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": e })),
        )
            .into_response(),
    }
}

/// 对单个实例执行健康检查
async fn health_check_rsshub_instance(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    // 验证用户身份
    let user_id = match get_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let rsshub_service = RsshubService::new(db.clone());

    // 获取实例
    let instance = match rsshub_instances::Entity::find_by_id(id).one(&db).await {
        Ok(Some(i)) => i,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "success": false, "error": "Instance not found" })),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": e.to_string() })),
            )
                .into_response()
        }
    };

    // 检查权限
    if instance.user_id != Some(user_id) && instance.user_id.is_some() {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "success": false, "error": "Permission denied" })),
        )
            .into_response();
    }

    match rsshub_service.health_check(&instance).await {
        Ok(response_time) => (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "healthy": true,
                "response_time_ms": response_time
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "healthy": false,
                "error": e
            })),
        )
            .into_response(),
    }
}

/// 重置实例统计
async fn reset_rsshub_instance(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> impl IntoResponse {
    // 验证用户身份
    let user_id = match get_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let (_, is_admin) = get_user_and_admin_status(&headers).await;

    let rsshub_service = RsshubService::new(db);

    match rsshub_service
        .reset_instance_stats(id, Some(user_id), is_admin)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(json!({ "success": true }))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": e })),
        )
            .into_response(),
    }
}

/// 对所有实例执行健康检查
async fn health_check_all_rsshub_instances(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // 验证用户身份
    let user_id = match get_user_id_from_headers(&headers, &db).await {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let rsshub_service = RsshubService::new(db);

    match rsshub_service.check_all_instances(Some(user_id)).await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "success": true, "message": "Health check completed" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::build_feed_discovery_candidates;

    #[test]
    fn feed_discovery_candidates_cover_root_and_nested_paths() {
        let candidates = build_feed_discovery_candidates("example.com/blog").unwrap();

        assert_eq!(candidates.first().unwrap(), "https://example.com/blog");
        assert!(candidates.contains(&"https://example.com/blog/feed".to_string()));
        assert!(candidates.contains(&"https://example.com/rss.xml".to_string()));
    }

    #[test]
    fn feed_discovery_keeps_direct_feed_first_and_rejects_other_schemes() {
        let candidates = build_feed_discovery_candidates("https://example.com/feed.xml").unwrap();
        assert_eq!(candidates.first().unwrap(), "https://example.com/feed.xml");

        assert!(build_feed_discovery_candidates("ftp://example.com/feed.xml").is_err());
    }
}
