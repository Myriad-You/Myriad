//! Subject-private Tapp sandbox storage (validators + DB IO).
//!
//! Domain helpers for scheduler / agent paths that must not import
//! `crate::api::tapp_store`. HTTP handlers map [`TappStorageError`] to status codes.

use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, FromQueryResult, Statement,
    TransactionTrait,
};
use serde_json::Value;

pub use myriad_tapp_contract::storage::{
    is_host_storage_key, is_reserved_storage_route_key, validate_sandbox_storage_key,
    validate_storage_key, HOST_STORAGE_KEY_PREFIXES,
};

/// Per-install soft quota for sandbox + host-managed keys combined.
pub const TAPP_STORAGE_QUOTA_BYTES: i64 = 8 * 1024 * 1024;

/// Domain errors for storage validation and IO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TappStorageError {
    InvalidKey(&'static str),
    Database,
    TooLarge,
}

impl TappStorageError {
    pub fn message(&self) -> String {
        match self {
            Self::InvalidKey(reason) => (*reason).to_string(),
            Self::Database => "Failed to update Tapp storage".to_string(),
            Self::TooLarge => "Storage value or quota exceeded".to_string(),
        }
    }
}

impl std::fmt::Display for TappStorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for TappStorageError {}

// Storage access identities (install owner vs subject)

/// Resolve the two storage identities attached to a Tapp runtime.
///
/// Sandbox storage belongs to the current subject. Installation settings,
/// shared data, and other host-managed resources remain attached to the
/// installation owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TappStorageAccess {
    pub owner_id: i32,
    pub subject_id: i32,
}

/// Domain errors for storage access resolution / installation writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TappStorageAccessError {
    /// No subject id on the request.
    Unauthenticated,
    /// Runtime grant subject does not match the authenticated subject.
    SubjectMismatch,
    /// Subject may read the install but cannot mutate host-managed resources.
    InstallationReadOnly,
}

impl TappStorageAccessError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unauthenticated => "TAPP_STORAGE_UNAUTHENTICATED",
            Self::SubjectMismatch => "TAPP_STORAGE_SUBJECT_MISMATCH",
            Self::InstallationReadOnly => "TAPP_INSTALLATION_READ_ONLY",
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::Unauthenticated => "Authentication required for storage access",
            Self::SubjectMismatch => "Invalid runtime grant subject",
            Self::InstallationReadOnly => "Only the installation owner can modify this resource",
        }
    }

    pub fn status_hint(&self) -> u16 {
        match self {
            Self::Unauthenticated => 401,
            Self::SubjectMismatch | Self::InstallationReadOnly => 403,
        }
    }
}

impl std::fmt::Display for TappStorageAccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for TappStorageAccessError {}

impl TappStorageAccess {
    pub fn from_owner_and_subject(owner_id: i32, subject_id: i32) -> Self {
        Self {
            owner_id,
            subject_id,
        }
    }

    /// Build access from a validated runtime grant + authenticated subject id.
    ///
    /// Missing subject → Unauthenticated; guest negative ids are valid subjects.
    pub fn from_grant_and_subject(
        grant_owner_id: i32,
        grant_subject_id: i32,
        claims_subject_id: Option<i32>,
    ) -> Result<Self, TappStorageAccessError> {
        let subject_id = claims_subject_id.ok_or(TappStorageAccessError::Unauthenticated)?;
        if grant_subject_id != subject_id {
            return Err(TappStorageAccessError::SubjectMismatch);
        }
        Ok(Self::from_owner_and_subject(grant_owner_id, subject_id))
    }

    pub fn can_manage_installation(self) -> bool {
        self.subject_id == self.owner_id
    }

    pub fn require_installation_write(self) -> Result<(), TappStorageAccessError> {
        if self.can_manage_installation() {
            Ok(())
        } else {
            Err(TappStorageAccessError::InstallationReadOnly)
        }
    }

    pub fn installation_namespace(self) -> i32 {
        self.owner_id
    }

    pub fn private_storage_namespace(self) -> i32 {
        self.subject_id
    }
}

/// Installation settings may be written by the install owner or a current admin.
pub fn can_write_installation_settings(access: TappStorageAccess, is_admin: bool) -> bool {
    is_admin || access.can_manage_installation()
}

