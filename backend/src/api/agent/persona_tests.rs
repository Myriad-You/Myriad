//! Unit tests for `persona.rs`.

/// 导入的人设不该继承上一次生成留下的视觉痕迹。
///
/// `merge_visual_profile` 会把缺席键从旧值补上，导入必须把这四个键显式写空：
/// visualIdentity / clothingStyle / sourceTags / personaExtraRequirements。
#[test]
fn an_imported_persona_inherits_nothing_from_a_generated_one() {
    let previous = json!({
        "gender": "female",
        "language": "zh-CN",
        "clothingStyle": "uniform",
        "visualIdentity": {"character": {"faceDesign": "生成出来的脸"}},
        "sourceTags": ["生成链选的词条"],
        "personaExtraRequirements": "生成链填的补充",
        "wardrobe": [{ "id": "w-old", "clothingStyle": "uniform" }],
        "activeOutfitId": "w-old"
    });
    // 本测试的 merge 输入（不是完整 OnboardingWizard 提交）。
    let incoming = json!({
        "gender": "female",
        "language": "zh-CN",
        "visualIdentity": null,
        "clothingStyle": null,
        "sourceTags": [],
        "personaExtraRequirements": ""
    });
    let merged = merge_visual_profile(
        sanitize_visual_profile(&incoming).expect("import payload is valid"),
        Some(&previous),
    );

    assert_eq!(merged["visualIdentity"], Value::Null);
    assert_eq!(merged["clothingStyle"], Value::Null);
    assert_eq!(merged["sourceTags"], json!([]));
    assert_eq!(merged["personaExtraRequirements"], json!(""));
    assert_eq!(merged["wardrobe"], json!([]));
    assert_eq!(merged["activeOutfitId"], Value::Null);
    // 身份留着：性别是导入页自己填的，不是继承来的。
    assert_eq!(merged["gender"], json!("female"));
}

fn test_outfit() -> Value {
    json!({
        "upperBodySilhouette": "窄肩与清晰领口，胸像轮廓紧凑，左右袖片伸入画面",
        "outfitConstruction": "水手领内搭叠短外套，领巾形成胸前主形，结构止于高腰",
        "sleeveArmDesign": "宽松袖口包住局部前臂，左右形状不完全对称，手可以不出现",
        "materialPlan": "哑光布料为主，丝带带柔和光泽，金属与宝石只用于小面积焦点",
        "heroAccessory": "左侧星形发夹与胸前星形扣形成一次呼应",
        "paletteHint": "粉色头发，淡紫与白为主体，深紫压边，少量金色点缀",
        "motif": "星轨与小型鸟笼，集中在发饰和胸前，不铺满服装"
    })
}

#[test]
fn wardrobe_is_kept_on_partial_visual_saves_and_cleared_with_identity() {
    let item = json!({
        "id": "w-urban",
        "clothingStyle": "urban",
        "outfit": test_outfit()
    });
    let previous = json!({
        "gender": "female",
        "clothingStyle": "urban",
        "wardrobe": [item],
        "activeOutfitId": "w-urban"
    });
    let kept = merge_visual_profile(
        sanitize_visual_profile(&json!({ "gender": "female" })).unwrap(),
        Some(&previous),
    );
    assert_eq!(kept["wardrobe"][0]["id"], "w-urban");
    assert_eq!(kept["activeOutfitId"], "w-urban");

    let cleared = merge_visual_profile(
        sanitize_visual_profile(&json!({
            "gender": "female",
            "visualIdentity": null
        }))
        .unwrap(),
        Some(&previous),
    );
    assert_eq!(cleared["wardrobe"], json!([]));
    assert_eq!(cleared["activeOutfitId"], Value::Null);
}

#[test]
fn a_full_body_set_is_never_the_worn_one() {
    let previous = json!({
        "gender": "female",
        "clothingStyle": "urban",
        "wardrobe": [
            { "id": "w-urban", "clothingStyle": "urban", "outfit": test_outfit() },
        ],
        "activeOutfitId": "w-urban"
    });
    let worn = merge_visual_profile(
        sanitize_visual_profile(&json!({
            "gender": "female",
            "wardrobe": [
                { "id": "w-urban", "clothingStyle": "urban", "outfit": test_outfit() },
                {
                    "id": "w-full",
                    "clothingStyle": "urban",
                    "outfit": test_outfit(),
                    "profile": "fullBody",
                    "referenceOutfitId": "w-urban"
                },
            ],
            "activeOutfitId": "w-full"
        }))
        .unwrap(),
        Some(&previous),
    );
    assert_eq!(worn["activeOutfitId"], Value::Null);
    assert_eq!(worn["wardrobe"][1]["profile"], "fullBody");
    assert_eq!(worn["wardrobe"][1]["referenceOutfitId"], "w-urban");
}

