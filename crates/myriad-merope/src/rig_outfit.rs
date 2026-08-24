//! Outfit-specific rig policy.
//!
//! The generic rig module owns meshes and animation. This module owns garment
//! topology inference, safety limits, and character-local contact anchors.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::rig::{RigBone, RigPart, RigPoint, RigSize, MAX_RIG_PARTS};
use crate::rig_contract::{
    ARMOR_OUTFIT_RULE, CAPE_OUTFIT_RULE, FITTED_OUTFIT_RULE, LONG_COAT_OUTFIT_RULE,
    LONG_SKIRT_OUTFIT_RULE, SECONDARY_PART_PATTERNS, SHORT_SKIRT_OUTFIT_RULE,
    WIDE_SLEEVE_OUTFIT_RULE,
};
use crate::rig_semantics::RigSemantics;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RigOutfitTopology {
    Fitted,
    ShortSkirt,
    LongSkirt,
    LongCoat,
    WideSleeve,
    Cape,
    Armor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigOutfitProfile {
    pub topologies: Vec<RigOutfitTopology>,
    pub secondary_part_ids: Vec<String>,
    #[serde(default = "default_scale")]
    pub torso_twist_scale: f32,
    #[serde(default = "default_scale")]
    pub secondary_motion_scale: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigSemanticAnchor {
    pub bone_id: String,
    /// Rest-pose canvas offset from the owning bone pivot.
    pub offset: RigPoint,
}

#[derive(Debug, Clone, Copy)]
struct OutfitSafetyRule {
    torso_twist_scale: f32,
    secondary_motion_scale: f32,
}

pub fn infer_outfit_profile(parts: &[RigPart]) -> RigOutfitProfile {
    let ids = parts
        .iter()
        .map(|part| part.id.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let has = |patterns: &[&str]| {
        ids.iter()
            .any(|id| patterns.iter().any(|pattern| id.contains(pattern)))
    };
    let mut topologies = Vec::new();
    if has(&["long-skirt", "maxi-skirt", "dress-hem"]) {
        topologies.push(RigOutfitTopology::LongSkirt);
    } else if has(&["skirt", "dress"]) {
        topologies.push(RigOutfitTopology::ShortSkirt);
    }
    if has(&["coat-tail", "long-coat", "trench"]) {
        topologies.push(RigOutfitTopology::LongCoat);
    }
    if has(&["wide-sleeve", "kimono-sleeve", "bell-sleeve"]) {
        topologies.push(RigOutfitTopology::WideSleeve);
    }
    if has(&["cape", "cloak"]) {
        topologies.push(RigOutfitTopology::Cape);
    }
    if has(&["armor", "pauldron", "plate"]) {
        topologies.push(RigOutfitTopology::Armor);
    }
    let secondary_part_ids = parts
        .iter()
        .filter(|part| is_secondary_motion_part(&part.id))
        .map(|part| part.id.clone())
        .collect();
    create_outfit_profile(topologies, secondary_part_ids)
}

pub(crate) fn create_outfit_profile(
    mut topologies: Vec<RigOutfitTopology>,
    secondary_part_ids: Vec<String>,
) -> RigOutfitProfile {
    deduplicate(&mut topologies);
    topologies.sort_by_key(|topology| topology_order(*topology));
    if topologies.is_empty() {
        topologies.push(RigOutfitTopology::Fitted);
    }
    let rules = topologies
        .iter()
        .copied()
        .map(outfit_safety_rule)
        .collect::<Vec<_>>();
    RigOutfitProfile {
        topologies,
        secondary_part_ids,
        torso_twist_scale: rules
            .iter()
            .map(|rule| rule.torso_twist_scale)
            .fold(1.0, f32::min),
        secondary_motion_scale: rules
            .iter()
            .map(|rule| rule.secondary_motion_scale)
            .fold(1.0, f32::min),
    }
}

pub(crate) fn default_semantic_anchors(
    bones: &[RigBone],
    semantics: Option<&RigSemantics>,
) -> HashMap<String, RigSemanticAnchor> {
    let mut anchors = HashMap::new();
    let mut insert = |name: &str, bone_id: &str, offset: RigPoint| {
        if bones.iter().any(|bone| bone.id == bone_id) {
            anchors.insert(
                name.to_string(),
                RigSemanticAnchor {
                    bone_id: bone_id.to_string(),
                    offset,
                },
            );
        }
    };
    let head_id = semantics
        .and_then(|value| value.bones.get("head"))
        .map(String::as_str)
        .unwrap_or("head");
    let torso_id = semantics
        .and_then(|value| value.bones.get("torso"))
        .map(String::as_str)
        .unwrap_or("body");
    insert("forehead", head_id, RigPoint { x: 0.0, y: -0.16 });
    insert("temple-right", head_id, RigPoint { x: -0.08, y: -0.12 });
    insert("chest", torso_id, RigPoint { x: 0.0, y: -0.14 });
    insert("chin", head_id, RigPoint { x: 0.0, y: 0.02 });
    anchors
}

pub(crate) fn outfit_profile_is_valid(profile: &RigOutfitProfile, parts: &[RigPart]) -> bool {
    let part_ids = parts
        .iter()
        .map(|part| part.id.as_str())
        .collect::<HashSet<_>>();
    let topology_count = profile
        .topologies
        .iter()
        .copied()
        .collect::<HashSet<_>>()
        .len();
    let secondary_count = profile
        .secondary_part_ids
        .iter()
        .collect::<HashSet<_>>()
        .len();
    !profile.topologies.is_empty()
        && profile.topologies.len() <= 7
        && topology_count == profile.topologies.len()
        && profile.secondary_part_ids.len() <= MAX_RIG_PARTS
        && secondary_count == profile.secondary_part_ids.len()
        && profile
            .secondary_part_ids
            .iter()
            .all(|id| !id.trim().is_empty() && part_ids.contains(id.as_str()))
        && profile.torso_twist_scale.is_finite()
        && (0.25..=1.0).contains(&profile.torso_twist_scale)
        && profile.secondary_motion_scale.is_finite()
        && (0.2..=1.0).contains(&profile.secondary_motion_scale)
}

pub(crate) fn semantic_anchors_are_valid(
    anchors: &HashMap<String, RigSemanticAnchor>,
    bones: &[RigBone],
    canvas: RigSize,
) -> bool {
    let bone_ids = bones
        .iter()
        .map(|bone| bone.id.as_str())
        .collect::<HashSet<_>>();
    anchors.iter().all(|(name, anchor)| {
        !name.trim().is_empty()
            && name.len() <= 64
            && bone_ids.contains(anchor.bone_id.as_str())
            && anchor.offset.x.is_finite()
            && anchor.offset.y.is_finite()
            && anchor.offset.x.abs() <= canvas.width * 2.0
            && anchor.offset.y.abs() <= canvas.height * 2.0
    })
}

fn outfit_safety_rule(topology: RigOutfitTopology) -> OutfitSafetyRule {
    let contract = match topology {
        RigOutfitTopology::Fitted => FITTED_OUTFIT_RULE,
        RigOutfitTopology::ShortSkirt => SHORT_SKIRT_OUTFIT_RULE,
        RigOutfitTopology::LongSkirt => LONG_SKIRT_OUTFIT_RULE,
        RigOutfitTopology::LongCoat => LONG_COAT_OUTFIT_RULE,
        RigOutfitTopology::WideSleeve => WIDE_SLEEVE_OUTFIT_RULE,
        RigOutfitTopology::Cape => CAPE_OUTFIT_RULE,
        RigOutfitTopology::Armor => ARMOR_OUTFIT_RULE,
    };
    OutfitSafetyRule {
        torso_twist_scale: contract.torso_twist_scale,
        secondary_motion_scale: contract.secondary_motion_scale,
    }
}

const fn topology_order(topology: RigOutfitTopology) -> u8 {
    match topology {
        RigOutfitTopology::Fitted => 0,
        RigOutfitTopology::ShortSkirt => 1,
        RigOutfitTopology::LongSkirt => 2,
        RigOutfitTopology::LongCoat => 3,
        RigOutfitTopology::WideSleeve => 4,
        RigOutfitTopology::Cape => 5,
        RigOutfitTopology::Armor => 6,
    }
}

const fn default_scale() -> f32 {
    1.0
}

fn is_secondary_motion_part(part_id: &str) -> bool {
    let id = part_id.to_ascii_lowercase();
    SECONDARY_PART_PATTERNS
        .iter()
        .any(|pattern| id.contains(pattern))
}

fn deduplicate(values: &mut Vec<RigOutfitTopology>) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(*value));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combined_topologies_use_the_most_conservative_rule() {
        let profile = create_outfit_profile(
            vec![
                RigOutfitTopology::Armor,
                RigOutfitTopology::LongSkirt,
                RigOutfitTopology::WideSleeve,
            ],
            Vec::new(),
        );
        assert_eq!(profile.torso_twist_scale, 0.62);
        assert_eq!(profile.secondary_motion_scale, 0.45);
        assert_eq!(
            profile.topologies,
            vec![
                RigOutfitTopology::LongSkirt,
                RigOutfitTopology::WideSleeve,
                RigOutfitTopology::Armor,
            ]
        );
    }

    #[test]
    fn empty_topology_uses_fitted_defaults() {
        let profile = create_outfit_profile(Vec::new(), Vec::new());
        assert_eq!(profile.topologies, vec![RigOutfitTopology::Fitted]);
        assert_eq!(profile.torso_twist_scale, 1.0);
        assert_eq!(profile.secondary_motion_scale, 1.0);
    }
}
