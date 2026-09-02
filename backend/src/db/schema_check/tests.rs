//! schema_check unit tests
use super::expected_indexes::get_expected_indexes;
use super::expected_schema::get_expected_schema;
use super::introspect::*;
use super::orchestrator::*;
use super::seeds::*;
use super::types::*;

use super::*;

#[test]
fn test_expected_schema_tables() {
    let tables = get_expected_schema();
    assert!(!tables.is_empty());

    // 验证关键表存在
    let table_names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
    assert!(table_names.contains(&"users"));
    assert!(table_names.contains(&"configurations"));
    assert!(table_names.contains(&"platforms"));
}

/// 近一个月新功能：须在 get_expected_schema / indexes 有完整条目
/// （数字系列 001/004/005 已 CREATE；本列表只校验 schema_check 侧期望）。
#[test]
fn test_recent_month_features_in_expected_schema() {
    let tables = get_expected_schema();
    let names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
    for required in [
        // 001 SITE ANALYTICS
        "analytics_page_daily",
        "analytics_visitor_seen",
        "analytics_event_daily",
        "analytics_event_visitor",
        "analytics_referrer_daily",
        "analytics_country_daily",
        "analytics_country_visitor",
        // 004
        "heartbeat_claims",
        "agent_intentions",
        "agent_autonomy_grants",
        // 005 扩展
        "federation_content_filters",
        "federation_policy_settings",
        "federation_domain_aliases",
        "federation_object_interactions",
        // 005（原 012/013）
        "federation_inbox_receipts",
    ] {
        assert!(
            names.contains(&required),
            "missing recent-feature table in get_expected_schema: {required}"
        );
    }

    let page = tables
        .iter()
        .find(|t| t.name == "analytics_page_daily")
        .expect("analytics_page_daily");
    for col in [
        "day",
        "path",
        "views",
        "unique_visitors",
        "engagement_ms",
        "engaged_views",
    ] {
        assert!(
            page.columns.iter().any(|c| c.name == col),
            "analytics_page_daily missing column {col}"
        );
    }

    let policy = tables
        .iter()
        .find(|t| t.name == "federation_policy_settings")
        .expect("federation_policy_settings");
    for col in [
        "min_trust_level",
        "allowed_domains",
        "auto_discover",
        "rate_max_requests",
        "rate_window_seconds",
        "rate_trusted_multiplier",
    ] {
        assert!(
            policy.columns.iter().any(|c| c.name == col),
            "federation_policy_settings missing column {col}"
        );
    }

    let instances = tables
        .iter()
        .find(|t| t.name == "federation_instances")
        .expect("federation_instances");
    assert!(
        instances.columns.iter().any(|c| c.name == "failing_since"),
        "federation_instances.failing_since must be in expected schema (005 + generic ADD)"
    );
    let delivery = tables
        .iter()
        .find(|t| t.name == "federation_delivery_queue")
        .expect("federation_delivery_queue");
    for col in ["lease_token", "lease_expires_at"] {
        assert!(
            delivery.columns.iter().any(|c| c.name == col),
            "federation_delivery_queue missing column {col}"
        );
    }

    let indexes = get_expected_indexes();
    let idx_names: Vec<&str> = indexes.iter().map(|i| i.name.as_str()).collect();
    for required in [
        "idx_analytics_page_daily_day",
        "idx_analytics_country_daily_day",
        "idx_heartbeat_claims_claimed_at",
        "idx_agent_intentions_user_status",
        "idx_agent_intentions_user_updated",
        "idx_agent_intentions_source_event",
        "idx_agent_intentions_user_source_event",
        "idx_federation_domain_aliases_new",
        "idx_fed_interactions_object_kind",
        "idx_fed_interactions_user_kind_created",
        "federation_inbox_receipts_pkey",
        "idx_delivery_lease_expiry",
        "idx_delivery_queue_target_domain",
    ] {
        assert!(
            idx_names.contains(&required),
            "missing recent-feature index in get_expected_indexes: {required}"
        );
    }
}

#[test]
fn test_users_schema_includes_tapp_install_disabled() {
    let tables = get_expected_schema();
    let users = tables
        .iter()
        .find(|t| t.name == "users")
        .expect("users table");
    assert!(
        users
            .columns
            .iter()
            .any(|c| c.name == "tapp_install_disabled"),
        "users must define tapp_install_disabled"
    );
}

#[test]
fn test_users_schema_includes_is_owner() {
    let tables = get_expected_schema();
    let users = tables
        .iter()
        .find(|t| t.name == "users")
        .expect("users table");
    assert!(
        users.columns.iter().any(|c| c.name == "is_owner"),
        "users must define is_owner (schema_check / base 001)"
    );
}

#[test]
fn test_users_schema_includes_token_version() {
    let tables = get_expected_schema();
    let users = tables
        .iter()
        .find(|t| t.name == "users")
        .expect("users table");
    let col = users
        .columns
        .iter()
        .find(|c| c.name == "token_version")
        .expect("users must define token_version (MYR-005 session epoch)");
    assert!(!col.is_nullable);
    assert_eq!(col.default_value.as_deref(), Some("0"));
}

#[test]
fn test_default_platform_seeds_include_x_and_core() {
    let seeds = default_platform_seeds();
    let names: Vec<&str> = seeds.iter().map(|s| s.name).collect();
    for required in [
        "github",
        "bilibili",
        "steam",
        "youtube",
        "netease_music",
        "bangumi",
        "x",
        "discord",
        "mal",
    ] {
        assert!(
            names.contains(&required),
            "missing default platform seed: {}",
            required
        );
    }
    // name 唯一
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len());
}

