//! Tapp 定时任务 API
//!
//! 提供定时任务的 CRUD 和 WebSocket 推送功能

use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, Query, State, WebSocketUpgrade,
    },
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use futures::{SinkExt, StreamExt};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::middleware::auth::Claims;
use crate::models::entities::tapp_scheduled_tasks::{
    ExecutionTarget, MissedPolicy, ScheduleType, TaskScope,
};
use crate::services::tapp_scheduler::TappSchedulerEngine;

/// 全局调度器引擎
static SCHEDULER_ENGINE: once_cell::sync::OnceCell<Arc<RwLock<TappSchedulerEngine>>> =
    once_cell::sync::OnceCell::new();

/// 初始化调度器引擎
pub async fn init_scheduler(db: DatabaseConnection) {
    let engine = TappSchedulerEngine::new(db);
    engine.start().await;
    let _ = SCHEDULER_ENGINE.set(Arc::new(RwLock::new(engine)));
    tracing::info!("[TappScheduler] Scheduler initialized");
}

/// 关闭调度器引擎
pub async fn shutdown_scheduler() {
    if let Some(scheduler) = SCHEDULER_ENGINE.get() {
        let engine = scheduler.read().await;
        engine.stop().await;
        tracing::info!("[TappScheduler] Scheduler shutdown");
    }
}

/// 获取调度器引擎
fn get_scheduler() -> Result<Arc<RwLock<TappSchedulerEngine>>, (StatusCode, Json<Value>)> {
    SCHEDULER_ENGINE.get().cloned().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Scheduler not initialized" })),
        )
    })
}

// ============ 请求/响应类型 ============

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
}

#[derive(Debug, Deserialize)]
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

// ============ 辅助函数 ============

fn parse_schedule_type(s: &str) -> Result<ScheduleType, (StatusCode, Json<Value>)> {
    match s.to_lowercase().as_str() {
        "cron" => Ok(ScheduleType::Cron),
        "interval" => Ok(ScheduleType::Interval),
        "once" => Ok(ScheduleType::Once),
        "daily" => Ok(ScheduleType::Daily),
        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("Invalid schedule type: {}", s) })),
        )),
    }
}

fn parse_execution_target(s: &str) -> Result<ExecutionTarget, (StatusCode, Json<Value>)> {
    match s.to_lowercase().as_str() {
        "backend" => Ok(ExecutionTarget::Backend),
        "frontend" => Ok(ExecutionTarget::Frontend),
        "both" => Ok(ExecutionTarget::Both),
        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("Invalid execution target: {}", s) })),
        )),
    }
}

fn parse_missed_policy(s: &str) -> Result<MissedPolicy, (StatusCode, Json<Value>)> {
    match s.to_lowercase().as_str() {
        "skip" => Ok(MissedPolicy::Skip),
        "run-once" | "runonce" => Ok(MissedPolicy::RunOnce),
        "run-all" | "runall" => Ok(MissedPolicy::RunAll),
        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("Invalid missed policy: {}", s) })),
        )),
    }
}

fn parse_scope(s: &str) -> Result<TaskScope, (StatusCode, Json<Value>)> {
    match s.to_lowercase().as_str() {
        "user" => Ok(TaskScope::User),
        "tapp" => Ok(TaskScope::Tapp),
        "global" => Ok(TaskScope::Global),
        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("Invalid scope: {}", s) })),
        )),
    }
}

fn parse_user_id(claims: &Claims) -> Result<i32, (StatusCode, Json<Value>)> {
    claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user ID" })),
        )
    })
}

fn task_to_response(task: &crate::models::entities::tapp_scheduled_tasks::Model) -> TaskResponse {
    TaskResponse {
        id: task.id,
        task_id: task.task_id.clone(),
        tapp_id: task.tapp_id.clone(),
        name: task.name.clone(),
        schedule_type: format!("{:?}", task.schedule_type).to_lowercase(),
        schedule: task.schedule_config.clone(),
        payload: task.payload.clone(),
        execution_target: task.execution_target.to_string(),
        enabled: task.enabled,
        missed_policy: format!("{:?}", task.missed_policy).to_lowercase(),
        scope: format!("{:?}", task.scope).to_lowercase(),
        next_run_at: task.next_run_at.map(|t| t.to_string()),
        last_run_at: task.last_run_at.map(|t| t.to_string()),
        last_run_result: task.last_run_result.clone(),
        stats: task.stats.clone(),
        created_at: task.created_at.to_string(),
    }
}

// ============ API 端点 ============

/// 注册定时任务
/// POST /api/tapp/scheduler/tasks
pub async fn register_task(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<RegisterTaskRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let scheduler = get_scheduler()?;
    let scheduler = scheduler.read().await;

    // 权限检查：需要 scheduler:register 权限
    // TODO: 实现权限检查
    // 注意：global scope 需要管理员权限

    let schedule_type = parse_schedule_type(&req.schedule_type)?;
    let execution_target = parse_execution_target(&req.execution_target)?;
    let missed_policy = parse_missed_policy(&req.missed_policy)?;
    let scope = parse_scope(&req.scope)?;

    let schedule_config = serde_json::to_value(&req.schedule).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("Invalid schedule config: {}", e) })),
        )
    })?;

    let retry_config = req.retry.map(|r| {
        json!({
            "max_retries": r.max_retries,
            "retry_delay": r.retry_delay,
        })
    });

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
            req.backend_actions,
            missed_policy,
            scope,
            retry_config,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
        })?;

    Ok(Json(json!({
        "success": true,
        "task": task_to_response(&task),
    })))
}

