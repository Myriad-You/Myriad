//! Process-local live presence. Not a table, not a grant.

use std::collections::HashMap;
use std::sync::RwLock;

use chrono::{Duration, Utc};
use once_cell::sync::Lazy;
use serde_json::Value;

use crate::services::agent::presence_window::PRESENCE_WINDOW_SECS;
use crate::services::agent::types::UserRequest;

use super::SelfLivePresence;

static LIVE: Lazy<RwLock<HashMap<i32, SelfLivePresence>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub fn remember_live_presence(user_id: i32, live: SelfLivePresence) {
    if user_id <= 0 {
        return;
    }
    if let Ok(mut map) = LIVE.write() {
        map.insert(user_id, live);
    }
}

pub fn last_live_presence(user_id: i32) -> SelfLivePresence {
    LIVE.read()
        .ok()
        .and_then(|map| map.get(&user_id).cloned())
        .filter(presence_is_fresh)
        .unwrap_or_default()
}

fn presence_is_fresh(live: &SelfLivePresence) -> bool {
    let Some(captured_at) = live.captured_at else {
        return false;
    };
    Utc::now().signed_duration_since(captured_at) <= Duration::seconds(PRESENCE_WINDOW_SECS)
}

/// Whitelist for inbound presence JSON. Unknown keys are ignored; perception
/// keeps only reader-safe text (local privacy never yields `summary`).
pub fn live_presence_from_custom_data(data: &Value) -> SelfLivePresence {
    let mut live = SelfLivePresence::default();
    apply_whitelisted_presence(&mut live, data);
    live.captured_at = Some(Utc::now());
    live
}

fn apply_whitelisted_presence(live: &mut SelfLivePresence, data: &Value) {
    if let Some(presence) = data.get("presence").and_then(Value::as_object) {
        live.speaking = presence
            .get("speaking")
            .and_then(Value::as_bool)
            .unwrap_or(live.speaking);
        live.face_visible = presence
            .get("faceVisible")
            .and_then(Value::as_bool)
            .unwrap_or(live.face_visible);
        live.speech_interruptible = presence
            .get("speechInterruptible")
            .and_then(Value::as_bool)
            .unwrap_or(live.speech_interruptible);
        if let Some(mode) = presence.get("visibleMode").and_then(Value::as_str) {
            live.visible_mode = Some(mode.to_string());
        }
        live.motion_intent = presence
            .get("motionIntent")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(live.motion_intent.clone());
        live.speech_intent = presence
            .get("speechIntent")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(live.speech_intent.clone());
    }
    if let Some(items) = data.get("perception").and_then(Value::as_array) {
        live.perception = items
            .iter()
            .filter_map(Value::as_object)
            .map(crate::services::agent::perception_view::perception_reader_text)
            .filter(|text| !text.is_empty())
            .map(|text| text.chars().take(160).collect())
            .take(crate::services::agent::perception_view::MAX_PERCEPTION_ITEMS)
            .collect();
    }
}

