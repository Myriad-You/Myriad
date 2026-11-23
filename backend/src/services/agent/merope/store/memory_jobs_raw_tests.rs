//! A private talk's stretch: due once its box is full, and gone over now
//! when another part of her needs it.
use super::*;
use crate::services::agent::memory::unified;
use crate::services::agent::merope::chat_lines::{
    self,
    tests::{line as message, session},
};
use crate::services::agent::merope::chat_remember::ChatMemoryUpdates;
use crate::services::agent::merope::chat_remember::settle_raw::Read;

type At = chrono::DateTime<chrono::FixedOffset>;

fn line(present: Audience, at: chrono::DateTime<chrono::FixedOffset>, answered: bool) -> Payload {
    Payload::Settle {
        present,
        from: at,
        last: at,
        heard: vec![(at, answered)],
    }
}

/// Their lines and her answers, as the talk goes: `exchanges` lines, each
/// answered.
async fn talk(
    db: &DatabaseConnection,
    id: &str,
    present: Audience,
    began: chrono::DateTime<chrono::FixedOffset>,
    exchanges: i64,
) {
    for index in 0..exchanges {
        let at = began + chrono::Duration::seconds(index * 20);
        // As it lands, and again once she has answered.
        enqueue(db, id, 7, line(present.clone(), at, false), None)
            .await
            .unwrap();
        enqueue(db, id, 7, line(present.clone(), at, true), None)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn a_private_talk_is_gone_over_once_twelve_lines_wait() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let began = crate::clock::now().fixed_offset() - chrono::Duration::minutes(2);
    // Eleven: their sixth line landed, not yet answered.
    talk(db, "settle", Audience::private(7), began, 5).await;
    let sixth = began + chrono::Duration::seconds(100);
    enqueue(
        db,
        "settle",
        7,
        line(Audience::private(7), sixth, false),
        None,
    )
    .await
    .unwrap();
    let job = loaded(db, "settle").await;
    assert_eq!(job.data.as_ref().unwrap().waiting(), 11);
    assert!(
        job.ready > job.last_arrival,
        "eleven wait for them to go quiet"
    );
    // The same line held twice is not counted twice.
    enqueue(
        db,
        "settle",
        7,
        line(Audience::private(7), sixth, false),
        None,
    )
    .await
    .unwrap();
    assert_eq!(loaded(db, "settle").await.data.unwrap().waiting(), 11);
    // Her answer to it: twelve, gone over now.
    enqueue(
        db,
        "settle",
        7,
        line(Audience::private(7), sixth, true),
        None,
    )
    .await
    .unwrap();
    let job = loaded(db, "settle").await;
    assert_eq!(job.data.as_ref().unwrap().waiting(), 12);
    assert!(job.ready <= job.last_arrival, "twelve: at once");
    let claimed = claim(db).await.unwrap().expect("due");
    // What they say while it is gone over waits for the next, never lost.
    let later = sixth + chrono::Duration::seconds(20);
    enqueue(
        db,
        "settle",
        7,
        line(Audience::private(7), later, false),
        None,
    )
    .await
    .unwrap();
    finish(db, &claimed, Ok(Effect::NoChange)).await.unwrap();
    let job = loaded(db, "settle").await;
    assert!(matches!(job.data, Some(Payload::Settle { from, .. }) if from == later));
    assert_eq!(job.data.unwrap().waiting(), 1);
    schema.drop().await;
}

/// A group is gone over from its short notes, as before: a full box does
/// not hurry it.
#[tokio::test]
async fn a_group_waits_for_them_to_go_quiet_however_many_lines() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let began = crate::clock::now().fixed_offset() - chrono::Duration::minutes(2);
    talk(db, "group", Audience::group("g1", 7), began, 8).await;
    let job = loaded(db, "group").await;
    assert_eq!(job.data.as_ref().unwrap().waiting(), 16);
    assert!(job.ready > job.last_arrival);
    schema.drop().await;
}

