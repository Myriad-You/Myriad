//! Tapp 调度引擎服务
//!
//! 负责：
//! 1. 定时检查到期任务
//! 2. 执行后端可执行的任务
//! 3. 推送前端任务到 WebSocket 连接
//! 4. 管理任务执行历史

use chrono::{DateTime, Duration, NaiveTime, TimeZone, Utc};
use cron::Schedule;
use sea_orm::{
    sea_query::Expr, ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait,
    DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
    Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

use std::collections::HashMap;

use crate::api::tapp_runtime::shared_registry::{self, RegistryIdentity};
use crate::api::tapp_store::{
    read_storage_value, validate_sandbox_storage_key, validate_storage_value_size,
    write_storage_value, TappAiManifest, TappAiModelTier, TappAiOperation, TappAiOutputFormat,
};
use crate::config::ModelTier;
use crate::models::entities::tapp_scheduled_tasks::{
    self, BackendAction, BackendActionWrapper, ExecutionTarget, MissedPolicy, RetryConfig,
    ScheduleConfig, ScheduleType, TaskScope, TaskStats,
};
use crate::models::entities::tapp_task_executions::{self, ExecutionStatus};
use crate::services::permission_service::{TappPermission, TappPermissionService, UserRole};
use crate::services::platform_auto_refresh::{
    core_platform_from_task, is_core_platform_sync_task, CORE_PLATFORM_SYNC_TAPP_ID,
};
use crate::GLOBAL_DYNAMIC_CONFIG;

const MAX_SCHEDULER_FETCH_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_SCHEDULER_BACKEND_ACTIONS: usize = 8;
pub const MAX_SCHEDULER_RETRIES: i32 = 2;
pub const MAX_SCHEDULER_RETRY_DELAY_MS: i64 = 60_000;
const SCHEDULER_ACTION_BUDGET_MS: i64 = 130_000;
const MIN_SCHEDULER_LEASE_MINUTES: i64 = 15;
const MAX_SCHEDULER_LEASE_MINUTES: i64 = 360;
const SCHEDULER_PRESENCE_NAMESPACE: &str = "scheduler_ws_presence";
const SCHEDULER_MAILBOX_CHANNEL: &str = "scheduler_frontend";
const SCHEDULER_PRESENCE_TTL_SECONDS: i64 = 75;
const SCHEDULER_MESSAGE_TTL_SECONDS: i64 = 5 * 60;
const MAX_SCHEDULER_CONNECTIONS_PER_SUBJECT: usize = 8;
pub const SCHEDULER_PRESENCE_REFRESH_SECONDS: u64 = 20;
pub const SCHEDULER_MAILBOX_POLL_MILLIS: u64 = 500;
pub const SCHEDULER_MAILBOX_BATCH_SIZE: i64 = 32;

static SCHEDULER_DISPATCHED: AtomicU64 = AtomicU64::new(0);
static SCHEDULER_DELIVERY_FAILURES: AtomicU64 = AtomicU64::new(0);
static SCHEDULER_REQUEUED: AtomicU64 = AtomicU64::new(0);
static SCHEDULER_COMPLETED: AtomicU64 = AtomicU64::new(0);
static SCHEDULER_TIMEOUTS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerCounters {
    pub dispatched: u64,
    pub delivery_failures: u64,
    pub requeued: u64,
    pub completed: u64,
    pub timeouts: u64,
}

pub fn scheduler_counters() -> SchedulerCounters {
    SchedulerCounters {
        dispatched: SCHEDULER_DISPATCHED.load(Ordering::Relaxed),
        delivery_failures: SCHEDULER_DELIVERY_FAILURES.load(Ordering::Relaxed),
        requeued: SCHEDULER_REQUEUED.load(Ordering::Relaxed),
        completed: SCHEDULER_COMPLETED.load(Ordering::Relaxed),
        timeouts: SCHEDULER_TIMEOUTS.load(Ordering::Relaxed),
    }
}

/// Normalize the public SDK action shape (`type`) to the persisted Rust enum
/// tag (`action`) and validate every action before a task is stored.
pub fn normalize_backend_actions(
    actions: Option<serde_json::Value>,
) -> Result<Option<serde_json::Value>, String> {
    let Some(serde_json::Value::Array(actions)) = actions else {
        return match actions {
            None => Ok(None),
            Some(_) => Err("backendActions must be an array".to_string()),
        };
    };
    if actions.len() > MAX_SCHEDULER_BACKEND_ACTIONS {
        return Err(format!(
            "backendActions exceeds the maximum of {MAX_SCHEDULER_BACKEND_ACTIONS}"
        ));
    }

    let mut normalized = Vec::with_capacity(actions.len());
    for mut value in actions {
        let object = value
            .as_object_mut()
            .ok_or_else(|| "Each backend action must be an object".to_string())?;
        if !object.contains_key("action") {
            let action_type = object
                .remove("type")
                .ok_or_else(|| "Backend action requires type".to_string())?;
            object.insert("action".to_string(), action_type);
        }

        serde_json::from_value::<BackendActionWrapper>(value.clone())
            .map_err(|e| format!("Invalid backend action: {e}"))?;
        normalized.push(value);
    }

    Ok(Some(serde_json::Value::Array(normalized)))
}

/// Resolve the dynamic Tapp permissions needed by a validated backend action
/// pipeline. Registration and delayed execution both use this list.
pub fn backend_action_permissions(
    actions: &Option<serde_json::Value>,
) -> Result<Vec<TappPermission>, String> {
    let Some(serde_json::Value::Array(actions)) = actions else {
        return Ok(Vec::new());
    };

    let mut permissions = Vec::new();
    for value in actions {
        let wrapper: BackendActionWrapper = serde_json::from_value(value.clone())
            .map_err(|e| format!("Invalid backend action: {e}"))?;
        let permission = match wrapper.action {
            BackendAction::PlatformSync { .. } => Some(TappPermission::PlatformWrite),
            BackendAction::StorageSet { .. }
            | BackendAction::StorageDelete { .. }
            | BackendAction::StorageGet { .. } => Some(TappPermission::Storage),
            BackendAction::AiGenerate { .. } => Some(TappPermission::AiGenerate),
            BackendAction::Fetch { .. } => Some(TappPermission::NetworkFetch),
            BackendAction::NotificationQueue { .. } => Some(TappPermission::UiNotification),
            BackendAction::Transform { .. } => None,
        };
        if let Some(permission) = permission {
            permissions.push(permission);
        }
    }
    permissions.sort_by_key(|permission| permission.as_str());
    permissions.dedup();
    Ok(permissions)
}

/// Validate delayed backend actions against the installed Manifest contract.
/// Permissions alone are insufficient: AI execution must also be declared in
/// manifest.ai so every entry path uses the same V2 operation/model boundary.
pub fn validate_backend_action_declarations(
    manifest: &serde_json::Value,
    actions: &Option<serde_json::Value>,
) -> Result<Option<ModelTier>, String> {
    let Some(serde_json::Value::Array(actions)) = actions else {
        return Ok(None);
    };
    let uses_ai_generate = actions.iter().try_fold(false, |uses_ai, value| {
        let wrapper: BackendActionWrapper = serde_json::from_value(value.clone())
            .map_err(|error| format!("Invalid backend action: {error}"))?;
        Ok::<_, String>(uses_ai || matches!(wrapper.action, BackendAction::AiGenerate { .. }))
    })?;
    if !uses_ai_generate {
        return Ok(None);
    }

    let declaration: TappAiManifest = manifest
        .get("ai")
        .cloned()
        .ok_or_else(|| "ai.generate backend action requires manifest.ai".to_string())
        .and_then(|value| {
            serde_json::from_value(value)
                .map_err(|_| "Stored manifest.ai declaration is invalid".to_string())
        })?;
    if declaration.protocol_version != 2
        || !declaration.operations.contains(&TappAiOperation::Generate)
        || !declaration
            .output_formats
            .contains(&TappAiOutputFormat::Text)
    {
        return Err(
            "ai.generate backend action requires AI V2 generate operation and text output"
                .to_string(),
        );
    }
    Ok(Some(match declaration.model_tier {
        TappAiModelTier::Standard => ModelTier::Standard,
        TappAiModelTier::Pro => ModelTier::Pro,
    }))
}

#[derive(Clone, Copy)]
struct ScheduledExecutionAuthority {
    role: UserRole,
    owner_id: i32,
    ai_model_tier: Option<ModelTier>,
}

/// 任务执行上下文（发送给前端）
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskExecutionContext {
    pub id: i32,
    pub task_id: String,
    pub tapp_id: String,
    /// 注册任务的用户 ID
    pub user_id: i32,
    /// 任务作用域: user, tapp, global
    pub scope: String,
    pub scheduled_at: i64,
    pub executed_at: i64,
    pub is_compensation: bool,
    pub payload: Option<serde_json::Value>,
}

/// 前端任务推送消息
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendTaskMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub task: TaskExecutionContext,
    pub payload: Option<serde_json::Value>,
    pub scheduled_at: String,
    pub execution_id: i32,
    /// 目标用户列表（空表示广播给所有该 Tapp 的用户）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_users: Option<Vec<i32>>,
}

fn scheduler_mailbox_recipient(connection_id: &str) -> String {
    format!("connection:{connection_id}")
}

