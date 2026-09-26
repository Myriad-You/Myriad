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
        let code = quota_code(&error);
        tracing::info!(error = %error, "[Agent API] Turn rejected by AI quota");
        return HttpError::from((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": error, "code": code })),
        ));
    }
    tracing::error!(error = %error, "[Agent API] Processing failed");
    HttpError::from((
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": error,
            "code": "agent_processing_failed"
        })),
    ))
}

/// `AiQuotaError` renders as `CODE: message`, so the leading token is the code.
fn quota_code(error: &str) -> String {
    error
        .split(':')
        .next()
        .filter(|code| !code.is_empty())
        .unwrap_or("AI_QUOTA_EXCEEDED")
        .to_string()
}

/// Code for an SSE `error` event.
///
/// Every agent surface that streams can now fail on quota, and the client needs
/// to tell "you are out of budget / in cooldown" apart from "the agent broke" —
/// on the stream that distinction only exists in this code, because the HTTP
/// status was already sent as 200 when the stream opened.
pub(crate) fn agent_stream_error_code(error: &str, fallback: &str) -> String {
    if crate::services::ai_quota::is_client_limit_message(error) {
        tracing::info!(error = %error, "[Agent API] Stream rejected by AI quota");
        quota_code(error)
    } else {
        fallback.to_string()
    }
}

