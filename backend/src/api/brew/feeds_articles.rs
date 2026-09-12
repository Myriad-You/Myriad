use crate::error::HttpError;
use myriad_error::AppError;

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use chrono::Utc;
use futures::StreamExt;
use reqwest::Url;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
    QueryTrait, Statement,
};
use serde::Deserialize;
use serde_json::json;

use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::models::entities::{
    brew_annotations, brew_categories, brew_items, brew_podcasts, brew_sources, brew_user_states,
};
use crate::services::brew_parser::{FeedParser, ParseError, ParsedFeed};
use crate::services::brew_scheduler::get_brew_scheduler;
use crate::services::data_paths::paths;
use crate::services::icon_service::IconService;

use super::comments_rsshub;
use super::helpers::{
    brew_http_err, brew_store_http, build_feed_discovery_candidates, generate_opml,
    get_admin_user_id_from_headers, get_user_and_admin_status, overlay_requested_feed_type,
    parse_feed_type_label, parse_opml,
};
use super::notes;
use super::reading_sync_ws;

/// 创建 Brew API 路由
pub fn create_brew_routes(app_state: crate::state::AppState) -> Router<crate::state::AppState> {
    use axum::middleware::from_fn_with_state;
    Router::<crate::state::AppState>::new()
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
        // 手记（站长自写内容；写路径一律管理员）
        .route("/notes", post(notes::create_note))
        .route("/notes/preview", post(notes::preview_note))
        .route(
            "/notes/{id}",
            get(notes::get_note_draft)
                .put(notes::update_note)
                .delete(notes::delete_note),
        )
        // 文章获取
        .route("/items", get(list_items))
        .route("/items/{id}", get(reading_sync_ws::get_item))
        .route("/items/{id}/fulltext", get(reading_sync_ws::fetch_fulltext))
        // 阅读状态
        .route("/items/{id}/read", post(reading_sync_ws::mark_read))
        .route("/items/{id}/unread", post(reading_sync_ws::mark_unread))
        .route("/items/{id}/star", post(reading_sync_ws::star_item))
        .route("/items/{id}/unstar", post(reading_sync_ws::unstar_item))
        .route("/mark-all-read", post(reading_sync_ws::mark_all_read))
        // 用户评论（批注）
        .route(
            "/items/{id}/comments",
            get(comments_rsshub::list_comments).post(comments_rsshub::create_comment),
        )
        .route(
            "/comments/{id}",
            put(comments_rsshub::update_comment).delete(comments_rsshub::delete_comment),
        )
        .route(
            "/comments/{id}/replies",
            get(comments_rsshub::list_comment_replies),
        )
        // 离线同步
        .route("/sync-states", post(reading_sync_ws::sync_states))
        // 统计信息
        .route("/stats", get(reading_sync_ws::get_stats))
        // WebSocket（通知）
        .route(
            "/ws",
            get(reading_sync_ws::brew_websocket).route_layer(from_fn_with_state(
                app_state.clone(),
                crate::middleware::auth::auth_middleware,
            )),
        )
        // RSSHub 实例管理
        .route(
            "/rsshub/instances",
            get(comments_rsshub::list_rsshub_instances).post(comments_rsshub::add_rsshub_instance),
        )
        .route(
            "/rsshub/instances/{id}",
            put(comments_rsshub::update_rsshub_instance)
                .delete(comments_rsshub::delete_rsshub_instance),
        )
        .route(
            "/rsshub/instances/{id}/health-check",
            post(comments_rsshub::health_check_rsshub_instance),
        )
        .route(
            "/rsshub/instances/{id}/reset",
            post(comments_rsshub::reset_rsshub_instance),
        )
        .route(
            "/rsshub/health-check-all",
            post(comments_rsshub::health_check_all_rsshub_instances),
        )
        // 图标静态文件：Cache-Control max-age=86400；本层无 CompressionLayer
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
        .route_layer(from_fn_with_state(
            app_state.clone(),
            crate::api::tapp_runtime::brew_host_attribution,
        ))
}

// 订阅源管理

/// 最新文章预览
#[derive(Clone, Debug, serde::Serialize)]
struct ItemPreview {
    id: i32,
    title: String,
    summary: Option<String>,
    image: Option<String>,
    published_at: Option<i64>,
    is_read: bool,
    is_starred: bool,
    /// 预定义主题 key。首页「精选」磁贴靠预览里的 topic 聚类，
    /// 这样主题卡不需要额外接口。
    topic: Option<String>,
}

