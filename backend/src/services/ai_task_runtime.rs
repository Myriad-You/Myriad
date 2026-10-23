//! Process-local AI Task runtime shell.
//!
//! Owns the in-memory task map, local cancel watches, and state transitions that
//! also fan out to the durable registry + mailbox. HTTP handlers keep request
//! validation; provider execution is separate (`ai_task_provider`).

use std::collections::HashMap;

use once_cell::sync::Lazy;
use sea_orm::TransactionTrait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{RwLock, watch};

use crate::services::ai_quota::AiUsageSnapshot;
use crate::services::ai_task_registry::{
    AI_CANCEL_NAMESPACE, AI_TASK_MAILBOX_CHANNEL, AI_TASK_NAMESPACE, AiTaskSnapshot, AiTaskStatus,
    PersistedAiTask, TASK_RETENTION_SECONDS, persist_ai_task,
};
use crate::services::runtime_registry::{self as shared_registry, RegistryIdentity};
use myriad_tapp_contract::manifest::TappAiOperation;

/// Process-local task handle (includes cancel channel).
pub struct LocalAiTask {
    pub runtime_id: String,
    pub subject_id: i32,
    pub owner_id: i32,
    pub tapp_id: String,
    pub idempotency_key: Option<String>,
    pub request_hash: [u8; 32],
    pub snapshot: AiTaskSnapshot,
    cancel: watch::Sender<bool>,
    pub retain_until: i64,
}

impl LocalAiTask {
    /// Create a queued local task and a cancel receiver for the executor.
    pub fn new(
        runtime_id: String,
        subject_id: i32,
        owner_id: i32,
        tapp_id: String,
        idempotency_key: Option<String>,
        request_hash: [u8; 32],
        snapshot: AiTaskSnapshot,
    ) -> (Self, watch::Receiver<bool>) {
        let (cancel, receiver) = watch::channel(false);
        let retain_until = crate::clock::now().timestamp() + TASK_RETENTION_SECONDS;
        (
            Self {
                runtime_id,
                subject_id,
                owner_id,
                tapp_id,
                idempotency_key,
                request_hash,
                snapshot,
                cancel,
                retain_until,
            },
            receiver,
        )
    }

    pub fn to_persisted(&self) -> PersistedAiTask {
        PersistedAiTask {
            runtime_id: self.runtime_id.clone(),
            subject_id: self.subject_id,
            owner_id: self.owner_id,
            tapp_id: self.tapp_id.clone(),
            idempotency_key: self.idempotency_key.clone(),
            request_hash: self.request_hash,
            snapshot: self.snapshot.clone(),
            retain_until: self.retain_until,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskBroadcast {
    pub kind: String,
    pub payload: Value,
}

static AI_TASKS: Lazy<RwLock<HashMap<String, LocalAiTask>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

fn clean_tasks(tasks: &mut HashMap<String, LocalAiTask>, now: i64) {
    tasks.retain(|_, task| !task.snapshot.status.terminal() || task.retain_until > now);
    if tasks.capacity() > tasks.len().saturating_mul(4).max(64) {
        tasks.shrink_to(tasks.len().max(64));
    }
}

/// Insert a local task after durable registration succeeded.
pub async fn insert_local(task: LocalAiTask) {
    let mut tasks = AI_TASKS.write().await;
    clean_tasks(&mut tasks, crate::clock::now().timestamp());
    tasks.insert(task.snapshot.task_id.clone(), task);
}

/// Release terminal snapshots even when no new request reaches admission.
pub async fn cleanup_local_tasks() {
    let abandoned = {
        let mut tasks = AI_TASKS.write().await;
        clean_tasks(&mut tasks, crate::clock::now().timestamp());
        // A receiver lives in the producer future, including while queued for
        // polling or performing quota/ledger settlement. Age alone must never
        // terminate an executor which still owns its receiver.
        tasks
            .values()
            .filter(|task| !task.snapshot.status.terminal() && task.cancel.receiver_count() == 0)
            .take(8)
            .map(|task| task.snapshot.task_id.clone())
            .collect::<Vec<_>>()
    };
    for task_id in abandoned {
        // No detached cleanup jobs, and one stalled registry cannot stall the
        // maintenance loop. Terminal state becomes visible only after database I/O.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), finish_task(
            &task_id, AiTaskStatus::Cancelled, None,
            Some(serde_json::json!({"code": "AI_TASK_CANCELLED", "message": "AI task executor stopped"})), None,
        )).await;
    }
}

/// Run a local task while its caller awaits the result.
pub async fn run_owned_local(
    task: LocalAiTask,
    execution: impl std::future::Future<Output = ()> + Send + 'static,
) {
    struct CancelOnDrop(watch::Sender<bool>);
    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            let _ = self.0.send(true);
        }
    }
    let _cancel_on_drop = CancelOnDrop(task.cancel.clone());
    let task_id = task.snapshot.task_id.clone();
    insert_local(task).await;
    // The caller owns cancellation, while this registered executor owns its
    // terminal control path (quota, ledger, durable snapshot and mailbox).
    // Dropping a JoinHandle detaches it; the guard signals cooperative cancel.
    if let Err(error) = tokio::spawn(execution).await {
        tracing::error!(%error, %task_id, "[TAPP] AI task executor stopped unexpectedly");
        cleanup_local_tasks().await;
    }
}

