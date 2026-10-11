//! 前端编译好的装配导入：解析上传、编译、胸部分析，预览或落盘启用。

use std::collections::HashMap;

use axum::{
    Extension, Json,
    extract::{Multipart, State},
    http::StatusCode,
};
use myriad_error::AppError;
use myriad_merope::{
    CharacterAssetProfile, RigBone, RigCompileSource, RigLayerSource, RigManifest,
    RigMotionProfile, RigOutfitProfile, RigSemanticAnchor, RigSemantics, RigSize,
    RigSpatialProfile, RigTexture, compile_layered_rig, migrate_rig_manifest,
    validate_character_asset_source,
};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    ApiError, ApiResult, bad_request,
    full_body::bind_full_body_rig,
    internal_error,
    master::{MasterProvenance, MasterSlot, require_master_match, valid_generation_fingerprint},
    package::{bind_and_activate_outfit_rig, load_stored_manifest, rewrite_texture_urls},
    pose::rewrite_playback,
    require_merope_enabled, require_owner,
};
use crate::{
    middleware::auth::Claims,
    services::{image_generation, merope_rig, rig_chest_analysis, rig_part_analysis},
};

const MAX_RIG_IMPORT_SOURCE_BYTES: usize = 2 * 1024 * 1024;
const MAX_RIG_IMPORT_ATLAS_BYTES: usize = 20 * 1024 * 1024;
pub(super) const MAX_RIG_ANALYSIS_REFERENCE_BYTES: usize = 10 * 1024 * 1024;

fn rig_motion_seed(source_master_asset_id: &str) -> u32 {
    let mut hash: u32 = 2166136261;
    for byte in source_master_asset_id.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16777619);
    }
    hash
}

async fn compile_imported_rig(
    source: ImportRigSourceRequest,
    texture_url: String,
) -> ApiResult<(String, RigManifest)> {
    validate_character_asset_source(source.profile, &source.bones, &source.layers).map_err(
        |error| {
            tracing::error!(%error, "Rig character asset preflight failed");
            bad_request("Rig character asset is invalid")
        },
    )?;
    let source_master_asset_id = source.source_master_asset_id.clone();
    let manifest = tokio::task::spawn_blocking(move || {
        compile_layered_rig(RigCompileSource {
            rig_ir_version: source.rig_ir_version,
            character_asset_contract_version: Some(source.character_asset_contract_version),
            profile: source.profile,
            source_master_asset_id: Some(source.source_master_asset_id),
            source_generation_fingerprint: source.source_generation_fingerprint,
            canvas: source.canvas,
            textures: vec![RigTexture {
                id: source.atlas.id,
                url: texture_url,
                width: source.atlas.width,
                height: source.atlas.height,
            }],
            bones: source.bones,
            layers: source.layers,
            motion_profile: source.motion_profile,
            outfit_profile: source.outfit_profile,
            semantic_anchors: source.semantic_anchors,
            semantics: source.semantics,
            spatial_profile: source.spatial_profile,
            anime25d_playback: source.anime25d_playback,
        })
    })
    .await
    .map_err(internal_error)?
    .map_err(|error| {
        tracing::error!(%error, "Rig compilation failed");
        bad_request("Rig compilation failed")
    })?;
    let (manifest, _) = migrate_rig_manifest(manifest, rig_motion_seed(&source_master_asset_id))
        .map_err(|error| {
            tracing::error!(%error, "Rig manifest migration failed");
            bad_request("Rig compilation failed")
        })?;
    Ok((source_master_asset_id, manifest))
}

async fn prepare_import_chest_profile(
    user_id: i32,
    source: &mut ImportRigSourceRequest,
    gender: &str,
    analyze_with_ai: bool,
    analysis_reference: Option<&[u8]>,
) {
    let Some(playback) = source.anime25d_playback.as_mut() else {
        return;
    };
    if gender == "male" {
        rig_chest_analysis::apply_male_policy(playback);
        return;
    }
    if !analyze_with_ai {
        rig_chest_analysis::ensure_safe_enabled_profile(playback);
        return;
    }
    let Some(analysis_reference) = analysis_reference else {
        tracing::warn!(
            "imported rig composition missing from chest analysis; using geometry fallback"
        );
        rig_chest_analysis::ensure_safe_enabled_profile(playback);
        return;
    };
    let reference = match image_generation::ImageReference::new(
        analysis_reference.to_vec(),
        "image/png",
    ) {
        Ok(reference) => reference,
        Err(error) => {
            tracing::warn!(%error, "could not load imported rig reference for chest analysis; using geometry fallback");
            rig_chest_analysis::ensure_safe_enabled_profile(playback);
            return;
        }
    };
    crate::services::ai_cost_ledger::with_site_ai_ledger(
        user_id,
        "merope",
        "rig-chest-analysis",
        rig_chest_analysis::analyze_once_or_fallback(playback, &reference),
    )
    .await;
}

