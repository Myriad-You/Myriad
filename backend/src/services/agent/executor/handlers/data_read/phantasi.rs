use super::super::HandlerContext;
use crate::models::entities::{phantasi_items, phantasi_sources, phantasi_user_states};
use once_cell::sync::Lazy;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, QueryTrait, Statement,
};
use serde_json::{Value, json};
use std::collections::HashMap;

static RE_HTML_TAG: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"<[^>]+>").unwrap());
static RE_WHITESPACE: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"\s+").unwrap());

pub(super) fn phantasi_query_failed(
    context: &'static str,
    error: impl std::fmt::Display,
) -> String {
    tracing::error!(%error, context, "phantasi query failed");
    format!("Failed to {context}")
}

pub(super) fn visible_sources_query(is_admin: bool) -> sea_orm::Select<phantasi_sources::Entity> {
    let query = phantasi_sources::Entity::find();
    if is_admin {
        query
    } else {
        query.filter(phantasi_sources::Column::AdminOnly.eq(false))
    }
}

pub(super) fn visible_source_ids(is_admin: bool) -> sea_orm::sea_query::SelectStatement {
    visible_sources_query(is_admin)
        .select_only()
        .column(phantasi_sources::Column::Id)
        .into_query()
}

pub(super) fn source_visible(source: &phantasi_sources::Model, is_admin: bool) -> bool {
    is_admin || !source.admin_only
}

fn parse_optional_i32(v: &Value) -> Option<i32> {
    if let Some(n) = v.as_i64() {
        return i32::try_from(n).ok();
    }
    if let Some(n) = v.as_u64() {
        return i32::try_from(n).ok();
    }
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<i32>().ok())
}

/// Non-empty trimmed string from a JSON value.
fn parse_optional_str(v: &Value) -> Option<&str> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty())
}

/// Filters extracted from phantasi.read params (sourceId / source / sourceName / limit / since).
#[derive(Debug, Clone, PartialEq, Eq)]
struct PhantasiReadFilters {
    source_id: Option<i32>,
    source_name: Option<String>,
    limit: usize,
    since: Option<String>,
}

/// Align schema `source` with handler `sourceId` / `sourceName`.
/// - `sourceId` (int or numeric string) wins for id
/// - numeric `source` also resolves as id
/// - non-numeric `source` / `sourceName` resolve as name filter
fn parse_phantasi_read_filters(params: &HashMap<String, Value>) -> PhantasiReadFilters {
    let source_id = params
        .get("sourceId")
        .and_then(parse_optional_i32)
        .or_else(|| params.get("source").and_then(parse_optional_i32));

    let source_name = params
        .get("sourceName")
        .and_then(parse_optional_str)
        .map(|s| s.to_string())
        .or_else(|| {
            // Only treat `source` as a name when it is not a pure integer id
            params
                .get("source")
                .and_then(parse_optional_str)
                .filter(|s| s.parse::<i32>().is_err())
                .map(|s| s.to_string())
        });

    let limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(50)
        .clamp(1, 200) as usize;

    let since = params
        .get("since")
        .and_then(parse_optional_str)
        .map(|s| s.to_string());

    PhantasiReadFilters {
        source_id,
        source_name,
        limit,
        since,
    }
}

/// Resolve phantasi.article lookup keys: id (i32), guid/string id, or url/link.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PhantasiArticleLookup {
    item_id: Option<i32>,
    /// articleId / itemId 字符串形式（数字字符串也可作 guid 回退）
    article_key: Option<String>,
    url: Option<String>,
    source_id: Option<i32>,
}

fn parse_phantasi_article_lookup(params: &HashMap<String, Value>) -> PhantasiArticleLookup {
    let article_id_val = params
        .get("articleId")
        .or_else(|| params.get("itemId"))
        .or_else(|| params.get("id"));

    let item_id = article_id_val.and_then(parse_optional_i32);
    let article_key = article_id_val
        .and_then(parse_optional_str)
        .filter(|_| item_id.is_none())
        .map(|s| s.to_string())
        .or_else(|| {
            // Keep string form of numeric id as guid fallback only when provided as string
            article_id_val
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        });

    let url = params
        .get("url")
        .or_else(|| params.get("link"))
        .and_then(parse_optional_str)
        .map(|s| s.to_string());

    let source_id = params.get("sourceId").and_then(parse_optional_i32);

    PhantasiArticleLookup {
        item_id,
        article_key,
        url,
        source_id,
    }
}

fn phantasi_item_to_read_json(
    item: &phantasi_items::Model,
    source: Option<&phantasi_sources::Model>,
) -> Value {
    let src_name = source.map(|s| s.name.as_str()).unwrap_or("");
    let source_url = source.map(|s| s.url.as_str()).unwrap_or("");
    json!({
        "id": item.id,
        "guid": item.guid,
        "title": item.title,
        "link": item.link,
        "pubDate": item.published_at.to_rfc3339(),
        "publishedAt": item.published_at.to_rfc3339(),
        "summary": item.summary,
        "author": item.author,
        "sourceId": item.source_id,
        "sourceName": src_name,
        "_sourceId": item.source_id,
        "_feedTitle": src_name,
        "_feedUrl": source_url,
    })
}

fn phantasi_item_to_article_json(
    item: &phantasi_items::Model,
    source: Option<&phantasi_sources::Model>,
) -> Value {
    let content_str = item
        .content
        .as_deref()
        .or(item.summary.as_deref())
        .unwrap_or("");
    let plain_text = extract_plain_text(content_str);
    let source_name = source.map(|s| s.name.clone());
    // Prefer item link as the article URL; fall back to feed URL
    let source_url = if !item.link.is_empty() {
        item.link.clone()
    } else {
        source.map(|s| s.url.clone()).unwrap_or_default()
    };

    json!({
        "id": item.id,
        "guid": item.guid,
        "title": item.title,
        "content": content_str,
        "plainText": plain_text,
        "author": item.author,
        "publishedAt": item.published_at.to_rfc3339(),
        "sourceName": source_name,
        "sourceUrl": source_url,
        "link": item.link,
        "sourceId": item.source_id,
    })
}

