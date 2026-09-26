//! Canonical media paths. Disk location is `storage_key`, never a request URL.

use uuid::Uuid;

use super::error::MediaError;
use super::validate::extension_for_mime;

pub fn storage_key(public_id: Uuid, ext: &str) -> Result<String, MediaError> {
    let ext = normalize_ext(ext)?;
    let id = public_id.to_string();
    let prefix = id
        .get(..2)
        .ok_or_else(|| MediaError::invalid("Invalid asset id"))?;
    Ok(format!("{prefix}/{id}.{ext}"))
}

pub fn content_path(id: i32) -> String {
    format!("/api/media/{id}/content")
}

pub fn public_path(public_id: Uuid, filename: &str) -> String {
    format!("/media/assets/{public_id}/{filename}")
}

pub fn staging_url(public_id: Uuid) -> String {
    format!("/media/assets/{public_id}/staging")
}

pub fn compatible_url(public_id: Uuid, filename: &str) -> String {
    public_path(public_id, filename)
}

pub fn display_filename(original: &str, ext: &str, public_id: Uuid) -> String {
    let ext = normalize_ext(ext).unwrap_or("bin");
    let stem = std::path::Path::new(original)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let cleaned: String = stem
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .take(80)
        .collect();
    if cleaned.is_empty() {
        format!("{public_id}.{ext}")
    } else {
        format!("{cleaned}.{ext}")
    }
}

/// Keep a known local pathname. Query strings, origins, and `..` are rejected.
pub fn registered_local_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = if let Some(rest) = trimmed.split_once("://").map(|(_, rest)| rest) {
        rest.find('/').map(|i| &rest[i..])?
    } else {
        trimmed
    };
    let path = path.split('?').next().unwrap_or(path);
    if path.contains("..") || path.contains('\\') || path.contains('\0') {
        return None;
    }
    if !(path.starts_with("/media/federation/")
        || path.starts_with("/media/assets/")
        || path.starts_with("/api/media/")
        || path.starts_with("/api/phantasi/image-cache/")
        || path.starts_with("/api/brew/image-cache/"))
    {
        return None;
    }
    Some(path.to_string())
}

/// Cite a local media path. Absolute URLs must match an allowlisted origin;
/// path-only values still go through [`registered_local_path`].
pub fn cite_local_path(raw: &str, allowed_origins: &[String]) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains("://") {
        alias_local_path(trimmed, allowed_origins)
    } else {
        registered_local_path(trimmed)
    }
}

/// Extract a local alias path only when `raw` is path-only or its origin is allowlisted.
/// Foreign origins are rejected even if the pathname looks like a local media URL.
pub fn alias_local_path(raw: &str, allowed_origins: &[String]) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains("://") {
        let origin = origin_of(trimmed)?;
        if !allowed_origins
            .iter()
            .any(|allowed| same_origin(allowed, &origin))
        {
            return None;
        }
    }
    registered_local_path(trimmed)
}

/// Pathname of an absolute http(s) URL shaped like platform-owned media, whatever
/// its origin. Mirrors the frontend `siteMediaUrl` rule, which reads such values
/// through the current API origin because they may carry a previous site domain.
/// The shape alone proves nothing: callers must confirm the path resolves to
/// media on this instance before treating the value as local.
pub fn media_shaped_path(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return None;
    }
    let path = parsed.path();
    let content_id = path
        .strip_prefix("/api/media/")
        .and_then(|rest| rest.strip_suffix("/content"))
        .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()));
    if !(path.starts_with("/media/assets/") || path.starts_with("/media/federation/") || content_id)
    {
        return None;
    }
    registered_local_path(path)
}

fn origin_of(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw).ok()?;
    if parsed.cannot_be_a_base() {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    match parsed.port() {
        Some(port) => Some(format!("{}://{}:{port}", parsed.scheme(), host)),
        None => Some(format!("{}://{}", parsed.scheme(), host)),
    }
}

fn same_origin(allowed: &str, origin: &str) -> bool {
    origin_of(allowed)
        .or_else(|| {
            let trimmed = allowed.trim().trim_end_matches('/');
            (!trimmed.is_empty()).then(|| trimmed.to_ascii_lowercase())
        })
        .is_some_and(|allowed| allowed == origin)
}

pub fn filename_for_mime(name: &str, mime: &str, public_id: Uuid) -> Result<String, MediaError> {
    let ext =
        extension_for_mime(mime).ok_or_else(|| MediaError::invalid("Unsupported media type"))?;
    Ok(display_filename(name, ext, public_id))
}

fn normalize_ext(ext: &str) -> Result<&str, MediaError> {
    match ext {
        "jpg" | "png" | "gif" | "webp" | "mp4" | "webm" | "mov"
        | "mp3" | "m4a" | "flac" | "wav" | "ogg" | "aac" => Ok(ext),
        _ => Err(MediaError::invalid("Unsupported media type")),
    }
}

