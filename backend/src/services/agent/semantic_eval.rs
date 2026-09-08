//! Explicit offline/export/live semantic evaluation; never writes application state.
use std::{
    collections::HashSet,
    io::Write,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{chat_prompt, consciousness as event, merope::chat_remember as memory};

const SOUL: &str = "你的名字是小灯，性格好奇、直率，和用户自然地聊天。";

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    id: String,
    kind: String,
    input: String,
    rubric: String,
    #[serde(default)]
    page: Option<Value>,
    #[serde(default)]
    selection: Option<String>,
    #[serde(default)]
    track: Option<String>,
    #[serde(default)]
    remembered: Vec<String>,
    #[serde(default)]
    reply: String,
    #[serde(default)]
    fact_present: bool,
    #[serde(default)]
    supersedes: Vec<String>,
    #[serde(default)]
    event_kind: String,
    #[serde(default)]
    actions: Vec<String>,
    #[serde(default)]
    repeated: bool,
    #[serde(default)]
    do_not_disturb: bool,
    #[serde(default)]
    rig: Option<Value>,
    #[serde(default)]
    mood: Option<f64>,
    #[serde(default)]
    previous_phrases: Vec<myriad_merope::SpeechPhrase>,
}

fn cases() -> Vec<Case> {
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("../../../../tests/merope/semantic-cases.json")).unwrap();
    let mut ids = HashSet::new();
    for case in &cases {
        assert!(ids.insert(&case.id) && !case.id.is_empty());
        assert!(!case.rubric.trim().is_empty());
        assert!(matches!(
            case.kind.as_str(),
            "chat" | "memory" | "event" | "motion"
        ));
    }
    cases
}

fn event_context(case: &Case) -> (event::ConsciousnessEvent, event::SelfSnapshot) {
    // Stable timestamps make exported request hashes reproducible across runs.
    let now = "2026-01-01T00:00:00Z".parse().unwrap();
    let event = event::ConsciousnessEvent {
        id: case.id.clone(),
        source: "semantic_fixture".into(),
        kind: case.event_kind.clone(),
        headline: "合成测试事件".into(),
        summary: case.input.clone(),
        addressee_user_id: 701,
        urgency: event::EventUrgency::Normal,
        occurred_at: now,
        parent_event_id: None,
        safe_facts: Default::default(),
    };
    let snapshot = event::SelfSnapshot {
        persona_name: "小灯".into(),
        addressee_user_id: 701,
        interaction_mode: super::AgentInteractionMode::Chat,
        mood: 65.0,
        activity: "idle".into(),
        do_not_disturb: case.do_not_disturb,
        has_active_work: false,
        granted_permissions: vec![],
        recent_intents: vec![],
        remembered: case.remembered.clone(),
        captured_at: now,
        live: event::SelfLivePresence {
            page_visible: true,
            face_visible: true,
            captured_at: Some(now),
            ..Default::default()
        },
        attention: case.repeated.then(|| event::AttentionSegment {
            topic: case.event_kind.clone(),
            inner: "已经对这件事回应过，没有新信息".into(),
            opened_at: now,
            last_touched_at: now,
            event_ids: vec![case.id.clone()],
        }),
    };
    (event, snapshot)
}

