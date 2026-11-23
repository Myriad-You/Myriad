//! What they asked her not to keep stays on the site, and what another end
//! kept as that is never a fact here (a database of its own; skipped without
//! `MYRIAD_MEDIA_TEST_DATABASE_URL`).

use myriad_persona_log::row::Author;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use super::super::keys::test_root::TestRoot;
use super::super::{state, store};
use super::{ASKED_SOURCE, SYNC_SOURCE, TransactionTrait, take_in};

const OWNER: i32 = 41;
const PARTNER: &str = "partner_019b76daa80000112233445566778899";

async fn sources(db: &DatabaseConnection) -> Vec<(String, String)> {
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT id, source FROM agent_memories ORDER BY id",
    ))
    .await
    .unwrap()
    .iter()
    .map(|row| {
        (
            row.try_get("", "id").unwrap(),
            row.try_get("", "source").unwrap(),
        )
    })
    .collect()
}

#[tokio::test]
async fn what_they_asked_not_to_keep_stays_here_and_comes_as_itself() {
    let Some(url) = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"]) else {
        return;
    };
    let schema = crate::db::IsolatedSchema::migrated(&url, "her_log_asked").await;
    let db = &schema.db;
    db.execute_unprepared(&format!(
        "INSERT INTO users (id, username) VALUES ({OWNER}, 'owner')"
    ))
    .await
    .unwrap();
    store::start(db, OWNER).await.unwrap();
    super::super::test_partner::give(db, OWNER, PARTNER).await;
    let root = TestRoot([9; 32]);
    for (id, content, source) in [
        ("m1", "喜欢茶", "chat"),
        ("m2", "不要记和房东吵架的事", ASKED_SOURCE),
    ] {
        db.execute_unprepared(&format!(
            "INSERT INTO agent_memories (id, user_id, kind, content, source, venue, audience, \
             concepts, valid_from, created_at, updated_at) VALUES ('{id}', {OWNER}, 'preference', \
             '{content}', '{source}', 'private', '[{OWNER}]'::jsonb, '[]'::jsonb, now(), now(), now())"
        ))
        .await
        .unwrap();
    }
    let owner = super::super::owner(db, OWNER).await.unwrap();
    let personal = owner.personal();
    for _ in 0..2 {
        state::sync(db, &root, OWNER).await.unwrap();
        let objects: Vec<String> = store::rows(db, OWNER, &personal)
            .await
            .unwrap()
            .into_iter()
            .filter(|row| row.object.starts_with("memory:"))
            .map(|row| row.object)
            .collect();
        assert_eq!(objects, ["memory:m1"]);
    }
    // Written by another end that names it so, with nothing of it here.
    db.execute_unprepared("DELETE FROM agent_memories WHERE id = 'm2'")
        .await
        .unwrap();
    let txn = db.begin().await.unwrap();
    for (object, source) in [("memory:m3", ASKED_SOURCE), ("memory:m4", "elsewhere")] {
        store::write_row_on(
            &txn,
            &root,
            &owner,
            &personal,
            None,
            object,
            "memory.write",
            &json!({"at": 1, "kind": "preference", "text": "不要记体重", "source": source}),
            (Author::Owner, None),
        )
        .await
        .unwrap();
    }
    txn.commit().await.unwrap();
    // As from there.
    db.execute_unprepared(
        "UPDATE agent_log_rows SET origin = 'dev_mac', \
         envelope = jsonb_set(envelope, '{origin}', '\"dev_mac\"') WHERE object IN ('memory:m3', 'memory:m4')",
    )
    .await
    .unwrap();
    take_in(db, &root, &owner).await.unwrap();
    let kept = sources(db).await;
    for (id, source) in [("m3", ASKED_SOURCE), ("m4", SYNC_SOURCE)] {
        assert!(
            kept.contains(&(id.to_string(), source.to_string())),
            "{kept:?}"
        );
    }
    // Heard by no one here either, and still the owner's: comparing takes
    // nothing of it back where it came from.
    let heard_by: serde_json::Value = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT audience FROM agent_memories WHERE id = 'm3'",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "audience")
        .unwrap();
    assert_eq!(heard_by, json!([0]));
    state::sync(db, &root, OWNER).await.unwrap();
    let kinds: Vec<String> = store::rows(db, OWNER, &personal)
        .await
        .unwrap()
        .into_iter()
        .filter(|row| row.object == "memory:m3")
        .map(|row| row.kind)
        .collect();
    assert_eq!(kinds, ["memory.write"]);
    schema.drop().await;
}
