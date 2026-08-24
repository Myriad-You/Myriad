//! 任务存储模块
//!
//! 管理任务状态的内存存储和数据库持久化

use crate::models::entities::agent_tasks;
use crate::services::agent::task_store_pure::{
    self, is_terminal_past_retention, is_waiting_input_timed_out, lane_id_from_user_session,
    status_counts_from_iter, task_status_from_db_str, task_status_to_db_str,
    WAITING_INPUT_TIMEOUT_ERROR,
};
use crate::services::agent::types::*;
use chrono::Utc;
use once_cell::sync::Lazy;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Statement,
};
use serde_json::json;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

/// 全局任务状态存储
pub static TASK_STORE: Lazy<Arc<RwLock<TaskStore>>> =
    Lazy::new(|| Arc::new(RwLock::new(TaskStore::new())));

/// 全局数据库连接（用于任务持久化）
static DB_FOR_TASKS: Lazy<Arc<RwLock<Option<DatabaseConnection>>>> =
    Lazy::new(|| Arc::new(RwLock::new(None)));

/// 全局任务取消标记存储
pub static CANCELLATION_TOKENS: Lazy<Arc<RwLock<HashSet<String>>>> =
    Lazy::new(|| Arc::new(RwLock::new(HashSet::new())));

const STEERING_REGISTRY_NAMESPACE: &str = "agent_task_steering";

pub async fn enqueue_steering(
    db: &DatabaseConnection,
    task_id: &str,
    instruction: String,
) -> Result<(), String> {
    // One shared record per instruction prevents concurrent writers on
    // different replicas from overwriting each other.
    let record_id = format!("steer_{}", uuid::Uuid::new_v4().simple());
    crate::services::tapp_registry::put(
        db,
        STEERING_REGISTRY_NAMESPACE,
        &record_id,
        crate::services::tapp_registry::RegistryIdentity {
            subject_id: None,
            owner_id: None,
            tapp_id: None,
            runtime_id: Some(task_id),
        },
        &instruction,
        (Utc::now() + chrono::Duration::minutes(30)).timestamp(),
    )
    .await
    .map_err(|error| format!("Failed to persist steering instruction: {error}"))?;
    Ok(())
}

pub async fn take_steering(db: &DatabaseConnection, task_id: &str) -> Vec<String> {
    match crate::services::tapp_registry::take_all_for_runtime::<String>(
        db,
        STEERING_REGISTRY_NAMESPACE,
        task_id,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(task_id = %task_id, %error, "[TaskStore] Failed to consume shared steering instruction");
            Vec::new()
        }
    }
}

/// 检查任务是否被请求取消
pub async fn is_cancelled(task_id: &str) -> bool {
    if CANCELLATION_TOKENS.read().await.contains(task_id) {
        return true;
    }
    let Some(db) = DB_FOR_TASKS.read().await.clone() else {
        return false;
    };
    agent_tasks::Entity::find_by_id(task_id)
        .filter(agent_tasks::Column::Status.eq("cancelled"))
        .one(&db)
        .await
        .ok()
        .flatten()
        .is_some()
}

/// 清除取消标记（任务完成或已处理取消后）
pub async fn clear_cancellation(task_id: &str) {
    let mut tokens = CANCELLATION_TOKENS.write().await;
    tokens.remove(task_id);
}

/// 任务存储
pub struct TaskStore {
    /// 任务 ID -> 任务状态
    tasks: HashMap<String, TaskState>,
    /// 用户 ID -> 任务 ID 列表
    user_tasks: HashMap<i32, Vec<String>>,
}

