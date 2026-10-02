use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::fmt::Write;

use crate::rig_contract::CharacterAssetProfile;

const APPEARANCE_VISUAL_PROFILE_KEYS: &[&str] = &["gender", "visualIdentity"];

/// Keep only the fields that actually change generated pixels.
/// Gender is repeated as a hard pixel constraint. Raw onboarding requirements,
/// language, tag seeds, and persona extras stay on the profile for editing;
/// the confirmed visual identity is the sole resolved appearance source.
pub fn appearance_visual_profile(visual_profile: &Value) -> Value {
    let Some(source) = visual_profile.as_object() else {
        return visual_profile.clone();
    };
    let mut appearance = Map::new();
    for key in APPEARANCE_VISUAL_PROFILE_KEYS {
        if let Some(value) = source.get(*key) {
            let value = if *key == "visualIdentity" && !value.is_null() {
                if let Some(gender) = source.get("gender").and_then(Value::as_str)
                    && !crate::visual_prompt::visual_identity_matches_gender_presentation(
                        value, gender,
                    )
                {
                    continue;
                }
                let Some(normalized) =
                    crate::visual_prompt::normalize_visual_identity_for_prompt(value)
                else {
                    continue;
                };
                normalized
            } else {
                value.clone()
            };
            appearance.insert((*key).to_string(), value);
        }
    }
    Value::Object(appearance)
}

/// Immutable input snapshot for the generated bust master portrait.
///
/// The portrait URL identifies pixels; this contract identifies what those
/// pixels were supposed to depict and which downstream rig contract they use.
/// `slot` is always `"master"` so existing generation fingerprints remain valid.
pub fn build_character_asset_contract(
    name: &str,
    visual_profile: &Value,
    additional_requirements: Option<&str>,
) -> Value {
    let bust = CharacterAssetProfile::Bust.contract();
    json!({
        "contractVersion": bust.contract_version,
        "slot": "master",
        "identity": {
            "name": bounded_text(name, 50),
            "visualProfile": appearance_visual_profile(visual_profile),
        },
        "output": {
            "width": bust.generation_width,
            "height": bust.generation_height,
            "portraitAspect": {
                "width": bust.aspect_width,
                "height": bust.aspect_height,
            },
            "rigCanvas": {
                "width": bust.canvas_width,
                "height": bust.canvas_height,
            },
            "framing": bust.framing,
            "view": "strict-centered-eye-level-zero-yaw-front",
            "background": bust.background,
        },
        "rendering": {
            "visualSchoolVersion": crate::visual_prompt::MEROPE_VISUAL_SCHOOL_VERSION,
            "styleReferenceSha256": crate::visual_prompt::MEROPE_STYLE_REFERENCE_SHA256,
            "styleReferenceRole": "rendering-technique-only",
        },
        "rig": {
            "maxRigidArmRotationDegrees": bust.max_rigid_arm_rotation_degrees,
            "requiredCapabilities": bust.required_capabilities,
        },
        "additionalRequirements": additional_requirements
            .map(|value| bounded_text(value, 2_000))
            .filter(|value| !value.is_empty()),
    })
}

/// What a full-body set is drawn from.
#[derive(Debug, Clone, Copy)]
pub enum FullBodySource<'a> {
    /// Redrawn from a bust set's master portrait, the identity anchor.
    Bust {
        portrait: &'a str,
        generation_fingerprint: Option<&'a str>,
    },
    /// Drawn from the confirmed design with this set's outfit on, like a bust
    /// master portrait.
    Design {
        name: &'a str,
        visual_profile: &'a Value,
    },
}

