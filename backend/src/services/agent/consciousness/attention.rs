//! Current attention segment. Process-local; expires; never 人设记忆.

use std::collections::HashMap;
use std::sync::RwLock;

use chrono::{DateTime, Duration, Utc};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

const ATTENTION_TTL_SECS: i64 = 15 * 60;
const EVENT_ID_CAP: usize = 16;
const SEGMENT_CAP: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionSegment {
    pub topic: String,
    /// Inner voice for this stretch. Not a remembered fact.
    pub inner: String,
    pub opened_at: DateTime<Utc>,
    pub last_touched_at: DateTime<Utc>,
    pub event_ids: Vec<String>,
}

static SEGMENTS: Lazy<RwLock<HashMap<i32, AttentionSegment>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

fn prune_segments(map: &mut HashMap<i32, AttentionSegment>, now: DateTime<Utc>) {
    map.retain(|_, segment| {
        now.signed_duration_since(segment.last_touched_at) <= Duration::seconds(ATTENTION_TTL_SECS)
    });
}

pub(crate) fn cleanup_attention() {
    if let Ok(mut map) = SEGMENTS.write() {
        prune_segments(&mut map, crate::clock::now());
    }
}

pub(crate) fn clear_user_attention(user_id: i32) {
    if let Ok(mut map) = SEGMENTS.write() {
        map.remove(&user_id);
    }
}

pub fn last_attention(user_id: i32) -> Option<AttentionSegment> {
    let mut map = SEGMENTS.write().ok()?;
    prune_segments(&mut map, crate::clock::now());
    map.get(&user_id).cloned()
}

pub fn next_attention_segment(
    current: Option<&AttentionSegment>,
    topic: &str,
    inner: &str,
    event_id: &str,
    now: DateTime<Utc>,
) -> AttentionSegment {
    if let Some(current) = current {
        let expired = now.signed_duration_since(current.last_touched_at)
            > Duration::seconds(ATTENTION_TTL_SECS);
        if !expired && current.topic == topic {
            let mut event_ids = current.event_ids.clone();
            if event_ids.last().map(String::as_str) != Some(event_id) {
                event_ids.push(event_id.to_string());
            }
            if event_ids.len() > EVENT_ID_CAP {
                let drop = event_ids.len() - EVENT_ID_CAP;
                event_ids.drain(..drop);
            }
            return AttentionSegment {
                topic: current.topic.clone(),
                inner: inner.to_string(),
                opened_at: current.opened_at,
                last_touched_at: now,
                event_ids,
            };
        }
    }
    AttentionSegment {
        topic: topic.to_string(),
        inner: inner.to_string(),
        opened_at: now,
        last_touched_at: now,
        event_ids: vec![event_id.to_string()],
    }
}

pub fn touch_attention(user_id: i32, topic: &str, inner: &str, event_id: &str, now: DateTime<Utc>) {
    if user_id <= 0 {
        return;
    }
    if let Ok(mut map) = SEGMENTS.write() {
        prune_segments(&mut map, now);
        let next = next_attention_segment(map.get(&user_id), topic, inner, event_id, now);
        if map.len() >= SEGMENT_CAP && !map.contains_key(&user_id) {
            if let Some(oldest) = map
                .iter()
                .min_by_key(|(_, segment)| segment.last_touched_at)
                .map(|(id, _)| *id)
            {
                map.remove(&oldest);
            }
        }
        map.insert(user_id, next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_user_churn_stays_bounded() {
        let now = crate::clock::now();
        for user_id in 10_000..12_000 {
            touch_attention(user_id, "topic", "inner", "event", now);
        }
        let mut map = SEGMENTS.write().unwrap();
        assert!(map.len() <= SEGMENT_CAP);
        map.retain(|user_id, _| !(10_000..12_000).contains(user_id));
    }

    #[test]
    fn same_topic_continues_the_segment() {
        let t0 = crate::clock::now();
        let first = next_attention_segment(None, "phantasi.source_error", "feed failed", "e1", t0);
        let t1 = t0 + Duration::seconds(5);
        let second = next_attention_segment(
            Some(&first),
            "phantasi.source_error",
            "still failing",
            "e2",
            t1,
        );
        assert_eq!(second.event_ids.len(), 2);
        assert_eq!(second.opened_at, first.opened_at);
        assert_eq!(second.last_touched_at, t1);
        assert_ne!(second.last_touched_at, first.last_touched_at);
    }

    #[test]
    fn topic_change_opens_a_new_segment() {
        let t0 = crate::clock::now();
        let first = next_attention_segment(None, "phantasi.source_error", "feed failed", "e1", t0);
        let t1 = t0 + Duration::seconds(5);
        let second = next_attention_segment(Some(&first), "agent.task_completed", "done", "e2", t1);
        assert_eq!(second.event_ids, vec!["e2".to_string()]);
        assert_eq!(second.opened_at, t1);
        assert_ne!(second.opened_at, first.opened_at);
    }

    #[test]
    fn expired_segment_is_not_read() {
        let stale = AttentionSegment {
            topic: "phantasi.source_error".into(),
            inner: "old".into(),
            opened_at: crate::clock::now() - Duration::seconds(ATTENTION_TTL_SECS + 30),
            last_touched_at: crate::clock::now() - Duration::seconds(ATTENTION_TTL_SECS + 10),
            event_ids: vec!["e1".into()],
        };
        if let Ok(mut map) = SEGMENTS.write() {
            map.insert(401, stale);
        }
        assert!(last_attention(401).is_none());
        assert!(!SEGMENTS.read().unwrap().contains_key(&401));
    }

    #[test]
    fn event_ids_drop_from_the_front_past_the_cap() {
        let t0 = crate::clock::now();
        let mut segment = next_attention_segment(None, "topic", "inner", "e0", t0);
        for i in 1..=EVENT_ID_CAP {
            segment = next_attention_segment(
                Some(&segment),
                "topic",
                "inner",
                &format!("e{i}"),
                t0 + Duration::seconds(i as i64),
            );
        }
        assert_eq!(segment.event_ids.len(), EVENT_ID_CAP);
        assert_eq!(segment.event_ids.first().map(String::as_str), Some("e1"));
        assert_eq!(segment.event_ids.last().map(String::as_str), Some("e16"));
    }
}
