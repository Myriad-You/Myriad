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

/// Returns true when the addressee moved from absent/expired to on-page.
pub fn remember_live_presence(user_id: i32, live: SelfLivePresence) -> bool {
    if user_id <= 0 {
        return false;
    }
    let was_present = live_presence_is_on_page(user_id);
    let now_present = live.page_visible;
    if let Ok(mut map) = LIVE.write() {
        map.insert(user_id, live);
    }
    !was_present && now_present
}

pub fn last_live_presence(user_id: i32) -> SelfLivePresence {
    let mut live = LIVE
        .read()
        .ok()
        .and_then(|map| map.get(&user_id).cloned())
        .filter(presence_is_fresh)
        .unwrap_or_default();
    // The page-presence lease is longer than individual observations. Age a
    // copy on read, never renew the stored TTL by repeatedly reading it.
    let elapsed = live.captured_at.map_or(0, |at| {
        Utc::now()
            .signed_duration_since(at)
            .num_milliseconds()
            .max(0)
    });
    for item in &mut live.perception_payload {
        let remaining = item
            .get("ttlMs")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .saturating_sub(elapsed)
            .max(0);
        item["ttlMs"] = remaining.into();
    }
    live.perception_payload.retain(|item| {
        item.get("ttlMs")
            .and_then(Value::as_i64)
            .is_some_and(|ttl| ttl > 0)
    });
    refresh_perception_text(&mut live);
    live
}

fn refresh_perception_text(live: &mut SelfLivePresence) {
    live.perception = live
        .perception_payload
        .iter()
        .filter_map(Value::as_object)
        .map(crate::services::agent::perception_view::perception_reader_text)
        .filter(|text| !text.is_empty())
        .map(|text| text.chars().take(160).collect())
        .take(crate::services::agent::perception_view::MAX_PERCEPTION_ITEMS)
        .collect();
}

pub fn live_presence_is_on_page(user_id: i32) -> bool {
    last_live_presence(user_id).page_visible
}

/// Looking at her: on the page and the Agent panel is open.
pub fn live_presence_panel_open(user_id: i32) -> bool {
    let live = last_live_presence(user_id);
    live.page_visible && live.panel_visible
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
        live.page_visible = presence
            .get("pageVisible")
            .and_then(Value::as_bool)
            .unwrap_or(live.page_visible);
        live.panel_visible = presence
            .get("panelVisible")
            .and_then(Value::as_bool)
            .unwrap_or(live.panel_visible);
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
        live.perception_payload = items
            .iter()
            .filter_map(sanitize_perception)
            .take(crate::services::agent::perception_view::MAX_PERCEPTION_ITEMS)
            .collect();
        refresh_perception_text(live);
    }
    if let Some(music) = data.get("musicStatus") {
        live.music_status = sanitize_music_status(music);
    }
}

fn sanitize_perception(value: &Value) -> Option<Value> {
    let obj = value.as_object()?;
    let source = obj
        .get("sourceId")?
        .as_str()?
        .chars()
        .take(80)
        .collect::<String>();
    let kind = obj.get("kind")?.as_str()?;
    if source.is_empty()
        || !matches!(
            kind,
            "page" | "pointer" | "surface" | "music" | "voice" | "presence" | "screen"
        )
    {
        return None;
    }
    let privacy = obj
        .get("privacy")
        .and_then(Value::as_str)
        .unwrap_or("local");
    if !matches!(privacy, "local" | "consented" | "system") {
        return None;
    }
    let ttl = obj
        .get("ttlMs")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .clamp(0, 90_000);
    if ttl == 0 {
        return None;
    }
    let mut facts = serde_json::Map::new();
    if let Some(input) = obj.get("safeFacts").and_then(Value::as_object) {
        for (key, value) in input.iter().take(12) {
            let value = match value {
                Value::String(text) => Value::String(text.chars().take(120).collect()),
                Value::Bool(_) | Value::Number(_) => value.clone(),
                _ => continue,
            };
            facts.insert(key.chars().take(40).collect(), value);
        }
    }
    Some(serde_json::json!({
        "sourceId": source,
        "kind": kind,
        "ttlMs": ttl,
        "summary": if privacy == "local" { String::new() } else {
            obj.get("summary").and_then(Value::as_str).unwrap_or("").chars().take(400).collect()
        },
        "safeFacts": facts,
        "privacy": privacy,
    }))
}