pub fn parse_storage_key(key: &str) -> Result<(Uuid, &'static str), MediaError> {
    let (prefix, rest) = key
        .split_once('/')
        .ok_or_else(|| MediaError::invalid("Invalid storage key"))?;
    if prefix.len() != 2
        || !prefix.bytes().all(|b| b.is_ascii_hexdigit())
        || rest.contains('/')
        || rest.contains("..")
    {
        return Err(MediaError::invalid("Invalid storage key"));
    }
    let (id, ext) = rest
        .rsplit_once('.')
        .ok_or_else(|| MediaError::invalid("Invalid storage key"))?;
    let public_id = Uuid::parse_str(id).map_err(|_| MediaError::invalid("Invalid storage key"))?;
    if public_id.to_string().get(..2) != Some(prefix) {
        return Err(MediaError::invalid("Invalid storage key"));
    }
    let ext = match ext {
        "jpg" => "jpg",
        "png" => "png",
        "gif" => "gif",
        "webp" => "webp",
        "mp4" => "mp4",
        "webm" => "webm",
        "mov" => "mov",
        "mp3" => "mp3",
        "m4a" => "m4a",
        "flac" => "flac",
        "wav" => "wav",
        "ogg" => "ogg",
        "aac" => "aac",
        _ => return Err(MediaError::invalid("Invalid storage key")),
    };
    Ok((public_id, ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_key_stays_inside_media_root() {
        let id = Uuid::parse_str("3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708").unwrap();
        let key = storage_key(id, "png").unwrap();
        assert_eq!(key, "3f/3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708.png");
        assert!(parse_storage_key(&key).is_ok());
        assert!(parse_storage_key("../secret.png").is_err());
        assert!(parse_storage_key("3f/../etc/passwd").is_err());
        assert!(parse_storage_key("3f/3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708.png/extra").is_err());
    }

    #[test]
    fn registered_paths_do_not_follow_foreign_origins() {
        assert_eq!(
            registered_local_path("https://site.example/media/federation/1/a.jpg"),
            Some("/media/federation/1/a.jpg".into())
        );
        assert!(registered_local_path("https://other.site/media/federation/1/a.jpg").is_some());
        assert_eq!(
            registered_local_path("/media/federation/1/a.jpg?track=1"),
            Some("/media/federation/1/a.jpg".into())
        );
        assert!(registered_local_path("/media/federation/../secret").is_none());
        assert!(registered_local_path("/tmp/x.png").is_none());
        assert!(registered_local_path("").is_none());
    }

    #[test]
    fn alias_paths_require_an_allowlisted_origin() {
        let allowed = ["https://site.example".to_string()];
        assert_eq!(
            alias_local_path(
                "https://site.example/media/federation/1/a.jpg?utm=1",
                &allowed
            ),
            Some("/media/federation/1/a.jpg".into())
        );
        assert!(
            alias_local_path("https://other.site/media/federation/1/a.jpg", &allowed).is_none()
        );
        assert_eq!(
            alias_local_path("/media/federation/1/a.jpg", &allowed),
            Some("/media/federation/1/a.jpg".into())
        );
        assert!(alias_local_path("/media/federation/1/%2e%2e/secret", &[]).is_some());
        assert!(alias_local_path("/media/federation/../secret", &[]).is_none());
        assert_eq!(
            cite_local_path(
                "/media/assets/3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708/a.png",
                &[]
            ),
            Some("/media/assets/3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708/a.png".into())
        );
        assert!(
            cite_local_path(
                "https://other.site/media/assets/3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708/a.png",
                &allowed
            )
            .is_none()
        );
    }

    #[test]
    fn display_filename_ignores_client_path() {
        let id = Uuid::nil();
        assert_eq!(
            display_filename("../../etc/passwd", "png", id),
            "passwd.png"
        );
        assert_eq!(display_filename("Photo 1.PNG", "jpg", id), "Photo1.jpg");
    }

    #[test]
    fn media_shaped_paths_match_the_frontend_site_media_rule() {
        let asset = "/media/assets/3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708/a.png";
        assert_eq!(
            media_shaped_path(&format!("https://old.example{asset}?v=1#x")),
            Some(asset.into())
        );
        assert_eq!(
            media_shaped_path("http://old.example:8080/media/federation/1/a.jpg"),
            Some("/media/federation/1/a.jpg".into())
        );
        assert_eq!(
            media_shaped_path("https://old.example/api/media/42/content"),
            Some("/api/media/42/content".into())
        );
        // Path-only values are the trusted cite path's job, not this one.
        assert!(media_shaped_path(asset).is_none());
        for rejected in [
            "https://old.example/api/media/42",
            "https://old.example/api/media/42/content/extra",
            "https://old.example/api/media/x/content",
            "https://old.example/api/phantasi/image-cache/aa/b.png",
            "https://old.example/uploads/media/assets/a.png",
            "https://old.example/media/federation/../secret",
            "https://old.example/media/federation/%2e%2e/secret",
            "ftp://old.example/media/federation/1/a.jpg",
            "data:image/png;base64,AAAA",
        ] {
            assert!(media_shaped_path(rejected).is_none(), "{rejected}");
        }
    }
}
