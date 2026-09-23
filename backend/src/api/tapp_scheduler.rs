//! Tapp 定时任务 API
//!
//! 提供定时任务的 CRUD 和 WebSocket 推送功能

use axum::{
    Extension, Json,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

use crate::api::tapp_runtime::RuntimeGrantContext;
use crate::api::tapp_runtime::common::{resolve_accessible_tapp, verify_tapp_approved_permissions};
use crate::error::HttpError;
use crate::middleware::auth::{Claims, ensure_current_admin_on};
use crate::models::entities::tapp_scheduled_tasks::{
    ExecutionTarget, MissedPolicy, ScheduleType, TaskScope,
};
use crate::services::permission_service::TappPermission;
use crate::services::tapp_scheduler::{
    MAX_SCHEDULER_RETRIES, MAX_SCHEDULER_RETRY_DELAY_MS, SCHEDULER_MAILBOX_POLL_MILLIS,
    SCHEDULER_PRESENCE_REFRESH_SECONDS, TappSchedulerEngine, backend_action_permissions_of,
    drain_frontend_messages, normalize_backend_actions_parsed, register_frontend_connection,
    scheduler_engine as service_scheduler, try_scheduler_engine, unregister_frontend_connection,
    validate_backend_action_declarations_of,
};
use uuid::Uuid;

/// Initialize the process-wide scheduler engine (owned by services).
pub async fn init_scheduler(db: DatabaseConnection) {
    crate::services::tapp_scheduler::init_scheduler(db).await;
}

/// Shut down the process-wide scheduler engine.
pub async fn shutdown_scheduler() {
    crate::services::tapp_scheduler::shutdown_scheduler().await;
}

/// HTTP-facing handle: 503 when the engine has not been started.
fn get_scheduler() -> Result<Arc<TappSchedulerEngine>, HttpError> {
    service_scheduler().map_err(|_| {
        HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json("Scheduler not initialized")),
        ))
    })
}

// 请求/响应类型

#[derive(Debug, Deserialize)]
pub struct RegisterTaskRequest {
    pub tapp_id: String,
    pub task_id: String,
    pub name: String,
    pub schedule_type: String,
    pub schedule: ScheduleConfigRequest,
    #[serde(default)]
    pub payload: Option<Value>,
    #[serde(default = "default_execution_target")]
    pub execution_target: String,
    #[serde(default)]
    pub backend_actions: Option<Value>,
    #[serde(default = "default_missed_policy")]
    pub missed_policy: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default)]
    pub retry: Option<RetryConfigRequest>,
}

fn default_execution_target() -> String {
    "frontend".to_string()
}

fn default_missed_policy() -> String {
    "skip".to_string()
}

