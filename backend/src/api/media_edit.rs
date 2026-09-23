//! Edits are returned inline; only explicit confirmation persists a new asset.

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_paths_cannot_escape_federation_storage() {
        let paths = crate::services::media::LegacyPaths {
            federation_root: std::path::PathBuf::from("/data/federation_media"),
            cache_images: std::path::PathBuf::from("/cache/images"),
        };
        assert!(
            crate::services::media::legacy::legacy_disk_path(
                &paths,
                "/media/federation/1/picture.png"
            )
            .is_some()
        );
        for path in [
            "/media/federation/../secret",
            "/media/federation/1/../../secret",
            "https://example.org/a.png",
            "/media/federation/1/%2e%2e",
            "/media/federation/1/a/b",
        ] {
            assert!(
                crate::services::media::legacy::legacy_disk_path(&paths, path).is_none(),
                "{path}"
            );
        }
    }

    #[test]
    fn save_edit_does_not_call_federation_store() {
        let src = include_str!("media_edit.rs");
        assert!(!src.contains(concat!("store_federation", "_media")));
    }
    #[test]
    fn image_validation_rejects_spoofed_and_truncated_payloads() {
        let png = BASE64.decode("iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAACXBIWXMAAAPoAAAD6AG1e1JrAAAADklEQVQImWNw6fj/H4QBFnsFlbfmtiMAAAAASUVORK5CYII=").unwrap();
        assert!(validate_image(&png, "image/png").is_ok());
        assert!(validate_candidate(&png, "image/png").is_ok());
        assert!(validate_candidate(&vec![0; MAX_EDIT_BYTES + 1], "image/png").is_err());
        assert!(validate_image(&png, "image/jpeg").is_err());
        assert!(validate_image(&png[..8], "image/png").is_err());
        assert!(validate_image(&[], "image/png").is_err());
        assert!(validate_image(&vec![0; MAX_BYTES + 1], "image/png").is_err());
    }

    #[test]
    fn rejects_blank_prompts_and_unbounded_generation_dimensions() {
        assert!(validate_edit_request("  ", 1024, 1024).is_err());
        assert!(validate_edit_request("edit", 0, 1024).is_err());
        assert!(validate_edit_request("edit", 8192, 8192).is_err());
        assert!(validate_edit_request("edit", 1024, 1024).is_ok());
    }
}

use crate::models::entities::media_assets;
use crate::services::{
    data_paths::paths,
    image_generation::{self, ImageReference},
    media::{
        LegacyPaths, MediaActor, MediaContext, MediaExposure, MediaService, MediaSource,
        MediaStore, NewMediaBytes, legacy::legacy_disk_path,
    },
};
use crate::{GLOBAL_DYNAMIC_CONFIG, error::HttpError, extract::AuthedClaims};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use myriad_error::AppError;
use sea_orm::{DatabaseConnection, EntityTrait};
use serde::Deserialize;
use serde_json::{Value, json};

static EDIT_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
const MAX_BYTES: usize = 10 * 1024 * 1024;
// Base64 + JSON must also fit the saver profile's 8 MiB authenticated body cap.
const MAX_EDIT_BYTES: usize = 5 * 1024 * 1024;

#[derive(Deserialize)]
pub struct EditPreview {
    prompt: String,
    width: u32,
    height: u32,
}
#[derive(Deserialize)]
pub struct SaveEdit {
    image: String,
    #[serde(default)]
    generated: bool,
}

fn validate_edit_request(prompt: &str, width: u32, height: u32) -> Result<(), HttpError> {
    if prompt.trim().is_empty()
        || prompt.chars().count() > 2000
        || !(256..=2048).contains(&width)
        || !(256..=2048).contains(&height)
    {
        return Err(HttpError(AppError::bad_request(
            "Use a prompt of 1–2000 characters and dimensions between 256 and 2048 pixels",
        )));
    }
    Ok(())
}

