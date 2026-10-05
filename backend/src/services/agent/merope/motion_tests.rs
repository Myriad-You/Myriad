//! Unit tests for `motion.rs`, kept beside it so the module stays readable.

use super::*;

#[test]
fn truncates_on_character_boundary() {
    assert_eq!(truncate("你好吗", 2), "你好");
}

#[test]
fn phases_have_stable_wire_names() {
    assert_eq!(MotionPhase::Reaction.as_str(), "reaction");
    assert_eq!(MotionPhase::Reaction.activity(), "thinking");
    assert_eq!(MotionPhase::Delivery.activity(), "talking");
    assert_eq!(MotionPhase::Mood.activity(), "idle");
    assert_eq!(
        serde_json::to_string(&MotionPhase::Delivery).unwrap(),
        "\"delivery\""
    );
}

#[test]
fn directive_wire_shape_is_camel_case_and_semantic_only() {
    let value = serde_json::to_value(PerformanceDirective {
        phase: MotionPhase::Reaction,
        mood_revision: 42,
        motion_style: "open".to_string(),
        phrases: vec![SpeechPhrase {
            text: "你觉得呢？".to_string(),
            intent: "check-in".to_string(),
        }],
        score: Vec::new(),
        plan: ChatPerformancePlan {
            baseline: None,
            cues: vec![myriad_merope::ChatPerformanceCue {
                intent: "listen".to_string(),
                at_ms: 0,
                intensity: 1.0,
                tempo: 1.0,
                fade_in_ms: 100,
                fade_out_ms: 200,
                interrupt: "if-lower".to_string(),
            }],
        },
    })
    .unwrap();
    assert_eq!(value["phase"], "reaction");
    assert_eq!(value["moodRevision"], 42);
    assert_eq!(value["motionStyle"], "open");
    assert!(value.pointer("/plan/cues/0/atMs").is_some());
    assert!(value.get("driver").is_none());
    assert_eq!(value["phrases"][0]["intent"], "check-in");
    assert_eq!(value["phrases"][0]["text"], "你觉得呢？");
}

#[test]
fn motion_schema_exposes_new_expressions_only_as_semantic_cues() {
    let schema = motion_schema(PERFORMANCE_CUE_INTENTS);
    assert_eq!(
        schema.pointer("/properties/phrases/maxItems"),
        Some(&serde_json::json!(6))
    );
    assert_eq!(
        schema.pointer("/properties/phrases/items/properties/intent/enum"),
        Some(&serde_json::json!(PERFORMANCE_PHRASE_INTENTS))
    );
    let intents = schema
        .pointer("/properties/cues/items/properties/intent/enum")
        .and_then(serde_json::Value::as_array)
        .unwrap();
    assert!(intents.iter().any(|value| value == "think"));
    assert!(intents.iter().any(|value| value == "dizzy"));
    assert!(intents.iter().any(|value| value == "cry"));
    assert!(intents.iter().any(|value| value == "angry"));
    assert!(intents.iter().any(|value| value == "speechless"));
    assert!(intents.iter().any(|value| value == "maniac"));
    assert!(intents.iter().any(|value| value == "silly"));
    assert!(intents.iter().any(|value| value == "lovestruck"));
    let prompt = motion_system_prompt(PERFORMANCE_CUE_INTENTS);
    assert!(prompt.contains("show the face this person would show"));
    assert!(prompt.contains("Pick expressions by personality"));
    assert!(!prompt.contains("只有文本明确表现"));
    assert!(!prompt.contains("不要夸张"));
    assert!(!prompt.contains("不要连续重复"));
    assert!(!prompt.contains("rig.capabilities 为空"));
    assert!(prompt.contains(&PERFORMANCE_BASELINE_EXPRESSIONS.join("/")));
    assert!(prompt.contains(&PERFORMANCE_POSTURES.join("/")));
    assert!(prompt.contains(&PERFORMANCE_CUE_INTENTS.join("/")));
    assert!(!prompt.contains("angleZ"));
    assert!(prompt.contains("previouslyIssuedPhrases"));
    assert!(prompt.contains("omit baseline"));
    assert!(prompt.contains("If there is no new intent"));
    assert!(prompt.contains("Do not wait for words like 呆呆 / 狂笑 / 做一下"));
    assert!(prompt.contains("self-deprecation"));
    assert!(prompt.contains("jokes and self-deprecation use silly"));
    assert!(prompt.contains("silly/cry that fit semantically play through the eyes"));
    assert!(prompt.contains("Capability being available is not a reason to pick it"));
    assert!(prompt.contains("refusal, dodge, hesitation do not automatically become coy"));
    assert!(prompt.contains("needing closed eyes is not needing silly"));
    assert!(prompt.contains("rig.activeBehaviors"));
    assert!(prompt.contains("preparation→stroke→hold→recovery"));
    assert!(prompt.contains("Music entrain is ongoing body rhythm"));
    assert!(prompt.contains("persona"));
    assert_eq!(
        schema.pointer("/properties/continue/type"),
        Some(&serde_json::json!("boolean"))
    );
    assert_eq!(
        schema.pointer("/properties/baseline/type"),
        Some(&serde_json::json!("object"))
    );
    assert!(schema.get("required").is_none());
}

