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

    for (user_id, mut task) in waiting {
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
            continue;
        }

        let session_id = crate::services::agent::executor::task_store::session_id_from_lane_id(
            task.lane_id.as_deref(),
        );
        let run = create_run(user_id, session_id.clone()).await;
        let run_id = run.run_id().to_string();
        let task_id = task.task_id.clone();

        let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentProgressEvent>(32);
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
        tokio::spawn(async move {
            spawn_restored_wait_loop(user_id, task_id, session_id_loop, run_id, tx).await;
        });
    }
}

/// Lightweight wait-loop for boot-restored tasks (same terminal guarantees as process_stream).
pub(crate) async fn spawn_restored_wait_loop(
    user_id: i32,
    task_id: String,
    session_id: String,
    run_id: String,
    tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
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
                let still_waiting = response_value
                    .pointer("/task/status")
                    .and_then(|s| s.as_str())
                    == Some("waiting_for_input");
                if still_waiting {
                    continue;
                }
                let task_success = response_value
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
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
                            "任务已完成".into()
                        } else {
                            "任务未完成".into()
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
