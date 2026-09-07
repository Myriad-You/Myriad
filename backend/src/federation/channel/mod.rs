//! Federation DM channels: early activity buffer, CRUD, accept, and E2E key exchange.
//!
//! - `buffer`: ChannelMessage / KeyExchange that arrive before the channel row
//! - `types`: request/response structs
//! - `crud`: local Channel CRUD and send/get messages
//! - `inbox`: inbound ChannelOpen / Message / Close and early-activity flush
//! - `e2e`: JWT-sealed session, key exchange, accept

mod buffer;
mod crud;
mod e2e;
mod inbox;
mod types;

pub(crate) use buffer::{buffer_early_channel_activity, EARLY_MSG_MAX_PER_CHANNEL};
pub use crud::*;
pub use e2e::*;
pub use inbox::*;
pub use types::*;

#[cfg(test)]
mod split_contract_tests {
    #[test]
    fn buffer_owns_early_activity_not_crud() {
        let src = include_str!("buffer.rs");
        assert!(src.contains("buffer_early_channel_activity"));
        assert!(!src.contains("fn create_channel"));
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
