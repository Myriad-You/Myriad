//! schema_check unit tests
use super::expected_indexes::get_expected_indexes;
use super::expected_schema::get_expected_schema;
use super::introspect::*;
use super::types::*;

use super::*;

#[test]
fn agent_tasks_status_check_matches_parser() {
    let sql = super::ensure_heals::agent_tasks_status_check_sql();
    for status in myriad_agent_rules::TASK_STATUS_DB_VALUES {
        assert!(
            sql.contains(&format!("'{status}'")),
            "CHECK must include {status}"
        );
    }
    assert!(sql.contains("agent_tasks_status_check"));
}

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

/// 001/004/005 扩展表：须在 get_expected_schema / indexes 有完整条目。
#[test]
fn test_folded_extension_tables_in_expected_schema() {
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
        // 005
        "federation_inbox_receipts",
        // 003
        "phantasi_note_docs",
        "phantasi_note_authors",
        "phantasi_source_applications",
        "media_assets",
        "media_references",
        "media_url_aliases",
        "media_migration_jobs",
        // 006
        "user_identities",
    ] {
        assert!(
            names.contains(&required),
            "missing table in get_expected_schema: {required}"
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
        "idx_user_identities_provider_uid",
        "idx_user_identities_user",
        "idx_user_identities_provider_email",
        "idx_platform_metadata_user_platform",
        "idx_media_assets_public_id",
        "idx_media_references_slot",
        "idx_media_url_aliases_local_path",
        "idx_media_migration_jobs_source",
    ] {
        assert!(
            idx_names.contains(&required),
            "missing index in get_expected_indexes: {required}"
        );
    }
}

#[test]
fn uniqueness_heals_are_invoked_and_partial() {
    let orchestrator = include_str!("orchestrator.rs");
    for heal in [
        "ensure_phantasi_note_source_unique",
        "ensure_phantasi_source_url_key_unique",
        "ensure_rsshub_global_url_unique",
        "ensure_phantasi_application_pending_unique",
        "ensure_tapp_shortcut_chord_unique",
    ] {
        assert!(orchestrator.contains(heal), "orchestrator must call {heal}");
    }
    let heals = include_str!("ensure_heals.rs");
    assert!(
        heals.contains("AND key <> NEW.key"),
        "quota INSERT must exclude the conflicting unique key so UPSERT does not double-count",
    );
    let migration = include_str!("../../../migrations/002_tapp_system.rs");
    assert!(
        migration.contains("AND key <> NEW.key"),
        "migration quota INSERT must match the heal",
    );
    assert!(
        heals.contains("ORDER BY fetched_at DESC NULLS LAST"),
        "platform_metadata unique heal must keep the newest snapshot, not the smallest id"
    );
    assert!(heals.contains("UPDATE metadata_history"));
    assert!(heals.contains("idx_phantasi_sources_note_type"));
    assert!(heals.contains("idx_rsshub_instances_global_url"));
    assert!(heals.contains("CASE health_status"));
    assert!(heals.contains("active channel relationship collision across different channel_id"));
    assert!(heals.contains("shortcut chord collision across different bindings"));
    assert!(heals.contains("idx_phantasi_source_applications_pending_site"));
    assert!(heals.contains("idx_tapp_shortcuts_owner_chord"));
    let indexes = get_expected_indexes();
    let names: Vec<&str> = indexes.iter().map(|idx| idx.name.as_str()).collect();
    for partial in [
        "idx_phantasi_sources_note_type",
        "idx_rsshub_instances_global_url",
        "idx_phantasi_source_applications_pending_site",
        "idx_tapp_shortcuts_owner_chord",
        "idx_media_assets_producer_key",
    ] {
        assert!(
            !names.contains(&partial),
            "{partial} is partial unique and must not go through generic index DDL"
        );
    }
}

