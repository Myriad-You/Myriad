//! Asked to be left alone for a while (`left_alone`): taken with the intents,
//! held till it ends, and what came due meanwhile (`late`), on a database of
//! their own, as `intent_tests`. Every word here is made up.

use chrono::{DateTime, Duration, TimeZone, Utc};
use myriad_nodes::body::{IntentStatus, Origin};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use super::talk_tests;
use super::{audience, intending, intent_store, intents_due, late};
use crate::services::agent::memory::{manage, unified};
use crate::services::agent::merope::left_alone::{self, Standing};
use crate::services::agent::speech::{self, Source, admit};
use crate::services::channel_group::g0;
use crate::services::her_log::pen;

const SESSION: &str = "s_alone";

/// A schema of its own with account `who`, which no other test here has: what
/// the process remembers of what waited is kept by account.
async fn schema(prefix: &str, who: i32) -> Option<crate::db::IsolatedSchema> {
    let schema = talk_tests::schema(prefix).await?;
    schema
        .db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO users (id, username) VALUES ($1, $2)",
            [who.into(), format!("w{who}").into()],
        ))
        .await
        .unwrap();
    Some(schema)
}

async fn held(db: &DatabaseConnection, who: i32) -> Vec<intent_store::Held> {
    intent_store::held(db, who, &audience::private_talk(who, None))
        .await
        .unwrap()
}

fn now() -> DateTime<Utc> {
    crate::services::agent::merope::clock::now()
}

/// Their line in `who`'s private talk; its id.
async fn said(db: &DatabaseConnection, who: i32, text: &str) -> i32 {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) VALUES ($1, $3, 't', $2, 0, false, NOW(), NOW()) \
         ON CONFLICT (id) DO NOTHING",
        [SESSION.into(), json!({"mode": "chat"}).into(), who.into()],
    ))
    .await
    .unwrap();
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_messages (session_id, role, content, created_at) \
         VALUES ($1, 'user', $2, NOW()) RETURNING id",
        [SESSION.into(), text.into()],
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get("", "id")
    .unwrap()
}

/// They said `text` and it was judged with `marks` and `take`; its message,
/// and what she was asked.
async fn judged(
    db: &DatabaseConnection,
    who: i32,
    text: &str,
    take: Value,
    marks: Value,
) -> (i32, Value) {
    let id = said(db, who, text).await;
    let mut world = g0::World::new(0.5);
    world.base = crate::clock::now();
    world
        .models
        .entry(intending::OPERATION.to_string())
        .or_default()
        .push_back(Some(
            json!({"take": take, "drop": [], "marks": marks}).to_string(),
        ));
    g0::enter(world);
    intending::after_turn(db, who, None, SESSION, text, now())
        .await
        .unwrap();
    let asked = g0::leave()
        .unwrap()
        .log
        .into_iter()
        .find(|line| line["kind"] == "model")
        .expect("judged");
    (id, asked["what"].clone())
}

/// What she was told she knows of them, as she was asked.
fn known(asked: &Value) -> Value {
    let input: Value = serde_json::from_str(asked["input"].as_str().unwrap()).unwrap();
    input["known"].clone()
}

/// Rows of it, closed ones too.
async fn rows(db: &DatabaseConnection) -> i64 {
    talk_tests::count(
        db,
        "SELECT COUNT(*) AS n FROM agent_memories WHERE source = 'left_alone'",
    )
    .await
}

fn end(fields: Value) -> Value {
    let mut end = json!({"how": "unsaid", "which": "none", "weekday": "none", "count": 0,
                         "edge": "none", "date": "", "time": "", "begins": "now"});
    for (key, value) in fields.as_object().unwrap() {
        end[key] = value.clone();
    }
    json!([{"kind": "bound", "end": end}])
}

fn unbound() -> Value {
    json!([{"kind": "unbound", "end": null}])
}

/// `marks` with a change of what she knows beside them.
fn changed(marks: Value) -> Value {
    let mut marks = marks;
    marks
        .as_array_mut()
        .unwrap()
        .push(json!({"kind": "change", "end": null}));
    marks
}

fn marks(marks: Value) -> left_alone::Marks {
    left_alone::Marks::of(&json!({ "marks": marks }).to_string())
}

/// How long till it is over, in hours, rounded; none when it is not held.
async fn left(db: &DatabaseConnection, who: i32) -> Option<i64> {
    match left_alone::standing(db, who).await {
        Standing::Held(until) => Some(((until - crate::clock::now()).num_minutes() + 30) / 60),
        _ => None,
    }
}

