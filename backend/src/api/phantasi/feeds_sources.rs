//! Phantasi feed sources: list, CRUD, discover, and categories.
use crate::error::HttpError;
use myriad_error::AppError;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::Utc;
use futures::StreamExt;
use reqwest::Url;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Statement, TransactionTrait,
};
use serde::Deserialize;
use serde_json::json;

use crate::models::entities::{phantasi_categories, phantasi_items, phantasi_sources};
use crate::services::icon_service::IconService;
use crate::services::phantasi_parser::{FeedParser, ParseError, ParsedFeed};
use crate::services::phantasi_scheduler::get_phantasi_scheduler;

use super::helpers::{
    build_feed_discovery_candidates, get_admin_user_id_from_headers, get_phantasi_viewer,
    materialize_source_icon, normalize_http_url, overlay_requested_feed_type,
    parse_feed_type_label, persist_source_icon, phantasi_http_err, phantasi_store_http,
};

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
    /// 订阅主题名。首页磁贴靠预览里的 topic 聚类，不另开列表接口。
    topic: Option<String>,
}

/// 带最新文章的订阅源响应
#[derive(Clone, Debug, serde::Serialize)]
struct SourceWithRecentItems {
    #[serde(flatten)]
    source: phantasi_sources::SourceResponse,
    recent_items: Vec<ItemPreview>,
    /// 近 `PULSE_WINDOW_DAYS` 天每篇文章距今天数，最多 `PULSE_MAX_POINTS` 个，
    /// 已按新→旧。派生字段、不落库。`pulses` 为空时前端仍可从 `published_at` 合成。
    pulses: Vec<i32>,
}

/// 节律图窗口（天）。与前端 `CADENCE_WINDOW_DAYS` 同为 730。
const PULSE_WINDOW_DAYS: i64 = 730;
/// 每个源最多回多少根节律线。上千篇的源必须截断，否则响应体白胀几十倍。
const PULSE_MAX_POINTS: i64 = 60;

/// 节律查询 SQL：一次 LATERAL 查询覆盖全部源，禁止 N+1。
///
/// `source_count` 决定 `$1..$n` 占位符个数。返回「距今天数」不是时间戳。
pub(crate) fn build_pulses_sql(source_count: usize) -> String {
    let src_ph = (1..=source_count)
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT source.id AS source_id,
                GREATEST(0, (EXTRACT(EPOCH FROM (now() - i.published_at)) / 86400)::int) AS days_ago
         FROM phantasi_sources source
         CROSS JOIN LATERAL (
             SELECT published_at FROM phantasi_items
             WHERE source_id = source.id
               AND published_at > now() - INTERVAL '{PULSE_WINDOW_DAYS} days'
             ORDER BY published_at DESC NULLS LAST, id DESC LIMIT {PULSE_MAX_POINTS}
         ) i
         WHERE source.id IN ({src_ph})
         ORDER BY source.id, days_ago"
    )
}

fn build_previews_sql(source_count: usize) -> String {
    let src_ph = (2..source_count + 2)
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let summary = phantasi_items::preview_summary_sql("i.summary");
    format!(
        "SELECT i.id, i.source_id, i.title, {summary} AS summary, i.image,
                i.published_at, i.topic, COALESCE(s.is_read, false) AS is_read,
                COALESCE(s.is_starred, false) AS is_starred
         FROM phantasi_sources source
         CROSS JOIN LATERAL (
             SELECT id, source_id, title, summary, image, published_at, topic
             FROM phantasi_items WHERE source_id = source.id
             ORDER BY published_at DESC NULLS LAST, id DESC LIMIT 8
         ) i
         LEFT JOIN phantasi_user_states s ON s.item_id = i.id AND s.user_id = $1
         WHERE source.id IN ({src_ph})
         ORDER BY source.id, i.published_at DESC NULLS LAST, i.id DESC"
    )
}

#[derive(Debug, Default, Deserialize)]
pub struct ListSourcesQuery {
    /// `catalog` returns source rows only — no unread overlay, previews, or pulses.
    view: Option<String>,
    /// Comma-separated category tokens. Aliases like `friends` / `友链` match 友情链接.
    category: Option<String>,
    /// Journal board slice: `feeds` | `notes` | `sites`. Matches frontend `sourcesForBoard`.
    board: Option<String>,
}