#[tokio::test]
async fn another_part_of_her_hurries_a_talk_only_when_lines_of_theirs_wait() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    // Nothing waiting: nothing to hurry.
    assert_eq!(hurry(db, "settle", 7).await.unwrap(), None);
    let began = crate::clock::now().fixed_offset() - chrono::Duration::minutes(1);
    talk(db, "settle", Audience::private(7), began, 1).await;
    assert!(loaded(db, "settle").await.ready > loaded(db, "settle").await.last_arrival);
    // Another's job is not hurried for this one.
    assert_eq!(hurry(db, "settle", 8).await.unwrap(), None);
    assert_eq!(hurry(db, "settle", 7).await.unwrap(), Some(began));
    let job = loaded(db, "settle").await;
    assert!(job.ready <= job.last_arrival + 1, "due now");
    assert_eq!(
        waiting_for(db, "settle", began).await.unwrap(),
        WaitingFor::Waiting
    );
    // Taken up and gone over: no longer waiting; hurrying again finds
    // nothing.
    let claimed = claim(db).await.unwrap().expect("due");
    assert_eq!(
        hurry(db, "settle", 7).await.unwrap(),
        Some(began),
        "being gone over: waited on as it is"
    );
    assert_eq!(
        waiting_for(db, "settle", began).await.unwrap(),
        WaitingFor::Waiting
    );
    finish(db, &claimed, Ok(Effect::NoChange)).await.unwrap();
    assert_eq!(
        waiting_for(db, "settle", began).await.unwrap(),
        WaitingFor::Over
    );
    assert_eq!(hurry(db, "settle", 7).await.unwrap(), None);
    schema.drop().await;
}

/// A stretch that failed keeps its wait before the next try: hurrying it
/// does not try a connection that just dropped again at once.
#[tokio::test]
async fn a_failed_stretch_keeps_its_wait_when_hurried() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let began = crate::clock::now().fixed_offset() - chrono::Duration::minutes(1);
    talk(db, "settle", Audience::private(7), began, 1).await;
    due(db, "settle").await;
    let claimed = claim(db).await.unwrap().expect("due");
    finish(
        db,
        &claimed,
        Err(memory_jobs::Failure::Model(call::Failure::Timeout)),
    )
    .await
    .unwrap();
    let waits = loaded(db, "settle").await.ready;
    assert_eq!(hurry(db, "settle", 7).await.unwrap(), Some(began));
    assert_eq!(loaded(db, "settle").await.ready, waits);
    schema.drop().await;
}

async fn said_at(db: &DatabaseConnection, id: i32) -> At {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT created_at FROM agent_messages WHERE id = $1",
        [id.into()],
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get("", "created_at")
    .unwrap()
}

/// Going over gives `fact` (none: nothing), of the lines `read`, and lets
/// `let_go` go.
fn from_lines(fact: Option<&str>, read: &[(At, i32)], let_go: &[i32]) -> Effect {
    let updates: Vec<ChatMemoryUpdate> = fact
        .into_iter()
        .map(|fact| ChatMemoryUpdate {
            standing: Default::default(),
            topic: None,
            cues: Vec::new(),
            fact: Some(fact.into()),
            supersedes: vec![],
            evidence: Some(fact.into()),
            concepts: vec![],
        })
        .collect();
    Effect::SettledFromLines {
        settled: vec![Vec::new(); updates.len()],
        updates: ChatMemoryUpdates {
            updates,
            ..Default::default()
        },
        rest: None,
        read: read.to_vec(),
        partway: None,
        let_go: let_go.to_vec(),
    }
}

async fn settle_from(db: &DatabaseConnection, from: At) -> Claim {
    enqueue(
        db,
        "settle",
        7,
        line(Audience::private(7), from, true),
        None,
    )
    .await
    .unwrap();
    due(db, "settle").await;
    claim(db).await.unwrap().expect("due")
}

