use crate::services::background_processor::{
    BACKGROUND_PROCESSOR, TASK_SUPPORTED_PLATFORMS, submit_and_start_platform_task,
};
/// 后台任务管理 API
///
/// 提供异步任务提交、状态查询、进度跟踪等功能
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use myriad_error::AppError;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Serialize, Deserialize)]
pub struct SubmitTaskRequest {
    pub platform: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskResponse {
    pub id: String,
    pub platform: String,
    pub status: String,
    pub progress: f32,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

/// 提交平台数据处理任务
///
/// POST /api/tasks
/// Body: { "platform": "netease" }
pub async fn submit_task(
    State(_db): State<DatabaseConnection>,
    Json(payload): Json<SubmitTaskRequest>,
) -> (StatusCode, Json<Value>) {
    let platform = payload.platform.to_lowercase();

    // 提交任务到后台处理器
    match submit_and_start_platform_task(platform.clone()).await {
        Ok(task_id) => (
            StatusCode::ACCEPTED,
            Json(json!({
                "success": true,
                "task_id": task_id,
                "platform": platform,
                "status": "pending",
                "message": "Task submitted successfully. Use GET /api/tasks/{task_id} to check status."
            })),
        ),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": err,
                "code": "invalid_platform",
            })),
        ),
    }
}

/// 获取任务状态
///
/// GET /api/tasks/{task_id}
pub async fn get_task_status(
    State(_db): State<DatabaseConnection>,
    Path(task_id): Path<String>,
) -> (StatusCode, Json<Value>) {
    match BACKGROUND_PROCESSOR.get_task_status(&task_id).await {
        Some(task) => {
            let response = TaskResponse {
                id: task.id,
                platform: task.platform,
                status: format!("{:?}", task.status),
                progress: task.progress,
                error: task.error,
                created_at: task.created_at.to_rfc3339(),
                updated_at: task.updated_at.to_rfc3339(),
                completed_at: task.completed_at.map(|t| t.to_rfc3339()),
            };

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "task": response
                })),
            )
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(AppError::fail_json("Task not found")),
        ),
    }
}

/// 获取平台当前任务
///
/// GET /api/tasks/platform/{platform}
pub async fn get_platform_task(
    State(_db): State<DatabaseConnection>,
    Path(platform): Path<String>,
) -> (StatusCode, Json<Value>) {
    match BACKGROUND_PROCESSOR.get_platform_task(&platform).await {
        Some(task) => {
            let response = TaskResponse {
                id: task.id,
                platform: task.platform,
                status: format!("{:?}", task.status),
                progress: task.progress,
                error: task.error,
                created_at: task.created_at.to_rfc3339(),
                updated_at: task.updated_at.to_rfc3339(),
                completed_at: task.completed_at.map(|t| t.to_rfc3339()),
            };

            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "task": response
                })),
            )
        }
        None => (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "task": null,
                "message": "No active task for this platform"
            })),
        ),
    }
}

/// 平台列表提示（不是任务列表）
///
/// GET /api/tasks
pub async fn list_tasks(State(_db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    // 本路由返回平台列表提示；按平台查 GET /api/tasks/platform/{platform}。
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": "Use GET /api/tasks/platform/{platform} to check specific platform tasks",
            "supported_platforms": TASK_SUPPORTED_PLATFORMS
        })),
    )
}