impl TaskStore {
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            user_tasks: HashMap::new(),
        }
    }

    /// 存储任务（同时异步保存到数据库）
    pub fn store(&mut self, user_id: i32, task: TaskState) {
        let task_id = task.task_id.clone();

        // 先 insert 到内存，再从内存里借出一份 clone 给异步任务
        // 避免 task 被 move 前的额外 clone
        self.tasks.insert(task_id.clone(), task);
        self.user_tasks
            .entry(user_id)
            .or_default()
            .push(task_id.clone());

        // 从内存中取出已存储的任务做一次 clone 用于持久化
        // 这样可在 TaskState 较大时只 clone 一次，而非两次
        if let Some(task_for_db) = self.tasks.get(&task_id).cloned() {
            tokio::spawn(async move {
                if let Err(e) = save_task_to_db(user_id, &task_for_db).await {
                    tracing::warn!("保存任务到数据库失败: {}", e);
                }
            });
        }
    }

    /// 获取任务
    pub fn get(&self, task_id: &str) -> Option<&TaskState> {
        self.tasks.get(task_id)
    }

    /// 获取任务（可变）
    pub fn get_mut(&mut self, task_id: &str) -> Option<&mut TaskState> {
        self.tasks.get_mut(task_id)
    }

    /// 获取用户的所有任务
    pub fn get_user_tasks(&self, user_id: i32) -> Vec<&TaskState> {
        self.user_tasks
            .get(&user_id)
            .map(|ids| ids.iter().filter_map(|id| self.tasks.get(id)).collect())
            .unwrap_or_default()
    }

    /// In-memory task counts by status (process-local; not cross-replica).
    pub fn status_counts(&self) -> (usize, usize, usize, usize, usize, usize, usize) {
        status_counts_from_iter(self.tasks.values().map(|t| &t.status))
    }

    /// 清理过期任务
    ///
    /// - 已完成/失败超过24小时的任务
    /// - WaitingForInput 超过2小时未响应的任务（标记为超时失败）
    pub async fn cleanup_expired(&mut self) {
        let now = Utc::now();
        let mut expired_ids: Vec<String> = Vec::new();
        let mut timed_out: Vec<(i32, TaskState)> = Vec::new();

        for (id, task) in &mut self.tasks {
            // 已完成的任务：终态保留窗口后清理
            if let Some(completed_at) = &task.completed_at {
                if is_terminal_past_retention(*completed_at, now) {
                    expired_ids.push(id.clone());
                }
            }
            // WaitingForInput 任务：超时未响应则标记为失败终态（本轮不删除）
            else if task.status == TaskStatus::WaitingForInput
                && is_waiting_input_timed_out(task.started_at, now)
            {
                let age_hours = (now - task.started_at).num_hours();
                tracing::info!(
                    task_id = %id,
                    age_hours = age_hours,
                    "[TaskStore] Expiring abandoned WaitingForInput task"
                );
                task.status = TaskStatus::Failed;
                task.error = Some(WAITING_INPUT_TIMEOUT_ERROR.to_string());
                task.completed_at = Some(now);
                if let Some(user_id) = self.user_tasks.iter().find_map(|(user_id, ids)| {
                    ids.iter().any(|task_id| task_id == id).then_some(*user_id)
                }) {
                    timed_out.push((user_id, task.clone()));
                }
            }
        }

        // 超时是一个可查询的失败终态，不应在同一轮清理中立即删除。
        // 先持久化，后续按上面的统一 24 小时终态保留策略删除。
        for (user_id, task) in timed_out {
            if let Err(error) = save_task_to_db(user_id, &task).await {
                tracing::warn!(
                    task_id = %task.task_id,
                    %error,
                    "持久化等待输入超时状态失败"
                );
            }
        }

        for id in &expired_ids {
            self.tasks.remove(id);
            for task_list in self.user_tasks.values_mut() {
                task_list.retain(|tid| tid != id);
            }
        }

        // 同步清理数据库中的过期任务
        if !expired_ids.is_empty() {
            if let Err(e) = cleanup_expired_tasks_from_db(&expired_ids).await {
                tracing::warn!("清理数据库过期任务失败: {}", e);
            }
        }
    }
}

impl Default for TaskStore {
    fn default() -> Self {
        Self::new()
    }
}

// 数据库操作

