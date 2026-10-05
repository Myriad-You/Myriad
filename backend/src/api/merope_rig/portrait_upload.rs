//! 上传立绘：换上主人自己的画，同一事务里作废旧装配。

use axum::{
    Extension, Json,
    extract::{Multipart, Path, State},
    http::StatusCode,
};
use myriad_merope::BustPortraitRefusal;
use sea_orm::{DatabaseConnection, TransactionTrait};
use serde_json::{Value, json};

use super::{
    ApiResult, bad_request,
    full_body::{finish, lock_persona, save_visual_profile},
    internal_error, not_found,
    portrait::bind_worn_portrait,
    require_merope_enabled, require_owner,
};
use crate::{
    middleware::auth::Claims,
    services::{agent::merope, image_generation, merope_rig},
};

mod trim;

const MAX_PORTRAIT_BYTES: usize = 10 * 1024 * 1024;

pub(super) fn uploaded_portrait_reference(
    bytes: impl Into<axum::body::Bytes>,
) -> ApiResult<image_generation::ImageReference> {
    let bytes = bytes.into();
    let mime = match image::guess_format(&bytes) {
        Ok(image::ImageFormat::Png) => "image/png",
        Ok(image::ImageFormat::Jpeg) => "image/jpeg",
        Ok(image::ImageFormat::WebP) => "image/webp",
        _ => return Err(bad_request("Portrait must be a PNG, JPEG, or WebP image")),
    };
    image_generation::ImageReference::new(bytes, mime)
        .map_err(|error| bad_request(&error.to_string()))
}

/// The picture with its blank margins cut away (see [`trim`]), or as
/// uploaded when there is nothing to cut.
async fn trimmed_portrait(
    reference: image_generation::ImageReference,
) -> image_generation::ImageReference {
    let bytes = reference.bytes.clone();
    match tokio::task::spawn_blocking(move || trim::trim_portrait(&bytes, MAX_PORTRAIT_BYTES)).await
    {
        Ok(Some(trimmed)) => {
            image_generation::ImageReference::new(trimmed.bytes, trimmed.media_type)
                .unwrap_or(reference)
        }
        _ => reference,
    }
}

/// The one `image` field of a portrait upload, checked to be a supported
/// picture and cut to what is drawn on it.
pub(super) async fn read_portrait_upload(
    mut multipart: Multipart,
) -> ApiResult<image_generation::ImageReference> {
    let mut image_bytes = None;
    while let Some(field) = multipart.next_field().await.map_err(|error| {
        tracing::error!(%error, "Invalid portrait upload");
        bad_request("Invalid portrait image")
    })? {
        match field.name() {
            Some("image") if image_bytes.is_none() => {
                let bytes = field.bytes().await.map_err(|error| {
                    tracing::error!(%error, "Invalid portrait image");
                    bad_request("Invalid portrait image")
                })?;
                if bytes.len() > MAX_PORTRAIT_BYTES {
                    return Err(bad_request("Portrait image exceeds 10 MB"));
                }
                let reference = uploaded_portrait_reference(bytes)?;
                image_bytes = Some(trimmed_portrait(reference).await);
            }
            Some("image") => {
                return Err(bad_request("Portrait upload fields must not be duplicated"));
            }
            _ => return Err(bad_request("Portrait upload contains an unsupported field")),
        }
    }
    image_bytes.ok_or_else(|| bad_request("Portrait upload is missing image"))
}

/// Stores an uploaded portrait privately; returns its catalog URL.
pub(super) async fn persist_uploaded_portrait(
    db: &DatabaseConnection,
    user_id: i32,
    reference: image_generation::ImageReference,
) -> ApiResult<String> {
    let actor = crate::services::media::MediaActor::admin(user_id)
        .map_err(|error| internal_error(error.to_string()))?;
    let (asset, _) =
        crate::services::media::MediaService::from_data_paths(crate::services::data_paths::paths())
            .persist_ready_bytes(
                db,
                crate::services::media::MediaContext::site(
                    actor,
                    crate::services::media::MediaSource::Upload,
                ),
                crate::services::media::NewMediaBytes {
                    bytes: reference.bytes,
                    claimed_mime: reference.media_type,
                    filename: "portrait".into(),
                    max_bytes: MAX_PORTRAIT_BYTES,
                    derived_from_id: None,
                    exposure: crate::services::media::MediaExposure::Private,
                },
            )
            .await
            .map_err(|error| match error {
                crate::services::media::MediaError::Invalid { .. } => {
                    bad_request(&error.to_string())
                }
                _ => internal_error(error.to_string()),
            })?;
    Ok(asset.catalog_url())
}

