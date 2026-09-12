//! Heartbeat 主动式 Agent
//!
//! 基于 cron 调度的定时任务系统，让 Agent 可以主动执行任务。
//! 任务定义在 Agent 数据目录的 `HEARTBEAT.md`（DataPaths），支持热加载。
//!
//! Cron 匹配按**服务器本地时间**，支持标准 5 字段语法：
//! `*`、数字、列表 `a,b,c`、区间 `a-b`、步进 `*/n` / `a-b/n` / `a/n`。
//! 星期字段 0 和 7 均表示周日；日/星期同时受限时按标准 cron 语义取"或"。
//!
//! 多副本：通过 `heartbeat_claims` 表按 (task_id, minute_bucket) CAS 认领，
//! 避免同一分钟被多个 backend 重复执行。崩溃后超过 STALE 窗口可重认领。

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Datelike, Local, Timelike, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// 单次 heartbeat 任务墙钟超时（秒）
pub const HEARTBEAT_TASK_TIMEOUT_SECS: u64 = 600;

/// 认领卡住后允许重认领的阈值（秒），略长于执行超时
const CLAIM_STALE_SECS: i64 = 900;

/// 当前进行中的 heartbeat 执行数（关机 drain 用）
static HEARTBEAT_INFLIGHT: AtomicUsize = AtomicUsize::new(0);

/// RAII：进入/离开 in-flight 计数
pub struct HeartbeatInflightGuard;

