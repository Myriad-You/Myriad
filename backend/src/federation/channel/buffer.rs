//! Early ChannelMessage / KeyExchange buffer.
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

// Early channel activity buffer
//
// ChannelOpen can race with ChannelMessage / KeyExchange: the latter may arrive
// before the channel row exists. Buffer briefly; on ChannelOpen flush in order.
// If the buffer is full or the entry expires, return an error so the peer retries.

pub(super) struct BufferedChannelActivity {
    pub(super) actor_url: String,
    pub(super) activity: serde_json::Value,
    buffered_at: Instant,
}

const EARLY_MSG_TTL: Duration = Duration::from_secs(120);
pub(crate) const EARLY_MSG_MAX_PER_CHANNEL: usize = 64;
const EARLY_MSG_MAX_TOTAL: usize = 256;

fn early_activity_buffer() -> &'static Mutex<HashMap<String, Vec<BufferedChannelActivity>>> {
    static BUF: OnceLock<Mutex<HashMap<String, Vec<BufferedChannelActivity>>>> = OnceLock::new();
    BUF.get_or_init(|| Mutex::new(HashMap::new()))
}

fn purge_expired_early_activities(map: &mut HashMap<String, Vec<BufferedChannelActivity>>) {
    let now = Instant::now();
    map.retain(|_, msgs| {
        msgs.retain(|m| now.duration_since(m.buffered_at) < EARLY_MSG_TTL);
        !msgs.is_empty()
    });
}

/// Buffer ChannelMessage or KeyExchange that arrived before the channel row exists.
pub(crate) fn buffer_early_channel_activity(
    channel_id: &str,
    actor_url: &str,
    activity: &serde_json::Value,
) -> bool {
    let Ok(mut map) = early_activity_buffer().lock() else {
        return false;
    };
    purge_expired_early_activities(&mut map);

    let total: usize = map.values().map(|v| v.len()).sum();
    if total >= EARLY_MSG_MAX_TOTAL {
        return false;
    }

    let entry = map.entry(channel_id.to_string()).or_default();
    if entry.len() >= EARLY_MSG_MAX_PER_CHANNEL {
        return false;
    }

    // Deduplicate by AP activity id or messageId
    let dedupe_key = activity.get("id").and_then(|v| v.as_str()).or_else(|| {
        activity
            .get("object")
            .and_then(|o| o.get("messageId"))
            .and_then(|v| v.as_str())
    });
    if let Some(key) = dedupe_key {
        let already = entry.iter().any(|m| {
            m.activity.get("id").and_then(|v| v.as_str()) == Some(key)
                || m.activity
                    .get("object")
                    .and_then(|o| o.get("messageId"))
                    .and_then(|v| v.as_str())
                    == Some(key)
        });
        if already {
            return true;
        }
    }

    entry.push(BufferedChannelActivity {
        actor_url: actor_url.to_string(),
        activity: activity.clone(),
        buffered_at: Instant::now(),
    });
    true
}

/// Backward-compatible name used by ChannelMessage path.
pub(super) fn buffer_early_channel_message(
    channel_id: &str,
    actor_url: &str,
    activity: &serde_json::Value,
) -> bool {
    buffer_early_channel_activity(channel_id, actor_url, activity)
}

pub(super) fn take_early_channel_activities(channel_id: &str) -> Vec<BufferedChannelActivity> {
    let Ok(mut map) = early_activity_buffer().lock() else {
        return Vec::new();
    };
    purge_expired_early_activities(&mut map);
    map.remove(channel_id).unwrap_or_default()
}

#[cfg(test)]
mod channel_early_buffer_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn early_buffer_accepts_and_dedupes_by_activity_id() {
        let id = "ch_test_buffer_unit";
        // clear any prior via purge by using unique id
        let act = json!({"id": "https://example.com/act/1", "type": "Create"});
        assert!(buffer_early_channel_activity(
            id,
            "https://a.example/actor",
            &act
        ));
        // second with same id is treated as success (dedupe)
        assert!(buffer_early_channel_activity(
            id,
            "https://a.example/actor",
            &act
        ));
    }

    #[test]
    fn early_buffer_rejects_when_per_channel_cap_reached() {
        let id = "ch_test_buffer_cap";
        for i in 0..EARLY_MSG_MAX_PER_CHANNEL {
            let act = json!({"id": format!("https://example.com/act/{i}"), "type": "Create"});
            assert!(
                buffer_early_channel_activity(id, "https://a.example/actor", &act),
                "i={i}"
            );
        }
        let overflow = json!({"id": "https://example.com/act/overflow", "type": "Create"});
        assert!(!buffer_early_channel_activity(
            id,
            "https://a.example/actor",
            &overflow
        ));
    }
}