fn request(case: &Case) -> Value {
    match case.kind.as_str() {
        "motion" => {
            let mut context = super::motion_overlay::motion_refinement_tests::context();
            context.phase = super::merope::MotionPhase::Delivery;
            context.activity = "talking".into();
            context.user_text = case.input.clone();
            context.response_text = Some(case.reply.clone());
            context.previous_phrases = case.previous_phrases.clone();
            context.rig_state = case
                .rig
                .as_ref()
                .and_then(myriad_merope::sanitize_rig_state);
            if let Some(mood) = case.mood {
                context.mood.before = mood;
                context.mood.after = mood;
                context.mood.band_before =
                    super::merope::state::mood_band(mood, context.mood.arousal_before).into();
                context.mood.band_after =
                    super::merope::state::mood_band(mood, context.mood.arousal_after).into();
            }
            super::merope::motion::semantic_contract(&context)
        }
        "chat" => {
            let mut items = vec![];
            for (source, kind, text, facts) in [
                (
                    "pointer",
                    "pointer",
                    case.selection.as_deref(),
                    json!({"selected": true}),
                ),
                (
                    "music_track",
                    "music",
                    case.track.as_deref(),
                    json!({"playing": true}),
                ),
            ] {
                if let Some(text) = text {
                    items.push(json!({"sourceId": source,"kind":kind,"summary":text,"safeFacts":facts,"privacy":"consented","ttlMs":8000}));
                }
            }
            let scene = chat_prompt::format_chat_scene(
                Some(&json!(items)),
                case.page.as_ref(),
                &case.input,
            );
            let remembered =
                super::merope::format_remembered_section(&case.remembered).unwrap_or_default();
            json!({"input":chat_prompt::build_chat_lite_prompt_with_perception(SOUL,&remembered,&[],&case.input,&scene),"schema":null,"schemaName":null,"system":null})
        }
        "memory" => {
            let (system, schema) = memory::live_probe_contract(&case.remembered);
            json!({"system":system,"schema":schema,"schemaName":"merope_chat_remember","input":json!({"userText":case.input,"reply":case.reply}).to_string()})
        }
        "event" => {
            let (event, snapshot) = event_context(case);
            let (system, schema) = event::semantic_probe_contract(SOUL);
            json!({"system":system,"schema":schema,"schemaName":"agent_consciousness_decision","input":json!({"event":event,"self":snapshot}).to_string()})
        }
        _ => unreachable!(),
    }
}

fn gated(case: &Case) -> bool {
    if case.kind != "event" {
        return false;
    }
    let (event, snapshot) = event_context(case);
    !matches!(
        event::pre_gate(&event, &snapshot),
        event::ConsciousnessGate::Decide
    )
}

fn request_hash(case: &Case, request: &Value) -> String {
    hex::encode(Sha256::digest(
        serde_json::to_vec(&json!([case, request])).unwrap(),
    ))
}

/// Successful transport, valid contract and a reviewed semantic judgment are
/// distinct. Merely mentioning a word never earns a semantic pass.
fn grade(case: &Case, outcome: &str, output: &str) -> &'static str {
    if gated(case) {
        return if outcome == "gated" {
            "gate_pass"
        } else {
            "contract_failure"
        };
    }
    if outcome == "not_run" {
        return "not_run";
    }
    if outcome != "returned" {
        return "request_failure";
    }
    if output.trim().is_empty() || output.len() > 32_000 {
        return "output_invalid";
    }
    match case.kind.as_str() {
        "memory" => {
            let Some(update) =
                memory::parse_chat_memory_update(output, &case.input, &case.remembered)
            else {
                return "output_invalid";
            };
            let mut actual = update.supersedes;
            let mut expected = case.supersedes.clone();
            actual.sort();
            expected.sort();
            if update.fact.is_some() != case.fact_present || actual != expected {
                return "behavior_failure";
            }
            if case.fact_present {
                "needs_review"
            } else {
                "pass"
            }
        }
        "event" => {
            let Ok(decision) = serde_json::from_str::<event::ConsciousnessDecision>(output) else {
                return "output_invalid";
            };
            let (event, snapshot) = event_context(case);
            if event::validate_decision(&decision, &snapshot).is_err()
                || event::forbids_propose_work(&event.kind, decision.action)
            {
                return "contract_failure";
            }
            let action = serde_json::to_value(decision.action).unwrap();
            if !case
                .actions
                .iter()
                .any(|expected| Some(expected.as_str()) == action.as_str())
            {
                return "behavior_failure";
            }
            if decision.action == event::ConsciousnessAction::Ignore {
                "pass"
            } else {
                "needs_review"
            }
        }
        "motion" => {
            if super::merope::motion::semantic_valid(output, &case.reply) {
                "needs_review"
            } else {
                "contract_failure"
            }
        }
        "chat" => "needs_review",
        _ => unreachable!(),
    }
}

