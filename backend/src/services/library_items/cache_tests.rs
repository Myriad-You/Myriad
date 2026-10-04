use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn items(title: &str) -> Vec<LibraryItem> {
    vec![LibraryItem {
        id: "1".into(),
        item_type: "game".into(),
        title: title.into(),
        cover: None,
        platform: "Steam".into(),
        metadata: json!({}),
    }]
}

#[tokio::test]
async fn idle_expiry_releases_the_cache_but_preserves_active_readers() {
    let cache = LibraryAssemblyCache::default();
    let reader = cache.get_or_load(42, || async { items("A") }).await;
    let released = Arc::downgrade(&reader);
    let created = cache
        .state
        .read()
        .unwrap()
        .entry
        .as_ref()
        .unwrap()
        .cached_at;
    cache.cleanup_at(created + LIBRARY_ASSEMBLY_CACHE_TTL - Duration::from_nanos(1));
    assert!(cache.get(42).is_some());
    cache.cleanup_at(created + LIBRARY_ASSEMBLY_CACHE_TTL);
    assert!(cache.get(42).is_none());
    assert_eq!(reader.items()[0].title, "A");
    assert!(released.upgrade().is_some());
    drop(reader);
    assert!(released.upgrade().is_none());
}

#[tokio::test]
async fn sixteen_cold_readers_share_one_load_and_one_arc() {
    let cache = Arc::new(LibraryAssemblyCache::default());
    let loads = Arc::new(AtomicUsize::new(0));
    let start = Arc::new(tokio::sync::Barrier::new(17));
    let mut requests = Vec::new();
    for _ in 0..16 {
        let (cache, loads, start) = (cache.clone(), loads.clone(), start.clone());
        requests.push(tokio::spawn(async move {
            start.wait().await;
            cache
                .get_or_load(42, || async {
                    loads.fetch_add(1, Ordering::SeqCst);
                    tokio::task::yield_now().await;
                    items("A")
                })
                .await
        }));
    }
    start.wait().await;
    let mut responses = Vec::new();
    for request in requests {
        responses.push(request.await.unwrap());
    }
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    assert!(
        responses
            .iter()
            .all(|items| Arc::ptr_eq(items, &responses[0]))
    );
}

#[tokio::test]
async fn refresh_during_loading_does_not_restore_the_old_cache() {
    let cache = Arc::new(LibraryAssemblyCache::default());
    let (started, observe_start) = tokio::sync::oneshot::channel();
    let (release, wait_release) = tokio::sync::oneshot::channel();
    let loading_cache = cache.clone();
    let old_request = tokio::spawn(async move {
        loading_cache
            .get_or_load(42, || async {
                started.send(()).unwrap();
                wait_release.await.unwrap();
                items("Old")
            })
            .await
    });
    observe_start.await.unwrap();
    cache.invalidate();
    release.send(()).unwrap();
    assert_eq!(old_request.await.unwrap().items()[0].title, "Old");
    assert!(cache.get(42).is_none());
    let fresh = cache.get_or_load(42, || async { items("New") }).await;
    assert_eq!(fresh.items()[0].title, "New");
    assert!(Arc::ptr_eq(&fresh, &cache.get(42).unwrap()));
}

#[tokio::test]
async fn cancelled_loader_releases_the_gate_for_following_requests() {
    let cache = Arc::new(LibraryAssemblyCache::default());
    let (started, observe_start) = tokio::sync::oneshot::channel();
    let loading_cache = cache.clone();
    let request = tokio::spawn(async move {
        loading_cache
            .get_or_load(42, || async {
                started.send(()).unwrap();
                std::future::pending::<Vec<LibraryItem>>().await
            })
            .await
    });
    observe_start.await.unwrap();
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    let fresh = tokio::time::timeout(
        Duration::from_secs(1),
        cache.get_or_load(42, || async { items("New") }),
    )
    .await
    .unwrap();
    assert_eq!(fresh.items()[0].title, "New");
}

#[tokio::test]
async fn empty_results_are_reused_and_users_do_not_share_data() {
    let cache = LibraryAssemblyCache::default();
    let empty = cache.get_or_load(42, || async { Vec::new() }).await;
    let hit = cache
        .get_or_load(42, || async { panic!("empty result is a hit") })
        .await;
    assert!(Arc::ptr_eq(&empty, &hit));
    let other = cache.get_or_load(7, || async { items("Other") }).await;
    assert_eq!(other.items()[0].title, "Other");
    assert!(cache.get(42).is_none());
}

#[tokio::test]
async fn source_counts_follow_the_snapshot_and_refresh_with_the_items() {
    let cache = LibraryAssemblyCache::default();
    let first = cache.get_or_load(42, || async { items("Steam") }).await;
    assert_eq!(
        json!(first.available_sources()),
        json!(collect_library_source_options(first.items())),
    );
    let disabled = LibrarySourcePreferences {
        categories: HashMap::from([("game".into(), vec![])]),
        ..LibrarySourcePreferences::default()
    };
    assert_eq!(
        paginate_library_items(first.items(), Some(&disabled), None, None, None)
            .unwrap()
            .total,
        0,
    );
    assert_eq!(first.available_sources()["game"][0].count, 1);

    cache.invalidate();
    let fresh = cache
        .get_or_load(42, || async {
            let mut items = items("Bangumi");
            items[0].platform = "bgm".into();
            items.push(items[0].clone());
            items
        })
        .await;
    assert!(!Arc::ptr_eq(&first, &fresh));
    assert_eq!(fresh.available_sources()["game"][0].source, "Bangumi");
    assert_eq!(fresh.available_sources()["game"][0].count, 2);
    assert_eq!(fresh.items().len(), 2);
    assert_eq!(first.available_sources()["game"][0].source, "Steam");
    assert_eq!(first.available_sources()["game"][0].count, 1);
}