/// Something of hers to say to them unasked: whether it goes on.
async fn may(db: &DatabaseConnection, who: i32, thing: &str, called: bool) -> bool {
    let to = speech::private(who);
    let mut wish = speech::wish(to.clone(), Source::Reach, "s", to);
    wish.thing = Some(thing.into());
    wish.called = called;
    let mut world = g0::World::new(0.5);
    world.base = crate::clock::now();
    g0::enter(world);
    let may = admit::goes_on(&admit::admit(db, &wish).await);
    g0::leave();
    may
}

/// A tick at `at`: how often she was asked whether she would write, and how
/// many lines of hers were written.
struct Tick {
    asked: usize,
    wrote: usize,
    /// What she wrote about.
    about: String,
}

/// The world at `at`: she is `awake`, and is asked with `judges`; her line
/// `sent` or not.
async fn tick(
    db: &DatabaseConnection,
    at: DateTime<Utc>,
    awake: bool,
    judges: &[Option<Value>],
    sent: bool,
) -> Tick {
    tick_with(db, at, awake, judges, sent, 6).await
}

/// [`tick`], with `lines` of hers to be had, and no more.
async fn tick_with(
    db: &DatabaseConnection,
    at: DateTime<Utc>,
    awake: bool,
    judges: &[Option<Value>],
    sent: bool,
    lines: usize,
) -> Tick {
    let mut world = g0::World::new(0.5);
    world.base = at;
    world.asleep_for = (!awake).then_some(3.0);
    if sent {
        world.sendable.insert("site".into());
    }
    world
        .models
        .entry("reach_judge".into())
        .or_default()
        .extend(judges.iter().map(|one| one.as_ref().map(Value::to_string)));
    world
        .says
        .entry("intent_raise".into())
        .or_default()
        .extend((0..lines).map(|one| (0.0, Ok(format!("那片子你看了吗 {at} {one}")))));
    g0::enter(world);
    intents_due::tick_at(db, at).await;
    let world = g0::leave().unwrap();
    Tick {
        asked: world
            .log
            .iter()
            .filter(|line| line["what"]["operation"] == "reach_judge")
            .count(),
        wrote: lines - world.says["intent_raise"].len(),
        about: world
            .log
            .iter()
            .filter(|line| line["kind"] == "say")
            .filter_map(|line| line["what"]["prompt"].as_str())
            .collect(),
    }
}

/// `{"kind": "mine"}` in an hour.
fn mine(text: &str, hours: i64) -> Value {
    let at =
        (crate::clock::now() + Duration::hours(hours)).with_timezone(&chrono_tz::Asia::Shanghai);
    json!({"kind": "mine", "text": text, "action": "say",
           "at": at.format("%Y-%m-%dT%H:%M").to_string(), "after": null,
           "afterDone": false, "note": null, "echo": null})
}

async fn waiting(db: &DatabaseConnection, who: i32) -> usize {
    held(db, who)
        .await
        .iter()
        .filter(|one| one.waiting())
        .count()
}

async fn with_status(db: &DatabaseConnection, who: i32, status: IntentStatus) -> usize {
    held(db, who)
        .await
        .iter()
        .filter(|one| one.status == status)
        .count()
}

/// They asked, for three hours, and what she meant to say in an hour.
async fn alone_with_two(db: &DatabaseConnection, who: i32) {
    judged(
        db,
        who,
        "三小时内别找我",
        json!([mine("问爬山怎么样", 1), mine("说那部片子", 1)]),
        end(json!({"how": "hours", "count": 3})),
    )
    .await;
    assert_eq!(waiting(db, who).await, 2);
    // Held off while it is not over.
    let during = tick(
        db,
        crate::clock::now() + Duration::hours(2),
        true,
        &[],
        true,
    )
    .await;
    assert_eq!((during.asked, during.wrote), (0, 0));
    assert_eq!(waiting(db, who).await, 2);
}

