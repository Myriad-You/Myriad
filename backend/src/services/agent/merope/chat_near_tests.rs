//! Her private chat nearest by meaning, on a database of its own: only the
//! lines a chat turn may look back through are given vectors or found, and
//! whatever forgets a line takes its vector with it.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use super::*;
use crate::services::agent::memory::unified;
use crate::services::agent::merope::chat_lines::tests::{
    OTHER, OWNER, count, line, quoting, schema, session,
};
use crate::services::channel_group::g0;

/// A world where every one of `texts`, and what is asked, means the same.
fn meaning_alike(texts: &[&str], asked: &str) {
    let mut world = g0::World::new(0.5);
    for text in texts.iter().chain([&asked]) {
        world.vectors.insert(text.to_string(), vec![1.0, 0.0, 0.0]);
    }
    g0::enter(world);
}

fn texts(found: &[(String, Who, String)]) -> Vec<&str> {
    found.iter().map(|(_, _, text)| text.as_str()).collect()
}

/// What a private chat turn may be shown is their own private chat and
/// nothing else: never a group's talk, a work session, an archived
/// conversation, a piece of work coming back, or another person's chat;
/// none of those is even given a vector.
#[tokio::test]
async fn only_their_own_private_chat_is_found() {
    let Some(schema) = schema("chat_near_audience").await else {
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
    meaning_alike(&said, "吉他");
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

    let lines = private_lines(db, OWNER, 3000).await;
    let read: Vec<&str> = lines.iter().map(|line| line.content.as_str()).collect();
    assert_eq!(read, [said[0], said[1]], "only their private chat is read");

    assert_eq!(fill(db, OWNER).await.given, 2);
    assert_eq!(
        fill(db, OWNER).await.given,
        0,
        "a line given a vector is not asked again"
    );
    assert_eq!(
        count(db, "SELECT COUNT(*) AS n FROM agent_message_embeddings").await,
        2,
        "nothing outside it is given a vector"
    );
    let found = nearest(db, OWNER, "吉他", 10, false).await;
    assert_eq!(texts(&found), [said[0], said[1]]);
    // The other person's own turn finds only theirs.
    fill(db, OTHER).await;
    let found = nearest(db, OTHER, "吉他", 10, false).await;
    assert_eq!(texts(&found), [said[6]]);
    g0::leave();
    schema.drop().await;
}

/// What they took back (a memory let go with nothing in its place, or
/// deleted) is not brought up again, nor her answer to it; what was only
/// corrected still is.
#[tokio::test]
async fn what_they_took_back_is_not_found() {
    let Some(schema) = schema("chat_near_withdrawn").await else {
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
    meaning_alike(&said, "香");
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
        [corrected.into(), newer.into()],
    ))
    .await
    .unwrap();

    let read: Vec<String> = private_lines(db, OWNER, 3000)
        .await
        .into_iter()
        .map(|line| line.content)
        .collect();
    assert_eq!(
        read,
        [said[2], said[3], said[4]],
        "the line and her answer go"
    );
    fill(db, OWNER).await;
    let found = nearest(db, OWNER, "香", 10, false).await;
    assert_eq!(texts(&found), [said[2], said[3], said[4]]);
    // Scrolling back by words reads the same lines.
    let by_words = super::super::remembering::look_back(db, OWNER, "香菜", "", 3).await;
    assert!(
        !texts(&by_words).iter().any(|text| text.contains("香菜")),
        "{by_words:?}"
    );
    g0::leave();
    schema.drop().await;
}