pub fn live_presence_from_request(request: &UserRequest) -> SelfLivePresence {
    let mut live = SelfLivePresence::default();
    let Some(context) = request.context.as_ref() else {
        live.captured_at = Some(Utc::now());
        return live;
    };
    live.rig_state = context.rig_state.clone();
    live.visible_mode = Some(context.interaction_mode.as_str().to_string());
    if let Some(rig) = live.rig_state.as_ref() {
        live.speaking = rig.speaking;
        live.face_visible = rig.face_visible;
        live.motion_intent = rig.acting.intent.clone();
        live.speech_intent = if rig.speaking {
            Some("speech".into())
        } else {
            Some("idle".into())
        };
        live.speech_interruptible = rig.speaking;
    }
    if let Some(data) = context.custom_data.as_ref() {
        apply_whitelisted_presence(&mut live, data);
    }
    live.captured_at = Some(Utc::now());
    live
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent::types::{RequestContext, UserRequest};
    use crate::services::agent::AgentInteractionMode;

    #[test]
    fn live_presence_does_not_invent_grants() {
        let request = UserRequest {
            raw_input: "hi".into(),
            timestamp: chrono::Utc::now(),
            user_id: 7,
            context: Some(RequestContext {
                interaction_mode: AgentInteractionMode::Chat,
                custom_data: Some(serde_json::json!({
                    "presence": {
                        "speaking": true,
                        "speechInterruptible": true,
                        "visibleMode": "chat",
                        "speechIntent": "tts"
                    },
                    "perception": [{ "summary": "music idle" }]
                })),
                ..Default::default()
            }),
        };
        let live = live_presence_from_request(&request);
        assert!(live.speaking);
        assert_eq!(live.visible_mode.as_deref(), Some("chat"));
        assert_eq!(live.perception, vec!["music idle"]);
        remember_live_presence(7, live.clone());
        assert_eq!(last_live_presence(7).speech_intent.as_deref(), Some("tts"));
        assert!(last_live_presence(7).captured_at.is_some());
    }

    #[test]
    fn last_live_presence_expires_stale_facts() {
        let mut live = SelfLivePresence {
            speaking: true,
            perception: vec!["hours-old".into()],
            captured_at: Some(Utc::now() - Duration::seconds(120)),
            ..Default::default()
        };
        remember_live_presence(91, live.clone());
        let expired = last_live_presence(91);
        assert!(!expired.speaking);
        assert!(expired.perception.is_empty());

        live.captured_at = Some(Utc::now() - Duration::seconds(60));
        remember_live_presence(91, live.clone());
        let fresh = last_live_presence(91);
        assert!(fresh.speaking);
        assert_eq!(fresh.perception, vec!["hours-old"]);
    }

    #[test]
    fn live_presence_honors_local_perception_privacy() {
        let request = UserRequest {
            raw_input: "hi".into(),
            timestamp: chrono::Utc::now(),
            user_id: 93,
            context: Some(RequestContext {
                interaction_mode: AgentInteractionMode::Chat,
                custom_data: Some(serde_json::json!({
                    "perception": [{
                        "kind": "pointer",
                        "privacy": "local",
                        "summary": "SECRET_PAGE_BODY",
                        "safeFacts": { "route": "/inbox" }
                    }]
                })),
                ..Default::default()
            }),
        };
        let live = live_presence_from_request(&request);
        assert!(
            !live
                .perception
                .iter()
                .any(|line| line.contains("SECRET_PAGE_BODY")),
            "local summary must not reach the autonomy read path"
        );
        assert_eq!(live.perception, vec!["route=/inbox"]);
    }

    #[test]
    fn live_presence_keeps_consented_perception_summary() {
        let request = UserRequest {
            raw_input: "hi".into(),
            timestamp: chrono::Utc::now(),
            user_id: 94,
            context: Some(RequestContext {
                interaction_mode: AgentInteractionMode::Chat,
                custom_data: Some(serde_json::json!({
                    "perception": [{
                        "kind": "page",
                        "privacy": "consented",
                        "summary": "Hello article",
                        "safeFacts": { "title": "Hello" }
                    }]
                })),
                ..Default::default()
            }),
        };
        let live = live_presence_from_request(&request);
        assert_eq!(live.perception, vec!["Hello article"]);
    }

    #[test]
    fn live_presence_keeps_more_than_eight_perception_rows() {
        let rows: Vec<_> = (0..9)
            .map(|i| {
                serde_json::json!({
                    "kind": "presence",
                    "privacy": "consented",
                    "summary": format!("src-{i}"),
                })
            })
            .collect();
        let request = UserRequest {
            raw_input: "hi".into(),
            timestamp: chrono::Utc::now(),
            user_id: 95,
            context: Some(RequestContext {
                interaction_mode: AgentInteractionMode::Chat,
                custom_data: Some(serde_json::json!({ "perception": rows })),
                ..Default::default()
            }),
        };
        let live = live_presence_from_request(&request);
        assert_eq!(live.perception.len(), 9);
        assert_eq!(live.perception[8], "src-8");
    }

    #[test]
    fn last_live_presence_treats_missing_timestamp_as_expired() {
        remember_live_presence(
            92,
            SelfLivePresence {
                speaking: true,
                captured_at: None,
                ..Default::default()
            },
        );
        let expired = last_live_presence(92);
        assert!(!expired.speaking);
        assert!(expired.perception.is_empty());
    }

    #[test]
    fn custom_data_whitelist_drops_unknown_keys_and_raw_perception() {
        let live = live_presence_from_custom_data(&serde_json::json!({
            "presence": {
                "speaking": true,
                "__admin": true
            },
            "perception": [{ "summary": "x", "raw": "<pixels>" }],
            "secret": "nope"
        }));
        assert!(live.speaking);
        let encoded = serde_json::to_value(&live).unwrap();
        assert!(encoded.get("__admin").is_none());
        assert_eq!(
            encoded.get("perception").and_then(|v| v.as_array()),
            Some(&vec![serde_json::json!("x")])
        );
        assert!(encoded.get("secret").is_none());
        assert!(!encoded.to_string().contains("__admin"));
        assert!(!encoded.to_string().contains("<pixels>"));
    }

    #[test]
    fn custom_data_whitelist_truncates_perception_count_and_length() {
        let cap = crate::services::agent::perception_view::MAX_PERCEPTION_ITEMS;
        let long: String = "s".repeat(161);
        let mut items = Vec::new();
        for i in 0..15 {
            items.push(serde_json::json!({
                "summary": format!("{long}-{i}"),
                "privacy": "consented"
            }));
        }
        let live = live_presence_from_custom_data(&serde_json::json!({
            "perception": items
        }));
        assert_eq!(live.perception.len(), cap);
        assert!(live.perception.iter().all(|row| row.chars().count() == 160));
    }

    #[test]
    fn remembering_custom_data_stamps_fresh_captured_at() {
        let live = live_presence_from_custom_data(&serde_json::json!({
            "presence": { "speaking": true }
        }));
        remember_live_presence(95, live);
        let stored = last_live_presence(95);
        assert!(stored.speaking);
        let captured = stored.captured_at.expect("fresh timestamp");
        assert!(Utc::now().signed_duration_since(captured) < Duration::seconds(2));
    }
}
