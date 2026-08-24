use std::{env, fs, path::PathBuf};

use serde_json::Value;

fn main() {
    let contract_path = PathBuf::from("../../shared/merope_rig_contract.json");
    println!("cargo:rerun-if-changed={}", contract_path.display());
    let contract: Value = serde_json::from_str(
        &fs::read_to_string(&contract_path).expect("read merope rig contract"),
    )
    .expect("parse merope rig contract");
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
         pub const CHARACTER_ASSET_CONTRACT_VERSION: u16 = {};\n\
         pub const PORTRAIT_ASPECT_WIDTH: u32 = {};\n\
         pub const PORTRAIT_ASPECT_HEIGHT: u32 = {};\n\
         pub const PORTRAIT_GENERATION_WIDTH: u32 = {};\n\
         pub const PORTRAIT_GENERATION_HEIGHT: u32 = {};\n\
         pub const PORTRAIT_CANVAS_WIDTH: f32 = {:.8}_f32;\n\
         pub const PORTRAIT_CANVAS_HEIGHT: f32 = {:.8}_f32;\n\
         pub const MAX_RIGID_ARM_ROTATION_DEGREES: f32 = {:.8}_f32;\n\
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
        number(&["characterAsset", "contractVersion"]),
        number(&["characterAsset", "portrait", "aspect", "width"]),
        number(&["characterAsset", "portrait", "aspect", "height"]),
        number(&["characterAsset", "portrait", "generationPixels", "width"]),
        number(&["characterAsset", "portrait", "generationPixels", "height"]),
        contract["characterAsset"]["portrait"]["canvas"]["width"]
            .as_f64()
            .expect("missing characterAsset.portrait.canvas.width"),
        contract["characterAsset"]["portrait"]["canvas"]["height"]
            .as_f64()
            .expect("missing characterAsset.portrait.canvas.height"),
        contract["characterAsset"]["rig"]["maxRigidArmRotationDegrees"]
            .as_f64()
            .expect("missing characterAsset.rig.maxRigidArmRotationDegrees"),
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
    assert_eq!(
        number(&["characterAsset", "portrait", "aspect", "width"])
            * number(&["characterAsset", "portrait", "generationPixels", "height"]),
        number(&["characterAsset", "portrait", "aspect", "height"])
            * number(&["characterAsset", "portrait", "generationPixels", "width"]),
        "portrait generation dimensions must match the canonical aspect"
    );
    let required_capabilities = contract["characterAsset"]["rig"]["requiredCapabilities"]
        .as_array()
        .expect("missing characterAsset.rig.requiredCapabilities");
    generated.push_str("pub const CHARACTER_ASSET_REQUIRED_CAPABILITIES: &[&str] = &[\n");
    for capability in required_capabilities {
        generated.push_str(&format!(
            "    {},\n",
            serde_json::to_string(
                capability
                    .as_str()
                    .expect("requiredCapabilities values must be strings")
            )
            .expect("serialize required character asset capability")
        ));
    }
    generated.push_str("];\n");
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
            "pub const {constant}: RigOutfitSafetyContract = RigOutfitSafetyContract {{ torso_twist_scale: {:.8}_f32, secondary_motion_scale: {:.8}_f32 }};\n",
            scalar("torsoTwistScale"),
            scalar("secondaryMotionScale"),
        ));
    }
    let output =
        PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("merope_rig_contract.rs");
    fs::write(output, generated).expect("write generated rig contract");
}