#[test]
fn media_asset_model_is_in_expected_schema_and_shared_sql() {
    let heals = include_str!("ensure_heals.rs");
    let migration = include_str!("../../../migrations/003_phantasi_system.rs");
    let sql = include_str!("../../../migrations/media_asset_model.sql");
    assert!(heals.contains("media_asset_model.sql"));
    assert!(migration.contains("media_asset_model.sql"));
    assert!(sql.contains("idx_media_assets_producer_key"));
    assert!(sql.contains("NULLS NOT DISTINCT"));
    assert!(sql.contains("ON DELETE RESTRICT"));
    assert!(sql.contains("ON DELETE SET NULL"));
    assert!(!sql.contains("ON DELETE CASCADE"));

    let tables = get_expected_schema();
    let assets = tables
        .iter()
        .find(|t| t.name == "media_assets")
        .expect("media_assets");
    for col in [
        "public_id",
        "scope",
        "owner_user_id",
        "storage_key",
        "state",
        "exposure",
        "source",
        "checksum_sha256",
        "write_token",
        "write_lease_until",
        "producer_key",
        "references_complete",
    ] {
        assert!(
            assets.columns.iter().any(|c| c.name == col),
            "media_assets missing column {col}"
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
    assert_eq!(col.default_value.as_deref(), Some("0"));
}

#[test]
fn test_users_schema_includes_locale() {
    let tables = get_expected_schema();
    let users = tables
        .iter()
        .find(|t| t.name == "users")
        .expect("users table");
    let col = users
        .columns
        .iter()
        .find(|c| c.name == "locale")
        .expect("users must define locale (account UI language)");
    assert_eq!(col.data_type, "character varying");
}

#[test]
fn test_agent_addressee_schema_includes_music_mood_cooldown() {
    let tables = get_expected_schema();
    let addressee = tables
        .iter()
        .find(|table| table.name == "agent_addressee_state")
        .expect("agent_addressee_state table");
    let column = addressee
        .columns
        .iter()
        .find(|column| column.name == "music_mood_credited_at")
        .expect("music mood cooldown must be durable and field-healed");
    assert_eq!(column.data_type, "timestamp with time zone");
}

#[test]
fn test_default_platform_seeds_include_x_and_core() {
    let seeds = default_platform_seeds();
    let names: Vec<&str> = seeds.iter().map(|s| s.name).collect();
    for required in crate::services::platform_id::PlatformId::ALL {
        assert!(
            names
                .iter()
                .any(|name| crate::services::platform_id::PlatformId::parse(name)
                    == Some(required)),
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
        values["user_perm_phantasi_comment_write"],
        serde_json::json!(false)
    );
    assert_eq!(
        values["guest_perm_phantasi_comment_write"],
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
    assert_eq!(col.default_value.as_deref(), Some("'[]'"));
}

#[test]
fn test_tapps_schema_includes_needs_reauthorization_marker() {
    // Durable re-authorization marker, default false.
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
    }
}

#[test]
fn test_generate_add_column_ddl() {
    let col = ColumnDef {
        name: "test_col".into(),
        data_type: "VARCHAR(255)".into(),
        default_value: Some("'default'".into()),
        not_null: false,
    };

    let ddl = generate_add_column_ddl("users", &col);
    assert!(ddl.contains("ALTER TABLE users"));
    assert!(ddl.contains("ADD COLUMN IF NOT EXISTS"));
    assert!(ddl.contains("test_col"));
    assert!(ddl.contains("DEFAULT 'default'"));
    assert!(!ddl.contains("NOT NULL"));
}

#[test]
fn test_generate_add_column_ddl_preserves_not_null() {
    let col = ColumnDef::new("username", "character varying").not_null();
    let ddl = generate_add_column_ddl("users", &col);
    assert!(
        ddl.contains("username character varying NOT NULL"),
        "repair ADD COLUMN must keep greenfield NOT NULL: {ddl}"
    );
    assert!(!ddl.contains("DEFAULT"));
}

#[test]
fn test_users_username_is_not_null_in_expected_schema() {
    let tables = get_expected_schema();
    let users = tables
        .iter()
        .find(|t| t.name == "users")
        .expect("users table");
    let username = users
        .columns
        .iter()
        .find(|c| c.name == "username")
        .expect("users.username");
    assert!(
        username.not_null,
        "greenfield users.username is NOT NULL; expected schema must record it"
    );
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

#[test]
fn platform_metadata_requires_user_platform_unique() {
    let idx = get_expected_indexes()
        .into_iter()
        .find(|item| item.name == "idx_platform_metadata_user_platform")
        .expect("idx_platform_metadata_user_platform");
    assert!(idx.is_unique);
    assert_eq!(idx.table, "platform_metadata");
    assert_eq!(idx.columns, vec!["user_id", "platform_name"]);
}

/// **CI 漂移闸门。**
///
/// 在一个刚跑完 `Migrator::up` 的全新数据库上，`report_schema_drift` 必须返回空。
/// 一旦不为空，就说明 `migrations/` 里的建表语句与 `get_expected_schema()` 已经不一致。
///
/// 需要真实 PostgreSQL。没有 `MYRIAD_SCHEMA_DRIFT_DB` 时静默跳过，
/// 这样本地 `cargo test` 不受影响；CI 里由 postgres service 提供该变量。
#[tokio::test]
async fn migrations_leave_no_schema_drift() {
    let Ok(url) = std::env::var("MYRIAD_SCHEMA_DRIFT_DB") else {
        eprintln!("skipping: set MYRIAD_SCHEMA_DRIFT_DB to run the schema drift gate");
        return;
    };

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
    // Fixture account for the upgrade rows below (user-owned tables reject
    // unknown positive user ids).
    db.execute_unprepared("INSERT INTO users (id, username) VALUES (2147483647, 'schema-fixture')")
        .await
        .expect("insert fixture user");
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

    db.execute_unprepared(r#"
ALTER TABLE tapps ADD COLUMN granted_permissions JSONB NOT NULL DEFAULT '[]';
ALTER TABLE platform_reports ADD COLUMN expires_at TIMESTAMP;
ALTER TABLE phantasi_sources ADD COLUMN unread_count INTEGER NOT NULL DEFAULT 0;
DROP INDEX idx_phantasi_items_source_recent;
DROP INDEX idx_metadata_history_user_date;
DROP INDEX idx_media_assets_created_id;
CREATE INDEX idx_phantasi_items_published ON phantasi_items (source_id, published_at);
CREATE INDEX idx_metadata_history_user ON metadata_history (user_id);
CREATE INDEX idx_users_github_id ON users (github_id);
ALTER TABLE phantasi_sources DROP COLUMN url_key, DROP COLUMN site_url_key;
INSERT INTO phantasi_sources (user_id, name, url, site_url)
VALUES (2147483647, 'url-key-upgrade-test', 'https://Blog.EXAMPLE/rss/#top', 'https://Blog.EXAMPLE/');
INSERT INTO tapps (tapp_id, user_id, name, version, manifest, file_path, code_path, approved_permissions, granted_permissions)
VALUES ('projection-upgrade-test', 2147483647, 'Test', '1', '{}', '', '', '["report:read"]', '["obsolete"]');
"#).await.unwrap();

    // Scope-less `federation_inbox_receipts` shape; `ensure_schema` must heal it.
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

    db.execute_unprepared(
        r#"
DROP INDEX idx_delivery_queue_activity_target;
DROP INDEX idx_timeline_user_activity;
INSERT INTO federation_delivery_queue (id, activity_id, target_inbox, target_domain, attempts)
VALUES (-2, 2147483647, 'https://schema.test/inbox', 'schema.test', 3),
       (-1, 2147483647, 'https://schema.test/inbox', 'schema.test', 0);
INSERT INTO federation_timeline (id, user_id, activity_id, is_read)
VALUES (-2, 2147483647, 'https://schema.test/activity', TRUE),
       (-1, 2147483647, 'https://schema.test/activity', FALSE);
"#,
    )
    .await
    .expect("seed duplicate rows in the legacy schema without unique indexes");

    ensure_schema(&db)
        .await
        .expect("schema heal must upgrade legacy receipts and deduplicate before creating indexes");

    let retired = db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, r#"
SELECT COUNT(*) AS n FROM information_schema.columns WHERE table_schema = current_schema()
AND (table_name, column_name) IN (('tapps', 'granted_permissions'), ('platform_reports', 'expires_at'), ('phantasi_sources', 'unread_count'))
"#)).await.unwrap().unwrap();
    assert_eq!(retired.try_get::<i64>("", "n").unwrap(), 0);
    let approved = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT approved_permissions FROM tapps WHERE tapp_id = 'projection-upgrade-test'",
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        approved
            .try_get::<serde_json::Value>("", "approved_permissions")
            .unwrap(),
        serde_json::json!(["report:read"])
    );
    let index = db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres,
        "SELECT indexdef FROM pg_indexes WHERE schemaname = current_schema() AND indexname = 'idx_phantasi_items_source_recent'"))
        .await.unwrap().unwrap();
    assert!(
        index
            .try_get::<String>("", "indexdef")
            .unwrap()
            .contains("published_at DESC NULLS LAST, id DESC")
    );
    let github_indexes = db.query_all_raw(Statement::from_string(DatabaseBackend::Postgres,
        "SELECT indexdef FROM pg_indexes WHERE schemaname = current_schema() AND tablename = 'users' AND indexdef LIKE '%(github_id)%'"))
        .await.unwrap();
    assert_eq!(github_indexes.len(), 1, "only the UNIQUE constraint index may cover users.github_id");
    assert!(
        github_indexes[0]
            .try_get::<String>("", "indexdef")
            .unwrap()
            .starts_with("CREATE UNIQUE INDEX")
    );
    let keys = db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres,
        "SELECT url_key, site_url_key FROM phantasi_sources WHERE name = 'url-key-upgrade-test'"))
        .await.unwrap().unwrap();
    assert_eq!(keys.try_get::<String>("", "url_key").unwrap(), "https://blog.example/rss");
    assert_eq!(keys.try_get::<String>("", "site_url_key").unwrap(), "https://blog.example");
    db.execute_unprepared("DELETE FROM phantasi_sources WHERE name = 'url-key-upgrade-test'")
        .await
        .unwrap();
    db.execute_unprepared("DELETE FROM tapps WHERE tapp_id = 'projection-upgrade-test'")
        .await
        .unwrap();

    for table in ["federation_delivery_queue", "federation_timeline"] {
        let rows = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                format!("SELECT id FROM {table} WHERE id IN (-2, -1)"),
            ))
            .await
            .expect("read deduplicated rows");
        assert_eq!(rows.len(), 1, "{table} must retain one row");
        assert_eq!(rows[0].try_get::<i32>("", "id").unwrap(), -2);
    }

    // Statement triggers also reject DELETEs that would affect zero rows.
    db.execute_unprepared(
        r#"
CREATE FUNCTION reject_schema_dedup_delete() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'valid unique indexes must skip dedup DELETE';
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER reject_schema_dedup_delete BEFORE DELETE ON federation_delivery_queue
    FOR EACH STATEMENT EXECUTE FUNCTION reject_schema_dedup_delete();
CREATE TRIGGER reject_schema_dedup_delete BEFORE DELETE ON federation_timeline
    FOR EACH STATEMENT EXECUTE FUNCTION reject_schema_dedup_delete();
"#,
    )
    .await
    .expect("guard normal startup against unconditional deduplication");
    ensure_schema(&db)
        .await
        .expect("repeated startup must not issue dedup DELETE when unique indexes exist");
    db.execute_unprepared(
        r#"
DROP TRIGGER reject_schema_dedup_delete ON federation_delivery_queue;
DROP TRIGGER reject_schema_dedup_delete ON federation_timeline;
DROP FUNCTION reject_schema_dedup_delete();
DELETE FROM federation_delivery_queue WHERE id = -2;
DELETE FROM federation_timeline WHERE id = -2;
DELETE FROM users WHERE id = 2147483647;
"#,
    )
    .await
    .expect("remove deduplication test fixtures");
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

    // `ROLLBACK TO SAVEPOINT` must drop post-savepoint writes in this txn.
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
    assert!(
        waiting
            .try_get::<Option<Uuid>>("", "lease_token")
            .unwrap()
            .is_none()
    );

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

    // 单靠次数不能撤销。worker 每 tick `LIMIT 1`，次数闸门必须配 streak 窗口。
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
    assert!(
        crate::federation::delivery::settle_remote_delivery_success(
            &outer,
            7,
            Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
            "other.example",
        )
        .await
        .expect("settle successful remote delivery")
    );
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