#[test]
fn test_default_config_seeds_include_quota_and_explicit_open_permissions() {
    let seeds = default_config_seeds();
    let values: std::collections::HashMap<_, _> = seeds.into_iter().collect();
    assert_eq!(values["user_ai_daily_calls"], serde_json::json!(50));
    assert_eq!(values["guest_ai_daily_tokens"], serde_json::json!(5000));
    assert_eq!(values["user_perm_storage_write"], serde_json::json!(false));
    assert_eq!(values["guest_perm_storage_write"], serde_json::json!(false));
    // federation 拆分后三个 Elevated 写域进入可配置下放集合，默认关闭
    assert_eq!(
        values["user_perm_federation_post"],
        serde_json::json!(false)
    );
    assert_eq!(
        values["user_perm_federation_channel"],
        serde_json::json!(false)
    );
    assert_eq!(
        values["user_perm_federation_room"],
        serde_json::json!(false)
    );
    assert_eq!(
        values["guest_perm_federation_post"],
        serde_json::json!(false)
    );
    assert_eq!(
        values["guest_perm_federation_channel"],
        serde_json::json!(false)
    );
    assert_eq!(
        values["guest_perm_federation_room"],
        serde_json::json!(false)
    );
    assert_eq!(
        values["user_perm_brew_comment_write"],
        serde_json::json!(false)
    );
    assert_eq!(
        values["guest_perm_brew_comment_write"],
        serde_json::json!(false)
    );
    assert!(!values.contains_key("user_perm_component_theme"));
    assert!(!values.contains_key("user_perm_shortcut_register"));
    assert_eq!(values["stash_hidden_capacity"], serde_json::json!(8));
    assert_eq!(values["stash_hidden_idle_seconds"], serde_json::json!(300));
    assert_eq!(values["resident_quota_per_app"], serde_json::json!(1));
    assert_eq!(values["resident_quota_site_total"], serde_json::json!(3));
}

#[tokio::test]
async fn runtime_config_seed_preserves_existing_values_and_is_idempotent() {
    let Ok(url) = std::env::var("MYRIAD_CONFIG_SEED_DB") else {
        eprintln!("skipping: set MYRIAD_CONFIG_SEED_DB to run the runtime config seed test");
        return;
    };

    use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement};
    let db = Database::connect(&url)
        .await
        .expect("connect to config seed database");
    db.execute_unprepared(
        r#"
        CREATE TABLE IF NOT EXISTS configurations (
            id SERIAL PRIMARY KEY,
            key VARCHAR(255) NOT NULL UNIQUE,
            value JSONB NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        TRUNCATE configurations;
        INSERT INTO configurations (key, value) VALUES
            ('user_perm_component_theme', 'false'::jsonb),
            ('user_ai_daily_calls', '999'::jsonb);
        "#,
    )
    .await
    .expect("prepare config seed database");

    let inserted = ensure_default_config(&db)
        .await
        .expect("seed missing runtime config rows");
    assert!(inserted > 0);

    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT key, value FROM configurations".to_string(),
        ))
        .await
        .expect("read seeded config rows");
    let values: std::collections::HashMap<String, serde_json::Value> = rows
        .into_iter()
        .map(|row| {
            (
                row.try_get::<String>("", "key").expect("config key"),
                row.try_get::<serde_json::Value>("", "value")
                    .expect("config value"),
            )
        })
        .collect();
    assert_eq!(
        values["user_perm_component_theme"],
        serde_json::json!(false)
    );
    assert_eq!(values["user_ai_daily_calls"], serde_json::json!(999));
    assert_eq!(
        values["user_perm_shortcut_register"],
        serde_json::json!(true)
    );
    assert_eq!(values["stash_hidden_capacity"], serde_json::json!(8));
    assert_eq!(values["resident_quota_site_total"], serde_json::json!(3));

    let second = ensure_default_config(&db)
        .await
        .expect("repeat config seed");
    assert_eq!(second, 0, "runtime config seed must be idempotent");
}

#[test]
fn test_tapps_schema_includes_approved_permissions() {
    let tables = get_expected_schema();
    let tapps = tables
        .iter()
        .find(|t| t.name == "tapps")
        .expect("tapps table");
    let col = tapps
        .columns
        .iter()
        .find(|c| c.name == "approved_permissions")
        .expect("tapps.approved_permissions must be in expected schema (002 + generic ADD)");
    assert_eq!(col.data_type, "jsonb");
    assert!(!col.is_nullable);
    assert_eq!(col.default_value.as_deref(), Some("'[]'"));
}

#[test]
fn test_tapps_schema_includes_needs_reauthorization_marker() {
    // Durable re-authorization marker, non-null, default false.
    let tables = get_expected_schema();
    let tapps = tables
        .iter()
        .find(|t| t.name == "tapps")
        .expect("tapps table");
    let col = tapps
        .columns
        .iter()
        .find(|c| c.name == "needs_reauthorization")
        .expect("tapps.needs_reauthorization must be in expected schema (002 + 016 + generic ADD)");
    assert_eq!(col.data_type, "boolean");
    assert!(!col.is_nullable);
    assert_eq!(col.default_value.as_deref(), Some("false"));
}

#[test]
fn test_tapp_storage_schema_includes_credential_fields() {
    let tables = get_expected_schema();
    let storage = tables
        .iter()
        .find(|table| table.name == "tapp_storage")
        .expect("tapp_storage table");
    for (name, data_type) in [
        ("encrypted_value", "text"),
        ("binding_fingerprint", "character varying"),
    ] {
        let column = storage
            .columns
            .iter()
            .find(|column| column.name == name)
            .unwrap_or_else(|| panic!("tapp_storage.{name} must be field-healed"));
        assert_eq!(column.data_type, data_type);
        assert!(column.is_nullable);
    }
}

#[test]
fn test_generate_add_column_ddl() {
    let col = ColumnDef {
        name: "test_col".into(),
        data_type: "VARCHAR(255)".into(),
        is_nullable: true,
        default_value: Some("'default'".into()),
    };

    let ddl = generate_add_column_ddl("users", &col);
    assert!(ddl.contains("ALTER TABLE users"));
    assert!(ddl.contains("ADD COLUMN IF NOT EXISTS"));
    assert!(ddl.contains("test_col"));
    assert!(ddl.contains("DEFAULT 'default'"));
}

#[test]
fn test_generate_create_index_ddl() {
    let idx = IndexDef {
        name: "idx_test".into(),
        table: "users".into(),
        columns: vec!["col1".into(), "col2".into()],
        is_unique: true,
    };

    let ddl = generate_create_index_ddl(&idx);
    assert!(ddl.contains("CREATE UNIQUE INDEX IF NOT EXISTS"));
    assert!(ddl.contains("idx_test"));
    assert!(ddl.contains("col1, col2"));
}