/// The rig's moving parts (rig_part_analysis): one found already rides along;
/// none is looked for here, where the import waits.
fn prepare_import_part_profile(source: &mut ImportRigSourceRequest) {
    if let Some(playback) = source.anime25d_playback.as_mut() {
        rig_part_analysis::ensure_profile(playback);
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportRigAtlasRequest {
    id: String,
    width: u32,
    height: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportRigSourceRequest {
    #[serde(default)]
    rig_ir_version: Option<u16>,
    character_asset_contract_version: u16,
    #[serde(default)]
    profile: CharacterAssetProfile,
    source_master_asset_id: String,
    #[serde(default)]
    source_generation_fingerprint: Option<String>,
    canvas: RigSize,
    atlas: ImportRigAtlasRequest,
    bones: Vec<RigBone>,
    layers: Vec<RigLayerSource>,
    #[serde(default)]
    motion_profile: Option<RigMotionProfile>,
    #[serde(default)]
    outfit_profile: Option<RigOutfitProfile>,
    #[serde(default)]
    semantic_anchors: HashMap<String, RigSemanticAnchor>,
    #[serde(default)]
    semantics: Option<RigSemantics>,
    #[serde(default)]
    spatial_profile: Option<RigSpatialProfile>,
    #[serde(default)]
    anime25d_playback: Option<Value>,
}

struct ParsedRigImport {
    source: ImportRigSourceRequest,
    atlas_bytes: Vec<u8>,
    analysis_reference_bytes: Option<Vec<u8>>,
}

async fn parse_rig_import(
    mut multipart: Multipart,
    profile: CharacterAssetProfile,
) -> ApiResult<ParsedRigImport> {
    let mut source_bytes = None;
    let mut atlas_bytes = None;
    let mut analysis_reference_bytes = None;
    while let Some(field) = multipart.next_field().await.map_err(|error| {
        tracing::error!(%error, "Invalid rig import body");
        bad_request("Invalid rig import")
    })? {
        match field.name() {
            Some("source") if source_bytes.is_none() => {
                let bytes = field.bytes().await.map_err(|error| {
                    tracing::error!(%error, "Invalid rig source");
                    bad_request("Invalid rig import")
                })?;
                if bytes.len() > MAX_RIG_IMPORT_SOURCE_BYTES {
                    return Err(bad_request("Rig source exceeds 2 MB"));
                }
                source_bytes = Some(bytes);
            }
            Some("atlas") if atlas_bytes.is_none() => {
                if field.content_type() != Some("image/png") {
                    return Err(bad_request("Rig atlas must be a PNG image"));
                }
                let bytes = field.bytes().await.map_err(|error| {
                    tracing::error!(%error, "Invalid rig atlas");
                    bad_request("Invalid rig import")
                })?;
                if bytes.len() > MAX_RIG_IMPORT_ATLAS_BYTES {
                    return Err(bad_request("Rig atlas exceeds 20 MB"));
                }
                atlas_bytes = Some(bytes.to_vec());
            }
            Some("analysisReference") if analysis_reference_bytes.is_none() => {
                if field.content_type() != Some("image/png") {
                    return Err(bad_request("Rig analysis reference must be a PNG image"));
                }
                let bytes = field.bytes().await.map_err(|error| {
                    tracing::error!(%error, "Invalid rig analysis reference");
                    bad_request("Invalid rig import")
                })?;
                if bytes.len() > MAX_RIG_ANALYSIS_REFERENCE_BYTES {
                    return Err(bad_request("Rig analysis reference exceeds 10 MB"));
                }
                analysis_reference_bytes = Some(bytes.to_vec());
            }
            Some("source" | "atlas" | "analysisReference") => {
                return Err(bad_request("Rig import fields must not be duplicated"));
            }
            _ => return Err(bad_request("Rig import contains an unsupported field")),
        }
    }
    let mut source: ImportRigSourceRequest = serde_json::from_slice(
        source_bytes
            .as_deref()
            .ok_or_else(|| bad_request("Rig import is missing source metadata"))?,
    )
    .map_err(|error| {
        tracing::error!(%error, "Invalid rig source metadata");
        bad_request("Invalid rig import")
    })?;
    // A bust import never lands in the full-body slot, nor the reverse.
    if source.profile != profile {
        return Err(bad_request("Rig import is for another asset profile"));
    }
    if let Some(fingerprint) = &mut source.source_generation_fingerprint {
        if !valid_generation_fingerprint(fingerprint) {
            return Err(bad_request(
                "Rig source generation fingerprint must be a SHA-256 hex digest",
            ));
        }
        fingerprint.make_ascii_lowercase();
    }
    let atlas_bytes = atlas_bytes.ok_or_else(|| bad_request("Rig import is missing atlas PNG"))?;
    let (atlas_bytes, dimensions) = merope_rig::validate_png_dimensions(atlas_bytes)
        .await
        .map_err(png_validation_error)?;
    if dimensions != (source.atlas.width, source.atlas.height) {
        return Err(bad_request(
            "Rig atlas dimensions do not match source metadata",
        ));
    }
    if source.atlas.id.trim().is_empty()
        || source
            .layers
            .iter()
            .any(|layer| layer.texture_id != source.atlas.id)
    {
        return Err(bad_request("Rig atlas contract is invalid"));
    }
    let analysis_reference_bytes = if let Some(reference) = analysis_reference_bytes {
        Some(
            merope_rig::validate_png_dimensions(reference)
                .await
                .map_err(png_validation_error)?
                .0,
        )
    } else {
        None
    };
    Ok(ParsedRigImport {
        source,
        atlas_bytes,
        analysis_reference_bytes,
    })
}

pub(super) fn png_validation_error(error: merope_rig::PngValidationError) -> ApiError {
    match error {
        merope_rig::PngValidationError::Invalid(message) => bad_request(&message),
        merope_rig::PngValidationError::Busy => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json(
                "Rig image validation is busy; retry shortly",
            )),
        ),
        merope_rig::PngValidationError::WorkerFailed => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AppError::public_json("Rig image validation failed")),
        ),
    }
}