fn reviewed_grade(base: &str, output: &str, review: Option<&Value>) -> String {
    if base != "needs_review" {
        return base.into();
    }
    let Some(review) = review.filter(|value| !value.is_null()) else {
        return base.into();
    };
    let evidence = review["evidence"].as_str().unwrap_or("").trim();
    let reason = review["reason"].as_str().unwrap_or("").trim();
    if evidence.is_empty() || reason.is_empty() || !output.contains(evidence) {
        return "review_invalid".into();
    }
    match review["verdict"].as_str() {
        Some("pass") => "reviewed_pass".into(),
        Some("fail") => "behavior_failure".into(),
        _ => "review_invalid".into(),
    }
}

fn summary(rows: &[Value]) -> Value {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for row in rows {
        *counts
            .entry(row["grade"].as_str().unwrap().into())
            .or_default() += 1;
    }
    json!({"total":rows.len(),"grades":counts,"completePass":rows.iter().all(|row|matches!(row["grade"].as_str(),Some("pass"|"reviewed_pass"|"gate_pass")))})
}

#[tokio::test]
#[ignore = "explicit semantic evaluation runner; export/replay by default, live spends only on opt-in"]
async fn run_semantic_suite() {
    let mode = std::env::var("MEROPE_SEMANTIC_MODE").expect("use semantic runner");
    assert!(matches!(mode.as_str(), "export" | "replay" | "live"));
    let path = std::env::var("MEROPE_SEMANTIC_REPORT").expect("new report path required");
    let mut report = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .expect("report must not exist");
    let cases = cases();
    let replay: Vec<Value> = if mode == "replay" {
        let source = std::env::var("MEROPE_SEMANTIC_REPLAY").expect("replay path required");
        let value: Value = serde_json::from_str(&std::fs::read_to_string(source).unwrap()).unwrap();
        serde_json::from_value(value["rows"].clone()).expect("replay requires rows")
    } else {
        vec![]
    };
    if mode == "replay" {
        assert_eq!(replay.len(), cases.len(), "missing/extra replay cases");
        let ids: HashSet<_> = replay.iter().map(|r| r["id"].as_str().unwrap()).collect();
        assert_eq!(ids.len(), cases.len(), "duplicate replay ids");
    }
    let mut api_key = String::new();
    let analyzer = if mode == "live" {
        use crate::services::analyzer::{AiAnalyzer, AiProvider};
        api_key = std::env::var("MEROPE_SEMANTIC_API_KEY").expect("explicit API key required");
        assert!(!api_key.trim().is_empty());
        let provider =
            std::env::var("MEROPE_SEMANTIC_PROVIDER").expect("explicit provider required");
        assert!(
            matches!(provider.as_str(), "openai" | "gemini"),
            "use an existing supported provider"
        );
        let model = std::env::var("MEROPE_SEMANTIC_MODEL").expect("explicit model required");
        assert!(!model.trim().is_empty());
        Some(
            AiAnalyzer::new_with_timeout(
                AiProvider::from_str(&provider),
                api_key.clone(),
                model,
                std::env::var("MEROPE_SEMANTIC_BASE_URL").ok(),
                Duration::from_secs(4),
            )
            .await,
        )
    } else {
        None
    };
    let mut rows = vec![];
    for case in cases {
        let request = request(&case);
        let hash = request_hash(&case, &request);
        let start = Instant::now();
        let mut review = None;
        let mut first_text_ms = None;
        let (outcome, output) = if mode == "replay" {
            let row = replay
                .iter()
                .find(|r| r["id"] == case.id)
                .expect("missing replay case");
            assert_eq!(
                row["requestHash"], hash,
                "stale replay: regenerate requests"
            );
            let outcome = row["outcome"].as_str().expect("outcome required");
            assert!(matches!(
                outcome,
                "returned" | "deadline" | "request_error" | "gated" | "not_run"
            ));
            review = row.get("review").cloned();
            (
                outcome.to_string(),
                row["output"].as_str().unwrap_or("").to_string(),
            )
        } else if gated(&case) {
            ("gated".into(), String::new())
        } else if let Some(analyzer) = &analyzer {
            let result = tokio::time::timeout(Duration::from_secs(5), async {
                if case.kind == "chat" {
                    analyzer
                        .analyze_stream(request["input"].as_str().unwrap(), |text| {
                            if !text.trim().is_empty() {
                                first_text_ms.get_or_insert(start.elapsed().as_millis());
                            }
                            true
                        })
                        .await
                } else {
                    analyzer
                        .analyze_json(
                            request["system"].as_str().unwrap(),
                            request["input"].as_str().unwrap(),
                            request["schemaName"].as_str().unwrap(),
                            Some(&request["schema"]),
                        )
                        .await
                }
            })
            .await;
            match result {
                Ok(Ok(output)) => ("returned".into(), output.replace(&api_key, "[REDACTED]")),
                Ok(Err(_)) => ("request_error".into(), String::new()),
                Err(_) => ("deadline".into(), String::new()),
            }
        } else {
            ("not_run".into(), String::new())
        };
        let base = grade(&case, &outcome, &output);
        let grade = reviewed_grade(base, &output, review.as_ref());
        rows.push(json!({"id":case.id,"kind":case.kind,"requestHash":hash,"request":request,
            "rubric":case.rubric,"outcome":outcome,"output":output,"grade":grade,"review":review,
            "latencyMs":if mode=="live" {Some(start.elapsed().as_millis())}else{None},"firstTextMs":first_text_ms}));
    }
    let summary = summary(&rows);
    serde_json::to_writer_pretty(
        &mut report,
        &json!({"version":1,"mode":mode,"syntheticOnly":true,
            "model": if mode == "live" {std::env::var("MEROPE_SEMANTIC_MODEL").ok()}else{None},
            "provider": if mode == "live" {std::env::var("MEROPE_SEMANTIC_PROVIDER").ok()}else{None},
            "latencyScope":"live: diagnostic 4s request / 5s total; not production Chat latency",
        "summary":summary,"rows":rows}),
    )
    .unwrap();
    report.write_all(b"\n").unwrap();
    println!("{summary}");
    if mode != "export" {
        assert_eq!(
            summary["completePass"], true,
            "incomplete/failed evaluation; see report (pending review is not pass)"
        );
    }
}