pub(crate) fn completed_turn_intention_status(
    response: &ApiResponse,
) -> crate::services::agent::consciousness::IntentStatus {
    use crate::services::agent::consciousness::IntentStatus;
    match response.response_type.as_str() {
        _ if response
            .task
            .as_ref()
            .is_some_and(|task| task.status == "waiting_for_input") =>
        {
            IntentStatus::Waiting
        }
        "confirmation_required" => IntentStatus::Waiting,
        "answer" | "task_completed"
            if response.success
                && response
                    .task
                    .as_ref()
                    .is_none_or(|task| task.status == "completed")
                && !response.data.as_ref().is_some_and(|data| {
                    data.get("blocked").and_then(Value::as_bool) == Some(true)
                        || data.get("unsupported").and_then(Value::as_bool) == Some(true)
                }) =>
        {
            IntentStatus::Completed
        }
        _ => IntentStatus::Failed,
    }
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
    let interaction_mode = req
        .context
        .as_ref()
        .and_then(|context| context.mode)
        .unwrap_or_default();
    let source_intent_id = req
        .context
        .as_ref()
        .and_then(|context| context.intention_id.clone());
    if source_intent_id.is_some()
        && interaction_mode != crate::services::agent::AgentInteractionMode::Work
    {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(
                "An accepted intention must enter Work mode",
            )),
        )));
    }
    validate_intention_work_request(&db, source_intent_id.as_deref(), user_id, &req.input).await?;

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
    let session_id = ensure_session(&db, client_session_id.as_deref(), user_id, interaction_mode)
        .await
        .map_err(|error| {
            tracing::error!(%error, "[Agent API] Failed to ensure session");
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::public_json("Could not prepare Agent session")),
            ))
        })?;
    let lane_key = LaneQueue::make_lane_key(user_id, Some(&session_id));
    let conversation_history = load_session_history(
        &db,
        &session_id,
        20,
        interaction_mode == crate::services::agent::AgentInteractionMode::Chat,
    )
    .await
    .map_err(|error| {
        tracing::error!(%error, "[Agent API] Failed to load session history");
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AppError::public_json("Could not load Agent session")),
        ))
    })?;
    if let Err(error) =
        require_user_message_persisted(persist_user_message(&db, &session_id, &req.input).await)
    {
        tracing::error!(%error, "[Agent API] Failed to persist user message");
        return Err(HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AppError::public_json("Could not save Agent message")),
        )));
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
    let autonomy_cap =
        autonomy_cap_for_intention(&db, source_intent_id.as_deref(), user_id).await?;
    if let Some(ref mut ctx) = user_request.context {
        // User-accepted Work must not inherit a client-supplied ceiling.
        // Only autonomy-accepted intentions keep a server-computed cap.
        ctx.autonomy_permission_cap = autonomy_cap;
    }

    // 同一 lane 串行；全局许可 4；`acquire_timeout` 默认 60s
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
                Json(AppError::public_json(e)),
            ))
        })?;

    begin_intention_work(
        &db,
        source_intent_id.as_deref(),
        user_id,
        Some(session_id.clone()),
        None,
    )
    .await?;

    // 创建 Agent 并处理请求
    let agent = Agent::new(db.clone()).await;
    let response = match agent.process(user_request).await {
        Ok(response) => response,
        Err(error) => {
            advance_intention_work(
                &db,
                source_intent_id.as_deref(),
                user_id,
                crate::services::agent::consciousness::IntentStatus::Failed,
                Some(error.clone()),
            )
            .await;
            return Err(agent_turn_error(error));
        }
    };
    let intention_status = if response.is_successful_outcome() {
        crate::services::agent::consciousness::IntentStatus::Completed
    } else if response
        .task
        .as_ref()
        .is_some_and(|task| task.status == crate::services::agent::TaskStatus::WaitingForInput)
    {
        crate::services::agent::consciousness::IntentStatus::Waiting
    } else {
        crate::services::agent::consciousness::IntentStatus::Failed
    };
    advance_intention_work(
        &db,
        source_intent_id.as_deref(),
        user_id,
        intention_status,
        Some(response.message.clone()),
    )
    .await;

    let mut api_response: ApiResponse = response.into();
    let metadata = json!({
        "suggestions": &api_response.suggestions,
        "dataDisplay": &api_response.data_display,
        "frontendAction": &api_response.frontend_action,
        "data": &api_response.data,
        "task": &api_response.task,
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
    let run = start_process_run(db, claims, req).await?;
    Ok(Sse::new(agent_run_event_stream(run))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

/// Save what a Chat turn had said before it was cut off, marked as such, so
/// the next turn knows where she stopped.
async fn save_cut_off(
    db: &DatabaseConnection,
    session_id: &str,
    text: &str,
    voice: bool,
    run_id: &str,
) {
    let metadata = json!({
        crate::services::agent::chat_prompt::CUT_OFF_KEY:
            crate::services::agent::chat_prompt::cut_off_kind(voice),
        "runId": run_id,
    });
    if let Err(error) = persist_assistant_message(db, session_id, None, text, Some(metadata)).await
    {
        tracing::warn!(%error, "[Agent API] Failed to save a cut-off reply");
    }
}

/// A Chat turn stopped with no newer turn keeps what it had said itself.
async fn save_stopped(
    db: &DatabaseConnection,
    session_id: &str,
    in_group: bool,
    spoken: Option<&crate::services::agent::turn::Spoken>,
    run_id: &str,
) {
    let Some(spoken) = spoken else {
        return;
    };
    if let Some(text) = spoken.stopped() {
        if !session_id.is_empty() && !in_group {
            save_cut_off(db, session_id, &text, spoken.voice(), run_id).await;
        }
    }
}

/// Browser and realtime voice enter the same run lifecycle, history, budget,
/// Chat supersession and director. Only their event transports differ.
pub(crate) async fn start_process_run(
    db: DatabaseConnection,
    claims: Claims,
    mut req: ProcessRequest,
) -> Result<Arc<AgentRun>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    validate_input(&req.input)?;
    let interaction_mode = req
        .context
        .as_ref()
        .and_then(|context| context.mode)
        .unwrap_or_default();
    // Server-set only (`serde(skip)`): a group turn speaks as her, in Chat.
    let group = req
        .context
        .as_mut()
        .and_then(|context| context.group.take());
    if group.is_some() && interaction_mode != crate::services::agent::AgentInteractionMode::Chat {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("A group turn must be a Chat turn")),
        )));
    }
    let channel_chat = req
        .context
        .as_mut()
        .and_then(|context| context.channel_chat.take());
    if channel_chat.is_some()
        && interaction_mode != crate::services::agent::AgentInteractionMode::Chat
    {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(
                "A channel chat turn must be a Chat turn",
            )),
        )));
    }
    let source_intent_id = req
        .context
        .as_ref()
        .and_then(|context| context.intention_id.clone());
    if source_intent_id.is_some()
        && interaction_mode != crate::services::agent::AgentInteractionMode::Work
    {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(
                "An accepted intention must enter Work mode",
            )),
        )));
    }
    validate_intention_work_request(&db, source_intent_id.as_deref(), user_id, &req.input).await?;

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
    // A group turn keeps to that group's session; anything else to a private one.
    let session_id = match ensure_session_in(
        &db,
        client_session_id.as_deref(),
        user_id,
        interaction_mode,
        group.as_ref().map(|group| group.venue.as_str()),
    )
    .await
    {
        Ok(sid) => sid,
        Err(e) => {
            tracing::warn!("[Agent API] Failed to ensure session: {}", e);
            // Chat 或带 `source_intent_id` 的接单必须有持久会话，否则历史会分叉。
            if source_intent_id.is_some()
                || interaction_mode == crate::services::agent::AgentInteractionMode::Chat
            {
                return Err(HttpError::from((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(AppError::public_json(
                        "Could not prepare durable Agent session",
                    )),
                )));
            }
            // 不阻塞主流程，降级为无会话模式
            String::new()
        }
    };

    let has_session = !session_id.is_empty();

    // 后端 run 独立于本次 HTTP 连接；前端只订阅事件。
    // SSE 断连不取消任务：刷新 / reattach 依赖 run 继续存活；
    // 用户中断走 cancel_task / cancel_task_for_user。
    let run_id_for_meta = format!("run_{}", uuid::Uuid::new_v4().simple());
    let is_chat = interaction_mode == crate::services::agent::AgentInteractionMode::Chat;
    let in_group = group.is_some();
    // Spoken aloud: the text runs ahead of what they heard.
    let voice = req
        .context
        .as_ref()
        .and_then(|context| context.custom_data.as_ref())
        .and_then(|data| data.get("voice"))
        .and_then(|voice| voice.as_str())
        == Some("realtime");
    // Claim before loading history, so the previous Chat run is cancelled even
    // while this request waits for a lane permit, and so what it had said, if
    // it was cut off partway, lands before this message. Work never claims.
    let chat_claim = if is_chat {
        Some(
            crate::services::agent::turn::claim_speaking_turn(
                user_id,
                &session_id,
                &run_id_for_meta,
                voice,
            )
            .await,
        )
    } else {
        None
    };
    if let Some(cut) = chat_claim.as_ref().and_then(|claim| claim.cut_off.as_ref()) {
        // A group heard nothing until a reply was complete.
        if has_session && !in_group {
            save_cut_off(&db, &session_id, &cut.text, cut.voice, &cut.run_id).await;
        }
    }

    // 从数据库加载最近 20 条会话历史（替代前端传入的 conversation_history）。
    // A group turn answers in the group: its history is the group transcript.
    let conversation_history = if let Some(group) = &group {
        Some(group.transcript.clone())
    } else if has_session {
        let history = load_session_history(
            &db,
            &session_id,
            20,
            interaction_mode == crate::services::agent::AgentInteractionMode::Chat,
        )
        .await
        .map_err(|error| {
            tracing::error!(%error, "[Agent API] Failed to load session history");
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::public_json("Could not load Agent session")),
            ))
        })?;
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
        if let Err(e) =
            require_user_message_persisted(persist_user_message(&db, &session_id, &req.input).await)
        {
            tracing::error!("[Agent API] Failed to persist user message: {}", e);
            return Err(HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::public_json(
                    "Could not save durable Agent message",
                )),
            )));
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
        ctx.venue = group.as_ref().map(|group| group.venue.clone());
        ctx.chime = group.as_ref().and_then(|group| group.chime.clone());
        ctx.channel_chat = channel_chat.clone();
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
    let autonomy_cap =
        autonomy_cap_for_intention(&db, source_intent_id.as_deref(), user_id).await?;
    if let Some(ref mut ctx) = user_request.context {
        // User-accepted Work must not inherit a client-supplied ceiling.
        // Only autonomy-accepted intentions keep a server-computed cap.
        ctx.autonomy_permission_cap = autonomy_cap;
    }

    // Acquire input references before admission can launch an executor. This also
    // covers non-channel callers carrying local media in custom_data.
    if let Some(payload) = user_request
        .context
        .as_ref()
        .and_then(|ctx| ctx.custom_data.as_ref())
    {
        use sea_orm::TransactionTrait;
        let txn = db
            .begin()
            .await
            .map_err(|_| agent_turn_error("Could not save media references".into()))?;
        let actor = if claims.is_admin {
            crate::services::media::MediaActor::admin(user_id)
        } else {
            crate::services::media::MediaActor::user(user_id)
        }
        .ok();
        crate::services::media::bind_run_input(
            &txn,
            &run_id_for_meta,
            payload,
            &[],
            actor.as_ref(),
        )
        .await
        .map_err(|error| HttpError(error.into()))?;
        txn.commit()
            .await
            .map_err(|_| agent_turn_error("Could not save media references".into()))?;
    }

    begin_intention_work(
        &db,
        source_intent_id.as_deref(),
        user_id,
        has_session.then_some(session_id.clone()),
        Some(run_id_for_meta.clone()),
    )
    .await?;
    let run = crate::services::agent::run_hub::create_run_with_id(
        run_id_for_meta.clone(),
        user_id,
        has_session.then_some(session_id.clone()),
    )
    .await;
    // 注入 run_id，供确认手持（confirmation）复用同一 run hub / 通知身份
    if let Some(ref mut ctx) = user_request.context {
        ctx.run_id = Some(run_id_for_meta.clone());
    }

    // Agent/executor 继续使用有背压的 mpsc；独立转发器负责写入 run hub。
    // TaskCreated 时把 runId/taskId 写入会话历史，刷新后可 reattach。
    let (tx, rx) = tokio::sync::mpsc::channel::<ProgressEvent>(256);
    let run_for_forwarder = run.clone();
    let spoken_for_forwarder = chat_claim.as_ref().map(|claim| claim.spoken.clone());
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

            if let (Some(spoken), AgentProgressEvent::SummaryToken { token, .. }) =
                (&spoken_for_forwarder, &event)
            {
                spoken.push(token);
            }
            // 先 `publish`（hub fanout）；会话身份 persist 另 spawn，不挡热路径。
            run_for_forwarder.publish(event).await;

            // TaskCreated 后 best-effort 写会话身份，fire-and-forget。
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
    let source_intent_id_for_work = source_intent_id.clone();
    let (mut chat_cancel, chat_slot_id, chat_spoken) = match chat_claim {
        Some(claim) => (
            Some(claim.cancelled),
            Some(claim.slot_id),
            Some(claim.spoken),
        ),
        None => (None, None, None),
    };
    // tx 会被移动到 spawn 中，确保 channel 在任务完成前不会关闭
    let execution = tokio::spawn(async move {
        // 同一 lane 串行；全局许可 4；`acquire_timeout` 默认 60s
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
                        message: format!("Queued (about {ahead} ahead)…"),
                    })
                    .await;
            }
        }
        let acquire = if let Some(cancelled) = chat_cancel.as_mut() {
            tokio::select! {
                biased;
                _ = cancelled => None,
                result = queue.acquire_timeout(
                    &lane_key,
                    std::time::Duration::from_secs(LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS),
                ) => Some(result),
            }
        } else {
            Some(
                queue
                    .acquire_timeout(
                        &lane_key,
                        std::time::Duration::from_secs(LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS),
                    )
                    .await,
            )
        };
        let Some(acquire) = acquire else {
            let _ = tx
                .send(crate::services::agent::turn::superseded_turn_event())
                .await;
            if let Some(slot_id) = chat_slot_id {
                crate::services::agent::turn::finish_chat_turn(user_id, &session_id_clone, slot_id)
                    .await;
            }
            return;
        };
        let mut lane_guard = match acquire {
            Ok(guard) => Some(guard),
            Err(e) => {
                advance_intention_work(
                    &db_clone,
                    source_intent_id_for_work.as_deref(),
                    user_id,
                    crate::services::agent::consciousness::IntentStatus::Failed,
                    Some(e.clone()),
                )
                .await;
                let _ = tx
                    .send(AgentProgressEvent::Error {
                        task_id: None,
                        message: e,
                        code: "QUEUE_FULL".to_string(),
                    })
                    .await;
                if let Some(slot_id) = chat_slot_id {
                    crate::services::agent::turn::finish_chat_turn(
                        user_id,
                        &session_id_clone,
                        slot_id,
                    )
                    .await;
                }
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
        // New Chat replaces the previous Chat run. Work is never registered here,
        // so a Chat send cannot cancel background Work. This select is Chat
        // supersession or cancel_chat_turn/cancel_chat_run — not SSE disconnect.
        let turn_result = match chat_cancel.take() {
            Some(cancelled) => {
                tokio::select! {
                    biased;
                    result = agent.process_with_progress(user_request, tx.clone()) => result,
                    _ = cancelled => {
                        // Dropping process_with_progress skips its idle mark.
                        crate::services::agent::merope::mark_activity(
                            &db_clone,
                            user_id,
                            "idle",
                        )
                        .await;
                        save_stopped(&db_clone, &session_id_clone, in_group, chat_spoken.as_ref(), &run_id_for_meta).await;
                        let _ = tx
                            .send(crate::services::agent::turn::superseded_turn_event())
                            .await;
                        if let Some(slot_id) = chat_slot_id {
                            crate::services::agent::turn::finish_chat_turn(
                                user_id,
                                &session_id_clone,
                                slot_id,
                            )
                            .await;
                        }
                        return;
                    }
                }
            }
            _ => agent.process_with_progress(user_request, tx.clone()).await,
        };
        // Cut off just as it finished: only what it had said stands.
        if chat_spoken.as_ref().is_some_and(|spoken| !spoken.finish()) {
            save_stopped(
                &db_clone,
                &session_id_clone,
                in_group,
                chat_spoken.as_ref(),
                &run_id_for_meta,
            )
            .await;
            let _ = tx
                .send(crate::services::agent::turn::superseded_turn_event())
                .await;
            if let Some(slot_id) = chat_slot_id {
                crate::services::agent::turn::finish_chat_turn(user_id, &session_id_clone, slot_id)
                    .await;
            }
            return;
        }
        match turn_result {
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
                    advance_intention_work(
                        &db_clone,
                        source_intent_id_for_work.as_deref(),
                        user_id,
                        crate::services::agent::consciousness::IntentStatus::Waiting,
                        Some(api_response.message.clone()),
                    )
                    .await;
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

                    spawn_restored_wait_loop(
                        user_id,
                        task_id.clone(),
                        session_id_clone.clone(),
                        run_id_for_meta.clone(),
                        tx.clone(),
                        db_clone.clone(),
                        source_intent_id_for_work.clone(),
                    )
                    .await;
                } else {
                    // 非 waiting 路径：立即 `lane_guard.take()`，不等 spawn 结束
                    let _ = lane_guard.take();
                    // 非 waiting：先落会话元数据，再发 TaskCompleted

                    let parked_task_id = task_id.clone();
                    if !session_id_clone.is_empty() {
                        let metadata = json!({
                            "suggestions": &api_response.suggestions,
                            "dataDisplay": &api_response.data_display,
                            "frontendAction": &api_response.frontend_action,
                            "data": &api_response.data,
                            "runId": run_id_for_meta,
                            "taskId": if task_id.is_empty() { Value::Null } else { json!(task_id) },
                            "task": &api_response.task,
                        });
                        if let Err(e) = persist_assistant_message(
                            &db_clone,
                            &session_id_clone,
                            if parked_task_id.is_empty() {
                                None
                            } else {
                                Some(&parked_task_id)
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
                        .unwrap_or_else(|_| AppError::public_json("serialization failed"));
                    advance_intention_work(
                        &db_clone,
                        source_intent_id_for_work.as_deref(),
                        user_id,
                        completed_turn_intention_status(&api_response),
                        Some(api_response.message.clone()),
                    )
                    .await;

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
                advance_intention_work(
                    &db_clone,
                    source_intent_id_for_work.as_deref(),
                    user_id,
                    crate::services::agent::consciousness::IntentStatus::Failed,
                    Some(e.clone()),
                )
                .await;
                // A turn reserves AI quota before it runs, so an exhausted
                // budget arrives here alongside real faults. It is neither an
                // error to log at ERROR nor a "处理失败" for the transcript.
                let quota_rejected = crate::services::ai_quota::is_client_limit_message(&e);
                if !quota_rejected {
                    tracing::error!(error = %e, "[Agent API] Processing failed, sending error event");
                }
                let code = agent_stream_error_code(&e, "PROCESSING_ERROR");

                // 持久化错误消息
                if !session_id_clone.is_empty() {
                    let text = if quota_rejected {
                        e.clone()
                    } else {
                        "Processing failed".to_string()
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
        if let Some(slot_id) = chat_slot_id {
            crate::services::agent::turn::finish_chat_turn(user_id, &session_id_clone, slot_id)
                .await;
        }
        // spawn 结束；HTTP SSE 随 hub 终端事件结束，不是这里 drop mpsc。
    });

    run.register_execution(execution.abort_handle());

    Ok(run)
}

/// 重新订阅一个已存在的 Agent run。
/// GET /api/agent/runs/{run_id}/stream
pub async fn subscribe_run_stream(
    Extension(claims): Extension<Claims>,
    Path(run_id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    let run = match get_run_for_user(&run_id, user_id).await {
        Ok(Some(run)) => run,
        Ok(None) => {
            return Err(HttpError::from((
                StatusCode::NOT_FOUND,
                Json(AppError::public_json(
                    "Run not found, expired, or access denied",
                )),
            )));
        }
        Err(error) => {
            tracing::error!(%error, "[Agent API] Failed to rehydrate Agent run");
            return Err(HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::public_json("Could not load Agent run")),
            )));
        }
    };

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
            Json(AppError::public_json("Task not found or access denied")),
        ))),
    }
}

