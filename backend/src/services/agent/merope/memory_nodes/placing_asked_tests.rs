//! What they asked her not to keep is put on no node of hers: the whole of a
//! line with any of it stays off, and no line goes on while that is not told
//! (as `talk_tests`, on a database of its own).

use super::super::inbox;
use super::super::talk_tests::{OTHER, answering, asked, line, schema, talk};
use super::{Duration, place, place_within};
use crate::services::agent::merope::asked::tests::asks;
use crate::services::channel_group::g0;

const PLACED: &str = r#"{"spans":[{"start":1,"node":null,"gist":"周末去爬山"}]}"#;

#[tokio::test]
async fn a_line_of_what_they_asked_not_to_keep_goes_on_no_node() {
    let Some(schema) = schema("memory_nodes_asked").await else {
        return;
    };
    let db = &schema.db;
    asks(db, OTHER, "不要记和房东吵架的事").await;
    let lines = [
        line(1, true, "跟房东又闹僵了，对了周末去爬山", 0),
        line(2, true, "周末去爬山", 1),
    ];
    // Not told: nothing placed, and nothing asked of placing.
    answering(&[]);
    place(db, OTHER, None, &lines).await;
    assert_eq!(asked("memory_node_place"), 0);

    answering(&[
        ("not_to_keep", r#"{"drop":[0]}"#),
        ("memory_node_place", PLACED),
    ]);
    place(db, OTHER, None, &lines).await;
    let shown = g0::with(|world| {
        world
            .log
            .iter()
            .filter(|entry| entry["what"]["operation"] == "memory_node_place")
            .map(|entry| entry["what"]["input"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    })
    .unwrap();
    g0::leave();
    assert_eq!(shown.len(), 1);
    assert!(!shown[0].contains("房东"));
    let waiting = inbox::waiting(db, OTHER, &talk(), 10).await.unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].refs, ["message:2"]);

    // All there is left to place, said twice to be: placing is not asked.
    let all = r#"{"drop":[0]}"#;
    answering(&[("not_to_keep", all), ("not_to_keep", all)]);
    place(db, OTHER, None, &lines).await;
    assert_eq!(asked("memory_node_place"), 0);
    schema.drop().await;
}

/// One refused, and then nothing answered: none is placed, and all of it
/// waits to be placed later, that one too.
#[tokio::test]
async fn a_line_never_told_of_waits_with_those_beside_it() {
    let Some(schema) = schema("memory_nodes_asked_waits").await else {
        return;
    };
    let db = &schema.db;
    asks(db, OTHER, "不要记和房东吵架的事").await;
    let lines = [
        line(1, true, "周末去爬山", 0),
        line(2, true, "山上风很大", 1),
    ];
    let none = r#"{"drop":[]}"#;
    let refused = ("not_to_keep", "!refused");
    let told = [refused, refused, ("not_to_keep", none)];
    for again in [&[refused][..], &[]] {
        let answers = [&told[..], again, &[("memory_node_place", PLACED)]].concat();
        answering(&answers);
        assert!(place(db, OTHER, None, &lines).await.is_empty());
        assert_eq!(asked("memory_node_place"), 0);
        let later: Option<serde_json::Value> = crate::services::runtime_registry::get(
            db,
            super::backlog::NAMESPACE,
            &OTHER.to_string(),
        )
        .await
        .unwrap();
        assert!(later.is_some());
    }
    // Told the next time: both are there to be placed.
    answering(&[("not_to_keep", none), ("memory_node_place", PLACED)]);
    place(db, OTHER, None, &lines).await;
    let shown = g0::with(|world| {
        (world.log.iter())
            .find(|entry| entry["what"]["operation"] == "memory_node_place")
            .map(|entry| entry["what"]["input"].as_str().unwrap().to_string())
    })
    .flatten();
    g0::leave();
    assert!(shown.is_some_and(|shown| lines.iter().all(|line| shown.contains(&line.text))));
    schema.drop().await;
}

/// However slow the telling, it ends in the time there is: nothing is
/// placed, and all of it waits to be placed later.
#[tokio::test]
async fn a_slow_telling_is_left_for_later() {
    let Some(schema) = schema("memory_nodes_asked_slow").await else {
        return;
    };
    let db = &schema.db;
    asks(db, OTHER, "不要记和房东吵架的事").await;
    let lines = [
        line(1, true, "跟房东又闹僵了", 0),
        line(2, true, "周末去爬山", 1),
    ];
    answering(&[
        ("not_to_keep", r#"{"drop":[0]}"#),
        ("memory_node_place", PLACED),
    ]);
    g0::with(|world| {
        world
            .delays
            .entry("not_to_keep".into())
            .or_default()
            .push_back(20.0);
    });
    let began = std::time::Instant::now();
    let placed = place_within(db, OTHER, None, &lines, Duration::from_secs(1)).await;
    assert!(began.elapsed() < Duration::from_secs(8));
    assert!(placed.is_empty());
    assert_eq!(asked("memory_node_place"), 0);
    g0::leave();
    let later: Option<serde_json::Value> =
        crate::services::runtime_registry::get(db, super::backlog::NAMESPACE, &OTHER.to_string())
            .await
            .unwrap();
    assert!(later.is_some());
    schema.drop().await;
}
