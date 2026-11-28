//! What a memory of several lines came from, a few words of it said in many
//! lines, on a database of its own. Every word here is made up.

use super::*;
use crate::services::agent::memory::manage::{forget_all, forget_any, forget_line};
use crate::services::agent::merope::chat_lines::{
    private_lines,
    tests::{OTHER, line, schema, session},
};
use sea_orm::DatabaseConnection;

const CHAT: Option<&str> = Some(r#"{"mode":"chat"}"#);

/// A talk with '不是' said in four lines of it, theirs and hers, and the
/// memory `exam`, of the first two lines of theirs.
async fn talk(db: &DatabaseConnection) -> Vec<i32> {
    session(db, "mine", OTHER, CHAT, false).await;
    let mut ids = Vec::new();
    for (role, text, minutes) in [
        ("user", "我周五考试", 60),
        ("assistant", "是数学吗", 59),
        ("user", "不是", 58),
        ("assistant", "那是英语", 57),
        ("user", "那件事不是这样的", 50),
        ("assistant", "不是吧，真的吗", 49),
        ("user", "今天不是周三吗", 40),
    ] {
        ids.push(line(db, "mine", role, text, minutes, None).await);
    }
    db.execute_unprepared(
        "INSERT INTO agent_memories (id, user_id, kind, content, evidence, speaker, source, \
           venue, audience, concepts, importance, access_count, valid_from, created_at, updated_at) \
         VALUES ('exam', 7, 'fact', '周五考试，不是数学', '我周五考试 / 不是', 'user', 'chat', \
           'private', '[7]', '[]', 0.5, 0, NOW() - INTERVAL '60 minutes', \
           NOW() - INTERVAL '10 minutes', NOW())",
    )
    .await
    .unwrap();
    ids
}

async fn lines(db: &DatabaseConnection) -> Vec<String> {
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT content FROM agent_messages ORDER BY created_at",
    ))
    .await
    .unwrap()
    .iter()
    .map(|row| row.try_get("", "content").unwrap())
    .collect()
}

async fn kept(db: &DatabaseConnection) -> bool {
    db.query_one_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT 1 AS one FROM agent_memories WHERE id = 'exam'",
    ))
    .await
    .unwrap()
    .is_some()
}

/// Forgetting it takes the lines it came from with her answer, not every
/// line with a few words of it in it, theirs or hers.
#[tokio::test]
async fn forgetting_it_takes_no_other_line_with_a_few_of_its_words() {
    let Some(schema) = schema("origins_few").await else {
        return;
    };
    let db = &schema.db;
    talk(db).await;
    assert!(forget_any(db, "exam").await.unwrap());
    assert_eq!(
        lines(db).await,
        ["那件事不是这样的", "不是吧，真的吗", "今天不是周三吗"]
    );
    schema.drop().await;
}

/// One quoting a few words alone takes the line that says only them, no
/// other with them in it, and forgetting such a line leaves it.
#[tokio::test]
async fn one_quoting_a_few_words_alone_takes_no_other_line() {
    let Some(schema) = schema("origins_few_alone").await else {
        return;
    };
    let db = &schema.db;
    let ids = talk(db).await;
    db.execute_unprepared("UPDATE agent_memories SET evidence = '不是' WHERE id = 'exam'")
        .await
        .unwrap();
    assert!(
        forget_line(db, OTHER, "mine", ids[6])
            .await
            .unwrap()
            .is_some()
    );
    assert!(kept(db).await);
    assert!(forget_any(db, "exam").await.unwrap());
    assert_eq!(
        lines(db).await,
        [
            "我周五考试",
            "是数学吗",
            "那件事不是这样的",
            "不是吧，真的吗"
        ]
    );
    schema.drop().await;
}

