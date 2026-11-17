//! The lines a chat turn may look back through, on a database of its own:
//! their own private chat only, nothing they took back, and what another
//! end forgot gone here too.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use super::*;
use crate::services::agent::memory::unified::{self, Audience, MemoryKind, NewMemory, Speaker};
use myriad_merope::remembering::Who;

pub(in crate::services::agent::merope) const OWNER: i32 = 1;
pub(in crate::services::agent::merope) const OTHER: i32 = 7;

pub(in crate::services::agent::merope) async fn schema(
    prefix: &str,
) -> Option<crate::db::IsolatedSchema> {
    let url = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"])?;
    let schema = crate::db::IsolatedSchema::migrated(&url, prefix).await;
    schema
        .db
        .execute_unprepared(
            "INSERT INTO users (id, username, is_owner) VALUES (1, 'one', true), (7, 'seven', false)",
        )
        .await
        .unwrap();
    Some(schema)
}

/// A conversation of `user_id`'s with `context` (none: as the long-chat
/// test keeps one).
pub(in crate::services::agent::merope) async fn session(
    db: &DatabaseConnection,
    id: &str,
    user_id: i32,
    context: Option<&str>,
    archived: bool,
) {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) VALUES ($1, $2, 't', $3::jsonb, 0, $4, NOW(), NOW())",
        [
            id.into(),
            user_id.into(),
            context.map(str::to_string).into(),
            archived.into(),
        ],
    ))
    .await
    .unwrap();
}

/// A line said `minutes_ago`, its id.
pub(in crate::services::agent::merope) async fn line(
    db: &DatabaseConnection,
    session_id: &str,
    role: &str,
    content: &str,
    minutes_ago: i32,
    metadata: Option<&str>,
) -> i32 {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_messages (session_id, role, content, metadata, created_at) \
         VALUES ($1, $2, $3, $4::jsonb, NOW() - $5 * INTERVAL '1 minute') RETURNING id",
        [
            session_id.into(),
            role.into(),
            content.into(),
            metadata.map(str::to_string).into(),
            minutes_ago.into(),
        ],
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get::<i32>("", "id")
    .unwrap()
}

pub(in crate::services::agent::merope) async fn count(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "n")
        .unwrap()
}

/// A memory of the owner's in private that quotes `evidence`.
pub(in crate::services::agent::merope) async fn quoting(
    db: &DatabaseConnection,
    source: &'static str,
    evidence: &str,
) -> String {
    unified::remember(
        db,
        NewMemory {
            user_id: OWNER,
            kind: MemoryKind::Fact,
            content: format!("记得：{evidence}"),
            evidence: Some(evidence.to_string()),
            speaker: Speaker::User,
            source,
            audience: Audience::private(OWNER),
            importance: 0.5,
            concepts: Vec::new(),
        },
    )
    .await
    .unwrap()
    .unwrap()
}

fn contents(lines: &[Line]) -> Vec<&str> {
    lines.iter().map(|line| line.content.as_str()).collect()
}

fn found(found: &[(String, Who, String)]) -> Vec<&str> {
    found.iter().map(|(_, _, text)| text.as_str()).collect()
}

