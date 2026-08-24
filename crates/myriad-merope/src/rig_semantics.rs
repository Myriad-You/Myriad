//! Versioned semantic rig IR shared by import, migration, and runtimes.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::rig::RigBone;
use crate::rig_contract::{SECONDARY_PART_PATTERNS, SEMANTIC_BONE_ROLES, SEMANTIC_CHAIN_ROLES};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigSemantics {
    #[serde(default)]
    pub bones: HashMap<String, String>,
    #[serde(default)]
    pub chains: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub secondary_bone_ids: Vec<String>,
}

pub(crate) fn default_rig_semantics(
    bones: &[RigBone],
    explicit_secondary: &[String],
) -> RigSemantics {
    let ids = bones
        .iter()
        .map(|bone| bone.id.as_str())
        .collect::<HashSet<_>>();
    let canonical = [
        ("root", "root"),
        ("torso", "body"),
        ("head", "head"),
        ("face", "face"),
        ("left-eye", "left-eye"),
        ("right-eye", "right-eye"),
        ("mouth", "mouth"),
        ("handwear", "a25d-handwear"),
    ];
    let semantic_bones = canonical
        .into_iter()
        .filter(|(_, bone_id)| ids.contains(bone_id))
        .map(|(role, bone_id)| (role.to_string(), bone_id.to_string()))
        .collect::<HashMap<_, _>>();
    let chain_roles = [("torso", ["root", "torso", "head"])];
    let mut chains = HashMap::new();
    for (chain_role, bone_roles) in chain_roles {
        let chain = bone_roles
            .iter()
            .filter_map(|role| semantic_bones.get(*role).cloned())
            .collect::<Vec<_>>();
        if chain.len() == bone_roles.len() && connected(&chain, bones) {
            chains.insert(chain_role.to_string(), chain);
        }
    }
    let mut secondary_bone_ids = Vec::new();
    for bone_id in explicit_secondary
        .iter()
        .chain(bones.iter().filter_map(|bone| {
            let normalized = bone.id.to_ascii_lowercase();
            SECONDARY_PART_PATTERNS
                .iter()
                .any(|pattern| normalized.contains(pattern))
                .then_some(&bone.id)
        }))
    {
        if ids.contains(bone_id.as_str()) && !secondary_bone_ids.contains(bone_id) {
            secondary_bone_ids.push(bone_id.clone());
        }
    }
    RigSemantics {
        bones: semantic_bones,
        chains,
        secondary_bone_ids,
    }
}

/// Rebuilds the current semantic view while preserving valid custom mappings.
/// Obsolete articulated-limb roles are intentionally discarded during the
/// Rig IR v4 migration; extra legacy bones may remain inert in old packages.
pub(crate) fn migrate_rig_semantics(
    existing: Option<&RigSemantics>,
    bones: &[RigBone],
    explicit_secondary: &[String],
) -> RigSemantics {
    let mut migrated = default_rig_semantics(bones, explicit_secondary);
    let Some(existing) = existing else {
        return migrated;
    };
    let ids = bones
        .iter()
        .map(|bone| bone.id.as_str())
        .collect::<HashSet<_>>();
    let roles = SEMANTIC_BONE_ROLES.iter().copied().collect::<HashSet<_>>();
    for (role, bone_id) in &existing.bones {
        if roles.contains(role.as_str()) && ids.contains(bone_id.as_str()) {
            migrated.bones.insert(role.clone(), bone_id.clone());
        }
    }
    let chain_roles = SEMANTIC_CHAIN_ROLES.iter().copied().collect::<HashSet<_>>();
    for (role, chain) in &existing.chains {
        if chain_roles.contains(role.as_str())
            && chain.len() >= 2
            && chain.iter().all(|bone_id| ids.contains(bone_id.as_str()))
            && connected(chain, bones)
        {
            migrated.chains.insert(role.clone(), chain.clone());
        }
    }
    for bone_id in &existing.secondary_bone_ids {
        if ids.contains(bone_id.as_str()) && !migrated.secondary_bone_ids.contains(bone_id) {
            migrated.secondary_bone_ids.push(bone_id.clone());
        }
    }
    migrated
}

pub(crate) fn rig_semantics_are_valid(semantics: &RigSemantics, bones: &[RigBone]) -> bool {
    let ids = bones
        .iter()
        .map(|bone| bone.id.as_str())
        .collect::<HashSet<_>>();
    let roles = SEMANTIC_BONE_ROLES.iter().copied().collect::<HashSet<_>>();
    let chain_roles = SEMANTIC_CHAIN_ROLES.iter().copied().collect::<HashSet<_>>();
    semantics
        .bones
        .iter()
        .all(|(role, bone_id)| roles.contains(role.as_str()) && ids.contains(bone_id.as_str()))
        && semantics.chains.iter().all(|(role, chain)| {
            chain_roles.contains(role.as_str())
                && chain.len() >= 2
                && chain.len() <= bones.len()
                && chain.iter().collect::<HashSet<_>>().len() == chain.len()
                && chain.iter().all(|bone_id| ids.contains(bone_id.as_str()))
                && connected(chain, bones)
        })
        && semantics.secondary_bone_ids.len() <= bones.len()
        && semantics
            .secondary_bone_ids
            .iter()
            .collect::<HashSet<_>>()
            .len()
            == semantics.secondary_bone_ids.len()
        && semantics
            .secondary_bone_ids
            .iter()
            .all(|bone_id| ids.contains(bone_id.as_str()))
}

fn connected(chain: &[String], bones: &[RigBone]) -> bool {
    let parents = bones
        .iter()
        .map(|bone| (bone.id.as_str(), bone.parent.as_deref()))
        .collect::<HashMap<_, _>>();
    chain.windows(2).all(|pair| {
        parents.get(pair[1].as_str()).copied().flatten() == Some(pair[0].as_str())
            || parents.get(pair[0].as_str()).copied().flatten() == Some(pair[1].as_str())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rig::RigPoint;
    use crate::rig_outfit::default_semantic_anchors;

    #[test]
    fn explicit_semantics_decouple_roles_from_bone_names() {
        let bones = vec![
            bone("hips-x", None),
            bone("spine-x", Some("hips-x")),
            bone("skull-x", Some("spine-x")),
        ];
        let semantics = RigSemantics {
            bones: HashMap::from([
                ("root".into(), "hips-x".into()),
                ("torso".into(), "spine-x".into()),
                ("head".into(), "skull-x".into()),
            ]),
            chains: HashMap::from([(
                "torso".into(),
                vec!["hips-x".into(), "spine-x".into(), "skull-x".into()],
            )]),
            secondary_bone_ids: Vec::new(),
        };
        assert!(rig_semantics_are_valid(&semantics, &bones));
        let anchors = default_semantic_anchors(&bones, Some(&semantics));
        assert_eq!(anchors["forehead"].bone_id, "skull-x");
        assert_eq!(anchors["temple-right"].bone_id, "skull-x");
        assert_eq!(anchors["chin"].bone_id, "skull-x");
        assert_eq!(anchors["chest"].bone_id, "spine-x");
    }

    fn bone(id: &str, parent: Option<&str>) -> RigBone {
        RigBone {
            id: id.into(),
            parent: parent.map(str::to_string),
            pivot: RigPoint { x: 0.0, y: 0.0 },
        }
    }
}
