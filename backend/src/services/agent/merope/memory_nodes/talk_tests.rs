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
use super::{Author, RowIn, audience, facts, forget, heads, inbox, read, sorting, upkeep, write};
use crate::services::agent::memory::unified::Audience;
use crate::services::agent::merope::chat_remember::{ChatMemoryUpdate, ChatMemoryUpdates};
use crate::services::agent::merope::store::{self, Keep};
use crate::services::channel_group::g0;
use crate::services::her_log::pen;

pub(super) const OTHER: i32 = 7;

pub(super) async fn schema(prefix: &str) -> Option<crate::db::IsolatedSchema> {
    let url = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"])?;
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
            .or_default()
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

/// The node.standing rows of the talk, oldest first: item, text, whether
/// it ended, its sync, and its base.
async fn standing_rows(db: &DatabaseConnection) -> Vec<(String, String, bool, Value, Value)> {
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT body FROM agent_node_rows WHERE kind = 'node.standing' ORDER BY at, hlc",
    ))
    .await
    .unwrap()
    .iter()
    .map(|row| {
        let body: Value =
            serde_json::from_str(&row.try_get::<String>("", "body").unwrap()).unwrap();
        (
            body["item"].as_str().unwrap().to_string(),
            body["text"].as_str().unwrap().to_string(),
            body["ended"] == true,
            body["sync"].clone(),
            body["base"].clone(),
        )
    })
    .collect()
}

#[tokio::test]
async fn what_a_kept_fact_does_to_how_their_life_stands_is_written_where_it_stands() {
    let Some(schema) = schema("memory_nodes_standing_step").await else {
        return;
    };
    let db = &schema.db;
    let running = || {
        Some(GoesUnder::New {
            name: "跑步".into(),
            aliases: Vec::new(),
        })
    };
    // Something it starts: on the topic asked, theirs, kept on the site.
    answering(&[(
        "memory_standing_step",
        r#"{"whose":"them","set":[],"end":[],"add":[{"topic":"跑步","text":"每周三晚上夜跑"}]}"#,
    )]);
    settle(db, vec![update("每周三晚上夜跑", &[], running())]).await;
    assert_eq!(asked("memory_standing_step"), 1);
    let rows = standing_rows(db).await;
    assert_eq!(rows.len(), 1);
    let (item, text, ended, sync, base) = rows[0].clone();
    assert!(item.starts_with("them-"));
    assert_eq!((text.as_str(), ended), ("每周三晚上夜跑", false));
    assert_eq!(sync, json!({"local": crate::services::her_log::NODE}));
    assert_eq!(base, json!([]));
    // Someone else's news never ends their own item.
    answering(&[(
        "memory_standing_step",
        r#"{"whose":"男朋友","set":[{"item":1,"text":"男朋友不跑了"}],"end":[1],"add":[]}"#,
    )]);
    settle(db, vec![update("男朋友说他不跑步了", &[], running())]).await;
    assert_eq!(asked("memory_standing_step"), 1);
    assert_eq!(standing_rows(db).await.len(), 1);
    // Theirs ending: in the fact's own words, on the row it stood on.
    answering(&[(
        "memory_standing_step",
        r#"{"whose":"them","set":[],"end":[1],"add":[]}"#,
    )]);
    settle(db, vec![update("膝盖伤了，不夜跑了", &[], running())]).await;
    assert_eq!(asked("memory_standing_step"), 1);
    let rows = standing_rows(db).await;
    assert_eq!(rows.len(), 2);
    let (ended_item, text, ended, _, base) = rows[1].clone();
    assert_eq!(ended_item, item);
    assert_eq!((text.as_str(), ended), ("膝盖伤了，不夜跑了", true));
    assert_eq!(base.as_array().map(Vec::len), Some(1));
    let heads = read::heads(db, OTHER, &talk(), None).await.unwrap();
    let node = heads
        .iter()
        .find(|head| head.name.as_deref() == Some("跑步"))
        .unwrap();
    let head = read::node(db, OTHER, &talk(), &node.node_id)
        .await
        .unwrap()
        .unwrap();
    let now = &head["standing"][&item];
    assert_eq!(now["value"]["ended"], true);
    assert!(now.get("bothChanged").is_none());
    // An ended item is not shown again.
    assert!(myriad_nodes::stands::shown_on(&node.node_id, &head).is_empty());
    schema.drop().await;
}

