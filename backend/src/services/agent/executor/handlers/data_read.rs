//! 数据读取能力处理器
//!
//! 处理 platform.read, brew.read, config.get, fuzzy.search 等读取类能力

use super::HandlerContext;
use crate::models::entities::{
    brew_items, brew_sources, brew_user_states, tapp_scheduled_tasks, tapps,
};
use crate::services::agent::executor::utils::{validate_platform_name, VALID_PLATFORMS};
use crate::services::netease_utils::{get_random_china_ip, get_random_user_agent};
use once_cell::sync::Lazy;
use sea_orm::{
    ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde_json::{json, Value};
use std::cmp::Reverse;
use std::collections::HashMap;

static RE_HTML_TAG: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"<[^>]+>").unwrap());
static RE_WHITESPACE: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"\s+").unwrap());

/// 执行数据读取能力
pub async fn execute(
    capability_id: &str,
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    match capability_id {
        "platform.read" => execute_platform_read(params).await,
        "platform.stats" => execute_platform_stats(params).await,
        "brew.read" => execute_brew_read(params, ctx).await,
        "brew.sources" => execute_brew_sources(params, ctx).await,
        "brew.items" => execute_brew_items(params, ctx).await,
        "brew.article" => execute_brew_article(params, ctx).await,
        "brew.stats" => execute_brew_stats(params, ctx).await,
        "brew.discover" => execute_brew_discover(params, ctx).await,
        "brew.page" => execute_brew_page_content(params, ctx).await,
        "brew.generateReadingList" => execute_brew_generate_reading_list(params, ctx).await,
        "tapp.page" => execute_tapp_page_content(params, ctx).await,
        "fuzzy.search" | "search.fuzzy" => execute_fuzzy_search(params, ctx).await,
        "config.get" => execute_config_get(params).await,
        "time.info" => execute_time_info(params).await,
        "auth.status" => execute_auth_status(params).await,
        // 音乐平台
        "netease.playlist" => execute_netease_playlist(params).await,
        "netease.searchPlaylist" => execute_netease_search_playlist(params, ctx).await,
        // GitHub
        "github.repos" => execute_github_repos(params).await,
        // 追加能力
        "bilibili.bangumi" => execute_bilibili_bangumi(params).await,
        "steam.wishlist" => execute_steam_wishlist(params).await,
        "tapp.widget" => execute_tapp_widget(params, ctx).await,
        "permission.check" => execute_permission_check(params).await,
        "platform.connection" => execute_platform_connection(params).await,
        "stats.overview" => execute_stats_overview(params).await,
        "profile.summary" => execute_profile_summary(params).await,
        "search.global" => execute_search_global(params).await,
        "task.status" => execute_task_status(params).await,
        "metadata.history" => execute_metadata_history(params).await,
        "tapp.list" => execute_tapp_list(params, ctx).await,
        "scheduler.list" => execute_scheduler_list(params, ctx).await,
        "rsshub.instances" => execute_rsshub_instances(params).await,
        "context.reference" => execute_context_reference(params).await,
        // 补充的能力
        "database.anime" | "database.game" | "database.artist" => {
            execute_database_query(capability_id, params).await
        }
        "random.content" => execute_random_content(params).await,
        "report.list" => execute_report_list(params).await,
        _ => Err(format!("Unknown data_read capability: {}", capability_id)),
    }
}

// ============================================================================
// Platform 相关
// ============================================================================

