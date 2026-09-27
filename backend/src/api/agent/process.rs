//! Agent API — process
use super::*;
use crate::error::HttpError;

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
            return Err(agent_turn_error(error).into());
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

/// Browser and realtime voice: the login is turned into a user behind the
/// agent gate, then [`crate::services::agent::run::start`] runs it.
pub(crate) async fn start_process_run(
    db: DatabaseConnection,
    claims: Claims,
    req: ProcessRequest,
) -> Result<Arc<AgentRun>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    let caller = crate::services::agent::run::Caller {
        user_id,
        is_admin: claims.is_admin,
    };
    crate::services::agent::run::start(db, caller, req)
        .await
        .map_err(Into::into)
}

/// An answer to her question, from a login: [`crate::services::agent::run::answer`].
pub(crate) async fn start_answer_run(
    db: DatabaseConnection,
    claims: Claims,
    task_id: String,
    question_id: String,
    answer: String,
    session_id: Option<String>,
) -> Result<Arc<AgentRun>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    crate::services::agent::run::answer(db, user_id, task_id, question_id, answer, session_id)
        .await
        .map_err(Into::into)
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
        .map_err(|error| agent_turn_error(error).into())
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

    #[tokio::test]
    async fn run_service_errors_keep_the_http_contract() {
        use crate::services::agent::run::{agent_turn_error, intention_error, validate_input};
        use axum::{body::to_bytes, response::IntoResponse};
        let cases = [
            (validate_input("  ").unwrap_err(), 400, Some("bad_request")),
            (
                intention_error(sea_orm::DbErr::RecordNotFound("missing".into())),
                404,
                Some("not_found"),
            ),
            (
                intention_error(sea_orm::DbErr::Custom("stale".into())),
                409,
                Some("conflict"),
            ),
            (
                agent_turn_error(AiQuotaError::DailyCallLimit { anonymous: false }.to_string()),
                429,
                Some("AI_DAILY_CALL_LIMIT"),
            ),
            (
                agent_turn_error("injected failure".into()),
                500,
                Some("agent_processing_failed"),
            ),
        ];
        for (error, status, code) in cases {
            let expected_body = error.to_json();
            let response = crate::error::HttpError::from(error).into_response();
            assert_eq!(response.status().as_u16(), status);
            let body: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 64 * 1024).await.unwrap())
                    .unwrap();
            assert_eq!(body, expected_body);
            if let Some(code) = code {
                assert_eq!(body["code"], code);
            }
        }
    }

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

use myriad_error::AppError;