/// A chat turn of theirs looks back through their own private chat and
/// nothing else: never a group's talk, a work session, an archived
/// conversation, a piece of work coming back, or another person's chat;
/// scrolling back by words reads the same.
#[tokio::test]
async fn only_their_own_private_chat_is_read() {
    let Some(schema) = schema("chat_lines_audience").await else {
        return;
    };
    let db = &schema.db;
    let said = [
        "PRIVATE 我在学吉他",
        "BARE 吉他弦断了",
        "GROUP 群里说吉他",
        "WORK 帮我查吉他价格",
        "ARCHIVED 旧对话里的吉他",
        "PIECE 办事结果：吉他",
        "OTHER 别人的吉他",
    ];
    session(db, "mine", OWNER, Some(r#"{"mode":"chat"}"#), false).await;
    session(db, "bare", OWNER, None, false).await;
    session(
        db,
        "group",
        OWNER,
        Some(r#"{"mode":"chat","venue":"qq:42"}"#),
        false,
    )
    .await;
    session(db, "work", OWNER, Some(r#"{"mode":"work"}"#), false).await;
    session(db, "old", OWNER, Some(r#"{"mode":"chat"}"#), true).await;
    session(db, "theirs", OTHER, Some(r#"{"mode":"chat"}"#), false).await;
    line(db, "mine", "user", said[0], 50, None).await;
    line(db, "bare", "user", said[1], 45, None).await;
    line(db, "group", "user", said[2], 40, None).await;
    line(db, "work", "user", said[3], 35, None).await;
    line(db, "old", "user", said[4], 30, None).await;
    line(
        db,
        "mine",
        "assistant",
        said[5],
        25,
        Some(r#"{"piece":"p1"}"#),
    )
    .await;
    line(db, "theirs", "user", said[6], 20, None).await;

    assert_eq!(
        contents(&private_lines(db, OWNER, 3000).await),
        [said[0], said[1]]
    );
    assert_eq!(contents(&private_lines(db, OTHER, 3000).await), [said[6]]);
    let by_words = super::super::remembering::look_back(db, OWNER, "吉他", "", 10).await;
    let by_words = found(&by_words);
    for never in &said[2..] {
        assert!(
            !by_words.iter().any(|text| text.contains(never)),
            "{never}: {by_words:?}"
        );
    }
    schema.drop().await;
}

/// What they took back (a memory let go with nothing in its place, or
/// deleted) is not brought up again, nor her answer to it, though the words
/// stay in the chat; what a later fact replaced still is, and says so
/// where she reads it.
#[tokio::test]
async fn what_they_took_back_is_not_read() {
    let Some(schema) = schema("chat_lines_withdrawn").await else {
        return;
    };
    let db = &schema.db;
    let said = [
        "我其实不喜欢香菜",
        "好，那以后不放香菜",
        "我住在北京朝阳",
        "朝阳那边挺方便",
        "周末想去爬香山",
    ];
    session(db, "mine", OWNER, Some(r#"{"mode":"chat"}"#), false).await;
    for (index, text) in said.iter().enumerate() {
        let role = if index % 2 == 0 { "user" } else { "assistant" };
        line(db, "mine", role, text, 60 - index as i32 * 5, None).await;
    }
    let let_go = quoting(db, unified::SHORT_NOTE_SOURCE, "不喜欢香菜").await;
    let corrected = quoting(db, unified::SHORT_NOTE_SOURCE, "住在北京朝阳").await;
    let newer = quoting(db, unified::SHORT_NOTE_SOURCE, "住在北京海淀").await;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memories SET invalid_reason = 'superseded', superseded_by = NULL WHERE id = $1",
        [let_go.into()],
    ))
    .await
    .unwrap();
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memories SET invalid_reason = 'superseded', superseded_by = $2 WHERE id = $1",
        [corrected.into(), newer.clone().into()],
    ))
    .await
    .unwrap();

    // What a later fact took the place of is still read, as said, and
    // known as replaced: their line only, not her answer to it.
    let read = private_lines(db, OWNER, 3000).await;
    assert_eq!(
        contents(&read),
        [said[2], said[3], said[4]],
        "the line and her answer are left out"
    );
    assert_eq!(
        read.iter().map(|line| line.replaced).collect::<Vec<_>>(),
        [true, false, false]
    );
    assert_eq!(
        count(db, "SELECT COUNT(*) AS n FROM agent_messages").await,
        5,
        "and stay in the chat"
    );
    // Where she reads it, its day says so; one nothing replaced has its
    // day alone. Their words are as said either way.
    let day = |index: usize| super::found_on("2026-11-17".into(), read[index].replaced);
    assert_eq!(day(0), format!("2026-11-17, {}", super::REPLACED_SINCE));
    assert_eq!(day(1), "2026-11-17");
    // The later fact taken back too: nothing she keeps took its place, so
    // the line is no longer marked.
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memories SET invalid_reason = 'deleted', invalid_at = now() WHERE id = $1",
        [newer.into()],
    ))
    .await
    .unwrap();
    let read = private_lines(db, OWNER, 3000).await;
    assert!(read.iter().all(|line| !line.replaced), "{read:?}");
    let by_words = super::super::remembering::look_back(db, OWNER, "香菜", "", 3).await;
    assert!(
        !found(&by_words).iter().any(|text| text.contains("香菜")),
        "{by_words:?}"
    );
    schema.drop().await;
}

/// A memory another end forgot, taken in here (`her_log::memories::take_in`),
/// takes the words it came from here too: the line it quotes and her
/// answer, as forgetting it here does.
#[tokio::test]
async fn a_memory_forgotten_elsewhere_takes_its_lines_here() {
    let Some(schema) = schema("chat_lines_taken_in").await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", OWNER, Some(r#"{"mode":"chat"}"#), false).await;
    let cat = line(db, "mine", "user", "我的猫叫团子", 30, None).await;
    let answer = line(db, "mine", "assistant", "团子几岁了", 29, None).await;
    let kept = line(db, "mine", "user", "我在学日语", 20, None).await;
    let memory = quoting(db, unified::SHORT_NOTE_SOURCE, "猫叫团子").await;
    let txn = sea_orm::TransactionTrait::begin(db).await.unwrap();
    let gone = crate::services::agent::memory::origins::forget_taken_in(
        &txn,
        OWNER,
        unified::OWN_VENUE,
        &memory,
    )
    .await
    .unwrap();
    txn.commit().await.unwrap();
    let mut went: Vec<i32> = gone.iter().map(|line| line.id).collect();
    went.sort_unstable();
    assert_eq!(went, [cat, answer]);
    let left: Vec<i32> = private_lines(db, OWNER, 3000)
        .await
        .iter()
        .map(|line| line.id)
        .collect();
    assert_eq!(left, [kept]);
    // Someone else's memory, or a group's, takes nothing.
    let theirs = quoting(db, unified::SHORT_NOTE_SOURCE, "在学日语").await;
    let txn = sea_orm::TransactionTrait::begin(db).await.unwrap();
    let gone = crate::services::agent::memory::origins::forget_taken_in(
        &txn,
        OTHER,
        unified::OWN_VENUE,
        &theirs,
    )
    .await
    .unwrap();
    txn.commit().await.unwrap();
    assert!(gone.is_empty());
    schema.drop().await;
}