/// 删除定时任务
/// DELETE /api/tapp/scheduler/:tapp_id/tasks/:task_id
pub async fn unregister_task(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, task_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let scheduler = get_scheduler()?;
    let scheduler = scheduler.read().await;

    scheduler
        .unregister_task(user_id, &tapp_id, &task_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
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
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let scheduler = get_scheduler()?;
    let scheduler = scheduler.read().await;

    let tasks = scheduler
        .list_tasks(user_id, query.tapp_id.as_deref())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
        })?;

    let task_responses: Vec<TaskResponse> = tasks.iter().map(task_to_response).collect();

    Ok(Json(json!({
        "success": true,
        "tasks": task_responses,
        "total": task_responses.len(),
    })))
}

/// 获取 Tapp 的任务列表
/// GET /api/tapp/scheduler/:tapp_id/tasks
pub async fn list_tapp_tasks(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let scheduler = get_scheduler()?;
    let scheduler = scheduler.read().await;

    let tasks = scheduler
        .list_tasks(user_id, Some(&tapp_id))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
        })?;

    let task_responses: Vec<TaskResponse> = tasks.iter().map(task_to_response).collect();

    Ok(Json(json!({
        "success": true,
        "tapp_id": tapp_id,
        "tasks": task_responses,
        "total": task_responses.len(),
    })))
}

/// 获取单个任务
/// GET /api/tapp/scheduler/:tapp_id/tasks/:task_id
pub async fn get_task(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, task_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let scheduler = get_scheduler()?;
    let scheduler = scheduler.read().await;

    let task = scheduler
        .get_task(user_id, &tapp_id, &task_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("Task {} not found", task_id) })),
            )
        })?;

    Ok(Json(json!({
        "success": true,
        "task": task_to_response(&task),
    })))
}

/// 启用任务
/// POST /api/tapp/scheduler/:tapp_id/tasks/:task_id/enable
pub async fn enable_task(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, task_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let scheduler = get_scheduler()?;
    let scheduler = scheduler.read().await;

    scheduler
        .set_task_enabled(user_id, &tapp_id, &task_id, true)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
        })?;

    Ok(Json(json!({
        "success": true,
        "enabled": true,
    })))
}

/// 禁用任务
/// POST /api/tapp/scheduler/:tapp_id/tasks/:task_id/disable
pub async fn disable_task(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, task_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let scheduler = get_scheduler()?;
    let scheduler = scheduler.read().await;

    scheduler
        .set_task_enabled(user_id, &tapp_id, &task_id, false)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
        })?;

    Ok(Json(json!({
        "success": true,
        "enabled": false,
    })))
}

/// 手动触发任务
/// POST /api/tapp/scheduler/:tapp_id/tasks/:task_id/trigger
pub async fn trigger_task(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, task_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = parse_user_id(&claims)?;
    let scheduler = get_scheduler()?;
    let scheduler = scheduler.read().await;

    scheduler
        .trigger_task(user_id, &tapp_id, &task_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
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
    ws: WebSocketUpgrade,
    Extension(claims): Extension<Claims>,
) -> impl IntoResponse {
    let user_id: i32 = claims.sub.parse().unwrap_or(-1);
    ws.on_upgrade(move |socket| handle_scheduler_socket(socket, user_id))
}

async fn handle_scheduler_socket(socket: WebSocket, user_id: i32) {
    tracing::info!("[TappScheduler] WebSocket connected for user {}", user_id);

    let scheduler = match SCHEDULER_ENGINE.get() {
        Some(s) => s.clone(),
        None => {
            tracing::error!("[TappScheduler] Scheduler not initialized");
            return;
        }
    };

    let (mut sender, mut receiver) = socket.split();

    // 订阅任务推送
    let mut task_rx = {
        let engine = scheduler.read().await;
        engine.subscribe_frontend_tasks()
    };

    // 发送欢迎消息
    let welcome = json!({
        "type": "connected",
        "user_id": user_id,
        "message": "Connected to scheduler"
    });
    if sender
        .send(Message::Text(serde_json::to_string(&welcome).unwrap()))
        .await
        .is_err()
    {
        return;
    }

    // 并发处理：接收任务推送 + 处理客户端消息
    loop {
        tokio::select! {
            // 接收任务推送
            task_result = task_rx.recv() => {
                match task_result {
                    Ok(task_msg) => {
                        // 根据 scope 和 target_users 决定是否推送
                        let should_send = match &task_msg.target_users {
                            // 有明确的目标用户列表
                            Some(users) => users.contains(&user_id),
                            // 没有目标用户列表，根据 scope 处理
                            None => {
                                match task_msg.task.scope.as_str() {
                                    "user" => task_msg.task.user_id == user_id,
                                    "tapp" => true,  // TODO: 检查用户是否安装了该 Tapp
                                    "global" => true, // 广播给所有在线用户
                                    _ => task_msg.task.user_id == user_id,
                                }
                            }
                        };

                        if should_send {
                            let msg = serde_json::to_string(&task_msg).unwrap_or_default();
                            if sender.send(Message::Text(msg)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("[TappScheduler] Task broadcast error: {}", e);
                        // 重新订阅
                        let engine = scheduler.read().await;
                        task_rx = engine.subscribe_frontend_tasks();
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
                                    serde_json::to_string(&pong).unwrap()
                                )).await;
                            } else if msg.get("type").and_then(|t| t.as_str()) == Some("task:complete") {
                                // 任务完成报告
                                tracing::debug!("[TappScheduler] Task complete report: {:?}", msg);
                                // TODO: 更新任务执行状态
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

    tracing::info!(
        "[TappScheduler] WebSocket disconnected for user {}",
        user_id
    );
}