fn is_catalog_view(view: Option<&str>) -> bool {
    view == Some("catalog")
}

fn normalize_listed_category_token(token: &str) -> String {
    let trimmed = token.trim();
    match trimmed.to_lowercase().as_str() {
        "friends" | "friend" | "friendlink" | "friend-link" | "friend_link" | "friend-links"
        | "friend_links" | "friend links" | "友链" | "友情連結" | "友情链接" => {
            "友情链接".to_string()
        }
        "mine" | "me" | "我的" | "我" => "我".to_string(),
        _ => trimmed.to_string(),
    }
}

fn source_matches_listed_category(
    category: Option<&str>,
    source_type: &phantasi_sources::SourceType,
    wanted: &str,
) -> bool {
    let want = normalize_listed_category_token(wanted);
    if want.is_empty() {
        return false;
    }
    let hit = category
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .any(|part| normalize_listed_category_token(part) == want);
    if hit {
        return true;
    }
    want == "友情链接"
        && *source_type == phantasi_sources::SourceType::Link
        && category.map(str::trim).unwrap_or("").is_empty()
}

fn listed_board_keeps(
    source_type: &phantasi_sources::SourceType,
    category: Option<&str>,
    admin_only: bool,
    board: &str,
) -> bool {
    match board {
        "feeds" => {
            *source_type != phantasi_sources::SourceType::Link
                && *source_type != phantasi_sources::SourceType::Note
        }
        "notes" => {
            *source_type == phantasi_sources::SourceType::Note
                || (!admin_only && source_matches_listed_category(category, source_type, "我"))
        }
        "sites" => {
            *source_type == phantasi_sources::SourceType::Link
                || source_matches_listed_category(category, source_type, "友情链接")
        }
        _ => true,
    }
}

fn retain_listed_sources_for_board(
    sources: &mut Vec<phantasi_sources::Model>,
    board: Option<&str>,
) {
    let Some(board) = board.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    if !matches!(board, "feeds" | "notes" | "sites") {
        return;
    }
    sources.retain(|source| {
        listed_board_keeps(
            &source.source_type,
            source.category.as_deref(),
            source.admin_only,
            board,
        )
    });
}

fn retain_listed_sources_for_category(
    sources: &mut Vec<phantasi_sources::Model>,
    category: Option<&str>,
) {
    let Some(raw) = category.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let wanted: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();
    if wanted.is_empty() {
        return;
    }
    sources.retain(|source| {
        wanted.iter().any(|token| {
            source_matches_listed_category(source.category.as_deref(), &source.source_type, token)
        })
    });
}