async fn editable_asset(
    db: &DatabaseConnection,
    headers: &HeaderMap,
    id: i32,
) -> Result<media_assets::Model, HttpError> {
    crate::middleware::auth::verify_current_admin_from_headers(headers, db)
        .await
        .map_err(HttpError::from)?;
    let asset = media_assets::Entity::find_by_id(id)
        .one(db)
        .await
        .map_err(|_| HttpError(AppError::internal("Failed to read media")))?
        .ok_or_else(|| HttpError(AppError::not_found("Media not found")))?;
    if !matches!(
        asset.mime.as_str(),
        "image/png" | "image/jpeg" | "image/webp"
    ) {
        return Err(HttpError(AppError::bad_request(
            "Only PNG, JPEG and WebP images can be edited",
        )));
    }
    Ok(asset)
}

fn validate_image(bytes: &[u8], mime: &str) -> Result<(), HttpError> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(HttpError(AppError::bad_request(
            "Image must be at most 10 MB",
        )));
    }
    image_generation::validate_media_type(mime)
        .and_then(|()| image_generation::validate_magic(bytes, mime))
        .map_err(|_| HttpError(AppError::bad_request("Invalid image")))?;
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| HttpError(AppError::bad_request("Invalid image")))?;
    let (w, h) = reader
        .into_dimensions()
        .map_err(|_| HttpError(AppError::bad_request("Invalid image")))?;
    if w == 0 || h == 0 || w > 8192 || h > 8192 || u64::from(w) * u64::from(h) > 16_777_216 {
        return Err(HttpError(AppError::bad_request(
            "Image dimensions exceed the editing limit",
        )));
    }
    Ok(())
}

fn validate_candidate(bytes: &[u8], mime: &str) -> Result<(), HttpError> {
    if bytes.len() > MAX_EDIT_BYTES {
        return Err(HttpError(
            AppError::bad_request("Edited image exceeds 5 MB; reduce its resolution")
                .with_code("MEDIA_EDIT_TOO_LARGE"),
        ));
    }
    validate_image(bytes, mime)
}

async fn read_file_capped(path: std::path::PathBuf) -> Result<Vec<u8>, HttpError> {
    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(|_| HttpError(AppError::not_found("Source image is missing")))?;
    if metadata.len() > MAX_BYTES as u64 {
        return Err(HttpError(AppError::bad_request(
            "Reference image is too large",
        )));
    }
    tokio::fs::read(path)
        .await
        .map_err(|_| HttpError(AppError::not_found("Source image is missing")))
}

async fn read_reference(asset: &media_assets::Model) -> Result<ImageReference, HttpError> {
    let store = MediaStore::new(paths().media.clone());
    let bytes = if let Some(key) = asset.storage_key.as_deref() {
        let path = store
            .final_path(key)
            .map_err(|_| HttpError(AppError::not_found("Source image is missing")))?;
        read_file_capped(path).await?
    } else if let Some(path) = legacy_disk_path(&LegacyPaths::from_data_paths(paths()), &asset.url)
    {
        read_file_capped(path).await?
    } else {
        crate::services::image_cache::ImageCacheService::new()
            .read_local_public_url(&asset.url)
            .await
            .map_err(|_| HttpError(AppError::bad_request("Source image is unavailable")))?
            .0
    };
    // validate_image already ran the MIME + magic checks ImageReference::new performs.
    validate_image(&bytes, &asset.mime)?;
    Ok(ImageReference {
        bytes: bytes.into(),
        media_type: asset.mime.clone(),
    })
}

