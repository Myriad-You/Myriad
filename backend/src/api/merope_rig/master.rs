//! 立绘血统：装配包只认生成它的那张立绘和那份生成契约。

use axum::{Json, http::StatusCode};
use myriad_merope::{
    CHARACTER_ASSET_CONTRACT_VERSION, PORTRAIT_CANVAS_HEIGHT, PORTRAIT_CANVAS_WIDTH, RigManifest,
    build_character_asset_contract, character_asset_contract_fingerprint,
};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use super::{ApiResult, internal_error, not_found};
use crate::services::agent::merope;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MasterProvenance {
    pub(super) asset_id: String,
    pub(super) generation_fingerprint: Option<String>,
    pub(super) gender: String,
    pub(super) outfit_id: Option<String>,
}

pub(super) fn valid_generation_fingerprint(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|character| character.is_ascii_hexdigit())
}

pub(super) fn portrait_generation_fingerprint(
    name: &str,
    visual_profile: &Value,
    value: Option<&Value>,
) -> ApiResult<Option<String>> {
    let Some(document) = value else {
        return Ok(None);
    };
    let Some(fingerprint) = document.get("fingerprint").and_then(Value::as_str) else {
        return if document.get("contract").is_none() && document.get("pending").is_some() {
            Ok(None)
        } else {
            Err(internal_error(
                "stored portrait generation contract is invalid",
            ))
        };
    };
    if !valid_generation_fingerprint(fingerprint) {
        return Err(internal_error(
            "stored portrait generation fingerprint is invalid",
        ));
    }
    let contract = document
        .get("contract")
        .ok_or_else(|| internal_error("stored portrait generation contract is missing"))?;
    if character_asset_contract_fingerprint(contract) != fingerprint.to_ascii_lowercase() {
        return Err(internal_error(
            "stored portrait generation fingerprint does not match its contract",
        ));
    }
    let additional_requirements = contract
        .get("additionalRequirements")
        .and_then(Value::as_str);
    let expected = build_character_asset_contract(name, visual_profile, additional_requirements);
    if contract != &expected {
        tracing::warn!(
            "stored portrait generation contract does not match the current visual identity; serving portrait without fingerprint"
        );
        return Ok(None);
    }
    Ok(Some(fingerprint.to_ascii_lowercase()))
}

pub(super) async fn current_master(db: &DatabaseConnection) -> ApiResult<Option<MasterProvenance>> {
    let persona = merope::get_persona(db).await.map_err(internal_error)?;
    let Some(persona) = persona else {
        return Ok(None);
    };
    Ok(master_from_persona(&persona))
}

pub(super) fn master_from_persona(
    persona: &crate::models::entities::agent_persona::Model,
) -> Option<MasterProvenance> {
    let gender = persona
        .visual_profile
        .as_ref()
        .and_then(|profile| profile.get("gender"))
        .and_then(Value::as_str)
        .filter(|value| matches!(*value, "female" | "male" | "nonbinary" | "unspecified"))
        .unwrap_or("unspecified")
        .to_string();
    let asset_id = persona.portrait_asset_id.clone()?;
    Some(MasterProvenance {
        asset_id,
        generation_fingerprint: myriad_merope::active_outfit_generation_fingerprint(
            persona.visual_profile.as_ref(),
        )
        .or_else(|| {
            portrait_generation_fingerprint(
                persona.name.trim(),
                persona.visual_profile.as_ref().unwrap_or(&Value::Null),
                persona.portrait_generation.as_ref(),
            )
            .unwrap_or(None)
        }),
        gender,
        outfit_id: persona
            .visual_profile
            .as_ref()
            .and_then(|profile| profile.get("activeOutfitId"))
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

pub(super) async fn require_master_match(
    db: &DatabaseConnection,
    source_master_asset_id: &str,
    source_generation_fingerprint: Option<&str>,
) -> ApiResult<MasterProvenance> {
    let stored = current_master(db)
        .await?
        .ok_or_else(|| not_found("Site portrait is missing"))?;
    let supplied_fingerprint = source_generation_fingerprint.map(str::to_ascii_lowercase);
    if stored.asset_id != source_master_asset_id
        || stored.generation_fingerprint != supplied_fingerprint
    {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Character master asset or generation contract changed before rig import",
                "code": "character_asset_provenance_changed"
            })),
        ));
    }
    Ok(stored)
}

