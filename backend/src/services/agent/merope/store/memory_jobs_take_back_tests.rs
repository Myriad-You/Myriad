//! What they take back, end to end through a memory job: let go at once,
//! held for the second look with their line's message, and forgotten on
//! her nodes only when the second look agrees (`take_back_check`).

use super::*;
use crate::services::agent::memory::unified;
use crate::services::agent::merope::chat_lines::tests::{line, session};
use crate::services::agent::merope::memory_nodes::{self, Author, RowIn};
use crate::services::channel_group::g0;

/// A fact she knows for good, on a node of hers in their talk.
async fn known_on_node(db: &DatabaseConnection, known: &str) {
    let Effect::Chat(lasting) = fact_of(known) else {
        unreachable!()
    };
    assert!(
        crate::services::agent::merope::store::apply_chat_memory_updates_on(
            db,
            7,
            crate::clock::now().fixed_offset(),
            &lasting,
            &Audience::private(7),
            crate::services::agent::merope::store::Keep::Lasting,
        )
        .await
        .unwrap()
    );
    let memory = memory_of(db, known).await;
    let audience = memory_nodes::audience::private_talk(7, None);
    let made = myriad_nodes::entries::fact_rows(
        serde_json::from_value(serde_json::json!({
            "audience": audience,
            "start": {"name": known},
            "facts": [{"memory": memory, "text": known, "saidBy": ["partner"]}],
        }))
        .unwrap(),
    )
    .unwrap();
    let rows: Vec<RowIn> = made
        .rows
        .into_iter()
        .map(|row| RowIn {
            id: row.id,
            node: made.node.clone(),
            kind: row.kind,
            body: row.body,
            author: Author::Her,
        })
        .collect();
    let thought = crate::services::her_log::pen::think().unwrap();
    memory_nodes::write(db, 7, &audience, &rows, Some(&thought))
        .await
        .unwrap();
}

async fn memory_of(db: &DatabaseConnection, content: &str) -> String {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT id FROM agent_memories WHERE content = $1 AND source = 'chat'",
        [content.into()],
    ))
    .await
    .unwrap()
    .expect("kept")
    .try_get("", "id")
    .unwrap()
}

/// The entry of `known` on her node: still there.
async fn on_node(db: &DatabaseConnection, known: &str) -> bool {
    let memory = memory_of(db, known).await;
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT count(*) AS n FROM agent_node_rows WHERE user_id = 7 AND kind = 'node.entry' \
         AND body <> '' AND body::jsonb->'refs' ? ('memory:' || $1)",
        [memory.into()],
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get::<i64>("", "n")
    .unwrap()
        > 0
}

/// Where the second look at `known` stands, and the message it reads.
async fn check(db: &DatabaseConnection, known: &str) -> (String, Option<i32>) {
    let memory = memory_of(db, known).await;
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT state, said_ref FROM agent_takeback_check WHERE user_id = 7 AND memory_id = $1",
            [memory.into()],
        ))
        .await
        .unwrap()
        .expect("held for the second look");
    (
        row.try_get("", "state").unwrap(),
        row.try_get("", "said_ref").unwrap(),
    )
}

