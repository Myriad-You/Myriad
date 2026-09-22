//! Shared Phantasi HTTP helpers (auth extractors, OPML, error mapping).
//!
//! Kept as a real submodule so feeds / reading / comments do not need `include!`.

use axum::{Json, http::StatusCode};
use myriad_error::AppError;
use reqwest::Url;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, DatabaseConnection};
use serde_json::json;

use crate::error::HttpError;
use crate::middleware::auth::{
    authenticate_optional_request, authenticate_request, ensure_current_admin_on,
    verify_current_admin_from_headers,
};
use crate::models::entities::phantasi_sources;
use crate::services::icon_service::IconService;

pub(crate) fn phantasi_http_err(status: StatusCode, error: impl Into<String>) -> HttpError {
    HttpError::from((status, Json(AppError::fail_json(error))))
}

pub(crate) fn phantasi_store_http(
    context: &'static str,
    error: impl std::fmt::Display,
) -> HttpError {
    tracing::error!(%error, context, "phantasi store failed");
    phantasi_http_err(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Failed to {context}"),
    )
}

/// 从请求头获取用户 ID（含 session epoch 校验）
pub(crate) async fn get_user_id_from_headers(
    headers: &axum::http::HeaderMap,
    db: &DatabaseConnection,
) -> Result<i32, HttpError> {
    match authenticate_request(headers, db).await {
        Ok(claims) => claims
            .sub
            .parse::<i32>()
            .map_err(|_| phantasi_http_err(StatusCode::UNAUTHORIZED, "Invalid user ID")),
        Err(_) => Err(phantasi_http_err(StatusCode::UNAUTHORIZED, "Unauthorized")),
    }
}

/// 无凭据返回 None；提供凭据时必须通过当前会话校验。
pub(crate) async fn get_optional_user_id_from_headers(
    headers: &axum::http::HeaderMap,
    db: &DatabaseConnection,
) -> Result<Option<i32>, HttpError> {
    authenticate_optional_request(headers, db)
        .await
        .map_err(|response| HttpError::from(response.status()))?
        .map(|claims| {
            claims
                .sub
                .parse::<i32>()
                .map_err(|_| phantasi_http_err(StatusCode::UNAUTHORIZED, "Invalid user ID"))
        })
        .transpose()
}

/// 检查请求头中的用户是否为管理员
/// 返回 (Option<user_id>, is_admin)
pub(crate) async fn get_user_and_admin_status(
    headers: &axum::http::HeaderMap,
    db: &DatabaseConnection,
) -> (Option<i32>, bool) {
    match authenticate_request(headers, db).await {
        Ok(claims) => {
            let user_id = claims.sub.parse::<i32>().ok().filter(|id| *id != 0);
            let is_admin = ensure_current_admin_on(&claims, db).await.is_ok();
            (user_id, is_admin)
        }
        Err(_) => (None, false),
    }
}

/// 公开读：无凭据当游客；带了凭据就必须是当前有效会话。
pub(crate) async fn get_optional_user_and_admin_status(
    headers: &axum::http::HeaderMap,
    db: &DatabaseConnection,
) -> Result<(Option<i32>, bool), HttpError> {
    let Some(user_id) = get_optional_user_id_from_headers(headers, db).await? else {
        return Ok((None, false));
    };
    let (_, is_admin) = get_user_and_admin_status(headers, db).await;
    Ok((Some(user_id), is_admin))
}

fn phantasi_access_allowed(level: &str, user_id: Option<i32>, is_admin: bool) -> bool {
    match level {
        "all" => true,
        "authenticated" => user_id.is_some(),
        "admin" => is_admin,
        _ => false,
    }
}

pub(crate) async fn require_phantasi_module_access(
    db: &DatabaseConnection,
    user_id: Option<i32>,
    is_admin: bool,
) -> Result<(), HttpError> {
    let preferences = myriad_module_visibility::try_load_module_visibility_preferences(db)
        .await
        .map_err(|error| phantasi_store_http("load module visibility", error))?;
    if phantasi_access_allowed(preferences.module_visibility("phantasi"), user_id, is_admin) {
        return Ok(());
    }
    Err(phantasi_http_err(StatusCode::NOT_FOUND, "Not found"))
}