/// 初始化任务存储的数据库连接
pub async fn init_task_store_db(db: DatabaseConnection) {
    {
        let mut db_guard = DB_FOR_TASKS.write().await;
        *db_guard = Some(db.clone());
    }

    // 从数据库加载未完成的任务
    if let Err(e) = load_pending_tasks_from_db(&db).await {
        tracing::warn!("加载待处理任务失败: {}", e);
    }
}

/// 从数据库加载未完成的任务
async fn load_pending_tasks_from_db(db: &DatabaseConnection) -> Result<(), String> {
    // pending/running 没有可安全恢复的执行 continuation。先在权威数据库中
    // 原子终结，避免每次重启都把同一任务再次识别为“被中断”。
    let interrupted = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE agent_tasks
SET status = 'cancelled',
    error = $1,
    completed_at = COALESCE(completed_at, NOW()),
    updated_at = NOW()
WHERE status IN ('pending', 'running')
"#,
            vec![crate::services::agent::response_agent::task_interrupted().into()],
        ))
        .await
        .map_err(|error| {
            tracing::error!("Failed to finalize interrupted tasks: {error}");
            "Failed to finalize interrupted tasks".to_string()
        })?
        .rows_affected();

    let pending_tasks = agent_tasks::Entity::find()
        .filter(agent_tasks::Column::Status.eq("waiting_for_input"))
        .order_by_desc(agent_tasks::Column::StartedAt)
        .all(db)
        .await
        .map_err(|e| {
            tracing::error!("Failed to list pending tasks: {e}");
            "Failed to list pending tasks".to_string()
        })?;

    let mut store = TASK_STORE.write().await;
    for task_model in pending_tasks {
        if let Ok(task_state) = task_model_to_state(&task_model) {
            // WaitingForInput has a persisted recipe/context/question and can
            // be resumed by any replica when its interaction completes.
            let user_id = task_model.user_id;
            let task_id = task_state.task_id.clone();
            store.tasks.insert(task_id.clone(), task_state);
            store.user_tasks.entry(user_id).or_default().push(task_id);
        }
    }

    if interrupted > 0 {
        tracing::info!(
            interrupted = interrupted,
            waiting_restored = store.tasks.len(),
            "[TaskStore] Boot: cancelled in-flight pending/running; restored waiting_for_input tasks (answer path works; original wait-loop must re-register on next answer)"
        );
    } else {
        tracing::info!(
            waiting_restored = store.tasks.len(),
            "[TaskStore] Boot: restored waiting_for_input tasks from database"
        );
    }
    Ok(())
}

/// Statuses that interrupt/cancel should target (public for API alignment tests).
pub use task_store_pure::is_cancellable_task_status;

/// Snapshot of waiting tasks currently in memory (after boot load).
/// Used to re-create run hubs + WAITING_TASKS loops.
pub async fn list_waiting_tasks_snapshot() -> Vec<(i32, TaskState)> {
    let store = TASK_STORE.read().await;
    let mut out = Vec::new();
    for (user_id, ids) in &store.user_tasks {
        for id in ids {
            if let Some(task) = store.tasks.get(id) {
                if task.status == TaskStatus::WaitingForInput {
                    out.push((*user_id, task.clone()));
                }
            }
        }
    }
    out
}

/// 将数据库模型转换为任务状态
fn task_model_to_state(model: &agent_tasks::Model) -> Result<TaskState, String> {
    let status = task_status_from_db_str(&model.status);

    let step_results: HashMap<String, StepResult> =
        serde_json::from_value(model.step_results.clone()).unwrap_or_default();

    let mut pending_question: Option<UserQuestion> = model
        .pending_question
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    if let Some(ref mut q) = pending_question {
        q.ensure_expires_at();
    }

    let execution_context: Option<ExecutionContext> = model
        .execution_context
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let recipe: Option<Recipe> = model
        .recipe
        .as_ref()
        .and_then(|value| serde_json::from_value(value.clone()).ok());

    // Prefer stored lane_id; fall back to reconstructing from session_id for older rows.
    let lane_id = model.lane_id.clone().or_else(|| {
        model
            .session_id
            .as_ref()
            .and_then(|sid| lane_id_from_user_session(model.user_id, sid))
    });

    Ok(TaskState {
        task_id: model.id.clone(),
        recipe_id: model.recipe_id.clone(),
        status,
        current_step: model.current_step as usize,
        step_results,
        started_at: model.started_at.into(),
        completed_at: model.completed_at.map(|t| t.into()),
        error: model.error.clone(),
        progress: model.progress as u8,
        pending_question,
        execution_context,
        lane_id,
        execution_trace: None,
        recipe,
    })
}

