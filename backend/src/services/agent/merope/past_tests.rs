//! On a database of their own (skipped without
//! `MYRIAD_MEROPE_TEST_DATABASE_URL`).

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};

use super::{Cleared, REOPEN, after, cleared_at, close_late};
use crate::services::agent::AgentInteractionMode as Mode;
use crate::services::agent::merope::{
    chat_lines::private_lines, clear_persona_on, reach, speaking_context, store,
};
use crate::services::agent::sessions;

const OTHER: i32 = 8;
const GROUP: &str = "telegram:-100123";

async fn fixture(name: &str) -> Option<crate::db::IsolatedSchema> {
    let url = crate::db::test_url(&["MYRIAD_MEROPE_TEST_DATABASE_URL"])?;
    let schema = crate::db::IsolatedSchema::migrated(&url, name).await;
    schema
        .db
        .execute_unprepared(
            "INSERT INTO users (id, username, is_owner) VALUES (7, 'owner', true), \
             (8, 'other', false); \
             INSERT INTO agent_persona (id, name, updated_at) VALUES ('site', 'Merope', NOW());",
        )
        .await
        .unwrap();
    let db = &schema.db;
    session(db, "old_plain", "{}").await;
    session(db, "old_group", r#"{"mode":"chat","venue":"telegram:-100123"}"#).await;
    session(db, "old", r#"{"mode":"chat"}"#).await;
    session(db, "old_work", r#"{"mode":"work"}"#).await;
    for old in ["old", "old_group", "old_work"] {
        said(db, old, "user", "my fern looks dry").await;
        said(db, old, "assistant", "give it water").await;
    }
    Some(schema)
}

async fn session(db: &DatabaseConnection, id: &str, context: &str) {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_sessions (id, user_id, context, message_count, archived, created_at, \
         last_active_at) VALUES ($1, 8, $2::json, 2, false, NOW(), NOW())",
        [id.into(), context.into()],
    ))
    .await
    .unwrap();
}

async fn said(db: &DatabaseConnection, session: &str, role: &str, content: &str) {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_messages (session_id, role, content, created_at) \
         VALUES ($1, $2, $3, NOW())",
        [session.into(), role.into(), content.into()],
    ))
    .await
    .unwrap();
}

async fn clear(db: &DatabaseConnection) {
    let txn = db.begin().await.unwrap();
    clear_persona_on(&txn).await.unwrap();
    txn.commit().await.unwrap();
}

/// What she reads of what was said with `OTHER`, by who reads it: how many
/// lines.
async fn read(db: &DatabaseConnection) -> Vec<(&'static str, usize)> {
    let hour = (crate::clock::now() - chrono::Duration::hours(1)).fixed_offset();
    let history = |session: &'static str| async move {
        sessions::load_session_history(db, session, 20, true)
            .await
            .unwrap()
            .len()
    };
    vec![
        ("private lines", private_lines(db, OTHER, 50).await.len()),
        (
            "private history",
            sessions::load_private_chat_history(db, OTHER, 20)
                .await
                .unwrap()
                .len(),
        ),
        ("private window", history("old").await),
        ("group window", history("old_group").await),
        (
            "lines since",
            store::chat_lines_since(db, hour, 100).await.unwrap().len(),
        ),
        (
            "their lines",
            store::their_lines(db, OTHER, 100).await.unwrap().len(),
        ),
        (
            "their lines in private",
            store::their_lines_in(db, OTHER, None, 100).await.unwrap().len(),
        ),
        (
            "their lines in a group",
            store::their_lines_in(db, OTHER, Some(GROUP), 100)
                .await
                .unwrap()
                .len(),
        ),
        (
            "the conversation to write first in",
            usize::from(store::latest_open_session(db, OTHER).await.unwrap().is_some()),
        ),
        ("their talk lately", reach::recent_talk(db, OTHER).await.len()),
    ]
}

/// Cleared, none of what was said before is read, in private or in a group;
/// what is said since is. Their own talk is all there still, Work's goes on
/// as it did, and taking the marks off again brings it all back.
#[tokio::test]
async fn what_was_said_before_is_read_by_none_of_the_next() {
    let Some(schema) = fixture("past_read").await else {
        return;
    };
    let db = &schema.db;
    for (reader, lines) in read(db).await {
        assert!(lines > 0, "{reader} reads nothing to begin with");
    }
    clear(db).await;
    for (reader, lines) in read(db).await {
        assert_eq!(lines, 0, "{reader}");
    }
    let work = sessions::load_session_history(db, "old_work", 20, false)
        .await
        .unwrap();
    assert_eq!(work.len(), 2, "Work's is not hers");
    session(db, "new", r#"{"mode":"chat"}"#).await;
    said(db, "new", "user", "hello").await;
    let private = private_lines(db, OTHER, 50).await;
    assert_eq!(private.len(), 1);
    let history = sessions::load_private_chat_history(db, OTHER, 20)
        .await
        .unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].content, "hello");
    // All of it is there for the one it was with.
    let kept: i64 = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) AS n FROM agent_messages WHERE session_id LIKE 'old%'",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap();
    assert_eq!(kept, 6);

    let at = cleared_at(db).await.unwrap().unwrap();
    let opened = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            REOPEN,
            [at.to_rfc3339().into()],
        ))
        .await
        .unwrap();
    assert_eq!(opened.rows_affected(), 3);
    for (reader, lines) in read(db).await {
        assert!(lines > 0, "{reader}");
    }
    schema.drop().await;
}