/// Snapshot of a local task if still present.
pub async fn local_snapshot(task_id: &str) -> Option<AiTaskSnapshot> {
    let mut tasks = AI_TASKS.write().await;
    clean_tasks(&mut tasks, crate::clock::now().timestamp());
    tasks.get(task_id).map(|task| task.snapshot.clone())
}

/// Count active + retained local tasks for a subject (preflight).
pub async fn local_subject_counts(subject_id: i32) -> (usize, usize) {
    let mut tasks = AI_TASKS.write().await;
    clean_tasks(&mut tasks, crate::clock::now().timestamp());
    let mut active = 0usize;
    let mut retained = 0usize;
    for task in tasks.values() {
        if task.subject_id != subject_id {
            continue;
        }
        retained += 1;
        if !task.snapshot.status.terminal() {
            active += 1;
        }
    }
    (active, retained)
}

async fn cancel_matching_local(predicate: impl Fn(&LocalAiTask) -> bool) -> usize {
    let senders = {
        let tasks = AI_TASKS.read().await;
        tasks
            .values()
            .filter(|task| !task.snapshot.status.terminal() && predicate(task))
            .map(|task| task.cancel.clone())
            .collect::<Vec<_>>()
    };
    for sender in &senders {
        let _ = sender.send(true);
    }
    senders.len()
}

async fn cancel_shared(
    subject_id: Option<i32>,
    tapp_id: Option<&str>,
    predicate: impl Fn(&PersistedAiTask) -> bool,
) -> usize {
    let Ok(db) = crate::services::process_db::database() else {
        return 0;
    };
    let tasks = shared_registry::list(&db, AI_TASK_NAMESPACE, subject_id, tapp_id)
        .await
        .unwrap_or_default();
    let mut cancelled = 0;
    for row in tasks {
        let Ok(task) = serde_json::from_value::<PersistedAiTask>(row.payload) else {
            continue;
        };
        if task.snapshot.status.terminal() || !predicate(&task) {
            continue;
        }
        if shared_registry::put(
            &db,
            AI_CANCEL_NAMESPACE,
            &task.snapshot.task_id,
            RegistryIdentity {
                subject_id: Some(task.subject_id),
                owner_id: Some(task.owner_id),
                tapp_id: Some(&task.tapp_id),
                runtime_id: Some(&task.runtime_id),
            },
            &true,
            task.retain_until,
        )
        .await
        .is_ok()
        {
            cancelled += 1;
        }
    }
    cancelled
}

/// Cancel tasks bound to a runtime grant (local + shared).
pub async fn cancel_runtime_ai_tasks(runtime_id: &str) -> usize {
    let local = cancel_matching_local(|task| task.runtime_id == runtime_id).await;
    let shared = cancel_shared(None, None, |task| task.runtime_id == runtime_id).await;
    local.max(shared)
}