/// 获取订阅源列表（带最新文章预览）
/// 游客可访问（只读）。未读聚合跳过；预览 SQL 仍 LEFT JOIN 出 is_read/is_starred（游客 $1=-1）。
/// 非管理员用户看不到 admin_only=true 的订阅源
/// `view=catalog` 只回源行，给友情链接这类不读预览的首页卡。
/// `category` 在图标物化和预览 SQL 之前收窄源行，避免友情链接卡把整份目录拉回去。
/// `board` 按手帐板块切：订阅不含入口/笔记，笔记含 note/`我`，朋友们含入口和友情链接。
pub(crate) async fn list_sources(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Query(query): Query<ListSourcesQuery>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let (user_id, is_admin) = get_phantasi_viewer(&headers, &db).await?;

    // 获取订阅源（非管理员过滤掉 admin_only=true 的源）
    let mut source_query =
        phantasi_sources::Entity::find().order_by_asc(phantasi_sources::Column::Name);

    if !is_admin {
        source_query = source_query.filter(phantasi_sources::Column::AdminOnly.eq(false));
    }
    // `feeds` 与内存过滤同口径，可在 SQL 里丢掉入口/笔记，少物化图标、少跑预览 SQL。
    // notes/sites 有分类别名，不能收成 SQL。
    if query.board.as_deref() == Some("feeds") {
        source_query = source_query.filter(phantasi_sources::Column::SourceType.is_not_in([
            phantasi_sources::SourceType::Link,
            phantasi_sources::SourceType::Note,
        ]));
    }

    let mut sources = match source_query.all(&db).await {
        Ok(s) => s,
        Err(e) => {
            return Err(phantasi_store_http("list sources", e));
        }
    };
    retain_listed_sources_for_category(&mut sources, query.category.as_deref());
    retain_listed_sources_for_board(&mut sources, query.board.as_deref());
    for source in &mut sources {
        let raw = source.icon.take();
        source.icon = materialize_source_icon(&db, source.id, raw).await;
    }

    if is_catalog_view(query.view.as_deref()) {
        let responses: Vec<SourceWithRecentItems> = sources
            .into_iter()
            .map(|s| SourceWithRecentItems {
                source: s.into(),
                recent_items: Vec::new(),
                pulses: Vec::new(),
            })
            .collect();
        return Ok(Json(json!({ "success": true, "sources": responses })));
    }

    // 获取所有订阅源的最新文章（每个源最多 8 篇）
    let source_ids: Vec<i32> = sources.iter().map(|s| s.id).collect();

    // 并行三个 SQL 查询，均只传输必要字段：
    // (a) 每源未读数：SQL 聚合，避免把所有 item_id 拉到内存再过滤
    // (b) 每源最新 8 篇预览：每源索引 LIMIT 8，仅加载预览字段
    // (c) 每源节律：一次 LATERAL 查询拿全部源的近两年发布时间，绝不 N+1
    let (source_unread_counts, items_by_source, mut pulses_by_source) = tokio::join!(
        // (a) 未读数：LEFT JOIN phantasi_user_states，统计无已读状态的文章数
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
                         FROM phantasi_items i \
                         LEFT JOIN phantasi_user_states s \
                           ON s.item_id = i.id AND s.user_id = $1 AND s.is_read = TRUE \
                         WHERE i.source_id IN ({src_ph}) AND s.item_id IS NULL \
                         GROUP BY i.source_id"
                    );
                    let mut values: Vec<sea_orm::Value> = vec![uid.into()];
                    values.extend(source_ids.iter().map(|&id| sea_orm::Value::Int(Some(id))));
                    let stmt =
                        Statement::from_sql_and_values(DatabaseBackend::Postgres, &sql, values);
                    let rows = db.query_all_raw(stmt).await?;
                    for row in &rows {
                        let src: i32 = row.try_get("", "source_id").unwrap_or(0);
                        let cnt: i32 = row.try_get("", "unread_count").unwrap_or(0);
                        counts.insert(src, cnt);
                    }
                }
            }
            Ok::<_, sea_orm::DbErr>(counts)
        },
        // (b) 每源最新 8 篇预览。
        async {
            let mut map: std::collections::HashMap<i32, Vec<ItemPreview>> =
                std::collections::HashMap::new();
            if !source_ids.is_empty() {
                let sql = build_previews_sql(source_ids.len());
                let uid_val: i32 = user_id.unwrap_or(-1);
                let mut values: Vec<sea_orm::Value> = vec![uid_val.into()];
                values.extend(source_ids.iter().map(|&id| sea_orm::Value::Int(Some(id))));
                let stmt = Statement::from_sql_and_values(DatabaseBackend::Postgres, &sql, values);
                let rows = db.query_all_raw(stmt).await?;
                for row in &rows {
                    let id: i32 = row.try_get("", "id").unwrap_or(0);
                    let source_id: i32 = row.try_get("", "source_id").unwrap_or(0);
                    let title: String = row.try_get("", "title").unwrap_or_default();
                    let summary: Option<String> = row.try_get("", "summary").ok().flatten();
                    let image: Option<String> = row.try_get("", "image").ok().flatten();
                    let published_at: Option<sea_orm::entity::prelude::DateTimeWithTimeZone> =
                        row.try_get("", "published_at").ok();
                    let is_read: bool = row.try_get("", "is_read").unwrap_or(false);
                    let is_starred: bool =
                        is_admin && row.try_get("", "is_starred").unwrap_or(false);
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
            Ok::<_, sea_orm::DbErr>(map)
        },
        // (c) 每源节律：每源 LIMIT PULSE_MAX_POINTS，按新→旧。
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
                        tracing::warn!(error = %e, "phantasi pulses query failed; tiles fall back");
                    }
                }
            }
            map
        }
    );
    let source_unread_counts =
        source_unread_counts.map_err(|error| phantasi_store_http("source unread counts", error))?;
    let mut items_by_source =
        items_by_source.map_err(|error| phantasi_store_http("source previews", error))?;

    // 构建响应，使用 SQL 计算的真实未读数
    let responses: Vec<SourceWithRecentItems> = sources
        .into_iter()
        .map(|s| {
            let source_id = s.id;
            let real_unread_count = source_unread_counts.get(&source_id).copied().unwrap_or(0);
            let mut response: phantasi_sources::SourceResponse = s.into();
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
    /// 来源类型：`link` / `phantasiai` / 其余 → Rss。笔记不走本 DTO。
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
    let existing = phantasi_sources::Entity::find()
        .filter(phantasi_sources::Column::Url.eq(url))
        .one(&db)
        .await
        .map_err(|error| phantasi_store_http("find existing source", error))?;

    if existing.is_some() {
        return Err(HttpError::from((
            StatusCode::CONFLICT,
            Json(AppError::fail_json("Already subscribed to this feed")),
        )));
    }

    // 解析 source_type
    let source_type = match req.source_type.as_deref() {
        Some("link") => phantasi_sources::SourceType::Link,
        Some("phantasiai") => phantasi_sources::SourceType::Phantasiai,
        _ => phantasi_sources::SourceType::Rss,
    };

    // 检查是否是 Notion 类型
    let is_notion = req.feed_type.as_deref() == Some("notion")
        || crate::services::notion_service::NotionService::parse_notion_url(url).is_ok();

    // 获取源信息
    let (name, description, icon, site_url, feed_type, extra_config) =
        if source_type == phantasi_sources::SourceType::Link {
            // 纯链接类型不需要解析，直接添加
            (
                req.name.unwrap_or_else(|| url.to_string()),
                None,
                None,
                Some(url.to_string()),
                phantasi_sources::FeedType::Rss,
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
                    phantasi_sources::FeedType::Notion,
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
                        phantasi_sources::FeedType::RssHub
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
                            return Err(phantasi_http_err(status, e.user_message()));
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
                                phantasi_sources::FeedType::RssHub
                            } else {
                                phantasi_sources::FeedType::Rss
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
    let rsshub_route = if feed_type == phantasi_sources::FeedType::RssHub {
        // 优先使用前端传入的路由
        req.rsshub_route.clone().or_else(|| {
            // 否则从 URL 中提取
            Url::parse(url).ok().map(|u| u.path().to_string())
        })
    } else {
        None
    };

    let new_source = phantasi_sources::ActiveModel {
        user_id: Set(user_id),
        name: Set(name),
        url: Set(url.to_string()),
        feed_type: Set(feed_type),
        source_type: Set(source_type.clone()),
        category: Set(req.category),
        icon: Set(icon.clone()),
        description: Set(description),
        site_url: Set(site_url),
        update_interval: Set(if source_type == phantasi_sources::SourceType::Link {
            0
        } else {
            req.update_interval.unwrap_or(30)
        }),
        enabled: Set(req.enabled.unwrap_or(true)),
        error_count: Set(0),
        item_count: Set(0),
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
                    let mut active: phantasi_sources::ActiveModel = source.clone().into();
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
            if source_type != phantasi_sources::SourceType::Link {
                if let Some(scheduler) = get_phantasi_scheduler() {
                    let _ = scheduler.refresh_source(source.id).await;
                }
            }

            // 重新获取最新的 source 数据
            let updated_source = phantasi_sources::Entity::find_by_id(source.id)
                .one(&db)
                .await
                .map_err(|error| phantasi_store_http("reload source", error))?
                .ok_or_else(|| phantasi_store_http("reload source", "missing after save"))?;

            let response: phantasi_sources::SourceResponse = updated_source.into();
            Ok(Json(json!({ "success": true, "source": response })))
        }
        Err(e) => Err(phantasi_store_http("save source", e)),
    }
}

/// 更新订阅源（需要管理员权限）
pub(crate) async fn update_source(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
    Json(req): Json<phantasi_sources::UpdateSourceRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    get_admin_user_id_from_headers(&headers, &db).await?;

    let source = phantasi_sources::Entity::find_by_id(id).one(&db).await;

    match source {
        Ok(Some(source)) => {
            let source_is_note = source.source_type == phantasi_sources::SourceType::Note;
            let requested_type = req.source_type.as_deref();
            let changes_note_transport = req.url.is_some()
                || req.feed_type.is_some()
                || req.rsshub_route.is_some()
                || req.extra_config.is_some()
                || req
                    .category
                    .as_deref()
                    .is_some_and(|category| category.trim() != "我")
                || requested_type.is_some_and(|value| value != "note");
            if (source_is_note && changes_note_transport)
                || (!source_is_note && requested_type == Some("note"))
            {
                return Err(phantasi_http_err(
                    StatusCode::BAD_REQUEST,
                    "Note sources cannot change subscription type",
                ));
            }
            let mut active: phantasi_sources::ActiveModel = source.into();

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
            if let Some(url) = req.url {
                active.url = Set(normalize_http_url(&url)
                    .map_err(|error| phantasi_http_err(StatusCode::BAD_REQUEST, error))?
                    .to_string());
            }
            if let Some(ref source_type_str) = req.source_type {
                let st = match source_type_str.as_str() {
                    "link" => phantasi_sources::SourceType::Link,
                    "phantasiai" => phantasi_sources::SourceType::Phantasiai,
                    "rss" | "rsshub" => phantasi_sources::SourceType::Rss,
                    "note" => phantasi_sources::SourceType::Note,
                    _ => {
                        return Err(phantasi_http_err(
                            StatusCode::BAD_REQUEST,
                            "Invalid source type",
                        ));
                    }
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
            if let Some(rsshub_route) = req.rsshub_route {
                active.rsshub_route = Set(if rsshub_route.trim().is_empty() {
                    None
                } else {
                    Some(rsshub_route.trim().to_string())
                });
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
                    let response: phantasi_sources::SourceResponse = updated.into();
                    Ok(Json(json!({ "success": true, "source": response })))
                }
                Err(e) => Err(phantasi_store_http("update source", e)),
            }
        }
        Ok(None) => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::fail_json("Source not found")),
        ))),
        Err(e) => Err(phantasi_store_http("find source", e)),
    }
}