fn default_scope() -> String {
    "user".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScheduleConfigRequest {
    #[serde(default)]
    pub cron: Option<String>,
    #[serde(default)]
    pub interval: Option<i64>,
    #[serde(default)]
    pub at: Option<i64>,
    #[serde(default)]
    pub time: Option<String>,
    /// Wall-clock TZ for daily: `local` (process TZ) | `UTC` | `+08:00`
    #[serde(default)]
    pub timezone: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryConfigRequest {
    #[serde(default)]
    pub max_retries: i32,
    #[serde(default)]
    pub retry_delay: i64,
}

#[derive(Debug, Deserialize)]
pub struct ListTasksQuery {
    pub tapp_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskResponse {
    pub id: i32,
    pub task_id: String,
    pub tapp_id: String,
    pub name: String,
    pub schedule_type: String,
    pub schedule: Value,
    pub payload: Option<Value>,
    pub execution_target: String,
    pub enabled: bool,
    pub missed_policy: String,
    pub scope: String,
    pub next_run_at: Option<String>,
    pub last_run_at: Option<String>,
    pub last_run_result: Option<Value>,
    pub stats: Value,
    pub created_at: String,
}

/// `{success, task}` 单任务响应；直接序列化，不先物化成 JSON Value。
#[derive(Debug, Serialize)]
pub struct TaskEnvelope {
    pub success: bool,
    pub task: TaskResponse,
}

impl TaskEnvelope {
    fn new(task: crate::models::entities::tapp_scheduled_tasks::Model) -> Self {
        Self {
            success: true,
            task: task_to_response(task),
        }
    }
}

/// `{success, tapp_id?, tasks, total}` 列表响应；普通列表不输出 `tapp_id`。
#[derive(Debug, Serialize)]
pub struct TaskListEnvelope {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tapp_id: Option<String>,
    pub tasks: Vec<TaskResponse>,
    pub total: usize,
}

impl TaskListEnvelope {
    fn new(
        tapp_id: Option<String>,
        tasks: Vec<crate::models::entities::tapp_scheduled_tasks::Model>,
    ) -> Self {
        let tasks: Vec<TaskResponse> = tasks.into_iter().map(task_to_response).collect();
        Self {
            success: true,
            tapp_id,
            total: tasks.len(),
            tasks,
        }
    }
}

// 辅助函数

fn parse_schedule_type(s: &str) -> Result<ScheduleType, HttpError> {
    match s.to_lowercase().as_str() {
        "cron" => Ok(ScheduleType::Cron),
        "interval" => Ok(ScheduleType::Interval),
        "once" => Ok(ScheduleType::Once),
        "daily" => Ok(ScheduleType::Daily),
        _ => Err(bad_request("Invalid schedule config".to_string())),
    }
}

fn parse_execution_target(s: &str) -> Result<ExecutionTarget, HttpError> {
    match s.to_lowercase().as_str() {
        "backend" => Ok(ExecutionTarget::Backend),
        "frontend" => Ok(ExecutionTarget::Frontend),
        "both" => Ok(ExecutionTarget::Both),
        _ => Err(bad_request("Invalid schedule config".to_string())),
    }
}

fn parse_missed_policy(s: &str) -> Result<MissedPolicy, HttpError> {
    match s.to_lowercase().as_str() {
        "skip" => Ok(MissedPolicy::Skip),
        "run-once" | "runonce" => Ok(MissedPolicy::RunOnce),
        "run-all" | "runall" => Ok(MissedPolicy::RunAll),
        _ => Err(bad_request("Invalid schedule config".to_string())),
    }
}

fn parse_scope(s: &str) -> Result<TaskScope, HttpError> {
    match s.to_lowercase().as_str() {
        "user" => Ok(TaskScope::User),
        "tapp" => Ok(TaskScope::Tapp),
        "tapp-per-user" | "tapp_per_user" => Ok(TaskScope::TappPerUser),
        "global" => Ok(TaskScope::Global),
        _ => Err(bad_request("Invalid schedule config".to_string())),
    }
}

fn normalize_retry_config(retry: Option<RetryConfigRequest>) -> Result<Option<Value>, HttpError> {
    let Some(retry) = retry else {
        return Ok(None);
    };
    if !(0..=MAX_SCHEDULER_RETRIES).contains(&retry.max_retries) {
        return Err(bad_request(format!(
            "retry.maxRetries must be between 0 and {MAX_SCHEDULER_RETRIES}"
        )));
    }
    if !(0..=MAX_SCHEDULER_RETRY_DELAY_MS).contains(&retry.retry_delay) {
        return Err(bad_request(format!(
            "retry.retryDelay must be between 0 and {MAX_SCHEDULER_RETRY_DELAY_MS}ms"
        )));
    }
    Ok(Some(json!({
        "max_retries": retry.max_retries,
        "retry_delay": retry.retry_delay.max(1_000),
    })))
}

fn bad_request(message: impl Into<String>) -> HttpError {
    HttpError::from((
        StatusCode::BAD_REQUEST,
        Json(AppError::public_json(message)),
    ))
}

fn schedule_type_name(value: &ScheduleType) -> &'static str {
    match value {
        ScheduleType::Cron => "cron",
        ScheduleType::Interval => "interval",
        ScheduleType::Once => "once",
        ScheduleType::Daily => "daily",
    }
}

fn missed_policy_name(value: &MissedPolicy) -> &'static str {
    match value {
        MissedPolicy::Skip => "skip",
        MissedPolicy::RunOnce => "run-once",
        MissedPolicy::RunAll => "run-all",
    }
}

fn execution_target_name(value: &ExecutionTarget) -> &'static str {
    match value {
        ExecutionTarget::Backend => "backend",
        ExecutionTarget::Frontend => "frontend",
        ExecutionTarget::Both => "both",
    }
}

