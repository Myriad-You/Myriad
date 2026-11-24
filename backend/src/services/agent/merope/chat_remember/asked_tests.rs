//! What they ask her not to keep, as a line in a group and a private talk
//! gone over give it: the model's answers given by the test
//! (`channel_group::g0`).

use super::*;
use crate::services::agent::memory::unified::{self, Audience};
use crate::services::agent::merope::asked::tests::asks;
use crate::services::agent::merope::memory_jobs::Effect;
use crate::services::channel_group::g0;
use sea_orm::ConnectionTrait;

const THEM: i32 = 7;
const LINE: &str = "体重的事你以后都别记了，对了我养了只龟";

async fn fixture(prefix: &str) -> Option<crate::db::IsolatedSchema> {
    let url = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"])?;
    let schema = crate::db::IsolatedSchema::migrated(&url, prefix).await;
    schema
        .db
        .execute_unprepared("INSERT INTO users (id, username) VALUES (7, 'seven')")
        .await
        .unwrap();
    Some(schema)
}

/// An answer that stops at its output limit.
const CUT: &str = "CUT";

/// `line` in a group, written down with the model answering so, and what
/// was asked of it as each operation.
async fn in_group(
    db: &sea_orm::DatabaseConnection,
    line: &str,
    answers: &[(&str, &str)],
) -> (
    Result<Effect, crate::services::agent::merope::memory_jobs::Failure>,
    Vec<(String, String)>,
) {
    in_group_tried(db, line, answers, Tried::Again).await
}

async fn in_group_tried(
    db: &sea_orm::DatabaseConnection,
    line: &str,
    answers: &[(&str, &str)],
    tried: Tried,
) -> (
    Result<Effect, crate::services::agent::merope::memory_jobs::Failure>,
    Vec<(String, String)>,
) {
    let mut world = g0::World::new(0.5);
    for (operation, answer) in answers {
        if *answer == CUT {
            *world.cut_off.entry(operation.to_string()).or_default() += 1;
        }
        world
            .models
            .entry(operation.to_string())
            .or_default()
            .push_back(Some(answer.to_string()));
    }
    g0::enter(world);
    let effect = extract(
        db,
        THEM,
        line,
        "",
        crate::clock::now().fixed_offset(),
        &Audience::group("qq:42", THEM),
        &Default::default(),
        tried,
    )
    .await;
    let log = g0::leave().unwrap().log;
    let asked = log
        .iter()
        .filter(|entry| entry["kind"] == "model")
        .map(|entry| {
            let of = |key: &str| entry["what"][key].as_str().unwrap_or_default().to_string();
            (of("operation"), of("system"))
        })
        .collect();
    (effect, asked)
}

fn answered(facts: serde_json::Value, not_to_keep: serde_json::Value) -> String {
    serde_json::json!({
        "facts": facts, "supersedes": [], "supersedesEvidence": null, "said": [],
        "own": [], "putOnto": [], "notToKeep": not_to_keep,
    })
    .to_string()
}

