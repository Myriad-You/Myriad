//! 任务存储模块
//!
//! 管理任务状态的内存存储和数据库持久化

use crate::models::entities::agent_tasks;
use crate::services::agent::task_store_pure::{
    self, is_terminal_past_retention, is_waiting_input_timed_out, lane_id_from_user_session,
    status_counts_from_iter, task_status_from_db_str, task_status_to_db_str,
    waiting_input_timeout_error,
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
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

/// 全局任务状态存储
pub static TASK_STORE: Lazy<Arc<RwLock<TaskStore>>> =
    Lazy::new(|| Arc::new(RwLock::new(TaskStore::new())));

/// 全局数据库连接（用于任务持久化）
static DB_FOR_TASKS: Lazy<Arc<RwLock<Option<DatabaseConnection>>>> =
    Lazy::new(|| Arc::new(RwLock::new(None)));

/// 全局任务取消标记存储
pub static CANCELLATION_TOKENS: Lazy<Mutex<HashSet<String>>> =
    Lazy::new(|| Mutex::new(HashSet::new()));
static ACTIVE_TASK_EXECUTIONS: Lazy<Mutex<HashMap<String, usize>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Own the process-local cancellation marker only while execution is alive.
/// Drop runs for errors and aborted futures as well as normal completion.
pub(crate) struct CancellationGuard(String);
impl CancellationGuard {
    pub(crate) fn new(task_id: &str) -> Self {
        *ACTIVE_TASK_EXECUTIONS
            .lock()
            .unwrap()
            .entry(task_id.to_string())
            .or_default() += 1;
        Self(task_id.to_string())
    }
}
impl Drop for CancellationGuard {
    fn drop(&mut self) {
        let mut active = ACTIVE_TASK_EXECUTIONS.lock().unwrap();
        if let Some(count) = active.get_mut(&self.0) {
            *count -= 1;
            if *count == 0 {
                active.remove(&self.0);
                CANCELLATION_TOKENS.lock().unwrap().remove(&self.0);
            }
        }
    }
}

const TERMINAL_CACHE_LIMIT: usize = 256;

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
    .map_err(|error| {
        tracing::error!(%error, "failed to persist steering instruction");
        "Failed to persist steering instruction".to_string()
    })?;
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
    if CANCELLATION_TOKENS.lock().unwrap().contains(task_id) {
        return true;
    }
    if TASK_STORE
        .read()
        .await
        .get(task_id)
        .is_some_and(|task| task.status == TaskStatus::Cancelled)
    {
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
    let mut tokens = CANCELLATION_TOKENS.lock().unwrap();
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
    /// Work checkpoints are committed synchronously; never enqueue an older
    /// asynchronous snapshot that could overwrite a resumed or cancelled run.
    pub(crate) fn cache_committed(&mut self, user_id: i32, task: TaskState) {
        let ids = self.user_tasks.entry(user_id).or_default();
        if !ids.contains(&task.task_id) {
            ids.push(task.task_id.clone());
        }
        self.tasks.insert(task.task_id.clone(), task);
        self.trim_terminal_cache();
    }
    pub fn new() -> Self {
        Self {
            tasks: HashMap::new(),
            user_tasks: HashMap::new(),
        }
    }

    /// 存储任务（同时异步保存到数据库）
    pub fn store(&mut self, user_id: i32, task: TaskState) {
        let persist_at = next_recipe_persist_at();
        let task_for_db = task.clone();
        self.cache_committed(user_id, task);
        tokio::spawn(async move {
            if let Err(e) = save_task_to_db_at(user_id, &task_for_db, persist_at).await {
                tracing::warn!("保存任务到数据库失败: {}", e);
            }
        });
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

    fn remove_cached(&mut self, ids: &HashSet<String>) {
        self.tasks.retain(|id, _| !ids.contains(id));
        self.user_tasks.retain(|_, tasks| {
            tasks.retain(|id| !ids.contains(id));
            !tasks.is_empty()
        });
    }

    fn trim_terminal_cache(&mut self) {
        let mut terminal = self
            .tasks
            .values()
            .filter(|task| {
                matches!(
                    task.status,
                    TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
                )
            })
            .map(|task| {
                (
                    task.completed_at.unwrap_or(task.started_at),
                    task.task_id.clone(),
                )
            })
            .collect::<Vec<_>>();
        if terminal.len() > TERMINAL_CACHE_LIMIT {
            terminal.sort_unstable();
            let remove = terminal.len() - TERMINAL_CACHE_LIMIT;
            self.remove_cached(
                &terminal
                    .into_iter()
                    .take(remove)
                    .map(|(_, id)| id)
                    .collect(),
            );
        }
    }

    /// Reclaim only cached terminal snapshots; durable history stays in the DB.
    /// Live and waiting Work is never evicted by age or capacity pressure.
    fn reclaim_expired(&mut self) -> Vec<(i32, TaskState)> {
        let now = Utc::now();
        let mut expired = HashSet::new();
        let mut timed_out = Vec::new();
        for (id, task) in &mut self.tasks {
            if matches!(
                task.status,
                TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
            ) {
                if is_terminal_past_retention(task.completed_at.unwrap_or(task.started_at), now) {
                    expired.insert(id.clone());
                }
            } else if task.status == TaskStatus::WaitingForInput
                && !task
                    .recipe
                    .as_ref()
                    .is_some_and(crate::services::agent::work_loop::is_work_recipe)
                && is_waiting_input_timed_out(task.started_at, now)
            {
                task.status = TaskStatus::Failed;
                task.error = Some(waiting_input_timeout_error());
                task.completed_at = Some(now);
                if let Some(user_id) = self.user_tasks.iter().find_map(|(user_id, ids)| {
                    ids.iter().any(|task_id| task_id == id).then_some(*user_id)
                }) {
                    timed_out.push((user_id, task.clone()));
                }
            }
        }
        self.remove_cached(&expired);
        self.trim_terminal_cache();
        timed_out
    }

    #[cfg(test)]
    async fn cleanup_expired(&mut self) {
        for (user_id, task) in self.reclaim_expired() {
            let _ = save_task_to_db(user_id, &task).await;
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

    // Boot：pending/running 在库中原子标 cancelled；只把 waiting_for_input 载入内存。
    if let Err(e) = load_pending_tasks_from_db(&db).await {
        tracing::warn!("加载待处理任务失败: {}", e);
    }
}

/// Boot：pending/running 在库中原子标 cancelled；只把 waiting_for_input 载入内存。
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
  AND COALESCE(recipe->'metadata'->>'work_loop_version', '') != '1'
"#,
            vec![crate::services::agent::response_agent::task_interrupted().into()],
        ))
        .await
        .map_err(|error| {
            tracing::error!("Failed to finalize interrupted tasks: {error}");
            "Failed to finalize interrupted tasks".to_string()
        })?
        .rows_affected();

    crate::services::agent::work_loop::recover(db).await?;
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
    let status = task_status_from_db_str(&model.status)?;

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

    // 优先用存着的 `lane_id`；没有则从 `session_id` 重建。
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
    save_task_to_db_at(user_id, task, next_recipe_persist_at()).await
}

async fn save_task_to_db_at(
    user_id: i32,
    task: &TaskState,
    persist_at: chrono::DateTime<Utc>,
) -> Result<(), String> {
    if task
        .recipe
        .as_ref()
        .is_some_and(crate::services::agent::work_loop::is_work_recipe)
    {
        return Err("Work tasks must be saved through their revision-checked checkpoint".into());
    }
    let db_guard = DB_FOR_TASKS.read().await;
    let db = db_guard.as_ref().ok_or("Database is not connected")?;
    save_task_on_at(db, user_id, task, persist_at).await
}

pub(crate) async fn save_task_on(
    db: &impl ConnectionTrait,
    user_id: i32,
    task: &TaskState,
) -> Result<(), String> {
    save_task_on_at(db, user_id, task, next_recipe_persist_at()).await
}

fn next_recipe_persist_at() -> chrono::DateTime<Utc> {
    static LAST: Mutex<Option<chrono::DateTime<Utc>>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap();
    let now = Utc::now();
    let next = match *last {
        Some(prev) if prev >= now => prev + chrono::Duration::milliseconds(1),
        _ => now,
    };
    *last = Some(next);
    next
}

/// True when an older snapshot must not overwrite a newer durable row.
pub(crate) fn recipe_persist_is_stale(
    stored_updated_at: chrono::DateTime<Utc>,
    snapshot_at: chrono::DateTime<Utc>,
) -> bool {
    stored_updated_at >= snapshot_at
}

pub(crate) async fn save_task_on_at(
    db: &impl ConnectionTrait,
    user_id: i32,
    task: &TaskState,
    persist_at: chrono::DateTime<Utc>,
) -> Result<(), String> {
    let status_str = task_status_to_db_str(&task.status);
    let persist_at: chrono::DateTime<chrono::FixedOffset> = persist_at.into();

    // 检查任务是否已存在（使用 id 字段，它存储的是 task_id）
    let existing = agent_tasks::Entity::find_by_id(&task.task_id)
        .one(db)
        .await
        .map_err(|e| {
            tracing::error!("Failed to load task: {e}");
            "Failed to load task".to_string()
        })?;

    if let Some(existing_task) = existing {
        if recipe_persist_is_stale(existing_task.updated_at.with_timezone(&Utc), persist_at.with_timezone(&Utc))
        {
            return Ok(());
        }
        // 更新现有任务
        let mut active_model: agent_tasks::ActiveModel = existing_task.into();
        active_model.status = Set(status_str.to_string());
        active_model.updated_at = Set(persist_at);
        active_model.current_step = Set(task.current_step as i32);
        active_model.total_steps = Set(Some(
            task.recipe
                .as_ref()
                .map(|recipe| recipe.steps.len())
                .unwrap_or(task.step_results.len())
                .max(1) as i32,
        ));
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

        let updated = agent_tasks::Entity::update_many()
            .set(active_model)
            .filter(agent_tasks::Column::Id.eq(&task.task_id))
            .filter(agent_tasks::Column::UpdatedAt.lt(persist_at))
            .exec(db)
            .await
            .map_err(|e| {
                tracing::error!("Failed to update task: {e}");
                "Failed to update task".to_string()
            })?;
        if updated.rows_affected == 0 {
            return Ok(());
        }
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
            updated_at: Set(persist_at),
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

        if let Err(e) = new_task.insert(db).await {
            let lower = e.to_string().to_ascii_lowercase();
            if lower.contains("23505") || lower.contains("duplicate key") {
                return Box::pin(save_task_on_at(
                    db,
                    user_id,
                    task,
                    persist_at.with_timezone(&Utc),
                ))
                .await;
            }
            tracing::error!("Failed to create task: {e}");
            return Err("Failed to create task".to_string());
        }
    }

    Ok(())
}

/// 异步持久化任务（fire-and-forget）
pub fn persist_task_async(user_id: i32, task: TaskState) {
    let persist_at = next_recipe_persist_at();
    tokio::spawn(async move {
        if let Err(e) = save_task_to_db_at(user_id, &task, persist_at).await {
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
    store.cache_committed(user_id, task.clone());
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
    mark_cancelled_in_memory(task_id, reason).await;
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
    let mut store = TASK_STORE.write().await;
    if let Some(task) = store.get_mut(task_id) {
        task.status = TaskStatus::Cancelled;
        task.error = Some(reason.to_string());
        task.completed_at = Some(Utc::now());
    }
    // Serialize with owner Drop so cancellation cannot recreate an orphan
    // marker after its worker has exited. Durable rows cover remote/waiting work.
    let active = ACTIVE_TASK_EXECUTIONS.lock().unwrap();
    if active.contains_key(task_id) {
        CANCELLATION_TOKENS
            .lock()
            .unwrap()
            .insert(task_id.to_string());
    }
}

/// 获取用户的任务列表（内存 + 数据库合并）。
///
/// 内存已有的 id 原样保留；数据库按 started_at 降序最多并入 100 条缺失任务。
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
                        if task
                            .recipe
                            .as_ref()
                            .is_some_and(crate::services::agent::work_loop::is_work_recipe)
                        {
                            // Work checkpoints are committed before caching; the
                            // database also observes other replicas' progress.
                            by_id.insert(task.task_id.clone(), task);
                        } else {
                            by_id.entry(task.task_id.clone()).or_insert(task);
                        }
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

/// Preserve the existing 24-hour terminal-row retention independently of hot
/// cache eviction. Never delete live/waiting Work to reclaim process memory.
async fn cleanup_expired_tasks_from_db(db: &DatabaseConnection) -> Result<(), sea_orm::DbErr> {
    db.execute_raw(Statement::from_string(DatabaseBackend::Postgres,
        "DELETE FROM agent_tasks WHERE id IN (SELECT id FROM agent_tasks WHERE status IN ('completed', 'failed', 'cancelled') AND completed_at < NOW() - INTERVAL '24 hours' ORDER BY completed_at LIMIT 256)"
    )).await?;
    Ok(())
}

/// Called by the process-owned periodic sweep, independent of HTTP/SSE traffic.
/// Database I/O runs after releasing the task-store lock.
pub(crate) async fn cleanup_retained_state() {
    let timed_out = TASK_STORE.write().await.reclaim_expired();
    for (user_id, task) in timed_out {
        if let Err(error) = save_task_to_db(user_id, &task).await {
            tracing::warn!(task_id = %task.task_id, %error, "Failed to persist input timeout");
        }
    }
    crate::services::agent::run_hub::cleanup_retained_runs().await;
    if let Some(db) = DB_FOR_TASKS.read().await.clone() {
        if let Err(error) = cleanup_expired_tasks_from_db(&db).await {
            tracing::warn!(%error, "Failed to expire terminal task rows");
        }
    }
}

/// Retain request-driven opportunistic cleanup in addition to the idle sweep.
pub async fn maybe_cleanup_tasks() {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    if COUNTER.fetch_add(1, Ordering::Relaxed).is_multiple_of(20) {
        cleanup_retained_state().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires MYRIAD_MEMORY_TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
    async fn durable_retention_only_deletes_expired_terminal_rows_in_bounded_batches() {
        let url = std::env::var("MYRIAD_MEMORY_TEST_DATABASE_URL").expect("disposable test DB");
        let mut options = sea_orm::ConnectOptions::new(url);
        options.max_connections(1);
        let db = sea_orm::Database::connect(options).await.unwrap();
        db.execute_raw(Statement::from_string(DatabaseBackend::Postgres,
            "CREATE TEMP TABLE agent_tasks (id TEXT PRIMARY KEY, status TEXT, completed_at TIMESTAMPTZ)")).await.unwrap();
        db.execute_raw(Statement::from_string(DatabaseBackend::Postgres,
            "INSERT INTO agent_tasks SELECT n::text, 'completed', NOW() - INTERVAL '25 hours' FROM generate_series(1, 300) n")).await.unwrap();
        db.execute_raw(Statement::from_string(DatabaseBackend::Postgres,
            "INSERT INTO agent_tasks VALUES ('running', 'running', NOW() - INTERVAL '25 hours'), ('waiting', 'waiting_for_input', NULL), ('recent', 'completed', NOW())")).await.unwrap();
        cleanup_expired_tasks_from_db(&db).await.unwrap();
        let row = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT COUNT(*)::BIGINT AS count FROM agent_tasks",
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<i64>("", "count").unwrap(), 47);
        cleanup_expired_tasks_from_db(&db).await.unwrap();
        let row = db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres,
            "SELECT COUNT(*)::BIGINT AS count FROM agent_tasks WHERE id IN ('running', 'waiting', 'recent')")).await.unwrap().unwrap();
        assert_eq!(row.try_get::<i64>("", "count").unwrap(), 3);
    }

    #[tokio::test]
    async fn cancelled_execution_retains_marker_until_last_owner_exits() {
        let id = format!("owned-cancel-{}", uuid::Uuid::new_v4());
        let owner = CancellationGuard::new(&id);
        let second_owner = CancellationGuard::new(&id);
        mark_cancelled_in_memory(&id, "cancelled").await;
        assert!(is_cancelled(&id).await);
        drop(second_owner);
        assert!(CANCELLATION_TOKENS.lock().unwrap().contains(&id));
        drop(owner);
        assert!(!CANCELLATION_TOKENS.lock().unwrap().contains(&id));
        mark_cancelled_in_memory(&id, "late duplicate").await;
        assert!(!CANCELLATION_TOKENS.lock().unwrap().contains(&id));
    }

    #[tokio::test]
    async fn cancelling_inactive_or_missing_tasks_does_not_retain_markers() {
        let id = format!("inactive-cancel-{}", uuid::Uuid::new_v4());
        mark_cancelled_in_memory(&id, "cancelled").await;
        assert!(!CANCELLATION_TOKENS.lock().unwrap().contains(&id));
    }

    fn retained_task(id: &str, status: TaskStatus, hours_old: i64) -> TaskState {
        let mut task = TaskState::new(&Recipe::new(
            "retention",
            "retention",
            ExecutionType::Instant,
        ));
        task.task_id = id.into();
        task.status = status;
        task.started_at = Utc::now() - chrono::Duration::hours(hours_old);
        task.completed_at = Some(task.started_at);
        task
    }

    #[tokio::test]
    async fn cleanup_drops_empty_user_indexes_and_keeps_live_work() {
        let mut store = TaskStore::new();
        store.cache_committed(7001, retained_task("expired", TaskStatus::Completed, 25));
        store.cache_committed(7002, retained_task("running", TaskStatus::Running, 25));
        store.cleanup_expired().await;
        assert!(store.get("expired").is_none());
        assert!(!store.user_tasks.contains_key(&7001));
        assert!(
            store.get("running").is_some(),
            "stale timestamps must not evict active tasks"
        );
    }

    #[tokio::test]
    async fn repeated_task_cache_writes_do_not_duplicate_user_index() {
        let mut store = TaskStore::new();
        let task = retained_task("same", TaskStatus::Completed, 0);
        store.store(7003, task.clone());
        store.store(7003, task);
        assert_eq!(store.get_user_tasks(7003).len(), 1);
    }

    #[tokio::test]
    async fn cleanup_bounds_terminal_cache_but_keeps_active_and_waiting_tasks() {
        let mut store = TaskStore::new();
        for index in 0..2_000 {
            store.cache_committed(
                7004,
                retained_task(&format!("done-{index}"), TaskStatus::Completed, 0),
            );
        }
        let mut live = retained_task("live", TaskStatus::Running, 0);
        live.completed_at = None;
        store.cache_committed(7004, live);
        let mut waiting = retained_task("waiting", TaskStatus::WaitingForInput, 0);
        waiting.completed_at = None;
        store.cache_committed(7004, waiting);
        store.cleanup_expired().await;
        assert!(
            store.tasks.len() <= TERMINAL_CACHE_LIMIT + 2,
            "terminal hot cache must stay bounded"
        );
        assert!(store.get("live").is_some());
        assert!(store.get("waiting").is_some());
    }

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

    #[test]
    fn unknown_task_status_is_not_pending() {
        let now = Utc::now().fixed_offset();
        let model = agent_tasks::Model {
            id: "bad-status".to_string(),
            user_id: 1,
            recipe_id: "r".to_string(),
            name: None,
            status: "unknown_legacy".to_string(),
            current_step: 0,
            total_steps: Some(1),
            step_results: json!({}),
            execution_context: None,
            recipe: None,
            pending_question: None,
            progress: 0,
            error: None,
            original_request: None,
            session_id: None,
            lane_id: None,
            started_at: now,
            completed_at: None,
            updated_at: now,
        };
        let err = task_model_to_state(&model).expect_err("unknown status must fail closed");
        assert!(err.contains("unknown agent task status"));
    }

    #[test]
    fn older_recipe_snapshot_is_rejected() {
        let older = Utc::now();
        let newer = older + chrono::Duration::milliseconds(5);
        assert!(recipe_persist_is_stale(newer, older));
        assert!(!recipe_persist_is_stale(older, newer));
        let first = next_recipe_persist_at();
        let second = next_recipe_persist_at();
        assert!(second > first);
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
        assert!(
            retained
                .error
                .as_deref()
                .is_some_and(|value| value.contains("timed out"))
        );
        assert_eq!(store.get_user_tasks(7).len(), 1);
    }
}
