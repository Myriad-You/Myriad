use std::collections::HashSet;

use serde_json::Value;

const PLAYBACK_KIND: &str = "anime-2.5d-rig";
const PLAYBACK_VERSION: u64 = 7;
const PROJECT_NAME: &str = "Anime2.5DRig";
const MOUTH_MATERIALS: [&str; 6] = [
    "mouthClose",
    "mouthOpen",
    "mouthWide",
    "mouthRound",
    "mouthNarrow",
    "mouthManiac",
];

pub(crate) fn playback_is_valid(value: &Value) -> bool {
    let Some(playback) = value.as_object() else {
        return false;
    };
    let Some(canvas) = playback.get("pixelCanvas").and_then(Value::as_object) else {
        return false;
    };
    let Some(width) = finite_number(canvas.get("width")) else {
        return false;
    };
    let Some(height) = finite_number(canvas.get("height")) else {
        return false;
    };
    if width <= 0.0 || height <= 0.0 {
        return false;
    }
    playback.get("kind").and_then(Value::as_str) == Some(PLAYBACK_KIND)
        && playback.get("version").and_then(Value::as_u64) == Some(PLAYBACK_VERSION)
        && playback.get("engine").and_then(Value::as_str) == Some(PROJECT_NAME)
        && playback
            .get("layers")
            .and_then(Value::as_array)
            .is_some_and(|layers| !layers.is_empty())
        && playback.get("anchors").is_some_and(Value::is_object)
        && playback
            .get("mouthProfile")
            .is_some_and(|profile| mouth_profile_is_valid(profile, width, height))
        && playback
            .get("chestProfile")
            .is_some_and(|profile| chest_profile_is_valid(profile, width, height))
        && playback
            .get("shellProfile")
            .is_some_and(|profile| shell_profile_is_valid(profile, width, height))
}

fn mouth_profile_is_valid(value: &Value, width: f64, height: f64) -> bool {
    let Some(profile) = value.as_object() else {
        return false;
    };
    if profile.get("version").and_then(Value::as_u64) != Some(1)
        || !matches!(
            profile.get("source").and_then(Value::as_str),
            Some("alpha-contour" | "bounds-fallback")
        )
    {
        return false;
    }
    let Some(silhouettes) = profile.get("silhouettes").and_then(Value::as_array) else {
        return false;
    };
    let Some(bridges) = profile.get("bridges").and_then(Value::as_array) else {
        return false;
    };
    if silhouettes.len() != MOUTH_MATERIALS.len() || bridges.len() != 15 {
        return false;
    }

    let mut materials = HashSet::new();
    for silhouette in silhouettes {
        let Some(silhouette) = silhouette.as_object() else {
            return false;
        };
        let Some(material) = silhouette.get("material").and_then(Value::as_str) else {
            return false;
        };
        if !MOUTH_MATERIALS.contains(&material)
            || !materials.insert(material)
            || !number_in_range(silhouette.get("centerX"), -width, width * 2.0)
            || !number_in_range(silhouette.get("centerY"), -height, height * 2.0)
            || !number_in_range(silhouette.get("width"), 0.25, width)
            || !number_in_range(silhouette.get("height"), 0.25, height)
            || !number_in_range(silhouette.get("leftCornerY"), -height, height * 2.0)
            || !number_in_range(silhouette.get("rightCornerY"), -height, height * 2.0)
            || !number_in_range(silhouette.get("fillRatio"), 0.0, 1.0)
            || !number_in_range(silhouette.get("aperture"), 0.0, 4.0)
        {
            return false;
        }
    }

    let mut pairs = HashSet::new();
    for bridge in bridges {
        let Some(bridge) = bridge.as_object() else {
            return false;
        };
        let Some(first) = bridge.get("first").and_then(Value::as_str) else {
            return false;
        };
        let Some(second) = bridge.get("second").and_then(Value::as_str) else {
            return false;
        };
        let Some(first_index) = MOUTH_MATERIALS
            .iter()
            .position(|material| *material == first)
        else {
            return false;
        };
        let Some(second_index) = MOUTH_MATERIALS
            .iter()
            .position(|material| *material == second)
        else {
            return false;
        };
        let pair = if first_index < second_index {
            (first_index, second_index)
        } else {
            (second_index, first_index)
        };
        if first_index == second_index
            || !pairs.insert(pair)
            || !number_in_range(bridge.get("widthScale"), 0.75, 1.0)
            || !number_in_range(bridge.get("heightScale"), 0.65, 1.0)
            || !number_in_range(bridge.get("neutralization"), 0.0, 1.0)
            || !number_in_range(bridge.get("centerOffsetX"), -width / 4.0, width / 4.0)
            || !number_in_range(bridge.get("centerOffsetY"), -height / 4.0, height / 4.0)
        {
            return false;
        }
    }
    materials.len() == MOUTH_MATERIALS.len() && pairs.len() == 15
}

