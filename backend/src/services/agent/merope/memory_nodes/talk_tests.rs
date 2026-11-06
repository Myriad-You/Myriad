//! Her nodes written from private talk (placing a stretch, what going over
//! it keeps, sorting, her upkeep, forgetting), on a database of their own
//! (`MYRIAD_MEDIA_TEST_DATABASE_URL`, an isolated schema each), with the
//! model's answers given by the test (`channel_group::g0`).

use std::collections::VecDeque;

use chrono::Duration;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::{Value, json};

use super::facts::GoesUnder;
use super::placing::{self, Line};
use super::{Author, RowIn, audience, facts, forget, inbox, read, sorting, upkeep, write};
use crate::services::agent::memory::unified::Audience;
use crate::services::agent::merope::chat_remember::{ChatMemoryUpdate, ChatMemoryUpdates};
use crate::services::agent::merope::store::{self, Keep};
use crate::services::channel_group::g0;
use crate::services::her_log::pen;

pub(super) const OTHER: i32 = 7;

pub(super) async fn schema(prefix: &str) -> Option<crate::db::IsolatedSchema> {
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").ok()?;
    let schema = crate::db::IsolatedSchema::migrated(&url, prefix).await;
    schema
        .db
        .execute_unprepared("INSERT INTO users (id, username) VALUES (1, 'one'), (7, 'seven')")
        .await
        .unwrap();
    Some(schema)
}

pub(super) async fn count(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "n")
        .unwrap()
}

/// A world whose model gives `answers`, by operation, in order.
pub(super) fn answering(answers: &[(&str, &str)]) {
    let mut world = g0::World::new(0.5);
    for (operation, answer) in answers {
        world
            .models
            .entry(operation.to_string())
            .or_insert_with(VecDeque::new)
            .push_back(Some(answer.to_string()));
    }
    g0::enter(world);
}

/// The model calls the world was asked, by operation.
pub(super) fn asked(operation: &str) -> usize {
    let world = g0::leave().expect("a world");
    world
        .log
        .iter()
        .filter(|line| line["kind"] == "model" && line["what"]["operation"] == operation)
        .count()
}

pub(super) fn talk() -> String {
    audience::private_talk(OTHER, None)
}

