//! Federation inbox anti-replay (MYR-023).
//!
//! HTTP Date freshness alone is weak: a signed request can be re-played until
//! the Date falls outside the clock-skew window. This module keeps a short-lived
//! in-memory set of activity ids / body digests seen after a **successful**
//! signature verification, for a TTL matching (and slightly exceeding) that
//! date window so legitimate peer retries remain idempotent without re-running
//! side-effecting handlers.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use super::signature::HTTP_DATE_MAX_SKEW;
use super::types::normalize_activity_id;

/// How long a seen activity id / body digest is remembered.
///
/// Slightly longer than [`HTTP_DATE_MAX_SKEW`] so a request that still has a
/// fresh Date cannot slip past dedup near the edge of the skew window.
pub fn replay_dedup_ttl() -> chrono::Duration {
    HTTP_DATE_MAX_SKEW + chrono::Duration::minutes(5)
}

/// Soft cap on tracked keys (evict oldest on overflow).
const REPLAY_DEDUP_MAX_ENTRIES: usize = 16_384;

/// Build stable dedup keys for an inbound activity.
///
/// - Body digest always (covers pure HTTP signature replay of the same bytes).
/// - Normalized activity `id` when present (covers re-signed redeliveries of
///   the same ActivityPub object within the TTL).
pub fn replay_dedup_keys(activity_id: &str, body: &[u8]) -> Vec<String> {
    let mut keys = Vec::with_capacity(2);
    let digest = Sha256::digest(body);
    keys.push(format!("d:{}", hex::encode(digest)));
    let norm = normalize_activity_id(activity_id);
    if !norm.is_empty() {
        keys.push(format!("a:{norm}"));
    }
    keys
}

struct ReplayCache {
    /// key → earliest Instant when the entry may be dropped
    entries: HashMap<String, Instant>,
    /// Insertion order for overflow eviction (oldest first).
    order: Vec<String>,
}

impl ReplayCache {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            order: Vec::new(),
        }
    }

    fn purge_expired(&mut self, now: Instant) {
        self.order.retain(|k| {
            if self.entries.get(k).is_some_and(|until| *until > now) {
                true
            } else {
                self.entries.remove(k);
                false
            }
        });
    }

    /// Returns `true` if **any** key was already present (replay).
    /// Otherwise inserts all keys and returns `false` (first sighting).
    fn check_and_record(&mut self, keys: &[String], ttl: Duration, now: Instant) -> bool {
        self.purge_expired(now);
        if keys.is_empty() {
            return false;
        }
        if keys.iter().any(|k| self.entries.contains_key(k)) {
            return true;
        }
        let until = now + ttl;
        for k in keys {
            if self.entries.contains_key(k) {
                continue;
            }
            while self.entries.len() >= REPLAY_DEDUP_MAX_ENTRIES {
                if let Some(old) = self.order.first().cloned() {
                    self.order.remove(0);
                    self.entries.remove(&old);
                } else {
                    break;
                }
            }
            self.entries.insert(k.clone(), until);
            self.order.push(k.clone());
        }
        false
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

fn global_cache() -> &'static Mutex<ReplayCache> {
    static CACHE: OnceLock<Mutex<ReplayCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(ReplayCache::new()))
}

/// After signature verification: return `true` if this activity/body was
/// already accepted recently (caller should answer 202 without re-handling).
///
/// Fail-open on lock poison so a stuck mutex cannot deny all federation.
pub fn is_replay_or_record(keys: &[String]) -> bool {
    let ttl = std_duration_from_chrono(replay_dedup_ttl());
    let now = Instant::now();
    match global_cache().lock() {
        Ok(mut cache) => cache.check_and_record(keys, ttl, now),
        Err(poisoned) => {
            tracing::error!("federation replay cache lock poisoned; resetting");
            let mut cache = poisoned.into_inner();
            *cache = ReplayCache::new();
            cache.check_and_record(keys, ttl, now)
        }
    }
}