fn chest_profile_is_valid(value: &Value, width: f64, height: f64) -> bool {
    let Some(profile) = value.as_object() else {
        return false;
    };
    profile.get("version").and_then(Value::as_u64) == Some(2)
        && profile.get("enabled").is_some_and(Value::is_boolean)
        && matches!(
            profile.get("source").and_then(Value::as_str),
            Some("ai-vision" | "geometry-fallback" | "gender-policy")
        )
        && number_in_range(profile.get("centerX"), 0.0, width)
        && number_in_range(profile.get("centerY"), 0.0, height)
        && number_in_range(profile.get("radiusX"), 1.0, width / 2.0)
        && number_in_range(profile.get("radiusY"), 1.0, height / 2.0)
        && number_in_range(profile.get("visibleScale"), 0.0, 1.0)
        && number_in_range(profile.get("motionScale"), 0.0, 1.25)
        && number_in_range(profile.get("frequencyScale"), 0.75, 1.25)
        && number_in_range(profile.get("supportScale"), 0.0, 1.0)
        && number_in_range(profile.get("garmentMotionScale"), 0.0, 1.0)
        && number_in_range(profile.get("confidence"), 0.0, 1.0)
}

fn shell_profile_is_valid(value: &Value, width: f64, height: f64) -> bool {
    let Some(profile) = value.as_object() else {
        return false;
    };
    let Some(face_profile) = profile.get("faceProfile").and_then(Value::as_object) else {
        return false;
    };
    let Some(hair) = profile.get("hair").and_then(Value::as_object) else {
        return false;
    };
    let Some(pin) = hair.get("hairlinePin").and_then(Value::as_object) else {
        return false;
    };
    let Some(torso) = profile.get("torso").and_then(Value::as_object) else {
        return false;
    };
    let Some(points) = face_profile.get("points").and_then(Value::as_array) else {
        return false;
    };
    let Some(start_y) = finite_number(face_profile.get("startY")) else {
        return false;
    };
    let Some(end_y) = finite_number(face_profile.get("endY")) else {
        return false;
    };
    if profile.get("version").and_then(Value::as_u64) != Some(1)
        || !matches!(
            profile.get("source").and_then(Value::as_str),
            Some("anchor-derived" | "authored")
        )
        || !profile.get("enabled").is_some_and(Value::is_boolean)
        || !number_in_range(profile.get("blend"), 0.0, 1.0)
        || !shell_ellipsoid_is_valid(profile.get("head"), width, height)
        || !face_profile.get("enabled").is_some_and(Value::is_boolean)
        || !(0.0..=height).contains(&start_y)
        || !(0.0..=height * 1.2).contains(&end_y)
        || end_y <= start_y
        || points.len() != 5
        || !shell_ellipsoid_is_valid(profile.get("hair"), width, height)
        || !number_in_range(hair.get("frontGap"), 0.0, 0.6)
        || !number_in_range(hair.get("frontBulge"), 0.0, 1.5)
        || !number_in_range(hair.get("backDepth"), 0.0, 1.0)
        || !number_in_range(hair.get("crownRound"), 0.0, 1.0)
        || !pin.get("enabled").is_some_and(Value::is_boolean)
        || !matches!(
            pin.get("mode").and_then(Value::as_str),
            Some("rectangle" | "strand-roots")
        )
        || !number_in_range(pin.get("centerX"), -1.5, 1.5)
        || !number_in_range(pin.get("centerY"), -1.5, 1.5)
        || !number_in_range(pin.get("halfWidth"), 0.01, 2.0)
        || !number_in_range(pin.get("halfHeight"), 0.01, 2.0)
        || !number_in_range(pin.get("feather"), 0.0, 0.5)
        || !torso.get("enabled").is_some_and(Value::is_boolean)
        || !number_in_range(torso.get("blend"), 0.0, 1.0)
        || !number_in_range(torso.get("centerX"), 0.0, width)
        || !number_in_range(torso.get("radiusX"), 1.0, width)
        || !number_in_range(torso.get("radiusZ"), 1.0, width)
        // Optional: manifests compiled before the torso follow became
        // per-model carry no value, and the runtime reads those as a full
        // follow. Present but out of range is still a rejection.
        || torso
            .get("yawFollowScale")
            .is_some_and(|value| !number_in_range(Some(value), 0.0, 1.0))
    {
        return false;
    }

    let mut previous_v = -1.0;
    for point in points {
        let Some(point) = point.as_object() else {
            return false;
        };
        let Some(v) = finite_number(point.get("v")) else {
            return false;
        };
        if !(0.0..=1.0).contains(&v)
            || !number_in_range(point.get("z"), -0.4, 0.8)
            || v <= previous_v
        {
            return false;
        }
        previous_v = v;
    }
    true
}

fn shell_ellipsoid_is_valid(value: Option<&Value>, width: f64, height: f64) -> bool {
    let Some(value) = value.and_then(Value::as_object) else {
        return false;
    };
    number_in_range(value.get("centerX"), 0.0, width)
        && number_in_range(value.get("centerY"), 0.0, height)
        && number_in_range(value.get("radiusX"), 1.0, width)
        && number_in_range(value.get("radiusY"), 1.0, height)
        && number_in_range(value.get("radiusZ"), 1.0, width)
}

