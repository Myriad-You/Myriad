//! Agent API — process
use super::*;
use crate::error::HttpError;

/// Map an agent turn failure to an HTTP error.
///
/// A turn now reserves AI quota before it runs, so "you are out of budget" and
/// "the agent broke" arrive on the same `Err(String)` channel. Reporting a
/// budget rejection as a 500 would both mislead the user and hide a retryable
/// condition from the client, so client limits become 429 and carry the quota
/// code through for the UI to act on.
fn agent_turn_error(error: String) -> HttpError {
    if crate::services::ai_quota::is_client_limit_message(&error) {
        let code = error
            .split(':')
            .next()
            .unwrap_or("AI_QUOTA_EXCEEDED")
            .to_string();
        tracing::info!(error = %error, "[Agent API] Turn rejected by AI quota");
        return HttpError::from((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": error, "code": code })),
        ));
    }
    tracing::error!(error = %error, "[Agent API] Processing failed");
    HttpError::from((
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": error })),
    ))
}

// API 端点

/// 处理自然语言请求
/// POST /api/agent/process
pub async fn process(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<ProcessRequest>,
) -> Result<Json<ApiResponse>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    validate_input(&req.input)?;

    tracing::info!(
        user_id = user_id,
        input_len = req.input.len(),
        "[Agent API] Processing request"
    );

    let client_session_id = req
        .context
        .as_ref()
        .and_then(|c| c.session_id.as_deref())
        .map(str::to_string);
    let session_id = ensure_session(&db, client_session_id.as_deref(), user_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "[Agent API] Failed to ensure session");
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error })),
            ))
        })?;
    let lane_key = LaneQueue::make_lane_key(user_id, Some(&session_id));
    let conversation_history = load_session_history(&db, &session_id, 20).await;
    if let Err(error) = persist_user_message(&db, &session_id, &req.input).await {
        tracing::warn!(%error, "[Agent API] Failed to persist user message");
    }

    let mut user_request = UserRequest {
        raw_input: req.input,
        timestamp: chrono::Utc::now(),
        user_id,
        context: req.context.map(build_request_context),
    };

    if let Some(ref mut ctx) = user_request.context {
        ctx.lane_key = Some(lane_key.clone());
        ctx.session_id = Some(session_id.clone());
        ctx.conversation_history = if conversation_history.is_empty() {
            None
        } else {
            Some(conversation_history)
        };
    } else {
        user_request.context = Some(RequestContext {
            lane_key: Some(lane_key.clone()),
            session_id: Some(session_id.clone()),
            conversation_history: if conversation_history.is_empty() {
                None
            } else {
                Some(conversation_history)
            },
            ..Default::default()
        });
    }

    // 获取 Lane Queue 执行许可（同一用户串行，全局并发上限 4）
    let _guard = LANE_QUEUE
        .acquire_timeout(
            &lane_key,
            std::time::Duration::from_secs(LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS),
        )
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "[Agent API] Queue acquisition failed");
            HttpError::from((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": e })),
            ))
        })?;

    // 创建 Agent 并处理请求
    let agent = Agent::new(db.clone()).await;
    let response = agent.process(user_request).await.map_err(agent_turn_error)?;

    let mut api_response: ApiResponse = response.into();
    let metadata = json!({
        "suggestions": &api_response.suggestions,
        "dataDisplay": &api_response.data_display,
        "frontendAction": &api_response.frontend_action,
        "data": &api_response.data,
    });
    if let Err(error) = persist_assistant_message(
        &db,
        &session_id,
        api_response.task.as_ref().map(|task| task.task_id.as_str()),
        &api_response.message,
        Some(metadata),
    )
    .await
    {
        tracing::warn!(%error, "[Agent API] Failed to persist assistant message");
    }
    api_response.session_id = Some(session_id);

    Ok(Json(api_response))
}

