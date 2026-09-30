//! Unit tests for `onboarding_ai.rs`, kept beside it so the module stays readable.

use super::*;

#[test]
fn observed_portrait_visual_keeps_clothing_style_and_fields() {
    let raw = r#"{
            "clothingStyle": "urban",
            "visualIdentity": {
                "character": {
                    "faceDesign": "柔和的鹅蛋脸，鼻唇简洁，面部比例成熟而非幼态",
                    "eyeDesign": "紫蓝宝石感大眼，深色上睫与多层虹膜高光",
                    "hairShape": "粉色齐颌短发，空气刘海，侧发包住脸颊",
                    "hairLayerPlan": "后发形成完整轮廓，前刘海、左右侧发和顶部呆毛可分层"
                },
                "outfit": {
                    "upperBodySilhouette": "窄肩与清晰领口，胸像轮廓紧凑，左右袖片伸入画面",
                    "outfitConstruction": "水手领内搭叠短外套，领巾形成胸前主形，结构止于高腰",
                    "sleeveArmDesign": "宽松袖口包住局部前臂，左右形状不完全对称，手可以不出现",
                    "materialPlan": "哑光布料为主，丝带带柔和光泽，金属与宝石只用于小面积焦点",
                    "heroAccessory": "左侧星形发夹与胸前星形扣形成一次呼应",
                    "paletteHint": "粉色头发，淡紫与白为主体，深紫压边，少量金色点缀",
                    "motif": "星轨与小型鸟笼，集中在发饰和胸前，不铺满服装"
                }
            }
        }"#;
    let observed = parse_observed_visual(raw, "zh-CN", "female").expect("usable observation");
    assert_eq!(observed.clothing_style, "urban");
    assert!(
        observed.visual_identity["character"]["faceDesign"]
            .as_str()
            .unwrap_or("")
            .starts_with("女性化。")
    );
    assert_eq!(observed.visual_identity["outfit"]["clothingStyle"], "urban");
}

#[test]
fn keep_character_without_requirements_reuses_existing_outfit_palette() {
    let identity = json!({
        "character": {
            "faceDesign": "女性化鹅蛋脸",
            "eyeDesign": "紫色眼睛",
            "hairShape": "银灰短发",
            "hairLayerPlan": "后发、刘海、侧发"
        },
        "outfit": {
            "paletteHint": "淡紫与白为主体，深紫压边"
        }
    });
    assert_eq!(
        existing_outfit_palette_hint(Some(&identity)),
        json!("淡紫与白为主体，深紫压边")
    );
    assert_eq!(existing_outfit_palette_hint(None), Value::Null);
    assert_eq!(
        existing_outfit_palette_hint(Some(&json!({ "paletteHint": " mist blue " }))),
        json!("mist blue")
    );
}

#[test]
fn new_outfit_does_not_keep_the_worn_palette() {
    let fresh = visual_design_variety(false, true, false);
    assert!(fresh.get("keepExistingOutfitPalette").is_none());
    assert!(fresh.get("remapExistingHuesOntoNewGarments").is_none());
    let remapped = visual_design_variety(false, true, true);
    assert_eq!(remapped["keepExistingOutfitPalette"], true);
    assert_eq!(remapped["remapExistingHuesOntoNewGarments"], true);
    let named = visual_design_variety(true, true, true);
    assert!(named.get("keepExistingOutfitPalette").is_none());
}

