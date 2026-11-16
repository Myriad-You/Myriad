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
mod outbound;
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
        assert!(src.contains("record("));
        assert!(src.contains("is_unique_violation"));
    }
}

#[cfg(test)]
mod list_smoke_tests {
    use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement};

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

/// 真实 `send_message` 路径的 E2E fail-closed 行为（每个测试独立 schema，见 `test_db`）。
#[cfg(test)]
mod send_e2e_tests {
    use axum::http::StatusCode;
    use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

    use super::SendMessageRequest;
    use crate::federation::test_db::SchemaDb;

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
        let Some(fixture) = SchemaDb::new().await else {
            return;
        };
        let db = &fixture.db;
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
            let (user_id, username, channel_id) = active_channel(db, properties.clone()).await;
            let (status, body) =
                super::send_message(user_id, &username, &channel_id, db, &request(true))
                    .await
                    .expect_err("encrypt=true must fail closed");
            assert_eq!(status, StatusCode::BAD_REQUEST, "{properties:?}");
            assert_eq!(body.0["code"], "e2e_required", "{properties:?}");
            assert_eq!(
                side_effects(db, user_id, &channel_id).await,
                (0, 0),
                "no message / Activity may be written: {properties:?}"
            );
        }
        fixture.close().await;
    }

    #[tokio::test]
    async fn encrypt_true_with_session_stores_only_ciphertext() {
        let Some(fixture) = SchemaDb::new().await else {
            return;
        };
        let db = &fixture.db;
        let local = crate::federation::e2e::generate_keypair();
        let peer = crate::federation::e2e::generate_keypair();
        let (user_id, username, channel_id) = active_channel(
            db,
            Some(serde_json::json!({"e2e": {
                "local_public_key": local.public_key,
                "local_private_key": local.private_key,
                "remote_public_key": peer.public_key,
            }})),
        )
        .await;
        let resp = super::send_message(user_id, &username, &channel_id, db, &request(true))
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
        assert_eq!(side_effects(db, user_id, &channel_id).await, (1, 1));
        fixture.close().await;
    }

    #[tokio::test]
    async fn encrypt_false_stores_plaintext_without_session() {
        let Some(fixture) = SchemaDb::new().await else {
            return;
        };
        let db = &fixture.db;
        let (user_id, username, channel_id) = active_channel(db, None).await;
        let resp = super::send_message(user_id, &username, &channel_id, db, &request(false))
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
        fixture.close().await;
    }
}

/// Two users of one instance: a channel has a row for each and one history.
#[cfg(test)]
mod same_instance_tests {
    use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
    use serde_json::json;

    use super::{
        CreateChannelRequest, SendMessageRequest, accept_channel, close_channel, create_channel,
        delete_channel, get_messages, handle_channel_open, initiate_e2e_key_exchange, send_message,
    };
    use crate::federation::test_db::SchemaDb;
    use crate::federation::types::{actor_url, get_base_url};

    async fn user(db: &DatabaseConnection, username: &str) -> i32 {
        db.query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO users (username) VALUES ($1) RETURNING id",
            [username.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "id")
        .unwrap()
    }