/// 流式处理自然语言请求（带实时进度更新）
/// POST /api/agent/process/stream
pub async fn process_stream(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<ProcessRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    validate_input(&req.input)?;

    tracing::info!(
        user_id = user_id,
        input_len = req.input.len(),
        "[Agent API] Processing request with streaming"
    );

    let client_session_id = req
        .context
        .as_ref()
        .and_then(|c| c.session_id.as_deref())
        .map(|s| s.to_string());

    // 确保会话存在（自动创建或验证已有会话）
    let session_id = match ensure_session(&db, client_session_id.as_deref(), user_id).await {
        Ok(sid) => sid,
        Err(e) => {
            tracing::warn!("[Agent API] Failed to ensure session: {}", e);
            // 不阻塞主流程，降级为无会话模式
            String::new()
        }
    };

    let has_session = !session_id.is_empty();

    // 从数据库加载会话历史（替代前端传入的 conversation_history）
    let conversation_history = if has_session {
        let history = load_session_history(&db, &session_id, 20).await;
        if !history.is_empty() {
            tracing::info!(
                session_id = %session_id,
                history_count = history.len(),
                "[Agent API] Loaded conversation history from DB"
            );
        }
        Some(history)
    } else {
        None
    };

    if has_session {
        if let Err(e) = persist_user_message(&db, &session_id, &req.input).await {
            tracing::warn!("[Agent API] Failed to persist user message: {}", e);
        }
    }

    let lane_key = LaneQueue::make_lane_key(user_id, Some(&session_id));

    let mut user_request = UserRequest {
        raw_input: req.input,
        timestamp: chrono::Utc::now(),
        user_id,
        context: req.context.map(build_request_context),
    };

    // 将 lane_key 和 session_id 注入到请求上下文
    if let Some(ref mut ctx) = user_request.context {
        ctx.lane_key = Some(lane_key.clone());
        if has_session {
            ctx.session_id = Some(session_id.clone());
        }
        // 用数据库加载的历史覆盖前端传入的（服务端为 source of truth）
        if let Some(history) = conversation_history {
            ctx.conversation_history = Some(history);
        }
    } else {
        let mut new_ctx = RequestContext {
            lane_key: Some(lane_key.clone()),
            ..Default::default()
        };
        if has_session {
            new_ctx.session_id = Some(session_id.clone());
        }
        if let Some(history) = conversation_history {
            new_ctx.conversation_history = Some(history);
        }
        user_request.context = Some(new_ctx);
    }

    // 后端 run 独立于本次 HTTP 连接；前端只订阅事件。
    // 刻意不在 SSE 断连时取消任务：刷新 / reattach 依赖 run 继续存活；
    // 用户中断走 cancelTask API + is_cancelled 协作取消。
    let run = create_run(user_id, has_session.then_some(session_id.clone())).await;
    let run_id_for_meta = run.run_id().to_string();
    // 注入 run_id，供确认手持（confirmation）复用同一 run hub / 通知身份
    if let Some(ref mut ctx) = user_request.context {
        ctx.run_id = Some(run_id_for_meta.clone());
    }

    // Agent/executor 继续使用有背压的 mpsc；独立转发器负责写入 run hub。
    // On TaskCreated, persist runId/taskId into session history so mid-run
    // panel refresh can reattach (criterion 4) before wait/final complete.
    let (tx, rx) = tokio::sync::mpsc::channel::<ProgressEvent>(32);
    let run_for_forwarder = run.clone();
    let session_for_identity = session_id.clone();
    let db_for_identity = db.clone();
    let run_id_for_identity = run_id_for_meta.clone();
    tokio::spawn(async move {
        let mut rx = rx;
        let mut mid_run_identity_persisted = false;
        while let Some(event) = rx.recv().await {
            // Snapshot identity fields before moving event into publish.
            let mid_run_identity =
                if !mid_run_identity_persisted && !session_for_identity.is_empty() {
                    match &event {
                        AgentProgressEvent::TaskCreated {
                            task_id, message, ..
                        } => Some((task_id.clone(), message.clone())),
                        _ => None,
                    }
                } else {
                    None
                };

            // Criterion 5: live fanout first — never await DB on this hot path.
            run_for_forwarder.publish(event).await;

            // Criterion 4: best-effort session identity for reattach; fire-and-forget.
            if let Some((task_id, message)) = mid_run_identity {
                mid_run_identity_persisted = true;
                let db = db_for_identity.clone();
                let session_id = session_for_identity.clone();
                let run_id = run_id_for_identity.clone();
                tokio::spawn(async move {
                    let metadata = session_metadata_with_run_identity(
                        Some(json!({
                            "task": {
                                "taskId": task_id,
                                "status": "running",
                            },
                        })),
                        &run_id,
                        &task_id,
                    );
                    let _ = persist_assistant_message(
                        &db,
                        &session_id,
                        Some(&task_id),
                        &message,
                        Some(metadata),
                    )
                    .await;
                });
            }
        }
    });

    // 在后台执行任务
    let db_clone = db.clone();
    let session_id_clone = session_id.clone();
    let queue = LANE_QUEUE.clone();
    // tx 会被移动到 spawn 中，确保 channel 在任务完成前不会关闭
    tokio::spawn(async move {
        // 获取 Lane Queue 执行许可（同一用户串行，全局并发上限 4）
        // 注意：进入 wait-for-input 后必须释放，否则最多 4 个等待任务会堵死全局槽位
        {
            let qs = queue.get_status().await;
            if qs.available_permits == 0 || qs.waiting > 0 {
                let ahead = qs.waiting.saturating_add(1);
                let _ = tx
                    .send(AgentProgressEvent::Progress {
                        progress: 0,
                        completed_steps: 0,
                        total_steps: 0,
                        message: format!("排队中（前方约 {} 个任务）…", ahead),
                    })
                    .await;
            }
        }
        let mut lane_guard = match queue
            .acquire_timeout(
                &lane_key,
                std::time::Duration::from_secs(LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS),
            )
            .await
        {
            Ok(guard) => Some(guard),
            Err(e) => {
                let _ = tx
                    .send(AgentProgressEvent::Error {
                        task_id: None,
                        message: e,
                        code: "QUEUE_FULL".to_string(),
                    })
                    .await;
                return;
            }
        };

        let agent = Agent::new(db_clone.clone()).await;

        // 发送 session_id 给前端（让前端后续请求带上）
        if !session_id_clone.is_empty() {
            let _ = tx
                .send(AgentProgressEvent::SessionCreated {
                    session_id: session_id_clone.clone(),
                })
                .await;
        }

        // 使用带进度回调的处理方法
        match agent.process_with_progress(user_request, tx.clone()).await {
            Ok(response) => {
                let api_response: ApiResponse = response.into();
                let task_id = api_response
                    .task
                    .as_ref()
                    .map(|t| t.task_id.clone())
                    .unwrap_or_default();
                let success = api_response.success;

                // 检查任务是否在等待用户输入
                let is_waiting = api_response
                    .task
                    .as_ref()
                    .map(|t| t.status == "waiting_for_input")
                    .unwrap_or(false);

                if is_waiting && !task_id.is_empty() {
                    // 任务需要用户回答。前端已通过 waiting_for_input SSE 事件收到问题。
                    // 释放全局 lane 许可，避免无限等待占满 Semaphore(max=4)。
                    // resume 执行在 answer_stream 中重新获取许可。
                    drop(lane_guard.take());
                    tracing::info!(
                        task_id = %task_id,
                        "[Agent API] Released lane permit while waiting for user input"
                    );

                    // 持久化首次问题消息（含 runId，供刷新后 reattach）
                    if !session_id_clone.is_empty() {
                        let metadata = session_metadata_with_run_identity(
                            Some(json!({
                                "suggestions": &api_response.suggestions,
                                "data": &api_response.data,
                                "task": {
                                    "taskId": task_id,
                                    "status": "waiting_for_input",
                                    "pendingQuestion": api_response.task.as_ref().and_then(|t| t.pending_question.clone()),
                                },
                            })),
                            &run_id_for_meta,
                            &task_id,
                        );
                        let _ = persist_assistant_message(
                            &db_clone,
                            &session_id_clone,
                            Some(&task_id),
                            &api_response.message,
                            Some(metadata),
                        )
                        .await;
                    }

                    loop {
                        let (done_tx, done_rx) =
                            tokio::sync::oneshot::channel::<serde_json::Value>();
                        {
                            let mut map = WAITING_TASKS.write().await;
                            map.insert(
                                task_id.clone(),
                                WaitingTaskCtx {
                                    user_id, // process_stream 的 user_id 在外层 spawn 中可用
                                    progress_tx: tx.clone(),
                                    done_tx,
                                    session_id: session_id_clone.clone(),
                                },
                            );
                        }
                        tracing::info!(task_id = %task_id, "[Agent API] Task waiting for user input, keeping run alive");

                        // 本副本回答通过 oneshot 即时返回；跨副本回答没有本地
                        // WaitingTaskCtx，因此每 2 秒从权威数据库刷新一次。
                        match tokio::time::timeout(tokio::time::Duration::from_secs(2), done_rx)
                            .await
                        {
                            Ok(Ok(response_value)) => {
                                // 检查任务是否仍在等待用户输入（多轮提问）
                                let still_waiting = response_value
                                    .pointer("/task/status")
                                    .and_then(|s| s.as_str())
                                    == Some("waiting_for_input");

                                if still_waiting {
                                    // 仍有新问题需要用户回答，持久化中间状态后继续等待
                                    // 必须带 top-level runId/taskId（与首次 wait 一致），否则
                                    // 多轮后最新消息缺少 run 身份，刷新无法 re-subscribe。
                                    if !session_id_clone.is_empty() {
                                        let msg = response_value
                                            .get("message")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("需要更多信息");
                                        let metadata = session_metadata_with_run_identity(
                                            Some(response_value.clone()),
                                            &run_id_for_meta,
                                            &task_id,
                                        );
                                        let _ = persist_assistant_message(
                                            &db_clone,
                                            &session_id_clone,
                                            Some(&task_id),
                                            msg,
                                            Some(metadata),
                                        )
                                        .await;
                                    }
                                    tracing::info!(
                                        task_id = %task_id,
                                        "[Agent API] Task still waiting after answer, looping for next round"
                                    );
                                    continue;
                                }

                                // 任务真正完成
                                tracing::info!(
                                    "[Agent API] Answer result received, sending TaskCompleted"
                                );
                                let task_success = response_value
                                    .get("success")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(true);

                                // 持久化最终结果（同样带 runId/taskId）
                                if !session_id_clone.is_empty() {
                                    let final_msg = response_value
                                        .get("message")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("任务已完成");
                                    let metadata = session_metadata_with_run_identity(
                                        Some(response_value.clone()),
                                        &run_id_for_meta,
                                        &task_id,
                                    );
                                    let _ = persist_assistant_message(
                                        &db_clone,
                                        &session_id_clone,
                                        Some(&task_id),
                                        final_msg,
                                        Some(metadata),
                                    )
                                    .await;
                                }

                                if let Err(e) = tx
                                    .send(AgentProgressEvent::TaskCompleted {
                                        task_id: task_id.clone(),
                                        success: task_success,
                                        response: Box::new(response_value),
                                    })
                                    .await
                                {
                                    tracing::error!(
                                        task_id = %task_id,
                                        error = %e,
                                        "[Agent API] Failed to publish TaskCompleted to run hub"
                                    );
                                }
                                break;
                            }
                            Ok(Err(_)) => {
                                // done_tx 被丢弃：必须发布终态，否则 re-subscribe 会永久挂起
                                tracing::warn!(
                                    task_id = %task_id,
                                    "[Agent API] Answer sender dropped unexpectedly; terminalizing run"
                                );
                                let _ = take_waiting_task(&task_id, user_id).await;
                                let _ = tx.send(wait_loop_channel_dropped_event(&task_id)).await;
                                break;
                            }
                            Err(_) => {
                                tracing::debug!(task_id = %task_id, "[Agent API] Polling persisted waiting task state");
                                let _ = take_waiting_task(&task_id, user_id).await;

                                // 强制查数据库，不能让本副本的 waiting 缓存遮蔽
                                // 另一副本已写入的完成/失败状态。
                                let current_task =
                                    crate::services::agent::executor::refresh_task_for_user(
                                        &task_id, user_id,
                                    )
                                    .await;

                                // 问题过期：干净退出，释放 run，避免永久轮询
                                if let Some(task) = current_task.as_ref() {
                                    if task.status
                                        == crate::services::agent::types::TaskStatus::WaitingForInput
                                    {
                                        let expired = task
                                            .pending_question
                                            .as_ref()
                                            .is_some_and(|q| q.is_expired(chrono::Utc::now()));
                                        if expired {
                                            tracing::warn!(
                                                task_id = %task_id,
                                                "[Agent API] Waiting question expired; closing wait loop"
                                            );
                                            let response_value = json!({
                                                "success": false,
                                                "message": "等待用户输入已超时",
                                                "streamTerminal": true,
                                                "task": {
                                                    "taskId": task_id.clone(),
                                                    "status": "failed"
                                                }
                                            });
                                            let _ = tx
                                                .send(AgentProgressEvent::TaskCompleted {
                                                    task_id: task_id.clone(),
                                                    success: false,
                                                    response: Box::new(response_value),
                                                })
                                                .await;
                                            // 标记任务失败，避免幽灵 waiting
                                            if let Some(mut t) = crate::services::agent::executor::get_task_for_user(
                                                &task_id, user_id,
                                            )
                                            .await
                                            {
                                                t.status = crate::services::agent::types::TaskStatus::Failed;
                                                t.error = Some("等待用户输入已超时".into());
                                                t.completed_at = Some(chrono::Utc::now());
                                                t.pending_question = None;
                                                {
                                                    let mut store =
                                                        crate::services::agent::executor::TASK_STORE
                                                            .write()
                                                            .await;
                                                    store.store(user_id, t.clone());
                                                }
                                                crate::services::agent::executor::persist_task_async(
                                                    user_id, t,
                                                );
                                            }
                                            break;
                                        }
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
                                    // 等待输入不是失败或完成。继续保持后端 run 与回答入口，
                                    // 下一轮重新注册 oneshot；前端是否在线不影响任务状态。
                                    tracing::info!(
                                        task_id = %task_id,
                                        "[Agent API] Task still waiting for input; keeping run alive"
                                    );
                                    continue;
                                }

                                let (response_value, task_success) =
                                    if let Some(task) = current_task {
                                        let task_success = task.status
                                            == crate::services::agent::types::TaskStatus::Completed;
                                        let message = task.error.clone().unwrap_or_else(|| {
                                            if task_success {
                                                "任务已完成".to_string()
                                            } else {
                                                "任务未完成".to_string()
                                            }
                                        });
                                        (
                                            json!({
                                                "success": task_success,
                                                "message": message,
                                                "task": task,
                                            }),
                                            task_success,
                                        )
                                    } else {
                                        (
                                            json!({
                                                "success": false,
                                                "message": "任务状态已不可用",
                                                "task": {
                                                    "taskId": task_id.clone(),
                                                    "status": "failed"
                                                }
                                            }),
                                            false,
                                        )
                                    };

                                if let Err(e) = tx
                                    .send(AgentProgressEvent::TaskCompleted {
                                        task_id: task_id.clone(),
                                        success: task_success,
                                        response: Box::new(response_value),
                                    })
                                    .await
                                {
                                    tracing::error!(
                                        task_id = %task_id,
                                        error = %e,
                                        "[Agent API] Failed to send timeout TaskCompleted"
                                    );
                                }
                                break;
                            }
                        }
                    }
                } else {
                    // 非 waiting 路径：正常流程结束时 guard 会在 spawn 结束时 drop
                    let _ = lane_guard.take();
                    // 正常流程：立即发送 TaskCompleted

                    // 持久化 assistant 消息（含 runId/taskId，供会话恢复）
                    if !session_id_clone.is_empty() {
                        let metadata = json!({
                            "suggestions": &api_response.suggestions,
                            "dataDisplay": &api_response.data_display,
                            "frontendAction": &api_response.frontend_action,
                            "data": &api_response.data,
                            "runId": run_id_for_meta,
                            "taskId": if task_id.is_empty() { Value::Null } else { json!(task_id) },
                        });
                        if let Err(e) = persist_assistant_message(
                            &db_clone,
                            &session_id_clone,
                            if task_id.is_empty() {
                                None
                            } else {
                                Some(&task_id)
                            },
                            &api_response.message,
                            Some(metadata),
                        )
                        .await
                        {
                            tracing::warn!(
                                "[Agent API] Failed to persist assistant message: {}",
                                e
                            );
                        }
                    }

                    let response_value = serde_json::to_value(&api_response)
                        .unwrap_or_else(|_| json!({"error": "serialization failed"}));

                    tracing::info!("[Agent API] Sending TaskCompleted event");
                    let send_result = tx
                        .send(AgentProgressEvent::TaskCompleted {
                            task_id,
                            success,
                            response: Box::new(response_value),
                        })
                        .await;
                    if let Err(e) = send_result {
                        tracing::error!(error = %e, "[Agent API] Failed to send TaskCompleted event");
                    } else {
                        tracing::info!("[Agent API] TaskCompleted event sent successfully");
                    }
                }
            }
            Err(e) => {
                // A turn reserves AI quota before it runs, so an exhausted
                // budget arrives here alongside real faults. It is neither an
                // error to log at ERROR nor a "处理失败" for the transcript.
                let quota_rejected = crate::services::ai_quota::is_client_limit_message(&e);
                let code = if quota_rejected {
                    tracing::info!(error = %e, "[Agent API] Turn rejected by AI quota");
                    e.split(':').next().unwrap_or("AI_QUOTA_EXCEEDED").to_string()
                } else {
                    tracing::error!(error = %e, "[Agent API] Processing failed, sending error event");
                    "PROCESSING_ERROR".to_string()
                };

                // 持久化错误消息
                if !session_id_clone.is_empty() {
                    let text = if quota_rejected {
                        e.clone()
                    } else {
                        format!("处理失败: {}", e)
                    };
                    let _ = persist_assistant_message(
                        &db_clone,
                        &session_id_clone,
                        None,
                        &text,
                        Some(json!({"error": true, "code": code})),
                    )
                    .await;
                }

                let _ = tx
                    .send(AgentProgressEvent::Error {
                        task_id: None,
                        message: e.clone(),
                        code,
                    })
                    .await;
            }
        }
        // tx 在这里被 drop，channel 关闭，SSE 流结束
    });

    // SSE 只是 run hub 的一个订阅者；连接被关闭不会触碰后台 sender 或执行任务。
    let stream = agent_run_event_stream(run);

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

/// 重新订阅一个已存在的 Agent run。
/// GET /api/agent/runs/{run_id}/stream
pub async fn subscribe_run_stream(
    Extension(claims): Extension<Claims>,
    Path(run_id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    let run = get_run_for_user(&run_id, user_id).await.ok_or_else(|| HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Run not found, expired, or access denied" })),
        )))?;

    Ok(Sse::new(agent_run_event_stream(run))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

/// 获取任务状态
/// GET /api/agent/tasks/{task_id}
pub async fn get_task(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;

    tracing::debug!(
        user_id = user_id,
        task_id = %task_id,
        "[Agent API] Getting task status"
    );

    // 带所有权校验，防止 IDOR
    let agent = Agent::new(db).await;
    let task = agent.get_task_for_user(&task_id, user_id).await;

    match task {
        Some(task_state) => {
            let trace = task_state.execution_trace.as_ref().map(|t| {
                serde_json::json!({
                    "traceId": t.trace_id,
                    "totalDurationMs": t.total_duration_ms,
                    "tierUsage": t.tier_usage,
                    "steps": t.steps.iter().map(|s| serde_json::json!({
                        "stepId": s.step_id,
                        "capabilityId": s.capability_id,
                        "tierUsed": s.tier_used,
                        "durationMs": s.duration_ms,
                        "success": s.success,
                        "error": s.error,
                    })).collect::<Vec<_>>(),
                })
            });
            Ok(Json(json!({
                "success": true,
                "task": TaskInfo::from(&task_state),
                "results": task_state.step_results,
                "startedAt": task_state.started_at.to_rfc3339(),
                "completedAt": task_state.completed_at.map(|t| t.to_rfc3339()),
                "executionTrace": trace,
            })))
        }
        None => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Task not found or access denied" })),
        ))),
    }
}

