//! What was seen to still hold (`agent_node_kept`), on a database of its
//! own: a fact that leaves an item standing notes it, the latest three per
//! item; setting or ending the item, forgetting the memory or its row,
//! taking it back (once confirmed) lets them go, and they move with its talk.

use sea_orm::ConnectionTrait;

use super::facts::GoesUnder;
use super::talk_tests::{OTHER, answering, asked, count, memory_id, schema, settle, talk, update};
use super::{forget, kept};

fn running() -> Option<GoesUnder> {
    Some(GoesUnder::New {
        name: "跑步".into(),
        aliases: Vec::new(),
    })
}

/// What a kept fact does on the topic, as the model would answer.
async fn stands(db: &sea_orm::DatabaseConnection, step: &str, fact: &str) -> String {
    answering(&[("memory_standing_step", step)]);
    settle(db, vec![update(fact, &[], running())]).await;
    assert_eq!(asked("memory_standing_step"), 1);
    memory_id(db, fact).await
}

const LEFT: &str = r#"{"whose":"them","set":[],"end":[],"add":[]}"#;

async fn noted(db: &sea_orm::DatabaseConnection) -> Vec<String> {
    let node = db
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT node_id FROM agent_node_heads WHERE name = '跑步'",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "node_id")
        .unwrap();
    kept::of_node(db, OTHER, &talk(), &node)
        .await
        .unwrap()
        .into_iter()
        .map(|(item, memory, _)| {
            assert!(item.starts_with("them-"), "{item}");
            memory
        })
        .collect()
}

#[tokio::test]
async fn what_a_fact_leaves_standing_is_noted_until_it_changes() {
    let Some(schema) = schema("memory_nodes_kept").await else {
        return;
    };
    let db = &schema.db;
    stands(
        db,
        r#"{"whose":"them","set":[],"end":[],"add":[{"topic":"跑步","text":"每周三晚上夜跑"}]}"#,
        "每周三晚上夜跑",
    )
    .await;
    // Started by a fact: nothing seen to hold yet.
    assert!(noted(db).await.is_empty());

    // Each fact after it that leaves it standing is noted, the latest
    // three by when it was learned.
    let mut seen = Vec::new();
    for fact in [
        "今天跑完膝盖有点酸",
        "跑步时听播客",
        "新买了跑鞋",
        "跑完拉伸了很久",
    ] {
        seen.push(stands(db, LEFT, fact).await);
        let mut latest: Vec<String> = seen.iter().rev().take(3).cloned().collect();
        latest.sort();
        let mut now = noted(db).await;
        now.sort();
        assert_eq!(now, latest, "{fact}");
    }

    // Set: what was seen of its old words goes.
    stands(
        db,
        r#"{"whose":"them","set":[{"item":1,"text":"每周五晚上夜跑"}],"end":[],"add":[]}"#,
        "夜跑改到周五了",
    )
    .await;
    assert!(noted(db).await.is_empty());

    // Forgotten here: goes with the memory.
    let forgotten = stands(db, LEFT, "今天又去跑了").await;
    assert_eq!(noted(db).await, [forgotten.clone()]);
    forget::forget(db, std::slice::from_ref(&forgotten), &[])
        .await
        .unwrap();
    assert!(noted(db).await.is_empty());

    // Taken back, cited by nothing but what was seen: only hidden until
    // the second look agrees (`take_back_check`), then the sweep finds it
    // there.
    let taken = stands(db, LEFT, "跑完喝了杯冰美式").await;
    assert_eq!(noted(db).await, [taken.clone()]);
    db.execute_unprepared(&format!(
        "UPDATE agent_memories SET invalid_reason = 'superseded', invalid_at = now(), \
         superseded_by = NULL, updated_at = now() WHERE id = '{taken}'"
    ))
    .await
    .unwrap();
    forget::taken_back_here(db, OTHER, &talk()).await.unwrap();
    assert_eq!(noted(db).await, [taken.clone()]);
    db.execute_unprepared(&format!(
        "INSERT INTO agent_takeback_check (user_id, memory_id, state, at) \
         VALUES ({OTHER}, '{taken}', 'confirmed', now())"
    ))
    .await
    .unwrap();
    forget::taken_back_here(db, OTHER, &talk()).await.unwrap();
    assert!(noted(db).await.is_empty());
    assert_eq!(
        count(db, "SELECT count(*) AS n FROM agent_node_kept").await,
        0
    );
    schema.drop().await;
}

