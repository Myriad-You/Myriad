//! Agent API — heartbeat_mcp
use super::*;
use crate::error::HttpError;

// 队列状态

/// 获取 Lane Queue 状态
pub(crate) async fn queue_status() -> Json<Value> {
    let status = LANE_QUEUE.get_status().await;
    Json(json!({
        "total_lanes": status.total_lanes,
        "max_concurrent": status.max_concurrent,
        "available_permits": status.available_permits,
        "waiting": status.waiting,
    }))
}

// Heartbeat

/// 获取所有 Heartbeat 任务状态
pub(crate) async fn heartbeat_tasks(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    let manager = crate::services::agent::heartbeat::get_heartbeat().ok_or_else(|| {
        HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json("Heartbeat not initialized")),
        ))
    })?;

    let tasks = manager.get_tasks().await;
    Ok(Json(json!({ "tasks": tasks })))
}

/// 切换 Heartbeat 任务启用状态
pub(crate) async fn toggle_heartbeat(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    let manager = crate::services::agent::heartbeat::get_heartbeat().ok_or_else(|| {
        HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json("Heartbeat not initialized")),
        ))
    })?;

    match manager.toggle_task(&task_id).await {
        Some(enabled) => Ok(Json(json!({ "task_id": task_id, "enabled": enabled }))),
        None => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Task not found")),
        ))),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateHeartbeatBody {
    name: Option<String>,
    schedule: Option<String>,
    action: Option<String>,
    enabled: Option<bool>,
}

/// 更新 Heartbeat 任务（name / schedule / action / enabled）
pub(crate) async fn update_heartbeat(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(task_id): Path<String>,
    Json(body): Json<UpdateHeartbeatBody>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    let manager = crate::services::agent::heartbeat::get_heartbeat().ok_or_else(|| {
        HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json("Heartbeat not initialized")),
        ))
    })?;

    match manager
        .update_task(
            &task_id,
            body.name,
            body.schedule,
            body.action,
            body.enabled,
        )
        .await
    {
        Ok(task) => Ok(Json(json!({ "task": task }))),
        Err(e) if e.contains("not found") => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::public_json(e)),
        ))),
        Err(e) => Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(e)),
        ))),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateHeartbeatBody {
    name: String,
    schedule: String,
    action: String,
    #[serde(default = "default_heartbeat_enabled")]
    enabled: bool,
    id: Option<String>,
}

pub(crate) fn default_heartbeat_enabled() -> bool {
    true
}

/// 创建 Heartbeat 任务
pub(crate) async fn create_heartbeat(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<CreateHeartbeatBody>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    let manager = crate::services::agent::heartbeat::get_heartbeat().ok_or_else(|| {
        HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json("Heartbeat not initialized")),
        ))
    })?;

    match manager
        .add_task(body.id, body.name, body.schedule, body.action, body.enabled)
        .await
    {
        Ok(task) => Ok(Json(json!({ "task": task }))),
        Err(e) if e.contains("already exists") => Err(HttpError::from((
            StatusCode::CONFLICT,
            Json(AppError::public_json(e)),
        ))),
        Err(e) => Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(e)),
        ))),
    }
}

/// 删除 Heartbeat 任务
pub(crate) async fn delete_heartbeat(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    let manager = crate::services::agent::heartbeat::get_heartbeat().ok_or_else(|| {
        HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json("Heartbeat not initialized")),
        ))
    })?;

    match manager.delete_task(&task_id).await {
        Ok(()) => Ok(Json(json!({ "deleted": true, "task_id": task_id }))),
        Err(e) if e.contains("not found") => Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::public_json(e)),
        ))),
        Err(e) => Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(e)),
        ))),
    }
}

