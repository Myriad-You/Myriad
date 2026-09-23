//! Federation DM channels: CRUD, accept, and E2E key exchange.
//!
//! - `types`: request/response structs
//! - `crud`: local Channel CRUD and send/get messages
//! - `inbox`: inbound ChannelOpen / Message / Close
//! - `e2e`: JWT-sealed session, key exchange, accept
//!
//! Activities that arrive before the channel row exist fail closed so the peer
//! retries. Receipt + message_id idempotency is the durable path; there is no
//! in-memory early-activity buffer.

mod crud;
mod e2e;
mod inbox;
mod types;

pub use crud::*;
pub use e2e::*;
pub use inbox::*;
pub use types::*;

#[cfg(test)]
mod split_contract_tests {
    #[test]
    fn early_activities_retry_without_volatile_buffer() {
        let inbox = include_str!("inbox.rs");
        let e2e = include_str!("e2e.rs");
        assert!(inbox.contains("retry after ChannelOpen"));
        assert!(e2e.contains("retry after ChannelOpen"));
        assert!(!inbox.contains("buffer_early"));
        assert!(!e2e.contains("buffer_early"));
        assert!(!inbox.contains("flush_early_channel_messages"));
    }

    #[test]
    fn crud_owns_create_not_inbox() {
        let src = include_str!("crud.rs");
        assert!(src.contains("fn create_channel"));
        assert!(!src.contains("fn handle_channel_open"));
    }

    #[test]
    fn inbox_owns_channel_open() {
        assert!(include_str!("inbox.rs").contains("fn handle_channel_open"));
    }

    #[test]
    fn e2e_owns_key_exchange() {
        assert!(include_str!("e2e.rs").contains("fn handle_key_exchange"));
    }

    #[test]
    fn active_relationship_unique_is_partial() {
        let sql = super::ACTIVE_CHANNEL_RELATIONSHIP_UNIQUE_SQL;
        assert!(sql.contains("UNIQUE INDEX"));
        assert!(sql.contains("idx_channels_active_relationship"));
        assert!(sql.contains("user_id"));
        assert!(sql.contains("remote_actor_id"));
        assert!(sql.contains("channel_type"));
        assert!(sql.contains("pending"));
        assert!(sql.contains("accepted"));
        assert!(sql.contains("active"));
        assert!(sql.contains("WHERE status IN"));
    }

    #[test]
    fn create_channel_commits_with_outbound_intent() {
        let src = include_str!("crud.rs");
        assert!(src.contains("db.begin()"));
        assert!(src.contains("insert_local_activity"));
        assert!(src.contains("enqueue_delivery"));
        assert!(src.contains("is_unique_violation"));
    }
}

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
        let db = Database::connect(&database_url)
            .await
            .expect("connect test db");
        migration::Migrator::up(&db, None)
            .await
            .expect("migrator up");

        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO users (username) VALUES ($1) RETURNING id",
                [format!("channel-smoke-{}", uuid::Uuid::new_v4().simple()).into()],
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
