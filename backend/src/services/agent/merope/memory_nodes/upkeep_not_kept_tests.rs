//! Her upkeep writes nothing of what they asked her not to keep (as
//! `talk_tests`, on a database of its own).

use super::super::talk_tests::{answering, asked, count, schema, topic};
use super::super::upkeep;
use crate::services::agent::merope::asked::tests::asks;
use crate::services::channel_group::g0;

const ANSWER: &str = r#"{"actions":[
    {"do":"summary","sentences":[{"text":"和房东吵架了","because":["e1"]}]},
    {"do":"open","text":"周末要去爬山","because":["e2"]}]}"#;

#[tokio::test]
async fn her_upkeep_writes_nothing_they_asked_her_not_to_keep() {
    let Some(schema) = schema("memory_nodes_upkeep_asked").await else {
        return;
    };
    let db = &schema.db;
    topic(db, "住处", &[("m1", "和房东吵架"), ("m2", "周末去爬山")]).await;
    asks(db, super::super::talk_tests::OTHER, "不要记和房东吵架的事").await;
    let of =
        |kind: &str| format!("SELECT COUNT(*) AS n FROM agent_node_rows WHERE kind = '{kind}'");

    // Not told: nothing is written, and the node is still due.
    answering(&[("memory_node_upkeep", ANSWER)]);
    upkeep::tick(db).await;
    assert_eq!(asked("memory_node_upkeep"), 1);
    for kind in ["node.revised", "node.open_item", "node.upkept"] {
        assert_eq!(count(db, &of(kind)).await, 0, "{kind}");
    }

    // Nor with one refused, and then nothing answered: the other is not
    // written without it, and the node is still due.
    let refused = ("not_to_keep", "!refused");
    let told = [
        ("memory_node_upkeep", ANSWER),
        refused,
        refused,
        ("not_to_keep", r#"{"drop":[]}"#),
    ];
    for again in [&[refused][..], &[]] {
        answering(&[&told[..], again].concat());
        upkeep::tick(db).await;
        assert_eq!(asked("memory_node_upkeep"), 1);
        for kind in ["node.revised", "node.open_item", "node.upkept"] {
            assert_eq!(count(db, &of(kind)).await, 0, "{kind}");
        }
    }

    answering(&[
        ("memory_node_upkeep", ANSWER),
        ("not_to_keep", r#"{"drop":[0]}"#),
    ]);
    upkeep::tick(db).await;
    let shown = g0::with(|world| {
        (world.log.iter())
            .find(|entry| entry["what"]["operation"] == "not_to_keep")
            .map(|entry| entry["what"]["input"].as_str().unwrap().to_string())
    })
    .flatten();
    assert!(shown.is_some_and(|shown| shown.contains("房东") && shown.contains("爬山")));
    assert_eq!(asked("memory_node_upkeep"), 1);
    assert_eq!(count(db, &of("node.revised")).await, 0);
    assert_eq!(count(db, &of("node.open_item")).await, 1);
    assert_eq!(count(db, &of("node.upkept")).await, 1);
    schema.drop().await;
}