/// A line forgotten while the stretch it is of is gone over: nothing of
/// that answer is written, and the stretch is gone over again without it.
#[tokio::test]
async fn a_line_forgotten_while_it_is_gone_over_is_not_written_from() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", 7, Some(r#"{"mode":"chat"}"#), false).await;
    let cat = message(db, "mine", "user", "养了一只猫叫年糕", 5, None).await;
    let flat = message(db, "mine", "user", "下周要搬家", 4, None).await;
    let (cat, flat) = (
        (said_at(db, cat).await, cat),
        (said_at(db, flat).await, flat),
    );
    let claimed = settle_from(db, cat.0).await;
    let note = unified::remember(
        db,
        unified::NewMemory {
            user_id: 7,
            kind: unified::MemoryKind::Fact,
            content: "猫叫年糕".into(),
            evidence: Some("养了一只猫叫年糕".into()),
            speaker: unified::Speaker::User,
            source: unified::SHORT_NOTE_SOURCE,
            audience: Audience::private(7),
            importance: 0.5,
            concepts: Vec::new(),
        },
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        crate::services::agent::memory::manage::forget_any(db, &note)
            .await
            .unwrap()
    );
    finish(
        db,
        &claimed,
        Ok(from_lines(Some("养了一只猫叫年糕"), &[cat, flat], &[])),
    )
    .await
    .unwrap();
    let job = loaded(db, "settle").await;
    assert!(job.data.is_some(), "gone over again");
    assert!(job.read.is_none(), "nothing of it read");
    assert_eq!(facts(db).await, 0);
    due(db, "settle").await;
    let again = claim(db).await.unwrap().expect("again");
    finish(db, &again, Ok(from_lines(Some("下周要搬家"), &[flat], &[])))
        .await
        .unwrap();
    assert_eq!(facts(db).await, 1);
    assert!(loaded(db, "settle").await.read.unwrap().has(flat.1));
    schema.drop().await;
}