/// Session id embedded in `user:{id}:session:{session_id}` lane keys.
pub use task_store_pure::session_id_from_lane_id;

/// 保存任务到数据库
pub async fn save_task_to_db(user_id: i32, task: &TaskState) -> Result<(), String> {
    let db_guard = DB_FOR_TASKS.read().await;
    let db = db_guard.as_ref().ok_or("Database is not connected")?;

    let status_str = task_status_to_db_str(&task.status);

    // 检查任务是否已存在（使用 id 字段，它存储的是 task_id）
    let existing = agent_tasks::Entity::find_by_id(&task.task_id)
        .one(db)
        .await
        .map_err(|e| {
            tracing::error!("Failed to load task: {e}");
            "Failed to load task".to_string()
        })?;

    if let Some(existing_task) = existing {
        // 更新现有任务
        let mut active_model: agent_tasks::ActiveModel = existing_task.into();
        active_model.status = Set(status_str.to_string());
        active_model.current_step = Set(task.current_step as i32);
        active_model.step_results = Set(json!(task.step_results));
        active_model.completed_at = Set(task.completed_at.map(|t| t.into()));
        active_model.error = Set(task.error.clone());
        active_model.progress = Set(task.progress as i16);
        active_model.pending_question = Set(task.pending_question.as_ref().map(|q| json!(q)));
        active_model.execution_context = Set(task.execution_context.as_ref().map(|c| json!(c)));
        active_model.recipe = Set(task.recipe.as_ref().map(|recipe| json!(recipe)));
        active_model.lane_id = Set(task.lane_id.clone());
        if let Some(sid) = session_id_from_lane_id(task.lane_id.as_deref()) {
            active_model.session_id = Set(Some(sid));
        }

        active_model.update(db).await.map_err(|e| {
            tracing::error!("Failed to update task: {e}");
            "Failed to update task".to_string()
        })?;
    } else {
        // 创建新任务
        let session_id = session_id_from_lane_id(task.lane_id.as_deref());
        let new_task = agent_tasks::ActiveModel {
            id: Set(task.task_id.clone()),
            user_id: Set(user_id),
            recipe_id: Set(task.recipe_id.clone()),
            status: Set(status_str.to_string()),
            current_step: Set(task.current_step as i32),
            step_results: Set(json!(task.step_results)),
            started_at: Set(task.started_at.into()),
            completed_at: Set(task.completed_at.map(|t| t.into())),
            error: Set(task.error.clone()),
            progress: Set(task.progress as i16),
            pending_question: Set(task.pending_question.as_ref().map(|q| json!(q))),
            execution_context: Set(task.execution_context.as_ref().map(|c| json!(c))),
            recipe: Set(task.recipe.as_ref().map(|recipe| json!(recipe))),
            original_request: Set(None),
            updated_at: Set(chrono::Utc::now().into()),
            session_id: Set(session_id),
            lane_id: Set(task.lane_id.clone()),
            name: Set(None),
            total_steps: Set(Some(
                task.recipe
                    .as_ref()
                    .map(|r| r.steps.len())
                    .unwrap_or(task.step_results.len())
                    .max(1) as i32,
            )),
        };

        new_task.insert(db).await.map_err(|e| {
            tracing::error!("Failed to create task: {e}");
            "Failed to create task".to_string()
        })?;
    }

    Ok(())
}

