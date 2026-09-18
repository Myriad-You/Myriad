//! PostgreSQL-backed leases and mailboxes shared by every Myriad backend replica.
//!
//! Workspace crate so `services` (agent, scheduler) and `api::tapp_runtime` share one
//! registry implementation **without** services depending on the HTTP API layer.
//!
//! The process-global DB handle lives in the backend binary; callers inject
//! `&DatabaseConnection` / `ConnectionTrait`. Use the backend adapter
//! `services::tapp_registry::database()` when a global handle is needed.

use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, FromQueryResult, Statement,
    TransactionTrait,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug, Clone, FromQueryResult)]
pub struct RegistryRow {
    pub record_id: String,
    pub runtime_id: Option<String>,
    pub payload: Value,
}

static CLEANUP_COUNTER: AtomicU32 = AtomicU32::new(0);

pub async fn maybe_cleanup(db: &impl ConnectionTrait) {
    if CLEANUP_COUNTER
        .fetch_add(1, Ordering::Relaxed)
        .is_multiple_of(256)
        && let Err(error) = cleanup(db).await
    {
        tracing::warn!(%error, "[TAPP] Shared registry cleanup failed");
    }
}

#[derive(Debug, Clone)]
pub struct RegistryIdentity<'a> {
    pub subject_id: Option<i32>,
    pub owner_id: Option<i32>,
    pub tapp_id: Option<&'a str>,
    pub runtime_id: Option<&'a str>,
}

pub async fn put<T: Serialize>(
    db: &impl ConnectionTrait,
    namespace: &str,
    record_id: &str,
    identity: RegistryIdentity<'_>,
    payload: &T,
    expires_at: i64,
) -> Result<(), DbErr> {
    let payload = serde_json::to_value(payload).map_err(|error| DbErr::Json(error.to_string()))?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
INSERT INTO tapp_runtime_registry
    (namespace, record_id, subject_id, owner_id, tapp_id, runtime_id, payload, expires_at, updated_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())
ON CONFLICT (namespace, record_id) DO UPDATE SET
    subject_id = EXCLUDED.subject_id,
    owner_id = EXCLUDED.owner_id,
    tapp_id = EXCLUDED.tapp_id,
    runtime_id = EXCLUDED.runtime_id,
    payload = EXCLUDED.payload,
    expires_at = EXCLUDED.expires_at,
    updated_at = NOW()
"#,
        vec![
            namespace.into(),
            record_id.into(),
            identity.subject_id.into(),
            identity.owner_id.into(),
            identity.tapp_id.map(str::to_string).into(),
            identity.runtime_id.map(str::to_string).into(),
            payload.into(),
            expires_at.into(),
        ],
    ))
    .await?;
    maybe_cleanup(db).await;
    Ok(())
}

/// Insert a registry row only if the id is absent or the existing row is expired.
///
/// Returns `true` when this caller now owns the id. `false` means a live row
/// already occupies it (used for one-time inbound nonces).
pub async fn put_if_absent<T: Serialize>(
    db: &impl ConnectionTrait,
    namespace: &str,
    record_id: &str,
    identity: RegistryIdentity<'_>,
    payload: &T,
    expires_at: i64,
) -> Result<bool, DbErr> {
    let payload = serde_json::to_value(payload).map_err(|error| DbErr::Json(error.to_string()))?;
    #[derive(FromQueryResult)]
    struct InsertedRow {
        #[allow(dead_code)]
        record_id: String,
    }
    let inserted = InsertedRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
INSERT INTO tapp_runtime_registry
    (namespace, record_id, subject_id, owner_id, tapp_id, runtime_id, payload, expires_at, updated_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())
ON CONFLICT (namespace, record_id) DO UPDATE SET
    subject_id = EXCLUDED.subject_id,
    owner_id = EXCLUDED.owner_id,
    tapp_id = EXCLUDED.tapp_id,
    runtime_id = EXCLUDED.runtime_id,
    payload = EXCLUDED.payload,
    expires_at = EXCLUDED.expires_at,
    updated_at = NOW()
WHERE tapp_runtime_registry.expires_at < EXTRACT(EPOCH FROM NOW())::BIGINT
RETURNING record_id
"#,
        vec![
            namespace.into(),
            record_id.into(),
            identity.subject_id.into(),
            identity.owner_id.into(),
            identity.tapp_id.map(str::to_string).into(),
            identity.runtime_id.map(str::to_string).into(),
            payload.into(),
            expires_at.into(),
        ],
    ))
    .one(db)
    .await?;
    maybe_cleanup(db).await;
    Ok(inserted.is_some())
}

