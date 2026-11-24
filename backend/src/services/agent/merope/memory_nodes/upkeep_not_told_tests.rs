//! An upkeep not told of by what they asked her not to keep (as
//! `talk_tests`, on a database of its own).

use serde_json::json;

use super::super::talk_tests::{OTHER, answering, asked, count, schema, talk, topic};
use super::super::{Author, RowIn, known, upkeep, write};
use super::*;
use crate::services::agent::merope::asked::tests::asks;
use crate::services::her_log::pen;

const ANSWER: &str = r#"{"actions":[
    {"do":"summary","sentences":[{"text":"和房东吵架了","because":["e1"]}]},
    {"do":"open","text":"周末要去爬山","because":["e2"]}]}"#;
const UPKEEP: (&str, &str) = ("memory_node_upkeep", ANSWER);
/// Both together, then each.
const REFUSED: [(&str, &str); 3] = [("not_to_keep", "!refused"); 3];

async fn noted(db: &DatabaseConnection, node: &str) -> Option<NotTold> {
    registry::get(db, NAMESPACE, &key(OTHER, &talk(), node))
        .await
        .unwrap()
}

/// What she wrote of them: versions, and marks given up on.
async fn wrote(db: &DatabaseConnection) -> (i64, i64) {
    let of = async |what: &str| {
        count(
            db,
            &format!("SELECT COUNT(*) AS n FROM agent_node_rows WHERE {what}"),
        )
        .await
    };
    (
        of("kind IN ('node.revised', 'node.open_item')").await,
        of("kind = 'node.upkept' AND body LIKE '%not_told%'").await,
    )
}

/// Something more on `node`.
async fn more(db: &DatabaseConnection, node: &str, memory: &str, text: &str) {
    let known = known::known(db, OTHER, &talk()).await.unwrap();
    let known = known.into_iter().find(|known| known.id == node).unwrap();
    let made = myriad_nodes::entries::fact_rows(
        serde_json::from_value(json!({
            "audience": talk(),
            "key": known.key.unwrap(),
            "facts": [{"memory": memory, "text": text, "saidBy": ["partner"]}],
        }))
        .unwrap(),
    )
    .unwrap();
    let rows: Vec<RowIn> = made
        .rows
        .into_iter()
        .map(|row| RowIn {
            id: row.id,
            node: made.node.clone(),
            kind: row.kind,
            body: row.body,
            author: Author::Her,
        })
        .collect();
    let thought = pen::think().unwrap();
    write::write(db, OTHER, &talk(), &rows, Some(&thought))
        .await
        .unwrap();
}

/// Not to be asked about, one node of the talk is paid for a night, not
/// every one due.
#[tokio::test]
async fn not_to_be_reached_a_talk_is_gone_over_no_more_that_night() {
    let Some(schema) = schema("memory_nodes_upkeep_unreached").await else {
        return;
    };
    let db = &schema.db;
    for name in ["住处", "周末"] {
        let (first, second) = (format!("{name}一"), format!("{name}二"));
        let facts = [(first.as_str(), "和房东吵架"), (second.as_str(), "去爬山")];
        topic(db, name, &facts).await;
    }
    asks(db, OTHER, "不要记和房东吵架的事").await;
    for away in ["!rejected", "!quota"] {
        answering(&[UPKEEP, ("not_to_keep", away), UPKEEP]);
        upkeep::tick(db).await;
        assert_eq!(asked("memory_node_upkeep"), 1, "{away}");
        assert_eq!(wrote(db).await, (0, 0));
    }
    // For what it says, the next is gone over all the same.
    let told = ("not_to_keep", r#"{"drop":[0]}"#);
    answering(&[&[UPKEEP][..], &REFUSED, &[UPKEEP, told]].concat());
    upkeep::tick(db).await;
    assert_eq!(asked("memory_node_upkeep"), 2);
    assert_eq!(wrote(db).await, (1, 0));
    schema.drop().await;
}

/// Not told of for what it says: paid for each night till it is given up on
/// as anything never told is, then marked, with nothing written, and not
/// paid for again till there is something new on it.
#[tokio::test]
async fn never_told_of_a_node_waits_for_something_new() {
    let Some(schema) = schema("memory_nodes_upkeep_not_told").await else {
        return;
    };
    let db = &schema.db;
    let node = topic(db, "住处", &[("m1", "和房东吵架"), ("m2", "周末去爬山")]).await;
    asks(db, OTHER, "不要记和房东吵架的事").await;
    let now = crate::clock::now().timestamp();
    let tick = async |paid: usize| {
        answering(&[&[UPKEEP][..], &REFUSED].concat());
        upkeep::tick(db).await;
        assert_eq!(asked("memory_node_upkeep"), paid);
    };
    let since = async |since: i64| {
        let not_told = NotTold {
            since,
            ..noted(db, &node).await.expect("noted")
        };
        keep(db, OTHER, &key(OTHER, &talk(), &node), &not_told, now).await;
    };
    tick(1).await;
    assert!(noted(db, &node).await.unwrap().since >= now);
    // Not for three days yet; nor with no other judgment answered since.
    call::judged_last(now);
    since(now - GIVES_UP_AFTER_SECS + 60).await;
    tick(1).await;
    call::judged_last(0);
    since(now - GIVES_UP_AFTER_SECS - 60).await;
    tick(1).await;
    assert_eq!(wrote(db).await, (0, 0));
    let kept = noted(db, &node).await.expect("still noted");
    assert_eq!(
        kept.since,
        now - GIVES_UP_AFTER_SECS - 60,
        "since the first"
    );
    call::judged_last(now);
    tick(1).await;
    assert_eq!(wrote(db).await, (0, 1), "marked, and nothing written");
    assert_eq!(noted(db, &node).await, None);
    tick(0).await;
    // Something new on it: gone over again, counted anew, and, told of,
    // written as ever.
    more(db, &node, "m3", "换了新锁").await;
    tick(1).await;
    assert!(noted(db, &node).await.unwrap().since >= now);
    answering(&[UPKEEP, ("not_to_keep", r#"{"drop":[0]}"#)]);
    upkeep::tick(db).await;
    assert_eq!(asked("memory_node_upkeep"), 1);
    assert_eq!(wrote(db).await, (1, 1));
    assert_eq!(noted(db, &node).await, None);
    schema.drop().await;
}