/// 获取用户的所有任务
/// GET /api/agent/tasks
/// 任务列表分页参数
#[derive(Debug, Deserialize, Default)]
pub struct TaskListQuery {
    /// 最多返回多少条，默认 20，最大 100
    #[serde(default = "default_task_limit")]
    pub limit: usize,
    /// 偏移量，默认 0
    #[serde(default)]
    pub offset: usize,
}

pub(crate) fn default_task_limit() -> usize {
    20
}

pub async fn list_tasks(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Query(pagination): Query<TaskListQuery>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;

    // 限制单次最多返回 100 条
    let limit = pagination.limit.min(100);
    let offset = pagination.offset;

    tracing::debug!(
        user_id = user_id,
        limit = limit,
        offset = offset,
        "[Agent API] Listing user tasks"
    );

    let agent = Agent::new(db).await;
    // get_user_tasks 已按 started_at 降序（最新在前），并合并 DB
    let all_tasks = agent.get_user_tasks(user_id).await;
    let total = all_tasks.len();

    let task_list: Vec<Value> = all_tasks
        .iter()
        .skip(offset)
        .take(limit)
        .map(|t| {
            json!({
                "taskId": t.task_id,
                "recipeId": t.recipe_id,
                "status": task_status_name(&t.status),
                "progress": t.progress,
                "startedAt": t.started_at.to_rfc3339(),
                "completedAt": t.completed_at.map(|time| time.to_rfc3339())
            })
        })
        .collect();

    Ok(Json(json!({
        "success": true,
        "tasks": task_list,
        "total": total,
        "limit": limit,
        "offset": offset,
        "hasMore": offset + limit < total
    })))
}

