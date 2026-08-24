//! Character-local spatial volumes used by contact IK and collision safety.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::rig::{RigBone, RigPart, RigPoint, RigSize};
use crate::rig_contract::MAX_RIG_COLLISION_VOLUMES;
use crate::rig_semantics::RigSemantics;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigCollisionVolume {
    pub id: String,
    pub bone_id: String,
    pub offset: RigPoint,
    pub radius: RigPoint,
    #[serde(default)]
    pub padding: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RigSpatialProfile {
    #[serde(default)]
    pub collision_volumes: Vec<RigCollisionVolume>,
}

pub(crate) fn infer_spatial_profile(
    canvas: RigSize,
    bones: &[RigBone],
    parts: &[RigPart],
    semantics: Option<&RigSemantics>,
) -> RigSpatialProfile {
    let mut collision_volumes = Vec::new();
    for role in ["head", "torso"] {
        let Some(bone_id) = semantics.and_then(|value| value.bones.get(role)) else {
            continue;
        };
        let Some(bone_index) = bones.iter().position(|bone| bone.id == *bone_id) else {
            continue;
        };
        let pivot = bones[bone_index].pivot;
        let points = parts
            .iter()
            .flat_map(|part| &part.vertices)
            .filter_map(|vertex| {
                let influence = vertex
                    .joints
                    .iter()
                    .zip(vertex.weights)
                    .filter(|(joint, _)| usize::from(**joint) == bone_index)
                    .map(|(_, weight)| weight)
                    .sum::<f32>();
                (influence >= 0.18).then_some(vertex.position)
            })
            .collect::<Vec<_>>();
        let (fallback_radius, fallback_offset, minimum, maximum) = if role == "head" {
            (
                RigPoint {
                    x: canvas.width * 0.105,
                    y: canvas.height * 0.13,
                },
                RigPoint {
                    x: 0.0,
                    y: -canvas.height * 0.025,
                },
                RigPoint {
                    x: canvas.width * 0.055,
                    y: canvas.height * 0.07,
                },
                RigPoint {
                    x: canvas.width * 0.18,
                    y: canvas.height * 0.2,
                },
            )
        } else {
            (
                RigPoint {
                    x: canvas.width * 0.14,
                    y: canvas.height * 0.2,
                },
                RigPoint {
                    x: 0.0,
                    y: canvas.height * 0.055,
                },
                RigPoint {
                    x: canvas.width * 0.07,
                    y: canvas.height * 0.1,
                },
                RigPoint {
                    x: canvas.width * 0.22,
                    y: canvas.height * 0.3,
                },
            )
        };
        let (offset, radius) = robust_volume(
            &points,
            pivot,
            fallback_offset,
            fallback_radius,
            minimum,
            maximum,
        );
        collision_volumes.push(RigCollisionVolume {
            id: role.to_string(),
            bone_id: bone_id.clone(),
            offset,
            radius,
            padding: canvas.width.min(canvas.height) * 0.006,
        });
    }
    RigSpatialProfile { collision_volumes }
}

pub(crate) fn spatial_profile_is_valid(
    profile: &RigSpatialProfile,
    bones: &[RigBone],
    canvas: RigSize,
) -> bool {
    let bone_ids = bones
        .iter()
        .map(|bone| bone.id.as_str())
        .collect::<HashSet<_>>();
    let ids = profile
        .collision_volumes
        .iter()
        .map(|volume| volume.id.as_str())
        .collect::<HashSet<_>>();
    profile.collision_volumes.len() <= MAX_RIG_COLLISION_VOLUMES
        && ids.len() == profile.collision_volumes.len()
        && profile.collision_volumes.iter().all(|volume| {
            !volume.id.trim().is_empty()
                && volume.id.len() <= 64
                && bone_ids.contains(volume.bone_id.as_str())
                && finite_point(volume.offset)
                && finite_point(volume.radius)
                && volume.radius.x > 0.0
                && volume.radius.y > 0.0
                && volume.radius.x <= canvas.width
                && volume.radius.y <= canvas.height
                && volume.offset.x.abs() <= canvas.width * 2.0
                && volume.offset.y.abs() <= canvas.height * 2.0
                && volume.padding.is_finite()
                && volume.padding >= 0.0
                && volume.padding <= canvas.width.max(canvas.height)
        })
}

fn robust_volume(
    points: &[RigPoint],
    pivot: RigPoint,
    fallback_offset: RigPoint,
    fallback_radius: RigPoint,
    minimum: RigPoint,
    maximum: RigPoint,
) -> (RigPoint, RigPoint) {
    if points.len() < 6 {
        return (fallback_offset, fallback_radius);
    }
    let mut xs = points.iter().map(|point| point.x).collect::<Vec<_>>();
    let mut ys = points.iter().map(|point| point.y).collect::<Vec<_>>();
    xs.sort_by(f32::total_cmp);
    ys.sort_by(f32::total_cmp);
    let lower = ((points.len() - 1) as f32 * 0.08).round() as usize;
    let upper = ((points.len() - 1) as f32 * 0.92).round() as usize;
    let center = RigPoint {
        x: (xs[lower] + xs[upper]) * 0.5,
        y: (ys[lower] + ys[upper]) * 0.5,
    };
    let radius = RigPoint {
        x: ((xs[upper] - xs[lower]) * 0.46).clamp(minimum.x, maximum.x),
        y: ((ys[upper] - ys[lower]) * 0.46).clamp(minimum.y, maximum.y),
    };
    (
        RigPoint {
            x: center.x - pivot.x,
            y: center.y - pivot.y,
        },
        radius,
    )
}

fn finite_point(point: RigPoint) -> bool {
    point.x.is_finite() && point.y.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn custom_semantic_bones_receive_character_local_volumes() {
        let bones = vec![
            bone("hips-x", None, 0.5, 0.8),
            bone("spine-x", Some("hips-x"), 0.5, 0.52),
            bone("skull-x", Some("spine-x"), 0.5, 0.25),
        ];
        let semantics = RigSemantics {
            bones: HashMap::from([
                ("torso".into(), "spine-x".into()),
                ("head".into(), "skull-x".into()),
            ]),
            chains: HashMap::new(),
            secondary_bone_ids: Vec::new(),
        };
        let profile = infer_spatial_profile(
            RigSize {
                width: 1.0,
                height: 1.0,
            },
            &bones,
            &[],
            Some(&semantics),
        );
        assert!(spatial_profile_is_valid(
            &profile,
            &bones,
            RigSize {
                width: 1.0,
                height: 1.0,
            }
        ));
        assert_eq!(profile.collision_volumes.len(), 2);
        assert_eq!(profile.collision_volumes[0].bone_id, "skull-x");
        assert_eq!(profile.collision_volumes[1].bone_id, "spine-x");
    }

    fn bone(id: &str, parent: Option<&str>, x: f32, y: f32) -> RigBone {
        RigBone {
            id: id.into(),
            parent: parent.map(str::to_string),
            pivot: RigPoint { x, y },
        }
    }
}
