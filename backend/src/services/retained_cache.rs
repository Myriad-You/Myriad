//! Small process-local caches: fixed capacity, expiring values, no background task.
use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

struct Entry<V> {
    value: V,
    expires: Instant,
    accessed: Instant,
}

pub(crate) struct RetainedCache<K, V> {
    entries: HashMap<K, Entry<V>>,
    capacity: usize,
    ttl: Duration,
    // Conservative lower bound: replacing or evicting a key may leave an earlier deadline.
    next_expiry: Option<Instant>,
}

impl<K: Clone + Eq + Hash, V> RetainedCache<K, V> {
    pub(crate) fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            capacity,
            ttl,
            next_expiry: None,
        }
    }

    pub(crate) fn get<Q: ?Sized + Eq + Hash>(&mut self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
    {
        self.get_at(key, Instant::now())
    }

    fn get_at<Q: ?Sized + Eq + Hash>(&mut self, key: &Q, now: Instant) -> Option<&V>
    where
        K: Borrow<Q>,
    {
        self.purge_at(now);
        let entry = self.entries.get_mut(key)?;
        entry.accessed = now;
        Some(&entry.value)
    }

    pub(crate) fn insert(&mut self, key: K, value: V) {
        self.insert_with_ttl(key, value, self.ttl);
    }

    pub(crate) fn insert_with_ttl(&mut self, key: K, value: V, ttl: Duration) {
        self.insert_at(key, value, ttl, Instant::now());
    }

    fn insert_at(&mut self, key: K, value: V, ttl: Duration, now: Instant) {
        self.purge_at(now);
        if self.capacity == 0 {
            return;
        }
        if self.entries.len() >= self.capacity && !self.entries.contains_key(&key) {
            if let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.accessed)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&oldest);
            }
        }
        let expires = now + ttl;
        self.entries.insert(
            key,
            Entry {
                value,
                expires,
                accessed: now,
            },
        );
        self.next_expiry = Some(self.next_expiry.map_or(expires, |next| next.min(expires)));
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn purge_expired(&mut self) {
        self.purge_at(Instant::now());
    }

    fn purge_at(&mut self, now: Instant) {
        if self.next_expiry.is_none_or(|expires| expires > now) {
            return;
        }
        let mut next_expiry: Option<Instant> = None;
        self.entries.retain(|_, entry| {
            if entry.expires <= now {
                return false;
            }
            next_expiry = Some(next_expiry.map_or(entry.expires, |next| next.min(entry.expires)));
            true
        });
        self.next_expiry = next_expiry;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn churn_is_bounded_and_idle_sweep_releases_values() {
        let now = Instant::now();
        let ttl = Duration::from_secs(60);
        let mut cache = RetainedCache::new(32, ttl);
        for id in 0..50_000 {
            cache.insert_at(id, vec![0u8; 128], ttl, now);
        }
        assert_eq!(cache.entries.len(), 32);
        cache.purge_at(now + ttl);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn access_protects_recent_values_but_does_not_extend_freshness() {
        let now = Instant::now();
        let ttl = Duration::from_secs(60);
        let mut cache = RetainedCache::new(2, ttl);
        cache.insert_at("a", 1, ttl, now);
        cache.insert_at("b", 2, ttl, now + Duration::from_secs(1));
        assert_eq!(cache.get_at("a", now + Duration::from_secs(2)), Some(&1));
        cache.insert_at("c", 3, ttl, now + Duration::from_secs(3));
        assert!(!cache.entries.contains_key("b"));
        assert_eq!(cache.get_at("a", now + ttl), None);
        assert_eq!(cache.get_at("c", now + ttl), Some(&3));
    }

    #[test]
    fn replacement_does_not_evict_other_keys_and_entry_ttls_differ() {
        let now = Instant::now();
        let ttl = Duration::from_secs(60);
        let mut cache = RetainedCache::new(2, ttl);
        cache.insert_at("a", 1, ttl, now);
        cache.insert_at("b", 2, ttl, now);
        cache.insert_at("a", 3, Duration::from_secs(1), now);
        assert_eq!(cache.entries.len(), 2);
        cache.purge_at(now + Duration::from_secs(1));
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.get_at("b", now + Duration::from_secs(1)), Some(&2));
    }

    #[test]
    fn replacing_or_evicting_earliest_key_keeps_other_deadlines_exact() {
        let now = Instant::now();
        let second = Duration::from_secs(1);
        let mut cache = RetainedCache::new(2, second * 10);
        cache.insert_at("a", 1, second, now);
        cache.insert_at("b", 2, second * 2, now);
        cache.insert_at("a", 3, second * 10, now);
        assert_eq!(cache.get_at("a", now + second), Some(&3));
        assert_eq!(cache.get_at("b", now + second * 2), None);
        assert_eq!(cache.get_at("a", now + second * 10), None);

        cache.insert_at("a", 1, second, now);
        cache.insert_at("b", 2, second * 2, now);
        cache.get_at("b", now + second / 4);
        cache.insert_at("c", 3, second * 3, now + second / 2);
        assert!(!cache.entries.contains_key("a"));
        assert_eq!(cache.get_at("b", now + second), Some(&2));
        assert_eq!(cache.get_at("b", now + second * 2), None);
        assert_eq!(cache.get_at("c", now + second * 3), Some(&3));
    }

    #[test]
    fn an_earlier_insert_expires_and_releases_unrequested_values() {
        use std::sync::Arc;
        let now = Instant::now();
        let second = Duration::from_secs(1);
        let mut cache = RetainedCache::new(2, second * 60);
        cache.insert_at("long", Arc::new(vec![1.0f32]), second * 60, now);
        let short = Arc::new(vec![2.0f32]);
        let released = Arc::downgrade(&short);
        cache.insert_at("short", short, second, now);
        assert!(released.upgrade().is_some());
        assert!(cache.get_at("long", now + second).is_some());
        assert!(released.upgrade().is_none());
        assert_eq!(cache.len(), 1);
        assert!(cache.get_at("long", now + second * 60).is_none());
    }

    #[test]
    #[ignore = "manual cache-hit timing workload; run alone with --nocapture"]
    fn profile_retained_cache_hits() {
        let keys: Vec<_> = (0..4096)
            .map(|id| ("embedding-model".to_string(), format!("{id:032x}")))
            .collect();
        let mut cache = RetainedCache::new(keys.len(), Duration::from_secs(3600));
        for (id, key) in keys.iter().enumerate() {
            cache.insert(key.clone(), id);
        }
        let iterations = 20_000;
        let mut samples = Vec::new();
        for _ in 0..5 {
            let start = Instant::now();
            for id in 0..iterations {
                std::hint::black_box(cache.get(std::hint::black_box(&keys[id % keys.len()])));
            }
            samples.push(start.elapsed().as_nanos() / iterations as u128);
        }
        samples.sort_unstable();
        println!(
            "retained_cache_hits: entries={} iterations={iterations} samples_ns_per_hit={samples:?} median_ns_per_hit={}",
            keys.len(),
            samples[samples.len() / 2]
        );
    }
}
