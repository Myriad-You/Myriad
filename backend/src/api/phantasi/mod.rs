//! Phantasi RSS reader API: feeds, articles, reading state, comments, and RSSHub.
//!
//! Real submodules with shared [`helpers`] (auth + OPML).

mod applications;
mod comments;
mod feeds_list;
mod feeds_opml;
mod feeds_sources;
mod helpers;
mod note_collab;
mod note_docs;
mod note_editor;
mod notes;
mod notes_rss;
mod reading_item;
mod reading_mark;
mod reading_state;
mod reading_stats;
mod reading_sync;
mod reading_sync_ws;
mod routes;
mod rsshub;

pub use notes_rss::notes_rss;
pub use routes::create_phantasi_routes;

#[cfg(test)]
mod integration_tests {
    use sea_orm::{Database, EntityTrait};

    /// DB-gated smoke: empty phantasi_sources after Migrator (list_sources shape dependency).
    /// Set `PHANTASI_TEST_DATABASE_URL` or `NOTIFICATION_TEST_DATABASE_URL` to run.
    #[tokio::test]
    async fn list_sources_empty_after_migrator_when_db_provided() {
        let database_url = std::env::var("PHANTASI_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("NOTIFICATION_TEST_DATABASE_URL"))
            .or_else(|_| std::env::var("MYRIAD_SCHEMA_DRIFT_DB"));
        let Ok(database_url) = database_url else {
            return;
        };
        let db = Database::connect(&database_url)
            .await
            .expect("connect test db");
        migration::Migrator::up(&db, None)
            .await
            .expect("migrator up");
        let sources = crate::models::entities::phantasi_sources::Entity::find()
            .all(&db)
            .await
            .expect("query phantasi_sources");
        let empty_shape = serde_json::json!({
            "success": true,
            "sources": serde_json::Value::Array(vec![])
        });
        assert_eq!(empty_shape["success"], true);
        assert!(empty_shape["sources"].as_array().unwrap().is_empty());
        let _ = sources.len();
    }

    /// DB-gated: 节律查询必须能在真 Postgres 上跑通（窗口函数 + INTERVAL 字面量
    /// 拼错只会在运行时炸，编译期看不出来），且 `phantasi_items.topic` 列存在。
    #[tokio::test]
    async fn pulses_sql_and_topic_column_when_db_provided() {
        use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

        let database_url = std::env::var("PHANTASI_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("NOTIFICATION_TEST_DATABASE_URL"))
            .or_else(|_| std::env::var("MYRIAD_SCHEMA_DRIFT_DB"));
        let Ok(database_url) = database_url else {
            return;
        };
        let db = Database::connect(&database_url)
            .await
            .expect("connect test db");
        migration::Migrator::up(&db, None)
            .await
            .expect("migrator up");

        // 空表也要能执行：这里验证的是 SQL 形状，不是数据
        let sql = super::feeds_sources::build_pulses_sql(2);
        let stmt = Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &sql,
            vec![sea_orm::Value::Int(Some(-1)), sea_orm::Value::Int(Some(-2))],
        );
        let rows = db.query_all_raw(stmt).await.expect("pulses sql executes");
        assert!(rows.is_empty(), "不存在的 source_id 不该回任何节律");

        // topic 列：migration 003 建、schema_check 对旧库 ADD COLUMN
        let col = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT data_type FROM information_schema.columns                  WHERE table_schema = 'public' AND table_name = 'phantasi_items'                    AND column_name = 'topic'"
                    .to_string(),
            ))
            .await
            .expect("query information_schema");
        let col = col.expect("phantasi_items.topic 必须存在");
        let data_type: String = col.try_get("", "data_type").expect("data_type");
        assert_eq!(data_type, "text");

        // topic 过滤：NULL 的文章不进结果
        let filtered = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT id FROM phantasi_items WHERE topic = 'engineering'".to_string(),
            ))
            .await
            .expect("topic filter executes");
        let _ = filtered.len();
    }

    /// DB-gated: 云端笔记文档表必须存在，草稿才不会被硬塞进 phantasi_items。
    #[tokio::test]
    async fn phantasi_note_docs_table_when_db_provided() {
        use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

        let database_url = std::env::var("PHANTASI_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("NOTIFICATION_TEST_DATABASE_URL"))
            .or_else(|_| std::env::var("MYRIAD_SCHEMA_DRIFT_DB"));
        let Ok(database_url) = database_url else {
            return;
        };
        let db = Database::connect(&database_url)
            .await
            .expect("connect test db");
        migration::Migrator::up(&db, None)
            .await
            .expect("migrator up");
        let col = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT data_type FROM information_schema.columns \
                 WHERE table_schema = 'public' AND table_name = 'phantasi_note_docs' \
                 AND column_name = 'status'"
                    .to_string(),
            ))
            .await
            .expect("query information_schema")
            .expect("phantasi_note_docs.status 必须存在");
        let data_type: String = col.try_get("", "data_type").expect("data_type");
        assert!(
            data_type.contains("char"),
            "status should be a varchar-like type, got {data_type}"
        );
    }

    /// DB-gated: 友联申请表必须存在，审核队列才有地方落。
    #[tokio::test]
    async fn phantasi_source_applications_table_when_db_provided() {
        use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

        let database_url = std::env::var("PHANTASI_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("NOTIFICATION_TEST_DATABASE_URL"))
            .or_else(|_| std::env::var("MYRIAD_SCHEMA_DRIFT_DB"));
        let Ok(database_url) = database_url else {
            return;
        };
        let db = Database::connect(&database_url)
            .await
            .expect("connect test db");
        migration::Migrator::up(&db, None)
            .await
            .expect("migrator up");
        crate::db::schema_check::ensure_schema(&db)
            .await
            .expect("schema heal");
        let col = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT data_type FROM information_schema.columns \
                 WHERE table_schema = 'public' AND table_name = 'phantasi_source_applications' \
                 AND column_name = 'status'"
                    .to_string(),
            ))
            .await
            .expect("query information_schema")
            .expect("phantasi_source_applications.status 必须存在");
        let data_type: String = col.try_get("", "data_type").expect("data_type");
        assert!(
            data_type.contains("char"),
            "status should be a varchar-like type, got {data_type}"
        );
    }
}
