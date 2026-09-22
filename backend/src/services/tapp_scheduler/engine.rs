// TappSchedulerEngine implementation: tick loop, dispatch, backend actions.

use chrono::{DateTime, Duration, FixedOffset, Local, NaiveTime, TimeZone, Utc};
use cron::Schedule;
use sea_orm::{
    ActiveModelTrait,
    ActiveValue::Set,
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, FromQueryResult,
    QueryFilter, QueryOrder, QuerySelect, Statement, TransactionTrait,
    sea_query::{Expr, OnConflict},
};
use serde_json::json;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::Ordering;
use std::sync::{Arc, LazyLock};
use tokio::sync::RwLock;

use crate::services::agent::ai_process_pure::USER_TEXT_MAX_CHARS;
use crate::services::tapp_registry::{self as shared_registry};
use crate::services::tapp_storage::{
    read_storage_value, validate_sandbox_storage_key, validate_storage_value_size,
    write_storage_value,
};
use myriad_tapp_contract::manifest::TappAiOperation;

use crate::GLOBAL_DYNAMIC_CONFIG;
use crate::models::entities::tapp_scheduled_tasks::{
    self, BackendAction, BackendActionWrapper, ExecutionTarget, MissedPolicy, RetryConfig,
    ScheduleConfig, ScheduleType, TaskScope, TaskStats,
};
use crate::models::entities::tapp_task_executions::{self, ExecutionStatus};
use crate::services::permission_service::{TappPermission, TappPermissionService, UserRole};
use crate::services::platform_auto_refresh::{
    CORE_PLATFORM_SYNC_TAPP_ID, core_platform_from_task, is_core_platform_sync_task,
};

use super::types_frontend::*;