/// Cancel tasks for one subject+tapp pair.
pub async fn cancel_tapp_ai_tasks(subject_id: i32, tapp_id: &str) -> usize {
    let local =
        cancel_matching_local(|task| task.subject_id == subject_id && task.tapp_id == tapp_id)
            .await;
    let shared = cancel_shared(Some(subject_id), Some(tapp_id), |_| true).await;
    local.max(shared)
}

/// Cancel all tasks for one install owner+tapp_id (any subject).
pub async fn cancel_all_tapp_ai_tasks(owner_id: i32, tapp_id: &str) -> usize {
    let local =
        cancel_matching_local(|task| task.owner_id == owner_id && task.tapp_id == tapp_id).await;
    let shared = cancel_shared(None, Some(tapp_id), |task| task.owner_id == owner_id).await;
    local.max(shared)
}

/// Signal cancel on a single local task id (if present and non-terminal).
pub async fn cancel_local_task(task_id: &str) -> bool {
    let sender = {
        let tasks = AI_TASKS.read().await;
        tasks
            .get(task_id)
            .filter(|task| !task.snapshot.status.terminal())
            .map(|task| task.cancel.clone())
    };
    if let Some(sender) = sender {
        let _ = sender.send(true);
        true
    } else {
        false
    }
}

/// Persist + broadcast the given task status (caller chooses the status).
pub async fn update_task_state(task_id: &str, status: AiTaskStatus) {
    let mut tasks = AI_TASKS.write().await;
    let persisted = if let Some(task) = tasks.get_mut(task_id) {
        task.snapshot.status = status;
        task.snapshot.updated_at = crate::clock::now().to_rfc3339();
        Some(task.to_persisted())
    } else {
        None
    };
    drop(tasks);
    if let (Some(task), Ok(db)) = (persisted, crate::services::process_db::database()) {
        if let Err(error) = persist_ai_task(&db, &task).await {
            tracing::error!(%error, task_id = %task.snapshot.task_id, "[TAPP] Failed to persist AI task state");
        }
        let _ = shared_registry::enqueue(
            &db,
            AI_TASK_MAILBOX_CHANNEL,
            task_id,
            &TaskBroadcast {
                kind: "state".to_string(),
                payload: serde_json::to_value(&task.snapshot).unwrap_or(Value::Null),
            },
            task.retain_until,
        )
        .await;
    }
}

/// Terminal transition: set result/error/usage, extend retention, persist + broadcast.
pub async fn finish_task(
    task_id: &str,
    status: AiTaskStatus,
    result: Option<Value>,
    error: Option<Value>,
    usage: Option<AiUsageSnapshot>,
) {
    // Keep the prior visible snapshot until its durable result and references commit.
    let mut tasks = AI_TASKS.write().await;
    let Some(local) = tasks.get_mut(task_id) else {
        return;
    };
    let mut task = local.to_persisted();
    task.snapshot.status = status;
    task.snapshot.result = result;
    task.snapshot.error = error;
    if let Some(usage) = usage {
        task.snapshot.usage = usage;
    }
    task.snapshot.updated_at = crate::clock::now().to_rfc3339();
    task.retain_until = crate::clock::now().timestamp() + TASK_RETENTION_SECONDS;
    if let Ok(db) = crate::services::process_db::database() {
        if let Err(error) = persist_terminal_task(&db, &task).await {
            tracing::error!(%error, %task_id, "Failed to commit AI result and media references");
            // Never expose the unprotected result. Persist a failure instead; if
            // storage is unavailable cleanup can retry the nonterminal local task.
            task.snapshot.status = AiTaskStatus::Failed;
            task.snapshot.result = None;
            task.snapshot.error = Some(serde_json::json!({
                "code": "AI_RESULT_COMMIT_FAILED", "message": "Could not save AI result"
            }));
            if persist_terminal_task(&db, &task).await.is_err() {
                return;
            }
        }
    } else if task.snapshot.result.is_some() {
        return;
    }
    local.snapshot = task.snapshot;
    local.retain_until = task.retain_until;
}