/// 删除订阅源（需要管理员权限）
pub(crate) async fn delete_source(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    get_admin_user_id_from_headers(&headers, &db).await?;

    let source = phantasi_sources::Entity::find_by_id(id).one(&db).await;

    match source {
        Ok(Some(source)) => {
            // 删除本地缓存的图标
            let icon_service = IconService::new();
            if let Err(e) = icon_service.delete_icon(id).await {
                tracing::warn!("Failed to delete icon for source {}: {}", id, e);
            }

            // 文章随源 CASCADE 删；笔记文档没有这条外键，先解开再删。
            let txn = db
                .begin()
                .await
                .map_err(|e| phantasi_store_http("begin source delete", e))?;
            crate::services::media::clear_rss_source(&txn, id)
                .await
                .map_err(|e| phantasi_store_http("release source media", e))?;
            if let Err(error) =
                crate::services::note_publish::detach_note_docs_for_source(&txn, source.id).await
            {
                if let Err(rollback) = txn.rollback().await {
                    tracing::warn!(error = %rollback, "source delete rollback failed");
                }
                return Err(error);
            }
            match phantasi_sources::Entity::delete_by_id(id).exec(&txn).await {
                Ok(_) => {
                    txn.commit()
                        .await
                        .map_err(|e| phantasi_store_http("commit source delete", e))?;
                    Ok(Json(json!({ "success": true })))
                }
                Err(e) => {
                    if let Err(rollback) = txn.rollback().await {
                        tracing::warn!(error = %rollback, "source delete rollback failed");
                    }
                    Err(phantasi_store_http("delete source", e))
                }
            }
        }
        Ok(None) => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::fail_json("Source not found")),
        ))),
        Err(e) => Err(phantasi_store_http("find source", e)),
    }
}

