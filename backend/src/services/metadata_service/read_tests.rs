use super::*;

fn fixture() -> Vec<(String, Value)> {
    vec![
        ("netease_chunk_2".into(), json!({"songs": [3]})),
        (
            "netease".into(),
            json!({"_chunked": true, "liked_songs": [1]}),
        ),
        ("netease".into(), json!({"liked_songs": [99]})),
        ("netease_chunk_1".into(), json!({"songs": [2]})),
        ("netease_chunk_1".into(), json!({"songs": [99]})),
        ("netease_chunk_invalid".into(), json!({"songs": [99]})),
        ("orphan_chunk_1".into(), json!({"songs": [99]})),
        ("github".into(), json!({"repos": ["new"]})),
        ("github".into(), json!({"repos": ["old"]})),
        ("unmerged".into(), json!({"liked_songs": [0]})),
        ("unmerged_chunk_1".into(), json!({"songs": [99]})),
    ]
}

#[test]
fn metadata_merge_keeps_latest_main_and_chunks_and_ignores_orphans() {
    let merged = merge_latest_metadata(fixture());
    assert_eq!(merged.len(), 3);
    assert_eq!(merged["netease"]["liked_songs"], json!([1, 2, 3]));
    assert_eq!(merged["github"]["repos"], json!(["new"]));
    assert_eq!(merged["unmerged"]["liked_songs"], json!([0]));
}

#[test]
fn metadata_merge_moves_main_and_chunk_string_allocations() {
    let main_text = "main".repeat(16384);
    let chunk_text = "chunk".repeat(16384);
    let (main_ptr, chunk_ptr) = (main_text.as_ptr(), chunk_text.as_ptr());
    let mut main = json!({"_chunked": true, "liked_songs": []});
    main["liked_songs"] = Value::Array(vec![Value::String(main_text)]);
    let mut chunk = json!({"songs": []});
    chunk["songs"] = Value::Array(vec![Value::String(chunk_text)]);
    let merged = merge_latest_metadata(vec![
        ("netease".into(), main),
        ("netease_chunk_1".into(), chunk),
    ]);
    let songs = merged["netease"]["liked_songs"].as_array().unwrap();
    assert_eq!(songs[0].as_str().unwrap().as_ptr(), main_ptr);
    assert_eq!(songs[1].as_str().unwrap().as_ptr(), chunk_ptr);
}

// Previous full-read grouping, retained only for the manual before/after workload.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn cloned_metadata(rows: Vec<(String, Value)>) -> HashMap<String, Value> {
    let mut result = HashMap::new();
    let mut chunks: HashMap<String, HashMap<i32, Value>> = HashMap::new();
    for (name, raw) in &rows {
        if name.contains("_chunk_") {
            let mut parts = name.split("_chunk_");
            let base = parts.next().unwrap_or_default();
            if let Some(index) = parts.next().and_then(|s| s.parse::<i32>().ok()) {
                chunks
                    .entry(base.into())
                    .or_default()
                    .entry(index)
                    .or_insert_with(|| raw.clone());
            }
        } else {
            result.entry(name.clone()).or_insert_with(|| raw.clone());
        }
    }
    for (name, chunks) in chunks {
        if let Some(main) = result.get_mut(&name) {
            merge_chunk_rows(&name, main, chunks);
        }
    }
    result
}

#[test]
#[ignore = "manual metadata merge timing/allocation workload"]
fn profile_metadata_merge() {
    let mut rows = vec![(
        "netease".into(),
        json!({"_chunked": true, "liked_songs": []}),
    )];
    for chunk in 0..10 {
        let songs: Vec<Value> = (0..100)
            .map(|id| json!({"id": id, "unused": "x".repeat(2048)}))
            .collect();
        rows.push((format!("netease_chunk_{chunk}"), json!({"songs": songs})));
    }
    assert_eq!(
        cloned_metadata(rows.clone()),
        merge_latest_metadata(rows.clone())
    );
    for (variant, merge) in [
        (
            "cloned",
            cloned_metadata as fn(Vec<(String, Value)>) -> HashMap<String, Value>,
        ),
        ("moved", merge_latest_metadata),
    ] {
        let mut samples = Vec::new();
        for _ in 0..30 {
            let input = rows.clone();
            let started = std::time::Instant::now();
            let output = merge(input);
            samples.push(started.elapsed().as_nanos());
            std::hint::black_box(output);
        }
        samples.sort_unstable();
        println!(
            "metadata_merge: variant={variant} median_ns={} calls=30 songs=1000 unused_bytes_per_song=2048",
            samples[15]
        );
    }
}