/// 带最新文章的订阅源响应
#[derive(Clone, Debug, serde::Serialize)]
struct SourceWithRecentItems {
    #[serde(flatten)]
    source: brew_sources::SourceResponse,
    recent_items: Vec<ItemPreview>,
    /// 近 `PULSE_WINDOW_DAYS` 天每篇文章距今天数，最多 `PULSE_MAX_POINTS` 个，
    /// 已按新→旧。派生字段、不落库。`pulses` 为空时前端仍可从 `published_at` 合成。
    pulses: Vec<i32>,
}

/// 节律图窗口（天）。与前端 `CADENCE_WINDOW_DAYS` 同为 730。
const PULSE_WINDOW_DAYS: i64 = 730;
/// 每个源最多回多少根节律线。上千篇的源必须截断，否则响应体白胀几十倍。
const PULSE_MAX_POINTS: i64 = 60;

/// 节律查询 SQL：一次窗口查询覆盖全部源，禁止 N+1。
///
/// `source_count` 决定 `$1..$n` 占位符个数。返回「距今天数」不是时间戳。
pub(crate) fn build_pulses_sql(source_count: usize) -> String {
    let src_ph = (1..=source_count)
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT source_id,                 GREATEST(0, (EXTRACT(EPOCH FROM (now() - published_at)) / 86400)::int)                   AS days_ago          FROM (            SELECT source_id, published_at,                   ROW_NUMBER() OVER                     (PARTITION BY source_id ORDER BY published_at DESC) AS rn            FROM brew_items            WHERE source_id IN ({src_ph})              AND published_at > now() - INTERVAL '{PULSE_WINDOW_DAYS} days'          ) t          WHERE rn <= {PULSE_MAX_POINTS}          ORDER BY source_id, days_ago"
    )
}