/// What a line in a group asks is kept as that, never as a fact, and the
/// rest of the line as ever.
#[tokio::test]
async fn a_line_in_a_group_asking_her_not_to_keep_something_is_taken_as_that() {
    let Some(schema) = fixture("asked_in_group").await else {
        return;
    };
    let answer = answered(
        serde_json::json!([{"fact": "养了只龟", "evidence": "我养了只龟", "concepts": []}]),
        serde_json::json!([{"fact": "不要记自己的体重", "evidence": "体重的事你以后都别记了"}]),
    );
    let (effect, asked) = in_group(
        &schema.db,
        LINE,
        &[
            ("chat_remember", &answer),
            ("not_to_keep", r#"{"notAsked":[]}"#),
            ("not_to_keep", r#"{"drop":[]}"#),
        ],
    )
    .await;
    let Ok(Effect::Chat(updates)) = effect else {
        panic!("kept");
    };
    let facts: Vec<_> = updates
        .updates
        .iter()
        .map(|update| update.fact.clone().unwrap())
        .collect();
    assert_eq!(facts, ["养了只龟", "不要记自己的体重"]);
    assert_eq!(updates.asked, [1].into());
    assert_eq!(asked.len(), 3);
    schema.drop().await;
}

/// Read as asking what it does not ask, it is written down again with
/// nothing of asking, so what it tells is kept.
#[tokio::test]
async fn a_line_in_a_group_asking_no_such_thing_is_kept_as_ever() {
    let Some(schema) = fixture("not_asked_in_group").await else {
        return;
    };
    let line = "我买房了，以后群里别说我买了房";
    let read = answered(
        serde_json::json!([]),
        serde_json::json!([{"fact": "不要记自己买房的事", "evidence": "以后群里别说我买了房"}]),
    );
    let plain = answered(
        serde_json::json!([{"fact": "买了房", "evidence": "我买房了", "concepts": []}]),
        serde_json::json!([]),
    );
    let (effect, asked) = in_group(
        &schema.db,
        line,
        &[
            ("chat_remember", &read),
            ("not_to_keep", r#"{"notAsked":[0]}"#),
            ("chat_remember", &plain),
        ],
    )
    .await;
    let Ok(Effect::Chat(updates)) = effect else {
        panic!("kept");
    };
    assert_eq!(updates.updates.len(), 1);
    assert_eq!(updates.updates[0].fact.as_deref(), Some("买了房"));
    assert!(updates.asked.is_empty());
    let prompts: Vec<_> = asked
        .iter()
        .filter(|(operation, _)| operation == "chat_remember")
        .map(|(_, system)| system.contains("notToKeep"))
        .collect();
    assert_eq!(prompts, [true, false]);

    schema.drop().await;
}

/// What a line in a group asks, read as it is and not judged, is not let
/// go: gone over again while it can be, and with the last try kept as read.
/// What else the line gives is kept only when it is told not to be of it.
#[tokio::test]
async fn what_a_line_in_a_group_asks_is_not_let_go_unjudged() {
    let Some(schema) = fixture("asked_untold").await else {
        return;
    };
    let db = &schema.db;
    let read = answered(
        serde_json::json!([{"fact": "养了只龟", "evidence": "我养了只龟", "concepts": []}]),
        serde_json::json!([{"fact": "不要记自己的体重", "evidence": "体重的事你以后都别记了"}]),
    );
    let kept = |effect| {
        let Ok(Effect::Chat(updates)) = effect else {
            panic!("kept");
        };
        updates
            .updates
            .into_iter()
            .map(|update| update.fact.unwrap())
            .collect::<Vec<_>>()
    };
    // Not told now, however it failed and on whichever try: again later.
    let first = ("chat_remember", read.as_str());
    let told_not = |effect: Result<Effect, _>| {
        matches!(
            effect,
            Err(crate::services::agent::merope::memory_jobs::Failure::NotTold(_))
        )
    };
    let judge = |answer| ("not_to_keep", answer);
    for answer in [CUT, "嗯", "!rejected", "!quota", "!rateLimited"] {
        for tried in [Tried::Again, Tried::Last] {
            let answers = [first, judge(answer), judge(answer), judge(answer)];
            let (effect, _) = in_group_tried(db, LINE, &answers, tried).await;
            assert!(told_not(effect), "{answer}");
        }
    }
    // Asked what it asks, and not what is of it.
    let half = [first, judge(r#"{"notAsked":[]}"#)];
    let (effect, _) = in_group_tried(db, LINE, &half, Tried::Last).await;
    assert!(told_not(effect));
    // Given up on: what they ask alone.
    let refused = [first, judge("!refused"), judge("!refused")];
    let (effect, _) = in_group_tried(db, LINE, &refused, Tried::GaveUp).await;
    assert_eq!(kept(effect), ["不要记自己的体重"]);
    // Not for who asks, nor for no answer at all.
    for answers in [&half[..1], &[first, judge("!rejected")][..]] {
        let (effect, _) = in_group_tried(db, LINE, answers, Tried::GaveUp).await;
        assert!(told_not(effect));
    }
    // With what it would have replaced left as it is.
    let replacing = serde_json::json!({
        "facts": [{"fact": "体重六十公斤", "evidence": "体重", "concepts": []}],
        "supersedes": ["养了只猫"], "supersedesEvidence": LINE, "said": [],
        "own": [], "putOnto": [],
        "notToKeep": [{"fact": "不要记自己的体重", "evidence": "体重的事你以后都别记了"}],
    })
    .to_string();
    unified::remember(
        db,
        unified::NewMemory {
            user_id: THEM,
            kind: unified::MemoryKind::Fact,
            content: "养了只猫".into(),
            evidence: None,
            speaker: unified::Speaker::User,
            source: unified::SHORT_NOTE_SOURCE,
            audience: Audience::group("qq:42", THEM),
            importance: 0.5,
            concepts: Vec::new(),
        },
    )
    .await
    .unwrap()
    .unwrap();
    let refused = judge("!refused");
    let last = [("chat_remember", replacing.as_str()), refused, refused];
    let not_taken_back = || {
        crate::services::agent::merope::not_to_keep_counts()["notTakenBack"]
            .as_u64()
            .unwrap()
    };
    let before = not_taken_back();
    let (effect, _) = in_group_tried(db, LINE, &last, Tried::GaveUp).await;
    assert!(not_taken_back() > before);
    let Ok(Effect::Chat(updates)) = effect else {
        panic!("what they ask is kept");
    };
    assert!(updates.updates.iter().all(|one| one.supersedes.is_empty()));
    assert_eq!(updates.updates.len(), 1);
    // An answer that is not one, time after time, then what is of it told.
    let untold = r#"{"notAsked":[3]}"#;
    let answers = [
        ("chat_remember", read.as_str()),
        ("not_to_keep", untold),
        ("not_to_keep", untold),
        ("not_to_keep", untold),
        ("not_to_keep", r#"{"drop":[]}"#),
    ];
    let (effect, _) = in_group_tried(db, LINE, &answers, Tried::GaveUp).await;
    assert_eq!(kept(effect), ["养了只龟", "不要记自己的体重"]);
    // Of two things, the one told, and not the one refused.
    let two = answered(
        serde_json::json!([
            {"fact": "养了只龟", "evidence": "我养了只龟", "concepts": []},
            {"fact": "今天没睡好", "evidence": "没睡好", "concepts": []},
        ]),
        serde_json::json!([{"fact": "不要记自己的体重", "evidence": "体重的事你以后都别记了"}]),
    );
    let answers = [
        ("chat_remember", two.as_str()),
        judge(r#"{"notAsked":[]}"#),
        refused,
        refused,
        judge(r#"{"drop":[]}"#),
        // Once more, and one told already.
        refused,
        judge(r#"{"drop":[]}"#),
    ];
    let line = format!("{LINE}，没睡好");
    for tried in [Tried::Again, Tried::GaveUp] {
        let (effect, _) = in_group_tried(db, &line, &answers, tried).await;
        let Ok(Effect::Chat(updates)) = effect else {
            panic!("kept");
        };
        assert!(updates.updates.iter().all(|one| one.supersedes.is_empty()));
        let facts: Vec<String> = (updates.updates.into_iter())
            .filter_map(|update| update.fact)
            .collect();
        assert_eq!(facts, ["今天没睡好", "不要记自己的体重"]);
    }
    schema.drop().await;
}

/// What they ask in a line holds for what else the line gives, and only
/// that line's judge need say so.
#[tokio::test]
async fn what_a_line_asks_holds_for_the_rest_of_it() {
    let Some(schema) = fixture("asked_of_itself").await else {
        return;
    };
    let read = answered(
        serde_json::json!([
            {"fact": "体重六十二公斤", "evidence": "我六十二公斤", "concepts": []},
            {"fact": "养了只龟", "evidence": "养了只龟", "concepts": []},
        ]),
        serde_json::json!([{"fact": "不要记自己的体重", "evidence": "体重你以后都别记"}]),
    );
    let (effect, asked) = in_group(
        &schema.db,
        "我六十二公斤，体重你以后都别记，对了养了只龟",
        &[
            ("chat_remember", &read),
            ("not_to_keep", r#"{"notAsked":[]}"#),
            ("not_to_keep", r#"{"drop":[0]}"#),
        ],
    )
    .await;
    let Ok(Effect::Chat(updates)) = effect else {
        panic!("kept");
    };
    let facts: Vec<_> = updates
        .updates
        .iter()
        .filter_map(|update| update.fact.as_deref())
        .collect();
    assert_eq!(facts, ["养了只龟", "不要记自己的体重"]);
    assert_eq!(asked.len(), 3);
    schema.drop().await;
}

/// Of two they are read as asking, one they do not: written down again
/// plainly, and the other still holds, for what is written down too.
#[tokio::test]
async fn of_what_a_line_is_read_as_asking_only_what_it_asks_holds() {
    let Some(schema) = fixture("asked_in_part").await else {
        return;
    };
    let line = "我六十二公斤，体重以后别记，还有我买房了群里别说";
    let read = answered(
        serde_json::json!([]),
        serde_json::json!([
            {"fact": "不要记自己的体重", "evidence": "体重以后别记"},
            {"fact": "不要记自己买房", "evidence": "我买房了群里别说"},
        ]),
    );
    let plain = answered(
        serde_json::json!([
            {"fact": "体重六十二公斤", "evidence": "我六十二公斤", "concepts": []},
            {"fact": "买了房", "evidence": "我买房了", "concepts": []},
        ]),
        serde_json::json!([]),
    );
    let (effect, _) = in_group(
        &schema.db,
        line,
        &[
            ("chat_remember", &read),
            ("not_to_keep", r#"{"notAsked":[1]}"#),
            ("chat_remember", &plain),
            ("not_to_keep", r#"{"drop":[0]}"#),
        ],
    )
    .await;
    let Ok(Effect::Chat(updates)) = effect else {
        panic!("kept");
    };
    let facts: Vec<_> = updates
        .updates
        .iter()
        .filter_map(|update| update.fact.as_deref())
        .collect();
    assert_eq!(facts, ["买了房", "不要记自己的体重"]);
    schema.drop().await;
}

/// A private talk gone over is shown what they asked, in its own place and
/// not among what she knows of them; what they ask in it is kept as that,
/// and what they take back of it is let go.
#[tokio::test]
async fn a_private_talk_gone_over_gives_what_they_ask_in_it() {
    let Some(schema) = fixture("asked_gone_over").await else {
        return;
    };
    let db = &schema.db;
    asks(db, THEM, "不要记和妹妹吵架的事").await;
    let spoken = |id: i32, theirs: bool, text: &str| settle_raw::Spoken {
        id,
        theirs,
        text: text.into(),
        at: crate::clock::now().fixed_offset(),
        gone: false,
        in_game: false,
    };
    let go_over = async |asking: &str| {
        let lines = [
            spoken(1, true, asking),
            spoken(2, false, "好"),
            spoken(3, true, "和妹妹吵架的事倒是可以记了"),
        ];
        let answer = serde_json::json!({
            "lasting": [],
            "tookBack": [{"entry": "不要记和妹妹吵架的事", "said": 2}],
            "moment": [], "known": [], "letGo": [], "said": [], "own": [], "putOnto": [],
            "notToKeep": [{
                "fact": "不要记自己的体重", "evidence": "体重的事你以后都别记了",
                "said": 0, "replaces": [],
            }],
        })
        .to_string();
        let mut world = g0::World::new(0.5);
        world
            .models
            .entry("chat_settle".into())
            .or_default()
            .push_back(Some(answer));
        g0::enter(world);
        let gone = settle_raw::go_over(
            db,
            THEM,
            &Audience::private(THEM),
            &lines,
            "2026-11-23 (Monday)",
            None,
            &[],
            None,
        )
        .await;
        let log = g0::leave().unwrap().log;
        (gone, log)
    };
    // A line that says more than that is judged as every line is, not let go.
    let (gone, _) = go_over("今天累得不行，体重的事你以后都别记了").await;
    let gone = gone.unwrap_or_else(|_| panic!("gone over"));
    assert_eq!(gone.updates.asked, [1].into());
    assert!(gone.let_go.is_empty());
    let (gone, log) = go_over("体重的事你以后都别记了。").await;
    let gone = gone.unwrap_or_else(|_| panic!("gone over"));
    assert_eq!(gone.left, 0);
    assert_eq!(gone.let_go, [1]);
    assert_eq!(gone.updates.asked, [1].into());
    assert_eq!(gone.updates.updates[0].supersedes, ["不要记和妹妹吵架的事"]);
    assert_eq!(gone.updates.taken_back_in, [(0, 3)].into());
    assert_eq!(
        gone.updates.updates[1].fact.as_deref(),
        Some("不要记自己的体重")
    );
    assert_eq!(gone.from, [vec![2], vec![0]]);
    let system = log
        .iter()
        .find(|entry| entry["kind"] == "model" && entry["what"]["operation"] == "chat_settle")
        .and_then(|entry| entry["what"]["system"].as_str())
        .unwrap();
    assert!(system.ends_with("Asked not to keep:\n- 不要记和妹妹吵架的事"));
    assert_eq!(system.matches("- 不要记和妹妹吵架的事").count(), 1);

    // Written: what they asked now holds, and what they took back does until
    // a second look agrees that they did.
    use sea_orm::TransactionTrait;
    let tx = db.begin().await.unwrap();
    let kept = crate::services::agent::merope::store::settle_chat_memory_on(
        &tx,
        THEM,
        crate::clock::now().fixed_offset(),
        &gone.updates,
        &[],
        &Audience::private(THEM),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert!(kept);
    let held = async || -> std::collections::BTreeSet<String> {
        unified::asked_not_to_keep(db, THEM)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.content)
            .collect()
    };
    assert_eq!(held().await.len(), 2);
    crate::services::agent::merope::memory_nodes::take_back_check::confirmed_now(db, THEM).await;
    assert_eq!(held().await, ["不要记自己的体重".to_string()].into());
    schema.drop().await;
}

/// Asked for over again in a private talk, in the words it is kept by, what
/// they may have let her keep (not looked at yet) is held to: that second
/// look lets it go no more.
#[tokio::test]
async fn asked_for_again_in_private_it_is_held_to() {
    use crate::services::agent::merope::memory_nodes::take_back_check;
    use sea_orm::{ConnectionTrait, TransactionTrait};
    const HELD: &str = "不要记和妹妹吵架的事";
    let Some(schema) = fixture("asked_again_gone_over").await else {
        return;
    };
    let db = &schema.db;
    asks(db, THEM, HELD).await;
    asks(db, THEM, "不要记自己的体重").await;
    let held = async || unified::asked_not_to_keep(db, THEM).await.unwrap();
    for row in held().await.iter().filter(|row| row.content == HELD) {
        take_back_check::hold_asked(db, THEM, &[row.id.clone()], Some(1))
            .await
            .unwrap();
    }
    let lines = [settle_raw::Spoken {
        id: 1,
        theirs: true,
        text: "算了，和妹妹吵架的事还是别记，体重也是".into(),
        at: crate::clock::now().fixed_offset(),
        gone: false,
        in_game: false,
    }];
    let again = |fact: &str, evidence: &str| serde_json::json!({ "fact": fact, "evidence": evidence, "said": 0, "replaces": [] });
    let answer = serde_json::json!({
        "lasting": [], "tookBack": [], "moment": [], "known": [], "letGo": [],
        "said": [], "own": [], "putOnto": [],
        "notToKeep": [
            again(HELD, "和妹妹吵架的事还是别记"),
            again("不要记自己的体重", "体重也是"),
        ],
    });
    let mut world = g0::World::new(0.5);
    world
        .models
        .insert("chat_settle".into(), [Some(answer.to_string())].into());
    g0::enter(world);
    let alone = Audience::private(THEM);
    let today = "2026-11-23 (Monday)";
    let gone = settle_raw::go_over(db, THEM, &alone, &lines, today, None, &[], None).await;
    g0::leave();
    let gone = gone.unwrap_or_else(|_| panic!("gone over"));
    // What waits for no second look is what they asked already.
    let asked: Vec<_> = (gone.updates.updates.iter())
        .map(|update| update.fact.as_deref())
        .collect();
    assert_eq!(asked, [Some(HELD)]);
    let tx = db.begin().await.unwrap();
    crate::services::agent::merope::store::settle_chat_memory_on(
        &tx,
        THEM,
        crate::clock::now().fixed_offset(),
        &gone.updates,
        &[],
        &alone,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let state: String = db
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT state FROM agent_takeback_check",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "state")
        .unwrap();
    assert_eq!(state, take_back_check::UNSURE);
    assert_eq!(held().await.len(), 2);
    schema.drop().await;
}

/// What is cached of a line's and a stretch's prompt is the same whatever
/// she knows of them and whatever they asked, and long enough to be.
#[test]
fn what_they_asked_comes_after_the_cached_part() {
    use crate::services::agent::merope::call::{REMEMBER_CACHE, SETTLE_CACHE};
    let some = ["likes tea".to_string(), "Known:\nnot one".to_string()];
    let asked = ["nothing about the landlord".to_string()];
    for examples in [Examples::Chinese, Examples::English] {
        let same = |prompts: [String; 3], until: &str| {
            let parts = prompts.map(|prompt| {
                let at = prompt.find(until).expect("the cache's end is in it");
                prompt[..at].to_string()
            });
            assert!(parts[0] == parts[1] && parts[1] == parts[2], "{until:?}");
            assert!(parts[0].len() >= 2048, "{until:?}");
            assert!(!parts[0].contains("likes tea") && !parts[0].contains("landlord"));
        };
        let line = |known| extract_system_prompt_asked(known, examples);
        same(
            [line(&[]), line(&some), line(&some[..1])],
            REMEMBER_CACHE.until.unwrap(),
        );
        let stretch = |known, asked| {
            myriad_merope::settle_raw::raw_settle_system_prompt(known, asked, examples)
        };
        same(
            [
                stretch(&[], &[]),
                stretch(&some, &asked),
                stretch(&some[..1], &[]),
            ],
            SETTLE_CACHE.until.unwrap(),
        );
    }
}

/// What a line corrects is replaced by the one new fact it tells; by none
/// of several, nor taken back then.
#[test]
fn what_is_corrected_is_replaced_by_one_new_fact_alone() {
    let update = |fact: Option<&str>, supersedes: &[&str]| ChatMemoryUpdate {
        fact: fact.map(str::to_string),
        supersedes: supersedes.iter().map(|old| old.to_string()).collect(),
        evidence: None,
        concepts: Vec::new(),
        topic: None,
        cues: Vec::new(),
        standing: Default::default(),
    };
    for (told, replaced) in [
        (&["搬到了苏州"][..], &["住在杭州"][..]),
        (&["搬到了苏州", "养了只猫"][..], &[][..]),
        (&[][..], &[][..]),
    ] {
        let mut updates = ChatMemoryUpdates {
            updates: vec![update(None, &["住在杭州"])],
            said_new: SaidNew::Something,
            ..Default::default()
        };
        updates
            .updates
            .extend(told.iter().map(|fact| update(Some(fact), &[])));
        updates.correct_by_what_is_told();
        assert!(updates.updates[0].supersedes.is_empty());
        let replaces: Vec<&String> = updates
            .updates
            .iter()
            .flat_map(|update| &update.supersedes)
            .collect();
        assert_eq!(replaces, replaced, "{told:?}");
    }
}
