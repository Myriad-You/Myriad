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

/// `node`, noted, as not told since and last for what it says.
async fn untold(db: &DatabaseConnection, node: &str, since: i64, last: i64) {
    let not_told = NotTold {
        untold: Some((since, last)),
        ..noted(db, node).await.expect("noted")
    };
    let now = crate::clock::now().timestamp();
    keep(db, OTHER, &key(OTHER, &talk(), node), &not_told, now).await;
}

/// Two nodes of a talk, each with what was asked not to be kept and not.
async fn two(db: &DatabaseConnection) -> [String; 2] {
    asks(db, OTHER, "不要记和房东吵架的事").await;
    let one = async |name: &str| {
        let (first, second) = (format!("{name}一"), format!("{name}二"));
        let facts = [(first.as_str(), "和房东吵架"), (second.as_str(), "去爬山")];
        topic(db, name, &facts).await
    };
    [one("住处").await, one("周末").await]
}

/// The one of `nodes` that is noted.
async fn the_noted(db: &DatabaseConnection, nodes: &[String; 2]) -> usize {
    let [first, second] = nodes;
    match (noted(db, first).await, noted(db, second).await) {
        (Some(_), None) => 0,
        (None, Some(_)) => 1,
        both => panic!("not one of them: {both:?}"),
    }
}

