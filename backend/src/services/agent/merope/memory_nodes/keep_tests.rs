//! Her nodes kept up and forgotten (her upkeep, forgetting a memory,
//! without her pen), on a database of their own, as `talk_tests`.

use sea_orm::TransactionTrait;
use serde_json::{Value, json};

use super::switch::{Mode, set_for_test};
use super::talk_tests::{
    OTHER, answering, asked, count, kept_today, line, memory_id, old_tables, schema, settle, talk,
    topic, update,
};
use super::{forget, placing, read, sorting, upkeep};
use crate::services::agent::merope::store::GoesUnder;
use crate::services::channel_group::g0;
use crate::services::her_log::pen;

#[tokio::test]
async fn her_upkeep_merges_a_duplicate_and_keeps_to_the_days_budget() {
    let Some(schema) = schema("memory_nodes_upkeep").await else {
        return;
    };
    let db = &schema.db;
    set_for_test(Some(Mode::Shadow));
    let first = topic(db, "houseplants", &[("m1", "has a fern")]).await;
    let second = topic(db, "plants at home", &[("m2", "waters on Sundays")]).await;
    // Five calls made already today: one is left.
    let audience = talk();
    answering(&[(
        "memory_node_upkeep",
        r#"{"actions":[{"do":"absorb","into":"n2","how":"merged","reason":"one thing","because":["e1"],"as":null,"name":null}]}"#,
    )]);
    assert_eq!(upkeep::calls_today(db, OTHER, &audience).await.unwrap(), 0);
    crate::services::runtime_registry::put(
        db,
        upkeep::DAY_NAMESPACE,
        &format!(
            "{OTHER}:{audience}:{}",
            crate::services::agent::merope::clock::today()
        ),
        crate::services::runtime_registry::RegistryIdentity {
            subject_id: Some(OTHER),
            owner_id: Some(OTHER),
            tapp_id: None,
            runtime_id: None,
        },
        &5u32,
        crate::clock::now().timestamp() + 86_400,
    )
    .await
    .unwrap();
    upkeep::tick(db).await;
    let spent = upkeep::calls_today(db, OTHER, &audience).await.unwrap();
    assert_eq!(asked("memory_node_upkeep"), 1, "one call left today");
    assert_eq!(spent, 6);
    let heads = read::heads(db, OTHER, &audience, None).await.unwrap();
    let status = |id: &str| {
        heads
            .iter()
            .find(|head| head.node_id == id)
            .unwrap()
            .status
            .clone()
    };
    assert_eq!(status(&first), "absorbed");
    assert_eq!(status(&second), "active");
    let entries = read::entries(db, OTHER, &audience, &second).await.unwrap();
    assert_eq!(entries.len(), 2, "what was on it is on the other now");

    // The day's budget is spent: nothing more is asked today.
    answering(&[("memory_node_upkeep", r#"{"actions":[]}"#)]);
    upkeep::tick(db).await;
    assert_eq!(asked("memory_node_upkeep"), 0);
    set_for_test(None);
    schema.drop().await;
}

#[tokio::test]
async fn upkeep_with_nothing_due_asks_and_writes_nothing() {
    let Some(schema) = schema("memory_nodes_upkeep_idle").await else {
        return;
    };
    let db = &schema.db;
    set_for_test(Some(Mode::Shadow));
    topic(db, "houseplants", &[("m1", "has a fern")]).await;
    answering(&[("memory_node_upkeep", r#"{"actions":[]}"#)]);
    upkeep::tick(db).await;
    assert_eq!(asked("memory_node_upkeep"), 1, "a new node is due once");
    let rows = "SELECT COUNT(*) AS n FROM agent_node_rows";
    let after_mark = count(db, rows).await;
    assert_eq!(after_mark, 3, "created, the entry, and her mark");
    let marks = "SELECT COUNT(*) AS n FROM agent_node_rows WHERE kind = 'node.upkept'";
    assert_eq!(count(db, marks).await, 1);
    answering(&[]);
    upkeep::tick(db).await;
    assert_eq!(asked("memory_node_upkeep"), 0, "nothing changed since");
    assert_eq!(count(db, rows).await, after_mark);
    set_for_test(None);
    schema.drop().await;
}

#[tokio::test]
async fn forgetting_a_memory_takes_its_entry_and_an_empty_topic() {
    let Some(schema) = schema("memory_nodes_forget").await else {
        return;
    };
    let db = &schema.db;
    set_for_test(Some(Mode::Shadow));
    settle(
        db,
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
                "sees Dr. Lee",
                &[],
                Some(GoesUnder::New {
                    name: "seeing doctors".into(),
                    aliases: Vec::new(),
                }),
            ),
            update(
                "sees a dentist",
                &[],
                Some(GoesUnder::New {
                    name: "seeing doctors".into(),
                    aliases: Vec::new(),
                }),
            ),
        ],
    )
    .await;
    let heads = read::heads(db, OTHER, &talk(), None).await.unwrap();
    let id_of = |name: &str| {
        heads
            .iter()
            .find(|head| head.name.as_deref() == Some(name))
            .unwrap()
            .node_id
            .clone()
    };
    let (plants, doctors) = (id_of("their plants"), id_of("seeing doctors"));
    // A span of talk on the doctors' node, from a line that will be forgotten.
    answering(&[(
        "memory_node_place",
        r#"{"spans":[{"start":1,"node":"n1","gist":"a check-up"}]}"#,
    )]);
    let said = "seeing doctors again: the check-up went fine";
    let used = placing::place(db, OTHER, None, &[line(555, true, said, 0)]).await;
    assert_eq!(asked("memory_node_place"), 1);
    assert_eq!(used, vec![doctors.clone()]);

    let snake = memory_id(db, "has a snake plant").await;
    assert!(
        crate::services::agent::memory::manage::forget_any(db, &snake)
            .await
            .unwrap()
    );
    let entry = format!("SELECT COUNT(*) AS n FROM agent_node_rows WHERE body LIKE '%{snake}%'");
    assert_eq!(count(db, &entry).await, 0, "no word of it is kept");
    let heads = read::heads(db, OTHER, &talk(), None).await.unwrap();
    let plants_head = heads.iter().find(|head| head.node_id == plants).unwrap();
    assert_eq!(
        plants_head.status, "forgotten",
        "a topic left empty goes too"
    );
    assert_eq!(plants_head.name, None);
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) AS n FROM agent_node_rows WHERE body LIKE '%their plants%'"
        )
        .await,
        0
    );
    assert_eq!(
        old_tables(db).await.0,
        ["seeing doctors"],
        "as the old tables do"
    );

    // Its lines forgotten: the span of them goes, the node with entries stays.
    let transaction = db.begin().await.unwrap();
    forget::forget(&transaction, &[], &[555]).await.unwrap();
    transaction.commit().await.unwrap();
    let node = read::node(db, OTHER, &talk(), &doctors)
        .await
        .unwrap()
        .unwrap();
    assert!(node["spans"].as_array().is_none_or(Vec::is_empty));
    // Each row forgotten alone has a forget of its own, for the other sides.
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*) AS n FROM agent_node_rows WHERE kind = 'forget' \
             AND envelope->>'object' LIKE 'row:%'"
        )
        .await,
        2,
        "the snake plant's entry and the span"
    );
    assert_eq!(
        read::entries(db, OTHER, &talk(), &doctors)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        read::heads(db, OTHER, &talk(), None)
            .await
            .unwrap()
            .iter()
            .find(|head| head.node_id == doctors)
            .unwrap()
            .status,
        "active"
    );
    set_for_test(None);
    schema.drop().await;
}