/// One quoting a few words alone takes its line, and her diary of what that
/// line said: not what only has the same words in it.
#[tokio::test]
async fn a_few_words_take_what_says_only_them_in_her_diary() {
    let Some(schema) = schema("origins_few_known").await else {
        return;
    };
    let db = &schema.db;
    for known in [true, false] {
        db.execute_unprepared(
            "DELETE FROM agent_messages; DELETE FROM agent_sessions; DELETE FROM agent_diary",
        )
        .await
        .unwrap();
        let ids = talk(db).await;
        db.execute_unprepared(
            "UPDATE agent_memories SET evidence = '不是' WHERE id = 'exam'; \
             INSERT INTO agent_diary (id, user_id, content, source, created_at) VALUES \
               ('whole', 7, '不是', 'chat', NOW() - INTERVAL '30 minutes'), \
               ('part', 7, '说了不是数学', 'chat', NOW() - INTERVAL '30 minutes')",
        )
        .await
        .unwrap();
        if known {
            tell(db, &[ids[2]], "exam").await.unwrap();
        }
        assert!(forget_any(db, "exam").await.unwrap());
        let left = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT string_agg(id, ',') AS ids FROM agent_diary",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            left.try_get::<String>("", "ids").unwrap(),
            "part",
            "{known}"
        );
        assert!(!lines(db).await.contains(&"不是".to_string()));
    }
    schema.drop().await;
}

/// What they asked in a group, kept as of no place: forgetting it takes the
/// line there it is known to come from, and none in private.
#[tokio::test]
async fn what_they_asked_in_a_group_goes_with_its_line_there() {
    let Some(schema) = schema("origins_asked_group").await else {
        return;
    };
    let db = &schema.db;
    talk(db).await;
    session(
        db,
        "there",
        OTHER,
        Some(r#"{"mode":"chat","venue":"qq:42"}"#),
        false,
    )
    .await;
    let asking = line(db, "there", "user", "我周五考试的事别记了", 30, None).await;
    db.execute_unprepared(
        "UPDATE agent_memories SET source = 'asked', evidence = '我周五考试' WHERE id = 'exam'",
    )
    .await
    .unwrap();
    tell(db, &[asking], "exam").await.unwrap();
    db.execute_unprepared("UPDATE agent_memories SET created_at = NOW() WHERE id = 'exam'")
        .await
        .unwrap();
    let before = lines(db).await.len();
    assert!(forget_any(db, "exam").await.unwrap());
    let left = lines(db).await;
    assert_eq!(left.len(), before - 1, "{left:?}");
    assert!(left.contains(&"我周五考试".to_string()));
    schema.drop().await;
}

/// Forgetting another line with a few of its words in it leaves it, and the
/// lines it came from.
#[tokio::test]
async fn forgetting_another_line_with_a_few_of_its_words_leaves_it() {
    let Some(schema) = schema("origins_few_line").await else {
        return;
    };
    let db = &schema.db;
    let ids = talk(db).await;
    for other in [ids[4], ids[6]] {
        assert!(
            forget_line(db, OTHER, "mine", other)
                .await
                .unwrap()
                .is_some()
        );
    }
    assert!(kept(db).await);
    let left = lines(db).await;
    assert!(left.contains(&"我周五考试".to_string()) && left.contains(&"不是".to_string()));
    // One it came from takes it.
    assert!(
        forget_line(db, OTHER, "mine", ids[0])
            .await
            .unwrap()
            .is_some()
    );
    assert!(!kept(db).await);
    schema.drop().await;
}

/// Taken back, the lines it came from are not looked back to, with her
/// answer; another with a few of its words in it is.
#[tokio::test]
async fn taken_back_it_hides_no_other_line_with_a_few_of_its_words() {
    let Some(schema) = schema("origins_few_back").await else {
        return;
    };
    let db = &schema.db;
    talk(db).await;
    db.execute_unprepared(
        "UPDATE agent_memories SET invalid_at = NOW(), invalid_reason = 'superseded' \
         WHERE id = 'exam'",
    )
    .await
    .unwrap();
    let said: Vec<String> = private_lines(db, OTHER, 100)
        .await
        .into_iter()
        .map(|line| line.content)
        .collect();
    assert_eq!(
        said,
        ["那件事不是这样的", "不是吧，真的吗", "今天不是周三吗"]
    );
    schema.drop().await;
}

/// Known to come from a line, it goes with that line and no other saying
/// the very same.
#[tokio::test]
async fn known_to_come_from_a_line_it_goes_with_that_one_alone() {
    let Some(schema) = schema("origins_told").await else {
        return;
    };
    let db = &schema.db;
    let ids = talk(db).await;
    let again = line(db, "mine", "user", "我周五考试", 30, None).await;
    tell(db, &[ids[0], ids[2]], "exam").await.unwrap();
    // Noted once, and beside what else is noted of the line.
    tell(db, &[ids[0]], "exam").await.unwrap();
    let noted: String = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT metadata::text AS noted FROM agent_messages WHERE id = $1",
            [ids[0].into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "noted")
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&noted).unwrap(),
        serde_json::json!({ "told": ["exam"] })
    );
    assert!(
        forget_line(db, OTHER, "mine", again)
            .await
            .unwrap()
            .is_some()
    );
    assert!(kept(db).await);
    let again = line(db, "mine", "user", "我周五考试", 30, None).await;
    assert!(forget_any(db, "exam").await.unwrap());
    let left = lines(db).await;
    assert!(!left.contains(&"不是".to_string()));
    assert_eq!(left.iter().filter(|text| *text == "我周五考试").count(), 1);
    let _ = again;
    schema.drop().await;
}