/// A world that answers the second look with `says` (none: no answer is
/// given for it), and going over a stretch with `settle`.
fn world(says: &[&str], settle: Option<&str>) {
    let mut world = g0::World::new(0.5);
    for says in says {
        world
            .models
            .entry("take_back_check".into())
            .or_default()
            .push_back(Some(format!(r#"{{"says":"{says}"}}"#)));
    }
    if let Some(settle) = settle {
        world
            .models
            .entry("chat_settle".into())
            .or_default()
            .push_back(Some(settle.into()));
    }
    g0::enter(world);
}

/// How many second looks were asked.
fn looked() -> usize {
    let world = g0::leave().expect("a world");
    world
        .log
        .iter()
        .filter(|line| line["kind"] == "model" && line["what"]["operation"] == "take_back_check")
        .count()
}

/// Their line `text`, held to be gone over alone, claimed.
async fn their_line(db: &DatabaseConnection, id: &str, text: &str) -> Claim {
    let mut state = crate::services::agent::merope::store::get_or_create_state(db, 7)
        .await
        .unwrap()
        .into_active_model();
    state.last_user_message_at = Set(Some(crate::clock::now().fixed_offset()));
    let input = state
        .update(db)
        .await
        .unwrap()
        .last_user_message_at
        .unwrap();
    enqueue(
        db,
        id,
        7,
        Payload::Chat {
            user_text: text.into(),
            reply: String::new(),
            input_at: input,
            present: Audience::private(7),
            turn: TurnContext::default(),
            lines: vec![input],
        },
        None,
    )
    .await
    .unwrap();
    due(db, id).await;
    claim(db).await.unwrap().expect("their line")
}

/// What a line that only lets go of `known` leaves.
fn lets_go(known: &str) -> Effect {
    Effect::Chat(
        crate::services::agent::merope::chat_remember::ChatMemoryUpdates {
            updates: vec![ChatMemoryUpdate {
                standing: Default::default(),
                topic: None,
                cues: Vec::new(),
                fact: None,
                supersedes: vec![known.into()],
                evidence: None,
                concepts: vec![],
            }],
            ..Default::default()
        },
    )
}

/// A line that only lets go: the memory goes at once, the second look reads
/// that very line, and only an answer that they took it back forgets what
/// rests on it on her nodes. While she is paused, nobody is asked.
#[tokio::test]
async fn a_line_that_only_lets_go_is_looked_at_again_with_that_line() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let (quit, cat) = ("受够了组长，想辞职", "养了一只猫叫年糕");
    known_on_node(db, quit).await;
    known_on_node(db, cat).await;
    session(db, "talk", 7, Some(r#"{"mode":"chat"}"#), false).await;
    // A change, as the second look sees it: let go, nothing forgotten.
    let changed = line(db, "talk", "user", "辞职那个你就当没听过吧", 0, None).await;
    let job = their_line(db, "quit", "辞职那个你就当没听过吧").await;
    world(&["changed"], None);
    finish(db, &job, Ok(lets_go(quit))).await.unwrap();
    assert_eq!(looked(), 1);
    assert_eq!(
        check(db, quit).await,
        ("changed".to_string(), Some(changed))
    );
    assert!(on_node(db, quit).await, "nothing forgotten for a change");
    // Taken back, as the second look agrees: forgotten there, and only it.
    let taken = line(db, "talk", "user", "猫那个当我没说", 0, None).await;
    let job = their_line(db, "cat", "猫那个当我没说").await;
    world(&["takes_back"], None);
    finish(db, &job, Ok(lets_go(cat))).await.unwrap();
    assert_eq!(looked(), 1);
    assert_eq!(check(db, cat).await, ("confirmed".to_string(), Some(taken)));
    assert!(!on_node(db, cat).await, "forgotten once confirmed");
    assert!(on_node(db, quit).await);
    // Paused: nothing is asked (one left waiting is asked later).
    db.execute_unprepared("UPDATE agent_takeback_check SET state = 'pending', asked_at = NULL")
        .await
        .unwrap();
    let job = their_line(db, "paused", "在吗").await;
    world(&[], None);
    finish(db, &job, Ok(Effect::Paused)).await.unwrap();
    assert_eq!(looked(), 0);
    schema.drop().await;
}

/// Going over a stretch: what they took back is held with the message its
/// `said` points to, and forgotten on her nodes once the second look agrees.
#[tokio::test]
async fn a_stretch_holds_what_they_took_back_with_the_line_it_names() {
    let Some(schema) = fixture().await else {
        return;
    };
    let db = &schema.db;
    let quit = "想辞职";
    known_on_node(db, quit).await;
    session(db, "talk", 7, Some(r#"{"mode":"chat"}"#), false).await;
    line(
        db,
        "talk",
        "assistant",
        "你上次说想辞职，后来想好了吗？",
        5,
        None,
    )
    .await;
    let said = line(db, "talk", "user", "辞职那个当没听过吧", 4, None).await;
    unified::remember(
        db,
        unified::NewMemory {
            user_id: 7,
            kind: unified::MemoryKind::Fact,
            content: "辞职那个当没听过".into(),
            evidence: Some("辞职那个当没听过吧".into()),
            speaker: unified::Speaker::User,
            source: unified::SHORT_NOTE_SOURCE,
            audience: Audience::private(7),
            importance: 0.6,
            concepts: Vec::new(),
        },
    )
    .await
    .unwrap()
    .expect("a note");
    let began = crate::clock::now().fixed_offset() - chrono::Duration::minutes(10);
    enqueue(
        db,
        "settle",
        7,
        Payload::Settle {
            present: Audience::private(7),
            from: began,
            last: began,
        },
        None,
    )
    .await
    .unwrap();
    due(db, "settle").await;
    let job = claim(db).await.unwrap().expect("a stretch over");
    world(
        &["takes_back"],
        Some(r#"{"lasting":[],"tookBack":[{"entry":"想辞职","said":1}],"moment":[],"known":[]}"#),
    );
    let effect = crate::services::agent::merope::chat_remember::settle(
        db,
        7,
        &Audience::private(7),
        began,
        None,
    )
    .await
    .unwrap_or_else(|_| panic!("gone over"));
    finish(db, &job, Ok(effect)).await.unwrap();
    assert_eq!(looked(), 1);
    assert_eq!(check(db, quit).await, ("confirmed".to_string(), Some(said)));
    assert!(!on_node(db, quit).await, "forgotten once confirmed");
    schema.drop().await;
}