fn scheduler_store_failed(context: &'static str, error: impl std::fmt::Display) -> String {
    tracing::error!(%error, context, "scheduler store failed");
    format!("Failed to {context}")
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

        // Freeze who is due this round. Claim one at a time so a long first
        // run cannot start later leases from this same timestamp.
        let due_cutoff = now;
        loop {
            let Some(task) = Self::claim_next_due_task(db, due_cutoff).await? else {
                break;
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

    /// Claim the next due row for this round. `next_run_at` doubles as a
    /// recovery lease: another replica cannot run the same occurrence, while a
    /// crashed worker makes the task eligible again after the lease expires.
    ///
    /// `due_cutoff` is the tick's due set. The lease itself starts at claim time
    /// so a long earlier task does not eat later tasks' recovery window.
    async fn claim_next_due_task(
        db: &DatabaseConnection,
        due_cutoff: DateTime<Utc>,
    ) -> Result<Option<tapp_scheduled_tasks::Model>, String> {
        let claim_now = Utc::now();
        let txn = db
            .begin()
            .await
            .map_err(|error| scheduler_store_failed("begin claim", error))?;
        let current = tapp_scheduled_tasks::Model::find_by_statement(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT * FROM tapp_scheduled_tasks
               WHERE enabled = TRUE
                 AND next_run_at IS NOT NULL
                 AND next_run_at <= $1
               ORDER BY next_run_at ASC
               LIMIT 1
               FOR UPDATE SKIP LOCKED"#,
            [due_cutoff.into()],
        ))
        .one(&txn)
        .await
        .map_err(|error| scheduler_store_failed("lock due task", error))?;
        let Some(current) = current else {
            txn.rollback().await.ok();
            return Ok(None);
        };

        let lease_duration = Self::recovery_lease_duration(&current, due_cutoff);
        let mut active: tapp_scheduled_tasks::ActiveModel = current.clone().into();
        active.next_run_at = Set(Some((claim_now + lease_duration).into()));
        active.updated_at = Set(claim_now.into());
        active
            .update(&txn)
            .await
            .map_err(|error| scheduler_store_failed("claim due task", error))?;
        txn.commit()
            .await
            .map_err(|error| scheduler_store_failed("commit claim", error))?;
        Ok(Some(current))
    }

    /// Size the crash-recovery lease from retry_config and backend_actions (retries/delay/action count clamped; plus compensation and the 5-minute frontend timeout).
    /// `max_retries` / `retry_delay` are clamped to `MAX_SCHEDULER_RETRIES` /
    /// `MAX_SCHEDULER_RETRY_DELAY_MS`.
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
            .map_err(|error| scheduler_store_failed("begin missed stats", error))?;
        let current = tapp_scheduled_tasks::Entity::find_by_id(task.id)
            .lock_exclusive()
            .one(&txn)
            .await
            .map_err(|error| scheduler_store_failed("lock missed stats", error))?
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
            .map_err(|error| scheduler_store_failed("update missed stats", error))?;
        txn.commit()
            .await
            .map_err(|error| scheduler_store_failed("commit missed stats", error))?;
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

        tracing::error!(
            task_id = %task.task_id,
            attempts = max_retries + 1,
            %last_error,
            "scheduled task failed after retries"
        );
        Err("Scheduled task failed".to_string())
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
            .map_err(|error| scheduler_store_failed("create execution", error))?;

        let mut result: Option<serde_json::Value> = None;
        let mut awaiting_frontend = false;
        let start_time = std::time::Instant::now();
        let (mut status, mut error, wrappers, authority) =
            match parse_backend_action_wrappers(&task.backend_actions) {
                Err(error) => (ExecutionStatus::Failed, Some(error), Vec::new(), None),
                Ok(wrappers) => {
                    match Self::validate_task_execution_permissions(db, task, &wrappers).await {
                        Ok(authority) => {
                            (ExecutionStatus::Success, None, wrappers, Some(authority))
                        }
                        Err(error) => (ExecutionStatus::Failed, Some(error), wrappers, None),
                    }
                }
            };

        // 根据执行目标处理
        match task.execution_target {
            ExecutionTarget::Backend | ExecutionTarget::Both => {
                // 执行后端操作
                if status == ExecutionStatus::Success && task.backend_actions.is_some() {
                    match Self::execute_backend_actions(
                        db,
                        task,
                        &wrappers,
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
                    // target_users=None；入队时 `can_receive_frontend_task` 过滤
                    ("tapp".to_string(), None)
                }
                TaskScope::TappPerUser => {
                    // 只推送给注册任务的用户
                    ("tapp-per-user".to_string(), Some(vec![task.user_id]))
                }
                TaskScope::Global => {
                    // target_users=None；入队时 can_receive_frontend_task 过滤
                    ("global".to_string(), None)
                }
            };

            let context = TaskExecutionContext {
                id: task.id,
                task_id: task.task_id.clone(),
                tapp_id: task.tapp_id.clone(),
                user_id: task.user_id,
                scope: scope_str,
                // Same unit/format as outer message.scheduled_at (RFC3339)
                scheduled_at: scheduled_at.to_rfc3339(),
                executed_at: now.to_rfc3339(),
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
            // 保持 running 并推进 next_run；终态由 task:complete 或 5 分钟超时落库。
            let mut execution_update: tapp_task_executions::ActiveModel = execution.into();
            execution_update.result = Set(result.clone());
            execution_update
                .update(db)
                .await
                .map_err(|error| scheduler_store_failed("keep frontend pending", error))?;

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
            .map_err(|error| scheduler_store_failed("update execution", error))?;

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
                .map_err(|error| scheduler_store_failed("list connections", error))?;
        let mut deliveries = 0usize;
        let mut first_error = None;
        let mut audience = HashMap::<i32, bool>::new();
        for connection in active_connections {
            let user_id = connection.subject_id;
            if message
                .target_users
                .as_ref()
                .is_some_and(|users| !users.contains(&user_id))
            {
                continue;
            }
            let allowed = if let Some(&allowed) = audience.get(&user_id) {
                allowed
            } else {
                let allowed = Self::can_receive_frontend_task(db, user_id, task).await?;
                audience.insert(user_id, allowed);
                allowed
            };
            if !allowed {
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
                    first_error
                        .get_or_insert_with(|| scheduler_store_failed("enqueue task", error));
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
                    .query_one_raw(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        "SELECT is_admin FROM users WHERE id = $1 LIMIT 1",
                        [user_id.into()],
                    ))
                    .await
                    .map_err(|error| scheduler_store_failed("verify global audience", error))?;
                Ok(row
                    .and_then(|row| row.try_get::<bool>("", "is_admin").ok())
                    .unwrap_or(false))
            }
            TaskScope::Tapp => {
                let row = db
                    .query_one_raw(Statement::from_sql_and_values(
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
                    .map_err(|error| scheduler_store_failed("verify tapp audience", error))?;
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
            .map_err(|error| scheduler_store_failed("query frontend execution", error))?
            .ok_or_else(|| format!("Execution {} not found", execution_id))?;

        if execution.status != ExecutionStatus::Running {
            return Ok(());
        }

        let task = tapp_scheduled_tasks::Entity::find_by_id(execution.scheduled_task_id)
            .one(&self.db)
            .await
            .map_err(|error| scheduler_store_failed("query scheduled task", error))?
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
            .map_err(|error| scheduler_store_failed("query stale frontend executions", error))?;

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

        let now = Utc::now();
        let executed_at = execution.executed_at.with_timezone(&Utc);
        let duration_ms = (now - executed_at)
            .num_milliseconds()
            .clamp(0, i32::MAX as i64) as i32;
        let result = execution.result;

        // Multiple Tapp-scope clients may acknowledge the same broadcast. Claim
        // the running row atomically so task stats are finalized exactly once.
        let txn = db
            .begin()
            .await
            .map_err(|error| scheduler_store_failed("begin frontend completion", error))?;
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
            .exec(&txn)
            .await
            .map_err(|error| scheduler_store_failed("finalize frontend execution", error))?;
        if update.rows_affected == 0 {
            txn.rollback().await.ok();
            return Ok(());
        }

        let task = Self::update_task_after_frontend_completion(
            &txn,
            execution.scheduled_task_id,
            &status,
            result,
            error.clone(),
        )
        .await?;
        txn.commit()
            .await
            .map_err(|error| scheduler_store_failed("commit frontend completion", error))?;

        SCHEDULER_COMPLETED.fetch_add(1, Ordering::Relaxed);
        if status == ExecutionStatus::Timeout {
            SCHEDULER_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
        }
        if matches!(status, ExecutionStatus::Failed | ExecutionStatus::Timeout) {
            Self::notify_task_failure(
                &task,
                error.as_deref().unwrap_or("The scheduled task failed"),
            )
            .await;
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
                    Some("Scheduled task failed"),
                    error,
                    "error",
                )
                .await;
        }
    }

    /// 每次真正执行前重新读取当前角色和动态权限。
    /// 不能沿用注册时的角色；执行前重算授予，并再与安装批准集求交。
    async fn validate_task_execution_permissions(
        db: &DatabaseConnection,
        task: &tapp_scheduled_tasks::Model,
        wrappers: &[BackendActionWrapper],
    ) -> Result<ScheduledExecutionAuthority, String> {
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT is_admin FROM users WHERE id = $1 LIMIT 1",
                [task.user_id.into()],
            ))
            .await
            .map_err(|error| scheduler_store_failed("verify user role", error))?
            .ok_or_else(|| "Scheduler user no longer exists".to_string())?;
        let is_admin = row
            .try_get::<bool>("", "is_admin")
            .map_err(|error| scheduler_store_failed("read user role", error))?;
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
        required.extend(backend_action_permissions_of(wrappers));

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

        let tapp = crate::services::tapp_ownership::resolve_accessible_tapp(
            db,
            task.user_id,
            &task.tapp_id,
        )
        .await
        .map_err(|error| {
            tracing::error!(%error, "scheduled tapp is no longer accessible");
            "Scheduled Tapp is no longer accessible".to_string()
        })?;
        crate::services::tapp_runtime_grant::refuse_if_needs_reauthorization(
            tapp.needs_reauthorization,
        )
        .map_err(|error| error.message())?;
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
        let ai_model_tier = validate_backend_action_declarations_of(&tapp.manifest, wrappers)?;
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
        action_wrappers: &[BackendActionWrapper],
        authority: &ScheduledExecutionAuthority,
    ) -> Result<serde_json::Value, String> {
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
                    // 后端动作是串行流水线；失败即 `return Err`，不让后续模板读到错误上下文。
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
            } => Ok(Self::action_transform(
                context,
                input,
                extract.as_deref(),
                template.as_deref(),
            )),
        }
    }

    /// 解析字符串中的模板变量 {{varName}} 或 {{varName.field}}
    fn resolve_template(template: &str, context: &HashMap<String, serde_json::Value>) -> String {
        Self::fill_template(template, context, None)
    }

    fn resolve_template_with_input(
        template: &str,
        context: &HashMap<String, serde_json::Value>,
        input: &serde_json::Value,
    ) -> String {
        Self::fill_template(template, context, Some(("_input", input)))
    }

    fn fill_template(
        template: &str,
        context: &HashMap<String, serde_json::Value>,
        overlay: Option<(&str, &serde_json::Value)>,
    ) -> String {
        static TEMPLATE_VAR: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(r"\{\{([^}]+)\}\}").expect("scheduler template pattern")
        });
        TEMPLATE_VAR
            .replace_all(template, |caps: &regex::Captures| {
                let path = caps.get(1).map_or("", |m| m.as_str()).trim();
                Self::get_value_by_path(context, path, overlay)
            })
            .to_string()
    }

    fn json_null() -> &'static serde_json::Value {
        static NULL: serde_json::Value = serde_json::Value::Null;
        &NULL
    }

    fn lookup_json_path<'a>(root: &'a serde_json::Value, parts: &[&str]) -> &'a serde_json::Value {
        let mut current = root;
        for part in parts {
            current = match current {
                serde_json::Value::Object(map) => map.get(*part).unwrap_or(Self::json_null()),
                serde_json::Value::Array(arr) => part
                    .parse::<usize>()
                    .ok()
                    .and_then(|idx| arr.get(idx))
                    .unwrap_or(Self::json_null()),
                _ => Self::json_null(),
            };
        }
        current
    }

    fn json_to_template_string(value: &serde_json::Value) -> String {
        match value {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Null => String::new(),
            other => other.to_string(),
        }
    }

    /// 根据路径获取值，支持 varName.field.subfield
    fn get_value_by_path(
        context: &HashMap<String, serde_json::Value>,
        path: &str,
        overlay: Option<(&str, &serde_json::Value)>,
    ) -> String {
        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() {
            return String::new();
        }

        let root = if overlay.is_some_and(|(name, _)| name == parts[0]) {
            overlay.unwrap().1
        } else {
            let Some(value) = context.get(parts[0]) else {
                return format!("{{{{{}}}}}", path);
            };
            value
        };
        Self::json_to_template_string(Self::lookup_json_path(root, &parts[1..]))
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
                        if let Some(root) = context.get(parts[0]) {
                            let mut current = root;
                            for part in &parts[1..] {
                                current = match current {
                                    serde_json::Value::Object(map) => {
                                        map.get(*part).unwrap_or(Self::json_null())
                                    }
                                    _ => Self::json_null(),
                                };
                            }
                            return current.clone();
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
    ) -> serde_json::Value {
        // 获取输入值
        let input_value = context.get(input).unwrap_or(Self::json_null());
        let extracted = if let Some(path) = extract {
            let parts: Vec<&str> = path.split('.').collect();
            Self::lookup_json_path(input_value, &parts).clone()
        } else {
            input_value.clone()
        };

        if let Some(tpl) = template {
            serde_json::Value::String(Self::resolve_template_with_input(tpl, context, &extracted))
        } else {
            extracted
        }
    }

    /// 执行平台同步
    async fn action_platform_sync(
        db: &DatabaseConnection,
        platform: &str,
    ) -> Result<serde_json::Value, String> {
        tracing::info!("[TappScheduler] Platform sync: {}", platform);
        let data =
            crate::services::platform_refresh::refresh_platform_for_scheduler(db, platform).await?;
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
            .map_err(|error| scheduler_store_failed("delete storage", error))?;

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
        if prompt.chars().count() > USER_TEXT_MAX_CHARS {
            return Err(format!(
                "Prompt too long (max {USER_TEXT_MAX_CHARS} characters)"
            ));
        }
        if let Some(reason) = myriad_prompt_security::validate_prompt_security(prompt) {
            return Err(format!("Prompt contains disallowed content: {reason}"));
        }
        tracing::info!(
            task_id = %task.task_id,
            prompt_chars = prompt.chars().count(),
            "[TappScheduler] AI generate"
        );
        let tier = authority
            .ai_model_tier
            .ok_or_else(|| "Scheduled AI action has no validated Manifest tier".to_string())?;
        let text = crate::services::governed_text::execute_governed_text(
            db,
            crate::services::governed_text::GovernedTextRequest {
                role: authority.role,
                subject_id: task.user_id,
                owner_id: authority.owner_id,
                tapp_id: task.tapp_id.clone(),
                source: "scheduler".to_string(),
                operation: TappAiOperation::Generate,
                tier,
                system_prompt: "You are executing a background task for a sandboxed Tapp. Do not reveal system information, execute code, or access external URLs. Return concise text only.".to_string(),
                prompt: prompt.to_string(),
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

        let response = request.send().await.map_err(|e| {
            tracing::warn!(error = %e, "Scheduled fetch failed");
            "Fetch failed".to_string()
        })?;

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
            .map_err(|error| scheduler_store_failed("begin frontend dispatch", error))?;
        let current = tapp_scheduled_tasks::Entity::find_by_id(task.id)
            .lock_exclusive()
            .one(&txn)
            .await
            .map_err(|error| scheduler_store_failed("lock frontend dispatch", error))?
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
            .map_err(|error| scheduler_store_failed("advance frontend task", error))?;
        txn.commit()
            .await
            .map_err(|error| scheduler_store_failed("commit frontend dispatch", error))?;
        Ok(())
    }

    /// 前端回执只补齐最终状态；total_runs 已在 dispatch 时增加。
    async fn update_task_after_frontend_completion(
        db: &impl ConnectionTrait,
        task_id: i32,
        status: &ExecutionStatus,
        result: Option<serde_json::Value>,
        error: Option<String>,
    ) -> Result<tapp_scheduled_tasks::Model, String> {
        let current = tapp_scheduled_tasks::Entity::find_by_id(task_id)
            .lock_exclusive()
            .one(db)
            .await
            .map_err(|error| scheduler_store_failed("lock frontend stats", error))?
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
            .update(db)
            .await
            .map_err(|error| scheduler_store_failed("finalize frontend task stats", error))
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
            .map_err(|error| scheduler_store_failed("begin task stats", error))?;
        let current = tapp_scheduled_tasks::Entity::find_by_id(task.id)
            .lock_exclusive()
            .one(&txn)
            .await
            .map_err(|error| scheduler_store_failed("lock task stats", error))?
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
            .map_err(|error| scheduler_store_failed("update task", error))?;

        txn.commit()
            .await
            .map_err(|error| scheduler_store_failed("commit task stats", error))?;

        Ok(())
    }

    /// 计算下次执行时间
    pub fn calculate_next_run(
        schedule_type: &ScheduleType,
        schedule_config: &serde_json::Value,
        from: DateTime<Utc>,
    ) -> Result<Option<DateTime<Utc>>, String> {
        let config: ScheduleConfig =
            serde_json::from_value(schedule_config.clone()).map_err(|error| {
                tracing::error!(%error, "Invalid schedule config");
                "Invalid schedule config".to_string()
            })?;

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
                let time = NaiveTime::parse_from_str(&time_str, "%H:%M").map_err(|error| {
                    tracing::warn!(%error, time = %time_str, "invalid daily time");
                    format!("Invalid time format (use HH:mm): {time_str}")
                })?;
                // Wall clock: process local TZ by default (TZ env / container),
                // not UTC — matches "每天上午 9 点" docs and operator intuition.
                let next = daily_next_wall_clock(time, from, config.timezone.as_deref())?;
                Ok(Some(next))
            }
            ScheduleType::Cron => {
                let cron_str = config.cron.ok_or("Missing cron")?;
                let schedule = Schedule::from_str(&cron_str).map_err(|error| {
                    tracing::warn!(%error, cron = %cron_str, "invalid cron expression");
                    format!("Invalid cron expression: {cron_str}")
                })?;

                let next = schedule.after(&from).next();
                Ok(next)
            }
        }
    }
}