fn task_scope_name(value: &TaskScope) -> &'static str {
    match value {
        TaskScope::User => "user",
        TaskScope::Tapp => "tapp",
        TaskScope::TappPerUser => "tapp-per-user",
        TaskScope::Global => "global",
    }
}

fn parse_user_id(claims: &Claims) -> Result<i32, HttpError> {
    crate::services::tapp_ownership::positive_user_id(&claims.sub).ok_or_else(|| {
        HttpError::from((
            StatusCode::UNAUTHORIZED,
            Json(AppError::public_json("Invalid user ID")),
        ))
    })
}

fn task_to_response(task: crate::models::entities::tapp_scheduled_tasks::Model) -> TaskResponse {
    TaskResponse {
        id: task.id,
        task_id: task.task_id,
        tapp_id: task.tapp_id,
        name: task.name,
        schedule_type: schedule_type_name(&task.schedule_type).to_string(),
        schedule: task.schedule_config,
        payload: task.payload,
        execution_target: execution_target_name(&task.execution_target).to_string(),
        enabled: task.enabled,
        missed_policy: missed_policy_name(&task.missed_policy).to_string(),
        scope: task_scope_name(&task.scope).to_string(),
        // RFC3339 for Safari/FE Date parsing (not chrono Display).
        next_run_at: task.next_run_at.map(|t| {
            let dt: chrono::DateTime<chrono::Utc> = t.into();
            dt.to_rfc3339()
        }),
        last_run_at: task.last_run_at.map(|t| {
            let dt: chrono::DateTime<chrono::Utc> = t.into();
            dt.to_rfc3339()
        }),
        last_run_result: task.last_run_result,
        stats: task.stats,
        created_at: {
            let dt: chrono::DateTime<chrono::Utc> = task.created_at.into();
            dt.to_rfc3339()
        },
    }
}

// API 端点

/// 注册定时任务
/// POST /api/tapp/scheduler/tasks
pub async fn register_task(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Json(req): Json<RegisterTaskRequest>,
) -> Result<Json<TaskEnvelope>, HttpError> {
    runtime_grant.require_tapp_id(&req.tapp_id)?;
    runtime_grant.require(TappPermission::SchedulerRegister)?;
    let user_id = parse_user_id(&claims)?;

    let schedule_type = parse_schedule_type(&req.schedule_type)?;
    let execution_target = parse_execution_target(&req.execution_target)?;
    let missed_policy = parse_missed_policy(&req.missed_policy)?;
    let scope = parse_scope(&req.scope)?;

    // 跨全局用户执行只允许当前仍为管理员的账号创建，不能信任旧 JWT 中的角色。
    if matches!(scope, TaskScope::Global) {
        ensure_current_admin_on(&claims, &db).await?;
    }

    let (backend_actions, wrappers) =
        normalize_backend_actions_parsed(req.backend_actions).map_err(bad_request)?;
    if matches!(
        execution_target,
        ExecutionTarget::Backend | ExecutionTarget::Both
    ) && wrappers.is_empty()
    {
        return Err(bad_request(
            "backendActions are required when executionTarget is backend or both",
        ));
    }
    let action_permissions = backend_action_permissions_of(&wrappers);
    for permission in &action_permissions {
        runtime_grant.require(*permission)?;
    }
    let mut installed_permissions = vec![TappPermission::SchedulerRegister];
    installed_permissions.extend(action_permissions);
    verify_tapp_approved_permissions(&db, user_id, &req.tapp_id, &installed_permissions).await?;
    let tapp = resolve_accessible_tapp(&db, user_id, &req.tapp_id).await?;
    if tapp.user_id != runtime_grant.owner_id() {
        return Err(HttpError::from((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Runtime grant installation mismatch",
                "code": "RUNTIME_GRANT_OWNER_MISMATCH"
            })),
        )));
    }
    validate_backend_action_declarations_of(&tapp.manifest, &wrappers).map_err(bad_request)?;

    let schedule_config = serde_json::to_value(&req.schedule).map_err(|e| {
        tracing::warn!("Invalid schedule config: {e}");
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid schedule config",
                "code": "schedule_invalid"
            })),
        )
    })?;

    let retry_config = normalize_retry_config(req.retry)?;

    let scheduler = get_scheduler()?;

    let task = scheduler
        .register_task(
            user_id,
            &req.tapp_id,
            &req.task_id,
            &req.name,
            schedule_type,
            schedule_config,
            req.payload,
            execution_target,
            backend_actions,
            missed_policy,
            scope,
            retry_config,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::public_json(e)),
            )
        })?;

    Ok(Json(TaskEnvelope::new(task)))
}