/// Insert with a per-subject live-record limit on an already-open transaction.
///
/// The caller must hold the transaction that should commit or roll back with
/// any other durable mutation (for example consuming a prepared request).
pub async fn put_with_subject_limit_on<T: Serialize>(
    db: &impl ConnectionTrait,
    namespace: &str,
    record_id: &str,
    identity: RegistryIdentity<'_>,
    payload: &T,
    expires_at: i64,
    max_records: usize,
) -> Result<bool, DbErr> {
    let subject_id = identity
        .subject_id
        .ok_or_else(|| DbErr::Custom("subject_id is required for a registry limit".to_string()))?;
    let payload = serde_json::to_value(payload).map_err(|error| DbErr::Json(error.to_string()))?;
    let lock_key = format!("tapp_registry:{namespace}:{subject_id}");
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        vec![lock_key.into()],
    ))
    .await?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM tapp_runtime_registry WHERE namespace = $1 AND subject_id = $2 AND expires_at <= EXTRACT(EPOCH FROM NOW())::BIGINT",
        vec![namespace.into(), subject_id.into()],
    ))
    .await?;

    #[derive(FromQueryResult)]
    struct CountRow {
        count: i64,
    }
    let count = CountRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT COUNT(*)::BIGINT AS count FROM tapp_runtime_registry WHERE namespace = $1 AND subject_id = $2 AND record_id <> $3 AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT",
        vec![namespace.into(), subject_id.into(), record_id.into()],
    ))
    .one(db)
    .await?
    .map_or(0, |row| row.count);
    if count >= max_records as i64 {
        return Ok(false);
    }

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
INSERT INTO tapp_runtime_registry
    (namespace, record_id, subject_id, owner_id, tapp_id, runtime_id, payload, expires_at, updated_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())
ON CONFLICT (namespace, record_id) DO UPDATE SET
    subject_id = EXCLUDED.subject_id,
    owner_id = EXCLUDED.owner_id,
    tapp_id = EXCLUDED.tapp_id,
    runtime_id = EXCLUDED.runtime_id,
    payload = EXCLUDED.payload,
    expires_at = EXCLUDED.expires_at,
    updated_at = NOW()
"#,
        vec![
            namespace.into(),
            record_id.into(),
            identity.subject_id.into(),
            identity.owner_id.into(),
            identity.tapp_id.map(str::to_string).into(),
            identity.runtime_id.map(str::to_string).into(),
            payload.into(),
            expires_at.into(),
        ],
    ))
    .await?;
    Ok(true)
}

pub async fn put_with_subject_limit<T: Serialize>(
    db: &DatabaseConnection,
    namespace: &str,
    record_id: &str,
    identity: RegistryIdentity<'_>,
    payload: &T,
    expires_at: i64,
    max_records: usize,
) -> Result<bool, DbErr> {
    let transaction = db.begin().await?;
    let inserted = put_with_subject_limit_on(
        &transaction,
        namespace,
        record_id,
        identity,
        payload,
        expires_at,
        max_records,
    )
    .await?;
    if inserted {
        transaction.commit().await?;
        maybe_cleanup(db).await;
        Ok(true)
    } else {
        transaction.rollback().await?;
        Ok(false)
    }
}