/// Whether an item matches article lookup (id / guid / url). Pure helper for tests.
#[cfg(test)]
fn article_lookup_matches(
    lookup: &PhantasiArticleLookup,
    item_id: i32,
    guid: &str,
    link: &str,
) -> bool {
    if let Some(id) = lookup.item_id {
        if item_id == id {
            return true;
        }
    }
    if let Some(ref key) = lookup.article_key {
        if guid == key.as_str() || link == key.as_str() {
            return true;
        }
    }
    if let Some(ref url) = lookup.url {
        if link == url.as_str() || guid == url.as_str() {
            return true;
        }
    }
    false
}

pub(super) async fn execute_phantasi_read(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let filters = parse_phantasi_read_filters(params);
    let is_admin = crate::services::agent::user_is_current_admin(ctx.db, ctx.user_id).await;

    let sources = visible_sources_query(is_admin)
        .all(ctx.db)
        .await
        .map_err(|error| phantasi_query_failed("fetch phantasi sources", error))?;
    let source_map: HashMap<i32, &phantasi_sources::Model> =
        sources.iter().map(|s| (s.id, s)).collect();

    if let Some(sid) = filters.source_id {
        if !source_map.contains_key(&sid) {
            return Ok(json!({
                "items": [],
                "total": 0,
                "sourceId": filters.source_id,
                "sourceName": filters.source_name,
                "matched": false,
                "notFound": true,
                "message": "Feed not found",
            }));
        }
    }

    // Resolve sourceName → source ids (case-insensitive contains)
    let name_source_ids: Option<Vec<i32>> = if let Some(ref name) = filters.source_name {
        let name_lower = name.to_lowercase();
        let ids: Vec<i32> = sources
            .iter()
            .filter(|s| {
                s.name.to_lowercase().contains(&name_lower)
                    || s.url.to_lowercase().contains(&name_lower)
                    || s.site_url
                        .as_deref()
                        .map(|u| u.to_lowercase().contains(&name_lower))
                        .unwrap_or(false)
            })
            .map(|s| s.id)
            .collect();
        Some(ids)
    } else {
        None
    };

    if let Some(ref ids) = name_source_ids {
        if ids.is_empty() && filters.source_id.is_none() {
            return Ok(json!({
                "items": [],
                "total": 0,
                "sourceId": filters.source_id,
                "sourceName": filters.source_name,
                "matched": false,
                "notFound": true,
                "searchedFor": filters.source_name,
                "message": "Feed not found",
            }));
        }
    }

    let mut query = phantasi_items::preview_query(
        phantasi_items::Entity::find()
            .filter(phantasi_items::Column::SourceId.in_subquery(visible_source_ids(is_admin))),
    )
    .order_by_desc(phantasi_items::Column::PublishedAt);

    if let Some(sid) = filters.source_id {
        query = query.filter(phantasi_items::Column::SourceId.eq(sid));
    } else if let Some(ref ids) = name_source_ids {
        if !ids.is_empty() {
            query = query.filter(phantasi_items::Column::SourceId.is_in(ids.clone()));
        }
    }

    if let Some(ref since_str) = filters.since {
        if let Ok(since_dt) = chrono::DateTime::parse_from_rfc3339(since_str) {
            query = query.filter(phantasi_items::Column::PublishedAt.gte(since_dt));
        } else {
            tracing::warn!(since = %since_str, "[phantasi.read] Invalid since (expected RFC3339), ignoring");
        }
    }

    let items = query
        .limit(filters.limit as u64)
        .all(ctx.db)
        .await
        .map_err(|error| phantasi_query_failed("fetch phantasi items", error))?;

    let last_updated = items
        .first()
        .map(|i| i.published_at.to_rfc3339())
        .or_else(|| {
            sources
                .iter()
                .filter_map(|s| s.last_fetched_at)
                .max()
                .map(|t| t.to_rfc3339())
        });

    let out_items: Vec<Value> = items
        .iter()
        .map(|item| phantasi_item_to_read_json(item, source_map.get(&item.source_id).copied()))
        .collect();

    let matched_source_name = filters
        .source_id
        .and_then(|sid| {
            source_map
                .get(&sid)
                .map(|s| s.name.clone())
                .or(filters.source_name.clone())
        })
        .or_else(|| {
            name_source_ids.as_ref().and_then(|ids| {
                ids.first()
                    .and_then(|id| source_map.get(id).map(|s| s.name.clone()))
            })
        });

    Ok(json!({
        "items": out_items,
        "total": out_items.len(),
        "sourceId": filters.source_id,
        "sourceName": matched_source_name,
        "lastUpdated": last_updated,
        "matched": true,
    }))
}

pub(super) async fn user_unread_by_source(
    db: &impl ConnectionTrait,
    user_id: i32,
    is_admin: bool,
) -> Result<HashMap<i32, i32>, String> {
    if user_id <= 0 {
        return Ok(HashMap::new());
    }
    let sql = if is_admin {
        "SELECT i.source_id, COUNT(*)::int AS unread_count \
         FROM phantasi_items i \
         WHERE NOT EXISTS ( \
           SELECT 1 FROM phantasi_user_states s \
           WHERE s.item_id = i.id AND s.user_id = $1 AND s.is_read = TRUE \
         ) \
         GROUP BY i.source_id"
    } else {
        "SELECT i.source_id, COUNT(*)::int AS unread_count \
         FROM phantasi_items i \
         INNER JOIN phantasi_sources src ON src.id = i.source_id AND src.admin_only = FALSE \
         WHERE NOT EXISTS ( \
           SELECT 1 FROM phantasi_user_states s \
           WHERE s.item_id = i.id AND s.user_id = $1 AND s.is_read = TRUE \
         ) \
         GROUP BY i.source_id"
    };
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            [user_id.into()],
        ))
        .await
        .map_err(|error| phantasi_query_failed("count unread by source", error))?;
    let mut map = HashMap::new();
    for row in rows {
        let id: i32 = row.try_get("", "source_id").unwrap_or(0);
        let count: i32 = row.try_get("", "unread_count").unwrap_or(0);
        if id > 0 {
            map.insert(id, count);
        }
    }
    Ok(map)
}