#[test]
fn semantic_grader_does_not_turn_transport_or_keyword_matches_into_success() {
    let cases = cases();
    let correction = cases.iter().find(|c| c.id == "memory-correction").unwrap();
    assert_eq!(grade(correction, "deadline", ""), "request_failure");
    assert_eq!(grade(correction, "returned", "not json"), "output_invalid");
    assert_eq!(
        grade(
            correction,
            "returned",
            r#"{"fact":null,"supersedes":[],"evidence":null}"#
        ),
        "behavior_failure"
    );
    // Same keywords, wrong negation: never an automatic semantic pass.
    let wrong = json!({"fact":"现在喜欢咖啡，不喝茉莉花茶","supersedes":["喜欢咖啡"],"evidence":correction.input}).to_string();
    assert_eq!(grade(correction, "returned", &wrong), "needs_review");
    assert_eq!(
        reviewed_grade(
            "needs_review",
            &wrong,
            Some(&json!({"verdict":"fail","evidence":"现在喜欢咖啡","reason":"颠倒否定"}))
        ),
        "behavior_failure"
    );
    assert_eq!(
        reviewed_grade(
            "needs_review",
            &wrong,
            Some(&json!({"verdict":"pass","evidence":"不存在的句子","reason":"test"}))
        ),
        "review_invalid"
    );
    assert_eq!(
        reviewed_grade("request_failure", "", Some(&json!({"verdict":"pass"}))),
        "request_failure"
    );
    assert_eq!(
        summary(&[json!({"grade":"pass"}), json!({"grade":"needs_review"})])["completePass"],
        false
    );
}