fn rig(capabilities: &[&str], speaking: bool) -> RigStateSummary {
    myriad_merope::sanitize_rig_state(&serde_json::json!({
        "expression": "steady",
        "posture": "neutral",
        "owners": { "mouth": "idle", "expression": "idle", "gaze": "idle", "headBody": "idle" },
        "speaking": speaking,
        "capabilities": capabilities,
    }))
    .expect("summary")
}

/// The offered set and the enforced set are one predicate (`offered_cue_intents`).
#[test]
fn the_director_is_only_offered_cues_that_survive_the_filter() {
    for (capabilities, speaking) in [
        (&["head-body", "mouth-shapes"][..], false),
        (&["head-body", "cry-eye", "silly-eye"][..], false),
        (&["head-body", "maniac-mouth", "silly-mouth"][..], true),
        (&[][..], false),
    ] {
        let state = rig(capabilities, speaking);
        let offered = offered_cue_intents(Some(&state));
        assert!(!offered.is_empty(), "{capabilities:?}");

        let plan = ChatPerformancePlan {
            baseline: None,
            cues: PERFORMANCE_CUE_INTENTS
                .iter()
                .map(|intent| ChatPerformanceCue {
                    intent: (*intent).to_string(),
                    at_ms: 0,
                    intensity: 1.0,
                    tempo: 1.0,
                    fade_in_ms: 120,
                    fade_out_ms: 200,
                    interrupt: "replace".to_string(),
                })
                .collect(),
        };
        let survived: Vec<String> = refine_performance_plan(plan, &state)
            .cues
            .into_iter()
            .map(|cue| cue.intent)
            .collect();
        assert_eq!(survived, offered, "{capabilities:?} speaking={speaking}");

        let schema = motion_schema(&offered);
        let enumerated = schema
            .pointer("/properties/cues/items/properties/intent/enum")
            .and_then(|value| value.as_array())
            .expect("intent enum");
        assert_eq!(enumerated.len(), offered.len(), "{capabilities:?}");

        // The prompt must not describe a cue the schema forbids.
        let prompt = motion_system_prompt(&offered);
        for intent in PERFORMANCE_CUE_INTENTS {
            assert_eq!(
                prompt.contains(&format!("- {intent}：")),
                offered.contains(intent),
                "{intent} for {capabilities:?}"
            );
        }
    }
}

/// A client that sends no rig state keeps the whole vocabulary.
#[test]
fn a_missing_summary_still_offers_every_cue() {
    assert_eq!(offered_cue_intents(None), PERFORMANCE_CUE_INTENTS.to_vec());
}