#[tokio::test]
async fn without_her_pen_none_of_her_rows_are_written() {
    let Some(schema) = schema("memory_nodes_no_pen").await else {
        return;
    };
    let db = &schema.db;
    set_for_test(Some(Mode::Shadow));
    pen::reset_for_test();
    assert!(pen::quiet("dev_mac", 1, std::time::Duration::ZERO).await);
    assert!(pen::think().is_none());
    settle(db, kept_today()).await;
    answering(&[
        (
            "memory_node_place",
            r#"{"spans":[{"start":1,"node":null,"gist":"a plant"}]}"#,
        ),
        ("memory_node_sort", r#"{"facts":[]}"#),
        ("memory_node_upkeep", r#"{"actions":[]}"#),
    ]);
    placing::place(db, OTHER, None, &[line(301, true, "a plant", 0)]).await;
    sorting::sort_for(db, OTHER, None, 3).await;
    upkeep::tick(db).await;
    let world = g0::leave().unwrap();
    assert!(!world.log.iter().any(|line| line["kind"] == "model"));
    for table in [
        "agent_node_rows",
        "agent_node_placements",
        "agent_node_inbox",
    ] {
        assert_eq!(
            count(db, &format!("SELECT COUNT(*) AS n FROM {table}")).await,
            0,
            "{table}"
        );
    }
    // The old topics go on as ever.
    assert_eq!(old_tables(db).await.0, ["their plants"]);
    pen::reset_for_test();
    set_for_test(None);
    schema.drop().await;
}

#[test]
fn null_fields_of_an_action_are_left_out_for_the_core() {
    let raw = r#"{"actions":[{"do":"status","status":"resting","as":null,"because":null}]}"#;
    let plain: Value = serde_json::from_str(&upkeep::plain(raw)).unwrap();
    assert_eq!(
        plain,
        json!({"actions": [{"do": "status", "status": "resting"}]})
    );
    assert_eq!(upkeep::plain("not json"), "not json");
    // The schemas are what the model is held to: every action field named.
    let schema = upkeep::schema();
    let required = schema["properties"]["actions"]["items"]["required"]
        .as_array()
        .unwrap();
    assert!(required.iter().any(|field| field == "do"));
    assert_eq!(placing::schema()["required"], json!(["spans"]));
}