pub fn validate_storage_value_size(value: &Value) -> Result<(), TappStorageError> {
    let size = serde_json::to_vec(value)
        .map_err(|_| TappStorageError::InvalidKey("Value is not serializable JSON"))?
        .len();
    if size > 1024 * 1024 {
        return Err(TappStorageError::TooLarge);
    }
    Ok(())
}

#[derive(FromQueryResult)]
struct StorageBytesRow {
    bytes: i64,
}

#[derive(Debug, FromQueryResult)]
pub struct SandboxStorageEntry {
    pub id: i32,
    pub key: String,
    pub value: Value,
    pub created_at: sea_orm::prelude::DateTimeWithTimeZone,
    pub updated_at: sea_orm::prelude::DateTimeWithTimeZone,
}

/// SQL-level boundary for subject-private sandbox storage. Queries using this
/// predicate never load host-managed records or their encrypted columns into
/// the generic storage response path.
const SANDBOX_STORAGE_PREDICATE_SQL: &str = r#"
key <> '_settings'
AND key <> '_private'
AND NOT starts_with(key, '_settings.')
AND NOT starts_with(key, '_credentials.')
AND NOT starts_with(key, '_shared.')
AND NOT starts_with(key, '_private.')
AND NOT starts_with(key, '_component:')
AND NOT starts_with(key, '_shortcut:')
AND NOT starts_with(key, '_report:')
"#;

pub async fn sandbox_storage_entries(
    db: &impl ConnectionTrait,
    user_id: i32,
    tapp_id: &str,
) -> Result<Vec<SandboxStorageEntry>, TappStorageError> {
    let sql = format!(
        "SELECT id, key, value, created_at, updated_at FROM tapp_storage \
         WHERE user_id = $1 AND tapp_id = $2 AND ({SANDBOX_STORAGE_PREDICATE_SQL}) \
         ORDER BY id"
    );
    SandboxStorageEntry::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        vec![user_id.into(), tapp_id.into()],
    ))
    .all(db)
    .await
    .map_err(|_| TappStorageError::Database)
}

pub async fn sandbox_storage_count(
    db: &impl ConnectionTrait,
    user_id: i32,
    tapp_id: &str,
) -> Result<u64, TappStorageError> {
    #[derive(FromQueryResult)]
    struct CountRow {
        count: i64,
    }
    let sql = format!(
        "SELECT COUNT(*)::BIGINT AS count FROM tapp_storage \
         WHERE user_id = $1 AND tapp_id = $2 AND ({SANDBOX_STORAGE_PREDICATE_SQL})"
    );
    CountRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        vec![user_id.into(), tapp_id.into()],
    ))
    .one(db)
    .await
    .map_err(|_| TappStorageError::Database)
    .map(|row| row.map_or(0, |row| row.count.max(0) as u64))
}

pub async fn clear_sandbox_storage(
    db: &impl ConnectionTrait,
    user_id: i32,
    tapp_id: &str,
) -> Result<(), TappStorageError> {
    let sql = format!(
        "DELETE FROM tapp_storage \
         WHERE user_id = $1 AND tapp_id = $2 AND ({SANDBOX_STORAGE_PREDICATE_SQL})"
    );
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        vec![user_id.into(), tapp_id.into()],
    ))
    .await
    .map_err(|_| TappStorageError::Database)?;
    Ok(())
}

pub async fn storage_bytes(
    db: &impl ConnectionTrait,
    user_id: i32,
    tapp_id: &str,
) -> Result<i64, TappStorageError> {
    StorageBytesRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"SELECT COALESCE(SUM(
               octet_length(key)
               + octet_length(value::text)
               + COALESCE(octet_length(encrypted_value), 0)
           ), 0)::BIGINT AS bytes
           FROM tapp_storage WHERE user_id = $1 AND tapp_id = $2"#,
        vec![user_id.into(), tapp_id.into()],
    ))
    .one(db)
    .await
    .map_err(|_| TappStorageError::Database)
    .map(|row| row.map_or(0, |row| row.bytes))
}

