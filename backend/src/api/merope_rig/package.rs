//! 已编译的装配包：读取、校验身份、启用到穿着的衣服上。

use std::sync::OnceLock;

use axum::{
    Extension, Json,
    body::Body,
    extract::{Path, State},
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use myriad_error::AppError;
use myriad_merope::{CharacterAssetProfile, RigManifest};
use sea_orm::{DatabaseConnection, EntityTrait, QuerySelect, TransactionTrait};
use serde_json::{Value, json};

use super::{
    ApiError, ApiResult, bad_request, internal_error,
    master::{MasterProvenance, manifest_matches_master, master_from_persona},
    not_found, require_merope_enabled, require_owner,
};
use crate::{
    middleware::auth::Claims,
    services::{agent::merope, merope_rig, retained_cache::RetainedCache, rig_chest_analysis},
};

pub(super) fn rewrite_texture_urls(mut manifest: RigManifest, asset_id: &str) -> RigManifest {
    let url = merope_rig::public_atlas_url(asset_id);
    for texture in &mut manifest.textures {
        texture.url = url.clone();
    }
    manifest
}

pub(super) async fn load_stored_manifest(asset_id: &str) -> ApiResult<RigManifest> {
    let bytes = merope_rig::read_manifest_bytes(asset_id)
        .await
        .map_err(|_| not_found("Active rig is missing"))?;
    serde_json::from_slice(&bytes).map_err(|error| {
        tracing::error!(%error, "Stored rig is invalid");
        bad_request("Stored rig is invalid")
    })
}

fn verified_packages() -> &'static tokio::sync::RwLock<RetainedCache<String, ()>> {
    static VERIFIED_PACKAGES: OnceLock<tokio::sync::RwLock<RetainedCache<String, ()>>> =
        OnceLock::new();
    VERIFIED_PACKAGES.get_or_init(|| {
        tokio::sync::RwLock::new(RetainedCache::new(
            256,
            std::time::Duration::from_secs(3600),
        ))
    })
}

pub(crate) async fn cleanup_verified_packages() {
    verified_packages().write().await.purge_expired();
}

pub(super) async fn package_identity_matches(
    asset_id: &str,
    manifest: &RigManifest,
) -> ApiResult<bool> {
    let verified = verified_packages();
    if verified.write().await.get(asset_id).is_some() {
        return Ok(true);
    }
    let atlas_bytes = merope_rig::read_atlas_bytes(asset_id)
        .await
        .map_err(|_| not_found("Active rig atlas is missing"))?;
    let expected =
        merope_rig::package_id_for_manifest(&atlas_bytes, manifest).map_err(internal_error)?;
    let matches = expected == asset_id;
    if matches {
        verified.write().await.insert(asset_id.to_string(), ());
    }
    Ok(matches)
}

pub(super) async fn bind_and_activate_outfit_rig(
    db: &DatabaseConnection,
    user_id: i32,
    asset_id: &str,
    expected: &MasterProvenance,
    expected_rig: Option<&str>,
) -> ApiResult<()> {
    let transaction = db.begin().await.map_err(internal_error)?;
    merope::api::store::lock_persona_on(&transaction)
        .await
        .map_err(internal_error)?;
    // The row lock spans provenance validation, outfit binding and activation.
    // A concurrent portrait/outfit UPDATE cannot slip between those operations.
    let row =
        crate::models::entities::agent_persona::Entity::find_by_id(merope::api::store::PERSONA_ROW_ID)
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(internal_error)?;
    if row.as_ref().and_then(master_from_persona).as_ref() != Some(expected) {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Character master or worn outfit changed before rig activation",
                "code": "character_asset_provenance_changed"
            })),
        ));
    }
    if let Some(row) = row {
        if !rig_revision_matches(row.visual_profile.as_ref(), expected_rig) {
            return Err(rig_revision_conflict());
        }
        let mut profile = row.visual_profile.clone().unwrap_or_else(|| json!({}));
        if !myriad_merope::bind_active_outfit_rig(&mut profile, asset_id) {
            return Err(bad_request("The worn outfit is missing"));
        }
        if let Err(error) = merope::upsert_persona_on(
            &transaction,
            row.name,
            row.personality,
            merope::PortraitUpdate::Keep,
            merope::PersonaContractUpdate {
                visual_profile: merope::JsonDocumentUpdate::Set(profile),
                ..Default::default()
            },
            user_id,
        )
        .await
        {
            let _ = transaction.rollback().await;
            return Err(internal_error(error));
        }
    }
    let persisted = match merope_rig::persist_active_asset(&transaction, Some(asset_id)).await {
        Ok(value) => value,
        Err(error) => {
            let _ = transaction.rollback().await;
            return Err(internal_error(error));
        }
    };
    transaction.commit().await.map_err(internal_error)?;
    merope_rig::mirror_active_asset(persisted).await;
    Ok(())
}

