use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
/// 后台数据处理系统
///
/// In-memory task records + `processing_platforms` dedup.
/// Offload spawn lives in `api/tasks.rs`; `queue` is push-only (never drained).
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

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
    /// 任务队列
    queue: Arc<Mutex<Vec<String>>>,
    /// 正在处理的平台（防止重复）
    processing_platforms: Arc<RwLock<HashMap<String, String>>>, // platform -> task_id
}

impl BackgroundProcessor {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            queue: Arc::new(Mutex::new(Vec::new())),
            processing_platforms: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 提交新的处理任务
    pub async fn submit_task(&self, platform: String) -> Result<String, String> {
        // 检查是否已有该平台的处理任务
        let processing = self.processing_platforms.read().await;
        if let Some(existing_task_id) = processing.get(&platform) {
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
                        return Ok(existing_task_id.clone());
                    }
                    _ => {}
                }
            }
        }
        drop(processing);

        // 创建新任务
        let task_id = format!("{}_{}", platform, Utc::now().timestamp_millis());
        let task = ProcessingTask {
            id: task_id.clone(),
            platform: platform.clone(),
            status: TaskStatus::Pending,
            progress: 0.0,
            error: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
        };

        // 保存任务
        let mut tasks = self.tasks.write().await;
        tasks.insert(task_id.clone(), task);
        drop(tasks);

        // 注册处理中的平台
        let mut processing = self.processing_platforms.write().await;
        processing.insert(platform.clone(), task_id.clone());
        drop(processing);

        // 添加到队列
        let mut queue = self.queue.lock().await;
        queue.push(task_id.clone());
        drop(queue);

        tracing::info!("✓ Task {} created for platform {}", task_id, platform);
        Ok(task_id)
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
                drop(tasks);
                let mut processing = self.processing_platforms.write().await;
                processing.remove(&platform);
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

    /// 总数 > 100 时，在已有 `completed_at` 的任务里只留最近 50 个。
    pub async fn cleanup_old_tasks(&self) {
        let mut tasks = self.tasks.write().await;

        if tasks.len() <= 100 {
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
