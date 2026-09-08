use super::super::HandlerContext;
use crate::models::entities::{brew_items, brew_sources, brew_user_states};
use once_cell::sync::Lazy;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};
use serde_json::{json, Value};
use std::collections::HashMap;

static RE_HTML_TAG: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"<[^>]+>").unwrap());
static RE_WHITESPACE: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"\s+").unwrap());

pub(super) fn brew_query_failed(context: &'static str, error: impl std::fmt::Display) -> String {
    tracing::error!(%error, context, "brew query failed");
    format!("Failed to {context}")
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

/// Filters extracted from brew.read params (sourceId / source / sourceName / limit / since).
#[derive(Debug, Clone, PartialEq, Eq)]
struct BrewReadFilters {
    source_id: Option<i32>,
    source_name: Option<String>,
    limit: usize,
    since: Option<String>,
}

/// Align schema `source` with handler `sourceId` / `sourceName`.
/// - `sourceId` (int or numeric string) wins for id
/// - numeric `source` also resolves as id
/// - non-numeric `source` / `sourceName` resolve as name filter
fn parse_brew_read_filters(params: &HashMap<String, Value>) -> BrewReadFilters {
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

    BrewReadFilters {
        source_id,
        source_name,
        limit,
        since,
    }
}

/// Resolve brew.article lookup keys: id (i32), guid/string id, or url/link.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BrewArticleLookup {
    item_id: Option<i32>,
    /// Raw articleId when not purely numeric (guid / link fallback)
    article_key: Option<String>,
    url: Option<String>,
    source_id: Option<i32>,
}

fn parse_brew_article_lookup(params: &HashMap<String, Value>) -> BrewArticleLookup {
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

    BrewArticleLookup {
        item_id,
        article_key,
        url,
        source_id,
    }
}

fn brew_item_to_read_json(item: &brew_items::Model, source: Option<&brew_sources::Model>) -> Value {
    let src_name = source.map(|s| s.name.as_str()).unwrap_or("");
    let source_url = source.map(|s| s.url.as_str()).unwrap_or("");
    json!({
        "id": item.id,
        "guid": item.guid,
        "title": item.title,
        "link": item.link,
        "pubDate": item.published_at.to_rfc3339(),
        "publishedAt": item.published_at.to_rfc3339(),
        "content": item.content,
        "summary": item.summary,
        "author": item.author,
        "sourceId": item.source_id,
        "sourceName": src_name,
        "_sourceId": item.source_id,
        "_feedTitle": src_name,
        "_feedUrl": source_url,
    })
}

