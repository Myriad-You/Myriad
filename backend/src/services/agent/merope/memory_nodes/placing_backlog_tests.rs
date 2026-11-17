//! What placing could not place, placed later at her upkeep, let go after
//! a week, and never written twice (as `talk_tests`, on a database of
//! their own, the model's answers given by the test).

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use super::super::super::talk_tests::{OTHER, answering, asked, count, schema, talk, topic};
use super::super::super::{inbox, upkeep};
use super::super::{self as placing, Line};
use super::{NAMESPACE, failures};
use crate::services::channel_group::g0;
use crate::services::runtime_registry as registry;

/// `texts` said in their private talk an hour ago, a minute apart, theirs
/// and hers in turn.
async fn said(db: &DatabaseConnection, texts: &[&str]) -> Vec<Line> {
    db.execute_unprepared(
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) VALUES ('backlog-talk', 7, 't', '{\"mode\":\"chat\"}'::jsonb, \
         0, false, NOW(), NOW()) ON CONFLICT DO NOTHING",
    )
    .await
    .unwrap();
    let began = crate::clock::now().fixed_offset() - chrono::Duration::minutes(60);
    let mut lines = Vec::new();
    for (index, text) in texts.iter().enumerate() {
        let theirs = index % 2 == 0;
        let at = began + chrono::Duration::minutes(index as i64);
        let id: i32 = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO agent_messages (session_id, role, content, metadata, created_at) \
                 VALUES ('backlog-talk', $1, $2, '{}'::jsonb, $3) RETURNING id",
                [
                    if theirs { "user" } else { "assistant" }.into(),
                    text.to_string().into(),
                    at.into(),
                ],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "id")
            .unwrap();
        lines.push(Line {
            id,
            theirs,
            text: text.to_string(),
            at,
        });
    }
    lines
}

async fn kept(db: &DatabaseConnection) -> Option<Value> {
    registry::get(db, NAMESPACE, &OTHER.to_string())
        .await
        .unwrap()
}

#[tokio::test]
async fn what_placing_could_not_place_is_placed_at_her_upkeep() {
    let Some(schema) = schema("memory_nodes_backlog").await else {
        return;
    };
    let db = &schema.db;
    let lines = said(db, &["my fern got new leaves", "that's lovely"]).await;
    // Cut off: nothing placed, kept to place later.
    answering(&[("memory_node_place", "{}")]);
    g0::with(|world| world.cut_off.insert("memory_node_place".into(), 1));
    assert!(placing::place(db, OTHER, None, &lines).await.is_empty());
    assert_eq!(asked("memory_node_place"), 1);
    assert!(kept(db).await.is_some(), "kept to place later");
    assert!(
        inbox::waiting(db, OTHER, &talk(), 10)
            .await
            .unwrap()
            .is_empty(),
        "nothing placed yet"
    );
    let before = failures();
    // At her upkeep: placed, and let go.
    answering(&[(
        "memory_node_place",
        r#"{"spans":[{"start":1,"node":null,"gist":"their fern grows"}]}"#,
    )]);
    upkeep::tick(db).await;
    assert_eq!(asked("memory_node_place"), 1);
    let waiting = inbox::waiting(db, OTHER, &talk(), 10).await.unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(
        waiting[0].refs,
        lines
            .iter()
            .map(|line| format!("message:{}", line.id))
            .collect::<Vec<_>>()
    );
    assert!(kept(db).await.is_none(), "placed: let go");
    assert!(failures().placed_later > before.placed_later);
    // Nothing kept: nothing asked.
    answering(&[]);
    upkeep::tick(db).await;
    assert_eq!(asked("memory_node_place"), 0);
    schema.drop().await;
}

#[tokio::test]
async fn what_is_never_placed_in_a_week_is_let_go_and_counted() {
    let Some(schema) = schema("memory_nodes_backlog_week").await else {
        return;
    };
    let db = &schema.db;
    let lines = said(
        db,
        &["my fern got new leaves", "that's lovely", "and a cactus"],
    )
    .await;
    let now = crate::clock::now().timestamp();
    registry::put(
        db,
        NAMESPACE,
        &OTHER.to_string(),
        registry::RegistryIdentity {
            subject_id: Some(OTHER),
            owner_id: Some(OTHER),
            tapp_id: None,
            runtime_id: None,
        },
        &json!({
            "from": lines[0].at.timestamp_micros(),
            "until": lines[2].at.timestamp_micros(),
            "since": now - 8 * 86_400,
            "failures": 2,
            "at": now - 86_400,
        }),
        now + 86_400,
    )
    .await
    .unwrap();
    let before = failures();
    answering(&[]);
    upkeep::tick(db).await;
    assert_eq!(asked("memory_node_place"), 0, "not asked again");
    assert!(kept(db).await.is_none(), "let go");
    assert_eq!(failures().let_go, before.let_go + 3, "each line counted");
    schema.drop().await;
}