/// Extend TTL for a still-live record of this subject. Missing/expired rows
/// return false so the caller can run full admission.
pub async fn touch_live(
    db: &impl ConnectionTrait,
    namespace: &str,
    record_id: &str,
    subject_id: i32,
    expires_at: i64,
) -> Result<bool, DbErr> {
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE tapp_runtime_registry
SET expires_at = $4, updated_at = NOW()
WHERE namespace = $1
  AND record_id = $2
  AND subject_id = $3
  AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT
"#,
            vec![
                namespace.into(),
                record_id.into(),
                subject_id.into(),
                expires_at.into(),
            ],
        ))
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn get<T: DeserializeOwned>(
    db: &impl ConnectionTrait,
    namespace: &str,
    record_id: &str,
) -> Result<Option<T>, DbErr> {
    #[derive(FromQueryResult)]
    struct PayloadRow {
        payload: Value,
    }
    let row = PayloadRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT payload FROM tapp_runtime_registry WHERE namespace = $1 AND record_id = $2 AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT",
        vec![namespace.into(), record_id.into()],
    ))
    .one(db)
    .await?;
    row.map(|row| {
        serde_json::from_value(row.payload).map_err(|error| DbErr::Json(error.to_string()))
    })
    .transpose()
}

pub async fn list(
    db: &impl ConnectionTrait,
    namespace: &str,
    subject_id: Option<i32>,
    tapp_id: Option<&str>,
) -> Result<Vec<RegistryRow>, DbErr> {
    RegistryRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
SELECT record_id, runtime_id, payload
FROM tapp_runtime_registry
WHERE namespace = $1
  AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT
  AND ($2::INTEGER IS NULL OR subject_id = $2)
  AND ($3::TEXT IS NULL OR tapp_id = $3)
ORDER BY updated_at ASC
"#,
        vec![
            namespace.into(),
            subject_id.into(),
            tapp_id.map(str::to_string).into(),
        ],
    ))
    .all(db)
    .await
}

/// Return the distinct live subjects registered in a namespace.
///
/// Scheduler WebSocket presence uses this to discover users connected to any
/// backend replica without exposing individual connection records.
pub async fn list_subject_ids(
    db: &impl ConnectionTrait,
    namespace: &str,
) -> Result<Vec<i32>, DbErr> {
    #[derive(FromQueryResult)]
    struct SubjectRow {
        subject_id: i32,
    }

    let rows = SubjectRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
SELECT DISTINCT subject_id
FROM tapp_runtime_registry
WHERE namespace = $1
  AND subject_id IS NOT NULL
  AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT
ORDER BY subject_id
"#,
        vec![namespace.into()],
    ))
    .all(db)
    .await?;
    Ok(rows.into_iter().map(|row| row.subject_id).collect())
}

#[derive(Debug, Clone, FromQueryResult)]
pub struct RegistryEndpoint {
    pub record_id: String,
    pub subject_id: i32,
}

/// Return every live connection together with its authenticated subject.
pub async fn list_subject_endpoints(
    db: &impl ConnectionTrait,
    namespace: &str,
) -> Result<Vec<RegistryEndpoint>, DbErr> {
    RegistryEndpoint::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
SELECT record_id, subject_id
FROM tapp_runtime_registry
WHERE namespace = $1
  AND subject_id IS NOT NULL
  AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT
ORDER BY updated_at, record_id
"#,
        vec![namespace.into()],
    ))
    .all(db)
    .await
}

pub async fn count_namespace(db: &impl ConnectionTrait, namespace: &str) -> Result<i64, DbErr> {
    #[derive(FromQueryResult)]
    struct CountRow {
        count: i64,
    }

    Ok(CountRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT COUNT(*)::BIGINT AS count FROM tapp_runtime_registry WHERE namespace = $1 AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT",
        vec![namespace.into()],
    ))
    .one(db)
    .await?
    .map_or(0, |row| row.count))
}

pub async fn count_distinct_subjects(
    db: &impl ConnectionTrait,
    namespace: &str,
) -> Result<i64, DbErr> {
    #[derive(FromQueryResult)]
    struct CountRow {
        count: i64,
    }

    Ok(CountRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
SELECT COUNT(DISTINCT subject_id)::BIGINT AS count
FROM tapp_runtime_registry
WHERE namespace = $1
  AND subject_id IS NOT NULL
  AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT
"#,
        vec![namespace.into()],
    ))
    .one(db)
    .await?
    .map_or(0, |row| row.count))
}