#[tokio::test]
async fn asked_to_be_left_alone_she_says_nothing_of_hers_till_it_ends() {
    let who = 701;
    let Some(schema) = schema("memory_nodes_left_alone", who).await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    assert!(!left_alone::holds(db, who).await);
    assert!(may(db, who, "reach|x", false).await);
    let in_an_hour = (now() + Duration::hours(1)).with_timezone(&chrono_tz::Asia::Shanghai);
    let take = json!([
        {"kind": "instruction", "text": "remind them to take the pills", "action": "say",
         "at": in_an_hour.format("%Y-%m-%dT%H:%M").to_string(), "after": null,
         "afterDone": false, "note": null, "echo": "好"},
        mine("ask how the hike went", 1),
    ]);
    let (message, asked) = judged(
        db,
        who,
        "一小时后提醒我吃药，这三天别找我聊天了",
        take,
        end(json!({"how": "days", "count": 3, "which": "this"})),
    )
    .await;
    assert!(asked["system"].as_str().unwrap().contains("\"unbound\""));
    assert!(left_alone::holds(db, who).await);
    assert!(!may(db, who, "reach|x", false).await, "hers is not said");
    assert!(!may(db, who, "own|x", false).await, "nor what she meant to");
    assert!(
        may(db, who, "reach|x", true).await,
        "what they called for is"
    );
    assert!(
        may(db, who, "intent|x", false).await,
        "and what they told her to say"
    );
    // Seen with their memories, not recalled, with no word of theirs in it.
    let kept = unified::latest_of(db, who, left_alone::SOURCE, 5)
        .await
        .unwrap();
    assert_eq!(kept.len(), 1);
    assert!(kept[0].content.starts_with("→ "));
    assert!(!kept[0].content.contains("吃药"));
    // Two hours on: the reminder comes due and is said, hers waits.
    tick(db, now() + Duration::hours(2), true, &[], true).await;
    for intent in held(db, who).await {
        assert_eq!(
            intent.waiting(),
            intent.origin == Some(Origin::Her),
            "{}",
            intent.text
        );
    }
    // Its words are not to be kept, and taken back in the talk: it holds.
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_messages SET metadata = \
         '{\"takenBack\": 1, \"notKept\": true}'::json \
         WHERE id = $1",
        [message.into()],
    ))
    .await
    .unwrap();
    assert!(
        left_alone::holds(db, who).await,
        "they asked not to keep it"
    );
    // She may come to them again: over.
    let (_, asked) = judged(db, who, "没事你可以找我了", json!([]), unbound()).await;
    let told = known(&asked);
    let told = told[0].as_str().unwrap_or_default();
    assert!(told.starts_with("They asked you not to come to them until"));
    assert!(!left_alone::holds(db, who).await);
    assert!(may(db, who, "reach|x", false).await);
    // Nothing held: nothing known, and nothing to end.
    let (_, asked) = judged(db, who, "嗯", json!([]), json!([])).await;
    assert_eq!(known(&asked), json!([]));
    assert_eq!(
        rows(db).await,
        1,
        "kept a while, for what came due meanwhile"
    );
    schema.drop().await;
}

#[tokio::test]
async fn what_they_ask_last_takes_the_place_of_what_they_asked_before() {
    let who = 702;
    let Some(schema) = schema("memory_nodes_left_alone_last", who).await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    let asks = async |text: &str, marks: Value| judged(db, who, text, json!([]), marks).await.0;
    let hours = |count: i64| end(json!({"how": "hours", "count": count}));
    // No end, or only later: nothing is held off.
    asks("忙完再说", end(json!({}))).await;
    asks(
        "下周别找我",
        end(json!({"how": "week", "which": "next", "begins": "later"})),
    )
    .await;
    assert_eq!((left(db, who).await, rows(db).await), (None, 0));
    asks("三天别找我", hours(72)).await;
    assert_eq!(left(db, who).await, Some(72));
    // Another, shorter, beside it: the later of the two, whichever way.
    asks("今晚也别找", hours(1)).await;
    assert_eq!((left(db, who).await, rows(db).await), (Some(72), 1));
    asks("五天别找我", hours(120)).await;
    assert_eq!(left(db, who).await, Some(120));
    // They change it: it is that one, longer or shorter.
    asks("改成一小时就行", changed(hours(1))).await;
    assert_eq!((left(db, who).await, rows(db).await), (Some(1), 1));
    asks("两件事", {
        let mut both = hours(2);
        both.as_array_mut()
            .unwrap()
            .extend(hours(5).as_array().unwrap().clone());
        both
    })
    .await;
    assert_eq!(left(db, who).await, Some(5));
    // One with no end, or that only starts later, leaves it as it is.
    asks("忙完再说", end(json!({}))).await;
    asks(
        "明天也别找",
        end(json!({"how": "tomorrow", "begins": "later"})),
    )
    .await;
    assert_eq!(left(db, who).await, Some(5));
    // Told in the other order: the later message holds.
    let first = said(db, who, "先说的").await;
    let last = said(db, who, "后说的").await;
    let heed = async |message: i32, what: Value| {
        left_alone::heed(db, who, message, &marks(what), crate::clock::now())
            .await
            .unwrap();
    };
    heed(last, changed(hours(2))).await;
    heed(first, changed(hours(9))).await;
    heed(first, unbound()).await;
    assert_eq!(left(db, who).await, Some(2));
    // Over, or never begun, as they said last; the one before it, judged
    // after: not heeded.
    heed(last + 2, unbound()).await;
    heed(last + 1, hours(72)).await;
    assert_eq!(left(db, who).await, None);
    db.execute_unprepared("DELETE FROM agent_memories WHERE source = 'left_alone'")
        .await
        .unwrap();
    heed(last + 4, unbound()).await;
    assert_eq!((left(db, who).await, rows(db).await), (None, 1));
    heed(last + 3, hours(72)).await;
    assert_eq!(left(db, who).await, None);
    heed(last + 5, hours(2)).await;
    assert_eq!(left(db, who).await, Some(2));
    db.execute_unprepared("DELETE FROM agent_memories WHERE source = 'left_alone'")
        .await
        .unwrap();
    heed(last, changed(hours(2))).await;
    // Both in one: it is put off, not ended, whichever way.
    let mut both = hours(1);
    both.as_array_mut()
        .unwrap()
        .extend(unbound().as_array().unwrap().clone());
    asks("可以找我了，不过一小时后", both).await;
    assert_eq!(left(db, who).await, Some(1));
    asks("又要久一点", hours(4)).await;
    assert_eq!(left(db, who).await, Some(4));
    // Two months is the furthest.
    let before = left(db, who).await;
    asks(
        "2099 年以前别找我",
        end(json!({"how": "date", "date": "2099-01-01", "edge": "before"})),
    )
    .await;
    assert_eq!(left(db, who).await, before);
    // Nothing to end.
    asks("可以找我了", unbound()).await;
    let unbound_again = asks("可以找我了", unbound()).await;
    assert!(unbound_again > 0);
    assert_eq!(left(db, who).await, None);
    assert_eq!(rows(db).await, 1);
    schema.drop().await;
}

