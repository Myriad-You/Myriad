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
    // The later fact taken back too: what it replaced goes with it, as
    // forgetting takes a memory's whole line, so the line and her answer
    // are left out now.
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memories SET invalid_reason = 'deleted', invalid_at = now() WHERE id = $1",
        [newer.into()],
    ))
    .await
    .unwrap();
    let read = private_lines(db, OWNER, 3000).await;
    assert_eq!(contents(&read), [said[4]], "{read:?}");
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

async fn retire(db: &DatabaseConnection, id: &str, reason: &str, by: Option<&str>) {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memories SET invalid_reason = $2, superseded_by = $3 WHERE id = $1",
        [id.into(), reason.into(), by.map(str::to_string).into()],
    ))
    .await
    .unwrap();
}

/// A line is known as replaced however late she went over it: said three
/// days ago, noted then, gone over into a lasting fact only today, and
/// that fact replaced since. The fact was written days after the words;
/// the note it came from was not.
#[tokio::test]
async fn a_line_gone_over_days_later_is_still_known_as_replaced() {
    let Some(schema) = schema("chat_lines_gone_over_late").await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", OWNER, Some(r#"{"mode":"chat"}"#), false).await;
    let three_days = 3 * 24 * 60;
    line(db, "mine", "user", "我住在北京朝阳", three_days, None).await;
    line(
        db,
        "mine",
        "assistant",
        "朝阳那边挺方便",
        three_days - 1,
        None,
    )
    .await;
    line(db, "mine", "user", "周末想去爬香山", 5, None).await;
    let note = quoting(db, unified::SHORT_NOTE_SOURCE, "住在北京朝阳").await;
    let lasting = quoting(db, "chat", "我住在北京朝阳").await;
    let newer = quoting(db, "chat", "搬到北京海淀了").await;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memories SET created_at = NOW() - $2 * INTERVAL '1 minute' WHERE id = $1",
        [note.clone().into(), (three_days - 2).into()],
    ))
    .await
    .unwrap();
    retire(db, &note, "settled", Some(&lasting)).await;
    let flags = |read: Vec<Line>| read.iter().map(|line| line.replaced).collect::<Vec<_>>();
    assert_eq!(flags(private_lines(db, OWNER, 3000).await), [false; 3]);

    retire(db, &lasting, "superseded", Some(&newer)).await;
    let read = private_lines(db, OWNER, 3000).await;
    assert_eq!(contents(&read).len(), 3, "nothing is hidden for it");
    assert_eq!(flags(read), [true, false, false]);

    // What replaced it let go, with nothing in its place: taken back, and
    // what it replaced with it, and the note gone over into that. The line
    // and her answer are left out.
    retire(db, &newer, "superseded", None).await;
    let read = private_lines(db, OWNER, 3000).await;
    assert_eq!(contents(&read), ["周末想去爬香山"]);
    schema.drop().await;
}

/// A line's day is the day on her clock. The database gives the moment at
/// no offset, where what was said in her small hours is still the day
/// before; read so, a change of mind after midnight would be dated before
/// what it changed.
#[tokio::test]
async fn a_line_is_dated_on_her_clock() {
    use chrono::TimeZone;
    let zone = super::super::clock::Zone::Named(chrono_tz::Asia::Singapore);
    let small_hours = zone
        .with_ymd_and_hms(2026, 11, 10, 0, 30, 0)
        .unwrap()
        .with_timezone(&chrono::Utc)
        .fixed_offset();
    assert_eq!(small_hours.format("%Y-%m-%d").to_string(), "2026-11-09");
    let day = super::super::clock::in_zone(zone, async { super::day(&small_hours) }).await;
    assert_eq!(day, "2026-11-10");
    assert_eq!(day_of(&found_on(day.clone(), true)), day);
    assert_eq!(day_of(REPLACED_SINCE), REPLACED_SINCE);

    // And that is how the database gives it.
    let Some(schema) = schema("chat_lines_clock").await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", OWNER, Some(r#"{"mode":"chat"}"#), false).await;
    line(db, "mine", "user", "我在学吉他", 30, None).await;
    let read = private_lines(db, OWNER, 3000).await;
    assert_eq!(read[0].at.offset().local_minus_utc(), 0);
    schema.drop().await;
}

