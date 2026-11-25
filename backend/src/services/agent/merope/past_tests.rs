//! On a database of their own (skipped without
//! `MYRIAD_MEROPE_TEST_DATABASE_URL`).

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};

use super::{Cleared, REOPEN, UNRECORD, after, cleared_at, close_late};
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
    db.execute_unprepared(UNRECORD).await.unwrap();
    assert!(cleared_at(db).await.unwrap().is_none());
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
        // A page or a call that keeps the old one: all in the one since.
        for _ in 0..3 {
            let kept = sessions::ensure_session_in(db, Some(old), OTHER, Mode::Chat, venue)
                .await
                .unwrap();
            assert_eq!(kept, new);
            sessions::persist_user_message_left_for_her(db, &kept, "still there?", "web")
                .await
                .unwrap();
        }
    }
    let begun: i64 = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) AS n FROM agent_sessions \
             WHERE context->>'mode' = 'chat' AND (context->>'past') IS NULL",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap();
    assert_eq!(begun, 2, "one for each old one");
    // Asked for none, a new one is begun as ever.
    let another = sessions::ensure_session(db, None, OTHER, Mode::Chat)
        .await
        .unwrap();
    assert!(another != "old");
    let work = sessions::ensure_session_in(db, Some("old_work"), OTHER, Mode::Work, None)
        .await
        .unwrap();
    assert_eq!(work, "old_work");
    schema.drop().await;
}

