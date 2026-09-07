//! Deterministic cross-boundary acceptance. Real producers/consumers, no model.
use chrono::{Duration, Utc};
use serde_json::{json, Value};

use super::{chat_prompt, consciousness};

#[tokio::test]
async fn parallel_director_reaches_wire_without_metadata_in_prose_or_global_pose_changes() {
    use super::merope::{MoodTransition, MotionContext, MotionPhase};
    use super::types::AgentProgressEvent;
    let context = MotionContext {
        user_id: 1,
        phase: MotionPhase::Delivery,
        mood: MoodTransition {
            before: 70.0,
            after: 70.0,
            arousal_before: 48.0,
            arousal_after: 48.0,
            band_before: "calm".into(),
            band_after: "calm".into(),
            delta: 0.0,
            cause: "test".into(),
            revision: 1,
        },
        activity: "talking".into(),
        user_text: "我们怎么办？".into(),
        response_text: None,
        previous_phrases: Vec::new(),
        task_success: None,
        rig_state: None,
        motion_style: "even".into(),
    };
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);
    let (mut delivery, guard) = super::motion_overlay::spawn_chat_motion_refinement_with(
        context,
        tx.clone(),
        |context| async move {
            let phrases = myriad_merope::grounded_speech_phrases(
                &json!([
                    {"text":"也许可以试试。", "intent":"hesitate"},
                    {"text":"不过先把原因说清楚。", "intent":"explain"},
                    {"text":"你觉得呢？", "intent":"check-in"},
                    {"text":"你真的这么想吗？", "intent":"tease"},
                ]),
                context.response_text.as_deref(),
            );
            Some(super::merope::PerformanceDirective {
                phase: context.phase,
                mood_revision: context.mood.revision,
                motion_style: context.motion_style,
                plan: Default::default(),
                phrases,
            })
        },
    );
    let mut text = String::new();
    let mut events = Vec::new();
    for sentence in [
        "也许可以试试。",
        "不过先把原因说清楚。",
        "你觉得呢？",
        "你真的这么想吗？",
    ] {
        text.push_str(sentence);
        delivery.observe(sentence, &tx);
        loop {
            let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
                .await
                .unwrap()
                .unwrap();
            let AgentProgressEvent::PerformancePlan { performance } = &event else {
                panic!("not a performance event")
            };
            if performance.phrases.is_empty() {
                continue;
            } // Immediate local floor.
            assert!(performance.plan.baseline.is_none());
            assert!(performance.plan.cues.is_empty());
            assert!(performance
                .phrases
                .iter()
                .all(|phrase| text.contains(&phrase.text)));
            events.push(serde_json::to_value(event).unwrap());
            break;
        }
    }
    guard.stop().await;
    assert_eq!(events.len(), 4);
    assert_eq!(chat_prompt::chat_safe_content(&text), text);
    assert_eq!(super::chat_music::peel_chat_live_reply(&text).0, text);
    assert!(!text.contains("[[delivery:"));
    if let Ok(path) = std::env::var("MEROPE_BEHAVIOR_WIRE_PATH") {
        let path = std::path::Path::new(&path).with_file_name("delivery.json");
        std::fs::write(
            path,
            serde_json::to_vec(&json!({ "text": text, "events": events })).unwrap(),
        )
        .unwrap();
    }
}