/// Lines are held against the memories written after them only, each
/// within its own reach: from a day before the first note it was gone over
/// from was taken (before it was written, for one gone over from none).
#[test]
fn each_memory_reaches_its_own_lines() {
    let at = |hours: i64| {
        chrono::DateTime::parse_from_rfc3339("2026-11-10T00:00:00+00:00").unwrap()
            + chrono::Duration::hours(hours)
    };
    let line = |id: i32, hours: i64, hers: bool, content: &str| Line {
        id,
        at: at(hours),
        hers,
        content: content.into(),
        replaced: false,
    };
    let lines = [
        line(1, 0, false, "我住在北京朝阳"),
        line(2, 1, true, "朝阳那边挺方便"),
        line(3, 60, false, "我住在北京朝阳 来着"),
        line(4, 200, false, "我住在北京朝阳"),
    ];
    let quoted = |written: i64, noted: i64, taken_back: bool| Quoted {
        quote: "住在北京朝阳".into(),
        written: at(written),
        noted: at(noted),
        taken_back,
        replaced: !taken_back,
    };
    // Replaced, written at 72h from a note at 2h: the lines from a day
    // before the note on, up to when it was written; theirs only.
    let (gone, replaced) = became(&lines, vec![quoted(72, 2, false)]);
    assert!(gone.is_empty());
    assert_eq!(replaced, HashSet::from([1, 3]));
    // Taken back, the same days: the same lines are gone, with her answer
    // to the first. Gone over days after the words, what was taken back
    // still does not come up again.
    let (gone, replaced) = became(&lines, vec![quoted(72, 2, true)]);
    assert_eq!(gone, HashSet::from([1, 2, 3]));
    // Gone over from no note: a day before it was written, as it was.
    assert!(replaced.is_empty());
    let (gone, replaced) = became(&lines, vec![quoted(72, 72, true)]);
    assert_eq!(gone, HashSet::from([3]));
    assert!(replaced.is_empty());
    // Both on one line: taken back wins, and her answer goes with it.
    let (gone, replaced) = became(&lines, vec![quoted(72, 2, false), quoted(3, 3, true)]);
    assert_eq!(gone, HashSet::from([1, 2]));
    assert_eq!(replaced, HashSet::from([3]));
}

/// A zone where the moment `at` falls on another day than the database
/// gives it (which gives it at no offset).
pub(in crate::services::agent::merope) fn another_day(
    at: chrono::DateTime<chrono::FixedOffset>,
) -> super::super::clock::Zone {
    use chrono::Timelike;
    super::super::clock::Zone::Named(if at.with_timezone(&chrono::Utc).hour() < 12 {
        chrono_tz::Etc::GMTPlus12
    } else {
        chrono_tz::Etc::GMTMinus12
    })
}

/// Scrolling back by words, a line of theirs a later fact replaced is read
/// as said, its day saying so; and its day is the day on her clock.
#[tokio::test]
async fn scrolling_back_a_replaced_line_says_so_beside_her_day() {
    let Some(schema) = schema("chat_lines_scrolled_marked").await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", OWNER, Some(r#"{"mode":"chat"}"#), false).await;
    let three_days = 3 * 24 * 60;
    line(db, "mine", "user", "我住在北京朝阳", three_days, None).await;
    line(
        db,
        "mine",
        "assistant",
        "朝阳那边挺方便",
        three_days - 1,
        None,
    )
    .await;
    // The talk still in front of her is not scrolled back to: enough said
    // since that those two are behind it.
    let in_front = crate::services::agent::sessions::PRIVATE_CHAT_IN_FRONT as i32 + 2;
    for index in 0..in_front {
        let role = if index % 2 == 0 { "user" } else { "assistant" };
        line(
            db,
            "mine",
            role,
            &format!("后来的第{index}句"),
            in_front - index,
            None,
        )
        .await;
    }
    let old = quoting(db, "chat", "住在北京朝阳").await;
    let newer = quoting(db, "chat", "搬到北京海淀了").await;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memories SET created_at = NOW() - $2 * INTERVAL '1 minute' WHERE id = $1",
        [old.clone().into(), (three_days - 2).into()],
    ))
    .await
    .unwrap();
    retire(db, &old, "superseded", Some(&newer)).await;

    let at = private_lines(db, OWNER, 3000).await[0].at;
    let zone = another_day(at);
    let hers = super::super::clock::in_zone(zone, async { super::day(&at) }).await;
    assert_ne!(hers, at.format("%Y-%m-%d").to_string());
    let look_back = super::super::remembering::look_back(db, OWNER, "住在北京朝阳", "", 3);
    let by_words = super::super::clock::in_zone(zone, look_back).await;
    let theirs = by_words
        .iter()
        .find(|(_, _, text)| text.contains("我住在北京朝阳"))
        .unwrap_or_else(|| panic!("{by_words:?}"));
    assert_eq!(theirs.0, found_on(hers.clone(), true));
    assert_eq!(theirs.2, "我住在北京朝阳", "as said");
    // Her answer below it: the same day, and nothing beside it.
    let answer = by_words
        .iter()
        .find(|(_, _, text)| text.contains("挺方便"))
        .unwrap_or_else(|| panic!("{by_words:?}"));
    assert_eq!(answer.0, hers);
    schema.drop().await;
}