/// Parse `UTC`, `Z`, `+08:00`, `-05:00`, `UTC+8`, `UTC+08:00`.
fn parse_daily_fixed_offset(raw: &str) -> Result<FixedOffset, String> {
    let s = raw.trim();
    if s.eq_ignore_ascii_case("utc") || s.eq_ignore_ascii_case("z") {
        return FixedOffset::east_opt(0).ok_or_else(|| "Invalid UTC offset".into());
    }
    let body = s
        .strip_prefix("UTC")
        .or_else(|| s.strip_prefix("utc"))
        .or_else(|| s.strip_prefix("Gmt"))
        .or_else(|| s.strip_prefix("GMT"))
        .unwrap_or(s)
        .trim();
    // +HH:MM / -HH:MM / +H / +HH
    let (sign, rest) = if let Some(r) = body.strip_prefix('+') {
        (1i32, r)
    } else if let Some(r) = body.strip_prefix('-') {
        (-1i32, r)
    } else {
        return Err(format!(
            "Invalid timezone '{raw}': use local, UTC, or fixed offset like +08:00"
        ));
    };
    let rest = rest.trim();
    let (hh, mm) = if let Some((h, m)) = rest.split_once(':') {
        (
            h.parse::<i32>()
                .map_err(|_| format!("Invalid timezone hour in '{raw}'"))?,
            m.parse::<i32>()
                .map_err(|_| format!("Invalid timezone minute in '{raw}'"))?,
        )
    } else {
        let h = rest
            .parse::<i32>()
            .map_err(|_| format!("Invalid timezone hour in '{raw}'"))?;
        (h, 0)
    };
    if !(0..=14).contains(&hh) || !(0..60).contains(&mm) {
        return Err(format!("Timezone offset out of range: '{raw}'"));
    }
    let secs = sign * (hh * 3600 + mm * 60);
    FixedOffset::east_opt(secs).ok_or_else(|| format!("Invalid timezone offset: '{raw}'"))
}