/// A long line is queued a part at a time: forgotten, no part of it stays
/// queued; another's part does.
#[tokio::test]
async fn forgetting_a_long_line_takes_each_part_of_it_still_queued() {
    let Some(schema) = schema("origins_long").await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", OTHER, CHAT, false).await;
    let text: String = (0..400).map(|n| format!("第{n}件小事说给你听。")).collect();
    let parts: Vec<String> = myriad_merope::settle_raw::line_parts(&text)
        .into_iter()
        .map(str::to_string)
        .collect();
    assert!(parts.len() > 2);
    let id = line(db, "mine", "user", &text, 5, None).await;
    for (nth, part) in parts
        .iter()
        .map(String::as_str)
        .chain(["别的一句话，说了点别的"])
        .enumerate()
    {
        let payload = serde_json::json!({ "owner": OTHER, "data": { "Chat": { "user_text": part, "reply": "" } } });
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO runtime_registry (namespace, record_id, owner_id, payload, expires_at) \
             VALUES ('merope_memory_jobs', $1, 7, $2::jsonb, 9999999999)",
            [format!("part-{nth}").into(), payload.into()],
        ))
        .await
        .unwrap();
    }
    assert!(forget_line(db, OTHER, "mine", id).await.unwrap().is_some());
    let queued: Vec<String> = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT record_id FROM runtime_registry",
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get("", "record_id").unwrap())
        .collect();
    assert_eq!(queued, [format!("part-{}", parts.len())]);
    schema.drop().await;
}

/// Lines said one after another are queued as one, a line each: forgotten,
/// a short one is not kept through that, however it ends; what only has its
/// words in it is.
#[tokio::test]
async fn forgetting_a_short_line_takes_what_it_is_queued_in() {
    let Some(schema) = schema("origins_folded").await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", OTHER, CHAT, false).await;
    let id = line(db, "mine", "user", "我怀孕了", 5, None).await;
    for (record, text, reply) in [
        ("folded", "我怀孕了\n今天天气不错", ""),
        ("last", "今天天气不错\n我怀孕了。", ""),
        ("answer", "今天天气不错", "我怀孕了"),
        ("other", "今天天气不错\n她说我怀孕了会很累", ""),
    ] {
        let payload = serde_json::json!({ "owner": OTHER, "data": { "Chat": { "user_text": text, "reply": reply } } });
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO runtime_registry (namespace, record_id, owner_id, payload, expires_at) \
             VALUES ('merope_memory_jobs', $1, 7, $2::jsonb, 9999999999)",
            [record.into(), payload.into()],
        ))
        .await
        .unwrap();
    }
    assert!(forget_line(db, OTHER, "mine", id).await.unwrap().is_some());
    let queued: Vec<String> = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT record_id FROM runtime_registry",
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get("", "record_id").unwrap())
        .collect();
    assert_eq!(queued, ["other"]);
    schema.drop().await;
}