/// 获取用户的所有任务
/// GET /api/agent/tasks
/// 任务列表分页参数
#[derive(Debug, Deserialize, Default)]
pub struct TaskListQuery {
    /// 最多返回多少条，默认 20。`list_tasks` 上限 100，`list_traces` 上限 50。
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

/// 获取执行追踪列表。GET /api/agent/traces；limit 默认 20，上限 50。
pub async fn list_traces(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Query(pagination): Query<TaskListQuery>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    let limit = pagination.limit.min(50);

    let agent = Agent::new(db).await;
    let all_tasks = agent.get_user_tasks(user_id).await;

    // 只返回带 execution_trace 的任务（all_tasks 已按最新优先；不按 status 过滤）。
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
    let capabilities =
        crate::services::agent::get_capabilities_summary_for_user(&db, user_id).await;

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
        // 等待输入的 run 堵在 `done_rx`（boot 里 2s timeout 再轮询）；取消必须立刻 send。
        if let Some(waiting) = take_waiting_task(&task_id, user_id).await {
            let _ = waiting.done_tx.send(json!({
                "success": false,
                "responseType": "error",
                "message": "The task was cancelled",
                "code": "task_cancelled",
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
            Json(AppError::public_json("Task not found or access denied")),
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendAckRequest {
    pub step_id: String,
    #[serde(default)]
    pub music_status: Option<Value>,
    #[serde(default)]
    pub window_state: Option<Value>,
}

/// Live snapshot from the browser after query_windows / music_get_status ran.
/// POST /api/agent/tasks/{task_id}/frontend-ack
pub async fn frontend_step_ack(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(task_id): Path<String>,
    Json(req): Json<FrontendAckRequest>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &_db).await?;
    if crate::services::agent::executor::get_task_for_user(&task_id, user_id)
        .await
        .is_none()
    {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Task not found")),
        )));
    }
    let accepted = crate::services::agent::executor::submit_frontend_ack(
        &task_id,
        &req.step_id,
        json!({
            "musicStatus": req.music_status,
            "windowState": req.window_state,
        }),
    );
    Ok(Json(json!({ "success": true, "accepted": accepted })))
}

/// 回答任务中的问题
/// POST /api/agent/tasks/{task_id}/answer
///
/// `Agent::resume_task`（内部 `executor.resume_with_answer`），不从头 `process`。
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
        // Resume reserves its own budget, so a quota rejection can surface here
        // too and must not be reported as a server error.
        .map_err(agent_turn_error)
}

/// 回答问题（SSE 流式版本）
/// POST /api/agent/tasks/{task_id}/answer/stream
pub async fn answer_task_question_stream(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(task_id): Path<String>,
    Json(req): Json<AnswerQuestionRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, HttpError> {
    let run = start_answer_run(db, claims, task_id, req.question_id, req.answer, None).await?;
    Ok(Sse::new(agent_run_event_stream(run))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

pub(crate) async fn start_answer_run(
    db: DatabaseConnection,
    claims: Claims,
    task_id: String,
    question_id: String,
    answer: String,
    session_id: Option<String>,
) -> Result<Arc<AgentRun>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    validate_input(&answer)?;
    let task = crate::services::agent::executor::get_task_for_user(&task_id, user_id)
        .await
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Task not found"})),
            ))
        })?;
    let session_id =
        myriad_agent_rules::session_id_from_lane_id(task.lane_id.as_deref()).or(session_id);
    if task
        .recipe
        .as_ref()
        .is_some_and(crate::services::agent::work_loop::is_work_recipe)
    {
        Agent::new(db.clone())
            .await
            .validate_work_answer(
                &task_id,
                &UserAnswer {
                    task_id: task_id.clone(),
                    question_id: question_id.clone(),
                    answer: answer.clone(),
                    skipped: false,
                },
                user_id,
            )
            .await
            .map_err(|message| {
                HttpError::from((StatusCode::CONFLICT, Json(json!({"error":message}))))
            })?;
    }
    let run = create_run(user_id, session_id).await;
    let (tx, mut rx) = tokio::sync::mpsc::channel::<ProgressEvent>(256);
    let run_for_forwarder = run.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            run_for_forwarder.publish(event).await;
        }
    });

    let req = AnswerQuestionRequest {
        question_id,
        answer,
    };
    let execution = spawn_answer_resume(db, user_id, task_id, req, tx);
    run.register_execution(execution.abort_handle());
    Ok(run)
}