#[test]
fn motion_prompt_indexes_every_contract_expression() {
    assert_eq!(
        BASELINE_INDEX
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        PERFORMANCE_BASELINE_EXPRESSIONS.to_vec()
    );
    assert_eq!(
        POSTURE_INDEX
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        PERFORMANCE_POSTURES.to_vec()
    );
    assert_eq!(
        CUE_INDEX
            .iter()
            .map(|(name, _, _)| *name)
            .collect::<Vec<_>>(),
        PERFORMANCE_CUE_INTENTS.to_vec()
    );
    let prompt = motion_system_prompt(PERFORMANCE_CUE_INTENTS);
    for name in PERFORMANCE_BASELINE_EXPRESSIONS {
        assert!(prompt.contains(&format!("- {name}：")), "{name}");
    }
    for name in PERFORMANCE_POSTURES {
        assert!(prompt.contains(&format!("- {name}：")), "{name}");
    }
    for name in PERFORMANCE_CUE_INTENTS {
        assert!(prompt.contains(&format!("- {name}：")), "{name}");
    }
}

#[test]
fn motion_persona_payload_carries_temperament() {
    let blank = crate::models::entities::agent_persona::Model {
        id: "site".into(),
        name: "瞳".into(),
        personality: "气质：认真\n社交：慢热".into(),
        persona_json: Some(serde_json::json!({
            "summary": "认真，慢热，亲近之后会软。",
            "temperament": ["慢热", "嘴硬心软", "认真起来很轴"],
            "socialStyle": "先看，再靠近。",
            "speechStyle": "话短，不客套。",
        })),
        visual_profile: None,
        portrait_asset_id: None,
        portrait_generation: None,
        avatar_asset_id: None,
        avatar_generation: None,
        updated_by: None,
        updated_at: chrono::Utc::now().into(),
    };
    let payload = motion_persona_payload(Some(&blank), "restrained");
    assert_eq!(payload["name"], "瞳");
    assert_eq!(payload["motionStyle"], "restrained");
    assert!(payload["personality"].as_str().unwrap().contains("认真"));
    assert_eq!(payload["temperament"][0], "慢热");
    assert_eq!(payload["socialStyle"], "先看，再靠近。");
    let fallback = motion_persona_payload(None, "even");
    assert_eq!(fallback["name"], "Arael");
    assert_eq!(fallback["motionStyle"], "even");
    assert!(fallback["temperament"].as_array().unwrap().is_empty());
}

