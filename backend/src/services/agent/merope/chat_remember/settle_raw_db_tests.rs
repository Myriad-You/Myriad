//! Going over a private talk from the lines kept of it, on a database of its
//! own, with the model's answers given by the test (`channel_group::g0`).

use serde_json::{Value, json};

use super::super::{Rests, SaidNew};
use super::lines::{Lines, lines};
use super::*;
use crate::services::agent::memory::unified::{self, Audience};
use crate::services::agent::merope::chat_lines::tests::{OTHER, line, schema, session};
use crate::services::agent::merope::chat_remember::hold_chat_words;
use crate::services::agent::merope::memory_jobs::{Payload, key};
use crate::services::agent::merope::store::memory_jobs::{Job, NAMESPACE};
use crate::services::channel_group::g0;
use crate::services::runtime_registry as registry;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

type At = chrono::DateTime<chrono::FixedOffset>;

const CHAT: Option<&str> = Some(r#"{"mode":"chat"}"#);

fn fact(text: &str, evidence: &str, from: &[usize]) -> Value {
    json!({
        "fact": text, "evidence": evidence, "from": from, "concepts": [], "whenFor": [],
        "topic": {"on": "", "name": ""},
        "happened": {"from": "", "to": "", "about": false},
        "state": "", "open": null, "replaces": [],
    })
}

fn answer(lasting: &[Value], took_back: &[Value], let_go: &[usize]) -> String {
    json!({
        "lasting": lasting, "tookBack": took_back, "moment": [], "known": [], "letGo": let_go,
        "said": [], "own": [], "putOnto": [], "notToKeep": [],
    })
    .to_string()
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

/// A world answering `answers`, in order, as going over.
fn answering(answers: &[String]) {
    let mut world = g0::World::new(0.5);
    world
        .models
        .entry("chat_settle".into())
        .or_default()
        .extend(answers.iter().cloned().map(Some));
    g0::enter(world);
}

/// How many times going over was asked.
fn asked() -> usize {
    g0::leave()
        .unwrap()
        .log
        .iter()
        .filter(|entry| entry["kind"] == "model" && entry["what"]["operation"] == "chat_settle")
        .count()
}

async fn over(
    db: &DatabaseConnection,
    from: At,
    last: At,
    read: Option<&Read>,
    answers: &[String],
) -> (Effect, usize) {
    answering(answers);
    let effect = settle_raw(db, OTHER, &Audience::private(OTHER), from, last, read).await;
    (
        effect.unwrap_or_else(|failure| panic!("{failure}")),
        asked(),
    )
}

async fn job(db: &DatabaseConnection, kind: &str) -> Option<Job> {
    let id = key(&[kind, &OTHER.to_string(), &Audience::private(OTHER).venue()]);
    registry::get(db, NAMESPACE, &id).await.unwrap()
}

/// As production has it: their message is kept first, and noted as said a
/// moment later. The stretch begins with the message, and one noted as
/// beginning after it still goes over it.
#[tokio::test]
async fn a_stretch_begins_with_its_first_message() {
    let Some(schema) = schema("raw_first_message").await else {
        return;
    };
    let db = &schema.db;
    let alone = Audience::private(OTHER);
    session(db, "mine", OTHER, CHAT, false).await;
    let id = line(db, "mine", "user", "我下周要搬家了", 1, None).await;
    let kept = said_at(db, id).await;
    let noted = kept + chrono::Duration::milliseconds(800);
    hold_chat_words(
        db,
        OTHER,
        "我下周要搬家了".into(),
        noted,
        alone.clone(),
        Default::default(),
        false,
    )
    .await;
    let held = job(db, "settle").await.expect("held");
    assert!(
        matches!(&held.data, Some(Payload::Settle { from, last, heard, .. })
            if *from == kept && *last == kept && heard[0].0 == kept),
        "{:?}",
        held.data
    );
    for from in [kept, noted] {
        let (effect, _) = over(
            db,
            from,
            noted,
            None,
            &[answer(&[fact("下周要搬家", "下周要搬家", &[0])], &[], &[])],
        )
        .await;
        let Effect::SettledFromLines { updates, read, .. } = effect else {
            panic!("gone over");
        };
        assert_eq!(updates.updates[0].fact.as_deref(), Some("下周要搬家"));
        assert_eq!(
            updates.said_from.get("下周要搬家"),
            Some(&Rests {
                first: kept,
                lines: vec![id],
            })
        );
        assert_eq!(read, [(kept, id)]);
    }

    // Said in a game: noted as that.
    line(db, "mine", "user", "我把你的城堡全拆了", 0, None).await;
    let turn = crate::services::agent::merope::TurnContext {
        in_game: true,
        ..Default::default()
    };
    hold_chat_words(
        db,
        OTHER,
        "我把你的城堡全拆了".into(),
        crate::clock::now().fixed_offset() + chrono::Duration::milliseconds(800),
        alone.clone(),
        turn,
        false,
    )
    .await;
    let read = lines(db, OTHER, &alone, kept, Lines::From(5))
        .await
        .unwrap();
    assert_eq!(
        read.iter().map(|line| line.in_game).collect::<Vec<_>>(),
        [false, true]
    );
    schema.drop().await;
}

/// What was said to her at work is written down on its own, for good: no
/// stretch of a talk to go over.
#[tokio::test]
async fn what_was_said_at_work_is_written_down_on_its_own() {
    let Some(schema) = schema("raw_at_work").await else {
        return;
    };
    let db = &schema.db;
    let at = crate::clock::now().fixed_offset();
    hold_chat_words(
        db,
        OTHER,
        "我下周要搬家了".into(),
        at,
        Audience::private(OTHER),
        Default::default(),
        true,
    )
    .await;
    assert!(job(db, "settle").await.is_none());
    assert!(matches!(
        job(db, "chat").await.and_then(|job| job.data),
        Some(Payload::Chat { for_good: true, .. })
    ));
    // And what it tells is what she knows for good.
    db.execute_unprepared(
        "UPDATE runtime_registry SET payload = jsonb_set(payload, '{ready}', '0') \
         WHERE namespace = 'merope_memory_jobs'",
    )
    .await
    .unwrap();
    let claimed = crate::services::agent::merope::store::memory_jobs::claim(db)
        .await
        .unwrap()
        .expect("due");
    let told = ChatMemoryUpdates {
        updates: vec![ChatMemoryUpdate {
            standing: Default::default(),
            fact: Some("下周要搬家".into()),
            supersedes: Vec::new(),
            evidence: Some("我下周要搬家了".into()),
            concepts: Vec::new(),
            topic: None,
            cues: Vec::new(),
        }],
        said_new: SaidNew::Something,
        ..Default::default()
    };
    crate::services::agent::merope::store::memory_jobs::finish(
        db,
        &claimed,
        Ok(Effect::Chat(told)),
    )
    .await
    .unwrap();
    let kept: Vec<(String, String)> = unified::active_in(
        db,
        OTHER,
        &Audience::private(OTHER),
        &unified::MemoryKind::ABOUT_PERSON,
    )
    .await
    .unwrap()
    .into_iter()
    .map(|row| (row.source, row.content))
    .collect();
    assert_eq!(kept, [("chat".to_string(), "下周要搬家".to_string())]);
    schema.drop().await;
}

/// A draft never sent was not said; what another of her ends keeps (copied
/// from there, or left for her there) is context only.
#[tokio::test]
async fn a_draft_is_not_said_and_what_another_end_keeps_is_context() {
    let Some(schema) = schema("raw_elsewhere").await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", OTHER, CHAT, false).await;
    let unsent = Some(r#"{"cutOff":"unsent"}"#);
    line(db, "mine", "assistant", "这句没发出去", 9, unsent).await;
    let heard = Some(r#"{"cutOff":"unheard_end"}"#);
    let spoken = line(db, "mine", "assistant", "这句念到一半", 8, heard).await;
    let copied = line(db, "mine", "user", "在电脑上说的", 7, None).await;
    let left = line(
        db,
        "mine",
        "user",
        "她在电脑上时留的",
        6,
        Some(r#"{"leftForHer":true}"#),
    )
    .await;
    let here = line(db, "mine", "user", "在这里说的", 5, None).await;
    db.execute_unprepared(&format!(
        "UPDATE agent_messages SET origin = 'mac' WHERE id = {copied}"
    ))
    .await
    .unwrap();
    let since = crate::clock::now().fixed_offset() - chrono::Duration::hours(1);
    let read = lines(db, OTHER, &Audience::private(OTHER), since, Lines::From(9))
        .await
        .unwrap();
    let read: Vec<(i32, bool)> = read.iter().map(|line| (line.id, line.gone)).collect();
    assert_eq!(
        read,
        [(spoken, false), (copied, true), (left, true), (here, false)]
    );
    schema.drop().await;
}

/// More than is read at once, and none of theirs among it: what was read is
/// done with, and the stretch goes on where it left off.
#[tokio::test]
async fn a_stretch_longer_than_is_read_at_once_goes_on() {
    let Some(schema) = schema("raw_long").await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", OTHER, CHAT, false).await;
    for minutes in (2..=LINES as i32 + 6).rev() {
        line(db, "mine", "assistant", "她一直在说", minutes, None).await;
    }
    let id = line(db, "mine", "user", "我下周要搬家了", 1, None).await;
    let last = said_at(db, id).await;
    let began = last - chrono::Duration::minutes(200);
    let moving = answer(&[fact("下周要搬家", "下周要搬家", &[7])], &[], &[]);
    let (effect, asked) = over(db, began, last, None, &[]).await;
    let Effect::SettledFromLines {
        updates,
        rest: Some(rest),
        read,
        ..
    } = effect
    else {
        panic!("goes on");
    };
    assert_eq!((asked, updates.updates.len()), (0, 0));
    assert_eq!(read.len(), LINES as usize);
    let read = Read::default().with(&read);
    let (effect, asked) = over(db, rest, last, Some(&read), &[moving]).await;
    let Effect::SettledFromLines {
        updates,
        rest: None,
        read,
        ..
    } = effect
    else {
        panic!("gone over");
    };
    assert_eq!(asked, 1);
    assert_eq!(updates.updates.len(), 1);
    assert!(read.iter().any(|(_, read)| *read == id));
    schema.drop().await;
}

/// Said and taken back in the same stretch: kept as nothing, and the line
/// is let go, whichever of the two asks found it.
#[tokio::test]
async fn a_line_taken_back_in_its_own_stretch_is_let_go() {
    let Some(schema) = schema("raw_let_go").await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", OTHER, CHAT, false).await;
    let said = line(db, "mine", "user", "我其实想辞职", 5, None).await;
    line(db, "mine", "assistant", "怎么了", 4, None).await;
    let back = line(db, "mine", "user", "算了当我没说", 3, None).await;
    let from = said_at(db, said).await;
    let last = said_at(db, back).await;
    let quit = || fact("想辞职", "我其实想辞职", &[0]);
    for answers in [
        vec![answer(&[quit()], &[], &[0, 2])],
        // The first ask accounts for neither line.
        vec![answer(&[], &[], &[]), answer(&[], &[], &[0, 2])],
    ] {
        let (effect, asked) = over(db, from, last, None, &answers).await;
        let Effect::SettledFromLines {
            updates, let_go, ..
        } = effect
        else {
            panic!("gone over");
        };
        assert_eq!(asked, answers.len());
        assert!(updates.updates.is_empty(), "{updates:?}");
        assert_eq!(let_go, [said, back]);
    }
    schema.drop().await;
}

/// Asked again, an earlier line is taken back that the first answer took
/// facts from: what the second look says is taken back goes, and the line
/// with the last of it; with no answer, all of it stays.
#[tokio::test]
async fn what_is_taken_back_when_asked_again_is_not_kept() {
    let Some(schema) = schema("raw_again_let_go").await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", OTHER, CHAT, false).await;
    let said = line(db, "mine", "user", "我其实想辞职，下周要搬家", 5, None).await;
    let back = line(db, "mine", "user", "辞职那句当我没说", 3, None).await;
    let from = said_at(db, said).await;
    let last = said_at(db, back).await;
    let quit = || fact("想辞职", "我其实想辞职", &[0]);
    let moving = || fact("下周要搬家", "下周要搬家", &[0]);
    let both = || answer(&[quit(), moving()], &[], &[]);
    let says = |says: &str| Some(json!({ "says": says }).to_string());
    let alone = || answer(&[quit()], &[], &[]);
    for (first, looks, kept, let_go) in [
        (alone(), vec![says("takes_back")], vec![], vec![said, back]),
        (alone(), vec![says("neither")], vec!["想辞职"], vec![back]),
        (alone(), vec![None], vec!["想辞职"], vec![back]),
        (
            both(),
            vec![says("takes_back"), says("neither")],
            vec!["下周要搬家"],
            vec![back],
        ),
        (
            both(),
            vec![says("takes_back"), says("takes_back")],
            vec![],
            vec![said, back],
        ),
        // None of it in the line as a whole: each in its own words.
        (
            both(),
            ["neither", "neither", "takes_back", "takes_back"]
                .map(says)
                .into(),
            vec![],
            vec![said, back],
        ),
        (
            both(),
            vec![None, None, None, None],
            vec!["想辞职", "下周要搬家"],
            vec![back],
        ),
    ] {
        let mut world = g0::World::new(0.5);
        let answers = [first, answer(&[], &[], &[0, 1])].map(Some);
        world.models.insert("chat_settle".into(), answers.into());
        world.models.insert("take_back_check".into(), looks.into());
        g0::enter(world);
        let effect = settle_raw(db, OTHER, &Audience::private(OTHER), from, last, None).await;
        g0::leave();
        let Ok(Effect::SettledFromLines {
            updates,
            let_go: lines,
            ..
        }) = effect
        else {
            panic!("gone over");
        };
        let facts: Vec<_> = (updates.updates.iter())
            .filter_map(|update| update.fact.as_deref())
            .collect();
        assert_eq!(facts, kept);
        let mut lines = lines;
        lines.sort_unstable();
        assert_eq!(lines, let_go);
    }
    schema.drop().await;
}

/// A fact of a line let go all the same is looked at again: taken back, it
/// is not kept and its line is let go; something else, it replaces what it
/// says it does; unanswered, it is kept beside it.
#[tokio::test]
async fn a_fact_of_a_line_let_go_is_looked_at_again() {
    let Some(schema) = schema("raw_doubted").await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", OTHER, CHAT, false).await;
    let said = line(db, "mine", "user", "我下个月要去东京出差一周", 5, None).await;
    let back = line(db, "mine", "user", "出差那句当我没说", 3, None).await;
    let from = said_at(db, said).await;
    let last = said_at(db, back).await;
    holds(db, "chat", "下个月要去大阪").await;
    let mut trip = fact("下个月要去东京", "我下个月要去东京", &[0]);
    trip["replaces"] = json!(["下个月要去大阪"]);
    let says = |says: &str| Some(json!({ "says": says }).to_string());
    let osaka = || vec!["下个月要去大阪".to_string()];
    let nothing = answer(&[], &[], &[]);
    let of = |let_go: &[usize]| answer(&[trip.clone()], &[], let_go);
    // In the first answer, in the one about the lines it left, and in the
    // first with its line let go by the other too.
    let (once, again) = (vec![of(&[0, 1])], vec![nothing, of(&[0, 1])]);
    let both = || vec![of(&[0]), answer(&[], &[], &[0, 1])];
    for (answers, looks, kept, let_go) in [
        (
            once.clone(),
            vec![says("takes_back")],
            None,
            vec![said, back],
        ),
        (
            once.clone(),
            vec![says("neither")],
            Some(osaka()),
            vec![back],
        ),
        (once, vec![None], Some(Vec::new()), vec![back]),
        (
            again.clone(),
            vec![says("takes_back")],
            None,
            vec![said, back],
        ),
        (
            again.clone(),
            vec![says("neither")],
            Some(osaka()),
            vec![back],
        ),
        (again, vec![None], Some(Vec::new()), vec![back]),
        // Told once to be something else, it is.
        (
            both(),
            vec![says("neither"), says("takes_back")],
            Some(osaka()),
            vec![back],
        ),
        (
            both(),
            vec![None, says("takes_back")],
            None,
            vec![said, back],
        ),
        (both(), vec![None, None], Some(Vec::new()), vec![back]),
    ] {
        let mut world = g0::World::new(0.5);
        let answers: std::collections::VecDeque<_> = answers.into_iter().map(Some).collect();
        world.models.insert("chat_settle".into(), answers);
        world.models.insert("take_back_check".into(), looks.into());
        g0::enter(world);
        let effect = settle_raw(db, OTHER, &Audience::private(OTHER), from, last, None).await;
        g0::leave();
        let Ok(Effect::SettledFromLines {
            updates,
            let_go: mut lines,
            ..
        }) = effect
        else {
            panic!("gone over");
        };
        let facts: Vec<_> = (updates.updates.iter())
            .filter(|update| update.fact.is_some())
            .map(|update| update.supersedes.clone())
            .collect();
        assert_eq!(facts, Vec::from_iter(kept));
        lines.sort_unstable();
        assert_eq!(lines, let_go);
    }
    schema.drop().await;
}

/// A fact is told of the lines that say what it quotes, whatever lines it is
/// said to be from; what she said, of hers. All of them say when it was.
#[tokio::test]
async fn what_is_kept_is_told_of_the_lines_that_say_it() {
    let Some(schema) = schema("raw_told").await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", OTHER, CHAT, false).await;
    let mut ids = Vec::new();
    for (role, text, minutes) in [
        ("user", "那件事不是这样的", 9),
        ("user", "我周五考试，数学那门", 8),
        ("user", "不是", 7),
        ("assistant", "加油，好呀，你一定可以的", 6),
        ("assistant", "好呀", 5),
    ] {
        ids.push(line(db, "mine", role, text, minutes, None).await);
    }
    let (from, last) = (said_at(db, ids[0]).await, said_at(db, ids[4]).await);
    let says = json!({ "line": "好呀", "evidence": "好呀", "concepts": [] });
    for (evidence, named, told, first) in [
        ("我周五考试，数学 / 不是", vec![], vec![1, 2], 1),
        ("我周五考试，数学 / 不是", vec![0, 1, 2], vec![1, 2], 0),
        // None says it alone: all it is from.
        ("那件事不 / 数学那", vec![0, 1], vec![0, 1], 0),
    ] {
        let mut whole: Value =
            serde_json::from_str(&answer(&[fact("周五考数学", evidence, &named)], &[], &[]))
                .unwrap();
        whole["own"] = json!([says]);
        answering(&[whole.to_string(), answer(&[], &[], &[])]);
        let effect = settle_raw(db, OTHER, &Audience::private(OTHER), from, last, None).await;
        g0::leave();
        let Ok(Effect::SettledFromLines { updates, .. }) = effect else {
            panic!("gone over");
        };
        let rests = &updates.said_from[evidence];
        let told: Vec<i32> = told.into_iter().map(|index: usize| ids[index]).collect();
        assert_eq!(rests.lines, told, "{evidence}");
        assert_eq!(rests.first, said_at(db, ids[first]).await, "{evidence}");
        assert_eq!(updates.said_from["好呀"].lines, [ids[4]]);
        assert_eq!(updates.said_from["好呀"].first, said_at(db, ids[3]).await);
    }
    schema.drop().await;
}

/// Asked again only while there is the time for it: what the first answer
/// gave is kept, and the line accounted for nowhere stays as said.
#[tokio::test]
async fn nothing_more_is_asked_without_the_time_for_it() {
    let Some(schema) = schema("raw_time").await else {
        return;
    };
    let db = &schema.db;
    let spoken = |id: i32, text: &str| Spoken {
        id,
        theirs: true,
        text: text.into(),
        at: crate::clock::now().fixed_offset(),
        gone: false,
        in_game: false,
    };
    let talk = [spoken(1, "我下周要搬家了"), spoken(2, "最近在学吉他")];
    let first = || answer(&[fact("下周要搬家", "下周要搬家", &[0])], &[], &[]);
    let again = || answer(&[fact("在学吉他", "最近在学吉他", &[1])], &[], &[]);
    // Enough for the first, and not for another; enough for both.
    let secs = std::time::Duration::from_secs;
    for (time, times, left, facts) in [(secs(10), 1, 1, 1), (secs(3600), 2, 0, 2)] {
        let until = std::time::Instant::now() + time;
        answering(&[first(), again()]);
        let gone = go_over(
            db,
            OTHER,
            &Audience::private(OTHER),
            &talk,
            "2026-11-23 (Monday)",
            None,
            &[],
            Some(until),
        )
        .await
        .unwrap_or_else(|failure| panic!("{failure}"));
        assert_eq!(asked(), times);
        assert_eq!((gone.left, gone.updates.updates.len()), (left, facts));
    }
    schema.drop().await;
}

async fn holds(db: &DatabaseConnection, source: &'static str, content: &str) {
    unified::remember(
        db,
        unified::NewMemory {
            user_id: OTHER,
            kind: unified::MemoryKind::Fact,
            content: content.into(),
            evidence: Some(content.into()),
            speaker: unified::Speaker::User,
            source,
            audience: Audience::private(OTHER),
            importance: 0.6,
            concepts: Vec::new(),
        },
    )
    .await
    .unwrap()
    .unwrap();
}

async fn standing(db: &DatabaseConnection) -> Vec<String> {
    let mut held: Vec<String> = unified::active_in(
        db,
        OTHER,
        &Audience::private(OTHER),
        &unified::MemoryKind::ABOUT_PERSON,
    )
    .await
    .unwrap()
    .into_iter()
    .map(|row| row.content)
    .collect();
    held.sort();
    held
}

/// What another end kept, and short notes from before, are not shown as
/// known for good, nor checked against; a fact said to replace one of them
/// beside one it can is still kept, and replaces that.
#[tokio::test]
async fn only_what_talks_here_kept_for_good_is_known_and_changed() {
    let Some(schema) = schema("raw_known").await else {
        return;
    };
    let db = &schema.db;
    let alone = Audience::private(OTHER);
    for (source, content) in [
        ("chat", "在杭州上班"),
        (unified::SHORT_NOTE_SOURCE, "最近在杭州上班，想换工作"),
        (unified::SYNC_SOURCE, "在杭州的公司上班"),
    ] {
        holds(db, source, content).await;
    }
    let spoken = Spoken {
        id: 1,
        theirs: true,
        text: "我不在杭州上班了，去上海了".into(),
        at: crate::clock::now().fixed_offset(),
        gone: false,
        in_game: false,
    };
    let known = what_she_knows(db, OTHER, &alone, std::slice::from_ref(&spoken))
        .await
        .unwrap_or_else(|failure| panic!("{failure}"));
    assert_eq!(known.known, ["在杭州上班"]);

    // Nothing of theirs is alike but what cannot be changed: no one is
    // asked which it replaces.
    let new = |supersedes: &[&str]| ChatMemoryUpdates {
        updates: vec![ChatMemoryUpdate {
            standing: Default::default(),
            fact: Some("在上海的公司上班".into()),
            supersedes: supersedes.iter().map(|old| old.to_string()).collect(),
            evidence: Some("去上海了".into()),
            concepts: Vec::new(),
            topic: None,
            cues: Vec::new(),
        }],
        ..Default::default()
    };
    answering(&[]);
    let mut updates = new(&[]);
    super::super::already::check_for_good(
        db,
        OTHER,
        &alone,
        &known.known,
        &mut updates,
        std::time::Duration::from_secs(15),
    )
    .await;
    let checked = g0::leave().unwrap().log;
    assert!(
        !checked
            .iter()
            .any(|entry| entry["what"]["operation"] == "chat_already")
    );
    assert_eq!(updates, new(&[]));

    use sea_orm::TransactionTrait;
    let tx = db.begin().await.unwrap();
    let kept = crate::services::agent::merope::store::settle_chat_memory_on(
        &tx,
        OTHER,
        crate::clock::now().fixed_offset(),
        &new(&["在杭州上班", "在杭州的公司上班"]),
        &[],
        &alone,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(kept);
    let mut left = vec![
        "最近在杭州上班，想换工作",
        "在杭州的公司上班",
        "在上海的公司上班",
    ];
    left.sort_unstable();
    assert_eq!(standing(db).await, left);
    schema.drop().await;
}

/// The first ask has no more than the time there is, however slow.
#[tokio::test]
async fn the_first_ask_ends_within_the_time_there_is() {
    let Some(schema) = schema("raw_first_ask").await else {
        return;
    };
    let mut world = g0::World::new(0.5);
    world
        .models
        .entry("chat_settle".into())
        .or_default()
        .push_back(Some(answer(&[], &[], &[])));
    world
        .delays
        .entry("chat_settle".into())
        .or_default()
        .push_back(20.0);
    g0::enter(world);
    let began = std::time::Instant::now();
    let spoken = Spoken {
        id: 1,
        theirs: true,
        text: "我下周要搬家了".into(),
        at: crate::clock::now().fixed_offset(),
        gone: false,
        in_game: false,
    };
    let gone = go_over(
        &schema.db,
        OTHER,
        &Audience::private(OTHER),
        &[spoken],
        "2026-11-23 (Monday)",
        None,
        &[],
        Some(began + std::time::Duration::from_secs(4)),
    )
    .await;
    g0::leave();
    assert!(gone.is_err());
    assert!(began.elapsed() < std::time::Duration::from_secs(10));
    schema.drop().await;
}

/// As production writes them: a line heard on another of her ends, and one
/// said with her switch off (and the answer to it), are context only.
#[tokio::test]
async fn what_is_heard_elsewhere_or_without_her_is_noted_as_that() {
    use crate::services::agent::sessions::{
        persist_assistant_message, persist_user_message, persist_user_message_left_for_her,
    };
    let Some(schema) = schema("raw_without_her").await else {
        return;
    };
    let db = &schema.db;
    session(db, "mine", OTHER, CHAT, false).await;
    let world = |away| {
        let mut world = g0::World::new(0.5);
        world.away = away;
        world
    };
    g0::enter(world(false));
    persist_user_message(db, "mine", "我下周要搬家了")
        .await
        .unwrap();
    persist_user_message_left_for_her(db, "mine", "在电脑上说的话", "onebot")
        .await
        .unwrap();
    g0::leave();
    g0::enter(world(true));
    persist_user_message(db, "mine", "她不在的时候说的")
        .await
        .unwrap();
    persist_assistant_message(db, "mine", None, "好的", Some(json!({ "runId": "r" })))
        .await
        .unwrap();
    g0::leave();
    line(
        db,
        "mine",
        "assistant",
        "她在电脑上发的",
        0,
        Some(r#"{"lentSay":"dev_mac/say_9"}"#),
    )
    .await;
    let began = crate::clock::now().fixed_offset() - chrono::Duration::hours(1);
    let spoken = lines(db, OTHER, &Audience::private(OTHER), began, Lines::From(10))
        .await
        .unwrap();
    let gone: Vec<(String, bool)> = spoken
        .into_iter()
        .map(|line| (line.text, line.gone))
        .collect();
    assert_eq!(gone.len(), 5, "{gone:?}");
    for (text, gone) in gone {
        assert_eq!(gone, text != "我下周要搬家了", "{text}");
    }
    schema.drop().await;
}
