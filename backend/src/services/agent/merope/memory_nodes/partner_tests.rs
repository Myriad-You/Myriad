//! The site owner's talk moving to the partner id ([`super::partner`]).

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use super::audience;
use super::partner;
use super::{Author, RowIn, write};
use crate::services::her_log::{pen, test_partner::PARTNER};

const OWNER: i32 = 1;

async fn schema(prefix: &str) -> Option<crate::db::IsolatedSchema> {
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").ok()?;
    let schema = crate::db::IsolatedSchema::migrated(&url, prefix).await;
    schema
        .db
        .execute_unprepared("INSERT INTO users (id, username) VALUES (1, 'one')")
        .await
        .unwrap();
    Some(schema)
}

/// A topic she starts in `audience` with one fact on it, as her rows.
fn topic(audience: &str, memory: &str, text: &str) -> (String, Vec<RowIn>) {
    named(audience, "houseplants", memory, text)
}

/// [`topic`], named `name`.
fn named(audience: &str, name: &str, memory: &str, text: &str) -> (String, Vec<RowIn>) {
    let made = myriad_nodes::entries::fact_rows(
        serde_json::from_value(json!({
            "audience": audience,
            "start": {"name": name, "aliases": ["the green ones"]},
            "facts": [{"memory": memory, "text": text, "saidBy": ["partner"]}],
        }))
        .unwrap(),
    )
    .unwrap();
    let rows = made
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
    (made.node, rows)
}

/// Every row of `audience`: its id, dseq, envelope audience and body.
async fn rows(db: &DatabaseConnection, audience: &str) -> Vec<(String, i64, String, String)> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT row_id, dseq, envelope, body FROM agent_node_rows \
         WHERE user_id = $1 AND audience = $2 ORDER BY dseq",
        [OWNER.into(), audience.into()],
    ))
    .await
    .unwrap()
    .into_iter()
    .map(|row| {
        let envelope: Value = row.try_get("", "envelope").unwrap();
        (
            row.try_get("", "row_id").unwrap(),
            row.try_get("", "dseq").unwrap(),
            envelope["audience"].as_str().unwrap().to_string(),
            row.try_get("", "body").unwrap(),
        )
    })
    .collect()
}

async fn heads(db: &DatabaseConnection, audience: &str) -> Vec<String> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT node_id FROM agent_node_heads WHERE user_id = $1 AND audience = $2",
        [OWNER.into(), audience.into()],
    ))
    .await
    .unwrap()
    .into_iter()
    .map(|row| row.try_get("", "node_id").unwrap())
    .collect()
}

/// Before the account has its partner id her talk stays under the old
/// name; once it has, a batch still sent under the old name moves the whole
/// talk first (envelopes, bodies, heads, the same node and row ids and
/// counts) and is written under the new one, and moving again moves nothing.
#[tokio::test]
async fn the_site_owners_talk_moves_to_the_partner_id_once_there_is_one() {
    let Some(schema) = schema("memory_nodes_partner").await else {
        return;
    };
    let db = &schema.db;
    partner::remember(None);
    let old = partner::legacy_talk();
    let new = format!("personal:{PARTNER}");
    assert_eq!(audience::owners_talk(), old);
    let thought = pen::think().expect("her mind is here in tests");
    let (node, first) = topic(&old, "m1", "has a snake plant");
    write::write(db, OWNER, &old, &first, Some(&thought))
        .await
        .unwrap();
    let before = rows(db, &old).await;
    assert_eq!(before.len(), 3);

    crate::services::her_log::test_partner::give(db, OWNER, PARTNER).await;
    let (same, second) = topic(&old, "m2", "repots in spring");
    assert_eq!(same, node, "the node keeps its id under either name");
    let written = write::write(db, OWNER, &old, &second, Some(&thought))
        .await
        .unwrap();
    assert_eq!(written.rows.len(), 1, "only the new fact is new");
    assert!(rows(db, &old).await.is_empty());
    assert!(heads(db, &old).await.is_empty());
    assert_eq!(heads(db, &new).await, vec![node.clone()]);
    let after = rows(db, &new).await;
    assert_eq!(after.len(), 4);
    for (was, now) in before.iter().zip(&after) {
        assert_eq!(
            (&was.0, was.1),
            (&now.0, now.1),
            "the same row on its place"
        );
        assert_eq!(now.2, new);
        assert!(!now.3.contains("local-owner"), "{}", now.3);
    }
    assert_eq!(after[3].1, 4, "the count goes on without a hole");
    assert!(
        after[3].3.contains(&new),
        "the new fact is said under the new name"
    );
    assert_eq!(partner::moved_to().as_deref(), Some(PARTNER));
    assert_eq!(audience::owners_talk(), new);
    assert_eq!(
        audience::person_key(&new, OWNER),
        format!("person:{PARTNER}")
    );
    assert_eq!(
        myriad_nodes::node_id(&new, &format!("person:{PARTNER}")).unwrap(),
        myriad_nodes::node_id(&old, "person:local-owner").unwrap(),
        "「关于你」 stays the same node"
    );

    // Moving again moves nothing.
    assert_eq!(partner::move_talk(db, OWNER, PARTNER).await.unwrap(), 0);
    assert_eq!(rows(db, &new).await, after);
    schema.drop().await;
}

/// A talk that cannot move (a row under the new name already stands where
/// one under the old name does) stays under the old name, and what she
/// writes there is still kept.
#[tokio::test]
async fn a_talk_that_cannot_move_keeps_being_written_under_the_old_name() {
    let Some(schema) = schema("memory_nodes_partner_clash").await else {
        return;
    };
    let db = &schema.db;
    let old = partner::legacy_talk();
    let new = format!("personal:{PARTNER}");
    let thought = pen::think().expect("her mind is here in tests");
    partner::remember(None);
    let (_, first) = topic(&old, "m1", "has a snake plant");
    write::write(db, OWNER, &old, &first, Some(&thought))
        .await
        .unwrap();
    // Something already under the new name, on the same places.
    let (_, theirs) = named(&new, "bees", "m9", "keeps bees");
    write::write(db, OWNER, &new, &theirs, Some(&thought))
        .await
        .unwrap();

    crate::services::her_log::test_partner::give(db, OWNER, PARTNER).await;
    let (_, second) = topic(&old, "m2", "repots in spring");
    let written = write::write(db, OWNER, &old, &second, Some(&thought))
        .await
        .unwrap();
    assert_eq!(written.rows.len(), 1);
    assert_eq!(rows(db, &old).await.len(), 4, "still under the old name");
    assert_eq!(rows(db, &new).await.len(), 3, "nothing half moved");
    assert_eq!(partner::moved_to(), None);
    schema.drop().await;
}
