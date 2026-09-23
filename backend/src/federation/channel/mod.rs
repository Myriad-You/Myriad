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

/// 真实 `send_message` 路径的 E2E fail-closed 行为（需要测试库，未配置时跳过）。
#[cfg(test)]
mod send_e2e_tests {
    use axum::http::StatusCode;
    use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};
    use sea_orm_migration::MigratorTrait;

    use super::SendMessageRequest;

    async fn test_db() -> Option<DatabaseConnection> {
        let database_url = std::env::var("CHANNEL_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("NOTIFICATION_TEST_DATABASE_URL"))
            .or_else(|_| std::env::var("MYRIAD_SCHEMA_DRIFT_DB"))
            .ok()?;
        let db = Database::connect(&database_url)
            .await
            .expect("connect test db");
        migration::Migrator::up(&db, None)
            .await
            .expect("migrator up");
        Some(db)
    }

    /// 新建本地用户 + 远端 actor + active channel，返回 (user_id, username, channel_id)。
    async fn active_channel(
        db: &DatabaseConnection,
        properties: Option<serde_json::Value>,
    ) -> (i32, String, String) {
        let tag = uuid::Uuid::new_v4().simple().to_string();
        let username = format!("chsend-{tag}");
        let user_id: i32 = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO users (username) VALUES ($1) RETURNING id",
                [username.clone().into()],
            ))
            .await
            .expect("insert user")
            .expect("row")
            .try_get("", "id")
            .expect("id");
        let actor = format!("https://peer.example/users/{tag}");
        let remote_id: i32 = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"INSERT INTO federation_remote_actors (actor_url, domain, inbox_url)
                   VALUES ($1, 'peer.example', $2) RETURNING id"#,
                [actor.clone().into(), format!("{actor}/inbox").into()],
            ))
            .await
            .expect("insert remote actor")
            .expect("row")
            .try_get("", "id")
            .expect("id");
        let channel_id = format!("ch-{tag}");
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_channels
               (channel_id, user_id, remote_actor_id, channel_type, status, initiated_by,
                properties, created_at)
               VALUES ($1, $2, $3, 'text', 'active', 'local', $4, NOW())"#,
            [
                channel_id.clone().into(),
                user_id.into(),
                remote_id.into(),
                properties.into(),
            ],
        ))
        .await
        .expect("insert channel");
        (user_id, username, channel_id)
    }

    /// 本次发送留下的消息与出站 Activity 数量。
    async fn side_effects(db: &DatabaseConnection, user_id: i32, channel_id: &str) -> (i64, i64) {
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT
                     (SELECT COUNT(*) FROM federation_channel_messages WHERE channel_id = $1)
                       AS messages,
                     (SELECT COUNT(*) FROM federation_activities WHERE user_id = $2)
                       AS activities"#,
                [channel_id.into(), user_id.into()],
            ))
            .await
            .expect("count")
            .expect("row");
        (
            row.try_get("", "messages").expect("messages"),
            row.try_get("", "activities").expect("activities"),
        )
    }

    fn request(encrypt: bool) -> SendMessageRequest {
        SendMessageRequest {
            message_type: None,
            payload: serde_json::json!({"text": "secret plaintext"}),
            reply_to: None,
            encrypt: Some(encrypt),
        }
    }

    #[tokio::test]
    async fn encrypt_true_without_usable_session_writes_nothing() {
        let Some(db) = test_db().await else {
            return;
        };
        let local = crate::federation::e2e::generate_keypair();
        let cases = [
            // 缺 session：从未做过 key-exchange
            None,
            // 缺 key：e2e 状态缺本地私钥
            Some(serde_json::json!({"e2e": {"local_public_key": local.public_key}})),
            // 未建立：没有对端公钥
            Some(serde_json::json!({"e2e": {
                "local_public_key": local.public_key,
                "local_private_key": local.private_key,
            }})),
            // 对端公钥非法：加密前即失败
            Some(serde_json::json!({"e2e": {
                "local_public_key": local.public_key,
                "local_private_key": local.private_key,
                "remote_public_key": "not-a-key",
            }})),
        ];
        for properties in cases {
            let (user_id, username, channel_id) = active_channel(&db, properties.clone()).await;
            let (status, body) =
                super::send_message(user_id, &username, &channel_id, &db, &request(true))
                    .await
                    .expect_err("encrypt=true must fail closed");
            assert_eq!(status, StatusCode::BAD_REQUEST, "{properties:?}");
            assert_eq!(body.0["code"], "e2e_required", "{properties:?}");
            assert_eq!(
                side_effects(&db, user_id, &channel_id).await,
                (0, 0),
                "no message / Activity may be written: {properties:?}"
            );
        }
    }

    #[tokio::test]
    async fn encrypt_true_with_session_stores_only_ciphertext() {
        let Some(db) = test_db().await else {
            return;
        };
        let local = crate::federation::e2e::generate_keypair();
        let peer = crate::federation::e2e::generate_keypair();
        let (user_id, username, channel_id) = active_channel(
            &db,
            Some(serde_json::json!({"e2e": {
                "local_public_key": local.public_key,
                "local_private_key": local.private_key,
                "remote_public_key": peer.public_key,
            }})),
        )
        .await;
        let resp = super::send_message(user_id, &username, &channel_id, &db, &request(true))
            .await
            .expect("encrypted send");
        assert!(resp.is_encrypted);
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT payload::text AS payload, is_encrypted
                   FROM federation_channel_messages WHERE channel_id = $1"#,
                [channel_id.clone().into()],
            ))
            .await
            .expect("select")
            .expect("row");
        let stored: String = row.try_get("", "payload").expect("payload");
        assert!(row.try_get::<bool>("", "is_encrypted").expect("flag"));
        assert!(!stored.contains("secret plaintext"), "plaintext must not be stored");
        assert_eq!(side_effects(&db, user_id, &channel_id).await, (1, 1));
    }

    #[tokio::test]
    async fn encrypt_false_stores_plaintext_without_session() {
        let Some(db) = test_db().await else {
            return;
        };
        let (user_id, username, channel_id) = active_channel(&db, None).await;
        let resp = super::send_message(user_id, &username, &channel_id, &db, &request(false))
            .await
            .expect("plaintext send");
        assert!(!resp.is_encrypted);
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT payload, is_encrypted
                   FROM federation_channel_messages WHERE channel_id = $1"#,
                [channel_id.clone().into()],
            ))
            .await
            .expect("select")
            .expect("row");
        let stored: serde_json::Value = row.try_get("", "payload").expect("payload");
        assert_eq!(stored, serde_json::json!({"text": "secret plaintext"}));
        assert!(!row.try_get::<bool>("", "is_encrypted").expect("flag"));
    }
}