pub async fn register_frontend_connection(
    db: &DatabaseConnection,
    user_id: i32,
    connection_id: &str,
) -> Result<(), String> {
    let expires_at = Utc::now().timestamp() + SCHEDULER_PRESENCE_TTL_SECONDS;
    let inserted = shared_registry::put_with_subject_limit(
        db,
        SCHEDULER_PRESENCE_NAMESPACE,
        connection_id,
        RegistryIdentity {
            subject_id: Some(user_id),
            owner_id: None,
            tapp_id: None,
            runtime_id: Some(connection_id),
        },
        &json!({ "connectedAt": Utc::now().to_rfc3339() }),
        expires_at,
        MAX_SCHEDULER_CONNECTIONS_PER_SUBJECT,
    )
    .await
    .map_err(|error| format!("Failed to register scheduler connection: {error}"))?;
    if inserted {
        Ok(())
    } else {
        Err(format!(
            "Scheduler connection limit exceeded ({MAX_SCHEDULER_CONNECTIONS_PER_SUBJECT})"
        ))
    }
}

pub async fn unregister_frontend_connection(
    db: &DatabaseConnection,
    connection_id: &str,
) -> Result<(), String> {
    shared_registry::delete(db, SCHEDULER_PRESENCE_NAMESPACE, connection_id)
        .await
        .map(|_| ())
        .map_err(|error| format!("Failed to unregister scheduler connection: {error}"))
}

pub async fn drain_frontend_messages(
    db: &DatabaseConnection,
    connection_id: &str,
) -> Result<Vec<FrontendTaskMessage>, String> {
    shared_registry::drain(
        db,
        SCHEDULER_MAILBOX_CHANNEL,
        &scheduler_mailbox_recipient(connection_id),
        SCHEDULER_MAILBOX_BATCH_SIZE,
    )
    .await
    .map_err(|error| format!("Failed to drain scheduler mailbox: {error}"))
}