/// Not to be asked about, one node of the talk is paid for a night, not
/// every one due; the next night the other is first.
#[tokio::test]
async fn not_to_be_reached_a_talk_is_gone_over_no_more_that_night() {
    for (away, name) in [
        ("!rejected", "memory_nodes_upkeep_rejected"),
        ("!quota", "memory_nodes_upkeep_unreached"),
        ("", "memory_nodes_upkeep_no_answer"),
    ] {
        let Some(schema) = schema(name).await else {
            return;
        };
        let db = &schema.db;
        let nodes = two(db).await;
        let away: &[(&str, &str)] = match away {
            "" => &[],
            away => &[("not_to_keep", away)],
        };
        answering(&[&[UPKEEP][..], away].concat());
        upkeep::tick(db).await;
        assert_eq!(asked("memory_node_upkeep"), 1, "{away:?}");
        assert_eq!(wrote(db).await, (0, 0));
        let held = the_noted(db, &nodes).await;
        assert_eq!(noted(db, &nodes[held]).await.unwrap().untold, None);
        // Stably so of the one, the other is not held up by it.
        let told = ("not_to_keep", r#"{"drop":[0]}"#);
        answering(&[&[UPKEEP, told, UPKEEP][..], away].concat());
        upkeep::tick(db).await;
        assert_eq!(asked("memory_node_upkeep"), 2, "{away:?}");
        assert_eq!(wrote(db).await, (1, 0), "{away:?}");
        assert_eq!(the_noted(db, &nodes).await, held, "not skipped");
        schema.drop().await;
    }
}

/// Not told for what it says, the next of the talk is gone over all the same.
#[tokio::test]
async fn for_what_it_says_the_next_is_gone_over() {
    let Some(schema) = schema("memory_nodes_upkeep_says").await else {
        return;
    };
    let db = &schema.db;
    let nodes = two(db).await;
    let told = ("not_to_keep", r#"{"drop":[0]}"#);
    answering(&[&[UPKEEP][..], &REFUSED, &[UPKEEP, told]].concat());
    upkeep::tick(db).await;
    assert_eq!(asked("memory_node_upkeep"), 2);
    assert_eq!(wrote(db).await, (1, 0));
    the_noted(db, &nodes).await;
    schema.drop().await;
}

/// One of them refused and the other not to be asked about: it counts, but
/// it is not given up on, for the other may be told, and the talk is gone
/// over no more that night.
#[tokio::test]
async fn refused_and_then_not_reached_is_not_given_up_on() {
    let Some(schema) = schema("memory_nodes_upkeep_both").await else {
        return;
    };
    let db = &schema.db;
    let nodes = two(db).await;
    let [both, each] = [("not_to_keep", "!refused"); 2];
    let away = ("not_to_keep", "!quota");
    answering(&[UPKEEP, both, each, away]);
    upkeep::tick(db).await;
    assert_eq!(asked("memory_node_upkeep"), 1);
    let held = the_noted(db, &nodes).await;
    let now = crate::clock::now().timestamp();
    let since = now - GIVES_UP_AFTER_SECS - 60;
    let counted = noted(db, &nodes[held]).await.unwrap().untold;
    assert!(
        counted.is_some_and(|(since, _)| since >= now - 60),
        "it counts"
    );
    untold(db, &nodes[held], since, now).await;
    call::judged_last(now);
    let told = ("not_to_keep", r#"{"drop":[0]}"#);
    answering(&[UPKEEP, told, UPKEEP, both, each, away]);
    upkeep::tick(db).await;
    assert_eq!(asked("memory_node_upkeep"), 2);
    assert_eq!(wrote(db).await, (1, 0), "not marked");
    assert_eq!(the_noted(db, &nodes).await, held);
    let counted = noted(db, &nodes[held]).await.unwrap().untold;
    assert_eq!(counted.map(|(since, _)| since), Some(since));
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
    let since = async |since: i64| untold(db, &node, since, now).await;
    let counted = async || noted(db, &node).await.expect("noted").untold.unwrap();
    tick(1).await;
    assert!(counted().await.0 >= now);
    // Not for three days yet; nor with no other judgment answered since.
    call::judged_last(now);
    since(now - GIVES_UP_AFTER_SECS + 60).await;
    tick(1).await;
    call::judged_last(0);
    since(now - GIVES_UP_AFTER_SECS - 60).await;
    tick(1).await;
    assert_eq!(wrote(db).await, (0, 0));
    let first = now - GIVES_UP_AFTER_SECS - 60;
    assert_eq!(counted().await.0, first, "since the first");
    call::judged_last(now);
    tick(1).await;
    assert_eq!(wrote(db).await, (0, 1), "marked, and nothing written");
    assert_eq!(noted(db, &node).await, None);
    tick(0).await;
    // Something new on it: gone over again, counted anew, and, told of,
    // written as ever.
    more(db, &node, "m3", "换了新锁").await;
    tick(1).await;
    assert!(counted().await.0 >= now);
    answering(&[UPKEEP, ("not_to_keep", r#"{"drop":[0]}"#)]);
    upkeep::tick(db).await;
    assert_eq!(asked("memory_node_upkeep"), 1);
    assert_eq!(wrote(db).await, (1, 1));
    assert_eq!(noted(db, &node).await, None);
    schema.drop().await;
}

/// Not told of two days and more ago, it is as if for the first time.
#[tokio::test]
async fn not_told_of_long_ago_is_counted_anew() {
    let Some(schema) = schema("memory_nodes_upkeep_lapsed").await else {
        return;
    };
    let db = &schema.db;
    let node = topic(db, "住处", &[("m1", "和房东吵架"), ("m2", "周末去爬山")]).await;
    asks(db, OTHER, "不要记和房东吵架的事").await;
    let now = crate::clock::now().timestamp();
    call::judged_last(now);
    for last in [now - LAPSES - 60, now - LAPSES + 60] {
        answering(&[&[UPKEEP][..], &REFUSED].concat());
        upkeep::tick(db).await;
        assert_eq!(asked("memory_node_upkeep"), 1);
        assert_eq!(wrote(db).await, (0, 0), "not marked");
        assert!(noted(db, &node).await.unwrap().untold.unwrap().0 >= now);
        untold(db, &node, now - GIVES_UP_AFTER_SECS - 60, last).await;
    }
    answering(&[&[UPKEEP][..], &REFUSED].concat());
    upkeep::tick(db).await;
    assert_eq!(wrote(db).await, (0, 1), "within them it was as before");
    schema.drop().await;
}