/// **CI 漂移闸门。**
///
/// 在一个刚跑完 `Migrator::up` 的全新数据库上，`report_schema_drift` 必须返回空。
/// 一旦不为空，就说明 `migrations/` 里的建表语句与本文件的权威结构列表
/// （49 个 TableDef / 554 个 ColumnDef）已经不一致 —— 也就是审计指出的
/// "5228 行 runtime healer 与 migration 重复定义并已发生漂移"。
///
/// 需要真实 PostgreSQL。没有 `MYRIAD_SCHEMA_DRIFT_DB` 时静默跳过，
/// 这样本地 `cargo test` 不受影响；CI 里由 postgres service 提供该变量。
#[tokio::test]
async fn migrations_leave_no_schema_drift() {
    let Ok(url) = std::env::var("MYRIAD_SCHEMA_DRIFT_DB") else {
        eprintln!("skipping: set MYRIAD_SCHEMA_DRIFT_DB to run the schema drift gate");
        return;
    };

    use sea_orm_migration::MigratorTrait;
    let db = sea_orm::Database::connect(&url)
        .await
        .expect("connect to the drift-check database");

    crate::db::Migrator::up(&db, None)
        .await
        .expect("migrations must apply cleanly to an empty database");

    let drift = report_schema_drift(&db)
        .await
        .expect("drift report must succeed");

    assert!(
        drift.is_empty(),
        "migrations and the schema_check expectation list disagree on {} item(s).\n\
         Either the migration is missing this structure, or schema_check declares \n\
         something the migrations never create:\n{}",
        drift.len(),
        drift.summary()
    );

    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, TransactionTrait};
    let invalid = db
        .execute_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
INSERT INTO tapp_storage
    (user_id, tapp_id, key, value, encrypted_value, created_at, updated_at)
VALUES
    (2147483647, 'schema.constraint.test', 'ordinary', '{}'::jsonb, 'ciphertext', NOW(), NOW())
"#
            .to_string(),
        ))
        .await;
    assert!(
        invalid.is_err(),
        "tapp_storage must reject encrypted payloads outside _credentials.*"
    );

    // A review deployment may already have recorded the short-lived first
    // 012 migration while retaining its scope-less table. 012/013 names are
    // purged from seaql_migrations before up; schema_check rebuilds that shape.
    db.execute_unprepared(
        r#"
DROP TABLE federation_inbox_receipts;
CREATE TABLE federation_inbox_receipts (
    id BIGSERIAL PRIMARY KEY,
    signer TEXT NOT NULL,
    activity_id TEXT NOT NULL,
    body_digest CHAR(64) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'processing',
    attempts INTEGER NOT NULL DEFAULT 1,
    lease_until TIMESTAMPTZ,
    outcome_status SMALLINT,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    accepted_at TIMESTAMPTZ,
    CONSTRAINT federation_inbox_receipts_identity_unique UNIQUE (signer, activity_id)
);
"#,
    )
    .await
    .expect("create the legacy receipt shape");

    ensure_schema(&db)
        .await
        .expect("schema heal must upgrade a database that recorded old 012");
    let upgraded_drift = report_schema_drift(&db)
        .await
        .expect("upgraded receipt schema drift report must succeed");
    assert!(
        upgraded_drift.is_empty(),
        "legacy receipt upgrade must restore the authoritative schema:\n{}",
        upgraded_drift.summary()
    );

    let legacy_column_count = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"SELECT COUNT(*)::BIGINT AS count
               FROM information_schema.columns
               WHERE table_schema = current_schema()
                 AND table_name = 'federation_inbox_receipts'
                 AND column_name IN ('id', 'attempts', 'lease_until', 'updated_at', 'accepted_at')"#
                .to_string(),
        ))
        .await
        .expect("inspect upgraded receipt columns")
        .expect("column count row");
    assert_eq!(
        legacy_column_count
            .try_get::<i64>("", "count")
            .expect("read legacy receipt column count"),
        0,
        "healer must remove every column unique to the scope-less receipt shape"
    );

    // Permanent handler rejection must preserve the claimed receipt while
    // removing every DB effect performed after the handler savepoint.
    let txn = db.begin().await.expect("begin receipt savepoint probe");
    txn.execute_unprepared(
        "CREATE TEMP TABLE receipt_savepoint_probe (value INTEGER) ON COMMIT DROP",
    )
    .await
    .expect("create receipt savepoint probe table");
    crate::federation::inbox::begin_receipt_handler_effects(&txn)
        .await
        .expect("create handler savepoint");
    txn.execute_unprepared("INSERT INTO receipt_savepoint_probe (value) VALUES (1)")
        .await
        .expect("write simulated handler side effect");
    crate::federation::inbox::rollback_receipt_handler_effects(&txn)
        .await
        .expect("rollback simulated rejected-handler effects");
    let remaining = txn
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*)::BIGINT AS count FROM receipt_savepoint_probe".to_string(),
        ))
        .await
        .expect("query receipt savepoint probe")
        .expect("receipt savepoint count row");
    assert_eq!(
        remaining
            .try_get::<i64>("", "count")
            .expect("read receipt savepoint count"),
        0,
        "permanent rejection must not commit handler writes"
    );
    txn.rollback()
        .await
        .expect("rollback receipt savepoint probe");
}