/// Federation FK heal against real PostgreSQL (report-only unless
/// `MYRIAD_FEDERATION_APPLY_FKS` is set): the set-based catalog + orphan
/// queries must run cleanly and be repeatable.
#[tokio::test]
async fn federation_fk_heal_runs_against_real_catalog() {
    let Ok(url) = std::env::var("MYRIAD_SCHEMA_DRIFT_DB") else {
        eprintln!("skipping: set MYRIAD_SCHEMA_DRIFT_DB to run the federation FK heal");
        return;
    };

    let db = sea_orm::Database::connect(&url)
        .await
        .expect("connect to the drift-check database");
    crate::db::Migrator::up(&db, None)
        .await
        .expect("migrations must apply");
    for _ in 0..2 {
        super::ensure_heals::ensure_federation_foreign_keys(&db)
            .await
            .expect("federation FK heal must succeed");
    }
}

/// 历史非帖子行被清掉、旧 Update 行并回原帖；其余行不动，重复执行无副作用。
#[tokio::test]
async fn timeline_heal_keeps_only_posts() {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
        return;
    };
    let db = &fixture.db;
    db.execute_unprepared(
        r#"
        INSERT INTO users (id, username) VALUES (2, 'bob'), (3, 'carol');
        INSERT INTO federation_remote_actors (id, actor_url, domain, inbox_url) VALUES
            (21, 'https://r.example/users/amy', 'r.example', 'https://r.example/users/amy/inbox'),
            (22, 'https://r.example/users/eve', 'r.example', 'https://r.example/users/eve/inbox');
        INSERT INTO federation_timeline
            (user_id, activity_id, remote_actor_id, activity_type, object_type,
             content_preview, content_json, received_at) VALUES
            (2, 'https://r/c1', 21, 'Create', 'Note', 'v1',
             '{"id": "https://r/n/1", "content": "v1"}', '2026-01-01T00:00:00Z'),
            (2, 'https://r/u1', 21, 'Update', 'Note', 'v2',
             '{"id": "https://r/n/1", "content": "v2"}', '2026-01-02T00:00:00Z'),
            (2, 'https://r/u2', 21, 'Update', 'Note', 'v3',
             '{"id": "https://r/n/1", "content": "v3"}', '2026-01-03T00:00:00Z'),
            (3, 'https://r/c1', 21, 'Create', 'Note', 'v1',
             '{"id": "https://r/n/1", "content": "v1"}', '2026-01-01T00:00:00Z'),
            (2, 'https://r/u3', 22, 'Update', 'Note', 'forged',
             '{"id": "https://r/n/1", "content": "forged"}', '2026-01-04T00:00:00Z'),
            (2, 'https://r/u4', 21, 'Update', 'Note', 'orphan',
             '{"id": "https://r/n/2", "content": "orphan"}', '2026-01-04T00:00:00Z'),
            (2, 'https://r/p1', 21, 'Update', 'Person', NULL,
             '{"id": "https://r.example/users/amy", "type": "Person"}', NOW()),
            (2, 'https://r/d1', 21, 'Delete', NULL, NULL, '"https://r/n/9"', NOW()),
            (2, 'https://r/x1', 21, 'Undo', 'Announce', NULL, '{"type": "Announce"}', NOW()),
            (2, 'https://r/l1', 21, 'Like', NULL, NULL, '"https://r/n/1"', NOW()),
            (2, 'https://r/a1', 21, 'Announce', 'Note', 'boost',
             '{"id": "https://r/n/5"}', NOW());
        "#,
    )
    .await
    .unwrap();

    let rows = || async {
        let mut rows: Vec<(i32, String, String)> = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT user_id, activity_id, COALESCE(content_preview, '') AS preview \
                 FROM federation_timeline ORDER BY user_id, activity_id",
            ))
            .await
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row.try_get("", "user_id").unwrap(),
                    row.try_get("", "activity_id").unwrap(),
                    row.try_get("", "preview").unwrap(),
                )
            })
            .collect();
        rows.sort();
        rows
    };
    let expected = vec![
        (2, "https://r/a1".to_string(), "boost".to_string()),
        // 最新一条 Update 并进 Create 行。
        (2, "https://r/c1".to_string(), "v3".to_string()),
        // 另一个 Actor 投来的同 id 行不算这篇的编辑，保留原样。
        (2, "https://r/u3".to_string(), "forged".to_string()),
        // 没有 Create 行的 Update 是唯一副本，保留。
        (2, "https://r/u4".to_string(), "orphan".to_string()),
        // carol 没有 Update 行，不受影响。
        (3, "https://r/c1".to_string(), "v1".to_string()),
    ];
    for _ in 0..2 {
        super::ensure_heals::ensure_timeline_posts_only(db)
            .await
            .expect("timeline heal must succeed");
        assert_eq!(rows().await, expected);
    }
    let merged: String = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT content_json->>'content' AS c FROM federation_timeline \
             WHERE user_id = 2 AND activity_id = 'https://r/c1'",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "c")
        .unwrap();
    assert_eq!(merged, "v3");

    fixture.close().await;
}