/// Kept on the site's side while a side of theirs would refuse it; written
/// to go once every side reads it, and what was kept before goes then too
/// (`share_kept`, as when a side comes), under its own id.
#[tokio::test]
async fn how_their_life_stands_goes_to_their_sides_once_every_side_reads_it() {
    let Some(schema) = schema("memory_nodes_standing_shared").await else {
        return;
    };
    let db = &schema.db;
    let topic = |name: &str| {
        Some(GoesUnder::New {
            name: name.into(),
            aliases: Vec::new(),
        })
    };
    let local = json!({"local": crate::services::her_log::NODE});
    // A side of theirs still on log2: kept on the site.
    crate::services::her_log::relay::remember_side(db, OTHER, "dev_mac", 2)
        .await
        .unwrap();
    answering(&[(
        "memory_standing_step",
        r#"{"whose":"them","set":[],"end":[],"add":[{"topic":"跑步","text":"每周三晚上夜跑"}]}"#,
    )]);
    settle(db, vec![update("每周三晚上夜跑", &[], topic("跑步"))]).await;
    let rows = standing_rows(db).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].3, local);
    // Every side reads log3: written to go.
    crate::services::her_log::relay::remember_side(db, OTHER, "dev_mac", 3)
        .await
        .unwrap();
    answering(&[(
        "memory_standing_step",
        r#"{"whose":"them","set":[],"end":[],"add":[{"topic":"游泳","text":"周末去游泳"}]}"#,
    )]);
    settle(db, vec![update("周末去游泳", &[], topic("游泳"))]).await;
    let rows = standing_rows(db).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0].3.clone(), rows[1].3.clone()),
        (local, json!("shared"))
    );
    // The one kept before goes too, once.
    let shared = super::share::share_kept(db, OTHER).await.unwrap();
    assert_eq!(shared, 1);
    let rows = standing_rows(db).await;
    assert!(rows.iter().all(|row| row.3 == json!("shared")), "{rows:?}");
    assert_eq!(super::share::share_kept(db, OTHER).await.unwrap(), 0);
    schema.drop().await;
}

/// How things stand on 「跑步」 in account 7's talk, as shown.
async fn running_stands(db: &DatabaseConnection) -> Vec<String> {
    let heads = read::heads(db, OTHER, &talk(), None).await.unwrap();
    let Some(node) = heads
        .iter()
        .find(|head| head.name.as_deref() == Some("跑步"))
    else {
        return Vec::new();
    };
    let Some(head) = read::node(db, OTHER, &talk(), &node.node_id).await.unwrap() else {
        return Vec::new();
    };
    myriad_nodes::stands::shown_on(&node.node_id, &head)
        .into_iter()
        .map(|item| item.text)
        .collect()
}

#[tokio::test]
async fn how_their_life_stands_goes_with_the_memory_it_rests_on() {
    let Some(schema) = schema("memory_nodes_standing_forget").await else {
        return;
    };
    let db = &schema.db;
    let running = || {
        Some(GoesUnder::New {
            name: "跑步".into(),
            aliases: Vec::new(),
        })
    };
    answering(&[(
        "memory_standing_step",
        r#"{"whose":"them","set":[],"end":[],"add":[{"topic":"跑步","text":"每周三晚上夜跑"}]}"#,
    )]);
    settle(db, vec![update("每周三晚上夜跑", &[], running())]).await;
    answering(&[(
        "memory_standing_step",
        r#"{"whose":"them","set":[{"item":1,"text":"每周五晚上夜跑"}],"end":[],"add":[]}"#,
    )]);
    settle(db, vec![update("夜跑改到周五了", &[], running())]).await;
    assert_eq!(running_stands(db).await, vec!["每周五晚上夜跑".to_string()]);
    // The change forgotten: it stands as it did before it.
    let changed = memory_id(db, "夜跑改到周五了").await;
    forget::forget(db, &[changed], &[]).await.unwrap();
    assert_eq!(running_stands(db).await, vec!["每周三晚上夜跑".to_string()]);
    // What it began with forgotten too: nothing of it stands.
    let began = memory_id(db, "每周三晚上夜跑").await;
    forget::forget(db, &[began], &[]).await.unwrap();
    assert!(running_stands(db).await.is_empty());
    let left = count(
        db,
        "SELECT count(*) AS n FROM agent_node_rows WHERE kind = 'node.standing' AND body <> ''",
    )
    .await;
    assert_eq!(left, 0);
    schema.drop().await;
}

