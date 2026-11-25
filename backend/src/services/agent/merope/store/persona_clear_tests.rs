//! What goes when her persona is cleared, and what stays, on a database of
//! their own (skipped without `MYRIAD_MEROPE_TEST_DATABASE_URL`).

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::json;

use crate::services::agent::merope::{clear_persona_on, clear_sealed_on, on_mind, undertone};
use crate::services::runtime_registry::{self, RegistryIdentity};

const OWNER: i32 = 7;
const OTHER: i32 = 8;

async fn fixture(name: &str) -> Option<crate::db::IsolatedSchema> {
    let url = crate::db::test_url(&["MYRIAD_MEROPE_TEST_DATABASE_URL"])?;
    let schema = crate::db::IsolatedSchema::migrated(&url, name).await;
    let db = &schema.db;
    db.execute_unprepared(&format!(
        "INSERT INTO users (id, username, is_owner) VALUES ({OWNER}, 'owner', true), \
         ({OTHER}, 'other', false); \
         INSERT INTO agent_persona (id, name, updated_at) VALUES ('site', 'Merope', NOW());"
    ))
    .await
    .unwrap();
    for (id, user, source, venue) in [
        ("sync_private", Some(OWNER), "sync", "private"),
        ("sync_own", None, "sync", "own"),
        ("screen", Some(OWNER), "screen", "private"),
        ("said_owner", Some(OWNER), "self_said", "private"),
        ("said_other", Some(OTHER), "self_said", "private"),
        ("chat_other", Some(OTHER), "chat", "private"),
        ("meme", None, "meme", "own"),
        ("work", Some(OWNER), "work", "private"),
        ("work_tried", Some(OWNER), "work_tried", "private"),
        ("asked", Some(OWNER), "asked", "private"),
        ("import", Some(OWNER), "import", "private"),
    ] {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_memories (id, user_id, kind, content, source, venue, audience, \
             concepts, valid_from, created_at, updated_at) VALUES ($1, $2, 'fact', $1, $3, $4, \
             '[]'::jsonb, '[]'::jsonb, NOW(), NOW(), NOW())",
            [id.into(), user.into(), source.into(), venue.into()],
        ))
        .await
        .unwrap();
    }
    for (id, user) in [("int_owner", OWNER), ("int_other", OTHER)] {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_intentions (id, user_id, source_event_id, summary, reason_code, \
             status, proposal, accept_source, created_at, updated_at) VALUES ($1, $2, $1, 'water \
             the fern', 'due', 'proposed', '{}'::jsonb, 'user', NOW(), NOW())",
            [id.into(), user.into()],
        ))
        .await
        .unwrap();
    }
    // Talk: another's own, theirs in a group, the owner's.
    for (session, user, context) in [
        ("s_other", OTHER, r#"{"mode":"chat"}"#),
        ("s_group", OTHER, r#"{"mode":"chat","venue":"qq:5"}"#),
        ("s_owner", OWNER, r#"{"mode":"chat"}"#),
    ] {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_sessions (id, user_id, context, message_count, archived, \
             created_at, last_active_at) VALUES ($1, $2, $3::json, 2, false, NOW(), NOW())",
            [session.into(), user.into(), context.into()],
        ))
        .await
        .unwrap();
        for role in ["user", "assistant"] {
            db.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO agent_messages (session_id, role, content, created_at) \
                 VALUES ($1, $2, 'the fern looks dry', NOW())",
                [session.into(), role.into()],
            ))
            .await
            .unwrap();
        }
    }
    let mind =
        on_mind::OnMind::new("whether the fern lives", None, "talk", crate::clock::now()).unwrap();
    on_mind::keep(db, &mind).await;
    // Another record of the site's own, not hers.
    for namespace in [undertone::NAMESPACE, "not_hers"] {
        runtime_registry::put(
            db,
            namespace,
            "undertone",
            RegistryIdentity {
                subject_id: None,
                owner_id: None,
                tapp_id: None,
                runtime_id: None,
            },
            &json!({ "day": "2026-11-24" }),
            (crate::clock::now() + chrono::Duration::days(1)).timestamp(),
        )
        .await
        .unwrap();
    }
    Some(schema)
}

async fn ids(db: &DatabaseConnection, sql: &str) -> Vec<String> {
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        sql.to_string(),
    ))
    .await
    .unwrap()
    .iter()
    .map(|row| row.try_get("", "id").unwrap())
    .collect()
}

async fn memories(db: &DatabaseConnection) -> Vec<String> {
    ids(db, "SELECT id FROM agent_memories ORDER BY id").await
}

async fn intentions(db: &DatabaseConnection) -> Vec<String> {
    ids(db, "SELECT id FROM agent_intentions ORDER BY id").await
}

async fn talk(db: &DatabaseConnection) -> Vec<String> {
    ids(
        db,
        "SELECT session_id || ' ' || role AS id FROM agent_messages ORDER BY session_id, role",
    )
    .await
}

async fn records(db: &DatabaseConnection) -> Vec<String> {
    ids(
        db,
        "SELECT namespace AS id FROM runtime_registry \
         WHERE namespace IN ('merope_on_mind', 'merope_undertone', 'not_hers') ORDER BY namespace",
    )
    .await
}

/// Cleared, or what was left of her let go: what another end kept, what a
/// look at their screen told her, what she said of herself, what stays on
/// her mind, her undertone and what she meant to do for the owner go with
/// her. Work's lessons, what was carried over from before, what they asked
/// her not to keep, another's suggestions and everyone's talk stay.
#[tokio::test]
async fn what_was_hers_and_the_owners_goes_with_her() {
    for (name, sealed) in [("persona_clear", false), ("persona_clear_sealed", true)] {
        let Some(schema) = fixture(name).await else {
            return;
        };
        let db = &schema.db;
        assert_eq!(memories(db).await.len(), 11);
        assert_eq!(
            records(db).await,
            ["merope_on_mind", "merope_undertone", "not_hers"]
        );
        let txn = db.begin().await.unwrap();
        if sealed {
            clear_sealed_on(&txn).await.unwrap();
        } else {
            clear_persona_on(&txn).await.unwrap();
        }
        txn.commit().await.unwrap();
        assert_eq!(
            memories(db).await,
            ["asked", "import", "work", "work_tried"]
        );
        assert_eq!(intentions(db).await, ["int_other"]);
        assert_eq!(records(db).await, ["not_hers"]);
        assert_eq!(
            talk(db).await,
            [
                "s_group assistant",
                "s_group user",
                "s_other assistant",
                "s_other user",
                "s_owner assistant",
                "s_owner user"
            ]
        );
        // Nothing left to go: the same again.
        clear_persona_on(db).await.unwrap();
        assert_eq!(intentions(db).await, ["int_other"]);
        schema.drop().await;
    }
}

/// Clearing her that does not go through leaves her whole.
#[tokio::test]
async fn clearing_her_undone_leaves_all_of_her() {
    let Some(schema) = fixture("persona_clear_undone").await else {
        return;
    };
    let db = &schema.db;
    let txn = db.begin().await.unwrap();
    clear_persona_on(&txn).await.unwrap();
    txn.rollback().await.unwrap();
    assert_eq!(memories(db).await.len(), 11);
    assert_eq!(intentions(db).await, ["int_other", "int_owner"]);
    assert_eq!(
        records(db).await,
        ["merope_on_mind", "merope_undertone", "not_hers"]
    );
    schema.drop().await;
}
