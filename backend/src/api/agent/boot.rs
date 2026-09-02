//! Agent API — boot
use super::*;

// Boot recovery for waiting tasks

/// After process restart, re-create run hubs and wait-loops for
/// `waiting_for_input` tasks so answer/subscribe keep working and notifications
/// stay consistent. Called once from `main` after `init_task_store`.
pub async fn restore_waiting_runs_after_boot() {
    let waiting = crate::services::agent::executor::task_store::list_waiting_tasks_snapshot().await;
    if waiting.is_empty() {
        tracing::info!("[Agent API] Boot restore: no waiting_for_input tasks");
        return;
    }

    tracing::info!(
        count = waiting.len(),
        "[Agent API] Boot restore: re-creating run hubs for waiting tasks"
    );
    let ledger_db = crate::services::tapp_registry::database().await.ok();

    for (user_id, mut task) in waiting {
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
        if task
            .pending_question
            .as_ref()
            .is_some_and(|q| q.is_expired(chrono::Utc::now()))
        {
            tracing::warn!(
                task_id = %task.task_id,
                "[Agent API] Boot restore: expiring abandoned waiting question"
            );
            task.status = crate::services::agent::types::TaskStatus::Failed;
            task.error = Some("等待用户输入已超时（服务重启后发现已过期）".into());
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
                    Some("等待用户输入已超时（服务重启后发现已过期）".into()),
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
    RetryAccepted,
    FailInterrupted,
}

/// Decide whether a stranded Running intention is safe to retry.
/// Waiting tasks keep the original Work. A persisted executor task means
/// steps may already have run — fail closed instead of repeating side effects.
pub fn classify_stranded_running(
    has_waiting_task: bool,
    persisted_task_status: Option<&str>,
) -> RunningRecovery {
    if has_waiting_task || persisted_task_status == Some("waiting_for_input") {
        return RunningRecovery::RestoreWaiting;
    }
    match persisted_task_status {
        None => RunningRecovery::RetryAccepted,
        Some("pending" | "running" | "cancelled" | "completed" | "failed") => {
            RunningRecovery::FailInterrupted
        }
        Some(_) => RunningRecovery::FailInterrupted,
    }
}

/// Running intentions with no restored wait-loop never finish. Put them back
/// to Accepted so the autonomy tick (or the proposal card) can claim again.
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
        let persisted_status = match session_id.as_deref() {
            Some(session_id) => {
                latest_task_status_for_session(db, intent.user_id, session_id).await
            }
            None => None,
        };
        match classify_stranded_running(attached_waiting, persisted_status.as_deref()) {
            RunningRecovery::RestoreWaiting => continue,
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
                if let Err(error) = store
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
                    tracing::warn!(
                        %error,
                        intent_id = %intent.id,
                        "[Agent API] Boot restore: fail interrupted intention failed"
                    );
                } else {
                    failed += 1;
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

async fn latest_task_status_for_session(
    db: &DatabaseConnection,
    user_id: i32,
    session_id: &str,
) -> Option<String> {
    use crate::models::entities::agent_tasks;
    agent_tasks::Entity::find()
        .filter(agent_tasks::Column::UserId.eq(user_id))
        .filter(agent_tasks::Column::SessionId.eq(session_id))
        .order_by_desc(agent_tasks::Column::UpdatedAt)
        .one(db)
        .await
        .ok()
        .flatten()
        .map(|row| row.status)
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
    loop {
        let (done_tx, done_rx) = tokio::sync::oneshot::channel::<serde_json::Value>();
        {
            let mut map = WAITING_TASKS.write().await;
            map.insert(
                task_id.clone(),
                WaitingTaskCtx {
                    user_id,
                    progress_tx: tx.clone(),
                    done_tx,
                    session_id: session_id.clone(),
                },
            );
        }
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
                                .unwrap_or("需要更多信息");
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
                        Some("等待通道已关闭".into()),
                    )
                    .await;
                }
                let _ = tx.send(wait_loop_channel_dropped_event(&task_id)).await;
                break;
            }
            Err(_) => {
                let _ = take_waiting_task(&task_id, user_id).await;
                let current_task =
                    crate::services::agent::executor::refresh_task_for_user(&task_id, user_id)
                        .await;

                if let Some(task) = current_task.as_ref() {
                    if task.status == crate::services::agent::types::TaskStatus::WaitingForInput
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
                                Some("等待用户输入已超时".into()),
                            )
                            .await;
                        }
                        let response_value = json!({
                            "success": false,
                            "message": "等待用户输入已超时",
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
                            t.error = Some("等待用户输入已超时".into());
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
                    (
                        json!({ "success": task_success, "message": message, "task": task }),
                        task_success,
                    )
                } else {
                    (
                        json!({
                            "success": false,
                            "message": "任务状态已不可用",
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
    use super::{classify_stranded_running, wait_response_still_waiting, RunningRecovery};
    use serde_json::json;

    #[test]
    fn stranded_running_without_a_task_may_retry() {
        assert_eq!(
            classify_stranded_running(false, None),
            RunningRecovery::RetryAccepted
        );
    }

    #[test]
    fn waiting_tasks_are_restored_not_retried() {
        assert_eq!(
            classify_stranded_running(true, Some("cancelled")),
            RunningRecovery::RestoreWaiting
        );
        assert_eq!(
            classify_stranded_running(false, Some("waiting_for_input")),
            RunningRecovery::RestoreWaiting
        );
    }

    #[test]
    fn interrupted_executor_tasks_fail_closed() {
        for status in ["pending", "running", "cancelled", "completed", "failed"] {
            assert_eq!(
                classify_stranded_running(false, Some(status)),
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