#[tokio::test]
async fn what_she_knew_before_stands_first_and_a_new_fact_ends_it() {
    let Some(schema) = schema("memory_nodes_standing_built").await else {
        return;
    };
    let db = &schema.db;
    let running = || {
        Some(GoesUnder::New {
            name: "跑步".into(),
            aliases: Vec::new(),
        })
    };
    // Kept before anything stood: nothing to build from but itself, and
    // the step starts nothing.
    answering(&[(
        "memory_standing_step",
        r#"{"whose":"them","set":[],"end":[],"add":[]}"#,
    )]);
    let mut first = update("每周三晚上夜跑", &[], running());
    first.standing.happened = Some(myriad_merope::happened::Happened {
        from: "2026-08-21".into(),
        to: None,
        about: false,
    });
    settle(db, vec![first]).await;
    assert_eq!(asked("memory_standing_build"), 0);
    assert!(standing_rows(db).await.is_empty());
    // A rule's 「现在」 (where a session is now) in the same talk is not
    // hers: it does not count as built.
    let node = read::heads(db, OTHER, &talk(), None)
        .await
        .unwrap()
        .into_iter()
        .find(|head| head.name.as_deref() == Some("跑步"))
        .unwrap()
        .node_id;
    let session = json!({
        "version": myriad_nodes::BODY_VERSION,
        "sync": myriad_nodes::Sync::Local(crate::services::her_log::NODE.to_string()),
        "saidBy": ["third_party"],
        "base": [],
        "item": "now",
        "text": "quiet for long, its tests still running",
        "because": ["session:s0123456789abcdef01234567"],
    });
    write::write(
        db,
        OTHER,
        &talk(),
        &[RowIn {
            id: myriad_nodes::entries::row_id_from("node.standing", &node, "now", "rule"),
            node,
            kind: "node.standing".into(),
            body: session,
            author: Author::Rule,
        }],
        None,
    )
    .await
    .unwrap();
    let rule_rows = standing_rows(db).await.len();
    assert_eq!(rule_rows, 1);
    // The next stretch: first how things stood (from what she knew, not
    // from what it kept), then what it does to that.
    answering(&[
        (
            "memory_standing_build",
            r#"{"items":[{"whose":"them","text":"每周三晚上夜跑","because":[1]}]}"#,
        ),
        (
            "memory_standing_step",
            r#"{"whose":"them","set":[],"end":[1],"add":[]}"#,
        ),
    ]);
    settle(db, vec![update("膝盖伤了，不夜跑了", &[], running())]).await;
    // The day she was told, read in the zone it was written in: the world's
    // (+08:00), not the site's after it is left (an hour apart in Tokyo, so
    // they differ from midnight to one there).
    let today = crate::clock::now()
        .with_timezone(&crate::services::agent::merope::clock::zone())
        .format("%Y-%m-%d")
        .to_string();
    let world = g0::leave().expect("a world");
    let built: Vec<&Value> = world
        .log
        .iter()
        .filter(|line| {
            line["kind"] == "model" && line["what"]["operation"] == "memory_standing_build"
        })
        .collect();
    assert_eq!(built.len(), 1);
    let input = built[0]["what"]["input"].as_str().unwrap();
    assert!(input.contains("每周三晚上夜跑") && !input.contains("膝盖"));
    // Each memory with when it happened, not when it was put on the node.
    assert!(input.contains("2026-08-21"), "{input}");
    let rows: Vec<_> = standing_rows(db)
        .await
        .into_iter()
        .filter(|row| row.0 != "now")
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].1, "每周三晚上夜跑");
    assert_eq!(rows[0].4, json!([]));
    assert_eq!(rows[1].0, rows[0].0, "the same item, ended");
    assert_eq!(
        (rows[1].1.as_str(), rows[1].2),
        ("膝盖伤了，不夜跑了", true)
    );
    assert_eq!(rows[1].4.as_array().map(Vec::len), Some(1));
    // Since: what it rests on happened then; the news, with no day of its
    // own, the day she was told.
    let sinces: Vec<Value> = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT body FROM agent_node_rows WHERE kind = 'node.standing' AND author = 'her' \
             ORDER BY at, hlc",
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| {
            serde_json::from_str::<Value>(&row.try_get::<String>("", "body").unwrap()).unwrap()
                ["since"]
                .clone()
        })
        .collect();
    assert_eq!(sinces, vec![json!("2026-08-21"), json!(today)]);
    schema.drop().await;
}