pub async fn preview_site_rig(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    multipart: Multipart,
) -> ApiResult<Json<Value>> {
    preview_rig(&db, &claims, multipart, MasterSlot::WornBust).await
}

/// Compiles an import of a master portrait without keeping it.
pub(super) async fn preview_rig(
    db: &DatabaseConnection,
    claims: &Claims,
    multipart: Multipart,
    slot: MasterSlot<'_>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    let user_id = require_owner(claims, db).await?;
    let ParsedRigImport {
        mut source,
        analysis_reference_bytes,
        ..
    } = parse_rig_import(multipart, slot.profile()).await?;
    let analysis_reference_bytes = analysis_reference_bytes
        .ok_or_else(|| bad_request("Rig preview is missing its analysis reference"))?;
    let master = require_master_match(
        db,
        slot,
        &source.source_master_asset_id,
        source.source_generation_fingerprint.as_deref(),
    )
    .await?;
    prepare_import_chest_profile(
        user_id,
        &mut source,
        &master.gender,
        true,
        Some(&analysis_reference_bytes),
    )
    .await;
    // The moving parts are looked for after the import, off its path (full_body).
    prepare_import_part_profile(&mut source);
    let (_, manifest) = compile_imported_rig(source, "preview://rig-atlas".to_owned()).await?;
    Ok(Json(json!({ "manifest": manifest, "persisted": false })))
}

pub async fn import_site_rig(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    multipart: Multipart,
) -> ApiResult<Json<Value>> {
    let imported = import_rig(&db, &claims, multipart, MasterSlot::WornBust).await?;
    bind_and_activate_outfit_rig(
        &db,
        imported.user_id,
        &imported.asset_id,
        &imported.master,
        None,
    )
    .await?;
    spawn_part_inventory(&db, &imported, None);
    Ok(Json(
        json!({ "manifest": imported.manifest, "assetId": imported.asset_id }),
    ))
}