/// 热重载 MCP 配置（mcp_servers.json）
pub(crate) async fn reload_mcp(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    match crate::services::agent::mcp::reload_mcp().await {
        Ok(()) => {
            let tools = if let Some(m) = crate::services::agent::mcp::get_mcp_manager() {
                m.list_tools().await.len()
            } else {
                0
            };
            Ok(Json(json!({ "reloaded": true, "tool_count": tools })))
        }
        Err(e) => Err(HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json(e)),
        ))),
    }
}

/// MCP 服务器状态列表
pub(crate) async fn mcp_status(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    let manager = crate::services::agent::mcp::get_mcp_manager().ok_or_else(|| {
        HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json("MCP manager not initialized")),
        ))
    })?;
    let servers = manager.list_server_status().await;
    let tools = manager.list_tools().await.len();
    Ok(Json(json!({ "servers": servers, "tool_count": tools })))
}

/// GET /mcp/config — full on-disk config (incl. disabled) for admin UI editing.
pub(crate) async fn mcp_get_config(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    let manager = crate::services::agent::mcp::get_mcp_manager().ok_or_else(|| {
        HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json("MCP manager not initialized")),
        ))
    })?;
    let config = manager.read_config().await.map_err(|error| {
        HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json(error)),
        ))
    })?;
    let status = manager.list_server_status().await;
    let tools = manager.list_tools().await.len();
    Ok(Json(json!({
        "config": config,
        "config_path": manager.config_path_display(),
        "runtime": {
            "servers": status,
            "tool_count": tools,
        },
    })))
}

/// PUT /mcp/config — replace mcp_servers.json and hot-reload children.
pub(crate) async fn mcp_put_config(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    let manager = crate::services::agent::mcp::get_mcp_manager().ok_or_else(|| {
        HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json("MCP manager not initialized")),
        ))
    })?;

    // Accept either `{ "servers": [...] }` or `{ "config": { "servers": [...] } }`.
    let config_val = body.get("config").cloned().unwrap_or_else(|| {
        if body.get("servers").is_some() {
            body.clone()
        } else {
            json!({ "servers": [] })
        }
    });

    let parsed: crate::services::agent::mcp::config::McpServersConfig =
        serde_json::from_value(config_val).map_err(|e| {
            tracing::warn!("invalid MCP config: {e}");
            HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "invalid MCP config",
                    "code": "mcp_config_invalid"
                })),
            ))
        })?;

    match manager.replace_config(parsed).await {
        Ok(saved) => {
            let status = manager.list_server_status().await;
            let tools = manager.list_tools().await.len();
            Ok(Json(json!({
                "saved": true,
                "reloaded": true,
                "config": saved,
                "config_path": manager.config_path_display(),
                "runtime": {
                    "servers": status,
                    "tool_count": tools,
                },
            })))
        }
        Err(e) => {
            tracing::error!(error = %e, "MCP config replace failed");
            let validation = e.starts_with("server ")
                || e.contains("duplicate")
                || e.contains("empty")
                || e.contains("too many")
                || e.contains("too long")
                || e.contains("may only");
            let status = if validation {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            };
            let public = if validation {
                e
            } else {
                "Failed to save MCP config".to_string()
            };
            Err(HttpError::from((
                status,
                Json(json!({
                    "error": public,
                    "code": if validation {
                        "mcp_config_invalid"
                    } else {
                        "mcp_config_save_failed"
                    }
                })),
            )))
        }
    }
}

// Skills & Memory

/// 获取可用技能列表
pub(crate) async fn list_skills() -> Result<Json<Value>, HttpError> {
    let registry = crate::services::agent::skill::get_skill_registry().ok_or_else(|| {
        HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json("Skill registry not initialized")),
        ))
    })?;

    let skills = registry.get_all().await;
    // 获取 skill stats（如果 SkillEvolution 已初始化）
    let stats_map = match crate::services::agent::skill_evolution::get_skill_evolution() {
        Some(evo) => evo.get_all_stats().await,
        None => std::collections::HashMap::new(),
    };

    let skills_json: Vec<Value> = skills
        .iter()
        .map(|s| {
            let (success_count, failure_count) = stats_map
                .get(&s.id)
                .map(|st| (st.success_count, st.failure_count))
                .unwrap_or((0, 0));
            json!({
                "id": s.id,
                "name": s.name,
                "description": s.description,
                "category": s.category,
                "origin": s.origin,
                "successCount": success_count,
                "failureCount": failure_count,
                "tierHint": s.tier_hint,
                "parameters": s.parameters,
            })
        })
        .collect();

    Ok(Json(json!({ "skills": skills_json })))
}