#[test]
fn explicit_continue_is_not_a_plan() {
    assert_eq!(
        parse_motion_decision(r#"{"continue":true}"#),
        Some(MotionDecision::Continue)
    );
    assert_eq!(
        parse_motion_decision(r#"{"continue":true,"cues":[]}"#),
        Some(MotionDecision::Continue)
    );
    assert_eq!(
        parse_motion_decision("```json\n{\"continue\": true}\n```"),
        Some(MotionDecision::Continue)
    );
}

#[test]
fn empty_object_is_invalid_not_continue() {
    assert_eq!(parse_motion_decision("{}"), None);
    assert_eq!(parse_motion_decision(r#"{"cues":[]}"#), None);
    assert_eq!(parse_motion_decision(r#"{"continue":false}"#), None);
}

#[test]
fn continue_schema_forbids_new_direction_without_requiring_the_flag() {
    let schema = motion_schema(PERFORMANCE_CUE_INTENTS);
    let branch = &schema["allOf"][0];
    assert_eq!(branch["if"]["required"], serde_json::json!(["continue"]));
    assert_eq!(branch["if"]["properties"]["continue"]["const"], true);
    assert_eq!(
        branch["then"]["not"]["required"],
        serde_json::json!(["baseline"])
    );
    for field in ["cues", "phrases"] {
        assert_eq!(branch["then"]["properties"][field]["maxItems"], 0);
    }
    // The contradictory shape observed in the live probe stays rejected.
    assert!(parse_motion_decision(r#"{"continue":true,"cues":[{"intent":"respond","atMs":180,"fadeInMs":80,"fadeOutMs":400,"intensity":0.7,"interrupt":"replace","tempo":1.0}],"phrases":[]}"#).is_none());
}

#[test]
fn illegal_baseline_without_cues_is_invalid() {
    assert_eq!(
        parse_motion_decision(
            r#"{"baseline":{"expression":"angry","posture":"attack"},"cues":[]}"#
        ),
        None
    );
}

#[test]
fn legal_plan_is_perform() {
    let decision = parse_motion_decision(
        r#"{"cues":[{"intent":"listen","atMs":0,"intensity":1,"tempo":1,"fadeInMs":80,"fadeOutMs":120,"interrupt":"if-lower"}]}"#,
    );
    match decision {
        Some(MotionDecision::Perform(plan)) => {
            assert_eq!(plan.cues.len(), 1);
            assert_eq!(plan.cues[0].intent, "listen");
        }
        other => panic!("expected perform, got {other:?}"),
    }
}

#[test]
fn continue_with_a_plan_is_rejected() {
    assert_eq!(
        parse_motion_decision(r#"{"continue":true,"cues":[{"intent":"listen"}]}"#),
        None
    );
    assert_eq!(
        parse_motion_decision(
            r#"{"continue":true,"baseline":{"expression":"warm","posture":"open","motionEnergy":1,"attention":0.8}}"#
        ),
        None
    );
}

/// Chat speaks plain prose; the director observes it on a separate task.
#[test]
fn streaming_chat_refines_actual_delivery_without_delaying_text() {
    let src = include_str!("../process_chat.rs");
    assert!(src.contains("let performance = None;"));
    let chat = src
        .find("stream_strict_lite_chat_response")
        .expect("chat lite call");
    let local_reaction = src
        .find("local_directive(&reaction_context)")
        .expect("local reaction");
    assert!(
        local_reaction < chat,
        "Chat must react before the reply stream starts"
    );
    assert!(src.contains("spawn_chat_motion_refinement("));
    let streaming = include_str!("../confirmation_and_tasks/chat_stream.rs");
    assert!(streaming.contains("emit_chat_delta(&tx, delta, speech_delivery.as_ref()).await"));
    assert!(!streaming.contains("SpeechDeliveryStream"));
    assert!(!include_str!("../chat_prompt.rs").contains("[[delivery:"));
}

#[test]
fn immediate_reaction_precedes_text_and_landing_cannot_delay_stream_close() {
    let src = include_str!("../process_chat.rs");
    let floor = src.find("local_directive(&reaction_context)").unwrap();
    let delivery = src.find("local_directive(&delivery_context)").unwrap();
    assert!(floor < delivery);
    let chat = src[floor..]
        .find("stream_strict_lite_chat_response")
        .unwrap()
        + floor;
    // What they say in chat is already in the conversation she reads: no
    // second copy of it is kept as something that happened.
    assert!(!src.contains("note_chat_diary"));
    assert!(chat < delivery);
    let finish = src
        .find("response_agent::finish_stream(&progress_tx)")
        .unwrap();
    assert!(finish < delivery);
    let stop = src.find("guard.stop().await").unwrap();
    assert!(finish < stop && stop < delivery);
    let overlay = include_str!("../motion_overlay.rs");
    assert!(overlay.contains("self.task.abort();"));
    assert!(src[delivery..].contains("performance.plan.cues.clear()"));
}

#[test]
fn chat_director_phrase_only_decision_needs_no_replacement_pose() {
    assert_eq!(
        parse_motion_decision(r#"{"phrases":[{"text":"你觉得呢？","intent":"check-in"}]}"#),
        Some(MotionDecision::Perform(ChatPerformancePlan::default()))
    );
    for invalid in [
        r#"{"phrases":[{"text":"你觉得呢？","intent":"driver"}]}"#,
        r#"{"baseline":{},"phrases":[{"text":"你觉得呢？","intent":"ask"}]}"#,
        r#"{"continue":true,"phrases":[{"text":"你觉得呢？","intent":"ask"}]}"#,
    ] {
        assert_eq!(parse_motion_decision(invalid), None);
    }
}

/// MOTION_TIMEOUT >= 8s; MOTION_TOTAL_TIMEOUT > MOTION_TIMEOUT.
/// Request paths must not `handle.await.ok().flatten()`.
#[test]
fn motion_lite_budget_clears_the_observed_success_latency() {
    assert!(MOTION_TIMEOUT >= Duration::from_secs(8));
    assert!(MOTION_TOTAL_TIMEOUT > MOTION_TIMEOUT);
    let src = concat!(
        include_str!("../process_and_recipe.rs"),
        include_str!("../process_chat.rs"),
        include_str!("../process_work.rs")
    );
    assert!(
        !src.contains("handle.await.ok().flatten()"),
        "no request path may block on the director's budget"
    );
}

#[test]
fn a_dropped_lite_call_still_leaves_the_round_something_to_play() {
    let plan = local_performance_plan(
        MotionPhase::Delivery,
        &MoodTransition {
            before: 50.0,
            after: 50.0,
            arousal_before: 48.0,
            arousal_after: 48.0,
            band_before: "calm".to_string(),
            band_after: "calm".to_string(),
            delta: 0.0,
            cause: "test".to_string(),
            revision: 1,
        },
        None,
        Some("已经好了。"),
        "even",
        None,
    );
    assert!(!plan_is_empty(&plan));
}

#[test]
fn user_can_ask_for_a_named_expression() {
    assert_eq!(user_requested_cue("你能做一下呆呆的表情吗"), Some("silly"));
    assert_eq!(user_requested_cue("做一下狂笑"), Some("maniac"));
    assert_eq!(user_requested_cue("狂笑一下"), Some("maniac"));
    assert_eq!(user_requested_cue("来个哭脸"), Some("cry"));
    assert_eq!(user_requested_cue("哭一下"), Some("cry"));
    assert_eq!(user_requested_cue("make a silly face"), Some("silly"));
    assert_eq!(user_requested_cue("好开心"), None);
    assert_eq!(user_requested_cue("昨晚我狂笑了一路"), None);
    assert_eq!(user_requested_cue("做点好玩的表情"), None);
    assert_eq!(user_requested_cue("你能告诉我昨天狂笑的事吗"), None);
    assert_eq!(user_requested_cue("说说你犯蠢的事吧"), None);
    // A Latin alias inside a longer word is not an ask, even next to a
    // request marker: `cry` lives in `cryptic`, `think` in `thinking`.
    assert_eq!(user_requested_cue("make a cryptic joke"), None);
    assert_eq!(user_requested_cue("make a plan, thinking it through"), None);
    assert_eq!(user_requested_cue("make a think face"), Some("think"));
}

#[test]
fn asked_expression_waits_for_the_character_to_answer() {
    assert_eq!(
        play_along_requested_cue(MotionPhase::Reaction, "做一下狂笑", None),
        None
    );
    assert_eq!(
        play_along_requested_cue(MotionPhase::Delivery, "做一下狂笑", None),
        None
    );
    assert_eq!(
        play_along_requested_cue(MotionPhase::Delivery, "做一下狂笑", Some("  ")),
        None
    );
    assert_eq!(
        play_along_requested_cue(MotionPhase::Delivery, "做一下狂笑", Some("才不给你做。")),
        None
    );
    assert_eq!(
        play_along_requested_cue(MotionPhase::Delivery, "做一下狂笑", Some("好啊，看我的。")),
        Some("maniac")
    );
}

#[test]
fn asked_expression_survives_lite_failure_when_the_face_can_play_it() {
    let plan = apply_user_requested_cue(
        ChatPerformancePlan::default(),
        MotionPhase::Delivery,
        "做一下狂笑",
        Some("好啊，看我的。"),
        None,
    );
    assert_eq!(plan.cues[0].intent, "maniac");
    assert_eq!(plan.cues[0].fade_in_ms, 180);
    assert_eq!(plan.cues[0].fade_out_ms, 420);
    assert_eq!(plan.baseline.as_ref().unwrap().expression, "steady");
    let cry = apply_user_requested_cue(
        ChatPerformancePlan::default(),
        MotionPhase::Delivery,
        "来个哭脸",
        Some("行，给你哭一个。"),
        None,
    );
    assert_eq!(cry.baseline.as_ref().unwrap().expression, "steady");
    let blocked = myriad_merope::sanitize_rig_state(&serde_json::json!({
        "capabilities": ["head-body"]
    }))
    .unwrap();
    let skipped = apply_user_requested_cue(
        ChatPerformancePlan::default(),
        MotionPhase::Delivery,
        "做一下狂笑",
        Some("好啊，看我的。"),
        Some(&blocked),
    );
    assert!(skipped.cues.is_empty());
    let too_early = apply_user_requested_cue(
        ChatPerformancePlan::default(),
        MotionPhase::Reaction,
        "做一下狂笑",
        None,
        None,
    );
    assert!(too_early.cues.is_empty());
}

#[test]
fn asked_mouth_expression_plays_even_while_speaking() {
    let state = myriad_merope::sanitize_rig_state(&serde_json::json!({
        "speaking": true,
        "capabilities": ["head-body", "maniac-mouth"]
    }))
    .unwrap();
    let lite = parse_performance_plan(
            r#"{"cues":[{"intent":"listen","atMs":0,"intensity":1,"tempo":1,"fadeInMs":80,"fadeOutMs":120,"interrupt":"replace"},{"intent":"maniac","atMs":0,"intensity":1,"tempo":1,"fadeInMs":80,"fadeOutMs":120,"interrupt":"replace"}]}"#,
        )
        .unwrap();
    let refined = refine_performance_plan(lite, &state);
    assert!(refined.cues.iter().all(|cue| cue.intent != "maniac"));
    let plan = apply_user_requested_cue(
        refined,
        MotionPhase::Delivery,
        "做一下狂笑",
        Some("好啊，看我的。"),
        Some(&state),
    );
    assert_eq!(plan.cues[0].intent, "maniac");
}

#[test]
fn hidden_face_skips_motion() {
    let hidden = myriad_merope::sanitize_rig_state(&serde_json::json!({
        "pageVisible": false,
        "faceVisible": true
    }))
    .unwrap();
    assert!(face_is_hidden(Some(&hidden)));
    let no_face = myriad_merope::sanitize_rig_state(&serde_json::json!({
        "pageVisible": true,
        "faceVisible": false
    }))
    .unwrap();
    assert!(face_is_hidden(Some(&no_face)));
    assert!(!face_is_hidden(None));
}

#[test]
fn a_score_alone_is_a_performance_and_its_moves_follow_the_body() {
    // Only beats, no baseline: still new direction, e.g. while listening.
    assert!(matches!(
        parse_motion_decision(r#"{"score":[{"atMs":300,"move":{"kind":"nod","count":2}}]}"#),
        Some(MotionDecision::Perform(_))
    ));
    assert!(parse_motion_decision(r#"{"score":[{"atMs":300,"move":{"kind":"moonwalk"}}]}"#).is_none());
    assert!(parse_motion_decision(r#"{"continue":true,"score":[{"atMs":0,"move":{"kind":"nod"}}]}"#).is_none());

    let head_only = rig(&["head-body"], false);
    let schema = motion_schema_for_state(&offered_cue_intents(Some(&head_only)), Some(&head_only));
    let kinds: Vec<&str> = schema
        .pointer("/properties/score/items/properties/move/properties/kind/enum")
        .and_then(|value| value.as_array())
        .expect("move kinds")
        .iter()
        .filter_map(|value| value.as_str())
        .collect();
    assert!(kinds.contains(&"nod") && kinds.contains(&"shrug"));
    assert!(!kinds.contains(&"beat"), "no hand beat without arms");
    assert!(!kinds.contains(&"glance"), "no eye glance without independent eyes");
    assert_eq!(
        schema.pointer("/properties/score/items/properties/pose"),
        schema.pointer("/properties/baseline/properties/pose"),
        "a beat's pose uses the same controls as the standing pose"
    );

    let prompt = motion_system_prompt(PERFORMANCE_CUE_INTENTS);
    for (kind, _, _) in myriad_merope::PERFORMANCE_SCORE_MOVES {
        assert!(prompt.contains(&format!("- {kind}: ")), "{kind}");
    }
}