/// Public Journal boundary: invalid credentials are rejected and the complete
/// all/authenticated/admin visibility matrix is enforced before data access.
pub(crate) async fn get_phantasi_viewer(
    headers: &axum::http::HeaderMap,
    db: &DatabaseConnection,
) -> Result<(Option<i32>, bool), HttpError> {
    let viewer = get_optional_user_and_admin_status(headers, db).await?;
    require_phantasi_module_access(db, viewer.0, viewer.1).await?;
    Ok(viewer)
}

pub(crate) async fn get_phantasi_user_and_admin_status(
    headers: &axum::http::HeaderMap,
    db: &DatabaseConnection,
) -> Result<(i32, bool), HttpError> {
    let user_id = get_user_id_from_headers(headers, db).await?;
    let (_, is_admin) = get_user_and_admin_status(headers, db).await;
    require_phantasi_module_access(db, Some(user_id), is_admin).await?;
    Ok((user_id, is_admin))
}

/// 从请求头获取管理员用户 ID（用于管理功能）
/// 非管理员返回 403 Forbidden
pub(crate) async fn get_admin_user_id_from_headers(
    headers: &axum::http::HeaderMap,
    db: &DatabaseConnection,
) -> Result<i32, HttpError> {
    let claims = verify_current_admin_from_headers(headers, db)
        .await
        .map_err(|(status, body)| HttpError::from((status, body)))?;

    claims
        .sub
        .parse::<i32>()
        .map_err(|_| phantasi_http_err(StatusCode::UNAUTHORIZED, "Invalid user ID"))
}

pub(crate) const OPML_UNCATEGORIZED: &str = "Uncategorized";
const OPML_UNCATEGORIZED_LEFTOVER: &str = "未分类";

pub(crate) struct OpmlFeed {
    pub title: String,
    pub url: String,
    pub category: Option<String>,
    pub site_url: Option<String>,
}

enum OpmlFrame {
    Folder(Option<String>),
    Feed,
}