fn brew_item_to_article_json(
    item: &brew_items::Model,
    source: Option<&brew_sources::Model>,
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
    lookup: &BrewArticleLookup,
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

pub(super) async fn execute_brew_read(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let filters = parse_brew_read_filters(params);

    let sources = brew_sources::Entity::find()
        .all(ctx.db)
        .await
        .map_err(|error| brew_query_failed("fetch brew sources", error))?;
    let source_map: HashMap<i32, &brew_sources::Model> =
        sources.iter().map(|s| (s.id, s)).collect();

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

    let mut query = brew_items::Entity::find().order_by_desc(brew_items::Column::PublishedAt);

    if let Some(sid) = filters.source_id {
        query = query.filter(brew_items::Column::SourceId.eq(sid));
    } else if let Some(ref ids) = name_source_ids {
        if !ids.is_empty() {
            query = query.filter(brew_items::Column::SourceId.is_in(ids.clone()));
        }
    }

    if let Some(ref since_str) = filters.since {
        if let Ok(since_dt) = chrono::DateTime::parse_from_rfc3339(since_str) {
            query = query.filter(brew_items::Column::PublishedAt.gte(since_dt));
        } else {
            tracing::warn!(since = %since_str, "[brew.read] Invalid since (expected RFC3339), ignoring");
        }
    }

    let items = query
        .limit(filters.limit as u64)
        .all(ctx.db)
        .await
        .map_err(|error| brew_query_failed("fetch brew items", error))?;

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
        .map(|item| brew_item_to_read_json(item, source_map.get(&item.source_id).copied()))
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

pub(super) async fn execute_brew_sources(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    use crate::services::agent::executor::utils::{
        best_loose_match, brew_category_token_matches, normalize_brew_category_filter,
        normalize_brew_source_type_filter, MatchKind,
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
        .map(normalize_brew_category_filter);
    let source_type_filter = params
        .get("sourceType")
        .or_else(|| params.get("source_type"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(normalize_brew_source_type_filter);

    let all_sources = brew_sources::Entity::find()
        .order_by_asc(brew_sources::Column::Name)
        .all(ctx.db)
        .await
        .map_err(|error| brew_query_failed("fetch brew sources", error))?;

    let total_in_system = all_sources.len();

    fn feed_type_str(ft: &brew_sources::FeedType) -> &'static str {
        match ft {
            brew_sources::FeedType::Rss => "rss",
            brew_sources::FeedType::Atom => "atom",
            brew_sources::FeedType::JsonFeed => "json_feed",
            brew_sources::FeedType::Notion => "notion",
            brew_sources::FeedType::RssHub => "rsshub",
        }
    }

    fn source_to_json(s: &brew_sources::Model, match_kind: Option<MatchKind>) -> Value {
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
            "unreadCount": s.unread_count,
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
    let structurally_filtered: Vec<&brew_sources::Model> = all_sources
        .iter()
        .filter(|s| {
            if let Some(ref cat) = category_filter {
                let ok = s
                    .category
                    .as_deref()
                    .map(|c| brew_category_token_matches(c, cat))
                    .unwrap_or(false);
                // Friend-link category: also accept pure link sources tagged only via sourceType
                // when category field is empty (legacy rows) — only for the friend-link bucket.
                let ok = if !ok && cat == "友情链接" {
                    s.source_type == brew_sources::SourceType::Link
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
            .map(|s| source_to_json(s, None))
            .collect();
        return Ok(json!({
            "sources": sources,
            "total": sources.len(),
            "totalInSystem": total_in_system,
            "matched": true,
        }));
    };

    tracing::info!(query = %needle, "[brew.sources] Filtering sources by name/keyword");

    let mut scored: Vec<(&brew_sources::Model, MatchKind)> = Vec::new();
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
            .map(|(s, kind)| source_to_json(s, Some(*kind)))
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

pub(super) async fn execute_brew_items(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    use crate::services::agent::executor::utils::{best_loose_match, loose_text_match, MatchKind};

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
        tracing::info!(original_filter = ?filter_target, "[brew.items] Detected generic latest request, ignoring filter");
        None
    } else {
        filter_target
    };

    // 从数据库读取所有订阅源和文章
    let mut all_items: Vec<Value> = Vec::new();
    let mut available_sources: Vec<String> = Vec::new();
    let mut all_authors: Vec<String> = Vec::new();

    let sources = brew_sources::Entity::find()
        .all(ctx.db)
        .await
        .unwrap_or_default();
    let source_map: std::collections::HashMap<i32, &brew_sources::Model> =
        sources.iter().map(|s| (s.id, s)).collect();

    for source in &sources {
        if !source.name.is_empty() {
            available_sources.push(source.name.clone());
        }
    }

    let items = brew_items::Entity::find()
        .order_by_desc(brew_items::Column::PublishedAt)
        .limit(500)
        .all(ctx.db)
        .await
        .unwrap_or_default();

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
            "content": item.content,
            "author": item.author,
            "_sourceId": item.source_id,
            "_feedTitle": src_name,
            "_feedUrl": source_url,
        }));
    }

    // Prefer explicit sourceId (from brew.sources → brew.items pipeline)
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
            tracing::info!(filter = %name, "[brew.items] Filtering by source/author");

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
                            "[brew.items] Soft-matched single high-confidence source/author"
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
                "type": "brew_open_article",
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
    item: brew_items::Model,
) -> Result<Value, String> {
    let source = brew_sources::Entity::find_by_id(item.source_id)
        .one(ctx.db)
        .await
        .map_err(|error| brew_query_failed("fetch brew source", error))?;
    Ok(brew_item_to_article_json(&item, source.as_ref()))
}

pub(super) async fn execute_brew_article(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let lookup = parse_brew_article_lookup(params);

    if lookup.item_id.is_none() && lookup.article_key.is_none() && lookup.url.is_none() {
        return Err("Missing article lookup: provide articleId (id/guid) or url/link".to_string());
    }

    // Build OR conditions for id / guid / link
    let mut cond = sea_orm::Condition::any();
    let mut has_key = false;

    if let Some(id) = lookup.item_id {
        cond = cond.add(brew_items::Column::Id.eq(id));
        let id_str = id.to_string();
        cond = cond.add(brew_items::Column::Guid.eq(id_str.clone()));
        cond = cond.add(brew_items::Column::Link.eq(id_str));
        has_key = true;
    }
    if let Some(ref key) = lookup.article_key {
        cond = cond.add(brew_items::Column::Guid.eq(key.clone()));
        cond = cond.add(brew_items::Column::Link.eq(key.clone()));
        has_key = true;
    }
    if let Some(ref url) = lookup.url {
        cond = cond.add(brew_items::Column::Link.eq(url.clone()));
        cond = cond.add(brew_items::Column::Guid.eq(url.clone()));
        has_key = true;
    }

    if !has_key {
        return Err("Missing article lookup: provide articleId (id/guid) or url/link".to_string());
    }

    let mut q = brew_items::Entity::find().filter(cond);
    if let Some(sid) = lookup.source_id {
        q = q.filter(brew_items::Column::SourceId.eq(sid));
    }

    // Prefer newest match if multiple (e.g. same link re-ingested)
    let item = q
        .order_by_desc(brew_items::Column::PublishedAt)
        .one(ctx.db)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Agent brew article fetch failed");
            "Article not found".to_string()
        })?;

    match item {
        Some(item) => load_article_with_source(ctx, item).await,
        None => Err("Article not found".to_string()),
    }
}

pub(super) async fn execute_brew_stats(
    _params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let total_sources = brew_sources::Entity::find()
        .count(ctx.db)
        .await
        .map_err(|error| brew_query_failed("count brew sources", error))?
        as i64;

    let total_items = brew_items::Entity::find()
        .count(ctx.db)
        .await
        .map_err(|error| brew_query_failed("count brew items", error))?
        as i64;

    let user_id = ctx.user_id;
    let (unread_count, starred_count) = if user_id > 0 {
        let starred_count = brew_user_states::Entity::find()
            .filter(brew_user_states::Column::UserId.eq(user_id))
            .filter(brew_user_states::Column::IsStarred.eq(true))
            .count(ctx.db)
            .await
            .map_err(|error| brew_query_failed("count starred items", error))?
            as i64;

        // Unread ≈ items without a is_read=true state for this user
        let read_count = brew_user_states::Entity::find()
            .filter(brew_user_states::Column::UserId.eq(user_id))
            .filter(brew_user_states::Column::IsRead.eq(true))
            .count(ctx.db)
            .await
            .map_err(|error| brew_query_failed("count read items", error))?
            as i64;

        let unread_count = total_items.saturating_sub(read_count);
        (unread_count, starred_count)
    } else {
        // No auth context: treat all items as unread, no stars
        (total_items, 0)
    };

    Ok(json!({
        "totalSources": total_sources,
        "totalItems": total_items,
        "unreadCount": unread_count,
        "starredCount": starred_count,
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
mod brew_db_helpers_tests {
    use super::super::brew_generate::{outbound_web_search_allowed, parse_allow_web_search};
    use super::super::permission::install_permission_is_granted;
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
    fn brew_read_filters_align_source_and_source_id() {
        let mut params = HashMap::new();
        params.insert("sourceId".into(), json!(3));
        params.insert("limit".into(), json!(10));
        let f = parse_brew_read_filters(&params);
        assert_eq!(f.source_id, Some(3));
        assert_eq!(f.limit, 10);
        assert!(f.source_name.is_none());

        let mut params = HashMap::new();
        params.insert("source".into(), json!("12"));
        let f = parse_brew_read_filters(&params);
        assert_eq!(f.source_id, Some(12));
        assert!(f.source_name.is_none());

        let mut params = HashMap::new();
        params.insert("source".into(), json!("akiday"));
        let f = parse_brew_read_filters(&params);
        assert!(f.source_id.is_none());
        assert_eq!(f.source_name.as_deref(), Some("akiday"));

        let mut params = HashMap::new();
        params.insert("sourceName".into(), json!("天利"));
        params.insert("sourceId".into(), json!("5"));
        let f = parse_brew_read_filters(&params);
        assert_eq!(f.source_id, Some(5));
        assert_eq!(f.source_name.as_deref(), Some("天利"));

        let mut params = HashMap::new();
        params.insert("limit".into(), json!(9999));
        let f = parse_brew_read_filters(&params);
        assert_eq!(f.limit, 200); // clamped
    }

    #[test]
    fn brew_article_lookup_id_guid_url() {
        let mut params = HashMap::new();
        params.insert("articleId".into(), json!(101));
        let l = parse_brew_article_lookup(&params);
        assert_eq!(l.item_id, Some(101));
        assert!(l.url.is_none());

        let mut params = HashMap::new();
        params.insert("articleId".into(), json!("guid-abc"));
        let l = parse_brew_article_lookup(&params);
        assert!(l.item_id.is_none());
        assert_eq!(l.article_key.as_deref(), Some("guid-abc"));

        let mut params = HashMap::new();
        params.insert("url".into(), json!("https://example.com/post"));
        params.insert("sourceId".into(), json!(2));
        let l = parse_brew_article_lookup(&params);
        assert_eq!(l.url.as_deref(), Some("https://example.com/post"));
        assert_eq!(l.source_id, Some(2));

        let mut params = HashMap::new();
        params.insert("itemId".into(), json!("55"));
        let l = parse_brew_article_lookup(&params);
        assert_eq!(l.item_id, Some(55));
    }

    #[test]
    fn article_lookup_matches_by_id_guid_or_link() {
        let by_id = BrewArticleLookup {
            item_id: Some(7),
            article_key: None,
            url: None,
            source_id: None,
        };
        assert!(article_lookup_matches(&by_id, 7, "g", "https://x"));
        assert!(!article_lookup_matches(&by_id, 8, "g", "https://x"));

        let by_guid = BrewArticleLookup {
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

        let by_url = BrewArticleLookup {
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
            !outbound_web_search_allowed(true, true, Some(&["brew:read".to_string()])),
            "autonomy cap without ai:search must not outbound"
        );
        assert!(outbound_web_search_allowed(
            true,
            true,
            Some(&["brew:read".to_string(), "ai:search".to_string()])
        ));
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