/// 修复前的半撤回转发：已撤回的删掉剩下一半，没撤回的补回缺的一半，残留时间线行
/// 清掉；健康转发与纯 Announce 不动，不写任何活动与投递，重复执行无副作用。
#[tokio::test]
async fn repost_heal_reconciles_half_withdrawn_reposts() {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
        return;
    };
    let db = &fixture.db;
    // 与 announce_object 同形的转发 Create：活动 id https://h/act/{k}，
    // Note id https://h/notes/repost_{k}，引用 {quoted}。
    let create = |key: &str, quoted: &str| {
        format!(
            r#"('https://h/act/{key}', 1, 'Create', 'repost',
               '{{"type": "Create", "id": "https://h/act/{key}", "object": {{
                   "type": "Note", "id": "https://h/notes/repost_{key}",
                   "mfp:kind": "repost", "mfp:contentId": "repost_{key}",
                   "mfp:quotedObjectId": "{quoted}", "quoteUrl": "{quoted}"}}}}',
               true, '2026-01-01T00:00:00Z')"#
        )
    };
    let activities = [
        create("ok", "https://r/n/ok"),
        create("a1", "https://r/n/a1"),
        create("a2", "https://r/n/a2"),
        create("a3", "https://r/n/shared"),
        create("ok2", "https://r/n/shared"),
        create("b1", "https://r/n/b1"),
        create("b2", "https://r/n/b2"),
        create("t1", "https://r/n/t1"),
        // 旧的取消转发：Delete 转发 Note。
        r#"('https://h/act/del-a1', 1, 'Delete', NULL,
            '{"type": "Delete", "object": "https://h/notes/repost_a1"}', true, NOW())"#
            .to_string(),
        // 旧的撤回发布：Delete 原 Create 活动 id。
        r#"('https://h/act/del-b1', 1, 'Delete', 'repost',
            '{"type": "Delete", "object": "https://h/act/b1"}', true, NOW())"#
            .to_string(),
        r#"('https://h/act/ann', 1, 'Announce', NULL,
            '{"type": "Announce", "object": "https://r/n/ann"}', true, NOW())"#
            .to_string(),
    ];
    db.execute_unprepared(&format!(
        r#"
        INSERT INTO users (id, username) VALUES (1, 'alice'), (2, 'bob');
        INSERT INTO federation_activities
            (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
        VALUES {};
        INSERT INTO federation_object_interactions (user_id, object_id, kind, activity_id) VALUES
            (1, 'https://r/n/ok', 'announce', 'https://h/act/ok'),
            (1, 'https://r/n/shared', 'announce', 'https://h/act/ok2'),
            (1, 'https://r/n/b1', 'announce', 'https://h/act/b1'),
            (1, 'https://r/n/b2', 'announce', 'https://h/act/b2'),
            (1, 'https://r/n/ann', 'announce', 'https://h/act/ann');
        INSERT INTO federation_published_content
            (user_id, content_type, content_id, activity_id, visibility, published_at) VALUES
            (1, 'repost', 'repost_ok', 'https://h/act/ok', 'public', '2026-01-01T00:00:00Z'),
            (1, 'repost', 'repost_ok2', 'https://h/act/ok2', 'public', '2026-01-01T00:00:00Z'),
            (1, 'repost', 'repost_a1', 'https://h/act/a1', 'public', '2026-01-01T00:00:00Z'),
            (1, 'repost', 'repost_a2', 'https://h/act/a2', 'public', '2026-01-01T00:00:00Z'),
            (1, 'repost', 'repost_a3', 'https://h/act/a3', 'public', '2026-01-01T00:00:00Z');
        INSERT INTO federation_timeline (user_id, activity_id, activity_type, object_type) VALUES
            (1, 'https://h/act/ok', 'Create', 'repost'),
            (2, 'https://h/act/ok', 'Create', 'repost'),
            (1, 'https://h/act/t1', 'Create', 'repost'),
            (2, 'https://h/act/t1', 'Create', 'repost');
        "#,
        activities.join(",\n")
    ))
    .await
    .unwrap();

    let pairs = |sql: &'static str| async move {
        let mut rows: Vec<(String, String)> = db
            .query_all_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
            .await
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row.try_get_by_index(0).unwrap(),
                    row.try_get_by_index(1).unwrap(),
                )
            })
            .collect();
        rows.sort();
        rows
    };
    let s = |a: &str, b: &str| (a.to_string(), b.to_string());
    let count = |sql: &'static str| async move {
        db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
            .await
            .unwrap()
            .unwrap()
            .try_get_by_index::<i64>(0)
            .unwrap()
    };
    for _ in 0..2 {
        super::ensure_heals::ensure_repost_state_consistent(db)
            .await
            .expect("repost heal must succeed");
        assert_eq!(
            pairs("SELECT activity_id, object_id FROM federation_object_interactions").await,
            vec![
                // a2 没撤回过：补回标记。
                s("https://h/act/a2", "https://r/n/a2"),
                s("https://h/act/ann", "https://r/n/ann"),
                // b1 已有 Delete：标记删掉；b2 没撤回：标记保留并补回已发布行。
                s("https://h/act/b2", "https://r/n/b2"),
                s("https://h/act/ok", "https://r/n/ok"),
                // a3 与 ok2 引用同一对象，唯一约束下不补 a3 的标记。
                s("https://h/act/ok2", "https://r/n/shared"),
            ]
        );
        assert_eq!(
            pairs("SELECT activity_id, content_id FROM federation_published_content").await,
            vec![
                // a1 已有 Delete：已发布行删掉。
                s("https://h/act/a2", "repost_a2"),
                s("https://h/act/a3", "repost_a3"),
                s("https://h/act/b2", "repost_b2"),
                s("https://h/act/ok", "repost_ok"),
                s("https://h/act/ok2", "repost_ok2"),
            ]
        );
        assert_eq!(
            pairs("SELECT user_id::text, activity_id FROM federation_timeline").await,
            vec![s("1", "https://h/act/ok"), s("2", "https://h/act/ok")]
        );
        assert_eq!(count("SELECT COUNT(*) FROM federation_activities").await, 11);
        assert_eq!(
            count("SELECT COUNT(*) FROM federation_delivery_queue").await,
            0
        );
    }

    fixture.close().await;
}