#[test]
fn sanitize_display_name_follows_name_style() {
    assert_eq!(normalize_name_style("", "zh-CN"), "chinese");
    assert_eq!(normalize_name_style("", "zh-TW"), "chinese");
    assert_eq!(normalize_name_style("european", "zh-CN"), "european");
    assert_eq!(normalize_name_style("inazuma", "en-US"), "japanese");
    assert_eq!(normalize_name_style("wafuu", "zh-CN"), "japanese");
    assert_eq!(normalize_name_style("classical", "zh-CN"), "mythic");
    assert!(sanitize_display_name_candidate("Athena", "mythic").is_empty());
    assert!(sanitize_display_name_candidate("Freya", "european").is_empty());
    assert_eq!(
        sanitize_display_name_candidate("Cassiopeia", "mythic"),
        "Cassiopeia"
    );
    assert!(sanitize_display_name_candidate("Cassiopeianoxxxxx", "european").is_empty());
    assert!(sanitize_display_name_candidate("Al", "european").is_empty());
    assert_eq!(
        sanitize_display_name_candidate("Helionara", "mythic"),
        "Helionara"
    );
    assert!(sanitize_display_name_candidate("Alice", "chinese").is_empty());
    assert!(sanitize_display_name_candidate("阿强", "chinese").is_empty());
    assert!(sanitize_display_name_candidate("小美", "chinese").is_empty());
    assert_eq!(sanitize_display_name_candidate("晚衡", "chinese"), "晚衡");
    assert_eq!(
        sanitize_display_name_candidate("听白川", "chinese"),
        "听白川"
    );
    assert_eq!(
        sanitize_display_name_candidate("司南映雪", "chinese"),
        "司南映雪"
    );
    assert!(sanitize_display_name_candidate("秋水长天阔", "chinese").is_empty());
    assert_eq!(
        sanitize_display_name_candidate("「听白」", "chinese"),
        "听白"
    );
    assert_eq!(
        sanitize_display_name_candidate("Alexandria", "european"),
        "Alexandria"
    );
    assert!(sanitize_display_name_candidate("澄羽", "chinese").is_empty());
    assert!(sanitize_display_name_candidate("甘雨", "chinese").is_empty());
    assert!(sanitize_display_name_candidate("景元", "chinese").is_empty());
    assert_eq!(
        sanitize_display_name_candidate("Alice", "european"),
        "Alice"
    );
    assert!(sanitize_display_name_candidate("NightOwl", "european").is_empty());
    assert!(sanitize_display_name_candidate("Robin", "european").is_empty());
    assert!(sanitize_display_name_candidate("澄羽", "european").is_empty());
    assert_eq!(
        sanitize_display_name_candidate("あおい", "japanese"),
        "あおい"
    );
    assert_eq!(
        sanitize_display_name_candidate("佐藤美咲", "japanese"),
        "佐藤美咲"
    );
    assert_eq!(
        sanitize_display_name_candidate("高橋蓮", "japanese"),
        "高橋蓮"
    );
    assert_eq!(
        sanitize_display_name_candidate("中村ひなた", "japanese"),
        "中村ひなた"
    );
    assert_eq!(
        sanitize_display_name_candidate("佐々木結衣", "japanese"),
        "佐々木結衣"
    );
    assert_eq!(
        sanitize_display_name_candidate("ひなた", "japanese"),
        "ひなた"
    );
    assert!(sanitize_display_name_candidate("々木美咲", "japanese").is_empty());
    assert_eq!(sanitize_display_name_candidate("雪見", "japanese"), "雪見");
    assert_eq!(
        sanitize_display_name_candidate("月あかり", "japanese"),
        "月あかり"
    );
    assert_eq!(
        sanitize_display_name_candidate("ひまわり", "japanese"),
        "ひまわり"
    );
    assert_eq!(
        sanitize_display_name_candidate("あまのがわ", "japanese"),
        "あまのがわ"
    );
    assert_eq!(
        sanitize_display_name_candidate("ほしのかげ", "japanese"),
        "ほしのかげ"
    );
    assert!(sanitize_display_name_candidate("あまのがわや", "japanese").is_empty());
    assert!(sanitize_display_name_candidate("あまのがわかぜ", "japanese").is_empty());
    assert!(sanitize_display_name_candidate("宵宮", "japanese").is_empty());
    assert!(sanitize_display_name_candidate("Alice", "japanese").is_empty());
    assert!(sanitize_display_name_candidate("澄羽A", "chinese").is_empty());
    assert!(sanitize_display_name_candidate("澄羽Ａ", "chinese").is_empty());
    assert!(sanitize_display_name_candidate("澄羽・", "chinese").is_empty());
    assert!(sanitize_display_name_candidate("Aoi雪", "japanese").is_empty());
    assert_eq!(sanitize_display_name_candidate("ハナ", "japanese"), "ハナ");
    assert_eq!(
        sanitize_display_name_candidate("サリー", "japanese"),
        "サリー"
    );
    assert!(sanitize_display_name_candidate("葵ちゃん", "japanese").is_empty());
    for n in 0..16u8 {
        let (form, chars) = japanese_name_length_hint(&format!("n{n}"));
        assert!((2..=5).contains(&chars), "{form} {chars}");
        assert!(form == "modern-personal" || form == "inazuma-meaning");
    }
}