#[tokio::test]
async fn one_over_is_kept_two_days_and_one_unreadable_holds_a_week() {
    use crate::services::agent::memory::unified::Audience;
    let who = 703;
    let Some(schema) = schema("memory_nodes_left_alone_ends", who).await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    let ends = marks(end(json!({"how": "hours", "count": 2})));
    let two_hours_ago = crate::clock::now() - Duration::hours(3);
    left_alone::heed(db, who, 5, &ends, two_hours_ago)
        .await
        .unwrap();
    assert!(matches!(
        left_alone::standing(db, who).await,
        Standing::Free { ended: Some(_) }
    ));
    assert_eq!(rows(db).await, 1);
    db.execute_unprepared(
        "UPDATE agent_memories SET evidence = jsonb_set(evidence::jsonb, '{until}', \
         to_jsonb((now() - interval '3 days')::text))::text WHERE source = 'left_alone'",
    )
    .await
    .unwrap();
    // Not read as a moment: holds for now, from when it was written.
    assert!(left_alone::holds(db, who).await);
    db.execute_unprepared("DELETE FROM agent_memories WHERE source = 'left_alone'")
        .await
        .unwrap();
    left_alone::heed(db, who, 6, &ends, two_hours_ago - Duration::days(3))
        .await
        .unwrap();
    assert!(matches!(
        left_alone::standing(db, who).await,
        Standing::Free { ended: None }
    ));
    assert_eq!(rows(db).await, 0, "over a while ago: gone");
    // One whose end does not read, heard by no one.
    left_alone::heed(db, who, 7, &ends, crate::clock::now())
        .await
        .unwrap();
    db.execute_unprepared(
        "UPDATE agent_memories SET evidence = '{\"until\":\"晚点\"}' WHERE source = 'left_alone'",
    )
    .await
    .unwrap();
    assert!(left_alone::holds(db, who).await);
    assert!(!left_alone::holds(db, 1).await);
    assert!(!may(db, who, "reach|x", false).await);
    let group = speech::group("qq:42");
    let mut wish = speech::wish(group.clone(), Source::Reach, "s", group);
    wish.thing = Some("reach|y".into());
    g0::enter(g0::World::new(0.5));
    assert!(admit::goes_on(&admit::admit(db, &wish).await));
    g0::leave();
    // Its words are no one's to change, and its end is not in them.
    let id = unified::latest_of(db, who, left_alone::SOURCE, 5)
        .await
        .unwrap()[0]
        .id
        .clone();
    assert!(!manage::update_any(db, &id, "明天以前").await.unwrap());
    // As a build that knows no source of it reads it: as any of theirs.
    db.execute_unprepared("UPDATE agent_memories SET source = 'chat' WHERE source = 'left_alone'")
        .await
        .unwrap();
    for present in [Audience::private(who), Audience::group("qq:42", who)] {
        let recalled = unified::active_in(db, who, &present, &unified::MemoryKind::ABOUT_PERSON)
            .await
            .unwrap();
        assert!(recalled.is_empty());
    }
    db.execute_unprepared(
        "UPDATE agent_memories SET source = 'left_alone', created_at = now() - interval '8 days'",
    )
    .await
    .unwrap();
    assert!(!left_alone::holds(db, who).await, "not for ever");
    // The rows cannot be read: not told, she does not come to them.
    db.execute_unprepared("ALTER TABLE agent_memories RENAME TO agent_memories_gone")
        .await
        .unwrap();
    assert_eq!(left_alone::standing(db, who).await, Standing::Unknown);
    assert!(left_alone::holds(db, who).await);
    db.execute_unprepared("ALTER TABLE agent_memories_gone RENAME TO agent_memories")
        .await
        .unwrap();
    schema.drop().await;
}