fn spawn_answer_resume(
    db: DatabaseConnection,
    user_id: i32,
    task_id: String,
    req: AnswerQuestionRequest,
    tx: tokio::sync::mpsc::Sender<ProgressEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let session_from_waiting = {
            let map = WAITING_TASKS.lock().unwrap();
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

        let agent = Agent::new(db.clone()).await;
        let answer = UserAnswer {
            question_id: req.question_id,
            task_id: task_id.clone(),
            answer: req.answer,
            skipped: false,
        };
        let is_work = task_for_lane
            .as_ref()
            .and_then(|task| task.recipe.as_ref())
            .is_some_and(crate::services::agent::work_loop::is_work_recipe);
        if is_work {
            if let Err(message) = agent.validate_work_answer(&task_id, &answer, user_id).await {
                // The lane may have queued this behind another answer. Reject
                // locally, leaving the original run and wait context intact.
                let _ = tx
                    .send(AgentProgressEvent::Error {
                        task_id: Some(task_id),
                        message,
                        code: "INVALID_ANSWER".into(),
                    })
                    .await;
                return;
            }
        }
        // Persist before consuming the waiter. A store failure must leave the
        // original wait context in place so the user can retry.
        if let Some(session_id) = session_from_waiting.as_ref().filter(|s| !s.is_empty()) {
            if let Err(error) = require_user_message_persisted(
                persist_user_message(&db, session_id, &answer.answer).await,
            ) {
                tracing::error!(%error, "[Agent API] Failed to persist user message");
                let _ = tx
                    .send(AgentProgressEvent::Error {
                        task_id: Some(task_id),
                        message: "Could not save durable Agent message".into(),
                        code: "SESSION_STORE_FAILED".into(),
                    })
                    .await;
                return;
            }
        }
        // Keep every waiter until continuation succeeds. Taking it earlier
        // makes a failed resume unretryable.
        let original_progress = WAITING_TASKS
            .lock()
            .unwrap()
            .get(&task_id)
            .filter(|ctx| ctx.user_id == user_id)
            .map(|ctx| ctx.progress_tx.clone());

        // Existing run subscribers and this continuation observe the same progress.
        let resume_tx = if let Some(original) = original_progress {
            let current = tx.clone();
            let (progress, mut events) = tokio::sync::mpsc::channel::<ProgressEvent>(256);
            tokio::spawn(async move {
                while let Some(event) = events.recv().await {
                    let _ = current.send(event.clone()).await;
                    let _ = original.send(event).await;
                }
            });
            progress
        } else {
            tx.clone()
        };

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

                let response_value = serde_json::to_value(&api_response)
                    .unwrap_or_else(|_| AppError::public_json("serialization failed"));

                // 唤醒 `spawn_restored_wait_loop` 的 `done_tx`（不是 process_stream 本体）
                if let Some(ctx) = take_waiting_task(&task_id, user_id).await {
                    let _ = ctx.done_tx.send(response_value.clone());
                }

                // 仍等待或已完成都发 TaskCompleted（payload 里的 status 区分）
                let _ = tx
                    .send(AgentProgressEvent::TaskCompleted {
                        task_id: final_task_id,
                        success,
                        response: Box::new(response_value),
                    })
                    .await;
            }
            Err(e) => {
                let code = agent_stream_error_code(&e, "RESUME_ERROR");
                if code == "RESUME_ERROR" {
                    tracing::error!(error = %e, "[Agent API] Resume failed");
                }

                let _ = tx
                    .send(AgentProgressEvent::Error {
                        task_id: Some(task_id),
                        message: e,
                        code,
                    })
                    .await;
            }
        }
    })
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
    let combined_input = format!("{}\nAdditional context: {}", req.original_input, req.answer);

    let user_request = UserRequest {
        raw_input: combined_input,
        timestamp: chrono::Utc::now(),
        user_id,
        context: req.context.map(build_request_context),
    };

    let agent = Agent::new(db).await;
    let response = agent
        .process(user_request)
        .await
        .map_err(agent_turn_error)?;

    Ok(Json(response.into()))
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

