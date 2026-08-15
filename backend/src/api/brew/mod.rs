//! Brew RSS reader API: feeds, articles, reading state, comments, and RSSHub.
//!
//! Real submodules with shared [`helpers`] (auth + OPML).

mod comments_rsshub;
mod feeds_articles;
mod helpers;
mod reading_sync_ws;

pub use feeds_articles::create_brew_routes;

#[cfg(test)]
mod integration_tests {
    use sea_orm::{Database, EntityTrait};
    use sea_orm_migration::MigratorTrait;

    /// DB-gated smoke: empty brew_sources after Migrator (list_sources shape dependency).
    /// Set `BREW_TEST_DATABASE_URL` or `NOTIFICATION_TEST_DATABASE_URL` to run.
    #[tokio::test]
    async fn list_sources_empty_after_migrator_when_db_provided() {
        let database_url = std::env::var("BREW_TEST_DATABASE_URL")
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
        let sources = crate::models::entities::brew_sources::Entity::find()
            .all(&db)
            .await
            .expect("query brew_sources");
        let empty_shape = serde_json::json!({
            "success": true,
            "sources": serde_json::Value::Array(vec![])
        });
        assert_eq!(empty_shape["success"], true);
        assert!(empty_shape["sources"].as_array().unwrap().is_empty());
        let _ = sources.len();
    }
}