/// A topic she keeps in account 7's talk, with facts on it.
pub(super) async fn topic(db: &DatabaseConnection, name: &str, facts: &[(&str, &str)]) -> String {
    let made = myriad_nodes::entries::fact_rows(
        serde_json::from_value(json!({
            "audience": talk(),
            "start": {"name": name},
            "facts": facts.iter().map(|(memory, text)| json!({"memory": memory, "text": text, "saidBy": ["partner"]})).collect::<Vec<_>>(),
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
    let thought = pen::think().unwrap();
    write::write(db, OTHER, &talk(), &rows, Some(&thought))
        .await
        .unwrap();
    made.node
}

pub(super) fn line(id: i32, theirs: bool, text: &str, minutes: i64) -> Line {
    Line {
        id,
        theirs,
        text: text.into(),
        at: (crate::clock::now() - Duration::minutes(60 - minutes)).fixed_offset(),
    }
}

pub(super) fn update(
    text: &str,
    supersedes: &[&str],
    topic: Option<GoesUnder>,
) -> ChatMemoryUpdate {
    ChatMemoryUpdate {
        standing: Default::default(),
        fact: Some(text.into()),
        supersedes: supersedes.iter().map(|old| old.to_string()).collect(),
        evidence: Some(text.into()),
        concepts: Vec::new(),
        cues: Vec::new(),
        topic,
    }
}

/// What going over a stretch kept, written as it is today, then put on her
/// nodes as it is after.
pub(super) async fn settle(db: &DatabaseConnection, updates: Vec<ChatMemoryUpdate>) {
    let present = Audience::private(OTHER);
    let transaction = db.begin().await.unwrap();
    for update in &updates {
        store::apply_chat_memory_update_on(
            &transaction,
            OTHER,
            crate::clock::now().fixed_offset(),
            update,
            &present,
            Keep::Lasting,
        )
        .await
        .unwrap();
    }
    transaction.commit().await.unwrap();
    let updates = ChatMemoryUpdates {
        updates,
        ..ChatMemoryUpdates::default()
    };
    facts::settled(db, OTHER, None, &present, &updates).await;
}

pub(super) async fn memory_id(db: &DatabaseConnection, content: &str) -> String {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT id FROM agent_memories WHERE content = $1",
        [content.into()],
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get("", "id")
    .unwrap()
}

/// Where a memory was put: how, and the name of its node.
pub(super) async fn placed(
    db: &DatabaseConnection,
    content: &str,
) -> Option<(String, Option<String>)> {
    let id = memory_id(db, content).await;
    let placement = super::placements::placement(db, &id).await.unwrap()?;
    let name = match &placement.node_id {
        Some(node) => read::heads(db, OTHER, &talk(), None)
            .await
            .unwrap()
            .into_iter()
            .find(|head| &head.node_id == node)
            .and_then(|head| head.name),
        None => None,
    };
    Some((placement.how.as_str().to_string(), name))
}

/// The old topic tables, as names: each topic, and each memory's topic.
pub(super) async fn old_tables(
    db: &DatabaseConnection,
) -> (Vec<String>, Vec<(String, Option<String>)>) {
    let topics = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT name FROM agent_topic_nodes ORDER BY name",
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get("", "name").unwrap())
        .collect();
    let under = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT m.content, t.name FROM agent_memory_topics p \
             JOIN agent_memories m ON m.id = p.memory_id \
             LEFT JOIN agent_topic_nodes t ON t.user_id = p.user_id AND t.id = p.topic_id \
             ORDER BY m.content",
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| {
            (
                row.try_get("", "content").unwrap(),
                row.try_get("", "name").unwrap(),
            )
        })
        .collect();
    (topics, under)
}

pub(super) fn kept_today() -> Vec<ChatMemoryUpdate> {
    vec![
        update(
            "has a snake plant",
            &[],
            Some(GoesUnder::New {
                name: "their plants".into(),
                aliases: Vec::new(),
            }),
        ),
        update(
            "bought a fern",
            &[],
            Some(GoesUnder::New {
                name: "Their  Plants".into(),
                aliases: vec!["greenery".into()],
            }),
        ),
        update("lives in Austin", &[], Some(GoesUnder::AboutThem)),
        update("had a long day", &[], Some(GoesUnder::Nowhere)),
    ]
}

#[tokio::test]
async fn a_stretch_is_cut_into_spans_on_her_nodes_and_the_inbox() {
    let Some(schema) = schema("memory_nodes_placing").await else {
        return;
    };
    let db = &schema.db;
    let plants = topic(db, "houseplants", &[("m0", "has a fern")]).await;
    let stretch = vec![
        line(101, true, "my fern got new leaves", 0),
        line(102, false, "that's lovely", 1),
        line(103, true, "also I'm off to Kyoto next week", 2),
        line(104, false, "have a good trip!", 3),
    ];
    answering(&[(
        "memory_node_place",
        r#"{"spans":[{"start":1,"node":"n1","gist":"their fern grows"},{"start":3,"node":null,"gist":"a trip to Kyoto"}]}"#,
    )]);
    let used = placing::place(db, OTHER, None, &stretch).await;
    assert_eq!(asked("memory_node_place"), 1);
    assert_eq!(used, vec![plants.clone()]);
    let spans = read::node(db, OTHER, &talk(), &plants)
        .await
        .unwrap()
        .unwrap()["spans"]
        .clone();
    assert_eq!(spans.as_array().unwrap().len(), 1);
    assert_eq!(spans[0]["text"], "their fern grows");
    assert_eq!(spans[0]["refs"], json!(["message:101", "message:102"]));
    assert_eq!(spans[0]["saidBy"], json!(["her", "partner"]));
    let waiting = inbox::waiting(db, OTHER, &talk(), 10).await.unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].id, "message:103");
    assert_eq!(waiting[0].text, "a trip to Kyoto");
    assert_eq!(waiting[0].refs, ["message:103", "message:104"]);

    // Gone over again (a retry): nothing new, no call, the same nodes.
    answering(&[]);
    assert_eq!(
        placing::place(db, OTHER, None, &stretch).await,
        vec![plants.clone()]
    );
    assert_eq!(asked("memory_node_place"), 0);

    // An answer that does not read writes nothing.
    let rows = "SELECT COUNT(*) AS n FROM agent_node_rows";
    let before = count(db, rows).await;
    answering(&[("memory_node_place", "I would put it with the plants")]);
    let more = vec![line(105, true, "the cactus is fine too", 4)];
    assert!(placing::place(db, OTHER, None, &more).await.is_empty());
    assert_eq!(asked("memory_node_place"), 1);
    assert_eq!(count(db, rows).await, before);
    assert_eq!(
        inbox::waiting(db, OTHER, &talk(), 10).await.unwrap().len(),
        1
    );
    schema.drop().await;
}

