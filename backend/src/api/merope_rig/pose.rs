//! 姿势修正：改写当前装配包的修正表，另存为新包并换上。

use axum::{Extension, Json, extract::State};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    ApiResult, bad_request, internal_error,
    master::{manifest_matches_master, master_from_persona},
    not_found,
    package::{
        bind_and_activate_outfit_rig, load_stored_manifest, package_identity_matches,
        rewrite_texture_urls, rig_revision_conflict, rig_revision_matches,
    },
    require_merope_enabled, require_owner,
};
use crate::{
    middleware::auth::Claims,
    services::{agent::merope, merope_rig},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavePoseCorrectionsRequest {
    asset_id: String,
    corrections: Value,
}

pub async fn save_pose_corrections(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<SavePoseCorrectionsRequest>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    let user_id = require_owner(&claims, &db).await?;
    let previous = merope_rig::normalize_asset_id(&payload.asset_id)
        .ok_or_else(|| bad_request("Invalid rig asset id"))?;
    let persona = merope::get_persona(&db).await.map_err(internal_error)?;
    if !rig_revision_matches(
        persona.as_ref().and_then(|row| row.visual_profile.as_ref()),
        Some(&previous),
    ) {
        return Err(rig_revision_conflict());
    }
    let master = persona
        .as_ref()
        .and_then(master_from_persona)
        .ok_or_else(|| not_found("Site portrait is missing"))?;
    let mut manifest = load_stored_manifest(&previous).await?;
    if !manifest_matches_master(&manifest, &master)
        || !package_identity_matches(&previous, &manifest).await?
    {
        return Err(rig_revision_conflict());
    }
    let playback = manifest
        .anime25d_playback
        .as_mut()
        .ok_or_else(|| bad_request("Anime2.5D playback is missing"))?;
    if !myriad_merope::replace_anime25d_pose_corrections(playback, &payload.corrections) {
        return Err(bad_request("Invalid pose corrections"));
    }
    let atlas = merope_rig::read_atlas_bytes(&previous)
        .await
        .map_err(internal_error)?;
    let asset_id =
        merope_rig::package_id_for_manifest(&atlas, &manifest).map_err(internal_error)?;
    let manifest = rewrite_texture_urls(manifest, &asset_id);
    let json = serde_json::to_string_pretty(&manifest).map_err(internal_error)?;
    // The old package stays recoverable. CAS under the persona row lock prevents
    // an overlapping import, edit or outfit switch from being overwritten.
    merope_rig::persist_package(&asset_id, &atlas, &json)
        .await
        .map_err(internal_error)?;
    bind_and_activate_outfit_rig(&db, user_id, &asset_id, &master, Some(&previous)).await?;
    Ok(Json(json!({ "manifest": manifest, "assetId": asset_id })))
}

#[cfg(test)]
mod pose_correction_tests {
    use super::*;

    #[test]
    fn revision_guard_rejects_replaced_rig_and_switched_outfit() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let mut profile = json!({ "activeOutfitId": "one", "wardrobe": [
            { "id": "one", "rigAssetId": a }, { "id": "two", "rigAssetId": b }
        ] });
        assert!(rig_revision_matches(Some(&profile), Some(&a)));
        assert!(!rig_revision_matches(None, Some(&a)));
        profile["activeOutfitId"] = json!("two");
        assert!(!rig_revision_matches(Some(&profile), Some(&a)));
        profile["activeOutfitId"] = json!("one");
        profile["wardrobe"][0]["rigAssetId"] = json!(b);
        assert!(!rig_revision_matches(Some(&profile), Some(&a)));
    }

    #[test]
    fn correction_request_cannot_replace_textures_or_other_profiles() {
        let mut payload = json!({ "assetId": "a".repeat(64), "corrections": [] });
        assert!(serde_json::from_value::<SavePoseCorrectionsRequest>(payload.clone()).is_ok());
        payload["shellProfile"] = json!({});
        assert!(serde_json::from_value::<SavePoseCorrectionsRequest>(payload).is_err());
    }
}