/// What she knew on a node goes first, by node: one built elsewhere in the
/// talk does not stand for a node a new fact is put on; a node one of hers
/// stood on is not built again; and one built to nothing is gone over again
/// the next time a fact is put on it.
#[tokio::test]
async fn what_she_knew_on_a_node_stands_first_whatever_stands_elsewhere() {
    let Some(schema) = schema("memory_nodes_standing_by_node").await else {
        return;
    };
    let db = &schema.db;
    let under = |name: &str| {
        Some(GoesUnder::New {
            name: name.into(),
            aliases: Vec::new(),
        })
    };
    let step_nothing = r#"{"whose":"them","set":[],"end":[],"add":[]}"#;
    // One of hers stands on 跑步; 咖啡 holds a memory, none of hers.
    answering(&[
        (
            "memory_standing_step",
            r#"{"whose":"them","set":[],"end":[],"add":[{"topic":"跑步","text":"每周三晚上夜跑"}]}"#,
        ),
        ("memory_standing_step", step_nothing),
    ]);
    settle(
        db,
        vec![
            update("每周三晚上夜跑", &[], under("跑步")),
            update("常去春山咖啡看书", &[], under("咖啡")),
        ],
    )
    .await;
    assert_eq!(asked("memory_standing_build"), 0, "nothing on them before");
    assert_eq!(standing_rows(db).await.len(), 1);
    // A fact on 咖啡: built first though 跑步 stands; built to nothing.
    answering(&[
        ("memory_standing_build", r#"{"items":[]}"#),
        ("memory_standing_step", step_nothing),
    ]);
    settle(db, vec![update("春山咖啡周末人很多", &[], under("咖啡"))]).await;
    assert_eq!(asked("memory_standing_build"), 1, "by node, not by talk");
    assert_eq!(standing_rows(db).await.len(), 1);
    // Built to nothing, so gone over again: this time something stands.
    answering(&[
        (
            "memory_standing_build",
            r#"{"items":[{"whose":"them","text":"常去春山咖啡看书","because":[1]}]}"#,
        ),
        ("memory_standing_step", step_nothing),
    ]);
    settle(db, vec![update("春山咖啡换了新的店长", &[], under("咖啡"))]).await;
    assert_eq!(asked("memory_standing_build"), 1, "built to nothing: again");
    assert_eq!(standing_rows(db).await.len(), 2);
    // Both now hold one of hers: neither is built again.
    answering(&[
        ("memory_standing_step", step_nothing),
        ("memory_standing_step", step_nothing),
    ]);
    settle(
        db,
        vec![
            update("夜跑改到周四了", &[], under("跑步")),
            update("春山咖啡要搬家", &[], under("咖啡")),
        ],
    )
    .await;
    assert_eq!(asked("memory_standing_build"), 0, "stood on: not again");
    schema.drop().await;
}

/// What the step was shown of each fact since the world was entered.
fn step_inputs() -> Vec<String> {
    let world = g0::leave().expect("a world");
    world
        .log
        .iter()
        .filter(|line| {
            line["kind"] == "model" && line["what"]["operation"] == "memory_standing_step"
        })
        .filter_map(|line| line["what"]["input"].as_str().map(str::to_string))
        .collect()
}

/// On 「工作」: 「在趣养做运营」, then rewritten on what `then` says as
/// `rewritten`, each kept as a fact of its own.
async fn work_stands(db: &DatabaseConnection, then: &str, rewritten: &str) {
    let work = || {
        Some(GoesUnder::New {
            name: "工作".into(),
            aliases: Vec::new(),
        })
    };
    answering(&[(
        "memory_standing_step",
        r#"{"whose":"them","set":[],"end":[],"add":[{"topic":"工作","text":"在趣养做运营"}]}"#,
    )]);
    settle(db, vec![update("在趣养做运营", &[], work())]).await;
    let set =
        json!({"whose": "them", "set": [{"item": 1, "text": rewritten}], "end": [], "add": []});
    answering(&[("memory_standing_step", set.to_string().as_str())]);
    settle(db, vec![update(then, &[], work())]).await;
}

/// How things stand on 「工作」, as shown.
async fn work_now(db: &DatabaseConnection) -> Vec<String> {
    let heads = read::heads(db, OTHER, &talk(), None).await.unwrap();
    let node = heads
        .iter()
        .find(|head| head.name.as_deref() == Some("工作"))
        .unwrap();
    let head = read::node(db, OTHER, &talk(), &node.node_id)
        .await
        .unwrap()
        .unwrap();
    myriad_nodes::stands::shown_on(&node.node_id, &head)
        .into_iter()
        .map(|item| item.text)
        .collect()
}

/// A fact on 「工作」 with nothing to change: what the step was shown.
async fn shown_next(db: &DatabaseConnection, text: &str) -> String {
    answering(&[(
        "memory_standing_step",
        r#"{"whose":"them","set":[],"end":[],"add":[]}"#,
    )]);
    settle(
        db,
        vec![update(
            text,
            &[],
            Some(GoesUnder::New {
                name: "工作".into(),
                aliases: Vec::new(),
            }),
        )],
    )
    .await;
    step_inputs().join("\n")
}

/// A memory forgotten goes from how their life stands with every version
/// written on what rests on it: v1 [m1] → v2 on v1 [m2] → v3 on v2 [m3];
/// m2 forgotten, v2 and v3 are stubs and v1 stands again, and nothing of
/// m2 is shown to the step after.
#[tokio::test]
async fn what_was_written_on_a_forgotten_memory_goes_with_it() {
    let Some(schema) = schema("memory_nodes_standing_written_on").await else {
        return;
    };
    let db = &schema.db;
    work_stands(db, "同事小王在追她", "在趣养做运营，同事小王在追她").await;
    answering(&[(
        "memory_standing_step",
        r#"{"whose":"them","set":[{"item":1,"text":"在趣养做主管，同事小王在追她"}],"end":[],"add":[]}"#,
    )]);
    settle(
        db,
        vec![update(
            "升职做了主管",
            &[],
            Some(GoesUnder::New {
                name: "工作".into(),
                aliases: Vec::new(),
            }),
        )],
    )
    .await;
    assert_eq!(work_now(db).await, vec!["在趣养做主管，同事小王在追她"]);
    let wooed = memory_id(db, "同事小王在追她").await;
    forget::forget(db, &[wooed.clone()], &[]).await.unwrap();
    // The memory itself goes too, as forgetting does.
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memories SET invalid_at = NOW(), invalid_reason = 'deleted' WHERE id = $1",
        [wooed.into()],
    ))
    .await
    .unwrap();
    let stubs = count(
        db,
        "SELECT count(*) AS n FROM agent_node_rows WHERE kind = 'node.standing' AND body = ''",
    )
    .await;
    assert_eq!(stubs, 2, "v2 and v3");
    assert_eq!(work_now(db).await, vec!["在趣养做运营"]);
    let shown = shown_next(db, "趣养搬了新办公室").await;
    assert!(
        shown.contains("在趣养做运营") && !shown.contains("小王"),
        "{shown}"
    );
    schema.drop().await;
}