/// A lasting fact taken back takes the words of the short notes it was
/// gone over from too: a note quotes its own line, which the fact's words
/// do not hold, and that line is theirs to have unsaid all the same. One
/// replaced by a later fact marks the note's line the same way.
#[tokio::test]
async fn what_a_note_quoted_goes_with_the_fact_it_was_gone_over_into() {
    let Some(schema) = schema("chat_lines_note_quotes").await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", OWNER, Some(r#"{"mode":"chat"}"#), false).await;
    let said = [
        "我真的讨厌香菜",
        "记住了，不放香菜",
        "说起来我不喜欢香菜好多年了",
        "周末想去爬香山",
    ];
    for (index, text) in said.iter().enumerate() {
        let role = if index == 1 { "assistant" } else { "user" };
        line(db, "mine", role, text, 60 - index as i32 * 5, None).await;
    }
    let note = quoting(db, unified::SHORT_NOTE_SOURCE, "真的讨厌香菜").await;
    let lasting = quoting(db, "chat", "不喜欢香菜").await;
    retire(db, &note, "settled", Some(&lasting)).await;
    assert_eq!(contents(&private_lines(db, OWNER, 3000).await), said);

    // Replaced by a later fact: both lines of theirs say so.
    let newer = quoting(db, "chat", "现在能吃一点香菜了").await;
    retire(db, &lasting, "superseded", Some(&newer)).await;
    let read = private_lines(db, OWNER, 3000).await;
    assert_eq!(contents(&read), said);
    assert_eq!(
        read.iter().map(|line| line.replaced).collect::<Vec<_>>(),
        [true, false, true, false]
    );
    // Let go, with nothing in its place: neither line comes up again, nor
    // her answer to the first.
    retire(db, &lasting, "superseded", None).await;
    assert_eq!(contents(&private_lines(db, OWNER, 3000).await), [said[3]]);
    schema.drop().await;
}

/// Taking back the last of a line of corrections takes back each it
/// corrected: 「周五去看牙」, replaced by 「改到周六看牙」, replaced by
/// 「改到周日看牙」, the last taken back. All three are taken back, as
/// her nodes read them too, and each line they quote is left out with
/// her answer.
#[tokio::test]
async fn taking_back_the_last_correction_takes_back_what_it_corrected() {
    let Some(schema) = schema("chat_lines_taken_back_line").await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", OWNER, Some(r#"{"mode":"chat"}"#), false).await;
    let said = [
        ("user", "周五去看牙"),
        ("assistant", "好，周五"),
        ("user", "改到周六看牙吧"),
        ("assistant", "周六也行"),
        ("user", "还是改到周日看牙"),
        ("assistant", "那就周日"),
        ("user", "今天下雨了"),
    ];
    for (index, (role, text)) in said.iter().enumerate() {
        line(db, "mine", role, text, 40 - index as i32 * 5, None).await;
    }
    let friday = quoting(db, "chat", "周五去看牙").await;
    let saturday = quoting(db, "chat", "改到周六看牙").await;
    let sunday = quoting(db, "chat", "改到周日看牙").await;
    retire(db, &friday, "superseded", Some(&saturday)).await;
    retire(db, &saturday, "superseded", Some(&sunday)).await;
    assert_eq!(
        contents(&private_lines(db, OWNER, 3000).await).len(),
        7,
        "replaced, not hidden"
    );
    retire(db, &sunday, "superseded", None).await;
    let named = crate::services::agent::merope::memory_nodes::known::named(
        db,
        vec![friday.clone(), saturday.clone(), sunday.clone()],
    )
    .await
    .unwrap();
    for id in [&friday, &saturday, &sunday] {
        assert!(named[id].taken_back, "{id}");
    }
    let read = private_lines(db, OWNER, 3000).await;
    assert_eq!(contents(&read), ["今天下雨了"], "{read:?}");
    schema.drop().await;
}