/// 修复前写入的自治授权去掉管理员专属权限，保持顺序；修复后写入的行、
/// 本来就只含候选权限的行都不动；重复运行不再改任何行。
#[tokio::test]
async fn legacy_autonomy_grants_lose_admin_only_permissions_once() {
    let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
        return;
    };
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let mut options = sea_orm::ConnectOptions::new(url);
    options.max_connections(1).sqlx_logging(false);
    let db = sea_orm::Database::connect(options).await.unwrap();
    db.execute_unprepared(&format!(
        r#"CREATE TEMP TABLE agent_autonomy_grants (
            user_id INTEGER PRIMARY KEY, allowed_permissions JSONB NOT NULL DEFAULT '[]'::jsonb,
            revoked BOOLEAN NOT NULL DEFAULT false,
            created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL);
        INSERT INTO agent_autonomy_grants VALUES
            (1, '["http:fetch","system:admin","scheduler:write","phantasi:admin"]', false,
             '{cutoff}'::timestamptz - INTERVAL '1 day', '{cutoff}'::timestamptz - INTERVAL '1 day'),
            (2, '["http:fetch"]', false,
             '{cutoff}'::timestamptz - INTERVAL '1 day', '{cutoff}'::timestamptz - INTERVAL '1 day'),
            (3, '["system:admin"]', false,
             '{cutoff}'::timestamptz + INTERVAL '1 hour', '{cutoff}'::timestamptz + INTERVAL '1 hour');"#,
        cutoff = super::ensure_heals::AUTONOMY_EMPTY_GRANT_FIX_AT
    ))
    .await
    .unwrap();

    let read = |user_id: i32| {
        let db = &db;
        async move {
            let row = db
                .query_one_raw(Statement::from_string(
                    DatabaseBackend::Postgres,
                    format!(
                        "SELECT allowed_permissions::text AS p, updated_at::text AS u \
                         FROM agent_autonomy_grants WHERE user_id = {user_id}"
                    ),
                ))
                .await
                .unwrap()
                .unwrap();
            (
                row.try_get::<String>("", "p").unwrap(),
                row.try_get::<String>("", "u").unwrap(),
            )
        }
    };
    let untouched_before = read(2).await;

    super::ensure_heals::narrow_legacy_autonomy_grants(&db)
        .await
        .unwrap();
    let narrowed = read(1).await;
    assert_eq!(narrowed.0, r#"["http:fetch", "scheduler:write"]"#);
    assert_eq!(read(2).await, untouched_before);
    assert_eq!(read(3).await.0, r#"["system:admin"]"#);

    super::ensure_heals::narrow_legacy_autonomy_grants(&db)
        .await
        .unwrap();
    assert_eq!(read(1).await, narrowed, "a second run changes nothing");
}

/// 旧日记里的事实复制进统一记忆：被更正的保持失效，原行保留，重复运行不重复写。
#[tokio::test]
async fn diary_facts_move_into_unified_memory_once() {
    let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
        return;
    };
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let mut options = sea_orm::ConnectOptions::new(url);
    options.max_connections(1).sqlx_logging(false);
    let db = sea_orm::Database::connect(options).await.unwrap();
    db.execute_unprepared(
        &super::ensure_heals::AGENT_MEMORIES_DDL
            .replace("CREATE TABLE IF NOT EXISTS", "CREATE TEMP TABLE")
            .replace("REFERENCES users(id) ON DELETE CASCADE", ""),
    )
    .await
    .unwrap();
    db.execute_unprepared(
        r#"CREATE TEMP TABLE agent_diary (id VARCHAR(64) PRIMARY KEY, user_id INTEGER NOT NULL,
            content TEXT NOT NULL, source VARCHAR(16) NOT NULL, created_at TIMESTAMPTZ NOT NULL);
        INSERT INTO agent_diary VALUES
            ('a', 7, 'likes tea', 'remember', NOW() - INTERVAL '1 day'),
            ('b', 7, 'likes coffee', 'remember_retired', NOW() - INTERVAL '2 days'),
            ('c', 7, 'said hello', 'chat', NOW());"#,
    )
    .await
    .unwrap();
    for _ in 0..2 {
        super::ensure_heals::migrate_diary_facts_to_memories(&db)
            .await
            .unwrap();
    }
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id, content, invalid_reason, audience::text AS audience FROM agent_memories ORDER BY id"
                .to_string(),
        ))
        .await
        .unwrap();
    let summary: Vec<(String, String, Option<String>, String)> = rows
        .iter()
        .map(|row| {
            (
                row.try_get("", "id").unwrap(),
                row.try_get("", "content").unwrap(),
                row.try_get("", "invalid_reason").unwrap(),
                row.try_get("", "audience").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            ("diary_a".into(), "likes tea".into(), None, "[7]".into()),
            (
                "diary_b".into(),
                "likes coffee".into(),
                Some("superseded".into()),
                "[7]".into()
            ),
        ]
    );
    let diary_left = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*)::int AS n FROM agent_diary".to_string(),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i32>("", "n")
        .unwrap();
    assert_eq!(diary_left, 3, "the original rows stay for rollback");
}

