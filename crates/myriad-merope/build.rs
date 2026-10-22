use std::{collections::HashSet, env, fs, path::PathBuf};

use serde_json::Value;

fn f32_lit(value: f64) -> String {
    let value = value as f32;
    let mut text = format!("{value:?}");
    if !text.contains('.') && !text.contains('e') && !text.contains('E') {
        text.push_str(".0");
    }
    format!("{text}_f32")
}

fn main() {
    let contract_path = PathBuf::from("../../shared/merope_rig_contract.json");
    let performance_contract_path = PathBuf::from("../../shared/merope_performance_contract.json");
    println!("cargo:rerun-if-changed={}", contract_path.display());
    println!(
        "cargo:rerun-if-changed={}",
        performance_contract_path.display()
    );
    let contract: Value = serde_json::from_str(
        &fs::read_to_string(&contract_path).expect("read merope rig contract"),
    )
    .expect("parse merope rig contract");
    let performance_contract: Value = serde_json::from_str(
        &fs::read_to_string(&performance_contract_path).expect("read merope performance contract"),
    )
    .expect("parse merope performance contract");
    let number = |path: &[&str]| -> u64 {
        path.iter()
            .fold(&contract, |value, key| &value[*key])
            .as_u64()
            .unwrap_or_else(|| panic!("missing numeric rig contract field: {}", path.join(".")))
    };
    let mut generated = format!(
        "pub const RIG_SCHEMA_VERSION: u8 = {};\n\
         pub const RIG_IR_VERSION: u16 = {};\n\
         pub const MIN_SUPPORTED_RIG_IR_VERSION: u16 = {};\n\
         pub const MAX_RIG_BONES: usize = {};\n\
         pub const MAX_GPU_RIG_BONES: usize = {};\n\
         pub const MAX_RIG_TEXTURES: usize = {};\n\
         pub const MAX_RIG_PARTS: usize = {};\n\
         pub const MAX_RIG_VERTICES_PER_PART: usize = {};\n\
         pub const MAX_RIG_TOTAL_VERTICES: usize = {};\n\
         pub const MAX_RIG_COLLISION_VOLUMES: usize = {};\n",
        number(&["schemaVersion"]),
        number(&["rigIrVersion"]),
        number(&["minSupportedRigIrVersion"]),
        number(&["limits", "maxBones"]),
        number(&["limits", "maxGpuBones"]),
        number(&["limits", "maxTextures"]),
        number(&["limits", "maxParts"]),
        number(&["limits", "maxVerticesPerPart"]),
        number(&["limits", "maxTotalVertices"]),
        number(&["limits", "maxCollisionVolumes"]),
    );
    assert!(
        number(&["limits", "maxGpuBones"]) >= number(&["limits", "maxBones"]),
        "GPU rig capacity must cover every accepted manifest bone"
    );
    assert!(
        number(&["minSupportedRigIrVersion"]) <= number(&["rigIrVersion"]),
        "minimum supported Rig IR cannot exceed the current version"
    );
    let profiles = contract["characterAsset"]["profiles"]
        .as_object()
        .expect("missing characterAsset.profiles");
    let mut profile_names: Vec<&str> = profiles.keys().map(String::as_str).collect();
    profile_names.sort_unstable();
    assert_eq!(
        profile_names,
        ["bust", "fullBody"],
        "character asset profiles must be exactly the ones CharacterAssetProfile names"
    );
    for (profile, constant) in [
        ("bust", "BUST_ASSET_CONTRACT"),
        ("fullBody", "FULL_BODY_ASSET_CONTRACT"),
    ] {
        let definition = &profiles[profile];
        let portrait = |group: &str, axis: &str| {
            number(&[
                "characterAsset",
                "profiles",
                profile,
                "portrait",
                group,
                axis,
            ])
        };
        let float = |value: &Value, field: &str| {
            f32_lit(
                value
                    .as_f64()
                    .unwrap_or_else(|| panic!("missing {profile}.{field}")),
            )
        };
        let text = |value: &Value, field: &str| {
            serde_json::to_string(
                value
                    .as_str()
                    .unwrap_or_else(|| panic!("missing {profile}.{field}")),
            )
            .expect("serialize profile text")
        };
        assert_eq!(
            portrait("aspect", "width") * portrait("generationPixels", "height"),
            portrait("aspect", "height") * portrait("generationPixels", "width"),
            "{profile} portrait generation dimensions must match its aspect"
        );
        let capabilities = definition["rig"]["requiredCapabilities"]
            .as_array()
            .unwrap_or_else(|| panic!("missing {profile}.rig.requiredCapabilities"))
            .iter()
            .map(|capability| text(capability, "rig.requiredCapabilities"))
            .collect::<Vec<_>>()
            .join(", ");
        generated.push_str(&format!(
            "pub const {constant}: CharacterAssetProfileContract = CharacterAssetProfileContract {{\n    \
             contract_version: {},\n    \
             aspect_width: {},\n    \
             aspect_height: {},\n    \
             generation_width: {},\n    \
             generation_height: {},\n    \
             canvas_width: {},\n    \
             canvas_height: {},\n    \
             framing: {},\n    \
             background: {},\n    \
             max_rigid_arm_rotation_degrees: {},\n    \
             required_capabilities: &[{capabilities}],\n}};\n",
            number(&["characterAsset", "profiles", profile, "contractVersion"]),
            portrait("aspect", "width"),
            portrait("aspect", "height"),
            portrait("generationPixels", "width"),
            portrait("generationPixels", "height"),
            float(&definition["portrait"]["canvas"]["width"], "portrait.canvas.width"),
            float(&definition["portrait"]["canvas"]["height"], "portrait.canvas.height"),
            text(&definition["portrait"]["framing"], "portrait.framing"),
            text(&definition["portrait"]["background"], "portrait.background"),
            float(
                &definition["rig"]["maxRigidArmRotationDegrees"],
                "rig.maxRigidArmRotationDegrees"
            ),
        ));
    }
    let secondary_patterns = contract["secondaryPartPatterns"]
        .as_array()
        .expect("missing secondaryPartPatterns");
    generated.push_str("pub const SECONDARY_PART_PATTERNS: &[&str] = &[\n");
    for pattern in secondary_patterns {
        generated.push_str(&format!(
            "    {},\n",
            serde_json::to_string(
                pattern
                    .as_str()
                    .expect("secondaryPartPatterns values must be strings")
            )
            .expect("serialize secondary part pattern")
        ));
    }
    generated.push_str("];\n");
    let presentation_slots = contract["presentationSlots"]
        .as_object()
        .expect("missing presentationSlots");
    generated.push_str("pub const PRESENTATION_SLOT_VARIANTS: &[(&str, &str, &[&str])] = &[\n");
    for (slot, definition) in presentation_slots {
        let fallback = definition["fallback"]
            .as_str()
            .unwrap_or_else(|| panic!("missing presentationSlots.{slot}.fallback"));
        let variants = definition["variants"]
            .as_array()
            .unwrap_or_else(|| panic!("missing presentationSlots.{slot}.variants"));
        assert!(
            variants
                .iter()
                .any(|variant| variant.as_str() == Some(fallback)),
            "presentation slot fallback must be an allowed variant: {slot}"
        );
        generated.push_str(&format!(
            "    ({}, {}, &[",
            serde_json::to_string(slot).expect("serialize presentation slot"),
            serde_json::to_string(fallback).expect("serialize presentation fallback")
        ));
        for variant in variants {
            generated.push_str(&format!(
                "{}, ",
                serde_json::to_string(variant.as_str().unwrap_or_else(|| panic!(
                    "presentationSlots.{slot}.variants must be strings"
                )))
                .expect("serialize presentation variant")
            ));
        }
        generated.push_str("]),\n");
    }
    generated.push_str("];\n");
    for (field, constant) in [
        ("semanticBoneRoles", "SEMANTIC_BONE_ROLES"),
        ("semanticChainRoles", "SEMANTIC_CHAIN_ROLES"),
    ] {
        let values = contract[field]
            .as_array()
            .unwrap_or_else(|| panic!("missing {field}"));
        generated.push_str(&format!("pub const {constant}: &[&str] = &[\n"));
        for value in values {
            generated.push_str(&format!(
                "    {},\n",
                serde_json::to_string(
                    value
                        .as_str()
                        .unwrap_or_else(|| panic!("{field} values must be strings"))
                )
                .expect("serialize semantic role")
            ));
        }
        generated.push_str("];\n");
    }
    for (topology, constant) in [
        ("fitted", "FITTED_OUTFIT_RULE"),
        ("short-skirt", "SHORT_SKIRT_OUTFIT_RULE"),
        ("long-skirt", "LONG_SKIRT_OUTFIT_RULE"),
        ("long-coat", "LONG_COAT_OUTFIT_RULE"),
        ("wide-sleeve", "WIDE_SLEEVE_OUTFIT_RULE"),
        ("cape", "CAPE_OUTFIT_RULE"),
        ("armor", "ARMOR_OUTFIT_RULE"),
    ] {
        let rule = &contract["outfitSafety"][topology];
        let scalar = |field: &str| {
            rule[field]
                .as_f64()
                .unwrap_or_else(|| panic!("missing {topology}.{field}"))
        };
        generated.push_str(&format!(
            "pub const {constant}: RigOutfitSafetyContract = RigOutfitSafetyContract {{ torso_twist_scale: {}, secondary_motion_scale: {} }};\n",
            f32_lit(scalar("torsoTwistScale")),
            f32_lit(scalar("secondaryMotionScale")),
        ));
    }
    let performance_intents = performance_contract["cueIntents"]
        .as_array()
        .expect("missing performance cueIntents");
    let performance_priorities = performance_contract["cuePriorities"]
        .as_object()
        .expect("missing performance cuePriorities");
    assert_eq!(
        performance_intents.len(),
        performance_priorities.len(),
        "every performance cue intent must have exactly one priority"
    );
    let mut performance_intent_names = HashSet::new();
    for intent in performance_intents {
        let intent = intent.as_str().expect("cueIntents values must be strings");
        assert!(
            performance_intent_names.insert(intent),
            "performance cue intents must be unique: {intent}"
        );
        assert!(
            performance_priorities
                .get(intent)
                .and_then(Value::as_u64)
                .is_some_and(|priority| (1..=3).contains(&priority)),
            "performance cue priority must be 1..=3: {intent}"
        );
    }
    assert!(
        performance_priorities
            .keys()
            .all(|intent| performance_intent_names.contains(intent.as_str())),
        "performance cue priorities cannot contain unknown intents"
    );
    for (field, constant) in [
        ("baselineExpressions", "PERFORMANCE_BASELINE_EXPRESSIONS"),
        ("postures", "PERFORMANCE_POSTURES"),
        ("phraseIntents", "PERFORMANCE_PHRASE_INTENTS"),
        ("cueIntents", "PERFORMANCE_CUE_INTENTS"),
        ("interruptModes", "PERFORMANCE_INTERRUPT_MODES"),
    ] {
        let values = performance_contract[field]
            .as_array()
            .unwrap_or_else(|| panic!("missing performance {field}"));
        generated.push_str(&format!("pub const {constant}: &[&str] = &[\n"));
        for value in values {
            generated.push_str(&format!(
                "    {},\n",
                serde_json::to_string(
                    value
                        .as_str()
                        .unwrap_or_else(|| panic!("performance {field} values must be strings"))
                )
                .expect("serialize performance contract value")
            ));
        }
        generated.push_str("];\n");
    }
    generated
        .push_str("pub const PERFORMANCE_BODY_CONTROLS: &[(&str, f32, f32, &str, &str)] = &[\n");
    for (name, control) in performance_contract["bodyControls"]
        .as_object()
        .expect("bodyControls")
    {
        let min = control["min"].as_f64().expect("body control min");
        let max = control["max"].as_f64().expect("body control max");
        assert!(min.is_finite() && max.is_finite() && min <= max);
        generated.push_str(&format!(
            "({}, {}, {}, {}, {}),\n",
            serde_json::to_string(name).unwrap(),
            f32_lit(min),
            f32_lit(max),
            control["capability"],
            control["description"]
        ));
    }
    generated.push_str("];\n");
    for (key, name) in [
        ("transitionMs", "BODY_POSE_TRANSITION_MS"),
        ("holdMs", "BODY_POSE_HOLD_MS"),
        ("minTransitionMs", "BODY_POSE_MIN_TRANSITION_MS"),
        ("maxTransitionMs", "BODY_POSE_MAX_TRANSITION_MS"),
        ("maxHoldMs", "BODY_POSE_MAX_HOLD_MS"),
    ] {
        generated.push_str(&format!(
            "pub const {name}: u32 = {};\n",
            performance_contract["bodyPoseTiming"][key]
                .as_u64()
                .expect(key)
        ));
    }
    for (field, constant) in [
        ("scoreSides", "PERFORMANCE_SCORE_SIDES"),
        ("scoreDirections", "PERFORMANCE_SCORE_DIRECTIONS"),
    ] {
        let values = performance_contract[field]
            .as_array()
            .unwrap_or_else(|| panic!("missing performance {field}"));
        generated.push_str(&format!("pub const {constant}: &[&str] = &[\n"));
        for value in values {
            generated.push_str(&format!(
                "    {},\n",
                serde_json::to_string(value.as_str().expect("score enum values must be strings"))
                    .unwrap()
            ));
        }
        generated.push_str("];\n");
    }
    let body_controls = performance_contract["bodyControls"]
        .as_object()
        .expect("bodyControls");
    // The director names how far a control goes, not a number: each tier is a
    // share of the way from the control's rest to the end the sign points at.
    generated.push_str("pub const PERFORMANCE_EXTENT_TIERS: &[(&str, f32)] = &[\n");
    let mut tiers: Vec<(&String, f64)> = performance_contract["extentTiers"]
        .as_object()
        .expect("missing performance extentTiers")
        .iter()
        .map(|(tier, share)| (tier, share.as_f64().expect("extent tier share")))
        .collect();
    // Smallest first, as the director reads them.
    tiers.sort_by(|a, b| a.1.total_cmp(&b.1));
    for (tier, share) in tiers {
        assert!(share > 0.0 && share <= 1.0, "extent tier {tier} must be a share in (0, 1]");
        generated.push_str(&format!(
            "    ({}, {}),\n",
            serde_json::to_string(tier).unwrap(),
            f32_lit(share)
        ));
    }
    generated.push_str("];\n");
    // A control whose drawn look does not grow evenly names its own tiers: a
    // drawing swapped in shows ghosted halfway, so it has only its full one.
    // Named levels the director picks for baseline energy and attention, a
    // cue's intensity and a beat's tempo, instead of writing a number.
    for (field, constant) in [
        ("motionEnergy", "PERFORMANCE_ENERGY_LEVELS"),
        ("attention", "PERFORMANCE_ATTENTION_LEVELS"),
        ("cueIntensity", "PERFORMANCE_INTENSITY_LEVELS"),
        ("tempo", "PERFORMANCE_TEMPO_LEVELS"),
    ] {
        let mut levels: Vec<(&String, f64)> = performance_contract["levels"][field]
            .as_object()
            .unwrap_or_else(|| panic!("missing performance levels.{field}"))
            .iter()
            .map(|(name, value)| (name, value.as_f64().expect("level value")))
            .collect();
        levels.sort_by(|a, b| a.1.total_cmp(&b.1));
        generated.push_str(&format!("pub const {constant}: &[(&str, f32)] = &[\n"));
        for (name, value) in levels {
            generated.push_str(&format!(
                "    ({}, {}),\n",
                serde_json::to_string(name).unwrap(),
                f32_lit(value)
            ));
        }
        generated.push_str("];\n");
    }
    generated.push_str(
        "pub const PERFORMANCE_BODY_CONTROL_TIERS: &[(&str, &[(&str, f32)])] = &[\n",
    );
    for (name, control) in body_controls {
        let Some(own) = control.get("tiers") else {
            continue;
        };
        let mut own: Vec<(&String, f64)> = own
            .as_object()
            .expect("body control tiers")
            .iter()
            .map(|(tier, share)| (tier, share.as_f64().expect("body control tier share")))
            .collect();
        own.sort_by(|a, b| a.1.total_cmp(&b.1));
        let own: Vec<String> = own
            .iter()
            .map(|(tier, share)| {
                assert!(
                    performance_contract["extentTiers"].get(tier.as_str()).is_some(),
                    "body control {name} names unknown tier {tier}"
                );
                assert!(*share > 0.0 && *share <= 1.0, "body control {name} tier {tier}");
                format!("({}, {})", serde_json::to_string(tier).unwrap(), f32_lit(*share))
            })
            .collect();
        generated.push_str(&format!(
            "    ({}, &[{}]),\n",
            serde_json::to_string(name).unwrap(),
            own.join(", ")
        ));
    }
    generated.push_str("];\n");
    generated.push_str("pub const PERFORMANCE_BODY_CONTROL_RESTS: &[(&str, f32)] = &[\n");
    for (name, control) in body_controls {
        let min = control["min"].as_f64().expect("body control min");
        let max = control["max"].as_f64().expect("body control max");
        let rest = control
            .get("rest")
            .map_or(0.0_f64.clamp(min, max), |rest| rest.as_f64().expect("body control rest"));
        assert!((min..=max).contains(&rest), "body control {name} must rest within its range");
        generated.push_str(&format!(
            "    ({}, {}),\n",
            serde_json::to_string(name).unwrap(),
            f32_lit(rest)
        ));
    }
    generated.push_str("];\n");
    generated.push_str(
        "pub const PERFORMANCE_SCORE_MOVES: &[(&str, &[&str], &str)] = &[\n",
    );
    for (name, scored) in performance_contract["scoreMoves"]
        .as_object()
        .expect("missing performance scoreMoves")
    {
        let controls = scored["controls"].as_array().expect("score move controls");
        assert!(!controls.is_empty(), "score move {name} must move something");
        let controls: Vec<String> = controls
            .iter()
            .map(|control| {
                let control = control.as_str().expect("score move control must be a string");
                assert!(
                    body_controls.contains_key(control),
                    "score move {name} uses unknown body control {control}"
                );
                serde_json::to_string(control).unwrap()
            })
            .collect();
        generated.push_str(&format!(
            "({}, &[{}], {}),\n",
            serde_json::to_string(name).unwrap(),
            controls.join(", "),
            scored["description"]
        ));
    }
    generated.push_str("];\n");
    for (key, name) in [
        ("maxBeats", "SCORE_MAX_BEATS"),
        ("maxAtMs", "SCORE_MAX_AT_MS"),
        ("maxOffsetMs", "SCORE_MAX_OFFSET_MS"),
        ("maxCount", "SCORE_MAX_COUNT"),
        ("maxAnchorChars", "SCORE_MAX_ANCHOR_CHARS"),
    ] {
        generated.push_str(&format!(
            "pub const {name}: u32 = {};\n",
            performance_contract["scoreTiming"][key].as_u64().expect(key)
        ));
    }
    for (key, name) in [("minTempo", "SCORE_MIN_TEMPO"), ("maxTempo", "SCORE_MAX_TEMPO")] {
        generated.push_str(&format!(
            "pub const {name}: f32 = {};\n",
            f32_lit(performance_contract["scoreTiming"][key].as_f64().expect(key))
        ));
    }
    // Looks drawn from her own controls, named so the director can pick a
    // whole face at once; every control and tier must exist.
    generated.push_str(
        "/// (name, look, [(control, tier)])\n\
         pub type FacePreset = (&'static str, &'static str, &'static [(&'static str, &'static str)]);\n\
         pub const PERFORMANCE_FACE_PRESETS: &[FacePreset] = &[\n",
    );
    for (name, preset) in performance_contract["facePresets"]
        .as_object()
        .expect("missing performance facePresets")
    {
        let description = preset["description"]
            .as_str()
            .unwrap_or_else(|| panic!("face preset {name} needs a description"));
        let targets: Vec<String> = preset["targets"]
            .as_object()
            .unwrap_or_else(|| panic!("face preset {name} needs targets"))
            .iter()
            .map(|(control, tier)| {
                let tier = tier.as_str().expect("face preset tier");
                let body = body_controls
                    .get(control)
                    .unwrap_or_else(|| panic!("face preset {name} names unknown control {control}"));
                let strength = tier.trim_start_matches('-');
                let named = body
                    .get("tiers")
                    .unwrap_or(&performance_contract["extentTiers"])
                    .get(strength)
                    .is_some();
                let rest = body.get("rest").and_then(|r| r.as_f64()).unwrap_or(0.0_f64.clamp(
                    body["min"].as_f64().unwrap(),
                    body["max"].as_f64().unwrap(),
                ));
                let room = if tier.starts_with('-') {
                    body["min"].as_f64().unwrap() < rest
                } else {
                    body["max"].as_f64().unwrap() > rest
                };
                assert!(named && room, "face preset {name}: {control} has no tier {tier}");
                format!(
                    "({}, {})",
                    serde_json::to_string(control).unwrap(),
                    serde_json::to_string(tier).unwrap()
                )
            })
            .collect();
        generated.push_str(&format!(
            "    ({}, {}, &[{}]),\n",
            serde_json::to_string(name).unwrap(),
            serde_json::to_string(description).unwrap(),
            targets.join(", ")
        ));
    }
    generated.push_str("];\n");
    generated.push_str("/// How she gets from one look to the next: (name, description).\n");
    generated.push_str("pub const PERFORMANCE_FACE_SHIFTS: &[(&str, &str)] = &[\n");
    for (name, shift) in performance_contract["faceShifts"]
        .as_object()
        .expect("missing performance faceShifts")
    {
        let description = shift["description"]
            .as_str()
            .unwrap_or_else(|| panic!("face shift {name} needs a description"));
        for key in ["durationMs", "staggerMs", "pauseMs", "passMs"] {
            assert!(
                shift[key].as_u64().is_some(),
                "face shift {name} needs {key}"
            );
        }
        let damping = shift["damping"].as_f64().unwrap_or(-1.0);
        assert!(
            damping > 0.0 && damping <= 1.0,
            "face shift {name} needs damping in (0, 1]"
        );
        for key in ["anticipation", "arc"] {
            assert!(
                shift[key].as_f64().is_some_and(|v| v >= 0.0),
                "face shift {name} needs {key} >= 0"
            );
        }
        generated.push_str(&format!(
            "    ({}, {}),\n",
            serde_json::to_string(name).unwrap(),
            serde_json::to_string(description).unwrap()
        ));
    }
    generated.push_str("];\n");
    let output =
        PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("merope_rig_contract.rs");
    fs::write(output, generated).expect("write generated rig contract");
}