pub(super) async fn execute_phantasi_sources(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    use crate::services::agent::executor::utils::{
        MatchKind, best_loose_match, normalize_phantasi_category_filter,
        normalize_phantasi_source_type_filter, phantasi_category_token_matches,
    };

    let query = params
        .get("query")
        .or_else(|| params.get("name"))
        .or_else(|| params.get("keyword"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let category_filter = params
        .get("category")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(normalize_phantasi_category_filter);
    let source_type_filter = params
        .get("sourceType")
        .or_else(|| params.get("source_type"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(normalize_phantasi_source_type_filter);

    let is_admin = crate::services::agent::user_is_current_admin(ctx.db, ctx.user_id).await;
    let mut source_query =
        phantasi_sources::Entity::find().order_by_asc(phantasi_sources::Column::Name);
    if !is_admin {
        source_query = source_query.filter(phantasi_sources::Column::AdminOnly.eq(false));
    }
    let all_sources = source_query
        .all(ctx.db)
        .await
        .map_err(|error| phantasi_query_failed("fetch phantasi sources", error))?;
    let unread_by_source = user_unread_by_source(ctx.db, ctx.user_id, is_admin).await?;

    let total_in_system = all_sources.len();

    fn feed_type_str(ft: &phantasi_sources::FeedType) -> &'static str {
        match ft {
            phantasi_sources::FeedType::Rss => "rss",
            phantasi_sources::FeedType::Atom => "atom",
            phantasi_sources::FeedType::JsonFeed => "json_feed",
            phantasi_sources::FeedType::Notion => "notion",
            phantasi_sources::FeedType::RssHub => "rsshub",
        }
    }

    fn source_to_json(
        s: &phantasi_sources::Model,
        match_kind: Option<MatchKind>,
        unread: i32,
    ) -> Value {
        let mut obj = json!({
            "id": s.id,
            "name": s.name,
            "url": s.url,
            "siteUrl": s.site_url,
            "category": s.category,
            "sourceType": s.source_type.as_str(),
            "feedType": feed_type_str(&s.feed_type),
            "enabled": s.enabled,
            "itemCount": s.item_count,
            "unreadCount": unread,
            "icon": s.icon,
            "description": s.description,
            // Prefer success timestamp for "latest update" (not last_fetched_at failures)
            "lastSuccessAt": s.last_success_at.map(|t| t.timestamp_millis()),
            "lastFetchedAt": s.last_fetched_at.map(|t| t.timestamp_millis()),
        });
        if let Some(kind) = match_kind {
            obj.as_object_mut()
                .unwrap()
                .insert("matchKind".to_string(), json!(kind.as_str()));
        }
        obj
    }

    // Optional category / sourceType pre-filter (structural, not name search).
    // 友情链接: category aliases (friends/friendlink/友链) normalize to "友情链接";
    // sourceType aliases (friendlink/友情链接/友链) normalize to "link".
    let structurally_filtered: Vec<&phantasi_sources::Model> = all_sources
        .iter()
        .filter(|s| {
            if let Some(ref cat) = category_filter {
                let ok = s
                    .category
                    .as_deref()
                    .map(|c| phantasi_category_token_matches(c, cat))
                    .unwrap_or(false);
                // Friend-link category: also accept pure link sources tagged only via sourceType
                // when category field is empty (legacy rows) — only for the friend-link bucket.
                let ok = if !ok && cat == "友情链接" {
                    s.source_type == phantasi_sources::SourceType::Link
                        && s.category
                            .as_deref()
                            .map(|c| c.trim().is_empty())
                            .unwrap_or(true)
                } else {
                    ok
                };
                if !ok {
                    return false;
                }
            }
            if let Some(ref st) = source_type_filter {
                if s.source_type.as_str() != st.as_str() {
                    return false;
                }
            }
            true
        })
        .collect();

    let Some(needle) = query else {
        let sources: Vec<Value> = structurally_filtered
            .iter()
            .map(|s| source_to_json(s, None, unread_by_source.get(&s.id).copied().unwrap_or(0)))
            .collect();
        return Ok(json!({
            "sources": sources,
            "total": sources.len(),
            "totalInSystem": total_in_system,
            "matched": true,
        }));
    };

    tracing::info!(query = %needle, "[phantasi.sources] Filtering sources by name/keyword");

    let mut scored: Vec<(&phantasi_sources::Model, MatchKind)> = Vec::new();
    for s in &structurally_filtered {
        let fields = [
            s.name.as_str(),
            s.url.as_str(),
            s.site_url.as_deref().unwrap_or(""),
            s.category.as_deref().unwrap_or(""),
            s.description.as_deref().unwrap_or(""),
        ];
        if let Some(kind) = best_loose_match(&fields, needle) {
            scored.push((s, kind));
        }
    }

    // Prefer stronger matches, then name order (already sorted from DB)
    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.name.cmp(&b.0.name)));

    if !scored.is_empty() {
        let sources: Vec<Value> = scored
            .iter()
            .map(|(s, kind)| {
                source_to_json(
                    s,
                    Some(*kind),
                    unread_by_source.get(&s.id).copied().unwrap_or(0),
                )
            })
            .collect();
        return Ok(json!({
            "sources": sources,
            "total": sources.len(),
            "totalInSystem": total_in_system,
            "matched": true,
            "searchedFor": needle,
            "matchKind": scored[0].1.as_str(),
        }));
    }

    // Filter missed — do NOT claim the system has no sources
    let name_pool: Vec<&str> = all_sources
        .iter()
        .filter(|s| !s.name.is_empty())
        .map(|s| s.name.as_str())
        .collect();
    let suggestions: Vec<&str> = name_pool
        .iter()
        .copied()
        .filter(|name| best_loose_match(&[*name], needle).is_some())
        .take(5)
        .collect();
    let suggestions = if suggestions.is_empty() {
        name_pool.into_iter().take(5).collect::<Vec<_>>()
    } else {
        suggestions
    };

    Ok(json!({
        "sources": [],
        "total": 0,
        "totalInSystem": total_in_system,
        "matched": false,
        "notFound": true,
        "searchedFor": needle,
        "suggestions": suggestions,
        "message": if total_in_system == 0 {
            "No feeds are available".to_string()
        } else {
            "Feed not found".to_string()
        },
    }))
}

pub(super) async fn execute_phantasi_items(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    use crate::services::agent::executor::utils::{MatchKind, best_loose_match, loose_text_match};

    let source_id = params.get("sourceId").and_then(|v| v.as_i64());
    let source_name = params.get("sourceName").and_then(|v| v.as_str());
    let author_filter = params.get("author").and_then(|v| v.as_str());
    let name_param = params.get("name").and_then(|v| v.as_str());
    let query_param = params.get("query").and_then(|v| v.as_str());
    let keyword_param = params.get("keyword").and_then(|v| v.as_str());
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
    let action_type = params.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let select_first = params
        .get("selectFirst")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let open_article = params
        .get("openArticle")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // 统一获取过滤名称
    let filter_target = author_filter
        .or(source_name)
        .or(name_param)
        .or(query_param)
        .or(keyword_param);

    // 检测是否是"通用最新文章"请求
    let is_generic_latest_request = {
        let is_generic_keyword = filter_target
            .map(|t| {
                let t_lower = t.to_lowercase();
                t_lower == "latest_article"
                    || t_lower == "latest"
                    || t_lower == "newest"
                    || t_lower == "recent"
                    || t_lower == "最新"
                    || t_lower == "最新文章"
                    || t_lower == "最新的文章"
                    || t_lower == "latest articles"
                    || t_lower == "最近"
                    || t_lower == "最近文章"
                    || t_lower.contains("latest_article")
                    || t_lower.contains("newest_article")
            })
            .unwrap_or(false);
        let is_select_first_only = select_first && filter_target.is_none() && source_id.is_none();
        is_generic_keyword || is_select_first_only
    };

    let filter_target = if is_generic_latest_request {
        tracing::info!(original_filter = ?filter_target, "[phantasi.items] Detected generic latest request, ignoring filter");
        None
    } else {
        filter_target
    };

    // 只读可见订阅源；文章按 PublishedAt 降序最多 500 条
    let mut all_items: Vec<Value> = Vec::new();
    let mut available_sources: Vec<String> = Vec::new();
    let mut all_authors: Vec<String> = Vec::new();

    let is_admin = crate::services::agent::user_is_current_admin(ctx.db, ctx.user_id).await;
    let sources = visible_sources_query(is_admin)
        .all(ctx.db)
        .await
        .map_err(|error| phantasi_query_failed("fetch phantasi sources", error))?;
    let source_map: std::collections::HashMap<i32, &phantasi_sources::Model> =
        sources.iter().map(|s| (s.id, s)).collect();

    for source in &sources {
        if !source.name.is_empty() {
            available_sources.push(source.name.clone());
        }
    }

    let items = phantasi_items::preview_query(
        phantasi_items::Entity::find()
            .filter(phantasi_items::Column::SourceId.in_subquery(visible_source_ids(is_admin))),
    )
    .order_by_desc(phantasi_items::Column::PublishedAt)
    .limit(500)
    .all(ctx.db)
    .await
    .map_err(|error| phantasi_query_failed("fetch phantasi items", error))?;

    for item in items {
        let source = source_map.get(&item.source_id);
        let src_name = source.map(|s| s.name.as_str()).unwrap_or("");
        let source_url = source.map(|s| s.url.as_str()).unwrap_or("");

        if let Some(author) = &item.author {
            if !author.is_empty() && !all_authors.contains(author) {
                all_authors.push(author.clone());
            }
        }

        all_items.push(json!({
            "id": item.id,
            "guid": item.guid,
            "title": item.title,
            "link": item.link,
            "pubDate": item.published_at.to_rfc3339(),
            "summary": item.summary,
            "author": item.author,
            "_sourceId": item.source_id,
            "_feedTitle": src_name,
            "_feedUrl": source_url,
        }));
    }

    // Prefer explicit sourceId (from phantasi.sources → phantasi.items pipeline)
    let mut matched_source: Option<String> = None;
    let mut match_kind: Option<&'static str> = None;

    if let Some(sid) = source_id {
        all_items.retain(|item| {
            item.get("_sourceId")
                .and_then(|v| v.as_i64())
                .map(|id| id == sid)
                .unwrap_or(false)
        });
        if let Some(src) = source_map.get(&(sid as i32)) {
            matched_source = Some(src.name.clone());
            match_kind = Some("sourceId");
        }
    }

    // Name/author filter only when not already scoped by sourceId
    if source_id.is_none() {
        if let Some(name) = filter_target {
            let name_lower = name.to_lowercase();
            tracing::info!(filter = %name, "[phantasi.items] Filtering by source/author");

            let strict_matches: Vec<Value> = all_items
                .iter()
                .filter(|item| {
                    let author = item.get("author").and_then(|v| v.as_str()).unwrap_or("");
                    let feed_title = item
                        .get("_feedTitle")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    feed_title.to_lowercase().contains(&name_lower)
                        || author.to_lowercase().contains(&name_lower)
                })
                .cloned()
                .collect();

            if !strict_matches.is_empty() {
                all_items = strict_matches;
                match_kind = Some("contains");
                if let Some(first) = all_items.first() {
                    let feed_title = first
                        .get("_feedTitle")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let author = first.get("author").and_then(|v| v.as_str()).unwrap_or("");
                    matched_source = Some(if !feed_title.is_empty() {
                        feed_title.to_string()
                    } else if !author.is_empty() {
                        author.to_string()
                    } else {
                        "unknown".to_string()
                    });
                }
            } else {
                // 尝试匹配 feed_url
                let url_matches: Vec<Value> = all_items
                    .iter()
                    .filter(|item| {
                        let feed_url = item.get("_feedUrl").and_then(|v| v.as_str()).unwrap_or("");
                        feed_url.to_lowercase().contains(&name_lower)
                    })
                    .cloned()
                    .collect();

                if !url_matches.is_empty() {
                    all_items = url_matches;
                    match_kind = Some("contains");
                    if let Some(first) = all_items.first() {
                        let feed_title = first
                            .get("_feedTitle")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        matched_source = Some(if !feed_title.is_empty() {
                            feed_title.to_string()
                        } else {
                            "unknown".to_string()
                        });
                    }
                } else {
                    // Soft match: loose name match on feed titles / source names
                    let mut soft_hits: Vec<(String, MatchKind)> = Vec::new();
                    for src_name in &available_sources {
                        if let Some(kind) = loose_text_match(src_name, name) {
                            if !soft_hits.iter().any(|(n, _)| n == src_name) {
                                soft_hits.push((src_name.clone(), kind));
                            }
                        }
                    }
                    // Also soft-match authors that appear on items
                    for author in &all_authors {
                        if let Some(kind) = loose_text_match(author, name) {
                            if !soft_hits.iter().any(|(n, _)| n == author) {
                                soft_hits.push((author.clone(), kind));
                            }
                        }
                    }
                    soft_hits.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

                    if soft_hits.len() == 1 {
                        let (picked, kind) = &soft_hits[0];
                        let picked_lower = picked.to_lowercase();
                        tracing::info!(
                            filter = %name,
                            picked = %picked,
                            match_kind = %kind.as_str(),
                            "[phantasi.items] Soft-matched single high-confidence source/author"
                        );
                        all_items = all_items
                            .iter()
                            .filter(|item| {
                                let feed_title = item
                                    .get("_feedTitle")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_lowercase();
                                let author = item
                                    .get("author")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_lowercase();
                                feed_title == picked_lower || author == picked_lower
                            })
                            .cloned()
                            .collect();
                        matched_source = Some(picked.clone());
                        match_kind = Some(kind.as_str());
                    } else if soft_hits.len() > 1 {
                        // Multiple close names → ambiguous; do not auto-pick
                        let suggestions: Vec<&str> =
                            soft_hits.iter().map(|(n, _)| n.as_str()).take(5).collect();
                        let choices: Vec<Value> = suggestions
                            .iter()
                            .map(|s| json!({ "value": s, "label": *s }))
                            .collect();
                        return Ok(json!({
                            "items": [],
                            "total": 0,
                            "sourceId": source_id,
                            "notFound": false,
                            "searchedFor": name,
                            "ambiguous": true,
                            "ambiguous_reason": crate::services::agent::response_agent::feed_ambiguous(name),
                            "suggestions": suggestions,
                            "choices": choices,
                            "availableSources": available_sources.iter().filter(|s| !s.is_empty()).collect::<Vec<_>>(),
                            "hint": crate::services::agent::response_agent::feed_hint(&suggestions.join(", "))
                        }));
                    } else {
                        all_items.clear();
                    }
                }
            }

            if all_items.is_empty() {
                let valid_sources: Vec<&String> =
                    available_sources.iter().filter(|s| !s.is_empty()).collect();
                let valid_authors: Vec<&String> =
                    all_authors.iter().filter(|s| !s.is_empty()).collect();

                let suggestions: Vec<&str> = valid_sources
                    .iter()
                    .chain(valid_authors.iter())
                    .filter(|s| best_loose_match(&[s.as_str()], name).is_some())
                    .map(|s| s.as_str())
                    .take(5)
                    .collect();

                let choices: Vec<Value> = if !suggestions.is_empty() {
                    suggestions
                        .iter()
                        .map(|s| json!({ "value": s, "label": *s }))
                        .collect()
                } else {
                    valid_sources
                        .iter()
                        .chain(valid_authors.iter())
                        .take(5)
                        .map(|s| json!({ "value": s, "label": s }))
                        .collect()
                };

                // No close multi-match → not ambiguous; filter simply missed
                return Ok(json!({
                    "items": [],
                    "total": 0,
                    "sourceId": source_id,
                    "notFound": true,
                    "searchedFor": name,
                    "ambiguous": false,
                    "suggestions": suggestions,
                    "choices": choices,
                    "availableSources": valid_sources,
                    "hint": crate::services::agent::response_agent::feed_hint(&suggestions.join(", "))
                }));
            }
        }
    }

    all_items.truncate(limit);

    // 如果需要打开第一篇文章
    let should_open_first =
        (action_type == "navigate" || select_first || open_article || is_generic_latest_request)
            && !all_items.is_empty();

    if should_open_first {
        let first_item = &all_items[0];
        let article_id = first_item
            .get("id")
            .map(|v| {
                if let Some(n) = v.as_i64() {
                    n.to_string()
                } else {
                    v.as_str().unwrap_or("").to_string()
                }
            })
            .unwrap_or_default();
        let article_link = first_item
            .get("link")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let article_title = first_item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        Ok(json!({
            "items": all_items,
            "total": all_items.len(),
            "sourceId": source_id,
            "matchedSource": matched_source,
            "matchKind": match_kind,
            "frontendAction": {
                "type": "phantasi_open_article",
                "timestamp": chrono::Utc::now().timestamp_millis(),
                "params": {
                    "articleId": article_id,
                    "articleLink": article_link,
                    "openLatest": true,
                    "openReader": true
                }
            },
            "articleInfo": {
                "id": article_id,
                "link": article_link,
                "title": article_title
            }
        }))
    } else {
        Ok(json!({
            "items": all_items,
            "total": all_items.len(),
            "sourceId": source_id,
            "matchedSource": matched_source,
            "matchKind": match_kind,
        }))
    }
}

async fn load_article_with_source(
    ctx: &HandlerContext<'_>,
    item: phantasi_items::Model,
) -> Result<Value, String> {
    let is_admin = crate::services::agent::user_is_current_admin(ctx.db, ctx.user_id).await;
    let source = phantasi_sources::Entity::find_by_id(item.source_id)
        .one(ctx.db)
        .await
        .map_err(|error| phantasi_query_failed("fetch phantasi source", error))?;
    match source {
        Some(ref src) if source_visible(src, is_admin) => {
            Ok(phantasi_item_to_article_json(&item, Some(src)))
        }
        _ => Err("Article not found".to_string()),
    }
}

pub(super) async fn execute_phantasi_article(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let lookup = parse_phantasi_article_lookup(params);

    if lookup.item_id.is_none() && lookup.article_key.is_none() && lookup.url.is_none() {
        return Err("Missing article lookup: provide articleId (id/guid) or url/link".to_string());
    }

    // Build OR conditions for id / guid / link
    let mut cond = sea_orm::Condition::any();
    let mut has_key = false;

    if let Some(id) = lookup.item_id {
        cond = cond.add(phantasi_items::Column::Id.eq(id));
        let id_str = id.to_string();
        cond = cond.add(phantasi_items::Column::Guid.eq(id_str.clone()));
        cond = cond.add(phantasi_items::Column::Link.eq(id_str));
        has_key = true;
    }
    if let Some(ref key) = lookup.article_key {
        cond = cond.add(phantasi_items::Column::Guid.eq(key.clone()));
        cond = cond.add(phantasi_items::Column::Link.eq(key.clone()));
        has_key = true;
    }
    if let Some(ref url) = lookup.url {
        cond = cond.add(phantasi_items::Column::Link.eq(url.clone()));
        cond = cond.add(phantasi_items::Column::Guid.eq(url.clone()));
        has_key = true;
    }

    if !has_key {
        return Err("Missing article lookup: provide articleId (id/guid) or url/link".to_string());
    }

    let mut q = phantasi_items::Entity::find().filter(cond);
    if let Some(sid) = lookup.source_id {
        q = q.filter(phantasi_items::Column::SourceId.eq(sid));
    }

    // Prefer newest match if multiple (e.g. same link re-ingested)
    let item = q
        .order_by_desc(phantasi_items::Column::PublishedAt)
        .one(ctx.db)
        .await
        .map_err(|error| phantasi_query_failed("fetch phantasi article", error))?;

    match item {
        Some(item) => load_article_with_source(ctx, item).await,
        None => Err("Article not found".to_string()),
    }
}

pub(super) async fn execute_phantasi_stats(
    _params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let user_id = ctx.user_id;
    let is_admin = crate::services::agent::user_is_current_admin(ctx.db, user_id).await;
    // One statement returns all four counters. Reading stats only apply to a
    // signed-in subject ($1 > 0); starred stays admin-only as before.
    let stats_sql = if is_admin {
        "SELECT \
           (SELECT COUNT(*) FROM phantasi_sources)::int AS total_sources, \
           (SELECT COALESCE(SUM(item_count), 0) FROM phantasi_sources)::int AS total_items, \
           (CASE WHEN $1 > 0 THEN ( \
             SELECT COUNT(*) FROM phantasi_items i \
             WHERE NOT EXISTS ( \
               SELECT 1 FROM phantasi_user_states s \
               WHERE s.item_id = i.id AND s.user_id = $1 AND s.is_read = TRUE \
             ) \
           ) ELSE 0 END)::int AS unread_count, \
           (CASE WHEN $1 > 0 THEN ( \
             SELECT COUNT(*) FROM phantasi_user_states s \
             INNER JOIN phantasi_items i ON i.id = s.item_id \
             WHERE s.user_id = $1 AND s.is_starred = TRUE \
           ) ELSE 0 END)::int AS starred_count"
    } else {
        "WITH src AS (SELECT id, item_count FROM phantasi_sources WHERE admin_only = FALSE) \
         SELECT \
           (SELECT COUNT(*) FROM src)::int AS total_sources, \
           (SELECT COALESCE(SUM(item_count), 0) FROM src)::int AS total_items, \
           (CASE WHEN $1 > 0 THEN ( \
             SELECT COUNT(*) FROM phantasi_items i \
             INNER JOIN src ON src.id = i.source_id \
             WHERE NOT EXISTS ( \
               SELECT 1 FROM phantasi_user_states s \
               WHERE s.item_id = i.id AND s.user_id = $1 AND s.is_read = TRUE \
             ) \
           ) ELSE 0 END)::int AS unread_count, \
           0::int AS starred_count"
    };
    let row = ctx
        .db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            stats_sql,
            [user_id.into()],
        ))
        .await
        .map_err(|error| phantasi_query_failed("count sources", error))?
        .ok_or_else(|| phantasi_query_failed("count sources", "empty totals"))?;
    let count = |column: &'static str| {
        row.try_get::<i32>("", column)
            .map(i64::from)
            .map_err(|error| phantasi_query_failed("read phantasi stats", error))
    };

    Ok(json!({
        "totalSources": count("total_sources")?,
        "totalItems": count("total_items")?,
        "unreadCount": count("unread_count")?,
        "starredCount": count("starred_count")?,
        "userId": if user_id > 0 { Value::from(user_id) } else { Value::Null },
    }))
}

