use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use crate::rig_contract::{
    CHARACTER_ASSET_CONTRACT_VERSION, MAX_RIG_BONES, MAX_RIG_PARTS, MAX_RIG_TEXTURES,
    MAX_RIG_TOTAL_VERTICES, MAX_RIG_VERTICES_PER_PART, MIN_SUPPORTED_RIG_IR_VERSION,
    PORTRAIT_CANVAS_HEIGHT, PORTRAIT_CANVAS_WIDTH, RIG_IR_VERSION, RIG_SCHEMA_VERSION,
};
use crate::rig_contract::{CHARACTER_ASSET_REQUIRED_CAPABILITIES, PRESENTATION_SLOT_VARIANTS};
use crate::rig_outfit::{
    default_semantic_anchors, outfit_profile_is_valid, semantic_anchors_are_valid,
};
pub use crate::rig_outfit::{infer_outfit_profile, RigOutfitProfile, RigSemanticAnchor};
pub use crate::rig_semantics::RigSemantics;
use crate::rig_semantics::{default_rig_semantics, migrate_rig_semantics, rig_semantics_are_valid};
use crate::rig_spatial::{infer_spatial_profile, spatial_profile_is_valid, RigSpatialProfile};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigSize {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigPoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RigQuality {
    #[serde(rename = "layered-2d")]
    Layered2d,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigTexture {
    pub id: String,
    pub url: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigBone {
    pub id: String,
    pub parent: Option<String>,
    pub pivot: RigPoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigVertex {
    pub position: RigPoint,
    pub uv: RigPoint,
    pub joints: [u8; 4],
    pub weights: [f32; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigPart {
    pub id: String,
    pub texture_id: String,
    pub z_index: i16,
    pub opacity: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    pub vertices: Vec<RigVertex>,
    pub indices: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigBreathMotionProfile {
    pub min_frequency_hz: f32,
    pub max_frequency_hz: f32,
    pub amplitude: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigBlinkMotionProfile {
    pub min_interval_seconds: f32,
    pub max_interval_seconds: f32,
    pub duration_seconds: f32,
    pub double_chance: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigSecondaryMotionProfile {
    pub enabled: bool,
    pub frequency_hz: f32,
    pub damping_ratio: f32,
    pub response: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigMotionProfile {
    pub seed: u32,
    pub breath: RigBreathMotionProfile,
    pub blink: RigBlinkMotionProfile,
    pub secondary: RigSecondaryMotionProfile,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigManifest {
    pub schema_version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rig_ir_version: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_asset_contract_version: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_master_asset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_generation_fingerprint: Option<String>,
    pub quality: RigQuality,
    pub canvas: RigSize,
    pub textures: Vec<RigTexture>,
    pub bones: Vec<RigBone>,
    pub parts: Vec<RigPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion_profile: Option<RigMotionProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outfit_profile: Option<RigOutfitProfile>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub semantic_anchors: HashMap<String, RigSemanticAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantics: Option<RigSemantics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spatial_profile: Option<RigSpatialProfile>,
    /// Warp/stencil playback document replicated from Anime2.5DRig (MIT).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anime25d_playback: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigBoneHandle {
    pub bone_id: String,
    pub start: RigPoint,
    pub end: RigPoint,
    pub falloff: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigLayerMeshSource {
    pub vertices: Vec<RigPoint>,
    pub indices: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigLayerSource {
    pub id: String,
    pub texture_id: String,
    pub texture_bounds: RigRect,
    pub z_index: i16,
    pub opacity: f32,
    #[serde(default)]
    pub slot: Option<String>,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub contours: Vec<Vec<RigPoint>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<RigLayerMeshSource>,
    pub bone_handles: Vec<RigBoneHandle>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RigCompileSource {
    pub rig_ir_version: Option<u16>,
    pub character_asset_contract_version: Option<u16>,
    pub source_master_asset_id: Option<String>,
    pub source_generation_fingerprint: Option<String>,
    pub canvas: RigSize,
    pub textures: Vec<RigTexture>,
    pub bones: Vec<RigBone>,
    pub layers: Vec<RigLayerSource>,
    pub motion_profile: Option<RigMotionProfile>,
    pub outfit_profile: Option<RigOutfitProfile>,
    pub semantic_anchors: HashMap<String, RigSemanticAnchor>,
    pub semantics: Option<RigSemantics>,
    pub spatial_profile: Option<RigSpatialProfile>,
    pub anime25d_playback: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum RigValidationError {
    #[error("unsupported rig schema version")]
    SchemaVersion,
    #[error("rig canvas is invalid")]
    Canvas,
    #[error("character asset contract is invalid")]
    AssetContract,
    #[error("rig contains too many bones")]
    TooManyBones,
    #[error("rig identifier is empty or duplicated")]
    Identifier,
    #[error("rig bone hierarchy is invalid")]
    BoneHierarchy,
    #[error("rig mesh is invalid")]
    Mesh,
    #[error("rig skin weights are invalid")]
    SkinWeights,
    #[error("rig motion profile is invalid")]
    MotionProfile,
    #[error("rig outfit profile is invalid")]
    OutfitProfile,
    #[error("rig semantic anchors are invalid")]
    SemanticAnchors,
    #[error("rig semantic IR is invalid")]
    Semantics,
    #[error("rig spatial profile is invalid")]
    SpatialProfile,
    #[error("Anime2.5D playback contract is invalid")]
    Anime25DPlayback,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum RigCompileError {
    #[error("rig source IR version is not current")]
    IrVersion,
    #[error("rig source character asset contract is not current")]
    AssetContract,
    #[error("rig source contour is invalid")]
    Contour,
    #[error("rig source texture bounds are invalid")]
    TextureBounds,
    #[error("rig source bone handle is invalid")]
    BoneHandle,
    #[error("rig source mesh exceeds index limits")]
    MeshTooLarge,
    #[error("rig source exceeds complexity limits")]
    Complexity,
    #[error("rig source triangulation failed")]
    Triangulation,
    #[error(transparent)]
    Validation(#[from] RigValidationError),
}

impl RigManifest {
    pub fn validate(&self) -> Result<(), RigValidationError> {
        if self.schema_version != RIG_SCHEMA_VERSION {
            return Err(RigValidationError::SchemaVersion);
        }
        if self.rig_ir_version.is_some_and(|version| {
            !(MIN_SUPPORTED_RIG_IR_VERSION..=RIG_IR_VERSION).contains(&version)
        }) {
            return Err(RigValidationError::Semantics);
        }
        if self
            .character_asset_contract_version
            .is_some_and(|version| version != CHARACTER_ASSET_CONTRACT_VERSION)
            || self
                .source_master_asset_id
                .as_deref()
                .is_some_and(|value| value.trim().is_empty() || value.len() > 512)
            || self
                .source_generation_fingerprint
                .as_deref()
                .is_some_and(|value| {
                    value.len() != 64
                        || !value.chars().all(|character| character.is_ascii_hexdigit())
                })
        {
            return Err(RigValidationError::AssetContract);
        }
        if !self.canvas.width.is_finite()
            || !self.canvas.height.is_finite()
            || self.canvas.width <= 0.0
            || self.canvas.height <= 0.0
        {
            return Err(RigValidationError::Canvas);
        }
        if self
            .anime25d_playback
            .as_ref()
            .is_some_and(|playback| !crate::anime25d_contract::playback_is_valid(playback))
        {
            return Err(RigValidationError::Anime25DPlayback);
        }
        if self.bones.is_empty() || self.bones.len() > MAX_RIG_BONES {
            return Err(RigValidationError::TooManyBones);
        }
        if self.textures.is_empty()
            || self.textures.len() > MAX_RIG_TEXTURES
            || self.parts.is_empty()
            || self.parts.len() > MAX_RIG_PARTS
            || self
                .parts
                .iter()
                .map(|part| part.vertices.len())
                .sum::<usize>()
                > MAX_RIG_TOTAL_VERTICES
        {
            return Err(RigValidationError::Mesh);
        }

        let texture_ids = unique_ids(self.textures.iter().map(|texture| texture.id.as_str()))?;
        let bone_ids = unique_ids(self.bones.iter().map(|bone| bone.id.as_str()))?;
        unique_ids(self.parts.iter().map(|part| part.id.as_str()))?;
        if self.textures.iter().any(|texture| {
            texture.url.trim().is_empty()
                || texture.width == 0
                || texture.height == 0
                || texture.width > 16_384
                || texture.height > 16_384
        }) {
            return Err(RigValidationError::Mesh);
        }

        for bone in &self.bones {
            if !point_is_finite(bone.pivot)
                || bone
                    .parent
                    .as_deref()
                    .is_some_and(|parent| parent == bone.id || !bone_ids.contains(parent))
            {
                return Err(RigValidationError::BoneHierarchy);
            }
        }
        validate_bone_cycles(&self.bones)?;

        for part in &self.parts {
            if !texture_ids.contains(part.texture_id.as_str())
                || part.vertices.len() < 3
                || part.indices.len() < 3
                || part.indices.len() % 3 != 0
                || part.vertices.len() > MAX_RIG_VERTICES_PER_PART
                || !part.opacity.is_finite()
                || !(0.0..=1.0).contains(&part.opacity)
                || part.slot.is_some() != part.variant.is_some()
                || part
                    .slot
                    .as_deref()
                    .is_some_and(|value| value.trim().is_empty() || value.len() > 64)
                || part
                    .variant
                    .as_deref()
                    .is_some_and(|value| value.trim().is_empty() || value.len() > 64)
                || part
                    .indices
                    .iter()
                    .any(|index| usize::from(*index) >= part.vertices.len())
            {
                return Err(RigValidationError::Mesh);
            }
            for vertex in &part.vertices {
                if !point_is_finite(vertex.position)
                    || !point_is_finite(vertex.uv)
                    || vertex
                        .joints
                        .iter()
                        .any(|joint| usize::from(*joint) >= self.bones.len())
                    || vertex
                        .weights
                        .iter()
                        .any(|weight| !weight.is_finite() || *weight < 0.0 || *weight > 1.0)
                    || (vertex.weights.iter().sum::<f32>() - 1.0).abs() > 0.002
                {
                    return Err(RigValidationError::SkinWeights);
                }
            }
        }
        if self.rig_ir_version.unwrap_or(0) >= 3 {
            validate_presentation_parts(&self.parts)?;
        }

        if self
            .motion_profile
            .as_ref()
            .is_some_and(|profile| !motion_profile_is_valid(profile))
        {
            return Err(RigValidationError::MotionProfile);
        }
        if self
            .outfit_profile
            .as_ref()
            .is_some_and(|profile| !outfit_profile_is_valid(profile, &self.parts))
        {
            return Err(RigValidationError::OutfitProfile);
        }
        if !semantic_anchors_are_valid(&self.semantic_anchors, &self.bones, self.canvas) {
            return Err(RigValidationError::SemanticAnchors);
        }
        if self
            .semantics
            .as_ref()
            .is_some_and(|semantics| !rig_semantics_are_valid(semantics, &self.bones))
        {
            return Err(RigValidationError::Semantics);
        }
        if self.rig_ir_version.is_some()
            && (self.semantics.is_none() || self.spatial_profile.is_none())
        {
            return Err(RigValidationError::Semantics);
        }
        if self
            .spatial_profile
            .as_ref()
            .is_some_and(|profile| !spatial_profile_is_valid(profile, &self.bones, self.canvas))
        {
            return Err(RigValidationError::SpatialProfile);
        }
        Ok(())
    }
}

fn validate_presentation_parts(parts: &[RigPart]) -> Result<(), RigValidationError> {
    let mut variants_by_slot = HashMap::<&str, HashSet<&str>>::new();
    for part in parts {
        let (Some(slot), Some(variant)) = (part.slot.as_deref(), part.variant.as_deref()) else {
            continue;
        };
        let Some((_, _, allowed)) = PRESENTATION_SLOT_VARIANTS
            .iter()
            .find(|(candidate, _, _)| *candidate == slot)
        else {
            return Err(RigValidationError::Mesh);
        };
        if !allowed.contains(&variant) {
            return Err(RigValidationError::Mesh);
        }
        variants_by_slot.entry(slot).or_default().insert(variant);
    }
    for (slot, variants) in variants_by_slot {
        let fallback = PRESENTATION_SLOT_VARIANTS
            .iter()
            .find(|(candidate, _, _)| *candidate == slot)
            .map(|(_, fallback, _)| *fallback)
            .ok_or(RigValidationError::Mesh)?;
        if !variants.contains(fallback) {
            return Err(RigValidationError::Mesh);
        }
    }
    Ok(())
}

pub fn default_rig_motion_profile(seed: u32) -> RigMotionProfile {
    RigMotionProfile {
        seed: seed.max(1),
        breath: RigBreathMotionProfile {
            min_frequency_hz: 0.16,
            max_frequency_hz: 0.27,
            amplitude: 0.0036,
        },
        blink: RigBlinkMotionProfile {
            min_interval_seconds: 2.7,
            max_interval_seconds: 6.8,
            duration_seconds: 0.24,
            double_chance: 0.16,
        },
        secondary: RigSecondaryMotionProfile {
            enabled: true,
            frequency_hz: 2.15,
            damping_ratio: 0.52,
            response: 0.58,
        },
    }
}

pub fn migrate_rig_manifest(
    mut manifest: RigManifest,
    seed: u32,
) -> Result<(RigManifest, bool), RigValidationError> {
    let mut changed = false;
    if manifest.motion_profile.is_none() {
        manifest.motion_profile = Some(default_rig_motion_profile(seed));
        changed = true;
    }
    if manifest.outfit_profile.is_none() {
        manifest.outfit_profile = Some(infer_outfit_profile(&manifest.parts));
        changed = true;
    }
    let secondary = manifest
        .outfit_profile
        .as_ref()
        .map(|profile| profile.secondary_part_ids.as_slice())
        .unwrap_or_default();
    let migrated_semantics =
        migrate_rig_semantics(manifest.semantics.as_ref(), &manifest.bones, secondary);
    if manifest.semantics.as_ref() != Some(&migrated_semantics) {
        manifest.semantics = Some(migrated_semantics);
        changed = true;
    }
    if manifest.spatial_profile.is_none() {
        manifest.spatial_profile = Some(infer_spatial_profile(
            manifest.canvas,
            &manifest.bones,
            &manifest.parts,
            manifest.semantics.as_ref(),
        ));
        changed = true;
    }
    if manifest.rig_ir_version != Some(RIG_IR_VERSION)
        && validate_presentation_parts(&manifest.parts).is_ok()
    {
        manifest.rig_ir_version = Some(RIG_IR_VERSION);
        changed = true;
    }
    if manifest.semantic_anchors.is_empty() {
        manifest.semantic_anchors =
            default_semantic_anchors(&manifest.bones, manifest.semantics.as_ref());
        changed = true;
    }
    manifest.validate()?;
    Ok((manifest, changed))
}

/// Authoritative server-side gate for the current upper-body character asset.
/// Browser PSD preflight improves feedback, but cannot be the trust boundary.
pub fn validate_character_asset_source(
    bones: &[RigBone],
    layers: &[RigLayerSource],
) -> Result<(), RigValidationError> {
    let has_layer = |prefix: &str| {
        layers.iter().any(|layer| {
            layer.id == prefix
                || layer
                    .id
                    .strip_prefix(prefix)
                    .is_some_and(|suffix| suffix.starts_with('-'))
        })
    };
    let has_variant = |slot: &str, variant: &str| {
        layers.iter().any(|layer| {
            layer.slot.as_deref() == Some(slot) && layer.variant.as_deref() == Some(variant)
        })
    };
    let parent_is = |bone_id: &str, parent_id: &str| {
        bones
            .iter()
            .any(|bone| bone.id == bone_id && bone.parent.as_deref() == Some(parent_id))
    };
    let rigid_fragment = |side: &str| {
        let layer_id = format!("a25d-handwear-{side}");
        layers.iter().any(|layer| {
            layer.id == layer_id
                && layer.bone_handles.len() == 1
                && layer.bone_handles[0].bone_id == layer_id
        }) && parent_is(&layer_id, "a25d-handwear")
    };
    let forbidden_bone = bones.iter().any(|bone| {
        let id = bone.id.to_ascii_lowercase();
        [
            "shoulder",
            "upper-arm",
            "upper_arm",
            "elbow",
            "forearm",
            "wrist",
            "thigh",
            "knee",
            "calf",
            "ankle",
            "leg",
            "foot",
        ]
        .iter()
        .any(|segment| id.contains(segment))
    });
    let forbidden_layer = layers.iter().any(|layer| {
        let id = layer.id.to_ascii_lowercase();
        id.contains("legwear") || id.contains("footwear")
    });
    let canonical_skeleton = parent_is("body", "root")
        && parent_is("head", "body")
        && parent_is("face", "head")
        && parent_is("left-eye", "face")
        && parent_is("right-eye", "face")
        && parent_is("mouth", "face")
        && parent_is("a25d-handwear", "body");
    let has_capability = |capability: &str| match capability {
        "separate-face" => has_layer("a25d-face"),
        "independent-eyes" => has_variant("eye-left", "open") && has_variant("eye-right", "open"),
        "blink" => has_variant("eye-left", "closed") && has_variant("eye-right", "closed"),
        "dizzy-eye-variant" => {
            has_variant("eye-left", "dizzy") && has_variant("eye-right", "dizzy")
        }
        "squeeze-eye-variant" => {
            has_variant("eye-left", "squeeze") && has_variant("eye-right", "squeeze")
        }
        "cry-eye-variant" => has_variant("eye-left", "cry") && has_variant("eye-right", "cry"),
        "silly-eye-variant" => {
            has_variant("eye-left", "silly") && has_variant("eye-right", "silly")
        }
        "lovestruck-heart-pupils" => {
            has_layer("a25d-lovestruck-heart-left") && has_layer("a25d-lovestruck-heart-right")
        }
        "lovestruck-face-effects" => {
            has_layer("a25d-lovestruck-face-effect") && has_layer("a25d-lovestruck-drool")
        }
        "cry-mouth-variant" => has_variant("mouth", "cry"),
        "maniac-mouth-variant" => has_variant("mouth", "maniac"),
        "silly-mouth-variant" => has_variant("mouth", "silly"),
        "mouth-shapes" => ["closed", "open", "wide", "round", "narrow"]
            .iter()
            .all(|variant| has_variant("mouth", variant)),
        "separate-front-hair" => has_layer("a25d-front-hair"),
        "separate-back-hair" => has_layer("a25d-back-hair"),
        "separate-topwear" => has_layer("a25d-topwear"),
        "rigid-left-arm-fragment" => rigid_fragment("left"),
        "rigid-right-arm-fragment" => rigid_fragment("right"),
        _ => false,
    };
    let required_capabilities = CHARACTER_ASSET_REQUIRED_CAPABILITIES
        .iter()
        .all(|capability| has_capability(capability));
    if canonical_skeleton && required_capabilities && !forbidden_bone && !forbidden_layer {
        Ok(())
    } else {
        Err(RigValidationError::AssetContract)
    }
}

pub fn compile_layered_rig(source: RigCompileSource) -> Result<RigManifest, RigCompileError> {
    if source
        .rig_ir_version
        .is_some_and(|version| version != RIG_IR_VERSION)
    {
        return Err(RigCompileError::IrVersion);
    }
    if source.character_asset_contract_version != Some(CHARACTER_ASSET_CONTRACT_VERSION)
        || source
            .source_master_asset_id
            .as_deref()
            .is_none_or(|value| value.trim().is_empty() || value.len() > 512)
        || source
            .source_generation_fingerprint
            .as_deref()
            .is_some_and(|value| {
                value.len() != 64 || !value.chars().all(|character| character.is_ascii_hexdigit())
            })
        || (source.canvas.width - PORTRAIT_CANVAS_WIDTH).abs() > 0.0001
        || (source.canvas.height - PORTRAIT_CANVAS_HEIGHT).abs() > 0.0001
    {
        return Err(RigCompileError::AssetContract);
    }
    if source.bones.is_empty()
        || source.bones.len() > MAX_RIG_BONES
        || source.textures.is_empty()
        || source.textures.len() > MAX_RIG_TEXTURES
        || source.layers.is_empty()
        || source.layers.len() > MAX_RIG_PARTS
        || source
            .layers
            .iter()
            .map(|layer| {
                layer
                    .mesh
                    .as_ref()
                    .map(|mesh| mesh.vertices.len())
                    .unwrap_or_else(|| layer.contours.iter().map(Vec::len).sum())
            })
            .sum::<usize>()
            > MAX_RIG_TOTAL_VERTICES
    {
        return Err(RigCompileError::Complexity);
    }
    let bone_indexes = source
        .bones
        .iter()
        .enumerate()
        .map(|(index, bone)| (bone.id.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut parts = Vec::with_capacity(source.layers.len());
    for layer in source.layers {
        let explicit_mesh_valid = layer.mesh.as_ref().is_some_and(|mesh| {
            mesh.vertices.len() >= 3
                && mesh.vertices.len() <= MAX_RIG_VERTICES_PER_PART
                && mesh.indices.len() >= 3
                && mesh.indices.len() % 3 == 0
                && mesh.vertices.iter().all(|point| point_is_finite(*point))
                && mesh
                    .indices
                    .iter()
                    .all(|index| usize::from(*index) < mesh.vertices.len())
        });
        let contours_valid = !layer.contours.is_empty()
            && layer.contours[0].len() >= 3
            && layer
                .contours
                .iter()
                .flatten()
                .all(|point| point_is_finite(*point));
        if (layer.mesh.is_some() && !explicit_mesh_valid)
            || (layer.mesh.is_none() && !contours_valid)
        {
            return Err(RigCompileError::Contour);
        }
        if !rect_is_valid(layer.texture_bounds) {
            return Err(RigCompileError::TextureBounds);
        }
        if layer.bone_handles.is_empty()
            || layer.bone_handles.iter().any(|handle| {
                !bone_indexes.contains_key(handle.bone_id.as_str())
                    || !point_is_finite(handle.start)
                    || !point_is_finite(handle.end)
                    || !handle.falloff.is_finite()
                    || handle.falloff <= 0.0
            })
        {
            return Err(RigCompileError::BoneHandle);
        }
        let (points, indices) = if let Some(mesh) = layer.mesh {
            (mesh.vertices, mesh.indices)
        } else {
            let mut points = Vec::new();
            let mut hole_indices = Vec::new();
            for (index, contour) in layer.contours.iter().enumerate() {
                if index > 0 {
                    hole_indices.push(points.len());
                }
                points.extend_from_slice(contour);
            }
            if points.len() > usize::from(u16::MAX) {
                return Err(RigCompileError::MeshTooLarge);
            }
            let coordinates = points
                .iter()
                .flat_map(|point| [f64::from(point.x), f64::from(point.y)])
                .collect::<Vec<_>>();
            let raw_indices = earcutr::earcut(&coordinates, &hole_indices, 2)
                .map_err(|_| RigCompileError::Triangulation)?;
            if raw_indices.len() < 3 {
                return Err(RigCompileError::Triangulation);
            }
            let indices = raw_indices
                .into_iter()
                .map(|index| u16::try_from(index).map_err(|_| RigCompileError::MeshTooLarge))
                .collect::<Result<Vec<_>, _>>()?;
            (points, indices)
        };
        let bounds = point_bounds(&points).ok_or(RigCompileError::Contour)?;
        let weights = compute_bounded_weights(
            &points,
            &indices,
            &layer.bone_handles,
            &bone_indexes,
            source.bones.len(),
        );
        let vertices = points
            .iter()
            .zip(weights)
            .map(|(position, influences)| RigVertex {
                position: *position,
                uv: RigPoint {
                    x: layer.texture_bounds.x
                        + ((position.x - bounds.x) / bounds.width) * layer.texture_bounds.width,
                    y: layer.texture_bounds.y
                        + ((position.y - bounds.y) / bounds.height) * layer.texture_bounds.height,
                },
                joints: influences.0,
                weights: influences.1,
            })
            .collect();
        parts.push(RigPart {
            id: layer.id,
            texture_id: layer.texture_id,
            z_index: layer.z_index,
            opacity: layer.opacity,
            slot: layer.slot,
            variant: layer.variant,
            vertices,
            indices,
        });
    }
    let outfit_profile = source
        .outfit_profile
        .or_else(|| Some(infer_outfit_profile(&parts)));
    let secondary = outfit_profile
        .as_ref()
        .map(|profile| profile.secondary_part_ids.as_slice())
        .unwrap_or_default();
    let semantics = source
        .semantics
        .or_else(|| Some(default_rig_semantics(&source.bones, secondary)));
    let semantic_anchors = if source.semantic_anchors.is_empty() {
        default_semantic_anchors(&source.bones, semantics.as_ref())
    } else {
        source.semantic_anchors
    };
    let spatial_profile = source.spatial_profile.or_else(|| {
        Some(infer_spatial_profile(
            source.canvas,
            &source.bones,
            &parts,
            semantics.as_ref(),
        ))
    });
    let manifest = RigManifest {
        schema_version: RIG_SCHEMA_VERSION,
        rig_ir_version: source.rig_ir_version.or(Some(RIG_IR_VERSION)),
        character_asset_contract_version: source.character_asset_contract_version,
        source_master_asset_id: source.source_master_asset_id,
        source_generation_fingerprint: source.source_generation_fingerprint,
        quality: RigQuality::Layered2d,
        canvas: source.canvas,
        textures: source.textures,
        bones: source.bones,
        parts,
        motion_profile: source.motion_profile,
        outfit_profile,
        semantic_anchors,
        semantics,
        spatial_profile,
        anime25d_playback: source.anime25d_playback,
    };
    manifest.validate()?;
    Ok(manifest)
}

fn compute_bounded_weights(
    points: &[RigPoint],
    indices: &[u16],
    handles: &[RigBoneHandle],
    bone_indexes: &HashMap<&str, usize>,
    bone_count: usize,
) -> Vec<([u8; 4], [f32; 4])> {
    let mut weights = points
        .iter()
        .map(|point| raw_handle_weights(*point, handles, bone_indexes, bone_count))
        .collect::<Vec<_>>();
    let adjacency = mesh_adjacency(points.len(), indices);
    let anchors = points
        .iter()
        .map(|point| {
            let nearby = handles
                .iter()
                .filter_map(|handle| {
                    let bone = *bone_indexes.get(handle.bone_id.as_str())?;
                    let distance = point_segment_distance(*point, handle.start, handle.end);
                    (distance <= handle.falloff * 0.12).then_some((bone, distance))
                })
                .collect::<Vec<_>>();
            // Pin only vertices that unambiguously belong to one handle.
            // Overlap regions are precisely where a joint needs blended
            // weights; choosing the nearest handle there made layered seams
            // rigid even when the source supplied multiple influences.
            (nearby.len() == 1).then(|| nearby[0].0)
        })
        .collect::<Vec<_>>();
    for _ in 0..32 {
        let previous = weights.clone();
        for index in 0..weights.len() {
            if let Some(anchor) = anchors[index] {
                weights[index].fill(0.0);
                weights[index][anchor] = 1.0;
                continue;
            }
            if adjacency[index].is_empty() {
                continue;
            }
            for bone in 0..bone_count {
                let neighbor_average = adjacency[index]
                    .iter()
                    .map(|neighbor| previous[*neighbor][bone])
                    .sum::<f32>()
                    / adjacency[index].len() as f32;
                weights[index][bone] =
                    (previous[index][bone] * 0.58 + neighbor_average * 0.42).clamp(0.0, 1.0);
            }
            normalize_weights(&mut weights[index]);
        }
    }
    weights
        .into_iter()
        .map(|weights| top_four_weights(&weights))
        .collect()
}

fn raw_handle_weights(
    point: RigPoint,
    handles: &[RigBoneHandle],
    bone_indexes: &HashMap<&str, usize>,
    bone_count: usize,
) -> Vec<f32> {
    let mut weights = vec![0.0; bone_count];
    for handle in handles {
        let bone = bone_indexes[handle.bone_id.as_str()];
        let distance = point_segment_distance(point, handle.start, handle.end) / handle.falloff;
        weights[bone] += 1.0 / (0.04 + distance * distance);
    }
    normalize_weights(&mut weights);
    weights
}

fn top_four_weights(weights: &[f32]) -> ([u8; 4], [f32; 4]) {
    let mut ranked = weights.iter().copied().enumerate().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.total_cmp(&left.1));
    let mut joints = [0_u8; 4];
    let mut selected = [0.0_f32; 4];
    for (slot, (bone, weight)) in ranked.into_iter().take(4).enumerate() {
        joints[slot] = u8::try_from(bone).unwrap_or(0);
        selected[slot] = weight;
    }
    let total = selected.iter().sum::<f32>();
    if total > f32::EPSILON {
        selected.iter_mut().for_each(|weight| *weight /= total);
    } else {
        selected[0] = 1.0;
    }
    (joints, selected)
}

fn mesh_adjacency(vertex_count: usize, indices: &[u16]) -> Vec<Vec<usize>> {
    let mut adjacency = vec![Vec::new(); vertex_count];
    for triangle in indices.chunks_exact(3) {
        for (from, to) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            let from = usize::from(from);
            let to = usize::from(to);
            if !adjacency[from].contains(&to) {
                adjacency[from].push(to);
            }
            if !adjacency[to].contains(&from) {
                adjacency[to].push(from);
            }
        }
    }
    adjacency
}

fn normalize_weights(weights: &mut [f32]) {
    let total = weights.iter().sum::<f32>();
    if total > f32::EPSILON {
        weights.iter_mut().for_each(|weight| *weight /= total);
    } else if let Some(first) = weights.first_mut() {
        *first = 1.0;
    }
}

fn point_segment_distance(point: RigPoint, start: RigPoint, end: RigPoint) -> f32 {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length_squared = dx * dx + dy * dy;
    if length_squared <= f32::EPSILON {
        return ((point.x - start.x).powi(2) + (point.y - start.y).powi(2)).sqrt();
    }
    let amount =
        (((point.x - start.x) * dx + (point.y - start.y) * dy) / length_squared).clamp(0.0, 1.0);
    let closest = RigPoint {
        x: start.x + dx * amount,
        y: start.y + dy * amount,
    };
    ((point.x - closest.x).powi(2) + (point.y - closest.y).powi(2)).sqrt()
}

fn point_bounds(points: &[RigPoint]) -> Option<RigRect> {
    let first = *points.first()?;
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (first.x, first.x, first.y, first.y);
    for point in &points[1..] {
        min_x = min_x.min(point.x);
        max_x = max_x.max(point.x);
        min_y = min_y.min(point.y);
        max_y = max_y.max(point.y);
    }
    let width = max_x - min_x;
    let height = max_y - min_y;
    (width > f32::EPSILON && height > f32::EPSILON).then_some(RigRect {
        x: min_x,
        y: min_y,
        width,
        height,
    })
}

fn rect_is_valid(rect: RigRect) -> bool {
    rect.x.is_finite()
        && rect.y.is_finite()
        && rect.width.is_finite()
        && rect.height.is_finite()
        && rect.width > 0.0
        && rect.height > 0.0
        && rect.x >= 0.0
        && rect.y >= 0.0
        && rect.x + rect.width <= 1.0
        && rect.y + rect.height <= 1.0
}

fn unique_ids<'a>(
    ids: impl Iterator<Item = &'a str>,
) -> Result<HashSet<&'a str>, RigValidationError> {
    let mut unique = HashSet::new();
    for id in ids {
        if id.trim().is_empty() || !unique.insert(id) {
            return Err(RigValidationError::Identifier);
        }
    }
    Ok(unique)
}

fn validate_bone_cycles(bones: &[RigBone]) -> Result<(), RigValidationError> {
    let parents = bones
        .iter()
        .map(|bone| (bone.id.as_str(), bone.parent.as_deref()))
        .collect::<HashMap<_, _>>();
    for bone in bones {
        let mut seen = HashSet::new();
        let mut current = Some(bone.id.as_str());
        while let Some(id) = current {
            if !seen.insert(id) {
                return Err(RigValidationError::BoneHierarchy);
            }
            current = parents.get(id).copied().flatten();
        }
    }
    Ok(())
}

fn point_is_finite(point: RigPoint) -> bool {
    point.x.is_finite() && point.y.is_finite()
}

fn motion_profile_is_valid(profile: &RigMotionProfile) -> bool {
    profile.breath.min_frequency_hz.is_finite()
        && (0.05..=2.0).contains(&profile.breath.min_frequency_hz)
        && profile.breath.max_frequency_hz.is_finite()
        && (profile.breath.min_frequency_hz..=2.0).contains(&profile.breath.max_frequency_hz)
        && profile.breath.amplitude.is_finite()
        && (0.0..=0.05).contains(&profile.breath.amplitude)
        && profile.blink.min_interval_seconds.is_finite()
        && (0.5..=30.0).contains(&profile.blink.min_interval_seconds)
        && profile.blink.max_interval_seconds.is_finite()
        && (profile.blink.min_interval_seconds..=30.0).contains(&profile.blink.max_interval_seconds)
        && profile.blink.duration_seconds.is_finite()
        && (0.05..=1.0).contains(&profile.blink.duration_seconds)
        && profile.blink.double_chance.is_finite()
        && (0.0..=1.0).contains(&profile.blink.double_chance)
        && profile.secondary.frequency_hz.is_finite()
        && (0.1..=12.0).contains(&profile.secondary.frequency_hz)
        && profile.secondary.damping_ratio.is_finite()
        && (0.05..=3.0).contains(&profile.secondary.damping_ratio)
        && profile.secondary.response.is_finite()
        && (0.0..=2.0).contains(&profile.secondary.response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rig_outfit::RigOutfitTopology;

    fn sample_vertex(x: f32, y: f32) -> RigVertex {
        RigVertex {
            position: RigPoint { x, y },
            uv: RigPoint { x, y },
            joints: [0, 0, 0, 0],
            weights: [1.0, 0.0, 0.0, 0.0],
        }
    }

    fn sample_manifest() -> RigManifest {
        let canvas = RigSize {
            width: PORTRAIT_CANVAS_WIDTH,
            height: PORTRAIT_CANVAS_HEIGHT,
        };
        let bones = vec![
            RigBone {
                id: "root".to_string(),
                parent: None,
                pivot: RigPoint { x: 0.5, y: 0.8 },
            },
            RigBone {
                id: "body".to_string(),
                parent: Some("root".to_string()),
                pivot: RigPoint { x: 0.5, y: 0.55 },
            },
            RigBone {
                id: "head".to_string(),
                parent: Some("body".to_string()),
                pivot: RigPoint { x: 0.5, y: 0.25 },
            },
        ];
        let parts = vec![RigPart {
            id: "body".to_string(),
            texture_id: "atlas".to_string(),
            z_index: 0,
            opacity: 1.0,
            slot: None,
            variant: None,
            vertices: vec![
                sample_vertex(0.0, 0.0),
                sample_vertex(1.0, 0.0),
                sample_vertex(0.0, 1.0),
            ],
            indices: vec![0, 1, 2],
        }];
        let semantics = default_rig_semantics(&bones, &[]);
        let spatial_profile = infer_spatial_profile(canvas, &bones, &parts, Some(&semantics));
        RigManifest {
            schema_version: RIG_SCHEMA_VERSION,
            rig_ir_version: Some(RIG_IR_VERSION),
            character_asset_contract_version: Some(CHARACTER_ASSET_CONTRACT_VERSION),
            source_master_asset_id: Some("/portrait.png".to_string()),
            source_generation_fingerprint: None,
            quality: RigQuality::Layered2d,
            canvas,
            textures: vec![RigTexture {
                id: "atlas".to_string(),
                url: "/atlas.png".to_string(),
                width: 256,
                height: 256,
            }],
            bones,
            parts,
            motion_profile: None,
            outfit_profile: None,
            semantic_anchors: HashMap::new(),
            semantics: Some(semantics),
            spatial_profile: Some(spatial_profile),
            anime25d_playback: None,
        }
    }

    #[test]
    fn sample_manifest_is_valid_without_clip_stack() {
        let rig = sample_manifest();
        rig.validate().unwrap();
        assert_eq!(rig.quality, RigQuality::Layered2d);
        assert_eq!(
            rig.canvas,
            RigSize {
                width: PORTRAIT_CANVAS_WIDTH,
                height: PORTRAIT_CANVAS_HEIGHT,
            }
        );
    }

    #[test]
    fn manifest_rejects_invalid_anime25d_playback() {
        let mut rig = sample_manifest();
        rig.anime25d_playback = Some(serde_json::json!({
            "kind": "anime-2.5d-rig",
            "version": 6
        }));
        assert_eq!(rig.validate(), Err(RigValidationError::Anime25DPlayback));
    }

    #[test]
    fn generation_fingerprint_is_carried_and_validated() {
        let fingerprint = "a".repeat(64);
        let mut rig = sample_manifest();
        rig.source_generation_fingerprint = Some(fingerprint.clone());
        rig.validate().unwrap();
        assert_eq!(rig.source_generation_fingerprint, Some(fingerprint));

        rig.source_generation_fingerprint = Some("not-a-sha256".to_string());
        assert_eq!(rig.validate(), Err(RigValidationError::AssetContract));
    }

    #[test]
    fn presentation_slots_require_known_variants_and_stable_fallbacks() {
        let mut rig = sample_manifest();
        rig.parts[0].slot = Some("mouth".to_string());
        rig.parts[0].variant = Some("open".to_string());
        assert_eq!(rig.validate(), Err(RigValidationError::Mesh));
        rig.parts[0].variant = Some("closed".to_string());
        rig.validate().unwrap();
        rig.parts[0].variant = Some("invented".to_string());
        assert_eq!(rig.validate(), Err(RigValidationError::Mesh));
        rig.parts[0].slot = Some("invented-slot".to_string());
        assert_eq!(rig.validate(), Err(RigValidationError::Mesh));
    }

    #[test]
    fn migration_enriches_legacy_rig_once() {
        let legacy = sample_manifest();
        let (migrated, changed) = migrate_rig_manifest(legacy, 42).unwrap();
        assert!(changed);
        assert_eq!(migrated.motion_profile.as_ref().unwrap().seed, 42);
        let (repeated, changed_again) = migrate_rig_manifest(migrated, 99).unwrap();
        assert!(!changed_again);
        assert_eq!(repeated.motion_profile.as_ref().unwrap().seed, 42);
    }

    #[test]
    fn migration_keeps_legacy_custom_presentation_slots_on_supported_ir() {
        let mut legacy = sample_manifest();
        legacy.rig_ir_version = Some(MIN_SUPPORTED_RIG_IR_VERSION);
        legacy.parts[0].slot = Some("custom-emblem".to_string());
        legacy.parts[0].variant = Some("lit".to_string());
        let (migrated, _) = migrate_rig_manifest(legacy, 9).unwrap();
        assert_eq!(migrated.rig_ir_version, Some(MIN_SUPPORTED_RIG_IR_VERSION));
        assert_eq!(migrated.parts[0].slot.as_deref(), Some("custom-emblem"));
        assert_eq!(migrated.parts[0].variant.as_deref(), Some("lit"));
        migrated.validate().unwrap();
    }

    #[test]
    fn validates_optional_procedural_motion_profile() {
        let mut rig = sample_manifest();
        rig.motion_profile = Some(RigMotionProfile {
            seed: 42,
            breath: RigBreathMotionProfile {
                min_frequency_hz: 0.16,
                max_frequency_hz: 0.28,
                amplitude: 0.004,
            },
            blink: RigBlinkMotionProfile {
                min_interval_seconds: 2.5,
                max_interval_seconds: 7.0,
                duration_seconds: 0.24,
                double_chance: 0.15,
            },
            secondary: RigSecondaryMotionProfile {
                enabled: true,
                frequency_hz: 2.1,
                damping_ratio: 0.5,
                response: 0.6,
            },
        });
        rig.validate().unwrap();
        let encoded = serde_json::to_value(&rig).unwrap();
        assert_eq!(encoded["motionProfile"]["seed"], 42);
        assert_eq!(encoded["motionProfile"]["secondary"]["dampingRatio"], 0.5);
        rig.motion_profile.as_mut().unwrap().secondary.damping_ratio = 0.0;
        assert_eq!(rig.validate(), Err(RigValidationError::MotionProfile));
    }

    #[test]
    fn rejects_invalid_skin_weight_sum() {
        let mut rig = sample_manifest();
        rig.parts[0].vertices[0].weights = [0.2, 0.2, 0.2, 0.0];
        assert_eq!(rig.validate(), Err(RigValidationError::SkinWeights));
    }

    #[test]
    fn rejects_bone_cycles() {
        let mut rig = sample_manifest();
        rig.bones[0].parent = Some("head".to_string());
        assert_eq!(rig.validate(), Err(RigValidationError::BoneHierarchy));
    }

    #[test]
    fn character_asset_source_requires_real_face_layers_and_rigid_side_arms() {
        let bone = |id: &str, parent: Option<&str>| RigBone {
            id: id.to_string(),
            parent: parent.map(str::to_string),
            pivot: RigPoint { x: 0.5, y: 0.5 },
        };
        let layer =
            |id: &str, slot: Option<&str>, variant: Option<&str>, bone_id: &str| RigLayerSource {
                id: id.to_string(),
                texture_id: "atlas".to_string(),
                texture_bounds: RigRect {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                z_index: 0,
                opacity: 1.0,
                slot: slot.map(str::to_string),
                variant: variant.map(str::to_string),
                contours: Vec::new(),
                mesh: None,
                bone_handles: vec![RigBoneHandle {
                    bone_id: bone_id.to_string(),
                    start: RigPoint { x: 0.0, y: 0.0 },
                    end: RigPoint { x: 1.0, y: 1.0 },
                    falloff: 1.0,
                }],
            };
        let mut bones = vec![
            bone("root", None),
            bone("body", Some("root")),
            bone("head", Some("body")),
            bone("face", Some("head")),
            bone("left-eye", Some("face")),
            bone("right-eye", Some("face")),
            bone("mouth", Some("face")),
            bone("a25d-handwear", Some("body")),
            bone("a25d-handwear-left", Some("a25d-handwear")),
            bone("a25d-handwear-right", Some("a25d-handwear")),
        ];
        let mut layers = vec![
            layer("a25d-face", None, None, "face"),
            layer("a25d-front-hair", None, None, "head"),
            layer("a25d-back-hair", None, None, "head"),
            layer("a25d-topwear", None, None, "body"),
            layer(
                "a25d-eye-open-left",
                Some("eye-left"),
                Some("open"),
                "left-eye",
            ),
            layer(
                "a25d-eye-close-left",
                Some("eye-left"),
                Some("closed"),
                "left-eye",
            ),
            layer(
                "a25d-eye-dizzy-left",
                Some("eye-left"),
                Some("dizzy"),
                "left-eye",
            ),
            layer(
                "a25d-eye-squeeze-left",
                Some("eye-left"),
                Some("squeeze"),
                "left-eye",
            ),
            layer(
                "a25d-eye-cry-left",
                Some("eye-left"),
                Some("cry"),
                "left-eye",
            ),
            layer(
                "a25d-eye-silly-left",
                Some("eye-left"),
                Some("silly"),
                "left-eye",
            ),
            layer(
                "a25d-eye-open-right",
                Some("eye-right"),
                Some("open"),
                "right-eye",
            ),
            layer(
                "a25d-eye-close-right",
                Some("eye-right"),
                Some("closed"),
                "right-eye",
            ),
            layer(
                "a25d-eye-dizzy-right",
                Some("eye-right"),
                Some("dizzy"),
                "right-eye",
            ),
            layer(
                "a25d-eye-squeeze-right",
                Some("eye-right"),
                Some("squeeze"),
                "right-eye",
            ),
            layer(
                "a25d-eye-cry-right",
                Some("eye-right"),
                Some("cry"),
                "right-eye",
            ),
            layer(
                "a25d-eye-silly-right",
                Some("eye-right"),
                Some("silly"),
                "right-eye",
            ),
            layer("a25d-mouth-open", Some("mouth"), Some("open"), "mouth"),
            layer("a25d-mouth-close", Some("mouth"), Some("closed"), "mouth"),
            layer("a25d-mouth-wide", Some("mouth"), Some("wide"), "mouth"),
            layer("a25d-mouth-round", Some("mouth"), Some("round"), "mouth"),
            layer("a25d-mouth-narrow", Some("mouth"), Some("narrow"), "mouth"),
            layer("a25d-mouth-cry", Some("mouth"), Some("cry"), "mouth"),
            layer("a25d-mouth-maniac", Some("mouth"), Some("maniac"), "mouth"),
            layer("a25d-mouth-silly", Some("mouth"), Some("silly"), "mouth"),
            layer("a25d-lovestruck-heart-left", None, None, "left-eye"),
            layer("a25d-lovestruck-heart-right", None, None, "right-eye"),
            layer("a25d-lovestruck-face-effect", None, None, "face"),
            layer("a25d-lovestruck-drool", None, None, "face"),
            layer("a25d-handwear-left", None, None, "a25d-handwear-left"),
            layer("a25d-handwear-right", None, None, "a25d-handwear-right"),
        ];
        validate_character_asset_source(&bones, &layers).unwrap();

        let right_arm = layers.pop().unwrap();
        assert_eq!(
            validate_character_asset_source(&bones, &layers),
            Err(RigValidationError::AssetContract)
        );
        layers.push(right_arm);
        bones.push(bone("left-shoulder", Some("body")));
        assert_eq!(
            validate_character_asset_source(&bones, &layers),
            Err(RigValidationError::AssetContract)
        );
    }

    #[test]
    fn compiles_contour_mesh_with_bounded_weights() {
        let bones = vec![
            RigBone {
                id: "left".to_string(),
                parent: None,
                pivot: RigPoint { x: 0.2, y: 0.5 },
            },
            RigBone {
                id: "right".to_string(),
                parent: None,
                pivot: RigPoint { x: 0.8, y: 0.5 },
            },
        ];
        let rig = compile_layered_rig(RigCompileSource {
            rig_ir_version: Some(RIG_IR_VERSION),
            character_asset_contract_version: Some(CHARACTER_ASSET_CONTRACT_VERSION),
            source_master_asset_id: Some("/portrait.png".to_string()),
            source_generation_fingerprint: None,
            canvas: RigSize {
                width: PORTRAIT_CANVAS_WIDTH,
                height: PORTRAIT_CANVAS_HEIGHT,
            },
            textures: vec![RigTexture {
                id: "atlas".to_string(),
                url: "/atlas.png".to_string(),
                width: 1024,
                height: 1024,
            }],
            bones,
            layers: vec![
                RigLayerSource {
                    id: "body".to_string(),
                    texture_id: "atlas".to_string(),
                    texture_bounds: RigRect {
                        x: 0.0,
                        y: 0.0,
                        width: 1.0,
                        height: 1.0,
                    },
                    z_index: 0,
                    opacity: 1.0,
                    slot: None,
                    variant: None,
                    contours: vec![vec![
                        RigPoint { x: 0.1, y: 0.1 },
                        RigPoint { x: 0.9, y: 0.1 },
                        RigPoint { x: 0.9, y: 0.9 },
                        RigPoint { x: 0.1, y: 0.9 },
                    ]],
                    mesh: None,
                    bone_handles: vec![
                        RigBoneHandle {
                            bone_id: "left".to_string(),
                            start: RigPoint { x: 0.1, y: 0.2 },
                            end: RigPoint { x: 0.1, y: 0.8 },
                            falloff: 0.45,
                        },
                        RigBoneHandle {
                            bone_id: "right".to_string(),
                            start: RigPoint { x: 0.9, y: 0.2 },
                            end: RigPoint { x: 0.9, y: 0.8 },
                            falloff: 0.45,
                        },
                    ],
                },
                RigLayerSource {
                    id: "a25d-topwear".to_string(),
                    texture_id: "atlas".to_string(),
                    texture_bounds: RigRect {
                        x: 0.0,
                        y: 0.0,
                        width: 1.0,
                        height: 1.0,
                    },
                    z_index: 1,
                    opacity: 1.0,
                    slot: None,
                    variant: None,
                    contours: vec![],
                    mesh: Some(RigLayerMeshSource {
                        vertices: vec![
                            RigPoint { x: 0.1, y: 0.1 },
                            RigPoint { x: 0.9, y: 0.1 },
                            RigPoint { x: 0.9, y: 0.9 },
                            RigPoint { x: 0.1, y: 0.9 },
                            RigPoint { x: 0.5, y: 0.5 },
                        ],
                        indices: vec![0, 1, 4, 1, 2, 4, 2, 3, 4, 3, 0, 4],
                    }),
                    bone_handles: vec![
                        RigBoneHandle {
                            bone_id: "left".to_string(),
                            start: RigPoint { x: 0.1, y: 0.2 },
                            end: RigPoint { x: 0.1, y: 0.8 },
                            falloff: 0.45,
                        },
                        RigBoneHandle {
                            bone_id: "right".to_string(),
                            start: RigPoint { x: 0.9, y: 0.2 },
                            end: RigPoint { x: 0.9, y: 0.8 },
                            falloff: 0.45,
                        },
                    ],
                },
            ],
            motion_profile: None,
            outfit_profile: None,
            semantic_anchors: HashMap::new(),
            semantics: None,
            spatial_profile: None,
            anime25d_playback: None,
        })
        .unwrap();
        assert_eq!(rig.quality, RigQuality::Layered2d);
        assert_eq!(rig.parts[0].indices.len(), 6);
        assert_eq!(rig.parts[1].vertices.len(), 5);
        assert_eq!(rig.parts[1].indices.len(), 12);
        assert!(rig.parts[0].vertices.iter().any(|vertex| vertex
            .weights
            .iter()
            .filter(|weight| **weight > 0.02)
            .count()
            >= 2));
        assert!(rig.parts[0]
            .vertices
            .iter()
            .all(|vertex| (vertex.weights.iter().sum::<f32>() - 1.0).abs() < 0.002));
    }

    #[test]
    fn compile_entry_rejects_legacy_ir_even_though_stored_manifests_remain_readable() {
        let source = RigCompileSource {
            rig_ir_version: Some(MIN_SUPPORTED_RIG_IR_VERSION),
            character_asset_contract_version: None,
            source_master_asset_id: None,
            source_generation_fingerprint: None,
            canvas: RigSize {
                width: 1.0,
                height: 1.0,
            },
            textures: vec![],
            bones: vec![],
            layers: vec![],
            motion_profile: None,
            outfit_profile: None,
            semantic_anchors: HashMap::new(),
            semantics: None,
            spatial_profile: None,
            anime25d_playback: None,
        };
        assert_eq!(compile_layered_rig(source), Err(RigCompileError::IrVersion));
    }

    #[test]
    fn outfit_profile_inference_limits_wide_sleeves_and_long_skirts() {
        let sample = sample_manifest();
        let mut sleeve = sample.parts[0].clone();
        sleeve.id = "left-wide-sleeve".to_string();
        let mut skirt = sample.parts[0].clone();
        skirt.id = "long-skirt-front".to_string();
        let profile = infer_outfit_profile(&[sleeve, skirt]);
        assert!(profile.topologies.contains(&RigOutfitTopology::WideSleeve));
        assert!(profile.topologies.contains(&RigOutfitTopology::LongSkirt));
        assert_eq!(profile.torso_twist_scale, 0.9);
        assert_eq!(profile.secondary_motion_scale, 0.78);
    }
}