/// 获取订阅源列表（带最新文章预览）
/// 游客可访问（只读）。未读聚合跳过；预览 SQL 仍 LEFT JOIN 出 is_read/is_starred（游客 $1=-1）。
/// 非管理员用户看不到 admin_only=true 的订阅源
pub(crate) async fn list_sources(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 获取用户 ID 和管理员状态
    let (user_id, is_admin) = get_user_and_admin_status(&headers, &db).await;

    // 获取订阅源（非管理员过滤掉 admin_only=true 的源）
    let mut query = brew_sources::Entity::find().order_by_asc(brew_sources::Column::Name);

    if !is_admin {
        query = query.filter(brew_sources::Column::AdminOnly.eq(false));
    }

    let sources = match query.all(&db).await {
        Ok(s) => s,
        Err(e) => {
            return Err(brew_store_http("list sources", e));
        }
    };

    // 获取所有订阅源的最新文章（每个源最多 8 篇）
    let source_ids: Vec<i32> = sources.iter().map(|s| s.id).collect();

    // 并行三个 SQL 查询，均只传输必要字段：
    // (a) 每源未读数：SQL 聚合，避免把所有 item_id 拉到内存再过滤
    // (b) 每源最新 8 篇预览：窗口函数 `rn <= 8`，仅加载预览字段
    // (c) 每源节律：一次窗口查询拿全部源的近两年发布时间，绝不 N+1
    let (source_unread_counts, mut items_by_source, mut pulses_by_source) = tokio::join!(
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
                    if let Ok(rows) = db.query_all_raw(stmt).await {
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
        // (b) 每源最新 8 篇预览（`rn <= 8`）。
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
                    "SELECT id, source_id, title, summary, image, published_at, topic, \
                            COALESCE(is_read, false) AS is_read, \
                            COALESCE(is_starred, false) AS is_starred \
                     FROM ( \
                       SELECT i.id, i.source_id, i.title, i.summary, i.image, \
                              i.published_at, i.topic, s.is_read, s.is_starred, \
                              ROW_NUMBER() OVER \
                                (PARTITION BY i.source_id ORDER BY i.published_at DESC NULLS LAST) AS rn \
                       FROM brew_items i \
                       LEFT JOIN brew_user_states s \
                         ON s.item_id = i.id AND s.user_id = $1 \
                       WHERE i.source_id IN ({src_ph}) \
                     ) ranked \
                     WHERE rn <= 8"
                );
                let uid_val: i32 = user_id.unwrap_or(-1);
                let mut values: Vec<sea_orm::Value> = vec![uid_val.into()];
                values.extend(source_ids.iter().map(|&id| sea_orm::Value::Int(Some(id))));
                let stmt = Statement::from_sql_and_values(DatabaseBackend::Postgres, &sql, values);
                if let Ok(rows) = db.query_all_raw(stmt).await {
                    for row in &rows {
                        let id: i32 = row.try_get("", "id").unwrap_or(0);
                        let source_id: i32 = row.try_get("", "source_id").unwrap_or(0);
                        let title: String = row.try_get("", "title").unwrap_or_default();
                        let summary: Option<String> = row.try_get("", "summary").ok().flatten();
                        let image: Option<String> = row.try_get("", "image").ok().flatten();
                        let published_at: Option<sea_orm::entity::prelude::DateTimeWithTimeZone> =
                            row.try_get("", "published_at").ok();
                        let is_read: bool = row.try_get("", "is_read").unwrap_or(false);
                        let is_starred: bool = row.try_get("", "is_starred").unwrap_or(false);
                        let topic: Option<String> = row.try_get("", "topic").ok().flatten();
                        map.entry(source_id).or_default().push(ItemPreview {
                            id,
                            title,
                            summary,
                            image,
                            published_at: published_at.map(|dt| dt.timestamp_millis()),
                            is_read,
                            is_starred,
                            topic,
                        });
                    }
                }
            }
            map
        },
        // (c) 每源节律：ROW_NUMBER() 截到 PULSE_MAX_POINTS，窗口内按新→旧。
        // 后端算「距今天数」；`pulses` 为空时前端仍可从预览时间戳合成。
        async {
            let mut map: std::collections::HashMap<i32, Vec<i32>> =
                std::collections::HashMap::new();
            if !source_ids.is_empty() {
                let sql = build_pulses_sql(source_ids.len());
                let values: Vec<sea_orm::Value> = source_ids
                    .iter()
                    .map(|&id| sea_orm::Value::Int(Some(id)))
                    .collect();
                let stmt = Statement::from_sql_and_values(DatabaseBackend::Postgres, &sql, values);
                match db.query_all_raw(stmt).await {
                    Ok(rows) => {
                        for row in &rows {
                            let src: i32 = row.try_get("", "source_id").unwrap_or(0);
                            let days: i32 = row.try_get("", "days_ago").unwrap_or(0);
                            map.entry(src).or_default().push(days);
                        }
                    }
                    Err(e) => {
                        // 节律查询失败只记日志、pulses 为空；不把源列表打成 500。
                        tracing::warn!(error = %e, "brew pulses query failed; tiles fall back");
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
                pulses: pulses_by_source.remove(&source_id).unwrap_or_default(),
            }
        })
        .collect();

    Ok(Json(json!({ "success": true, "sources": responses })))
}

/// 添加订阅源
#[derive(Debug, Deserialize)]
pub struct AddSourceRequest {
    url: String,
    name: Option<String>,
    category: Option<String>,
    update_interval: Option<i32>,
    /// 来源类型：`link` / `brewlia` / 其余 → Rss。手记不走本 DTO。
    source_type: Option<String>,
    /// Feed 类型: rss, atom, json_feed, notion, rsshub
    feed_type: Option<String>,
    /// RSSHub 路由路径（仅当 feed_type = rsshub 时使用）
    rsshub_route: Option<String>,
    /// 额外配置（用于 Notion token 等敏感信息）
    extra_config: Option<serde_json::Value>,
    /// 仅管理员可见
    admin_only: Option<bool>,
    /// 导入包可带自定义图标 / 描述 / 站点链接，优先于抓取结果
    icon: Option<String>,
    description: Option<String>,
    site_url: Option<String>,
    enabled: Option<bool>,
    sort_order: Option<i32>,
}