/// 删除定时任务
/// DELETE /api/tapp/scheduler/{tapp_id}/tasks/{task_id}
pub async fn unregister_task(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, task_id)): Path<(String, String)>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(TappPermission::SchedulerRegister)?;
    let user_id = parse_user_id(&claims)?;
    let scheduler = get_scheduler()?;

    scheduler
        .unregister_task(user_id, &tapp_id, &task_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::public_json(e)),
            )
        })?;

    Ok(Json(json!({
        "success": true,
        "message": format!("Task {} deleted", task_id),
    })))
}

/// 获取任务列表
/// GET /api/tapp/scheduler/tasks
pub async fn list_tasks(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Query(query): Query<ListTasksQuery>,
) -> Result<Json<TaskListEnvelope>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    let scheduler = get_scheduler()?;

    let tasks = scheduler
        .list_tasks(user_id, query.tapp_id.as_deref())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::public_json(e)),
            )
        })?;

    Ok(Json(TaskListEnvelope::new(None, tasks)))
}

/// 获取 Tapp 的任务列表
/// GET /api/tapp/scheduler/{tapp_id}/tasks
pub async fn list_tapp_tasks(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
) -> Result<Json<TaskListEnvelope>, HttpError> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(TappPermission::SchedulerRegister)?;
    let user_id = parse_user_id(&claims)?;
    let scheduler = get_scheduler()?;

    let tasks = scheduler
        .list_tasks(user_id, Some(&tapp_id))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::public_json(e)),
            )
        })?;

    Ok(Json(TaskListEnvelope::new(Some(tapp_id), tasks)))
}

/// 获取单个任务
/// GET /api/tapp/scheduler/{tapp_id}/tasks/{task_id}
pub async fn get_task(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, task_id)): Path<(String, String)>,
) -> Result<Json<TaskEnvelope>, HttpError> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(TappPermission::SchedulerRegister)?;
    let user_id = parse_user_id(&claims)?;
    let scheduler = get_scheduler()?;

    let task = scheduler
        .get_task(user_id, &tapp_id, &task_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::public_json(e)),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(AppError::public_json("Task not found")),
            )
        })?;

    Ok(Json(TaskEnvelope::new(task)))
}

/// 启用任务
/// POST /api/tapp/scheduler/{tapp_id}/tasks/{task_id}/enable
pub async fn enable_task(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, task_id)): Path<(String, String)>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(TappPermission::SchedulerRegister)?;
    let user_id = parse_user_id(&claims)?;
    let scheduler = get_scheduler()?;

    scheduler
        .set_task_enabled(user_id, &tapp_id, &task_id, true)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::public_json(e)),
            )
        })?;

    Ok(Json(json!({
        "success": true,
        "enabled": true,
    })))
}

/// 禁用任务
/// POST /api/tapp/scheduler/{tapp_id}/tasks/{task_id}/disable
pub async fn disable_task(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, task_id)): Path<(String, String)>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(TappPermission::SchedulerRegister)?;
    let user_id = parse_user_id(&claims)?;
    let scheduler = get_scheduler()?;

    scheduler
        .set_task_enabled(user_id, &tapp_id, &task_id, false)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::public_json(e)),
            )
        })?;

    Ok(Json(json!({
        "success": true,
        "enabled": false,
    })))
}

/// 手动触发任务
/// POST /api/tapp/scheduler/{tapp_id}/tasks/{task_id}/trigger
pub async fn trigger_task(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, task_id)): Path<(String, String)>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(TappPermission::SchedulerRegister)?;
    let user_id = parse_user_id(&claims)?;
    let scheduler = get_scheduler()?;

    scheduler
        .trigger_task(user_id, &tapp_id, &task_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::public_json(e)),
            )
        })?;

    Ok(Json(json!({
        "success": true,
        "triggered": true,
    })))
}