pub(crate) async fn persist_terminal_task(
    db: &sea_orm::DatabaseConnection,
    task: &PersistedAiTask,
) -> Result<(), sea_orm::DbErr> {
    let txn = db.begin().await?;
    let expires = chrono::DateTime::from_timestamp(task.retain_until, 0);
    crate::services::media::bind_ai_task(
        &txn,
        &task.snapshot.task_id,
        task.snapshot.result.as_ref().unwrap_or(&Value::Null),
        &[],
        expires,
    )
    .await
    .map_err(|error| sea_orm::DbErr::Custom(error.to_string()))?;
    persist_ai_task(&txn, task).await?;
    let kind = match task.snapshot.status {
        AiTaskStatus::Completed => "result",
        AiTaskStatus::Cancelled => "cancelled",
        _ => "error",
    };
    shared_registry::enqueue(
        &txn,
        AI_TASK_MAILBOX_CHANNEL,
        &task.snapshot.task_id,
        &TaskBroadcast {
            kind: kind.into(),
            payload: serde_json::to_value(&task.snapshot)
                .map_err(|error| sea_orm::DbErr::Json(error.to_string()))?,
        },
        task.retain_until,
    )
    .await?;
    txn.commit().await
}

/// Stable operation name for cost ledger rows.
pub fn operation_name(operation: TappAiOperation) -> &'static str {
    match operation {
        TappAiOperation::Generate => "generate",
        TappAiOperation::Analyze => "analyze",
        TappAiOperation::Chat => "chat",
        TappAiOperation::Image => "image",
        TappAiOperation::Search => "search",
    }
}

#[cfg(test)]
mod tests {
    use super::{LocalAiTask, clean_tasks, operation_name};
    use crate::services::ai_quota::{AiCooldownStatus, AiUsageCounter, AiUsageSnapshot};
    use crate::services::ai_task_registry::{AiTaskDelivery, AiTaskSnapshot, AiTaskStatus};
    use crate::services::permission_service::UserRole;
    use myriad_tapp_contract::manifest::TappAiOperation;
    use std::collections::HashMap;

    fn usage() -> AiUsageSnapshot {
        AiUsageSnapshot {
            calls: AiUsageCounter {
                limit: Some(10),
                used: 0,
                remaining: Some(10),
                resets_at: "x".into(),
            },
            tokens: AiUsageCounter {
                limit: Some(100),
                used: 0,
                remaining: Some(100),
                resets_at: "x".into(),
            },
            cooldown: AiCooldownStatus {
                required_seconds: 0,
                remaining_seconds: 0,
            },
            restricted: false,
            restriction_reason: None,
            unlimited: false,
            role: UserRole::User,
        }
    }

    #[test]
    fn operation_names_are_stable() {
        assert_eq!(operation_name(TappAiOperation::Generate), "generate");
        assert_eq!(operation_name(TappAiOperation::Image), "image");
    }

    #[test]
    fn local_task_to_persisted_copies_identity() {
        let snapshot = AiTaskSnapshot {
            task_id: "ait_1".into(),
            status: AiTaskStatus::Queued,
            operation: TappAiOperation::Generate,
            delivery: AiTaskDelivery::Result,
            created_at: "t".into(),
            updated_at: "t".into(),
            result: None,
            error: None,
            usage: usage(),
        };
        let (task, _rx) = LocalAiTask::new(
            "rt".into(),
            1,
            2,
            "com.example.app".into(),
            Some("key".into()),
            [0u8; 32],
            snapshot,
        );
        let persisted = task.to_persisted();
        assert_eq!(persisted.runtime_id, "rt");
        assert_eq!(persisted.subject_id, 1);
        assert_eq!(persisted.owner_id, 2);
        assert_eq!(persisted.tapp_id, "com.example.app");
        assert_eq!(persisted.snapshot.task_id, "ait_1");
    }

