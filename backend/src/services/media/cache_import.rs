//! Cached images that site content cites become durable public assets. The
//! asset's public id is derived from the cache path, so the path keeps
//! resolving to it after the cache evicts the file, with nothing recorded
//! beside the asset itself.

use chrono::Utc;
use sea_orm::{ActiveModelTrait, ConnectionTrait, Set};
use uuid::Uuid;

use crate::models::entities::media_assets;
use crate::services::image_cache::ImageCacheService;

use super::assets;
use super::error::MediaError;
use super::store::MediaStore;
use super::types::{MediaExposure, MediaScope, MediaSource, MediaState};
use super::urls::{compatible_url, filename_for_mime, storage_key};
use super::validate::extension_for_mime;

const CACHE_PREFIX: &str = "/api/phantasi/image-cache/";

/// The public id an imported copy of the cached file at `path` carries. The
/// cache reads its file names case-blind, so the id is too: every spelling
/// that reaches the file reaches its asset.
pub(super) fn cached_public_id(path: &str) -> Option<Uuid> {
    if !path.starts_with(CACHE_PREFIX) {
        return None;
    }
    let digest = <sha2::Sha256 as sha2::Digest>::digest(path.to_ascii_lowercase().as_bytes());
    let mut identity = [0u8; 16];
    identity.copy_from_slice(&digest[..16]);
    Some(uuid::Builder::from_custom_bytes(identity).into_uuid())
}

/// Import the cached file at `path` in the caller's transaction. `None` when
/// the path is not a cached image the media store accepts, or the cache no
/// longer holds it.
pub(super) async fn import_cached_citation(
    db: &impl ConnectionTrait,
    store: &MediaStore,
    cache: &ImageCacheService,
    path: &str,
) -> Result<Option<i32>, MediaError> {
    let Some(public_id) = cached_public_id(path) else {
        return Ok(None);
    };
    if let Some(row) = assets::find_by_public_id(db, public_id).await? {
        return Ok(Some(row.id));
    }
    let Some(disk) = cache.local_path_for_public_url(path) else {
        return Ok(None);
    };
    let Some(mime) = image_mime(path) else {
        return Ok(None);
    };
    let Some(ext) = extension_for_mime(mime) else {
        return Ok(None);
    };
    let key = storage_key(public_id, ext)?;
    // The file lands in the store before the caller commits. The token is the
    // public id, so a rolled-back attempt's copy is found and reused by the
    // retry instead of leaving another file behind.
    store.remove_owned_temp(public_id).await?;
    let copy = match store.publish_from_path(&key, public_id, &disk).await {
        Ok(copy) => copy,
        Err(MediaError::Missing) => return Ok(None),
        Err(error) => return Err(error),
    };
    let _ = store.remove_owned_temp(public_id).await;
    let name = path.rsplit('/').next().unwrap_or("media").to_string();
    let url = compatible_url(public_id, &filename_for_mime(&name, mime, public_id)?);
    let now = Utc::now().fixed_offset();
    let row = media_assets::ActiveModel {
        kind: Set("upload".into()),
        public_id: Set(Some(public_id)),
        url: Set(url),
        mime: Set(mime.into()),
        name: Set(name),
        size: Set(copy.size as i64),
        created_at: Set(now),
        updated_at: Set(Some(now)),
        state: Set(Some(MediaState::Ready.as_str().into())),
        state_since: Set(Some(now)),
        exposure: Set(Some(MediaExposure::Public.as_str().into())),
        first_published_at: Set(Some(now)),
        source: Set(Some(MediaSource::Import.as_str().into())),
        scope: Set(Some(MediaScope::Site.as_str().into())),
        storage_key: Set(Some(key)),
        checksum_sha256: Set(Some(copy.checksum_sha256)),
        references_complete: Set(true),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(Some(row.id))
}

/// MIME of a cached image the media store accepts, from its extension.
fn image_mime(path: &str) -> Option<&'static str> {
    let ext = std::path::Path::new(path).extension()?.to_str()?;
    Some(match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_cache_paths_have_a_derived_id() {
        let path = "/api/phantasi/image-cache/ab/ab00.png";
        assert_eq!(cached_public_id(path), cached_public_id(path));
        assert_ne!(
            cached_public_id(path),
            cached_public_id("/api/phantasi/image-cache/ab/ab01.png")
        );
        assert_eq!(cached_public_id("/media/assets/x/y.png"), None);
        assert_eq!(
            cached_public_id("/api/phantasi/image-cache/ab/AB00.PNG"),
            cached_public_id(path),
            "the cache serves both spellings from one file"
        );
    }
}
