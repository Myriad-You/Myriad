//! What is on her mind about someone and what she wants, as her intents
//! kept in step with their rows, on a database of their own, as
//! `intent_tests`. Every word here is made up.

use chrono::{DateTime, Duration, Utc};
use myriad_nodes::body::{IntentStatus, Origin};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, Statement};
use serde_json::json;

use super::home_intents::{self, Stepped, THREAD, WANT};
use super::intent_store::{self, Held};
use super::switch::{Mode, set_for_test};
use super::talk_tests::{OTHER, schema, talk};
use super::{audience, intents_due, write};
use crate::models::entities::agent_memories;
use crate::services::agent::memory::{manage, unified};
use crate::services::agent::merope::threads::{self, Kept};
use crate::services::channel_group::g0;
use crate::services::her_log::pen;

const SESSION: &str = "s_home";

fn now() -> DateTime<Utc> {
    crate::services::agent::merope::clock::now()
}

/// A line of theirs in account 7's private talk; its id.
async fn said(db: &DatabaseConnection, text: &str) -> i32 {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) VALUES ($1, 7, 't', $2, 0, false, NOW(), NOW()) \
         ON CONFLICT (id) DO NOTHING",
        [SESSION.into(), json!({"mode": "chat"}).into()],
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

fn kept(about: &str, then: &str, hours: Option<i64>) -> Kept {
    Kept {
        about: about.into(),
        then: then.into(),
        due_in_hours: hours,
    }
}

/// A thread kept as her inner pass keeps it, grown from their line `text`.
async fn grown(db: &DatabaseConnection, text: &str, thread: Kept) -> String {
    said(db, text).await;
    let from = home_intents::origin_of(db, OTHER, text, now()).await;
    assert!(from.is_some(), "their line is found");
    threads::keep_from(db, OTHER, &thread, now(), from.as_deref()).await;
    threads::open(db, OTHER)
        .await
        .into_iter()
        .find(|open| open.about == thread.about)
        .unwrap()
        .id
}

async fn intents(db: &DatabaseConnection) -> Vec<Held> {
    intent_store::held(db, OTHER, &talk())
        .await
        .unwrap()
        .into_iter()
        .filter(|held| held.id.starts_with(THREAD))
        .collect()
}

async fn reason(db: &DatabaseConnection, id: &str) -> Option<String> {
    agent_memories::Entity::find_by_id(id.to_string())
        .one(db)
        .await
        .unwrap()
        .and_then(|row| row.invalid_reason)
}

#[tokio::test]
async fn a_thread_grown_from_their_words_is_her_intent_on_about_you() {
    let Some(schema) = schema("memory_nodes_home_threads").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    set_for_test(Some(Mode::Shadow));
    let at = now();
    let exam = grown(
        db,
        "明天考试，有点慌",
        kept("考试", "问问考得怎么样", Some(20)),
    )
    .await;
    let held = intents(db).await;
    assert_eq!(held.len(), 1);
    let intent = &held[0];
    assert_eq!(intent.id, home_intents::intent_id(THREAD, &exam));
    assert_eq!((intent.origin, intent.waiting()), (Some(Origin::Her), true));
    assert_eq!(intent.text, "考试 — 问问考得怎么样");
    assert_eq!(
        intent.node,
        myriad_nodes::node_id(&talk(), &audience::person_key(&talk(), OTHER)).unwrap()
    );
    assert!(intent.because.iter().any(|id| id.starts_with("message:")));
    assert!(intent.because.contains(&format!("memory:{exam}")));
    let wakes = intent.trigger.as_ref().unwrap()["at"].as_i64().unwrap();
    let due = (at + Duration::hours(20)).timestamp_micros();
    assert!((wakes - due).abs() < 60_000_000, "at its due time");

    // Kept with nothing of theirs under it (her night's): only a thread.
    threads::wish(db, OTHER, "搬家", "问问新家怎么样").await;
    assert_eq!(intents(db).await.len(), 1);
    assert_eq!(threads::open(db, OTHER).await.len(), 2);

    // The row retiring settles the intent: she wrote first about it.
    threads::close(db, OTHER, std::slice::from_ref(&exam), "reached_out").await;
    let settled = intents(db).await;
    assert_eq!(settled[0].status, IntentStatus::Said);

    // The intent settled (called off) retires its row.
    let trip = grown(db, "下周去旅行", kept("旅行", "问问去哪儿玩了", None)).await;
    let waiting = intents(db)
        .await
        .into_iter()
        .find(|held| held.waiting())
        .unwrap();
    assert!(waiting.trigger.is_none(), "no time: her next chance");
    let thought = pen::think().unwrap();
    let off = intent_store::settle_row(&waiting, IntentStatus::Dropped);
    write::write(db, OTHER, &talk(), &[off], Some(&thought))
        .await
        .unwrap();
    home_intents::in_step(db, OTHER).await;
    assert_eq!(reason(db, &trip).await.as_deref(), Some("intent_dropped"));
    assert!(
        threads::open(db, OTHER)
            .await
            .iter()
            .all(|open| open.id != trip)
    );

    // In step: a pass again changes nothing.
    assert_eq!(
        home_intents::threads_in_step(db, OTHER, None, now())
            .await
            .unwrap(),
        Stepped {
            refused: 1,
            ..Stepped::default()
        }
    );
    set_for_test(None);
    schema.drop().await;
}