/// Next daily fire after `from` for HH:mm wall clock in the given zone.
fn daily_next_wall_clock(
    time: NaiveTime,
    from: DateTime<Utc>,
    timezone: Option<&str>,
) -> Result<DateTime<Utc>, String> {
    let tz = timezone
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("local");

    if tz.eq_ignore_ascii_case("local") {
        return daily_next_in_local(time, from);
    }
    if tz.eq_ignore_ascii_case("utc") || tz.eq_ignore_ascii_case("z") {
        return Ok(daily_next_in_offset(
            time,
            from,
            FixedOffset::east_opt(0).unwrap(),
        ));
    }
    let offset = parse_daily_fixed_offset(tz)?;
    Ok(daily_next_in_offset(time, from, offset))
}

fn daily_next_in_local(time: NaiveTime, from: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
    let from_local = from.with_timezone(&Local);
    let today = from_local.date_naive();
    for day_offset in 0i64..2 {
        let day = today + Duration::days(day_offset);
        let naive = day.and_time(time);
        let local_dt = match Local.from_local_datetime(&naive) {
            chrono::LocalResult::Single(dt) => dt,
            chrono::LocalResult::Ambiguous(earliest, _latest) => earliest,
            chrono::LocalResult::None => continue, // skipped DST gap
        };
        if local_dt > from_local {
            return Ok(local_dt.with_timezone(&Utc));
        }
    }
    // Fallback: +2 days (should be unreachable for normal TZ)
    let day = today + Duration::days(2);
    let naive = day.and_time(time);
    Local
        .from_local_datetime(&naive)
        .single()
        .or_else(|| Local.from_local_datetime(&naive).earliest())
        .map(|dt| dt.with_timezone(&Utc))
        .ok_or_else(|| "Could not resolve local daily schedule time".into())
}