/// 从 HTML 中提取纯文本
fn extract_plain_text(html: &str) -> String {
    // 移除 HTML 标签
    let text = RE_HTML_TAG.replace_all(html, " ");

    // 解码常见 HTML 实体
    let text = text
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"");

    // 压缩空白
    RE_WHITESPACE.replace_all(&text, " ").trim().to_string()
}

#[cfg(test)]
mod phantasi_db_helpers_tests {
    use super::super::permission::install_permission_is_granted;
    use super::super::phantasi_generate::{outbound_web_search_allowed, parse_allow_web_search};
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_optional_i32_accepts_int_and_numeric_string() {
        assert_eq!(parse_optional_i32(&json!(42)), Some(42));
        assert_eq!(parse_optional_i32(&json!(42u64)), Some(42));
        assert_eq!(parse_optional_i32(&json!("7")), Some(7));
        assert_eq!(parse_optional_i32(&json!("  9  ")), Some(9));
        assert_eq!(parse_optional_i32(&json!("akiday")), None);
        assert_eq!(parse_optional_i32(&json!(null)), None);
    }

    #[test]
    fn phantasi_read_filters_align_source_and_source_id() {
        let mut params = HashMap::new();
        params.insert("sourceId".into(), json!(3));
        params.insert("limit".into(), json!(10));
        let f = parse_phantasi_read_filters(&params);
        assert_eq!(f.source_id, Some(3));
        assert_eq!(f.limit, 10);
        assert!(f.source_name.is_none());

        let mut params = HashMap::new();
        params.insert("source".into(), json!("12"));
        let f = parse_phantasi_read_filters(&params);
        assert_eq!(f.source_id, Some(12));
        assert!(f.source_name.is_none());

        let mut params = HashMap::new();
        params.insert("source".into(), json!("akiday"));
        let f = parse_phantasi_read_filters(&params);
        assert!(f.source_id.is_none());
        assert_eq!(f.source_name.as_deref(), Some("akiday"));

        let mut params = HashMap::new();
        params.insert("sourceName".into(), json!("天利"));
        params.insert("sourceId".into(), json!("5"));
        let f = parse_phantasi_read_filters(&params);
        assert_eq!(f.source_id, Some(5));
        assert_eq!(f.source_name.as_deref(), Some("天利"));

        let mut params = HashMap::new();
        params.insert("limit".into(), json!(9999));
        let f = parse_phantasi_read_filters(&params);
        assert_eq!(f.limit, 200); // clamped
    }