/// 获取执行追踪列表
/// GET /api/agent/traces?limit=20
pub async fn list_traces(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Query(pagination): Query<TaskListQuery>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    let limit = pagination.limit.min(50);

    let agent = Agent::new(db).await;
    let all_tasks = agent.get_user_tasks(user_id).await;

    // 只返回有 execution_trace 的已完成任务（all_tasks 已按最新优先）
    let traces: Vec<Value> = all_tasks
        .iter()
        .filter_map(|t| {
            t.execution_trace.as_ref().map(|trace| {
                json!({
                    "traceId": trace.trace_id,
                    "taskId": t.task_id,
                    "recipeId": t.recipe_id,
                    "status": format!("{:?}", t.status).to_lowercase(),
                    "totalDurationMs": trace.total_duration_ms,
                    "tierUsage": trace.tier_usage,
                    "steps": trace.steps.iter().map(|s| json!({
                        "stepId": s.step_id,
                        "capabilityId": s.capability_id,
                        "tierUsed": s.tier_used,
                        "durationMs": s.duration_ms,
                        "success": s.success,
                        "error": s.error,
                    })).collect::<Vec<_>>(),
                    "startedAt": t.started_at.to_rfc3339(),
                    "completedAt": t.completed_at.map(|time| time.to_rfc3339()),
                })
            })
        })
        .take(limit)
        .collect();

    Ok(Json(json!({
        "success": true,
        "traces": traces,
        "total": traces.len(),
    })))
}