/// Only their words make one: a judgment that failed or does not read makes
/// none, and is asked once more; what it read in full first stays.
#[tokio::test]
async fn a_judgment_that_does_not_read_makes_none() {
    let who = 704;
    let Some(schema) = schema("memory_nodes_left_alone_unread", who).await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    let asking = async |answers: &[Option<String>]| {
        said(db, who, "今晚别找我了").await;
        let mut world = g0::World::new(0.5);
        world.base = crate::clock::now();
        world
            .models
            .entry(intending::OPERATION.to_string())
            .or_default()
            .extend(answers.iter().cloned());
        g0::enter(world);
        let judged = intending::after_turn(db, who, None, SESSION, "今晚别找我了", now())
            .await
            .unwrap_or_default();
        let asked = g0::leave()
            .unwrap()
            .log
            .iter()
            .filter(|line| line["kind"] == "model")
            .count();
        (
            asked,
            left_alone::holds(db, who).await,
            rows(db).await,
            judged.taken.len(),
        )
    };
    let answer = |take: Value, marks: Value| {
        Some(json!({"take": take, "drop": [], "marks": marks}).to_string())
    };
    let broken = json!([{"kind": "bound", "end": {"how": "soon"}}]);
    for unread in [Some("嗯".to_string()), answer(json!([]), broken.clone())] {
        assert_eq!(asking(&[unread.clone(), unread]).await, (2, false, 0, 0));
    }
    let (asked, held, kept, _) = asking(&[None, None]).await;
    assert!((1..=2).contains(&asked));
    assert_eq!((held, kept), (false, 0));
    // Its marks alone do not read, and the second is nothing: what she took
    // the first time stays.
    let (_, held, _, taken) = asking(&[
        answer(json!([mine("问爬山怎么样", 1)]), broken),
        Some("嗯".into()),
    ])
    .await;
    assert_eq!((held, taken), (false, 1));
    // Read the second time: as they said.
    let hours = answer(json!([]), end(json!({"how": "hours", "count": 2})));
    assert_eq!(asking(&[Some("嗯".into()), hours]).await, (2, true, 1, 0));
    schema.drop().await;
}

/// What she meant to say while they were to be left alone is not said all at
/// once when that is over: whether she still would is judged, of all of it,
/// and said in one message or not at all; nothing is settled till then.
#[tokio::test]
async fn what_waited_is_judged_again_at_once_when_it_is_over() {
    let who = 705;
    let Some(schema) = schema("memory_nodes_left_alone_late", who).await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    late::restart(who);
    let would = Some(json!({"reach_out": true, "about": "那部片子"}));
    let not = Some(json!({"reach_out": false, "about": null}));
    let over = || crate::clock::now() + Duration::hours(4);

    alone_with_two(db, who).await;
    // Asleep, or asked to write in vain: nothing is settled.
    let asleep = tick(db, over(), false, &[], true).await;
    assert_eq!((asleep.asked, asleep.wrote), (0, 0));
    assert_eq!(waiting(db, who).await, 2);
    let unanswered = tick(db, over(), true, &[None], true).await;
    assert_eq!((unanswered.asked, unanswered.wrote), (1, 0));
    assert_eq!(waiting(db, who).await, 2);
    // Not at every tick.
    let soon = tick(db, over(), true, &[would.clone()], true).await;
    assert_eq!((soon.asked, waiting(db, who).await), (0, 2));
    // What comes due meanwhile does not wait with them: it is said.
    judged(db, who, "嗯", json!([mine("问那家店", 4)]), json!([])).await;
    let new = tick(db, over() + Duration::hours(1), true, &[], true).await;
    assert_eq!((new.asked, new.wrote), (0, 1));
    assert_eq!(status(db, who, "那家店").await, IntentStatus::Said);
    assert_eq!(waiting(db, who).await, 2);
    late::restart(who);
    // That she would, about nothing, is no answer.
    for about in [json!(""), json!(null)] {
        let nothing = Some(json!({"reach_out": true, "about": about}));
        let vague = tick(db, over(), true, &[nothing], true).await;
        assert_eq!((vague.asked, vague.wrote), (1, 0));
        assert_eq!(waiting(db, who).await, 2);
        late::restart(who);
    }
    // It does not go out: the same.
    let unsent = tick(db, over(), true, &[would.clone()], false).await;
    assert_eq!((unsent.asked, unsent.wrote), (1, 1));
    assert_eq!(waiting(db, who).await, 2);
    late::restart(who);
    // It does: in one message, for all of it, and not again.
    let sent = tick(db, over(), true, &[would.clone()], true).await;
    assert_eq!((sent.asked, sent.wrote), (1, 1));
    for text in ["那部片子", "问爬山怎么样", "说那部片子"] {
        assert!(sent.about.contains(text), "{}", sent.about);
    }
    assert_eq!(waiting(db, who).await, 0);
    assert_eq!(with_status(db, who, IntentStatus::Said).await, 3);
    let again = tick(db, over() + Duration::hours(1), true, &[], true).await;
    assert_eq!((again.asked, again.wrote), (0, 0));

    // She would not: let go.
    alone_with_two_more(db, who).await;
    let no = tick(db, over() + Duration::hours(2), true, &[not], true).await;
    assert_eq!((no.asked, no.wrote), (1, 0));
    assert_eq!(waiting(db, who).await, 0);
    assert_eq!(with_status(db, who, IntentStatus::Dropped).await, 2);
    schema.drop().await;
}