    #[test]
    fn phantasi_article_lookup_id_guid_url() {
        let mut params = HashMap::new();
        params.insert("articleId".into(), json!(101));
        let l = parse_phantasi_article_lookup(&params);
        assert_eq!(l.item_id, Some(101));
        assert!(l.url.is_none());

        let mut params = HashMap::new();
        params.insert("articleId".into(), json!("guid-abc"));
        let l = parse_phantasi_article_lookup(&params);
        assert!(l.item_id.is_none());
        assert_eq!(l.article_key.as_deref(), Some("guid-abc"));

        let mut params = HashMap::new();
        params.insert("url".into(), json!("https://example.com/post"));
        params.insert("sourceId".into(), json!(2));
        let l = parse_phantasi_article_lookup(&params);
        assert_eq!(l.url.as_deref(), Some("https://example.com/post"));
        assert_eq!(l.source_id, Some(2));

        let mut params = HashMap::new();
        params.insert("itemId".into(), json!("55"));
        let l = parse_phantasi_article_lookup(&params);
        assert_eq!(l.item_id, Some(55));
    }

    #[test]
    fn article_lookup_matches_by_id_guid_or_link() {
        let by_id = PhantasiArticleLookup {
            item_id: Some(7),
            article_key: None,
            url: None,
            source_id: None,
        };
        assert!(article_lookup_matches(&by_id, 7, "g", "https://x"));
        assert!(!article_lookup_matches(&by_id, 8, "g", "https://x"));

        let by_guid = PhantasiArticleLookup {
            item_id: None,
            article_key: Some("guid-1".into()),
            url: None,
            source_id: None,
        };
        assert!(article_lookup_matches(
            &by_guid,
            1,
            "guid-1",
            "https://other"
        ));
        assert!(article_lookup_matches(
            &by_guid, 1, "other", "guid-1" // key also matches link
        ));

        let by_url = PhantasiArticleLookup {
            item_id: None,
            article_key: None,
            url: Some("https://example.com/a".into()),
            source_id: None,
        };
        assert!(article_lookup_matches(
            &by_url,
            1,
            "x",
            "https://example.com/a"
        ));
        assert!(!article_lookup_matches(
            &by_url,
            1,
            "x",
            "https://example.com/b"
        ));
    }