#[tokio::test]
async fn having_failed_twice_it_waits_a_day() {
    let Some(schema) = schema("memory_nodes_backlog_waits").await else {
        return;
    };
    let db = &schema.db;
    let lines = said(db, &["my fern got new leaves", "that's lovely"]).await;
    let now = crate::clock::now().timestamp();
    let waiting = json!({
        "from": lines[0].at.timestamp_micros(),
        "until": lines[1].at.timestamp_micros(),
        "since": now - 86_400,
        "failures": 2,
        "at": now - 3_600,
    });
    registry::put(
        db,
        NAMESPACE,
        &OTHER.to_string(),
        registry::RegistryIdentity {
            subject_id: Some(OTHER),
            owner_id: Some(OTHER),
            tapp_id: None,
            runtime_id: None,
        },
        &waiting,
        now + 86_400,
    )
    .await
    .unwrap();
    answering(&[]);
    upkeep::tick(db).await;
    assert_eq!(asked("memory_node_place"), 0, "not asked within the day");
    assert_eq!(kept(db).await, Some(waiting), "still kept as it was");
    schema.drop().await;
}

/// A long stretch: placing is shown its first 40 lines; the rest are kept,
/// and her next upkeep places them, 40 a call, each line once.
#[tokio::test]
async fn a_long_stretch_is_all_placed_by_her_next_upkeep_each_line_once() {
    let Some(schema) = schema("memory_nodes_backlog_long").await else {
        return;
    };
    let db = &schema.db;
    let texts: Vec<String> = (0..100).map(|index| format!("line-{index:03}")).collect();
    let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
    let lines = said(db, &texts).await;
    let inbox_span = r#"{"spans":[{"start":1,"node":null,"gist":"a long talk"}]}"#;
    answering(&[("memory_node_place", inbox_span)]);
    placing::place(db, OTHER, None, &lines).await;
    assert_eq!(asked("memory_node_place"), 1);
    let unplaced = kept(db).await.expect("the rest kept to place later");
    assert_eq!(
        unplaced["from"].as_i64(),
        Some(lines[40].at.timestamp_micros()),
        "from the first not shown"
    );
    let cited = |distinct: bool| {
        format!(
            "SELECT COUNT({}cited.ref) AS n FROM agent_node_inbox i \
             CROSS JOIN LATERAL jsonb_array_elements_text(i.item->'refs') AS cited(ref) \
             WHERE i.user_id = {OTHER}",
            if distinct { "DISTINCT " } else { "" }
        )
    };
    assert_eq!(count(db, &cited(false)).await, 40);
    // Her next upkeep: lines 41 to 80, then 81 to 100; all placed, let go.
    answering(&[
        ("memory_node_place", inbox_span),
        ("memory_node_place", inbox_span),
    ]);
    upkeep::tick(db).await;
    assert_eq!(asked("memory_node_place"), 2, "two calls in one upkeep");
    assert!(kept(db).await.is_none(), "all placed: let go");
    assert_eq!(count(db, &cited(false)).await, 100, "every line placed");
    assert_eq!(count(db, &cited(true)).await, 100, "each once");
    schema.drop().await;
}

#[tokio::test]
async fn the_same_lines_placed_twice_at_once_are_written_once() {
    let Some(schema) = schema("memory_nodes_backlog_twice").await else {
        return;
    };
    let db = &schema.db;
    let plants = topic(db, "houseplants", &[("m0", "has a fern")]).await;
    let lines = said(db, &["my fern got new leaves", "that's lovely"]).await;
    let answer = r#"{"spans":[{"start":1,"node":"n1","gist":"their fern grows"}]}"#;
    answering(&[("memory_node_place", answer), ("memory_node_place", answer)]);
    // Each waits on its answer after it looked for what was placed: both
    // see nothing placed, and both write.
    g0::with(|world| {
        world
            .delays
            .insert("memory_node_place".into(), [0.2, 0.2].into())
    });
    let (first, second) = tokio::join!(
        placing::place(db, OTHER, None, &lines),
        placing::place(db, OTHER, None, &lines)
    );
    assert_eq!(asked("memory_node_place"), 2, "both asked");
    assert!(first.contains(&plants) || second.contains(&plants));
    let spans = "SELECT COUNT(*) AS n FROM agent_node_rows WHERE kind = 'node.span'";
    assert_eq!(count(db, spans).await, 1, "written once");
    schema.drop().await;
}