/// 从数据库清理过期任务
async fn cleanup_expired_tasks_from_db(task_ids: &[String]) -> Result<(), String> {
    let db_guard = DB_FOR_TASKS.read().await;
    let db = db_guard.as_ref().ok_or("Database is not connected")?;

    for task_id in task_ids {
        agent_tasks::Entity::delete_by_id(task_id)
            .exec(db)
            .await
            .map_err(|e| {
                tracing::error!("Failed to delete task: {e}");
                "Failed to delete task".to_string()
            })?;
    }

    Ok(())
}

/// 异步持久化任务（fire-and-forget）
pub fn persist_task_async(user_id: i32, task: TaskState) {
    tokio::spawn(async move {
        if let Err(e) = save_task_to_db(user_id, &task).await {
            tracing::warn!("异步保存任务失败: {}", e);
        }
    });
}

// 公共 API

/// 获取任务状态（带所有权校验）
///
/// 仅当任务属于指定用户时才返回，防止 IDOR
pub async fn get_task_for_user(task_id: &str, user_id: i32) -> Option<TaskState> {
    let local = {
        let store = TASK_STORE.read().await;
        let user_owns_task = store
            .user_tasks
            .get(&user_id)
            .is_some_and(|ids| ids.iter().any(|id| id == task_id));
        if user_owns_task {
            store.get(task_id).cloned()
        } else {
            None
        }
    };
    if local.as_ref().is_some_and(|task| {
        matches!(
            task.status,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
        )
    }) {
        return local;
    }
    // Non-terminal local entries may have been resumed or cancelled by a
    // different replica. Prefer the database, falling back only if it is
    // temporarily unavailable.
    refresh_task_for_user(task_id, user_id).await.or(local)
}

/// Force a database refresh even when this replica has an older local copy.
/// Used by long-lived run hubs that must observe completion on another replica.
pub async fn refresh_task_for_user(task_id: &str, user_id: i32) -> Option<TaskState> {
    let db = DB_FOR_TASKS.read().await.clone()?;
    let model = agent_tasks::Entity::find_by_id(task_id)
        .filter(agent_tasks::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .ok()??;
    let task = task_model_to_state(&model).ok()?;
    let mut store = TASK_STORE.write().await;
    store.tasks.insert(task_id.to_string(), task.clone());
    let ids = store.user_tasks.entry(user_id).or_default();
    if !ids.iter().any(|id| id == task_id) {
        ids.push(task_id.to_string());
    }
    Some(task)
}

/// Claim one persisted waiting task for resume. The database transition is the
/// cross-replica mutex; local TASK_STORE locks alone cannot prevent two
/// backends from executing the same continuation.
pub async fn claim_task_for_resume(task_id: &str, user_id: i32) -> Result<bool, String> {
    let db = DB_FOR_TASKS
        .read()
        .await
        .clone()
        .ok_or("Database is not connected")?;
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE agent_tasks
SET status = 'running', updated_at = NOW()
WHERE id = $1 AND user_id = $2 AND status = 'waiting_for_input'
"#,
            vec![task_id.to_string().into(), user_id.into()],
        ))
        .await
        .map_err(|error| {
            tracing::error!("Failed to resume task: {error}");
            "Failed to resume task".to_string()
        })?;
    Ok(result.rows_affected() == 1)
}

/// 取消任务（带所有权校验）
///
/// 仅当任务属于指定用户时才取消，返回 true 表示已请求取消
pub async fn cancel_task_for_user(task_id: &str, user_id: i32) -> bool {
    let Some(db) = DB_FOR_TASKS.read().await.clone() else {
        return false;
    };
    let cancellation_error = crate::services::agent::response_agent::task_cancelled_by_user();
    let cancelled = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE agent_tasks
SET status = 'cancelled',
    error = $3,
    completed_at = COALESCE(completed_at, NOW()),
    updated_at = NOW()
WHERE id = $1 AND user_id = $2
  AND status IN ('pending', 'running', 'waiting_for_input', 'paused')
"#,
            vec![
                task_id.to_string().into(),
                user_id.into(),
                cancellation_error.clone().into(),
            ],
        ))
        .await
        .is_ok_and(|result| result.rows_affected() == 1);
    if !cancelled {
        return false;
    }

    mark_cancelled_in_memory(task_id, &cancellation_error).await;

    tracing::info!(task_id = %task_id, user_id = user_id, "[TaskStore] Task cancelled by user");
    true
}