/// Immutable input snapshot for a full-body set's picture: another source, or
/// a new instruction, is a new figure.
pub fn build_full_body_asset_contract(source: FullBodySource<'_>) -> Value {
    let full = CharacterAssetProfile::FullBody.contract();
    let (source, rendering) = match source {
        FullBodySource::Bust {
            portrait,
            generation_fingerprint,
        } => (
            json!({
                "bustPortrait": portrait,
                "bustGenerationFingerprint": generation_fingerprint,
            }),
            json!({
                "instructionVersion": crate::visual_prompt::FULL_BODY_PORTRAIT_INSTRUCTION_VERSION,
                "referenceRole": "bust-portrait-identity-anchor",
            }),
        ),
        FullBodySource::Design {
            name,
            visual_profile,
        } => (
            json!({
                "identity": {
                    "name": bounded_text(name, 50),
                    "visualProfile": appearance_visual_profile(visual_profile),
                },
            }),
            json!({
                "instructionVersion": crate::visual_prompt::FULL_BODY_PORTRAIT_INSTRUCTION_VERSION,
                "visualSchoolVersion": crate::visual_prompt::MEROPE_VISUAL_SCHOOL_VERSION,
                "styleReferenceSha256": crate::visual_prompt::MEROPE_STYLE_REFERENCE_SHA256,
                "styleReferenceRole": "rendering-technique-only",
            }),
        ),
    };
    json!({
        "contractVersion": full.contract_version,
        "slot": "fullBody",
        "source": source,
        "output": {
            "width": full.generation_width,
            "height": full.generation_height,
            "portraitAspect": {
                "width": full.aspect_width,
                "height": full.aspect_height,
            },
            "rigCanvas": {
                "width": full.canvas_width,
                "height": full.canvas_height,
            },
            "framing": full.framing,
            "background": full.background,
        },
        "rendering": rendering,
        "rig": {
            "maxRigidArmRotationDegrees": full.max_rigid_arm_rotation_degrees,
            "requiredCapabilities": full.required_capabilities,
        },
    })
}