    async fn count(db: &DatabaseConnection, sql: &str, channel_id: &str) -> i64 {
        db.query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            [channel_id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap()
    }

    async fn status(db: &DatabaseConnection, channel_id: &str, user_id: i32) -> Option<String> {
        db.query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT status FROM federation_channels WHERE channel_id = $1 AND user_id = $2",
            [channel_id.into(), user_id.into()],
        ))
        .await
        .unwrap()
        .map(|row| row.try_get("", "status").unwrap())
    }

    const MESSAGES: &str =
        "SELECT COUNT(*) AS n FROM federation_channel_messages WHERE channel_id = $1";

    /// Alice and bob with a channel bob has accepted.
    async fn accepted(db: &DatabaseConnection) -> (i32, i32, String) {
        let alice = user(db, "alice").await;
        let bob = user(db, "bob").await;
        let base = get_base_url().await;
        let request: CreateChannelRequest =
            serde_json::from_value(json!({ "remote_actor": actor_url(&base, "bob") })).unwrap();
        let channel = create_channel(alice, "alice", db, &request).await.unwrap();
        let channel_id = channel.channel_id;
        assert_eq!(
            status(db, &channel_id, bob).await.as_deref(),
            Some("pending")
        );
        accept_channel(bob, "bob", &channel_id, db).await.unwrap();
        assert_eq!(
            status(db, &channel_id, alice).await.as_deref(),
            Some("accepted")
        );
        assert_eq!(
            status(db, &channel_id, bob).await.as_deref(),
            Some("accepted")
        );
        (alice, bob, channel_id)
    }

    async fn say(db: &DatabaseConnection, user_id: i32, name: &str, channel_id: &str, text: &str) {
        let request: SendMessageRequest =
            serde_json::from_value(json!({ "payload": { "text": text } })).unwrap();
        send_message(user_id, name, channel_id, db, &request)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn both_ends_talk_and_each_keeps_the_history_until_both_delete() {
        let Some(fixture) = SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        let (alice, bob, channel_id) = accepted(db).await;

        say(db, alice, "alice", &channel_id, "hello").await;
        say(db, bob, "bob", &channel_id, "hi").await;
        assert_eq!(
            count(db, MESSAGES, &channel_id).await,
            2,
            "one row a message"
        );
        for (user_id, name) in [(alice, "alice"), (bob, "bob")] {
            let seen = get_messages(user_id, &channel_id, db, None, None)
                .await
                .unwrap();
            assert_eq!(seen.len(), 2, "{name} sees the whole talk");
        }

        close_channel(alice, "alice", &channel_id, db)
            .await
            .unwrap();
        assert_eq!(
            status(db, &channel_id, bob).await.as_deref(),
            Some("closed")
        );

        delete_channel(alice, &channel_id, db).await.unwrap();
        assert_eq!(status(db, &channel_id, alice).await, None);
        assert_eq!(
            count(db, MESSAGES, &channel_id).await,
            2,
            "bob still has it"
        );

        delete_channel(bob, &channel_id, db).await.unwrap();
        assert_eq!(count(db, MESSAGES, &channel_id).await, 0);
        fixture.close().await;
    }

    #[tokio::test]
    async fn a_deleted_account_leaves_the_other_end_its_history() {
        let Some(fixture) = SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        db.execute_unprepared(include_str!("../../../migrations/user_lifecycle.sql"))
            .await
            .unwrap();
        let (alice, bob, channel_id) = accepted(db).await;
        say(db, alice, "alice", &channel_id, "hello").await;

        let gone = |id: i32| format!("DELETE FROM users WHERE id = {id}");
        db.execute_unprepared(&gone(alice)).await.unwrap();
        assert_eq!(count(db, MESSAGES, &channel_id).await, 1);
        db.execute_unprepared(&gone(bob)).await.unwrap();
        assert_eq!(count(db, MESSAGES, &channel_id).await, 0);
        fixture.close().await;
    }

    #[tokio::test]
    async fn encrypted_messages_open_with_each_ends_own_keys() {
        let Some(fixture) = SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        let (alice, bob, channel_id) = accepted(db).await;
        initiate_e2e_key_exchange(alice, "alice", &channel_id, db)
            .await
            .unwrap();
        initiate_e2e_key_exchange(bob, "bob", &channel_id, db)
            .await
            .unwrap();

        let request: SendMessageRequest = serde_json::from_value(json!({
            "payload": { "text": "secret" },
            "encrypt": true,
        }))
        .unwrap();
        send_message(alice, "alice", &channel_id, db, &request)
            .await
            .unwrap();

        for user_id in [alice, bob] {
            let seen = get_messages(user_id, &channel_id, db, None, None)
                .await
                .unwrap();
            let message = seen
                .iter()
                .find(|message| message.message_type == "text")
                .expect("the message");
            assert_eq!(message.payload["text"], "secret");
        }
        fixture.close().await;
    }

    /// A database from before: one unique `channel_id`, messages keyed to it.
    #[tokio::test]
    async fn an_older_database_gives_up_its_constraints_and_keeps_its_rows() {
        let Some(fixture) = SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        let (alice, _, channel_id) = accepted(db).await;
        say(db, alice, "alice", &channel_id, "hello").await;
        db.execute_unprepared(&format!(
            "DELETE FROM federation_channels WHERE channel_id = '{channel_id}' AND user_id <> {alice};
             DROP INDEX idx_channels_channel_user;
             ALTER TABLE federation_channels
                 ADD CONSTRAINT federation_channels_channel_id_key UNIQUE (channel_id);
             ALTER TABLE federation_channel_messages
                 ADD CONSTRAINT fk_fed_channel_messages_channel FOREIGN KEY (channel_id)
                 REFERENCES federation_channels (channel_id) ON DELETE CASCADE;"
        ))
        .await
        .unwrap();

        for _ in 0..2 {
            db.execute_unprepared(migration::CHANNEL_ROWS_SQL)
                .await
                .unwrap();
        }

        let left = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT COUNT(*) AS n FROM pg_constraint \
                 WHERE conname IN ('federation_channels_channel_id_key', \
                                   'fk_fed_channel_messages_channel') \
                   AND connamespace = current_schema()::regnamespace",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<i64>("", "n")
            .unwrap();
        assert_eq!(left, 0);
        assert_eq!(count(db, MESSAGES, &channel_id).await, 1, "history stays");
        assert_eq!(
            status(db, &channel_id, alice).await.as_deref(),
            Some("active")
        );
        fixture.close().await;
    }

    /// Messages are shared by `channel_id`: a row for anyone but the other end
    /// of the talk would read it.
    #[tokio::test]
    async fn a_channel_id_cannot_be_opened_to_a_third_party() {
        let Some(fixture) = SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        let (_, _, channel_id) = accepted(db).await;
        let carol = user(db, "carol").await;
        let evil = "https://evil.example/users/mallory";
        db.execute_unprepared(&format!(
            "INSERT INTO federation_remote_actors (actor_url, domain, inbox_url) \
             VALUES ('{evil}', 'evil.example', '{evil}/inbox')"
        ))
        .await
        .unwrap();
        let base = get_base_url().await;
        let open = json!({
            "type": "myriad:ChannelOpen",
            "actor": evil,
            "to": [actor_url(&base, "carol")],
            "object": { "id": channel_id },
        });

        let refused = handle_channel_open(db, evil, &open).await.unwrap_err();
        assert!(refused.contains("not the remote party"), "{refused}");
        assert_eq!(status(db, &channel_id, carol).await, None);
        fixture.close().await;
    }

    #[test]
    fn the_other_end_is_the_owner_writing_to_its_peer() {
        let base = "https://site.example";
        let alice = "https://site.example/users/alice";
        let bob = "https://site.example/users/bob";
        assert!(super::inbox::is_other_end(base, "alice", bob, alice, "bob"));
        assert!(!super::inbox::is_other_end(base, "alice", bob, bob, "bob"));
        assert!(!super::inbox::is_other_end(
            base, "alice", bob, alice, "carol"
        ));
    }
}