/// A database from before the runtime registry was platform infrastructure
/// comes up with the platform names, its rows, indexes and user guard kept;
/// running the rename again changes nothing.
#[tokio::test]
async fn tapp_named_runtime_registry_is_renamed_in_place() {
    use sea_orm::{ConnectionTrait, Statement};
    // An isolated schema: safe on the shared test database.
    let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
        return;
    };
    let schema = crate::db::IsolatedSchema::migrated(&url, "registry_rename").await;
    let db = &schema.db;
    // Back to how an older database looks.
    db.execute_unprepared(
        "ALTER TABLE runtime_registry RENAME TO tapp_runtime_registry;
         ALTER TABLE runtime_mailbox RENAME TO tapp_runtime_mailbox;
         ALTER INDEX runtime_registry_pkey RENAME TO tapp_runtime_registry_pkey;
         ALTER INDEX idx_runtime_registry_subject RENAME TO idx_tapp_runtime_registry_subject;
         ALTER INDEX idx_runtime_registry_tapp RENAME TO idx_tapp_runtime_registry_tapp;
         ALTER INDEX idx_runtime_registry_runtime RENAME TO idx_tapp_runtime_registry_runtime;
         ALTER INDEX runtime_mailbox_pkey RENAME TO tapp_runtime_mailbox_pkey;
         ALTER INDEX idx_runtime_mailbox_recipient RENAME TO idx_tapp_runtime_mailbox_recipient;
         ALTER INDEX idx_runtime_mailbox_expiry RENAME TO idx_tapp_runtime_mailbox_expiry;
         ALTER SEQUENCE runtime_mailbox_message_id_seq RENAME TO tapp_runtime_mailbox_message_id_seq;
         ALTER TRIGGER trg_runtime_registry_subject_user ON tapp_runtime_registry
             RENAME TO trg_tapp_runtime_registry_subject_user;
         INSERT INTO tapp_runtime_registry (namespace, record_id, payload, expires_at)
             VALUES ('telegram_dm_session', 'k', '{\"kept\":true}', 9999999999);
         INSERT INTO tapp_runtime_mailbox (channel, runtime_id, payload, expires_at)
             VALUES ('ai_task', 'r', '{}', 9999999999);",
    )
    .await
    .unwrap();
    for _ in 0..2 {
        migration::rename_runtime_registry_if_needed(db).await.unwrap();
    }
    let names = |sql: &'static str| async move {
        db.query_all_raw(Statement::from_string(db.get_database_backend(), sql))
            .await
            .unwrap()
            .iter()
            .map(|row| row.try_get::<String>("", "name").unwrap())
            .collect::<Vec<_>>()
    };
    let tables = names(
        "SELECT table_name::text AS name FROM information_schema.tables \
         WHERE table_schema = current_schema() AND table_name LIKE '%runtime_%' ORDER BY 1",
    )
    .await;
    assert_eq!(tables, ["runtime_mailbox", "runtime_registry"]);
    let indexes = names(
        "SELECT indexname::text AS name FROM pg_indexes \
         WHERE schemaname = current_schema() AND indexname LIKE '%runtime_%' ORDER BY 1",
    )
    .await;
    assert!(indexes.iter().all(|name| !name.contains("tapp_runtime")), "{indexes:?}");
    assert_eq!(indexes.len(), 7, "{indexes:?}");
    let triggers = names(
        "SELECT tgname::text AS name FROM pg_trigger \
         WHERE tgrelid = to_regclass('runtime_registry') AND NOT tgisinternal",
    )
    .await;
    assert_eq!(triggers, ["trg_runtime_registry_subject_user"]);
    let kept =
        names("SELECT payload->>'kept' AS name FROM runtime_registry WHERE record_id = 'k'").await;
    assert_eq!(kept, ["true"]);
    db.execute_unprepared(
        "INSERT INTO runtime_mailbox (channel, runtime_id, payload, expires_at) VALUES ('ai_task', 'r', '{}', 1)",
    )
    .await
    .unwrap();
    schema.drop().await;
}