/// 手动刷新订阅源（需要管理员权限）
pub(crate) async fn refresh_source(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    get_admin_user_id_from_headers(&headers, &db).await?;

    let source = phantasi_sources::Entity::find_by_id(id).one(&db).await;

    match source {
        Ok(Some(_)) => match get_phantasi_scheduler() {
            Some(scheduler) => match scheduler.refresh_source(id).await {
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
                    Err(phantasi_http_err(status, error))
                }
            },
            _ => Err(HttpError::from((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(AppError::fail_json("Scheduler not available")),
            ))),
        },
        Ok(None) => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::fail_json("Source not found")),
        ))),
        Err(e) => Err(phantasi_store_http("find source", e)),
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
                phantasi_sources::FeedType::Rss => "rss",
                phantasi_sources::FeedType::Atom => "atom",
                phantasi_sources::FeedType::JsonFeed => "json_feed",
                phantasi_sources::FeedType::Notion => "notion",
                phantasi_sources::FeedType::RssHub => "rsshub",
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
            )));
        }
    };
    let requested_url = candidates.remove(0);
    let parser = FeedParser::new();

    // 用户输入本身已经是 Feed 时立即返回，不额外请求候选地址。
    // 301/308 落在 `permanent_url`，前端按 autocompleted 写入新地址。
    let direct_error = match parser.fetch_feed(&requested_url).await {
        Ok(fetched) => {
            let discovered = fetched
                .permanent_url
                .unwrap_or_else(|| requested_url.clone());
            return discover_success_response(&requested_url, discovered, fetched.feed);
        }
        Err(error) => error.user_message(),
    };

    // 常见后缀最多 4 个并发探测；每个请求仍经过 FeedParser 的 SSRF 防护。
    let mut attempts = futures::stream::iter(candidates.into_iter().map(|candidate| {
        let parser = &parser;
        async move {
            let result = parser.fetch_feed(&candidate).await;
            (candidate, result)
        }
    }))
    .buffer_unordered(4);

    while let Some((candidate, result)) = attempts.next().await {
        if let Ok(fetched) = result {
            let discovered = fetched.permanent_url.unwrap_or(candidate);
            return discover_success_response(&requested_url, discovered, fetched.feed);
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

// 分类管理

/// 获取分类列表（公开读：关访客门 404；坏凭据 401）
pub(crate) async fn list_categories(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
) -> Result<Json<serde_json::Value>, HttpError> {
    get_phantasi_viewer(&headers, &db).await?;
    let categories = phantasi_categories::Entity::find()
        .order_by_asc(phantasi_categories::Column::SortOrder)
        .all(&db)
        .await;

    match categories {
        Ok(cats) => Ok(Json(json!({
            "success": true,
            "categories": cats
                .into_iter()
                .map(phantasi_categories::CategoryResponse::from)
                .collect::<Vec<_>>(),
        }))),
        Err(e) => Err(phantasi_store_http("list categories", e)),
    }
}

pub(crate) async fn create_category(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Json(req): Json<phantasi_categories::CreateCategoryRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 创建分类需要管理员权限
    let user_id = get_admin_user_id_from_headers(&headers, &db).await?;

    let now = Utc::now();
    let new_cat = phantasi_categories::ActiveModel {
        user_id: Set(user_id),
        name: Set(req.name),
        icon: Set(req.icon),
        color: Set(req.color),
        sort_order: Set(0),
        created_at: Set(now.into()),
        ..Default::default()
    };

    match new_cat.insert(&db).await {
        Ok(cat) => Ok(Json(json!({
            "success": true,
            "category": phantasi_categories::CategoryResponse::from(cat),
        }))),
        Err(e) => Err(phantasi_store_http("save category", e)),
    }
}

pub(crate) async fn update_category(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
    Json(req): Json<phantasi_categories::UpdateCategoryRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 更新分类需要管理员权限。共享目录按 id，不按创建者。
    get_admin_user_id_from_headers(&headers, &db).await?;

    let cat = phantasi_categories::Entity::find_by_id(id).one(&db).await;

    match cat {
        Ok(Some(cat)) => {
            let mut active: phantasi_categories::ActiveModel = cat.into();
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
                Ok(updated) => Ok(Json(json!({
                    "success": true,
                    "category": phantasi_categories::CategoryResponse::from(updated),
                }))),
                Err(e) => Err(phantasi_store_http("update category", e)),
            }
        }
        Ok(None) => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::fail_json("Category not found")),
        ))),
        Err(e) => Err(phantasi_store_http("find category", e)),
    }
}