    #[test]
    fn extract_plain_text_strips_tags() {
        let plain = extract_plain_text("<p>Hello&nbsp;<b>world</b></p>");
        assert_eq!(plain, "Hello world");
    }

    #[test]
    fn allow_web_search_is_opt_in_only() {
        let empty = HashMap::new();
        assert!(!parse_allow_web_search(&empty));

        let mut params = HashMap::new();
        params.insert("allowWebSearch".into(), json!(false));
        assert!(!parse_allow_web_search(&params));

        let mut params = HashMap::new();
        params.insert("allowWebSearch".into(), json!(true));
        assert!(parse_allow_web_search(&params));

        let mut params = HashMap::new();
        params.insert("useWebSearch".into(), json!("yes"));
        assert!(parse_allow_web_search(&params));

        let mut params = HashMap::new();
        params.insert("webSearch".into(), json!(1));
        assert!(parse_allow_web_search(&params));

        let mut params = HashMap::new();
        params.insert("external".into(), json!("on"));
        assert!(parse_allow_web_search(&params));

        // Explicit false / garbage must not enable
        let mut params = HashMap::new();
        params.insert("webSearch".into(), json!("no"));
        assert!(!parse_allow_web_search(&params));
    }

    #[test]
    fn outbound_web_search_requires_granted_ai_search() {
        assert!(!outbound_web_search_allowed(false, true, None));
        assert!(!outbound_web_search_allowed(true, false, None));
        assert!(outbound_web_search_allowed(true, true, None));
        assert!(
            !outbound_web_search_allowed(true, true, Some(&["phantasi:read".to_string()])),
            "autonomy cap without ai:search must not outbound"
        );
        assert!(outbound_web_search_allowed(
            true,
            true,
            Some(&["phantasi:read".to_string(), "ai:search".to_string()])
        ));
    }