/// A version resting on a memory taken back where forgetting never reached
/// it (taken back on another of her ends, or before forgetting reached
/// 「现在」) is forgotten before the next fact is read: it stands as it
/// did before, and nothing of it is shown to the step.
#[tokio::test]
async fn what_rests_on_a_memory_taken_back_elsewhere_is_not_shown_again() {
    for (name, taken_back) in [
        (
            "memory_nodes_standing_taken_back_elsewhere",
            // Put aside with nothing in its place, as a taking back comes in.
            "UPDATE agent_memories SET invalid_at = NOW(), invalid_reason = 'superseded', \
             superseded_by = NULL WHERE content = '同事小王在追她'",
        ),
        (
            "memory_nodes_standing_deleted_before",
            // Deleted, as before forgetting reached 「现在」.
            "UPDATE agent_memories SET invalid_at = NOW(), invalid_reason = 'deleted' \
             WHERE content = '同事小王在追她'",
        ),
    ] {
        let Some(schema) = schema(name).await else {
            return;
        };
        let db = &schema.db;
        work_stands(db, "同事小王在追她", "在趣养做运营，同事小王在追她").await;
        db.execute_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            taken_back,
        ))
        .await
        .unwrap();
        // Its rows are still here, whole.
        assert_eq!(work_now(db).await, vec!["在趣养做运营，同事小王在追她"]);
        let shown = shown_next(db, "升职做了主管").await;
        assert!(
            shown.contains("在趣养做运营") && !shown.contains("小王"),
            "{name}: {shown}"
        );
        assert_eq!(work_now(db).await, vec!["在趣养做运营"], "{name}");
        schema.drop().await;
    }
}

/// A memory taken back as a stretch is gone over (put aside with nothing in
/// its place) goes from her nodes in that same writing, as when it is
/// forgotten: v1 [m1] → v2 on v1 [m2] → v3 on v2 [m3]; m2 taken back, v2
/// and v3 are stubs before anything else is read, v1 stands again, and
/// nothing of m2 is shown to the step after.
#[tokio::test]
async fn what_was_written_on_a_memory_taken_back_goes_with_it() {
    let Some(schema) = schema("memory_nodes_standing_taken_back_here").await else {
        return;
    };
    let db = &schema.db;
    work_stands(db, "同事小王在追她", "在趣养做运营，同事小王在追她").await;
    answering(&[(
        "memory_standing_step",
        r#"{"whose":"them","set":[{"item":1,"text":"在趣养做主管，同事小王在追她"}],"end":[],"add":[]}"#,
    )]);
    settle(
        db,
        vec![update(
            "升职做了主管",
            &[],
            Some(GoesUnder::New {
                name: "工作".into(),
                aliases: Vec::new(),
            }),
        )],
    )
    .await;
    assert_eq!(work_now(db).await, vec!["在趣养做主管，同事小王在追她"]);
    assert!(take_back(db, "同事小王在追她").await, "taken back");
    let stubs = count(
        db,
        "SELECT count(*) AS n FROM agent_node_rows WHERE kind = 'node.standing' AND body = ''",
    )
    .await;
    assert_eq!(stubs, 2, "v2 and v3, before anything else is read");
    assert_eq!(work_now(db).await, vec!["在趣养做运营"]);
    let shown = shown_next(db, "趣养搬了新办公室").await;
    assert!(
        shown.contains("在趣养做运营") && !shown.contains("小王"),
        "{shown}"
    );
    schema.drop().await;
}

