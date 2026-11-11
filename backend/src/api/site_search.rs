//! `GET /api/search?q=`：Agent 面板的输入框边输边查站内内容。
//!
//! 访客不能提问，但能搜。只查公开的内容：已发布的笔记（和博客搜索同一套口径），
//! 资料库里按来源偏好显示的条目。每个模块按访客能不能看到它来决定查不查，
//! 和导航岛隐藏条目是同一个判断。

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::phantasi::phantasi_access_allowed;
use crate::extract::OptionalViewer;
use crate::services::library_items::{LibraryItem, normalize_library_item_for_client};

/// 关键词最长多少字符：每敲一下都是一次查询，超长串不该拖慢它。
const QUERY_MAX_CHARS: usize = 100;
/// 每组最多几条：面板里放得下，再多就该去博客的搜索页。
const GROUP_LIMIT: usize = 8;

#[derive(Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    q: String,
}

pub async fn site_search(
    State(db): State<DatabaseConnection>,
    viewer: OptionalViewer,
    Query(query): Query<SearchQuery>,
) -> Response {
    let keyword = keyword(&query.q);
    let mut posts = Vec::new();
    let mut library = Vec::new();
    if !keyword.is_empty() {
        let user_id = viewer
            .claims
            .as_ref()
            .and_then(|claims| claims.subject().map(|subject| subject.id()));
        let visibility =
            match crate::services::module_visibility::cached_module_visibility_preferences(&db)
                .await
            {
                Ok(visibility) => visibility,
                Err(error) => return failed("load module visibility", &error),
            };
        let shown = |module: &str| {
            phantasi_access_allowed(
                visibility.module_visibility(module),
                user_id,
                viewer.is_admin,
            )
        };
        if shown("phantasi") {
            match crate::services::blog::search_posts(&db, &keyword, GROUP_LIMIT as u64).await {
                Ok(results) => posts = results.hits.iter().map(post_json).collect(),
                Err(error) => return failed("search notes", error),
            }
        }
        if shown("library") {
            // 资料库还没抓过或站长还没设好时，只是这一组没有结果。
            if let Ok((items, preferences)) = crate::api::profile::site_library(&db).await {
                library = items
                    .items()
                    .iter()
                    .filter(|item| preferences.source_enabled(&item.item_type, &item.platform))
                    .filter(|item| title_matches(&item.title, &keyword))
                    .take(GROUP_LIMIT)
                    .map(|item| library_json(item.clone()))
                    .collect();
            }
        }
    }
    let mut response =
        Json(json!({ "success": true, "posts": posts, "library": library })).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}

/// 去掉首尾空白、按字符截到上限。
fn keyword(raw: &str) -> String {
    raw.trim().chars().take(QUERY_MAX_CHARS).collect()
}

/// 资料库只比标题：子串匹配，英文不分大小写，中文一个字也算。
fn title_matches(title: &str, keyword: &str) -> bool {
    title.to_lowercase().contains(&keyword.to_lowercase())
}

fn post_json(hit: &crate::services::blog::SearchHit) -> Value {
    let post = &hit.post;
    json!({
        "title": post.title,
        "href": myriad_blog::post_path(&myriad_blog::post_slug(post)),
        "excerpt": hit.excerpt,
        "published_at": post.published_at,
        "category": post.category,
    })
}

fn library_json(item: LibraryItem) -> Value {
    let item = normalize_library_item_for_client(item);
    json!({
        "title": item.title,
        "item_type": item.item_type,
        "platform": item.platform,
        "cover": item.cover,
    })
}

fn failed(what: &str, error: &str) -> Response {
    tracing::warn!(%error, "site search: {what} failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "success": false, "message": "Search failed" })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keyword_is_trimmed_and_capped_by_characters() {
        assert_eq!(keyword("  搜索  "), "搜索");
        assert_eq!(keyword(&"字".repeat(150)).chars().count(), QUERY_MAX_CHARS);
    }

    #[test]
    fn library_titles_match_a_part_ignoring_case() {
        assert!(title_matches("Blue Archive", "archive"));
        assert!(title_matches("塞尔达传说", "塞"));
        assert!(!title_matches("Blue Archive", "zelda"));
    }

    /// Visitors search too, and `OptionalViewer` needs the optional auth layer
    /// to tell who they are: without it every search answers 500.
    #[test]
    fn the_route_is_open_to_visitors_through_optional_auth() {
        let src = include_str!("../router/authenticated.rs");
        let route = src
            .split("\"/api/search\"")
            .nth(1)
            .and_then(|rest| rest.split(".route(").next())
            .expect("the route");
        assert!(route.contains("optional_auth_middleware"));
        assert!(!route.contains("middleware::auth::auth_middleware"));
    }
}