pub async fn mailbox_depth(db: &impl ConnectionTrait, channel: &str) -> Result<i64, DbErr> {
    #[derive(FromQueryResult)]
    struct CountRow {
        count: i64,
    }

    Ok(CountRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT COUNT(*)::BIGINT AS count FROM tapp_runtime_mailbox WHERE channel = $1 AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT",
        vec![channel.into()],
    ))
    .one(db)
    .await?
    .map_or(0, |row| row.count))
}

pub async fn delete(
    db: &impl ConnectionTrait,
    namespace: &str,
    record_id: &str,
) -> Result<bool, DbErr> {
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM tapp_runtime_registry WHERE namespace = $1 AND record_id = $2",
            vec![namespace.into(), record_id.into()],
        ))
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn take<T: DeserializeOwned>(
    db: &impl ConnectionTrait,
    namespace: &str,
    record_id: &str,
) -> Result<Option<T>, DbErr> {
    #[derive(FromQueryResult)]
    struct PayloadRow {
        payload: Value,
    }
    let row = PayloadRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM tapp_runtime_registry WHERE namespace = $1 AND record_id = $2 AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT RETURNING payload",
        vec![namespace.into(), record_id.into()],
    ))
    .one(db)
    .await?;
    row.map(|row| {
        serde_json::from_value(row.payload).map_err(|error| DbErr::Json(error.to_string()))
    })
    .transpose()
}

/// Atomically consume a live record only when it belongs to the expected subject.
pub async fn take_for_subject<T: DeserializeOwned>(
    db: &impl ConnectionTrait,
    namespace: &str,
    record_id: &str,
    subject_id: i32,
) -> Result<Option<T>, DbErr> {
    #[derive(FromQueryResult)]
    struct PayloadRow {
        payload: Value,
    }
    let row = PayloadRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM tapp_runtime_registry WHERE namespace = $1 AND record_id = $2 AND subject_id = $3 AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT RETURNING payload",
        vec![namespace.into(), record_id.into(), subject_id.into()],
    ))
    .one(db)
    .await?;
    row.map(|row| {
        serde_json::from_value(row.payload).map_err(|error| DbErr::Json(error.to_string()))
    })
    .transpose()
}

/// Atomically consume every live record for one runtime identity.
pub async fn take_all_for_runtime<T: DeserializeOwned>(
    db: &impl ConnectionTrait,
    namespace: &str,
    runtime_id: &str,
) -> Result<Vec<T>, DbErr> {
    #[derive(FromQueryResult)]
    struct PayloadRow {
        payload: Value,
    }
    let rows = PayloadRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "WITH deleted AS (DELETE FROM tapp_runtime_registry WHERE namespace = $1 AND runtime_id = $2 AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT RETURNING payload, updated_at) SELECT payload FROM deleted ORDER BY updated_at ASC",
        vec![namespace.into(), runtime_id.into()],
    ))
    .all(db)
    .await?;
    rows.into_iter()
        .map(|row| {
            serde_json::from_value(row.payload).map_err(|error| DbErr::Json(error.to_string()))
        })
        .collect()
}

pub async fn delete_matching(
    db: &impl ConnectionTrait,
    namespace: &str,
    subject_id: Option<i32>,
    owner_id: Option<i32>,
    tapp_id: Option<&str>,
    runtime_id: Option<&str>,
) -> Result<u64, DbErr> {
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
DELETE FROM tapp_runtime_registry
WHERE namespace = $1
  AND ($2::INTEGER IS NULL OR subject_id = $2)
  AND ($3::INTEGER IS NULL OR owner_id = $3)
  AND ($4::TEXT IS NULL OR tapp_id = $4)
  AND ($5::TEXT IS NULL OR runtime_id = $5)
"#,
            vec![
                namespace.into(),
                subject_id.into(),
                owner_id.into(),
                tapp_id.map(str::to_string).into(),
                runtime_id.map(str::to_string).into(),
            ],
        ))
        .await?;
    Ok(result.rows_affected())
}