pub(crate) async fn add_source(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<AddSourceRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 添加订阅源需要管理员权限
    let user_id = get_admin_user_id_from_headers(&headers, &db).await?;

    // 验证 URL
    let url = req.url.trim();
    if url.is_empty() {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::fail_json("URL is required")),
        )));
    }

    // 检查是否已订阅
    let existing = brew_sources::Entity::find()
        .filter(brew_sources::Column::UserId.eq(user_id))
        .filter(brew_sources::Column::Url.eq(url))
        .one(&db)
        .await;

    if let Ok(Some(_)) = existing {
        return Err(HttpError::from((
            StatusCode::CONFLICT,
            Json(AppError::fail_json("Already subscribed to this feed")),
        )));
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
                return Err(HttpError::from((
                    StatusCode::BAD_REQUEST,
                    Json(AppError::fail_json(
                        "Notion source requires extra_config with token",
                    )),
                )));
            };

            // 尝试验证 Notion 源
            let notion_service = crate::services::notion_service::NotionService::new();

            // 解析 Notion URL
            let (resource_type, resource_id) =
                match crate::services::notion_service::NotionService::parse_notion_url(url) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!("Invalid Notion URL: {e}");
                        return Err(HttpError::from((
                            StatusCode::BAD_REQUEST,
                            Json(json!({
                                "success": false,
                                "error": "Invalid Notion URL",
                                "code": "notion_url_invalid",
                            })),
                        )));
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
                    tracing::warn!(error = %e, "Failed to fetch Notion source");
                    return Err(HttpError::from((
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "success": false,
                            "error": e.to_string(),
                            "code": "notion_fetch_failed"
                        })),
                    )));
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
                    tracing::warn!(error = %e, "Failed to add feed");
                    match e {
                        ParseError::FetchError(_) | ParseError::InvalidUrl(_) => {
                            let status = if matches!(e, ParseError::InvalidUrl(_)) {
                                StatusCode::BAD_REQUEST
                            } else {
                                StatusCode::BAD_GATEWAY
                            };
                            return Err(brew_http_err(status, e.user_message()));
                        }
                        ParseError::ParseError(_) | ParseError::UnsupportedFormat(_) => {
                            // 内容解析失败仍允许带名称添加
                            if req.name.is_none() {
                                return Err(HttpError::from((
                                    StatusCode::BAD_REQUEST,
                                    Json(json!({
                                        "success": false,
                                        "error": "Failed to parse feed. Please provide a name.",
                                        "code": "feed_parse_failed"
                                    })),
                                )));
                            }
                            let final_feed_type = if is_rsshub {
                                brew_sources::FeedType::RssHub
                            } else {
                                brew_sources::FeedType::Rss
                            };
                            (req.name.unwrap(), None, None, None, final_feed_type, None)
                        }
                    }
                }
            }
        };

    let overlay_text = |value: Option<String>| {
        value.and_then(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
    };
    let description = overlay_text(req.description.clone()).or(description);
    let icon = overlay_text(req.icon.clone()).or(icon);
    let site_url = overlay_text(req.site_url.clone()).or(site_url);
    let feed_type =
        overlay_requested_feed_type(feed_type, req.feed_type.as_deref(), source_type.clone());

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
        enabled: Set(req.enabled.unwrap_or(true)),
        error_count: Set(0),
        item_count: Set(0),
        unread_count: Set(0),
        extra_config: Set(extra_config),
        rsshub_route: Set(rsshub_route),
        admin_only: Set(req.admin_only.unwrap_or(false)),
        sort_order: Set(req.sort_order),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        ..Default::default()
    };

    match new_source.insert(&db).await {
        Ok(source) => {
            // 下载图标到本地
            if let Some(icon_url) = &icon {
                if let Some(local_path) = persist_source_icon(source.id, icon_url).await {
                    let mut active: brew_sources::ActiveModel = source.clone().into();
                    active.icon = Set(Some(local_path));
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
            Ok(Json(json!({ "success": true, "source": response })))
        }
        Err(e) => Err(brew_store_http("save source", e)),
    }
}

/// 获取单个订阅源（游客可访问；admin_only 源仅管理员可见）
pub(crate) async fn get_source(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let (_, is_admin) = get_user_and_admin_status(&headers, &db).await;

    let source = brew_sources::Entity::find_by_id(id).one(&db).await;

    match source {
        Ok(Some(source)) => {
            if source.admin_only && !is_admin {
                return Err(HttpError::from((
                    StatusCode::NOT_FOUND,
                    Json(AppError::fail_json("Source not found")),
                )));
            }
            let response: brew_sources::SourceResponse = source.into();
            Ok(Json(json!({ "success": true, "source": response })))
        }
        Ok(None) => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::fail_json("Source not found")),
        ))),
        Err(e) => Err(brew_store_http("find source", e)),
    }
}