#[test]
#[ignore = "run with node scripts/test-merope-behavior.mjs to generate frontend wire data"]
fn frontend_observations_reach_chat_and_event_context_without_stale_sources() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../../../../tests/merope/scenes.json")).unwrap();
    let path = std::env::var("MEROPE_BEHAVIOR_WIRE_PATH").expect("frontend wire artifact required");
    let rows: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(rows.len(), cases.len(), "no case may silently disappear");
    for (index, (case, row)) in cases.iter().zip(&rows).enumerate() {
        let id = case["id"].as_str().unwrap();
        assert_eq!(row["id"], case["id"]);
        let data = &row["data"];
        let scene = chat_prompt::format_chat_scene(data.get("perception"), None, "这个呢");
        let prompt = chat_prompt::build_chat_lite_prompt_with_perception(
            "你是测试人设。",
            "",
            &[],
            "这个呢",
            &scene,
        );
        let live = consciousness::live_presence_from_custom_data(data);
        // Reuse the same addressee across the sequence; do not hide stale state
        // by allocating a clean recipient for every scene.
        let user_id = 1_930_601;
        consciousness::remember_live_presence(user_id, live);
        let observed = consciousness::last_live_presence(user_id)
            .perception
            .join("\n");
        for expected in case["contains"].as_array().unwrap() {
            let expected = expected.as_str().unwrap();
            assert!(prompt.contains(expected), "{id}: Chat missing {expected}");
        }
        for expected in case["liveContains"].as_array().unwrap() {
            let fact = expected.as_str().unwrap();
            assert!(
                observed.contains(fact),
                "{id}: event context missing {fact}"
            );
        }
        for forbidden in case["absent"].as_array().unwrap() {
            let forbidden = forbidden.as_str().unwrap();
            assert!(
                !prompt.contains(forbidden),
                "{id}: Chat retained {forbidden}"
            );
            assert!(
                !observed.contains(forbidden),
                "{id}: event context retained {forbidden}"
            );
        }
        assert!(!prompt.contains("PRIVATE_MEDIA_URL"));
        assert!(consciousness::last_live_presence(user_id + 1)
            .perception
            .is_empty());
        println!("scene {}: {id} passed", index + 1);
    }
}

#[test]
fn expired_or_missing_ttl_is_not_a_current_observation_in_either_reader() {
    for ttl in [json!(0), json!(-1), Value::Null] {
        let data = json!({"perception": [{
            "sourceId": "page", "kind": "page", "privacy": "consented",
            "summary": "EXPIRED_PAGE", "safeFacts": {"title": "EXPIRED_PAGE"}, "ttlMs": ttl
        }]});
        let scene = chat_prompt::format_chat_scene(data.get("perception"), None, "这个呢");
        assert!(!scene.contains("EXPIRED_PAGE"), "Chat accepted ttl={ttl}");
        let live = consciousness::live_presence_from_custom_data(&data);
        assert!(
            live.perception.is_empty(),
            "event context accepted ttl={ttl}"
        );
    }
}

#[test]
fn observation_expiry_does_not_end_the_longer_page_presence_lease() {
    let mut live = consciousness::live_presence_from_custom_data(&json!({
        "presence": {"pageVisible": true},
        "perception": [{"sourceId": "page", "kind": "page", "privacy": "consented",
            "summary": "SHORT_LIVED_PAGE", "ttlMs": 2000}]
    }));
    live.captured_at = Some(Utc::now() - Duration::seconds(3));
    consciousness::remember_live_presence(1_930_603, live);
    let current = consciousness::last_live_presence(1_930_603);
    assert!(current.page_visible, "page presence lease is independent");
    assert!(
        current.perception.is_empty(),
        "expired text must not reach an event decision"
    );
    assert!(
        current.perception_payload.is_empty(),
        "expired payload must not reach another Chat ingress"
    );
}

#[test]
fn reading_a_live_observation_spends_its_original_ttl_without_renewing_or_double_aging() {
    let mut live = consciousness::live_presence_from_custom_data(&json!({
        "presence": {"pageVisible": true},
        "perception": [{"sourceId": "page", "kind": "page", "privacy": "consented",
            "summary": "STILL_CURRENT", "ttlMs": 8000}]
    }));
    live.captured_at = Some(Utc::now() - Duration::seconds(3));
    consciousness::remember_live_presence(1_930_604, live);
    let first = consciousness::last_live_presence(1_930_604);
    let first_ttl = first.perception_payload[0]["ttlMs"].as_i64().unwrap();
    assert!((0..=5000).contains(&first_ttl));
    assert_eq!(first.perception, ["STILL_CURRENT"]);
    for _ in 0..5 {
        let next = consciousness::last_live_presence(1_930_604);
        assert_eq!(next.perception, ["STILL_CURRENT"]);
        let ttl = next.perception_payload[0]["ttlMs"].as_i64().unwrap();
        assert!(
            ttl > 0 && ttl <= first_ttl,
            "reading must neither renew nor cumulatively subtract age"
        );
    }
}