#[allow(clippy::too_many_arguments)]
pub async fn delete_matching_payload_text(
    db: &impl ConnectionTrait,
    namespace: &str,
    subject_id: Option<i32>,
    owner_id: Option<i32>,
    tapp_id: Option<&str>,
    runtime_id: Option<&str>,
    payload_field: &str,
    payload_value: &str,
) -> Result<u64, DbErr> {
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
DELETE FROM tapp_runtime_registry
WHERE namespace = $1
  AND ($2::INTEGER IS NULL OR subject_id = $2)
  AND ($3::INTEGER IS NULL OR owner_id = $3)
  AND ($4::TEXT IS NULL OR tapp_id = $4)
  AND ($5::TEXT IS NULL OR runtime_id = $5)
  AND payload ->> ($6::TEXT) = $7
"#,
            vec![
                namespace.into(),
                subject_id.into(),
                owner_id.into(),
                tapp_id.map(str::to_string).into(),
                runtime_id.map(str::to_string).into(),
                payload_field.into(),
                payload_value.into(),
            ],
        ))
        .await?;
    Ok(result.rows_affected())
}

pub async fn enqueue<T: Serialize>(
    db: &impl ConnectionTrait,
    channel: &str,
    runtime_id: &str,
    payload: &T,
    expires_at: i64,
) -> Result<(), DbErr> {
    let payload = serde_json::to_value(payload).map_err(|error| DbErr::Json(error.to_string()))?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO tapp_runtime_mailbox (channel, runtime_id, payload, expires_at) VALUES ($1, $2, $3, $4)",
        vec![channel.into(), runtime_id.into(), payload.into(), expires_at.into()],
    ))
    .await?;
    maybe_cleanup(db).await;
    // NOTIFY is a best-effort latency hint. The mailbox remains the source of
    // truth until a consumer claims a message, and consumers poll if hints are missed.
    if let Err(error) = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT pg_notify('tapp_runtime_mailbox', $1)",
            vec![runtime_id.into()],
        ))
        .await
    {
        tracing::warn!(%error, channel, runtime_id, "[TAPP] Mailbox NOTIFY hint failed");
    }
    Ok(())
}

pub async fn drain<T: DeserializeOwned>(
    db: &impl ConnectionTrait,
    channel: &str,
    runtime_id: &str,
    limit: i64,
) -> Result<Vec<T>, DbErr> {
    #[derive(FromQueryResult)]
    struct PayloadRow {
        payload: Value,
    }

    let rows = PayloadRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
WITH claimed AS (
    SELECT message_id
    FROM tapp_runtime_mailbox
    WHERE channel = $1 AND runtime_id = $2
      AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT
    ORDER BY message_id
    LIMIT $3
    FOR UPDATE SKIP LOCKED
)
DELETE FROM tapp_runtime_mailbox AS mailbox
USING claimed
WHERE mailbox.message_id = claimed.message_id
RETURNING mailbox.payload
"#,
        vec![channel.into(), runtime_id.into(), limit.into()],
    ))
    .all(db)
    .await?;
    rows.into_iter()
        .map(|row| {
            serde_json::from_value(row.payload).map_err(|error| DbErr::Json(error.to_string()))
        })
        .collect()
}

pub async fn cleanup(db: &impl ConnectionTrait) -> Result<(), DbErr> {
    db.execute_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "DELETE FROM tapp_runtime_registry WHERE expires_at <= EXTRACT(EPOCH FROM NOW())::BIGINT"
            .to_string(),
    ))
    .await?;
    db.execute_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "DELETE FROM tapp_runtime_mailbox WHERE expires_at <= EXTRACT(EPOCH FROM NOW())::BIGINT"
            .to_string(),
    ))
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_identity_holds_optional_scopes() {
        let id = RegistryIdentity {
            subject_id: Some(7),
            owner_id: Some(1),
            tapp_id: Some("demo-tapp"),
            runtime_id: Some("rt-1"),
        };
        assert_eq!(id.subject_id, Some(7));
        assert_eq!(id.owner_id, Some(1));
        assert_eq!(id.tapp_id, Some("demo-tapp"));
        assert_eq!(id.runtime_id, Some("rt-1"));
    }

    #[test]
    fn registry_identity_allows_anonymous_presence() {
        let id = RegistryIdentity {
            subject_id: None,
            owner_id: None,
            tapp_id: None,
            runtime_id: None,
        };
        assert!(id.subject_id.is_none());
        assert!(id.tapp_id.is_none());
    }
}