/// 获取记忆条目（当前用户）
pub(crate) async fn list_memories(
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    let memory = crate::services::agent::memory::get_memory().ok_or_else(|| {
        HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json("Memory not initialized")),
        ))
    })?;

    let entries = memory.list_recent(50, user_id).await;
    let memories_json: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "id": e.id,
                "memoryType": e.memory_type,
                "content": e.content,
                "source": e.source,
                "createdAt": e.created_at,
                "tier": e.tier,
                "importance": e.importance,
                "entities": e.entities,
                "relatedCapabilities": e.related_capabilities,
            })
        })
        .collect();

    Ok(Json(json!({ "memories": memories_json })))
}

/// 删除记忆条目（仅本人）
pub(crate) async fn delete_memory(
    Extension(claims): Extension<Claims>,
    Path(memory_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    let memory = crate::services::agent::memory::get_memory().ok_or_else(|| {
        HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json("Memory not initialized")),
        ))
    })?;

    if memory.remove_memory(&memory_id, user_id).await {
        Ok(Json(json!({ "success": true })))
    } else {
        Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Memory not found")),
        )))
    }
}

/// 更新记忆条目（仅本人）
pub(crate) async fn update_memory(
    Extension(claims): Extension<Claims>,
    Path(memory_id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    let content = body["content"].as_str().ok_or_else(|| {
        HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Missing field: content")),
        ))
    })?;

    let memory = crate::services::agent::memory::get_memory().ok_or_else(|| {
        HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json("Memory not initialized")),
        ))
    })?;

    if memory.update_memory(&memory_id, content, user_id).await {
        Ok(Json(json!({ "success": true })))
    } else {
        Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Memory not found")),
        )))
    }
}

/// 删除技能
pub(crate) async fn delete_skill(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(skill_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    let evo = crate::services::agent::skill_evolution::get_skill_evolution().ok_or_else(|| {
        HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json("Skill evolution not initialized")),
        ))
    })?;

    evo.delete_skill(&skill_id)
        .await
        .map_err(|e| HttpError::from((StatusCode::BAD_REQUEST, Json(AppError::public_json(e)))))?;

    Ok(Json(json!({ "success": true })))
}

// Session Control (Steer / Interrupt)

/// Stop the live Chat generation. Does not cancel Work.
pub(crate) async fn cancel_chat_turn(
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    let session_id = body
        .get("sessionId")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let cancelled = crate::services::agent::turn::cancel_chat_turn(user_id, session_id).await;
    Ok(Json(json!({ "success": cancelled })))
}

