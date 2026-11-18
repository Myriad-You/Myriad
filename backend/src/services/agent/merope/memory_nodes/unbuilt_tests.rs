//! A node with something on it and no 「现在」 of hers, built at upkeep
//! (`unbuilt`), on a database of its own, as `talk_tests`.

use sea_orm::ConnectionTrait;

use super::talk_tests::{OTHER, answering, asked, count, schema, talk, topic};
use super::{Author, RowIn, forget, read, standing, unbuilt, upkeep, write};
use crate::services::channel_group::g0;
use crate::services::her_log::pen;

/// Memories account 7 holds, by id.
async fn held(db: &sea_orm::DatabaseConnection, memories: &[&str]) {
    for id in memories {
        db.execute_unprepared(&format!(
            "INSERT INTO agent_memories (id, user_id, kind, content, source, venue, valid_from, \
             created_at, updated_at) VALUES ('{id}', {OTHER}, 'fact', '{id}', 'chat', 'private', \
             now(), now() - interval '3 days', now())"
        ))
        .await
        .unwrap();
    }
}

async fn hers(db: &sea_orm::DatabaseConnection) -> i64 {
    count(
        db,
        "SELECT count(*) AS n FROM agent_node_rows WHERE kind = 'node.standing' \
         AND author = 'her' AND body <> ''",
    )
    .await
}

async fn kept_empty(db: &sea_orm::DatabaseConnection) -> i64 {
    count(
        db,
        &format!(
            "SELECT count(*) AS n FROM runtime_registry WHERE namespace = '{}'",
            unbuilt::NAMESPACE
        ),
    )
    .await
}

/// One more fact on `node`, `memory` saying `text`.
async fn one_more(db: &sea_orm::DatabaseConnection, node: &str, memory: &str, text: &str) {
    let key = read::heads(db, OTHER, &talk(), None)
        .await
        .unwrap()
        .into_iter()
        .find(|head| head.node_id == node)
        .and_then(|head| head.node_key)
        .unwrap();
    let made = myriad_nodes::entries::fact_rows(
        serde_json::from_value(serde_json::json!({
            "audience": talk(),
            "key": key,
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

/// The model calls the world was asked, by operation: builds, and upkeep.
fn calls() -> (usize, usize) {
    let world = g0::leave().expect("a world");
    let of = |operation: &str| {
        world
            .log
            .iter()
            .filter(|line| line["kind"] == "model" && line["what"]["operation"] == operation)
            .count()
    };
    (of("memory_standing_build"), of("memory_node_upkeep"))
}

/// A node kept before 「现在」 was written is built at the next upkeep, and
/// not asked again once something of hers stands there.
#[tokio::test]
async fn a_node_kept_before_now_is_built_at_upkeep_once() {
    let Some(schema) = schema("memory_nodes_unbuilt").await else {
        return;
    };
    let db = &schema.db;
    held(db, &["m1"]).await;
    topic(db, "夜跑", &[("m1", "每周三晚上夜跑")]).await;
    answering(&[(
        "memory_standing_build",
        r#"{"items":[{"whose":"them","text":"每周三晚上夜跑","because":[1]}]}"#,
    )]);
    upkeep::tick(db).await;
    // Read in the world's day, before it is left.
    let spent = upkeep::calls_today(db, OTHER, &talk()).await.unwrap() as usize;
    let (built, kept_up) = calls();
    assert_eq!(built, 1);
    assert_eq!(hers(db).await, 1);
    assert_eq!(kept_empty(db).await, 0);
    // Not the site owner's talk: the build is one of the day's calls.
    assert_eq!(spent, built + kept_up);
    answering(&[]);
    upkeep::tick(db).await;
    assert_eq!(asked("memory_standing_build"), 0, "built: not asked again");
    schema.drop().await;
}

/// A node built to nothing is asked again only once something on it
/// changes; forgotten, what was kept of it goes.
#[tokio::test]
async fn a_node_built_to_nothing_waits_for_something_new() {
    let Some(schema) = schema("memory_nodes_unbuilt_empty").await else {
        return;
    };
    let db = &schema.db;
    held(db, &["m1"]).await;
    let node = topic(db, "体检", &[("m1", "上周做了体检")]).await;
    answering(&[("memory_standing_build", r#"{"items":[]}"#)]);
    upkeep::tick(db).await;
    assert_eq!(asked("memory_standing_build"), 1);
    assert_eq!((hers(db).await, kept_empty(db).await), (0, 1));
    answering(&[]);
    upkeep::tick(db).await;
    assert_eq!(asked("memory_standing_build"), 0, "nothing new on it");
    held(db, &["m2"]).await;
    one_more(db, &node, "m2", "体检说要复查").await;
    answering(&[("memory_standing_build", r#"{"items":[]}"#)]);
    upkeep::tick(db).await;
    assert_eq!(
        asked("memory_standing_build"),
        1,
        "asked again once it changed"
    );
    assert_eq!(kept_empty(db).await, 1);
    forget::forget_nodes(db, OTHER, &talk(), &[node].into_iter().collect())
        .await
        .unwrap();
    assert_eq!(kept_empty(db).await, 0, "goes with the node");
    schema.drop().await;
}

/// A node's first building, read again under the lock: one of hers that
/// stands there by then (another build got there first) keeps it out.
#[tokio::test]
async fn a_build_that_comes_second_writes_nothing() {
    let Some(schema) = schema("memory_nodes_unbuilt_second").await else {
        return;
    };
    let db = &schema.db;
    held(db, &["m1"]).await;
    let node = topic(db, "夜跑", &[("m1", "每周三晚上夜跑")]).await;
    // Written while the build was asking: in the rows, not yet in the heads
    // it read.
    db.execute_unprepared(&format!(
        "INSERT INTO agent_node_rows (user_id, audience, node_id, row_id, kind, body, author, at, \
         hlc, origin, envelope) VALUES ({OTHER}, '{}', '{node}', 'first', 'node.standing', \
         '{{\"text\":\"每周三晚上夜跑\"}}', 'her', now(), '0', 'site', '{{}}')",
        talk()
    ))
    .await
    .unwrap();
    answering(&[(
        "memory_standing_build",
        r#"{"items":[{"whose":"them","text":"每周三晚上夜跑","because":[1]}]}"#,
    )]);
    let pen = pen::think().unwrap();
    let written = standing::build(db, OTHER, OTHER, &talk(), &[], &pen)
        .await
        .unwrap();
    assert_eq!(asked("memory_standing_build"), 1);
    assert_eq!(written, 0);
    assert_eq!(hers(db).await, 1, "only the first");
    schema.drop().await;
}

/// What is kept of a node built to nothing changes with the entries it
/// holds, those a merge brought in too, not with their order.
#[test]
fn what_is_kept_changes_with_the_entries_held() {
    let held = |rows: &[&str]| {
        unbuilt::seen(&serde_json::json!({
            "entries": rows.iter().map(|row| serde_json::json!({"row": row})).collect::<Vec<_>>(),
        }))
    };
    assert_eq!(held(&["a", "b"]), held(&["b", "a"]));
    assert_ne!(held(&["a", "b"]), held(&["a", "b", "merged"]));
    assert_ne!(held(&["a", "b"]), held(&[]));
}
