//! Federation media upload, MIME classification, and attachment URL checks.

use axum::{http::StatusCode, Json};
use serde_json::json;
use std::path::{Path, PathBuf};

use super::types::MediaUploadResponse;
use crate::federation::types::get_base_url;

/// 保存联邦媒体附件，返回可被 AP attachment 引用的公开 URL。
pub async fn store_federation_media(
    user_id: i32,
    filename: &str,
    mime: &str,
    bytes: &[u8],
) -> Result<MediaUploadResponse, (StatusCode, Json<serde_json::Value>)> {
    let mime = mime
        .split(';')
        .next()
        .unwrap_or(mime)
        .trim()
        .to_ascii_lowercase();
    let attachment_type = classify_media_mime(&mime).ok_or_else(|| {
        (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Json(json!({
                "error": "Unsupported media type",
                "allowed": ["image/jpeg","image/png","image/gif","image/webp","video/mp4","video/webm","video/quicktime"]
            })),
        )
    })?;

    let max = if attachment_type == "Image" {
        crate::federation::limits::note_image_limit()
    } else {
        crate::federation::limits::note_video_limit()
    };
    if bytes.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Empty file"})),
        ));
    }
    if bytes.len() > max {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({
                "error": format!("File too large (max {} bytes for {})", max, attachment_type),
                "max_bytes": max,
            })),
        ));
    }

    let ext_raw = extension_for_mime(&mime)
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            Path::new(filename)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("bin")
                .to_ascii_lowercase()
        });
    // Sanitize extension
    let ext: String = ext_raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    let ext = if ext.is_empty() {
        "bin".to_string()
    } else {
        ext
    };

    let media_id = uuid::Uuid::new_v4();
    let stored_name = format!("{}.{}", media_id, ext);
    let dir = federation_media_dir(user_id);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| {
        tracing::error!("Failed to create media dir: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to store media"})),
        )
    })?;

    let path = dir.join(&stored_name);
    tokio::fs::write(&path, bytes).await.map_err(|e| {
        tracing::error!("Failed to write media file: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to store media"})),
        )
    })?;

    let base_url = get_base_url().await;
    let url = format!(
        "{}/media/federation/{}/{}",
        base_url.trim_end_matches('/'),
        user_id,
        stored_name
    );

    let safe_name = Path::new(filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("upload")
        .chars()
        .take(200)
        .collect::<String>();

    Ok(MediaUploadResponse {
        url,
        media_type: mime,
        name: safe_name,
        size: bytes.len() as u64,
        attachment_type: attachment_type.to_string(),
    })
}

pub fn federation_media_root() -> PathBuf {
    crate::services::data_paths::paths()
        .root
        .join("federation_media")
}

fn federation_media_dir(user_id: i32) -> PathBuf {
    federation_media_root().join(user_id.to_string())
}

pub(super) fn classify_media_mime(mime: &str) -> Option<&'static str> {
    match mime {
        "image/jpeg" | "image/png" | "image/gif" | "image/webp" => Some("Image"),
        "video/mp4" | "video/webm" | "video/quicktime" => Some("Video"),
        _ => None,
    }
}

fn extension_for_mime(mime: &str) -> Option<&'static str> {
    match mime {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "video/mp4" => Some("mp4"),
        "video/webm" => Some("webm"),
        "video/quicktime" => Some("mov"),
        _ => None,
    }
}

