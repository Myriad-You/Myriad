//! The second look before what they took back is forgotten on her nodes
//! (`take_back_check`), on a database of its own, with two people and three
//! audiences: someone's private talk with her, the site owner's, and a group
//! kept with the site owner.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use super::talk_tests::{OTHER, answering, asked, count, schema};
use super::{Author, RowIn, audience, forget, take_back_check, write};
use crate::services::agent::merope::chat_lines::tests::{line, session};
use crate::services::her_log::pen;

/// The site owner, here.
const OWNER: i32 = 1;

/// A memory of `user`'s, held.
async fn memory(db: &DatabaseConnection, id: &str, user: i32, content: &str) {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_memories (id, user_id, kind, content, source, venue, valid_from, \
         created_at, updated_at) VALUES ($1, $2, 'fact', $3, 'chat', 'private', now(), now(), now())",
        [id.into(), user.into(), content.into()],
    ))
    .await
    .unwrap();
}

/// An entry of the memory `memory` on a node `name` in `user`'s `audience`,
/// written as her upkeep writes.
async fn on_node(db: &DatabaseConnection, user: i32, audience: &str, name: &str, memory: &str) {
    let said_by = if audience.starts_with("group:") {
        "other_person"
    } else {
        "partner"
    };
    let made = myriad_nodes::entries::fact_rows(
        serde_json::from_value(json!({
            "audience": audience,
            "start": {"name": name},
            "facts": [{"memory": memory, "text": memory, "saidBy": [said_by]}],
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
    write::write(db, user, audience, &rows, Some(&thought))
        .await
        .unwrap();
}

/// Entries in `user`'s `audience` resting on `memory` that are forgotten
/// (stubs), and that still stand.
async fn entries(db: &DatabaseConnection, user: i32, audience: &str) -> (i64, i64) {
    let stubs = count(
        db,
        &format!(
            "SELECT count(*) AS n FROM agent_node_rows WHERE user_id = {user} \
             AND audience = '{audience}' AND kind = 'node.entry' AND body = ''"
        ),
    )
    .await;
    let standing = count(
        db,
        &format!(
            "SELECT count(*) AS n FROM agent_node_rows WHERE user_id = {user} \
             AND audience = '{audience}' AND kind = 'node.entry' AND body <> ''"
        ),
    )
    .await;
    (stubs, standing)
}

/// Taken back (let go with nothing in its place), as letting go writes it.
async fn let_go(db: &DatabaseConnection, ids: &[&str]) {
    for id in ids {
        db.execute_unprepared(&format!(
            "UPDATE agent_memories SET invalid_at = now(), invalid_reason = 'superseded', \
             superseded_by = NULL, updated_at = now() WHERE id = '{id}'"
        ))
        .await
        .unwrap();
    }
}

async fn state(db: &DatabaseConnection, memory: &str) -> String {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT state FROM agent_takeback_check WHERE memory_id = $1",
        [memory.into()],
    ))
    .await
    .unwrap()
    .map(|row| row.try_get::<String>("", "state").unwrap())
    .unwrap_or_default()
}

/// Swept before each audience is read, as her turns there would.
async fn swept(db: &DatabaseConnection, audiences: &[(i32, &str)]) {
    for (user, audience) in audiences {
        forget::taken_back_here(db, *user, audience).await.unwrap();
    }
}

/// Only a taking back the second look agrees with forgets what rests on it,
/// and only where that is; a change, an answer that does not come, a line
/// that is not theirs, one with no line, and one taken back before there
/// was a second look are only let go and hidden. Deleting still forgets.
#[tokio::test]
async fn only_what_the_second_look_agrees_was_taken_back_is_forgotten() {
    let Some(schema) = schema("memory_nodes_take_back_check").await else {
        return;
    };
    let db = &schema.db;
    let theirs = audience::private_talk(OTHER, Some(OWNER));
    let owners = audience::private_talk(OWNER, Some(OWNER));
    let group = audience::group("telegram:-100");
    let all = [
        (OTHER, theirs.as_str()),
        (OWNER, owners.as_str()),
        (OWNER, group.as_str()),
    ];
    // Theirs: one taken back for good (a), one changed (b), one said in the
    // group (g), one with no line (d), one taken back before (e), one
    // deleted (f). The owner's: one with no answer (c), one whose line is
    // someone else's (h).
    for (id, user) in [
        ("a", OTHER),
        ("b", OTHER),
        ("g", OTHER),
        ("d", OTHER),
        ("e", OTHER),
        ("f", OTHER),
        ("c", OWNER),
        ("h", OWNER),
    ] {
        memory(db, id, user, &format!("记着的 {id}")).await;
    }
    for (name, id) in [("a", "a"), ("b", "b"), ("d", "d"), ("e", "e"), ("f", "f")] {
        on_node(db, OTHER, &theirs, name, id).await;
    }
    on_node(db, OWNER, &group, "g", "g").await;
    on_node(db, OWNER, &owners, "c", "c").await;
    on_node(db, OWNER, &owners, "h", "h").await;
    session(db, "s7", OTHER, Some(r#"{"mode":"chat"}"#), false).await;
    session(db, "s1", OWNER, Some(r#"{"mode":"chat"}"#), false).await;
    let a_said = line(db, "s7", "user", "那个当我没说", 3, None).await;
    let b_said = line(db, "s7", "user", "现在不一样了", 3, None).await;
    let g_said = line(db, "s7", "user", "群里那句别记", 3, None).await;
    let c_said = line(db, "s1", "user", "算了别记了", 3, None).await;
    // Let go as taken back, each held for the second look in its message.
    let_go(db, &["a", "b", "g", "d", "e", "c", "h"]).await;
    take_back_check::hold(db, OTHER, &["a".into()], Some(a_said))
        .await
        .unwrap();
    take_back_check::hold(db, OTHER, &["b".into()], Some(b_said))
        .await
        .unwrap();
    take_back_check::hold(db, OTHER, &["g".into()], Some(g_said))
        .await
        .unwrap();
    take_back_check::hold(db, OTHER, &["d".into()], None)
        .await
        .unwrap();
    take_back_check::hold(db, OWNER, &["c".into()], Some(c_said))
        .await
        .unwrap();
    // Their line, not the owner's.
    take_back_check::hold(db, OWNER, &["h".into()], Some(a_said))
        .await
        .unwrap();
    // One not theirs is not held for them.
    take_back_check::hold(db, OWNER, &["b".into()], Some(b_said))
        .await
        .unwrap();
    assert_eq!(
        count(
            db,
            "SELECT count(*) AS n FROM agent_takeback_check WHERE memory_id = 'b'"
        )
        .await,
        1
    );
    db.execute_unprepared(
        "UPDATE agent_memories SET invalid_at = now(), invalid_reason = 'deleted', \
         updated_at = now() WHERE id = 'f'",
    )
    .await
    .unwrap();
    // Before the second look: hidden, nothing forgotten anywhere but what
    // was deleted.
    swept(db, &all).await;
    assert_eq!(entries(db, OTHER, &theirs).await, (1, 4));
    assert_eq!(entries(db, OWNER, &owners).await, (0, 2));
    assert_eq!(entries(db, OWNER, &group).await, (0, 1));
    assert_eq!(state(db, "d").await, take_back_check::UNSURE);
    assert_eq!(state(db, "e").await, "");
    // Theirs looked at: a taken back for good, b a change, g a part.
    answering(&[
        ("take_back_check", r#"{"says":"takes_back"}"#),
        ("take_back_check", r#"{"says":"changed"}"#),
        ("take_back_check", r#"{"says":"takes_back_part"}"#),
    ]);
    take_back_check::go_over(db, OTHER).await;
    assert_eq!(asked("take_back_check"), 3);
    assert_eq!(state(db, "a").await, take_back_check::CONFIRMED);
    assert_eq!(state(db, "b").await, take_back_check::CHANGED);
    assert_eq!(state(db, "g").await, take_back_check::CHANGED);
    // Forgotten at once where a rests, and nowhere else.
    assert_eq!(entries(db, OTHER, &theirs).await, (2, 3));
    assert_eq!(entries(db, OWNER, &owners).await, (0, 2));
    assert_eq!(entries(db, OWNER, &group).await, (0, 1));
    // The owner's: with the model out of reach, c (held first) spends no
    // try and waits, and nothing more is asked now.
    answering(&[]);
    take_back_check::go_over(db, OWNER).await;
    assert_eq!(asked("take_back_check"), 1);
    assert_eq!(state(db, "h").await, take_back_check::PENDING);
    assert_eq!(state(db, "c").await, take_back_check::PENDING);
    assert_eq!(
        count(
            db,
            "SELECT count(*) AS n FROM agent_takeback_check WHERE memory_id = 'c' AND tries = 0"
        )
        .await,
        1
    );
    db.execute_unprepared("UPDATE agent_takeback_check SET asked_at = NULL")
        .await
        .unwrap();
    // h's line is not theirs, so it is not asked about and is left unsure;
    // c gets answers that cannot be read: unsure after the third.
    for tries in 1..=3 {
        answering(&[("take_back_check", "不是 JSON")]);
        take_back_check::go_over(db, OWNER).await;
        assert_eq!(asked("take_back_check"), 1, "try {tries}");
        assert_eq!(state(db, "h").await, take_back_check::UNSURE);
        let expected = if tries < 3 {
            take_back_check::PENDING
        } else {
            take_back_check::UNSURE
        };
        assert_eq!(state(db, "c").await, expected, "try {tries}");
        // Asked again only once nobody is asking.
        db.execute_unprepared("UPDATE agent_takeback_check SET asked_at = NULL")
            .await
            .unwrap();
    }
    // Nothing more to ask.
    answering(&[]);
    take_back_check::go_over(db, OTHER).await;
    take_back_check::go_over(db, OWNER).await;
    assert_eq!(asked("take_back_check"), 0);
    // The sweep forgets only what was confirmed or deleted, wherever it is
    // read.
    swept(db, &all).await;
    assert_eq!(entries(db, OTHER, &theirs).await, (2, 3));
    assert_eq!(entries(db, OWNER, &owners).await, (0, 2));
    assert_eq!(entries(db, OWNER, &group).await, (0, 1));
    // What the status page shows: counts only.
    assert_eq!(
        take_back_check::counts(db).await,
        json!({"pending": 0, "changed": 2, "unsure": 3})
    );
    schema.drop().await;
}

/// A confirmed taking back whose forgetting did not happen at once (another
/// end's, or one that failed) is forgotten by the sweep, in each audience
/// it rests in; one not confirmed is not.
#[tokio::test]
async fn the_sweep_forgets_what_was_confirmed_in_each_audience() {
    let Some(schema) = schema("memory_nodes_take_back_swept").await else {
        return;
    };
    let db = &schema.db;
    let theirs = audience::private_talk(OTHER, Some(OWNER));
    let owners = audience::private_talk(OWNER, Some(OWNER));
    let group = audience::group("telegram:-100");
    memory(db, "x", OTHER, "记着的 x").await;
    memory(db, "y", OWNER, "记着的 y").await;
    on_node(db, OTHER, &theirs, "x", "x").await;
    on_node(db, OWNER, &group, "x", "x").await;
    on_node(db, OWNER, &owners, "y", "y").await;
    let_go(db, &["x", "y"]).await;
    db.execute_unprepared(
        "INSERT INTO agent_takeback_check (user_id, memory_id, state, at) \
         VALUES (7, 'x', 'confirmed', now()), (1, 'y', 'pending', now())",
    )
    .await
    .unwrap();
    // Forgotten wherever it rests, as forgetting a memory is.
    forget::taken_back_here(db, OTHER, &theirs).await.unwrap();
    assert_eq!(entries(db, OTHER, &theirs).await, (1, 0));
    assert_eq!(entries(db, OWNER, &group).await, (1, 0));
    forget::taken_back_here(db, OWNER, &group).await.unwrap();
    forget::taken_back_here(db, OWNER, &owners).await.unwrap();
    assert_eq!(entries(db, OWNER, &owners).await, (0, 1));
    schema.drop().await;
}