/// 获取系统能力列表
/// GET /api/agent/capabilities
pub async fn list_capabilities(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    let is_admin = crate::services::agent::user_is_current_admin(&db, user_id).await;
    let capabilities =
        crate::services::agent::get_capabilities_summary_for_user(is_admin).await;

    Ok(Json(json!({
        "success": true,
        "capabilities": capabilities
    })))
}

/// 取消任务
/// POST /api/agent/tasks/{task_id}/cancel
pub async fn cancel_task(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;

    tracing::info!(
        user_id = user_id,
        task_id = %task_id,
        "[Agent API] Cancelling task"
    );

    // 通过 Agent 接口取消（含所有权校验，防止 IDOR）
    let agent = Agent::new(db).await;
    let cancelled = agent.cancel_task_for_user(&task_id, user_id).await;

    if cancelled {
        // 等待输入中的 run 正阻塞在 done_rx；显式取消必须立即唤醒它，
        // 否则通知会在最多十分钟内仍错误显示为“等待回答”。
        if let Some(waiting) = take_waiting_task(&task_id, user_id).await {
            let _ = waiting.done_tx.send(json!({
                "success": false,
                "responseType": "error",
                "message": "任务已取消",
                "task": {
                    "taskId": task_id,
                    "status": "cancelled",
                    "progress": 0
                }
            }));
        }
        Ok(Json(json!({
            "success": true,
            "message": "Task cancellation requested",
            "taskId": task_id
        })))
    } else {
        Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Task not found or access denied" })),
        )))
    }
}

/// 回答问题请求
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerQuestionRequest {
    /// 问题 ID
    pub question_id: String,
    /// 用户答案
    pub answer: String,
}

