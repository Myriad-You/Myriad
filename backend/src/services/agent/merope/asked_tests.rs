//! What they asked not to keep, held to as something is about to be kept:
//! asked of a database of its own, the judge's answers given by the test
//! (`channel_group::g0`).

use super::*;
use crate::services::agent::memory::unified::{self, Audience};
use crate::services::agent::merope::call;
use crate::services::agent::merope::chat_remember::{ChatMemoryUpdate, Said, SaidNew};
use crate::services::channel_group::g0;
use myriad_merope::not_to_keep::ITEMS_PER_ASK;
use sea_orm::ConnectionTrait;

const THEM: i32 = 7;
const OTHER: i32 = 8;
const ASK: &str = "不要记和房东吵架的事";

async fn asked_in(prefix: &str) -> Option<crate::db::IsolatedSchema> {
    let url = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"])?;
    let schema = crate::db::IsolatedSchema::migrated(&url, prefix).await;
    schema
        .db
        .execute_unprepared("INSERT INTO users (id, username) VALUES (7, 'seven'), (8, 'eight')")
        .await
        .unwrap();
    asks(&schema.db, THEM, ASK).await;
    Some(schema)
}

/// `user_id` asked her not to keep something.
pub(in crate::services::agent::merope) async fn asks(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    ask: &str,
) {
    unified::remember(
        db,
        unified::NewMemory {
            user_id,
            kind: unified::MemoryKind::Preference,
            content: ask.into(),
            evidence: Some("以后别记".into()),
            speaker: unified::Speaker::User,
            source: unified::ASKED_SOURCE,
            audience: Audience::private(user_id),
            importance: 1.0,
            concepts: Vec::new(),
        },
    )
    .await
    .unwrap()
    .unwrap();
}

fn items(texts: &[&str]) -> Vec<String> {
    texts.iter().map(|text| text.to_string()).collect()
}

/// Why `shown` of theirs, that comes again, could not be told of ([`told`]).
async fn failed(
    db: &sea_orm::DatabaseConnection,
    shown: &[&str],
    answers: &[Option<&str>],
) -> Failure {
    answering(answers);
    let out = to_leave_out_or_again(db, THEM, &items(shown)).await;
    g0::leave();
    out.expect_err("told of")
}

fn answering(answers: &[Option<&str>]) {
    let mut world = g0::World::new(0.5);
    let answers = answers.iter().map(|answer| answer.map(str::to_string));
    world.models.insert("not_to_keep".into(), answers.collect());
    g0::enter(world);
}

/// `items` of `user_id` with the judge answering `answers` in order (None:
/// unanswered), and what was asked of it.
async fn told(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    shown: &[&str],
    answers: &[Option<&str>],
) -> (Option<Vec<usize>>, Vec<(String, String)>) {
    let (out, asks) = told_or_failed(db, user_id, shown, answers).await;
    (out.ok().map(|out| out.into_iter().collect()), asks)
}

async fn told_or_failed(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    shown: &[&str],
    answers: &[Option<&str>],
) -> (Result<BTreeSet<usize>, Failure>, Vec<(String, String)>) {
    answering(answers);
    let out = to_leave_out(db, user_id, &items(shown)).await;
    let log = g0::leave().unwrap().log;
    let asks = log
        .iter()
        .filter(|entry| entry["kind"] == "model")
        .map(|entry| {
            let of = |key: &str| entry["what"][key].as_str().unwrap_or_default().to_string();
            (of("system"), of("input"))
        })
        .collect();
    (out, asks)
}