/// A database from before the AI cost ledger was platform infrastructure
/// comes up with the platform name; Tapp rows keep their `tapp_id`, site
/// rows lose the `__<source>__` stand-in, and new site rows need none.
#[tokio::test]
async fn tapp_named_ai_cost_ledger_is_renamed_in_place() {
    use sea_orm::{ConnectionTrait, Statement};
    // An isolated schema: safe on the shared test database.
    let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
        return;
    };
    let schema = crate::db::IsolatedSchema::migrated(&url, "ledger_rename").await;
    let db = &schema.db;
    db.execute_unprepared(
        "ALTER TABLE ai_cost_ledger RENAME TO tapp_ai_cost_ledger;
         ALTER INDEX ai_cost_ledger_pkey RENAME TO tapp_ai_cost_ledger_pkey;
         ALTER INDEX idx_ai_cost_subject_time RENAME TO idx_tapp_ai_cost_subject_time;
         ALTER INDEX idx_ai_cost_tapp_time RENAME TO idx_tapp_ai_cost_tapp_time;
         ALTER SEQUENCE ai_cost_ledger_id_seq RENAME TO tapp_ai_cost_ledger_id_seq;
         ALTER TRIGGER trg_ai_cost_ledger_subject_user ON tapp_ai_cost_ledger
             RENAME TO trg_tapp_ai_cost_ledger_subject_user;
         INSERT INTO tapp_ai_cost_ledger
             (subject_id, owner_id, tapp_id, task_id, source, operation, provider, model, status)
             VALUES (0, 0, 'com.example.app', 't', 'runtime', 'chat', 'p', 'm', 'ok'),
                    (0, 0, '__merope__', 't', 'merope', 'chat', 'p', 'm', 'ok');
         ALTER TABLE tapp_ai_cost_ledger ALTER COLUMN tapp_id SET NOT NULL;",
    )
    .await
    .unwrap();
    for _ in 0..2 {
        migration::rename_ai_cost_ledger_if_needed(db).await.unwrap();
    }
    let names = |sql: &'static str| async move {
        db.query_all_raw(Statement::from_string(db.get_database_backend(), sql))
            .await
            .unwrap()
            .iter()
            .map(|row| {
                row.try_get::<Option<String>>("", "name")
                    .unwrap()
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names("SELECT indexname::text AS name FROM pg_indexes WHERE schemaname = current_schema() AND indexname LIKE '%ai_cost%' ORDER BY 1").await,
        ["ai_cost_ledger_pkey", "idx_ai_cost_subject_time", "idx_ai_cost_tapp_time"]
    );
    assert_eq!(
        names("SELECT tgname::text AS name FROM pg_trigger WHERE tgrelid = to_regclass('ai_cost_ledger') AND NOT tgisinternal").await,
        ["trg_ai_cost_ledger_subject_user"]
    );
    assert_eq!(
        names("SELECT tapp_id AS name FROM ai_cost_ledger ORDER BY source DESC").await,
        ["com.example.app", ""]
    );
    db.execute_unprepared(
        "INSERT INTO ai_cost_ledger (subject_id, owner_id, task_id, source, operation, provider, model, status)
         VALUES (0, 0, 't', 'agent', 'chat', 'p', 'm', 'ok')",
    )
    .await
    .unwrap();
    schema.drop().await;
}