/// 回答任务中的问题
/// POST /api/agent/tasks/{task_id}/answer
///
/// 使用 resume_with_answer 从暂停点恢复执行，而不是重新从头处理。
pub async fn answer_task_question(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(task_id): Path<String>,
    Json(req): Json<AnswerQuestionRequest>,
) -> Result<Json<ApiResponse>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;

    tracing::info!(
        user_id = user_id,
        task_id = %task_id,
        question_id = %req.question_id,
        "[Agent API] Answering task question (resume mode)"
    );

    let agent = Agent::new(db.clone()).await;

    let answer = UserAnswer {
        question_id: req.question_id,
        task_id: task_id.clone(),
        answer: req.answer,
        skipped: false,
    };

    agent
        .resume_task(&task_id, answer, user_id)
        .await
        .map(|response| Json(ApiResponse::from(response)))
        .map_err(|e| {
            tracing::error!(error = %e, "[Agent API] Failed to resume task");
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("恢复任务失败: {}", e) })),
            ))
        })
}

/// 回答问题（SSE 流式版本）
/// POST /api/agent/tasks/{task_id}/answer/stream
pub async fn answer_task_question_stream(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(task_id): Path<String>,
    Json(req): Json<AnswerQuestionRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;

    tracing::info!(
        user_id = user_id,
        task_id = %task_id,
        question_id = %req.question_id,
        "[Agent API] Answering task question (SSE stream mode)"
    );

    let (tx, rx) = tokio::sync::mpsc::channel::<ProgressEvent>(32);

    let db_clone = db.clone();
    tokio::spawn(async move {
        // Resolve session/lane BEFORE take so we match process_stream's session lane.
        let session_from_waiting = {
            let map = WAITING_TASKS.read().await;
            map.get(&task_id)
                .filter(|ctx| ctx.user_id == user_id)
                .map(|ctx| ctx.session_id.clone())
                .filter(|s| !s.is_empty())
        };
        let task_for_lane =
            crate::services::agent::executor::get_task_for_user(&task_id, user_id).await;
        let lane_key = LaneQueue::resolve_answer_lane_key(
            user_id,
            task_for_lane.as_ref().and_then(|t| t.lane_id.as_deref()),
            session_from_waiting.as_deref(),
        );

        // resume 执行前重新获取 lane 许可（process_stream 在 wait-for-input 时已释放）
        let queue = LANE_QUEUE.clone();
        let _lane_guard = match queue
            .acquire_timeout(
                &lane_key,
                std::time::Duration::from_secs(LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS),
            )
            .await
        {
            Ok(guard) => guard,
            Err(e) => {
                let _ = tx
                    .send(AgentProgressEvent::Error {
                        task_id: Some(task_id.clone()),
                        message: e,
                        code: "QUEUE_FULL".to_string(),
                    })
                    .await;
                return;
            }
        };

        let agent = Agent::new(db_clone.clone()).await;

        // 从 WAITING_TASKS 获取后端 run 上下文（仅所有者可取，防跨用户抢 oneshot）
        let waiting_ctx = take_waiting_task(&task_id, user_id).await;

        // 持久化用户的回答到会话消息历史（确保后续 Planner 能看到完整对话）
        let ctx_session_id = waiting_ctx
            .as_ref()
            .map(|ctx| ctx.session_id.clone())
            .or(session_from_waiting)
            .unwrap_or_default();
        if !ctx_session_id.is_empty() {
            let _ = persist_user_message(&db_clone, &ctx_session_id, &req.answer).await;
        }

        let answer = UserAnswer {
            question_id: req.question_id,
            task_id: task_id.clone(),
            answer: req.answer,
            skipped: false,
        };

        let resume_tx = waiting_ctx
            .as_ref()
            .map(|ctx| ctx.progress_tx.clone())
            .unwrap_or_else(|| tx.clone());

        match agent
            .resume_task_with_progress(&task_id, answer, user_id, resume_tx)
            .await
        {
            Ok(response) => {
                let api_response: ApiResponse = response.into();
                let final_task_id = api_response
                    .task
                    .as_ref()
                    .map(|t| t.task_id.clone())
                    .unwrap_or_default();
                let success = api_response.success;

                // 检查任务是否仍然在等待用户输入（多轮提问场景）
                let still_waiting = api_response
                    .task
                    .as_ref()
                    .map(|t| t.status == "waiting_for_input")
                    .unwrap_or(false);

                let response_value = serde_json::to_value(&api_response)
                    .unwrap_or_else(|_| json!({"error": "serialization failed"}));

                // 回传结果给 process_stream（如果它在等待）
                // process_stream 的循环会检查 status 决定是否继续等待
                if let Some(ctx) = waiting_ctx {
                    let _ = ctx.done_tx.send(response_value.clone());
                }

                // 在 answer_stream 自己的 SSE 上发送事件
                if still_waiting {
                    // 任务仍在等待：发送 TaskCompleted（携带 pendingQuestion 数据，
                    // 前端 handleAgentResponse 会检测到并显示新问题）
                    // 这里仍然发 TaskCompleted 以便 executeSSERequest resolve
                    let _ = tx
                        .send(AgentProgressEvent::TaskCompleted {
                            task_id: final_task_id,
                            success,
                            response: Box::new(response_value),
                        })
                        .await;
                } else {
                    // 任务真正完成
                    let _ = tx
                        .send(AgentProgressEvent::TaskCompleted {
                            task_id: final_task_id,
                            success,
                            response: Box::new(response_value),
                        })
                        .await;
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "[Agent API] Resume failed");

                // 回传错误给 process_stream
                if let Some(ctx) = waiting_ctx {
                    let _ = ctx.done_tx.send(json!({
                        "success": false,
                        "message": e.clone(),
                        "responseType": "error",
                    }));
                }

                let _ = tx
                    .send(AgentProgressEvent::Error {
                        task_id: Some(task_id),
                        message: e,
                        code: "RESUME_ERROR".to_string(),
                    })
                    .await;
            }
        }
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(|event| {
        let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
        Ok(Event::default().data(data))
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

/// 提供澄清回答
/// POST /api/agent/clarify
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClarifyRequest {
    /// 原始请求
    pub original_input: String,
    /// 澄清点 ID
    pub clarification_id: String,
    /// 用户选择或回答
    pub answer: String,
    /// 上下文
    #[serde(default)]
    pub context: Option<ProcessContext>,
}

pub async fn clarify(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<ClarifyRequest>,
) -> Result<Json<ApiResponse>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    validate_input(&req.original_input)?;

    tracing::info!(
        user_id = user_id,
        clarification_id = %req.clarification_id,
        "[Agent API] Processing clarification"
    );

    // 将澄清合并到原始请求
    let combined_input = format!("{}\n补充说明：{}", req.original_input, req.answer);

    let user_request = UserRequest {
        raw_input: combined_input,
        timestamp: chrono::Utc::now(),
        user_id,
        context: req.context.map(build_request_context),
    };

    let agent = Agent::new(db).await;
    let response = agent.process(user_request).await.map_err(|e| HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e })),
        )))?;

    Ok(Json(response.into()))
}

