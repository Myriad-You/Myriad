//! Federation media MIME classification and attachment URL checks.
//! Uploads persist through the platform media service, not this module.

struct StoredMediaKind {
    attachment_type: &'static str,
    #[cfg(test)]
    extension: &'static str,
}

fn stored_media_kind(mime: &str) -> Option<StoredMediaKind> {
    Some(match mime {
        "image/jpeg" => StoredMediaKind {
            attachment_type: "Image",
            #[cfg(test)]
            extension: "jpg",
        },
        "image/png" => StoredMediaKind {
            attachment_type: "Image",
            #[cfg(test)]
            extension: "png",
        },
        "image/gif" => StoredMediaKind {
            attachment_type: "Image",
            #[cfg(test)]
            extension: "gif",
        },
        "image/webp" => StoredMediaKind {
            attachment_type: "Image",
            #[cfg(test)]
            extension: "webp",
        },
        "video/mp4" => StoredMediaKind {
            attachment_type: "Video",
            #[cfg(test)]
            extension: "mp4",
        },
        "video/webm" => StoredMediaKind {
            attachment_type: "Video",
            #[cfg(test)]
            extension: "webm",
        },
        "video/quicktime" => StoredMediaKind {
            attachment_type: "Video",
            #[cfg(test)]
            extension: "mov",
        },
        _ => return None,
    })
}

pub fn classify_media_mime(mime: &str) -> Option<&'static str> {
    stored_media_kind(mime).map(|kind| kind.attachment_type)
}

#[cfg(test)]
fn extension_for_mime(mime: &str) -> Option<&'static str> {
    stored_media_kind(mime).map(|kind| kind.extension)
}

/// Human-readable reason if `url` is not a local media URL of this site.
/// Returns `None` when the URL is acceptable.
pub(super) fn attachment_url_rejection_reason(base_url: &str, url: &str) -> Option<&'static str> {
    let url = url.trim();
    if url.is_empty() {
        return Some("Invalid attachment URL");
    }
    let base = base_url.trim_end_matches('/');
    let relative = url
        .strip_prefix(base)
        .or_else(|| url.starts_with('/').then_some(url))
        .unwrap_or(url);
    if let Some(rest) = relative.strip_prefix("/api/media/") {
        let id = rest.strip_suffix("/content").unwrap_or(rest);
        return if id.parse::<i32>().ok().is_some_and(|value| value > 0) {
            None
        } else {
            Some("Invalid attachment URL")
        };
    }
    let Some(path) = relative.strip_prefix("/media/assets/") else {
        return Some("Invalid attachment URL");
    };
    let (id, file) = match path.split_once('/') {
        Some(parts) => parts,
        None => return Some("Invalid attachment URL"),
    };
    if uuid::Uuid::parse_str(id).is_err()
        || file.is_empty()
        || file.contains("..")
        || file.contains('/')
        || !file
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

    fn validate_attachment_url(base_url: &str, url: &str) -> bool {
        attachment_url_rejection_reason(base_url, url).is_none()
    }

    #[test]
    fn validate_attachment_url_requires_local_media_path() {
        let base = "https://example.com";
        assert!(validate_attachment_url(
            base,
            "https://example.com/media/assets/11111111-1111-1111-1111-111111111111/abc.jpg"
        ));
        assert!(!validate_attachment_url(
            base,
            "https://evil.com/media/assets/11111111-1111-1111-1111-111111111111/abc.jpg"
        ));
        assert!(!validate_attachment_url(
            base,
            "https://example.com/media/assets/11111111-1111-1111-1111-111111111111/../x.jpg"
        ));
        assert!(!validate_attachment_url(base, ""));
        assert!(!validate_attachment_url(base, "   "));
        assert!(!validate_attachment_url(
            base,
            "https://example.com/media/assets/11111111-1111-1111-1111-111111111111/"
        ));
        assert!(!validate_attachment_url(
            base,
            "https://example.com/media/assets/11111111-1111-1111-1111-111111111111/bad name.jpg"
        ));
    }

    #[test]
    fn attachment_url_rejection_reason_is_specific() {
        let base = "https://example.com";
        assert_eq!(
            attachment_url_rejection_reason(base, ""),
            Some("Invalid attachment URL")
        );
        assert_eq!(
            attachment_url_rejection_reason(
                base,
                "https://evil.com/media/assets/11111111-1111-1111-1111-111111111111/abc.jpg"
            ),
            Some("Invalid attachment URL")
        );
        assert_eq!(
            attachment_url_rejection_reason(
                base,
                "https://example.com/media/assets/11111111-1111-1111-1111-111111111111/abc.jpg"
            ),
            None
        );
        assert_eq!(
            attachment_url_rejection_reason(
                base,
                "https://example.com/media/assets/11111111-1111-1111-1111-111111111111/shot.png"
            ),
            None
        );
        assert_eq!(
            attachment_url_rejection_reason(
                base,
                "/media/assets/11111111-1111-1111-1111-111111111111/shot.png"
            ),
            None
        );
        assert_eq!(
            attachment_url_rejection_reason(
                base,
                "https://evil.com/media/assets/11111111-1111-1111-1111-111111111111/shot.png"
            ),
            Some("Invalid attachment URL")
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
        assert!(
            attachment_url_rejection_reason(
                base,
                "https://myriad.example/media/assets/11111111-1111-1111-1111-111111111111/../etc"
            )
            .is_some()
        );
        assert!(
            attachment_url_rejection_reason(base, "https://myriad.example/media/assets/11111111-1111-1111-1111-111111111111/ok-file.jpg"
            )
            .is_none()
        );
        assert!(attachment_url_rejection_reason(base, "").is_some());
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
        assert!(
            attachment_url_rejection_reason(
                base,
                "https://myriad.example/media/assets/11111111-1111-1111-1111-111111111111/../x"
            )
            .is_some()
        );
        assert!(
            attachment_url_rejection_reason(
                base,
                "https://myriad.example/media/assets/11111111-1111-1111-1111-111111111111/ok.jpg"
            )
            .is_none()
        );
        assert!(attachment_url_rejection_reason(base, "").is_some());
    }

    #[test]
    fn stored_media_uses_mime_extension_not_filename() {
        let src = include_str!("media.rs");
        assert!(!src.contains(concat!("store_federation", "_media")));
        assert_eq!(extension_for_mime("image/jpeg"), Some("jpg"));
        assert_eq!(classify_media_mime("image/jpeg"), Some("Image"));
        assert!(stored_media_kind("application/pdf").is_none());
        assert!(!src.contains("unwrap_or(\"bin\")"));
    }
}