/// As [`alone_with_two`], whose intents are not those of another round.
async fn alone_with_two_more(db: &DatabaseConnection, who: i32) {
    judged(
        db,
        who,
        "再三小时内别找我",
        json!([mine("问搬家怎么样", 5), mine("说那本书", 5)]),
        end(json!({"how": "hours", "count": 6})),
    )
    .await;
    assert_eq!(waiting(db, who).await, 2);
}

/// Told from when each was meant for, a restart loses none of them.
#[tokio::test]
async fn a_restart_loses_none_of_what_waited() {
    let who = 706;
    let Some(schema) = schema("memory_nodes_left_alone_restart", who).await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    late::restart(who);
    alone_with_two(db, who).await;
    let would = Some(json!({"reach_out": true, "about": "那部片子"}));
    late::restart(who);
    let over = crate::clock::now() + Duration::hours(4);
    let sent = tick(db, over, true, &[would], true).await;
    assert_eq!((sent.asked, sent.wrote), (1, 1), "in one message");
    assert_eq!(waiting(db, who).await, 0);
    schema.drop().await;
}

/// An end gone by more than two days ago is told from the rest as long as
/// anything that came due before it waits.
#[tokio::test]
async fn one_over_is_kept_as_long_as_what_came_due_meanwhile_waits() {
    let who = 707;
    let Some(schema) = schema("memory_nodes_left_alone_kept", who).await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    late::restart(who);
    alone_with_two(db, who).await;
    let over = crate::clock::now() + Duration::hours(4);
    // They sleep, and it goes on for days: asked of her nothing, said nothing.
    for days in [1, 3, 9] {
        let nothing = tick(db, over + Duration::days(days), false, &[], true).await;
        assert_eq!((nothing.asked, nothing.wrote), (0, 0));
        late::restart(who);
        assert_eq!(rows(db).await, 1);
    }
    // Awake, in one message, and not one by one.
    let would = Some(json!({"reach_out": true, "about": "那部片子"}));
    let sent = tick(db, over + Duration::days(9), true, &[would], true).await;
    assert_eq!((sent.asked, sent.wrote), (1, 1));
    assert_eq!(waiting(db, who).await, 0);
    // With nothing left, it goes: as the next one is asked for.
    late::restart(who);
    let mut world = g0::World::new(0.5);
    world.base = over + Duration::days(9);
    g0::enter(world);
    left_alone::standing(db, who).await;
    g0::leave();
    assert_eq!(rows(db).await, 0);
    schema.drop().await;
}