async fn execute_platform_read(params: &HashMap<String, Value>) -> Result<Value, String> {
    let platform_raw = params
        .get("platform")
        .and_then(|v| v.as_str())
        .ok_or("Missing platform parameter")?;
    let platform_lower = platform_raw.to_lowercase();
    let platform = validate_platform_name(&platform_lower)?;

    let cache_file = format!("cache/platforms/{}_filtered.json", platform);
    let content = tokio::fs::read_to_string(&cache_file)
        .await
        .map_err(|e| format!("Failed to read platform data: {}", e))?;

    let data: Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse platform data: {}", e))?;

    let items = extract_platform_items(&platform.to_lowercase(), &data);
    let mut filtered_items = items;

    // 时间过滤
    if let Some(since) = params.get("since").and_then(|v| v.as_str()) {
        if let Ok(since_time) = chrono::DateTime::parse_from_rfc3339(since) {
            filtered_items.retain(|item| {
                item.get("createdAt")
                    .or_else(|| item.get("created_at"))
                    .or_else(|| item.get("timestamp"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|t| t >= since_time)
                    .unwrap_or(true)
            });
        }
    }

    // 数量限制
    if let Some(limit) = params.get("limit").and_then(|v| v.as_u64()) {
        filtered_items.truncate(limit as usize);
    }

    Ok(json!({
        "platform": platform,
        "items": filtered_items,
        "total": filtered_items.len(),
        "raw_data": data
    }))
}

async fn execute_platform_stats(params: &HashMap<String, Value>) -> Result<Value, String> {
    let platform_raw = params
        .get("platform")
        .and_then(|v| v.as_str())
        .ok_or("Missing platform parameter")?;
    let platform_lower = platform_raw.to_lowercase();
    let platform = validate_platform_name(&platform_lower)?;

    let cache_file = format!("cache/platforms/{}_filtered.json", platform);
    let content = tokio::fs::read_to_string(&cache_file)
        .await
        .map_err(|e| format!("Failed to read {}: {}", cache_file, e))?;

    let data: Value =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse JSON: {}", e))?;

    // 根据平台类型使用专门的分析函数
    match platform.to_lowercase().as_str() {
        "steam" => analyze_steam_stats(&data),
        "bilibili" => analyze_bilibili_stats(&data),
        "github" => analyze_github_stats(&data),
        "netease" => analyze_netease_stats(&data),
        _ => {
            // 通用平台统计
            let items = extract_platform_items(&platform.to_lowercase(), &data);
            let total = items.len();

            let mut distribution: HashMap<String, usize> = HashMap::new();
            for item in &items {
                if let Some(item_type) = item.get("type").and_then(|v| v.as_str()) {
                    *distribution.entry(item_type.to_string()).or_default() += 1;
                }
            }

            Ok(json!({
                "platform": platform,
                "total": total,
                "distribution": distribution
            }))
        }
    }
}

/// 分析 Bilibili 统计数据
fn analyze_bilibili_stats(data: &Value) -> Result<Value, String> {
    let content_analysis = data.get("content_analysis");

    // 获取番剧分析
    let anime_analysis = content_analysis
        .and_then(|v| v.get("anime_analysis"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let anime_count = anime_analysis.len();

    // 统计番剧类型分布
    let mut genre_distribution: HashMap<String, usize> = HashMap::new();
    for anime in &anime_analysis {
        if let Some(genres) = anime.get("genres").and_then(|v| v.as_object()) {
            for genre in genres.keys() {
                *genre_distribution.entry(genre.clone()).or_default() += 1;
            }
        }
    }

    // 获取观看进度统计
    let mut completed = 0;
    let mut watching = 0;
    for anime in &anime_analysis {
        if let Some(progress) = anime.get("progress").and_then(|v| v.as_str()) {
            if progress.contains("已看完") || progress.contains("全部") {
                completed += 1;
            } else {
                watching += 1;
            }
        }
    }

    // 用户摘要
    let username = data
        .get("user_summary")
        .and_then(|v| v.get("username"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    let summary = content_analysis
        .and_then(|v| v.get("summary"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    Ok(json!({
        "platform": "bilibili",
        "username": username,
        "summary": summary,
        "statistics": {
            "total_anime": anime_count,
            "completed": completed,
            "watching": watching
        },
        "distribution": {
            "by_genre": genre_distribution
        },
        "top_anime": anime_analysis.iter().take(10).collect::<Vec<_>>()
    }))
}

/// 分析 GitHub 统计数据
fn analyze_github_stats(data: &Value) -> Result<Value, String> {
    let content_analysis = data.get("content_analysis");

    // 语言分布
    let language_distribution = content_analysis
        .and_then(|v| v.get("language_distribution"))
        .cloned()
        .unwrap_or(json!({}));

    // 仓库数据
    let repositories = content_analysis
        .and_then(|v| v.get("repositories"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let repo_count = repositories.len();

    // 计算总 stars
    let total_stars: u64 = repositories
        .iter()
        .filter_map(|r| r.get("stars").and_then(|v| v.as_u64()))
        .sum();

    // 用户摘要
    let username = data
        .get("user_summary")
        .and_then(|v| v.get("username"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    let summary = content_analysis
        .and_then(|v| v.get("summary"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    Ok(json!({
        "platform": "github",
        "username": username,
        "summary": summary,
        "statistics": {
            "total_repos": repo_count,
            "total_stars": total_stars
        },
        "distribution": {
            "by_language": language_distribution
        },
        "top_repos": repositories.iter().take(10).collect::<Vec<_>>()
    }))
}

/// 分析网易云音乐统计数据
fn analyze_netease_stats(data: &Value) -> Result<Value, String> {
    let content_analysis = data.get("content_analysis");
    let artist_analysis = content_analysis.and_then(|v| v.get("artist_analysis"));

    // 获取 Top 艺术家
    let top_artists = artist_analysis
        .and_then(|v| v.get("top_artists"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // 风格分布
    let genre_analysis = artist_analysis
        .and_then(|v| v.get("genre_analysis"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // 统计播放次数
    let total_plays: u64 = top_artists
        .iter()
        .filter_map(|a| a.get("play_count").and_then(|v| v.as_u64()))
        .sum();

    // 用户摘要
    let username = data
        .get("user_summary")
        .and_then(|v| v.get("username"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    let music_summary = content_analysis
        .and_then(|v| v.get("music_summary"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    Ok(json!({
        "platform": "netease",
        "username": username,
        "summary": music_summary,
        "statistics": {
            "total_artists": top_artists.len(),
            "total_plays": total_plays,
            "genres_count": genre_analysis.len()
        },
        "distribution": {
            "by_genre": genre_analysis
        },
        "top_artists": top_artists.iter().take(10).collect::<Vec<_>>()
    }))
}

/// 分析 Steam 游戏统计数据
fn analyze_steam_stats(data: &Value) -> Result<Value, String> {
    // 获取游戏列表
    let recent_games = data
        .get("content_analysis")
        .and_then(|v| v.get("recent_games"))
        .and_then(|v| v.as_array());

    // 计算游戏时间分布
    let mut total_playtime: u64 = 0;
    let mut game_count = 0;
    let mut playtime_distribution: HashMap<String, u64> = HashMap::new();
    let mut games_by_time: Vec<(String, u64)> = Vec::new();

    // 从 recent_games 获取数据
    if let Some(games) = recent_games {
        for game in games {
            let name = game
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            let playtime = game.get("playtime").and_then(|v| v.as_u64()).unwrap_or(0);

            total_playtime += playtime;
            game_count += 1;
            games_by_time.push((name.to_string(), playtime));
        }
    }

    // 排序（按游戏时间降序）
    games_by_time.sort_by_key(|b| Reverse(b.1));

    // 计算时间段分布
    for (_, playtime) in &games_by_time {
        let hours = *playtime / 60;
        let category = match hours {
            0..=10 => "少于10小时",
            11..=50 => "10-50小时",
            51..=100 => "50-100小时",
            101..=200 => "100-200小时",
            _ => "200小时以上",
        };
        *playtime_distribution
            .entry(category.to_string())
            .or_default() += 1;
    }

    // 获取 Top 10 游戏
    let top_games: Vec<Value> = games_by_time
        .iter()
        .take(10)
        .map(|(name, playtime)| {
            json!({
                "name": name,
                "playtime_minutes": playtime,
                "playtime_hours": *playtime as f64 / 60.0
            })
        })
        .collect();

    // 获取类型分析
    let genre_analysis = data
        .get("content_analysis")
        .and_then(|v| v.get("genre_analysis"))
        .cloned()
        .unwrap_or(json!([]));

    // 获取用户摘要
    let username = data
        .get("user_summary")
        .and_then(|v| v.get("username"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    let summary = data
        .get("content_analysis")
        .and_then(|v| v.get("summary"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    Ok(json!({
        "platform": "steam",
        "username": username,
        "summary": summary,
        "statistics": {
            "total_games": game_count,
            "total_playtime_hours": total_playtime as f64 / 60.0
        },
        "distribution": {
            "by_playtime": playtime_distribution,
            "by_genre": genre_analysis
        },
        "top_games": top_games
    }))
}

// ============================================================================
// Brew 相关
// ============================================================================

/// Opt-in flag for external/web search fallback (brew.generateReadingList).
/// Accepts allowWebSearch / useWebSearch / webSearch / allowExternal / external.
/// Default false — local miss must not force ai.webSearch.
fn parse_allow_web_search(params: &HashMap<String, Value>) -> bool {
    const KEYS: &[&str] = &[
        "allowWebSearch",
        "useWebSearch",
        "webSearch",
        "allowExternal",
        "external",
    ];
    for key in KEYS {
        if let Some(v) = params.get(*key) {
            if v.as_bool() == Some(true) {
                return true;
            }
            if matches!(v.as_i64(), Some(1)) || matches!(v.as_u64(), Some(1)) {
                return true;
            }
            if let Some(s) = v.as_str() {
                let s = s.trim().to_lowercase();
                if matches!(s.as_str(), "true" | "1" | "yes" | "on") {
                    return true;
                }
            }
        }
    }
    false
}

/// Parse a JSON value as optional i32 (integer, unsigned, or numeric string).
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

fn brew_item_to_read_json(
    item: &brew_items::Model,
    source: Option<&brew_sources::Model>,
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

async fn execute_brew_read(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let filters = parse_brew_read_filters(params);

    let sources = brew_sources::Entity::find()
        .all(ctx.db)
        .await
        .map_err(|e| format!("Failed to fetch brew sources: {}", e))?;
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
                "message": format!(
                    "未找到匹配「{}」的订阅源",
                    filters.source_name.as_deref().unwrap_or("")
                ),
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
        .map_err(|e| format!("Failed to fetch brew items: {}", e))?;

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

    let matched_source_name = filters.source_id.and_then(|sid| {
        source_map
            .get(&sid)
            .map(|s| s.name.clone())
            .or(filters.source_name.clone())
    }).or_else(|| {
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

async fn execute_brew_sources(
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
        .map_err(|e| format!("Failed to fetch brew sources: {}", e))?;

    let total_in_system = all_sources.len();

    fn source_type_str(st: &brew_sources::SourceType) -> &'static str {
        match st {
            brew_sources::SourceType::Link => "link",
            brew_sources::SourceType::Rss => "rss",
            brew_sources::SourceType::Brewlia => "brewlia",
        }
    }

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
            "sourceType": source_type_str(&s.source_type),
            "feedType": feed_type_str(&s.feed_type),
            "enabled": s.enabled,
            "itemCount": s.item_count,
            "unreadCount": s.unread_count,
            "icon": s.icon,
            "description": s.description,
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
                if source_type_str(&s.source_type) != st.as_str() {
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
            "系统中暂无订阅源".to_string()
        } else {
            format!(
                "未找到匹配「{}」的订阅源（系统中共有 {} 个订阅源）",
                needle, total_in_system
            )
        },
    }))
}

async fn execute_brew_items(
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
                        let feed_url =
                            item.get("_feedUrl").and_then(|v| v.as_str()).unwrap_or("");
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
        .map_err(|e| format!("Failed to fetch brew source: {}", e))?;
    Ok(brew_item_to_article_json(&item, source.as_ref()))
}

async fn execute_brew_article(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let lookup = parse_brew_article_lookup(params);

    if lookup.item_id.is_none() && lookup.article_key.is_none() && lookup.url.is_none() {
        return Err(
            "Missing article lookup: provide articleId (id/guid) or url/link".to_string(),
        );
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
        return Err(
            "Missing article lookup: provide articleId (id/guid) or url/link".to_string(),
        );
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
        .map_err(|e| format!("Failed to fetch brew article: {}", e))?;

    match item {
        Some(item) => load_article_with_source(ctx, item).await,
        None => Err(format!(
            "未找到文章: id={:?}, key={:?}, url={:?}",
            lookup.item_id, lookup.article_key, lookup.url
        )),
    }
}

async fn execute_brew_stats(
    _params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let total_sources = brew_sources::Entity::find()
        .count(ctx.db)
        .await
        .map_err(|e| format!("Failed to count brew sources: {}", e))? as i64;

    let total_items = brew_items::Entity::find()
        .count(ctx.db)
        .await
        .map_err(|e| format!("Failed to count brew items: {}", e))? as i64;

    let user_id = ctx.user_id;
    let (unread_count, starred_count) = if user_id > 0 {
        let starred_count = brew_user_states::Entity::find()
            .filter(brew_user_states::Column::UserId.eq(user_id))
            .filter(brew_user_states::Column::IsStarred.eq(true))
            .count(ctx.db)
            .await
            .map_err(|e| format!("Failed to count starred items: {}", e))?
            as i64;

        // Unread ≈ items without a is_read=true state for this user
        let read_count = brew_user_states::Entity::find()
            .filter(brew_user_states::Column::UserId.eq(user_id))
            .filter(brew_user_states::Column::IsRead.eq(true))
            .count(ctx.db)
            .await
            .map_err(|e| format!("Failed to count read items: {}", e))?
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

/// AI 生成阅读列表
/// 根据用户需求筛选并生成符合条件的阅读列表
async fn execute_brew_generate_reading_list(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let criteria = params
        .get("criteria")
        .and_then(|v| v.as_str())
        .or_else(|| params.get("keyword").and_then(|v| v.as_str()))
        .or_else(|| params.get("topic").and_then(|v| v.as_str()))
        .or_else(|| params.get("query").and_then(|v| v.as_str()))
        .unwrap_or("最新文章");
    let max_items = params
        .get("maxItems")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;
    let source_name_filter = params.get("sourceName").and_then(|v| v.as_str());
    let days_back = params.get("daysBack").and_then(|v| v.as_i64()).unwrap_or(7);
    // Opt-in only: do not force ai.webSearch when local keyword miss.
    // Cascade escalation for other capabilities is owned by myriad-149.
    let allow_web_search = parse_allow_web_search(params);

    // 获取关键词过滤条件（支持多种参数名）
    let keyword = params
        .get("keyword")
        .and_then(|v| v.as_str())
        .or_else(|| params.get("topic").and_then(|v| v.as_str()))
        .or_else(|| {
            params
                .get("filters")
                .and_then(|v| v.get("keyword"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            params
                .get("filters")
                .and_then(|v| v.get("topic"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| params.get("query").and_then(|v| v.as_str()))
        .unwrap_or("");

    tracing::info!(
        keyword = %keyword,
        criteria = %criteria,
        allow_web_search = allow_web_search,
        "[brew.generateReadingList] Parameters parsed"
    );

    // 计算时间范围
    let cutoff_time = chrono::Utc::now() - chrono::Duration::days(days_back);

    // 从数据库获取文章
    let sources = brew_sources::Entity::find()
        .all(ctx.db)
        .await
        .unwrap_or_default();
    let source_map: std::collections::HashMap<i32, &brew_sources::Model> =
        sources.iter().map(|s| (s.id, s)).collect();

    // 查询文章，按时间筛选
    let base_query = brew_items::Entity::find()
        .filter(brew_items::Column::PublishedAt.gte(cutoff_time))
        .order_by_desc(brew_items::Column::PublishedAt);

    // 如果指定了订阅源名称，过滤
    let base_query = if let Some(name) = source_name_filter {
        let matching_source_ids: Vec<i32> = sources
            .iter()
            .filter(|s| s.name.to_lowercase().contains(&name.to_lowercase()))
            .map(|s| s.id)
            .collect();
        if !matching_source_ids.is_empty() {
            base_query.filter(brew_items::Column::SourceId.is_in(matching_source_ids))
        } else {
            base_query
        }
    } else {
        base_query
    };

    // 最小候选数量
    const MIN_CANDIDATES: usize = 20;
    let fetch_limit: u64 = 200;

    // 尝试关键词筛选
    let mut items: Vec<brew_items::Model> = if !keyword.is_empty() {
        use sea_orm::Condition;
        let keyword_lower = format!("%{}%", keyword.to_lowercase());
        let keyword_condition = Condition::any()
            .add(brew_items::Column::Title.like(&keyword_lower))
            .add(brew_items::Column::Content.like(&keyword_lower));

        let keyword_items = base_query
            .clone()
            .filter(keyword_condition)
            .limit(fetch_limit)
            .all(ctx.db)
            .await
            .unwrap_or_default();

        tracing::info!(
            keyword = %keyword,
            found = keyword_items.len(),
            "[brew.generateReadingList] Keyword search results"
        );

        // If keyword search is empty, do not pad with unrelated local articles.
        // Web search is only attempted later when allowWebSearch is explicit.
        if keyword_items.is_empty() {
            tracing::info!(
                keyword = %keyword,
                allow_web_search = allow_web_search,
                "[brew.generateReadingList] No keyword matches in local brew_items"
            );
            vec![]
        } else if keyword_items.len() < MIN_CANDIDATES {
            // 只有当有部分结果时才补充相关文章
            tracing::info!(
                keyword = %keyword,
                found = keyword_items.len(),
                "[brew.generateReadingList] Keyword results insufficient, fetching more articles"
            );
            let keyword_ids: std::collections::HashSet<i32> =
                keyword_items.iter().map(|i| i.id).collect();
            let additional = base_query
                .limit(fetch_limit)
                .all(ctx.db)
                .await
                .unwrap_or_default();

            // 合并，去重
            let mut combined = keyword_items;
            for item in additional {
                if !keyword_ids.contains(&item.id) && combined.len() < fetch_limit as usize {
                    combined.push(item);
                }
            }
            combined
        } else {
            keyword_items
        }
    } else {
        // 无关键词，直接取最新
        base_query
            .limit(fetch_limit)
            .all(ctx.db)
            .await
            .unwrap_or_default()
    };

    // Local miss / thin results: only call AI web search when explicitly opted in.
    // Default: honest empty + local suggestions (do not force ai.webSearch / Gemini).
    let needs_more = items.is_empty() || (items.len() < 3 && !keyword.is_empty());
    if needs_more {
        let list_name = if !keyword.is_empty() {
            keyword
        } else {
            criteria
        };
        let available_sources: Vec<&str> = sources
            .iter()
            .filter(|s| !s.name.is_empty())
            .map(|s| s.name.as_str())
            .take(8)
            .collect();

        if allow_web_search && (!keyword.is_empty() || !criteria.is_empty()) {
            tracing::info!(
                keyword = %keyword,
                db_results = items.len(),
                "[brew.generateReadingList] allowWebSearch=true, trying AI web search"
            );

            let search_query = if !keyword.is_empty() {
                format!("{} 相关文章 新闻 资讯", keyword)
            } else {
                format!("{} 相关文章", criteria)
            };

            match trigger_ai_web_search_for_reading_list(&search_query, max_items, ctx).await {
                Ok(web_results) if !web_results.is_empty() => {
                    tracing::info!(
                        results = web_results.len(),
                        "[brew.generateReadingList] AI web search returned results"
                    );
                    return Ok(json!({
                        "readingList": web_results,
                        "totalMatched": web_results.len(),
                        "listName": format!("网络搜索 - {}", list_name),
                        "criteria": criteria,
                        "fromWebSearch": true,
                        "allowWebSearch": true,
                        "message": crate::services::agent::response_agent::web_search_fallback(web_results.len()),
                        "action": {
                            "type": "reading_list",
                            "payload": {
                                "items": web_results,
                                "name": format!("网络搜索 - {}", list_name),
                                "fromWebSearch": true
                            }
                        }
                    }));
                }
                Ok(_) => {
                    tracing::info!("[brew.generateReadingList] AI web search returned no results");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "[brew.generateReadingList] AI web search failed");
                }
            }

            // Opt-in web search attempted but empty/failed
            let mut suggestions = vec![
                "尝试更换关键词".to_string(),
                "放宽 daysBack 或去掉 sourceName 限制".to_string(),
                "订阅更多相关的 RSS 源".to_string(),
                "检查 Gemini API Key 是否已配置".to_string(),
            ];
            if !available_sources.is_empty() {
                suggestions.insert(
                    0,
                    format!("本地已有订阅：{}", available_sources.join("、")),
                );
            }

            return Ok(json!({
                "readingList": [],
                "totalMatched": 0,
                "listName": list_name,
                "criteria": criteria,
                "matched": false,
                "notFound": true,
                "fromWebSearch": false,
                "allowWebSearch": true,
                "message": crate::services::agent::response_agent::no_articles_found(
                    if keyword.is_empty() && criteria.is_empty() {
                        "请提供搜索关键词"
                    } else {
                        "本地与联网搜索均未返回结果"
                    }
                ),
                "suggestions": suggestions,
                "availableSources": available_sources,
                "action": {
                    "type": "reading_list",
                    "payload": {
                        "items": [],
                        "name": list_name
                    }
                }
            }));
        }

        // No explicit web/external request — if we still have a few local hits,
        // continue to AI local ranking; otherwise honest empty.
        if !items.is_empty() {
            tracing::info!(
                keyword = %keyword,
                db_results = items.len(),
                "[brew.generateReadingList] Thin local results, ranking without web search"
            );
        } else {
            tracing::info!(
                keyword = %keyword,
                allow_web_search = false,
                "[brew.generateReadingList] Local keyword empty; honest empty (no forced webSearch)"
            );

            let searched = if !keyword.is_empty() {
                keyword
            } else {
                criteria
            };
            let mut suggestions = vec![
                "尝试更换或放宽关键词".to_string(),
                "增大 daysBack 查看更早文章".to_string(),
                "用 brew.items / brew.read 浏览本地订阅".to_string(),
                "订阅更多相关 RSS 源后再生成列表".to_string(),
            ];
            if !available_sources.is_empty() {
                suggestions.insert(
                    0,
                    format!("可浏览的本地订阅：{}", available_sources.join("、")),
                );
            }
            if !allow_web_search {
                suggestions.push(
                    "如需联网补充，请显式传 allowWebSearch=true".to_string(),
                );
            }

            return Ok(json!({
                "readingList": [],
                "totalMatched": 0,
                "listName": list_name,
                "criteria": criteria,
                "matched": false,
                "notFound": true,
                "fromWebSearch": false,
                "allowWebSearch": false,
                "searchedFor": searched,
                "message": crate::services::agent::response_agent::no_articles_found(
                    &format!("本地订阅中无「{}」相关文章", searched)
                ),
                "suggestions": suggestions,
                "availableSources": available_sources,
                "action": {
                    "type": "reading_list",
                    "payload": {
                        "items": [],
                        "name": list_name
                    }
                }
            }));
        }
    }

    // 限制单个来源的最大数量（不超过总数的 1/2），确保多样性
    let max_per_source = (items.len() / 2).max(3); // 至少保留3篇
    let mut source_counts: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
    items.retain(|item| {
        let count = source_counts.entry(item.source_id).or_insert(0);
        if *count < max_per_source {
            *count += 1;
            true
        } else {
            false
        }
    });

    // 确保至少有 MIN_CANDIDATES 篇（如果原始数据足够）
    let candidates_count = items.len().min(MIN_CANDIDATES.max(max_items * 2));
    let items: Vec<_> = items.into_iter().take(candidates_count).collect();

    // 准备 AI 分析的内容（摘要限制为前50字，去除HTML标签）
    let articles_for_ai: Vec<Value> = items
        .iter()
        .map(|item| {
            let source = source_map.get(&item.source_id);
            // 提取纯文本摘要：简单去除HTML标签，取前50字符
            let summary = item
                .content
                .as_ref()
                .map(|c| {
                    // 简单的HTML标签去除
                    let mut in_tag = false;
                    let text: String = c
                        .chars()
                        .filter(|&ch| {
                            if ch == '<' {
                                in_tag = true;
                                return false;
                            }
                            if ch == '>' {
                                in_tag = false;
                                return false;
                            }
                            !in_tag && ch != '\n' && ch != '\r'
                        })
                        .take(50)
                        .collect();
                    text.trim().to_string()
                })
                .unwrap_or_default();
            json!({
                "id": item.id,
                "title": item.title,
                "sourceName": source.map(|s| s.name.as_str()).unwrap_or(""),
                "summary": summary
            })
        })
        .collect();

    // 调用 AI 进行筛选和排序
    let ai_analyzer = ctx.ai_analyzer.ok_or("AI analyzer not configured")?;

    // 构建关键词提示（如果有）
    let keyword_hint = if !keyword.is_empty() {
        format!("\n关键词筛选条件：{}\n注意：候选文章已按关键词预筛选，请进一步判断与主题的真正相关性，排除标题党或仅表面相关的文章。", keyword)
    } else {
        String::new()
    };

    let prompt = format!(
        r#"你是一个智能阅读助手。请根据用户的需求从以下文章中筛选最符合条件的文章。

用户需求：{}{}

可选文章（JSON数组）：
{}

请返回一个JSON对象，格式如下：
{{
  "selectedIds": [文章ID数组，按推荐度排序，最多{}篇],
  "listName": "为这个阅读列表起一个简短的名字（与用户需求相关）",
  "reasons": {{
    "文章ID": "为什么推荐这篇文章（一句话）"
  }}
}}

筛选标准：
1. 与用户需求的相关性（最重要）
2. 内容质量和价值
3. 时效性
4. 如果没有真正符合条件的文章，selectedIds 可以为空数组

只返回JSON，不要其他内容。"#,
        criteria,
        keyword_hint,
        serde_json::to_string_pretty(&articles_for_ai).unwrap_or_default(),
        max_items
    );

    let ai_response = ai_analyzer.analyze(&prompt).await.map_err(|e| {
        tracing::error!(error = %e, "[brew.generateReadingList] AI analysis failed");
        format!("AI 分析失败: {}", e)
    })?;

    // 解析 AI 响应
    let ai_result: Value = extract_json_from_response(&ai_response)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| {
            json!({
                "selectedIds": items.iter().take(max_items).map(|i| i.id).collect::<Vec<_>>(),
                "listName": format!("阅读列表 - {}", criteria),
                "reasons": {}
            })
        });

    let selected_ids: Vec<i64> = ai_result
        .get("selectedIds")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();

    let list_name = ai_result
        .get("listName")
        .and_then(|v| v.as_str())
        .unwrap_or("智能阅读列表")
        .to_string();

    let reasons = ai_result.get("reasons").cloned().unwrap_or(json!({}));

    // 构建最终阅读列表
    let reading_list: Vec<Value> = selected_ids
        .iter()
        .filter_map(|&id| {
            items.iter().find(|item| item.id as i64 == id).map(|item| {
                let source = source_map.get(&item.source_id);
                let reason = reasons
                    .get(id.to_string())
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                json!({
                    "id": item.id,
                    "title": item.title,
                    "author": item.author,
                    "sourceName": source.map(|s| s.name.as_str()).unwrap_or(""),
                    "publishedAt": item.published_at.to_rfc3339(),
                    "summary": item.content.as_ref()
                        .map(|c| {
                            // 简单提取摘要：去除HTML标签，取前200字符
                            let text: String = c.chars()
                                .filter(|&ch| ch != '<' && ch != '>')
                                .take(200)
                                .collect();
                            text
                        })
                        .unwrap_or_default(),
                    "relevanceReason": reason,
                    "link": item.link
                })
            })
        })
        .take(max_items)
        .collect();

    tracing::info!(
        criteria = %criteria,
        total_candidates = items.len(),
        selected_count = reading_list.len(),
        "[brew.generateReadingList] Generated reading list"
    );

    Ok(json!({
        "readingList": reading_list,
        "totalMatched": reading_list.len(),
        "listName": list_name,
        "criteria": criteria,
        "action": {
            "type": "reading_list",
            "payload": {
                "items": reading_list,
                "name": list_name
            }
        }
    }))
}

// ============================================================================
// Config / Auth / Time
// ============================================================================

async fn execute_config_get(params: &HashMap<String, Value>) -> Result<Value, String> {
    let section = params
        .get("section")
        .and_then(|v| v.as_str())
        .unwrap_or("all");

    let mut config = json!({});

    if section == "all" || section == "ai" {
        config["ai"] = json!({
            "openai_enabled": std::env::var("OPENAI_API_KEY").is_ok(),
            "gemini_enabled": std::env::var("GEMINI_API_KEY").is_ok()
        });
    }

    if section == "all" || section == "platforms" {
        config["platforms"] = json!({
            "bilibili": std::env::var("BILIBILI_COOKIE").is_ok(),
            "steam": std::env::var("STEAM_API_KEY").is_ok(),
            "github": std::env::var("GITHUB_TOKEN").is_ok(),
            "netease": std::env::var("NETEASE_COOKIE").is_ok()
        });
    }

    Ok(json!({
        "section": section,
        "config": config
    }))
}

async fn execute_time_info(params: &HashMap<String, Value>) -> Result<Value, String> {
    use chrono::{Datelike, Timelike};

    let timezone = params
        .get("timezone")
        .and_then(|v| v.as_str())
        .unwrap_or("Asia/Shanghai");
    let now = chrono::Utc::now();

    let weekday = match now.weekday() {
        chrono::Weekday::Mon => "星期一",
        chrono::Weekday::Tue => "星期二",
        chrono::Weekday::Wed => "星期三",
        chrono::Weekday::Thu => "星期四",
        chrono::Weekday::Fri => "星期五",
        chrono::Weekday::Sat => "星期六",
        chrono::Weekday::Sun => "星期日",
    };

    Ok(json!({
        "datetime": now.to_rfc3339(),
        "timestamp": now.timestamp(),
        "timezone": timezone,
        "weekday": weekday,
        "year": now.year(),
        "month": now.month(),
        "day": now.day(),
        "hour": now.hour(),
        "minute": now.minute()
    }))
}

async fn execute_auth_status(_params: &HashMap<String, Value>) -> Result<Value, String> {
    Ok(json!({
        "isAuthenticated": true,
        "message": "Auth status check - requires session context",
        "linkedPlatforms": VALID_PLATFORMS
    }))
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 从平台数据中提取标准化的 items
fn extract_platform_items(platform: &str, data: &Value) -> Vec<Value> {
    match platform {
        "steam" => {
            let mut items = Vec::new();
            if let Some(games) = data
                .get("content_analysis")
                .and_then(|v| v.get("recent_games"))
                .and_then(|v| v.as_array())
            {
                for game in games {
                    items.push(json!({
                        "type": "game",
                        "name": game.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown"),
                        "playtime_minutes": game.get("playtime").and_then(|v| v.as_u64()).unwrap_or(0),
                        "appid": game.get("appid"),
                        "icon_url": game.get("icon_url")
                    }));
                }
            }
            items
        }
        "bilibili" => {
            let mut items = Vec::new();
            if let Some(anime_list) = data
                .get("content_analysis")
                .and_then(|v| v.get("anime_analysis"))
                .and_then(|v| v.as_array())
            {
                for anime in anime_list {
                    items.push(json!({
                        "type": "anime",
                        "title": anime.get("title").and_then(|v| v.as_str()).unwrap_or("Unknown"),
                        "progress": anime.get("progress"),
                        "genres": anime.get("genres")
                    }));
                }
            }
            items
        }
        "github" => {
            let mut items = Vec::new();
            if let Some(repos) = data
                .get("content_analysis")
                .and_then(|v| v.get("repositories"))
                .and_then(|v| v.as_array())
            {
                items.extend(repos.clone());
            }
            items
        }
        "netease" => {
            let mut items = Vec::new();
            if let Some(artists) = data
                .get("content_analysis")
                .and_then(|v| v.get("artist_analysis"))
                .and_then(|v| v.get("top_artists"))
                .and_then(|v| v.as_array())
            {
                for artist in artists {
                    items.push(json!({
                        "type": "artist",
                        "name": artist.get("name"),
                        "play_count": artist.get("play_count")
                    }));
                }
            }
            items
        }
        // Bangumi / MAL 过滤结果同构：top_rated / watching / recent
        "bangumi" | "mal" => {
            let mut items = Vec::new();
            let content = data.get("content_analysis");
            for key in ["top_rated_subjects", "watching_subjects", "recent_updates"] {
                if let Some(subjects) = content.and_then(|v| v.get(key)).and_then(|v| v.as_array())
                {
                    for subject in subjects {
                        items.push(json!({
                            "type": subject.get("subject_type").and_then(|v| v.as_str()).unwrap_or("subject"),
                            "title": subject.get("title"),
                            "rate": subject.get("rate"),
                            "collection_type": subject.get("collection_type"),
                            "subject_id": subject.get("subject_id"),
                            "platform": platform
                        }));
                    }
                }
            }
            items
        }
        "x" => data
            .get("content_analysis")
            .and_then(|v| v.get("top_posts").or_else(|| v.get("recent_posts")))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        "discord" => data
            .get("content_analysis")
            .and_then(|v| v.get("guilds_preview"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => data
            .get("items")
            .or_else(|| data.get("data"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
    }
}

/// 从 AI 响应中提取 JSON 字符串
fn extract_json_from_response(response: &str) -> Option<String> {
    // 尝试找到 JSON 代码块
    if let Some(start) = response.find("```json") {
        let content_start = start + 7;
        if let Some(end) = response[content_start..].find("```") {
            return Some(
                response[content_start..content_start + end]
                    .trim()
                    .to_string(),
            );
        }
    }

    // 尝试找到普通代码块
    if let Some(start) = response.find("```") {
        let content_start = start + 3;
        // 跳过可能的语言标识
        let actual_start = response[content_start..]
            .find('\n')
            .map(|n| content_start + n + 1)
            .unwrap_or(content_start);
        if let Some(end) = response[actual_start..].find("```") {
            return Some(
                response[actual_start..actual_start + end]
                    .trim()
                    .to_string(),
            );
        }
    }

    // 尝试直接解析为 JSON（查找 { 和 } 的匹配）
    if let Some(start) = response.find('{') {
        let mut depth = 0;
        let mut end_pos = start;
        for (i, ch) in response[start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end_pos = start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        if end_pos > start {
            return Some(response[start..end_pos].to_string());
        }
    }

    None
}

/// 解析 RSS/Atom 内容 (legacy cache helper; DB path preferred for brew.*)
#[allow(dead_code)]
fn parse_brew_content(content: &str) -> Vec<Value> {
    let mut items = Vec::new();

    // 简单的正则提取
    let item_pattern = regex::Regex::new(r"(?s)<(?:item|entry)>(.*?)</(?:item|entry)>").ok();
    let title_re = regex::Regex::new(r"<title[^>]*>(?:<!\[CDATA\[)?(.*?)(?:\]\]>)?</title>").ok();
    let link_re = regex::Regex::new(r#"<link[^>]*(?:href="([^"]+)"[^>]*)?>([^<]*)</link>"#).ok();
    let date_re = regex::Regex::new(r"<(?:pubDate|published|updated)>([^<]+)</").ok();

    if let Some(pattern) = item_pattern {
        for cap in pattern.captures_iter(content) {
            if let Some(item_content) = cap.get(1) {
                let item_str = item_content.as_str();

                let title = title_re
                    .as_ref()
                    .and_then(|r| r.captures(item_str))
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string());

                let link = link_re
                    .as_ref()
                    .and_then(|r| r.captures(item_str))
                    .and_then(|c| c.get(1).or(c.get(2)))
                    .map(|m| m.as_str().to_string());

                let date = date_re
                    .as_ref()
                    .and_then(|r| r.captures(item_str))
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string());

                items.push(json!({
                    "title": title,
                    "link": link,
                    "pubDate": date
                }));
            }
        }
    }

    items
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

// ============================================================================
// 模糊搜索
// ============================================================================

async fn execute_fuzzy_search(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let query = params
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or("Missing query parameter")?;
    let scope = params
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("all");
    let search_type = params.get("type").and_then(|v| v.as_str());
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

    let query_lower = query.to_lowercase();
    let mut results: Vec<Value> = Vec::new();

    // 搜索 Brew 订阅源
    if scope == "all" || scope == "brew" {
        if search_type.is_none() || search_type == Some("source") {
            let sources = brew_sources::Entity::find()
                .all(ctx.db)
                .await
                .map_err(|e| format!("Database error: {}", e))?;

            for source in sources {
                let name_lower = source.name.to_lowercase();
                let score = calculate_fuzzy_score(&query_lower, &name_lower);

                if score > 0.3 {
                    results.push(json!({
                        "id": source.id.to_string(),
                        "name": source.name,
                        "type": "source",
                        "scope": "brew",
                        "score": score,
                        "metadata": {
                            "icon": source.icon,
                            "siteUrl": source.site_url,
                            "category": source.category
                        }
                    }));
                }
            }
        }

        // 搜索 Brew 内容项
        if search_type.is_none() || search_type == Some("item") {
            let items = brew_items::Entity::find()
                .limit(100)
                .all(ctx.db)
                .await
                .map_err(|e| format!("Database error: {}", e))?;

            for item in items {
                let title_lower = item.title.to_lowercase();
                let score = calculate_fuzzy_score(&query_lower, &title_lower);

                if score > 0.3 {
                    results.push(json!({
                        "id": item.guid.clone(),
                        "name": item.title,
                        "type": "item",
                        "scope": "brew",
                        "score": score,
                        "metadata": {
                            "sourceId": item.source_id,
                            "link": item.link,
                            "publishedAt": item.published_at
                        }
                    }));
                }
            }
        }
    }

    // 搜索 Tapp 应用
    if (scope == "all" || scope == "tapp") && (search_type.is_none() || search_type == Some("app"))
    {
        let apps = tapps::Entity::find()
            .all(ctx.db)
            .await
            .map_err(|e| format!("Database error: {}", e))?;

        for app in apps {
            let name_lower = app.name.to_lowercase();
            let score = calculate_fuzzy_score(&query_lower, &name_lower);

            let desc_score = app
                .description
                .as_ref()
                .map(|d| calculate_fuzzy_score(&query_lower, &d.to_lowercase()))
                .unwrap_or(0.0);

            let max_score = score.max(desc_score * 0.8);

            if max_score > 0.3 {
                results.push(json!({
                    "id": app.id.to_string(),
                    "name": app.name,
                    "type": "app",
                    "scope": "tapp",
                    "score": max_score,
                    "metadata": {
                        "description": app.description,
                        "author": app.author,
                        "version": app.version,
                        "icon": app.icon
                    }
                }));
            }
        }
    }

    // 按得分排序
    results.sort_by(|a, b| {
        let score_a = a.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let score_b = b.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        score_b
            .partial_cmp(&score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    results.truncate(limit);

    if results.is_empty() {
        let can_discover = scope == "all" || scope == "brew";
        let discovery_hint = if can_discover {
            Some(json!({
                "searchQuery": query,
                "message": crate::services::agent::response_agent::not_found_in_feeds(query),
                "suggestAction": "brew.discover",
                "suggestParams": { "query": query }
            }))
        } else {
            None
        };

        return Ok(json!({
            "results": [],
            "total": 0,
            "query": query,
            "notFound": true,
            "canDiscover": can_discover,
            "discoveryHint": discovery_hint,
            "suggestions": ["请尝试其他关键词", "检查拼写是否正确"]
        }));
    }

    let high_score_count = results
        .iter()
        .filter(|r| r.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0) > 0.7)
        .count();

    let needs_choice = high_score_count > 1 || (results.len() > 1 && high_score_count == 0);

    Ok(json!({
        "results": results,
        "total": results.len(),
        "query": query,
        "choices": if needs_choice {
            Some(results.iter().map(|r| json!({
                "value": r.get("id"),
                "label": r.get("name")
            })).collect::<Vec<_>>())
        } else {
            None
        }
    }))
}

/// 计算模糊匹配得分
fn calculate_fuzzy_score(query: &str, target: &str) -> f64 {
    if query == target {
        return 1.0;
    }

    if target.contains(query) {
        let ratio = query.len() as f64 / target.len() as f64;
        return 0.7 + (ratio * 0.3);
    }

    let query_words: Vec<&str> = query.split_whitespace().collect();
    let target_words: Vec<&str> = target.split_whitespace().collect();

    let mut matched_words = 0;
    for qw in &query_words {
        for tw in &target_words {
            if tw.contains(qw) || qw.contains(tw) {
                matched_words += 1;
                break;
            }
        }
    }

    if !query_words.is_empty() {
        let word_ratio = matched_words as f64 / query_words.len() as f64;
        if word_ratio > 0.0 {
            return 0.3 + (word_ratio * 0.4);
        }
    }

    let query_chars: Vec<char> = query.chars().collect();
    let target_chars: Vec<char> = target.chars().collect();

    let mut matches = 0;
    for qc in &query_chars {
        if target_chars.contains(qc) {
            matches += 1;
        }
    }

    if !query_chars.is_empty() {
        let char_ratio = matches as f64 / query_chars.len() as f64;
        return char_ratio * 0.3;
    }

    0.0
}

// ============================================================================
// Brew 发现
// ============================================================================

async fn execute_brew_discover(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let url = params.get("url").and_then(|v| v.as_str());
    let query = params.get("query").and_then(|v| v.as_str());
    let auto_verify = params
        .get("autoVerify")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let mut feeds = Vec::new();
    let mut discovery_methods = Vec::new();

    // 策略1: 如果提供了 URL，直接尝试解析
    if let Some(feed_url) = url {
        discovery_methods.push("direct_url");
        match try_parse_feed(feed_url).await {
            Ok(feed_info) => {
                feeds.push(feed_info);
            }
            Err(e) => {
                tracing::warn!("Failed to parse feed URL {}: {}", feed_url, e);
            }
        }
    }

    // 策略2: 如果提供了查询词，从 RSSHub 路由中搜索
    if let Some(search_query) = query {
        discovery_methods.push("rsshub_routes");

        let rsshub_results = query_rsshub_routes(search_query).await?;

        for mut route in rsshub_results {
            if auto_verify {
                if let Some(route_url) = route.get("url").and_then(|v| v.as_str()) {
                    match try_parse_feed(route_url).await {
                        Ok(verified_info) => {
                            route["verified"] = json!(true);
                            route["title"] =
                                verified_info.get("title").cloned().unwrap_or(json!(null));
                            route["itemCount"] =
                                verified_info.get("itemCount").cloned().unwrap_or(json!(0));
                            route["feedType"] = verified_info
                                .get("feedType")
                                .cloned()
                                .unwrap_or(json!("unknown"));
                        }
                        Err(_) => {
                            route["verified"] = json!(false);
                            route["verifyError"] = json!("无法访问或解析此 RSS 源");
                        }
                    }
                }
            }
            feeds.push(route);
        }

        // 策略3: 如果查询词本身就是 URL，尝试网站 RSS 自动发现
        if looks_like_url(search_query) {
            discovery_methods.push("website_autodiscover");
            if let Ok(discovered) = discover_rss_from_website(search_query).await {
                for feed in discovered {
                    if !feeds.iter().any(|f| f.get("url") == feed.get("url")) {
                        feeds.push(feed);
                    }
                }
            }
        }

        // 策略4: 如果前面的策略都没找到结果，尝试 AI 联网搜索
        if feeds.is_empty() && ctx.ai_analyzer.is_some() {
            discovery_methods.push("ai_web_search");
            if let Ok(ai_results) = ai_search_rss_feeds(search_query, ctx).await {
                for feed in ai_results {
                    feeds.push(feed);
                }
            }
        }
    }

    if url.is_none() && query.is_none() {
        return Err("需要提供 url 或 query 参数".to_string());
    }

    let found = !feeds.is_empty();
    let verified_count = feeds
        .iter()
        .filter(|f| f.get("verified") == Some(&json!(true)))
        .count();

    Ok(json!({
        "found": found,
        "feeds": feeds,
        "total": feeds.len(),
        "verifiedCount": verified_count,
        "discoveryMethods": discovery_methods,
        "suggestions": if !found {
            json!({
                "message": crate::services::agent::response_agent::no_rss_found(),
                "tips": [
                    "尝试更具体的关键词",
                    "直接提供 RSS/Atom URL",
                    "可以让 AI 联网搜索"
                ],
                "aiSearchPrompt": format!(
                    "请帮我搜索「{}」的 RSS 订阅地址",
                    query.unwrap_or("")
                )
            })
        } else {
            json!(null)
        }
    }))
}

/// 使用 AI 联网搜索 RSS 订阅源
async fn ai_search_rss_feeds(query: &str, ctx: &HandlerContext<'_>) -> Result<Vec<Value>, String> {
    let analyzer = ctx.ai_analyzer.ok_or("AI analyzer not available")?;

    // 构建搜索查询
    let search_query = format!("{} RSS feed URL", query);

    // 使用 AI 推断常见 RSS 地址（注意：AI 没有实时联网能力，依赖已有知识）
    let prompt = format!(
        "根据你的知识，推断「{}」可能的 RSS/Atom 订阅源地址。\n\n\
        规则：\n\
        1. 优先返回常见平台的已知 RSS 格式（如 WordPress 的 /feed/、GitHub 的 .atom、Reddit 的 .rss 等）\n\
        2. 可以返回 RSSHub (rsshub.app) 提供的路由\n\
        3. 只返回你有较高把握的 URL，不确定的不要返回\n\
        4. 以 JSON 数组格式返回：[{{\"url\": \"...\", \"name\": \"...\", \"confidence\": \"high|medium\"}}]\n\n\
        请直接返回 JSON 数组。",
        search_query
    );

    let result = analyzer
        .analyze(&prompt)
        .await
        .map_err(|e| format!("AI search failed: {}", e))?;

    let mut feeds = Vec::new();

    // 预编译正则表达式
    let url_re =
        regex::Regex::new(r#"https?://[^\s<>"')\]]+(?:rss|feed|atom|xml)[^\s<>"')\]]*"#).unwrap();

    // 从 AI 响应中提取 RSS URL
    for cap in url_re.captures_iter(&result) {
        let url = cap.get(0).map(|m| m.as_str()).unwrap_or("");
        if !url.is_empty()
            && !feeds
                .iter()
                .any(|f: &Value| f.get("url") == Some(&json!(url)))
        {
            feeds.push(json!({
                "url": url,
                "name": "",
                "description": "AI 联网搜索发现",
                "source": "ai_web_search",
                "verified": false
            }));
        }
    }

    tracing::info!(
        query = %query,
        found = feeds.len(),
        "[Agent] AI RSS search completed"
    );

    Ok(feeds)
}

fn looks_like_url(s: &str) -> bool {
    let s_lower = s.trim().to_lowercase();
    s_lower.starts_with("http://")
        || s_lower.starts_with("https://")
        || s_lower.starts_with("www.")
        || (s_lower.contains('.') && !s_lower.contains(' ') && s_lower.len() < 100)
}

async fn try_parse_feed(url: &str) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("Mozilla/5.0 (compatible; MyriadBot/1.0)")
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let content = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    // 检测 Feed 类型
    let feed_type = if content.contains("<rss") {
        "rss"
    } else if content.contains("<feed") {
        "atom"
    } else {
        "unknown"
    };

    // 提取标题
    let title_re = regex::Regex::new(r"<title[^>]*>(?:<!\[CDATA\[)?(.*?)(?:\]\]>)?</title>").ok();
    let title = title_re
        .and_then(|r| r.captures(&content))
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_default();

    // 统计条目数量
    let item_count = content.matches("<item>").count() + content.matches("<entry>").count();

    Ok(json!({
        "url": url,
        "title": title,
        "feedType": feed_type,
        "itemCount": item_count,
        "verified": true
    }))
}

/// 查询 RSSHub 路由 - 支持缓存和远程获取
async fn query_rsshub_routes(query: &str) -> Result<Vec<Value>, String> {
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    // 尝试读取本地缓存
    let routes_data = load_rsshub_routes_cache().await;

    if let Some(routes) = routes_data {
        // 在路由中搜索匹配项
        if let Some(routes_array) = routes.as_array() {
            for route in routes_array {
                // 获取路由信息
                let name = route.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let path = route.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let description = route
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let requires_config = route
                    .get("requiresConfig")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let config_params = route.get("configParams").and_then(|v| v.as_array());

                // 排除需要额外配置的路由（如需要 cookie、token、key 等）
                if requires_config {
                    continue;
                }
                if let Some(params) = config_params {
                    let has_sensitive_param = params.iter().any(|p| {
                        let param_name = p.as_str().unwrap_or("");
                        param_name.contains("cookie")
                            || param_name.contains("token")
                            || param_name.contains("key")
                            || param_name.contains("secret")
                            || param_name.contains("password")
                    });
                    if has_sensitive_param {
                        continue;
                    }
                }

                // 模糊匹配
                let name_lower = name.to_lowercase();
                let desc_lower = description.to_lowercase();
                let path_lower = path.to_lowercase();

                let name_score = calculate_fuzzy_score(&query_lower, &name_lower);
                let desc_score = calculate_fuzzy_score(&query_lower, &desc_lower) * 0.7;
                let path_score = calculate_fuzzy_score(&query_lower, &path_lower) * 0.5;

                let max_score = name_score.max(desc_score).max(path_score);

                if max_score > 0.3 {
                    results.push(json!({
                        "name": name,
                        "path": path,
                        "url": format!("https://rsshub.app{}", path),
                        "description": description,
                        "source": "rsshub",
                        "score": max_score,
                        "verified": false  // 需要实际验证 URL 是否可用
                    }));
                }
            }
        }
    }

    // 如果缓存中没有结果，使用硬编码的热门路由
    if results.is_empty() {
        let popular_routes = vec![
            ("知乎日报", "/zhihu/daily", "知乎日报，每日推荐"),
            ("知乎热榜", "/zhihu/hotlist", "知乎热门话题榜单"),
            ("微博热搜", "/weibo/search/hot", "微博实时热搜榜"),
            ("B站排行榜", "/bilibili/ranking/0/3/1", "B站全站排行榜"),
            (
                "GitHub Trending",
                "/github/trending/daily/any",
                "GitHub 每日趋势项目",
            ),
            ("Hacker News", "/hackernews/best", "Hacker News 最佳"),
            ("少数派首页", "/sspai/index", "少数派首页文章"),
            ("IT之家", "/ithome", "IT之家最新资讯"),
            ("36氪", "/36kr/newsflashes", "36氪快讯"),
            ("豆瓣电影", "/douban/movie/playing", "豆瓣正在上映"),
            ("抖音热搜", "/douyin/trending", "抖音热搜榜"),
            (
                "即刻精选",
                "/jike/topic/text/553870e8e4b0cafb0a1bef68",
                "即刻精选内容",
            ),
        ];

        for (name, path, desc) in popular_routes {
            let name_lower = name.to_lowercase();
            let desc_lower = desc.to_lowercase();

            let score = calculate_fuzzy_score(&query_lower, &name_lower)
                .max(calculate_fuzzy_score(&query_lower, &desc_lower) * 0.7);

            if score > 0.3 {
                results.push(json!({
                    "name": name,
                    "path": path,
                    "url": format!("https://rsshub.app{}", path),
                    "description": desc,
                    "source": "rsshub",
                    "score": score,
                    "verified": true
                }));
            }
        }
    }

    // 按得分排序
    results.sort_by(|a, b| {
        let score_a = a.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let score_b = b.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        score_b
            .partial_cmp(&score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 限制返回数量
    results.truncate(10);

    Ok(results)
}

/// 加载 RSSHub 路由缓存
async fn load_rsshub_routes_cache() -> Option<Value> {
    use crate::services::data_paths::paths;

    // 尝试读取本地缓存文件
    let cache_path = paths().rsshub_routes_cache();
    if let Ok(content) = tokio::fs::read_to_string(&cache_path).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            // 检查缓存是否过期（7天）
            if let Some(cached_at) = data.get("cachedAt").and_then(|v| v.as_i64()) {
                let now = chrono::Utc::now().timestamp();
                if now - cached_at < 7 * 24 * 3600 {
                    return data.get("routes").cloned();
                }
            }
        }
    }

    // 缓存不存在或已过期，尝试从远程获取
    fetch_rsshub_routes().await.ok()
}

/// 从 RSSHub 官方获取路由数据
async fn fetch_rsshub_routes() -> Result<Value, String> {
    use crate::services::data_paths::paths;

    // 从 RSSHub 的 radar-rules 获取（包含大量路由信息）
    let radar_url = "https://raw.githubusercontent.com/DIYgod/RSSHub/master/lib/radar-rules.js";

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    // 尝试获取 radar-rules（这是一个 JS 文件，包含路由规则）
    match client.get(radar_url).send().await {
        Ok(response) if response.status().is_success() => {
            if let Ok(content) = response.text().await {
                // 解析 radar-rules.js 提取路由信息
                let routes = parse_rsshub_radar_rules(&content);

                // 缓存到本地
                let cache_data = json!({
                    "cachedAt": chrono::Utc::now().timestamp(),
                    "source": "radar-rules",
                    "routes": routes
                });

                let cache_path = paths().rsshub_routes_cache();
                let _ = tokio::fs::create_dir_all(paths().cache.clone()).await;
                let _ = tokio::fs::write(
                    &cache_path,
                    serde_json::to_string_pretty(&cache_data)
                        .unwrap_or_else(|_| cache_data.to_string()),
                )
                .await;

                return Ok(routes);
            }
        }
        _ => {}
    }

    // 如果获取失败，返回空数组
    Ok(json!([]))
}

/// 解析 RSSHub radar-rules.js 提取路由信息
fn parse_rsshub_radar_rules(content: &str) -> Value {
    let mut routes = Vec::new();

    // radar-rules.js 的格式大致为:
    // module.exports = {
    //     'zhihu.com': { _name: '知乎', daily: [{ title: '日报', ... }] },
    //     ...
    // }

    // 使用正则提取域名和路由信息
    let domain_re = regex::Regex::new(r#"'([^']+\.[^']+)':\s*\{"#).unwrap();
    let name_re = regex::Regex::new(r#"_name:\s*['"]([^'"]+)['"]"#).unwrap();
    let route_re = regex::Regex::new(r#"(\w+):\s*\[\s*\{\s*title:\s*['"]([^'"]+)['"]"#).unwrap();
    let target_re = regex::Regex::new(r#"target:\s*['"]([^'"]+)['"]"#).unwrap();

    // 按域名块分割
    let blocks: Vec<&str> = content.split("': {").collect();

    for block in blocks.iter().skip(1) {
        // 提取域名
        let domain = if let Some(prev_part) = blocks.iter().find(|b| !block.starts_with(*b)) {
            // 从前一个块的末尾提取域名
            domain_re
                .captures(prev_part)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str())
                .unwrap_or("")
        } else {
            ""
        };

        // 提取名称
        let name = name_re
            .captures(block)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            .unwrap_or("");

        // 提取路由
        for cap in route_re.captures_iter(block) {
            let _route_key = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let title = cap.get(2).map(|m| m.as_str()).unwrap_or("");

            // 提取 target（RSSHub 路径）
            if let Some(target_cap) = target_re.captures(block) {
                let target = target_cap.get(1).map(|m| m.as_str()).unwrap_or("");

                if !target.is_empty() && !name.is_empty() {
                    // 检查是否需要额外参数（路径中包含 :param 且不是可选的）
                    let requires_config = target.contains(":")
                        && !target.contains("?")
                        && target.matches(':').count() > 1;

                    routes.push(json!({
                        "name": format!("{} - {}", name, title),
                        "path": target,
                        "description": format!("{} 的 {} 订阅", name, title),
                        "domain": domain,
                        "requiresConfig": requires_config
                    }));
                }
            }
        }
    }

    // 添加一些已知的无需配置的热门路由（作为后备）
    let popular_routes = vec![
        ("知乎日报", "/zhihu/daily", "知乎日报，每日推荐"),
        ("知乎热榜", "/zhihu/hotlist", "知乎热门话题榜单"),
        ("微博热搜", "/weibo/search/hot", "微博实时热搜榜"),
        ("B站排行榜", "/bilibili/ranking/0/3/1", "B站全站排行榜"),
        (
            "GitHub Trending",
            "/github/trending/daily/any",
            "GitHub 每日趋势项目",
        ),
        ("Hacker News", "/hackernews/best", "Hacker News 最佳"),
        ("少数派首页", "/sspai/index", "少数派首页文章"),
        ("IT之家", "/ithome", "IT之家最新资讯"),
        ("36氪", "/36kr/newsflashes", "36氪快讯"),
        ("抖音热搜", "/douyin/trending", "抖音热搜榜"),
        ("豆瓣电影", "/douban/movie/playing", "豆瓣正在上映"),
        (
            "即刻精选",
            "/jike/topic/text/553870e8e4b0cafb0a1bef68",
            "即刻精选内容",
        ),
    ];

    for (name, path, desc) in popular_routes {
        // 检查是否已存在
        let exists = routes
            .iter()
            .any(|r| r.get("path").and_then(|p| p.as_str()) == Some(path));

        if !exists {
            routes.push(json!({
                "name": name,
                "path": path,
                "description": desc,
                "domain": "",
                "requiresConfig": false,
                "verified": true
            }));
        }
    }

    json!(routes)
}

async fn discover_rss_from_website(url: &str) -> Result<Vec<Value>, String> {
    let normalized_url = if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else if url.starts_with("www.") || looks_like_url(url) {
        format!("https://{}", url)
    } else {
        return Err("输入不是有效的 URL".to_string());
    };

    let mut feeds = Vec::new();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("Mozilla/5.0 (compatible; MyriadBot/1.0)")
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let response = client
        .get(&normalized_url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let html = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    // 查找 RSS/Atom 链接
    let rss_re = regex::Regex::new(
        r#"<link[^>]*rel=[\"']alternate[\"'][^>]*type=[\"']application/(rss|atom)\+xml[\"'][^>]*href=[\"']([^\"']+)[\"']"#
    ).unwrap();

    let base_url = reqwest::Url::parse(&normalized_url).ok();

    for cap in rss_re.captures_iter(&html) {
        if let Some(href) = cap.get(2) {
            let feed_url = if href.as_str().starts_with("http") {
                href.as_str().to_string()
            } else if let Some(ref base) = base_url {
                base.join(href.as_str())
                    .map(|u| u.to_string())
                    .unwrap_or_default()
            } else {
                continue;
            };

            if let Ok(feed_info) = try_parse_feed(&feed_url).await {
                feeds.push(json!({
                    "url": feed_url,
                    "title": feed_info.get("title"),
                    "feedType": feed_info.get("feedType"),
                    "itemCount": feed_info.get("itemCount"),
                    "source": "website_autodiscover",
                    "verified": true
                }));
            }
        }
    }

    Ok(feeds)
}

// ============================================================================
// Brew 页面内容
// ============================================================================

async fn execute_brew_page_content(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let level = params
        .get("level")
        .and_then(|v| v.as_str())
        .unwrap_or("sources");
    let source_id = params.get("sourceId").and_then(|v| v.as_i64());
    let item_id = params.get("itemId").and_then(|v| v.as_str());
    let filter = params
        .get("filter")
        .and_then(|v| v.as_str())
        .unwrap_or("all");
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20);

    match level {
        "sources" => {
            let sources = brew_sources::Entity::find()
                .order_by_desc(brew_sources::Column::UpdatedAt)
                .all(ctx.db)
                .await
                .map_err(|e| format!("Failed to fetch sources: {}", e))?;

            let source_list: Vec<Value> = sources
                .iter()
                .map(|s| {
                    json!({
                        "id": s.id,
                        "name": s.name,
                        "url": s.url.clone(),
                        "icon": s.icon.clone(),
                        "category": s.category.clone(),
                        "unreadCount": s.unread_count,
                        "itemCount": s.item_count,
                        "lastUpdated": s.updated_at.to_string()
                    })
                })
                .collect();

            Ok(json!({
                "level": "sources",
                "hierarchy": {
                    "level": "list",
                    "current": { "view": "all_sources" }
                },
                "content": {
                    "title": "订阅源",
                    "sources": source_list,
                    "metadata": { "totalSources": sources.len() }
                },
                "stats": {
                    "totalSources": sources.len(),
                    "totalItems": 0,
                    "unreadCount": 0
                },
                "navigation": {
                    "currentFilter": filter,
                    "availableFilters": ["all", "unread", "starred", "today"],
                    "canGoBack": false,
                    "parentPath": "/"
                }
            }))
        }
        "items" => {
            let source_id = source_id.ok_or("Missing sourceId for items level")?;

            let source = brew_sources::Entity::find_by_id(source_id as i32)
                .one(ctx.db)
                .await
                .map_err(|e| format!("Failed to fetch source: {}", e))?
                .ok_or("Source not found")?;

            let items = brew_items::Entity::find()
                .filter(brew_items::Column::SourceId.eq(source_id as i32))
                .order_by_desc(brew_items::Column::PublishedAt)
                .all(ctx.db)
                .await
                .map_err(|e| format!("Failed to fetch items: {}", e))?;

            let item_list: Vec<Value> = items
                .iter()
                .take(limit as usize)
                .map(|item| {
                    json!({
                        "id": item.id,
                        "guid": item.guid.clone(),
                        "title": item.title.clone(),
                        "summary": item.summary.as_ref().map(|s| {
                            if s.len() > 200 { let i = s.floor_char_boundary(200); format!("{}...", &s[..i]) } else { s.clone() }
                        }),
                        "link": item.link.clone(),
                        "author": item.author.clone(),
                        "publishedAt": item.published_at.to_string(),
                        "isRead": false,
                        "isStarred": false
                    })
                })
                .collect();

            Ok(json!({
                "level": "items",
                "hierarchy": {
                    "level": "nested",
                    "parent": {
                        "type": "source",
                        "id": source.id,
                        "name": source.name.clone()
                    },
                    "current": { "view": "item_list" }
                },
                "content": {
                    "title": source.name.clone(),
                    "items": item_list,
                    "metadata": {
                        "sourceId": source.id,
                        "totalItems": items.len()
                    }
                },
                "stats": {
                    "totalItems": items.len(),
                    "unreadCount": items.len(),
                    "starredCount": 0
                },
                "navigation": {
                    "currentFilter": filter,
                    "availableFilters": ["all", "unread", "starred"],
                    "canGoBack": true,
                    "parentPath": "/brew"
                }
            }))
        }
        "detail" | "reader" => {
            let item_guid = item_id.ok_or("Missing itemId for detail/reader level")?;

            let item = brew_items::Entity::find()
                .filter(brew_items::Column::Guid.eq(item_guid))
                .one(ctx.db)
                .await
                .map_err(|e| format!("Failed to fetch item: {}", e))?
                .ok_or("Item not found")?;

            let source = brew_sources::Entity::find_by_id(item.source_id)
                .one(ctx.db)
                .await
                .map_err(|e| format!("Failed to fetch source: {}", e))?;

            let user_id = params
                .get("userId")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32);

            let user_state = if let Some(uid) = user_id {
                brew_user_states::Entity::find()
                    .filter(brew_user_states::Column::UserId.eq(uid))
                    .filter(brew_user_states::Column::ItemId.eq(item.id))
                    .one(ctx.db)
                    .await
                    .ok()
                    .flatten()
            } else {
                None
            };

            let word_count = item.word_count.unwrap_or_else(|| {
                item.content
                    .as_ref()
                    .map(|c| c.chars().count() as i32)
                    .unwrap_or(0)
            });
            let reading_time = item
                .reading_time
                .unwrap_or_else(|| (word_count as f32 / 500.0).ceil() as i32);

            Ok(json!({
                "level": "reader",
                "hierarchy": {
                    "level": "detail",
                    "parent": {
                        "type": "source",
                        "id": item.source_id,
                        "name": source.as_ref().map(|s| s.name.clone())
                    },
                    "current": {
                        "type": "item",
                        "id": item.id,
                        "guid": item.guid.clone()
                    }
                },
                "content": {
                    "title": item.title.clone(),
                    "article": {
                        "id": item.id,
                        "guid": item.guid.clone(),
                        "title": item.title.clone(),
                        "content": item.content.clone(),
                        "summary": item.summary.clone(),
                        "link": item.link.clone(),
                        "author": item.author.clone(),
                        "image": item.image.clone(),
                        "publishedAt": item.published_at.to_string(),
                        "wordCount": word_count,
                        "readingTime": reading_time,
                        "fulltextFetched": item.fulltext_fetched,
                        "audioUrl": item.audio_url.clone(),
                        "videoUrl": item.video_url.clone()
                    },
                    "source": source.as_ref().map(|s| json!({
                        "id": s.id,
                        "name": s.name.clone(),
                        "icon": s.icon.clone(),
                        "siteUrl": s.site_url.clone(),
                        "sourceType": format!("{:?}", s.source_type)
                    }))
                },
                "readerState": {
                    "isRead": user_state.as_ref().map(|s| s.is_read).unwrap_or(false),
                    "isStarred": user_state.as_ref().map(|s| s.is_starred).unwrap_or(false),
                    "readProgress": user_state.as_ref().and_then(|s| s.read_progress),
                    "readAt": user_state.as_ref().and_then(|s| s.read_at.map(|t| t.to_string())),
                    "notes": user_state.as_ref().and_then(|s| s.notes.clone())
                },
                "navigation": {
                    "canGoBack": true,
                    "parentPath": format!("/brew/source/{}", item.source_id)
                },
                "actions": {
                    "available": [
                        "markAsRead", "toggleStar", "updateProgress",
                        "addNote", "fetchFulltext", "shareArticle"
                    ]
                }
            }))
        }
        _ => Err(format!("Unknown brew page level: {}", level)),
    }
}

// ============================================================================
// Tapp 页面内容
// ============================================================================

async fn execute_tapp_page_content(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    super::ui_control::execute_tapp_page_content(params, ctx).await
}

// ============================================================================
// 音乐平台相关
// ============================================================================

/// 读取网易云歌单数据
async fn execute_netease_playlist(params: &HashMap<String, Value>) -> Result<Value, String> {
    let query_type = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("playlists");

    let cache_file = "cache/platforms/netease_filtered.json";
    if let Ok(content) = tokio::fs::read_to_string(cache_file).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            let result = match query_type {
                "playlists" => data
                    .get("content_analysis")
                    .and_then(|v| v.get("playlists"))
                    .cloned()
                    .unwrap_or(json!([])),
                "recent" => data
                    .get("content_analysis")
                    .and_then(|v| v.get("recent_songs"))
                    .cloned()
                    .unwrap_or(json!([])),
                "favorites" => data
                    .get("content_analysis")
                    .and_then(|v| v.get("favorite_songs"))
                    .cloned()
                    .unwrap_or(json!([])),
                _ => json!([]),
            };

            return Ok(json!({
                "type": query_type,
                "data": result
            }));
        }
    }

    Err("Failed to read Netease data".to_string())
}

/// 联网搜索网易云歌单
async fn execute_netease_search_playlist(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let keyword = params
        .get("keyword")
        .and_then(|v| v.as_str())
        .unwrap_or("轻音乐");
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

    tracing::info!(
        keyword = %keyword,
        limit = limit,
        "[data_read] Searching Netease playlists online"
    );

    // 映射关键词到网易云分类
    let mapped_category = map_keyword_to_netease_category(keyword);

    // 判断是否需要 AI 辅助理解
    let final_category = if mapped_category == keyword && keyword.chars().count() > 2 {
        // 映射没变化，说明是模糊描述，尝试调用 AI 理解
        if let Some(ai_analyzer) = ctx.ai_analyzer {
            match ai_understand_music_intent(ai_analyzer, keyword).await {
                Ok(ai_category) => {
                    tracing::info!(
                        keyword = %keyword,
                        ai_category = %ai_category,
                        "[data_read] AI understood music intent"
                    );
                    ai_category
                }
                Err(_) => "轻音乐".to_string(),
            }
        } else {
            "轻音乐".to_string()
        }
    } else {
        mapped_category
    };

    // 尝试多种分类
    let categories_to_try = vec![
        final_category.clone(),
        "轻音乐".to_string(),
        "流行".to_string(),
    ];

    let mut all_playlists: Vec<Value> = Vec::new();

    // 使用项目已有的 IP 伪装
    let client_ip = get_random_china_ip();
    let proxy_ip = get_random_china_ip();
    let forwarded_for = format!("{}, {}", client_ip, proxy_ip);
    let user_agent = get_random_user_agent();
    let client = reqwest::Client::new();

    for category in &categories_to_try {
        let encoded_category = urlencoding::encode(category);
        let search_url = format!(
            "https://music.163.com/api/playlist/list?cat={}&order=hot&offset=0&total=true&limit={}",
            encoded_category,
            limit * 2
        );

        if let Ok(response) = client
            .get(&search_url)
            .header("Referer", "https://music.163.com/")
            .header("User-Agent", user_agent)
            .header("X-Forwarded-For", forwarded_for.clone())
            .header("X-Real-IP", client_ip.clone())
            .send()
            .await
        {
            if let Ok(data) = response.json::<Value>().await {
                if let Some(playlists) = data.get("playlists").and_then(|p| p.as_array()) {
                    if !playlists.is_empty() {
                        tracing::info!(
                            category = %category,
                            count = playlists.len(),
                            "[data_read] Found playlists with category"
                        );
                        all_playlists = playlists.clone();
                        break;
                    }
                }
            }
        }
    }

    // 按关键词相关性排序
    let keyword_lower = keyword.to_lowercase();
    let mut scored_playlists: Vec<(Value, i32)> = all_playlists
        .into_iter()
        .map(|p| {
            let score = score_playlist_relevance(&p, &keyword_lower);
            (p, score)
        })
        .collect();

    scored_playlists.sort_by_key(|b| Reverse(b.1));

    // 格式化输出
    let formatted_playlists: Vec<Value> = scored_playlists
        .into_iter()
        .take(limit)
        .map(|(p, _score)| {
            json!({
                "id": p.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
                "name": p.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                "trackCount": p.get("trackCount").and_then(|v| v.as_i64()).unwrap_or(0),
                "playCount": p.get("playCount").and_then(|v| v.as_i64()).unwrap_or(0),
                "coverUrl": p.get("coverImgUrl").and_then(|v| v.as_str()).unwrap_or(""),
                "creator": p.get("creator").and_then(|c| c.get("nickname")).and_then(|v| v.as_str()).unwrap_or(""),
                "description": p.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                "tags": p.get("tags").and_then(|t| t.as_array()).map(|arr|
                    arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>()
                ).unwrap_or_default()
            })
        })
        .collect();

    if formatted_playlists.is_empty() {
        return Ok(json!({
            "success": false,
            "keyword": keyword,
            "playlists": [],
            "message": crate::services::agent::response_agent::playlist_not_found()
        }));
    }

    let recommended_id = formatted_playlists
        .first()
        .and_then(|p| p.get("id"))
        .and_then(|id| id.as_i64())
        .map(|n| n.to_string());

    let recommended_name = formatted_playlists
        .first()
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());

    Ok(json!({
        "success": true,
        "keyword": keyword,
        "mappedCategory": final_category,
        "playlists": formatted_playlists,
        "recommendedPlaylistId": recommended_id,
        "recommendedPlaylistName": recommended_name,
        "source": "netease",
        "message": crate::services::agent::response_agent::playlist_found(formatted_playlists.len(), keyword)
    }))
}

/// 读取 GitHub 仓库数据
async fn execute_github_repos(params: &HashMap<String, Value>) -> Result<Value, String> {
    let query_type = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("repos");

    let cache_file = "cache/platforms/github_filtered.json";
    if let Ok(content) = tokio::fs::read_to_string(cache_file).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            let result = match query_type {
                "repos" => data
                    .get("content_analysis")
                    .and_then(|v| v.get("recent_repos"))
                    .cloned()
                    .unwrap_or(json!([])),
                "contributions" => data
                    .get("content_analysis")
                    .and_then(|v| v.get("contribution_calendar"))
                    .cloned()
                    .unwrap_or(json!([])),
                "starred" => data
                    .get("content_analysis")
                    .and_then(|v| v.get("starred_repos"))
                    .cloned()
                    .unwrap_or(json!([])),
                _ => data.get("content_analysis").cloned().unwrap_or(json!({})),
            };

            return Ok(json!({
                "type": query_type,
                "data": result
            }));
        }
    }

    Err("Failed to read GitHub data".to_string())
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 映射关键词到网易云分类
fn map_keyword_to_netease_category(keyword: &str) -> String {
    let keyword_lower = keyword.to_lowercase();

    // 关键词到网易云分类的映射
    let mappings: Vec<(&[&str], &str)> = vec![
        (
            &["工作", "办公", "专注", "学习", "编程", "coding"],
            "轻音乐",
        ),
        (&["睡眠", "睡前", "入睡", "助眠", "安静"], "轻音乐"),
        (&["放松", "舒缓", "休闲", "轻松"], "轻音乐"),
        (&["运动", "健身", "跑步", "动感", "激情"], "电子"),
        (&["古风", "中国风", "国风"], "古风"),
        (&["摇滚", "rock"], "摇滚"),
        (&["民谣", "folk"], "民谣"),
        (&["电子", "edm", "electronic", "dj"], "电子"),
        (&["说唱", "嘻哈", "rap", "hip-hop"], "说唱"),
        (&["流行", "pop", "热门"], "流行"),
        (&["古典", "classical", "钢琴", "交响"], "古典"),
        (&["爵士", "jazz"], "爵士"),
        (&["蓝调", "blues"], "蓝调"),
        (&["乡村", "country"], "乡村"),
        (&["acg", "动漫", "二次元", "日语"], "ACG"),
        (&["华语", "中文", "国语"], "华语"),
        (&["英文", "欧美", "英语"], "欧美"),
        (&["日语", "日本", "日系"], "日语"),
        (&["韩语", "韩国", "韩流", "kpop"], "韩语"),
    ];

    for (keywords, category) in mappings {
        for k in keywords {
            if keyword_lower.contains(k) {
                return category.to_string();
            }
        }
    }

    // 没匹配上就返回原关键词
    keyword.to_string()
}

/// 计算歌单与关键词的相关性分数
fn score_playlist_relevance(playlist: &Value, keyword: &str) -> i32 {
    let mut score = 0;

    // 名称匹配
    if let Some(name) = playlist.get("name").and_then(|n| n.as_str()) {
        let name_lower = name.to_lowercase();
        if name_lower.contains(keyword) {
            score += 100;
        }
        // 部分匹配
        for word in keyword.split_whitespace() {
            if name_lower.contains(word) {
                score += 30;
            }
        }
    }

    // 描述匹配
    if let Some(desc) = playlist.get("description").and_then(|d| d.as_str()) {
        let desc_lower = desc.to_lowercase();
        if desc_lower.contains(keyword) {
            score += 50;
        }
        // 检查相关词
        for term in get_related_terms(keyword) {
            if desc_lower.contains(term) {
                score += 15;
            }
        }
    }

    // 标签匹配
    if let Some(tags) = playlist.get("tags").and_then(|t| t.as_array()) {
        for tag in tags {
            if let Some(tag_str) = tag.as_str() {
                if tag_str.to_lowercase().contains(keyword) {
                    score += 80;
                }
                // 检查相关词
                for term in get_related_terms(keyword) {
                    if tag_str.to_lowercase().contains(term) {
                        score += 25;
                    }
                }
            }
        }
    }

    // 播放量加分（热门歌单优先）
    if let Some(play_count) = playlist.get("playCount").and_then(|p| p.as_i64()) {
        score += (play_count / 1_000_000).min(20) as i32;
    }

    score
}

/// 获取关键词的相关词/同义词
fn get_related_terms(keyword: &str) -> Vec<&'static str> {
    let term_groups: &[&[&str]] = &[
        // 放松相关
        &[
            "放松", "轻松", "舒缓", "休息", "休闲", "慵懒", "惬意", "chill",
        ],
        // 安静相关
        &["安静", "静心", "静谧", "宁静", "平静", "冥想", "禅"],
        // 学习/工作相关
        &[
            "学习", "阅读", "读书", "看书", "工作", "专注", "集中", "效率", "coding", "编程",
        ],
        // 睡眠相关
        &["睡眠", "助眠", "入睡", "晚安", "深夜", "夜晚", "催眠"],
        // 运动相关
        &["运动", "健身", "跑步", "锻炼", "燃脂", "有氧", "gym"],
        // 轻音乐相关
        &[
            "轻音乐",
            "纯音乐",
            "器乐",
            "钢琴",
            "吉他",
            "小提琴",
            "无人声",
        ],
        // 治愈相关
        &["治愈", "温暖", "温馨", "舒适", "暖心", "感动"],
        // 伤感相关
        &["伤感", "难过", "悲伤", "失恋", "分手", "孤独", "寂寞"],
        // 欢快相关
        &["欢快", "开心", "快乐", "愉悦", "活力", "元气", "阳光"],
        // ACG相关
        &["acg", "动漫", "二次元", "日漫", "番剧", "游戏", "anime"],
    ];

    for group in term_groups {
        if group
            .iter()
            .any(|t| keyword.contains(t) || t.contains(keyword))
        {
            return group.iter().filter(|&&t| t != keyword).copied().collect();
        }
    }

    Vec::new()
}

/// AI 理解音乐意图
async fn ai_understand_music_intent(
    ai_analyzer: &crate::services::analyzer::AiAnalyzer,
    user_input: &str,
) -> Result<String, String> {
    let prompt = format!(
        r#"用户想听的音乐描述是："{}"

请分析用户的音乐需求，然后返回一个最匹配的网易云音乐分类标签。
可选分类：流行、轻音乐、电子、摇滚、民谣、说唱、古风、古典、爵士、蓝调、ACG、华语、欧美、日语、韩语

只返回分类名称，不要任何解释。"#,
        user_input
    );

    match ai_analyzer.analyze(&prompt).await {
        Ok(response) => {
            let category = response.trim().to_string();
            // 验证返回的分类是否有效
            let valid_categories = [
                "流行",
                "轻音乐",
                "电子",
                "摇滚",
                "民谣",
                "说唱",
                "古风",
                "古典",
                "爵士",
                "蓝调",
                "ACG",
                "华语",
                "欧美",
                "日语",
                "韩语",
            ];
            if valid_categories.contains(&category.as_str()) {
                Ok(category)
            } else {
                // AI 返回了无效分类，使用默认值
                Ok("轻音乐".to_string())
            }
        }
        Err(e) => Err(format!("AI analysis failed: {}", e)),
    }
}

// ============================================================================
// 追加的数据读取能力
// ============================================================================

/// 获取 B 站追番列表
async fn execute_bilibili_bangumi(params: &HashMap<String, Value>) -> Result<Value, String> {
    let bangumi_type = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("anime");

    let cache_file = "cache/platforms/bilibili_filtered.json";
    if let Ok(content) = tokio::fs::read_to_string(cache_file).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            let bangumis = data
                .get("content_analysis")
                .and_then(|v| v.get("anime_analysis"))
                .cloned()
                .unwrap_or(json!([]));

            return Ok(json!({
                "type": bangumi_type,
                "bangumis": bangumis,
                "total": bangumis.as_array().map(|a| a.len()).unwrap_or(0)
            }));
        }
    }
    Err("Failed to read Bilibili bangumi data".to_string())
}

/// 获取 Steam 愿望单
async fn execute_steam_wishlist(params: &HashMap<String, Value>) -> Result<Value, String> {
    let _ = params; // 未使用参数
    let cache_file = "cache/platforms/steam_filtered.json";
    if let Ok(content) = tokio::fs::read_to_string(cache_file).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            let wishlist = data
                .get("content_analysis")
                .and_then(|v| v.get("wishlist"))
                .cloned()
                .unwrap_or(json!([]));

            return Ok(json!({
                "wishlist": wishlist,
                "total": wishlist.as_array().map(|a| a.len()).unwrap_or(0)
            }));
        }
    }
    Err("Failed to read Steam wishlist data".to_string())
}

/// 获取 Tapp Widget 配置
async fn execute_tapp_widget(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let tapp_id = params
        .get("tappId")
        .and_then(Value::as_str)
        .ok_or("Missing tappId parameter")?;
    let mut page_params = params.clone();
    page_params.insert("level".to_string(), json!("widgets"));
    let page = super::ui_control::execute_tapp_page_content(&page_params, ctx).await?;
    let widgets = page
        .get("content")
        .and_then(|content| content.get("widgets"))
        .cloned()
        .unwrap_or_else(|| json!([]));

    Ok(json!({
        "tappId": tapp_id,
        "total": widgets.as_array().map(Vec::len).unwrap_or(0),
        "widgets": widgets
    }))
}

/// 权限检查
async fn execute_permission_check(params: &HashMap<String, Value>) -> Result<Value, String> {
    let permission = params
        .get("permission")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let role = params
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("user");

    // 基础权限列表
    let basic_permissions = [
        "widget:register",
        "platform:read",
        "report:read",
        "storage",
        "ui:notification",
        "ui:fullscreen",
        "ui:theme",
        "ui:confirm",
        "media:read",
        "event:subscribe",
    ];

    let elevated_permissions = [
        "ai:generate",
        "ai:analyze",
        "ai:chat",
        "report:write",
        "network:fetch",
        "media:control",
        "component:theme",
        "shortcut:register",
        "event:publish",
    ];

    let privileged_permissions = ["platform:write", "platform:register", "component:agent"];

    let has_permission = match role {
        "admin" => true,
        "user" => {
            basic_permissions.contains(&permission) || elevated_permissions.contains(&permission)
        }
        "guest" => basic_permissions.contains(&permission),
        _ => false,
    };

    let reason = if has_permission {
        "Permission granted".to_string()
    } else if privileged_permissions.contains(&permission) {
        "This permission requires admin role".to_string()
    } else {
        format!(
            "Permission '{}' not available for role '{}'",
            permission, role
        )
    };

    Ok(json!({
        "permission": permission,
        "role": role,
        "hasPermission": has_permission,
        "reason": reason
    }))
}

/// 平台连接状态
async fn execute_platform_connection(params: &HashMap<String, Value>) -> Result<Value, String> {
    let platform = params
        .get("platform")
        .and_then(|v| v.as_str())
        .unwrap_or("all");

    let platforms: Vec<&str> = if platform == "all" {
        VALID_PLATFORMS.to_vec()
    } else {
        vec![platform]
    };

    let mut connections = Vec::new();
    for p in platforms {
        let cache_file = format!("cache/platforms/{}_filtered.json", p);
        let connected = tokio::fs::metadata(&cache_file).await.is_ok();
        let last_sync = if connected {
            tokio::fs::metadata(&cache_file)
                .await
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339())
        } else {
            None
        };

        connections.push(json!({
            "platform": p,
            "connected": connected,
            "lastSync": last_sync
        }));
    }

    Ok(json!({
        "connections": connections
    }))
}

/// 数据统计概览
async fn execute_stats_overview(params: &HashMap<String, Value>) -> Result<Value, String> {
    let _ = params; // 未使用参数
    let mut stats = json!({
        "totalGames": 0,
        "totalPlaytime": 0,
        "totalAnime": 0,
        "totalBangumiCollections": 0,
        "totalSongs": 0,
        "totalRepos": 0
    });

    // Steam 统计
    if let Ok(content) = tokio::fs::read_to_string("cache/platforms/steam_filtered.json").await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            if let Some(games) = data
                .get("content_analysis")
                .and_then(|v| v.get("recent_games"))
                .and_then(|v| v.as_array())
            {
                stats["totalGames"] = json!(games.len());
                let total_time: i64 = games
                    .iter()
                    .filter_map(|g| g.get("playtime").and_then(|t| t.as_i64()))
                    .sum();
                stats["totalPlaytime"] = json!(total_time);
            }
        }
    }

    // Bilibili 统计
    if let Ok(content) = tokio::fs::read_to_string("cache/platforms/bilibili_filtered.json").await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            if let Some(anime) = data
                .get("content_analysis")
                .and_then(|v| v.get("anime_analysis"))
                .and_then(|v| v.as_array())
            {
                stats["totalAnime"] = json!(anime.len());
            }
        }
    }

    // Bangumi 统计
    if let Ok(content) = tokio::fs::read_to_string("cache/platforms/bangumi_filtered.json").await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            let items = extract_platform_items("bangumi", &data);
            stats["totalBangumiCollections"] = json!(items.len());
        }
    }

    // GitHub 统计
    if let Ok(content) = tokio::fs::read_to_string("cache/platforms/github_filtered.json").await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            if let Some(repos) = data
                .get("content_analysis")
                .and_then(|v| v.get("recent_repos"))
                .and_then(|v| v.as_array())
            {
                stats["totalRepos"] = json!(repos.len());
            }
        }
    }

    // Netease 统计
    if let Ok(content) = tokio::fs::read_to_string("cache/platforms/netease_filtered.json").await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            if let Some(songs) = data
                .get("content_analysis")
                .and_then(|v| v.get("favorite_songs"))
                .and_then(|v| v.as_array())
            {
                stats["totalSongs"] = json!(songs.len());
            }
        }
    }

    Ok(stats)
}

/// 用户画像摘要
async fn execute_profile_summary(params: &HashMap<String, Value>) -> Result<Value, String> {
    let platforms = params
        .get("platforms")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_else(|| VALID_PLATFORMS.to_vec());

    let interests: Vec<String> = Vec::new();
    let mut activities = Vec::new();
    let mut platform_stats = json!({});

    for platform in &platforms {
        let cache_file = format!("cache/platforms/{}_filtered.json", platform);
        if let Ok(content) = tokio::fs::read_to_string(&cache_file).await {
            if let Ok(data) = serde_json::from_str::<Value>(&content) {
                // 提取用户名
                let username = data
                    .get("user_summary")
                    .and_then(|v| v.get("username"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown");

                // 提取统计
                let items = extract_platform_items(platform, &data);

                platform_stats[*platform] = json!({
                    "username": username,
                    "itemCount": items.len()
                });

                // 收集兴趣
                match *platform {
                    "steam" => activities.push("游戏".to_string()),
                    "bilibili" => activities.push("追番".to_string()),
                    "bangumi" => activities.push("收藏番剧/书籍/游戏".to_string()),
                    "mal" => activities.push("动画/漫画列表".to_string()),
                    "github" => activities.push("编程".to_string()),
                    "netease" => activities.push("听歌".to_string()),
                    "x" => activities.push("发帖与互动".to_string()),
                    "discord" => activities.push("社区交流".to_string()),
                    _ => {}
                }
            }
        }
    }

    Ok(json!({
        "summary": crate::services::agent::response_agent::active_platforms(platforms.len()),
        "interests": interests,
        "activities": activities,
        "platformStats": platform_stats
    }))
}

/// 全局搜索
async fn execute_search_global(params: &HashMap<String, Value>) -> Result<Value, String> {
    let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
    let platforms = params
        .get("platforms")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_else(|| VALID_PLATFORMS.to_vec());
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

    // 检查是否有有效的搜索关键词
    if query.is_empty() {
        return Ok(json!({
            "query": query,
            "results": [],
            "total": 0,
            "message": crate::services::agent::response_agent::search_empty_hint(),
            "supportedPlatforms": VALID_PLATFORMS,
            "hint": "试试搜索你已有数据中的内容，例如：'搜索我的 Steam 游戏'、'查看 GitHub 仓库'"
        }));
    }

    let mut results = Vec::new();
    let query_lower = query.to_lowercase();

    for platform in &platforms {
        let cache_file = format!("cache/platforms/{}_filtered.json", platform);
        if let Ok(content) = tokio::fs::read_to_string(&cache_file).await {
            if let Ok(data) = serde_json::from_str::<Value>(&content) {
                let items = extract_platform_items(platform, &data);

                for item in items {
                    // 搜索标题、名称等字段
                    let matches = item
                        .get("name")
                        .or(item.get("title"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_lowercase().contains(&query_lower))
                        .unwrap_or(false);

                    if matches {
                        results.push(json!({
                            "platform": platform,
                            "item": item
                        }));

                        if results.len() >= limit {
                            break;
                        }
                    }
                }
            }
        }

        if results.len() >= limit {
            break;
        }
    }

    // 提供更好的无结果提示
    let message = if results.is_empty() {
        crate::services::agent::response_agent::search_no_results(query)
    } else {
        crate::services::agent::response_agent::search_results_found(results.len(), query)
    };

    Ok(json!({
        "query": query,
        "results": results,
        "total": results.len(),
        "message": message,
        "searchedPlatforms": platforms
    }))
}

/// 查询后台任务状态
async fn execute_task_status(params: &HashMap<String, Value>) -> Result<Value, String> {
    let task_id = params.get("taskId").and_then(|v| v.as_str());
    let platform = params.get("platform").and_then(|v| v.as_str());

    // 模拟任务状态查询
    Ok(json!({
        "taskId": task_id,
        "platform": platform,
        "status": "completed",
        "progress": 100.0,
        "message": "Task status query - requires database integration"
    }))
}

/// 元数据历史查询
async fn execute_metadata_history(params: &HashMap<String, Value>) -> Result<Value, String> {
    let platform = params
        .get("platform")
        .and_then(|v| v.as_str())
        .unwrap_or("all");
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10);

    // 读取缓存的历史数据
    let mut history = Vec::new();
    let cache_file = format!("cache/platforms/{}_filtered.json", platform);

    if let Ok(metadata) = tokio::fs::metadata(&cache_file).await {
        if let Ok(modified) = metadata.modified() {
            let modified_time = chrono::DateTime::<chrono::Utc>::from(modified);
            history.push(json!({
                "platform": platform,
                "lastModified": modified_time.to_rfc3339(),
                "size": metadata.len()
            }));
        }
    }

    Ok(json!({
        "platform": platform,
        "history": history,
        "limit": limit,
        "note": "Full history requires database integration"
    }))
}

/// Tapp 列表
async fn execute_tapp_list(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let admin_id = crate::api::tapp_runtime::common::get_admin_user_id(ctx.db)
        .await
        .map_err(|(_, body)| {
            body.0
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Failed to resolve administrator")
                .to_string()
        })?;
    let is_admin = crate::services::agent::user_is_current_admin(ctx.db, ctx.user_id).await;
    let mut query = tapps::Entity::find();
    if !is_admin {
        query = query.filter(
            tapps::Column::UserId
                .eq(ctx.user_id)
                .or(tapps::Column::UserId.eq(admin_id)),
        );
    }

    let enabled_filter = params.get("enabled").and_then(Value::as_bool);
    let category_filter = params.get("category").and_then(Value::as_str);
    let records = query
        .order_by_desc(tapps::Column::UpdatedAt)
        .all(ctx.db)
        .await
        .map_err(|e| format!("Failed to fetch Tapps: {e}"))?;
    let items: Vec<Value> = records
        .into_iter()
        .filter(|tapp| {
            enabled_filter.is_none_or(|enabled| {
                let active = matches!(
                    tapp.status,
                    tapps::TappStatus::Installed | tapps::TappStatus::Running
                );
                active == enabled
            })
        })
        .filter(|tapp| {
            category_filter.is_none_or(|category| {
                tapp.manifest
                    .get("category")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == category)
            })
        })
        .map(|tapp| {
            json!({
                "id": tapp.tapp_id,
                "name": tapp.name,
                "version": tapp.version,
                "description": tapp.description,
                "icon": tapp.icon,
                "status": format!("{:?}", tapp.status).to_lowercase(),
                "hasCore": tapp.manifest.get("hasCore").and_then(Value::as_bool).unwrap_or(false),
                "hasPage": tapp.manifest.get("hasPage").and_then(Value::as_bool).unwrap_or(false),
                "hasWidget": tapp.manifest.get("hasWidget").and_then(Value::as_bool).unwrap_or(false),
                "backgroundRequirements": tapp.manifest
                    .get("backgroundRequirements")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
            })
        })
        .collect();

    Ok(json!({
        "tapps": items,
        "total": items.len()
    }))
}

/// 定时任务列表
async fn execute_scheduler_list(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let tapp_id = params.get("tappId").and_then(Value::as_str);
    let enabled_filter = params.get("enabled").and_then(Value::as_bool);
    let scheduler = crate::api::tapp_scheduler::scheduler_engine()?;
    let scheduler = scheduler.read().await;
    let tasks = scheduler.list_tasks(ctx.user_id, tapp_id).await?;
    let tasks: Vec<Value> = tasks
        .into_iter()
        .filter(|task| enabled_filter.is_none_or(|enabled| task.enabled == enabled))
        .map(|task| {
            let schedule_type = match task.schedule_type {
                tapp_scheduled_tasks::ScheduleType::Cron => "cron",
                tapp_scheduled_tasks::ScheduleType::Interval => "interval",
                tapp_scheduled_tasks::ScheduleType::Once => "once",
                tapp_scheduled_tasks::ScheduleType::Daily => "daily",
            };
            let execution_target = match task.execution_target {
                tapp_scheduled_tasks::ExecutionTarget::Backend => "backend",
                tapp_scheduled_tasks::ExecutionTarget::Frontend => "frontend",
                tapp_scheduled_tasks::ExecutionTarget::Both => "both",
            };
            let scope = match task.scope {
                tapp_scheduled_tasks::TaskScope::User => "user",
                tapp_scheduled_tasks::TaskScope::Tapp => "tapp",
                tapp_scheduled_tasks::TaskScope::TappPerUser => "tapp-per-user",
                tapp_scheduled_tasks::TaskScope::Global => "global",
            };
            json!({
                "id": task.id,
                "taskId": task.task_id,
                "tappId": task.tapp_id,
                "name": task.name,
                "scheduleType": schedule_type,
                "schedule": task.schedule_config,
                "payload": task.payload,
                "executionTarget": execution_target,
                "backendActions": task.backend_actions,
                "enabled": task.enabled,
                "scope": scope,
                "nextRunAt": task.next_run_at.map(|value| value.to_rfc3339()),
                "lastRunAt": task.last_run_at.map(|value| value.to_rfc3339()),
                "lastRunResult": task.last_run_result,
                "stats": task.stats,
            })
        })
        .collect();

    Ok(json!({
        "tasks": tasks,
        "total": tasks.len()
    }))
}

/// RSSHub 实例列表
async fn execute_rsshub_instances(params: &HashMap<String, Value>) -> Result<Value, String> {
    let _ = params; // 未使用参数
                    // RSSHub 实例列表 - 简化实现
    Ok(json!({
        "instances": [],
        "healthyCount": 0,
        "message": "RSSHub instances require database integration"
    }))
}

/// 上下文引用能力
async fn execute_context_reference(_params: &HashMap<String, Value>) -> Result<Value, String> {
    // context.reference 不应被直接调用——步骤间数据传递通过 executor 的
    // resolve_params() 自动处理 xxxFrom 引用。如果走到这里说明 recipe 配置有误。
    Err("context.reference 不应被直接调用。请使用 xxxFrom 参数引用上游步骤的输出。".to_string())
}

// ============================================================================
// 补充能力
// ============================================================================

/// 数据库查询 (anime/game/artist)
async fn execute_database_query(
    capability_id: &str,
    params: &HashMap<String, Value>,
) -> Result<Value, String> {
    let db_type = capability_id.split('.').next_back().unwrap_or("anime");
    let title_query = params
        .get("title")
        .or(params.get("name"))
        .and_then(|v| v.as_str());
    let genre_query = params.get("genre").and_then(|v| v.as_str());

    let db_file = format!("data/{}_database.json", db_type);
    let content = tokio::fs::read_to_string(&db_file).await.map_err(|_| {
        format!(
            "{} 数据库文件不存在（{}）。请先导入相关数据。",
            db_type, db_file
        )
    })?;

    let data: Value = serde_json::from_str(&content).unwrap_or(json!({}));
    let entries = data
        .get("entries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let results: Vec<Value> = entries
        .into_iter()
        .filter(|entry| {
            let title_match = title_query
                .map(|q| {
                    entry
                        .get("title")
                        .and_then(|v| v.as_str())
                        .map(|t| t.to_lowercase().contains(&q.to_lowercase()))
                        .unwrap_or(false)
                })
                .unwrap_or(true);

            let genre_match = genre_query
                .map(|q| {
                    entry
                        .get("genres")
                        .or(entry.get("genre"))
                        .and_then(|v| v.as_array())
                        .map(|genres| {
                            genres.iter().any(|g| {
                                g.as_str()
                                    .map(|s| s.to_lowercase().contains(&q.to_lowercase()))
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false)
                })
                .unwrap_or(true);

            title_match && genre_match
        })
        .take(50)
        .collect();

    Ok(json!({
        "database": db_type,
        "results": results,
        "count": results.len()
    }))
}

/// 随机内容
async fn execute_random_content(params: &HashMap<String, Value>) -> Result<Value, String> {
    let platform = params
        .get("platform")
        .and_then(|v| v.as_str())
        .unwrap_or("steam");
    let count = params.get("count").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

    let cache_file = format!("cache/platforms/{}_filtered.json", platform);
    if let Ok(content) = tokio::fs::read_to_string(&cache_file).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            let items = extract_platform_items_for_random(platform, &data);

            // 随机选取
            use rand::seq::IndexedRandom;
            let mut rng = rand::rng();
            let selected: Vec<_> = items
                .sample(&mut rng, count.min(items.len()))
                .cloned()
                .collect();

            return Ok(json!({
                "platform": platform,
                "items": selected,
                "totalAvailable": items.len()
            }));
        }
    }

    Err(format!("No data available for platform: {}", platform))
}

/// 提取平台项目用于随机选择
fn extract_platform_items_for_random(platform: &str, data: &Value) -> Vec<Value> {
    match platform {
        "steam" => data
            .get("games")
            .or(data.get("items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        "bilibili" => data
            .get("videos")
            .or(data.get("items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        "github" => data
            .get("repos")
            .or(data.get("items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        "netease" => data
            .get("songs")
            .or(data.get("items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        "bangumi" => data
            .get("collections")
            .or(data.get("items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        "mal" => {
            let mut items = Vec::new();
            for key in ["anime_list", "manga_list", "items"] {
                if let Some(arr) = data.get(key).and_then(|v| v.as_array()) {
                    items.extend(arr.clone());
                }
            }
            if items.is_empty() {
                // filtered cache 走 content_analysis
                return extract_platform_items("mal", data);
            }
            items
        }
        "x" => data
            .get("tweets")
            .or(data.get("items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        "discord" => data
            .get("guilds")
            .or(data.get("items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => data
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
    }
}

/// 报告列表
async fn execute_report_list(params: &HashMap<String, Value>) -> Result<Value, String> {
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
    let platform = params.get("platform").and_then(|v| v.as_str());

    let reports_dir = std::path::Path::new("data/reports");
    let mut reports = Vec::new();

    if reports_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(reports_dir) {
            for entry in entries.flatten().take(limit) {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    let filename = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown");

                    // 按平台过滤
                    if let Some(p) = platform {
                        if !filename.contains(p) {
                            continue;
                        }
                    }

                    let metadata = std::fs::metadata(&path).ok();
                    reports.push(json!({
                        "id": filename,
                        "path": path.to_string_lossy(),
                        "size": metadata.as_ref().map(|m| m.len()),
                        "modified": metadata.and_then(|m| m.modified().ok())
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs())
                    }));
                }
            }
        }
    }

    Ok(json!({
        "reports": reports,
        "total": reports.len()
    }))
}

// ============================================================================
// AI 联网搜索辅助函数
// ============================================================================

/// 为阅读列表触发 AI 联网搜索
/// 当数据库中找不到相关内容时，使用 Gemini Grounding Search 搜索网络
async fn trigger_ai_web_search_for_reading_list(
    query: &str,
    max_items: usize,
    _ctx: &HandlerContext<'_>,
) -> Result<Vec<Value>, String> {
    use crate::GLOBAL_DYNAMIC_CONFIG;

    // 从全局配置读取 Gemini API Key
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;

    tracing::info!(
        has_key = config.gemini_api_key.is_some(),
        "[AI Web Search] Checking Gemini API configuration"
    );

    let api_key = config.gemini_api_key.clone().ok_or_else(|| {
        tracing::error!("[AI Web Search] Gemini API Key is not configured");
        crate::services::agent::response_agent::api_key_not_configured("Gemini")
            + "，请在设置中配置 API Key"
    })?;

    if api_key.is_empty() {
        tracing::error!("[AI Web Search] Gemini API Key is empty");
        return Err(crate::services::agent::response_agent::api_key_not_configured("Gemini"));
    }

    let model = if config.gemini_model.is_empty() {
        "gemini-2.0-flash".to_string()
    } else {
        config.gemini_model.clone()
    };

    drop(config);

    // 构建搜索提示词 - 优化：更明确的指令，强调 JSON 格式和详细摘要
    let search_prompt = format!(
        r#"你是一个智能阅读助手。用户想要阅读关于「{query}」的文章。

任务：使用 Google Search 搜索相关的新闻、文章或资讯，然后整理成阅读列表。

输出要求：
1. 返回 {max_items} 篇最相关的文章
2. 必须是纯 JSON 数组格式，不要任何其他文字、解释或 markdown 标记
3. 每篇文章必须包含以下字段：
   - "id": 从 1 开始的数字
   - "title": 文章完整标题（string，不要截断）
   - "link": 文章的原始 URL（⚠️ 重要：必须是文章页面的真实 URL，不能是 Google 搜索结果页面或重定向链接，必须以 https:// 或 http:// 开头）
   - "summary": 文章内容摘要（string，⚠️ 重要：150-300 字，详细描述文章的主要内容、核心观点和关键信息，让读者无需点开就能了解文章大意）
   - "sourceName": 来源网站名称（string）
   - "author": 作者（string，如不确定填 ""）
   - "publishedAt": ISO 8601 日期时间格式（string，如 "2026-01-10T12:00:00Z"）
   - "relevanceReason": 推荐理由（string，一句话说明为什么这篇文章值得阅读）

筛选标准：
- 优先选择权威媒体和专业网站的内容
- 内容必须与「{query}」高度相关
- 优先最新发布的内容
- 排除付费墙、需要登录的内容
- 排除聚合页面、搜索结果页，只要实际文章页

⚠️ 关于 link 字段的特别说明：
- 必须是可以直接访问的文章页面 URL
- 不要使用 Google AMP 链接（google.com/amp/...）
- 不要使用搜索结果链接（google.com/url?...）
- 如果原始 URL 包含追踪参数，保留主要路径即可

示例输出格式：
[{{"id":1,"title":"完整的文章标题","link":"https://www.example.com/news/article-123","summary":"这篇文章详细介绍了...（150-300字的详细摘要）","sourceName":"Example新闻","author":"张三","publishedAt":"2026-01-10T12:00:00Z","relevanceReason":"推荐理由"}}]

现在请搜索并返回 JSON 数组："#,
        query = query,
        max_items = max_items
    );

    // 构建 Gemini API 请求（带 Google Search grounding）
    let request_body = json!({
        "contents": [{
            "parts": [{
                "text": search_prompt
            }]
        }],
        "tools": [{
            "google_search": {}
        }],
        "generationConfig": {
            "temperature": 0.2,
            "maxOutputTokens": 8192,
            "responseMimeType": "application/json"
        }
    });

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model, api_key
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    tracing::info!(query = %query, "[AI Web Search] Searching for reading list content");

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Gemini API request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("Gemini API error {}: {}", status, error_text));
    }

    let response_json: Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Gemini response: {}", e))?;

    // 提取 AI 回复内容
    let ai_text = response_json
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("");

    // 尝试从 AI 回复中提取 JSON 数组
    let mut results = extract_json_array_from_ai_response(ai_text);

    // 如果从文本中提取失败，尝试从 grounding metadata 提取
    if results.is_empty() {
        tracing::info!("[AI Web Search] Trying to extract from grounding metadata");

        // 先收集 grounding supports 中的内容片段（用于生成摘要）
        let mut content_snippets: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();

        if let Some(grounding_supports) = response_json
            .get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("groundingMetadata"))
            .and_then(|m| m.get("groundingSupports"))
            .and_then(|s| s.as_array())
        {
            for support in grounding_supports {
                if let (Some(segment), Some(chunk_indices)) = (
                    support
                        .get("segment")
                        .and_then(|s| s.get("text"))
                        .and_then(|t| t.as_str()),
                    support
                        .get("groundingChunkIndices")
                        .and_then(|i| i.as_array()),
                ) {
                    for idx in chunk_indices {
                        if let Some(idx_num) = idx.as_u64() {
                            content_snippets
                                .entry(idx_num.to_string())
                                .or_default()
                                .push(segment.to_string());
                        }
                    }
                }
            }
        }

        if let Some(grounding_metadata) = response_json
            .get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("groundingMetadata"))
        {
            if let Some(chunks) = grounding_metadata
                .get("groundingChunks")
                .and_then(|c| c.as_array())
            {
                for (idx, chunk) in chunks.iter().enumerate().take(max_items) {
                    if let Some(web) = chunk.get("web") {
                        let uri = web.get("uri").and_then(|u| u.as_str()).unwrap_or("");
                        let title = web
                            .get("title")
                            .and_then(|t| t.as_str())
                            .unwrap_or("未知标题");

                        // 跳过 Google 搜索结果页和 AMP 链接
                        if uri.is_empty()
                            || uri.contains("google.com/url")
                            || uri.contains("google.com/amp")
                            || uri.contains("webcache.googleusercontent.com")
                        {
                            continue;
                        }

                        // 从 content_snippets 生成摘要
                        let summary = content_snippets
                            .get(&idx.to_string())
                            .map(|snippets| snippets.join(" "))
                            .filter(|s| s.len() > 20)
                            .unwrap_or_else(|| {
                                format!("来自 {} 的文章: {}", extract_domain_from_url(uri), title)
                            });

                        results.push(json!({
                            "id": results.len() + 1,
                            "title": title,
                            "link": uri,
                            "summary": summary,
                            "sourceName": extract_domain_from_url(uri),
                            "publishedAt": chrono::Utc::now().to_rfc3339(),
                            "relevanceReason": "AI 联网搜索结果",
                            "fromWebSearch": true
                        }));
                    }
                }
            }
        }

        tracing::info!(
            results = results.len(),
            "[AI Web Search] Extracted from grounding metadata"
        );
    }

    // 确保每个结果都有必要的字段，并转换为正确格式
    let results: Vec<Value> = results
        .into_iter()
        .enumerate()
        .filter_map(|(idx, mut item)| {
            // 确保有 id（转为数字）
            let id = item
                .get("id")
                .and_then(|v| v.as_i64())
                .unwrap_or((idx + 1) as i64);
            item["id"] = json!(id);

            // 确保 title 存在
            if item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .is_empty()
            {
                item["title"] = json!("未知标题");
            }

            // 验证并清理 link
            let link = item.get("link").and_then(|v| v.as_str()).unwrap_or("");
            if link.is_empty() {
                return None;
            }

            // 清理链接：移除 Google 重定向和 AMP 链接
            let cleaned_link = clean_search_result_url(link);
            if cleaned_link.is_empty() || !cleaned_link.starts_with("http") {
                tracing::warn!(original_link = %link, "[AI Web Search] Invalid link, skipping");
                return None;
            }
            item["link"] = json!(cleaned_link);

            // 确保 sourceName 存在
            if item
                .get("sourceName")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .is_empty()
            {
                item["sourceName"] = json!(extract_domain_from_url(&cleaned_link));
            }

            // 确保 summary 存在且有足够长度
            let summary = item.get("summary").and_then(|v| v.as_str()).unwrap_or("");
            if summary.is_empty() || summary.len() < 30 {
                let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let source = item
                    .get("sourceName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                item["summary"] = json!(
                    crate::services::agent::response_agent::article_summary_placeholder(
                        source, title
                    )
                );
            }

            // 确保 publishedAt 存在
            if item
                .get("publishedAt")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .is_empty()
            {
                item["publishedAt"] = json!(chrono::Utc::now().to_rfc3339());
            }

            // 确保 author 存在
            if item.get("author").is_none() {
                item["author"] = json!("");
            }

            // 确保 relevanceReason 存在
            if item
                .get("relevanceReason")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .is_empty()
            {
                item["relevanceReason"] = json!("AI 联网搜索推荐");
            }

            // 标记来源
            item["fromWebSearch"] = json!(true);

            Some(item)
        })
        .take(max_items)
        .collect();

    tracing::info!(results = results.len(), "[AI Web Search] Search completed");

    Ok(results)
}

/// 清理搜索结果 URL，移除 Google 重定向和追踪参数
fn clean_search_result_url(url: &str) -> String {
    let url = url.trim();

    // 跳过 Google 搜索结果页面的重定向链接
    if url.contains("google.com/url?") {
        // 尝试从 Google 重定向链接中提取真实 URL
        if let Some(start) = url.find("url=").or_else(|| url.find("q=")) {
            let param_start = start
                + if url[start..].starts_with("url=") {
                    4
                } else {
                    2
                };
            let param_value = &url[param_start..];
            let end = param_value.find('&').unwrap_or(param_value.len());
            let decoded = urlencoding::decode(&param_value[..end]).unwrap_or_default();
            if decoded.starts_with("http") {
                return decoded.to_string();
            }
        }
        return String::new();
    }

    // 跳过 Google AMP 链接
    if url.contains("google.com/amp/") || url.contains("/amp/s/") {
        // 尝试提取原始 URL
        if let Some(amp_pos) = url.find("/amp/s/").or_else(|| url.find("google.com/amp/")) {
            let clean_start = if url[amp_pos..].starts_with("/amp/s/") {
                amp_pos + 7
            } else if let Some(pos) = url[amp_pos..].find("/amp/") {
                amp_pos + pos + 5
            } else {
                return String::new();
            };
            let cleaned = &url[clean_start..];
            // 添加 https:// 如果没有
            if cleaned.starts_with("http") {
                return cleaned.to_string();
            } else {
                return format!("https://{}", cleaned);
            }
        }
        return String::new();
    }

    // 跳过 Google 缓存
    if url.contains("webcache.googleusercontent.com") {
        return String::new();
    }

    // 移除常见的追踪参数
    if let Some(query_start) = url.find('?') {
        let base_url = &url[..query_start];
        let query = &url[query_start + 1..];

        // 保留必要的参数，移除追踪参数
        let tracking_params = [
            "utm_source",
            "utm_medium",
            "utm_campaign",
            "utm_content",
            "utm_term",
            "fbclid",
            "gclid",
            "ref",
            "source",
            "mc_cid",
            "mc_eid",
        ];

        let clean_params: Vec<&str> = query
            .split('&')
            .filter(|param| {
                let key = param.split('=').next().unwrap_or("");
                !tracking_params.contains(&key)
            })
            .collect();

        if clean_params.is_empty() {
            return base_url.to_string();
        } else {
            return format!("{}?{}", base_url, clean_params.join("&"));
        }
    }

    url.to_string()
}

/// 从 AI 响应中提取 JSON 数组
fn extract_json_array_from_ai_response(text: &str) -> Vec<Value> {
    // 尝试找到 JSON 数组
    let json_start = text.find('[');
    let json_end = text.rfind(']');

    if let (Some(start), Some(end)) = (json_start, json_end) {
        if end > start {
            let json_str = &text[start..=end];
            if let Ok(arr) = serde_json::from_str::<Vec<Value>>(json_str) {
                return arr;
            }
        }
    }

    // 尝试解析 markdown 代码块中的 JSON
    if text.contains("```json") {
        let parts: Vec<&str> = text.split("```json").collect();
        if parts.len() > 1 {
            if let Some(json_part) = parts[1].split("```").next() {
                if let Ok(arr) = serde_json::from_str::<Vec<Value>>(json_part.trim()) {
                    return arr;
                }
            }
        }
    }

    // 尝试普通代码块
    if text.contains("```") {
        let parts: Vec<&str> = text.split("```").collect();
        for part in parts {
            let trimmed = part.trim();
            if trimmed.starts_with('[') {
                if let Ok(arr) = serde_json::from_str::<Vec<Value>>(trimmed) {
                    return arr;
                }
            }
        }
    }

    vec![]
}

/// 从 URL 中提取域名
fn extract_domain_from_url(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .split('/')
        .next()
        .unwrap_or("未知来源")
        .to_string()
}

#[cfg(test)]
mod brew_db_helpers_tests {
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
            &by_guid,
            1,
            "other",
            "guid-1" // key also matches link
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
}