/// 系统路径请求协作式取消（Heartbeat 超时等），不校验 user_id。
///
/// 写入取消标记 + DB 状态，执行器在步骤边界通过 [`is_cancelled`] 退出。
pub async fn request_cancel(task_id: &str, reason: &str) -> bool {
    {
        let mut tokens = CANCELLATION_TOKENS.write().await;
        tokens.insert(task_id.to_string());
    }

    let Some(db) = DB_FOR_TASKS.read().await.clone() else {
        // 无 DB 时仍保留内存标记，执行器可协作退出
        mark_cancelled_in_memory(task_id, reason).await;
        return true;
    };

    let cancelled = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE agent_tasks
SET status = 'cancelled',
    error = $2,
    completed_at = COALESCE(completed_at, NOW()),
    updated_at = NOW()
WHERE id = $1
  AND status IN ('pending', 'running', 'waiting_for_input', 'paused')
"#,
            vec![task_id.to_string().into(), reason.to_string().into()],
        ))
        .await
        .is_ok_and(|result| result.rows_affected() == 1);

    mark_cancelled_in_memory(task_id, reason).await;
    if cancelled {
        tracing::info!(task_id = %task_id, reason = %reason, "[TaskStore] Task cancel requested (system)");
    } else {
        tracing::debug!(
            task_id = %task_id,
            reason = %reason,
            "[TaskStore] request_cancel: no active row (already terminal or missing)"
        );
    }
    true
}

async fn mark_cancelled_in_memory(task_id: &str, reason: &str) {
    {
        let mut tokens = CANCELLATION_TOKENS.write().await;
        tokens.insert(task_id.to_string());
    }
    let mut store = TASK_STORE.write().await;
    if let Some(task) = store.get_mut(task_id) {
        task.status = TaskStatus::Cancelled;
        task.error = Some(reason.to_string());
        task.completed_at = Some(Utc::now());
    }
}

/// 获取用户的任务列表（内存 + 数据库合并）。
///
/// 内存中的非终态任务优先（更新鲜）；数据库补充重启后仅落库的完成/失败/取消任务，
/// 避免 list_tasks 在进程重启后「空列表」造成前端无法恢复。
pub async fn get_user_tasks(user_id: i32) -> Vec<TaskState> {
    let mut by_id: HashMap<String, TaskState> = HashMap::new();

    {
        let store = TASK_STORE.read().await;
        for task in store.get_user_tasks(user_id) {
            by_id.insert(task.task_id.clone(), task.clone());
        }
    }

    if let Some(db) = DB_FOR_TASKS.read().await.clone() {
        match agent_tasks::Entity::find()
            .filter(agent_tasks::Column::UserId.eq(user_id))
            .order_by_desc(agent_tasks::Column::StartedAt)
            .limit(100)
            .all(&db)
            .await
        {
            Ok(models) => {
                for model in models {
                    if let Ok(task) = task_model_to_state(&model) {
                        // 已有内存副本则保留（活跃执行路径）
                        by_id.entry(task.task_id.clone()).or_insert(task);
                    }
                }
            }
            Err(error) => {
                tracing::warn!(
                    user_id = user_id,
                    %error,
                    "[TaskStore] Failed to list tasks from database; returning memory only"
                );
            }
        }
    }

    let mut tasks: Vec<TaskState> = by_id.into_values().collect();
    tasks.sort_by_key(|b| Reverse(b.started_at));
    tasks
}