async fn persist_source_icon(source_id: i32, icon: &str) -> Option<String> {
    match IconService::new().download_icon(source_id, icon).await {
        Ok(Some(info)) => Some(info.local_path),
        Ok(None) => None,
        Err(error) => {
            tracing::warn!(%error, source_id, "Failed to persist source icon");
            None
        }
    }
}

/// 更新订阅源（需要管理员权限）
pub(crate) async fn update_source(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
    Json(req): Json<brew_sources::UpdateSourceRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 更新订阅源需要管理员权限
    let user_id = get_admin_user_id_from_headers(&headers, &db).await?;

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
                // 空字符串清除档位锁（与 theme_color / icon 同一约定）。
                active.card_size = Set(if card_size.is_empty() {
                    None
                } else {
                    Some(card_size)
                });
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
                // 支持空字符串清除图标；data URI / 外链尽量落盘，避免库里长期存整段 base64
                active.icon = Set(if icon.is_empty() {
                    None
                } else {
                    Some(persist_source_icon(id, &icon).await.unwrap_or(icon))
                });
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
                active.feed_type = Set(parse_feed_type_label(feed_type_str));
            }
            if let Some(description) = req.description {
                active.description = Set(if description.trim().is_empty() {
                    None
                } else {
                    Some(description)
                });
            }
            if let Some(site_url) = req.site_url {
                active.site_url = Set(if site_url.trim().is_empty() {
                    None
                } else {
                    Some(site_url)
                });
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
                    Ok(Json(json!({ "success": true, "source": response })))
                }
                Err(e) => Err(brew_store_http("update source", e)),
            }
        }
        Ok(None) => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::fail_json("Source not found")),
        ))),
        Err(e) => Err(brew_store_http("find source", e)),
    }
}

/// 删除订阅源（需要管理员权限）
pub(crate) async fn delete_source(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 删除订阅源需要管理员权限
    let user_id = get_admin_user_id_from_headers(&headers, &db).await?;

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
                Ok(_) => Ok(Json(json!({ "success": true }))),
                Err(e) => Err(brew_store_http("delete source", e)),
            }
        }
        Ok(None) => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::fail_json("Source not found")),
        ))),
        Err(e) => Err(brew_store_http("find source", e)),
    }
}

/// 手动刷新订阅源（需要管理员权限）
pub(crate) async fn refresh_source(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 刷新订阅源需要管理员权限
    let user_id = get_admin_user_id_from_headers(&headers, &db).await?;

    // 验证所有权
    let source = brew_sources::Entity::find_by_id(id)
        .filter(brew_sources::Column::UserId.eq(user_id))
        .one(&db)
        .await;

    match source {
        Ok(Some(_)) => {
            if let Some(scheduler) = get_brew_scheduler() {
                match scheduler.refresh_source(id).await {
                    Ok(new_count) => Ok(Json(json!({ "success": true, "new_items": new_count }))),
                    Err(error) => {
                        tracing::error!(%error, "Failed to refresh source");
                        let status = if error.starts_with("Failed to fetch feed") {
                            StatusCode::BAD_GATEWAY
                        } else if error.starts_with("Failed to parse feed") {
                            StatusCode::UNPROCESSABLE_ENTITY
                        } else if error.starts_with("Invalid feed URL") {
                            StatusCode::BAD_REQUEST
                        } else if error == "Source not found" {
                            StatusCode::NOT_FOUND
                        } else {
                            StatusCode::INTERNAL_SERVER_ERROR
                        };
                        Err(brew_http_err(status, error))
                    }
                }
            } else {
                Err(HttpError::from((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(AppError::fail_json("Scheduler not available")),
                )))
            }
        }
        Ok(None) => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::fail_json("Source not found")),
        ))),
        Err(e) => Err(brew_store_http("find source", e)),
    }
}

/// 探测订阅源信息
#[derive(Debug, Deserialize)]
pub struct DiscoverRequest {
    url: String,
}

fn discover_success_response(
    requested_url: &str,
    discovered_url: String,
    feed: ParsedFeed,
) -> Result<Json<serde_json::Value>, HttpError> {
    let autocompleted = requested_url != discovered_url;
    Ok(Json(json!({
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
    })))
}