/// Load installation settings that a declared HTTP API may interpolate.
///
/// Only Manifest-declared keys are returned. Stored values win; otherwise the
/// declared `defaultValue` is used. Missing keys are omitted so templates stay
/// unresolved. This loader does not read `_credentials.` secrets.
pub async fn load_declared_setting_values(
    db: &DatabaseConnection,
    owner_id: i32,
    tapp_id: &str,
    declared: &[myriad_tapp_contract::manifest::TappSettingDef],
) -> Result<std::collections::BTreeMap<String, Value>, TappStorageError> {
    if declared.is_empty() {
        return Ok(std::collections::BTreeMap::new());
    }

    #[derive(FromQueryResult)]
    struct SettingRow {
        key: String,
        value: Value,
    }

    let stored = SettingRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
SELECT key, value
FROM tapp_storage
WHERE user_id = $1
  AND tapp_id = $2
  AND starts_with(key, '_settings.')
"#,
        vec![owner_id.into(), tapp_id.into()],
    ))
    .all(db)
    .await
    .map_err(|_| TappStorageError::Database)?;

    let mut stored_by_key = std::collections::BTreeMap::new();
    for row in stored {
        if let Some(key) = row.key.strip_prefix("_settings.") {
            stored_by_key.insert(key.to_string(), row.value);
        }
    }

    let mut values = std::collections::BTreeMap::new();
    for setting in declared {
        if let Some(value) = stored_by_key
            .get(&setting.key)
            .cloned()
            .or_else(|| setting.default_value.clone())
        {
            values.insert(setting.key.clone(), value);
        }
    }
    Ok(values)
}

pub async fn read_storage_value(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    key: &str,
) -> Result<Value, TappStorageError> {
    #[derive(FromQueryResult)]
    struct StorageValueRow {
        value: Value,
    }
    let item = StorageValueRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT value FROM tapp_storage WHERE user_id = $1 AND tapp_id = $2 AND key = $3",
        vec![user_id.into(), tapp_id.into(), key.into()],
    ))
    .one(db)
    .await
    .map_err(|_| TappStorageError::Database)?;
    Ok(item.map_or(Value::Null, |item| item.value))
}

pub async fn write_storage_value(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    key: &str,
    value: Value,
) -> Result<(), TappStorageError> {
    let txn = db.begin().await.map_err(|_| TappStorageError::Database)?;
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        vec![format!("tapp-storage:{user_id}:{tapp_id}").into()],
    ))
    .await
    .map_err(|_| TappStorageError::Database)?;

    #[derive(FromQueryResult)]
    struct ProjectedBytesRow {
        bytes: i64,
    }
    let projected = ProjectedBytesRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
SELECT (
    COALESCE(SUM(
        octet_length(key)
        + octet_length(value::text)
        + COALESCE(octet_length(encrypted_value), 0)
    )
        FILTER (WHERE key <> $3), 0)
    + octet_length($3)
    + octet_length($4::jsonb::text)
)::BIGINT AS bytes
FROM tapp_storage
WHERE user_id = $1 AND tapp_id = $2
"#,
        vec![
            user_id.into(),
            tapp_id.into(),
            key.into(),
            value.clone().into(),
        ],
    ))
    .one(&txn)
    .await
    .map_err(|_| TappStorageError::Database)?
    .map_or(i64::MAX, |row| row.bytes);
    if projected > TAPP_STORAGE_QUOTA_BYTES {
        txn.rollback().await.ok();
        return Err(TappStorageError::TooLarge);
    }
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
INSERT INTO tapp_storage (tapp_id, user_id, key, value, created_at, updated_at)
VALUES ($1, $2, $3, $4, NOW(), NOW())
ON CONFLICT (user_id, tapp_id, key) DO UPDATE SET
    value = EXCLUDED.value,
    updated_at = NOW()