/// What they took back in the stretch they said it in is not looked back
/// to, nor her answer to it, with what the stretch keeps or with nothing;
/// nothing of it is forgotten.
#[tokio::test]
async fn a_line_let_go_in_its_stretch_is_not_brought_up_again() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", 7, Some(r#"{"mode":"chat"}"#), false).await;
    let quit = message(db, "mine", "user", "我其实想辞职", 9, Some(r#"{"a":1}"#)).await;
    let hers = message(db, "mine", "assistant", "怎么了", 8, None).await;
    let back = message(db, "mine", "user", "算了当我没说", 7, None).await;
    let kept = message(db, "mine", "user", "下周要搬家", 6, None).await;
    // Kept with nothing, as JSON says it.
    db.execute_unprepared(&format!(
        "UPDATE agent_messages SET metadata = 'null'::json WHERE id = {back}"
    ))
    .await
    .unwrap();
    let mut read = Vec::new();
    for id in [quit, back, kept] {
        read.push((said_at(db, id).await, id));
    }
    let claimed = settle_from(db, read[0].0).await;
    finish(db, &claimed, Ok(from_lines(None, &read, &[quit, back])))
        .await
        .unwrap();
    let left: Vec<i32> = chat_lines::private_lines(db, 7, 100)
        .await
        .iter()
        .map(|line| line.id)
        .collect();
    assert_eq!(left, [kept]);
    let went = chat_lines::taken_back_with(db, 7, |_| true).await.unwrap();
    assert_eq!(went, [quit, hers, back].into());
    // As no memory's.
    let of_memories = chat_lines::taken_back_with(db, 7, |memory| !memory.is_empty())
        .await
        .unwrap();
    assert!(of_memories.is_empty());
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT content, metadata->>'a' AS a FROM agent_messages WHERE id = $1",
            [quit.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.try_get::<String>("", "content").unwrap(),
        "我其实想辞职"
    );
    assert_eq!(row.try_get::<String>("", "a").unwrap(), "1");
    schema.drop().await;
}

/// Lines sent back to be written one by one are those not gone over, and
/// are gone over: none is gone over again with what comes after, and what
/// comes after is not held up by one of them.
#[tokio::test]
async fn lines_sent_back_one_by_one_are_gone_over_once() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", 7, Some(r#"{"mode":"chat"}"#), false).await;
    let mut ids = Vec::new();
    for (role, text, minutes, metadata) in [
        ("user", "上一段说完了的话", 62, None),
        ("user", "周末要搬家", 60, None),
        (
            "assistant",
            "没发出去的回答",
            39,
            Some(r#"{"cutOff":"unsent"}"#),
        ),
        ("user", "在电脑上说的话", 39, Some(r#"{"leftForHer":true}"#)),
        ("user", "最近在学吉他", 40, Some(r#"{"inGame":true}"#)),
        ("user", "明天要考试了", 20, None),
    ] {
        let id = message(db, "mine", role, text, minutes, metadata).await;
        ids.push((said_at(db, id).await, id));
    }
    // The first gone over before.
    let claimed = settle_from(db, ids[0].0).await;
    finish(db, &claimed, Ok(from_lines(None, &ids[..1], &[])))
        .await
        .unwrap();
    // Each begins with its own first line.
    for began in [ids[1].0, ids[4].0, ids[5].0] {
        settle::unread::never_read(db, settle_from(db, began).await).await;
    }
    let mut queued = Vec::new();
    for (at, id) in &ids {
        let key = memory_jobs::key(&["chat", "7", "private", &at.timestamp_micros().to_string()]);
        if registry::get::<Job>(db, NAMESPACE, &key)
            .await
            .unwrap()
            .is_some()
        {
            queued.push(*id);
        }
    }
    assert_eq!(queued, [ids[1].1, ids[4].1], "each of theirs, once");
    // With no answer of hers that was never sent, and each a line of a game
    // or not as it was.
    for (nth, in_game) in [(1, false), (4, true)] {
        let at = ids[nth].0;
        let key = memory_jobs::key(&["chat", "7", "private", &at.timestamp_micros().to_string()]);
        let Some(Payload::Chat { reply, turn, .. }) = loaded(db, &key).await.data else {
            panic!("written down one by one");
        };
        assert_eq!((reply.as_str(), turn.in_game), ("", in_game));
    }
    let job = loaded(db, "settle").await;
    let read = job.read.clone().unwrap();
    assert!(read.has(ids[4].1) && !read.has(ids[5].1));
    // A draft never sent is not among what was read.
    assert!(!read.has(ids[2].1) && read.has(ids[3].1));
    // What comes after is read from after them on.
    let from = job.unread_from.unwrap();
    assert!(Read::looks_from(Some(&read), from) > ids[1].0);
    schema.drop().await;
}

/// Let go unread after its last try, a hurried stretch is not in time, with
/// what it brought along or not; brought along by the next, it is waited on
/// with it.
#[tokio::test]
async fn a_hurried_stretch_left_unread_is_not_gone_over() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let ago = |minutes| crate::clock::now().fixed_offset() - chrono::Duration::minutes(minutes);
    let fares = async |from| waiting_for(db, "settle", from).await.unwrap();
    let (first, second) = (ago(60), ago(30));
    settle::unread::never_read(db, settle_from(db, first).await).await;
    assert_eq!(fares(first).await, WaitingFor::Unread);
    let claimed = settle_from(db, second).await;
    assert_eq!(fares(first).await, WaitingFor::Waiting, "brought along");
    assert_eq!(fares(second).await, WaitingFor::Waiting);
    settle::unread::never_read(db, claimed).await;
    assert_eq!(fares(first).await, WaitingFor::Unread);
    assert_eq!(fares(second).await, WaitingFor::Unread);
    let claimed = settle_from(db, ago(1)).await;
    finish(db, &claimed, Ok(Effect::NoChange)).await.unwrap();
    assert_eq!(fares(first).await, WaitingFor::Over);
    assert_eq!(fares(second).await, WaitingFor::Over);
    schema.drop().await;
}

async fn active(db: &DatabaseConnection) -> Vec<(String, Option<String>)> {
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT m.content, b.content AS by FROM agent_memories m \
         LEFT JOIN agent_memories b ON b.id = m.superseded_by ORDER BY m.content",
    ))
    .await
    .unwrap()
    .iter()
    .map(|row| {
        (
            row.try_get("", "content").unwrap(),
            row.try_get("", "by").unwrap(),
        )
    })
    .collect()
}

async fn knows(db: &DatabaseConnection, content: &str) {
    unified::remember(
        db,
        unified::NewMemory {
            user_id: 7,
            kind: unified::MemoryKind::Fact,
            content: content.into(),
            evidence: Some(content.into()),
            speaker: unified::Speaker::User,
            source: "chat",
            audience: Audience::private(7),
            importance: 0.6,
            concepts: Vec::new(),
        },
    )
    .await
    .unwrap()
    .unwrap();
}

/// A line of theirs kept for good, written down on its own, said at `at`:
/// it lets go of `old` and tells `facts` (as `late`, beside what she said).
async fn on_its_own(db: &DatabaseConnection, id: &str, at: At, old: &[&str], facts: &[&str]) {
    enqueue(
        db,
        id,
        7,
        Payload::Chat {
            user_text: facts.join("，"),
            reply: String::new(),
            input_at: at,
            present: Audience::private(7),
            turn: TurnContext::default(),
            lines: vec![at],
            for_good: true,
        },
        None,
    )
    .await
    .unwrap();
    due(db, id).await;
    let claimed = claim(db).await.unwrap().expect("due");
    let update = |fact: Option<&str>, old: &[&str]| ChatMemoryUpdate {
        standing: Default::default(),
        topic: None,
        cues: Vec::new(),
        fact: fact.map(str::to_string),
        supersedes: old.iter().map(|old| old.to_string()).collect(),
        evidence: Some(fact.unwrap_or("改口").to_string()),
        concepts: vec![],
    };
    let updates = std::iter::once(update(None, old))
        .chain(facts.iter().map(|fact| update(Some(fact), &[])))
        .collect();
    finish(
        db,
        &claimed,
        Ok(Effect::Chat(ChatMemoryUpdates {
            updates,
            said: facts
                .iter()
                .filter(|_| id == "late")
                .map(|_| crate::services::agent::merope::chat_remember::Said {
                    said: "猫都很会撒娇".into(),
                    evidence: "猫都很会撒娇".into(),
                    hers: false,
                    concepts: Vec::new(),
                })
                .collect(),
            said_new: crate::services::agent::merope::chat_remember::SaidNew::Something,
            ..Default::default()
        })),
    )
    .await
    .unwrap();
}

/// A correction in a line kept for good is made by the one thing it tells,
/// never a taking back; with nothing told that is kept, or several, it is
/// not made.
#[tokio::test]
async fn a_line_kept_for_good_corrects_by_what_it_tells() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    knows(db, "住在杭州").await;
    let at = crate::clock::now().fixed_offset() - chrono::Duration::days(3);
    on_its_own(db, "none", at, &["住在杭州"], &[]).await;
    assert_eq!(active(db).await, [("住在杭州".to_string(), None)]);
    let alive = unified::active_in(
        db,
        7,
        &Audience::private(7),
        &unified::MemoryKind::ABOUT_PERSON,
    )
    .await
    .unwrap();
    assert_eq!(alive.len(), 1, "not made with nothing told");
    on_its_own(db, "told", at, &["住在杭州"], &["搬到上海了"]).await;
    assert_eq!(
        active(db).await,
        [
            ("住在杭州".to_string(), Some("搬到上海了".to_string())),
            ("搬到上海了".to_string(), None),
        ]
    );
    knows(db, "在学吉他").await;
    on_its_own(db, "two", at, &["在学吉他"], &["改学钢琴了", "养了只猫"]).await;
    let now = active(db).await;
    for content in ["在学吉他", "改学钢琴了", "养了只猫"] {
        assert!(now.contains(&(content.to_string(), None)), "{now:?}");
    }
    assert_eq!(now.len(), 5);
    schema.drop().await;
}

/// What a line written down days later tells is of that line: found by it,
/// and said when it was.
#[tokio::test]
async fn a_line_written_down_later_is_known_by_its_message() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", 7, Some(r#"{"mode":"chat"}"#), false).await;
    let id = message(db, "mine", "user", "养了只猫", 3 * 24 * 60, Some("{}")).await;
    let at = said_at(db, id).await;
    on_its_own(db, "late", at, &[], &["养了只猫"]).await;
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT k.id, k.valid_from, (m.metadata->'told')::jsonb AS told \
             FROM agent_memories k, agent_messages m WHERE m.id = $1 AND k.source = 'chat'",
            [id.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    let kept: String = row.try_get("", "id").unwrap();
    assert_eq!(row.try_get::<At>("", "valid_from").unwrap(), at);
    assert_eq!(
        row.try_get::<serde_json::Value>("", "told").unwrap(),
        serde_json::json!([kept])
    );
    // What she said back then, too, as of then.
    let hers = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT valid_from FROM agent_memories WHERE source = 'said'",
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(hers.try_get::<At>("", "valid_from").unwrap(), at);
    schema.drop().await;
}

/// A pass that ends within a long message: what is left of it goes on next,
/// and, left unread, is all of it that is written down one by one.
#[tokio::test]
async fn a_long_message_is_gone_on_with_where_a_pass_ended() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", 7, Some(r#"{"mode":"chat"}"#), false).await;
    let long: String = (0..200).map(|n| format!("第{n}件小事说给你听。")).collect();
    let parts = myriad_merope::settle_raw::line_parts(&long).len();
    assert!(parts > 2);
    let id = message(db, "mine", "user", &long, 90, None).await;
    let at = said_at(db, id).await;
    let claimed = settle_from(db, at).await;
    let effect = Effect::SettledFromLines {
        settled: Vec::new(),
        updates: ChatMemoryUpdates::default(),
        rest: Some(at),
        read: Vec::new(),
        partway: Some((id, 2)),
        let_go: Vec::new(),
    };
    finish(db, &claimed, Ok(effect)).await.unwrap();
    let job = loaded(db, "settle").await;
    let read = job.read.unwrap();
    assert!(read.partway == Some((id, 2)) && !read.has(id));
    // Never read, with the stretches after.
    due(db, "settle").await;
    settle::unread::never_read(db, claim(db).await.unwrap().expect("goes on")).await;
    for began in [60, 30] {
        let began = crate::clock::now().fixed_offset() - chrono::Duration::minutes(began);
        settle::unread::never_read(db, settle_from(db, began).await).await;
    }
    let mut queued = Vec::new();
    for part in 0..parts {
        let moment = at.timestamp_micros().to_string();
        let nth = part.to_string();
        let mut key = vec!["chat", "7", "private", moment.as_str()];
        if part > 0 {
            key.push(&nth);
        }
        let key = memory_jobs::key(&key);
        if registry::get::<Job>(db, NAMESPACE, &key)
            .await
            .unwrap()
            .is_some()
        {
            queued.push(part);
        }
    }
    assert_eq!(queued, (2..parts).collect::<Vec<_>>());
    let read = loaded(db, "settle").await.read.unwrap();
    assert!(read.partway.is_none() && read.has(id));
    schema.drop().await;
}

