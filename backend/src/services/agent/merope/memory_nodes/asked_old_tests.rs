//! What she held of someone before they asked her not to keep something,
//! held to what they asked (`asked::old`), on a database of their own, as
//! `intent_tests`. Every word here is made up.

use chrono::Duration;
use myriad_merope::on_mind::OnMind;
use myriad_merope::sore::Weight;
use myriad_nodes::body::{IntentStatus, Origin};
use sea_orm::{ConnectionTrait, DatabaseConnection};

use super::home_intents::{self, THREAD};
use super::intent_tests::{held, told};
use super::intents_due;
use super::talk_tests::{OTHER, schema};
use crate::services::agent::memory::unified::{self, Audience};
use crate::services::agent::merope::asked::{old, tests::asks};
use crate::services::agent::merope::memory_jobs::Payload;
use crate::services::agent::merope::threads::{self, Comes, Kept};
use crate::services::agent::merope::{bits, chat_days, on_mind, sore, store};
use crate::services::channel_group::g0;
use crate::services::her_log::pen;

const ASK: &str = "不要记和房东吵架的事";

fn now() -> chrono::DateTime<chrono::Utc> {
    crate::services::agent::merope::clock::now()
}

/// A thread she keeps of them, grown from what they said for later.
async fn thread(db: &DatabaseConnection, about: &str, then: &str, hours: i64) {
    let kept = Kept {
        about: about.into(),
        then: then.into(),
        comes: Comes::In(Some(hours)),
    };
    let line = super::intent_tests::SAID;
    let from = home_intents::origin_of(db, OTHER, line, now()).await;
    assert!(from.is_some(), "their line is found");
    threads::keep_from(db, OTHER, &kept, now(), from.as_deref()).await;
}

/// What they asked is still to be held to what she held.
async fn waits(db: &DatabaseConnection) {
    store::memory_jobs::enqueue(
        db,
        "asked_over",
        OTHER,
        Payload::AskedOver {
            asked: crate::clock::now().fixed_offset(),
            venue: None,
            stays: Vec::new(),
        },
        None,
    )
    .await
    .unwrap();
}

fn soon() -> std::time::Instant {
    std::time::Instant::now() + std::time::Duration::from_secs(60)
}

/// A world whose judge of what they asked answers `answers`, in order
/// (None: no answer), and whose lines are `says`.
fn judging(answers: &[Option<&str>], says: &[&str]) {
    let mut world = g0::World::new(0.5);
    for answer in answers {
        world
            .models
            .entry("not_to_keep".into())
            .or_default()
            .push_back(answer.map(str::to_string));
    }
    for line in says {
        world
            .says
            .entry("intent_raise".into())
            .or_default()
            .push_back((0.0, Ok(line.to_string())));
    }
    g0::enter(world);
}

/// What the judge was asked, and what she was asked to say.
fn seen() -> (Vec<String>, Vec<String>) {
    let log = g0::leave().expect("a world").log;
    let of = |kind: &str, operation: &str, key: &str| {
        log.iter()
            .filter(|line| line["kind"] == kind && line["what"]["operation"] == operation)
            .map(|line| line["what"][key].as_str().unwrap_or_default().to_string())
            .collect::<Vec<_>>()
    };
    (
        of("model", "not_to_keep", "input"),
        of("say", "intent_raise", "prompt"),
    )
}

async fn waiting_texts(db: &DatabaseConnection) -> Vec<String> {
    let mut texts: Vec<String> = held(db)
        .await
        .into_iter()
        .filter(|held| held.waiting())
        .map(|held| held.text)
        .collect();
    texts.sort();
    texts
}

/// Two threads, a sore spot, something on her mind and an intent of hers,
/// and two things they told her to do.
async fn all_kinds(db: &DatabaseConnection) {
    told(db, now()).await;
    g0::leave();
    thread(db, "房东", "问问和房东和好没有", 20).await;
    thread(db, "快递", "提醒取快递", 30).await;
    sore::keep(
        db,
        OTHER,
        Audience::private(OTHER),
        "说起房东时冲我发了火",
        Weight::Hurt,
    )
    .await;
    let mind = OnMind::new("房东会不会赶他走", Some(OTHER), "听他说的", now()).unwrap();
    on_mind::keep(db, &mind).await;
    asks(db, OTHER, ASK).await;
}

