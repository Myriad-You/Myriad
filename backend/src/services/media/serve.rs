//! Public media read decisions. Disk paths come from storage_key or the image
//! cache's own layout, never from concatenating the request URL.

use std::path::PathBuf;

use sea_orm::ConnectionTrait;
use uuid::Uuid;

use crate::models::entities::media_assets;

use super::access::can_read;
use super::assets;
use super::cache_import::cached_public_id;
use super::error::MediaError;
use super::store::MediaStore;
use super::types::{MediaActor, MediaExposure, MediaState};
use super::urls::filename_for_mime;
use super::validate::extension_for_mime;

pub const PUBLIC_CACHE_CONTROL: &str = "public, max-age=300, must-revalidate";
pub const CACHE_FALLBACK_CONTROL: &str = "public, max-age=604800, immutable";
pub const NO_STORE: &str = "private, no-store";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileServe {
    pub path: PathBuf,
    pub mime: String,
    pub etag: Option<String>,
    pub cache_control: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServeOutcome {
    File(FileServe),
    NotFound { no_store: bool },
}

pub fn public_filename_ok(name: &str, mime: &str, public_id: Uuid, filename: &str) -> bool {
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return false;
    }
    let Ok(display) = filename_for_mime(name, mime, public_id) else {
        return false;
    };
    let Some(ext) = extension_for_mime(mime) else {
        return false;
    };
    filename == display || filename == format!("{public_id}.{ext}")
}

pub async fn resolve_authenticated_content(
    db: &impl ConnectionTrait,
    store: &MediaStore,
    id: i32,
    actor: &MediaActor,
) -> Result<ServeOutcome, MediaError> {
    let Some(row) = assets::find_by_id(db, id).await? else {
        return Ok(ServeOutcome::NotFound { no_store: true });
    };
    let asset = match assets::to_domain(row.clone(), 0) {
        Ok(asset) => asset,
        Err(_) => return Ok(ServeOutcome::NotFound { no_store: true }),
    };
    if !can_read(actor, &asset) {
        return Ok(ServeOutcome::NotFound { no_store: true });
    }
    file_from_row(store, &row, NO_STORE)
}

pub async fn resolve_public_asset(
    db: &impl ConnectionTrait,
    store: &MediaStore,
    public_id: Uuid,
    filename: &str,
) -> Result<ServeOutcome, MediaError> {
    let Some(row) = assets::find_by_public_id(db, public_id).await? else {
        return Ok(ServeOutcome::NotFound { no_store: true });
    };
    if !ready_public(&row) || !public_filename_ok(&row.name, &row.mime, public_id, filename) {
        return Ok(ServeOutcome::NotFound { no_store: true });
    }
    file_from_row(store, &row, PUBLIC_CACHE_CONTROL)
}

/// The permanent address of a private asset, for a reader who is signed in.
/// Public assets never reach this: [`resolve_public_asset`] serves them first.
pub async fn resolve_private_asset(
    db: &impl ConnectionTrait,
    store: &MediaStore,
    public_id: Uuid,
    filename: &str,
    actor: &MediaActor,
) -> Result<ServeOutcome, MediaError> {
    let Some(row) = assets::find_by_public_id(db, public_id).await? else {
        return Ok(ServeOutcome::NotFound { no_store: true });
    };
    if !public_filename_ok(&row.name, &row.mime, public_id, filename) {
        return Ok(ServeOutcome::NotFound { no_store: true });
    }
    let Ok(asset) = assets::to_domain(row.clone(), 0) else {
        return Ok(ServeOutcome::NotFound { no_store: true });
    };
    if !can_read(actor, &asset) {
        return Ok(ServeOutcome::NotFound { no_store: true });
    }
    file_from_row(store, &row, NO_STORE)
}

/// An image-cache path. Once site content cited it, the cached file was
/// imported as an asset: that asset answers, and a deleted or private one is
/// never served from the cache instead. Otherwise the cached file itself.
pub async fn resolve_cached_image(
    db: &impl ConnectionTrait,
    store: &MediaStore,
    local_path: &str,
) -> Result<ServeOutcome, MediaError> {
    let Some(public_id) = cached_public_id(local_path) else {
        return Ok(ServeOutcome::NotFound { no_store: true });
    };
    if let Some(row) = assets::find_by_public_id(db, public_id).await? {
        if !ready_public(&row) {
            return Ok(ServeOutcome::NotFound { no_store: true });
        }
        return file_from_row(store, &row, PUBLIC_CACHE_CONTROL);
    }
    let cache = crate::services::image_cache::ImageCacheService::new();
    let Some(disk) = cache.display_path_for_public_url(local_path) else {
        return Ok(ServeOutcome::NotFound { no_store: true });
    };
    if tokio::fs::metadata(&disk).await.is_err() {
        return Ok(ServeOutcome::NotFound { no_store: false });
    }
    Ok(ServeOutcome::File(FileServe {
        mime: mime_from_path(&disk).to_string(),
        path: disk,
        etag: None,
        cache_control: CACHE_FALLBACK_CONTROL,
    }))
}

fn ready_public(row: &media_assets::Model) -> bool {
    MediaState::parse(row.state.as_deref().unwrap_or("")).ok() == Some(MediaState::Ready)
        && MediaExposure::parse(row.exposure.as_deref().unwrap_or("")).ok()
            == Some(MediaExposure::Public)
}

fn file_from_row(
    store: &MediaStore,
    row: &media_assets::Model,
    cache_control: &'static str,
) -> Result<ServeOutcome, MediaError> {
    let Some(key) = row.storage_key.as_deref() else {
        return Ok(ServeOutcome::NotFound { no_store: true });
    };
    let path = store.final_path(key)?;
    Ok(ServeOutcome::File(FileServe {
        path,
        mime: row.mime.clone(),
        etag: row.checksum_sha256.clone(),
        cache_control,
    }))
}

fn mime_from_path(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_filename_ignores_disk_tail() {
        let id = Uuid::parse_str("3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708").unwrap();
        assert!(public_filename_ok(
            "Photo 1.PNG",
            "image/png",
            id,
            "Photo1.png"
        ));
        assert!(public_filename_ok(
            "Photo 1.PNG",
            "image/png",
            id,
            "3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708.png"
        ));
        assert!(!public_filename_ok(
            "Photo 1.PNG",
            "image/png",
            id,
            "../secret.png"
        ));
        assert!(!public_filename_ok(
            "Photo 1.PNG",
            "image/png",
            id,
            "other.png"
        ));
    }
}
