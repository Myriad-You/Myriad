//! Raw-cache enrichment contracts and a repeatable, disk-free profiling workload.
use super::*;

type Enrich = fn(&mut [Value], &Value);

fn fixtures(count: usize, padding_bytes: usize) -> Vec<(Enrich, Value)> {
    let rows = |make: fn(usize, &str) -> Value| -> Vec<Value> {
        let padding = "x".repeat(padding_bytes);
        (0..count).map(|id| make(id, &padding)).collect()
    };
    vec![
        (
            enrich_github_items,
            json!({"user": {"login": "owner"}, "repos": rows(|id, padding| json!({
                "name": format!(" Title {id} "), "html_url": "https://example.com/repo",
                "description": "description", "stargazers_count": 12, "language": "Rust",
                "unused": padding
            }))}),
        ),
        (
            enrich_xbox_items,
            json!({"achievements": {"titles": rows(|id, padding| json!({
                "name": format!(" Title {id} "), "titleId": id + 1,
                "displayImage": "https://example.com/cover.jpg", "unused": padding
            }))}}),
        ),
        (
            enrich_bangumi_items,
            json!({"collections": rows(|id, padding| json!({
                "subject": {"name": format!(" Title {id} "), "name_cn": format!("别名{id}"),
                    "id": id + 1, "images": {"large": "https://example.com/cover.jpg"}},
                "subject_id": id + 1, "rate": 9, "ep_status": 3, "unused": padding
            }))}),
        ),
        (
            enrich_mal_items,
            json!({"anime_list": rows(|id, padding| json!({
                "node": {"title": format!(" Title {id} "), "id": id + 1,
                    "main_picture": {"medium": "https://example.com/cover.jpg"}},
                "list_status": {"score": 8, "num_episodes_watched": 3}, "unused": padding
            }))}),
        ),
        (
            enrich_netease_items,
            json!({"liked_songs": rows(|id, padding| json!({
                "name": format!(" Title {id} "), "id": id + 1,
                "ar": [{"name": "Artist"}],
                "al": {"name": "Album", "picUrl": "https://example.com/cover.jpg"},
                "fee": 1, "unused": padding
            }))}),
        ),
    ]
}

fn items(count: usize) -> Vec<Value> {
    (0..count)
        .map(|id| json!({"title": format!("title {id}"), "id": "", "metadata": {}}))
        .collect()
}

#[test]
fn raw_enrichment_preserves_identity_covers_metadata_and_aliases() {
    for (index, (enrich, raw)) in fixtures(2, 128).into_iter().enumerate() {
        let mut projected = items(2);
        projected.push(json!({"title": "missing", "metadata": {}}));
        if index == 2 {
            projected.push(json!({"title": "别名1", "metadata": {}}));
        }
        enrich(&mut projected, &raw);
        assert_eq!(projected[2], json!({"title": "missing", "metadata": {}}));
        for (id, item) in projected[..2].iter().enumerate() {
            assert_eq!(item["title"], format!("title {id}"));
            assert!(item.get("unused").is_none());
            match index {
                0 => {
                    assert_eq!(item["type"], "repo");
                    assert_eq!(item["url"], "https://example.com/repo");
                    assert_eq!(item["description"], "description");
                    assert_eq!(item["metadata"], json!({"stars": 12, "language": "Rust"}));
                }
                _ => {
                    assert_eq!(item["id"], (id + 1).to_string());
                    assert_eq!(item["image"], "https://example.com/cover.jpg");
                    assert_eq!(item["cover"], item["image"]);
                }
            }
        }
        if index == 2 {
            assert_eq!(projected[3]["id"], "2");
            assert_eq!(projected[0]["metadata"], json!({"rate": 9, "ep_status": 3}));
        }
        if index == 3 {
            assert_eq!(
                projected[0]["metadata"],
                json!({"score": 8, "num_episodes_watched": 3})
            );
        }
        if index == 4 {
            assert_eq!(projected[0]["type"], "music");
            assert_eq!(projected[0]["album"], "Album");
            assert_eq!(
                projected[0]["metadata"],
                json!({
                    "id": "1", "album": "Album", "fee": 1, "isVip": true
                })
            );
        }
    }
}

#[cfg(feature = "hotpath")]
#[test]
#[ignore = "manual profiling workload; run alone with --features hotpath-alloc --nocapture"]
fn profile_raw_enrichment() {
    // Fixture construction and output cloning happen outside measured functions.
    let fixtures = fixtures(1000, 2048);
    let items = items(100);
    let _profile = hotpath::HotpathGuardBuilder::new("raw_enrichment").build();
    for _ in 0..50 {
        for (enrich, raw) in &fixtures {
            let mut projected = items.clone();
            enrich(
                std::hint::black_box(&mut projected),
                std::hint::black_box(raw),
            );
            std::hint::black_box(projected);
        }
    }
}

#[test]
fn netease_artist_matches_last_duplicate_and_title_fallback_keeps_first() {
    let raw = json!({"liked_songs": [
        {"name": " Shared ", "id": 1, "ar": [{"name": "Alpha"}], "fee": 1,
            "al": {"name": "Album A", "picUrl": "http://example.com/a.jpg"}},
        {"name": "shared", "id": "beta", "ar": [{"name": "Beta"}],
            "privilege": {"fee": 0}, "is_vip": true,
            "al": {"name": "Album B", "picUrl": "http://example.com/b.jpg"}},
        {"name": "SHARED", "id": 3, "ar": [{"name": " ALPHA "}], "fee": 4,
            "isVip": false, "al": {"name": "Album C", "picUrl": "http://example.com/c.jpg"}}
    ]});
    let mut projected = vec![
        json!({"title": "SHARED", "artist": "alpha", "id": "netease_123",
            "image": "https://example.com/keep.jpg", "metadata": {}}),
        json!({"name": "shared", "description": " Beta · Existing album", "id": "stable-id",
            "metadata": {"id": "kept", "album": "kept", "fee": 9, "isVip": false}}),
        json!({"title": "shared", "artist": "unknown", "id": "music_123", "metadata": {}}),
        json!({"title": "missing", "metadata": {}}),
    ];
    enrich_netease_items(&mut projected, &raw);
    assert_eq!(projected[0]["id"], "3");
    assert_eq!(projected[0]["image"], "https://example.com/keep.jpg");
    assert_eq!(projected[0]["metadata"]["fee"], 4);
    assert_eq!(projected[0]["metadata"]["isVip"], false);
    assert_eq!(projected[1]["id"], "stable-id");
    assert_eq!(projected[1]["image"], "https://example.com/b.jpg");
    assert_eq!(projected[1]["album"], "Album B");
    assert_eq!(
        projected[1]["metadata"],
        json!({
            "id": "kept", "album": "kept", "fee": 0, "isVip": true
        })
    );
    assert_eq!(projected[2]["id"], "1");
    assert_eq!(projected[2]["image"], "https://example.com/a.jpg");
    assert_eq!(projected[2]["metadata"]["isVip"], true);
    assert_eq!(projected[3], json!({"title": "missing", "metadata": {}}));
}