impl HeartbeatInflightGuard {
    pub fn enter() -> Self {
        HEARTBEAT_INFLIGHT.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for HeartbeatInflightGuard {
    fn drop(&mut self) {
        HEARTBEAT_INFLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}

/// 优雅关机：等待 in-flight heartbeat 结束（带上限）
pub async fn wait_inflight_drain(timeout: std::time::Duration) {
    let start = std::time::Instant::now();
    loop {
        let n = HEARTBEAT_INFLIGHT.load(Ordering::SeqCst);
        if n == 0 {
            return;
        }
        if start.elapsed() >= timeout {
            tracing::warn!(
                remaining = n,
                "[Heartbeat] Shutdown drain timed out with in-flight tasks"
            );
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// Heartbeat 任务定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatTask {
    /// 任务 ID
    pub id: String,
    /// 显示名称
    pub name: String,
    /// Cron 表达式
    pub schedule: String,
    /// 要执行的自然语言指令
    pub action: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 上次**完成**执行时间（运行时状态，不从文件加载；序列化为 lastRun 供前端展示）
    #[serde(skip_deserializing, rename = "lastRun")]
    pub last_run: Option<DateTime<Utc>>,
    /// 上次执行结果（运行时状态）
    #[serde(skip_deserializing, rename = "lastResult")]
    pub last_result: Option<String>,
    /// 本进程内上次**调度认领**时间（仅用于同分钟本地去重；不落盘、不序列化）
    #[serde(skip)]
    pub last_reserved: Option<DateTime<Utc>>,
}

fn default_true() -> bool {
    true
}

/// Heartbeat 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    pub tasks: Vec<HeartbeatTask>,
}

/// 持久化视图：只写回配置字段，不把运行时状态写进 HEARTBEAT.md
#[derive(Serialize)]
struct PersistTask<'a> {
    id: &'a str,
    name: &'a str,
    schedule: &'a str,
    action: &'a str,
    enabled: bool,
}

#[derive(Serialize)]
struct PersistConfig<'a> {
    tasks: Vec<PersistTask<'a>>,
}

/// Heartbeat 管理器
pub struct HeartbeatManager {
    tasks: RwLock<Vec<HeartbeatTask>>,
    config_path: PathBuf,
    /// frontmatter 之后的 Markdown 正文（写回文件时原样保留）
    body: RwLock<String>,
    /// 上次加载/写入时文件的 mtime，用于检测外部修改并自动热加载
    loaded_mtime: RwLock<Option<std::time::SystemTime>>,
}

impl HeartbeatManager {
    /// 从 HEARTBEAT.md 加载
    pub async fn new(config_path: PathBuf) -> Self {
        let (tasks, body) = Self::load_file(&config_path).await.unwrap_or_default();
        let count = tasks.len();
        let mtime = Self::file_mtime(&config_path).await;

        let manager = Self {
            tasks: RwLock::new(tasks),
            config_path,
            body: RwLock::new(body),
            loaded_mtime: RwLock::new(mtime),
        };

        tracing::info!("[Heartbeat] Loaded {} tasks", count);
        manager
    }

    /// 读取文件 mtime
    async fn file_mtime(path: &std::path::Path) -> Option<std::time::SystemTime> {
        tokio::fs::metadata(path).await.ok()?.modified().ok()
    }

    /// 检测 HEARTBEAT.md 是否被外部修改，若是则自动重新加载（真·热加载）
    ///
    /// 在每次调度检查和读写任务前调用，保证手动编辑文件后无需重启/调 API 即可生效，
    /// 也避免 toggle 持久化用内存中的过期配置覆盖用户的手动修改。
    async fn maybe_reload_if_changed(&self) {
        let current = Self::file_mtime(&self.config_path).await;
        let stale = {
            let loaded = self.loaded_mtime.read().await;
            current != *loaded
        };
        if stale {
            tracing::info!("[Heartbeat] HEARTBEAT.md changed on disk, hot-reloading");
            self.reload().await;
        }
    }

    /// 从 YAML frontmatter 加载任务，返回 (tasks, markdown 正文)
    async fn load_file(path: &std::path::Path) -> Option<(Vec<HeartbeatTask>, String)> {
        let content = tokio::fs::read_to_string(path).await.ok()?;

        // 解析 YAML frontmatter
        let trimmed = content.trim_start();
        if !trimmed.starts_with("---") {
            return None;
        }

        let after_first = &trimmed[3..];
        let end_idx = after_first.find("\n---")?;
        let frontmatter = &after_first[..end_idx];
        // 正文：跳过闭合分隔符 "\n---" 及其后的换行
        let body = after_first[end_idx + 4..]
            .trim_start_matches('\r')
            .trim_start_matches('\n')
            .to_string();

        let config: HeartbeatConfig = match serde_yaml::from_str(frontmatter) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    "[Heartbeat] Failed to parse HEARTBEAT.md frontmatter: {}",
                    e
                );
                return None;
            }
        };
        Some((config.tasks, body))
    }

    /// 将当前任务配置写回 HEARTBEAT.md（原子写入，保留正文）
    async fn persist(&self) {
        let yaml = {
            let tasks = self.tasks.read().await;
            let view = PersistConfig {
                tasks: tasks
                    .iter()
                    .map(|t| PersistTask {
                        id: &t.id,
                        name: &t.name,
                        schedule: &t.schedule,
                        action: &t.action,
                        enabled: t.enabled,
                    })
                    .collect(),
            };
            match serde_yaml::to_string(&view) {
                Ok(y) => y,
                Err(e) => {
                    tracing::warn!("[Heartbeat] Failed to serialize tasks: {}", e);
                    return;
                }
            }
        };
        let body = self.body.read().await.clone();
        let content = format!("---\n{}---\n\n{}", yaml, body);

        let tmp_path = self.config_path.with_extension("md.tmp");
        if let Err(e) = tokio::fs::write(&tmp_path, &content).await {
            tracing::warn!("[Heartbeat] Failed to write config: {}", e);
            return;
        }
        if let Err(e) = tokio::fs::rename(&tmp_path, &self.config_path).await {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            tracing::warn!("[Heartbeat] Failed to persist config: {}", e);
            return;
        }
        // 记录自己写入后的 mtime，避免下次误判为外部修改
        *self.loaded_mtime.write().await = Self::file_mtime(&self.config_path).await;
    }

    /// 获取所有任务状态
    pub async fn get_tasks(&self) -> Vec<HeartbeatTask> {
        self.maybe_reload_if_changed().await;
        self.tasks.read().await.clone()
    }

    /// 切换任务启用状态（持久化到 HEARTBEAT.md，重启后保留）
    pub async fn toggle_task(&self, task_id: &str) -> Option<bool> {
        // 先同步磁盘上的最新配置，再在其上应用 toggle，防止覆盖外部修改
        self.maybe_reload_if_changed().await;
        let new_state = {
            let mut tasks = self.tasks.write().await;
            let task = tasks.iter_mut().find(|t| t.id == task_id)?;
            task.enabled = !task.enabled;
            task.enabled
        };
        tracing::info!(
            "[Heartbeat] Task '{}' toggled to {}",
            task_id,
            if new_state { "enabled" } else { "disabled" }
        );
        self.persist().await;
        Some(new_state)
    }

    /// 更新任务字段（name / schedule / action / enabled），校验 cron 后持久化
    pub async fn update_task(
        &self,
        task_id: &str,
        name: Option<String>,
        schedule: Option<String>,
        action: Option<String>,
        enabled: Option<bool>,
    ) -> Result<HeartbeatTask, String> {
        self.maybe_reload_if_changed().await;
        if let Some(ref s) = schedule {
            if !is_valid_cron_expr(s) {
                return Err(format!(
                    "Invalid cron schedule '{}': expected 5 fields (min hour dom month dow)",
                    s
                ));
            }
        }
        if let Some(ref a) = action {
            if a.trim().is_empty() {
                return Err("action must not be empty".to_string());
            }
        }
        let updated = {
            let mut tasks = self.tasks.write().await;
            let task = tasks
                .iter_mut()
                .find(|t| t.id == task_id)
                .ok_or_else(|| format!("Task '{}' not found", task_id))?;
            if let Some(n) = name {
                let n = n.trim().to_string();
                if n.is_empty() {
                    return Err("name must not be empty".to_string());
                }
                task.name = n;
            }
            if let Some(s) = schedule {
                task.schedule = s.trim().to_string();
            }
            if let Some(a) = action {
                task.action = a;
            }
            if let Some(e) = enabled {
                task.enabled = e;
            }
            task.clone()
        };
        self.persist().await;
        tracing::info!(task_id = %task_id, "[Heartbeat] Task updated");
        Ok(updated)
    }

    /// 新增任务并持久化到 HEARTBEAT.md
    ///
    /// - `id` 为空时从 name 生成 slug，并保证唯一
    /// - schedule 必须是合法 5 字段 cron
    /// - action / name 不可为空
    pub async fn add_task(
        &self,
        id: Option<String>,
        name: String,
        schedule: String,
        action: String,
        enabled: bool,
    ) -> Result<HeartbeatTask, String> {
        self.maybe_reload_if_changed().await;

        let name = name.trim().to_string();
        if name.is_empty() {
            return Err("name must not be empty".to_string());
        }
        let action = action.trim().to_string();
        if action.is_empty() {
            return Err("action must not be empty".to_string());
        }
        let schedule = schedule.trim().to_string();
        if !is_valid_cron_expr(&schedule) {
            return Err(format!(
                "Invalid cron schedule '{}': expected 5 fields (min hour dom month dow)",
                schedule
            ));
        }

        let task = {
            let mut tasks = self.tasks.write().await;
            let requested_id = id
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let task_id = if let Some(raw) = requested_id {
                let candidate = slugify_id(&raw);
                if candidate.is_empty() {
                    return Err("id must contain at least one alphanumeric character".to_string());
                }
                if tasks.iter().any(|t| t.id == candidate) {
                    return Err(format!("Task id '{}' already exists", candidate));
                }
                candidate
            } else {
                unique_slug_from_name(&name, &tasks)
            };

            let task = HeartbeatTask {
                id: task_id,
                name,
                schedule,
                action,
                enabled,
                last_run: None,
                last_result: None,
                last_reserved: None,
            };
            tasks.push(task.clone());
            task
        };

        self.persist().await;
        tracing::info!(task_id = %task.id, "[Heartbeat] Task created");
        Ok(task)
    }

    /// 删除任务并持久化（不存在则 Err）
    pub async fn delete_task(&self, task_id: &str) -> Result<(), String> {
        self.maybe_reload_if_changed().await;
        let removed = {
            let mut tasks = self.tasks.write().await;
            let before = tasks.len();
            tasks.retain(|t| t.id != task_id);
            before != tasks.len()
        };
        if !removed {
            return Err(format!("Task '{}' not found", task_id));
        }
        self.persist().await;
        tracing::info!(task_id = %task_id, "[Heartbeat] Task deleted");
        Ok(())
    }

    /// 删除 `claimed_at` 早于 `keep_hours` 的认领记录（`keep_hours` 至少 1）。
    pub async fn cleanup_old_claims(db: &DatabaseConnection, keep_hours: i64) -> u64 {
        let hours = keep_hours.max(1);
        let sql = format!(
            r#"
            DELETE FROM heartbeat_claims
            WHERE claimed_at < NOW() - INTERVAL '{hours} hours'
            "#
        );
        match db
            .execute_raw(Statement::from_string(DbBackend::Postgres, sql))
            .await
        {
            Ok(result) => {
                let n = result.rows_affected();
                if n > 0 {
                    tracing::info!(
                        deleted = n,
                        keep_hours = hours,
                        "[Heartbeat] Cleaned old claims"
                    );
                }
                n
            }
            Err(e) => {
                tracing::warn!(error = %e, "[Heartbeat] Claim cleanup failed");
                0
            }
        }
    }

    /// 记录任务执行结果
    pub async fn record_result(&self, task_id: &str, result: &str) {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
            task.last_run = Some(Utc::now());
            task.last_result = Some(result.to_string());
        }
    }

    /// 检查哪些任务应该在当前分钟执行
    ///
    /// 仅标记本地 `last_reserved`（调度去重），**不**写 `last_run`。
    /// `last_run` 只在 `record_result`（任务完成/失败）时更新，避免崩溃后 UI
    /// 显示“已跑”而实际未完成。多副本去重由 `try_claim_execution` 负责。
    pub async fn check_due_tasks(&self) -> Vec<HeartbeatTask> {
        // 调度前按 mtime 热加载 HEARTBEAT.md
        self.maybe_reload_if_changed().await;
        let now_local = Local::now();
        let now_utc = Utc::now();
        let mut tasks = self.tasks.write().await;

        let mut due = Vec::new();
        for task in tasks.iter_mut() {
            if !task.enabled {
                continue;
            }
            if !cron_matches(&task.schedule, &now_local) {
                continue;
            }
            // 同一 UTC unix 分钟内本进程不重复调度（含已在跑 / 已完成）
            let already_reserved = task
                .last_reserved
                .map(|t| t.timestamp() / 60 == now_utc.timestamp() / 60)
                .unwrap_or(false);
            let already_completed = task
                .last_run
                .map(|t| t.timestamp() / 60 == now_utc.timestamp() / 60)
                .unwrap_or(false);
            if already_reserved || already_completed {
                continue;
            }
            task.last_reserved = Some(now_utc);
            due.push(task.clone());
        }
        due
    }

    /// 多副本 CAS 认领：同一 (task_id, minute_bucket) 仅一个副本执行。
    ///
    /// - 新认领
    /// - `failed` 可立即重认领（超时/失败后允许同桶恢复，由调度侧 last_reserved 防风暴）
    /// - 卡住超过 `CLAIM_STALE_SECS` 的 running 可重认领
    /// - 已 `done` 的桶不再执行
    /// - DB 不可用时返回 `true`（单机降级，依赖进程内 last_reserved）
    pub async fn try_claim_execution(
        db: &DatabaseConnection,
        task_id: &str,
        minute_bucket: i64,
    ) -> bool {
        let sql = format!(
            r#"
            INSERT INTO heartbeat_claims (task_id, minute_bucket, status, claimed_at)
            VALUES ($1, $2, 'running', NOW())
            ON CONFLICT (task_id, minute_bucket) DO UPDATE
            SET status = 'running',
                claimed_at = NOW(),
                completed_at = NULL
            WHERE heartbeat_claims.status = 'failed'
               OR (
                    heartbeat_claims.status = 'running'
                    AND heartbeat_claims.claimed_at < NOW() - INTERVAL '{stale} seconds'
               )
            RETURNING task_id
            "#,
            stale = CLAIM_STALE_SECS
        );
        match db
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Postgres,
                sql,
                [task_id.into(), minute_bucket.into()],
            ))
            .await
        {
            Ok(Some(_)) => true,
            Ok(None) => {
                tracing::debug!(
                    task_id = %task_id,
                    minute_bucket,
                    "[Heartbeat] Claim skipped (held by another replica or already done)"
                );
                false
            }
            Err(e) => {
                tracing::warn!(
                    task_id = %task_id,
                    error = %e,
                    "[Heartbeat] Claim query failed; allowing local execution"
                );
                true
            }
        }
    }

    /// 将认领标为 done（不可再认领）或 failed（可立即重认领）。
    ///
    /// - `done`：成功完成，同分钟桶不可再认领
    /// - `failed`：超时/失败，允许后续重认领（见 try_claim）
    pub async fn complete_claim(
        db: &DatabaseConnection,
        task_id: &str,
        minute_bucket: i64,
        status: &str,
    ) {
        let status = if status == "failed" { "failed" } else { "done" };
        let result = db
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Postgres,
                r#"
                UPDATE heartbeat_claims
                SET status = $3, completed_at = NOW()
                WHERE task_id = $1 AND minute_bucket = $2
                "#,
                [task_id.into(), minute_bucket.into(), status.into()],
            ))
            .await;
        if let Err(e) = result {
            tracing::warn!(
                task_id = %task_id,
                error = %e,
                "[Heartbeat] Failed to complete claim"
            );
        }
    }

    /// 当前 UTC 分钟桶（unix_ts / 60）
    pub fn current_minute_bucket() -> i64 {
        Utc::now().timestamp() / 60
    }

    /// 重新加载配置（保留运行时状态：last_run / last_result / last_reserved）
    pub async fn reload(&self) {
        if let Some((mut new_tasks, new_body)) = Self::load_file(&self.config_path).await {
            let mut current = self.tasks.write().await;
            for task in new_tasks.iter_mut() {
                if let Some(old) = current.iter().find(|t| t.id == task.id) {
                    task.last_run = old.last_run;
                    task.last_result = old.last_result.clone();
                    task.last_reserved = old.last_reserved;
                }
            }
            *current = new_tasks;
            drop(current);
            *self.body.write().await = new_body;
            *self.loaded_mtime.write().await = Self::file_mtime(&self.config_path).await;
            tracing::info!("[Heartbeat] Reloaded configuration");
        }
    }
}