/// What her upkeep wrote resting on an entry of a memory taken back or
/// forgotten (a summary sentence names its evidence by row, `entry:<row>`)
/// goes with it at once: before upkeep writes the summary again, it is in
/// no head she is shown.
#[tokio::test]
async fn a_summary_resting_on_what_goes_goes_with_it() {
    for (name, taken_back) in [
        ("memory_nodes_summary_taken_back", true),
        ("memory_nodes_summary_forgotten", false),
    ] {
        let Some(schema) = schema(name).await else {
            return;
        };
        let db = &schema.db;
        let (wooed, node, _) = summed_up(db).await;
        let before = read::node(db, OTHER, &talk(), &node)
            .await
            .unwrap()
            .unwrap();
        assert!(
            before["summary"]["value"].to_string().contains("小王"),
            "{name}"
        );
        if taken_back {
            db.execute_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "UPDATE agent_memories SET invalid_at = NOW(), invalid_reason = 'superseded', \
                 superseded_by = NULL WHERE content = '同事小王在追她'",
            ))
            .await
            .unwrap();
            forget::taken_back_here(db, OTHER, &talk()).await.unwrap();
        } else {
            forget::forget(db, &[wooed], &[]).await.unwrap();
        }
        let after = read::node(db, OTHER, &talk(), &node)
            .await
            .unwrap()
            .unwrap();
        assert!(!after.to_string().contains("小王"), "{name}: {after}");
        schema.drop().await;
    }
}

/// 「工作」 as [`work_stands`] leaves it, and a summary her upkeep wrote on
/// it resting on the entry of 「同事小王在追她」: that memory, the node,
/// the entry's row.
async fn summed_up(db: &DatabaseConnection) -> (String, String, String) {
    work_stands(db, "同事小王在追她", "在趣养做运营，同事小王在追她").await;
    let wooed = memory_id(db, "同事小王在追她").await;
    let entry: String = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT row_id FROM agent_node_rows WHERE kind = 'node.entry' AND body LIKE $1",
            [format!("%memory:{wooed}%").into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "row_id")
        .unwrap();
    let node = read::heads(db, OTHER, &talk(), None)
        .await
        .unwrap()
        .into_iter()
        .find(|head| head.name.as_deref() == Some("工作"))
        .unwrap()
        .node_id;
    let summary = json!({
        "version": myriad_nodes::BODY_VERSION,
        "sync": {"local": crate::services::her_log::NODE},
        "saidBy": ["her"],
        "base": [],
        "field": "summary",
        "value": [{"text": "在趣养做运营，同事小王在追她。", "because": [format!("entry:{entry}")]}],
    });
    let thought = pen::think().unwrap();
    write::write(
        db,
        OTHER,
        &talk(),
        &[RowIn {
            id: myriad_nodes::entries::row_id_from("node.revised", &node, "summary", "1"),
            node: node.clone(),
            kind: "node.revised".into(),
            body: summary,
            author: Author::Her,
        }],
        Some(&thought),
    )
    .await
    .unwrap();
    (wooed, node, entry)
}

/// What a forgetting before upkeep's evidence was followed left (a summary
/// resting on an entry it took, whole) goes the next time a stretch's
/// facts are read there.
#[tokio::test]
async fn what_an_earlier_forgetting_left_goes_at_the_next_stretch() {
    let Some(schema) = schema("memory_nodes_summary_left_before").await else {
        return;
    };
    let db = &schema.db;
    let (_, node, entry) = summed_up(db).await;
    // Forgotten as before: the entry alone, its summary left.
    let kept = heads::rows_of(db, OTHER, &talk()).await.unwrap();
    let row = kept
        .iter()
        .find(|kept| kept.row.id == entry)
        .unwrap()
        .row
        .clone();
    forget::forget_one(db, OTHER, &talk(), &node, forget::Forgets::Row(&row.id))
        .await
        .unwrap();
    forget::stub(db, OTHER, &[row]).await.unwrap();
    let folded = heads::fold_rows(&heads::rows_of(db, OTHER, &talk()).await.unwrap()).unwrap();
    heads::refresh(db, OTHER, &talk(), &folded).await.unwrap();
    let left = read::node(db, OTHER, &talk(), &node)
        .await
        .unwrap()
        .unwrap();
    assert!(left["summary"]["value"].to_string().contains("小王"));
    shown_next(db, "趣养搬了新办公室").await;
    let after = read::node(db, OTHER, &talk(), &node)
        .await
        .unwrap()
        .unwrap();
    assert!(after["summary"].is_null(), "{}", after["summary"]);
    schema.drop().await;
}

