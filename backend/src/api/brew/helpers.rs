//! Shared Brew HTTP helpers (auth extractors, OPML, error mapping).
//!
//! Kept as a real submodule so feeds / reading / comments do not need `include!`.

use axum::{http::StatusCode, Json};
use myriad_error::AppError;
use reqwest::Url;
use sea_orm::DatabaseConnection;
use serde_json::json;

use crate::error::HttpError;
use crate::middleware::auth::{
    authenticate_optional_request, authenticate_request, ensure_current_admin_on,
    verify_current_admin_from_headers,
};
use crate::models::entities::brew_sources;

pub(crate) fn brew_http_err(status: StatusCode, error: impl Into<String>) -> HttpError {
    HttpError::from((status, Json(AppError::fail_json(error))))
}

pub(crate) fn brew_store_http(context: &'static str, error: impl std::fmt::Display) -> HttpError {
    tracing::error!(%error, context, "brew store failed");
    brew_http_err(
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
            .map_err(|_| brew_http_err(StatusCode::UNAUTHORIZED, "Invalid user ID")),
        Err(_) => Err(brew_http_err(StatusCode::UNAUTHORIZED, "Unauthorized")),
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
                .map_err(|_| brew_http_err(StatusCode::UNAUTHORIZED, "Invalid user ID"))
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
            let user_id = claims.sub.parse::<i32>().ok();
            let is_admin = ensure_current_admin_on(&claims, db).await.is_ok();
            (user_id, is_admin)
        }
        Err(_) => (None, false),
    }
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
        .map_err(|_| brew_http_err(StatusCode::UNAUTHORIZED, "Invalid user ID"))
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

pub(crate) fn parse_feed_type_label(label: &str) -> brew_sources::FeedType {
    match label.trim().to_ascii_lowercase().as_str() {
        "notion" => brew_sources::FeedType::Notion,
        "atom" => brew_sources::FeedType::Atom,
        "json" | "json_feed" => brew_sources::FeedType::JsonFeed,
        "rsshub" => brew_sources::FeedType::RssHub,
        _ => brew_sources::FeedType::Rss,
    }
}

/// 请求里的 `rss` 是添加表单默认值，不能盖掉解析结果。
/// 只有明确的 atom / json / json_feed / rsshub / notion 才覆盖。
pub(crate) fn overlay_requested_feed_type(
    parsed: brew_sources::FeedType,
    requested: Option<&str>,
    source_type: brew_sources::SourceType,
) -> brew_sources::FeedType {
    if source_type == brew_sources::SourceType::Link {
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
pub(crate) fn generate_opml(sources: &[brew_sources::Model]) -> String {
    let mut opml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
  <head>
    <title>Myriad Brew subscriptions</title>
  </head>
  <body>
"#,
    );

    let mut by_category: std::collections::BTreeMap<String, Vec<&brew_sources::Model>> =
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample_source(
        name: &str,
        url: &str,
        category: Option<&str>,
        site_url: Option<&str>,
    ) -> brew_sources::Model {
        let now = Utc::now().into();
        brew_sources::Model {
            id: 1,
            user_id: 1,
            name: name.to_string(),
            url: url.to_string(),
            feed_type: brew_sources::FeedType::Rss,
            source_type: brew_sources::SourceType::Rss,
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
            unread_count: 0,
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
        assert!(opml.contains("Myriad Brew subscriptions"));
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
            brew_sources::FeedType::RssHub
        );
        assert_eq!(
            parse_feed_type_label("json_feed"),
            brew_sources::FeedType::JsonFeed
        );
        assert_eq!(
            parse_feed_type_label("json"),
            brew_sources::FeedType::JsonFeed
        );
    }

    #[test]
    fn overlay_feed_type_keeps_parsed_atom_when_request_is_default_rss() {
        let parsed = brew_sources::FeedType::Atom;
        assert_eq!(
            overlay_requested_feed_type(parsed.clone(), Some("rss"), brew_sources::SourceType::Rss),
            brew_sources::FeedType::Atom
        );
        assert_eq!(
            overlay_requested_feed_type(parsed, None, brew_sources::SourceType::Rss),
            brew_sources::FeedType::Atom
        );
    }

    #[test]
    fn overlay_feed_type_honors_explicit_rsshub_and_notion() {
        assert_eq!(
            overlay_requested_feed_type(
                brew_sources::FeedType::Rss,
                Some("rsshub"),
                brew_sources::SourceType::Rss
            ),
            brew_sources::FeedType::RssHub
        );
        assert_eq!(
            overlay_requested_feed_type(
                brew_sources::FeedType::Rss,
                Some("notion"),
                brew_sources::SourceType::Rss
            ),
            brew_sources::FeedType::Notion
        );
    }
}