#[tokio::test]
async fn what_going_over_keeps_goes_on_her_nodes() {
    let Some(schema) = schema("memory_nodes_settled").await else {
        return;
    };
    let db = &schema.db;
    settle(db, kept_today()).await;
    let plants = Some(("node".to_string(), Some("their plants".to_string())));
    assert_eq!(placed(db, "has a snake plant").await, plants);
    assert_eq!(
        placed(db, "bought a fern").await,
        plants,
        "one name, one node"
    );
    assert_eq!(
        placed(db, "lives in Austin").await,
        Some(("about".into(), Some("seven".into())))
    );
    assert_eq!(
        placed(db, "had a long day").await,
        Some(("nowhere".into(), None))
    );
    let heads = read::heads(db, OTHER, &talk(), None).await.unwrap();
    assert_eq!(heads.len(), 2, "their plants and 「关于你」");
    let topic = heads
        .iter()
        .find(|head| head.kind.as_deref() == Some("topic"))
        .unwrap();
    assert_eq!(topic.aliases, ["greenery"]);
    let entries = read::entries(db, OTHER, &talk(), &topic.node_id)
        .await
        .unwrap();
    let memories: Vec<String> = entries
        .iter()
        .flat_map(|entry| entry.memories.clone())
        .collect();
    assert_eq!(
        memories,
        [
            memory_id(db, "has a snake plant").await,
            memory_id(db, "bought a fern").await
        ]
    );
    assert!(entries.iter().all(|entry| entry.said_by == ["partner"]));
    let person = heads
        .iter()
        .find(|head| head.kind.as_deref() == Some("person"))
        .unwrap();
    assert_eq!(person.node_key.as_deref(), Some("person:7"));

    // What replaces a fact goes where it was, whatever was decided for it.
    settle(
        db,
        vec![update(
            "has two snake plants",
            &["has a snake plant"],
            Some(GoesUnder::Nowhere),
        )],
    )
    .await;
    assert_eq!(placed(db, "has two snake plants").await, plants);
    // The old tables are no longer written.
    let (topics, under) = old_tables(db).await;
    assert!(topics.is_empty() && under.is_empty());
    schema.drop().await;
}

#[tokio::test]
async fn what_she_knew_before_is_sorted_onto_her_nodes() {
    let Some(schema) = schema("memory_nodes_sorting").await else {
        return;
    };
    let db = &schema.db;
    topic(db, "their plants", &[("m0", "has a cactus")]).await;
    db.execute_unprepared(
        "INSERT INTO agent_memories (id, user_id, kind, content, source, venue, valid_from, \
         created_at, updated_at) VALUES \
         ('s1', 7, 'fact', 'bought a fern', 'chat', 'private', now(), now() - interval '3 day', now()), \
         ('s2', 7, 'fact', 'sees Dr. Lee', 'chat', 'private', now(), now() - interval '2 day', now()), \
         ('s3', 7, 'fact', 'cannot eat mango', 'import', 'private', now(), now() - interval '1 day', now()), \
         ('s4', 7, 'fact', 'a note', 'chat_note', 'private', now(), now(), now())",
    )
    .await
    .unwrap();
    answering(&[(
        "memory_node_sort",
        r#"{"facts":[{"fact":1,"topic":{"on":"t1","name":"","aliases":[]}},{"fact":2,"topic":{"on":"","name":"seeing doctors","aliases":[]}},{"fact":3,"topic":{"on":"","name":"","aliases":[]}}]}"#,
    )]);
    assert_eq!(sorting::sort_for(db, OTHER, None, 3).await, 3);
    assert_eq!(
        asked("memory_node_sort"),
        1,
        "the next batch has nothing left"
    );
    let names = |how: &str, name: &str| Some((how.to_string(), Some(name.to_string())));
    assert_eq!(
        placed(db, "bought a fern").await,
        names("node", "their plants")
    );
    assert_eq!(
        placed(db, "sees Dr. Lee").await,
        names("node", "seeing doctors")
    );
    assert_eq!(
        placed(db, "cannot eat mango").await,
        names("about", "seven")
    );
    assert_eq!(
        placed(db, "a note").await,
        None,
        "a short note is not sorted"
    );
    schema.drop().await;
}