#[cfg(test)]
mod quota_error_tests {
    use super::{
        ApiResponse, agent_stream_error_code, completed_turn_intention_status, quota_code,
    };
    use crate::services::agent::consciousness::IntentStatus;
    use serde_json::json;

    fn response(response_type: &str) -> ApiResponse {
        ApiResponse {
            success: true,
            response_type: response_type.into(),
            message: String::new(),
            data: None,
            data_display: None,
            suggestions: vec![],
            task: None,
            frontend_action: None,
            performance: None,
            session_id: None,
        }
    }
    use crate::services::ai_quota::AiQuotaError;

    #[test]
    fn stream_errors_carry_the_quota_code_instead_of_a_generic_one() {
        // On a stream the HTTP status was already sent as 200, so this code is
        // the only place the client can tell a budget limit from a crash.
        for (error, expected) in [
            (
                AiQuotaError::Cooldown {
                    remaining_seconds: 7,
                },
                "AI_COOLDOWN_ACTIVE",
            ),
            (
                AiQuotaError::DailyCallLimit { anonymous: false },
                "AI_DAILY_CALL_LIMIT",
            ),
            (
                AiQuotaError::DailyTokenLimit { anonymous: true },
                "AI_ANONYMOUS_DAILY_TOKEN_LIMIT",
            ),
        ] {
            assert_eq!(
                agent_stream_error_code(&error.to_string(), "PROCESSING_ERROR"),
                expected
            );
        }
    }