#[tokio::test]
async fn what_is_of_it_is_closed_and_what_they_told_her_to_do_stays() {
    let Some(schema) = schema("memory_nodes_asked_old").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    all_kinds(db).await;
    assert_eq!(waiting_texts(db).await.len(), 5);
    // The threads, the sore spot, her mind, her own intent, in that order.
    judging(&[Some(r#"{"drop":[0,2,3,4]}"#)], &[]);
    assert!(old::let_go(db, OTHER, soon(), false, 0, None).await.is_ok());
    let (judged, _) = seen();
    assert_eq!(judged.len(), 1, "{judged:?}");
    for shown in [
        "和房东和好",
        "取快递",
        "冲我发了火",
        "赶他走",
        "how the fern",
    ] {
        assert!(judged[0].contains(shown), "{shown}: {}", judged[0]);
    }
    for theirs in ["check the weather", "water the fern"] {
        assert!(!judged[0].contains(theirs), "{theirs}");
    }
    let left: Vec<String> = threads::open(db, OTHER)
        .await
        .into_iter()
        .map(|thread| thread.about)
        .collect();
    assert_eq!(left, ["快递"]);
    assert!(sore::kept(db, OTHER).await.unwrap().is_empty());
    assert!(on_mind::now(db).await.is_none());
    let all = held(db).await;
    assert!(
        all.iter()
            .filter(|held| held.origin == Some(Origin::Partner))
            .all(|held| held.waiting())
    );
    let mine: Vec<_> = all
        .iter()
        .filter(|held| held.origin == Some(Origin::Her) && held.waiting())
        .collect();
    assert_eq!(mine.len(), 1);
    assert!(mine[0].id.starts_with(THREAD) && mine[0].text.contains("快递"));
    assert!(
        all.iter()
            .filter(|held| held.text.contains("how the fern"))
            .all(|held| held.status == IntentStatus::Dropped)
    );
    // Nothing of it comes up later either.
    let later = now() + Duration::hours(40);
    judging(&[], &["the fern!", "快递到了"]);
    intents_due::tick_at(db, later).await;
    let (_, said) = seen();
    assert!(
        said.iter().all(|prompt| !prompt.contains("房东")),
        "{said:?}"
    );
    schema.drop().await;
}

#[tokio::test]
async fn unjudged_nothing_is_closed_and_only_what_they_told_her_is_said() {
    let Some(schema) = schema("memory_nodes_asked_old_waits").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    // Outside any world, whose clock is its own.
    let later = now() + Duration::hours(22);
    all_kinds(db).await;
    // Nothing new asked: nothing judged.
    judging(&[], &[]);
    assert!(old::done_first(db, OTHER).await);
    assert!(seen().0.is_empty());

    waits(db).await;
    judging(&[None; 12], &["the fern!"]);
    assert!(
        old::let_go(db, OTHER, soon(), false, 0, None)
            .await
            .is_err()
    );
    assert!(!old::done_first(db, OTHER).await);
    intents_due::tick_at(db, later).await;
    let (_, said) = seen();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("water the fern"));
    assert_eq!(threads::open(db, OTHER).await.len(), 2);
    assert!(on_mind::now(db).await.is_some());
    assert!(
        held(db)
            .await
            .iter()
            .any(|held| held.waiting() && held.text.contains("和好没有")),
        "still hers, for when it can be judged"
    );

    // Judged and none of it what they asked: done with, and said.
    judging(&[Some(r#"{"drop":[]}"#)], &[]);
    assert!(old::let_go(db, OTHER, soon(), false, 0, None).await.is_ok());
    assert_eq!(seen().0.len(), 1);
    db.execute_unprepared("DELETE FROM runtime_registry WHERE record_id = 'asked_over'")
        .await
        .unwrap();
    judging(&[], &["和好没有？"]);
    intents_due::tick_at(db, later).await;
    let (judged, said) = seen();
    // Nothing is judged where she is about to say it.
    assert!(judged.is_empty());
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("和好没有"));
    schema.drop().await;
}

/// What she makes of them, from each of `sources`.
async fn makes(db: &DatabaseConnection, made: &[(&'static str, &str, Option<&str>)]) {
    for (source, content, evidence) in made {
        unified::remember(
            db,
            unified::NewMemory {
                user_id: OTHER,
                kind: unified::MemoryKind::Fact,
                content: content.to_string(),
                evidence: evidence.map(str::to_string),
                speaker: unified::Speaker::Agent,
                source,
                audience: Audience::private(OTHER),
                importance: 0.7,
                concepts: Vec::new(),
            },
        )
        .await
        .unwrap()
        .unwrap();
    }
}

async fn made(db: &DatabaseConnection, sources: &[&str]) -> Vec<String> {
    let mut made = Vec::new();
    for source in sources {
        made.extend(
            unified::venue_source_rows(db, Some(OTHER), "private", source, 500)
                .await
                .unwrap()
                .into_iter()
                .map(|row| row.content),
        );
    }
    made
}

/// What is not told, however it failed and however long it is tried, stays
/// as it is; what is told to go goes meanwhile.
#[tokio::test]
async fn what_cannot_be_told_stays_and_what_is_told_goes() {
    let Some(schema) = schema("memory_nodes_asked_old_untold").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    asks(db, OTHER, ASK).await;
    let sources = [bits::US_SOURCE, bits::LANDS_SOURCE];
    makes(
        db,
        &[
            (bits::US_SOURCE, "常听他抱怨房东的人", None),
            (bits::LANDS_SOURCE, "话太多", None),
        ],
    )
    .await;
    // Away, the account refused, its quota gone, too many asks: nothing
    // closed, and never given up on.
    for answer in [
        None,
        Some("!rejected"),
        Some("!quota"),
        Some("!rateLimited"),
    ] {
        for gives_up in [false, true] {
            judging(&[answer; 9], &[]);
            assert!(
                old::let_go(db, OTHER, soon(), gives_up, 0, None)
                    .await
                    .is_err()
            );
            g0::leave();
            assert_eq!(made(db, &sources).await.len(), 2, "{answer:?}");
        }
    }
    // No answer in time: as any of those.
    for gives_up in [false, true] {
        let mut world = g0::World::new(0.5);
        world
            .delays
            .insert("not_to_keep".into(), [1_000.0; 3].into());
        g0::enter(world);
        let in_time = std::time::Instant::now() + std::time::Duration::from_secs(3);
        assert!(
            old::let_go(db, OTHER, in_time, gives_up, 0, None)
                .await
                .is_err()
        );
        g0::leave();
        assert_eq!(made(db, &sources).await.len(), 2);
    }
    // Answers that are not one, together and one by one: nothing closed, and
    // tried again; tried long enough, let be as it is, and counted.
    let no = Some("不好说");
    judging(&[no; 9], &[]);
    assert!(
        old::let_go(db, OTHER, soon(), false, 0, None)
            .await
            .is_err()
    );
    g0::leave();
    let before = counts();
    judging(&[no; 9], &[]);
    assert!(old::let_go(db, OTHER, soon(), true, 0, None).await.is_ok());
    g0::leave();
    assert!(counts() >= before + 2);
    assert_eq!(made(db, &sources).await.len(), 2);
    // Cut off at its limit, as one that is not an answer.
    let mut world = g0::World::new(0.5);
    world
        .models
        .insert("not_to_keep".into(), vec![Some("{}".to_string()); 9].into());
    world.cut_off.insert("not_to_keep".into(), 9);
    g0::enter(world);
    assert!(
        old::let_go(db, OTHER, soon(), false, 0, None)
            .await
            .is_err()
    );
    g0::leave();
    assert_eq!(made(db, &sources).await.len(), 2);
    // Both together never told; then the first told to go, the second never.
    let goes = Some(r#"{"drop":[0]}"#);
    judging(&[no, no, no, goes, goes, no, no, no], &[]);
    assert!(
        old::let_go(db, OTHER, soon(), false, 0, None)
            .await
            .is_err()
    );
    g0::leave();
    assert_eq!(made(db, &sources).await, ["话太多"]);
    schema.drop().await;
}

fn counts() -> u64 {
    crate::services::agent::merope::asked::counts()["letBe"]
        .as_u64()
        .unwrap()
}

/// The judge answering `answers`, one an ask, over what she held of them:
/// whether it is done, and what each ask was of.
async fn over(db: &DatabaseConnection, answers: &[Option<String>]) -> (bool, Vec<Vec<String>>) {
    over_of(db, answers, None).await
}

/// [`over`], of a job.
async fn over_of(
    db: &DatabaseConnection,
    answers: &[Option<String>],
    own: Option<store::memory_jobs::Own<'_>>,
) -> (bool, Vec<Vec<String>>) {
    let answers: Vec<Option<&str>> = answers.iter().map(Option::as_deref).collect();
    judging(&answers, &[]);
    let done = old::let_go(db, OTHER, soon(), false, 0, own).await.is_ok();
    let asked = seen()
        .0
        .iter()
        .map(|input| {
            let input: serde_json::Value = serde_json::from_str(input).unwrap();
            let items = input["items"].as_array().unwrap();
            items
                .iter()
                .map(|item| item["text"].as_str().unwrap().to_string())
                .collect()
        })
        .collect();
    (done, asked)
}

/// Bits, her diary of her days with them and what she made of them before
/// are held to it too, however many asks that takes.
#[tokio::test]
async fn bits_her_diary_and_what_they_were_to_her_are_held_to_it() {
    let Some(schema) = schema("memory_nodes_asked_old_made").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    asks(db, OTHER, ASK).await;
    let was = r#"{"before":"总跟房东过不去的人","first":{"us":"房东的仇人","at":"2026-09-01T00:00:00+08:00"}}"#;
    makes(db, &[(bits::US_SOURCE, "一起爬山的人", Some(was))]).await;
    let sources = [bits::US_SOURCE, bits::SOURCE, chat_days::SOURCE];
    let some = async |from: usize, to: usize| {
        for n in from..to {
            let text = format!("说的第{n}件");
            makes(db, &[(sources[1 + n % 2], text.as_str(), None)]).await;
        }
    };
    some(0, 30).await;
    makes(db, &[(bits::SOURCE, "学房东敲门", None)]).await;
    makes(db, &[(chat_days::SOURCE, "听他骂了一晚上房东", None)]).await;
    some(30, 60).await;
    // One ask never answered for: what the others were told of goes by
    // itself another time, and nothing is lost for it.
    let none = || Some(r#"{"drop":[]}"#.to_string());
    let (done, asked) = over(db, &[none(), none(), none()]).await;
    assert!(done);
    assert_eq!(asked.len(), 3, "{asked:?}");
    assert_eq!(made(db, &sources).await.len(), 63);
    let of_it: Vec<Option<String>> = asked
        .iter()
        .map(|items| {
            let places: Vec<usize> = (0..items.len())
                .filter(|place| items[*place].contains("房东"))
                .collect();
            Some(serde_json::json!({ "drop": places }).to_string())
        })
        .collect();
    assert!(over(db, &of_it).await.0);
    let left = made(db, &sources).await;
    assert_eq!(left.len(), 61);
    assert!(left.iter().all(|text| !text.contains("房东")), "{left:?}");
    let us = bits::us(db, OTHER).await.unwrap();
    assert_eq!(us.now, "一起爬山的人");
    assert!(us.before.is_none() && us.first.is_none());
    // Not shown her, and still kept.
    let row = unified::venue_source_rows(db, Some(OTHER), "private", bits::US_SOURCE, 1)
        .await
        .unwrap()
        .remove(0);
    let kept: serde_json::Value = serde_json::from_str(&row.evidence.unwrap()).unwrap();
    assert_eq!(kept["before"], "总跟房东过不去的人");
    assert_eq!(kept["first"]["us"], "房东的仇人");
    // Asked about more than a hundred at a time: each part by its own places.
    some(60, 130).await;
    let held: std::collections::BTreeSet<String> = made(db, &sources).await.into_iter().collect();
    let first = || Some(r#"{"drop":[0]}"#.to_string());
    let answers: Vec<_> = std::iter::repeat_with(first).take(12).collect();
    let (done, asked) = over(db, &answers).await;
    assert!(done);
    assert!(asked.iter().map(Vec::len).sum::<usize>() > 130);
    let left: std::collections::BTreeSet<String> = made(db, &sources).await.into_iter().collect();
    let gone: std::collections::BTreeSet<String> = held.difference(&left).cloned().collect();
    let firsts: std::collections::BTreeSet<String> =
        asked.into_iter().map(|mut items| items.remove(0)).collect();
    assert_eq!(gone, firsts);
    schema.drop().await;
}

/// What she made of them the last time is of it, how it started is not: only
/// that is not shown her any more, nor in what she makes of them next.
#[tokio::test]
async fn only_what_is_of_it_of_what_she_made_of_them_before_goes() {
    let Some(schema) = schema("memory_nodes_asked_old_before").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    asks(db, OTHER, ASK).await;
    let was = r#"{"before":"总跟房东过不去的人","first":{"us":"半夜来聊天的人","at":"2026-09-01T00:00:00+08:00"}}"#;
    makes(db, &[(bits::US_SOURCE, "一起爬山的人", Some(was))]).await;
    judging(&[Some(r#"{"drop":[0]}"#)], &[]);
    old::let_go(db, OTHER, soon(), false, 0, None)
        .await
        .unwrap();
    let asked: serde_json::Value = serde_json::from_str(&seen().0[0]).unwrap();
    assert_eq!(asked["items"][0]["text"], "总跟房东过不去的人");
    let us = bits::us(db, OTHER).await.unwrap();
    assert_eq!((us.now.as_str(), us.before), ("一起爬山的人", None));
    assert_eq!(us.first.unwrap().0, "半夜来聊天的人");
    // Gone over again, only the rest is asked about.
    judging(&[Some(r#"{"drop":[]}"#)], &[]);
    old::let_go(db, OTHER, soon(), false, 0, None)
        .await
        .unwrap();
    let asked: serde_json::Value = serde_json::from_str(&seen().0[0]).unwrap();
    assert_eq!(asked["items"].as_array().unwrap().len(), 2);
    schema.drop().await;
}

/// In a group: what she would come back to there waits while someone of it
/// is to be held to what they asked, and what is said of them goes.
#[tokio::test]
async fn what_she_would_come_back_to_in_a_group_is_held_to_it() {
    let Some(schema) = schema("memory_nodes_asked_old_group").await else {
        return;
    };
    let db = &schema.db;
    asks(db, OTHER, ASK).await;
    crate::services::agent::merope::chat_lines::tests::session(
        db,
        "his",
        OTHER,
        Some(r#"{"mode":"chat","venue":"qq:42"}"#),
        false,
    )
    .await;
    let called = format!("UPDATE users SET display_name = '阿北' WHERE id = {OTHER}");
    db.execute_unprepared(&called).await.unwrap();
    crate::services::agent::merope::chat_lines::tests::session(
        db,
        "hers",
        1,
        Some(r#"{"mode":"chat","venue":"qq:42"}"#),
        false,
    )
    .await;
    for about in ["房东", "爬山"] {
        let kept = Kept {
            about: about.into(),
            then: format!("问问{about}怎么样了"),
            comes: Comes::In(Some(1)),
        };
        threads::keep_in_group(db, "qq:42", &kept, now()).await;
    }
    assert!(old::done_first_in(db, "qq:42").await);
    let data = Payload::AskedOver {
        asked: crate::clock::now().fixed_offset(),
        venue: Some("qq:42".into()),
        stays: Vec::new(),
    };
    store::memory_jobs::enqueue(db, "asked_in", OTHER, data, None)
        .await
        .unwrap();
    assert!(!old::done_first_in(db, "qq:42").await);
    assert!(old::done_first_in(db, "qq:43").await);
    assert!(old::done_first(db, OTHER).await, "in private nothing waits");
    let of_it = threads::open_in_group(db, "qq:42")
        .await
        .iter()
        .rev()
        .position(|thread| thread.about == "房东")
        .unwrap();
    // Another of the group asked nothing: nothing is asked of them.
    judging(&[], &[]);
    assert!(
        old::let_go_in(db, 1, "qq:42", soon(), false, 0, None)
            .await
            .is_ok()
    );
    assert!(seen().0.is_empty());
    // Nor of one whose name is not known, or of a group they are not in.
    for (user_id, venue, name) in [(OTHER, "qq:42", "' '"), (OTHER, "qq:43", "'阿北'")] {
        db.execute_unprepared(&format!(
            "UPDATE users SET display_name = {name}, username = '' WHERE id = {user_id}"
        ))
        .await
        .unwrap();
        judging(&[Some(r#"{"drop":[0,1]}"#); 2], &[]);
        assert!(
            old::let_go_in(db, user_id, venue, soon(), false, 0, None)
                .await
                .is_ok()
        );
        assert!(seen().0.is_empty());
    }
    assert_eq!(threads::open_in_group(db, "qq:42").await.len(), 2);
    let drop = format!(r#"{{"drop":[{of_it}]}}"#);
    judging(&[Some(&drop)], &[]);
    assert!(
        old::let_go_in(db, OTHER, "qq:42", soon(), false, 0, None)
            .await
            .is_ok()
    );
    // Said of them, by their name.
    let (judged, _) = seen();
    let judged: serde_json::Value = serde_json::from_str(&judged[0]).unwrap();
    assert_eq!(judged["who"], "阿北");
    let left: Vec<String> = threads::open_in_group(db, "qq:42")
        .await
        .into_iter()
        .map(|thread| thread.about)
        .collect();
    assert_eq!(left, ["爬山"]);
    schema.drop().await;
}

#[tokio::test]
async fn what_she_takes_up_herself_is_not_kept_unless_judged_and_theirs_is() {
    let Some(schema) = schema("memory_nodes_asked_taken").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    asks(db, OTHER, ASK).await;
    // The judge does not answer.
    let (_, judged) = told(db, now()).await;
    g0::leave();
    assert_eq!((judged.taken.len(), judged.refused), (2, 1));
    assert!(
        held(db)
            .await
            .iter()
            .all(|held| held.origin == Some(Origin::Partner) && held.waiting())
    );
    schema.drop().await;
}

#[path = "asked_old_stays_tests.rs"]
mod stays;