fn std_duration_from_chrono(d: chrono::Duration) -> Duration {
    Duration::from_secs(d.num_seconds().max(0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_dedup_keys_include_digest_and_normalized_id() {
        let body = br#"{"id":"https://A.example/activities/1/?x=1","type":"Follow"}"#;
        let keys = replay_dedup_keys("https://A.example/activities/1/?x=1#frag", body);
        assert_eq!(keys.len(), 2);
        assert!(keys[0].starts_with("d:"));
        assert_eq!(keys[0].len(), 2 + 64); // d: + sha256 hex
        assert_eq!(keys[1], "a:https://a.example/activities/1");
    }

    #[test]
    fn replay_dedup_keys_digest_only_when_id_missing() {
        let keys = replay_dedup_keys("", b"hello");
        assert_eq!(keys.len(), 1);
        assert!(keys[0].starts_with("d:"));
    }

    #[test]
    fn ttl_is_at_least_date_skew_window() {
        assert!(replay_dedup_ttl() >= HTTP_DATE_MAX_SKEW);
        assert!(replay_dedup_ttl() <= HTTP_DATE_MAX_SKEW + chrono::Duration::minutes(15));
    }

    #[test]
    fn first_sighting_accepted_second_is_replay() {
        let mut cache = ReplayCache::new();
        let keys = replay_dedup_keys("https://peer.example/a/1", b"body-a");
        let ttl = Duration::from_secs(600);
        let t0 = Instant::now();
        assert!(!cache.check_and_record(&keys, ttl, t0));
        assert!(cache.check_and_record(&keys, ttl, t0 + Duration::from_secs(1)));
        // Same activity id, different body → still replay (id key hits).
        let keys2 = replay_dedup_keys("https://peer.example/a/1", b"body-b");
        assert!(cache.check_and_record(&keys2, ttl, t0 + Duration::from_secs(2)));
        // Different id and body → fresh.
        let keys3 = replay_dedup_keys("https://peer.example/a/2", b"body-c");
        assert!(!cache.check_and_record(&keys3, ttl, t0 + Duration::from_secs(3)));
    }

    #[test]
    fn pure_body_replay_caught_without_activity_id() {
        let mut cache = ReplayCache::new();
        let keys = replay_dedup_keys("", b"same-bytes");
        let ttl = Duration::from_secs(600);
        let t0 = Instant::now();
        assert!(!cache.check_and_record(&keys, ttl, t0));
        assert!(cache.check_and_record(&keys, ttl, t0));
    }

    #[test]
    fn expired_entries_allow_reaccept() {
        let mut cache = ReplayCache::new();
        let keys = replay_dedup_keys("https://peer.example/a/x", b"z");
        let ttl = Duration::from_secs(10);
        let t0 = Instant::now();
        assert!(!cache.check_and_record(&keys, ttl, t0));
        assert!(cache.check_and_record(&keys, ttl, t0 + Duration::from_secs(1)));
        // After TTL, purged on next call.
        assert!(!cache.check_and_record(&keys, ttl, t0 + Duration::from_secs(11)));
    }

    #[test]
    fn overflow_evicts_oldest() {
        let mut cache = ReplayCache::new();
        let ttl = Duration::from_secs(600);
        let t0 = Instant::now();
        // Fill past the soft cap with unique digests.
        for i in 0..(REPLAY_DEDUP_MAX_ENTRIES + 8) {
            let body = format!("body-{i}");
            let keys = replay_dedup_keys("", body.as_bytes());
            assert!(
                !cache.check_and_record(&keys, ttl, t0),
                "unique body {i} should not be a replay"
            );
        }
        assert!(cache.len() <= REPLAY_DEDUP_MAX_ENTRIES);
        // First body should have been evicted.
        let first = replay_dedup_keys("", b"body-0");
        assert!(!cache.check_and_record(&first, ttl, t0));
    }
}