// ID / slug

/// ASCII 字母数字转小写；其余作分隔符压缩为内部 `-`，首尾 `-` 去掉。
fn slugify_id(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut prev_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// 从任务名 slug 生成唯一 id；slug 为空时用 `task`，冲突则 `{base}-N`。
fn unique_slug_from_name(name: &str, existing: &[HeartbeatTask]) -> String {
    let base = {
        let s = slugify_id(name);
        if s.is_empty() {
            "task".to_string()
        } else {
            s
        }
    };
    if !existing.iter().any(|t| t.id == base) {
        return base;
    }
    for n in 2u32.. {
        let candidate = format!("{base}-{n}");
        if !existing.iter().any(|t| t.id == candidate) {
            return candidate;
        }
    }
    // unreachable in practice
    format!("{base}-{}", Utc::now().timestamp())
}

// Cron 匹配

/// 校验 5 字段 cron 表达式语法（不求值）
fn is_valid_cron_expr(expr: &str) -> bool {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        return false;
    }
    parts.iter().all(|field| {
        field.split(',').all(|item| {
            let item = item.trim();
            if item.is_empty() {
                return false;
            }
            let (base, step) = match item.split_once('/') {
                Some((b, s)) => match s.parse::<u32>() {
                    Ok(n) if n > 0 => (b, Some(n)),
                    _ => return false,
                },
                None => (item, None),
            };
            let _ = step;
            if base == "*" {
                return true;
            }
            if let Some((a, b)) = base.split_once('-') {
                return match (a.parse::<u32>(), b.parse::<u32>()) {
                    (Ok(lo), Ok(hi)) => lo <= hi,
                    _ => false,
                };
            }
            base.parse::<u32>().is_ok()
        })
    })
}