/// Cancel every cancellable task for the user (Pending/Running/WaitingForInput/Paused), then `process`.
pub(crate) async fn interrupt_session(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, HttpError> {
    // `context: None` → Work. Gate: module visibility and granted `AiChat` (non-admin).
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    let new_input = body
        .get("input")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json("Missing 'input' field")),
            ))
        })?
        .to_string();

    // 取消当前用户所有非终态任务（running / waiting / paused / pending）
    let agent = Agent::new(db.clone()).await;
    let tasks = agent.get_user_tasks(user_id).await;
    let mut cancelled_count = 0;
    for task in &tasks {
        if crate::services::agent::executor::task_store::is_cancellable_task_status(&task.status)
            && agent.cancel_task_for_user(&task.task_id, user_id).await
        {
            cancelled_count += 1;
            // 立即唤醒 wait-loop，避免通知/run 仍卡在 waiting
            if let Some(waiting) = take_waiting_task(&task.task_id, user_id).await {
                let _ = waiting.done_tx.send(json!({
                    "success": false,
                    "responseType": "error",
                    "message": "The task was cancelled",
                    "code": "task_cancelled",
                    "streamTerminal": true,
                    "task": {
                        "taskId": task.task_id,
                        "status": "cancelled",
                        "progress": 0
                    }
                }));
            }
        }
    }

    // 提交新请求（通过 LaneQueue 保护并发）
    let lane_key = LaneQueue::make_lane_key(user_id, None);
    let _guard = LANE_QUEUE
        .acquire_timeout(
            &lane_key,
            std::time::Duration::from_secs(LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS),
        )
        .await
        .map_err(|e| {
            HttpError::from((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(AppError::public_json(e)),
            ))
        })?;

    let request = crate::services::agent::UserRequest {
        raw_input: new_input.clone(),
        timestamp: chrono::Utc::now(),
        user_id,
        context: None,
    };

    let new_agent = Agent::new(db).await;
    match new_agent.process(request).await {
        Ok(response) => {
            // Same camelCase agent wire shape as POST /process (ApiResponse::from).
            let api_response: crate::api::agent::types::ApiResponse = response.into();
            Ok(Json(json!({
                "success": true,
                "cancelledTasks": cancelled_count,
                "cancelled_tasks": cancelled_count,
                "response": api_response,
            })))
        }
        Err(e) => Err(HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AppError::public_json(e)),
        ))),
    }
}

/// 向当前会话注入补充指令（转向）
pub(crate) async fn steer_session(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    let instruction = body
        .get("instruction")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json("Missing 'instruction' field")),
            ))
        })?;

    // 校验指令长度（复用 validate_input 的上限逻辑）
    if instruction.is_empty() || instruction.chars().count() > MAX_INPUT_LEN {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(
                "Instruction must be non-empty and within length limits",
            )),
        )));
    }

    let requested_task_id = body.get("taskId").and_then(Value::as_str);
    let running_tasks: Vec<_> = crate::services::agent::executor::get_user_tasks(user_id)
        .await
        .into_iter()
        .filter(|task| task.status == crate::services::agent::types::TaskStatus::Running)
        .collect();
    let task_id = if let Some(requested) = requested_task_id {
        let task = crate::services::agent::executor::get_task_for_user(requested, user_id)
            .await
            .filter(|task| task.status == crate::services::agent::types::TaskStatus::Running)
            .ok_or_else(|| {
                HttpError::from((
                    StatusCode::NOT_FOUND,
                    Json(AppError::public_json("Running task not found")),
                ))
            })?;
        task.task_id
    } else {
        match running_tasks.as_slice() {
            [task] => task.task_id.clone(),
            [] => {
                return Err(HttpError::from((
                    StatusCode::CONFLICT,
                    Json(AppError::public_json("No running task to steer")),
                )));
            }
            _ => {
                return Err(HttpError::from((
                    StatusCode::CONFLICT,
                    Json(AppError::public_json(
                        "Multiple tasks are running; taskId is required",
                    )),
                )));
            }
        }
    };

    crate::services::agent::executor::enqueue_steering(&db, &task_id, instruction.to_string())
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to enqueue steering instruction");
            HttpError::from((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": "Failed to persist steering instruction",
                    "code": "steering_unavailable"
                })),
            ))
        })?;

    // Keep an audit/session trace after the instruction is accepted for execution.
    if let Some(mem) = crate::services::agent::memory::get_memory() {
        mem.remember(
            &format!("Mid-task steering instruction: {}", instruction),
            crate::services::agent::memory::MemoryType::SessionInsight,
            user_id,
        )
        .await;
    }

    Ok(Json(json!({
        "success": true,
        "message": "Steering instruction queued for the next step boundary",
        "taskId": task_id,
        "queued": true,
        "instruction": instruction,
    })))
}
use myriad_error::AppError;