/// WebSocket 连接处理（接收任务推送）
/// GET /api/tapp/scheduler/ws
pub async fn scheduler_websocket(
    State(db): State<DatabaseConnection>,
    ws: WebSocketUpgrade,
    Extension(claims): Extension<Claims>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let allowed = crate::middleware::ws_origin::allowed_origins_from_global_config().await;
    if let Err(err) =
        crate::middleware::ws_origin::assert_ws_origin_for_cookie_session(&headers, &allowed)
    {
        return err.into_response();
    }
    let user_id = match parse_user_id(&claims) {
        Ok(id) => id,
        Err(error) => return error.into_response(),
    };
    ws.on_upgrade(move |socket| handle_scheduler_socket(socket, user_id, db))
        .into_response()
}

async fn handle_scheduler_socket(socket: WebSocket, user_id: i32, db: DatabaseConnection) {
    tracing::info!("[TappScheduler] WebSocket connected for user {}", user_id);

    let scheduler = match try_scheduler_engine() {
        Some(s) => s,
        None => {
            tracing::error!("[TappScheduler] Scheduler not initialized");
            return;
        }
    };

    let (mut sender, mut receiver) = socket.split();
    let connection_id = format!("scheduler_ws_{}", Uuid::new_v4().simple());
    if let Err(error) = register_frontend_connection(&db, user_id, &connection_id).await {
        tracing::error!(%error, user_id, "[TappScheduler] Failed to register WebSocket presence");
        return;
    }
    let mut presence_refresh = tokio::time::interval(tokio::time::Duration::from_secs(
        SCHEDULER_PRESENCE_REFRESH_SECONDS,
    ));
    let mut mailbox_poll = tokio::time::interval(tokio::time::Duration::from_millis(
        SCHEDULER_MAILBOX_POLL_MILLIS,
    ));

    // 发送欢迎消息
    let welcome = json!({
        "type": "connected",
        "user_id": user_id,
        "message": "Connected to scheduler"
    });
    if sender
        .send(Message::Text(
            serde_json::to_string(&welcome).unwrap().into(),
        ))
        .await
        .is_err()
    {
        let _ = unregister_frontend_connection(&db, &connection_id).await;
        return;
    }

    // 并发处理：接收任务推送 + 处理客户端消息
    loop {
        tokio::select! {
            _ = presence_refresh.tick() => {
                if let Err(error) = register_frontend_connection(&db, user_id, &connection_id).await {
                    tracing::warn!(%error, user_id, "[TappScheduler] Failed to refresh WebSocket presence");
                }
            }
            _ = mailbox_poll.tick() => {
                match drain_frontend_messages(&db, &connection_id).await {
                    Ok(messages) => {
                        let mut disconnected = false;
                        for task_msg in messages {
                            let msg = match serde_json::to_string(&task_msg) {
                                Ok(msg) => msg,
                                Err(error) => {
                                    tracing::error!(%error, "[TappScheduler] Failed to serialize queued task");
                                    continue;
                                }
                            };
                            if sender.send(Message::Text(msg.into())).await.is_err() {
                                disconnected = true;
                                break;
                            }
                        }
                        if disconnected {
                            break;
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, user_id, "[TappScheduler] Failed to poll shared mailbox");
                    }
                }
            }
            // 处理客户端消息
            client_msg = receiver.next() => {
                match client_msg {
                    Some(Ok(Message::Text(text))) => {
                        // 处理客户端消息（心跳、任务完成报告等）
                        if let Ok(msg) = serde_json::from_str::<Value>(&text) {
                            if msg.get("type").and_then(|t| t.as_str()) == Some("ping") {
                                let pong = json!({ "type": "pong" });
                                let _ = sender.send(Message::Text(
                                    serde_json::to_string(&pong).unwrap().into()
                                )).await;
                            } else if msg.get("type").and_then(|t| t.as_str()) == Some("task:complete") {
                                let execution_id = msg
                                    .get("executionId")
                                    .and_then(Value::as_i64)
                                    .and_then(|id| i32::try_from(id).ok());
                                let success = msg
                                    .get("success")
                                    .and_then(Value::as_bool);
                                let error = msg
                                    .get("error")
                                    .and_then(Value::as_str)
                                    .map(str::to_string);

                                if let (Some(execution_id), Some(success)) = (execution_id, success) {
                                    if let Err(error) = scheduler
                                        .complete_frontend_execution(
                                            user_id,
                                            execution_id,
                                            success,
                                            error,
                                        )
                                        .await
                                    {
                                        tracing::warn!(
                                            "[TappScheduler] Failed to complete execution {}: {}",
                                            execution_id,
                                            error
                                        );
                                    }
                                } else {
                                    tracing::warn!(
                                        "[TappScheduler] Invalid task:complete payload: {:?}",
                                        msg
                                    );
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    if let Err(error) = unregister_frontend_connection(&db, &connection_id).await {
        tracing::warn!(%error, user_id, "[TappScheduler] Failed to remove WebSocket presence");
    }

    tracing::info!(
        "[TappScheduler] WebSocket disconnected for user {}",
        user_id
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_ws_does_not_register_subject_minus_one() {
        let src = include_str!("tapp_scheduler.rs");
        let production = src.split("#[cfg(test)]").next().expect("production");
        assert!(production.contains("positive_user_id"));
        assert!(!production.contains("unwrap_or(-1)"));
        assert!(production.contains("parse_user_id(&claims)"));
    }

    #[test]
    fn normalizes_sdk_backend_action_tag() {
        let (normalized, _) = normalize_backend_actions_parsed(Some(json!([
            { "type": "storage.set", "key": "lastSync", "value": 1 },
            { "action": "transform", "input": "lastSync" }
        ])))
        .expect("actions should normalize");
        let normalized = normalized.expect("actions should remain present");

        let actions = normalized.as_array().expect("actions array");
        assert_eq!(actions[0]["action"], "storage.set");
        assert!(actions[0].get("type").is_none());
        assert_eq!(actions[1]["action"], "transform");
    }

    #[test]
    fn task_list_envelope_keeps_wire_shape() {
        let plain = serde_json::to_value(TaskListEnvelope::new(None, Vec::new())).unwrap();
        assert_eq!(plain, json!({"success": true, "tasks": [], "total": 0}));

        let scoped =
            serde_json::to_value(TaskListEnvelope::new(Some("com.example".into()), Vec::new()))
                .unwrap();
        assert_eq!(
            scoped,
            json!({"success": true, "tapp_id": "com.example", "tasks": [], "total": 0})
        );
    }

    #[test]
    fn retry_request_uses_sdk_camel_case() {
        let retry: RetryConfigRequest = serde_json::from_value(json!({
            "maxRetries": 3,
            "retryDelay": 2500
        }))
        .expect("retry config should deserialize");

        assert_eq!(retry.max_retries, 3);
        assert_eq!(retry.retry_delay, 2500);
    }

    #[test]
    fn retry_config_is_bounded_by_scheduler_lease_contract() {
        let normalized = normalize_retry_config(Some(RetryConfigRequest {
            max_retries: MAX_SCHEDULER_RETRIES,
            retry_delay: 0,
        }))
        .expect("bounded retry should be accepted")
        .expect("retry config");
        assert_eq!(normalized["retry_delay"], 1_000);

        assert!(
            normalize_retry_config(Some(RetryConfigRequest {
                max_retries: MAX_SCHEDULER_RETRIES + 1,
                retry_delay: 1_000,
            }))
            .is_err()
        );
        assert!(
            normalize_retry_config(Some(RetryConfigRequest {
                max_retries: 0,
                retry_delay: MAX_SCHEDULER_RETRY_DELAY_MS + 1,
            }))
            .is_err()
        );
    }

    #[test]
    fn scope_wire_names_are_stable() {
        assert_eq!(
            parse_scope("tapp-per-user").unwrap(),
            TaskScope::TappPerUser
        );
        assert_eq!(task_scope_name(&TaskScope::TappPerUser), "tapp-per-user");
        assert_eq!(missed_policy_name(&MissedPolicy::RunOnce), "run-once");
    }
}
use myriad_error::AppError;