/// 完整 5 字段 cron 匹配（分 时 日 月 星期）
fn cron_matches(expr: &str, now: &DateTime<Local>) -> bool {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        return false;
    }

    let minute_ok = cron_field_matches(parts[0], now.minute());
    let hour_ok = cron_field_matches(parts[1], now.hour());
    let month_ok = cron_field_matches(parts[3], now.month());

    // 0 = 周日；星期字段额外接受 7 表示周日
    let dow = now.weekday().num_days_from_sunday();
    let dom_restricted = parts[2] != "*";
    let dow_restricted = parts[4] != "*";
    let dom_ok = cron_field_matches(parts[2], now.day());
    let dow_ok = cron_field_matches(parts[4], dow) || (dow == 0 && cron_field_matches(parts[4], 7));

    // 标准 cron 语义：日和星期同时受限时，任一命中即可
    let day_ok = if dom_restricted && dow_restricted {
        dom_ok || dow_ok
    } else {
        dom_ok && dow_ok
    };

    minute_ok && hour_ok && month_ok && day_ok
}

/// 单字段匹配：支持逗号分隔的多个 item
fn cron_field_matches(field: &str, value: u32) -> bool {
    field
        .split(',')
        .any(|item| cron_item_matches(item.trim(), value))
}

/// 单 item 匹配：`*`、`N`、`A-B`、`*/N`、`A-B/N`、`A/N`
fn cron_item_matches(item: &str, value: u32) -> bool {
    if item.is_empty() {
        return false;
    }

    let (base, step) = match item.split_once('/') {
        Some((b, s)) => match s.parse::<u32>() {
            Ok(n) if n > 0 => (b, Some(n)),
            _ => return false,
        },
        None => (item, None),
    };

    let (start, end) = if base == "*" {
        (0u32, u32::MAX)
    } else if let Some((a, b)) = base.split_once('-') {
        match (a.parse::<u32>(), b.parse::<u32>()) {
            (Ok(a), Ok(b)) if a <= b => (a, b),
            _ => return false,
        }
    } else {
        match base.parse::<u32>() {
            Ok(n) => match step {
                // 裸数字带步进（如 "5/15"）按 vixie cron 语义视为 "5-max/15"
                Some(_) => (n, u32::MAX),
                None => return value == n,
            },
            Err(_) => return false,
        }
    };

    if value < start || value > end {
        return false;
    }
    match step {
        Some(s) => (value - start).is_multiple_of(s),
        None => true,
    }
}