#[tokio::test]
async fn past_eight_the_oldest_thread_intent_goes_with_its_row() {
    let Some(schema) = schema("memory_nodes_home_cap").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    // Kept as rows first (the old cap keeps the same number), then taken.
    set_for_test(Some(Mode::Off));
    let mut rows = Vec::new();
    for n in 0..9 {
        let line = said(db, &format!("事{n}")).await;
        let id = unified::remember(
            db,
            unified::NewMemory {
                user_id: OTHER,
                kind: unified::MemoryKind::Fact,
                content: format!("问问事{n}"),
                evidence: Some(
                    json!({"about": format!("事{n}"), "due": null, "hers": false, "from": format!("message:{line}")})
                        .to_string(),
                ),
                speaker: unified::Speaker::Agent,
                source: threads::SOURCE,
                audience: unified::Audience::private(OTHER),
                importance: 0.5,
                concepts: Vec::new(),
            },
        )
        .await
        .unwrap()
        .unwrap();
        rows.push(id);
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    set_for_test(Some(Mode::Shadow));
    let stepped = home_intents::threads_in_step(db, OTHER, None, now())
        .await
        .unwrap();
    assert_eq!((stepped.intended, stepped.settled), (9, 1));
    let waiting: Vec<Held> = intents(db)
        .await
        .into_iter()
        .filter(Held::waiting)
        .collect();
    assert_eq!(waiting.len(), home_intents::THREADS_WAITING);
    let gone: Vec<&String> = rows
        .iter()
        .filter(|row| {
            !waiting
                .iter()
                .any(|held| held.id == home_intents::intent_id(THREAD, row))
        })
        .collect();
    assert_eq!(gone, vec![&rows[0]], "the oldest");
    assert_eq!(reason(db, gone[0]).await.as_deref(), Some("intent_dropped"));
    set_for_test(None);
    schema.drop().await;
}

#[tokio::test]
async fn read_from_her_nodes_a_thread_comes_due_as_her_intent_not_the_old_way() {
    let Some(schema) = schema("memory_nodes_home_due").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    set_for_test(Some(Mode::Shadow));
    let exam = grown(db, "明天考试", kept("考试", "问问考得怎么样", Some(1))).await;
    // In shadow the old way still sees it.
    let open = threads::open(db, OTHER).await;
    assert_eq!(home_intents::not_intents(db, OTHER, open).await.len(), 1);
    set_for_test(Some(Mode::On));
    let open = threads::open(db, OTHER).await;
    assert!(home_intents::not_intents(db, OTHER, open).await.is_empty());
    // Come due: said, and its row with it.
    let later = now() + Duration::hours(2);
    let mut world = g0::World::new(0.5);
    world
        .says
        .entry("intent_raise".to_string())
        .or_default()
        .push_back((0.0, Ok("考得怎么样？".to_string())));
    g0::enter(world);
    assert!(intents_due::tick_at(db, later).await.is_empty());
    let log = g0::leave().expect("a world").log;
    assert!(
        log.iter()
            .any(|line| line["kind"] == "say" && line["what"]["operation"] == "intent_raise")
    );
    assert_eq!(intents(db).await[0].status, IntentStatus::Said);
    assert_eq!(reason(db, &exam).await.as_deref(), Some("intent_said"));
    assert!(threads::open(db, OTHER).await.is_empty());
    set_for_test(None);
    schema.drop().await;
}

#[tokio::test]
async fn what_she_wants_is_her_intent_on_herself() {
    let Some(schema) = schema("memory_nodes_home_wants").await else {
        return;
    };
    let db = &schema.db;
    db.execute_unprepared("UPDATE users SET is_owner = true WHERE id = 1")
        .await
        .unwrap();
    pen::reset_for_test();
    set_for_test(Some(Mode::Shadow));
    let guitar = unified::remember_own(db, "学会弹吉他", "{}", Vec::new(), "want")
        .await
        .unwrap()
        .unwrap();
    let stepped = home_intents::wants_in_step(db, Some(1)).await.unwrap();
    assert_eq!(stepped.intended, 1);
    let hers = |db: &DatabaseConnection| {
        let db = db.clone();
        async move {
            intent_store::held(&db, 1, audience::OWN)
                .await
                .unwrap()
                .into_iter()
                .filter(|held| held.id.starts_with(WANT))
                .collect::<Vec<Held>>()
        }
    };
    let held = hers(db).await;
    assert_eq!(held.len(), 1);
    assert_eq!(
        (
            held[0].text.as_str(),
            held[0].origin,
            held[0].trigger.is_none()
        ),
        ("学会弹吉他", Some(Origin::Her), true)
    );
    assert_eq!(held[0].because, vec![format!("memory:{guitar}")]);
    assert_eq!(
        held[0].node,
        myriad_nodes::node_id(audience::OWN, "her").unwrap()
    );
    // It came true: done.
    unified::retire_own(db, &guitar, "came_true").await.unwrap();
    home_intents::wants_in_step(db, Some(1)).await.unwrap();
    assert_eq!(hers(db).await[0].status, IntentStatus::Done);
    // Nothing of hers without a site owner to keep it with.
    assert_eq!(
        home_intents::wants_in_step(db, None).await.unwrap(),
        Stepped::default()
    );
    set_for_test(None);
    schema.drop().await;
}

#[tokio::test]
async fn forgetting_a_thread_takes_her_intent() {
    let Some(schema) = schema("memory_nodes_home_forget").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    set_for_test(Some(Mode::Shadow));
    let exam = grown(db, "明天考试", kept("考试", "问问考得怎么样", Some(20))).await;
    assert_eq!(intents(db).await.len(), 1);
    assert!(manage::forget_any(db, &exam).await.unwrap());
    assert!(intents(db).await.is_empty());
    let words = "SELECT COUNT(*) AS n FROM agent_node_rows WHERE body LIKE '%考得怎么样%'";
    assert_eq!(super::talk_tests::count(db, words).await, 0);
    set_for_test(None);
    schema.drop().await;
}

#[tokio::test]
async fn read_from_her_nodes_a_thread_that_is_her_intent_does_not_fade() {
    let Some(schema) = schema("memory_nodes_home_fade").await else {
        return;
    };
    let db = &schema.db;
    pen::reset_for_test();
    set_for_test(Some(Mode::On));
    let long_ago = now() - Duration::days(10);
    // Due nine days ago: past its time by more than a few days.
    said(db, "上周的考试").await;
    let from = home_intents::origin_of(db, OTHER, "上周的考试", now()).await;
    threads::keep_from(
        db,
        OTHER,
        &kept("考试", "问问考得怎么样", Some(24)),
        long_ago,
        from.as_deref(),
    )
    .await;
    // One with nothing of theirs under it: only a thread.
    threads::keep(
        db,
        OTHER,
        &kept("搬家", "问问新家怎么样", Some(24)),
        long_ago,
    )
    .await;
    let open = threads::open(db, OTHER).await;
    let exam = open
        .iter()
        .find(|thread| thread.about == "考试")
        .unwrap()
        .id
        .clone();
    let move_ = open
        .iter()
        .find(|thread| thread.about == "搬家")
        .unwrap()
        .id
        .clone();
    assert_eq!(intents(db).await.len(), 1);
    threads::let_fade(db).await;
    assert_eq!(
        reason(db, &move_).await.as_deref(),
        Some("faded"),
        "a thread fades"
    );
    assert_eq!(reason(db, &exam).await, None, "her intent waits");
    assert!(intents(db).await[0].waiting());
    // In shadow it fades as today, and its intent with it.
    set_for_test(Some(Mode::Shadow));
    threads::let_fade(db).await;
    assert_eq!(reason(db, &exam).await.as_deref(), Some("faded"));
    assert_eq!(intents(db).await[0].status, IntentStatus::Dropped);
    set_for_test(None);
    schema.drop().await;
}