fn daily_next_in_offset(
    time: NaiveTime,
    from: DateTime<Utc>,
    offset: FixedOffset,
) -> DateTime<Utc> {
    let from_local = from.with_timezone(&offset);
    let today = from_local.date_naive();
    for day_offset in 0i64..2 {
        let day = today + Duration::days(day_offset);
        let naive = day.and_time(time);
        if let Some(local_dt) = offset.from_local_datetime(&naive).single() {
            if local_dt > from_local {
                return local_dt.with_timezone(&Utc);
            }
        }
    }
    let day = today + Duration::days(2);
    let naive = day.and_time(time);
    offset
        .from_local_datetime(&naive)
        .single()
        .unwrap_or_else(|| offset.from_utc_datetime(&naive))
        .with_timezone(&Utc)
}

// keep impl methods that follow (register_task etc.) in a separate impl block
impl TappSchedulerEngine {
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

        let on_conflict = OnConflict::columns([
            tapp_scheduled_tasks::Column::UserId,
            tapp_scheduled_tasks::Column::TappId,
            tapp_scheduled_tasks::Column::TaskId,
        ])
        .do_nothing()
        .to_owned();
        let task = match tapp_scheduled_tasks::Entity::insert(task)
            .on_conflict(on_conflict)
            .exec_with_returning(&self.db)
            .await
        {
            Ok(task) => {
                tracing::info!(
                    "[TappScheduler] Registered task {} for tapp {} (user {})",
                    task_id,
                    tapp_id,
                    user_id
                );
                task
            }
            Err(sea_orm::DbErr::RecordNotInserted) => {
                tracing::debug!(
                    "[TappScheduler] Task {} already registered for tapp {} (user {}) — idempotent reuse",
                    task_id,
                    tapp_id,
                    user_id
                );
                tapp_scheduled_tasks::Entity::find()
                    .filter(tapp_scheduled_tasks::Column::UserId.eq(user_id))
                    .filter(tapp_scheduled_tasks::Column::TappId.eq(tapp_id))
                    .filter(tapp_scheduled_tasks::Column::TaskId.eq(task_id))
                    .one(&self.db)
                    .await
                    .map_err(|error| scheduler_store_failed("load scheduled task", error))?
                    .ok_or_else(|| "Failed to create scheduled task".to_string())?
            }
            Err(error) => {
                return Err(scheduler_store_failed("create scheduled task", error));
            }
        };
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
            .map_err(|error| scheduler_store_failed("find scheduled task", error))?
            .ok_or_else(|| format!("Task {} not found", task_id))?;