/// 以约 5% 的概率触发一次过期任务清理（请求驱动，避免独立定时任务）
pub async fn maybe_cleanup_tasks() {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    // 每 20 次请求清理一次过期任务
    if n.is_multiple_of(20) {
        let mut store = TASK_STORE.write().await;
        store.cleanup_expired().await;
    }
    // 每 100 次请求清理一次空闲 Lane（防止 HashMap 无限增长）
    if n.is_multiple_of(100) {
        crate::services::agent::LANE_QUEUE
            .cleanup_idle_lanes()
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellable_statuses_reexport_matches_domain() {
        // Adapter re-exports domain rule; keep smoke coverage on the public path.
        assert!(is_cancellable_task_status(&TaskStatus::WaitingForInput));
        assert!(!is_cancellable_task_status(&TaskStatus::Completed));
    }

    #[test]
    fn test_task_store() {
        let mut store = TaskStore::new();
        let task = TaskState {
            task_id: "test_task".to_string(),
            recipe_id: "test_recipe".to_string(),
            status: TaskStatus::Pending,
            current_step: 0,
            step_results: HashMap::new(),
            started_at: Utc::now(),
            completed_at: None,
            error: None,
            progress: 0,
            pending_question: None,
            execution_context: None,
            lane_id: None,
            execution_trace: None,
            recipe: None,
        };

        // 直接插入，不触发异步数据库保存
        store.tasks.insert(task.task_id.clone(), task.clone());
        store
            .user_tasks
            .entry(1)
            .or_default()
            .push(task.task_id.clone());

        assert!(store.get("test_task").is_some());
        assert_eq!(store.get_user_tasks(1).len(), 1);
    }

    #[test]
    fn task_model_restores_recipe_for_cross_replica_resume() {
        let recipe = Recipe::new("resume interaction", "open a Tapp", ExecutionType::Instant);
        let now = Utc::now().fixed_offset();
        let model = agent_tasks::Model {
            id: "task-with-recipe".to_string(),
            user_id: 7,
            recipe_id: recipe.id.clone(),
            name: None,
            status: "waiting_for_input".to_string(),
            current_step: 1,
            total_steps: Some(1),
            step_results: json!({}),
            execution_context: None,
            recipe: Some(json!(recipe)),
            pending_question: None,
            progress: 50,
            error: None,
            original_request: None,
            session_id: None,
            lane_id: None,
            started_at: now,
            completed_at: None,
            updated_at: now,
        };

        let restored = task_model_to_state(&model).expect("task model should deserialize");
        let restored_recipe = restored.recipe.expect("recipe should be restored");

        assert_eq!(restored.status, TaskStatus::WaitingForInput);
        assert_eq!(restored_recipe.id, model.recipe_id);
        assert_eq!(restored_recipe.execution_type, ExecutionType::Instant);
    }

    #[tokio::test]
    async fn waiting_timeout_is_retained_as_a_failed_terminal_task() {
        let mut store = TaskStore::new();
        let task = TaskState {
            task_id: "timed-out-wait".to_string(),
            recipe_id: "recipe".to_string(),
            status: TaskStatus::WaitingForInput,
            current_step: 0,
            step_results: HashMap::new(),
            started_at: Utc::now() - chrono::Duration::hours(3),
            completed_at: None,
            error: None,
            progress: 25,
            pending_question: None,
            execution_context: None,
            lane_id: None,
            execution_trace: None,
            recipe: None,
        };
        store.tasks.insert(task.task_id.clone(), task);
        store
            .user_tasks
            .insert(7, vec!["timed-out-wait".to_string()]);

        store.cleanup_expired().await;

        let retained = store
            .get("timed-out-wait")
            .expect("timeout should remain queryable during terminal retention");
        assert_eq!(retained.status, TaskStatus::Failed);
        assert!(retained.completed_at.is_some());
        assert!(retained
            .error
            .as_deref()
            .is_some_and(|value| value.contains("超时")));
        assert_eq!(store.get_user_tasks(7).len(), 1);
    }
}