/// What quotes nothing a line is found by goes by the line it was told in:
/// taken back, that line is not looked back to; that line forgotten, it is.
#[tokio::test]
async fn what_quotes_nothing_goes_by_the_line_it_was_told_in() {
    let Some(schema) = schema("origins_no_quote").await else {
        return;
    };
    let db = &schema.db;
    for (evidence, said) in [("对", "对"), ("[图片] 这是我家的猫", "[图片] 这是我家的猫")]
    {
        let ids = talk(db).await;
        let yes = line(db, "mine", "user", said, 30, None).await;
        let other = line(db, "mine", "user", "今天好累", 20, None).await;
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_memories SET evidence = $1 WHERE id = 'exam'",
            [evidence.into()],
        ))
        .await
        .unwrap();
        tell(db, &[yes], "exam").await.unwrap();
        db.execute_unprepared(
            "UPDATE agent_memories SET invalid_at = NOW(), invalid_reason = 'superseded' \
             WHERE id = 'exam'",
        )
        .await
        .unwrap();
        let read: Vec<i32> = private_lines(db, OTHER, 100)
            .await
            .into_iter()
            .map(|line| line.id)
            .collect();
        assert!(!read.contains(&yes), "{said}");
        assert!(read.contains(&other) && read.contains(&ids[2]), "{said}");
        db.execute_unprepared(
            "UPDATE agent_memories SET invalid_at = NULL, invalid_reason = NULL WHERE id = 'exam'",
        )
        .await
        .unwrap();
        assert!(forget_line(db, OTHER, "mine", yes).await.unwrap().is_some());
        assert!(!kept(db).await, "{said}");
        db.execute_unprepared("DELETE FROM agent_messages; DELETE FROM agent_sessions")
            .await
            .unwrap();
    }
    schema.drop().await;
}

/// What is noted of a line meanwhile stays beside what it taught her.
#[tokio::test]
async fn telling_keeps_what_is_noted_of_the_line_meanwhile() {
    use sea_orm::TransactionTrait;
    let Some(schema) = schema("origins_meanwhile").await else {
        return;
    };
    let db = &schema.db;
    let ids = talk(db).await;
    let first = ids[0];
    let meanwhile = db.begin().await.unwrap();
    meanwhile
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_messages SET metadata = '{\"notKept\": \"true\"}' WHERE id = $1",
            [first.into()],
        ))
        .await
        .unwrap();
    let telling = {
        let db = db.clone();
        tokio::spawn(async move { tell(&db, &[first], "exam").await })
    };
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    meanwhile.commit().await.unwrap();
    telling.await.unwrap().unwrap();
    let noted: String = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT metadata::text AS noted FROM agent_messages WHERE id = $1",
            [first.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "noted")
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&noted).unwrap(),
        serde_json::json!({ "notKept": "true", "told": ["exam"] })
    );
    schema.drop().await;
}

/// Two that quote the same few words go together: the line that says only
/// them goes, and gone for the one is not taken for the other's to be in
/// another line. Forgetting that line, too.
#[tokio::test]
async fn two_that_quote_the_same_few_words_take_one_line() {
    let Some(schema) = schema("origins_few_twice").await else {
        return;
    };
    let db = &schema.db;
    for by in ["one", "both", "line"] {
        db.execute_unprepared(
            "DELETE FROM agent_messages; DELETE FROM agent_sessions; DELETE FROM agent_memories",
        )
        .await
        .unwrap();
        session(db, "mine", OTHER, CHAT, false).await;
        let said = line(db, "mine", "user", "离婚了", 60, None).await;
        line(db, "mine", "assistant", "怎么突然", 59, None).await;
        line(db, "mine", "user", "你怎么知道我离婚了", 50, None).await;
        line(db, "mine", "assistant", "你说过的", 49, None).await;
        // The note, and what it was kept as; or two kept of one line.
        for (id, holds) in [("kept", "NULL"), ("note", "'kept'")] {
            let by = if by == "one" { holds } else { "NULL" };
            let gone = if by == "NULL" { "NULL" } else { "NOW()" };
            db.execute_unprepared(&format!(
                "INSERT INTO agent_memories (id, user_id, kind, content, evidence, speaker, \
                   source, venue, audience, concepts, importance, access_count, valid_from, \
                   created_at, updated_at, superseded_by, invalid_at) \
                 VALUES ('{id}', 7, 'fact', '{id}：离婚了', '离婚了', 'user', 'chat', 'private', \
                   '[7]', '[]', 0.5, 0, NOW() - INTERVAL '60 minutes', \
                   NOW() - INTERVAL '10 minutes', NOW(), {by}, {gone})"
            ))
            .await
            .unwrap();
        }
        assert!(match by {
            "one" => forget_any(db, "kept").await.unwrap(),
            "both" => {
                let both = ["kept".to_string(), "note".to_string()];
                forget_all(db, &both).await.unwrap()
            }
            _ => {
                let gone = forget_line(db, OTHER, "mine", said).await.unwrap();
                gone.is_some()
            }
        });
        assert_eq!(lines(db).await, ["你怎么知道我离婚了", "你说过的"], "{by}");
        let left = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT id FROM agent_memories",
            ))
            .await
            .unwrap();
        assert!(left.is_empty(), "{by}");
    }
    schema.drop().await;
}