    #[test]
    fn phantasi_stats_and_source_unread_match_host_sql() {
        let src = include_str!("phantasi.rs");
        let stats = {
            let start = src
                .find("pub(super) async fn execute_phantasi_stats")
                .expect("execute_phantasi_stats");
            let body = &src[start..];
            let end = body[1..]
                .find("\nfn extract_plain_text")
                .or_else(|| body[1..].find("\npub(super) async fn "))
                .map(|index| index + 1)
                .unwrap_or(body.len());
            &body[..end]
        };
        let unread = {
            let start = src
                .find("async fn user_unread_by_source")
                .expect("user_unread_by_source");
            let body = &src[start..];
            let end = body[1..]
                .find("\npub(super) async fn ")
                .map(|index| index + 1)
                .unwrap_or(body.len());
            &body[..end]
        };
        assert!(stats.contains("SUM(item_count)"));
        assert!(stats.contains("NOT EXISTS"));
        assert!(unread.contains("NOT EXISTS"));
        assert!(unread.contains("s.is_read = TRUE"));
        assert!(
            !stats.contains("phantasi_items::Entity::find()"),
            "agent stats must not materialize item IDs"
        );
        assert!(stats.contains("admin_only = FALSE"));
        assert!(stats.contains("CASE WHEN $1 > 0"));
        assert!(!stats.contains("total_items, 0"));
        assert!(stats.contains("phantasi_query_failed(\"count sources\""));
        assert_eq!(
            stats.matches("query_one_raw").count(),
            1,
            "agent stats must be one aggregate statement"
        );
        assert!(
            !stats.contains(".ok()"),
            "agent stats must not swallow store errors as zeros"
        );
        assert!(
            !stats.contains("unwrap_or"),
            "agent stats must not default failed aggregates to 0"
        );
    }

