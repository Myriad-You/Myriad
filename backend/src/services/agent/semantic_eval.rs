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

const SOUL: &str =
    "Your name is 小灯. You are curious and direct, and you chat naturally with the user.";

/// Shared acceptance loader. Credentials stay in the host; every connection is read-only.
pub(super) async fn load_configured_lite() -> sea_orm::DatabaseConnection {
    use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseBackend, Statement};
    dotenvy::dotenv().ok();
    // Do not let the key loader create a new key while doing read-only acceptance.
    let data_root = std::env::var("DATA_DIR").unwrap_or_else(|_| "data".into());
    assert!(
        std::env::var("MYRIAD_DATA_KEY").is_ok()
            || std::path::Path::new(&data_root)
                .join(".secret-key")
                .is_file(),
        "an existing host data key is required"
    );
    let url = std::env::var("DATABASE_URL").expect("host DATABASE_URL required");
    let mut url = url::Url::parse(&url).unwrap_or_else(|_| panic!("invalid host database URL"));
    // Every connection is read-only, including reconnects. No startup/migrations,
    // memory recall, user records, notifications or ledger writes are involved.
    let pairs: Vec<_> = url
        .query_pairs()
        .filter(|(key, _)| key != "options")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    url.query_pairs_mut()
        .clear()
        .extend_pairs(pairs)
        .append_pair("options", "-c default_transaction_read_only=on");
    let mut options = ConnectOptions::new(url.to_string());
    options.max_connections(1).sqlx_logging(false);
    let db = Database::connect(options)
        .await
        .unwrap_or_else(|_| panic!("read-only database unavailable"));
    let readonly = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SHOW default_transaction_read_only",
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        readonly
            .try_get::<String>("", "default_transaction_read_only")
            .unwrap(),
        "on"
    );
    let config = crate::services::config_service::ConfigService::new(db.clone())
        .load_config()
        .await
        .unwrap_or_else(|_| panic!("cannot load host model configuration"));
    *crate::GLOBAL_DYNAMIC_CONFIG.write().await = config;
    db
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Case {
    #[serde(default)]
    touch: Option<Value>,
    #[serde(default)]
    soul: Option<String>,
    #[serde(default)]
    arousal: Option<f64>,
    #[serde(default)]
    activity: String,
    #[serde(default)]
    remaining_ms: Option<u64>,
    #[serde(default)]
    local_reaction: Option<String>,
    #[serde(default)]
    consistency_group: Option<String>,
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
    let mut cases = cases;
    cases.extend(
        serde_json::from_str::<Vec<Case>>(include_str!(
            "../../../../tests/merope/touch-semantic-cases.json"
        ))
        .unwrap(),
    );
    cases.extend(
        serde_json::from_str::<Vec<Case>>(include_str!(
            "../../../../tests/merope/touch-response-cases.json"
        ))
        .unwrap(),
    );
    let mut ids = HashSet::new();
    for case in &cases {
        assert!(ids.insert(&case.id) && !case.id.is_empty());
        assert!(!case.rubric.trim().is_empty());
        assert!(matches!(
            case.kind.as_str(),
            "chat" | "memory" | "event" | "motion" | "touch"
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
        summary: if case.event_kind == "agent.merope.touch" {
            crate::api::agent::touch::completion_summary(
                &serde_json::from_value(case.touch.clone().unwrap()).unwrap(),
            )
        } else {
            case.input.clone()
        },
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
        mood: case.mood.unwrap_or(65.0),
        activity: if case.activity.is_empty() {
            "idle".into()
        } else {
            case.activity.clone()
        },
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
        "touch" => crate::api::agent::touch::appraisal_contract(
            case.soul.as_deref().unwrap_or(SOUL),
            &serde_json::from_value(case.touch.clone().expect("touch summary required")).unwrap(),
            case.mood.unwrap_or(70.0),
            case.arousal.unwrap_or(48.0),
            &case.activity,
        ),
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
            let mut contract = super::merope::motion::semantic_contract(&context);
            if let Some(soul) = &case.soul {
                let mut input: Value =
                    serde_json::from_str(contract["input"].as_str().unwrap()).unwrap();
                input["persona"]["name"] = json!("小灯");
                input["persona"]["personality"] = json!(soul);
                contract["input"] = json!(input.to_string());
            }
            contract
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
            let (system, schema) = event::semantic_probe_contract(
                case.soul.as_deref().unwrap_or(SOUL),
                &case.event_kind,
            );
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
    if event.kind == "agent.merope.touch"
        && (case.remaining_ms == Some(0)
            || !super::merope::decide_ingest(
                &event.kind,
                &super::merope::IngestSight {
                    on_page: true,
                    working: super::merope::activity_is_busy(&snapshot.activity),
                    do_not_disturb: snapshot.do_not_disturb,
                    ..Default::default()
                },
            )
            .allow_model)
    {
        return true;
    }
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
        "touch" => {
            let Some(value) = crate::api::agent::touch::parse_appraisal(output) else {
                return "contract_failure";
            };
            if case
                .actions
                .iter()
                .any(|action| value["reaction"] == *action)
            {
                "needs_review"
            } else {
                "behavior_failure"
            }
        }
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
            if decision.action == event::ConsciousnessAction::Ignore
                && event.kind != "agent.merope.touch"
            {
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
    let touch: Vec<_> = rows.iter().filter(|r| r["kind"] == "touch").collect();
    let attempted = touch
        .iter()
        .filter(|r| {
            matches!(
                r["outcome"].as_str(),
                Some("returned" | "deadline" | "request_error")
            )
        })
        .count();
    let valid = touch
        .iter()
        .filter(|r| r["touchMetrics"]["valid"] == true)
        .count();
    let timed = touch
        .iter()
        .filter(|r| r["touchMetrics"]["timely"].is_boolean())
        .count();
    let timely = touch
        .iter()
        .filter(|r| r["touchMetrics"]["timely"] == true)
        .count();
    let mut latencies: Vec<_> = touch
        .iter()
        .filter_map(|r| r["latencyMs"].as_u64())
        .collect();
    latencies.sort_unstable();
    let mut repeated = std::collections::BTreeMap::<String, Vec<Value>>::new();
    for row in &touch {
        if let Some(group) = row["touchMetrics"]["consistencyGroup"].as_str() {
            repeated
                .entry(group.into())
                .or_default()
                .push(json!({"id":row["id"],"output":row["output"],"grade":row["grade"]}));
        }
    }
    json!({"total":rows.len(),"grades":counts,"completePass":rows.iter().all(|row| row["withinRequestBudget"] != false && matches!(row["grade"].as_str(),Some("pass"|"reviewed_pass"|"gate_pass"))),
        "touch":{"attempted":attempted,"valid":valid,"timed":timed,"timely":timely,
            "validRate":(attempted>0).then(|| valid as f64 / attempted as f64),
            "timelyRate":(timed>0).then(|| timely as f64 / timed as f64),
            "p50Ms":latencies.get(latencies.len().saturating_sub(1)/2),
            "p95Ms":latencies.get((latencies.len()*95).div_ceil(100).saturating_sub(1)),
            "independentRepeatSamples":repeated,"visibleImprovement":null}})
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
    let filter = std::env::var("MEROPE_SEMANTIC_KIND").ok();
    let repeats = std::env::var("MEROPE_SEMANTIC_REPEAT")
        .unwrap_or_else(|_| "1".into())
        .parse::<usize>()
        .unwrap();
    assert!((1..=3).contains(&repeats));
    let cases: Vec<_> = cases()
        .into_iter()
        .filter(|c| {
            filter.as_ref().is_none_or(|kind| {
                if kind == "touch-response" {
                    c.event_kind == "agent.merope.touch"
                } else {
                    &c.kind == kind
                }
            })
        })
        .flat_map(|c| {
            (0..repeats).map(move |i| {
                let mut c = c.clone();
                if repeats > 1 {
                    c.id = format!("{}-sample-{}", c.id, i + 1);
                }
                c
            })
        })
        .collect();
    assert!(!cases.is_empty(), "unknown or empty case kind");
    let replay: Vec<Value> = if mode == "replay" {
        let source = std::env::var("MEROPE_SEMANTIC_REPLAY").expect("replay path required");
        let value: Value = serde_json::from_str(&std::fs::read_to_string(source).unwrap()).unwrap();
        serde_json::from_value(value["rows"].clone()).expect("replay requires rows")
    } else {
        vec![]
    };
    if mode == "replay" {
        assert!(replay.len() >= cases.len(), "missing replay cases");
        let ids: HashSet<_> = replay.iter().map(|r| r["id"].as_str().unwrap()).collect();
        assert_eq!(ids.len(), replay.len(), "duplicate replay ids");
    }
    let mut api_key = String::new();
    let diagnostic = std::env::var("MEROPE_SEMANTIC_DIAGNOSTIC_SECONDS")
        .ok()
        .map(|v| v.parse::<u64>().unwrap());
    assert!(
        diagnostic.is_none_or(|seconds| seconds == 15),
        "diagnostic deadline must be 15 seconds"
    );
    let mut model_info = Value::Null;
    let analyzer = if mode == "live" {
        let db = load_configured_lite().await;
        db.close().await.unwrap();
        let configured = crate::GLOBAL_DYNAMIC_CONFIG
            .read()
            .await
            .resolve_strict_lite_ai_config()
            .expect("configured Lite required");
        api_key = configured
            .api_key
            .filter(|key| !key.is_empty())
            .expect("configured Lite credentials required");
        model_info = json!({"model":configured.model,"provider":configured.provider});
        Some(
            crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(
                Duration::from_secs(diagnostic.unwrap_or(4)),
            ))
            .await
            .expect("configured Lite unavailable"),
        )
    } else {
        None
    };
    let director = if mode == "live" {
        crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(Duration::from_secs(
            diagnostic.unwrap_or(9),
        )))
        .await
    } else {
        None
    };
    let mut rows = vec![];
    let probe = std::env::var("MEROPE_SEMANTIC_PROBE").ok();
    assert!(
        probe
            .as_deref()
            .is_none_or(|p| matches!(p, "default" | "disabled")),
        "probe must be default or disabled"
    );
    let mut pending: std::collections::VecDeque<_> = cases.into();
    while let Some(case) = pending.pop_front() {
        let mut request = request(&case);
        if let Some(probe) = &probe {
            request["diagnosticProbe"] = json!({"reasoning":probe,"maxTokens":2048});
        }
        let hash = request_hash(&case, &request);
        let start = Instant::now();
        let mut review = None;
        let mut first_text_ms = None;
        let mut observation = crate::services::analyzer::probe::Observation::default();
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
        } else if let Some(analyzer) = if case.kind == "motion" {
            &director
        } else {
            &analyzer
        } {
            let result = tokio::time::timeout(
                Duration::from_secs(diagnostic.map(|seconds| seconds + 1).unwrap_or(
                    if case.kind == "touch" {
                        2
                    } else if case.kind == "motion" {
                        10
                    } else {
                        5
                    },
                )),
                async {
                    if case.kind == "chat" {
                        analyzer
                            .analyze_stream(request["input"].as_str().unwrap(), |text| {
                                if !text.trim().is_empty() {
                                    first_text_ms.get_or_insert(start.elapsed().as_millis());
                                }
                                true
                            })
                            .await
                    } else if let Some(probe) = &probe {
                        use crate::services::analyzer::probe::{Policy, Reasoning};
                        analyzer
                            .probe_json(
                                request["system"].as_str().unwrap(),
                                request["input"].as_str().unwrap(),
                                request["schemaName"].as_str().unwrap(),
                                &request["schema"],
                                Policy {
                                    reasoning: if probe == "disabled" {
                                        Reasoning::Disabled
                                    } else {
                                        Reasoning::Default
                                    },
                                    temperature: None,
                                    max_tokens: 2048,
                                },
                                &mut observation,
                            )
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
                },
            )
            .await;
            match result {
                Ok(Ok(output)) => ("returned".into(), output.replace(&api_key, "[REDACTED]")),
                Ok(Err(error)) => (
                    if error.chain().any(|e| {
                        e.downcast_ref::<reqwest::Error>()
                            .is_some_and(|e| e.is_timeout())
                    }) {
                        "deadline"
                    } else {
                        "request_error"
                    }
                    .into(),
                    String::new(),
                ),
                Err(_) => ("deadline".into(), String::new()),
            }
        } else {
            ("not_run".into(), String::new())
        };
        let base = grade(&case, &outcome, &output);
        // Follow the generated sentence, not an independently authored fixture.
        if case.kind == "event" && case.event_kind == "agent.merope.touch" && base == "needs_review"
        {
            let decision: event::ConsciousnessDecision = serde_json::from_str(&output).unwrap();
            if let Some(line) = decision.speech.or(decision.question) {
                let mut motion = case.clone();
                motion.id = format!("{}-director", case.id);
                motion.kind = "motion".into();
                motion.input = event_context(&case).0.summary;
                motion.reply = line;
                motion.rubric = format!(
                    "Check that motion, expression, this generated reply, and the already-shown touch reaction are consistent. {}",
                    case.rubric
                );
                pending.push_back(motion);
            }
        }
        let grade = reviewed_grade(base, &output, review.as_ref());
        let latency = if mode == "live" {
            Some(start.elapsed().as_millis() as u64)
        } else if mode == "replay" {
            replay
                .iter()
                .find(|r| r["id"] == case.id)
                .and_then(|r| r["latencyMs"].as_u64())
        } else {
            None
        };
        let reaction = (outcome == "returned")
            .then(|| crate::api::agent::touch::parse_appraisal(&output))
            .flatten();
        let touch_metrics = (case.kind == "touch").then(|| json!({
            "valid":reaction.is_some(),
            "timely":latency.zip(case.remaining_ms).map(|(ms, remaining)| reaction.is_some() && ms < remaining && ms <= 2000),
            "differsFromLocal":reaction.as_ref().map(|r| Some(r["reaction"].as_str().unwrap()) != case.local_reaction.as_deref()),
            "consistencyGroup":case.consistency_group,
            "scope":"synthetic remaining-contact window; excludes transport/state lookup/render latency; disagreement is not proof of visual improvement"
        }));
        rows.push(
            json!({"id":case.id,"kind":case.kind,"requestHash":hash,"request":request,
            "rubric":case.rubric,"outcome":outcome,"output":output,"grade":grade,"review":review,
            "latencyMs":latency,"firstTextMs":first_text_ms,"touchMetrics":touch_metrics,
            "probeObservation": if mode == "replay" { replay.iter().find(|r| r["id"] == case.id).and_then(|r| r.get("probeObservation")).cloned().unwrap_or(Value::Null) } else if probe.is_some() { json!(observation) } else { Value::Null },
            "withinRequestBudget":latency.map(|ms| ms <= if case.kind == "motion" {9000} else if case.kind == "touch" {2000} else {4000})}),
        );
    }
    let summary = summary(&rows);
    if mode == "replay" {
        assert_eq!(replay.len(), rows.len(), "extra/missing dependent stages");
    }
    serde_json::to_writer_pretty(
        &mut report,
        &json!({"version":1,"mode":mode,"syntheticOnly":true,
            "configuredLite":model_info,"diagnosticDeadlineSeconds":diagnostic,
            "latencyScope":"model calls only: touch budget 2s, event 4s, director 9s; diagnostic deadline never changes production budgets; excludes transport-to-app, state lookup and rendering",
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
fn touch_semantics_reject_blanket_acceptance_and_do_not_claim_unrun_success() {
    let cases = cases();
    let touch: Vec<_> = cases.iter().filter(|c| c.kind == "touch").collect();
    assert_eq!(touch.len(), 12);
    let continued = touch
        .iter()
        .find(|c| c.id == "touch-rendered-withdraw-repeat")
        .unwrap();
    assert_eq!(
        grade(continued, "returned", r#"{"reaction":"accept"}"#),
        "behavior_failure"
    );
    let input: Value = serde_json::from_str(request(continued)["input"].as_str().unwrap()).unwrap();
    assert_eq!(input["touch"]["displayedReaction"], "withdraw");
    let boundary = touch.iter().find(|c| c.id == "touch-boundary").unwrap();
    assert_eq!(
        grade(boundary, "returned", r#"{"reaction":"accept"}"#),
        "behavior_failure"
    );
    assert_eq!(
        grade(boundary, "returned", r#"{"reaction":"withdraw"}"#),
        "needs_review"
    );
    assert_eq!(
        grade(
            boundary,
            "returned",
            r#"{"reaction":"withdraw","speech":"stop"}"#
        ),
        "contract_failure"
    );
    assert_eq!(grade(boundary, "deadline", ""), "request_failure");
    assert_eq!(grade(boundary, "not_run", ""), "not_run");
    let exported = request(boundary);
    assert_eq!(exported["schemaName"], "touch_appraisal");
    assert!(!exported["input"].as_str().unwrap().contains("remainingMs"));
    assert!(!exported["input"]
        .as_str()
        .unwrap()
        .contains("localReaction"));
    let empty = summary(&[json!({"kind":"touch","outcome":"not_run","grade":"not_run"})]);
    assert!(empty["touch"]["validRate"].is_null());
    assert!(empty["touch"]["timelyRate"].is_null());
    assert!(empty["touch"]["visibleImprovement"].is_null());
    assert_eq!(empty["completePass"], false);
}

#[test]
fn touch_response_cases_use_production_summary_and_suppress_busy_expired_and_dnd() {
    let scenarios: Vec<_> = cases()
        .into_iter()
        .filter(|c| c.event_kind == "agent.merope.touch")
        .collect();
    assert_eq!(scenarios.len(), 6);
    for case in &scenarios {
        let suppressed = [
            "touch-response-talking",
            "touch-response-expired",
            "touch-response-dnd",
        ]
        .contains(&case.id.as_str());
        assert_eq!(gated(case), suppressed, "{}", case.id);
        assert_eq!(
            grade(case, if suppressed { "gated" } else { "not_run" }, ""),
            if suppressed { "gate_pass" } else { "not_run" }
        );
    }
    let withdrawal = scenarios
        .iter()
        .find(|c| c.id == "touch-response-withdraw")
        .unwrap();
    let request = request(withdrawal);
    let input: Value = serde_json::from_str(request["input"].as_str().unwrap()).unwrap();
    assert!(input["event"]["summary"]
        .as_str()
        .unwrap()
        .contains("Last reaction: withdrew"));
    assert!(request["system"]
        .as_str()
        .unwrap()
        .contains(withdrawal.soul.as_deref().unwrap()));
    assert_eq!(
        request["schema"]["properties"]["action"]["enum"],
        json!(["ignore", "speak", "ask"])
    );
    assert_eq!(request["schema"]["properties"]["memory"]["type"], "null");
    let silence = r#"{"action":"ignore","reason_code":"no_response","confidence":0.9,"memory":null,"speech":null,"question":null,"work_proposal":null}"#;
    assert_eq!(grade(withdrawal, "returned", silence), "needs_review");
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
        .contains("omit baseline"));
    assert_eq!(input["rig"]["activeBehaviors"][0]["function"], "uncertain");
}

#[test]
fn cases_use_production_contracts_and_replay_hashes_include_rubrics() {
    let cases = cases();
    assert_eq!(cases.iter().filter(|c| c.kind != "touch").count(), 27);
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
