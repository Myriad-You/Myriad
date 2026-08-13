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
        // 005 扩展
        "federation_content_filters",
        "federation_policy_settings",
        "federation_domain_aliases",
        "federation_object_interactions",
        // 012
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

    let indexes = get_expected_indexes();
    let idx_names: Vec<&str> = indexes.iter().map(|i| i.name.as_str()).collect();
    for required in [
        "idx_analytics_page_daily_day",
        "idx_analytics_country_daily_day",
        "idx_heartbeat_claims_claimed_at",
        "idx_federation_domain_aliases_new",
        "idx_fed_interactions_object_kind",
        "idx_fed_interactions_user_kind_created",
        "federation_inbox_receipts_pkey",
    ] {
        assert!(
            idx_names.contains(&required),
            "missing recent-feature index in get_expected_indexes: {required}"
        );
    }
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
    assert_eq!(values["user_perm_component_theme"], serde_json::json!(false));
    assert_eq!(values["user_ai_daily_calls"], serde_json::json!(999));
    assert_eq!(values["user_perm_shortcut_register"], serde_json::json!(true));
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
    // 012 migration while retaining its scope-less table. Rewriting 012 would
    // never run for that database, so 013 must repair the persisted shape.
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
DELETE FROM seaql_migrations
WHERE version = '013_federation_inbox_receipts_v2';
"#,
    )
    .await
    .expect("create the legacy receipt shape and rewind only migration 013");

    crate::db::Migrator::up(&db, None)
        .await
        .expect("013 must upgrade a database that already recorded old 012");
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
        "013 must remove every column unique to the scope-less receipt shape"
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
    message_id TEXT PRIMARY KEY
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