/// Real PostgreSQL regression coverage for the room outbox commit boundary and
/// per-delivery lease ownership. The CI schema-drift service supplies the DB;
/// temp tables shadow production names and the outer transaction rolls back.
#[tokio::test]
async fn federation_outbox_and_delivery_lease_db_contracts() {
    let Ok(url) = std::env::var("MYRIAD_SCHEMA_DRIFT_DB") else {
        eprintln!("skipping: set MYRIAD_SCHEMA_DRIFT_DB to run federation DB contracts");
        return;
    };

    use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement, TransactionTrait};
    use uuid::Uuid;

    let db = Database::connect(&url)
        .await
        .expect("connect to federation contract database");
    let outer = db
        .begin()
        .await
        .expect("begin federation contract transaction");
    outer
        .execute_unprepared(
            r#"
CREATE TEMP TABLE federation_activities (
    id SERIAL PRIMARY KEY,
    activity_id TEXT NOT NULL UNIQUE,
    user_id INTEGER,
    activity_type VARCHAR(64) NOT NULL,
    object_type VARCHAR(64),
    object_json JSON NOT NULL,
    is_local BOOLEAN NOT NULL DEFAULT TRUE,
    published_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
) ON COMMIT DROP;
CREATE TEMP TABLE users (
    id INTEGER PRIMARY KEY,
    username TEXT NOT NULL
) ON COMMIT DROP;
CREATE TEMP TABLE federation_room_messages (
    message_id TEXT PRIMARY KEY,
    room_id TEXT
) ON COMMIT DROP;
CREATE TEMP TABLE federation_rooms (
    room_id TEXT PRIMARY KEY,
    owner_actor TEXT NOT NULL,
    home_server TEXT NOT NULL
) ON COMMIT DROP;
CREATE TEMP TABLE federation_room_members (
    room_id TEXT NOT NULL,
    actor_url TEXT NOT NULL,
    is_local BOOLEAN NOT NULL,
    membership_status TEXT
) ON COMMIT DROP;
CREATE TEMP TABLE federation_remote_actors (
    id SERIAL PRIMARY KEY,
    actor_url TEXT NOT NULL UNIQUE,
    inbox_url TEXT NOT NULL,
    domain TEXT NOT NULL
) ON COMMIT DROP;
CREATE TEMP TABLE federation_instances (
    domain TEXT PRIMARY KEY,
    failure_count INTEGER NOT NULL DEFAULT 0,
    failing_since TIMESTAMPTZ,
    last_success_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
) ON COMMIT DROP;
CREATE TEMP TABLE federation_follows (
    id SERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL,
    remote_actor_id INTEGER NOT NULL,
    direction TEXT NOT NULL,
    status TEXT NOT NULL,
    accepted_at TIMESTAMPTZ
) ON COMMIT DROP;
CREATE TEMP TABLE federation_channels (
    id SERIAL PRIMARY KEY,
    channel_id TEXT NOT NULL UNIQUE,
    remote_actor_id INTEGER NOT NULL,
    status TEXT NOT NULL,
    closed_at TIMESTAMPTZ
) ON COMMIT DROP;
CREATE TEMP TABLE federation_channel_messages (
    channel_id TEXT NOT NULL,
    message_id TEXT PRIMARY KEY,
    payload JSON NOT NULL
) ON COMMIT DROP;
CREATE TEMP TABLE federation_file_transfers (
    channel_id TEXT NOT NULL,
    room_id TEXT,
    transfer_id TEXT PRIMARY KEY,
    status TEXT NOT NULL
) ON COMMIT DROP;
CREATE TEMP TABLE federation_delivery_queue (
    id SERIAL PRIMARY KEY,
    activity_id INTEGER NOT NULL,
    target_inbox TEXT NOT NULL,
    target_domain TEXT NOT NULL CHECK (target_domain <> 'reject.example'),
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 12,
    last_attempt_at TIMESTAMPTZ,
    lease_token UUID,
    lease_expires_at TIMESTAMPTZ,
    next_retry_at TIMESTAMPTZ DEFAULT NOW(),
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (activity_id, target_inbox)
) ON COMMIT DROP;
INSERT INTO federation_room_members
    (room_id, actor_url, is_local, membership_status)
VALUES ('room-contract', 'https://peer.example/users/bob', FALSE, 'active');
INSERT INTO federation_remote_actors (actor_url, inbox_url, domain)
VALUES ('https://peer.example/users/bob', 'https://peer.example/inbox', 'reject.example');
"#,
        )
        .await
        .expect("create isolated federation contract tables");

    let failed = outer.begin().await.expect("begin failed outbox savepoint");
    failed
        .execute_unprepared(
            "INSERT INTO federation_room_messages (message_id) VALUES ('message-failed')",
        )
        .await
        .expect("stage local room message");
    let fanout_error = crate::federation::room::fanout_to_remote_members(
        &failed,
        i32::MAX,
        "room-contract",
        "https://local.example/activities/failed",
        &serde_json::json!({"id": "https://local.example/activities/failed"}),
        "RoomMessage",
        "RoomMessage",
    )
    .await;
    assert!(
        fanout_error.is_err(),
        "outbox enqueue failure must escape the fanout helper"
    );
    failed
        .rollback()
        .await
        .expect("rollback failed outbox savepoint");

    for table in [
        "federation_room_messages",
        "federation_activities",
        "federation_delivery_queue",
    ] {
        let row = outer
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                format!("SELECT COUNT(*)::BIGINT AS count FROM {table}"),
            ))
            .await
            .expect("count rolled-back outbox rows")
            .expect("count row");
        assert_eq!(
            row.try_get::<i64>("", "count").expect("read count"),
            0,
            "{table} must roll back with a failed outbox enqueue"
        );
    }

    outer
        .execute_unprepared(
            "UPDATE federation_remote_actors SET domain = 'peer.example' WHERE actor_url = 'https://peer.example/users/bob'",
        )
        .await
        .expect("make outbox target valid");
    let committed = outer
        .begin()
        .await
        .expect("begin successful outbox savepoint");
    committed
        .execute_unprepared(
            "INSERT INTO federation_room_messages (message_id) VALUES ('message-committed')",
        )
        .await
        .expect("stage committed room message");
    let fanout = crate::federation::room::fanout_to_remote_members(
        &committed,
        i32::MAX,
        "room-contract",
        "https://local.example/activities/committed",
        &serde_json::json!({"id": "https://local.example/activities/committed"}),
        "RoomMessage",
        "RoomMessage",
    )
    .await
    .expect("enqueue durable room outbox row");
    assert_eq!(fanout.enqueued, 1);
    committed
        .commit()
        .await
        .expect("commit room message and outbox");

    outer
        .execute_unprepared(
            r#"
TRUNCATE federation_delivery_queue, federation_activities RESTART IDENTITY;
INSERT INTO federation_activities
    (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
VALUES
    ('activity-lease-1', 1, 'Create', 'Note', '{"id":"activity-lease-1"}', TRUE, NOW()),
    ('activity-lease-2', 1, 'Create', 'Note', '{"id":"activity-lease-2"}', TRUE, NOW());
INSERT INTO federation_delivery_queue
    (activity_id, target_inbox, target_domain, status, created_at, next_retry_at)
VALUES
    (1, 'https://one.example/inbox', 'one.example', 'pending', NOW() - INTERVAL '2 seconds', NOW()),
    (2, 'https://two.example/inbox', 'two.example', 'pending', NOW() - INTERVAL '1 second', NOW());
"#,
        )
        .await
        .expect("seed delivery lease rows");

    let first_token = Uuid::new_v4();
    let first = crate::federation::delivery::claim_next_delivery(&outer, first_token)
        .await
        .expect("claim first delivery")
        .expect("first delivery row");
    let first_id = first.try_get::<i32>("", "id").expect("first queue id");

    let waiting = outer
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT status, lease_token FROM federation_delivery_queue WHERE id <> 1 ORDER BY id LIMIT 1".to_string(),
        ))
        .await
        .expect("read waiting delivery")
        .expect("waiting delivery row");
    assert_eq!(waiting.try_get::<String>("", "status").unwrap(), "pending");
    assert!(waiting
        .try_get::<Option<Uuid>>("", "lease_token")
        .unwrap()
        .is_none());

    assert!(
        crate::federation::delivery::renew_delivery_lease(&outer, first_id, first_token)
            .await
            .expect("renew live delivery lease")
    );

    let second_token = Uuid::new_v4();
    let second = crate::federation::delivery::claim_next_delivery(&outer, second_token)
        .await
        .expect("claim second delivery")
        .expect("second delivery row");
    assert_ne!(second.try_get::<i32>("", "id").unwrap(), first_id);

    outer
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE federation_delivery_queue SET lease_expires_at = NOW() - INTERVAL '1 second' WHERE id = $1",
            [first_id.into()],
        ))
        .await
        .expect("expire crashed worker lease");
    let replacement_token = Uuid::new_v4();
    let replacement = crate::federation::delivery::claim_next_delivery(&outer, replacement_token)
        .await
        .expect("reclaim expired delivery")
        .expect("reclaimed row");
    assert_eq!(replacement.try_get::<i32>("", "id").unwrap(), first_id);
    assert_eq!(
        replacement.try_get::<String>("", "prev_status").unwrap(),
        "delivering"
    );
    assert!(
        !crate::federation::delivery::mark_delivery_delivered_if_owned(
            &outer,
            first_id,
            first_token
        )
        .await
        .expect("apply stale completion fence"),
        "old worker token must not publish an outcome after reclaim"
    );
    assert!(
        crate::federation::delivery::mark_delivery_delivered_if_owned(
            &outer,
            first_id,
            replacement_token
        )
        .await
        .expect("complete with current lease owner")
    );

    outer
        .execute_unprepared(
            r#"
TRUNCATE federation_delivery_queue, federation_activities, federation_instances,
         federation_follows, federation_channels, federation_channel_messages,
         federation_file_transfers, federation_room_messages, federation_room_members,
         federation_rooms, federation_remote_actors
         RESTART IDENTITY;

INSERT INTO federation_remote_actors (actor_url, inbox_url, domain)
VALUES
    ('https://peer.example/users/bob', 'https://peer.example/inbox', 'peer.example'),
    ('https://other.example/users/eve', 'https://other.example/inbox', 'other.example');
INSERT INTO federation_instances (domain, failure_count)
VALUES ('peer.example', 0), ('other.example', 0);
INSERT INTO federation_follows (user_id, remote_actor_id, direction, status, accepted_at)
VALUES
    (1, 1, 'outgoing', 'accepted', NOW()),
    (1, 1, 'incoming', 'accepted', NOW()),
    (1, 2, 'outgoing', 'accepted', NOW());
INSERT INTO federation_channels (channel_id, remote_actor_id, status)
VALUES
    ('channel-peer', 1, 'active'),
    ('channel-other', 2, 'active');
INSERT INTO federation_channel_messages (channel_id, message_id, payload)
VALUES ('channel-peer', 'message-history', '{"text":"keep me"}');
INSERT INTO federation_rooms (room_id, owner_actor, home_server)
VALUES
    ('room-remote', 'https://peer.example/users/bob', 'https://peer.example:8443'),
    ('room-local', 'https://local.example/users/alice', 'local.example');
INSERT INTO federation_room_messages (message_id, room_id)
VALUES ('room-message-history', 'room-remote');
INSERT INTO federation_file_transfers (channel_id, room_id, transfer_id, status)
VALUES
    ('channel-peer', NULL, 'transfer-peer', 'in-progress'),
    ('channel-other', NULL, 'transfer-other', 'in-progress'),
    ('', 'room-remote', 'transfer-room-remote', 'in-progress'),
    ('', 'room-local', 'transfer-room-local', 'in-progress');
INSERT INTO federation_room_members (room_id, actor_url, is_local, membership_status)
VALUES
    ('room-remote', 'https://peer.example/users/bob', FALSE, 'active'),
    ('room-remote', 'https://peer.example:8443/users/uncached', FALSE, 'pending'),
    ('room-remote', 'https://other.example/users/eve', FALSE, 'active'),
    ('room-remote', 'https://local.example/users/alice', TRUE, 'active'),
    ('room-local', 'https://peer.example/users/bob', FALSE, 'active'),
    ('room-local', 'https://local.example/users/alice', TRUE, 'active');
INSERT INTO federation_activities
    (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
SELECT
    'activity-failure-' || n,
    CASE WHEN n = 6 THEN 2 ELSE 1 END,
    'Create',
    'Note',
    json_build_object('id', 'activity-failure-' || n),
    TRUE,
    NOW()
FROM generate_series(1, 10) AS n;
INSERT INTO federation_delivery_queue
    (activity_id, target_inbox, target_domain, status, created_at, next_retry_at, error_message)
VALUES
    (1, 'https://peer.example/inbox/1', 'peer.example', 'pending', NOW(), NOW(), NULL),
    (2, 'https://peer.example/inbox/2', 'peer.example', 'pending', NOW(), NOW(), NULL),
    (3, 'https://peer.example/inbox/3', 'peer.example', 'pending', NOW(), NOW(), NULL),
    (4, 'https://peer.example/inbox/4', 'peer.example', 'pending', NOW(), NOW(), NULL),
    (5, 'https://peer.example/inbox/rejected', 'peer.example', 'pending', NOW(), NOW(), NULL),
    (6, 'https://peer.example/inbox/extra', 'peer.example', 'pending', NOW(), NOW(), NULL),
    (7, 'https://other.example/inbox', 'other.example', 'pending', NOW(), NOW(), NULL),
    (8, 'https://peer.example/inbox/completed', 'peer.example', 'delivered', NOW(), NULL, NULL),
    (9, 'https://peer.example/inbox/already-cancelled', 'peer.example', 'dead', NOW(), NULL,
     'cancelled: existing teardown'),
    (10, 'https://peer.example/inbox/local-failure', 'peer.example', 'dead',
     NOW() - INTERVAL '90 days', NULL, 'Key load failed: decrypt');
"#,
        )
        .await
        .expect("seed domain relationship revocation contract");

    // Claim a queue row and hand the settlement a live lease token.
    async fn claim_for_settlement(
        outer: &sea_orm::DatabaseTransaction,
        queue_id: i32,
    ) -> uuid::Uuid {
        let token = Uuid::new_v4();
        outer
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"UPDATE federation_delivery_queue
                   SET status = 'delivering', lease_token = $2,
                       lease_expires_at = NOW() + INTERVAL '10 minutes'
                   WHERE id = $1"#,
                [queue_id.into(), token.into()],
            ))
            .await
            .expect("claim delivery for failure settlement contract");
        token
    }

    async fn active_channel_count(outer: &sea_orm::DatabaseTransaction) -> i64 {
        outer
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT COUNT(*)::BIGINT AS count FROM federation_channels WHERE status = 'active'"
                    .to_string(),
            ))
            .await
            .expect("count active channels")
            .expect("active channel count row")
            .try_get::<i64>("", "count")
            .unwrap()
    }

    // Phase 1 — an unreachable streak accumulates without touching relationships.
    for queue_id in 1..=4 {
        let token = claim_for_settlement(&outer, queue_id).await;
        let settlement = crate::federation::delivery::settle_remote_delivery_failure(
            &outer,
            queue_id,
            token,
            "peer.example",
            1,
            "HTTP 503 from peer",
            crate::federation::delivery::RemoteDeliveryFailureDisposition::RetryAfter(60),
            true,
        )
        .await
        .expect("settle confirmed remote HTTP failure");

        assert!(settlement.applied);
        assert!(settlement.counted_toward_streak);
        assert_eq!(settlement.consecutive_failures, queue_id);
        assert!(!settlement.relationships_revoked);
        assert_eq!(active_channel_count(&outer).await, 2);
    }

    // Phase 2 — a permanent rejection proves the peer answered. It must leave the
    // streak untouched in both directions: no increment, no reset.
    let token = claim_for_settlement(&outer, 5).await;
    let rejected = crate::federation::delivery::settle_remote_delivery_failure(
        &outer,
        5,
        token,
        "peer.example",
        1,
        "PERMANENT HTTP 404: not_found",
        crate::federation::delivery::RemoteDeliveryFailureDisposition::Dead,
        false,
    )
    .await
    .expect("settle permanent peer rejection");
    assert!(rejected.applied);
    assert!(!rejected.counted_toward_streak);
    assert!(!rejected.relationships_revoked);
    assert_eq!(rejected.consecutive_failures, 4);
    let after_reject = outer
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"SELECT failure_count, (failing_since IS NULL) AS streak_cleared
               FROM federation_instances WHERE domain = 'peer.example'"#
                .to_string(),
        ))
        .await
        .expect("read health after permanent rejection")
        .expect("peer instance row after rejection");
    assert_eq!(after_reject.try_get::<i32>("", "failure_count").unwrap(), 4);
    assert!(!after_reject.try_get::<bool>("", "streak_cleared").unwrap());

    // Phase 3 — count alone must not revoke. A fan-out to one peer can burn an
    // arbitrary failure count inside a single worker tick while it restarts.
    outer
        .execute_unprepared(
            "UPDATE federation_instances SET failure_count = 500, failing_since = NOW()
             WHERE domain = 'peer.example';",
        )
        .await
        .expect("simulate an instantaneous failure burst");
    let token = claim_for_settlement(&outer, 6).await;
    let burst = crate::federation::delivery::settle_remote_delivery_failure(
        &outer,
        6,
        token,
        "peer.example",
        1,
        "Request failed: connection refused",
        crate::federation::delivery::RemoteDeliveryFailureDisposition::RetryAfter(60),
        true,
    )
    .await
    .expect("settle burst failure");
    assert_eq!(burst.consecutive_failures, 501);
    assert!(burst.streak_secs < 60);
    assert!(
        !burst.relationships_revoked,
        "a burst inside one tick must never revoke, however high the count"
    );
    assert_eq!(active_channel_count(&outer).await, 2);

    // Phase 4 — elapsed time alone must not revoke either.
    outer
        .execute_unprepared(
            "UPDATE federation_instances
             SET failure_count = 3, failing_since = NOW() - INTERVAL '30 days'
             WHERE domain = 'peer.example';",
        )
        .await
        .expect("simulate a long but sparse failure streak");
    let token = claim_for_settlement(&outer, 6).await;
    let sparse = crate::federation::delivery::settle_remote_delivery_failure(
        &outer,
        6,
        token,
        "peer.example",
        2,
        "Request failed: connection refused",
        crate::federation::delivery::RemoteDeliveryFailureDisposition::RetryAfter(60),
        true,
    )
    .await
    .expect("settle sparse failure");
    assert_eq!(sparse.consecutive_failures, 4);
    assert!(sparse.streak_secs > 29 * 24 * 60 * 60);
    assert!(
        !sparse.relationships_revoked,
        "an old streak with few failures must not revoke"
    );
    assert_eq!(active_channel_count(&outer).await, 2);

    // Phase 5 — sustained count *and* a week-long streak together do revoke.
    outer
        .execute_unprepared(
            "UPDATE federation_instances
             SET failure_count = 19, failing_since = NOW() - INTERVAL '8 days'
             WHERE domain = 'peer.example';",
        )
        .await
        .expect("simulate a sustained week-long outage");
    let token = claim_for_settlement(&outer, 6).await;
    let settlement = crate::federation::delivery::settle_remote_delivery_failure(
        &outer,
        6,
        token,
        "peer.example",
        3,
        "Request failed: connection refused",
        crate::federation::delivery::RemoteDeliveryFailureDisposition::RetryAfter(60),
        true,
    )
    .await
    .expect("settle the failure that crosses both thresholds");
    assert!(settlement.applied);
    assert_eq!(settlement.consecutive_failures, 20);
    assert!(settlement.relationships_revoked);
    assert_eq!(settlement.follows_removed, 2);
    assert_eq!(settlement.channels_closed, 1);
    assert_eq!(settlement.room_members_removed, 4);
    assert_eq!(settlement.transfers_cancelled, 2);
    // Rows 1-4 and 6 only. The delivered row, the already-cancelled row, the
    // permanently rejected row and the 90-day-old local dead-letter are all
    // terminal already and must be left alone.
    assert_eq!(settlement.deliveries_cancelled, 5);
    // Every owner of a cancelled row is reported so the caller can notify them;
    // the bulk sweep never reaches the per-row dead-letter path.
    assert_eq!(settlement.cancelled_owners, vec![(1, 4), (2, 1)]);

    // Revocation restarts the clock: a re-established relationship gets a fresh
    // count *and* a fresh window before it can ever be torn down again.
    let instance = outer
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"SELECT failure_count, (failing_since IS NULL) AS streak_cleared
               FROM federation_instances WHERE domain = 'peer.example'"#
                .to_string(),
        ))
        .await
        .expect("read reset failure streak")
        .expect("peer instance row");
    assert_eq!(instance.try_get::<i32>("", "failure_count").unwrap(), 0);
    assert!(instance.try_get::<bool>("", "streak_cleared").unwrap());

    let relationship_state = outer
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"SELECT
                 (SELECT COUNT(*) FROM federation_follows f
                  JOIN federation_remote_actors ra ON ra.id = f.remote_actor_id
                  WHERE ra.domain = 'peer.example')::BIGINT AS target_follows,
                 (SELECT COUNT(*) FROM federation_channels c
                  JOIN federation_remote_actors ra ON ra.id = c.remote_actor_id
                  WHERE ra.domain = 'peer.example' AND c.status = 'closed')::BIGINT AS closed_channels,
                 (SELECT COUNT(*) FROM federation_room_members
                  WHERE is_local = FALSE
                    AND LOWER(regexp_replace(
                          split_part(regexp_replace(BTRIM(actor_url), '^https?://', '', 'i'), '/', 1),
                          ':[0-9]+$', ''
                        )) = 'peer.example')::BIGINT AS target_members,
                 (SELECT COUNT(*) FROM federation_room_members
                  WHERE room_id = 'room-remote' AND is_local = TRUE)::BIGINT AS local_remote_room_members,
                 (SELECT COUNT(*) FROM federation_room_members
                  WHERE room_id = 'room-local' AND is_local = TRUE)::BIGINT AS local_local_room_members,
                 (SELECT COUNT(*) FROM federation_channel_messages
                  WHERE message_id = 'message-history')::BIGINT AS preserved_channel_messages,
                 (SELECT COUNT(*) FROM federation_room_messages
                  WHERE message_id = 'room-message-history')::BIGINT AS preserved_room_messages,
                 (SELECT COUNT(*) FROM federation_delivery_queue
                  WHERE target_domain = 'peer.example'
                    AND status IN ('pending', 'delivering'))::BIGINT AS unfinished_deliveries,
                 (SELECT COUNT(*) FROM federation_delivery_queue
                  WHERE target_domain = 'peer.example'
                    AND error_message ILIKE 'cancelled: federation relationship revoked%')::BIGINT AS cancelled_deliveries,
                 (SELECT COUNT(*) FROM federation_delivery_queue
                  WHERE target_domain = 'peer.example'
                    AND status = 'delivered')::BIGINT AS preserved_completed_deliveries,
                 (SELECT COUNT(*) FROM federation_delivery_queue
                  WHERE target_inbox = 'https://peer.example/inbox/already-cancelled'
                    AND status = 'dead'
                    AND error_message = 'cancelled: existing teardown')::BIGINT AS preserved_cancelled_delivery,
                 (SELECT COUNT(*) FROM federation_delivery_queue
                  WHERE target_inbox = 'https://peer.example/inbox/local-failure'
                    AND status = 'dead'
                    AND error_message = 'Key load failed: decrypt')::BIGINT AS preserved_local_dead_letter,
                 (SELECT COUNT(*) FROM federation_delivery_queue
                  WHERE target_inbox = 'https://peer.example/inbox/rejected'
                    AND status = 'dead'
                    AND error_message = 'PERMANENT HTTP 404: not_found')::BIGINT AS preserved_peer_rejection"#
                .to_string(),
        ))
        .await
        .expect("read revoked relationship state")
        .expect("relationship state row");
    assert_eq!(
        relationship_state
            .try_get::<i64>("", "target_follows")
            .unwrap(),
        0
    );
    assert_eq!(
        relationship_state
            .try_get::<i64>("", "closed_channels")
            .unwrap(),
        1
    );
    assert_eq!(
        relationship_state
            .try_get::<i64>("", "target_members")
            .unwrap(),
        0
    );
    assert_eq!(
        relationship_state
            .try_get::<i64>("", "local_remote_room_members")
            .unwrap(),
        0
    );
    assert_eq!(
        relationship_state
            .try_get::<i64>("", "local_local_room_members")
            .unwrap(),
        1
    );
    assert_eq!(
        relationship_state
            .try_get::<i64>("", "preserved_channel_messages")
            .unwrap(),
        1
    );
    assert_eq!(
        relationship_state
            .try_get::<i64>("", "preserved_room_messages")
            .unwrap(),
        1
    );
    assert_eq!(
        relationship_state
            .try_get::<i64>("", "unfinished_deliveries")
            .unwrap(),
        0
    );
    assert_eq!(
        relationship_state
            .try_get::<i64>("", "cancelled_deliveries")
            .unwrap(),
        5
    );
    assert_eq!(
        relationship_state
            .try_get::<i64>("", "preserved_completed_deliveries")
            .unwrap(),
        1
    );
    assert_eq!(
        relationship_state
            .try_get::<i64>("", "preserved_cancelled_delivery")
            .unwrap(),
        1
    );
    // A dead-letter that failed for a *local* reason keeps its real cause and its
    // eligibility for `retry_all_dead_for_user`, which skips `cancelled:` rows.
    // Relabelling it would both misattribute the failure and strand it forever.
    assert_eq!(
        relationship_state
            .try_get::<i64>("", "preserved_local_dead_letter")
            .unwrap(),
        1
    );
    assert_eq!(
        relationship_state
            .try_get::<i64>("", "preserved_peer_rejection")
            .unwrap(),
        1
    );

    let unrelated_state = outer
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"SELECT
                 (SELECT COUNT(*) FROM federation_follows f
                  JOIN federation_remote_actors ra ON ra.id = f.remote_actor_id
                  WHERE ra.domain = 'other.example' AND f.status = 'accepted')::BIGINT AS accepted_follows,
                 (SELECT COUNT(*) FROM federation_channels c
                  JOIN federation_remote_actors ra ON ra.id = c.remote_actor_id
                  WHERE ra.domain = 'other.example' AND c.status = 'active')::BIGINT AS active_channels,
                 (SELECT COUNT(*) FROM federation_room_members
                  WHERE actor_url LIKE 'https://other.example/%'
                    AND membership_status = 'active')::BIGINT AS active_members,
                 (SELECT COUNT(*) FROM federation_file_transfers
                  WHERE transfer_id = 'transfer-other' AND status = 'in-progress')::BIGINT AS active_transfers,
                 (SELECT COUNT(*) FROM federation_file_transfers
                  WHERE transfer_id = 'transfer-room-local' AND status = 'in-progress')::BIGINT AS active_local_room_transfers,
                 (SELECT COUNT(*) FROM federation_file_transfers
                  WHERE transfer_id IN ('transfer-peer', 'transfer-room-remote')
                    AND status = 'cancelled')::BIGINT AS cancelled_target_transfers,
                 (SELECT COUNT(*) FROM federation_delivery_queue
                  WHERE target_domain = 'other.example' AND status = 'pending')::BIGINT AS pending_deliveries"#
                .to_string(),
        ))
        .await
        .expect("read unrelated domain state")
        .expect("unrelated state row");
    for column in [
        "accepted_follows",
        "active_channels",
        "active_members",
        "active_transfers",
        "active_local_room_transfers",
        "pending_deliveries",
    ] {
        assert_eq!(unrelated_state.try_get::<i64>("", column).unwrap(), 1);
    }
    assert_eq!(
        unrelated_state
            .try_get::<i64>("", "cancelled_target_transfers")
            .unwrap(),
        2
    );

    outer
        .execute_unprepared(
            r#"
UPDATE federation_instances
SET failure_count = 19, failing_since = NOW() - INTERVAL '8 days'
WHERE domain = 'other.example';
UPDATE federation_delivery_queue
SET status = 'delivering', lease_token = '00000000-0000-0000-0000-000000000001',
    lease_expires_at = NOW() + INTERVAL '10 minutes'
WHERE target_domain = 'other.example';
"#,
        )
        .await
        .expect("prepare success streak reset contract");
    assert!(crate::federation::delivery::settle_remote_delivery_success(
        &outer,
        7,
        Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
        "other.example",
    )
    .await
    .expect("settle successful remote delivery"));
    // One success clears both halves of the gate, even from the very edge of it.
    let reset = outer
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"SELECT failure_count, (failing_since IS NULL) AS streak_cleared
               FROM federation_instances WHERE domain = 'other.example'"#
                .to_string(),
        ))
        .await
        .expect("read success reset")
        .expect("other instance row");
    assert_eq!(reset.try_get::<i32>("", "failure_count").unwrap(), 0);
    assert!(reset.try_get::<bool>("", "streak_cleared").unwrap());

    outer
        .execute_unprepared(
            r#"
UPDATE federation_instances
SET failure_count = 3, failing_since = NOW() - INTERVAL '2 days'
WHERE domain = 'other.example';
INSERT INTO federation_activities
    (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
VALUES
    ('activity-stale-success', 1, 'Create', 'Note', '{"id":"activity-stale-success"}', TRUE, NOW());
INSERT INTO federation_delivery_queue
    (activity_id, target_inbox, target_domain, status, lease_token, lease_expires_at, created_at)
VALUES
    (11, 'https://other.example/stale', 'other.example', 'delivering',
     '00000000-0000-0000-0000-000000000002', NOW() + INTERVAL '10 minutes', NOW());
"#,
        )
        .await
        .expect("prepare stale success settlement contract");
    assert!(
        !crate::federation::delivery::settle_remote_delivery_success(
            &outer,
            11,
            Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap(),
            "other.example",
        )
        .await
        .expect("reject stale successful delivery settlement")
    );
    // A reclaimed/cancelled lease cannot launder a domain back to healthy.
    let stale_health = outer
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"SELECT failure_count, (failing_since IS NULL) AS streak_cleared
               FROM federation_instances WHERE domain = 'other.example'"#
                .to_string(),
        ))
        .await
        .expect("read health after stale success")
        .expect("other instance health row");
    assert_eq!(stale_health.try_get::<i32>("", "failure_count").unwrap(), 3);
    assert!(!stale_health.try_get::<bool>("", "streak_cleared").unwrap());

    outer
        .rollback()
        .await
        .expect("rollback isolated federation contract tables");
}