fn finite_number(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

fn number_in_range(value: Option<&Value>, minimum: f64, maximum: f64) -> bool {
    finite_number(value).is_some_and(|value| (minimum..=maximum).contains(&value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_playback() -> Value {
        let silhouettes = MOUTH_MATERIALS
            .iter()
            .map(|material| {
                serde_json::json!({
                    "material": material,
                    "centerX": 50,
                    "centerY": 54,
                    "width": 20,
                    "height": 12,
                    "leftCornerY": 54,
                    "rightCornerY": 54,
                    "fillRatio": 0.5,
                    "aperture": 0.5
                })
            })
            .collect::<Vec<_>>();
        let mut bridges = Vec::new();
        for (first_index, first) in MOUTH_MATERIALS.iter().enumerate() {
            for second in MOUTH_MATERIALS.iter().skip(first_index + 1) {
                bridges.push(serde_json::json!({
                    "first": first,
                    "second": second,
                    "widthScale": 1,
                    "heightScale": 1,
                    "neutralization": 0,
                    "centerOffsetX": 0,
                    "centerOffsetY": 0
                }));
            }
        }
        serde_json::json!({
            "kind": PLAYBACK_KIND,
            "version": PLAYBACK_VERSION,
            "engine": PROJECT_NAME,
            "pixelCanvas": { "width": 100, "height": 120 },
            "layers": [{ "role": "face" }],
            "anchors": {},
            "mouthProfile": {
                "version": 1,
                "source": "alpha-contour",
                "silhouettes": silhouettes,
                "bridges": bridges
            },
            "chestProfile": {
                "version": 2,
                "enabled": true,
                "source": "geometry-fallback",
                "centerX": 50,
                "centerY": 75,
                "radiusX": 20,
                "radiusY": 18,
                "visibleScale": 0.5,
                "motionScale": 1,
                "frequencyScale": 1,
                "supportScale": 0.45,
                "garmentMotionScale": 0.65,
                "confidence": 0
            },
            "shellProfile": {
                "version": 1,
                "source": "anchor-derived",
                "enabled": true,
                "blend": 0.5,
                "head": {
                    "centerX": 50,
                    "centerY": 35,
                    "radiusX": 25,
                    "radiusY": 30,
                    "radiusZ": 18
                },
                "faceProfile": {
                    "enabled": true,
                    "startY": 10,
                    "endY": 75,
                    "points": [
                        { "v": 0.06, "z": 0.1 },
                        { "v": 0.42, "z": 0.02 },
                        { "v": 0.62, "z": 0.3 },
                        { "v": 0.78, "z": 0.06 },
                        { "v": 0.97, "z": 0.14 }
                    ]
                },
                "hair": {
                    "centerX": 50,
                    "centerY": 33,
                    "radiusX": 28,
                    "radiusY": 33,
                    "radiusZ": 19,
                    "frontGap": 0.18,
                    "frontBulge": 1,
                    "backDepth": 0.35,
                    "crownRound": 0,
                    "hairlinePin": {
                        "enabled": true,
                        "mode": "strand-roots",
                        "centerX": 0,
                        "centerY": -0.45,
                        "halfWidth": 1.1,
                        "halfHeight": 0.32,
                        "feather": 0.06
                    }
                },
                "torso": {
                    "enabled": true,
                    "blend": 0.5,
                    "centerX": 50,
                    "radiusX": 40,
                    "radiusZ": 25
                }
            }
        })
    }

    #[test]
    fn accepts_complete_v7_playback() {
        assert!(playback_is_valid(&sample_playback()));
    }

    #[test]
    fn rejects_obsolete_or_incomplete_playback() {
        let mut playback = sample_playback();
        playback["version"] = serde_json::json!(6);
        assert!(!playback_is_valid(&playback));

        let mut playback = sample_playback();
        playback.as_object_mut().unwrap().remove("mouthProfile");
        assert!(!playback_is_valid(&playback));

        let mut playback = sample_playback();
        playback["shellProfile"]
            .as_object_mut()
            .unwrap()
            .remove("torso");
        assert!(!playback_is_valid(&playback));

        let mut playback = sample_playback();
        playback["chestProfile"]
            .as_object_mut()
            .unwrap()
            .remove("garmentMotionScale");
        assert!(!playback_is_valid(&playback));
    }

    #[test]
    fn accepts_an_absent_or_bounded_per_model_torso_follow() {
        // Compiled before the follow became per-model: still live.
        let playback = sample_playback();
        assert!(playback["shellProfile"]["torso"]
            .get("yawFollowScale")
            .is_none());
        assert!(playback_is_valid(&playback));

        for scale in [0.0, 0.4, 1.0] {
            let mut playback = sample_playback();
            playback["shellProfile"]["torso"]["yawFollowScale"] = serde_json::json!(scale);
            assert!(playback_is_valid(&playback), "{scale}");
        }

        for scale in [-0.1, 1.4] {
            let mut playback = sample_playback();
            playback["shellProfile"]["torso"]["yawFollowScale"] = serde_json::json!(scale);
            assert!(!playback_is_valid(&playback), "{scale}");
        }
    }
}