/// What was noted stays with its person, audience, node and item: the
/// latest three of each, let go only where its item changes, its node or
/// memory goes, or its talk moves; a memory no longer held notes nothing.
#[tokio::test]
async fn what_was_noted_stays_with_its_person_audience_and_item() {
    let Some(schema) = schema("memory_nodes_kept_apart").await else {
        return;
    };
    let db = &schema.db;
    // Learned a day apart, in order: m1 first.
    held(
        db,
        &[
            ("m1", OTHER),
            ("m2", OTHER),
            ("m3", OTHER),
            ("m4", OTHER),
            ("m5", OTHER),
            ("n1", 1),
        ],
    )
    .await;
    let item = "them-0123456789";
    // The item stands on node n where it is noted: theirs in a and b, the
    // other person's in a.
    for (user, audience, row) in [(OTHER, "a", "ra"), (OTHER, "b", "rb"), (1, "a", "r1")] {
        standing_row(
            db,
            user,
            audience,
            "n",
            row,
            &format!(r#"{{"item":"{item}","text":"每周三晚上夜跑"}}"#),
        )
        .await;
    }
    let each = |memories: &[&str]| -> Vec<(String, String, String)> {
        memories
            .iter()
            .map(|memory| ("n".to_string(), item.to_string(), memory.to_string()))
            .collect()
    };
    let by = |user: i32, audience: &'static str| async move {
        let mut memories: Vec<String> = kept::of_node(db, user, audience, "n")
            .await
            .unwrap()
            .into_iter()
            .map(|(_, memory, _)| memory)
            .collect();
        memories.sort();
        memories
    };
    kept::note(db, OTHER, "a", &each(&["m1", "m2", "m3", "m4"]))
        .await
        .unwrap();
    kept::note(db, OTHER, "b", &each(&["m1"])).await.unwrap();
    kept::note(db, 1, "a", &each(&["n1"])).await.unwrap();
    // The latest three of the item, only where it is.
    assert_eq!(by(OTHER, "a").await, ["m2", "m3", "m4"]);
    assert_eq!(by(OTHER, "b").await, ["m1"]);
    assert_eq!(by(1, "a").await, ["n1"]);
    // A memory no longer held notes nothing.
    db.execute_unprepared("UPDATE agent_memories SET invalid_at = now() WHERE id = 'm5'")
        .await
        .unwrap();
    kept::note(db, OTHER, "a", &each(&["m5"])).await.unwrap();
    assert_eq!(by(OTHER, "a").await, ["m2", "m3", "m4"]);
    // The item set in one audience: only there.
    let body = format!(r#"{{"item":"{item}","text":"每周五晚上夜跑"}}"#);
    kept::row_written(db, OTHER, "a", "n", myriad_nodes::stands::KIND, &body)
        .await
        .unwrap();
    assert!(by(OTHER, "a").await.is_empty());
    assert_eq!(by(OTHER, "b").await, ["m1"]);
    assert_eq!(by(1, "a").await, ["n1"]);
    // Another kind of row, or a stub: nothing goes.
    kept::row_written(db, OTHER, "b", "n", "node.entry", &body)
        .await
        .unwrap();
    kept::row_written(db, OTHER, "b", "n", myriad_nodes::stands::KIND, "")
        .await
        .unwrap();
    assert_eq!(by(OTHER, "b").await, ["m1"]);
    // An item not standing where it would be noted: nothing.
    kept::note(
        db,
        OTHER,
        "b",
        &[("n".into(), "them-9999999999".into(), "m2".into())],
    )
    .await
    .unwrap();
    assert_eq!(by(OTHER, "b").await, ["m1"]);
    // Their talk moves, its rows with it: what was noted goes along.
    db.execute_unprepared("UPDATE agent_node_rows SET audience = 'c' WHERE row_id = 'rb'")
        .await
        .unwrap();
    kept::moved(db, OTHER, "b", "c").await.unwrap();
    assert!(by(OTHER, "b").await.is_empty());
    assert_eq!(by(OTHER, "c").await, ["m1"]);
    // The node forgotten whole, in one audience.
    kept::forget_node(db, OTHER, "c", "n").await.unwrap();
    assert!(by(OTHER, "c").await.is_empty());
    assert_eq!(by(1, "a").await, ["n1"]);
    // A memory forgotten: wherever it was noted.
    kept::forget(db, &["n1".to_string()]).await.unwrap();
    assert!(by(1, "a").await.is_empty());
    schema.drop().await;
}

/// What ended, what her log forgot, and what stayed behind when talk moved:
/// an ended item notes nothing; a row her log forgot takes its notes with it,
/// in its audience only; a move takes only the notes whose rows went, and the
/// place they went keeps the latest three.
#[tokio::test]
async fn what_ended_was_forgotten_or_stayed_behind_is_not_carried() {
    let Some(schema) = schema("memory_nodes_kept_left").await else {
        return;
    };
    let db = &schema.db;
    held(
        db,
        &[
            ("m1", OTHER),
            ("m2", OTHER),
            ("m3", OTHER),
            ("m4", OTHER),
            ("m5", OTHER),
        ],
    )
    .await;
    let item = "them-0123456789";
    let live = format!(r#"{{"item":"{item}","text":"每周三晚上夜跑"}}"#);
    let one =
        |node: &str, memory: &str| vec![(node.to_string(), item.to_string(), memory.to_string())];
    let by = |audience: &'static str, node: &'static str| async move {
        let mut memories: Vec<String> = kept::of_node(db, OTHER, audience, node)
            .await
            .unwrap()
            .into_iter()
            .map(|(_, memory, _)| memory)
            .collect();
        memories.sort();
        memories
    };
    // Ended on its node: a later version says so, nothing is noted.
    standing_row(db, OTHER, "a", "e", "e1", &live).await;
    standing_row(
        db,
        OTHER,
        "a",
        "e",
        "e2",
        &format!(r#"{{"item":"{item}","text":"每周三晚上夜跑","ended":true}}"#),
    )
    .await;
    kept::note(db, OTHER, "a", &one("e", "m1")).await.unwrap();
    assert!(by("a", "e").await.is_empty());
    // Her log forgot the rows on n, not the one on o; forgetting in a leaves
    // n's in b for b.
    for (audience, node, row) in [("a", "n", "ln"), ("a", "o", "lo"), ("b", "n", "lb")] {
        standing_row(db, OTHER, audience, node, row, &live).await;
        kept::note(db, OTHER, audience, &one(node, "m1"))
            .await
            .unwrap();
    }
    for (n, (row, audience, forgotten)) in
        [("ln", "a", true), ("lo", "a", false), ("lb", "b", true)]
            .into_iter()
            .enumerate()
    {
        db.execute_unprepared(&format!(
            "INSERT INTO agent_log_rows (user_id, id, audience, origin, dseq, hlc, object, kind, \
             forgotten, envelope, received_from, received_at) VALUES ({OTHER}, '{row}', \
             '{audience}', 'site', {n}, '0', 'node:x', 'node.standing', {forgotten}, '{{}}', \
             'site', now())"
        ))
        .await
        .unwrap();
    }
    kept::log_forgotten(db, OTHER, "a").await.unwrap();
    assert!(by("a", "n").await.is_empty());
    assert_eq!(by("a", "o").await, ["m1"]);
    assert_eq!(by("b", "n").await, ["m1"]);
    // Talk moves from c to d: node p's row goes, q's stays behind, and so
    // does r's, though d has r too. d already noted three on p; with c's
    // two, it keeps the latest three.
    standing_row(db, OTHER, "c", "p", "cp", &live).await;
    standing_row(db, OTHER, "c", "q", "cq", &live).await;
    standing_row(db, OTHER, "d", "p", "dp", &live).await;
    standing_row(db, OTHER, "c", "r", "cr", &live).await;
    standing_row(db, OTHER, "d", "r", "dr", &live).await;
    for memory in ["m1", "m2", "m3"] {
        kept::note(db, OTHER, "d", &one("p", memory)).await.unwrap();
    }
    for memory in ["m4", "m5"] {
        kept::note(db, OTHER, "c", &one("p", memory)).await.unwrap();
    }
    kept::note(db, OTHER, "c", &one("q", "m1")).await.unwrap();
    kept::note(db, OTHER, "c", &one("r", "m1")).await.unwrap();
    db.execute_unprepared("UPDATE agent_node_rows SET audience = 'd' WHERE row_id = 'cp'")
        .await
        .unwrap();
    kept::moved(db, OTHER, "c", "d").await.unwrap();
    assert!(by("c", "p").await.is_empty());
    assert_eq!(by("d", "p").await, ["m3", "m4", "m5"]);
    assert_eq!(by("c", "q").await, ["m1"]);
    assert_eq!(by("c", "r").await, ["m1"]);
    assert!(by("d", "r").await.is_empty());
    schema.drop().await;
}

/// Memories held by their people, learned a day apart in the order given.
async fn held(db: &sea_orm::DatabaseConnection, memories: &[(&str, i32)]) {
    for (n, (id, user)) in memories.iter().enumerate() {
        db.execute_unprepared(&format!(
            "INSERT INTO agent_memories (id, user_id, kind, content, source, venue, valid_from, \
             created_at, updated_at) VALUES ('{id}', {user}, 'fact', '{id}', 'chat', 'private', \
             now(), now() - interval '{} days', now())",
            10 - n
        ))
        .await
        .unwrap();
    }
}

/// A standing row on `node`, where `user`'s talk `audience` is.
async fn standing_row(
    db: &sea_orm::DatabaseConnection,
    user: i32,
    audience: &str,
    node: &str,
    row: &str,
    body: &str,
) {
    db.execute_unprepared(&format!(
        "INSERT INTO agent_node_rows (user_id, audience, node_id, row_id, kind, body, author, at, \
         hlc, origin, envelope) VALUES ({user}, '{audience}', '{node}', '{row}', 'node.standing', \
         '{body}', 'her', now(), '0', 'site', '{{}}')"
    ))
    .await
    .unwrap();
}

/// A memory a later fact replaced is known as replaced; one gone over into
/// another, one taken back, one held, one not here are not.
#[tokio::test]
async fn a_memory_a_later_fact_replaced_is_known_as_replaced() {
    let Some(schema) = schema("memory_nodes_known_replaced").await else {
        return;
    };
    let db = &schema.db;
    for (id, reason, by) in [
        ("old", "'superseded'", "'new'"),
        ("note", "'settled'", "'new'"),
        ("back", "'superseded'", "NULL"),
        ("new", "NULL", "NULL"),
    ] {
        db.execute_unprepared(&format!(
            "INSERT INTO agent_memories (id, user_id, kind, content, source, venue, valid_from, \
             created_at, updated_at, invalid_at, invalid_reason, superseded_by) VALUES ('{id}', \
             {OTHER}, 'fact', '{id}', 'chat', 'private', now(), now(), now(), \
             CASE WHEN {reason} IS NULL THEN NULL ELSE now() END, {reason}, {by})"
        ))
        .await
        .unwrap();
    }
    let named = super::known::named(
        db,
        ["old", "note", "back", "new", "gone"]
            .map(String::from)
            .to_vec(),
    )
    .await
    .unwrap();
    let replaced: Vec<&str> = ["old", "note", "back", "new", "gone"]
        .into_iter()
        .filter(|id| named[*id].replaced)
        .collect();
    assert_eq!(replaced, ["old"]);
    schema.drop().await;
}