pub async fn upload_portrait(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    multipart: Multipart,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let reference = read_portrait_upload(multipart).await?;
    let stored = crate::services::image_cache::StoredImage {
        url: persist_uploaded_portrait(&db, user_id, reference).await?,
        created: true,
    };
    let persona = merope::get_persona(&db).await.map_err(internal_error)?;
    let name = persona
        .as_ref()
        .map(|row| row.name.trim())
        .filter(|name| !name.is_empty())
        .unwrap_or("Arael")
        .to_string();
    let personality = persona
        .as_ref()
        .map(|row| row.personality.clone())
        .unwrap_or_default();
    // 换主图就是换血统源头，旧 Rig 当场作废——和 generate_portrait 一样放进同
    // 一个事务。读路径的 manifest_matches_master 也拦得住，但那是每次请求重读
    // 一遍旧包再丢掉，而 `/active` 是公开路由，首页挂件每次加载都会走到。
    let transaction = db.begin().await.map_err(internal_error)?;
    let public_url = match crate::services::media::normalize_local_url(
        &transaction,
        &stored.url,
        &crate::services::media::configured_origins().await,
    )
    .await
    {
        Ok(url) => url,
        Err(error) => {
            let _ = transaction.rollback().await;
            return Err(internal_error(error.to_string()));
        }
    };
    let visual_profile = merope::get_persona_on(&transaction)
        .await
        .map_err(internal_error)?
        .and_then(|persona| persona.visual_profile)
        .map(|mut profile| {
            bind_worn_portrait(&mut profile, &public_url, None);
            profile
        });
    if let Err(error) = merope::upsert_persona_on(
        &transaction,
        name,
        personality,
        merope::PortraitUpdate::Set(public_url.clone()),
        merope::PersonaContractUpdate {
            visual_profile: visual_profile
                .map(merope::JsonDocumentUpdate::Set)
                .unwrap_or(merope::JsonDocumentUpdate::Keep),
            portrait_generation: merope::JsonDocumentUpdate::Clear,
            ..Default::default()
        },
        user_id,
    )
    .await
    {
        let _ = transaction.rollback().await;
        return Err(internal_error(error));
    }
    let cleared_asset = match merope_rig::persist_active_asset(&transaction, None).await {
        Ok(asset_id) => asset_id,
        Err(error) => {
            let _ = transaction.rollback().await;
            return Err(internal_error(error));
        }
    };
    let saved_persona = merope::get_persona_on(&transaction)
        .await
        .map_err(internal_error)?;
    crate::services::media::bind_persona(
        &transaction,
        Some(&public_url),
        None,
        saved_persona
            .as_ref()
            .and_then(|p| p.visual_profile.as_ref()),
        &crate::services::media::configured_origins().await,
    )
    .await
    .map_err(|error| internal_error(error.to_string()))?;
    transaction.commit().await.map_err(internal_error)?;
    merope_rig::mirror_active_asset(cleared_asset).await;
    Ok(Json(json!({
        "portraitUrl": public_url,
        "portraitAssetId": public_url,
    })))
}

/// The owner's picture for a bust set that is not worn, so a set can be made
/// from ready-made art without putting it on. The worn set's picture is the
/// master portrait and goes through [`upload_portrait`].
pub async fn upload_outfit_portrait(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(outfit_id): Path<String>,
    multipart: Multipart,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let reference = read_portrait_upload(multipart).await?;
    let url = persist_uploaded_portrait(&db, user_id, reference).await?;
    let transaction = db.begin().await.map_err(internal_error)?;
    let written = async {
        let row = lock_persona(&transaction).await?;
        let public_url = crate::services::media::normalize_local_url(
            &transaction,
            &url,
            &crate::services::media::configured_origins().await,
        )
        .await
        .map_err(|error| internal_error(error.to_string()))?;
        let mut profile = row.visual_profile.clone().unwrap_or_else(|| json!({}));
        myriad_merope::bind_bust_outfit_portrait(&mut profile, &outfit_id, &public_url).map_err(
            |refusal| match refusal {
                BustPortraitRefusal::Missing => not_found("The outfit is missing"),
                BustPortraitRefusal::Worn => (
                    StatusCode::CONFLICT,
                    Json(json!({
                        "error": "The worn outfit's picture is the master portrait",
                        "code": "outfit_is_worn"
                    })),
                ),
                BustPortraitRefusal::Invalid => bad_request("Invalid portrait image"),
            },
        )?;
        crate::services::media::bind_persona(
            &transaction,
            row.portrait_asset_id.as_deref(),
            row.avatar_asset_id.as_deref(),
            Some(&profile),
            &crate::services::media::configured_origins().await,
        )
        .await
        .map_err(|error| internal_error(error.to_string()))?;
        save_visual_profile(&transaction, row, profile, user_id).await?;
        Ok(public_url)
    }
    .await;
    let public_url = finish(transaction, written).await?;
    Ok(Json(json!({ "portraitUrl": public_url })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portrait_upload_detects_supported_content_without_multipart_mime() {
        for (bytes, mime) in [
            (b"\x89PNG\r\n\x1a\n".as_slice(), "image/png"),
            (b"\xff\xd8\xff\xe0".as_slice(), "image/jpeg"),
            (b"RIFF\x00\x00\x00\x00WEBP".as_slice(), "image/webp"),
        ] {
            assert_eq!(
                uploaded_portrait_reference(bytes.to_vec())
                    .unwrap()
                    .media_type,
                mime
            );
        }
        assert!(uploaded_portrait_reference(b"GIF89a".to_vec()).is_err());
        assert!(uploaded_portrait_reference(b"not an image".to_vec()).is_err());
    }
}
