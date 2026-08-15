//! Federation DM channels: early activity buffer, CRUD, accept, and E2E key exchange.
//!
//! Single implementation module [`buffer_types_crud`] (formerly include!-split with accept_e2e).

mod buffer_types_crud;

pub use buffer_types_crud::*;

#[cfg(test)]
mod list_smoke_tests {
    use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    #[tokio::test]
    async fn list_channels_empty_for_new_user_when_db_provided() {
        let database_url = std::env::var("CHANNEL_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("NOTIFICATION_TEST_DATABASE_URL"))
            .or_else(|_| std::env::var("MYRIAD_SCHEMA_DRIFT_DB"));
        let Ok(database_url) = database_url else {
            return;
        };
        let db = Database::connect(&database_url).await.expect("connect test db");
        migration::Migrator::up(&db, None)
            .await
            .expect("migrator up");

        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO users (username) VALUES ($1) RETURNING id",
                [format!(
                    "channel-smoke-{}",
                    uuid::Uuid::new_v4().simple()
                )
                .into()],
            ))
            .await
            .expect("insert user")
            .expect("row");
        let user_id: i32 = row.try_get("", "id").expect("id");

        let channels = super::list_channels(user_id, "channel-smoke", &db)
            .await
            .expect("list_channels");
        assert!(channels.is_empty(), "new user must have zero channels");
    }
}
