use crate::services::smart_filter::SmartFilter;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
/// 后台数据处理系统
///
/// In-memory task records + `processing_platforms` dedup.
/// Submission and execution share one service boundary.
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Platforms accepted by POST /api/tasks reprocess (smart_filter ids; seeds use `netease_music`).
pub(crate) const TASK_SUPPORTED_PLATFORMS: &[&str] = &[
    "netease", "bilibili", "github", "steam", "youtube", "bangumi", "x", "discord", "mal", "xbox",
    "psn",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingTask {
    pub id: String,
    pub platform: String,
    pub status: TaskStatus,
    pub progress: f32,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

pub struct BackgroundProcessor {
    /// All task records (Pending/Processing/Completed/Failed) until cleanup.
    tasks: Arc<RwLock<HashMap<String, ProcessingTask>>>,
    /// 正在处理的平台（防止重复）
    processing_platforms: Arc<RwLock<HashMap<String, String>>>, // platform -> task_id
}

impl BackgroundProcessor {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            processing_platforms: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 提交新的处理任务
    async fn submit_task(&self, platform: &str) -> (String, bool) {
        // 检查是否已有该平台的处理任务
        let mut processing = self.processing_platforms.write().await;
        if let Some(existing_task_id) = processing.get(platform) {
            // 检查任务状态
            let tasks = self.tasks.read().await;
            if let Some(task) = tasks.get(existing_task_id) {
                match task.status {
                    TaskStatus::Pending | TaskStatus::Processing => {
                        tracing::info!(
                            "Task for {} already in progress: {}",
                            platform,
                            existing_task_id
                        );
                        return (existing_task_id.clone(), false);
                    }
                    _ => {}
                }
            }
        }

        // 创建新任务
        let task_id = format!("{}_{}", platform, uuid::Uuid::new_v4());
        let task = ProcessingTask {
            id: task_id.clone(),
            platform: platform.to_owned(),
            status: TaskStatus::Pending,
            progress: 0.0,
            error: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
        };

        // 保存任务
        self.cleanup_old_tasks().await;
        let mut tasks = self.tasks.write().await;
        tasks.insert(task_id.clone(), task);
        drop(tasks);

        // 注册处理中的平台
        processing.insert(platform.to_owned(), task_id.clone());
        drop(processing);

        tracing::info!("✓ Task {} created for platform {}", task_id, platform);
        (task_id, true)
    }

    /// 获取任务状态
    pub async fn get_task_status(&self, task_id: &str) -> Option<ProcessingTask> {
        let tasks = self.tasks.read().await;
        tasks.get(task_id).cloned()
    }

    /// 获取平台的当前任务
    pub async fn get_platform_task(&self, platform: &str) -> Option<ProcessingTask> {
        let processing = self.processing_platforms.read().await;
        let task_id = processing.get(platform)?.clone();
        drop(processing);
        self.get_task_status(&task_id).await
    }

    /// 更新任务状态
    pub async fn update_task(
        &self,
        task_id: &str,
        status: TaskStatus,
        progress: f32,
        error: Option<String>,
    ) {
        let mut processing = self.processing_platforms.write().await;
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.status = status.clone();
            task.progress = progress;
            task.error = error;
            task.updated_at = Utc::now();

            if status == TaskStatus::Completed || status == TaskStatus::Failed {
                task.completed_at = Some(Utc::now());

                // 从处理中列表移除
                let platform = task.platform.clone();
                if processing.get(&platform).is_some_and(|id| id == task_id) {
                    processing.remove(&platform);
                }
            }
        }
    }

    /// 标记任务完成
    pub async fn complete_task(&self, task_id: &str) {
        self.update_task(task_id, TaskStatus::Completed, 100.0, None)
            .await;
    }

    /// 标记任务失败
    pub async fn fail_task(&self, task_id: &str, error: String) {
        self.update_task(task_id, TaskStatus::Failed, 0.0, Some(error))
            .await;
    }

    /// 总数达到 100 时，在已有 `completed_at` 的任务里只留最近 50 个。
    pub async fn cleanup_old_tasks(&self) {
        let mut tasks = self.tasks.write().await;

        if tasks.len() < 100 {
            return;
        }

        // 按完成时间排序，删除最旧的任务
        let mut task_vec: Vec<_> = tasks
            .iter()
            .filter(|(_, t)| t.completed_at.is_some())
            .map(|(id, t)| (id.clone(), t.completed_at.unwrap()))
            .collect();

        task_vec.sort_by_key(|a| a.1);

        // 删除最旧的任务
        let to_remove = task_vec.len().saturating_sub(50);
        for (id, _) in task_vec.iter().take(to_remove) {
            tasks.remove(id);
        }

        tracing::info!("Cleaned up {} old tasks", to_remove);
    }

    /// 获取任务统计信息（用于监控）
    pub async fn get_task_stats(&self) -> (usize, usize, usize, usize, usize) {
        let tasks = self.tasks.read().await;

        let mut pending = 0;
        let mut processing = 0;
        let mut completed = 0;
        let mut failed = 0;

        for task in tasks.values() {
            match task.status {
                TaskStatus::Pending => pending += 1,
                TaskStatus::Processing => processing += 1,
                TaskStatus::Completed => completed += 1,
                TaskStatus::Failed => failed += 1,
            }
        }

        (tasks.len(), pending, processing, completed, failed)
    }

    /// 返回按最近更新时间倒序排列的任务快照，供管理员诊断使用。
    ///
    /// 任务错误可能包含平台响应摘要，因此这里只暴露给受管理员权限保护的
    /// 诊断接口，并由接口进一步限制数量和错误文本长度。
    pub async fn list_recent_tasks(&self, limit: usize) -> Vec<ProcessingTask> {
        let tasks = self.tasks.read().await;
        let mut recent = tasks.values().cloned().collect::<Vec<_>>();
        recent.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        recent.truncate(limit);
        recent
    }
}

// 全局后台处理器实例
use once_cell::sync::Lazy;
pub static BACKGROUND_PROCESSOR: Lazy<BackgroundProcessor> = Lazy::new(BackgroundProcessor::new);

/// Validate, atomically reuse/create a task, and start only the creator's worker.
pub(crate) async fn submit_and_start_platform_task(platform: String) -> Result<String, String> {
    if !TASK_SUPPORTED_PLATFORMS.contains(&platform.as_str()) {
        return Err(format!(
            "Invalid platform. Supported: {}",
            TASK_SUPPORTED_PLATFORMS.join(", ")
        ));
    }
    let (task_id, is_new) = BACKGROUND_PROCESSOR.submit_task(&platform).await;
    if is_new {
        let worker_task_id = task_id.clone();
        tokio::spawn(process_platform_task(worker_task_id, platform));
    }
    Ok(task_id)
}

/// 后台处理函数
async fn process_platform_task(task_id: String, platform: String) {
    let task_id = task_id.as_str();
    let platform = platform.as_str();
    use crate::services::background_processor::TaskStatus;
    use std::fs;

    tracing::info!("🚀 Starting background task {} for {}", task_id, platform);

    // 更新为处理中
    BACKGROUND_PROCESSOR
        .update_task(task_id, TaskStatus::Processing, 0.0, None)
        .await;

    // 读取分平台原始数据；缺失则失败（无回退源）
    let split_raw_path = crate::services::data_paths::platform_raw_file(platform);
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

    // Process and save via SmartFilter.
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

    #[tokio::test]
    async fn service_rejects_unknown_platform_without_registering_it() {
        let platform = format!("invalid-{}", uuid::Uuid::new_v4());
        assert!(
            submit_and_start_platform_task(platform.clone())
                .await
                .is_err()
        );
        assert!(
            BACKGROUND_PROCESSOR
                .get_platform_task(&platform)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn service_starts_new_task_and_reaches_terminal_state() {
        let id = submit_and_start_platform_task("github".into())
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let task = BACKGROUND_PROCESSOR.get_task_status(&id).await.unwrap();
                if matches!(task.status, TaskStatus::Completed | TaskStatus::Failed) {
                    assert!(task.completed_at.is_some());
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("new tasks must have an executor");
    }

    #[tokio::test]
    async fn completed_task_churn_is_bounded_without_waiting_for_cleanup() {
        let processor = BackgroundProcessor::new();
        for _ in 0..250 {
            let (id, _) = processor.submit_task("github").await;
            processor.complete_task(&id).await;
        }
        assert!(processor.get_task_stats().await.0 <= 100);
    }

    #[tokio::test]
    async fn repeated_completion_does_not_remove_new_platform_task() {
        let processor = BackgroundProcessor::new();
        let (old, _) = processor.submit_task("github").await;
        processor.complete_task(&old).await;
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let (new, _) = processor.submit_task("github").await;
        processor.complete_task(&old).await;
        assert_eq!(processor.get_platform_task("github").await.unwrap().id, new);
    }

    #[tokio::test]
    async fn concurrent_submissions_have_one_creator() {
        let processor = Arc::new(BackgroundProcessor::new());
        let mut submitters = tokio::task::JoinSet::new();
        for _ in 0..64 {
            let processor = processor.clone();
            submitters.spawn(async move { processor.submit_task("github").await });
        }
        let mut ids = std::collections::HashSet::new();
        let mut claims = 0;
        while let Some(result) = submitters.join_next().await {
            let (id, claimed) = result.unwrap();
            ids.insert(id);
            claims += usize::from(claimed);
        }
        assert_eq!(ids.len(), 1);
        assert_eq!(claims, 1);
        assert_eq!(processor.get_task_stats().await, (1, 1, 0, 0, 0));
    }

    #[tokio::test]
    async fn successive_tasks_have_unique_ids_without_clock_delay() {
        let processor = BackgroundProcessor::new();
        let mut ids = std::collections::HashSet::new();
        for _ in 0..250 {
            let (id, _) = processor.submit_task("github").await;
            assert!(ids.insert(id.clone()));
            processor.complete_task(&id).await;
        }
    }
}