/// 确认敏感操作
/// POST /api/agent/confirm
pub async fn confirm_operation(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<ConfirmRequest>,
) -> Result<Json<ApiResponse>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;

    if req.confirmed {
        return Err(HttpError::from((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Confirmed operations must use /api/agent/confirm/stream",
                "code": "confirmation_stream_required"
            })),
        )));
    }

    tracing::info!(
        confirmation_id = %req.confirmation_id,
        confirmed = req.confirmed,
        user_id = user_id,
        "[Agent API] Processing confirmation"
    );

    let confirmation = crate::services::agent::types::UserConfirmation {
        confirmation_id: req.confirmation_id,
        confirmed: req.confirmed,
        user_note: req.note,
        user_id,
    };

    let agent = Agent::new(db).await;
    let lane_key = agent
        .confirmation_lane_key(&confirmation.confirmation_id, user_id)
        .await
        .map_err(|error| HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error })),
            )))?
        .unwrap_or_else(|| LaneQueue::make_lane_key(user_id, None));
    let _guard = LANE_QUEUE
        .acquire_timeout(
            &lane_key,
            std::time::Duration::from_secs(LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS),
        )
        .await
        .map_err(|error| HttpError::from((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": error })),
            )))?;
    let response = agent
        .process_confirmation(confirmation)
        .await
        .map_err(|e| HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )))?;

    Ok(Json(response.into()))
}