pub fn character_asset_contract_fingerprint(contract: &Value) -> String {
    let encoded = serde_json::to_vec(contract).expect("character asset contract is serializable");
    let mut hasher = Sha256::new();
    hasher.update(encoded);
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut fingerprint, byte| {
            write!(&mut fingerprint, "{byte:02x}").expect("write fingerprint");
            fingerprint
        })
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete_profile(face_design: &str, material_plan: &str) -> Value {
        json!({
            "gender": "female",
            "visualIdentity": {
                "faceDesign": format!("女性化读取，{face_design}"),
                "eyeDesign": "中等偏大的紫色宝石眼，视线坚定",
                "hairShape": "银灰齐颌短发与偏分刘海",
                "hairLayerPlan": "后发、刘海和左右侧发形成独立轮廓",
                "upperBodySilhouette": "紧凑肩线、清楚领口与胸前焦点",
                "outfitConstruction": "敞开领口内搭叠短外套并止于高腰",
                "sleeveArmDesign": "左右袖片携局部前臂进入画面",
                "materialPlan": material_plan,
                "heroAccessory": "左胸星轨扣饰",
                "paletteHint": "雾蓝为主、银白为辅、金色点缀",
                "motif": "单一星轨弧线集中在胸前"
            }
        })
    }

    #[test]
    fn master_contract_carries_identity_output_and_rig_invariants() {
        let contract = build_character_asset_contract(
            " Nova ",
            &json!({ "gender": "nonbinary" }),
            Some(" gold eyes "),
        );
        assert_eq!(contract["slot"], "master");
        assert_eq!(contract["identity"]["name"], "Nova");
        assert_eq!(contract["identity"]["visualProfile"]["gender"], "nonbinary");
        assert_eq!(contract["output"]["width"], 1152);
        assert_eq!(contract["output"]["height"], 1536);
        assert_eq!(
            contract["output"]["portraitAspect"],
            json!({ "width": 3, "height": 4 })
        );
        assert_eq!(contract["output"]["rigCanvas"]["width"], 1.0);
        assert!(
            (contract["output"]["rigCanvas"]["height"].as_f64().unwrap() - (4.0 / 3.0)).abs()
                < 0.000_001
        );
        assert_eq!(contract["output"]["background"], "clean-near-white");
        assert_eq!(
            contract["rendering"]["visualSchoolVersion"],
            crate::visual_prompt::MEROPE_VISUAL_SCHOOL_VERSION
        );
        assert_eq!(
            contract["rendering"]["styleReferenceSha256"],
            crate::visual_prompt::MEROPE_STYLE_REFERENCE_SHA256
        );
        assert_eq!(
            contract["rendering"]["styleReferenceRole"],
            "rendering-technique-only"
        );
        assert_eq!(
            contract["output"]["view"],
            "strict-centered-eye-level-zero-yaw-front"
        );
        assert_eq!(
            contract["output"]["framing"],
            "close-full-head-through-lower-chest"
        );
        assert_eq!(contract["rig"]["maxRigidArmRotationDegrees"], 15.0);
        assert_eq!(character_asset_contract_fingerprint(&contract).len(), 64);
    }

    /// Every stored portrait was fingerprinted with this exact contract; a
    /// different byte here detaches every rig built before it.
    #[test]
    fn bust_contract_fingerprint_is_unchanged_by_the_profile_split() {
        let contract = build_character_asset_contract(
            "Nova",
            &json!({ "gender": "female", "visualIdentity": { "hairShape": "bob" } }),
            Some("extra"),
        );
        assert_eq!(
            character_asset_contract_fingerprint(&contract),
            "1e6c2e4e94adb0b45320230aab71fcd6a2aa4c2f441b19b7b2a77f0e0a62b641"
        );
    }

    #[test]
    fn a_full_figure_is_bound_to_what_it_was_drawn_from() {
        let bust = |portrait, fingerprint| {
            build_full_body_asset_contract(FullBodySource::Bust {
                portrait,
                generation_fingerprint: fingerprint,
            })
        };
        let a = "a".repeat(64);
        let first = bust("/media/bust.png", Some(a.as_str()));
        assert_eq!(first["slot"], "fullBody");
        assert_eq!(first["output"]["height"], 2048);
        let fingerprint = character_asset_contract_fingerprint(&first);
        assert_eq!(
            fingerprint,
            character_asset_contract_fingerprint(&bust("/media/bust.png", Some(a.as_str())))
        );
        let profile = complete_profile("soft oval face", "satin");
        let design = |profile| {
            build_full_body_asset_contract(FullBodySource::Design {
                name: "Merope",
                visual_profile: profile,
            })
        };
        assert_eq!(
            design(&profile)["rendering"]["styleReferenceRole"],
            "rendering-technique-only"
        );
        for other in [
            bust("/media/bust-2.png", Some(a.as_str())),
            bust("/media/bust.png", None),
            design(&profile),
        ] {
            assert_ne!(fingerprint, character_asset_contract_fingerprint(&other));
        }
        assert_ne!(
            character_asset_contract_fingerprint(&design(&profile)),
            character_asset_contract_fingerprint(&design(&complete_profile(
                "soft oval face",
                "wool"
            )))
        );
    }

    #[test]
    fn contract_fingerprint_changes_with_visual_identity() {
        let first = build_character_asset_contract(
            "Nova",
            &complete_profile("紧凑圆润鹅蛋脸", "哑光布料"),
            None,
        );
        let mut second_profile = complete_profile("紧凑圆润鹅蛋脸", "哑光布料");
        second_profile["visualIdentity"]["hairShape"] = json!("银灰高马尾与偏分刘海");
        let second = build_character_asset_contract("Nova", &second_profile, None);
        assert_ne!(
            character_asset_contract_fingerprint(&first),
            character_asset_contract_fingerprint(&second)
        );
    }

    #[test]
    fn contract_fingerprint_changes_with_gender_presentation() {
        let female = build_character_asset_contract(
            "Nova",
            &json!({ "gender": "female", "visualIdentity": { "hairShape": "bob" } }),
            None,
        );
        let male = build_character_asset_contract(
            "Nova",
            &json!({ "gender": "male", "visualIdentity": { "hairShape": "bob" } }),
            None,
        );
        assert_ne!(
            character_asset_contract_fingerprint(&female),
            character_asset_contract_fingerprint(&male)
        );
    }

    #[test]
    fn fingerprint_uses_the_same_canonical_identity_as_the_image_prompt() {
        let legacy = complete_profile("偏长的鹅蛋脸", "哑光布料，半写实厚涂");
        let canonical = complete_profile("紧凑柔和的鹅蛋脸", "哑光布料");
        assert_eq!(
            character_asset_contract_fingerprint(&build_character_asset_contract(
                "Nova", &legacy, None
            )),
            character_asset_contract_fingerprint(&build_character_asset_contract(
                "Nova", &canonical, None
            )),
        );
        assert_eq!(
            appearance_visual_profile(&legacy)["visualIdentity"]["character"]["faceDesign"],
            "女性化读取，紧凑柔和的鹅蛋脸"
        );
    }

    #[test]
    fn contract_never_falls_back_to_an_uncanonical_identity() {
        let invalid = json!({
            "gender": "female",
            "visualIdentity": {
                "character": {
                    "faceDesign": "三分之四视角",
                    "eyeDesign": "中等偏大紫色宝石眼",
                    "hairShape": "银灰短发",
                    "hairLayerPlan": "后发、刘海与侧发"
                },
                "outfit": {
                    "upperBodySilhouette": "紧凑肩胸",
                    "outfitConstruction": "敞开领口内搭叠短外套",
                    "sleeveArmDesign": "左右袖片",
                    "materialPlan": "哑光布料",
                    "heroAccessory": "左胸星扣",
                    "paletteHint": "雾蓝、银白、金色",
                    "motif": "星轨"
                }
            }
        });
        let appearance = appearance_visual_profile(&invalid);
        assert_eq!(appearance["gender"], "female");
        assert!(appearance.get("visualIdentity").is_none());
    }

    #[test]
    fn onboarding_source_inputs_do_not_change_portrait_fingerprint() {
        let core = json!({
            "gender": "female",
            "language": "zh-CN",
            "extraRequirements": "金色眼睛",
            "visualIdentity": { "hairShape": "短发" }
        });
        let mut with_seeds = core.clone();
        with_seeds["sourceTags"] = json!(["慢热"]);
        with_seeds["personaExtraRequirements"] = json!("话少");
        assert_eq!(
            character_asset_contract_fingerprint(&build_character_asset_contract(
                "Nova", &core, None
            )),
            character_asset_contract_fingerprint(&build_character_asset_contract(
                "Nova",
                &with_seeds,
                None
            )),
        );
        assert!(
            appearance_visual_profile(&with_seeds)
                .get("sourceTags")
                .is_none()
        );
        assert!(
            appearance_visual_profile(&with_seeds)
                .get("language")
                .is_none()
        );
        assert!(
            appearance_visual_profile(&with_seeds)
                .get("extraRequirements")
                .is_none()
        );
        assert_eq!(appearance_visual_profile(&with_seeds)["gender"], "female");
        let mut other_language = core.clone();
        other_language["language"] = json!("en-US");
        assert_eq!(
            character_asset_contract_fingerprint(&build_character_asset_contract(
                "Nova", &core, None
            )),
            character_asset_contract_fingerprint(&build_character_asset_contract(
                "Nova",
                &other_language,
                None
            )),
        );
        let mut other_raw_requirement = core.clone();
        other_raw_requirement["extraRequirements"] = json!("红色长发");
        assert_eq!(
            character_asset_contract_fingerprint(&build_character_asset_contract(
                "Nova", &core, None
            )),
            character_asset_contract_fingerprint(&build_character_asset_contract(
                "Nova",
                &other_raw_requirement,
                None
            )),
        );
    }
}