fn wardrobe_with_a_full_body_set() -> Value {
    json!([
        { "id": "w-urban", "clothingStyle": "urban", "outfit": test_outfit() },
        {
            "id": "w-full",
            "clothingStyle": "urban",
            "outfit": test_outfit(),
            "profile": "fullBody"
        },
    ])
}

#[test]
fn a_full_body_set_is_worn_apart_from_the_bust() {
    let previous = json!({
        "gender": "female",
        "clothingStyle": "urban",
        "wardrobe": wardrobe_with_a_full_body_set(),
        "activeOutfitId": "w-urban"
    });
    // A save names the wardrobe it wears from, as the workbench's always do.
    let worn = merge_visual_profile(
        sanitize_visual_profile(&json!({
            "gender": "female",
            "wardrobe": wardrobe_with_a_full_body_set(),
            "activeFullBodyOutfitId": "w-full"
        }))
        .unwrap(),
        Some(&previous),
    );
    assert_eq!(worn["activeFullBodyOutfitId"], "w-full");
    assert_eq!(worn["activeOutfitId"], "w-urban");

    // A bust is no full body to wear, and a full body is no bust to wear.
    let crossed = merge_visual_profile(
        sanitize_visual_profile(&json!({
            "gender": "female",
            "wardrobe": wardrobe_with_a_full_body_set(),
            "activeOutfitId": "w-full",
            "activeFullBodyOutfitId": "w-urban"
        }))
        .unwrap(),
        Some(&previous),
    );
    assert_eq!(crossed["activeOutfitId"], Value::Null);
    assert_eq!(crossed["activeFullBodyOutfitId"], Value::Null);
}

#[test]
fn the_worn_full_body_outlasts_other_saves_and_ends_with_its_set() {
    let previous = json!({
        "gender": "female",
        "clothingStyle": "urban",
        "wardrobe": wardrobe_with_a_full_body_set(),
        "activeOutfitId": "w-urban",
        "activeFullBodyOutfitId": "w-full"
    });
    // A save that does not mention it keeps it.
    let kept = merge_visual_profile(
        sanitize_visual_profile(&json!({ "gender": "female" })).unwrap(),
        Some(&previous),
    );
    assert_eq!(kept["activeFullBodyOutfitId"], "w-full");
    // Taking it off is explicit.
    let off = merge_visual_profile(
        sanitize_visual_profile(&json!({ "activeFullBodyOutfitId": null })).unwrap(),
        Some(&previous),
    );
    assert_eq!(off["activeFullBodyOutfitId"], Value::Null);
    // Deleting the set takes it off too.
    let deleted = merge_visual_profile(
        sanitize_visual_profile(&json!({
            "wardrobe": [
                { "id": "w-urban", "clothingStyle": "urban", "outfit": test_outfit() },
            ],
        }))
        .unwrap(),
        Some(&previous),
    );
    assert_eq!(deleted["activeFullBodyOutfitId"], Value::Null);
    assert_eq!(deleted["activeOutfitId"], "w-urban");
}