/// `content` taken back as a stretch is gone over (「当我没说」): put aside
/// with nothing in its place. Whether it was.
async fn take_back(db: &DatabaseConnection, content: &str) -> bool {
    let taken_back = ChatMemoryUpdate {
        fact: None,
        evidence: None,
        ..update("", &[content], None)
    };
    let transaction = db.begin().await.unwrap();
    let applied = store::apply_chat_memory_update_on(
        &transaction,
        OTHER,
        crate::clock::now().fixed_offset(),
        &taken_back,
        &Audience::private(OTHER),
        Keep::Lasting,
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    applied
}

/// The versions of how their life stands forgotten (stubs).
async fn standing_stubs(db: &DatabaseConnection) -> i64 {
    count(
        db,
        "SELECT count(*) AS n FROM agent_node_rows WHERE kind = 'node.standing' AND body = ''",
    )
    .await
}

/// How things stand on the topic `name`, as shown; nothing when no such
/// topic stands.
async fn now_on(db: &DatabaseConnection, name: &str) -> Vec<String> {
    let heads = read::heads(db, OTHER, &talk(), None).await.unwrap();
    let Some(node) = heads.iter().find(|head| head.name.as_deref() == Some(name)) else {
        return Vec::new();
    };
    let Some(head) = read::node(db, OTHER, &talk(), &node.node_id).await.unwrap() else {
        return Vec::new();
    };
    myriad_nodes::stands::shown_on(&node.node_id, &head)
        .into_iter()
        .map(|item| item.text)
        .collect()
}

fn teeth() -> Option<GoesUnder> {
    Some(GoesUnder::New {
        name: "看牙".into(),
        aliases: Vec::new(),
    })
}

/// Taking back the last word on something takes back what it corrected:
/// 「周五去看牙」 (v1), corrected to 「改到周六看牙」 (v2 on v1), then the
/// correction taken back: v1 and v2 are stubs, nothing of either stands,
/// and neither day is shown to the step after.
#[tokio::test]
async fn a_memory_taken_back_goes_with_what_it_replaced() {
    let Some(schema) = schema("memory_nodes_taken_back_line").await else {
        return;
    };
    let db = &schema.db;
    answering(&[(
        "memory_standing_step",
        r#"{"whose":"them","set":[],"end":[],"add":[{"topic":"看牙","text":"周五去看牙"}]}"#,
    )]);
    settle(db, vec![update("周五去看牙", &[], teeth())]).await;
    answering(&[(
        "memory_standing_step",
        r#"{"whose":"them","set":[{"item":1,"text":"周六去看牙"}],"end":[],"add":[]}"#,
    )]);
    settle(db, vec![update("改到周六看牙", &["周五去看牙"], teeth())]).await;
    assert_eq!(now_on(db, "看牙").await, vec!["周六去看牙"]);
    assert!(take_back(db, "改到周六看牙").await, "taken back");
    assert_eq!(standing_stubs(db).await, 2, "v1 and v2");
    assert!(now_on(db, "看牙").await.is_empty());
    answering(&[(
        "memory_standing_step",
        r#"{"whose":"them","set":[],"end":[],"add":[]}"#,
    )]);
    settle(db, vec![update("牙医诊所换了地址", &[], teeth())]).await;
    let shown = step_inputs().join("\n");
    assert!(
        shown.contains("牙医诊所换了地址") && !shown.contains("周五") && !shown.contains("周六"),
        "{shown}"
    );
    schema.drop().await;
}

/// Her nodes in this test's schema refuse every row written, or take them
/// again.
async fn refuse_node_rows(db: &DatabaseConnection, refuse: bool) {
    let here: String = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT current_schema() AS here",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "here")
        .unwrap();
    assert_ne!(here, "public", "only in the test's own schema");
    let sql = if refuse {
        format!(
            "CREATE FUNCTION \"{here}\".refuse_node_rows() RETURNS trigger LANGUAGE plpgsql AS \
             $$ BEGIN RAISE EXCEPTION 'refused in this test'; END $$; \
             CREATE TRIGGER refuse_node_rows BEFORE INSERT OR UPDATE ON \"{here}\".agent_node_rows \
             FOR EACH ROW EXECUTE FUNCTION \"{here}\".refuse_node_rows()"
        )
    } else {
        format!("DROP TRIGGER refuse_node_rows ON \"{here}\".agent_node_rows")
    };
    db.execute_unprepared(&sql).await.unwrap();
}

/// A memory taken back is let go even when her nodes cannot be written
/// then: what rests on it is left to the sweep and counted; a sweep that
/// stops is counted, and the next one forgets it.
#[tokio::test]
async fn a_memory_taken_back_is_let_go_when_her_nodes_fail() {
    let Some(schema) = schema("memory_nodes_taken_back_nodes_fail").await else {
        return;
    };
    let db = &schema.db;
    work_stands(db, "同事小王在追她", "在趣养做运营，同事小王在追她").await;
    refuse_node_rows(db, true).await;
    let before = forget::failures();
    assert!(take_back(db, "同事小王在追她").await, "taken back");
    let let_go = count(
        db,
        "SELECT count(*) AS n FROM agent_memories WHERE content = '同事小王在追她' \
         AND invalid_reason = 'superseded' AND superseded_by IS NULL",
    )
    .await;
    assert_eq!(let_go, 1, "let go whatever her nodes did");
    assert!(forget::failures().left_to_sweep > before.left_to_sweep);
    assert_eq!(standing_stubs(db).await, 0, "her nodes as they were");
    assert!(forget::taken_back_here(db, OTHER, &talk()).await.is_err());
    assert!(forget::failures().sweep_errors > before.sweep_errors);
    refuse_node_rows(db, false).await;
    forget::taken_back_here(db, OTHER, &talk()).await.unwrap();
    assert!(forget::failures().swept > before.swept);
    assert_eq!(standing_stubs(db).await, 1, "v2");
    assert_eq!(work_now(db).await, vec!["在趣养做运营"]);
    schema.drop().await;
}

/// A correction taken back on another of her ends (put aside with nothing
/// in its place, as it comes in) takes back what it corrected here too:
/// before the next fact is read, what rests on either is forgotten.
#[tokio::test]
async fn a_correction_taken_back_elsewhere_takes_back_what_it_corrected() {
    let Some(schema) = schema("memory_nodes_taken_back_line_elsewhere").await else {
        return;
    };
    let db = &schema.db;
    answering(&[(
        "memory_standing_step",
        r#"{"whose":"them","set":[],"end":[],"add":[{"topic":"看牙","text":"周五去看牙"}]}"#,
    )]);
    settle(db, vec![update("周五去看牙", &[], teeth())]).await;
    answering(&[(
        "memory_standing_step",
        r#"{"whose":"them","set":[{"item":1,"text":"周六去看牙"}],"end":[],"add":[]}"#,
    )]);
    settle(db, vec![update("改到周六看牙", &["周五去看牙"], teeth())]).await;
    db.execute_unprepared(
        "UPDATE agent_memories SET invalid_at = NOW(), invalid_reason = 'superseded', \
         superseded_by = NULL WHERE content = '改到周六看牙'",
    )
    .await
    .unwrap();
    answering(&[(
        "memory_standing_step",
        r#"{"whose":"them","set":[],"end":[],"add":[]}"#,
    )]);
    settle(db, vec![update("牙医诊所换了地址", &[], teeth())]).await;
    let shown = step_inputs().join("\n");
    assert!(
        !shown.contains("周五") && !shown.contains("周六"),
        "{shown}"
    );
    assert_eq!(standing_stubs(db).await, 2, "v1 and v2");
    schema.drop().await;
}

/// `update` kept as `keep` says, in a transaction of its own: whether it
/// was.
async fn kept(db: &DatabaseConnection, update: &ChatMemoryUpdate, keep: Keep) -> bool {
    let transaction = db.begin().await.unwrap();
    let applied = store::apply_chat_memory_update_on(
        &transaction,
        OTHER,
        crate::clock::now().fixed_offset(),
        update,
        &Audience::private(OTHER),
        keep,
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    applied
}

/// A line that corrects a short note with a new fact beside it
/// (「周五去看牙」 → 「改到周六看牙」, the correction going first on its
/// own, as a line's answer is read) replaces the note and takes nothing
/// back on her nodes; a line that only takes the note back forgets what
/// rests on it. (Her nodes hold only what is kept for good; the note is
/// put on one here so that what would go can be seen.)
#[tokio::test]
async fn a_correction_in_a_line_is_no_taking_back() {
    for (name, corrected, stubs) in [
        ("memory_nodes_line_correction", true, 0),
        ("memory_nodes_line_let_go", false, 1),
    ] {
        let Some(schema) = schema(name).await else {
            return;
        };
        let db = &schema.db;
        assert!(kept(db, &update("周五去看牙", &[], None), Keep::Note).await);
        let note = memory_id(db, "周五去看牙").await;
        topic(db, "看牙", &[(note.as_str(), "周五去看牙")]).await;
        let mut updates = vec![ChatMemoryUpdate {
            fact: None,
            evidence: None,
            ..update("", &["周五去看牙"], None)
        }];
        if corrected {
            updates.push(update("改到周六看牙", &[], None));
        }
        let updates = ChatMemoryUpdates {
            updates,
            ..ChatMemoryUpdates::default()
        };
        let transaction = db.begin().await.unwrap();
        assert!(
            store::apply_chat_memory_updates_on(
                &transaction,
                OTHER,
                crate::clock::now().fixed_offset(),
                &updates,
                &Audience::private(OTHER),
                Keep::Note,
            )
            .await
            .unwrap(),
            "{name}"
        );
        transaction.commit().await.unwrap();
        for (content, held) in [("周五去看牙", 0), ("改到周六看牙", i64::from(corrected))]
        {
            let sql = format!(
                "SELECT count(*) AS n FROM agent_memories WHERE content = '{content}' \
                 AND invalid_at IS NULL"
            );
            assert_eq!(count(db, &sql).await, held, "{name}: {content}");
        }
        let entries = count(
            db,
            "SELECT count(*) AS n FROM agent_node_rows WHERE kind = 'node.entry' AND body = ''",
        )
        .await;
        assert_eq!(entries, stubs, "{name}");
        schema.drop().await;
    }
}