    #[test]
    fn article_lookup_does_not_map_store_to_not_found() {
        let src = include_str!("phantasi.rs");
        let start = src
            .find("pub(super) async fn execute_phantasi_article")
            .expect("article");
        let body = &src[start..];
        let end = body[1..]
            .find("\npub(super) async fn ")
            .map(|index| index + 1)
            .unwrap_or(body.len());
        let article = &body[..end];
        assert!(article.contains("phantasi_query_failed(\"fetch phantasi article\""));
        assert!(
            !article.contains("\"Article not found\".to_string()\n        })?"),
            "store errors must not be disguised as missing articles"
        );
        assert!(
            article.contains("None => Err(\"Article not found\""),
            "a missing row is still not found"
        );
    }

    #[test]
    fn items_catalog_load_does_not_swallow_store_errors() {
        let src = include_str!("phantasi.rs");
        let start = src
            .find("pub(super) async fn execute_phantasi_items")
            .expect("items");
        let body = &src[start..];
        let end = body[1..]
            .find("\npub(super) async fn ")
            .or_else(|| body[1..].find("\nasync fn "))
            .map(|index| index + 1)
            .unwrap_or(body.len());
        let items = &body[..end];
        assert!(items.contains("phantasi_query_failed(\"fetch phantasi sources\""));
        assert!(items.contains("phantasi_query_failed(\"fetch phantasi items\""));
        assert!(
            items.contains("preview_query"),
            "items catalog must not load full content columns"
        );
        let sources_fetch = items
            .split("visible_sources_query(is_admin)")
            .nth(1)
            .unwrap_or("");
        let sources_fetch = sources_fetch.split("let source_map").next().unwrap_or("");
        assert!(
            !sources_fetch.contains("unwrap_or_default"),
            "item catalog load must not swallow store errors"
        );
    }

    #[test]
    fn agent_read_paths_use_visible_sources() {
        let src = include_str!("phantasi.rs");
        for name in [
            "pub(super) async fn execute_phantasi_read",
            "pub(super) async fn execute_phantasi_items",
            "async fn load_article_with_source",
            "pub(super) async fn execute_phantasi_stats",
        ] {
            let start = src.find(name).unwrap_or_else(|| panic!("{name}"));
            let body = &src[start..];
            let end = body[1..]
                .find("\npub(super) async fn ")
                .or_else(|| body[1..].find("\nasync fn "))
                .map(|index| index + 1)
                .unwrap_or(body.len());
            let fn_body = &body[..end];
            assert!(
                fn_body.contains("visible_sources_query")
                    || fn_body.contains("visible_source_ids")
                    || fn_body.contains("source_visible")
                    || fn_body.contains("admin_only = FALSE"),
                "{name} must apply admin_only visibility"
            );
            if name.contains("execute_phantasi_read") || name.contains("execute_phantasi_items") {
                assert!(
                    fn_body.contains("preview_query"),
                    "{name} must not load full content columns"
                );
            }
        }
        let read = {
            let start = src
                .find("fn phantasi_item_to_read_json")
                .expect("read json");
            &src[start..src.find("fn phantasi_item_to_article_json").unwrap()]
        };
        assert!(!read.contains("item.content"));
        let generate = include_str!("phantasi_generate.rs");
        assert!(generate.contains("visible_sources_query"));
        assert!(generate.contains("visible_source_ids"));
        let page = include_str!("pages.rs");
        assert!(page.contains("visible_sources_query"));
        assert!(page.contains("source_visible"));
        assert!(page.contains("user_unread_by_source"));
    }

    #[test]
    fn marked_install_permission_probe_is_never_granted() {
        assert!(
            !install_permission_is_granted(true, true),
            "marker must empty the granted layer even when the approved name still lists"
        );
        assert!(!install_permission_is_granted(true, false));
        assert!(install_permission_is_granted(false, true));
        assert!(!install_permission_is_granted(false, false));
    }
}