pub async fn requeue_frontend_message(
    db: &DatabaseConnection,
    connection_id: &str,
    message: &FrontendTaskMessage,
) -> Result<(), String> {
    shared_registry::enqueue(
        db,
        SCHEDULER_MAILBOX_CHANNEL,
        &scheduler_mailbox_recipient(connection_id),
        message,
        Utc::now().timestamp() + SCHEDULER_MESSAGE_TTL_SECONDS,
    )
    .await
    .map_err(|error| format!("Failed to requeue scheduler message: {error}"))?;
    SCHEDULER_REQUEUED.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

pub async fn active_frontend_subject_count(db: &DatabaseConnection) -> Result<usize, String> {
    shared_registry::list_subject_ids(db, SCHEDULER_PRESENCE_NAMESPACE)
        .await
        .map(|subjects| subjects.len())
        .map_err(|error| format!("Failed to count scheduler subjects: {error}"))
}

pub async fn scheduler_mailbox_depth(db: &DatabaseConnection) -> Result<i64, String> {
    shared_registry::mailbox_depth(db, SCHEDULER_MAILBOX_CHANNEL)
        .await
        .map_err(|error| format!("Failed to count scheduler mailbox: {error}"))
}

/// 调度引擎
pub struct TappSchedulerEngine {
    db: DatabaseConnection,
    /// 是否正在运行
    running: Arc<RwLock<bool>>,
}

impl TappSchedulerEngine {
    /// 创建调度引擎
    pub fn new(db: DatabaseConnection) -> Self {
        Self {
            db,
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// 启动调度引擎
    pub async fn start(&self) {
        let mut running = self.running.write().await;
        if *running {
            tracing::warn!("[TappScheduler] Already running");
            return;
        }
        *running = true;
        drop(running);

        tracing::info!("[TappScheduler] Starting scheduler engine");

        // 启动主调度循环
        let db = self.db.clone();
        let running = self.running.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));

            loop {
                interval.tick().await;

                // 检查是否停止
                if !*running.read().await {
                    tracing::info!("[TappScheduler] Scheduler stopped");
                    break;
                }

                // 执行调度
                if let Err(e) = Self::tick(&db).await {
                    tracing::error!("[TappScheduler] Tick error: {}", e);
                }
            }
        });
    }

    /// 停止调度引擎
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
        tracing::info!("[TappScheduler] Stopping scheduler engine");
    }

    /// 主调度循环 tick
    async fn tick(db: &DatabaseConnection) -> Result<(), String> {
        let now = Utc::now();
        tracing::debug!("[TappScheduler] Tick at {}", now);

        // 浏览器离线或 core 被销毁时可能收不到 task:complete；定期收敛悬挂执行，
        // 避免历史记录永久停在 running。
        Self::expire_stale_frontend_executions(db, now).await?;

        // 查找所有到期的启用任务
        let due_tasks = tapp_scheduled_tasks::Entity::find()
            .filter(tapp_scheduled_tasks::Column::Enabled.eq(true))
            .filter(tapp_scheduled_tasks::Column::NextRunAt.lte(now))
            .order_by_asc(tapp_scheduled_tasks::Column::NextRunAt)
            .all(db)
            .await
            .map_err(|e| format!("Failed to query due tasks: {}", e))?;

        tracing::debug!("[TappScheduler] Found {} due tasks", due_tasks.len());

        for task in due_tasks {
            let Some(task) = Self::claim_due_task(db, task, now).await? else {
                continue;
            };
            // 检查是否有错过的执行需要补偿
            let missed_count = Self::calculate_missed_executions(&task, now);

            if missed_count > 0 {
                tracing::info!(
                    "[TappScheduler] Task {} has {} missed executions, policy: {:?}",
                    task.task_id,
                    missed_count,
                    task.missed_policy
                );

                // 根据策略处理错过的执行
                match task.missed_policy {
                    MissedPolicy::Skip => {
                        // 跳过所有错过的执行，只更新统计
                        Self::update_missed_stats(db, &task, missed_count).await?;
                    }
                    MissedPolicy::RunOnce => {
                        // 只执行一次作为补偿
                        if let Err(e) = Self::execute_task_with_retry(db, &task, true).await {
                            tracing::error!(
                                "[TappScheduler] Compensation failed for {}: {}",
                                task.task_id,
                                e
                            );
                        }
                        if missed_count > 1 {
                            Self::update_missed_stats(db, &task, missed_count - 1).await?;
                        }
                    }
                    MissedPolicy::RunAll => {
                        // 补偿执行所有错过的（最多5次，避免过度补偿）
                        let max_compensations = missed_count.min(5);
                        for i in 0..max_compensations {
                            if let Err(e) = Self::execute_task_with_retry(db, &task, true).await {
                                tracing::error!(
                                    "[TappScheduler] Compensation {} failed for {}: {}",
                                    i + 1,
                                    task.task_id,
                                    e
                                );
                            }
                        }
                        if missed_count > 5 {
                            Self::update_missed_stats(db, &task, missed_count - 5).await?;
                        }
                    }
                }
            }

            // 执行当前到期的任务
            if let Err(e) = Self::execute_task_with_retry(db, &task, false).await {
                tracing::error!(
                    "[TappScheduler] Failed to execute task {} for tapp {}: {}",
                    task.task_id,
                    task.tapp_id,
                    e
                );
            }
        }

        Ok(())
    }

    /// Atomically claim one due row. `next_run_at` doubles as a recovery lease:
    /// another replica cannot run the same occurrence, while a crashed worker
    /// makes the task eligible again after the lease expires.
    async fn claim_due_task(
        db: &DatabaseConnection,
        candidate: tapp_scheduled_tasks::Model,
        now: DateTime<Utc>,
    ) -> Result<Option<tapp_scheduled_tasks::Model>, String> {
        let txn = db
            .begin()
            .await
            .map_err(|e| format!("Failed to begin scheduler claim: {e}"))?;
        let current = tapp_scheduled_tasks::Entity::find_by_id(candidate.id)
            .lock_exclusive()
            .one(&txn)
            .await
            .map_err(|e| format!("Failed to lock due task: {e}"))?;
        let Some(current) = current else {
            txn.rollback().await.ok();
            return Ok(None);
        };
        let is_due = current.enabled
            && current
                .next_run_at
                .is_some_and(|next| next.with_timezone(&Utc) <= now);
        if !is_due {
            txn.rollback().await.ok();
            return Ok(None);
        }

        let lease_duration = Self::recovery_lease_duration(&current, now);
        let mut active: tapp_scheduled_tasks::ActiveModel = current.clone().into();
        active.next_run_at = Set(Some((now + lease_duration).into()));
        active.updated_at = Set(now.into());
        active
            .update(&txn)
            .await
            .map_err(|e| format!("Failed to claim due task: {e}"))?;
        txn.commit()
            .await
            .map_err(|e| format!("Failed to commit scheduler claim: {e}"))?;
        Ok(Some(current))
    }

    /// Size the crash-recovery lease from the validated retry/action envelope.
    /// Legacy rows are clamped to the current contract, so malformed historical
    /// retry JSON cannot create either an instant duplicate or an endless lease.
    fn recovery_lease_duration(task: &tapp_scheduled_tasks::Model, now: DateTime<Utc>) -> Duration {
        let retry: RetryConfig = task
            .retry_config
            .as_ref()
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default();
        let retries = retry.max_retries.clamp(0, MAX_SCHEDULER_RETRIES) as i64;
        let retry_delay = retry.retry_delay.clamp(1_000, MAX_SCHEDULER_RETRY_DELAY_MS);
        let action_count = task
            .backend_actions
            .as_ref()
            .and_then(serde_json::Value::as_array)
            .map_or(1_i64, |actions| {
                actions.len().clamp(1, MAX_SCHEDULER_BACKEND_ACTIONS) as i64
            });
        let missed_count = Self::calculate_missed_executions(task, now).max(0);
        let compensation_count = match task.missed_policy {
            MissedPolicy::Skip => 0,
            MissedPolicy::RunOnce => i64::from(missed_count > 0),
            MissedPolicy::RunAll => missed_count.min(5),
        };
        let occurrence_count = compensation_count.saturating_add(1);
        let execution_budget = (retries + 1)
            .saturating_mul(action_count)
            .saturating_mul(SCHEDULER_ACTION_BUDGET_MS)
            .saturating_add(retries.saturating_mul(retry_delay))
            .saturating_mul(occurrence_count)
            .saturating_add(Duration::minutes(5).num_milliseconds());
        Duration::milliseconds(execution_budget).clamp(
            Duration::minutes(MIN_SCHEDULER_LEASE_MINUTES),
            Duration::minutes(MAX_SCHEDULER_LEASE_MINUTES),
        )
    }

    /// 计算错过的执行次数
    fn calculate_missed_executions(
        task: &tapp_scheduled_tasks::Model,
        now: chrono::DateTime<Utc>,
    ) -> i64 {
        let Some(next_run_at) = task.next_run_at else {
            return 0;
        };

        let next_run: chrono::DateTime<Utc> = next_run_at.into();

        // 如果还没到执行时间，没有错过
        if next_run > now {
            return 0;
        }

        // 计算错过了多少个周期
        match task.schedule_type {
            ScheduleType::Interval => {
                if let Ok(config) =
                    serde_json::from_value::<ScheduleConfig>(task.schedule_config.clone())
                {
                    if let Some(interval_ms) = config.interval {
                        let elapsed_ms = (now - next_run).num_milliseconds();
                        if interval_ms > 0 {
                            return elapsed_ms / interval_ms;
                        }
                    }
                }
                0
            }
            ScheduleType::Daily => {
                // 每日任务：计算差了多少天
                let days = (now - next_run).num_days();
                days.max(0)
            }
            ScheduleType::Once => {
                // 一次性任务不需要补偿
                0
            }
            ScheduleType::Cron => {
                // Cron 任务：计算错过的次数（最多检查10次）
                if let Ok(config) =
                    serde_json::from_value::<ScheduleConfig>(task.schedule_config.clone())
                {
                    if let Some(cron_str) = config.cron {
                        if let Ok(schedule) = cron::Schedule::from_str(&cron_str) {
                            let mut count = 0i64;
                            for next in schedule.after(&next_run) {
                                if next >= now {
                                    break;
                                }
                                count += 1;
                                if count >= 10 {
                                    break; // 最多统计10次错过
                                }
                            }
                            return count;
                        }
                    }
                }
                0
            }
        }
    }

    /// 更新错过执行的统计
    async fn update_missed_stats(
        db: &DatabaseConnection,
        task: &tapp_scheduled_tasks::Model,
        missed_count: i64,
    ) -> Result<(), String> {
        let txn = db
            .begin()
            .await
            .map_err(|e| format!("Failed to begin missed stats update: {e}"))?;
        let current = tapp_scheduled_tasks::Entity::find_by_id(task.id)
            .lock_exclusive()
            .one(&txn)
            .await
            .map_err(|e| format!("Failed to lock missed stats: {e}"))?
            .ok_or_else(|| "Scheduled task no longer exists".to_string())?;
        let mut stats: TaskStats =
            serde_json::from_value(current.stats.clone()).unwrap_or_default();
        stats.missed_runs += missed_count;

        let mut active: tapp_scheduled_tasks::ActiveModel = current.into();
        active.stats = Set(serde_json::to_value(&stats).unwrap_or(json!({})));
        active.updated_at = Set(Utc::now().into());

        active
            .update(&txn)
            .await
            .map_err(|e| format!("Failed to update missed stats: {}", e))?;
        txn.commit()
            .await
            .map_err(|e| format!("Failed to commit missed stats: {e}"))?;
        Ok(())
    }

    /// 带重试的任务执行
    async fn execute_task_with_retry(
        db: &DatabaseConnection,
        task: &tapp_scheduled_tasks::Model,
        is_compensation: bool,
    ) -> Result<(), String> {
        // 解析重试配置
        let retry_config: RetryConfig = task
            .retry_config
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let max_retries = retry_config.max_retries.clamp(0, MAX_SCHEDULER_RETRIES);
        let retry_delay_ms = retry_config
            .retry_delay
            .clamp(1000, MAX_SCHEDULER_RETRY_DELAY_MS);

        let mut last_error = String::new();

        for attempt in 0..=max_retries {
            match Self::execute_task(db, task, is_compensation, attempt, attempt == max_retries)
                .await
            {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last_error = e.clone();
                    if attempt < max_retries {
                        tracing::warn!(
                            "[TappScheduler] Task {} attempt {} failed: {}, retrying in {}ms",
                            task.task_id,
                            attempt + 1,
                            e,
                            retry_delay_ms
                        );
                        tokio::time::sleep(tokio::time::Duration::from_millis(
                            retry_delay_ms as u64,
                        ))
                        .await;
                    }
                }
            }
        }

        Err(format!(
            "All {} retries failed. Last error: {}",
            max_retries + 1,
            last_error
        ))
    }

    /// 执行单个任务
    async fn execute_task(
        db: &DatabaseConnection,
        task: &tapp_scheduled_tasks::Model,
        is_compensation: bool,
        retry_count: i32,
        final_attempt: bool,
    ) -> Result<(), String> {
        let now = Utc::now();
        let scheduled_at = task.next_run_at.unwrap_or(now.into());

        tracing::info!(
            "[TappScheduler] Executing task {} for tapp {} (user {})",
            task.task_id,
            task.tapp_id,
            task.user_id
        );

        // 创建执行记录
        let execution = tapp_task_executions::ActiveModel {
            scheduled_task_id: Set(task.id),
            user_id: Set(task.user_id),
            tapp_id: Set(task.tapp_id.clone()),
            task_id: Set(task.task_id.clone()),
            scheduled_at: Set(scheduled_at),
            executed_at: Set(now.into()),
            execution_target: Set(task.execution_target.to_string()),
            status: Set(ExecutionStatus::Running),
            is_compensation: Set(is_compensation),
            retry_count: Set(retry_count),
            ..Default::default()
        };

        let execution = execution
            .insert(db)
            .await
            .map_err(|e| format!("Failed to create execution record: {}", e))?;

        let mut result: Option<serde_json::Value> = None;
        let authority = Self::validate_task_execution_permissions(db, task).await;
        let mut error = authority.as_ref().err().cloned();
        let mut status = if error.is_some() {
            ExecutionStatus::Failed
        } else {
            ExecutionStatus::Success
        };
        let mut awaiting_frontend = false;
        let start_time = std::time::Instant::now();

        // 根据执行目标处理
        match task.execution_target {
            ExecutionTarget::Backend | ExecutionTarget::Both => {
                // 执行后端操作
                if status == ExecutionStatus::Success {
                    if let Some(actions) = &task.backend_actions {
                        match Self::execute_backend_actions(
                            db,
                            task,
                            actions,
                            authority.as_ref().expect("validated authority"),
                        )
                        .await
                        {
                            Ok(r) => result = Some(r),
                            Err(e) => {
                                error = Some(e);
                                status = ExecutionStatus::Failed;
                            }
                        }
                    }
                }
            }
            ExecutionTarget::Frontend => {
                // 仅前端执行，后端只记录
            }
        }

        // 后端阶段成功后再推送前端任务；前端回调的最终状态由 task:complete 上报。
        if status == ExecutionStatus::Success
            && matches!(
                task.execution_target,
                ExecutionTarget::Frontend | ExecutionTarget::Both
            )
        {
            // 根据 scope 决定推送目标
            let (scope_str, target_users) = match task.scope {
                TaskScope::User => {
                    // 用户级别：只推送给注册任务的用户
                    ("user".to_string(), Some(vec![task.user_id]))
                }
                TaskScope::Tapp => {
                    // Tapp 级别：推送给所有安装该 Tapp 的用户
                    // target_users = None 表示广播，前端/WebSocket 层根据 tapp_id 过滤
                    ("tapp".to_string(), None)
                }
                TaskScope::TappPerUser => {
                    // Tapp 用户级别：类似 Tapp 级别，但每个用户独立数据
                    // 只推送给注册任务的用户
                    ("tapp-per-user".to_string(), Some(vec![task.user_id]))
                }
                TaskScope::Global => {
                    // 全局级别：通常不需要前端推送，或只推送给管理员
                    // 这里设为广播，让前端过滤
                    ("global".to_string(), None)
                }
            };

            let context = TaskExecutionContext {
                id: task.id,
                task_id: task.task_id.clone(),
                tapp_id: task.tapp_id.clone(),
                user_id: task.user_id,
                scope: scope_str,
                scheduled_at: scheduled_at.timestamp_millis(),
                executed_at: now.timestamp_millis(),
                is_compensation,
                payload: task.payload.clone(),
            };

            let message = FrontendTaskMessage {
                msg_type: "task:execute".to_string(),
                task: context,
                payload: task.payload.clone(),
                scheduled_at: scheduled_at.to_rfc3339(),
                execution_id: execution.id,
                target_users,
            };

            match Self::enqueue_frontend_message(db, task, &message).await {
                Ok(deliveries) if deliveries > 0 => awaiting_frontend = true,
                Ok(_) => {
                    tracing::warn!(
                        task_id = %task.task_id,
                        tapp_id = %task.tapp_id,
                        "[TappScheduler] No live frontend scheduler audience"
                    );
                    error = Some("No frontend scheduler subscribers".to_string());
                    status = ExecutionStatus::Failed;
                }
                Err(dispatch_error) => {
                    tracing::error!(
                        error = %dispatch_error,
                        task_id = %task.task_id,
                        "[TappScheduler] Shared frontend dispatch failed"
                    );
                    error = Some(dispatch_error);
                    status = ExecutionStatus::Failed;
                }
            }
        }

        let duration_ms = start_time.elapsed().as_millis() as i32;

        if awaiting_frontend {
            // 保持 execution=running；只推进调度时间，最终成功/失败由 WS 完成消息落库。
            let mut execution_update: tapp_task_executions::ActiveModel = execution.into();
            execution_update.result = Set(result.clone());
            execution_update
                .update(db)
                .await
                .map_err(|e| format!("Failed to keep frontend execution pending: {}", e))?;

            Self::update_task_after_dispatch(db, task, result).await?;
            return Ok(());
        }

        // 更新执行记录
        let mut execution_update: tapp_task_executions::ActiveModel = execution.into();
        execution_update.completed_at = Set(Some(Utc::now().into()));
        execution_update.status = Set(status.clone());
        execution_update.result = Set(result.clone());
        execution_update.error = Set(error.clone());
        execution_update.duration_ms = Set(Some(duration_ms));
        execution_update
            .update(db)
            .await
            .map_err(|e| format!("Failed to update execution record: {}", e))?;

        // 更新任务状态和统计
        Self::update_task_after_execution(
            db,
            task,
            &status,
            result,
            error.clone(),
            status == ExecutionStatus::Success || final_attempt,
        )
        .await?;

        if status == ExecutionStatus::Failed {
            let failure = error.unwrap_or_else(|| "Scheduled task failed".to_string());
            Self::notify_task_failure(task, &failure).await;
            return Err(failure);
        }

        Ok(())
    }

    async fn enqueue_frontend_message(
        db: &DatabaseConnection,
        task: &tapp_scheduled_tasks::Model,
        message: &FrontendTaskMessage,
    ) -> Result<usize, String> {
        let active_connections =
            shared_registry::list_subject_endpoints(db, SCHEDULER_PRESENCE_NAMESPACE)
                .await
                .map_err(|error| format!("Failed to list scheduler connections: {error}"))?;
        let mut deliveries = 0usize;
        let mut first_error = None;
        for connection in active_connections {
            let user_id = connection.subject_id;
            if message
                .target_users
                .as_ref()
                .is_some_and(|users| !users.contains(&user_id))
            {
                continue;
            }
            if !Self::can_receive_frontend_task(db, user_id, task).await? {
                continue;
            }
            match shared_registry::enqueue(
                db,
                SCHEDULER_MAILBOX_CHANNEL,
                &scheduler_mailbox_recipient(&connection.record_id),
                message,
                Utc::now().timestamp() + SCHEDULER_MESSAGE_TTL_SECONDS,
            )
            .await
            {
                Ok(()) => {
                    deliveries += 1;
                    SCHEDULER_DISPATCHED.fetch_add(1, Ordering::Relaxed);
                }
                Err(error) => {
                    SCHEDULER_DELIVERY_FAILURES.fetch_add(1, Ordering::Relaxed);
                    first_error.get_or_insert_with(|| {
                        format!("Failed to enqueue scheduler task: {error}")
                    });
                }
            }
        }
        if deliveries > 0 {
            Ok(deliveries)
        } else if let Some(error) = first_error {
            Err(error)
        } else {
            Ok(0)
        }
    }

    async fn can_receive_frontend_task(
        db: &DatabaseConnection,
        user_id: i32,
        task: &tapp_scheduled_tasks::Model,
    ) -> Result<bool, String> {
        match task.scope {
            TaskScope::User | TaskScope::TappPerUser => Ok(task.user_id == user_id),
            TaskScope::Global => {
                let row = db
                    .query_one(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        "SELECT is_admin FROM users WHERE id = $1 LIMIT 1",
                        [user_id.into()],
                    ))
                    .await
                    .map_err(|error| {
                        format!("Failed to verify global scheduler audience: {error}")
                    })?;
                Ok(row
                    .and_then(|row| row.try_get::<bool>("", "is_admin").ok())
                    .unwrap_or(false))
            }
            TaskScope::Tapp => {
                let row = db
                    .query_one(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        r#"
SELECT EXISTS (
    SELECT 1
    FROM tapps
    WHERE tapp_id = $1
      AND (
          user_id = $2
          OR user_id = (
              SELECT id FROM users
              WHERE is_admin = true
              ORDER BY id
              LIMIT 1
          )
          OR EXISTS (
              SELECT 1 FROM users
              WHERE id = $2 AND is_admin = true
          )
      )
) AS allowed
"#,
                        [task.tapp_id.clone().into(), user_id.into()],
                    ))
                    .await
                    .map_err(|error| {
                        format!("Failed to verify Tapp scheduler audience: {error}")
                    })?;
                Ok(row
                    .and_then(|row| row.try_get::<bool>("", "allowed").ok())
                    .unwrap_or(false))
            }
        }
    }

    /// 接收已认证前端对 task:execute 的完成回执。
    pub async fn complete_frontend_execution(
        &self,
        user_id: i32,
        execution_id: i32,
        success: bool,
        error: Option<String>,
    ) -> Result<(), String> {
        let execution = tapp_task_executions::Entity::find_by_id(execution_id)
            .one(&self.db)
            .await
            .map_err(|e| format!("Failed to query frontend execution: {}", e))?
            .ok_or_else(|| format!("Execution {} not found", execution_id))?;

        if execution.status != ExecutionStatus::Running {
            return Ok(());
        }

        let task = tapp_scheduled_tasks::Entity::find_by_id(execution.scheduled_task_id)
            .one(&self.db)
            .await
            .map_err(|e| format!("Failed to query scheduled task: {}", e))?
            .ok_or_else(|| "Scheduled task no longer exists".to_string())?;
        if !Self::can_complete_frontend_execution(&self.db, user_id, &execution, &task).await? {
            return Err("Execution does not belong to the current scheduler audience".to_string());
        }

        let status = if success {
            ExecutionStatus::Success
        } else {
            ExecutionStatus::Failed
        };
        Self::finalize_frontend_execution(&self.db, execution, status, error).await
    }

    async fn can_complete_frontend_execution(
        db: &DatabaseConnection,
        user_id: i32,
        execution: &tapp_task_executions::Model,
        task: &tapp_scheduled_tasks::Model,
    ) -> Result<bool, String> {
        if matches!(task.scope, TaskScope::User | TaskScope::TappPerUser)
            && execution.user_id != user_id
        {
            return Ok(false);
        }
        Self::can_receive_frontend_task(db, user_id, task).await
    }

    async fn expire_stale_frontend_executions(
        db: &DatabaseConnection,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        let cutoff = (now - Duration::minutes(5)).fixed_offset();
        let stale = tapp_task_executions::Entity::find()
            .filter(tapp_task_executions::Column::Status.eq(ExecutionStatus::Running))
            .filter(tapp_task_executions::Column::CompletedAt.is_null())
            .filter(tapp_task_executions::Column::ExecutedAt.lte(cutoff))
            .all(db)
            .await
            .map_err(|e| format!("Failed to query stale frontend executions: {}", e))?;

        for execution in stale {
            if let Err(error) = Self::finalize_frontend_execution(
                db,
                execution,
                ExecutionStatus::Timeout,
                Some("Frontend task completion timed out".to_string()),
            )
            .await
            {
                tracing::error!("[TappScheduler] Failed to expire execution: {}", error);
            }
        }
        Ok(())
    }

    async fn finalize_frontend_execution(
        db: &DatabaseConnection,
        execution: tapp_task_executions::Model,
        status: ExecutionStatus,
        error: Option<String>,
    ) -> Result<(), String> {
        if execution.status != ExecutionStatus::Running {
            return Ok(());
        }

        let task = tapp_scheduled_tasks::Entity::find_by_id(execution.scheduled_task_id)
            .one(db)
            .await
            .map_err(|e| format!("Failed to query scheduled task: {}", e))?
            .ok_or_else(|| "Scheduled task no longer exists".to_string())?;

        let now = Utc::now();
        let executed_at = execution.executed_at.with_timezone(&Utc);
        let duration_ms = (now - executed_at)
            .num_milliseconds()
            .clamp(0, i32::MAX as i64) as i32;
        let result = execution.result.clone();

        // Multiple Tapp-scope clients may acknowledge the same broadcast. Claim
        // the running row atomically so task stats are finalized exactly once.
        let update = tapp_task_executions::Entity::update_many()
            .col_expr(
                tapp_task_executions::Column::CompletedAt,
                Expr::value(Some(now.fixed_offset())),
            )
            .col_expr(
                tapp_task_executions::Column::Status,
                Expr::value(status.clone()),
            )
            .col_expr(
                tapp_task_executions::Column::Error,
                Expr::value(error.clone()),
            )
            .col_expr(
                tapp_task_executions::Column::DurationMs,
                Expr::value(Some(duration_ms)),
            )
            .filter(tapp_task_executions::Column::Id.eq(execution.id))
            .filter(tapp_task_executions::Column::Status.eq(ExecutionStatus::Running))
            .exec(db)
            .await
            .map_err(|e| format!("Failed to finalize frontend execution: {}", e))?;
        if update.rows_affected == 0 {
            return Ok(());
        }

        SCHEDULER_COMPLETED.fetch_add(1, Ordering::Relaxed);
        if status == ExecutionStatus::Timeout {
            SCHEDULER_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
        }

        Self::update_task_after_frontend_completion(db, &task, &status, result, error.clone())
            .await?;
        if matches!(status, ExecutionStatus::Failed | ExecutionStatus::Timeout) {
            Self::notify_task_failure(&task, error.as_deref().unwrap_or("前端任务执行失败")).await;
        }
        Ok(())
    }

    async fn notify_task_failure(task: &tapp_scheduled_tasks::Model, error: &str) {
        let Some(manager) = crate::services::agent::notifications::get_notification_manager()
        else {
            return;
        };
        if let Some(platform) = core_platform_from_task(task) {
            manager
                .notify_platform_sync_error(task.user_id, platform, error)
                .await;
        } else {
            manager
                .notify_tapp(
                    task.user_id,
                    &task.tapp_id,
                    Some(&format!("定时任务失败: {}", task.name)),
                    error,
                    "error",
                )
                .await;
        }
    }

    /// 每次真正执行前重新读取当前角色和动态权限。
    /// 定时任务可能在注册数小时后才触发，不能永久沿用注册时的管理员/下放状态。
    async fn validate_task_execution_permissions(
        db: &DatabaseConnection,
        task: &tapp_scheduled_tasks::Model,
    ) -> Result<ScheduledExecutionAuthority, String> {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT is_admin FROM users WHERE id = $1 LIMIT 1",
                [task.user_id.into()],
            ))
            .await
            .map_err(|e| format!("Failed to verify scheduler user role: {}", e))?
            .ok_or_else(|| "Scheduler user no longer exists".to_string())?;
        let is_admin = row
            .try_get::<bool>("", "is_admin")
            .map_err(|e| format!("Failed to read scheduler user role: {}", e))?;
        let role = if is_admin {
            UserRole::Admin
        } else {
            UserRole::User
        };

        // Core platform refresh is configured only through the administrator
        // settings surface. It must not inherit an installed Tapp's ownership
        // or delegated permission lifecycle.
        if is_core_platform_sync_task(task) {
            return if is_admin {
                Ok(ScheduledExecutionAuthority {
                    role,
                    owner_id: task.user_id,
                    ai_model_tier: None,
                })
            } else {
                Err("Core platform refresh requires current administrator access".to_string())
            };
        }

        if matches!(task.scope, TaskScope::Global) && !is_admin {
            return Err("Global scheduler task requires current administrator access".to_string());
        }

        let mut required = vec![TappPermission::SchedulerRegister];
        required.extend(backend_action_permissions(&task.backend_actions)?);

        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        for permission in &required {
            if !TappPermissionService::check(&config, role, *permission) {
                return Err(format!(
                    "Permission revoked before scheduled execution: {}",
                    permission.as_str()
                ));
            }
        }
        drop(config);

        let tapp = crate::api::tapp_runtime::common::resolve_accessible_tapp(
            db,
            task.user_id,
            &task.tapp_id,
        )
        .await
        .map_err(|(_, body)| {
            body.0
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Scheduled Tapp is no longer accessible")
                .to_string()
        })?;
        let approved = tapp
            .approved_permissions
            .as_array()
            .cloned()
            .unwrap_or_default();
        for permission in required {
            if !approved
                .iter()
                .any(|value| value.as_str() == Some(permission.as_str()))
            {
                return Err(format!(
                    "Tapp permission revoked before scheduled execution: {}",
                    permission.as_str()
                ));
            }
        }
        let ai_model_tier =
            validate_backend_action_declarations(&tapp.manifest, &task.backend_actions)?;
        Ok(ScheduledExecutionAuthority {
            role,
            owner_id: tapp.user_id,
            ai_model_tier,
        })
    }

    /// 执行后端操作（支持结果链式传递）
    async fn execute_backend_actions(
        db: &DatabaseConnection,
        task: &tapp_scheduled_tasks::Model,
        actions_json: &serde_json::Value,
        authority: &ScheduledExecutionAuthority,
    ) -> Result<serde_json::Value, String> {
        // 尝试解析为新格式（带 resultAs），否则回退到旧格式
        let action_wrappers: Vec<BackendActionWrapper> =
            match serde_json::from_value(actions_json.clone()) {
                Ok(wrappers) => wrappers,
                Err(_) => {
                    // 回退：尝试解析为旧格式并转换
                    let actions: Vec<BackendAction> = serde_json::from_value(actions_json.clone())
                        .map_err(|e| format!("Invalid backend actions: {}", e))?;
                    actions
                        .into_iter()
                        .map(|action| BackendActionWrapper {
                            action,
                            result_as: None,
                            condition: None,
                        })
                        .collect()
                }
            };
        if action_wrappers.len() > MAX_SCHEDULER_BACKEND_ACTIONS {
            return Err(format!(
                "Backend action pipeline exceeds the maximum of {MAX_SCHEDULER_BACKEND_ACTIONS}"
            ));
        }

        // 结果上下文：存储命名结果
        let mut context: HashMap<String, serde_json::Value> = HashMap::new();
        // 特殊变量：上一个操作的结果
        context.insert("_last".to_string(), json!(null));
        context.insert("_taskId".to_string(), json!(task.task_id.clone()));
        context.insert("_tappId".to_string(), json!(task.tapp_id.clone()));
        context.insert("_userId".to_string(), json!(task.user_id));

        let mut results: Vec<serde_json::Value> = Vec::new();

        for wrapper in action_wrappers {
            // 检查条件执行
            if let Some(ref cond) = wrapper.condition {
                let cond_value = Self::resolve_template(cond, &context);
                if !Self::is_truthy(&cond_value) {
                    results.push(json!({ "skipped": true, "condition": cond }));
                    continue;
                }
            }

            // 执行操作（带模板替换）
            let result =
                Self::execute_single_action(db, task, &wrapper.action, &context, authority).await;

            match &result {
                Ok(r) => {
                    // 存储结果到上下文
                    context.insert("_last".to_string(), r.clone());
                    if let Some(ref name) = wrapper.result_as {
                        context.insert(name.clone(), r.clone());
                    }
                    results.push(json!({ "success": true, "result": r }));
                }
                Err(e) => {
                    // 后端动作是串行流水线；失败后继续会让后续模板读取到错误上下文，
                    // 且旧逻辑最终仍返回 Ok，导致任务被错误记为成功。
                    return Err(format!("Backend action failed: {}", e));
                }
            }
        }

        Ok(json!({ "actions": results, "context": context }))
    }

    /// 执行单个操作
    async fn execute_single_action(
        db: &DatabaseConnection,
        task: &tapp_scheduled_tasks::Model,
        action: &BackendAction,
        context: &HashMap<String, serde_json::Value>,
        authority: &ScheduledExecutionAuthority,
    ) -> Result<serde_json::Value, String> {
        match action {
            BackendAction::PlatformSync { platform } => {
                let platform = Self::resolve_template(platform, context);
                Self::action_platform_sync(db, &platform).await
            }
            BackendAction::StorageSet { key, value } => {
                let key = Self::resolve_template(key, context);
                let value = Self::resolve_json_templates(value, context);
                Self::action_storage_set(db, task.user_id, &task.tapp_id, &key, value).await
            }
            BackendAction::StorageGet { key } => {
                let key = Self::resolve_template(key, context);
                Self::action_storage_get(db, task.user_id, &task.tapp_id, &key).await
            }
            BackendAction::StorageDelete { key } => {
                let key = Self::resolve_template(key, context);
                Self::action_storage_delete(db, task.user_id, &task.tapp_id, &key).await
            }
            BackendAction::AiGenerate { prompt } => {
                let prompt = Self::resolve_template(prompt, context);
                Self::action_ai_generate(db, task, authority, &prompt).await
            }
            BackendAction::Fetch {
                url,
                method,
                headers,
                body,
            } => {
                let url = Self::resolve_template(url, context);
                let method = method.as_ref().map(|m| Self::resolve_template(m, context));
                let headers = headers
                    .as_ref()
                    .map(|h| Self::resolve_json_templates(h, context));
                let body = body
                    .as_ref()
                    .map(|b| Self::resolve_json_templates(b, context));
                Self::action_fetch(&url, method, headers, body).await
            }
            BackendAction::NotificationQueue {
                title,
                message,
                notification_type,
            } => {
                let title = title.as_ref().map(|t| Self::resolve_template(t, context));
                let message = Self::resolve_template(message, context);
                Self::action_notification_queue(
                    task.user_id,
                    &task.tapp_id,
                    title,
                    message,
                    notification_type.clone(),
                )
                .await
            }
            BackendAction::Transform {
                input,
                extract,
                template,
            } => Self::action_transform(context, input, extract.as_deref(), template.as_deref()),
        }
    }

    /// 解析字符串中的模板变量 {{varName}} 或 {{varName.field}}
    fn resolve_template(template: &str, context: &HashMap<String, serde_json::Value>) -> String {
        let re = regex::Regex::new(r"\{\{([^}]+)\}\}").unwrap();
        re.replace_all(template, |caps: &regex::Captures| {
            let path = caps.get(1).map_or("", |m| m.as_str()).trim();
            Self::get_value_by_path(context, path)
        })
        .to_string()
    }

    /// 根据路径获取值，支持 varName.field.subfield
    fn get_value_by_path(context: &HashMap<String, serde_json::Value>, path: &str) -> String {
        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() {
            return "".to_string();
        }

        let var_name = parts[0];
        let Some(mut value) = context.get(var_name).cloned() else {
            return format!("{{{{{}}}}}", path); // 保留原样
        };

        // 遍历路径
        for part in &parts[1..] {
            value = match value {
                serde_json::Value::Object(ref map) => {
                    map.get(*part).cloned().unwrap_or(serde_json::Value::Null)
                }
                serde_json::Value::Array(ref arr) => {
                    if let Ok(idx) = part.parse::<usize>() {
                        arr.get(idx).cloned().unwrap_or(serde_json::Value::Null)
                    } else {
                        serde_json::Value::Null
                    }
                }
                _ => serde_json::Value::Null,
            };
        }

        // 转换为字符串
        match value {
            serde_json::Value::String(s) => s,
            serde_json::Value::Null => "".to_string(),
            other => other.to_string(),
        }
    }

    /// 解析 JSON 中的模板变量
    fn resolve_json_templates(
        value: &serde_json::Value,
        context: &HashMap<String, serde_json::Value>,
    ) -> serde_json::Value {
        match value {
            serde_json::Value::String(s) => {
                // 检查是否是纯变量引用 "{{varName}}"
                let trimmed = s.trim();
                if trimmed.starts_with("{{")
                    && trimmed.ends_with("}}")
                    && trimmed.matches("{{").count() == 1
                {
                    let path = &trimmed[2..trimmed.len() - 2].trim();
                    // 直接返回原始值（保持类型）
                    let parts: Vec<&str> = path.split('.').collect();
                    if !parts.is_empty() {
                        if let Some(mut val) = context.get(parts[0]).cloned() {
                            for part in &parts[1..] {
                                val = match val {
                                    serde_json::Value::Object(ref map) => {
                                        map.get(*part).cloned().unwrap_or(serde_json::Value::Null)
                                    }
                                    _ => serde_json::Value::Null,
                                };
                            }
                            return val;
                        }
                    }
                }
                // 否则作为字符串模板处理
                serde_json::Value::String(Self::resolve_template(s, context))
            }
            serde_json::Value::Object(map) => {
                let new_map: serde_json::Map<String, serde_json::Value> = map
                    .iter()
                    .map(|(k, v)| (k.clone(), Self::resolve_json_templates(v, context)))
                    .collect();
                serde_json::Value::Object(new_map)
            }
            serde_json::Value::Array(arr) => serde_json::Value::Array(
                arr.iter()
                    .map(|v| Self::resolve_json_templates(v, context))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// 检查值是否为“真”
    fn is_truthy(value: &str) -> bool {
        !value.is_empty() && value != "false" && value != "null" && value != "0"
    }

    /// 数据转换操作
    fn action_transform(
        context: &HashMap<String, serde_json::Value>,
        input: &str,
        extract: Option<&str>,
        template: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        // 获取输入值
        let input_value = context
            .get(input)
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        // 如果有 extract，使用简单路径提取
        let extracted = if let Some(path) = extract {
            let parts: Vec<&str> = path.split('.').collect();
            let mut val = input_value.clone();
            for part in parts {
                val = match val {
                    serde_json::Value::Object(ref map) => {
                        map.get(part).cloned().unwrap_or(serde_json::Value::Null)
                    }
                    serde_json::Value::Array(ref arr) => {
                        if let Ok(idx) = part.parse::<usize>() {
                            arr.get(idx).cloned().unwrap_or(serde_json::Value::Null)
                        } else {
                            serde_json::Value::Null
                        }
                    }
                    _ => serde_json::Value::Null,
                };
            }
            val
        } else {
            input_value
        };

        // 如果有模板，应用模板
        if let Some(tpl) = template {
            let mut temp_context = context.clone();
            temp_context.insert("_input".to_string(), extracted);
            Ok(serde_json::Value::String(Self::resolve_template(
                tpl,
                &temp_context,
            )))
        } else {
            Ok(extracted)
        }
    }

    /// 执行平台同步
    async fn action_platform_sync(
        db: &DatabaseConnection,
        platform: &str,
    ) -> Result<serde_json::Value, String> {
        tracing::info!("[TappScheduler] Platform sync: {}", platform);
        let data = crate::api::profile::refresh_platform_for_scheduler(db, platform).await?;
        Ok(json!({ "platform": platform, "synced": true, "data": data }))
    }

    /// 执行存储设置
    async fn action_storage_set(
        db: &DatabaseConnection,
        user_id: i32,
        tapp_id: &str,
        key: &str,
        value: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        validate_sandbox_storage_key(key).map_err(str::to_string)?;
        validate_storage_value_size(&value).map_err(|_| "Storage value too large".to_string())?;
        write_storage_value(db, user_id, tapp_id, key, value)
            .await
            .map_err(|status| format!("Storage write failed: {status}"))?;

        Ok(json!({ "key": key, "set": true }))
    }

    /// 执行存储删除
    async fn action_storage_delete(
        db: &DatabaseConnection,
        user_id: i32,
        tapp_id: &str,
        key: &str,
    ) -> Result<serde_json::Value, String> {
        use crate::models::entities::tapp_storage;

        validate_sandbox_storage_key(key).map_err(str::to_string)?;

        tapp_storage::Entity::delete_many()
            .filter(tapp_storage::Column::UserId.eq(user_id))
            .filter(tapp_storage::Column::TappId.eq(tapp_id))
            .filter(tapp_storage::Column::Key.eq(key))
            .exec(db)
            .await
            .map_err(|e| format!("Storage delete failed: {}", e))?;

        Ok(json!({ "key": key, "deleted": true }))
    }

    /// 执行存储读取
    async fn action_storage_get(
        db: &DatabaseConnection,
        user_id: i32,
        tapp_id: &str,
        key: &str,
    ) -> Result<serde_json::Value, String> {
        validate_sandbox_storage_key(key).map_err(str::to_string)?;
        read_storage_value(db, user_id, tapp_id, key)
            .await
            .map_err(|status| format!("Storage read failed: {status}"))
    }

    /// 执行 AI 生成
    async fn action_ai_generate(
        db: &DatabaseConnection,
        task: &tapp_scheduled_tasks::Model,
        authority: &ScheduledExecutionAuthority,
        prompt: &str,
    ) -> Result<serde_json::Value, String> {
        if prompt.len() > 2000 {
            return Err("Prompt too long (max 2000 characters)".to_string());
        }
        if let Some(reason) = crate::api::tapp_runtime::common::validate_prompt_security(prompt) {
            return Err(format!("Prompt contains disallowed content: {reason}"));
        }
        tracing::info!(
            "[TappScheduler] AI generate: {}...",
            &prompt[..prompt.len().min(50)]
        );
        let tier = authority
            .ai_model_tier
            .ok_or_else(|| "Scheduled AI action has no validated Manifest tier".to_string())?;
        let text = crate::api::tapp_runtime::execute_governed_text(
            db,
            crate::api::tapp_runtime::GovernedTextRequest {
                role: authority.role,
                subject_id: task.user_id,
                owner_id: authority.owner_id,
                tapp_id: &task.tapp_id,
                source: "scheduler",
                operation: TappAiOperation::Generate,
                tier,
                system_prompt: "You are executing a background task for a sandboxed Tapp. Do not reveal system information, execute code, or access external URLs. Return concise text only.",
                prompt,
                client_ip: None,
            },
        )
        .await?;
        Ok(json!({ "text": text, "generated": true }))
    }

    /// 执行 HTTP 请求
    async fn action_fetch(
        url: &str,
        method: Option<String>,
        headers: Option<serde_json::Value>,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        let method_str = method.unwrap_or_else(|| "GET".to_string());
        let method = reqwest::Method::from_str(&method_str)
            .map_err(|_| format!("Invalid method: {}", method_str))?;
        let (target_url, client) = crate::services::outbound_security::build_public_http_client(
            url,
            std::time::Duration::from_secs(30),
            Some("Myriad-Tapp/1.0 (scheduler)"),
        )
        .await?;
        let mut request = client.request(method, target_url);

        if let Some(headers_json) = headers {
            if let Some(headers_map) = headers_json.as_object() {
                for (key, value) in headers_map {
                    if let Some(v) = value.as_str() {
                        let name = reqwest::header::HeaderName::from_str(key)
                            .map_err(|_| format!("Invalid HTTP header name: {key}"))?;
                        crate::services::outbound_security::validate_outbound_header(&name)?;
                        let value = reqwest::header::HeaderValue::from_str(v)
                            .map_err(|_| format!("Invalid HTTP header value: {key}"))?;
                        request = request.header(name, value);
                    }
                }
            }
        }

        if let Some(body_json) = body {
            request = request.json(&body_json);
        }

        let response = request
            .send()
            .await
            .map_err(|e| format!("Fetch failed: {}", e))?;

        let status = response.status().as_u16();
        let body = crate::services::outbound_security::read_limited_body(
            response,
            MAX_SCHEDULER_FETCH_RESPONSE_BYTES,
        )
        .await?;
        let body = String::from_utf8_lossy(&body).into_owned();

        Ok(json!({ "status": status, "body": body }))
    }

    /// 排队通知
    async fn action_notification_queue(
        user_id: i32,
        tapp_id: &str,
        title: Option<String>,
        message: String,
        notification_type: Option<String>,
    ) -> Result<serde_json::Value, String> {
        let notification_kind = notification_type.unwrap_or_else(|| "info".to_string());
        let manager = crate::services::agent::notifications::get_notification_manager()
            .ok_or_else(|| "Notification system is not initialized".to_string())?;
        let notification_id = manager
            .notify_tapp(
                user_id,
                tapp_id,
                title.as_deref(),
                &message,
                &notification_kind,
            )
            .await;

        Ok(json!({ "queued": true, "notificationId": notification_id }))
    }

    /// 前端任务已发出：推进 next_run，但在回执前不计入成功/失败。
    async fn update_task_after_dispatch(
        db: &DatabaseConnection,
        task: &tapp_scheduled_tasks::Model,
        backend_result: Option<serde_json::Value>,
    ) -> Result<(), String> {
        let now = Utc::now();
        let txn = db
            .begin()
            .await
            .map_err(|e| format!("Failed to begin frontend dispatch update: {e}"))?;
        let current = tapp_scheduled_tasks::Entity::find_by_id(task.id)
            .lock_exclusive()
            .one(&txn)
            .await
            .map_err(|e| format!("Failed to lock frontend dispatch task: {e}"))?
            .ok_or_else(|| "Scheduled task no longer exists".to_string())?;
        let mut stats: TaskStats =
            serde_json::from_value(current.stats.clone()).unwrap_or_default();
        stats.total_runs += 1;
        let next_run_at =
            Self::calculate_next_run(&task.schedule_type, &task.schedule_config, now)?;

        let mut active: tapp_scheduled_tasks::ActiveModel = current.into();
        active.last_run_at = Set(Some(now.into()));
        active.last_run_result = Set(Some(json!({
            "status": "running",
            "result": backend_result,
            "error": null,
        })));
        active.stats = Set(serde_json::to_value(&stats).unwrap_or(json!({})));
        active.next_run_at = Set(next_run_at.map(|time| time.into()));
        active.updated_at = Set(now.into());
        if matches!(task.schedule_type, ScheduleType::Once) {
            active.enabled = Set(false);
        }

        active
            .update(&txn)
            .await
            .map_err(|e| format!("Failed to advance frontend task: {}", e))?;
        txn.commit()
            .await
            .map_err(|e| format!("Failed to commit frontend dispatch: {e}"))?;
        Ok(())
    }

    /// 前端回执只补齐最终状态；total_runs 已在 dispatch 时增加。
    async fn update_task_after_frontend_completion(
        db: &DatabaseConnection,
        task: &tapp_scheduled_tasks::Model,
        status: &ExecutionStatus,
        result: Option<serde_json::Value>,
        error: Option<String>,
    ) -> Result<(), String> {
        let txn = db
            .begin()
            .await
            .map_err(|e| format!("Failed to begin frontend stats update: {e}"))?;
        let current = tapp_scheduled_tasks::Entity::find_by_id(task.id)
            .lock_exclusive()
            .one(&txn)
            .await
            .map_err(|e| format!("Failed to lock frontend stats task: {e}"))?
            .ok_or_else(|| "Scheduled task no longer exists".to_string())?;
        let mut stats: TaskStats =
            serde_json::from_value(current.stats.clone()).unwrap_or_default();
        match status {
            ExecutionStatus::Success => stats.success_runs += 1,
            ExecutionStatus::Failed | ExecutionStatus::Timeout => stats.failed_runs += 1,
            _ => {}
        }

        let mut active: tapp_scheduled_tasks::ActiveModel = current.into();
        active.last_run_result = Set(Some(json!({
            "status": format!("{:?}", status).to_lowercase(),
            "result": result,
            "error": error,
        })));
        active.stats = Set(serde_json::to_value(&stats).unwrap_or(json!({})));
        active.updated_at = Set(Utc::now().into());
        active
            .update(&txn)
            .await
            .map_err(|e| format!("Failed to finalize frontend task stats: {}", e))?;
        txn.commit()
            .await
            .map_err(|e| format!("Failed to commit frontend stats: {e}"))?;
        Ok(())
    }

    /// 更新任务执行后的状态
    async fn update_task_after_execution(
        db: &DatabaseConnection,
        task: &tapp_scheduled_tasks::Model,
        status: &ExecutionStatus,
        result: Option<serde_json::Value>,
        error: Option<String>,
        advance_schedule: bool,
    ) -> Result<(), String> {
        let now = Utc::now();

        let txn = db
            .begin()
            .await
            .map_err(|e| format!("Failed to begin task stats update: {e}"))?;
        let current = tapp_scheduled_tasks::Entity::find_by_id(task.id)
            .lock_exclusive()
            .one(&txn)
            .await
            .map_err(|e| format!("Failed to lock task stats: {e}"))?
            .ok_or_else(|| "Scheduled task no longer exists".to_string())?;
        let mut stats: TaskStats =
            serde_json::from_value(current.stats.clone()).unwrap_or_default();
        stats.total_runs += 1;
        match status {
            ExecutionStatus::Success => stats.success_runs += 1,
            ExecutionStatus::Failed | ExecutionStatus::Timeout => stats.failed_runs += 1,
            _ => {}
        }

        // 计算下次执行时间
        let next_run_at = if advance_schedule {
            Self::calculate_next_run(&task.schedule_type, &task.schedule_config, now)?
        } else {
            current.next_run_at.map(|time| time.with_timezone(&Utc))
        };

        // 更新任务
        let mut active: tapp_scheduled_tasks::ActiveModel = current.into();
        active.last_run_at = Set(Some(now.into()));
        active.last_run_result = Set(Some(json!({
            "status": format!("{:?}", status),
            "result": result,
            "error": error,
        })));
        active.stats = Set(serde_json::to_value(&stats).unwrap_or(json!({})));
        active.next_run_at = Set(next_run_at.map(|t| t.into()));
        active.updated_at = Set(now.into());

        // 如果是 once 类型且已执行，禁用任务
        if advance_schedule && matches!(task.schedule_type, ScheduleType::Once) {
            active.enabled = Set(false);
        }

        active
            .update(&txn)
            .await
            .map_err(|e| format!("Failed to update task: {}", e))?;

        txn.commit()
            .await
            .map_err(|e| format!("Failed to commit task stats: {e}"))?;

        Ok(())
    }

    /// 计算下次执行时间
    pub fn calculate_next_run(
        schedule_type: &ScheduleType,
        schedule_config: &serde_json::Value,
        from: DateTime<Utc>,
    ) -> Result<Option<DateTime<Utc>>, String> {
        let config: ScheduleConfig = serde_json::from_value(schedule_config.clone())
            .map_err(|e| format!("Invalid schedule config: {}", e))?;

        match schedule_type {
            ScheduleType::Interval => {
                let interval_ms = config.interval.ok_or("Missing interval")?;
                let duration = Duration::milliseconds(interval_ms);
                Ok(Some(from + duration))
            }
            ScheduleType::Once => {
                let at_ms = config.at.ok_or("Missing at")?;
                let at = DateTime::from_timestamp_millis(at_ms).ok_or("Invalid timestamp")?;
                if at > from {
                    Ok(Some(at))
                } else {
                    Ok(None) // 已过期
                }
            }
            ScheduleType::Daily => {
                let time_str = config.time.ok_or("Missing time")?;
                let time = NaiveTime::parse_from_str(&time_str, "%H:%M")
                    .map_err(|e| format!("Invalid time format: {}", e))?;

                let today = from.date_naive();
                let today_run = today.and_time(time);
                let today_run_utc = Utc.from_utc_datetime(&today_run);

                if today_run_utc > from {
                    Ok(Some(today_run_utc))
                } else {
                    // 明天
                    let tomorrow = today + Duration::days(1);
                    let tomorrow_run = tomorrow.and_time(time);
                    Ok(Some(Utc.from_utc_datetime(&tomorrow_run)))
                }
            }
            ScheduleType::Cron => {
                let cron_str = config.cron.ok_or("Missing cron")?;
                let schedule = Schedule::from_str(&cron_str)
                    .map_err(|e| format!("Invalid cron expression: {}", e))?;

                let next = schedule.after(&from).next();
                Ok(next)
            }
        }
    }

    /// 注册新任务
    #[allow(clippy::too_many_arguments)]
    pub async fn register_task(
        &self,
        user_id: i32,
        tapp_id: &str,
        task_id: &str,
        name: &str,
        schedule_type: ScheduleType,
        schedule_config: serde_json::Value,
        payload: Option<serde_json::Value>,
        execution_target: ExecutionTarget,
        backend_actions: Option<serde_json::Value>,
        missed_policy: MissedPolicy,
        scope: TaskScope,
        retry_config: Option<serde_json::Value>,
    ) -> Result<tapp_scheduled_tasks::Model, String> {
        let now = Utc::now();

        // 检查是否已存在
        let existing = tapp_scheduled_tasks::Entity::find()
            .filter(tapp_scheduled_tasks::Column::UserId.eq(user_id))
            .filter(tapp_scheduled_tasks::Column::TappId.eq(tapp_id))
            .filter(tapp_scheduled_tasks::Column::TaskId.eq(task_id))
            .one(&self.db)
            .await
            .map_err(|e| format!("Query failed: {}", e))?;

        if existing.is_some() {
            return Err(format!("Task {} already exists", task_id));
        }

        // 计算首次执行时间
        let next_run_at = Self::calculate_next_run(&schedule_type, &schedule_config, now)?;

        let task = tapp_scheduled_tasks::ActiveModel {
            task_id: Set(task_id.to_string()),
            tapp_id: Set(tapp_id.to_string()),
            user_id: Set(user_id),
            name: Set(name.to_string()),
            schedule_type: Set(schedule_type),
            schedule_config: Set(schedule_config),
            payload: Set(payload),
            execution_target: Set(execution_target),
            backend_actions: Set(backend_actions),
            enabled: Set(true),
            missed_policy: Set(missed_policy),
            scope: Set(scope),
            retry_config: Set(retry_config),
            next_run_at: Set(next_run_at.map(|t| t.into())),
            stats: Set(json!({"totalRuns":0,"successRuns":0,"failedRuns":0,"missedRuns":0})),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            ..Default::default()
        };

        let task = task
            .insert(&self.db)
            .await
            .map_err(|e| format!("Insert failed: {}", e))?;

        tracing::info!(
            "[TappScheduler] Registered task {} for tapp {} (user {})",
            task_id,
            tapp_id,
            user_id
        );

        Ok(task)
    }

    /// 删除任务
    pub async fn unregister_task(
        &self,
        user_id: i32,
        tapp_id: &str,
        task_id: &str,
    ) -> Result<(), String> {
        let task = tapp_scheduled_tasks::Entity::find()
            .filter(tapp_scheduled_tasks::Column::UserId.eq(user_id))
            .filter(tapp_scheduled_tasks::Column::TappId.eq(tapp_id))
            .filter(tapp_scheduled_tasks::Column::TaskId.eq(task_id))
            .one(&self.db)
            .await
            .map_err(|e| format!("Query failed: {}", e))?
            .ok_or_else(|| format!("Task {} not found", task_id))?;

        // 删除执行历史
        tapp_task_executions::Entity::delete_many()
            .filter(tapp_task_executions::Column::ScheduledTaskId.eq(task.id))
            .exec(&self.db)
            .await
            .map_err(|e| format!("Failed to delete executions: {}", e))?;

        // 删除任务
        tapp_scheduled_tasks::Entity::delete_by_id(task.id)
            .exec(&self.db)
            .await
            .map_err(|e| format!("Delete failed: {}", e))?;

        tracing::info!(
            "[TappScheduler] Unregistered task {} for tapp {} (user {})",
            task_id,
            tapp_id,
            user_id
        );

        Ok(())
    }

    /// 获取任务列表
    pub async fn list_tasks(
        &self,
        user_id: i32,
        tapp_id: Option<&str>,
    ) -> Result<Vec<tapp_scheduled_tasks::Model>, String> {
        let mut query = tapp_scheduled_tasks::Entity::find()
            .filter(tapp_scheduled_tasks::Column::UserId.eq(user_id))
            .filter(tapp_scheduled_tasks::Column::TappId.ne(CORE_PLATFORM_SYNC_TAPP_ID));

        if let Some(tid) = tapp_id {
            query = query.filter(tapp_scheduled_tasks::Column::TappId.eq(tid));
        }

        query
            .order_by_asc(tapp_scheduled_tasks::Column::CreatedAt)
            .all(&self.db)
            .await
            .map_err(|e| format!("Query failed: {}", e))
    }

    /// 获取单个任务
    pub async fn get_task(
        &self,
        user_id: i32,
        tapp_id: &str,
        task_id: &str,
    ) -> Result<Option<tapp_scheduled_tasks::Model>, String> {
        tapp_scheduled_tasks::Entity::find()
            .filter(tapp_scheduled_tasks::Column::UserId.eq(user_id))
            .filter(tapp_scheduled_tasks::Column::TappId.ne(CORE_PLATFORM_SYNC_TAPP_ID))
            .filter(tapp_scheduled_tasks::Column::TappId.eq(tapp_id))
            .filter(tapp_scheduled_tasks::Column::TaskId.eq(task_id))
            .one(&self.db)
            .await
            .map_err(|e| format!("Query failed: {}", e))
    }

    /// 启用/禁用任务
    pub async fn set_task_enabled(
        &self,
        user_id: i32,
        tapp_id: &str,
        task_id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        let task = self
            .get_task(user_id, tapp_id, task_id)
            .await?
            .ok_or_else(|| format!("Task {} not found", task_id))?;

        let mut active: tapp_scheduled_tasks::ActiveModel = task.into();
        active.enabled = Set(enabled);
        active.updated_at = Set(Utc::now().into());

        active
            .update(&self.db)
            .await
            .map_err(|e| format!("Update failed: {}", e))?;

        Ok(())
    }

    /// 手动触发任务
    pub async fn trigger_task(
        &self,
        user_id: i32,
        tapp_id: &str,
        task_id: &str,
    ) -> Result<(), String> {
        let task = self
            .get_task(user_id, tapp_id, task_id)
            .await?
            .ok_or_else(|| format!("Task {} not found", task_id))?;

        Self::execute_task(&self.db, &task, false, 0, true).await
    }
}

/// 实现 ToString 用于 ExecutionTarget
impl std::fmt::Display for ExecutionTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionTarget::Backend => write!(f, "backend"),
            ExecutionTarget::Frontend => write!(f, "frontend"),
            ExecutionTarget::Both => write!(f, "both"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_message_matches_scheduler_client_contract() {
        let message = FrontendTaskMessage {
            msg_type: "task:execute".to_string(),
            task: TaskExecutionContext {
                id: 7,
                task_id: "refresh".to_string(),
                tapp_id: "com.example.app".to_string(),
                user_id: 42,
                scope: "user".to_string(),
                scheduled_at: 1000,
                executed_at: 1100,
                is_compensation: false,
                payload: Some(json!({ "source": "timer" })),
            },
            payload: Some(json!({ "source": "timer" })),
            scheduled_at: "2026-07-10T00:00:00Z".to_string(),
            execution_id: 99,
            target_users: Some(vec![42]),
        };

        let value = serde_json::to_value(message).expect("message should serialize");
        assert_eq!(value["type"], "task:execute");
        assert_eq!(value["task"]["taskId"], "refresh");
        assert_eq!(value["task"]["tappId"], "com.example.app");
        assert_eq!(value["executionId"], 99);
        assert_eq!(value["payload"]["source"], "timer");
        assert!(value.get("execution_id").is_none());
    }

    #[test]
    fn shared_delivery_uses_subject_scoped_mailboxes() {
        assert_eq!(
            scheduler_mailbox_recipient("scheduler_ws_abc"),
            "connection:scheduler_ws_abc"
        );
    }

    #[test]
    fn interval_next_run_uses_milliseconds() {
        let from = DateTime::from_timestamp_millis(1_000_000).unwrap();
        let next = TappSchedulerEngine::calculate_next_run(
            &ScheduleType::Interval,
            &json!({ "interval": 30_000 }),
            from,
        )
        .expect("valid interval")
        .expect("next run");

        assert_eq!(next.timestamp_millis(), 1_030_000);
    }

    #[test]
    fn backend_action_pipeline_is_bounded_for_recovery_lease() {
        let actions = (0..=MAX_SCHEDULER_BACKEND_ACTIONS)
            .map(|index| {
                json!({
                    "type": "storage.set",
                    "key": format!("key{index}"),
                    "value": index,
                })
            })
            .collect::<Vec<_>>();

        assert!(normalize_backend_actions(Some(json!(actions))).is_err());
    }

    #[test]
    fn scheduled_ai_requires_matching_manifest_v2_declaration() {
        let actions = normalize_backend_actions(Some(json!([{
            "type": "ai.generate",
            "prompt": "Summarize {{input}}"
        }])))
        .unwrap();
        let without_ai = json!({
            "permissions": ["scheduler:register", "ai:generate"]
        });
        assert!(validate_backend_action_declarations(&without_ai, &actions).is_err());

        let declared = json!({
            "permissions": ["scheduler:register", "ai:generate"],
            "ai": {
                "protocolVersion": 2,
                "operations": ["generate"],
                "modelTier": "pro",
                "contextSources": [],
                "outputFormats": ["text"]
            }
        });
        assert_eq!(
            validate_backend_action_declarations(&declared, &actions).unwrap(),
            Some(ModelTier::Pro)
        );
    }
}