/// Whatever forgets a line takes its vector in the same statement, and a
/// line forgotten is never found: forgetting one line, forgetting a memory
/// (and the words it came from), and the account going.
#[tokio::test]
async fn forgetting_a_line_takes_its_vector() {
    let Some(schema) = schema("chat_near_forget").await else {
        return;
    };
    let db = &schema.db;
    let said = [
        "我的猫叫团子",
        "团子几岁了",
        "我下个月搬家",
        "搬去哪里呀",
        "我在学日语",
    ];
    meaning_alike(&said, "猫");
    session(db, "mine", OWNER, Some(r#"{"mode":"chat"}"#), false).await;
    let mut ids = Vec::new();
    for (index, text) in said.iter().enumerate() {
        let role = if index % 2 == 0 { "user" } else { "assistant" };
        ids.push(line(db, "mine", role, text, 60 - index as i32 * 5, None).await);
    }
    assert_eq!(fill(db, OWNER).await.given, 5);
    let vectors = |id: i32| {
        format!("SELECT COUNT(*) AS n FROM agent_message_embeddings WHERE message_id = {id}")
    };

    // One line forgotten by hand.
    crate::services::agent::memory::manage::forget_line(db, OWNER, "mine", ids[4])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(count(db, &vectors(ids[4])).await, 0);
    assert!(!texts(&nearest(db, OWNER, "猫", 10, false).await).contains(&said[4]));

    // A memory forgotten, with the line it came from and her answer.
    let memory = quoting(db, unified::SHORT_NOTE_SOURCE, "下个月搬家").await;
    assert!(
        crate::services::agent::memory::manage::forget_any(db, &memory)
            .await
            .unwrap()
    );
    assert_eq!(count(db, &vectors(ids[2])).await, 0);
    assert_eq!(count(db, &vectors(ids[3])).await, 0);
    let found = nearest(db, OWNER, "猫", 10, false).await;
    assert_eq!(texts(&found), [said[0], said[1]]);

    // A memory another end forgot, taken in here: the words it came from
    // go here too (`her_log::memories::take_in`).
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
    assert_eq!(went, [ids[0], ids[1]]);
    assert_eq!(count(db, &vectors(ids[0])).await, 0);
    assert!(nearest(db, OWNER, "猫", 10, false).await.is_empty());

    // The account gone, every vector with it.
    let last = line(db, "mine", "user", "还有一句", 1, None).await;
    g0::with(|world| world.vectors.insert("还有一句".into(), vec![1.0, 0.0, 0.0]));
    assert_eq!(fill(db, OWNER).await.given, 1);
    assert_eq!(count(db, &vectors(last)).await, 1);
    db.execute_unprepared("DELETE FROM users WHERE id = 1")
        .await
        .unwrap();
    assert_eq!(
        count(db, "SELECT COUNT(*) AS n FROM agent_message_embeddings").await,
        0
    );
    assert!(nearest(db, OWNER, "猫", 10, false).await.is_empty());
    g0::leave();
    schema.drop().await;
}

/// A line found by meaning that scrolling back showed cut is shown whole in
/// its place; one it did not show is added, oldest first.
#[test]
fn found_by_meaning_sits_beside_what_words_found() {
    let found = vec![
        (
            "2026-11-01".to_string(),
            Who::You,
            "… 第二步 / 第三步".to_string(),
        ),
        ("2026-11-03".to_string(), Who::They, "好的".to_string()),
    ];
    let near = vec![
        (
            "2026-11-01".to_string(),
            Who::You,
            "第一步 / 第二步 / 第三步".to_string(),
        ),
        (
            "2026-10-20".to_string(),
            Who::They,
            "更早的一句".to_string(),
        ),
    ];
    let all = beside(found, near);
    let read: Vec<&str> = all.iter().map(|(_, _, text)| text.as_str()).collect();
    assert_eq!(read, ["更早的一句", "第一步 / 第二步 / 第三步", "好的"]);
}

/// A line with nothing to embed is never asked for, and a whole rebuild
/// ends with every other line given a vector.
#[tokio::test]
async fn nothing_empty_is_asked_for_and_a_rebuild_ends() {
    let Some(schema) = schema("chat_near_empty").await else {
        return;
    };
    let db = &schema.db;
    // Only the line with words has a vector: asked for anything empty, the
    // model answers none and the whole request comes back without vectors.
    meaning_alike(&["我在学吉他"], "吉他");
    session(db, "mine", OWNER, Some(r#"{"mode":"chat"}"#), false).await;
    line(db, "mine", "user", "我在学吉他", 30, None).await;
    line(db, "mine", "assistant", "", 25, None).await;
    line(db, "mine", "user", "  \n ", 20, None).await;
    let given = tokio::time::timeout(std::time::Duration::from_secs(20), rebuild(db, OWNER))
        .await
        .expect("a rebuild ends");
    assert_eq!(given, 1);
    assert_eq!(
        count(db, "SELECT COUNT(*) AS n FROM agent_message_embeddings").await,
        1
    );
    assert_eq!(fill(db, OWNER).await, Filled::default(), "nothing left");
    g0::leave();
    schema.drop().await;
}

/// A line the model gives no vector (an empty one) is not kept, ends the
/// rebuild rather than being asked for again and again, and is not asked
/// for again by the next turn.
#[tokio::test]
async fn a_line_given_no_vector_ends_the_rebuild() {
    let Some(schema) = schema("chat_near_no_vector").await else {
        return;
    };
    let db = &schema.db;
    meaning_alike(&["我的猫叫团子"], "猫");
    g0::with(|world| world.vectors.insert("哈哈".into(), Vec::new()));
    session(db, "mine", OWNER, Some(r#"{"mode":"chat"}"#), false).await;
    line(db, "mine", "user", "我的猫叫团子", 30, None).await;
    line(db, "mine", "user", "哈哈", 25, None).await;
    let given = tokio::time::timeout(std::time::Duration::from_secs(20), rebuild(db, OWNER))
        .await
        .expect("a rebuild ends");
    assert_eq!(given, 1);
    assert_eq!(
        count(db, "SELECT COUNT(*) AS n FROM agent_message_embeddings").await,
        1,
        "no vector is kept for it"
    );
    assert_eq!(
        fill(db, OWNER).await,
        Filled::default(),
        "it is not asked for again"
    );
    g0::leave();
    schema.drop().await;
}

/// What a rebuild and a turn leave in the log is counts only: one line
/// each, a rebuild that finished too, and none of the words.
#[tokio::test]
async fn the_log_has_counts_and_none_of_the_words() {
    let Some(schema) = schema("chat_near_told").await else {
        return;
    };
    let db = &schema.db;
    meaning_alike(&["我的猫叫团子", "团子今天吐毛了"], "猫");
    g0::with(|world| world.vectors.insert("嗯嗯好的".into(), Vec::new()));
    session(db, "mine", OWNER, Some(r#"{"mode":"chat"}"#), false).await;
    line(db, "mine", "user", "我的猫叫团子", 30, None).await;
    line(db, "mine", "assistant", "团子今天吐毛了", 25, None).await;
    line(db, "mine", "user", "嗯嗯好的", 20, None).await;
    let logged = Logged::start();
    assert_eq!(rebuild(db, OWNER).await, 2);
    brought(db, OWNER, "猫", NEAREST, false).await;
    // No vector for what was asked: told apart from nothing near.
    assert!(brought(db, OWNER, "狗", NEAREST, false).await.is_empty());
    let log = logged.text();
    let rebuilt: Vec<&str> = log
        .lines()
        .filter(|line| line.contains("[chat-near] rebuilt"))
        .collect();
    assert_eq!(rebuilt.len(), 1, "{log}");
    assert!(rebuilt[0].contains("none left without a vector"), "{log}");
    for count in ["asks=2", "given=2", "left=0", "not_given=1"] {
        assert!(rebuilt[0].contains(count), "{count}: {log}");
    }
    let brought: Vec<&str> = log
        .lines()
        .filter(|line| line.contains("[chat-near] a private turn brought"))
        .collect();
    assert_eq!(brought.len(), 2, "{log}");
    assert!(brought[0].contains("lines=2"), "{log}");
    assert!(brought[0].contains("reached=looked"), "{log}");
    assert!(brought[1].contains("lines=0"), "{log}");
    assert!(brought[1].contains("reached=asked_unanswered"), "{log}");
    for words in ["猫", "团子", "吐毛", "嗯嗯"] {
        assert!(!log.contains(words), "{words} in the log: {log}");
    }
    drop(logged);
    g0::leave();
    schema.drop().await;
}

/// What this thread logs while it lives, as text (a test's runtime is its
/// own thread).
struct Logged {
    buffer: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    _guard: tracing::subscriber::DefaultGuard,
}

impl Logged {
    fn start() -> Self {
        #[derive(Clone)]
        struct Buffer(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl std::io::Write for Buffer {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let buffer = Buffer(Default::default());
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_max_level(tracing::Level::INFO)
            .with_ansi(false)
            .finish();
        Self {
            buffer: buffer.0,
            _guard: tracing::subscriber::set_default(subscriber),
        }
    }

    fn text(&self) -> String {
        String::from_utf8(self.buffer.lock().unwrap().clone()).unwrap()
    }
}