/// The panel's conversation and a chat app's each go on in one of their own,
/// whichever is written to first, message by message: a turn in one never
/// takes the place of one in the other. Two messages at once into one old
/// conversation go into one; cleared again, it goes on from there.
#[tokio::test]
async fn each_old_conversation_goes_on_in_one_of_its_own() {
    use crate::services::agent::turn;
    for (n, order) in [["old", "old_app"], ["old_app", "old"]].iter().enumerate() {
        let Some(schema) = fixture(&format!("past_apart_{n}")).await else {
            return;
        };
        let db = &schema.db;
        session(db, "old_app", r#"{"mode":"chat"}"#).await;
        clear(db).await;
        let on = async |old: &str| {
            sessions::ensure_session(db, Some(old), OTHER, Mode::Chat)
                .await
                .unwrap()
        };
        let mut goes_on = [String::new(), String::new()];
        for _ in 0..3 {
            for (since, old) in goes_on.iter_mut().zip(order) {
                let now = on(old).await;
                assert!(since.is_empty() || *since == now, "{old}");
                *since = now;
            }
        }
        let [one, other] = goes_on;
        assert_ne!(one, other);
        let (mut first, _) = turn::claim_chat_turn(OTHER, &one, "run_one").await;
        let (mut second, _) = turn::claim_chat_turn(OTHER, &other, "run_other").await;
        use tokio::sync::oneshot::error::TryRecvError;
        assert_eq!(first.try_recv(), Err(TryRecvError::Empty));
        assert_eq!(second.try_recv(), Err(TryRecvError::Empty));
        // At once, into one not gone on in yet.
        session(db, "old_call", r#"{"mode":"chat"}"#).await;
        clear(db).await;
        let (a, b, c) = tokio::join!(on("old_call"), on("old_call"), on("old_call"));
        assert!(a == b && b == c, "{a} {b} {c}");
        // What it went on in is of the past now too, and goes on.
        let again = on(order[0]).await;
        assert_ne!(again, one);
        assert_eq!(again, on(&one).await);
        assert_eq!(again, on(order[0]).await);
        schema.drop().await;
    }
}

/// Where an old conversation goes on: never in one they deleted (archived)
/// or another's, and through one that is of the past itself, deleted or not.
/// Her clearing undone, it is gone on in itself again.
#[tokio::test]
async fn where_an_old_conversation_goes_on_is_there_and_theirs() {
    let Some(schema) = fixture("past_since").await else {
        return;
    };
    let db = &schema.db;
    let on = async |old: &str| {
        sessions::ensure_session(db, Some(old), OTHER, Mode::Chat)
            .await
            .unwrap()
    };
    let archive = async |id: &str| {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_sessions SET archived = true WHERE id = $1",
            [id.into()],
        ))
        .await
        .unwrap();
    };
    clear(db).await;
    // Deleted: another takes its place, and every message goes into that.
    let deleted = on("old").await;
    archive(&deleted).await;
    let since = on("old").await;
    assert_ne!(since, deleted);
    assert_eq!(on("old").await, since);
    sessions::persist_user_message(db, &since, "still there?")
        .await
        .unwrap();
    let read = sessions::load_private_chat_history(db, OTHER, 20)
        .await
        .unwrap();
    assert_eq!(read.len(), 1, "read where it goes on");
    // Deleted and then of the past: followed to the one it goes on in.
    archive(&since).await;
    clear(db).await;
    let after = on(&since).await;
    assert_ne!(after, since);
    assert_eq!(on("old").await, after);
    // Another's is not theirs to go on in.
    db.execute_unprepared(
        "INSERT INTO agent_sessions (id, user_id, context, message_count, archived, created_at, \
         last_active_at) VALUES ('owners', 7, '{\"mode\":\"chat\"}', 0, false, NOW(), NOW()); \
         UPDATE agent_sessions SET context = context::jsonb || '{\"since\":\"owners\"}'::jsonb \
         WHERE id = 'old';",
    )
    .await
    .unwrap();
    let theirs = on("old").await;
    assert!(theirs != "owners" && theirs != after);
    assert_eq!(on("old").await, theirs);
    // Undone: the last clearing's are of before no more, with nothing noted.
    session(db, "last", r#"{"mode":"chat"}"#).await;
    clear(db).await;
    assert_ne!(on("last").await, "last");
    let at = cleared_at(db).await.unwrap().unwrap();
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        REOPEN,
        [at.to_rfc3339().into()],
    ))
    .await
    .unwrap();
    assert_eq!(on("last").await, "last");
    let noted: Option<String> = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT context->>'since' AS since FROM agent_sessions WHERE id = 'last'",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "since")
        .unwrap();
    assert_eq!(noted, None);
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
    // Begun after she was cleared and before that was committed.
    session(db, "between", r#"{"mode":"chat"}"#).await;
    said(db, "between", "user", "hello?").await;
    let committed = crate::clock::now();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    session(db, "since", r#"{"mode":"chat"}"#).await;
    said(db, "since", "user", "hello").await;
    close_late(db, committed).await;
    let left: Vec<String> = private_lines(db, OTHER, 50)
        .await
        .into_iter()
        .map(|line| line.content)
        .collect();
    assert_eq!(left, ["hello"]);
    schema.drop().await;
}

/// Where the next her goes, the chats of before do not go with her, and her
/// going takes none of them from the site: they keep their mark.
#[tokio::test]
async fn what_was_said_before_does_not_go_with_the_next_her() {
    use crate::services::persona_sync::{assets::Pictures, snapshot};
    let Some(schema) = fixture("past_moves").await else {
        return;
    };
    let db = &schema.db;
    db.execute_unprepared("UPDATE agent_sessions SET user_id = 7")
        .await
        .unwrap();
    clear(db).await;
    session(db, "since", r#"{"mode":"chat"}"#).await;
    said(db, "since", "user", "hello").await;
    db.execute_unprepared("UPDATE agent_sessions SET user_id = 7, message_count = 2")
        .await
        .unwrap();
    let taken: Vec<String> = snapshot::of_kind(
        db,
        &Pictures::new(db, 7),
        7,
        myriad_persona_sync::Kind::Conversation,
        None,
        100,
    )
    .await
    .unwrap()
    .into_iter()
    .map(|entity| entity.id)
    .collect();
    assert_eq!(taken, ["since"]);
    let txn = db.begin().await.unwrap();
    crate::services::agent::merope::retire_her_on(&txn, 7)
        .await
        .unwrap();
    txn.commit().await.unwrap();
    let left: Vec<String> = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT s.id FROM agent_sessions s JOIN agent_messages m ON m.session_id = s.id \
             WHERE s.context->>'mode' = 'chat' AND (s.context->>'past') IS NOT NULL \
             GROUP BY s.id ORDER BY s.id",
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get("", "id").unwrap())
        .collect();
    assert_eq!(left, ["old", "old_group"]);
    let since: i64 = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) AS n FROM agent_sessions WHERE id = 'since'",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap();
    assert_eq!(since, 0, "what moved with her goes");
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