pub async fn preview_edit(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Path(id): Path<i32>,
    Json(input): Json<EditPreview>,
) -> Result<Json<Value>, HttpError> {
    let asset = editable_asset(&db, &headers, id).await?;
    validate_edit_request(&input.prompt, input.width, input.height)?;
    let _permit = EDIT_SLOTS.try_acquire().map_err(|_| {
        HttpError(AppError::service_unavailable(
            "Image editor is busy; please try again",
        ))
    })?;
    let reference = read_reference(&asset).await?;
    let dynamic = GLOBAL_DYNAMIC_CONFIG.read().await;
    let config = image_generation::config_from_dynamic(&dynamic)
        .map_err(|_| HttpError(AppError::bad_request("Image generation is not configured")))?;
    drop(dynamic);
    let generated = image_generation::generate_image(
        &config,
        input.prompt.trim(),
        input.width,
        input.height,
        Some(&reference),
    )
    .await
    .map_err(|_| {
        HttpError(AppError::bad_gateway(
            "Image editing failed; please try again",
        ))
    })?;
    let (bytes, mime) = image_generation::load_generated_bytes(&generated)
        .await
        .map_err(|_| HttpError(AppError::bad_gateway("Unable to load generated image")))?;
    validate_candidate(&bytes, &mime)?;
    // No storage or catalog call on this path. Closing the browser discards the candidate.
    Ok(Json(
        json!({ "image": format!("data:{mime};base64,{}", BASE64.encode(&bytes)) }),
    ))
}

pub async fn save_edit(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    AuthedClaims(claims): AuthedClaims,
    Path(id): Path<i32>,
    Json(input): Json<SaveEdit>,
) -> Result<Json<Value>, HttpError> {
    let source = editable_asset(&db, &headers, id).await?;
    let (metadata, encoded) = input
        .image
        .strip_prefix("data:")
        .and_then(|s| s.split_once(','))
        .ok_or_else(|| HttpError(AppError::bad_request("Expected an inline image")))?;
    let mime = metadata
        .strip_suffix(";base64")
        .filter(|m| matches!(*m, "image/png" | "image/jpeg" | "image/webp"))
        .ok_or_else(|| HttpError(AppError::bad_request("Unsupported image format")))?;
    if encoded.len() > MAX_EDIT_BYTES.div_ceil(3) * 4 {
        return Err(HttpError(
            AppError::bad_request("Edited image exceeds 5 MB; reduce its resolution")
                .with_code("MEDIA_EDIT_TOO_LARGE"),
        ));
    }
    let bytes = BASE64
        .decode(encoded)
        .map_err(|_| HttpError(AppError::bad_request("Invalid image data")))?;
    validate_candidate(&bytes, mime)?;
    let user_id = claims
        .sub
        .parse()
        .map_err(|_| HttpError(AppError::unauthorized("Invalid user")))?;
    let stem = std::path::Path::new(&source.name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let ext = match mime {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => "png",
    };
    let name = format!(
        "{}-edited.{ext}",
        stem.chars().take(160).collect::<String>()
    );
    let actor = MediaActor::admin(user_id).map_err(|err| HttpError(err.into()))?;
    let source_kind = if input.generated {
        MediaSource::Generated
    } else {
        MediaSource::Upload
    };
    let created = MediaService::from_data_paths(paths())
        .create_from_bytes(
            &db,
            MediaContext::site(actor, source_kind),
            NewMediaBytes {
                bytes,
                claimed_mime: mime.to_string(),
                filename: name,
                max_bytes: MAX_EDIT_BYTES,
                derived_from_id: Some(source.id),
                exposure: MediaExposure::Private,
            },
        )
        .await
        .map_err(|err| HttpError(err.into()))?;
    Ok(Json(json!({
        "item": {
            "id": created.id,
            "public_id": created.public_id,
            "kind": created.kind,
            "url": created.catalog_url(),
            "content_path": created.content_path,
            "public_path": created.public_path,
            "mime": created.mime,
            "name": created.name,
            "size": created.size,
            "source": created.source.as_str(),
            "state": created.state.as_str(),
            "exposure": created.exposure.as_str(),
            "created_at": created.created_at.timestamp_millis(),
            "references": [],
            "derived_from_id": created.derived_from_id,
        }
    })))
}