    #[test]
    fn clean_tasks_keeps_active_and_unexpired_terminal() {
        let snapshot = AiTaskSnapshot {
            task_id: "ait_done".into(),
            status: AiTaskStatus::Completed,
            operation: TappAiOperation::Generate,
            delivery: AiTaskDelivery::Result,
            created_at: "t".into(),
            updated_at: "t".into(),
            result: None,
            error: None,
            usage: usage(),
        };
        let (mut done, _) = LocalAiTask::new(
            "rt".into(),
            1,
            1,
            "com.example.app".into(),
            None,
            [0u8; 32],
            snapshot,
        );
        done.retain_until = 100;
        let mut map = HashMap::new();
        map.insert("ait_done".into(), done);
        clean_tasks(&mut map, 101);
        assert!(map.is_empty());
    }

    fn local_task(
        id: &str,
        subject: i32,
        status: AiTaskStatus,
    ) -> (LocalAiTask, tokio::sync::watch::Receiver<bool>) {
        LocalAiTask::new(
            "rt".into(),
            subject,
            subject,
            "test".into(),
            None,
            [0; 32],
            AiTaskSnapshot {
                task_id: id.into(),
                status,
                operation: TappAiOperation::Generate,
                delivery: AiTaskDelivery::Result,
                created_at: "t".into(),
                updated_at: "t".into(),
                result: None,
                error: None,
                usage: usage(),
            },
        )
    }

    #[tokio::test]
    async fn preflight_releases_all_expired_terminal_records() {
        let subject = -91001;
        let mut tasks = super::AI_TASKS.write().await;
        for i in 0..64 {
            let (mut task, _) =
                local_task(&format!("expired-{i}"), subject, AiTaskStatus::Completed);
            task.retain_until = 1;
            tasks.insert(task.snapshot.task_id.clone(), task);
        }
        drop(tasks);
        assert_eq!(super::local_subject_counts(subject).await, (0, 0));
    }

    #[tokio::test]
    async fn expired_result_is_not_returned_by_local_snapshot() {
        let (mut task, _) = local_task("expired-snapshot", -91002, AiTaskStatus::Completed);
        task.retain_until = 1;
        super::AI_TASKS
            .write()
            .await
            .insert(task.snapshot.task_id.clone(), task);
        assert!(super::local_snapshot("expired-snapshot").await.is_none());
    }

    #[tokio::test]
    async fn dropping_governed_caller_allows_executor_terminal_cleanup() {
        let (task, mut cancel) = local_task("cancelled-producer", -91003, AiTaskStatus::Queued);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
        let parent = tokio::spawn(super::run_owned_local(task, async move {
            super::update_task_state("cancelled-producer", AiTaskStatus::Running).await;
            let _ = started_tx.send(());
            cancel.changed().await.unwrap();
            assert!(*cancel.borrow());
            super::finish_task(
                "cancelled-producer",
                AiTaskStatus::Cancelled,
                None,
                None,
                None,
            )
            .await;
            let _ = finished_tx.send(());
        }));
        started_rx.await.unwrap();
        parent.abort();
        let _ = parent.await;
        tokio::time::timeout(std::time::Duration::from_secs(1), finished_rx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            super::local_snapshot("cancelled-producer")
                .await
                .unwrap()
                .status,
            AiTaskStatus::Cancelled
        );
    }

    #[tokio::test]
    async fn idle_cleanup_reaps_abandoned_producers_but_keeps_live_work() {
        let (mut live, live_receiver) = local_task("live-producer", -91004, AiTaskStatus::Running);
        live.retain_until = 1;
        let (orphan, orphan_receiver) =
            local_task("orphan-producer", -91004, AiTaskStatus::Running);
        super::insert_local(live).await;
        super::insert_local(orphan).await;
        drop(orphan_receiver);
        super::cleanup_local_tasks().await;
        assert_eq!(
            super::local_snapshot("orphan-producer")
                .await
                .unwrap()
                .status,
            AiTaskStatus::Cancelled
        );
        assert_eq!(
            super::local_snapshot("live-producer").await.unwrap().status,
            AiTaskStatus::Running
        );
        let mut tasks = super::AI_TASKS.write().await;
        tasks.get_mut("orphan-producer").unwrap().retain_until = 1;
        drop(tasks);
        super::cleanup_local_tasks().await;
        assert!(super::local_snapshot("orphan-producer").await.is_none());
        super::AI_TASKS.write().await.remove("live-producer");
        drop(live_receiver);
    }
}