pub(super) fn rig_revision_matches(profile: Option<&Value>, expected: Option<&str>) -> bool {
    expected
        .is_none_or(|id| myriad_merope::active_outfit_rig_asset_id(profile).as_deref() == Some(id))
}

pub(super) fn rig_revision_conflict() -> ApiError {
    (
        StatusCode::CONFLICT,
        Json(
            json!({ "error": "The worn rig changed; reload before saving corrections", "code": "rig_asset_changed" }),
        ),
    )
}

pub async fn get_active_rig(crate::extract::Db(db): crate::extract::Db) -> ApiResult<Json<Value>> {
    // Read portrait and worn rig from the same committed persona snapshot.
    // A late configuration mirror must never select a different package.
    let persona = merope::get_persona(&db).await.map_err(internal_error)?;
    let master = persona.as_ref().and_then(master_from_persona);
    let asset_id = myriad_merope::active_outfit_rig_asset_id(
        persona.as_ref().and_then(|row| row.visual_profile.as_ref()),
    );
    figure_json(master, asset_id).await
}

/// A master and the package bound to it, played only when the package still
/// descends from that master; otherwise the master's portrait alone.
pub(super) async fn figure_json(
    master: Option<MasterProvenance>,
    asset_id: Option<String>,
) -> ApiResult<Json<Value>> {
    if let (Some(asset_id), Some(master)) = (asset_id, master.as_ref()) {
        match load_stored_manifest(&asset_id).await {
            Ok(mut manifest) if manifest_matches_master(&manifest, master) => {
                if matches!(
                    package_identity_matches(&asset_id, &manifest).await,
                    Ok(true)
                ) {
                    if master.gender == "male" {
                        if let Some(playback) = manifest.anime25d_playback.as_mut() {
                            rig_chest_analysis::apply_male_policy(playback);
                        }
                    }
                    let manifest = rewrite_texture_urls(manifest, &asset_id);
                    return Ok(Json(json!({
                        "manifest": manifest,
                        "portraitUrl": master.asset_id,
                        "generationFingerprint": master.generation_fingerprint,
                        "assetId": asset_id,
                    })));
                }
                tracing::warn!(
                    %asset_id,
                    "active merope rig package identity is stale; serving portrait only"
                );
            }
            Ok(_) => tracing::warn!(
                %asset_id,
                "active merope rig provenance is stale; serving portrait only"
            ),
            Err(_) => tracing::warn!(
                %asset_id,
                "active merope rig package is unavailable; serving portrait only"
            ),
        }
    }
    if let Some(master) = master {
        return Ok(Json(json!({
            "manifest": Value::Null,
            "portraitUrl": master.asset_id,
            "generationFingerprint": master.generation_fingerprint,
            "assetId": Value::Null,
        })));
    }
    Err(not_found("No site face is configured"))
}