fn unescape_xml(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn outline_attr(tag: &str, key: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{}=", key.to_ascii_lowercase());
    let pos = lower.find(&needle)?;
    let rest = tag[pos + needle.len()..].trim_start();
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let end = rest[1..].find(quote)?;
    Some(unescape_xml(&rest[1..1 + end]))
}

fn normalized_opml_category(name: Option<String>) -> Option<String> {
    name.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty()
            || trimmed == OPML_UNCATEGORIZED
            || trimmed == OPML_UNCATEGORIZED_LEFTOVER
        {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn current_opml_category(stack: &[OpmlFrame]) -> Option<String> {
    stack.iter().rev().find_map(|frame| match frame {
        OpmlFrame::Folder(name) => name.clone(),
        OpmlFrame::Feed => None,
    })
}

/// 解析 OPML：认嵌套分类文件夹、属性顺序、htmlUrl。
pub(crate) fn parse_opml(opml: &str) -> Vec<OpmlFeed> {
    let Ok(tag_re) = regex::Regex::new(r"(?i)</?outline\b[^>]*>") else {
        return Vec::new();
    };

    let mut feeds = Vec::new();
    let mut stack: Vec<OpmlFrame> = Vec::new();

    for tag_match in tag_re.find_iter(opml) {
        let tag = tag_match.as_str();
        if tag.as_bytes().get(1) == Some(&b'/') {
            stack.pop();
            continue;
        }

        let self_closing = tag.trim_end().ends_with("/>");
        let xml_url = outline_attr(tag, "xmlUrl").filter(|url| !url.trim().is_empty());
        let title = outline_attr(tag, "text")
            .or_else(|| outline_attr(tag, "title"))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let site_url = outline_attr(tag, "htmlUrl").filter(|url| !url.trim().is_empty());

        if let Some(url) = xml_url {
            let category = current_opml_category(&stack)
                .or_else(|| normalized_opml_category(outline_attr(tag, "category")));
            feeds.push(OpmlFeed {
                title: title.clone().unwrap_or_else(|| url.clone()),
                url,
                category,
                site_url,
            });
            if !self_closing {
                stack.push(OpmlFrame::Feed);
            }
            continue;
        }

        if !self_closing {
            stack.push(OpmlFrame::Folder(normalized_opml_category(title)));
        }
    }

    feeds
}

pub(crate) fn parse_feed_type_label(label: &str) -> phantasi_sources::FeedType {
    match label.trim().to_ascii_lowercase().as_str() {
        "notion" => phantasi_sources::FeedType::Notion,
        "atom" => phantasi_sources::FeedType::Atom,
        "json" | "json_feed" => phantasi_sources::FeedType::JsonFeed,
        "rsshub" => phantasi_sources::FeedType::RssHub,
        _ => phantasi_sources::FeedType::Rss,
    }
}

/// 请求里的 `rss` 是添加表单默认值，不能盖掉解析结果。
/// 只有明确的 atom / json / json_feed / rsshub / notion 才覆盖。
pub(crate) fn overlay_requested_feed_type(
    parsed: phantasi_sources::FeedType,
    requested: Option<&str>,
    source_type: phantasi_sources::SourceType,
) -> phantasi_sources::FeedType {
    if source_type == phantasi_sources::SourceType::Link {
        return parsed;
    }
    match requested.map(|label| label.trim().to_ascii_lowercase()) {
        Some(label)
            if matches!(
                label.as_str(),
                "notion" | "atom" | "json" | "json_feed" | "rsshub"
            ) =>
        {
            parse_feed_type_label(&label)
        }
        _ => parsed,
    }
}

pub(crate) fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// 生成 OPML 文件
pub(crate) fn generate_opml(sources: &[phantasi_sources::Model]) -> String {
    let mut opml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
  <head>
    <title>Myriad Phantasi subscriptions</title>
  </head>
  <body>
"#,
    );

    let mut by_category: std::collections::BTreeMap<String, Vec<&phantasi_sources::Model>> =
        std::collections::BTreeMap::new();

    for source in sources {
        let cat = source
            .category
            .clone()
            .unwrap_or_else(|| OPML_UNCATEGORIZED.to_string());
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

pub(crate) fn normalize_http_url(raw: &str) -> Result<Url, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("URL is required".to_string());
    }

    let normalized = if Url::parse(trimmed).is_ok() {
        trimmed.to_string()
    } else if !trimmed.contains("://") {
        format!("https://{trimmed}")
    } else {
        return Err("Invalid URL".to_string());
    };

    let parsed = Url::parse(&normalized).map_err(|_| "Invalid URL".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only HTTP and HTTPS URLs are supported".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("URL must not include credentials".to_string());
    }
    let mut parsed = parsed;
    parsed.set_fragment(None);
    Ok(parsed)
}

pub(crate) fn url_match_key(url: &str) -> String {
    let Ok(mut parsed) = Url::parse(url) else {
        return url.trim_end_matches('/').to_ascii_lowercase();
    };
    if let Some(host) = parsed.host_str().map(|host| host.to_ascii_lowercase()) {
        let _ = parsed.set_host(Some(&host));
    }
    parsed.set_fragment(None);
    let mut key = parsed.to_string();
    while key.ends_with('/') {
        key.pop();
    }
    key
}

pub(crate) fn build_feed_discovery_candidates(raw_url: &str) -> Result<Vec<String>, String> {
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

pub(crate) async fn persist_source_icon(source_id: i32, icon: &str) -> Option<String> {
    match IconService::new().download_icon(source_id, icon).await {
        Ok(Some(info)) => Some(info.local_path),
        Ok(None) => None,
        Err(error) => {
            tracing::warn!(%error, source_id, "Failed to persist source icon");
            None
        }
    }
}

/// Rewrite leftover `data:` icons onto disk so list/detail JSON stays path-sized.
pub(crate) async fn materialize_source_icon(
    db: &DatabaseConnection,
    source_id: i32,
    icon: Option<String>,
) -> Option<String> {
    let icon = icon.filter(|value| !value.trim().is_empty())?;
    if let Some(public) = phantasi_sources::public_icon(Some(&icon)) {
        return Some(public);
    }
    let path = persist_source_icon(source_id, &icon).await?;
    let active = phantasi_sources::ActiveModel {
        id: Set(source_id),
        icon: Set(Some(path.clone())),
        ..Default::default()
    };
    if let Err(error) = active.update(db).await {
        tracing::warn!(%error, source_id, "Failed to rewrite persisted source icon");
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn normalize_http_url_fills_https_and_strips_fragment() {
        let url = normalize_http_url("Example.COM/blog#top").expect("url");
        assert_eq!(url.host_str(), Some("example.com"));
        assert_eq!(url.path(), "/blog");
        assert_eq!(url.scheme(), "https");
        assert!(url.fragment().is_none());
        assert!(normalize_http_url("javascript:alert(1)").is_err());
        assert!(normalize_http_url("https://user:pass@example.com").is_err());
        assert_eq!(
            url_match_key("https://Example.COM/blog/"),
            "https://example.com/blog"
        );
        assert_eq!(
            url_match_key("https://example.com"),
            url_match_key("https://example.com/")
        );
    }

    #[test]
    fn module_access_uses_the_complete_viewer_matrix() {
        assert!(phantasi_access_allowed("all", None, false));
        assert!(!phantasi_access_allowed("authenticated", None, false));
        assert!(phantasi_access_allowed("authenticated", Some(2), false));
        assert!(!phantasi_access_allowed("admin", Some(2), false));
        assert!(phantasi_access_allowed("admin", Some(1), true));
        assert!(!phantasi_access_allowed("invalid", Some(1), true));
    }

    fn sample_source(
        name: &str,
        url: &str,
        category: Option<&str>,
        site_url: Option<&str>,
    ) -> phantasi_sources::Model {
        let now = Utc::now().into();
        phantasi_sources::Model {
            id: 1,
            user_id: 1,
            name: name.to_string(),
            url: url.to_string(),
            feed_type: phantasi_sources::FeedType::Rss,
            source_type: phantasi_sources::SourceType::Rss,
            category: category.map(str::to_string),
            icon: None,
            description: None,
            site_url: site_url.map(str::to_string),
            update_interval: 30,
            last_fetched_at: None,
            last_success_at: None,
            last_error: None,
            error_count: 0,
            enabled: true,
            item_count: 0,
            card_size: None,
            theme_color: None,
            sort_order: None,
            ai_style_tags: None,
            extra_config: None,
            rsshub_route: None,
            admin_only: false,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn parse_opml_reads_nested_category_and_html_url() {
        let opml = generate_opml(&[sample_source(
            "Example & Co",
            "https://example.com/feed.xml",
            Some("科技"),
            Some("https://example.com"),
        )]);
        let feeds = parse_opml(&opml);
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].title, "Example & Co");
        assert_eq!(feeds[0].url, "https://example.com/feed.xml");
        assert_eq!(feeds[0].category.as_deref(), Some("科技"));
        assert_eq!(feeds[0].site_url.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn parse_opml_maps_uncategorized_folder_to_none() {
        let opml = generate_opml(&[sample_source(
            "Plain",
            "https://plain.example/rss",
            None,
            None,
        )]);
        let feeds = parse_opml(&opml);
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].category, None);
        assert!(opml.contains("Uncategorized"));
        assert!(opml.contains("Myriad Phantasi subscriptions"));
    }

    #[test]
    fn parse_opml_maps_leftover_chinese_uncategorized_folder_to_none() {
        let opml = r#"<outline text="未分类"><outline text="Plain" xmlUrl="https://plain.example/rss"/></outline>"#;
        let feeds = parse_opml(opml);
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].category, None);
    }

    #[test]
    fn parse_opml_reads_inline_category_attribute() {
        let opml = r#"<outline text="Alpha" xmlUrl="https://a.example/rss" category="科技"/>"#;
        let feeds = parse_opml(opml);
        assert_eq!(feeds[0].category.as_deref(), Some("科技"));
    }

    #[test]
    fn parse_opml_accepts_xmlurl_before_text() {
        let opml =
            r#"<outline xmlUrl="https://a.example/rss" text="Alpha" htmlUrl="https://a.example"/>"#;
        let feeds = parse_opml(opml);
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].title, "Alpha");
        assert_eq!(feeds[0].url, "https://a.example/rss");
        assert_eq!(feeds[0].site_url.as_deref(), Some("https://a.example"));
    }

    #[test]
    fn parse_feed_type_label_covers_live_aliases() {
        assert_eq!(
            parse_feed_type_label("rsshub"),
            phantasi_sources::FeedType::RssHub
        );
        assert_eq!(
            parse_feed_type_label("json_feed"),
            phantasi_sources::FeedType::JsonFeed
        );
        assert_eq!(
            parse_feed_type_label("json"),
            phantasi_sources::FeedType::JsonFeed
        );
    }

    #[test]
    fn overlay_feed_type_keeps_parsed_atom_when_request_is_default_rss() {
        let parsed = phantasi_sources::FeedType::Atom;
        assert_eq!(
            overlay_requested_feed_type(
                parsed.clone(),
                Some("rss"),
                phantasi_sources::SourceType::Rss
            ),
            phantasi_sources::FeedType::Atom
        );
        assert_eq!(
            overlay_requested_feed_type(parsed, None, phantasi_sources::SourceType::Rss),
            phantasi_sources::FeedType::Atom
        );
    }

    #[test]
    fn overlay_feed_type_honors_explicit_rsshub_and_notion() {
        assert_eq!(
            overlay_requested_feed_type(
                phantasi_sources::FeedType::Rss,
                Some("rsshub"),
                phantasi_sources::SourceType::Rss
            ),
            phantasi_sources::FeedType::RssHub
        );
        assert_eq!(
            overlay_requested_feed_type(
                phantasi_sources::FeedType::Rss,
                Some("notion"),
                phantasi_sources::SourceType::Rss
            ),
            phantasi_sources::FeedType::Notion
        );
    }

    fn impl_fn<'a>(src: &'a str, name: &str) -> &'a str {
        let start = src
            .find(&format!("pub(crate) async fn {name}"))
            .expect(name);
        let body = &src[start..];
        let end = body[1..]
            .find("\npub(crate) async fn ")
            .map(|index| index + 1)
            .unwrap_or(body.len());
        &body[..end]
    }

    #[test]
    fn public_reads_use_the_complete_module_access_guard() {
        for (src, name) in [
            (include_str!("reading_item.rs"), "get_item"),
            (include_str!("reading_stats.rs"), "get_stats"),
            (include_str!("feeds_sources.rs"), "list_sources"),
            (include_str!("feeds_list.rs"), "list_items"),
            (include_str!("feeds_opml.rs"), "export_opml"),
            (include_str!("feeds_sources.rs"), "list_categories"),
            (include_str!("feeds_list.rs"), "list_subscription_topics"),
            (include_str!("comments.rs"), "list_comments"),
            (include_str!("comments.rs"), "list_comment_replies"),
        ] {
            let body = impl_fn(src, name);
            assert!(
                body.contains("get_phantasi_viewer"),
                "{name} must enforce all/authenticated/admin before data access"
            );
        }
        assert!(
            impl_fn(include_str!("applications.rs"), "create_application")
                .contains("get_phantasi_viewer")
        );
    }

    #[test]
    fn user_writes_use_the_complete_module_access_guard() {
        for (src, name) in [
            (include_str!("reading_mark.rs"), "update_item_state"),
            (include_str!("reading_mark.rs"), "mark_all_read"),
            (include_str!("reading_sync.rs"), "sync_states"),
            (include_str!("comments.rs"), "create_comment"),
            (include_str!("comments.rs"), "update_comment"),
            (include_str!("comments.rs"), "delete_comment"),
        ] {
            assert!(
                impl_fn(src, name).contains("get_phantasi_user_and_admin_status"),
                "{name} must enforce module visibility before writing"
            );
        }
    }

    #[test]
    fn shared_catalog_mutations_are_not_keyed_by_creator() {
        for name in [
            "add_source",
            "update_source",
            "delete_source",
            "refresh_source",
            "update_category",
            "delete_category",
        ] {
            let body = impl_fn(include_str!("feeds_sources.rs"), name);
            assert!(
                !body.contains("UserId.eq(user_id)"),
                "{name} must look up the shared catalog, not the creating admin"
            );
        }
        let import_opml = impl_fn(include_str!("feeds_opml.rs"), "import_opml");
        assert!(
            !import_opml.contains("UserId.eq(user_id)"),
            "import_opml must look up the shared catalog, not the creating admin"
        );
        let subscribe = include_str!("../../services/agent/executor/handlers/data_write.rs");
        let start = subscribe
            .find("async fn execute_phantasi_subscribe")
            .expect("subscribe");
        let body = &subscribe[start..];
        let end = body[1..]
            .find("\nasync fn ")
            .map(|index| index + 1)
            .unwrap_or(body.len());
        let subscribe = &body[..end];
        assert!(subscribe.contains("Url.eq"));
        assert!(
            !subscribe.contains("UserId.eq(user_id)"),
            "Agent subscribe must dedup by URL across the shared catalog"
        );
    }

    #[test]
    fn list_categories_omits_creator_and_maps_response() {
        let body = impl_fn(include_str!("feeds_sources.rs"), "list_categories");
        assert!(body.contains("CategoryResponse"));
        assert!(!body.contains("\"categories\": cats }"));
    }

    #[test]
    fn list_items_only_returns_previews() {
        let body = impl_fn(include_str!("feeds_list.rs"), "list_items");
        assert!(body.contains("phantasi_items::preview_query(items_query)"));
        assert!(!body.contains("query.projection"));
        assert!(
            body.contains("phantasi_store_http(\"count articles\""),
            "list_items must not swallow COUNT failures"
        );
        assert!(
            body.contains("try_join!"),
            "list_items extras must fail the request, not unwrap_or_default"
        );
        assert!(
            !body.contains("unwrap_or_default()"),
            "list_items extras must not swallow store errors"
        );
    }

    #[test]
    fn import_opml_does_not_swallow_existing_url_lookup() {
        let body = impl_fn(include_str!("feeds_opml.rs"), "import_opml");
        assert!(body.contains("phantasi_store_http(\"find existing sources\""));
        assert!(!body.contains("unwrap_or_default"));
    }

    #[test]
    fn list_sources_unread_preview_do_not_swallow() {
        let body = impl_fn(include_str!("feeds_sources.rs"), "list_sources");
        assert!(body.contains("phantasi_store_http(\"source unread counts\""));
        assert!(body.contains("phantasi_store_http(\"source previews\""));
        assert!(
            body.contains("pulses query failed"),
            "pulse overlay may degrade; unread/preview must not"
        );
    }

    #[test]
    fn list_sources_catalog_skips_preview_overlay() {
        let src = include_str!("feeds_sources.rs");
        assert!(src.contains("is_catalog_view"));
        assert!(src.contains("view=catalog"));
        let body = impl_fn(src, "list_sources");
        assert!(
            body.contains("SourceType.is_not_in"),
            "feeds board must drop link/note rows in SQL before overlay"
        );
        assert!(
            body.contains("is_catalog_view(query.view.as_deref())"),
            "catalog view must return source rows before unread/preview/pulse SQL"
        );
        let retain = body
            .find("retain_listed_sources_for_category")
            .expect("category filter");
        let board = body
            .find("retain_listed_sources_for_board")
            .expect("board filter");
        let catalog = body
            .find("is_catalog_view(query.view.as_deref())")
            .expect("catalog view");
        let unread = body.find("source unread counts").expect("unread overlay");
        assert!(
            retain < catalog,
            "category filter must run before catalog return"
        );
        assert!(
            board < catalog,
            "board filter must run before catalog return"
        );
        assert!(
            retain < unread,
            "category filter must run before preview SQL"
        );
        assert!(board < unread, "board filter must run before preview SQL");
    }

    #[test]
    fn add_source_lookups_do_not_swallow() {
        let body = impl_fn(include_str!("feeds_sources.rs"), "add_source");
        assert!(body.contains("phantasi_store_http(\"find existing source\""));
        assert!(body.contains("phantasi_store_http(\"reload source\""));
        assert!(
            !body.contains("if let Ok(Some(_))"),
            "URL dedup must not continue subscribe after a store error"
        );
        assert!(
            !body.contains("unwrap_or(source)"),
            "reload after insert must not fall back to the stale row"
        );
    }

    #[test]
    fn get_item_source_store_is_not_404() {
        let body = impl_fn(include_str!("reading_item.rs"), "get_item");
        assert!(body.contains("phantasi_store_http(\"find article source\""));
        assert!(body.contains("phantasi_store_http(\"count article extras\""));
        assert!(
            !body.contains("if let Ok(Some(source))"),
            "source lookup must distinguish store errors from missing rows"
        );
        assert!(!body.contains("unwrap_or(0)"));
    }

    #[test]
    fn source_response_omits_creator() {
        let src = include_str!("../../models/entities/phantasi_sources.rs");
        let start = src
            .find("pub struct SourceResponse")
            .expect("SourceResponse");
        let body = &src[start..];
        let end = body.find("impl From").unwrap_or(body.len());
        assert!(
            !&body[..end].contains("user_id"),
            "public source JSON must not include the creating admin"
        );
    }
}