    #[test]
    fn non_quota_failures_keep_the_callers_fallback_code() {
        for fallback in ["PROCESSING_ERROR", "RESUME_ERROR", "EXECUTION_ERROR"] {
            assert_eq!(
                agent_stream_error_code("Unknown capability_id: foo", fallback),
                fallback
            );
            // A ledger fault is a server error, not a client budget limit.
            assert_eq!(
                agent_stream_error_code(
                    &AiQuotaError::Ledger {
                        message: "db down".into()
                    }
                    .to_string(),
                    fallback
                ),
                fallback
            );
        }
    }

    #[test]
    fn quota_code_survives_a_message_without_a_colon() {
        assert_eq!(quota_code("AI_DAILY_CALL_LIMIT"), "AI_DAILY_CALL_LIMIT");
        assert_eq!(quota_code(""), "AI_QUOTA_EXCEEDED");
        assert_eq!(quota_code(": leading colon"), "AI_QUOTA_EXCEEDED");
    }

    #[test]
    fn confirmation_is_waiting_even_when_transport_succeeded() {
        assert_eq!(
            completed_turn_intention_status(&response("confirmation_required")),
            IntentStatus::Waiting
        );
        assert_eq!(
            completed_turn_intention_status(&response("answer")),
            IntentStatus::Completed
        );
        assert_eq!(
            completed_turn_intention_status(&response("clarification")),
            IntentStatus::Failed
        );
        let mut blocked = response("answer");
        blocked.data = Some(json!({ "blocked": true }));
        assert_eq!(
            completed_turn_intention_status(&blocked),
            IntentStatus::Failed
        );
    }
}

