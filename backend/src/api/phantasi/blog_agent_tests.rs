//! Real PostgreSQL regression entering through the Agent capability dispatcher.
use super::note_collab::note_collab_hub;
use crate::services::agent::{
    capability::CapabilityRegistry,
    executor::handlers::{HandlerContext, execute_capability},
};
use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement};
use serde_json::{Value, json};
use std::collections::HashMap;

async fn call(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    id: &str,
    params: Value,
) -> Result<Value, String> {
    let registry = CapabilityRegistry::new();
    let capability = registry.get(id).unwrap();
    let context = HandlerContext {
        db,
        user_id,
        ai_analyzer: None,
        task_id: None,
        step_id: None,
        execution_context: None,
        autonomy_permission_cap: None,
    };
    let params: HashMap<String, Value> = serde_json::from_value(params).unwrap();
    execute_capability(id, "", &capability.category, &params, &context).await
}

#[tokio::test]
async fn blog_agent_edits_use_editor_versions_history_and_collaboration() {
    let Ok(url) = std::env::var("BLOG_AGENT_TEST_DATABASE_URL") else {
        eprintln!("skip: BLOG_AGENT_TEST_DATABASE_URL is unset");
        return;
    };
    // Temporary tables and a single connection isolate all writes from durable data.
    let mut options = sea_orm::ConnectOptions::new(url);
    options
        .max_connections(1)
        .min_connections(1)
        .sqlx_logging(false);
    let db = Database::connect(options).await.unwrap();
    crate::services::note_publish::create_note_fixture_tables(&db, true).await;
    db.execute_unprepared("CREATE TEMP TABLE users (id INTEGER PRIMARY KEY, username TEXT, display_name TEXT, is_admin BOOLEAN, is_owner BOOLEAN, token_version INTEGER);
        INSERT INTO users VALUES (882001, 'Writer', NULL, true, true, 0), (882002, 'Reader', NULL, false, false, 0);
        CREATE TEMP TABLE phantasi_note_authors (doc_id INTEGER, user_id INTEGER, role TEXT, created_at TIMESTAMPTZ DEFAULT now(), PRIMARY KEY(doc_id, user_id));
        INSERT INTO phantasi_sources (id, user_id, name, url, feed_type, source_type, update_interval, error_count, enabled, item_count, admin_only, created_at, updated_at)
            VALUES (1, 882001, 'Notes', 'myriad:notes', 'rss', 'note', 0, 0, true, 1, false, now(), now());
        INSERT INTO phantasi_items (id, source_id, guid, title, link, content, content_md, published_at, fetched_at, fulltext_fetched, content_revision, slug)
            VALUES (9001, 1, 'agent-test', 'Live title', '/blog/posts/live/', '<p>Live body</p>', 'Live body', now(), now(), true, 1, 'live');
        INSERT INTO phantasi_note_docs (id, user_id, item_id, title, content_md, topic, status, revision, created_at, updated_at, last_edited_by)
            VALUES (41, 882001, 9001, 'Live title', 'Original **Markdown**', 'Writing', 'published', 1, now(), now(), 882001);").await.unwrap();
    let history_sql = include_str!("../../../migrations/note_editor.sql")
        .replace(
            "CREATE TABLE IF NOT EXISTS phantasi_note_history",
            "CREATE TEMP TABLE IF NOT EXISTS phantasi_note_history",
        )
        .replace(
            "FUNCTION phantasi_capture_note_history()",
            "FUNCTION pg_temp.phantasi_capture_note_history()",
        );
    db.execute_unprepared(&history_sql).await.unwrap();

    let list = call(
        &db,
        882001,
        "blog.notes",
        json!({"query":"writing", "status":"published", "limit":1}),
    )
    .await
    .unwrap();
    assert_eq!(list["total"], 1);
    assert_eq!(list["docs"][0]["id"], 41);
    assert_eq!(list["docs"][0]["content_md"], "");
    let read = call(&db, 882001, "blog.note", json!({"docId":41}))
        .await
        .unwrap();
    assert_eq!(read["doc"]["content_md"], "Original **Markdown**");
    assert!(
        call(&db, 882001, "blog.note", json!({"docId":9001}))
            .await
            .is_err(),
        "item id is not a document id"
    );
    let mut events = note_collab_hub().subscribe(41);
    let saved = call(
        &db,
        882001,
        "blog.note.update",
        json!({"docId":41, "revision":1, "content_md":"Agent **revision**", "topic":null}),
    )
    .await
    .unwrap();
    assert_eq!(saved["doc"]["revision"], 2);
    assert_eq!(saved["doc"]["title"], "Live title");
    assert_eq!(saved["doc"]["topic"], Value::Null);
    assert_eq!(saved["doc"]["status"], "published");
    // A save returns its receipt, not the Markdown the Agent just sent.
    assert!(saved["doc"].get("content_md").is_none());
    assert!(saved["doc"].get("authors").is_none());
    let event = tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event.content_md.as_deref(), Some("Agent **revision**"));
    assert_eq!(event.revision, Some(2));
    let history = db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres,
        "SELECT snapshot->>'content_md' AS body FROM phantasi_note_history WHERE doc_id=41 AND revision=1")).await.unwrap().unwrap();
    assert_eq!(
        history.try_get::<String>("", "body").unwrap(),
        "Original **Markdown**"
    );
    let live = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT content FROM phantasi_items WHERE id=9001",
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        live.try_get::<String>("", "content").unwrap(),
        "<p>Live body</p>"
    );
    assert!(
        call(
            &db,
            882001,
            "blog.note.update",
            json!({"docId":41, "revision":1, "content_md":"stale"})
        )
        .await
        .unwrap_err()
        .contains("updated elsewhere")
    );
    assert!(
        call(
            &db,
            882001,
            "blog.note.update",
            json!({"docId":41, "content_md":"missing revision"})
        )
        .await
        .is_err()
    );
    for id in ["blog.notes", "blog.note", "blog.note.update"] {
        assert!(
            call(
                &db,
                882002,
                id,
                json!({"docId":41,"revision":2,"title":"denied"})
            )
            .await
            .unwrap_err()
            .contains("administrator")
        );
    }
    let read = call(&db, 882001, "blog.note", json!({"docId":41}))
        .await
        .unwrap();
    assert_eq!(read["doc"]["content_md"], "Agent **revision**");
    assert_eq!(read["doc"]["revision"], 2);

    // Edits change only what they name, against the revision they were written for.
    let edited = call(
        &db,
        882001,
        "blog.note.update",
        json!({"docId":41, "revision":2, "edits":[{"find":"**revision**","replace":"*edit*"}]}),
    )
    .await
    .unwrap();
    assert_eq!(edited["doc"]["revision"], 3);
    let read = call(&db, 882001, "blog.note", json!({"docId":41}))
        .await
        .unwrap();
    assert_eq!(read["doc"]["content_md"], "Agent *edit*");
    for (params, error) in [
        (
            json!({"docId":41, "revision":2, "edits":[{"find":"Agent","replace":"x"}]}),
            "now revision 3",
        ),
        (
            json!({"docId":41, "revision":3, "edits":[{"find":"absent","replace":"x"}]}),
            "not in the note",
        ),
        (
            json!({"docId":41, "revision":3, "content_md":"x", "edits":[{"find":"Agent","replace":"x"}]}),
            "not both",
        ),
    ] {
        assert!(
            call(&db, 882001, "blog.note.update", params)
                .await
                .unwrap_err()
                .contains(error)
        );
    }
    db.close().await.unwrap();
}