/// 探测订阅源信息（需管理员；出站经 FeedParser SSRF 防护）
pub(crate) async fn discover_source(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<DiscoverRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    get_admin_user_id_from_headers(&headers, &db).await?;

    let mut candidates = match build_feed_discovery_candidates(&req.url) {
        Ok(candidates) => candidates,
        Err(error) => {
            return Err(HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(AppError::fail_json(error)),
            )))
        }
    };
    let requested_url = candidates.remove(0);
    let parser = FeedParser::new();

    // 用户输入本身已经是 Feed 时立即返回，不额外请求候选地址。
    let direct_error = match parser.fetch_and_parse(&requested_url).await {
        Ok(feed) => return discover_success_response(&requested_url, requested_url.clone(), feed),
        Err(error) => error.user_message(),
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

    tracing::warn!(error = %direct_error, "Unable to discover RSS/Atom feed");
    Err(HttpError::from((
        StatusCode::BAD_REQUEST,
        Json(json!({
            "success": false,
            "error": direct_error,
            "code": "feed_discover_failed"
        })),
    )))
}

// OPML 导入导出

/// 导入 OPML
#[derive(Debug, Deserialize)]
pub struct ImportOpmlRequest {
    opml: String,
}

pub(crate) async fn import_opml(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ImportOpmlRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 导入 OPML 需要管理员权限
    let user_id = get_admin_user_id_from_headers(&headers, &db).await?;

    // 解析 OPML
    let feeds = parse_opml(&req.opml);
    if feeds.is_empty() {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::fail_json("No feeds found in OPML")),
        )));
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
            site_url: Set(feed.site_url),
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
            return Err(brew_store_http("import sources", e));
        }
    }

    Ok(Json(json!({
        "success": true,
        "imported": imported,
        "skipped": skipped,
    })))
}

/// 导出 OPML（游客可访问；非管理员不导出 admin_only 源）
pub(crate) async fn export_opml(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, HttpError> {
    let (_, is_admin) = get_user_and_admin_status(&headers, &db).await;

    let mut query = brew_sources::Entity::find()
        // 手记源的 url 是 `myriad:notes`，不是一个可订阅的 feed。导出来别人
        // 导进去只会得到一个永远抓不动的源。
        .filter(brew_sources::Column::SourceType.ne(brew_sources::SourceType::Note))
        .order_by_asc(brew_sources::Column::Category)
        .order_by_asc(brew_sources::Column::Name);

    if !is_admin {
        query = query.filter(brew_sources::Column::AdminOnly.eq(false));
    }

    let sources = query.all(&db).await;

    match sources {
        Ok(sources) => {
            let opml = generate_opml(&sources);
            Ok((StatusCode::OK, [("Content-Type", "application/xml")], opml))
        }
        Err(e) => Err(brew_store_http("export sources", e)),
    }
}

// 分类管理

/// 获取分类列表（游客可访问）
pub(crate) async fn list_categories(
    State(db): State<DatabaseConnection>,
    _headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 游客可访问，获取所有分类
    let categories = brew_categories::Entity::find()
        .order_by_asc(brew_categories::Column::SortOrder)
        .all(&db)
        .await;

    match categories {
        Ok(cats) => Ok(Json(json!({ "success": true, "categories": cats }))),
        Err(e) => Err(brew_store_http("list categories", e)),
    }
}

pub(crate) async fn create_category(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<brew_categories::CreateCategoryRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 创建分类需要管理员权限
    let user_id = get_admin_user_id_from_headers(&headers, &db).await?;

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
        Ok(cat) => Ok(Json(json!({ "success": true, "category": cat }))),
        Err(e) => Err(brew_store_http("save category", e)),
    }
}

pub(crate) async fn update_category(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
    Json(req): Json<brew_categories::UpdateCategoryRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 更新分类需要管理员权限
    let user_id = get_admin_user_id_from_headers(&headers, &db).await?;

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
                Ok(updated) => Ok(Json(json!({ "success": true, "category": updated }))),
                Err(e) => Err(brew_store_http("update category", e)),
            }
        }
        Ok(None) => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::fail_json("Category not found")),
        ))),
        Err(e) => Err(brew_store_http("find category", e)),
    }
}