/// Lines that could not be put back in the queue one by one are still not
/// gone over: none of them is taken as read.
#[tokio::test]
async fn lines_that_could_not_be_sent_back_are_not_read() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", 7, Some(r#"{"mode":"chat"}"#), false).await;
    let id = message(db, "mine", "user", "周末要搬家", 90, None).await;
    let at = said_at(db, id).await;
    settle::unread::never_read(db, settle_from(db, at).await).await;
    let ago = |minutes| crate::clock::now().fixed_offset() - chrono::Duration::minutes(minutes);
    settle::unread::never_read(db, settle_from(db, ago(60)).await).await;
    db.execute_unprepared(
        "CREATE FUNCTION no_lines() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
           IF NEW.payload->'data'->'Chat' IS NOT NULL THEN RAISE EXCEPTION 'not now'; END IF; \
           RETURN NEW; END $$; \
         CREATE TRIGGER no_lines BEFORE INSERT ON runtime_registry \
           FOR EACH ROW EXECUTE FUNCTION no_lines()",
    )
    .await
    .unwrap();
    settle::unread::never_read(db, settle_from(db, ago(30)).await).await;
    let job = loaded(db, "settle").await;
    assert_eq!(job.unread_from, Some(at), "they wait");
    assert!(job.read.is_none_or(|read| !read.has(id)));
    schema.drop().await;
}

/// What a fact of a talk gone over replaces may be gone by then (forgotten
/// on the memory page, replaced by another of them): the fact is kept all
/// the same, for nothing else holds it.
#[tokio::test]
async fn a_fact_is_kept_though_what_it_replaces_is_gone() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", 7, Some(r#"{"mode":"chat"}"#), false).await;
    let id = message(db, "mine", "user", "搬到上海了", 9, None).await;
    let read = vec![(said_at(db, id).await, id)];
    let claimed = settle_from(db, read[0].0).await;
    let mut effect = from_lines(Some("搬到上海了"), &read, &[]);
    if let Effect::SettledFromLines { updates, .. } = &mut effect {
        updates.updates[0].supersedes = vec!["住在杭州".into()];
    }
    finish(db, &claimed, Ok(effect)).await.unwrap();
    assert_eq!(active(db).await, [("搬到上海了".to_string(), None)]);
    schema.drop().await;
}
