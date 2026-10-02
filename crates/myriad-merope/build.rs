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
    let output =
        PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("merope_rig_contract.rs");
    fs::write(output, generated).expect("write generated rig contract");
}