pub(crate) async fn delete_category(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 删除分类需要管理员权限
    let user_id = get_admin_user_id_from_headers(&headers, &db).await?;

    let cat = brew_categories::Entity::find_by_id(id)
        .filter(brew_categories::Column::UserId.eq(user_id))
        .one(&db)
        .await;

    match cat {
        Ok(Some(_)) => match brew_categories::Entity::delete_by_id(id).exec(&db).await {
            Ok(_) => Ok(Json(json!({ "success": true }))),
            Err(e) => Err(brew_store_http("delete category", e)),
        },
        Ok(None) => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::fail_json("Category not found")),
        ))),
        Err(e) => Err(brew_store_http("find category", e)),
    }
}

// 文章获取

/// 获取文章列表（游客可访问）
/// 游客不计算已读/收藏状态以节约计算
/// 非管理员看不到 admin_only 源下的文章
pub(crate) async fn list_items(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Query(query): Query<brew_items::ItemsQuery>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 获取可选用户 ID 与管理员状态
    let (user_id, is_admin) = get_user_and_admin_status(&headers, &db).await;

    // 可见订阅源 ID（非管理员过滤 admin_only）
    let mut sources_q = brew_sources::Entity::find()
        .select_only()
        .column(brew_sources::Column::Id);
    if !is_admin {
        sources_q = sources_q.filter(brew_sources::Column::AdminOnly.eq(false));
    }
    let all_sources: Vec<i32> = sources_q.into_tuple().all(&db).await.unwrap_or_default();

    if all_sources.is_empty() {
        return Ok(Json(json!({
            "success": true,
            "items": [],
            "total": 0,
            "page": 1,
            "per_page": 20,
            "next_cursor": null
        })));
    }

    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(20).clamp(1, 100);
    let cursor = match query
        .cursor
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        None => None,
        Some(raw) => match brew_items::decode_item_cursor(raw) {
            Some(cursor) => Some(cursor),
            None => {
                return Err(brew_http_err(
                    StatusCode::BAD_REQUEST,
                    "Invalid list cursor",
                ));
            }
        },
    };
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
                    return Ok(Json(json!({
                        "success": true,
                        "items": [],
                        "total": 0,
                        "page": page,
                        "per_page": per_page,
                        "next_cursor": null
                    })));
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

    // 按主题筛选。与 category 同级：只回该主题的文章，`topic IS NULL` 的天然落空。
    // 打标是离线的，读路径只读已有列，绝不在这里现算。
    if let Some(topic) = query
        .topic
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        items_query = items_query.filter(brew_items::Column::Topic.eq(topic));
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
            return Ok(Json(json!({
                "success": true,
                "items": [],
                "total": 0,
                "page": page,
                "per_page": per_page,
                "next_cursor": null
            })));
        }
        items_query = items_query.filter(brew_items::Column::SourceId.is_in(cat_sources));
    }

    // 排序
    let sort_order = query.sort_order.as_deref().unwrap_or("desc");
    items_query = brew_items::ordered_list_query(items_query, sort_order == "asc");
    if let Some(ref cursor) = cursor {
        items_query = brew_items::apply_item_cursor(items_query, sort_order == "asc", cursor);
    }

    // Extra row tells has-more. COUNT only on the first page; cursor pages skip it.
    let total = if cursor.is_some() {
        0
    } else {
        items_query.clone().count(&db).await.unwrap_or(0)
    };

    let preview = query.projection == Some(brew_items::ItemProjection::Preview);
    if preview {
        items_query = brew_items::preview_query(items_query);
    }

    let fetch = per_page as u64 + 1;
    let items = if cursor.is_some() {
        items_query.limit(fetch).all(&db).await
    } else {
        items_query
            .offset(((page - 1) * per_page) as u64)
            .limit(fetch)
            .all(&db)
            .await
    };

    match items {
        Ok(items) => {
            let (items, next_cursor) = brew_items::split_list_page(items, per_page);
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

            let response_items = brew_items::list_response_items(response_items, preview)
                .map_err(|error| brew_store_http("serialize articles", error))?;
            Ok(Json(json!({
                "success": true,
                "items": response_items,
                "total": total,
                "page": page,
                "per_page": per_page,
                "next_cursor": next_cursor,
            })))
        }
        Err(e) => Err(brew_store_http("list articles", e)),
    }
}
