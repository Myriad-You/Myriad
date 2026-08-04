//! Shared Brew HTTP helpers (auth extractors, OPML, error mapping).
//!
//! Kept as a real submodule so feeds / reading / comments do not need `include!`.

use axum::{http::StatusCode, Json};
use reqwest::Url;
use sea_orm::DatabaseConnection;
use serde_json::json;

use crate::error::HttpError;
use crate::middleware::auth::{
    authenticate_request, ensure_current_admin_on, verify_current_admin_from_headers,
};
use crate::models::entities::brew_sources;

pub(crate) fn brew_http_err(status: StatusCode, error: impl Into<String>) -> HttpError {
    HttpError::from((
        status,
        Json(json!({ "success": false, "error": error.into() })),
    ))
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

/// 从请求头获取可选用户 ID（用于游客访问）
/// 游客返回 None，登录用户返回 Some(user_id)
///
/// Crypto-only: soft optional paths; revoked tokens may still appear signed.
/// Prefer `authenticate_request` when a DB handle is available.
pub(crate) fn get_optional_user_id_from_headers(headers: &axum::http::HeaderMap) -> Option<i32> {
    crate::middleware::auth::verify_jwt_token(headers)
        .ok()
        .and_then(|claims| claims.sub.parse::<i32>().ok())
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

pub(crate) struct OpmlFeed {
    pub title: String,
    pub url: String,
    pub category: Option<String>,
}

/// 解析 OPML 文件
pub(crate) fn parse_opml(opml: &str) -> Vec<OpmlFeed> {
    let mut feeds = Vec::new();

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
                    category: None,
                });
            }
        }
    }

    let re2 = regex::Regex::new(
        r#"<outline[^>]*xmlUrl=["']([^"']+)["'][^>]*text=["']([^"']+)["'][^>]*/?"#,
    )
    .ok();

    if let Some(re) = re2 {
        for caps in re.captures_iter(opml) {
            if let (Some(url), Some(title)) = (caps.get(1), caps.get(2)) {
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
    <title>Myriad Brew 订阅导出</title>
  </head>
  <body>
"#,
    );

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