pub(crate) async fn delete_category(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    // 删除分类需要管理员权限。共享目录按 id，不按创建者。
    get_admin_user_id_from_headers(&headers, &db).await?;

    let cat = phantasi_categories::Entity::find_by_id(id).one(&db).await;

    match cat {
        Ok(Some(_)) => match phantasi_categories::Entity::delete_by_id(id)
            .exec(&db)
            .await
        {
            Ok(_) => Ok(Json(json!({ "success": true }))),
            Err(e) => Err(phantasi_store_http("delete category", e)),
        },
        Ok(None) => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::fail_json("Category not found")),
        ))),
        Err(e) => Err(phantasi_store_http("find category", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        listed_board_keeps, normalize_listed_category_token, source_matches_listed_category,
    };
    use crate::models::entities::phantasi_sources::SourceType;

    #[tokio::test]
    async fn postgres_source_previews_and_pulses_are_bounded_index_reads() {
        use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let mut options = sea_orm::ConnectOptions::new(url);
        options.max_connections(1).sqlx_logging(false);
        let db = sea_orm::Database::connect(options).await.unwrap();
        db.execute_unprepared(r#"
CREATE TEMP TABLE phantasi_sources (id INTEGER PRIMARY KEY);
CREATE TEMP TABLE phantasi_items (id INTEGER PRIMARY KEY, source_id INTEGER NOT NULL, title TEXT,
summary TEXT, image TEXT, published_at TIMESTAMPTZ, topic TEXT);
CREATE TEMP TABLE phantasi_user_states (item_id INTEGER, user_id INTEGER, is_read BOOLEAN, is_starred BOOLEAN,
PRIMARY KEY (item_id, user_id));
INSERT INTO phantasi_sources VALUES (1), (2), (3);
INSERT INTO phantasi_items (id, source_id, title, summary, published_at)
SELECT n, CASE WHEN n <= 2000 THEN 1 ELSE 2 END, n::text, repeat('s', 10000), NOW() FROM generate_series(1, 4000) n;
INSERT INTO phantasi_items (id, source_id, title) VALUES (4001, 1, 'undated'), (4002, 3, 'only undated');
INSERT INTO phantasi_user_states VALUES (2000, 42, true, true), (4000, 99, true, true);
"#).await.unwrap();
        db.execute_unprepared(migration::SOURCE_RECENT_INDEX_SQL)
            .await
            .unwrap();
        db.execute_unprepared("ANALYZE phantasi_items; ANALYZE phantasi_sources;")
            .await
            .unwrap();
        let sql = super::build_previews_sql(3);
        let values: Vec<sea_orm::Value> = vec![42.into(), 1.into(), 2.into(), 3.into()];
        let rows = db
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                &sql,
                values.clone(),
            ))
            .await
            .unwrap();
        assert_eq!(rows.len(), 17);
        assert_eq!(rows[0].try_get::<i32>("", "id").unwrap(), 2000);
        assert!(rows[0].try_get::<bool>("", "is_read").unwrap());
        assert_eq!(rows[8].try_get::<i32>("", "id").unwrap(), 4000);
        assert!(!rows[8].try_get::<bool>("", "is_read").unwrap());
        assert_eq!(rows[16].try_get::<i32>("", "id").unwrap(), 4002);
        assert!(rows[0].try_get::<String>("", "summary").unwrap().len() < 10000);
        let pulse_sql = super::build_pulses_sql(3);
        let pulse_values: Vec<sea_orm::Value> = vec![1.into(), 2.into(), 3.into()];
        let pulses = db
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                &pulse_sql,
                pulse_values.clone(),
            ))
            .await
            .unwrap();
        assert_eq!(pulses.len(), 120);
        for (query, bindings, bound) in [(sql, values, 8.0), (pulse_sql, pulse_values, 60.0)] {
            let row = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    format!("EXPLAIN (ANALYZE, FORMAT JSON) {query}"),
                    bindings,
                ))
                .await
                .unwrap()
                .unwrap();
            let plan: serde_json::Value = row.try_get("", "QUERY PLAN").unwrap();
            fn assert_bounded(node: &serde_json::Value, bound: f64) -> usize {
                let mut count = 0;
                if node["Index Name"] == "idx_phantasi_items_source_recent" {
                    assert!(node["Actual Rows"].as_f64().unwrap() <= bound, "{node}");
                    count += 1;
                }
                if let Some(children) = node["Plans"].as_array() {
                    count += children
                        .iter()
                        .map(|child| assert_bounded(child, bound))
                        .sum::<usize>();
                }
                count
            }
            assert!(assert_bounded(&plan[0]["Plan"], bound) > 0, "{plan}");
        }
        db.close().await.unwrap();
    }

    #[test]
    fn list_sources_rewrites_inline_icons_before_serialize() {
        let src = include_str!("feeds_sources.rs");
        let start = src
            .find("pub(crate) async fn list_sources")
            .expect("list_sources");
        let body = &src[start..];
        let end = body[1..]
            .find("\n/// 添加订阅源")
            .map(|index| index + 1)
            .unwrap_or(body.len());
        let list = &body[..end];
        assert!(list.contains("materialize_source_icon"));
        let retain = list
            .find("retain_listed_sources_for_category")
            .expect("category filter before icon rewrite");
        let board = list
            .find("retain_listed_sources_for_board")
            .expect("board filter before icon rewrite");
        let icons = list.find("materialize_source_icon").expect("icon rewrite");
        assert!(retain < icons, "category filter must run before icon I/O");
        assert!(board < icons, "board filter must run before icon I/O");
    }

    #[test]
    fn listed_category_aliases_collapse_to_canonical_names() {
        assert_eq!(normalize_listed_category_token("friends"), "友情链接");
        assert_eq!(normalize_listed_category_token("友链"), "友情链接");
        assert_eq!(normalize_listed_category_token("friend_links"), "友情链接");
        assert_eq!(normalize_listed_category_token("友情連結"), "友情链接");
        assert_eq!(normalize_listed_category_token("mine"), "我");
        assert_eq!(normalize_listed_category_token("技术"), "技术");
    }

    #[test]
    fn listed_category_matches_friend_link_tokens_and_legacy_links() {
        assert!(source_matches_listed_category(
            Some("友情链接"),
            &SourceType::Rss,
            "friends"
        ));
        assert!(source_matches_listed_category(
            Some("friend_links, 技术"),
            &SourceType::Rss,
            "友情链接"
        ));
        assert!(source_matches_listed_category(
            None,
            &SourceType::Link,
            "friends"
        ));
        assert!(!source_matches_listed_category(
            Some("技术"),
            &SourceType::Rss,
            "friends"
        ));
        assert!(!source_matches_listed_category(
            None,
            &SourceType::Rss,
            "friends"
        ));
        assert!(!source_matches_listed_category(
            Some("科学技术"),
            &SourceType::Rss,
            "技术"
        ));
    }

    #[test]
    fn listed_board_matches_journal_sources_for_board() {
        assert!(listed_board_keeps(&SourceType::Rss, None, false, "feeds"));
        assert!(listed_board_keeps(
            &SourceType::Phantasiai,
            None,
            false,
            "feeds"
        ));
        assert!(!listed_board_keeps(&SourceType::Link, None, false, "feeds"));
        assert!(!listed_board_keeps(&SourceType::Note, None, false, "feeds"));
        assert!(listed_board_keeps(
            &SourceType::Rss,
            Some("友情链接"),
            false,
            "feeds"
        ));

        assert!(listed_board_keeps(&SourceType::Note, None, false, "notes"));
        assert!(listed_board_keeps(&SourceType::Note, None, true, "notes"));
        assert!(listed_board_keeps(
            &SourceType::Rss,
            Some("我"),
            false,
            "notes"
        ));
        assert!(!listed_board_keeps(
            &SourceType::Rss,
            Some("我"),
            true,
            "notes"
        ));
        assert!(!listed_board_keeps(&SourceType::Link, None, false, "notes"));

        assert!(listed_board_keeps(&SourceType::Link, None, false, "sites"));
        assert!(listed_board_keeps(
            &SourceType::Rss,
            Some("友情链接"),
            false,
            "sites"
        ));
        assert!(listed_board_keeps(
            &SourceType::Link,
            Some("技术"),
            false,
            "sites"
        ));
        assert!(!listed_board_keeps(
            &SourceType::Rss,
            Some("技术"),
            false,
            "sites"
        ));
    }
}