/// How long she has known someone is of the her they talked with.
#[tokio::test]
async fn she_knows_no_one_from_before() {
    let Some(schema) = fixture("past_known").await else {
        return;
    };
    let db = &schema.db;
    clear(db).await;
    let (first, days) = speaking_context::acquaintance(db, OTHER).await.unwrap();
    // Work's conversation is the only one left that counts.
    assert!(first.is_some());
    assert_eq!(days, 1);
    db.execute_unprepared("DELETE FROM agent_messages WHERE session_id = 'old_work'")
        .await
        .unwrap();
    super::forget();
    let (first, days) = speaking_context::acquaintance(db, OTHER).await.unwrap();
    assert!(first.is_none(), "read again, not kept from before");
    assert_eq!(days, 0);
    schema.drop().await;
}

/// A message sent into an old conversation starts another; one into Work's
/// goes on there.
#[tokio::test]
async fn an_old_conversation_is_not_gone_on_in() {
    let Some(schema) = fixture("past_goes_on").await else {
        return;
    };
    let db = &schema.db;
    for (old, venue) in [("old", None), ("old_group", Some(GROUP))] {
        let same = sessions::ensure_session_in(db, Some(old), OTHER, Mode::Chat, venue)
            .await
            .unwrap();
        assert_eq!(same, old);
    }
    clear(db).await;
    for (old, venue) in [("old", None), ("old_group", Some(GROUP))] {
        let new = sessions::ensure_session_in(db, Some(old), OTHER, Mode::Chat, venue)
            .await
            .unwrap();
        assert_ne!(new, old);
        let again = sessions::ensure_session_in(db, Some(&new), OTHER, Mode::Chat, venue)
            .await
            .unwrap();
        assert_eq!(again, new, "the new one is gone on in");
    }
    let work = sessions::ensure_session_in(db, Some("old_work"), OTHER, Mode::Work, None)
        .await
        .unwrap();
    assert_eq!(work, "old_work");
    schema.drop().await;
}

/// A reply under way as she was cleared is of the past; so is a conversation
/// begun just before that was not there to be marked. One begun since is not.
#[tokio::test]
async fn what_was_under_way_is_of_the_past() {
    let Some(schema) = fixture("past_under_way").await else {
        return;
    };
    let db = &schema.db;
    clear(db).await;
    sessions::persist_assistant_message(db, "old", None, "and not too much", None)
        .await
        .unwrap();
    assert!(private_lines(db, OTHER, 50).await.is_empty());
    session(db, "late", r#"{"mode":"chat"}"#).await;
    db.execute_unprepared(
        "UPDATE agent_sessions SET created_at = NOW() - INTERVAL '1 hour' WHERE id = 'late'",
    )
    .await
    .unwrap();
    said(db, "late", "user", "are you there?").await;
    session(db, "since", r#"{"mode":"chat"}"#).await;
    said(db, "since", "user", "hello").await;
    close_late(db).await;
    let left: Vec<String> = private_lines(db, OTHER, 50)
        .await
        .into_iter()
        .map(|line| line.content)
        .collect();
    assert_eq!(left, ["hello"]);
    schema.drop().await;
}

/// Clearing her that does not go through marks nothing.
#[tokio::test]
async fn clearing_her_undone_marks_nothing() {
    let Some(schema) = fixture("past_undone").await else {
        return;
    };
    let db = &schema.db;
    let txn = db.begin().await.unwrap();
    clear_persona_on(&txn).await.unwrap();
    txn.rollback().await.unwrap();
    assert!(cleared_at(db).await.unwrap().is_none());
    for (reader, lines) in read(db).await {
        assert!(lines > 0, "{reader}");
    }
    schema.drop().await;
}

/// A group's lines end where she was cleared.
#[test]
fn lines_kept_of_a_group_end_where_she_was_cleared() {
    let at = |minutes: i64| crate::clock::now() + chrono::Duration::minutes(minutes);
    let lines = vec![at(-2), at(2)];
    let since = after(Some(Cleared { at: at(0) }), lines.clone(), |line| *line);
    assert_eq!(since, [lines[1]]);
    assert_eq!(after(None, lines.clone(), |line| *line), lines);
}
