//! What rests on a memory they took back, hidden where her nodes are read
//! before the second look forgets it (`hidden`), on a database of its own.

use std::collections::BTreeSet;

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use super::talk_tests::{OTHER, confirmed, count, schema, summed_up, talk};
use super::{forget, read};

const TAKE_BACK: &str = "UPDATE agent_memories SET invalid_at = NOW(), \
     invalid_reason = 'superseded', superseded_by = NULL WHERE content = '同事小王在追她'";

/// The rows of theirs forgotten (stubs).
async fn stubs(db: &sea_orm::DatabaseConnection) -> BTreeSet<String> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT row_id FROM agent_node_rows WHERE user_id = $1 AND body = '' AND kind <> 'forget'",
        [OTHER.into()],
    ))
    .await
    .unwrap()
    .iter()
    .map(|row| row.try_get::<String>("", "row_id").unwrap())
    .collect()
}

/// Hidden is what forgetting takes, row for row: before the second look
/// nothing is forgotten and it reads as gone; once it agrees, the rows
/// forgotten are the rows that were hidden.
#[tokio::test]
async fn what_is_hidden_is_what_forgetting_takes() {
    let Some(schema) = schema("memory_nodes_hidden_as_forgotten").await else {
        return;
    };
    let db = &schema.db;
    let (_, node, _) = summed_up(db).await;
    db.execute_unprepared(TAKE_BACK).await.unwrap();
    let hidden = super::hidden::going_of(db, OTHER, &talk()).await.unwrap();
    assert!(!hidden.is_empty());
    forget::taken_back_here(db, OTHER, &talk()).await.unwrap();
    let read = read::node(db, OTHER, &talk(), &node)
        .await
        .unwrap()
        .unwrap();
    assert!(!read.to_string().contains("小王"), "{read}");
    assert!(stubs(db).await.is_empty(), "only hidden");
    confirmed(db).await;
    assert_eq!(stubs(db).await, hidden);
    schema.drop().await;
}

/// Only hidden, it shows again once the memory is back (brought back, or
/// one not here yet come), as soon as her nodes are swept.
#[tokio::test]
async fn what_was_only_hidden_shows_again_when_the_memory_is_back() {
    let Some(schema) = schema("memory_nodes_hidden_back").await else {
        return;
    };
    let db = &schema.db;
    let (_, node, _) = summed_up(db).await;
    db.execute_unprepared(TAKE_BACK).await.unwrap();
    forget::taken_back_here(db, OTHER, &talk()).await.unwrap();
    let read = read::node(db, OTHER, &talk(), &node)
        .await
        .unwrap()
        .unwrap();
    assert!(!read.to_string().contains("小王"), "{read}");
    db.execute_unprepared(
        "UPDATE agent_memories SET invalid_at = NULL, invalid_reason = NULL \
         WHERE content = '同事小王在追她'",
    )
    .await
    .unwrap();
    forget::taken_back_here(db, OTHER, &talk()).await.unwrap();
    let read = read::node(db, OTHER, &talk(), &node)
        .await
        .unwrap()
        .unwrap();
    assert!(read.to_string().contains("小王"), "{read}");
    assert!(stubs(db).await.is_empty());
    schema.drop().await;
}

/// What waits in her inbox resting on a memory taken back is not filed,
/// and does not hold back what waits after it.
#[tokio::test]
async fn an_inbox_item_hidden_holds_back_none_after_it() {
    let Some(schema) = schema("memory_nodes_hidden_inbox").await else {
        return;
    };
    let db = &schema.db;
    db.execute_unprepared(
        "INSERT INTO agent_memories (id, user_id, kind, content, source, venue, valid_from, \
         created_at, updated_at, invalid_at, invalid_reason) VALUES ('gone', 7, 'fact', \
         '说过的', 'chat', 'private', now(), now(), now(), now(), 'superseded')",
    )
    .await
    .unwrap();
    for (at, refs) in [
        (1, r#"["memory:gone"]"#),
        (2, r#"["memory:gone"]"#),
        (3, r#"["memory:gone"]"#),
        (4, r#"["memory:gone"]"#),
        (5, "[]"),
    ] {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_node_inbox (user_id, audience, day, id, item, created_at) \
             VALUES ($1, $2, CURRENT_DATE, $3, \
               jsonb_build_object('saidBy', '[\"partner\"]'::jsonb, 'text', $4::text, \
                                  'refs', $5::jsonb), \
               now() + $6 * interval '1 second')",
            [
                OTHER.into(),
                talk().into(),
                format!("i{at}").into(),
                format!("说过的 {at}").into(),
                refs.into(),
                at.into(),
            ],
        ))
        .await
        .unwrap();
    }
    assert_eq!(
        count(
            db,
            "SELECT count(*) AS n FROM agent_node_inbox WHERE placed_at IS NULL"
        )
        .await,
        5
    );
    let waiting = super::hidden::inbox_waiting(db, OTHER, &talk(), 2)
        .await
        .unwrap();
    let ids: Vec<&str> = waiting.iter().map(|item| item.id.as_str()).collect();
    assert_eq!(ids, vec!["i5"]);
    schema.drop().await;
}
