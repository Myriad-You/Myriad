use super::*;

// Independent reference for expiry, oldest-first eviction and both budgets.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn previous_trim_response_cache(
    cache: &mut HashMap<String, CacheEntry>,
    now: Instant,
    cap: usize,
    byte_cap: usize,
) {
    cache.retain(|_, entry| entry.expires_at > now);
    while cache.len() > cap
        || cache.values().map(|entry| entry.size_bytes).sum::<usize>() > byte_cap
    {
        let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, entry)| entry.cached_at)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        cache.remove(&oldest);
    }
}

fn fixture(count: usize, now: Instant, size: Option<usize>) -> HashMap<String, CacheEntry> {
    let start = now - Duration::from_secs(10);
    (0..count)
        .map(|index| {
            (
                format!("key-{index:04}"),
                CacheEntry {
                    data: Arc::from(&b"{\"kept\":true}"[..]),
                    size_bytes: size.unwrap_or(64 + index % 7 * 51),
                    expires_at: if size.is_none() && index % 11 == 0 {
                        now
                    } else {
                        now + Duration::from_secs(60)
                    },
                    cached_at: start + Duration::from_micros(index as u64),
                },
            )
        })
        .collect()
}

fn copy_cache(cache: &HashMap<String, CacheEntry>) -> HashMap<String, CacheEntry> {
    cache
        .iter()
        .map(|(key, entry)| {
            (
                key.clone(),
                CacheEntry {
                    data: Arc::clone(&entry.data),
                    size_bytes: entry.size_bytes,
                    expires_at: entry.expires_at,
                    cached_at: entry.cached_at,
                },
            )
        })
        .collect()
}

fn contract(cache: &HashMap<String, CacheEntry>) -> std::collections::BTreeMap<&str, &[u8]> {
    cache
        .iter()
        .map(|(key, entry)| (key.as_str(), entry.data.as_ref()))
        .collect()
}

#[test]
fn response_trim_preserves_expiry_oldest_order_and_budget_boundaries() {
    let now = Instant::now();
    for length in [0, 1, 7, 32, 128] {
        let initial = fixture(length, now, None);
        for cap in [0, 1, 4, 16, 128] {
            for byte_cap in [0, 1, 200, 1000, 100_000] {
                let mut previous = copy_cache(&initial);
                let mut current = copy_cache(&initial);
                previous_trim_response_cache(&mut previous, now, cap, byte_cap);
                trim_response_cache(&mut current, now, cap, byte_cap);
                assert_eq!(
                    contract(&current),
                    contract(&previous),
                    "length={length} cap={cap} byte_cap={byte_cap}",
                );
            }
        }
    }
}

#[test]
fn response_trim_releases_payloads_and_handles_equal_ages() {
    let now = Instant::now();
    let mut cache = fixture(20, now, Some(100));
    let payloads: Vec<_> = cache
        .values()
        .map(|entry| Arc::downgrade(&entry.data))
        .collect();
    for entry in cache.values_mut() {
        entry.cached_at = now;
    }
    trim_response_cache(&mut cache, now, 5, 400);
    assert_eq!(cache.len(), 4);
    assert_eq!(
        payloads
            .iter()
            .filter(|weak| weak.upgrade().is_some())
            .count(),
        4
    );
    trim_response_cache(&mut cache, now + Duration::from_secs(60), 5, 400);
    assert!(cache.is_empty());
    assert!(payloads.iter().all(|weak| weak.upgrade().is_none()));
}

#[tokio::test]
async fn fresh_response_hits_share_read_lock_and_preserve_payload() {
    let now = Instant::now();
    let cache = RwLock::new(fixture(1, now, Some(100)));
    let reader = cache.read().await;
    let bytes = tokio::time::timeout(
        Duration::from_secs(1),
        cached_response_bytes(&cache, "key-0000"),
    )
    .await
    .expect("a fresh hit must not wait for the other reader")
    .unwrap();
    assert!(Arc::ptr_eq(&bytes, &reader["key-0000"].data));
    assert!(cached_response_bytes(&cache, "missing").await.is_none());
}

#[tokio::test]
async fn expired_response_read_releases_payload() {
    let now = Instant::now();
    let mut entries = fixture(1, now, None);
    let retained = Arc::downgrade(&entries["key-0000"].data);
    let cache = RwLock::new(std::mem::take(&mut entries));
    assert!(cached_response_bytes(&cache, "key-0000").await.is_none());
    assert!(cache.read().await.is_empty());
    assert!(retained.upgrade().is_none());
}

#[test]
#[ignore = "manual response-cache trim timing and allocation workload"]
fn profile_response_cache_trim() {
    #[cfg(feature = "hotpath")]
    let _profile = hotpath::HotpathGuardBuilder::new("response_cache_trim").build();
    let now = Instant::now();
    for (case, count, cap, byte_cap) in [
        ("unchanged", 2048, 2048, 96 * 1024 * 1024),
        ("one_eviction", 2049, 2048, 96 * 1024 * 1024),
        ("saver_profile", 2048, 128, 8 * 1024 * 1024),
        ("byte_pressure", 2048, 2048, 8 * 1024 * 1024),
    ] {
        // Logical retained sizes model a populated cache; shared fixture payloads
        // exclude JSON work and payload deallocation from this cache-policy probe.
        let initial = fixture(count, now, Some(40 * 1024));
        let mut old_contract = copy_cache(&initial);
        let mut new_contract = copy_cache(&initial);
        previous_trim_response_cache(&mut old_contract, now, cap, byte_cap);
        trim_response_cache(&mut new_contract, now, cap, byte_cap);
        assert_eq!(contract(&old_contract), contract(&new_contract));
        let mut previous = Vec::new();
        let mut current = Vec::new();
        for iteration in 0..30 {
            for old in if iteration % 2 == 0 {
                [true, false]
            } else {
                [false, true]
            } {
                let mut cache = copy_cache(&initial);
                let trim = if old {
                    previous_trim_response_cache
                } else {
                    trim_response_cache
                };
                let started = Instant::now();
                trim(&mut cache, now, cap, byte_cap);
                let elapsed = started.elapsed().as_nanos();
                if old {
                    previous.push(elapsed);
                } else {
                    current.push(elapsed);
                }
                std::hint::black_box(&cache);
            }
        }
        for (variant, mut samples) in [("previous", previous), ("current", current)] {
            samples.sort_unstable();
            println!(
                "response_cache_trim: case={case} variant={variant} median_ns={} calls=30 entries={count} cap={cap} byte_cap={byte_cap}",
                samples[15],
            );
        }
    }
}
