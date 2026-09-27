//! Skill 自我进化引擎
//!
//! Agent 不仅能使用 Skills，还能创建、改进、淘汰 Skills，
//! 形成"执行→评估→进化"闭环。
//!
//! 统计数据持久化到 `{skills_dir}/_stats.json`，服务重启后自动恢复。
//!
//! 安全约束：
//! - Agent 生成的 Skill 文件前缀固定为 `_auto_`
//! - Agent 不能修改手动创建的 Skill（origin: manual 只读）
//! - 自动创建的 Skill 需要通过 gating 校验
//! - 每日自动创建上限 10 个

use crate::services::agent::capability::CapabilityRef;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

use super::skill::{Skill, SkillOrigin, get_skill_registry};

fn skill_io_failed(action: &str, error: std::io::Error) -> String {
    tracing::error!(%error, action, "skill file io failed");
    match error.kind() {
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => {
            format!("{action}: storage is not writable")
        }
        ErrorKind::StorageFull => format!("{action}: not enough disk space"),
        ErrorKind::NotFound => format!("{action}: path not found"),
        ErrorKind::AlreadyExists => format!("{action}: already exists"),
        _ => action.to_string(),
    }
}

/// 淘汰阈值：失败率超过此值的自动 Skill 被清理
const PRUNE_FAILURE_RATE: f64 = 0.70;

/// 连续失败次数阈值
const CONSECUTIVE_FAILURE_LIMIT: u32 = 5;

/// 触发改进所需的最小样本量
const MIN_SAMPLES_FOR_ACTION: u32 = 3;

/// 统计文件名
const STATS_FILE: &str = "_stats.json";

/// 能力缺口文件名
const GAPS_FILE: &str = "_gaps.json";

/// 能力缺口检测结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityGap {
    /// 用户想做什么
    pub description: String,
    /// 缺少什么能力
    pub missing_capability: String,
    /// 是否有 http.fetch 等变通方案
    pub workaround: Option<String>,
    /// 建议开发者添加什么
    pub suggestion: String,
    /// 置信度 (0.0 - 1.0)，随报告次数递增
    pub confidence: f64,
    /// 首次报告时间
    pub first_seen: String,
    /// 报告次数
    pub report_count: u32,
}

/// Skill 执行统计（持久化到文件）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillStats {
    pub success_count: u32,
    pub failure_count: u32,
    pub consecutive_failures: u32,
    pub last_failure_reason: Option<String>,
    pub last_improved_at: Option<chrono::DateTime<Utc>>,
}

impl SkillStats {
    fn total(&self) -> u32 {
        self.success_count + self.failure_count
    }

    fn failure_rate(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            return 0.0;
        }
        self.failure_count as f64 / total as f64
    }
}

/// Skill 自我进化引擎
pub struct SkillEvolution {
    /// skills 目录
    skills_dir: PathBuf,
    /// 执行统计（skill_id -> stats）
    stats: Mutex<HashMap<String, SkillStats>>,
    /// 能力缺口累积
    capability_gaps: Mutex<Vec<CapabilityGap>>,
    /// 脏标记 — stats 发生变更但尚未写盘
    stats_dirty: std::sync::atomic::AtomicBool,
    /// gaps 脏标记
    gaps_dirty: std::sync::atomic::AtomicBool,
}

impl SkillEvolution {
    /// 创建新的进化引擎（从文件恢复统计）
    pub async fn new(skills_dir: PathBuf) -> Self {
        // 确保目录存在：auto-create 写技能文件、flush 写 _stats.json 都依赖它
        if let Err(e) = tokio::fs::create_dir_all(&skills_dir).await {
            tracing::warn!(
                "[SkillEvolution] Failed to create skills dir {}: {}",
                skills_dir.display(),
                e
            );
        }
        let stats = Self::load_stats_from_file(&skills_dir).await;
        let gaps = Self::load_gaps_from_file(&skills_dir).await;

        let stats_count = stats.len();
        let gaps_count = gaps.len();

        let engine = Self {
            skills_dir,
            stats: Mutex::new(stats),
            capability_gaps: Mutex::new(gaps),
            stats_dirty: std::sync::atomic::AtomicBool::new(false),
            gaps_dirty: std::sync::atomic::AtomicBool::new(false),
        };

        if stats_count > 0 || gaps_count > 0 {
            tracing::info!(
                stats = stats_count,
                gaps = gaps_count,
                "[SkillEvolution] Restored persisted state"
            );
        }

        engine
    }

    // 统计文件持久化