pub(crate) async fn wardrobe_outfit_face(
    db: &DatabaseConnection,
    outfit_id: &str,
) -> ApiResult<Json<Value>> {
    let outfit_id = outfit_id.trim();
    if outfit_id.is_empty() || outfit_id.chars().count() > myriad_merope::MAX_WARDROBE_ID_CHARS {
        return Err(bad_request("Invalid outfit id"));
    }
    let persona = merope::get_persona(db)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| not_found("No site face is configured"))?;
    let profile = persona
        .visual_profile
        .as_ref()
        .ok_or_else(|| not_found("Outfit is not in the wardrobe"))?;
    let looks = myriad_merope::looks_from_visual_profile(profile);
    let look = myriad_merope::wardrobe_look(&looks, outfit_id)
        .cloned()
        .ok_or_else(|| not_found("Outfit is not in the wardrobe"))?;
    if !look.playable() {
        return Err(not_found("Outfit has no portrait or rig"));
    }
    let gender = profile
        .get("gender")
        .and_then(Value::as_str)
        .filter(|value| matches!(*value, "female" | "male" | "nonbinary" | "unspecified"))
        .unwrap_or("unspecified");
    if let Some(rig_id) = look.rig_asset_id.as_deref() {
        if let Ok(mut manifest) = load_stored_manifest(rig_id).await {
            let matches_item = match look.portrait_asset_id.as_deref() {
                Some(portrait) => manifest_matches_master(
                    &manifest,
                    &MasterProvenance {
                        profile: CharacterAssetProfile::Bust,
                        asset_id: portrait.to_string(),
                        generation_fingerprint: look.generation_fingerprint.clone(),
                        gender: gender.to_string(),
                        outfit_id: Some(outfit_id.to_string()),
                    },
                ),
                None => manifest.validate().is_ok(),
            };
            if matches_item && matches!(package_identity_matches(rig_id, &manifest).await, Ok(true))
            {
                if gender == "male" {
                    if let Some(playback) = manifest.anime25d_playback.as_mut() {
                        rig_chest_analysis::apply_male_policy(playback);
                    }
                }
                let manifest = rewrite_texture_urls(manifest, rig_id);
                return Ok(Json(json!({
                    "manifest": manifest,
                    "portraitUrl": look.portrait_asset_id,
                    "generationFingerprint": look.generation_fingerprint,
                    "assetId": rig_id,
                    "outfitId": look.id,
                })));
            }
        }
    }
    if look.portrait_asset_id.is_some() {
        return Ok(Json(json!({
            "manifest": Value::Null,
            "portraitUrl": look.portrait_asset_id,
            "generationFingerprint": look.generation_fingerprint,
            "assetId": Value::Null,
            "outfitId": look.id,
        })));
    }
    Err(not_found("Outfit face is unavailable"))
}

pub async fn get_atlas(Path(asset_id): Path<String>) -> ApiResult<Response> {
    let asset_id = merope_rig::normalize_asset_id(&asset_id)
        .ok_or_else(|| bad_request("Invalid rig asset id"))?;
    let bytes = merope_rig::read_atlas_bytes(&asset_id).await.map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Rig atlas not found")),
        )
    })?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static("image/png"))
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        )
        .body(Body::from(bytes))
        .map_err(internal_error)
}

pub async fn get_site_rig(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    get_active_rig(crate::extract::Db(db)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_reader_uses_one_persona_snapshot_not_the_configuration_mirror() {
        let source = include_str!("package.rs");
        let reader = source
            .split("pub async fn get_active_rig(")
            .nth(1)
            .unwrap()
            .split("\npub ")
            .next()
            .unwrap();
        assert_eq!(reader.matches("merope::get_persona(&db)").count(), 1);
        assert!(reader.contains("active_outfit_rig_asset_id("));
        assert!(!reader.contains("GLOBAL_DYNAMIC_CONFIG"));
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let mut profile = json!({"activeOutfitId":"a", "wardrobe":[
            {"id":"a", "rigAssetId":a}, {"id":"b", "rigAssetId":b}
        ]});
        assert_eq!(
            myriad_merope::active_outfit_rig_asset_id(Some(&profile)),
            Some(a)
        );
        profile["activeOutfitId"] = json!("b");
        assert_eq!(
            myriad_merope::active_outfit_rig_asset_id(Some(&profile)),
            Some(b)
        );
        profile["wardrobe"][1]["rigAssetId"] = Value::Null;
        assert_eq!(
            myriad_merope::active_outfit_rig_asset_id(Some(&profile)),
            None
        );
    }
}