/// Her message of what came due was said at `at`, and the run ended before
/// they were settled: they are as they were.
async fn said_unsettled(db: &DatabaseConnection, who: i32, at: DateTime<Utc>) {
    let mut left = held(db, who).await;
    left.retain(|one| one.waiting());
    left.sort_by_key(|one| (one.trigger.as_ref().unwrap()["at"].as_i64(), one.at));
    let marks: Vec<String> = left.iter().map(|one| late::mark(&one.id)).collect();
    let mut world = g0::World::new(0.5);
    world.base = at;
    world.sendable.insert("site".into());
    world
        .says
        .entry("intent_raise".into())
        .or_default()
        .push_back((0.0, Ok("那部片子".into())));
    g0::enter(world);
    let went = crate::services::agent::merope::reach::say_now(
        db,
        who,
        Source::Reach,
        &format!("{}{}", late::SAID, marks.join(",")),
        "那部片子",
    )
    .await;
    g0::leave();
    assert!(went.is_ok());
    // The ledger's clock is not the world's.
    let moved = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_speech_said SET at = $1, \
             payload = jsonb_set(payload::jsonb, '{at}', to_jsonb($2::bigint)) \
             WHERE thing LIKE 'own|late|%'",
            [at.into(), at.timestamp_micros().into()],
        ))
        .await
        .unwrap();
    assert_eq!(moved.rows_affected(), 1);
    late::restart(who);
}

/// What to say of hers in an hour and in two, they asked to be left alone for
/// three; said in one message at four, not settled.
async fn said_of_two(db: &DatabaseConnection, who: i32) {
    pen::reset_for_test();
    late::restart(who);
    judged(
        db,
        who,
        "三小时内别找我",
        json!([mine("问爬山怎么样", 1), mine("说那部片子", 2)]),
        end(json!({"how": "hours", "count": 3})),
    )
    .await;
    said_unsettled(db, who, crate::clock::now() + Duration::hours(4)).await;
    assert_eq!(waiting(db, who).await, 2);
}

async fn status(db: &DatabaseConnection, who: i32, text: &str) -> IntentStatus {
    held(db, who)
        .await
        .iter()
        .find(|one| one.text.contains(text))
        .unwrap()
        .status
}

/// The same ones left after a restart: settled, not said again.
#[tokio::test]
async fn what_was_said_is_not_said_again_after_a_restart() {
    let who = 708;
    let Some(schema) = schema("memory_nodes_left_alone_same", who).await else {
        return;
    };
    let db = &schema.db;
    said_of_two(db, who).await;
    let then = crate::clock::now() + Duration::hours(5);
    let went = tick(
        db,
        then,
        true,
        &[Some(json!({"reach_out": true, "about": "x"}))],
        true,
    )
    .await;
    assert_eq!((went.asked, went.wrote), (0, 0));
    assert_eq!(with_status(db, who, IntentStatus::Said).await, 2);
    schema.drop().await;
}

/// One more came due after the message and before they were settled: the two
/// are settled, it is asked about on its own, and nothing is said twice.
#[tokio::test]
async fn what_came_due_after_the_message_is_asked_about_on_its_own() {
    let who = 709;
    let Some(schema) = schema("memory_nodes_left_alone_joined", who).await else {
        return;
    };
    let db = &schema.db;
    said_of_two(db, who).await;
    judged(
        db,
        who,
        "十小时内别找我",
        json!([mine("问搬家怎么样", 9)]),
        end(json!({"how": "hours", "count": 10})),
    )
    .await;
    let yes = Some(json!({"reach_out": true, "about": "搬家"}));
    let then = crate::clock::now() + Duration::hours(11);
    let went = tick(db, then, true, &[yes], true).await;
    assert_eq!((went.asked, went.wrote), (1, 1));
    for text in ["爬山", "那部片子", "搬家"] {
        assert_eq!(status(db, who, text).await, IntentStatus::Said, "{text}");
    }
    // Under its own thing, not the first one's.
    let things = talk_tests::count(
        db,
        "SELECT COUNT(DISTINCT thing) AS n FROM agent_speech_said \
         WHERE thing LIKE 'own|late|%'",
    )
    .await;
    assert_eq!(things, 2);
    schema.drop().await;
}

/// One of them is gone when they are looked at again: the other is not said a
/// second time.
#[tokio::test]
async fn the_rest_is_not_said_again_when_one_is_gone() {
    let who = 710;
    let Some(schema) = schema("memory_nodes_left_alone_first", who).await else {
        return;
    };
    let db = &schema.db;
    said_of_two(db, who).await;
    let first = held(db, who)
        .await
        .into_iter()
        .find(|one| one.text.contains("爬山"))
        .unwrap();
    let audience = audience::private_talk(who, None);
    assert!(intents_due::settle(db, who, &audience, &first, IntentStatus::Dropped).await);
    let yes = Some(json!({"reach_out": true, "about": "片子"}));
    let then = crate::clock::now() + Duration::hours(5);
    let went = tick(db, then, true, &[yes], true).await;
    assert_eq!((went.asked, went.wrote), (0, 0));
    assert_eq!(status(db, who, "那部片子").await, IntentStatus::Said);
    schema.drop().await;
}