/// Human-readable reason if `url` is not a valid local federation media URL for this user.
/// Returns `None` when the URL is acceptable.
pub(super) fn attachment_url_rejection_reason(
    base_url: &str,
    user_id: i32,
    url: &str,
) -> Option<&'static str> {
    let url = url.trim();
    if url.is_empty() {
        return Some("Invalid attachment URL");
    }
    let base = base_url.trim_end_matches('/');
    let prefix = format!("{}/media/federation/{}/", base, user_id);
    if !url.starts_with(&prefix) {
        return Some("Invalid attachment URL");
    }
    let rest = &url[prefix.len()..];
    if rest.is_empty() {
        return Some("Invalid attachment URL");
    }
    if rest.contains("..") || rest.contains('/') {
        return Some("Invalid attachment URL");
    }
    if !rest
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return Some("Invalid attachment URL");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_media_mime_allows_image_and_video() {
        assert_eq!(classify_media_mime("image/jpeg"), Some("Image"));
        assert_eq!(classify_media_mime("video/mp4"), Some("Video"));
        assert_eq!(classify_media_mime("application/pdf"), None);
    }

    fn validate_attachment_url(base_url: &str, user_id: i32, url: &str) -> bool {
        attachment_url_rejection_reason(base_url, user_id, url).is_none()
    }

    #[test]
    fn validate_attachment_url_requires_local_media_path() {
        let base = "https://example.com";
        assert!(validate_attachment_url(
            base,
            1,
            "https://example.com/media/federation/1/abc.jpg"
        ));
        assert!(!validate_attachment_url(
            base,
            1,
            "https://evil.com/media/federation/1/abc.jpg"
        ));
        assert!(!validate_attachment_url(
            base,
            1,
            "https://example.com/media/federation/1/../2/x.jpg"
        ));
        assert!(!validate_attachment_url(
            base,
            2,
            "https://example.com/media/federation/1/abc.jpg"
        ));
        assert!(!validate_attachment_url(base, 1, ""));
        assert!(!validate_attachment_url(base, 1, "   "));
        assert!(!validate_attachment_url(
            base,
            1,
            "https://example.com/media/federation/1/"
        ));
        assert!(!validate_attachment_url(
            base,
            1,
            "https://example.com/media/federation/1/bad name.jpg"
        ));
    }

    #[test]
    fn attachment_url_rejection_reason_is_specific() {
        let base = "https://example.com";
        assert_eq!(
            attachment_url_rejection_reason(base, 1, ""),
            Some("Invalid attachment URL")
        );
        assert_eq!(
            attachment_url_rejection_reason(base, 1, "https://evil.com/media/federation/1/abc.jpg"),
            Some("Invalid attachment URL")
        );
        assert_eq!(
            attachment_url_rejection_reason(
                base,
                1,
                "https://example.com/media/federation/1/abc.jpg"
            ),
            None
        );
    }

    #[test]
    fn classify_media_mime_rejects_unknown() {
        assert_eq!(classify_media_mime("application/pdf"), None);
        assert_eq!(classify_media_mime("text/plain"), None);
        assert_eq!(classify_media_mime("image/svg+xml"), None);
        assert_eq!(classify_media_mime("image/jpeg"), Some("Image"));
        assert_eq!(classify_media_mime("video/mp4"), Some("Video"));
    }

    #[test]
    fn extension_for_mime_maps_known_types() {
        assert_eq!(extension_for_mime("image/jpeg"), Some("jpg"));
        assert_eq!(extension_for_mime("image/png"), Some("png"));
        assert_eq!(extension_for_mime("video/quicktime"), Some("mov"));
        assert_eq!(extension_for_mime("application/octet-stream"), None);
    }

    #[test]
    fn attachment_url_rejects_path_traversal() {
        let base = "https://myriad.example";
        assert!(attachment_url_rejection_reason(
            base,
            1,
            "https://myriad.example/media/federation/1/../etc"
        )
        .is_some());
        assert!(attachment_url_rejection_reason(
            base,
            1,
            "https://myriad.example/media/federation/1/ok-file.jpg"
        )
        .is_none());
        assert!(attachment_url_rejection_reason(base, 1, "").is_some());
    }

    #[test]
    fn w175_classify_media_mime_rejects_unknown() {
        assert_eq!(classify_media_mime("application/pdf"), None);
        assert_eq!(classify_media_mime("text/html"), None);
        assert_eq!(classify_media_mime("image/jpeg"), Some("Image"));
        assert_eq!(classify_media_mime("video/webm"), Some("Video"));
    }

    #[test]
    fn w175_extension_for_mime_maps() {
        assert_eq!(extension_for_mime("image/png"), Some("png"));
        assert_eq!(extension_for_mime("image/webp"), Some("webp"));
        assert_eq!(extension_for_mime("video/mp4"), Some("mp4"));
        assert_eq!(extension_for_mime("audio/mpeg"), None);
    }

    #[test]
    fn w175_attachment_url_rejects_traversal() {
        let base = "https://myriad.example";
        assert!(attachment_url_rejection_reason(
            base,
            7,
            "https://myriad.example/media/federation/7/../x"
        )
        .is_some());
        assert!(attachment_url_rejection_reason(
            base,
            7,
            "https://myriad.example/media/federation/7/ok.jpg"
        )
        .is_none());
        assert!(attachment_url_rejection_reason(base, 7, "").is_some());
    }
}