/// A batch whose answer could not be read failed: the next batch is still
/// sorted, and the failed one is asked of again at a later upkeep.
#[tokio::test]
async fn a_batch_not_read_leaves_the_next_to_be_sorted() {
    let Some(schema) = schema("memory_nodes_sort_unread").await else {
        return;
    };
    let db = &schema.db;
    topic(db, "their plants", &[("m0", "has a cactus")]).await;
    let first = myriad_merope::topics::SORT_BATCH;
    let values: Vec<String> = (0..=first)
        .map(|n| {
            format!(
                "('u{n}', 7, 'fact', 'bought plant number {n}', 'chat', 'private', now(), \
                 now() - interval '{} minute', now())",
                first + 1 - n
            )
        })
        .collect();
    db.execute_unprepared(&format!(
        "INSERT INTO agent_memories (id, user_id, kind, content, source, venue, valid_from, \
         created_at, updated_at) VALUES {}",
        values.join(", ")
    ))
    .await
    .unwrap();
    answering(&[
        (
            "memory_node_sort",
            "Here is where they go, though not as JSON",
        ),
        (
            "memory_node_sort",
            r#"```json
{"facts":[{"fact":1,"topic":{"on":"t1","name":"","aliases":[]}}]}
```"#,
        ),
    ]);
    assert_eq!(sorting::sort_for(db, OTHER, None, 3).await, 1);
    assert_eq!(
        asked("memory_node_sort"),
        2,
        "the batch after the failed one"
    );
    assert_eq!(
        placed(db, &format!("bought plant number {first}")).await,
        Some(("node".to_string(), Some("their plants".to_string())))
    );
    assert_eq!(
        placed(db, "bought plant number 0").await,
        None,
        "the failed batch is still to sort"
    );
    schema.drop().await;
}

/// A fact a readable answer leaves out is put nowhere, decided: it is not
/// asked of again at the next upkeep.
#[tokio::test]
async fn a_fact_a_readable_answer_leaves_out_is_put_nowhere() {
    let Some(schema) = schema("memory_nodes_sort_left_out").await else {
        return;
    };
    let db = &schema.db;
    topic(db, "their plants", &[("m0", "has a cactus")]).await;
    db.execute_unprepared(
        "INSERT INTO agent_memories (id, user_id, kind, content, source, venue, valid_from, \
         created_at, updated_at) VALUES \
         ('a1', 7, 'fact', 'bought a fern', 'chat', 'private', now(), now() - interval '2 minute', now()), \
         ('a2', 7, 'fact', 'the weather was nice', 'chat', 'private', now(), now() - interval '1 minute', now())",
    )
    .await
    .unwrap();
    answering(&[(
        "memory_node_sort",
        r#"{"facts":[{"fact":1,"topic":{"on":"t1","name":"","aliases":[]}}]}"#,
    )]);
    assert_eq!(sorting::sort_for(db, OTHER, None, 3).await, 2);
    assert_eq!(
        placed(db, "bought a fern").await,
        Some(("node".to_string(), Some("their plants".to_string())))
    );
    assert_eq!(
        placed(db, "the weather was nice").await,
        Some(("nowhere".to_string(), None))
    );
    answering(&[]);
    assert_eq!(sorting::sort_for(db, OTHER, None, 3).await, 0);
    assert_eq!(asked("memory_node_sort"), 0, "nothing left to ask of");
    schema.drop().await;
}

#[tokio::test]
async fn when_it_happened_and_where_it_stands_go_on_its_entry() {
    let Some(schema) = schema("memory_nodes_standing").await else {
        return;
    };
    let db = &schema.db;
    let goes = || {
        Some(GoesUnder::New {
            name: "their laundry".into(),
            aliases: Vec::new(),
        })
    };
    let mut shirt = update("left a shirt at the cleaner's", &[], goes());
    shirt.standing = myriad_merope::happened::Standing {
        happened: Some(myriad_merope::happened::Happened {
            from: "2026-10-03".into(),
            to: None,
            about: false,
        }),
        state: Some("待取".into()),
        open: Some(true),
    };
    let coat = update("got a coat back", &[], goes());
    settle(db, vec![shirt, coat]).await;
    let body = |like: &str| {
        let sql = format!(
            "SELECT body FROM agent_node_rows WHERE kind = 'node.entry' AND body LIKE '%{like}%'"
        );
        async move {
            let row = db
                .query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
                .await
                .unwrap()
                .unwrap();
            serde_json::from_str::<Value>(&row.try_get::<String>("", "body").unwrap()).unwrap()
        }
    };
    let shirt = body("shirt").await;
    assert_eq!(shirt["happened"], json!({"from": "2026-10-03"}));
    assert_eq!(shirt["state"], "待取");
    assert_eq!(shirt["open"], true);
    // None given: none kept, the entry as it was before.
    let coat = body("coat").await;
    assert!(coat.get("happened").is_none() && coat.get("state").is_none());
    schema.drop().await;
}