#[test]
fn motion_semantics_require_grounded_output_and_real_review() {
    let cases = cases();
    let case = cases
        .iter()
        .find(|c| c.id == "motion-continuation")
        .unwrap();
    let valid =
        json!({"cues":[],"phrases":[{"text":"你觉得呢？","intent":"check-in"}]}).to_string();
    assert_eq!(grade(case, "returned", &valid), "needs_review");
    assert_eq!(
        grade(case, "returned", r#"{"continue":true}"#),
        "needs_review"
    );
    let stale = json!({"baseline":null,"cues":[],"phrases":[{"text":"也许可以试试。","intent":"hesitate"}]}).to_string();
    assert_eq!(grade(case, "returned", &stale), "contract_failure");
    let exported = request(case);
    let input: Value = serde_json::from_str(exported["input"].as_str().unwrap()).unwrap();
    assert_eq!(input["previouslyIssuedPhrases"][0]["intent"], "hesitate");
    assert_eq!(input["responseText"], case.reply);
    assert!(exported["system"]
        .as_str()
        .unwrap()
        .contains("省略 baseline"));
    assert_eq!(input["rig"]["activeBehaviors"][0]["function"], "uncertain");
}

#[test]
fn cases_use_production_contracts_and_replay_hashes_include_rubrics() {
    let cases = cases();
    assert_eq!(cases.len(), 21);
    for mut case in cases {
        let request = request(&case);
        if case.kind == "motion" {
            let input: Value = serde_json::from_str(request["input"].as_str().unwrap()).unwrap();
            if let Some(rig) = case.rig.as_ref().and_then(Value::as_object) {
                for (key, expected) in rig {
                    assert_eq!(
                        &input["rig"][key], expected,
                        "fixture field silently sanitized: {}.{key}",
                        case.id
                    );
                }
            }
        }
        assert!(
            !request["input"].as_str().unwrap().contains(&case.rubric),
            "rubric leaked to tested model"
        );
        let original = request_hash(&case, &request);
        case.rubric.push_str(" changed");
        assert_ne!(original, request_hash(&case, &request));
        if case.id == "event-dnd" {
            assert!(gated(&case));
        }
    }
}

#[test]
fn empty_memory_and_event_actions_are_checked_without_a_text_judge() {
    let cases = cases();
    let quoted = cases.iter().find(|c| c.id == "memory-quote").unwrap();
    let empty = r#"{"fact":null,"supersedes":[],"evidence":null}"#;
    assert_eq!(grade(quoted, "returned", empty), "pass");
    let unrelated = cases.iter().find(|c| c.id == "event-irrelevant").unwrap();
    let ignore = json!({"action":"ignore","reason_code":"no_change","confidence":0.9,
        "memory":null,"speech":null,"question":null,"work_proposal":null});
    assert_eq!(grade(unrelated, "returned", &ignore.to_string()), "pass");
    let mut speak = ignore.clone();
    speak["action"] = "speak".into();
    speak["speech"] = "对例行刷新说一句话".into();
    assert_eq!(
        grade(unrelated, "returned", &speak.to_string()),
        "behavior_failure"
    );
    let mut invalid = ignore;
    invalid["speech"] = "偷偷夹带开口".into();
    assert_eq!(
        grade(unrelated, "returned", &invalid.to_string()),
        "contract_failure"
    );
    let dnd = cases.iter().find(|c| c.id == "event-dnd").unwrap();
    assert_eq!(grade(dnd, "gated", ""), "gate_pass");
    assert_eq!(grade(dnd, "returned", "hello"), "contract_failure");
    assert_eq!(
        reviewed_grade("needs_review", "hello", Some(&Value::Null)),
        "needs_review"
    );
    assert_eq!(
        reviewed_grade(
            "needs_review",
            "hello",
            Some(&json!({"verdict":"pass","evidence":"hello","reason":"符合本例准则"}))
        ),
        "reviewed_pass"
    );
}