        // 删除执行历史
        tapp_task_executions::Entity::delete_many()
            .filter(tapp_task_executions::Column::ScheduledTaskId.eq(task.id))
            .exec(&self.db)
            .await
            .map_err(|error| scheduler_store_failed("delete executions", error))?;

        // 删除任务
        tapp_scheduled_tasks::Entity::delete_by_id(task.id)
            .exec(&self.db)
            .await
            .map_err(|error| scheduler_store_failed("delete scheduled task", error))?;

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
            .map_err(|error| scheduler_store_failed("list scheduled tasks", error))
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
            .map_err(|error| scheduler_store_failed("load scheduled task", error))
    }

    /// 启用/禁用任务
    pub async fn set_task_enabled(
        &self,
        user_id: i32,
        tapp_id: &str,
        task_id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        let result = tapp_scheduled_tasks::Entity::update_many()
            .col_expr(tapp_scheduled_tasks::Column::Enabled, Expr::value(enabled))
            .col_expr(
                tapp_scheduled_tasks::Column::UpdatedAt,
                Expr::value(Utc::now().fixed_offset()),
            )
            .filter(tapp_scheduled_tasks::Column::UserId.eq(user_id))
            .filter(tapp_scheduled_tasks::Column::TappId.eq(tapp_id))
            .filter(tapp_scheduled_tasks::Column::TappId.ne(CORE_PLATFORM_SYNC_TAPP_ID))
            .filter(tapp_scheduled_tasks::Column::TaskId.eq(task_id))
            .exec(&self.db)
            .await
            .map_err(|error| scheduler_store_failed("update scheduled task", error))?;
        if result.rows_affected == 0 {
            return Err(format!("Task {} not found", task_id));
        }
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

#[cfg(test)]
mod claim_contract_tests {
    #[test]
    fn tick_claims_one_due_row_at_claim_time() {
        let src = include_str!("engine.rs");
        let tick = src
            .split("async fn tick(")
            .nth(1)
            .and_then(|rest| rest.split("async fn claim_next_due_task").next())
            .expect("tick");
        assert!(tick.contains("due_cutoff"));
        assert!(
            !tick.contains(".all(db)"),
            "tick must not materialize every due task before claiming"
        );
        let claim = src
            .split("async fn claim_next_due_task")
            .nth(1)
            .and_then(|rest| rest.split("fn recovery_lease_duration").next())
            .expect("claim");
        assert!(claim.contains("FOR UPDATE SKIP LOCKED"));
        assert!(claim.contains("claim_now"));
        assert!(claim.contains("claim_now + lease_duration"));
        assert!(claim.contains("due_cutoff"));
    }
}

#[cfg(test)]
mod frontend_finalizer_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires a disposable MYRIAD_RUNTIME_ISOLATION_TEST_DB"]
    async fn frontend_finalizer_rolls_back_stats_failure_and_counts_one_winner() {
        use sea_orm::{ConnectOptions, Database, DbBackend, Schema};
        let url = std::env::var("MYRIAD_RUNTIME_ISOLATION_TEST_DB").unwrap();
        let admin = Database::connect(&url).await.unwrap();
        let schema_name = format!("finalizer_{}", uuid::Uuid::new_v4().simple());
        admin
            .execute_unprepared(&format!("CREATE SCHEMA {schema_name}"))
            .await
            .unwrap();
        let mut options = ConnectOptions::new(url);
        options.max_connections(4).map_sqlx_postgres_opts({
            let schema_name = schema_name.clone();
            move |options| options.options([("search_path", schema_name.as_str())])
        });
        let db = Database::connect(options).await.unwrap();
        let schema = Schema::new(DbBackend::Postgres);
        db.execute(&schema.create_table_from_entity(tapp_scheduled_tasks::Entity))
            .await
            .unwrap();
        db.execute(&schema.create_table_from_entity(tapp_task_executions::Entity))
            .await
            .unwrap();
        db.execute_unprepared("INSERT INTO tapp_scheduled_tasks
            (id, task_id, tapp_id, user_id, name, schedule_type, schedule_config, execution_target,
             enabled, missed_policy, scope, stats, created_at, updated_at)
            VALUES (1, 'test', 'test', 1, 'Test', 'interval', '{}', 'frontend', true, 'skip', 'user',
                    '{\"totalRuns\":1,\"successRuns\":0,\"failedRuns\":0}', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP);
            INSERT INTO tapp_task_executions
            (id, scheduled_task_id, user_id, tapp_id, task_id, scheduled_at, executed_at, execution_target, status, is_compensation, retry_count)
            VALUES (1, 1, 1, 'test', 'test', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, 'frontend', 'running', false, 0);
            ALTER TABLE tapp_scheduled_tasks ADD CONSTRAINT reject_stats CHECK ((stats->>'successRuns')::int = 0)")
            .await.unwrap();
        let execution = tapp_task_executions::Entity::find_by_id(1)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert!(
            TappSchedulerEngine::finalize_frontend_execution(
                &db,
                execution.clone(),
                ExecutionStatus::Success,
                None
            )
            .await
            .is_err()
        );
        assert_eq!(
            tapp_task_executions::Entity::find_by_id(1)
                .one(&db)
                .await
                .unwrap()
                .unwrap()
                .status,
            ExecutionStatus::Running
        );
        db.execute_unprepared("ALTER TABLE tapp_scheduled_tasks DROP CONSTRAINT reject_stats")
            .await
            .unwrap();
        let (receipt, timeout) = tokio::join!(
            TappSchedulerEngine::finalize_frontend_execution(
                &db,
                execution.clone(),
                ExecutionStatus::Success,
                None
            ),
            TappSchedulerEngine::finalize_frontend_execution(
                &db,
                execution.clone(),
                ExecutionStatus::Timeout,
                Some("timeout".into())
            ),
        );
        receipt.unwrap();
        timeout.unwrap();
        TappSchedulerEngine::finalize_frontend_execution(&db, execution, ExecutionStatus::Success, None)
            .await
            .unwrap();
        let task = tapp_scheduled_tasks::Entity::find_by_id(1)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let stats: TaskStats = serde_json::from_value(task.stats).unwrap();
        assert_eq!(stats.total_runs, 1);
        assert_eq!(stats.success_runs + stats.failed_runs, 1);
        admin
            .execute_unprepared(&format!("DROP SCHEMA {schema_name} CASCADE"))
            .await
            .unwrap();
    }
}