fn sanitize_music_status(value: &Value) -> Option<Value> {
    let obj = value.as_object()?;
    let song = obj
        .get("currentSong")
        .and_then(Value::as_object)
        .map(|song| {
            let text = |key: &str, max: usize| {
                song.get(key)
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .chars()
                    .take(max)
                    .collect::<String>()
            };
            serde_json::json!({
                "name": text("name", 80), "artist": text("artist", 80),
                "album": text("album", 80), "source": text("source", 40),
                "duration": song.get("duration").and_then(Value::as_u64).unwrap_or(0).min(86_400),
            })
        });
    Some(serde_json::json!({
        "isPlaying": obj.get("isPlaying").and_then(Value::as_bool).unwrap_or(false),
        "isEnabled": obj.get("isEnabled").and_then(Value::as_bool).unwrap_or(false),
        "currentSong": song,
        "currentSongIndex": obj.get("currentSongIndex").and_then(Value::as_u64).unwrap_or(0).min(10_000),
        "playlistLength": obj.get("playlistLength").and_then(Value::as_u64).unwrap_or(0).min(10_000),
        "currentLyric": obj.get("currentLyric").and_then(Value::as_str).unwrap_or("").chars().take(120).collect::<String>(),
    }))
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
                    "perception": [{
                        "sourceId": "music", "kind": "music", "ttlMs": 2000,
                        "privacy": "system", "summary": "music idle"
                    }]
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
            perception_payload: vec![serde_json::json!({
                "sourceId": "page", "kind": "page", "privacy": "consented",
                "summary": "hours-old", "ttlMs": 90_000,
            })],
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
                        "sourceId": "pointer",
                        "kind": "pointer",
                        "ttlMs": 2000,
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
                        "sourceId": "page",
                        "kind": "page",
                        "ttlMs": 8000,
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
                    "sourceId": format!("presence-{i}"),
                    "kind": "presence",
                    "ttlMs": 4000,
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
                "pageVisible": true,
                "panelVisible": true,
                "__admin": true
            },
            "perception": [{
                "sourceId": "presence", "kind": "presence", "ttlMs": 4000,
                "privacy": "system", "summary": "x", "raw": "<pixels>"
            }],
            "secret": "nope"
        }));
        assert!(live.speaking);
        assert!(live.page_visible);
        assert!(live.panel_visible);
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
                "sourceId": format!("presence-{i}"),
                "kind": "presence",
                "ttlMs": 4000,
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

    #[test]
    fn remember_reports_revival_onto_the_page() {
        let live = live_presence_from_custom_data(&serde_json::json!({
            "presence": { "pageVisible": true }
        }));
        assert!(remember_live_presence(96, live.clone()));
        assert!(live_presence_is_on_page(96));
        assert!(!live_presence_panel_open(96));
        assert!(!remember_live_presence(96, live));
    }

    #[test]
    fn panel_open_requires_the_page_and_the_panel() {
        let on_page = live_presence_from_custom_data(&serde_json::json!({
            "presence": { "pageVisible": true, "panelVisible": true }
        }));
        remember_live_presence(97, on_page);
        assert!(live_presence_is_on_page(97));
        assert!(live_presence_panel_open(97));
        let page_only = live_presence_from_custom_data(&serde_json::json!({
            "presence": { "pageVisible": true, "panelVisible": false }
        }));
        remember_live_presence(97, page_only);
        assert!(live_presence_is_on_page(97));
        assert!(!live_presence_panel_open(97));
    }
}