#[tokio::test]
async fn what_they_asked_is_judged_only_of_them_and_only_when_they_asked() {
    let Some(schema) = asked_in("asked_judged").await else {
        return;
    };
    let db = &schema.db;
    let two = ["跟楼上那位又闹僵了", "周末去爬山"];
    // No one who asked nothing, and nothing to keep: not asked at all.
    let (out, asks) = told(db, OTHER, &two, &[]).await;
    assert_eq!((out, asks.len()), (Some(vec![]), 0));
    let (out, asks) = told(db, THEM, &[], &[]).await;
    assert_eq!((out, asks.len()), (Some(vec![]), 0));
    let (out, asks) = told(db, THEM, &two, &[Some(r#"{"drop":[0]}"#)]).await;
    assert_eq!((out, asks.len()), (Some(vec![0]), 1));
    // What they asked is in what is judged, never in what is asked to do.
    assert!(!asks[0].0.contains(ASK) && asks[0].1.contains(ASK));

    // All of it: looked at again, and only what both say.
    let all = r#"{"drop":[0,1]}"#;
    let (out, asks) = told(db, THEM, &two, &[Some(all), Some(r#"{"drop":[0]}"#)]).await;
    assert_eq!((out, asks.len()), (Some(vec![0]), 2));
    let (out, _) = told(db, THEM, &two, &[Some(all), Some(all)]).await;
    assert_eq!(out, Some(vec![0, 1]));
    let (out, _) = told(db, THEM, &two, &[Some(all), None]).await;
    assert_eq!(out, None);

    // More than is asked about at once: in parts, each by its own indexes.
    let many: Vec<String> = (0..ITEMS_PER_ASK + 2).map(|n| format!("第{n}件")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    let answers = [Some(r#"{"drop":[3]}"#), Some(r#"{"drop":[1]}"#)];
    let (out, asks) = told(db, THEM, &many, &answers).await;
    assert_eq!((out, asks.len()), (Some(vec![3, ITEMS_PER_ASK + 1]), 2));
    schema.drop().await;
}

/// What she already holds, in more than one part: what a part was told
/// stands whatever comes of the next, one refused is asked about one by one,
/// and once all of a part is refused, the next is not till any is answered.
#[tokio::test]
async fn each_part_stands_whatever_comes_of_the_next() {
    let Some(schema) = asked_in("asked_parts").await else {
        return;
    };
    let db = &schema.db;
    let many: Vec<String> = (0..ITEMS_PER_ASK + 2).map(|n| format!("第{n}件")).collect();
    let judging_of = async |many: &[String], answers: Vec<&str>| {
        let mut world = g0::World::new(0.5);
        world.models.insert(
            "not_to_keep".into(),
            answers.iter().map(|one| Some(one.to_string())).collect(),
        );
        g0::enter(world);
        let soon = Instant::now() + Duration::from_secs(60);
        let told = told_of(db, THEM, None, many, soon).await;
        let log = g0::leave().unwrap().log;
        let asks = log.iter().filter(|entry| entry["kind"] == "model").count();
        (told, asks)
    };
    let judging = async |answers: Vec<&str>| judging_of(&many, answers).await;
    let (none, one) = (r#"{"drop":[]}"#, r#"{"drop":[0]}"#);
    let rest: BTreeSet<usize> = (ITEMS_PER_ASK..ITEMS_PER_ASK + 2).collect();
    // The second not to be reached.
    let (told, asks) = judging(vec![r#"{"drop":[3]}"#, "!rejected"]).await;
    assert_eq!((told.out, told.untold, asks), ([3].into(), rest, 2));
    assert!(!told.failure.unwrap().untold());
    // The first refused: one by one, the first of it and the last refused.
    let mut answers = vec!["!refused", one, one];
    answers.extend(vec![none; ITEMS_PER_ASK - 2]);
    answers.extend(["!refused", r#"{"drop":[1]}"#]);
    let (told, _) = judging(answers).await;
    assert_eq!(told.out, [0, ITEMS_PER_ASK + 1].into());
    assert_eq!(told.untold, [ITEMS_PER_ASK - 1].into());
    assert!(told.failure.unwrap().untold());
    // Nothing answered yet, all of the next refused: one by one too once
    // something is.
    let three: Vec<String> = (0..=2 * ITEMS_PER_ASK)
        .map(|n| format!("第{n}件"))
        .collect();
    let mut answers = vec!["!refused"; ITEMS_PER_ASK + 2];
    answers.push(none);
    answers.extend([one, one, "!refused"]);
    answers.extend(vec![none; ITEMS_PER_ASK - 2]);
    let (told, asks) = judging_of(&three, answers).await;
    assert_eq!(asks, 2 * ITEMS_PER_ASK + 4);
    assert_eq!(told.out, [ITEMS_PER_ASK].into());
    let mut untold: BTreeSet<usize> = (0..ITEMS_PER_ASK).collect();
    untold.insert(ITEMS_PER_ASK + 1);
    assert_eq!(told.untold, untold);
    // All of it refused.
    let (told, asks) = judging(vec!["!refused"; 2 * many.len()]).await;
    assert_eq!((told.out.len(), told.untold.len()), (0, many.len()));
    assert_eq!(asks, ITEMS_PER_ASK + 2);
    schema.drop().await;
}

/// One of what is about to be kept refused each time: it is not kept, and
/// counted; the rest is kept as told, there and then. None of it told, or
/// one that could not be asked about, none is kept now.
#[tokio::test]
async fn one_refused_holds_up_none_beside_it() {
    let Some(schema) = asked_in("asked_one_refused").await else {
        return;
    };
    let db = &schema.db;
    let three = ["周末去爬山", "不能说的那句", "跟楼上那位又闹僵了"];
    let (refused, kept, goes) = (
        Some("!refused"),
        Some(r#"{"drop":[]}"#),
        Some(r#"{"drop":[0]}"#),
    );
    let odd = Some("没有");
    let not_written = || counts()["notWritten"].as_u64().unwrap();
    let before = not_written();
    // Once more, and, refused still, one told already, whatever it says now.
    let answers = [refused, kept, refused, kept, refused, goes];
    let (out, asks) = told(db, THEM, &three, &answers).await;
    assert_eq!((out, asks.len()), (Some(vec![1]), 6));
    assert!(asks[5].1.contains(three[0]) && !asks[5].1.contains(three[1]));
    assert!(not_written() > before);
    // Told once more, it is as told.
    let (out, asks) = told(db, THEM, &three, &[refused, kept, refused, kept, kept]).await;
    assert_eq!((out, asks.len()), (Some(vec![]), 5));
    let answers = [refused, kept, refused, goes, goes, goes, goes];
    let (out, _) = told(db, THEM, &three, &answers).await;
    assert_eq!(out, Some(vec![1, 2]));
    // Nothing answered at last: none of it is let go of now, when it comes
    // again; what was told stands, when it does not.
    for last in [refused, odd, Some("!quota"), None] {
        let mut answers = vec![refused, kept, refused, kept, refused];
        answers.extend([last; 3]);
        let failure = failed(db, &three, &answers).await;
        assert!(failure.untold() && failure.refused(), "{last:?}");
        let (out, _) = told(db, THEM, &three, &answers).await;
        assert_eq!(out, Some(vec![1]), "{last:?}");
        let answers = [
            refused, kept, refused, goes, goes, last, odd, odd, last, last, last,
        ];
        let failure = failed(db, &three, &answers).await;
        assert!(failure.untold() && failure.refused(), "{last:?}");
        let (out, _) = told(db, THEM, &three, &answers).await;
        assert_eq!(out, Some(vec![1, 2]), "{last:?}");
    }
    // Asked of one told alone, when any was, not of one told with others.
    let more: Vec<String> = (0..ITEMS_PER_ASK + 2).map(|n| format!("第{n}件")).collect();
    let more: Vec<&str> = more.iter().map(String::as_str).collect();
    let answers = [kept, refused, refused, kept, refused, kept];
    let (out, asks) = told(db, THEM, &more, &answers).await;
    assert_eq!((out, asks.len()), (Some(vec![ITEMS_PER_ASK]), 6));
    assert!(asks[5].1.contains(more[ITEMS_PER_ASK + 1]) && !asks[5].1.contains("第0件"));
    // One that never reads, the same, and not asked about once more.
    let (out, _) = told(db, THEM, &three, &[refused, kept, odd, odd, odd, kept]).await;
    assert_eq!(out, Some(vec![1]));
    // One alone in its part, answered at last, is as told; never, as any.
    let many: Vec<String> = (0..=ITEMS_PER_ASK).map(|n| format!("第{n}件")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    let (out, _) = told(db, THEM, &many, &[kept, odd, odd, kept]).await;
    assert_eq!(out, Some(vec![]));
    let (out, asks) = told(db, THEM, &many, &[kept, odd, odd, odd, kept]).await;
    assert_eq!((out, asks.len()), (Some(vec![ITEMS_PER_ASK]), 5));
    failed(db, &many, &[kept, odd, odd, odd, refused]).await;
    let (out, _) = told(db, THEM, &many, &[kept, odd, odd, odd, refused]).await;
    assert_eq!(out, Some(vec![ITEMS_PER_ASK]));
    // Out of time before it was asked again: not let go of, but one that did
    // not read all the same. With nothing but no answer in time, none.
    for (answers, delays, counted, as_refused) in [
        (vec![refused, kept, odd], vec![0.0, 0.0, 0.6], true, true),
        // As it was to be asked about once more.
        (
            vec![refused, kept, refused, kept],
            vec![0.0, 0.0, 0.0, 0.6],
            true,
            true,
        ),
        (vec![odd, odd, odd], vec![0.6, 0.0, 0.0], true, false),
        (vec![kept, kept, kept], vec![60.0, 0.0, 0.0], false, false),
    ] {
        let mut world = g0::World::new(0.5);
        let answers = answers.into_iter().map(|one| one.map(str::to_string));
        world.models.insert("not_to_keep".into(), answers.collect());
        world.delays.insert("not_to_keep".into(), delays.into());
        g0::enter(world);
        let until = Instant::now() + LEAST + Duration::from_millis(400);
        let by = to_leave_out_of(db, THEM, None, &items(&three), until).await;
        g0::leave();
        let failure = by.expect_err("out of time");
        assert_eq!((failure.untold(), failure.refused()), (counted, as_refused));
    }
    for last in [refused, Some("!rejected"), None] {
        let (out, _) = told(db, THEM, &three, &[refused, refused, refused, last]).await;
        assert_eq!(out, None);
    }
    for last in [Some("!rejected"), None] {
        let (out, _) = told(db, THEM, &three, &[refused, kept, refused, last]).await;
        assert_eq!(out, None);
    }
    schema.drop().await;
}

#[tokio::test]
async fn an_answer_that_is_not_one_is_asked_again_and_never_lets_anything_through() {
    let Some(schema) = asked_in("asked_unanswered").await else {
        return;
    };
    let db = &schema.db;
    let two = ["跟楼上那位又闹僵了", "周末去爬山"];
    let (beyond, not_one) = (Some(r#"{"drop":[2]}"#), Some("没有"));
    let good = Some(r#"{"drop":[]}"#);
    let since = crate::clock::now().timestamp() - 1;
    call::judged_last(0);
    let (out, _) = told(db, THEM, &two, &[beyond, not_one, beyond]).await;
    assert_eq!(out, None);
    // None of that was an answer.
    assert!(!call::judged_since(since));
    let (out, asks) = told(db, THEM, &two, &[beyond, not_one, good]).await;
    assert_eq!((out, asks.len()), (Some(vec![]), 3));
    assert!(call::judged_since(since));
    let (out, asks) = told(db, THEM, &two, &[beyond, not_one, beyond, good]).await;
    // Then one by one, and the second could not be asked about.
    assert_eq!((out, asks.len()), (None, 5));
    let (out, _) = told(db, THEM, &two, &[]).await;
    assert_eq!(out, None);
    schema.drop().await;
}

/// Told in parts: one that could not be asked about is asked again whatever
/// the rest, in whichever order they came; what got no answer that will do
/// counts all the same, and is never let be for it.
#[test]
fn what_could_not_be_asked_about_hides_no_answer_that_will_not_do() {
    let part = |failure: CallFailure, shown: Option<CallFailure>| Told {
        out: [0].into(),
        untold: [1].into(),
        failure: Some(failure.into()),
        shown,
    };
    let refused = || part(CallFailure::Refused, Some(CallFailure::Refused));
    let away = || part(CallFailure::Timeout, None);
    for away_first in [false, true] {
        for gives_up in [false, true] {
            let mut parts = [refused(), away()];
            if away_first {
                parts.reverse();
            }
            let mut all = Told::default();
            for (nth, told) in parts.into_iter().enumerate() {
                all.and(told, 10 * nth);
            }
            assert_eq!(all.out, [0, 10].into());
            assert_eq!(all.untold, [1, 11].into());
            let failure = all.let_be(gives_up).expect_err("not all asked about");
            assert!(!failure.untold() && !failure.refused());
            assert_eq!(failure.counted(), Some(CallFailure::Refused));
        }
    }
    // None of it shown: as it was. All of it shown: let be in the end.
    let failure = away().let_be(true).expect_err("not asked about");
    assert!(matches!(failure, Failure::Model(CallFailure::Timeout)));
    assert_eq!(failure.counted(), None);
    let failure = refused().let_be(false).expect_err("not told");
    assert_eq!(failure.counted(), Some(CallFailure::Refused));
    assert!(failure.untold() && refused().let_be(true).is_ok());
}

/// What a line comes to loses what they asked not to keep, in its place:
/// what they take back, what they ask now and the rest stay as they were.
#[tokio::test]
async fn only_what_they_asked_not_to_keep_goes_from_a_line() {
    let Some(schema) = asked_in("asked_of_a_line").await else {
        return;
    };
    let update = |fact: Option<&str>, replaces: &[&str]| ChatMemoryUpdate {
        standing: Default::default(),
        topic: None,
        cues: Vec::new(),
        fact: fact.map(str::to_string),
        supersedes: replaces.iter().map(|old| old.to_string()).collect(),
        evidence: fact.map(str::to_string),
        concepts: Vec::new(),
    };
    let said = |text: &str| Said {
        said: text.into(),
        evidence: text.into(),
        hers: false,
        concepts: Vec::new(),
    };
    let before = ChatMemoryUpdates {
        updates: vec![
            update(None, &["以前的一件事"]),
            update(Some("又跟房东闹翻"), &["跟房东还行"]),
            update(Some("不要记自己的工资"), &[]),
            update(Some("周末去爬山"), &["周末在家"]),
        ],
        said: vec![said("劝他别跟房东计较"), said("答应陪他选鞋")],
        put_onto: vec!["一本讲租房的书".into(), "爬山的路线".into()],
        asked: [2].into(),
        said_new: SaidNew::Something,
        taken_back_in: [(0, 12)].into(),
        said_from: Default::default(),
    };
    let mut updates = before.clone();
    let mut world = g0::World::new(0.5);
    // Judged: the two facts, what she said, what they put her onto.
    world
        .models
        .entry("not_to_keep".into())
        .or_default()
        .push_back(Some(r#"{"drop":[0,2,4]}"#.into()));
    g0::enter(world);
    leave_out_of(&schema.db, THEM, &[], &mut updates, None)
        .await
        .unwrap();
    g0::leave();
    assert_eq!(updates.updates[1], update(None, &[]));
    for kept in [0, 2, 3] {
        assert_eq!(updates.updates[kept], before.updates[kept]);
    }
    assert_eq!(updates.said, vec![said("答应陪他选鞋")]);
    assert_eq!(updates.put_onto, vec!["爬山的路线".to_string()]);
    assert_eq!(updates.asked, before.asked);

    // A fact refused each time goes, and the rest stays. Once more refused,
    // and one told already told again.
    let mut updates = before.clone();
    let mut world = g0::World::new(0.5);
    let told = || Some(r#"{"drop":[]}"#.to_string());
    let refused = || Some("!refused".to_string());
    let mut answers = vec![refused(), refused()];
    answers.extend((0..5).map(|_| told()));
    answers.extend([refused(), told()]);
    world.models.insert("not_to_keep".into(), answers.into());
    g0::enter(world);
    let not_taken_back = || counts()["notTakenBack"].as_u64().unwrap();
    let counted = not_taken_back();
    leave_out_of(&schema.db, THEM, &[], &mut updates, None)
        .await
        .unwrap();
    g0::leave();
    // What is only taken back beside it stands, counted; what another is
    // told in place of goes.
    assert!(not_taken_back() > counted);
    for gone in [0, 1] {
        assert_eq!(updates.updates[gone], update(None, &[]));
    }
    for kept in [2, 3] {
        assert_eq!(updates.updates[kept], before.updates[kept]);
    }
    assert_eq!(leave_what_stands(&mut before.clone()), 1);
    assert_eq!(leave_what_stands(&mut updates), 0);
    assert_eq!((updates.said.len(), updates.put_onto.len()), (2, 2));

    // Unanswered, none of it is kept now.
    let mut updates = before.clone();
    g0::enter(g0::World::new(0.5));
    let left = leave_out_of(&schema.db, THEM, &[], &mut updates, None).await;
    g0::leave();
    assert!(left.is_err());
    schema.drop().await;
}