/// Looks for the moving parts of a rig just imported and bound, in the
/// background, when the import brought none along (rig_part_analysis):
/// `outfit` names a full-body set, none the worn bust.
pub(super) fn spawn_part_inventory(
    db: &DatabaseConnection,
    imported: &ImportedRig,
    outfit: Option<String>,
) {
    let found = imported
        .manifest
        .anime25d_playback
        .as_ref()
        .is_some_and(rig_part_analysis::has_inventory);
    let Some(reference) = imported.analysis_reference.clone().filter(|_| !found) else {
        return;
    };
    tokio::spawn(find_moving_parts(
        db.clone(),
        imported.user_id,
        outfit,
        imported.asset_id.clone(),
        imported.master.clone(),
        reference,
    ));
}

/// The rig's moving parts, looked for after its import so a slow vision
/// model never holds the import up, then saved into it as a new package,
/// which takes over only while the imported one is still the slot's.
pub(super) async fn find_moving_parts(
    db: DatabaseConnection,
    user_id: i32,
    outfit: Option<String>,
    asset_id: String,
    master: MasterProvenance,
    reference: Vec<u8>,
) {
    let Ok(reference) = image_generation::ImageReference::new(reference, "image/png") else {
        return;
    };
    let Ok(manifest) = load_stored_manifest(&asset_id).await else {
        return;
    };
    let Some(playback) = manifest.anime25d_playback.as_ref() else {
        return;
    };
    let Some(profile) = crate::services::ai_cost_ledger::with_site_ai_ledger(
        user_id,
        "merope",
        "rig-part-analysis",
        rig_part_analysis::inventory(playback, &reference),
    )
    .await
    else {
        return;
    };
    let parts = profile["parts"].as_array().map_or(0, Vec::len);
    let saved = match rewrite_playback(&asset_id, &master, |playback| {
        playback
            .as_object_mut()
            .ok_or_else(|| bad_request("Anime2.5D playback is missing"))?
            .insert("partProfile".to_string(), profile);
        Ok(())
    })
    .await
    {
        Ok((_, saved)) => saved,
        Err((status, _)) => {
            tracing::warn!(%status, ?outfit, "moving parts could not be saved into the rig");
            return;
        }
    };
    let bound = match &outfit {
        Some(id) => bind_full_body_rig(&db, user_id, id, &saved, &master, Some(&asset_id))
            .await
            .map(|_| ()),
        None => bind_and_activate_outfit_rig(&db, user_id, &saved, &master, Some(&asset_id)).await,
    };
    match bound {
        Ok(()) => tracing::info!(?outfit, parts, "moving parts found and saved into the rig"),
        Err((status, _)) => {
            tracing::warn!(%status, ?outfit, "the rig changed before its moving parts were saved")
        }
    }
}

/// A compiled, stored package not yet bound to the outfit it was made for.
pub(super) struct ImportedRig {
    pub(super) user_id: i32,
    pub(super) master: MasterProvenance,
    pub(super) manifest: RigManifest,
    pub(super) asset_id: String,
    /// The rig at rest as the page composed it, when sent: what the vision model looks at.
    pub(super) analysis_reference: Option<Vec<u8>>,
}

/// Compiles and stores an import of a master portrait.
pub(super) async fn import_rig(
    db: &DatabaseConnection,
    claims: &Claims,
    multipart: Multipart,
    slot: MasterSlot<'_>,
) -> ApiResult<ImportedRig> {
    require_merope_enabled().await?;
    let user_id = require_owner(claims, db).await?;
    let ParsedRigImport {
        mut source,
        atlas_bytes,
        analysis_reference_bytes,
    } = parse_rig_import(multipart, slot.profile()).await?;
    let source_master_asset_id = source.source_master_asset_id.clone();
    let source_generation_fingerprint = source.source_generation_fingerprint.clone();
    let master = require_master_match(
        db,
        slot,
        &source_master_asset_id,
        source_generation_fingerprint.as_deref(),
    )
    .await?;
    prepare_import_chest_profile(user_id, &mut source, &master.gender, false, None).await;
    prepare_import_part_profile(&mut source);
    let (_, manifest) = compile_imported_rig(source, "asset://atlas".to_owned()).await?;
    let asset_id =
        merope_rig::package_id_for_manifest(&atlas_bytes, &manifest).map_err(internal_error)?;
    let manifest = rewrite_texture_urls(manifest, &asset_id);
    let json = serde_json::to_string_pretty(&manifest).map_err(internal_error)?;
    merope_rig::persist_package(&asset_id, &atlas_bytes, &json)
        .await
        .map_err(internal_error)?;
    Ok(ImportedRig {
        user_id,
        master,
        manifest,
        asset_id,
        analysis_reference: analysis_reference_bytes,
    })
}
