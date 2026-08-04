//! Cross-replica AI Task registry (persist + atomic registration).
//!
//! Owns the durable gate used by both the public AI Task API and governed
//! host adapters. Process-local execution state (`AI_TASKS`, provider calls)
//! stays in the API module; this module only talks to the shared registry DB.

use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, DbErr, FromQueryResult, Statement,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::services::ai_quota::AiUsageSnapshot;
use crate::services::tapp_registry::{self as shared_registry, RegistryIdentity};
use myriad_tapp_contract::manifest::TappAiOperation;

pub const MAX_ACTIVE_TASKS_PER_SUBJECT: usize = 4;
pub const MAX_RETAINED_TASKS_PER_SUBJECT: usize = 64;
pub const TASK_RETENTION_SECONDS: i64 = 15 * 60;
pub const AI_TASK_NAMESPACE: &str = "ai_task";
pub const AI_CANCEL_NAMESPACE: &str = "ai_cancel";
pub const AI_TASK_MAILBOX_CHANNEL: &str = "ai_task_event_v2";

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AiTaskDelivery {
    #[default]
    Result,
    Stream,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AiTaskStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl AiTaskStatus {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTaskSnapshot {
    pub task_id: String,
    pub status: AiTaskStatus,
    pub operation: TappAiOperation,
    pub delivery: AiTaskDelivery,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
    pub usage: AiUsageSnapshot,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PersistedAiTask {
    pub runtime_id: String,
    pub subject_id: i32,
    pub owner_id: i32,
    pub tapp_id: String,
    pub idempotency_key: Option<String>,
    pub request_hash: [u8; 32],
    pub snapshot: AiTaskSnapshot,
    pub retain_until: i64,
}

#[derive(Debug)]
pub enum AiTaskRegistration {
    Inserted,
    Existing(Box<AiTaskSnapshot>),
    IdempotencyConflict,
    LimitReached,
}

/// Deterministic task id for idempotent requests; random id otherwise.
pub fn task_id_for_request(
    subject_id: i32,
    owner_id: i32,
    tapp_id: &str,
    idempotency_key: Option<&str>,
) -> String {
    let Some(idempotency_key) = idempotency_key else {
        return format!("ait_{}", Uuid::new_v4().simple());
    };
    let mut digest = Sha256::new();
    digest.update(subject_id.to_be_bytes());
    digest.update(owner_id.to_be_bytes());
    digest.update(tapp_id.as_bytes());
    digest.update([0]);
    digest.update(idempotency_key.as_bytes());
    format!("ait_{}", hex::encode(digest.finalize()))
}

/// Final cross-replica registration gate. Fast preflight checks may reject
/// obvious overload earlier, but only this transaction is authoritative.
pub async fn register_ai_task_atomically(
    db: &DatabaseConnection,
    task: &PersistedAiTask,
) -> Result<AiTaskRegistration, DbErr> {
    #[derive(FromQueryResult)]
    struct PayloadRow {
        payload: Value,
    }

    let transaction = db.begin().await?;
    let lock_key = format!("tapp_ai_task:{}", task.subject_id);
    transaction
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            vec![lock_key.into()],
        ))
        .await?;
    transaction
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "DELETE FROM tapp_runtime_registry WHERE namespace = $1 AND subject_id = $2 AND expires_at <= EXTRACT(EPOCH FROM NOW())::BIGINT",
            vec![AI_TASK_NAMESPACE.into(), task.subject_id.into()],
        ))
        .await?;
    let tasks = PayloadRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT payload FROM tapp_runtime_registry WHERE namespace = $1 AND subject_id = $2 AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT ORDER BY updated_at ASC",
        vec![AI_TASK_NAMESPACE.into(), task.subject_id.into()],
    ))
    .all(&transaction)
    .await?
    .into_iter()
    .filter_map(|row| serde_json::from_value::<PersistedAiTask>(row.payload).ok())
    .collect::<Vec<_>>();

    if let Some(key) = task.idempotency_key.as_deref() {
        if let Some(existing) = tasks.iter().find(|existing| {
            existing.owner_id == task.owner_id
                && existing.tapp_id == task.tapp_id
                && existing.idempotency_key.as_deref() == Some(key)
        }) {
            let outcome = if existing.request_hash == task.request_hash {
                AiTaskRegistration::Existing(Box::new(existing.snapshot.clone()))
            } else {
                AiTaskRegistration::IdempotencyConflict
            };
            transaction.rollback().await?;
            return Ok(outcome);
        }
    }

    let active = tasks
        .iter()
        .filter(|existing| !existing.snapshot.status.terminal())
        .count();
    if active >= MAX_ACTIVE_TASKS_PER_SUBJECT || tasks.len() >= MAX_RETAINED_TASKS_PER_SUBJECT {
        transaction.rollback().await?;
        return Ok(AiTaskRegistration::LimitReached);
    }

    let payload = serde_json::to_value(task).map_err(|error| DbErr::Json(error.to_string()))?;
    let inserted = transaction
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"
INSERT INTO tapp_runtime_registry
    (namespace, record_id, subject_id, owner_id, tapp_id, runtime_id, payload, expires_at, updated_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())
ON CONFLICT (namespace, record_id) DO NOTHING
"#,
            vec![
                AI_TASK_NAMESPACE.into(),
                task.snapshot.task_id.clone().into(),
                task.subject_id.into(),
                task.owner_id.into(),
                task.tapp_id.clone().into(),
                task.runtime_id.clone().into(),
                payload.into(),
                task.retain_until.into(),
            ],
        ))
        .await?
        .rows_affected();
    if inserted != 1 {
        transaction.rollback().await?;
        return Err(DbErr::Custom(
            "AI task registry ID collision without matching idempotency record".to_string(),
        ));
    }
    transaction.commit().await?;
    Ok(AiTaskRegistration::Inserted)
}

pub async fn persist_ai_task(
    db: &DatabaseConnection,
    task: &PersistedAiTask,
) -> Result<(), DbErr> {
    shared_registry::put(
        db,
        AI_TASK_NAMESPACE,
        &task.snapshot.task_id,
        RegistryIdentity {
            subject_id: Some(task.subject_id),
            owner_id: Some(task.owner_id),
            tapp_id: Some(&task.tapp_id),
            runtime_id: Some(&task.runtime_id),
        },
        task,
        task.retain_until,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{task_id_for_request, AiTaskStatus};

    #[test]
    fn task_id_without_idempotency_is_random_prefix() {
        let a = task_id_for_request(1, 1, "com.example.app", None);
        let b = task_id_for_request(1, 1, "com.example.app", None);
        assert!(a.starts_with("ait_"));
        assert!(b.starts_with("ait_"));
        assert_ne!(a, b);
    }

    #[test]
    fn task_id_with_idempotency_is_stable() {
        let a = task_id_for_request(1, 2, "com.example.app", Some("refresh:day"));
        let b = task_id_for_request(1, 2, "com.example.app", Some("refresh:day"));
        let c = task_id_for_request(1, 2, "com.example.app", Some("refresh:other"));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("ait_"));
    }

    #[test]
    fn terminal_status_boundary() {
        assert!(!AiTaskStatus::Queued.terminal());
        assert!(!AiTaskStatus::Running.terminal());
        assert!(AiTaskStatus::Completed.terminal());
        assert!(AiTaskStatus::Failed.terminal());
        assert!(AiTaskStatus::Cancelled.terminal());
    }
}