// 全局实例

/// 全局 Heartbeat 管理器
static HEARTBEAT_MANAGER: once_cell::sync::OnceCell<Arc<HeartbeatManager>> =
    once_cell::sync::OnceCell::new();

/// 初始化全局 Heartbeat 管理器
pub async fn init_heartbeat(config_path: PathBuf) {
    let manager = Arc::new(HeartbeatManager::new(config_path).await);
    let _ = HEARTBEAT_MANAGER.set(manager);
}

/// 获取全局 Heartbeat 管理器
pub fn get_heartbeat() -> Option<&'static Arc<HeartbeatManager>> {
    HEARTBEAT_MANAGER.get()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn local(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    #[test]
    fn test_cron_every_minute() {
        assert!(cron_matches("* * * * *", &local(2026, 7, 11, 12, 34)));
    }

    #[test]
    fn test_cron_fixed_time() {
        // 2026-07-11 是周六
        assert!(cron_matches("0 9 * * *", &local(2026, 7, 11, 9, 0)));
        assert!(!cron_matches("0 9 * * *", &local(2026, 7, 11, 9, 1)));
        assert!(!cron_matches("0 9 * * *", &local(2026, 7, 11, 10, 0)));
    }

    #[test]
    fn test_cron_minute_step() {
        assert!(cron_matches("*/15 * * * *", &local(2026, 7, 11, 8, 0)));
        assert!(cron_matches("*/15 * * * *", &local(2026, 7, 11, 8, 45)));
        assert!(!cron_matches("*/15 * * * *", &local(2026, 7, 11, 8, 20)));
    }

    #[test]
    fn test_cron_hour_step() {
        // 步进小时 cron（0 */6 * * *）必须匹配
        assert!(cron_matches("0 */6 * * *", &local(2026, 7, 11, 0, 0)));
        assert!(cron_matches("0 */6 * * *", &local(2026, 7, 11, 6, 0)));
        assert!(cron_matches("0 */6 * * *", &local(2026, 7, 11, 18, 0)));
        assert!(!cron_matches("0 */6 * * *", &local(2026, 7, 11, 7, 0)));
        assert!(!cron_matches("0 */6 * * *", &local(2026, 7, 11, 6, 1)));
    }

    #[test]
    fn test_cron_weekday() {
        // 2026-07-11 = 周六(6)，2026-07-12 = 周日(0)，2026-07-13 = 周一(1)
        assert!(cron_matches("0 9 * * 6", &local(2026, 7, 11, 9, 0)));
        assert!(!cron_matches("0 9 * * 1", &local(2026, 7, 11, 9, 0)));
        assert!(cron_matches("0 9 * * 1", &local(2026, 7, 13, 9, 0)));
        // 0 和 7 都是周日
        assert!(cron_matches("0 9 * * 0", &local(2026, 7, 12, 9, 0)));
        assert!(cron_matches("0 9 * * 7", &local(2026, 7, 12, 9, 0)));
    }

    #[test]
    fn test_cron_day_of_month() {
        assert!(cron_matches("0 0 1 * *", &local(2026, 7, 1, 0, 0)));
        assert!(!cron_matches("0 0 1 * *", &local(2026, 7, 11, 0, 0)));
    }

    #[test]
    fn test_cron_month() {
        assert!(cron_matches("0 0 * 7 *", &local(2026, 7, 11, 0, 0)));
        assert!(!cron_matches("0 0 * 8 *", &local(2026, 7, 11, 0, 0)));
    }

    #[test]
    fn test_cron_list_and_range() {
        assert!(cron_matches("0,30 * * * *", &local(2026, 7, 11, 5, 30)));
        assert!(!cron_matches("0,30 * * * *", &local(2026, 7, 11, 5, 15)));
        assert!(cron_matches("0 9-17 * * *", &local(2026, 7, 11, 13, 0)));
        assert!(!cron_matches("0 9-17 * * *", &local(2026, 7, 11, 18, 0)));
        assert!(cron_matches("0 9-17/4 * * *", &local(2026, 7, 11, 13, 0)));
        assert!(!cron_matches("0 9-17/4 * * *", &local(2026, 7, 11, 14, 0)));
    }

    #[test]
    fn test_cron_dom_dow_or_semantics() {
        // 日和星期同时受限：任一命中即可（标准 cron 语义）
        // 2026-07-13 是周一、13 号
        assert!(cron_matches("0 0 13 * *", &local(2026, 7, 13, 0, 0)));
        assert!(cron_matches("0 0 1 * 1", &local(2026, 7, 13, 0, 0))); // 星期命中
        assert!(cron_matches("0 0 13 * 5", &local(2026, 7, 13, 0, 0))); // 日命中
        assert!(!cron_matches("0 0 1 * 5", &local(2026, 7, 13, 0, 0))); // 都不命中
    }

    #[test]
    fn test_cron_invalid() {
        assert!(!cron_matches("bad expr", &local(2026, 7, 11, 0, 0)));
        assert!(!cron_matches("0 0 * *", &local(2026, 7, 11, 0, 0))); // 4 字段
        assert!(!cron_matches("*/0 * * * *", &local(2026, 7, 11, 0, 0))); // 步进为 0
    }

    #[test]
    fn test_is_valid_cron_expr() {
        assert!(is_valid_cron_expr("* * * * *"));
        assert!(is_valid_cron_expr("0 */6 * * *"));
        assert!(is_valid_cron_expr("0,30 9-17 * * 1-5"));
        assert!(!is_valid_cron_expr("0 0 * *")); // 4 字段
        assert!(!is_valid_cron_expr("*/0 * * * *"));
        assert!(!is_valid_cron_expr(""));
    }

    #[tokio::test]
    async fn test_toggle_persists_and_reload_keeps_runtime_state() {
        let dir = std::env::temp_dir().join(format!("hb_test_{}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("HEARTBEAT.md");
        tokio::fs::write(
            &path,
            "---\ntasks:\n  - id: t1\n    name: \"Task One\"\n    schedule: \"0 9 * * *\"\n    action: \"do stuff\"\n    enabled: false\n---\n\n# Body text\n",
        )
        .await
        .unwrap();

        let mgr = HeartbeatManager::new(path.clone()).await;

        // toggle 写回文件
        assert_eq!(mgr.toggle_task("t1").await, Some(true));
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(
            content.contains("enabled: true"),
            "toggle 应持久化: {content}"
        );
        assert!(content.contains("# Body text"), "正文应保留: {content}");
        // 运行时状态不应写入文件
        assert!(
            !content.contains("lastRun"),
            "运行时状态不应落盘: {content}"
        );

        // reload 保留运行时状态
        mgr.record_result("t1", "ok").await;
        mgr.reload().await;
        let tasks = mgr.get_tasks().await;
        assert!(tasks[0].enabled, "reload 后 toggle 状态应保留");
        assert_eq!(tasks[0].last_result.as_deref(), Some("ok"));
        assert!(tasks[0].last_run.is_some());

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[test]
    fn test_task_serializes_runtime_fields_camelcase() {
        // 前端 HeartbeatTask 类型期望 lastRun/lastResult（camelCase）
        let mut t: HeartbeatTask =
            serde_yaml::from_str("id: x\nname: n\nschedule: \"* * * * *\"\naction: a").unwrap();
        assert!(t.enabled, "enabled 缺省应为 true");
        assert!(t.last_run.is_none());
        t.last_run = Some(Utc::now());
        t.last_result = Some("ok".to_string());
        let v = serde_json::to_value(&t).unwrap();
        assert!(v.get("lastRun").is_some(), "应序列化 lastRun: {v}");
        assert_eq!(v["lastResult"], "ok");
    }

    #[tokio::test]
    async fn test_hot_reload_on_external_edit() {
        let dir = std::env::temp_dir().join(format!("hb_test_hot_{}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("HEARTBEAT.md");
        tokio::fs::write(
            &path,
            "---\ntasks:\n  - id: t1\n    name: \"One\"\n    schedule: \"0 9 * * *\"\n    action: \"a\"\n---\n",
        )
        .await
        .unwrap();

        let mgr = HeartbeatManager::new(path.clone()).await;
        assert_eq!(mgr.get_tasks().await.len(), 1);

        // 模拟用户手动编辑文件（确保 mtime 变化）
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        tokio::fs::write(
            &path,
            "---\ntasks:\n  - id: t1\n    name: \"One\"\n    schedule: \"0 9 * * *\"\n    action: \"a\"\n  - id: t2\n    name: \"Two\"\n    schedule: \"30 8 * * *\"\n    action: \"b\"\n---\n",
        )
        .await
        .unwrap();

        let tasks = mgr.get_tasks().await;
        assert_eq!(tasks.len(), 2, "外部编辑应被自动热加载");

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_check_due_tasks_dedup_within_minute() {
        let dir = std::env::temp_dir().join(format!("hb_test_dedup_{}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("HEARTBEAT.md");
        tokio::fs::write(
            &path,
            "---\ntasks:\n  - id: every\n    name: \"Every Minute\"\n    schedule: \"* * * * *\"\n    action: \"tick\"\n    enabled: true\n---\n",
        )
        .await
        .unwrap();

        let mgr = HeartbeatManager::new(path).await;
        let first = mgr.check_due_tasks().await;
        assert_eq!(first.len(), 1, "首次检查应返回到期任务");
        // 调度只写 last_reserved，不写 last_run
        let tasks = mgr.get_tasks().await;
        assert!(tasks[0].last_run.is_none(), "调度不应写 last_run");
        assert!(tasks[0].last_reserved.is_some());
        // 同一分钟内第二次检查不应重复触发（即使任务尚未完成）
        let second = mgr.check_due_tasks().await;
        assert!(second.is_empty(), "同一分钟内不应重复触发");

        // 完成后 last_run 才更新
        mgr.record_result("every", "ok").await;
        let tasks = mgr.get_tasks().await;
        assert!(tasks[0].last_run.is_some());
        assert_eq!(tasks[0].last_result.as_deref(), Some("ok"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn test_add_and_delete_task_persists() {
        let dir = std::env::temp_dir().join(format!("hb_test_crud_{}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("HEARTBEAT.md");
        tokio::fs::write(
            &path,
            "---\ntasks:\n  - id: t1\n    name: \"One\"\n    schedule: \"0 9 * * *\"\n    action: \"a\"\n    enabled: true\n---\n\n# Keep me\n",
        )
        .await
        .unwrap();

        let mgr = HeartbeatManager::new(path.clone()).await;

        // 无效 cron 拒绝
        let err = mgr
            .add_task(None, "Bad".into(), "0 0 * *".into(), "do it".into(), true)
            .await
            .unwrap_err();
        assert!(err.contains("Invalid cron"), "{err}");

        // 空 action 拒绝
        let err = mgr
            .add_task(None, "Bad".into(), "0 9 * * *".into(), "  ".into(), true)
            .await
            .unwrap_err();
        assert!(err.contains("action"), "{err}");

        // 自动 slug id
        let created = mgr
            .add_task(
                None,
                "Brew Daily Summary".into(),
                "0 9 * * *".into(),
                "总结 brew 订阅".into(),
                true,
            )
            .await
            .unwrap();
        assert_eq!(created.id, "brew-daily-summary");
        assert!(created.enabled);

        // 中文名回退 task / task-N
        let cjk = mgr
            .add_task(
                None,
                "每天检查".into(),
                "0 * * * *".into(),
                "检查更新".into(),
                false,
            )
            .await
            .unwrap();
        assert_eq!(cjk.id, "task");
        assert!(!cjk.enabled);

        // 重复 slug 自动加后缀
        let dup = mgr
            .add_task(
                None,
                "Brew Daily Summary".into(),
                "30 9 * * *".into(),
                "再总结一次".into(),
                true,
            )
            .await
            .unwrap();
        assert_eq!(dup.id, "brew-daily-summary-2");

        // 指定 id 冲突
        let err = mgr
            .add_task(
                Some("t1".into()),
                "X".into(),
                "0 0 * * *".into(),
                "a".into(),
                true,
            )
            .await
            .unwrap_err();
        assert!(err.contains("already exists"), "{err}");

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("brew-daily-summary"), "{content}");
        assert!(content.contains("# Keep me"), "正文应保留: {content}");
        assert!(!content.contains("lastRun"), "运行时状态不应落盘");

        // 删除
        mgr.delete_task("t1").await.unwrap();
        let tasks = mgr.get_tasks().await;
        assert!(!tasks.iter().any(|t| t.id == "t1"));
        let err = mgr.delete_task("t1").await.unwrap_err();
        assert!(err.contains("not found"), "{err}");

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(!content.contains("id: t1"), "{content}");
        assert!(content.contains("# Keep me"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[test]
    fn test_slugify_id() {
        assert_eq!(slugify_id("Brew Daily Summary"), "brew-daily-summary");
        assert_eq!(slugify_id("  Hello__World!! "), "hello-world");
        assert_eq!(slugify_id("每天检查"), "");
        assert_eq!(slugify_id("a--b"), "a-b");
    }
}