/// 确认敏感操作并通过可重连 run stream 执行续跑。
/// POST /api/agent/confirm/stream
pub async fn confirm_operation_stream(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<ConfirmRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    let confirmation = crate::services::agent::types::UserConfirmation {
        confirmation_id: req.confirmation_id.clone(),
        confirmed: req.confirmed,
        user_note: req.note,
        user_id,
    };

    // Peek session/lane before consume so the resume run stays attached to the
    // original conversation (history persistence + WAITING_TASKS answers).
    let agent_for_lookup = Agent::new(db.clone()).await;
    let resume_ctx = agent_for_lookup
        .confirmation_resume_context(&req.confirmation_id, user_id)
        .await
        .map_err(|error| HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error })),
            )))?;
    let session_id = resume_ctx
        .as_ref()
        .and_then(|ctx| ctx.session_id.clone())
        .filter(|s| !s.is_empty());
    let original_run_id = resume_ctx
        .as_ref()
        .and_then(|ctx| ctx.run_id.clone())
        .filter(|s| !s.is_empty());
    let lane_key = resume_ctx
        .and_then(|ctx| ctx.lane_key)
        .unwrap_or_else(|| LaneQueue::make_lane_key(user_id, session_id.as_deref()));

    // Prefer the original process run so notifications/UI stay on one identity.
    let run = if let Some(ref rid) = original_run_id {
        match get_run_for_user(rid, user_id).await {
            Some(existing) => existing,
            None => create_run(user_id, session_id.clone()).await,
        }
    } else {
        create_run(user_id, session_id.clone()).await
    };
    let run_for_task = run.clone();
    let db_clone = db.clone();
    tokio::spawn(async move {
        // Agent/executor progress events share the same run hub as the SSE subscriber.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<AgentProgressEvent>(32);
        let run_for_forwarder = run_for_task.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                run_for_forwarder.publish(event).await;
            }
        });

        let agent = Agent::new(db_clone.clone()).await;
        let _guard = match LANE_QUEUE
            .acquire_timeout(
                &lane_key,
                std::time::Duration::from_secs(LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS),
            )
            .await
        {
            Ok(guard) => guard,
            Err(error) => {
                let _ = tx
                    .send(AgentProgressEvent::Error {
                        task_id: None,
                        message: error,
                        code: "QUEUE_FULL".to_string(),
                    })
                    .await;
                return;
            }
        };

        if let Some(ref sid) = session_id {
            let _ = tx
                .send(AgentProgressEvent::SessionCreated {
                    session_id: sid.clone(),
                })
                .await;
        }

        match agent.process_confirmation(confirmation).await {
            Ok(response) => {
                let api_response: ApiResponse = response.into();
                let task_id = api_response
                    .task
                    .as_ref()
                    .map(|task| task.task_id.clone())
                    .unwrap_or_default();
                let success = api_response.success;
                let is_waiting = api_response
                    .task
                    .as_ref()
                    .map(|t| t.status == "waiting_for_input")
                    .unwrap_or(false);

                // Persist confirmation result (or missing-param question) into the
                // original session history so refresh keeps the full thread.
                if let Some(ref sid) = session_id {
                    let metadata = json!({
                        "suggestions": &api_response.suggestions,
                        "dataDisplay": &api_response.data_display,
                        "frontendAction": &api_response.frontend_action,
                        "data": &api_response.data,
                        "confirmationResume": true,
                    });
                    if let Err(e) = persist_assistant_message(
                        &db_clone,
                        sid,
                        if task_id.is_empty() {
                            None
                        } else {
                            Some(&task_id)
                        },
                        &api_response.message,
                        Some(metadata),
                    )
                    .await
                    {
                        tracing::warn!("[Agent API] Failed to persist confirmation result: {}", e);
                    }
                }

                if is_waiting && !task_id.is_empty() {
                    // Surface the first missing-param / Q&A prompt on the run
                    // hub, then keep the run alive for answer/resume rounds.
                    let mut waiting_response = serde_json::to_value(&api_response)
                        .unwrap_or_else(|_| json!({ "success": true, "message": "" }));
                    if let Some(object) = waiting_response.as_object_mut() {
                        object.insert("streamTerminal".to_string(), Value::Bool(false));
                        if let Some(ref sid) = session_id {
                            object.insert("sessionId".to_string(), Value::String(sid.clone()));
                        }
                    }
                    let _ = tx
                        .send(AgentProgressEvent::TaskCompleted {
                            task_id: task_id.clone(),
                            success: true,
                            response: Box::new(waiting_response),
                        })
                        .await;

                    // Keep run alive and register WAITING_TASKS so subsequent
                    // answers (and final reply) stay on this session.
                    loop {
                        let (done_tx, done_rx) =
                            tokio::sync::oneshot::channel::<serde_json::Value>();
                        {
                            let mut map = WAITING_TASKS.write().await;
                            map.insert(
                                task_id.clone(),
                                WaitingTaskCtx {
                                    user_id,
                                    progress_tx: tx.clone(),
                                    done_tx,
                                    session_id: session_id.clone().unwrap_or_default(),
                                },
                            );
                        }
                        tracing::info!(
                            task_id = %task_id,
                            session_id = ?session_id,
                            "[Agent API] Confirmation resume waiting for user input"
                        );

                        match tokio::time::timeout(tokio::time::Duration::from_secs(2), done_rx)
                            .await
                        {
                            Ok(Ok(response_value)) => {
                                let still_waiting = response_value
                                    .pointer("/task/status")
                                    .and_then(|s| s.as_str())
                                    == Some("waiting_for_input");

                                if still_waiting {
                                    if let Some(ref sid) = session_id {
                                        let msg = response_value
                                            .get("message")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("需要更多信息");
                                        let run_id = run_for_task.run_id();
                                        let metadata = session_metadata_with_run_identity(
                                            Some(response_value.clone()),
                                            run_id,
                                            &task_id,
                                        );
                                        let _ = persist_assistant_message(
                                            &db_clone,
                                            sid,
                                            Some(&task_id),
                                            msg,
                                            Some(metadata),
                                        )
                                        .await;
                                    }
                                    continue;
                                }

                                if let Some(ref sid) = session_id {
                                    let msg = response_value
                                        .get("message")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    if !msg.is_empty() {
                                        let run_id = run_for_task.run_id();
                                        let metadata = session_metadata_with_run_identity(
                                            Some(response_value.clone()),
                                            run_id,
                                            &task_id,
                                        );
                                        let _ = persist_assistant_message(
                                            &db_clone,
                                            sid,
                                            Some(&task_id),
                                            msg,
                                            Some(metadata),
                                        )
                                        .await;
                                    }
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
                                tracing::warn!(
                                    task_id = %task_id,
                                    "[Agent API] Confirmation answer sender dropped; terminalizing run"
                                );
                                let _ = take_waiting_task(&task_id, user_id).await;
                                let _ = tx.send(wait_loop_channel_dropped_event(&task_id)).await;
                                break;
                            }
                            Err(_) => {
                                let _ = take_waiting_task(&task_id, user_id).await;
                                let current_task =
                                    crate::services::agent::executor::refresh_task_for_user(
                                        &task_id, user_id,
                                    )
                                    .await;
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

                                let (response_value, task_success) =
                                    if let Some(task) = current_task {
                                        let task_success = task.status
                                            == crate::services::agent::types::TaskStatus::Completed;
                                        let message = task.error.clone().unwrap_or_else(|| {
                                            if task_success {
                                                "任务已完成".to_string()
                                            } else {
                                                "任务未完成".to_string()
                                            }
                                        });
                                        (
                                            json!({
                                                "success": task_success,
                                                "message": message,
                                                "task": task,
                                            }),
                                            task_success,
                                        )
                                    } else {
                                        (
                                            json!({
                                                "success": false,
                                                "message": "任务状态已不可用",
                                                "task": {
                                                    "taskId": task_id.clone(),
                                                    "status": "failed"
                                                }
                                            }),
                                            false,
                                        )
                                    };

                                if let Some(ref sid) = session_id {
                                    let msg = response_value
                                        .get("message")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    if !msg.is_empty() {
                                        let _ = persist_assistant_message(
                                            &db_clone,
                                            sid,
                                            Some(&task_id),
                                            msg,
                                            Some(response_value.clone()),
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
                } else {
                    let mut response_value = serde_json::to_value(api_response).unwrap_or_else(
                        |_| json!({ "success": false, "message": "Serialization failed" }),
                    );
                    if let Some(object) = response_value.as_object_mut() {
                        object.insert("streamTerminal".to_string(), Value::Bool(true));
                        if let Some(ref sid) = session_id {
                            object.insert("sessionId".to_string(), Value::String(sid.clone()));
                        }
                    }
                    let _ = tx
                        .send(AgentProgressEvent::TaskCompleted {
                            task_id,
                            success,
                            response: Box::new(response_value),
                        })
                        .await;
                }
            }
            Err(error) => {
                if let Some(ref sid) = session_id {
                    let _ = persist_assistant_message(
                        &db_clone,
                        sid,
                        None,
                        &format!("确认执行失败: {}", error),
                        Some(json!({ "error": true, "confirmationResume": true })),
                    )
                    .await;
                }
                let _ = tx
                    .send(AgentProgressEvent::Error {
                        task_id: None,
                        message: error,
                        code: "CONFIRMATION_EXECUTION_FAILED".to_string(),
                    })
                    .await;
            }
        }
    });

    Ok(Sse::new(agent_run_event_stream(run))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

#[derive(Debug, Deserialize)]
pub struct ConfirmRequest {
    #[serde(rename = "confirmationId")]
    pub confirmation_id: String,
    pub confirmed: bool,
    #[serde(default)]
    pub note: Option<String>,
}

/// 健康检查
/// GET /api/agent/health
pub async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "agent",
        "version": env!("CARGO_PKG_VERSION")
    }))
}