pub(super) fn manifest_matches_master(manifest: &RigManifest, master: &MasterProvenance) -> bool {
    manifest.validate().is_ok()
        && manifest.character_asset_contract_version == Some(CHARACTER_ASSET_CONTRACT_VERSION)
        && manifest.source_master_asset_id.as_deref() == Some(master.asset_id.as_str())
        && manifest.source_generation_fingerprint == master.generation_fingerprint
        && (manifest.canvas.width - PORTRAIT_CANVAS_WIDTH).abs() <= 0.0001
        && (manifest.canvas.height - PORTRAIT_CANVAS_HEIGHT).abs() <= 0.0001
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use myriad_merope::{
        RIG_SCHEMA_VERSION, RigBone, RigPart, RigPoint, RigQuality, RigSize, RigTexture, RigVertex,
    };

    use super::*;

    #[test]
    fn activation_anchor_tracks_outfit_even_when_portrait_is_shared() {
        let mut persona = crate::models::entities::agent_persona::Model {
            id: "site".into(),
            name: "Merope".into(),
            personality: String::new(),
            persona_json: None,
            visual_profile: Some(json!({"activeOutfitId": "a", "gender": "female"})),
            portrait_asset_id: Some("master-a".into()),
            portrait_generation: None,
            avatar_asset_id: None,
            avatar_generation: None,
            updated_by: None,
            updated_at: chrono::Utc::now().fixed_offset(),
        };
        let original = master_from_persona(&persona).unwrap();
        persona.name = "Renamed".into();
        assert_eq!(master_from_persona(&persona).as_ref(), Some(&original));
        persona.visual_profile.as_mut().unwrap()["activeOutfitId"] = json!("b");
        assert_ne!(master_from_persona(&persona).as_ref(), Some(&original));
        persona.visual_profile.as_mut().unwrap()["activeOutfitId"] = json!("a");
        persona.portrait_asset_id = Some("master-b".into());
        assert_ne!(master_from_persona(&persona).as_ref(), Some(&original));
        persona.portrait_asset_id = None;
        assert!(master_from_persona(&persona).is_none());
    }

    fn layered_stub_manifest(
        master_url: &str,
        source_generation_fingerprint: Option<String>,
    ) -> RigManifest {
        RigManifest {
            schema_version: RIG_SCHEMA_VERSION,
            rig_ir_version: None,
            character_asset_contract_version: Some(CHARACTER_ASSET_CONTRACT_VERSION),
            source_master_asset_id: Some(master_url.to_string()),
            source_generation_fingerprint,
            quality: RigQuality::Layered2d,
            canvas: RigSize {
                width: PORTRAIT_CANVAS_WIDTH,
                height: PORTRAIT_CANVAS_HEIGHT,
            },
            textures: vec![RigTexture {
                id: "atlas".to_string(),
                url: master_url.to_string(),
                width: 64,
                height: 64,
            }],
            bones: vec![RigBone {
                id: "root".to_string(),
                parent: None,
                pivot: RigPoint { x: 0.5, y: 0.8 },
            }],
            parts: vec![RigPart {
                id: "portrait".to_string(),
                texture_id: "atlas".to_string(),
                z_index: 0,
                opacity: 1.0,
                slot: None,
                variant: None,
                vertices: vec![
                    RigVertex {
                        position: RigPoint { x: 0.0, y: 0.0 },
                        uv: RigPoint { x: 0.0, y: 0.0 },
                        joints: [0, 0, 0, 0],
                        weights: [1.0, 0.0, 0.0, 0.0],
                    },
                    RigVertex {
                        position: RigPoint { x: 1.0, y: 0.0 },
                        uv: RigPoint { x: 1.0, y: 0.0 },
                        joints: [0, 0, 0, 0],
                        weights: [1.0, 0.0, 0.0, 0.0],
                    },
                    RigVertex {
                        position: RigPoint { x: 0.0, y: 1.0 },
                        uv: RigPoint { x: 0.0, y: 1.0 },
                        joints: [0, 0, 0, 0],
                        weights: [1.0, 0.0, 0.0, 0.0],
                    },
                ],
                indices: vec![0, 1, 2],
            }],
            motion_profile: None,
            outfit_profile: None,
            semantic_anchors: HashMap::new(),
            semantics: None,
            spatial_profile: None,
            anime25d_playback: None,
        }
    }

    #[test]
    fn active_manifest_must_match_master_contract_and_generation() {
        let fingerprint = "a".repeat(64);
        let master = MasterProvenance {
            asset_id: "/master.png".to_string(),
            generation_fingerprint: Some(fingerprint.clone()),
            gender: "female".to_string(),
            outfit_id: Some("default".into()),
        };
        let mut manifest = layered_stub_manifest("/master.png", Some(fingerprint.clone()));
        assert!(manifest_matches_master(&manifest, &master));

        manifest.source_generation_fingerprint = Some("b".repeat(64));
        assert!(!manifest_matches_master(&manifest, &master));
        manifest.source_generation_fingerprint = master.generation_fingerprint.clone();
        manifest.canvas.height = 1.0;
        assert!(!manifest_matches_master(&manifest, &master));
    }

    #[test]
    fn stored_portrait_contract_is_bound_to_its_fingerprint_and_current_identity() {
        let profile = json!({ "gender": "unspecified" });
        let contract = build_character_asset_contract("Nova", &profile, Some("soft morning light"));
        let fingerprint = character_asset_contract_fingerprint(&contract);
        let document = json!({
            "fingerprint": fingerprint,
            "contract": contract,
            "pending": { "token": "next-generation" }
        });
        assert_eq!(
            portrait_generation_fingerprint("Nova", &profile, Some(&document)).unwrap(),
            Some(fingerprint)
        );
        assert_eq!(
            portrait_generation_fingerprint(
                "Nova",
                &json!({ "gender": "female" }),
                Some(&document),
            )
            .unwrap(),
            None
        );

        let mut tampered = document;
        tampered["contract"]["additionalRequirements"] = json!("different light");
        assert!(portrait_generation_fingerprint("Nova", &profile, Some(&tampered)).is_err());
        assert_eq!(
            portrait_generation_fingerprint(
                "Nova",
                &profile,
                Some(&json!({ "pending": { "token": "first-generation" } })),
            )
            .unwrap(),
            None
        );
    }
}
