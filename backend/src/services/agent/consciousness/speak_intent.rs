//! In-process speak intents. Consciousness produces them; redeem is elsewhere.

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use chrono::{DateTime, Duration, Utc};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use super::EventUrgency;

/// Lifetime of an unredeemed opening. Not a platform config.
pub const SPEAK_INTENT_TTL_SECS: i64 = 15 * 60;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeakIntent {
    pub id: String,
    pub user_id: i32,
    pub source_event_id: String,
    pub topic: String,
    pub gist: String,
    pub priority: EventUrgency,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_intent_id: Option<String>,
}

static QUEUE: Lazy<RwLock<HashMap<i32, Vec<SpeakIntent>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub fn enqueue_speak_intent(intent: SpeakIntent) {
    if intent.user_id <= 0 {
        return;
    }
    if let Ok(mut map) = QUEUE.write() {
        map.entry(intent.user_id).or_default().push(intent);
    }
}

/// Take due intents. Expired ones are dropped with no sentence.
/// Two intents with the same `source_event_id` keep only the first.
pub fn drain_speak_intents(now: DateTime<Utc>) -> Vec<SpeakIntent> {
    let Ok(mut map) = QUEUE.write() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for queue in map.values_mut() {
        let mut seen = HashSet::new();
        let pending = std::mem::take(queue);
        for intent in pending {
            if intent.expires_at <= now {
                continue;
            }
            if !seen.insert(intent.source_event_id.clone()) {
                continue;
            }
            out.push(intent);
        }
    }
    map.retain(|_, queue| !queue.is_empty());
    out
}

pub fn new_speak_intent(
    user_id: i32,
    source_event_id: String,
    topic: String,
    gist: String,
    priority: EventUrgency,
    work_intent_id: Option<String>,
) -> SpeakIntent {
    let now = Utc::now();
    SpeakIntent {
        id: format!("spk_{}", uuid::Uuid::new_v4().simple()),
        user_id,
        source_event_id,
        topic,
        gist,
        priority,
        created_at: now,
        expires_at: now + Duration::seconds(SPEAK_INTENT_TTL_SECS),
        work_intent_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(user_id: i32, source: &str, expires_in_secs: i64) -> SpeakIntent {
        let now = Utc::now();
        SpeakIntent {
            id: format!("spk_{source}"),
            user_id,
            source_event_id: source.into(),
            topic: "agent.merope.platform_activity".into(),
            gist: "想说一声刚才的事".into(),
            priority: EventUrgency::Normal,
            created_at: now,
            expires_at: now + Duration::seconds(expires_in_secs),
            work_intent_id: None,
        }
    }

    #[test]
    fn expired_intents_are_dropped_without_redeem() {
        enqueue_speak_intent(intent(501, "evt-expired", -5));
        let due = drain_speak_intents(Utc::now());
        assert!(due.iter().all(|item| item.user_id != 501));
    }

    #[test]
    fn same_source_event_redeems_once() {
        enqueue_speak_intent(intent(502, "evt-dup", 60));
        enqueue_speak_intent(intent(502, "evt-dup", 60));
        let due: Vec<_> = drain_speak_intents(Utc::now())
            .into_iter()
            .filter(|item| item.user_id == 502)
            .collect();
        assert_eq!(due.len(), 1);
    }
}
