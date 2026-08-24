use crate::services::background_processor::BACKGROUND_PROCESSOR;
use crate::services::smart_filter::SmartFilter;
/// 后台任务管理 API
///
/// 提供异步任务提交、状态查询、进度跟踪等功能
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Platforms accepted by POST /api/tasks reprocess (must match smart_filter / seeds).
const TASK_SUPPORTED_PLATFORMS: &[&str] = &[
    "netease", "bilibili", "github", "steam", "youtube", "bangumi", "x", "discord", "mal", "xbox",
    "psn",
];

fn is_task_supported_platform(platform: &str) -> bool {
    TASK_SUPPORTED_PLATFORMS.contains(&platform)
}

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

    // 验证平台名称
    if !is_task_supported_platform(&platform) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": format!(
                    "Invalid platform. Supported: {}",
                    TASK_SUPPORTED_PLATFORMS.join(", ")
                )
            })),
        );
    }

    // 提交任务到后台处理器
    match BACKGROUND_PROCESSOR.submit_task(platform.clone()).await {
        Ok(task_id) => {
            // 在后台异步执行处理
            let task_id_clone = task_id.clone();
            let platform_clone = platform.clone();

            tokio::spawn(async move {
                process_platform_task(task_id_clone, platform_clone).await;
            });

            (
                StatusCode::ACCEPTED,
                Json(json!({
                    "success": true,
                    "task_id": task_id,
                    "platform": platform,
                    "status": "pending",
                    "message": "Task submitted successfully. Use GET /api/tasks/{task_id} to check status."
                })),
            )
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "error": err
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
            Json(json!({
                "success": false,
                "error": "Task not found"
            })),
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

/// 列出所有任务
///
/// GET /api/tasks
pub async fn list_tasks(State(_db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    // 由于 BackgroundProcessor 没有提供 list_all 方法，我们暂时返回提示
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": "Use GET /api/tasks/platform/{platform} to check specific platform tasks",
            "supported_platforms": TASK_SUPPORTED_PLATFORMS
        })),
    )
}

/// 后台处理函数
async fn process_platform_task(task_id: String, platform: String) {
    let task_id = task_id.as_str();
    let platform = platform.as_str();
    use crate::services::background_processor::TaskStatus;
    use std::fs;
    use std::path::PathBuf;

    tracing::info!("🚀 Starting background task {} for {}", task_id, platform);

    // 更新为处理中
    BACKGROUND_PROCESSOR
        .update_task(task_id, TaskStatus::Processing, 0.0, None)
        .await;

    // 尝试读取分平台的原始数据文件（优先）
    let split_raw_path = PathBuf::from(format!("./cache/raw/{}.json", platform));
    let mut platform_data_value: Option<Value> = None;

    if split_raw_path.exists() {
        tracing::info!("📦 Found split raw data for {}", platform);
        BACKGROUND_PROCESSOR
            .update_task(task_id, TaskStatus::Processing, 20.0, None)
            .await;

        match fs::read_to_string(&split_raw_path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(json) => {
                    platform_data_value = Some(json);
                }
                Err(e) => {
                    tracing::warn!("Failed to parse split raw data: {}", e);
                }
            },
            Err(e) => {
                tracing::warn!("Failed to read split raw data: {}", e);
            }
        }
    }

    // 如果分平台数据读取失败，报错
    if platform_data_value.is_none() {
        tracing::error!("Raw data file not found for platform: {}", platform);
        BACKGROUND_PROCESSOR
            .fail_task(task_id, "Raw data file not found".to_string())
            .await;
        return;
    }

    let platform_data = platform_data_value.unwrap();

    // 5. 处理并保存
    BACKGROUND_PROCESSOR
        .update_task(task_id, TaskStatus::Processing, 60.0, None)
        .await;

    // 再次更新进度，准备开始处理
    BACKGROUND_PROCESSOR
        .update_task(task_id, TaskStatus::Processing, 80.0, None)
        .await;

    let process_result =
        SmartFilter::process_and_save_single(platform, &platform_data).map_err(|error| {
            tracing::error!("Failed to process {platform}: {error}");
            format!("Failed to process {platform}")
        });

    match process_result {
        Ok(_) => {
            tracing::info!("✓ Successfully processed {} in task {}", platform, task_id);
            BACKGROUND_PROCESSOR.complete_task(task_id).await;
        }
        Err(error) => {
            BACKGROUND_PROCESSOR.fail_task(task_id, error).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_supported_platforms_include_youtube_and_peers() {
        assert!(is_task_supported_platform("youtube"));
        assert!(is_task_supported_platform("steam"));
        assert!(is_task_supported_platform("github"));
        assert!(!is_task_supported_platform("not-a-platform"));
        assert!(TASK_SUPPORTED_PLATFORMS.contains(&"youtube"));
    }
}