"#,
        vec![tapp_id.into(), user_id.into(), key.into(), value.into()],
    ))
    .await
    .map_err(|_| TappStorageError::Database)?;
    txn.commit().await.map_err(|_| TappStorageError::Database)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        can_write_installation_settings, is_host_storage_key, validate_sandbox_storage_key,
        validate_storage_key, validate_storage_value_size, TappStorageAccess,
        TappStorageAccessError, SANDBOX_STORAGE_PREDICATE_SQL,
    };
    use serde_json::json;

    #[test]
    fn sandbox_keys_reject_host_prefixes() {
        for key in [
            "_settings",
            "_settings.theme",
            "_credentials.wegame",
            "_shared.posts",
            "_private",
            "_private.token",
            "_component:x",
            "_shortcut:y",
            "_report:z",
        ] {
            assert!(validate_sandbox_storage_key(key).is_err(), "{key}");
            assert!(is_host_storage_key(key));
        }
        assert!(validate_sandbox_storage_key("user.preferences").is_ok());
        assert!(validate_storage_key("user.preferences").is_ok());
    }

    #[test]
    fn sandbox_query_predicate_covers_every_host_storage_prefix() {
        assert!(SANDBOX_STORAGE_PREDICATE_SQL.contains("key <> '_settings'"));
        assert!(SANDBOX_STORAGE_PREDICATE_SQL.contains("key <> '_private'"));
        for prefix in super::HOST_STORAGE_KEY_PREFIXES {
            assert!(
                SANDBOX_STORAGE_PREDICATE_SQL.contains(&format!("starts_with(key, '{prefix}')")),
                "missing SQL exclusion for {prefix}"
            );
        }
        assert!(!SANDBOX_STORAGE_PREDICATE_SQL.contains("encrypted_value"));
    }

    #[test]
    fn sandbox_keys_reject_route_reserved_names() {
        use super::is_reserved_storage_route_key;
        assert!(is_reserved_storage_route_key("entries"));
        assert!(is_reserved_storage_route_key("usage"));
        assert!(!is_reserved_storage_route_key("entries.v1"));
        assert!(!is_reserved_storage_route_key("my-usage"));
        for key in ["entries", "usage"] {
            let err = validate_sandbox_storage_key(key).expect_err(key);
            assert!(
                err.to_ascii_lowercase().contains("reserved"),
                "key={key} err={err}"
            );
        }
        assert!(validate_sandbox_storage_key("entries.v1").is_ok());
        assert!(validate_sandbox_storage_key("usage_stats").is_ok());
    }

    #[test]
    fn value_size_rejects_over_1mib() {
        let big = json!("x".repeat(1024 * 1024 + 8));
        assert!(validate_storage_value_size(&big).is_err());
        assert!(validate_storage_value_size(&json!({"ok": true})).is_ok());
    }

    #[tokio::test]
    async fn postgres_sandbox_queries_exclude_and_preserve_host_rows() {
        let Ok(url) = std::env::var("MYRIAD_TAPP_STORAGE_GUARD_DB") else {
            eprintln!("skipping: set MYRIAD_TAPP_STORAGE_GUARD_DB for the PostgreSQL guard test");
            return;
        };
        use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement};

        let db = Database::connect(&url)
            .await
            .expect("connect guard database");
        let user_id = 2_147_483_600_i32;
        let tapp_id = "codex.storage.guard";
        let delete_rows = || {
            Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "DELETE FROM tapp_storage WHERE user_id = $1 AND tapp_id = $2",
                vec![user_id.into(), tapp_id.into()],
            )
        };
        db.execute_raw(delete_rows())
            .await
            .expect("clean guard rows");
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
INSERT INTO tapp_storage
    (user_id, tapp_id, key, value, encrypted_value, binding_fingerprint, created_at, updated_at)
VALUES
    ($1, $2, 'ordinary.one', '{"visible":1}'::jsonb, NULL, NULL, NOW(), NOW()),
    ($1, $2, 'ordinary.two', '{"visible":2}'::jsonb, NULL, NULL, NOW(), NOW()),
    ($1, $2, '_settings.theme', '"dark"'::jsonb, NULL, NULL, NOW(), NOW()),
    ($1, $2, '_shared.posts', '[]'::jsonb, NULL, NULL, NOW(), NOW()),
    ($1, $2, '_private.token', '"owner-only"'::jsonb, NULL, NULL, NOW(), NOW()),
    ($1, $2, '_credentials.api', '{"kind":"credential","version":1}'::jsonb,
        'ciphertext-must-stay-host-only', $3, NOW(), NOW())