/// A line forgotten takes what was said right after it, when that is hers:
/// not her answer to what they said next.
#[tokio::test]
async fn a_line_forgotten_takes_no_answer_to_another() {
    let Some(schema) = schema("origins_no_answer").await else {
        return;
    };
    let db = &schema.db;
    talk(db).await;
    let unanswered = line(db, "mine", "user", "猫又跑出去了", 30, None).await;
    line(db, "mine", "user", "晚上吃什么好呢", 29, None).await;
    line(db, "mine", "assistant", "火锅吧", 28, None).await;
    db.execute_unprepared("UPDATE agent_memories SET evidence = '猫又跑出去了' WHERE id = 'exam'")
        .await
        .unwrap();
    tell(db, &[unanswered], "exam").await.unwrap();
    assert!(forget_any(db, "exam").await.unwrap());
    let left = lines(db).await;
    assert!(!left.contains(&"猫又跑出去了".to_string()), "{left:?}");
    assert!(left.contains(&"火锅吧".to_string()), "{left:?}");
    schema.drop().await;
}

/// A memory of `user_id`'s in `venue` (`of`) from `source`, quoting
/// `evidence`, written `minutes_ago`, known to come from `lines`.
async fn kept_of(
    db: &DatabaseConnection,
    id: &str,
    (user_id, venue): (i32, &str),
    source: &str,
    evidence: &str,
    minutes_ago: i32,
    lines: &[i32],
) {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_memories (id, user_id, kind, content, evidence, speaker, source, \
           venue, audience, concepts, importance, access_count, valid_from, created_at, \
           updated_at) \
         VALUES ($1, $2, 'fact', $1, $3, 'agent', $4, $5, jsonb_build_array($2), '[]', 0.4, 0, \
           NOW() - $6 * INTERVAL '1 minute', NOW() - $6 * INTERVAL '1 minute', NOW())",
        [
            id.into(),
            user_id.into(),
            evidence.into(),
            source.into(),
            venue.into(),
            minutes_ago.into(),
        ],
    ))
    .await
    .unwrap();
    tell(db, lines, id).await.unwrap();
}

async fn memories(db: &DatabaseConnection) -> Vec<String> {
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT id FROM agent_memories ORDER BY id",
    ))
    .await
    .unwrap()
    .iter()
    .map(|row| row.try_get("", "id").unwrap())
    .collect()
}

async fn lines_of(db: &DatabaseConnection, session: &str) -> Vec<String> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT content FROM agent_messages WHERE session_id = $1 ORDER BY created_at",
        [session.into()],
    ))
    .await
    .unwrap()
    .iter()
    .map(|row| row.try_get("", "content").unwrap())
    .collect()
}