    /// 从文件加载统计
    async fn load_stats_from_file(skills_dir: &Path) -> HashMap<String, SkillStats> {
        let path = skills_dir.join(STATS_FILE);
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => HashMap::new(),
        }
    }

    /// 从文件加载能力缺口
    async fn load_gaps_from_file(skills_dir: &Path) -> Vec<CapabilityGap> {
        let path = skills_dir.join(GAPS_FILE);
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    /// 持久化统计到文件（仅当脏标记为 true）
    ///
    /// 使用临时文件 + rename 实现原子写入，防止并发导致数据损坏
    pub async fn flush(&self) {
        if self
            .stats_dirty
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            let stats = self.stats.lock().await;
            let path = self.skills_dir.join(STATS_FILE);
            let tmp_path = self.skills_dir.join(format!(".tmp_{}", STATS_FILE));
            if let Ok(json) = serde_json::to_string_pretty(&*stats) {
                match tokio::fs::write(&tmp_path, json).await {
                    Ok(_) => {
                        if let Err(e) = tokio::fs::rename(&tmp_path, &path).await {
                            tracing::warn!("[SkillEvolution] Failed to rename stats: {}", e);
                            self.stats_dirty
                                .store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("[SkillEvolution] Failed to write stats: {}", e);
                        self.stats_dirty
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }
        }
        if self
            .gaps_dirty
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            let gaps = self.capability_gaps.lock().await;
            let path = self.skills_dir.join(GAPS_FILE);
            let tmp_path = self.skills_dir.join(format!(".tmp_{}", GAPS_FILE));
            if let Ok(json) = serde_json::to_string_pretty(&*gaps) {
                match tokio::fs::write(&tmp_path, json).await {
                    Ok(_) => {
                        if let Err(e) = tokio::fs::rename(&tmp_path, &path).await {
                            tracing::warn!("[SkillEvolution] Failed to rename gaps: {}", e);
                            self.gaps_dirty
                                .store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("[SkillEvolution] Failed to write gaps: {}", e);
                        self.gaps_dirty
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }
        }
    }

    fn mark_stats_dirty(&self) {
        self.stats_dirty
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    // 核心回调

    // AI 驱动的改进

    // 淘汰

    /// 淘汰低质量 Skill（失败率 > 70%，或连续 5 次失败）
    ///
    /// 由后台定时任务（每日）调用。
    pub async fn prune_skills(&self) -> Vec<String> {
        let stats = self.stats.lock().await;
        let mut to_prune = Vec::new();

        for (skill_id, stat) in stats.iter() {
            if stat.total() < MIN_SAMPLES_FOR_ACTION {
                continue;
            }

            let should_prune = stat.failure_rate() > PRUNE_FAILURE_RATE
                || stat.consecutive_failures >= CONSECUTIVE_FAILURE_LIMIT;

            if should_prune {
                to_prune.push(skill_id.clone());
            }
        }
        drop(stats);

        let mut pruned = Vec::new();
        for skill_id in to_prune {
            if let Some(registry) = get_skill_registry() {
                let lookup_id = CapabilityRef::parse(&skill_id)
                    .skill_id()
                    .unwrap_or(&skill_id);
                if let Some(skill) = registry.get(lookup_id).await {
                    if skill.origin == SkillOrigin::Manual {
                        continue;
                    }
                    if self.prune_single_skill(&skill).await {
                        pruned.push(skill_id);
                    }
                }
            }
        }

        // 重新加载
        if !pruned.is_empty() {
            // 从 stats 中移除已淘汰的条目
            {
                let mut stats = self.stats.lock().await;
                for id in &pruned {
                    stats.remove(id);
                }
                self.mark_stats_dirty();
            }
            if let Some(registry) = get_skill_registry() {
                registry.reload().await;
            }
        }

        // 写盘
        self.flush().await;

        pruned
    }

    /// 淘汰单个 Skill：软删除到 `_trash/`，保留可恢复副本
    async fn prune_single_skill(&self, skill: &Skill) -> bool {
        match soft_delete_skill_file(skill).await {
            Ok(dest) => {
                tracing::info!(
                    skill_id = %skill.id,
                    trash = %dest.display(),
                    "[SkillEvolution] Soft-deleted pruned skill"
                );
                if let Some(nm) = crate::services::agent::notifications::get_notification_manager()
                {
                    nm.notify_skill_evolution(
                        &skill.id,
                        "pruned",
                        &format!(
                            "Auto-skill \"{}\" was pruned for a high failure rate (moved to trash).",
                            skill.name
                        ),
                    )
                    .await;
                }
                self.flush().await;
                true
            }
            Err(e) => {
                tracing::warn!(
                    skill_id = %skill.id,
                    error = %e,
                    "[SkillEvolution] Failed to soft-delete skill file"
                );
                false
            }
        }
    }

    // 查询

    /// 获取所有统计（用于调试/API）
    pub async fn get_all_stats(&self) -> HashMap<String, SkillStats> {
        self.stats.lock().await.clone()
    }

    /// 手动删除一个 Agent 生成的 Skill（不允许删除 manual Skill）
    ///
    /// 软删除：移动到 skills/_trash/，不物理抹除。
    pub async fn delete_skill(&self, skill_id: &str) -> Result<(), SkillDeleteError> {
        let registry = get_skill_registry().ok_or_else(|| {
            SkillDeleteError::Rejected("Skill registry not initialized".to_string())
        })?;
        let skill = registry
            .get(skill_id)
            .await
            .ok_or_else(|| SkillDeleteError::Rejected(format!("Skill not found: {}", skill_id)))?;

        if skill.origin == SkillOrigin::Manual {
            return Err(SkillDeleteError::Rejected(
                "Cannot delete manual skills".to_string(),
            ));
        }

        if skill.file_path.exists() {
            soft_delete_skill_file(&skill)
                .await
                .map_err(SkillDeleteError::File)?;
        }

        // 清理统计
        {
            let mut stats = self.stats.lock().await;
            stats.remove(skill_id);
            self.mark_stats_dirty();
        }
        self.flush().await;

        // 重新加载 registry
        if let Some(r) = get_skill_registry() {
            r.reload().await;
        }

        tracing::info!(
            skill_id = skill_id,
            "[SkillEvolution] Manually soft-deleted skill"
        );
        Ok(())
    }
}

/// Why a manual skill delete did not happen.
#[derive(Debug)]
pub enum SkillDeleteError {
    /// The skill cannot be deleted: unknown, manual, or the registry is not ready.
    Rejected(String),
    /// Moving the skill file into the trash failed.
    File(String),
}

/// 将 skill 文件移入 `skills/_trash/`（带时间戳前缀），避免物理删除无法恢复。
/// 加载器只读 skills 目录顶层 `.md`，不进入子目录。
async fn soft_delete_skill_file(skill: &Skill) -> Result<PathBuf, String> {
    if !skill.file_path.exists() {
        return Err(format!("Skill file missing: {}", skill.file_path.display()));
    }
    let parent = skill
        .file_path
        .parent()
        .ok_or_else(|| "Skill file has no parent directory".to_string())?;
    let trash_dir = parent.join("_trash");
    tokio::fs::create_dir_all(&trash_dir)
        .await
        .map_err(|error| skill_io_failed("Failed to create skill trash directory", error))?;
    let base_name = skill
        .file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("skill.md");
    let dest = trash_dir.join(format!(
        "{}_{}",
        Utc::now().format("%Y%m%d%H%M%S"),
        base_name
    ));
    tokio::fs::rename(&skill.file_path, &dest)
        .await
        .map_err(|error| skill_io_failed("Failed to move skill to trash", error))?;
    Ok(dest)
}

/// 全局 SkillEvolution 实例
static SKILL_EVOLUTION: once_cell::sync::OnceCell<Arc<SkillEvolution>> =
    once_cell::sync::OnceCell::new();

/// 初始化全局 SkillEvolution（异步，从文件恢复状态）
///
/// 同时启动后台定时任务：
/// - 每 5 分钟 flush 脏数据到磁盘
/// - 每 24 小时执行一次 prune_skills
pub async fn init_skill_evolution(skills_dir: PathBuf) {
    let evolution = Arc::new(SkillEvolution::new(skills_dir).await);
    let _ = SKILL_EVOLUTION.set(evolution.clone());

    start_maintenance(crate::services::jobs::jobs(), evolution, FLUSH_INTERVAL);
}

const FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5 * 60);
/// 288 * 5min = 24h
const PRUNE_EVERY_TICKS: u64 = 288;

/// 后台定时任务：定期 flush + 每日 prune。挂在进程 job runner 上，
/// 停机时随其他后台任务一起停；每轮结束后再等满一个间隔，与原先的 sleep 循环一致。
fn start_maintenance(
    runner: &crate::services::jobs::JobRunner,
    evolution: Arc<SkillEvolution>,
    flush_interval: std::time::Duration,
) -> crate::services::jobs::JobHandle {
    let every = crate::services::jobs::Every::new(flush_interval)
        .after(flush_interval)
        .spaced();
    let mut tick_count: u64 = 0;
    runner.periodic("skill evolution maintenance", every, move || {
        tick_count += 1;
        let prune = tick_count.is_multiple_of(PRUNE_EVERY_TICKS);
        let evolution = evolution.clone();
        async move {
            // 每 5 分钟 flush
            evolution.flush().await;

            // 每 24 小时 prune
            if prune {
                let pruned = evolution.prune_skills().await;
                if !pruned.is_empty() {
                    tracing::info!(
                        count = pruned.len(),
                        skills = ?pruned,
                        "[SkillEvolution] Daily prune completed"
                    );
                }
            }
        }
    })
}

/// 获取全局 SkillEvolution
pub fn get_skill_evolution() -> Option<&'static Arc<SkillEvolution>> {
    SKILL_EVOLUTION.get()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn maintenance_runs_on_the_job_runner_and_stops_with_it() {
        let dir = std::env::temp_dir().join(format!(
            "myriad-skill-evolution-job-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let evolution = Arc::new(SkillEvolution::new(dir.clone()).await);
        evolution
            .stats_dirty
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let runner = crate::services::jobs::JobRunner::new();
        let handle = start_maintenance(&runner, evolution, Duration::from_millis(1));
        let stats = dir.join(STATS_FILE);
        tokio::time::timeout(Duration::from_secs(5), async {
            while !stats.exists() {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("runner job did not flush dirty stats");
        runner.shutdown(Duration::from_secs(1)).await;
        assert!(handle.is_cancelled());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