#[test]
fn empty_wardrobe_with_identity_becomes_the_default_outfit() {
    let identity = json!({
        "character": {
            "faceDesign": "成熟的鹅蛋脸与自然眉形",
            "eyeDesign": "金色多层虹膜与克制高光",
            "hairShape": "银灰齐颌短发与偏分刘海",
            "hairLayerPlan": "后发、刘海和左右侧发形成独立轮廓"
        },
        "outfit": test_outfit()
    });
    let profile = merge_visual_profile(
        sanitize_visual_profile(&json!({
            "gender": "female",
            "clothingStyle": "urban",
            "visualIdentity": identity,
            "wardrobe": []
        }))
        .expect("identity can seed the default outfit"),
        None,
    );
    assert_eq!(
        profile["wardrobe"][0]["id"],
        myriad_merope::DEFAULT_WARDROBE_ID
    );
    assert_eq!(
        profile["activeOutfitId"],
        myriad_merope::DEFAULT_WARDROBE_ID
    );
    assert!(profile["wardrobe"][0].get("name").is_none());

    let restored = merge_visual_profile(
        sanitize_visual_profile(&json!({
            "gender": "female",
            "clothingStyle": "idol",
            "visualIdentity": {
                "character": identity["character"],
                "outfit": test_outfit()
            },
            "wardrobe": [{
                "id": "w-new",
                "clothingStyle": "idol",
                "outfit": test_outfit()
            }],
            "activeOutfitId": "w-new"
        }))
        .expect("other outfits stay valid"),
        Some(&profile),
    );
    assert_eq!(
        restored["wardrobe"][0]["id"],
        myriad_merope::DEFAULT_WARDROBE_ID
    );
    assert_eq!(restored["wardrobe"][1]["id"], "w-new");
    assert_eq!(restored["activeOutfitId"], "w-new");

    let mut later_outfit = test_outfit();
    later_outfit["outfitConstruction"] =
        json!("敞开领口内搭叠短风衣，胸前只有一条结构线，止于高腰");
    let replaced = merge_visual_profile(
        sanitize_visual_profile(&json!({
            "gender": "female",
            "clothingStyle": "idol",
            "visualIdentity": {
                "character": identity["character"],
                "outfit": later_outfit
            },
            "wardrobe": []
        }))
        .expect("empty wardrobe is valid before merge"),
        Some(&profile),
    );
    assert_eq!(
        replaced["wardrobe"][0]["id"],
        myriad_merope::DEFAULT_WARDROBE_ID
    );
    assert_eq!(replaced["wardrobe"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        replaced["wardrobe"][0]["outfit"]["outfitConstruction"],
        profile["wardrobe"][0]["outfit"]["outfitConstruction"]
    );
}

/// 显式 `null` 才是清除。少了这一条，前端根本没有办法清掉这个字段。
#[test]
fn an_explicit_null_clears_the_clothing_style() {
    let cleared =
        sanitize_visual_profile(&json!({ "clothingStyle": null })).expect("null is a valid clear");
    assert_eq!(cleared["clothingStyle"], Value::Null);

    let kept = sanitize_visual_profile(&json!({ "clothingStyle": "uniform" }))
        .expect("a real style still normalizes");
    assert_eq!(kept["clothingStyle"], json!("uniform"));

    // 乱填仍然是 400，不会被 null 分支放过去。
    assert!(sanitize_visual_profile(&json!({ "clothingStyle": "not-a-style" })).is_err());
}

fn visual_profile_error_body(value: &Value) -> Value {
    sanitize_visual_profile(value)
        .expect_err("invalid visual profile")
        .0
        .to_json()
}

#[test]
fn visual_profile_error_names_the_failing_field() {
    let clothing = visual_profile_error_body(&json!({ "clothingStyle": "not-a-style" }));
    assert_eq!(clothing["code"], "visual_profile_invalid");
    assert_eq!(clothing["error"], "Visual profile is invalid");
    assert_eq!(
        clothing["message"],
        "clothingStyle is not a known clothing style"
    );

    let gender = visual_profile_error_body(&json!({ "gender": "unknown" }));
    assert_eq!(gender["message"], "gender is invalid");

    let mut identity = json!({
        "faceDesign": "成熟的鹅蛋脸与自然眉形",
        "eyeDesign": "金色多层虹膜与克制高光",
        "hairShape": "银灰齐颌短发与偏分刘海",
        "hairLayerPlan": "后发、刘海和左右侧发形成独立轮廓",
        "upperBodySilhouette": "紧凑肩线、清楚领口与胸前焦点",
        "outfitConstruction": "高领内搭叠短外套并止于高腰",
        "sleeveArmDesign": "左右袖片携局部前臂进入画面",
        "materialPlan": "哑光布料、银色金属与小面积宝石",
        "heroAccessory": "左胸星轨扣饰",
        "paletteHint": "雾蓝为主、银白为辅、金色点缀",
        "motif": "单一星轨弧线集中在胸前"
    });
    identity
        .as_object_mut()
        .expect("identity")
        .remove("eyeDesign");
    let missing = visual_profile_error_body(&json!({ "visualIdentity": identity }));
    assert_eq!(missing["message"], "visualIdentity.eyeDesign is empty");

    let extra = visual_profile_error_body(&json!({
        "extraRequirements": "a".repeat(myriad_merope::MAX_VISUAL_NOTES_CHARS + 1)
    }));
    assert_eq!(
        extra["message"],
        format!(
            "extraRequirements exceeds {} characters",
            myriad_merope::MAX_VISUAL_NOTES_CHARS
        )
    );

    let wardrobe = visual_profile_error_body(&json!({
        "wardrobe": [{ "id": "w-a" }]
    }));
    assert_eq!(wardrobe["message"], "wardrobe.0.clothingStyle is empty");
}

use super::*;

#[test]
fn wardrobe_face_is_readable_by_agent_users_and_does_not_wear() {
    let source = include_str!("persona.rs");
    let getter = source
        .split("/// GET /api/agent/wardrobe/{outfit_id}/face")
        .nth(1)
        .expect("wardrobe face")
        .split("/// PUT /api/agent/persona")
        .next()
        .expect("getter body");
    assert!(getter.contains("parse_user_id_with_agent_access"));
    assert!(getter.contains("wardrobe_outfit_face"));
    assert!(!getter.contains("require_site_owner"));
    assert!(!getter.contains("upsert_persona"));
    assert!(!getter.contains("persist_active_asset"));
    let routes = include_str!("routes.rs");
    assert!(routes.contains("/wardrobe/{outfit_id}/face"));
    assert!(routes.contains("get_wardrobe_face"));
}

#[test]
fn put_persona_points_the_live_rig_at_the_worn_outfit() {
    let source = include_str!("persona.rs");
    let put = source
        .split("/// PUT /api/agent/persona")
        .nth(1)
        .expect("PUT persona")
        .split("/// DELETE /api/agent/persona")
        .next()
        .expect("PUT body");
    assert!(
        put.contains("active_outfit_rig_asset_id"),
        "PUT must point the live rig at the worn outfit instead of wiping every saved package"
    );
    assert!(put.contains("persist_active_asset"));
}

#[test]
fn get_persona_returns_arousal_on_both_bodies() {
    let get = include_str!("persona.rs")
        .split("/// PUT /api/agent/persona")
        .next()
        .expect("GET persona");
    assert_eq!(
        get.matches("\"arousal\": arousal").count(),
        2,
        "empty fallback and saved persona GET must both return arousal"
    );
}

#[test]
fn visual_design_requires_an_explicit_valid_gender() {
    assert_eq!(required_visual_gender("female"), Some("female"));
    assert_eq!(required_visual_gender(" male "), Some("male"));
    assert_eq!(required_visual_gender("nonbinary"), Some("nonbinary"));
    assert_eq!(required_visual_gender("unspecified"), Some("unspecified"));
    assert_eq!(required_visual_gender(""), None);
    assert_eq!(required_visual_gender("invalid"), None);
}

#[test]
fn visual_design_requires_an_explicit_supported_language() {
    assert_eq!(required_visual_language("zh-CN"), Some("zh-CN"));
    assert_eq!(required_visual_language("zh-TW"), Some("zh-TW"));
    assert_eq!(required_visual_language("zh-HK"), Some("zh-TW"));
    assert_eq!(required_visual_language(" ja-JP "), Some("ja-JP"));
    assert_eq!(required_visual_language("en-US"), Some("en-US"));
    assert_eq!(required_visual_language(""), None);
    assert_eq!(required_visual_language("fr-FR"), None);
}

#[test]
fn draft_tags_use_onboarding_sanitize() {
    let tags = merope::api::report_dna::sanitize_onboarding_tags(&[
        "  夜战  ".into(),
        "夜战".into(),
        "喜欢独立游戏".into(),
    ]);
    assert_eq!(tags, vec!["夜战".to_string(), "喜欢独立游戏".to_string()]);
}

#[test]
fn portrait_accepts_site_assets_only() {
    assert_eq!(
        sanitize_portrait_asset_id(" /uploads/face.png "),
        Some("/uploads/face.png".to_string())
    );
    assert_eq!(
        sanitize_portrait_asset_id("asset_1-2.png"),
        Some("asset_1-2.png".to_string())
    );
    assert_eq!(sanitize_portrait_asset_id(""), Some(String::new()));
    assert_eq!(
        sanitize_portrait_asset_id("https://cdn.example.com/a.png"),
        None
    );
    assert_eq!(sanitize_portrait_asset_id("//cdn.example.com/a.png"), None);
    assert_eq!(sanitize_portrait_asset_id("javascript:alert(1)"), None);
    assert_eq!(sanitize_portrait_asset_id("/javascript:alert(1)"), None);
    assert_eq!(sanitize_portrait_asset_id("/uploads/../secret"), None);
    assert_eq!(sanitize_portrait_asset_id("face 1.png"), None);
}

#[test]
fn absent_portrait_keeps_and_null_clears() {
    let keep: PutPersonaRequest =
        serde_json::from_value(json!({ "name": "瞳", "personality": "认真" })).expect("keep");
    assert!(keep.portrait_asset_id.is_none());
    assert!(keep.persona.is_none());
    assert!(keep.visual_profile.is_none());

    let clear: PutPersonaRequest =
        serde_json::from_value(json!({ "name": "瞳", "portraitAssetId": null })).expect("clear");
    assert_eq!(clear.portrait_asset_id, Some(None));

    let clear_contracts: PutPersonaRequest = serde_json::from_value(json!({
        "name": "瞳",
        "persona": null,
        "visualProfile": null
    }))
    .expect("clear contracts");
    assert_eq!(clear_contracts.persona, Some(None));
    assert_eq!(clear_contracts.visual_profile, Some(None));

    let set: PutPersonaRequest =
        serde_json::from_value(json!({ "name": "瞳", "portraitAssetId": "/a.png" })).expect("set");
    assert_eq!(set.portrait_asset_id, Some(Some("/a.png".to_string())));
}

#[test]
fn visual_profile_keeps_generation_inputs_separate_from_spoken_persona() {
    let profile = sanitize_visual_profile(&json!({
        "gender": "nonbinary",
        "language": "zh-Hans",
        "clothingStyle": "fantasy",
        "extraRequirements": "金色眼睛",
        "visualIdentity": {
            "faceDesign": "成熟的鹅蛋脸与自然眉形",
            "eyeDesign": "金色多层虹膜与克制高光",
            "hairShape": "银灰齐颌短发与偏分刘海",
            "hairLayerPlan": "后发、刘海和左右侧发形成独立轮廓",
            "upperBodySilhouette": "紧凑肩线、清楚领口与胸前焦点",
            "outfitConstruction": "高领内搭叠短外套并止于高腰",
            "sleeveArmDesign": "左右袖片携局部前臂进入画面",
            "materialPlan": "哑光布料、银色金属与小面积宝石",
            "heroAccessory": "左胸星轨扣饰",
            "paletteHint": "雾蓝为主、银白为辅、金色点缀",
            "motif": "单一星轨弧线集中在胸前"
        }
    }))
    .expect("valid profile");
    assert_eq!(profile["language"], "zh-CN");
    assert_eq!(profile["clothingStyle"], "fantasy");
    assert_eq!(profile["extraRequirements"], "金色眼睛");
    assert!(profile["visualIdentity"].get("character").is_some());
    assert!(profile["visualIdentity"].get("outfit").is_some());
    assert_eq!(
        profile["visualIdentity"]["outfit"]["clothingStyle"],
        "fantasy"
    );
    assert_eq!(
        profile["visualIdentity"]["character"]["faceDesign"],
        "中性。成熟的鹅蛋脸与自然眉形"
    );
    assert!(myriad_merope::upper_body_visual_identity_is_complete(
        &profile["visualIdentity"]
    ));

    let kept = merge_visual_profile(
        sanitize_visual_profile(&json!({
            "gender": "nonbinary",
            "language": "zh-CN",
            "sourceTags": [" 慢热 ", "慢热", "嘴硬心软"]
        }))
        .expect("partial profile"),
        Some(&profile),
    );
    assert_eq!(kept["gender"], "nonbinary");
    assert_eq!(kept["sourceTags"], json!(["慢热", "嘴硬心软"]));
    assert_eq!(kept["visualIdentity"], profile["visualIdentity"]);
    assert_eq!(kept["clothingStyle"], "fantasy");
    assert!(kept.get("extraRequirements").is_none());
    assert!(kept.get("personaExtraRequirements").is_none());
}

#[test]
fn visual_profile_explicit_clears_survive_merge_and_context_changes_drop_identity() {
    let previous = json!({
        "gender": "female",
        "language": "zh-CN",
        "clothingStyle": "fantasy",
        "extraRequirements": "金色眼睛",
        "sourceTags": ["慢热"],
        "personaExtraRequirements": "话少",
        "visualIdentity": {
            "character": {
                "faceDesign": "紧凑柔和的鹅蛋脸与自然眉形",
                "eyeDesign": "中等偏大的金色多层虹膜与克制高光",
                "hairShape": "银灰齐颌短发与偏分刘海",
                "hairLayerPlan": "后发、刘海和左右侧发形成独立轮廓"
            },
            "outfit": {
                "clothingStyle": "fantasy",
                "upperBodySilhouette": "紧凑肩线、清楚领口与胸前焦点",
                "outfitConstruction": "高领内搭叠短外套并止于高腰",
                "sleeveArmDesign": "左右袖片携局部前臂进入画面",
                "materialPlan": "哑光布料、银色金属与小面积宝石",
                "heroAccessory": "左胸星轨扣饰",
                "paletteHint": "雾蓝为主、银白为辅、金色点缀",
                "motif": "单一星轨弧线集中在胸前"
            }
        }
    });
    let cleared = merge_visual_profile(
        sanitize_visual_profile(&json!({
            "gender": "female",
            "language": "zh-CN",
            "clothingStyle": "fantasy",
            "extraRequirements": "",
            "sourceTags": [],
            "personaExtraRequirements": "",
            "visualIdentity": null
        }))
        .expect("explicit clears"),
        Some(&previous),
    );
    assert_eq!(cleared["extraRequirements"], "");
    assert_eq!(cleared["sourceTags"], json!([]));
    assert_eq!(cleared["personaExtraRequirements"], "");
    assert!(cleared["visualIdentity"].is_null());

    let changed_gender = merge_visual_profile(
        sanitize_visual_profile(&json!({
            "gender": "male",
            "language": "zh-CN"
        }))
        .expect("changed context"),
        Some(&previous),
    );
    assert!(changed_gender.get("visualIdentity").is_none());
    assert_eq!(changed_gender["clothingStyle"], "fantasy");
}

#[test]
fn visual_profile_copies_outfit_clothing_style_to_root() {
    let profile = sanitize_visual_profile(&json!({
        "gender": "female",
        "language": "zh-CN",
        "visualIdentity": {
            "character": {
                "faceDesign": "成熟的鹅蛋脸与自然眉形",
                "eyeDesign": "金色多层虹膜与克制高光",
                "hairShape": "银灰齐颌短发与偏分刘海",
                "hairLayerPlan": "后发、刘海和左右侧发形成独立轮廓"
            },
            "outfit": {
                "clothingStyle": "japanese",
                "upperBodySilhouette": "紧凑肩线、清楚领口与胸前焦点",
                "outfitConstruction": "高领内搭叠短外套并止于高腰",
                "sleeveArmDesign": "左右袖片携局部前臂进入画面",
                "materialPlan": "哑光布料、银色金属与小面积宝石",
                "heroAccessory": "左胸星轨扣饰",
                "paletteHint": "雾蓝为主、银白为辅、金色点缀",
                "motif": "单一星轨弧线集中在胸前"
            }
        }
    }))
    .expect("modular profile");
    assert_eq!(profile["clothingStyle"], "japanese");
    assert_eq!(
        profile["visualIdentity"]["outfit"]["clothingStyle"],
        "japanese"
    );
    assert_eq!(
        myriad_merope::character_module(&profile["visualIdentity"]).unwrap()["faceDesign"],
        "女性化。成熟的鹅蛋脸与自然眉形"
    );
}

#[test]
fn visual_profile_keeps_persona_seeds() {
    let profile = sanitize_visual_profile(&json!({
        "gender": "male",
        "language": "en-US",
        "personaExtraRequirements": "quieter with strangers",
        "sourceTags": ["Night owl", "Clear boundaries"]
    }))
    .expect("seeds");
    assert_eq!(
        profile["personaExtraRequirements"],
        "quieter with strangers"
    );
    assert_eq!(
        profile["sourceTags"],
        json!(["Night owl", "Clear boundaries"])
    );

    let persona = sanitize_structured_persona(
        "瞳",
        &json!({
            "summary": "安静但对新事物有持续好奇心",
            "temperament": ["安静", "好奇"],
            "likes": ["雨声"],
            "drives": ["理解彼此"],
            "socialStyle": "先听，再回应。",
            "speechStyle": "简洁但温和。"
        }),
        Some(&profile),
    )
    .expect("valid persona");
    assert_eq!(persona["summary"], "安静但对新事物有持续好奇心");
    assert!(persona.get("gender").is_none());
}