"#,
            vec![user_id.into(), tapp_id.into(), "f".repeat(64).into()],
        ))
        .await
        .expect("insert guard rows");

        let entries = super::sandbox_storage_entries(&db, user_id, tapp_id)
            .await
            .expect("list sandbox rows");
        let keys: Vec<_> = entries.iter().map(|entry| entry.key.as_str()).collect();
        assert_eq!(keys, vec!["ordinary.one", "ordinary.two"]);
        assert_eq!(
            super::sandbox_storage_count(&db, user_id, tapp_id)
                .await
                .expect("count sandbox rows"),
            2
        );

        super::clear_sandbox_storage(&db, user_id, tapp_id)
            .await
            .expect("clear sandbox rows");
        let remaining = db
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT key FROM tapp_storage WHERE user_id = $1 AND tapp_id = $2 ORDER BY key",
                vec![user_id.into(), tapp_id.into()],
            ))
            .await
            .expect("read remaining host rows");
        let remaining: Vec<String> = remaining
            .into_iter()
            .map(|row| row.try_get("", "key").expect("key"))
            .collect();
        assert_eq!(
            remaining,
            vec![
                "_credentials.api",
                "_private.token",
                "_settings.theme",
                "_shared.posts",
            ]
        );

        db.execute_raw(delete_rows())
            .await
            .expect("remove guard rows");
    }

    #[test]
    fn private_storage_follows_subject_while_settings_follow_installation() {
        let viewer_of_admin = TappStorageAccess::from_owner_and_subject(1, 42);
        let second_viewer = TappStorageAccess::from_owner_and_subject(1, 43);
        assert_eq!(viewer_of_admin.private_storage_namespace(), 42);
        assert_eq!(second_viewer.private_storage_namespace(), 43);
        assert_eq!(viewer_of_admin.installation_namespace(), 1);
        assert_eq!(second_viewer.installation_namespace(), 1);
        assert!(!viewer_of_admin.can_manage_installation());
        assert!(viewer_of_admin.require_installation_write().is_err());

        let private_owner = TappStorageAccess::from_owner_and_subject(42, 42);
        assert_eq!(private_owner.private_storage_namespace(), 42);
        assert_eq!(private_owner.installation_namespace(), 42);
        assert!(private_owner.can_manage_installation());
        assert!(private_owner.require_installation_write().is_ok());

        // Tapp.storage → private_storage_namespace (subject).
        // Tapp.private / shared / settings → installation_namespace (owner).
        // Visitor Tapp.storage namespace ≠ owner installation namespace.
        assert_ne!(
            viewer_of_admin.private_storage_namespace(),
            viewer_of_admin.installation_namespace()
        );
        assert!(can_write_installation_settings(private_owner, false));
        assert!(!can_write_installation_settings(viewer_of_admin, false));

        let site_owner = TappStorageAccess::from_owner_and_subject(1, 1);
        assert_eq!(site_owner.private_storage_namespace(), 1);
        assert_eq!(site_owner.installation_namespace(), 1);
        assert!(site_owner.can_manage_installation());
    }

    #[test]
    fn installation_settings_allow_owner_or_current_admin_only() {
        let public_viewer = TappStorageAccess::from_owner_and_subject(1, 42);
        assert!(!can_write_installation_settings(public_viewer, false));
        assert!(can_write_installation_settings(public_viewer, true));

        let private_owner = TappStorageAccess::from_owner_and_subject(42, 42);
        assert!(can_write_installation_settings(private_owner, false));
    }

    #[test]
    fn grant_and_subject_resolution_rejects_mismatch() {
        assert_eq!(
            TappStorageAccess::from_grant_and_subject(1, 42, None).unwrap_err(),
            TappStorageAccessError::Unauthenticated
        );
        assert_eq!(
            TappStorageAccess::from_grant_and_subject(1, 42, Some(99)).unwrap_err(),
            TappStorageAccessError::SubjectMismatch
        );
        let access = TappStorageAccess::from_grant_and_subject(1, 42, Some(42)).unwrap();
        assert_eq!(access.installation_namespace(), 1);
        assert_eq!(access.private_storage_namespace(), 42);
    }

    #[test]
    fn installation_read_only_error_code() {
        assert_eq!(
            TappStorageAccessError::InstallationReadOnly.code(),
            "TAPP_INSTALLATION_READ_ONLY"
        );
        assert_eq!(
            TappStorageAccessError::InstallationReadOnly.status_hint(),
            403
        );
    }
}