/// One due before the message, and come to be after it (from another end of
/// hers): it was not in it, and is asked about.
#[tokio::test]
async fn what_came_to_be_after_the_message_was_not_in_it() {
    let who = 711;
    let Some(schema) = schema("memory_nodes_left_alone_since", who).await else {
        return;
    };
    let db = &schema.db;
    said_of_two(db, who).await;
    judged(db, who, "嗯", json!([mine("问搬家怎么样", 1)]), json!([])).await;
    assert_eq!(waiting(db, who).await, 3);
    let yes = Some(json!({"reach_out": true, "about": "搬家"}));
    let then = crate::clock::now() + Duration::hours(5);
    let went = tick(db, then, true, &[yes], true).await;
    assert_eq!((went.asked, went.wrote), (1, 1));
    assert_eq!(with_status(db, who, IntentStatus::Said).await, 3);
    schema.drop().await;
}

/// What waited for no time of its own is told from its message too, when a
/// restart has forgotten that it waited.
#[tokio::test]
async fn what_was_said_is_not_said_again_though_it_is_not_known_to_have_waited() {
    let who = 712;
    let Some(schema) = schema("memory_nodes_left_alone_unknown", who).await else {
        return;
    };
    let db = &schema.db;
    said_of_two(db, who).await;
    // As one with no time: that it came due meanwhile is told from nothing.
    db.execute_unprepared("DELETE FROM agent_memories WHERE source = 'left_alone'")
        .await
        .unwrap();
    let then = crate::clock::now() + Duration::hours(5);
    let went = tick(db, then, true, &[], true).await;
    assert_eq!((went.asked, went.wrote), (0, 0));
    assert_eq!(with_status(db, who, IntentStatus::Said).await, 2);
    schema.drop().await;
}

/// Only a message that was sent and that no way out took is one of the few
/// that let go of them: one that was not written is none.
#[tokio::test]
async fn one_that_was_not_written_is_tried_again_without_end() {
    let who = 721;
    let Some(schema) = schema("memory_nodes_left_alone_tries", who).await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    alone_with_two(db, who).await;
    let over = crate::clock::now() + Duration::hours(4);
    let would = Some(json!({"reach_out": true, "about": "那部片子"}));
    for _ in 0..8 {
        let unwritten = tick_with(db, over, true, &[would.clone()], true, 0).await;
        assert_eq!((unwritten.asked, unwritten.wrote), (1, 0));
        late::again(who);
    }
    assert_eq!(waiting(db, who).await, 2);
    for sent in 1..=6 {
        assert_eq!(waiting(db, who).await, 2, "{sent}");
        let unsent = tick(db, over, true, &[would.clone()], false).await;
        assert_eq!((unsent.asked, unsent.wrote), (1, 1));
        late::again(who);
    }
    assert_eq!(with_status(db, who, IntentStatus::Dropped).await, 2);
    schema.drop().await;
}

/// They just talked: what waited is not brought up in the middle of it.
#[tokio::test]
async fn what_waited_is_not_brought_up_in_the_middle_of_a_talk() {
    let who = 722;
    let Some(schema) = schema("memory_nodes_left_alone_talk", who).await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    alone_with_two(db, who).await;
    db.execute_unprepared("UPDATE agent_messages SET created_at = now() + interval '3 hours'")
        .await
        .unwrap();
    let would = Some(json!({"reach_out": true, "about": "那部片子"}));
    let over = crate::clock::now() + Duration::hours(4);
    let talking = tick(db, over, true, &[would.clone()], true).await;
    assert_eq!((talking.asked, waiting(db, who).await), (0, 2));
    let after = tick(db, over + Duration::hours(2), true, &[would], true).await;
    assert_eq!((after.asked, after.wrote), (1, 1));
    schema.drop().await;
}

/// It holds from when they said it, not from when it was judged.
#[tokio::test]
async fn it_holds_from_when_they_said_it() {
    let who = 723;
    let Some(schema) = schema("memory_nodes_left_alone_from", who).await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    said(db, who, "两小时别找我").await;
    db.execute_unprepared("UPDATE agent_messages SET created_at = now() - interval '1 hour'")
        .await
        .unwrap();
    let mut world = g0::World::new(0.5);
    world.base = crate::clock::now();
    let hours = end(json!({"how": "hours", "count": 2}));
    world
        .models
        .entry(intending::OPERATION.to_string())
        .or_default()
        .push_back(Some(
            json!({"take": [], "drop": [], "marks": hours}).to_string(),
        ));
    g0::enter(world);
    intending::after_turn(db, who, None, SESSION, "两小时别找我", now())
        .await
        .unwrap();
    g0::leave();
    assert_eq!(left(db, who).await, Some(1));
    schema.drop().await;
}
