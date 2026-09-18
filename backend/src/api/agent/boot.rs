//! Agent API — boot
use super::*;

// Boot recovery for waiting tasks
static ACTIVE_WAIT_LOOPS: once_cell::sync::Lazy<
    std::sync::Mutex<std::collections::HashSet<String>>,
> = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(Default::default()));
struct WaitLoopRegistration(String);
impl WaitLoopRegistration {
    fn claim(task_id: &str) -> Option<Self> {
        ACTIVE_WAIT_LOOPS
            .lock()
            .unwrap()
            .insert(task_id.to_owned())
            .then(|| Self(task_id.to_owned()))
    }
}
impl Drop for WaitLoopRegistration {
    fn drop(&mut self) {
        ACTIVE_WAIT_LOOPS.lock().unwrap().remove(&self.0);
    }
}

/// After process restart, re-create run hubs and wait-loops for
/// `waiting_for_input` tasks so answer/subscribe keep working and notifications
/// stay consistent. Called once from `persona::start` after `init_task_store`.
/// Uses the caller-held DB; does not consult the process-global slot.
pub async fn restore_waiting_runs_after_boot(db: &DatabaseConnection) {
    let waiting = crate::services::agent::executor::task_store::list_waiting_tasks_snapshot().await;
    let waiting: Vec<_> = waiting
        .into_iter()
        .filter(|(_, task)| !ACTIVE_WAIT_LOOPS.lock().unwrap().contains(&task.task_id))
        .collect();
    if waiting.is_empty() {
        tracing::info!("[Agent API] Boot restore: no waiting_for_input tasks");
        return;
    }

    tracing::info!(
        count = waiting.len(),
        "[Agent API] Boot restore: re-creating run hubs for waiting tasks"
    );
    let ledger_db = Some(db.clone());

    for (user_id, mut task) in waiting {
        let is_work = task
            .recipe
            .as_ref()
            .is_some_and(crate::services::agent::work_loop::is_work_recipe);
        if is_work {
            if let (Some(db), Some(question)) = (&ledger_db, &task.pending_question) {
                // Never write a boot snapshot over a newer Work continuation.
                let _ = crate::services::agent::work_loop::expire_question(
                    db,
                    &task.task_id,
                    user_id,
                    &question.question_id,
                )
                .await;
                let Some(current) =
                    crate::services::agent::executor::refresh_task_for_user(&task.task_id, user_id)
                        .await
                else {
                    continue;
                };
                task = current;
                if task.status != crate::services::agent::TaskStatus::WaitingForInput {
                    continue;
                }
            }
        }
        let session_id = crate::services::agent::executor::task_store::session_id_from_lane_id(
            task.lane_id.as_deref(),
        );
        let source_intent = if let (Some(db), Some(session_id)) = (&ledger_db, &session_id) {
            match crate::services::agent::consciousness::IntentStore::new(db.clone())
                .recoverable_for_session(user_id, session_id)
                .await
            {
                Ok(intent) => intent,
                Err(error) => {
                    tracing::warn!(%error, user_id, session_id, "[Agent API] Boot restore: intention lookup failed");
                    None
                }
            }
        } else {
            None
        };
        // Drop already-expired questions immediately so they don't block forever.
        if !is_work
            && task
                .pending_question
                .as_ref()
                .is_some_and(|q| q.is_expired(chrono::Utc::now()))
        {
            tracing::warn!(
                task_id = %task.task_id,
                "[Agent API] Boot restore: expiring abandoned waiting question"
            );
            task.status = crate::services::agent::types::TaskStatus::Failed;
            task.error = Some("Waiting for input timed out after restart".into());
            task.completed_at = Some(chrono::Utc::now());
            task.pending_question = None;
            {
                let mut store = crate::services::agent::executor::TASK_STORE.write().await;
                store.store(user_id, task.clone());
            }
            crate::services::agent::executor::persist_task_async(user_id, task);
            if let Some(db) = &ledger_db {
                advance_intention_work(
                    db,
                    source_intent.as_ref().map(|intent| intent.id.as_str()),
                    user_id,
                    crate::services::agent::consciousness::IntentStatus::Failed,
                    Some("Waiting for input timed out after restart".into()),
                )
                .await;
            }
            continue;
        }

        let run = create_run(user_id, session_id.clone()).await;
        let run_id = run.run_id().to_string();
        let task_id = task.task_id.clone();
        if let (Some(db), Some(session_id), Some(intent)) =
            (&ledger_db, &session_id, &source_intent)
        {
            let store = crate::services::agent::consciousness::IntentStore::new(db.clone());
            let result =
                if intent.status == crate::services::agent::consciousness::IntentStatus::Running {
                    store
                        .transition(
                            &intent.id,
                            user_id,
                            crate::services::agent::consciousness::IntentStatus::Waiting,
                            Some(session_id.clone()),
                            Some(run_id.clone()),
                            None,
                        )
                        .await
                        .map(|_| ())
                } else {
                    store
                        .reattach_work(&intent.id, user_id, session_id.clone(), run_id.clone())
                        .await
                };
            if let Err(error) = result {
                tracing::warn!(%error, intent_id = %intent.id, "[Agent API] Boot restore: intention reattach failed");
            }
        }

        let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentProgressEvent>(256);
        let run_for_forwarder = run.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                run_for_forwarder.publish(event).await;
            }
        });

        // Surface waiting state so re-subscribers get a usable snapshot.
        if let Some(ref q) = task.pending_question {
            let q_type = serde_json::to_value(&q.question_type)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| "free_text".to_string());
            let options = q.options.as_ref().map(|opts| {
                opts.iter()
                    .map(|o| crate::services::agent::types::QuestionOptionCompact {
                        value: o.value.clone(),
                        label: o.label.clone(),
                        description: o.description.clone(),
                    })
                    .collect()
            });
            let _ = tx
                .send(AgentProgressEvent::TaskCreated {
                    task_id: task_id.clone(),
                    message: q.question.clone(),
                    total_steps: 1,
                    step_descriptions: Vec::new(),
                })
                .await;
            let _ = tx
                .send(AgentProgressEvent::WaitingForInput {
                    task_id: task_id.clone(),
                    question_id: q.question_id.clone(),
                    question_type: q_type,
                    question: q.question.clone(),
                    context: if q.context.is_empty() {
                        None
                    } else {
                        Some(q.context.clone())
                    },
                    options,
                    required: q.required,
                    default_value: q.default_value.clone(),
                })
                .await;
        }

        let session_id_loop = session_id.unwrap_or_default();
        let source_intent_id = source_intent.map(|intent| intent.id);
        let loop_db = ledger_db.clone();
        tokio::spawn(async move {
            spawn_restored_wait_loop(
                user_id,
                task_id,
                session_id_loop,
                run_id,
                tx,
                loop_db,
                source_intent_id,
            )
            .await;
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunningRecovery {
    RestoreWaiting,
    KeepRunningWork,
    RestoreCompletedWork,
    RetryAccepted,
    FailInterrupted,
}

/// Decide whether a stranded Running intention is safe to retry.
/// Waiting tasks keep the original Work. A persisted executor task means
/// steps may already have run — fail closed instead of repeating side effects.
pub fn classify_stranded_running(
    has_waiting_task: bool,
    persisted_task_status: Option<&str>,
    has_work_checkpoint: bool,
) -> RunningRecovery {
    if has_waiting_task || persisted_task_status == Some("waiting_for_input") {
        return RunningRecovery::RestoreWaiting;
    }
    if has_work_checkpoint {
        match persisted_task_status {
            Some("running") => return RunningRecovery::KeepRunningWork,
            Some("completed") => return RunningRecovery::RestoreCompletedWork,
            _ => {}
        }
    }
    match persisted_task_status {
        None => RunningRecovery::RetryAccepted,
        Some("pending" | "running" | "cancelled" | "completed" | "failed") => {
            RunningRecovery::FailInterrupted
        }
        Some(_) => RunningRecovery::FailInterrupted,
    }
}

/// Running intentions with no restored wait-loop never finish. No executor
/// task → Accepted (tick/card can claim). Other persisted statuses → Failed.
pub async fn reclaim_stranded_running_intentions(db: &DatabaseConnection) {
    let store = crate::services::agent::consciousness::IntentStore::new(db.clone());
    let running = match store.list_running(32).await {
        Ok(running) => running,
        Err(error) => {
            tracing::warn!(%error, "[Agent API] Boot restore: list running intentions failed");
            return;
        }
    };
    if running.is_empty() {
        return;
    }
    let waiting = crate::services::agent::executor::task_store::list_waiting_tasks_snapshot().await;
    let mut reclaimed = 0u64;
    let mut failed = 0u64;
    for intent in running {
        let session_id = intent.work_session_id.clone();
        let attached_waiting = waiting.iter().any(|(user_id, task)| {
            *user_id == intent.user_id
                && crate::services::agent::executor::task_store::session_id_from_lane_id(
                    task.lane_id.as_deref(),
                ) == session_id
        });
        let persisted_task = match session_id.as_deref() {
            Some(session_id) => latest_task_for_session(db, intent.user_id, session_id).await,
            None => None,
        };
        let has_work_checkpoint = persisted_task.as_ref().is_some_and(|task| {
            task.recipe
                .as_ref()
                .and_then(|r| r.pointer("/metadata/work_loop_version"))
                == Some(&json!(1))
        });
        match classify_stranded_running(
            attached_waiting,
            persisted_task.as_ref().map(|task| task.status.as_str()),
            has_work_checkpoint,
        ) {
            RunningRecovery::RestoreWaiting | RunningRecovery::KeepRunningWork => continue,
            RunningRecovery::RestoreCompletedWork => {
                let task = persisted_task.as_ref().unwrap();
                let response =
                    crate::services::agent::work_loop::saved_response(db, &task.id, intent.user_id)
                        .await
                        .ok();
                let message = response
                    .as_ref()
                    .map(|r| r.message.clone())
                    .unwrap_or_else(|| "The task finished".into());
                if let (Some(session_id), Some(response)) = (&session_id, response) {
                    let metadata = serde_json::to_value(ApiResponse::from(response)).ok();
                    let _ = persist_assistant_message(
                        db,
                        session_id,
                        Some(&task.id),
                        &message,
                        metadata,
                    )
                    .await;
                }
                advance_intention_work(
                    db,
                    Some(&intent.id),
                    intent.user_id,
                    crate::services::agent::consciousness::IntentStatus::Completed,
                    Some(message),
                )
                .await;
            }
            RunningRecovery::RetryAccepted => {
                match store
                    .reclaim_running_to_accepted(&intent.id, intent.user_id)
                    .await
                {
                    Ok(true) => reclaimed += 1,
                    Ok(false) => {}
                    Err(error) => {
                        tracing::warn!(
                            %error,
                            intent_id = %intent.id,
                            "[Agent API] Boot restore: reclaim running intention failed"
                        );
                    }
                }
            }
            RunningRecovery::FailInterrupted => {
                match store
                    .transition(
                        &intent.id,
                        intent.user_id,
                        crate::services::agent::consciousness::IntentStatus::Failed,
                        None,
                        None,
                        Some("Interrupted before the task could be restored".into()),
                    )
                    .await
                {
                    Err(error) => {
                        tracing::warn!(
                            %error,
                            intent_id = %intent.id,
                            "[Agent API] Boot restore: fail interrupted intention failed"
                        );
                    }
                    _ => {
                        failed += 1;
                    }
                }
            }
        }
    }
    if reclaimed > 0 || failed > 0 {
        tracing::info!(
            reclaimed,
            failed,
            "[Agent API] Boot restore: resolved stranded Running intentions"
        );
    }
}

async fn latest_task_for_session(
    db: &DatabaseConnection,
    user_id: i32,
    session_id: &str,
) -> Option<crate::models::entities::agent_tasks::Model> {
    use crate::models::entities::agent_tasks;
    agent_tasks::Entity::find()
        .filter(agent_tasks::Column::UserId.eq(user_id))
        .filter(agent_tasks::Column::SessionId.eq(session_id))
        .order_by_desc(agent_tasks::Column::UpdatedAt)
        .one(db)
        .await
        .ok()
        .flatten()
}

pub(crate) fn wait_response_still_waiting(response_value: &Value) -> bool {
    response_value
        .pointer("/task/status")
        .and_then(|s| s.as_str())
        == Some("waiting_for_input")
}

/// Lightweight wait-loop for boot-restored tasks (same terminal guarantees as process_stream).
pub(crate) async fn spawn_restored_wait_loop(
    user_id: i32,
    task_id: String,
    session_id: String,
    run_id: String,
    tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
    ledger_db: Option<DatabaseConnection>,
    source_intent_id: Option<String>,
) {
    let Some(_registration) = WaitLoopRegistration::claim(&task_id) else {
        return;
    };
    loop {
        let (done_tx, done_rx) = tokio::sync::oneshot::channel::<serde_json::Value>();
        let _waiting = WaitingTaskRegistration::insert(
            &task_id,
            WaitingTaskCtx {
                registration: Arc::new(()),
                user_id,
                progress_tx: tx.clone(),
                done_tx,
                session_id: session_id.clone(),
            },
        );
        tracing::info!(
            task_id = %task_id,
            run_id = %run_id,
            "[Agent API] Restored wait-loop registered"
        );

        match tokio::time::timeout(tokio::time::Duration::from_secs(2), done_rx).await {
            Ok(Ok(response_value)) => {
                let still_waiting = wait_response_still_waiting(&response_value);
                if still_waiting {
                    if let Some(db) = &ledger_db {
                        if !session_id.is_empty() {
                            let msg = response_value
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or("More information is needed");
                            let metadata = session_metadata_with_run_identity(
                                Some(response_value.clone()),
                                &run_id,
                                &task_id,
                            );
                            let _ = persist_assistant_message(
                                db,
                                &session_id,
                                Some(&task_id),
                                msg,
                                Some(metadata),
                            )
                            .await;
                        }
                    }
                    continue;
                }
                let task_success = response_value
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                if let Some(db) = &ledger_db {
                    advance_intention_work(
                        db,
                        source_intent_id.as_deref(),
                        user_id,
                        if task_success {
                            crate::services::agent::consciousness::IntentStatus::Completed
                        } else {
                            crate::services::agent::consciousness::IntentStatus::Failed
                        },
                        response_value
                            .get("message")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    )
                    .await;
                }
                if let Some(db) = &ledger_db {
                    if !session_id.is_empty() {
                        let final_msg = response_value
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("The task finished");
                        let metadata = session_metadata_with_run_identity(
                            Some(response_value.clone()),
                            &run_id,
                            &task_id,
                        );
                        let _ = persist_assistant_message(
                            db,
                            &session_id,
                            Some(&task_id),
                            final_msg,
                            Some(metadata),
                        )
                        .await;
                    }
                }
                let _ = tx
                    .send(AgentProgressEvent::TaskCompleted {
                        task_id: task_id.clone(),
                        success: task_success,
                        response: Box::new(response_value),
                    })
                    .await;
                break;
            }
            Ok(Err(_)) => {
                let _ = take_waiting_task(&task_id, user_id).await;
                if let Some(db) = &ledger_db {
                    advance_intention_work(
                        db,
                        source_intent_id.as_deref(),
                        user_id,
                        crate::services::agent::consciousness::IntentStatus::Failed,
                        Some("The wait channel closed".into()),
                    )
                    .await;
                }
                let _ = tx.send(wait_loop_channel_dropped_event(&task_id)).await;
                break;
            }
            Err(_) => {
                let _ = take_waiting_task(&task_id, user_id).await;
                let mut current_task =
                    crate::services::agent::executor::refresh_task_for_user(&task_id, user_id)
                        .await;

                if let (Some(db), Some(task)) = (&ledger_db, &current_task) {
                    if task
                        .recipe
                        .as_ref()
                        .is_some_and(crate::services::agent::work_loop::is_work_recipe)
                    {
                        if let Some(question) = &task.pending_question {
                            let _ = crate::services::agent::work_loop::expire_question(
                                db,
                                &task_id,
                                user_id,
                                &question.question_id,
                            )
                            .await;
                            current_task = crate::services::agent::executor::refresh_task_for_user(
                                &task_id, user_id,
                            )
                            .await;
                        }
                    }
                }

                if let Some(task) = current_task.as_ref() {
                    if !task
                        .recipe
                        .as_ref()
                        .is_some_and(crate::services::agent::work_loop::is_work_recipe)
                        && task.status == crate::services::agent::types::TaskStatus::WaitingForInput
                        && task
                            .pending_question
                            .as_ref()
                            .is_some_and(|q| q.is_expired(chrono::Utc::now()))
                    {
                        if let Some(db) = &ledger_db {
                            advance_intention_work(
                                db,
                                source_intent_id.as_deref(),
                                user_id,
                                crate::services::agent::consciousness::IntentStatus::Failed,
                                Some("Waiting for input timed out".into()),
                            )
                            .await;
                        }
                        let response_value = json!({
                            "success": false,
                            "message": "Waiting for input timed out",
                            "code": "wait_input_timeout",
                            "streamTerminal": true,
                            "task": { "taskId": task_id, "status": "failed" }
                        });
                        let _ = tx
                            .send(AgentProgressEvent::TaskCompleted {
                                task_id: task_id.clone(),
                                success: false,
                                response: Box::new(response_value),
                            })
                            .await;
                        if let Some(mut t) =
                            crate::services::agent::executor::get_task_for_user(&task_id, user_id)
                                .await
                        {
                            t.status = crate::services::agent::types::TaskStatus::Failed;
                            t.error = Some("Waiting for input timed out".into());
                            t.completed_at = Some(chrono::Utc::now());
                            t.pending_question = None;
                            {
                                let mut store =
                                    crate::services::agent::executor::TASK_STORE.write().await;
                                store.store(user_id, t.clone());
                            }
                            crate::services::agent::executor::persist_task_async(user_id, t);
                        }
                        break;
                    }
                }

                if current_task.as_ref().is_some_and(|task| {
                    matches!(
                        task.status,
                        crate::services::agent::types::TaskStatus::Pending
                            | crate::services::agent::types::TaskStatus::Running
                            | crate::services::agent::types::TaskStatus::WaitingForInput
                            | crate::services::agent::types::TaskStatus::Paused
                    )
                }) {
                    continue;
                }

                let (response_value, task_success) = if let Some(task) = current_task {
                    let task_success =
                        task.status == crate::services::agent::types::TaskStatus::Completed;
                    let message = task.error.clone().unwrap_or_else(|| {
                        if task_success {
                            "The task finished".into()
                        } else {
                            "Processing failed".into()
                        }
                    });
                    let saved = if task
                        .recipe
                        .as_ref()
                        .is_some_and(crate::services::agent::work_loop::is_work_recipe)
                    {
                        if let Some(db) = &ledger_db {
                            crate::services::agent::work_loop::saved_response(db, &task_id, user_id)
                                .await
                                .ok()
                                .and_then(|response| {
                                    serde_json::to_value(ApiResponse::from(response)).ok()
                                })
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    (
                        saved.unwrap_or_else(
                            || json!({ "success": task_success, "message": message, "task": task }),
                        ),
                        task_success,
                    )
                } else {
                    (
                        json!({
                            "success": false,
                            "message": "The task is no longer available",
                            "code": "task_unavailable",
                            "task": { "taskId": task_id, "status": "failed" }
                        }),
                        false,
                    )
                };
                if let Some(db) = &ledger_db {
                    advance_intention_work(
                        db,
                        source_intent_id.as_deref(),
                        user_id,
                        if task_success {
                            crate::services::agent::consciousness::IntentStatus::Completed
                        } else {
                            crate::services::agent::consciousness::IntentStatus::Failed
                        },
                        response_value
                            .get("message")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    )
                    .await;
                }
                if let Some(db) = &ledger_db {
                    if !session_id.is_empty() {
                        let metadata = session_metadata_with_run_identity(
                            Some(response_value.clone()),
                            &run_id,
                            &task_id,
                        );
                        let _ = persist_assistant_message(
                            db,
                            &session_id,
                            Some(&task_id),
                            response_value
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or("The task finished"),
                            Some(metadata),
                        )
                        .await;
                    }
                }
                let _ = tx
                    .send(AgentProgressEvent::TaskCompleted {
                        task_id: task_id.clone(),
                        success: task_success,
                        response: Box::new(response_value),
                    })
                    .await;
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RunningRecovery, classify_stranded_running, wait_response_still_waiting};
    #[tokio::test]
    async fn abort_wait_loop_releases_context_and_progress_sender() {
        let task_id = uuid::Uuid::new_v4().to_string();
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let execution = tokio::spawn(super::spawn_restored_wait_loop(
            7319,
            task_id.clone(),
            "session".into(),
            "run".into(),
            tx,
            None,
            None,
        ));
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if super::WAITING_TASKS.lock().unwrap().contains_key(&task_id) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        execution.abort();
        let _ = execution.await;
        assert!(
            super::take_waiting_task(&task_id, 7319).await.is_none(),
            "aborted wait loop retained its context"
        );
        assert!(
            rx.recv().await.is_none(),
            "retained progress sender kept the run alive"
        );
    }

    #[tokio::test]
    async fn old_wait_registration_preserves_replacement_and_owner_check() {
        let task_id = uuid::Uuid::new_v4().to_string();
        let context = |user_id| {
            let (tx, _) = tokio::sync::mpsc::channel(1);
            let (done_tx, _) = tokio::sync::oneshot::channel();
            super::WaitingTaskCtx {
                registration: std::sync::Arc::new(()),
                user_id,
                progress_tx: tx,
                done_tx,
                session_id: "session".into(),
            }
        };
        let old = super::WaitingTaskRegistration::insert(&task_id, context(7319));
        let replacement = super::WaitingTaskRegistration::insert(&task_id, context(7320));
        drop(old);
        assert!(super::take_waiting_task(&task_id, 7319).await.is_none());
        assert_eq!(
            super::take_waiting_task(&task_id, 7320)
                .await
                .unwrap()
                .user_id,
            7320
        );
        drop(replacement);
        assert!(!super::WAITING_TASKS.lock().unwrap().contains_key(&task_id));
    }

    #[test]
    fn recovery_does_not_attach_two_waiters_and_releases_on_exit() {
        let task_id = uuid::Uuid::new_v4().to_string();
        let first = super::WaitLoopRegistration::claim(&task_id).unwrap();
        assert!(super::WaitLoopRegistration::claim(&task_id).is_none());
        drop(first);
        assert!(super::WaitLoopRegistration::claim(&task_id).is_some());
    }
    use serde_json::json;

    #[test]
    fn stranded_running_without_a_task_may_retry() {
        assert_eq!(
            classify_stranded_running(false, None, false),
            RunningRecovery::RetryAccepted
        );
    }

    #[test]
    fn waiting_tasks_are_restored_not_retried() {
        assert_eq!(
            classify_stranded_running(true, Some("cancelled"), false),
            RunningRecovery::RestoreWaiting
        );
        assert_eq!(
            classify_stranded_running(false, Some("waiting_for_input"), false),
            RunningRecovery::RestoreWaiting
        );
    }

    #[test]
    fn durable_work_survives_restart_before_waiting_or_after_completion() {
        assert_eq!(
            classify_stranded_running(false, Some("running"), true),
            RunningRecovery::KeepRunningWork
        );
        assert_eq!(
            classify_stranded_running(false, Some("completed"), true),
            RunningRecovery::RestoreCompletedWork
        );
        assert_eq!(
            classify_stranded_running(false, Some("cancelled"), true),
            RunningRecovery::FailInterrupted
        );
    }

    #[test]
    fn interrupted_executor_tasks_fail_closed() {
        for status in ["pending", "running", "cancelled", "completed", "failed"] {
            assert_eq!(
                classify_stranded_running(false, Some(status), false),
                RunningRecovery::FailInterrupted,
                "{status}"
            );
        }
    }

    #[test]
    fn multi_round_wait_keeps_the_loop_open() {
        assert!(wait_response_still_waiting(&json!({
            "success": true,
            "message": "还需要一个日期",
            "task": { "taskId": "t1", "status": "waiting_for_input" }
        })));
        assert!(!wait_response_still_waiting(&json!({
            "success": true,
            "message": "办完了",
            "task": { "taskId": "t1", "status": "completed" }
        })));
    }
}