/// A database from before the daily AI quota was platform infrastructure
/// comes up with the platform names; a Tapp's counts stay under its id, the
/// site's own move from their stand-ins to `site:` scopes, counts kept.
#[tokio::test]
async fn tapp_named_ai_quota_is_renamed_in_place() {
    use sea_orm::{ConnectionTrait, Statement};
    // An isolated schema: safe on the shared test database.
    let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
        return;
    };
    let schema = crate::db::IsolatedSchema::migrated(&url, "quota_rename").await;
    let db = &schema.db;
    db.execute_unprepared(
        "ALTER TABLE ai_quota_usage RENAME TO tapp_quota_usage;
         ALTER TABLE tapp_quota_usage RENAME COLUMN scope TO tapp_id;
         ALTER INDEX ai_quota_usage_pkey RENAME TO tapp_quota_usage_pkey;
         ALTER INDEX idx_ai_quota_unique RENAME TO idx_tapp_quota_unique;
         ALTER SEQUENCE ai_quota_usage_id_seq RENAME TO tapp_quota_usage_id_seq;
         ALTER TRIGGER trg_ai_quota_usage_subject_user ON tapp_quota_usage
             RENAME TO trg_tapp_quota_usage_subject_user;
         INSERT INTO tapp_quota_usage (tapp_id, user_id, quota_type, used, \"limit\", period_start, period_end)
             VALUES ('com.example.app', -1, 'ai_calls', 3, 10, NOW(), NOW()),
                    ('__agent__', -1, 'ai_calls', 5, 10, NOW(), NOW()),
                    ('__anonymous_ai_site__', -1, 'ai_calls', 7, 10, NOW(), NOW());",
    )
    .await
    .unwrap();
    for _ in 0..2 {
        migration::rename_ai_quota_usage_if_needed(db).await.unwrap();
    }
    let names = |sql: &'static str| async move {
        db.query_all_raw(Statement::from_string(db.get_database_backend(), sql))
            .await
            .unwrap()
            .iter()
            .map(|row| row.try_get::<String>("", "name").unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names("SELECT indexname::text AS name FROM pg_indexes WHERE schemaname = current_schema() AND indexname LIKE '%quota%' ORDER BY 1").await,
        ["ai_quota_usage_pkey", "idx_ai_quota_unique"]
    );
    assert_eq!(
        names("SELECT tgname::text AS name FROM pg_trigger WHERE tgrelid = to_regclass('ai_quota_usage') AND NOT tgisinternal").await,
        ["trg_ai_quota_usage_subject_user"]
    );
    assert_eq!(
        names("SELECT scope || '=' || used AS name FROM ai_quota_usage ORDER BY used").await,
        ["com.example.app=3", "site:agent=5", "site:anonymous=7"]
    );
    schema.drop().await;
}