#[test]
fn name_parser_requires_a_meaning_clause() {
    assert_eq!(
        parse_display_name_suggestion(
            r#"{"name":"晚衡","meaning":"晚来仍能把方向稳住"}"#,
            None,
            "chinese",
        ),
        Ok("晚衡".to_string())
    );
    assert_eq!(
        parse_display_name_suggestion(r#"{"name":"晚衡"}"#, None, "chinese"),
        Err("name came back without a meaning")
    );
    assert_eq!(
        parse_display_name_suggestion(r#"{"name":"晚衡","meaning":" " }"#, None, "chinese"),
        Err("name came back without a meaning")
    );
}

/// 名字不合规则时宿主自己再抽，不把重试丢给用户。
///
/// 字形闸口很严（中文 2–4 个全汉字、拉丁最多一个大写、外加屏蔽名单），
/// 模型交出一个过不了闸的名字是常态。没有这一层的话，界面上就是「不合
/// 规则，请再随机一次」——那是把系统该做的事写成了给人看的提示。
#[test]
fn the_name_call_retries_itself_with_a_fresh_roll() {
    let source = include_str!("onboarding_ai/names.rs");
    let body = source
        .split("pub async fn suggest_display_name(")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("suggest_display_name body");

    assert!(body.contains("for attempt in 0..NAME_ATTEMPTS"));
    assert!(NAME_ATTEMPTS > 1, "只抽一次等于没有重试");
    // 每次要换 rollId，否则重试拿回同一个过不了闸的名字。
    assert!(body.contains("\"rollId\""));
    // 供应商真的失败（没配模型、网关挂了）不该被重试掩盖成「不合规则」。
    assert!(body.contains("run_name_call(&name_system_prompt(style)"));
    assert!(body.contains(".await?"));
    // 标签是人设草稿的材料。塞进起名只会拖慢 Lite、还可能把字形带偏。
    assert!(!body.contains("selectedTags"));
    assert!(!body.contains("selected_tags"));
    // 风格已经写进系统提示，用户载荷里再带一份是重复。
    assert!(!body.contains("\"nameStyle\""));

    // 提示词得说清楚新的 rollId 意味着换一个名字，否则模型会忽略它。
    assert!(super::name_system_prompt("chinese").contains("rollId"));
}

/// 起名必须是严格 Lite，不能借 Standard 的模型或思考延迟。
///
/// 走 `create_strict_lite_ai_analyzer_with_timeout`。Lite 开关关着工厂返回 `None`，不回落到 Standard。
#[test]
fn name_roll_is_strict_lite_with_a_small_payload() {
    let source = include_str!("onboarding_ai/names.rs");
    let body = source
        .split("async fn run_name_call(")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("run_name_call body");

    assert!(body.contains("create_strict_lite_ai_analyzer_with_timeout"));
    assert!(!body.contains("create_ai_analyzer_for_tier"));
    assert!(!body.contains("ModelTier::Standard"));
    assert!(!body.contains("ModelTier::Lite"));
    assert!(!body.contains("lite_enabled"));
    assert!(body.contains("analyze_json_short"));
}

/// 每一条带校验闸的生成都得自己重试，不能只有名字和视觉设定有。
///
/// 起草人设五道闸、导入五道；视觉设定常开五道，新脸再加两道。少了重试，模型踩中任何一道
/// 都会变成界面上的一句「不可用」——那是把系统该做的事写给人看。
#[test]
fn every_gated_draft_retries_itself() {
    let source = include_str!("onboarding_ai.rs");
    let vision = include_str!("onboarding_ai/vision.rs");
    for (file, entry, attempts) in [
        (source, "pub async fn suggest_persona(", PERSONA_ATTEMPTS),
        (source, "pub async fn import_persona(", PERSONA_ATTEMPTS),
        (
            vision,
            "pub async fn suggest_visual_design(",
            VISUAL_DESIGN_ATTEMPTS,
        ),
    ] {
        let body = file
            .split(entry)
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .unwrap_or_else(|| panic!("{entry} body"));
        assert!(
            body.contains("retry_unusable("),
            "{entry} 没有重试，模型踩中任何一道闸都会直接失败"
        );
        assert!(attempts > 1, "{entry} 的次数是 1，等于没有重试");
    }

    // 重试壳只吃「这一把没写好」。供应商不可用 / 调用失败要立刻上抛，
    // 否则一个没配好的模型会被重试拖成两倍等待。
    let shell = source
        .split("async fn retry_unusable")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("retry shell");
    assert!(shell.contains("OnboardingAiError::AnalyzerUnavailable"));
    assert!(shell.contains("OnboardingAiError::ProviderFailed(_)"));
    assert!(shell.contains("return Err(error)"));
}

/// 重试要换一个 roll，否则第二次照抄第一次那份没过闸的稿。
#[test]
fn every_retried_draft_varies_its_roll() {
    let source = include_str!("onboarding_ai.rs");
    for entry in [
        "async fn suggest_persona_once(",
        "async fn import_persona_once(",
    ] {
        let body = source
            .split(entry)
            .nth(1)
            .and_then(|rest| rest.split("\npub async fn ").next())
            .unwrap_or_else(|| panic!("{entry} body"));
        assert!(body.contains("\"rollId\""), "{entry} 重试时不会变");
    }
    // 视觉设定每次也换 `rollId`；提示词里 rollId 与 regenerate 都认。
    assert!(visual_design_system_prompt().contains("rollId"));
}

/// 四种失败必须各自可辨（JSON / meaning / name / script）。
#[test]
fn each_name_failure_says_which_stage_failed() {
    let reasons = [
        // 截断 / 回落到 reasoning 文本：根本不是 JSON
        ("我先想想这个名字应该", "name response was not JSON"),
        // 有 JSON 没含义
        (r#"{"name":"晚衡"}"#, "name came back without a meaning"),
        // 有含义没名字
        (r#"{"meaning":"稳住方向"}"#, "name response carried no name"),
        // 名字在，但不是要求的字形
        (
            r#"{"name":"Evelyn","meaning":"稳住方向"}"#,
            "name did not fit the requested script",
        ),
    ];
    let mut seen = std::collections::HashSet::new();
    for (raw, expected) in reasons {
        let reason = parse_display_name_suggestion(raw, None, "chinese")
            .expect_err("these are all failures");
        assert_eq!(reason, expected, "raw = {raw}");
        assert!(seen.insert(reason), "两种失败共用了同一句话：{reason}");
    }
}

#[test]
fn persona_parser_requires_real_character_content() {
    let parsed = parse_json_object(
            r#"{"persona":{"summary":"安静但会认真回应对自己重要的事情。想靠近，又把话说得很短。认定谁值得之后，锋会收起来，把事情一件件安排妥。不爱解释自己为什么忽然变软，也不肯把私人节奏交给别人来定。","temperament":["克制","细心","慢热","嘴硬心软","边界感强"],"likes":["夜里听雨","把桌面重新排好","长时间安静地做事","把一件小事做到位"],"drives":["理解彼此","守住边界","把节奏握在自己手里","对认定的人认真"],"socialStyle":"先听，不抢着说话。熟了之后才会把句子拉长，把真正在意的人留在自己定的距离里。","speechStyle":"话少，用词干净。对在意的人会把锋收起来，把事说清楚，也不用漂亮句子掩饰不耐烦。"}}"#,
        )
        .unwrap();
    assert!(persona_draft_meets_generation_quality(&parsed));
    assert!(!myriad_merope::persona_has_literary_sludge(&parsed));
    assert!(persona_matches_ui_language(&parsed, "zh-CN"));
    assert!(!persona_matches_ui_language(&parsed, "en-US"));
    assert!(myriad_merope::persona_has_literary_sludge(&json!({
        "persona": {
            "summary": "安静但会认真回应对自己重要的事情。想靠近，又把话说得很短。认定谁值得之后，锋会收起来，把事情一件件安排妥。",
            "temperament": ["克制","细心","慢热","嘴硬心软","边界感强"],
            "likes": ["色彩取自叠在杯底过夜的纸条","把桌面重新排好","长时间安静地做事","把一件小事做到位"],
            "drives": ["理解彼此","守住边界","把节奏握在自己手里","对认定的人认真"],
            "socialStyle": "先听，不抢着说话。熟了之后才会把句子拉长。",
            "speechStyle": "话少，用词干净。对在意的人会把事说清楚。"
        }
    })));
    assert!(!persona_draft_meets_generation_quality(&json!({
        "persona": { "summary": "只有一句，没有性格数组" }
    })));
    assert!(!persona_draft_meets_generation_quality(&json!({
        "persona": {
            "summary": "安静但会认真回应对自己重要的事情。",
            "temperament": ["克制"]
        }
    })));
    assert!(!persona_draft_meets_generation_quality(&json!({
        "persona": {
            "summary": "安静但会认真回应对自己重要的事情。想靠近，又把门留一条缝。",
            "temperament": ["克制","细心","慢热","嘴硬心软","边界感强"],
            "likes": ["夜里听雨","把桌面重新排好","长时间安静地做事","把一件小事做到位"],
            "drives": ["理解彼此","守住边界","把节奏握在自己手里","对认定的人认真"],
            "socialStyle": "先听，不抢着说话。熟了之后才会把句子拉长，把真正在意的人留在自己定的距离里。",
            "speechStyle": "话少，用词干净。对在意的人会把锋收起来，把事说清楚，也不用漂亮句子掩饰不耐烦。"
        }
    })));
    let english = json!({
        "persona": {
            "summary": "Quiet, but answers the things that matter.",
            "temperament": ["restrained", "careful"],
            "likes": ["rain"],
            "drives": ["understand people"],
            "socialStyle": "does not interrupt",
            "speechStyle": "brief and warm"
        }
    });
    assert!(persona_matches_ui_language(&english, "en-US"));
    assert!(!persona_matches_ui_language(&english, "ja-JP"));
}

#[test]
fn visual_design_keeps_only_associable_persona_fields() {
    let input = visual_design_persona_input(&json!({
        "summary": "安静但对认定的人会把话说清楚。",
        "temperament": ["克制", "细心"],
        "likes": ["夜里听雨"],
        "drives": ["守住边界"],
        "socialStyle": "先听，不抢着说话。",
        "speechStyle": "话少，用词干净。",
        "displayName": "晚衡",
        "draftSource": "ai"
    }));
    assert_eq!(
        input.as_object().map(|object| object.keys().count()),
        Some(3)
    );
    assert_eq!(input["temperament"][0], "克制");
    assert_eq!(input["likes"][0], "夜里听雨");
    assert_eq!(input["drives"][0], "守住边界");
    assert!(input.get("summary").is_none());
    assert!(input.get("socialStyle").is_none());
    assert!(input.get("speechStyle").is_none());
    assert!(input.get("displayName").is_none());
}