#[cfg(test)]
mod cut_off_tests {
    /// What a cut-off turn had said is saved before this turn's history is
    /// read and before this message, so it sits where it was said.
    #[test]
    fn a_cut_off_reply_lands_before_the_message_that_cut_it_off() {
        let src = include_str!("process.rs");
        let body = src
            .split("pub(crate) async fn start_process_run(")
            .nth(1)
            .expect("start_process_run");
        let claim = body.find("claim_speaking_turn(").expect("claim");
        let save = body.find("save_cut_off(").expect("save");
        let history = body.find("load_session_history(").expect("history");
        let persist = body.find("persist_user_message(").expect("persist");
        assert!(claim < save && save < history && history < persist);
        // A turn that was cut off does not also save its whole reply.
        let finish = body.find("spoken.finish()").expect("finish check");
        let outcome = body.find("match turn_result").expect("turn outcome");
        let reply = outcome
            + body[outcome..]
                .find("persist_assistant_message(")
                .expect("reply persistence");
        assert!(finish < outcome && outcome < reply);
    }
}

#[cfg(test)]
mod resume_persist_tests {
    #[test]
    fn resume_takes_the_waiter_only_after_success() {
        let src = include_str!("process.rs");
        let body = src
            .split("fn spawn_answer_resume(")
            .nth(1)
            .and_then(|rest| rest.split("pub async fn clarify").next())
            .expect("spawn_answer_resume");
        let persist_at = body.find("persist_user_message").expect("persist");
        let resume_at = body.find("resume_task_with_progress").expect("resume");
        let take_at = body.find("take_waiting_task").expect("take");
        assert!(persist_at < resume_at, "persist before resume");
        assert!(
            resume_at < take_at,
            "a failed resume must not drop the original waiter"
        );
        let err = body
            .split("resume_task_with_progress")
            .nth(1)
            .and_then(|rest| rest.split("Err(e)").nth(1))
            .expect("resume err");
        assert!(
            !err.contains("take_waiting_task"),
            "resume failure must leave the waiter for retry"
        );
    }
}
use myriad_error::AppError;
