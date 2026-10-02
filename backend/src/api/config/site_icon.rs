//! Public site favicon bytes for PWA canvas compose (CORS on the API origin).
//!
//! Fetches only the configured `site_favicon`, not a caller-supplied URL.

use std::time::Duration;

use axum::{
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

use super::secrets::sanitize_site_favicon_url;
use crate::services::icon_service::parse_icon_data_uri;

const MAX_ICON_BYTES: usize = 512 * 1024;
const MIN_ICON_BYTES: usize = 10;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SiteIconKind {
    Data {
        bytes: Vec<u8>,
        content_type: String,
    },
    Remote(String),
    Unavailable,
}

fn icon_ext_to_mime(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn infer_remote_content_type(url: &str, header: Option<&str>) -> String {
    if let Some(ct) = header {
        let mime = ct.split(';').next().unwrap_or("").trim();
        if mime.to_ascii_lowercase().starts_with("image/") {
            return mime.to_string();
        }
    }
    let lower = url.to_ascii_lowercase();
    if lower.contains(".png") {
        "image/png".to_string()
    } else if lower.contains(".webp") {
        "image/webp".to_string()
    } else if lower.contains(".svg") {
        "image/svg+xml".to_string()
    } else if lower.contains(".jpg") || lower.contains(".jpeg") {
        "image/jpeg".to_string()
    } else if lower.contains(".gif") {
        "image/gif".to_string()
    } else {
        "image/x-icon".to_string()
    }
}

pub(crate) fn classify_configured_favicon(raw: &str) -> SiteIconKind {
    let Some(sanitized) = sanitize_site_favicon_url(raw) else {
        return SiteIconKind::Unavailable;
    };
    let trimmed = sanitized.trim();
    if trimmed.is_empty() {
        return SiteIconKind::Unavailable;
    }
    if trimmed.starts_with("data:image/") {
        return match parse_icon_data_uri(trimmed) {
            Some((bytes, ext)) => SiteIconKind::Data {
                bytes,
                content_type: icon_ext_to_mime(ext).to_string(),
            },
            None => SiteIconKind::Unavailable,
        };
    }
    if trimmed.starts_with('/') {
        return SiteIconKind::Unavailable;
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return SiteIconKind::Remote(trimmed.to_string());
    }
    SiteIconKind::Unavailable
}

fn icon_response(bytes: Vec<u8>, content_type: String) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "public, max-age=300".to_string()),
            (
                header::HeaderName::from_static("cross-origin-resource-policy"),
                "cross-origin".to_string(),
            ),
        ],
        bytes,
    )
        .into_response()
}

fn not_found() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

pub async fn get_site_icon(crate::extract::Db(db): crate::extract::Db) -> Response {
    let config_service = crate::services::config_service::ConfigService::new(db);
    let stored = config_service.load_config().await.unwrap_or_else(|error| {
        tracing::warn!(%error, "stored configuration could not be read; using defaults");
        crate::config::DynamicConfig::default()
    });
    let raw = stored
        .site_favicon
        .clone()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/favicon.webp".to_string());

    match classify_configured_favicon(&raw) {
        SiteIconKind::Unavailable => not_found(),
        SiteIconKind::Data {
            bytes,
            content_type,
        } => icon_response(bytes, content_type),
        SiteIconKind::Remote(url) => match fetch_remote_site_icon(&url).await {
            Some((bytes, content_type)) => icon_response(bytes, content_type),
            None => not_found(),
        },
    }
}

async fn fetch_remote_site_icon(url: &str) -> Option<(Vec<u8>, String)> {
    let (target_url, client) = crate::services::outbound_security::build_public_http_client(
        url,
        Duration::from_secs(10),
        Some("Mozilla/5.0 (compatible; MyriadSiteIcon/1.0)"),
    )
    .await
    .ok()?;

    let response = client.get(target_url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let header_ct = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = crate::services::outbound_security::read_limited_body(response, MAX_ICON_BYTES)
        .await
        .ok()?;
    if bytes.len() < MIN_ICON_BYTES {
        return None;
    }
    Some((bytes, infer_remote_content_type(url, header_ct.as_deref())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_and_empty_are_left_to_the_frontend() {
        assert_eq!(
            classify_configured_favicon("/favicon.webp"),
            SiteIconKind::Unavailable
        );
        assert_eq!(classify_configured_favicon(""), SiteIconKind::Unavailable);
        assert_eq!(
            classify_configured_favicon("javascript:alert(1)"),
            SiteIconKind::Unavailable
        );
    }

    #[test]
    fn public_http_favicon_is_fetched_by_the_api() {
        assert_eq!(
            classify_configured_favicon(
                "https://api.fuukei.org/myriad/frontend/public/siteicon.ico"
            ),
            SiteIconKind::Remote(
                "https://api.fuukei.org/myriad/frontend/public/siteicon.ico".to_string()
            )
        );
    }

    #[test]
    fn uploaded_png_data_uri_is_served_as_bytes() {
        let png = concat!(
            "data:image/png;base64,",
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="
        );
        match classify_configured_favicon(png) {
            SiteIconKind::Data {
                bytes,
                content_type,
            } => {
                assert!(bytes.len() > 10);
                assert_eq!(content_type, "image/png");
            }
            other => panic!("expected data icon, got {other:?}"),
        }
    }

    #[test]
    fn infer_type_prefers_image_header_then_url() {
        assert_eq!(
            infer_remote_content_type("https://x.example/a.ico", Some("image/png; charset=binary")),
            "image/png"
        );
        assert_eq!(
            infer_remote_content_type("https://x.example/siteicon.ico", None),
            "image/x-icon"
        );
    }
}