/// What she told them in her answer to a line that goes, kept as what she
/// said (`said`), goes with it, as she forgets any memory: by forgetting a
/// memory of that line, the line itself, or the line that taught her
/// nothing. What she said the same way in another answer still here stays,
/// as does what she said in a group or to someone else.
#[tokio::test]
async fn what_she_said_in_her_answer_goes_with_their_line() {
    let Some(schema) = schema("origins_said_answer").await else {
        return;
    };
    let db = &schema.db;
    for by in ["memory", "line", "line alone"] {
        db.execute_unprepared(
            "DELETE FROM agent_messages; DELETE FROM agent_sessions; \
             DELETE FROM agent_memories; DELETE FROM agent_forgotten",
        )
        .await
        .unwrap();
        session(db, "mine", OTHER, CHAT, false).await;
        let said = line(db, "mine", "user", "我上个月确诊了糖尿病", 60, None).await;
        let answer = line(db, "mine", "assistant", "那你记得每天按时吃药", 59, None).await;
        line(db, "mine", "user", "今天天气不错", 50, None).await;
        line(db, "mine", "assistant", "适合出去走走", 49, None).await;
        line(db, "mine", "user", "我又忘了吃药", 30, None).await;
        let again = line(db, "mine", "assistant", "那你记得每天按时吃药", 29, None).await;
        let group = r#"{"mode":"chat","venue":"qq:42"}"#;
        session(db, "there", OTHER, Some(group), false).await;
        let there = line(db, "there", "assistant", "那你记得每天按时吃药", 59, None).await;
        session(db, "theirs", 1, CHAT, false).await;
        let theirs = line(db, "theirs", "assistant", "那你记得每天按时吃药", 59, None).await;
        if by == "memory" || by == "line" {
            let evidence = "我上个月确诊了糖尿病";
            kept_of(db, "diagnosis", (OTHER, "private"), "chat", evidence, 58, &[said]).await;
        }
        let meds = "记得每天按时吃药";
        kept_of(db, "meds", (OTHER, "private"), "said", meds, 55, &[answer]).await;
        kept_of(db, "again", (OTHER, "private"), "said", meds, 25, &[again]).await;
        kept_of(db, "group", (OTHER, "group:qq:42"), "said", meds, 55, &[there]).await;
        kept_of(db, "theirs", (1, "private"), "said", meds, 55, &[theirs]).await;
        if by == "memory" {
            assert!(forget_any(db, "diagnosis").await.unwrap());
        } else {
            let gone = forget_line(db, OTHER, "mine", said).await.unwrap().unwrap();
            let gone: Vec<i32> = gone.iter().map(|line| line.id).collect();
            assert!(gone.contains(&said) && gone.contains(&answer), "{by}: {gone:?}");
        }
        assert_eq!(memories(db).await, ["again", "group", "theirs"], "{by}");
        assert_eq!(
            lines_of(db, "mine").await,
            ["今天天气不错", "适合出去走走", "我又忘了吃药", "那你记得每天按时吃药"],
            "{by}"
        );
        assert_eq!(lines_of(db, "there").await.len(), 1, "{by}");
        // Kept as forgotten, as every memory forgotten is.
        let marked = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT 1 AS one FROM agent_forgotten WHERE what = 'memory' AND mark = 'meds'",
            ))
            .await
            .unwrap();
        assert!(marked.is_some(), "{by}");
    }
    schema.drop().await;
}

/// What she said in several lines goes when any of them does, and with it
/// each of them, as a memory forgotten takes the lines it came from.
#[tokio::test]
async fn what_she_said_in_several_lines_goes_when_any_of_them_does() {
    let Some(schema) = schema("origins_said_several").await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", OTHER, CHAT, false).await;
    let said = line(db, "mine", "user", "我上个月确诊了糖尿病", 60, None).await;
    let answer = line(db, "mine", "assistant", "那你记得每天按时吃药", 59, None).await;
    line(db, "mine", "user", "好的我会的", 58, None).await;
    let later = line(db, "mine", "assistant", "吃完药告诉我一声", 57, None).await;
    let evidence = "记得每天按时吃药 / 吃完药告诉我一声";
    kept_of(db, "meds", (OTHER, "private"), "said", evidence, 55, &[answer, later]).await;
    assert!(forget_line(db, OTHER, "mine", said).await.unwrap().is_some());
    assert!(memories(db).await.is_empty());
    assert_eq!(lines_of(db, "mine").await, ["好的我会的"]);
    schema.drop().await;
}