#[tokio::test]
async fn tapp_storage_upgrade_heals_and_enforces_credential_constraint() {
    let Ok(url) = std::env::var("MYRIAD_TAPP_STORAGE_UPGRADE_DB") else {
        eprintln!("skipping: set MYRIAD_TAPP_STORAGE_UPGRADE_DB for the upgrade guard test");
        return;
    };
    use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement};

    let db = Database::connect(&url)
        .await
        .expect("connect to upgrade guard database");
    db.execute_unprepared(
        r#"
CREATE TABLE tapp_storage (
    id SERIAL PRIMARY KEY,
    tapp_id VARCHAR(255) NOT NULL,
    user_id INTEGER NOT NULL,
    key VARCHAR(255) NOT NULL,
    value JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
INSERT INTO tapp_storage (tapp_id, user_id, key, value)
VALUES ('upgrade.test', 1, 'ordinary', '{}'::jsonb);
ALTER TABLE tapp_storage ADD COLUMN encrypted_value TEXT;
ALTER TABLE tapp_storage ADD COLUMN binding_fingerprint VARCHAR(64);
"#,
    )
    .await
    .expect("create pre-credential storage shape");

    super::ensure_heals::ensure_tapp_storage_credential_constraint(&db)
        .await
        .expect("upgrade helper must add and validate the constraint");

    let invalid = db
        .execute_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "UPDATE tapp_storage SET encrypted_value = 'ciphertext' WHERE key = 'ordinary'"
                .to_string(),
        ))
        .await;
    assert!(
        invalid.is_err(),
        "healed constraint must reject invalid updates"
    );

    db.execute_unprepared(
        r#"
INSERT INTO tapp_storage
    (tapp_id, user_id, key, value, encrypted_value, binding_fingerprint)
VALUES
    ('upgrade.test', 1, '_credentials.api', '{}', 'ciphertext', repeat('f', 64));
"#,
    )
    .await
    .expect("healed constraint must accept a complete credential row");
}
