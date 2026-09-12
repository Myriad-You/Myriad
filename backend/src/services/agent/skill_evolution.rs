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

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use super::capability;
use super::external_pure::classify_outbound_fetch;
use super::skill::{get_skill_registry, Skill, SkillOrigin};

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

fn skill_ai_failed(label: &str, error: impl std::fmt::Display) -> String {
    let detail = error.to_string();
    tracing::error!(error = %detail, label, "skill AI failed");
    classify_outbound_fetch(label, &detail)
}

/// 每日自动创建 Skill 上限
const DAILY_AUTO_CREATE_LIMIT: u32 = 10;

/// Skill 改进冷却时间（秒）— 同一 Skill 24 小时内最多改进 1 次
const IMPROVE_COOLDOWN_SECS: i64 = 86400;

/// 失败率阈值：超过此值触发自动改进
const FAILURE_RATE_THRESHOLD: f64 = 0.30;

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
    /// 今日已创建的自动 Skill 数量
    daily_create_count: AtomicU32,
    /// 今日日期（用于重置计数器）
    daily_date: Mutex<NaiveDate>,
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
            daily_create_count: AtomicU32::new(0),
            daily_date: Mutex::new(Utc::now().date_naive()),
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

    fn mark_gaps_dirty(&self) {
        self.gaps_dirty
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    // 核心回调

    /// 执行后回调：记录成功/失败，触发进化动作
    ///
    /// 由 Executor 在每个步骤完成后调用。
    /// 当失败率超阈值时自动触发改进流程。
    pub async fn on_execution_complete(
        &self,
        capability_id: &str,
        success: bool,
        failure_reason: Option<&str>,
    ) {
        // Normalize stats key: strip "skill:" prefix so stats keys are consistent
        // with improve_skill / prune_skills which use bare skill IDs.
        let stats_key = capability_id
            .strip_prefix("skill:")
            .unwrap_or(capability_id);

        let (should_improve, should_prune) = {
            let mut stats = self.stats.lock().await;
            let entry = stats.entry(stats_key.to_string()).or_default();

            if success {
                entry.success_count += 1;
                entry.consecutive_failures = 0;
            } else {
                entry.failure_count += 1;
                entry.consecutive_failures += 1;
                entry.last_failure_reason = failure_reason.map(String::from);
            }

            self.mark_stats_dirty();

            let total = entry.total();
            let failure_rate = entry.failure_rate();

            let should_improve = !success
                && failure_rate > FAILURE_RATE_THRESHOLD
                && total >= MIN_SAMPLES_FOR_ACTION
                && match entry.last_improved_at {
                    Some(last) => (Utc::now() - last).num_seconds() > IMPROVE_COOLDOWN_SECS,
                    None => true,
                };

            let should_prune = failure_rate > PRUNE_FAILURE_RATE && total >= MIN_SAMPLES_FOR_ACTION
                || entry.consecutive_failures >= CONSECUTIVE_FAILURE_LIMIT;

            (should_improve, should_prune)
        };

        // 只对 Agent 生成的 Skill 触发自动进化
        if should_improve || should_prune {
            if let Some(registry) = get_skill_registry() {
                // stats_key is already normalized ("skill:" prefix stripped)
                let skill_id = stats_key;
                if let Some(skill) = registry.get(skill_id).await {
                    if skill.origin == SkillOrigin::Manual {
                        // 手动 Skill 不自动修改；should_prune 时才记警告
                        if should_prune {
                            tracing::warn!(
                                skill_id = skill_id,
                                "[SkillEvolution] Manual skill has high failure rate, needs developer attention"
                            );
                        }
                        return;
                    }

                    if should_prune {
                        tracing::info!(
                            skill_id = skill_id,
                            "[SkillEvolution] Auto-pruning low-quality skill"
                        );
                        self.prune_single_skill(&skill).await;
                    } else if should_improve {
                        tracing::info!(
                            skill_id = skill_id,
                            failure_reason = failure_reason,
                            "[SkillEvolution] Triggering AI-powered improvement"
                        );
                        // 写入 `Utc::now()` 作改进中标记；冷却看 elapsed > `IMPROVE_COOLDOWN_SECS`
                        // 成功后再写成实际完成时间，失败回滚
                        let improving_marker = Utc::now();
                        {
                            let mut stats = self.stats.lock().await;
                            if let Some(entry) = stats.get_mut(stats_key) {
                                entry.last_improved_at = Some(improving_marker);
                            }
                            self.mark_stats_dirty();
                        }
                        // 后台 AI 改进
                        let skill_id_owned = skill_id.to_string();
                        let stats_key_owned = stats_key.to_string();
                        let failure_reason_owned = failure_reason.map(String::from);
                        let old_instructions = skill.full_instructions.clone();
                        if let Some(evolution) = get_skill_evolution() {
                            let evo = evolution.clone();
                            crate::services::ai_cost_ledger::spawn_with_current_ai_attribution(
                                move || async move {
                                    match Self::ai_improve_skill(
                                        &evo,
                                        &skill_id_owned,
                                        &old_instructions,
                                        failure_reason_owned.as_deref(),
                                    )
                                    .await
                                    {
                                        Ok(()) => {
                                            // 成功：确认改进时间戳
                                            let mut stats = evo.stats.lock().await;
                                            if let Some(entry) = stats.get_mut(&stats_key_owned) {
                                                entry.last_improved_at = Some(Utc::now());
                                            }
                                            evo.mark_stats_dirty();
                                            drop(stats);
                                            evo.flush().await;
                                            if let Some(nm) = crate::services::agent::notifications::get_notification_manager()
                                        {
                                            nm.notify_skill_evolution(
                                                &skill_id_owned,
                                                "improved",
                                                "AI rewrote this auto-skill based on recent failures.",
                                            )
                                            .await;
                                        }
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                skill_id = %skill_id_owned,
                                                error = %e,
                                                "[SkillEvolution] AI improvement failed, rolling back cooldown"
                                            );
                                            // 失败：回滚 last_improved_at（允许下次失败重新触发）
                                            let mut stats = evo.stats.lock().await;
                                            if let Some(entry) = stats.get_mut(&stats_key_owned) {
                                                // 只回滚自己设置的时间戳，避免覆盖其他并发改进
                                                if entry.last_improved_at == Some(improving_marker)
                                                {
                                                    entry.last_improved_at = None;
                                                }
                                            }
                                            evo.mark_stats_dirty();
                                        }
                                    }
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    // AI 抽象化创建

    /// AI 驱动的 Skill 抽象化创建
    ///
    /// 与 `auto_create_skill_with_params` 不同：不是把用户原始输入当 trigger/name，
    /// 而是用 AI 从成功执行中提取可复用的抽象模式。
    ///
    /// 例如用户说"帮我看看最近B站有没有新番更新"，AI 会抽象为：
    /// - name: "平台内容更新检查"
    /// - triggers: ["更新", "新番", "最近内容", "检查更新"]
    /// - instructions: 参数化的通用流程（支持 ${platform}, ${content_type}）
    pub async fn auto_create_skill_abstracted(
        &self,
        user_input: &str,
        step_descriptions: &str,
        capabilities_used: &[String],
    ) -> Result<Skill, String> {
        use crate::config::ModelTier;
        use crate::services::ai::create_ai_analyzer_for_tier;

        let analyzer = create_ai_analyzer_for_tier(ModelTier::Standard)
            .await
            .ok_or("AI analyzer not available")?;

        let prompt = format!(
            "You extract reusable Skill templates from a successful run.\n\n\
            Original request: {}\n\n\
            Steps that ran:\n{}\n\n\
            Capabilities used: {}\n\n\
            Output this JSON only (no extra prose):\n\
            {{\n\
              \"name\": \"short Skill name (abstract; no person/show/platform names)\",\n\
              \"description\": \"one sentence on what this Skill does (abstract)\",\n\
              \"category\": \"category (media, social, game, data, creative)\",\n\
              \"triggers\": [\"3-6 abstract trigger phrases, include Chinese and English\"],\n\
              \"parameters\": [\"parameter slot names extracted from concrete values\"],\n\
              \"instructions\": \"parameterized instructions; use ${{param}} for variable parts\"\n\
            }}\n",
            user_input,
            step_descriptions,
            capabilities_used.join(", ")
        );

        let ai_result = analyzer
            .analyze(&prompt)
            .await
            .map_err(|error| skill_ai_failed("AI abstraction failed", error))?;

        // 解析 AI 输出的 JSON
        let json_str = extract_json_from_response(&ai_result)
            .ok_or("AI response does not contain valid JSON")?;

        let parsed: serde_json::Value = serde_json::from_str(&json_str).map_err(|error| {
            tracing::error!(%error, "Failed to parse skill AI JSON");
            "Failed to parse skill AI JSON".to_string()
        })?;

        let name = parsed["name"].as_str().unwrap_or("auto_skill").to_string();
        let description = parsed["description"].as_str().unwrap_or("").to_string();
        let category = parsed["category"].as_str().unwrap_or("auto").to_string();
        let triggers: Vec<String> = parsed["triggers"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let parameters: Vec<String> = parsed["parameters"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let instructions = parsed["instructions"]
            .as_str()
            .unwrap_or(step_descriptions)
            .to_string();

        if triggers.is_empty() || instructions.len() < 20 {
            return Err("AI abstraction produced insufficient content".to_string());
        }

        // 验证 triggers 质量：过短的触发词无效（容易误触发）
        let valid_triggers: Vec<String> = triggers
            .into_iter()
            .filter(|t| t.chars().count() >= 2)
            .collect();
        if valid_triggers.is_empty() {
            return Err("All triggers are too short (< 2 chars)".to_string());
        }

        // 验证参数在 instructions 中被引用
        let unreferenced: Vec<&str> = parameters
            .iter()
            .filter(|p| !instructions.contains(&format!("${{{}}}", p)))
            .map(|p| p.as_str())
            .collect();
        if !unreferenced.is_empty() {
            tracing::warn!(
                unreferenced = ?unreferenced,
                "[SkillEvolution] Parameters not referenced in instructions: {:?}",
                unreferenced
            );
        }

        // 验证 instructions 中的 ${} 都有闭合（精确扫描每个 ${ 是否有匹配的 }）
        if instructions.contains("${") {
            let mut unclosed = 0u32;
            let bytes = instructions.as_bytes();
            let mut i = 0;
            while i < bytes.len().saturating_sub(1) {
                if bytes[i] == b'$' && bytes[i + 1] == b'{' {
                    // 找到 ${，向后扫描直到找到 } 或到达末尾
                    if let Some(close_pos) = instructions[i + 2..].find('}') {
                        i = i + 2 + close_pos + 1; // 跳过整个 ${...}
                    } else {
                        unclosed += 1;
                        i += 2;
                    }
                } else {
                    i += 1;
                }
            }
            if unclosed > 0 {
                return Err(format!(
                    "Instructions have {} unclosed ${{}} placeholders",
                    unclosed
                ));
            }
        }

        // 委托给基础创建方法（传入参数声明）
        self.auto_create_skill_with_params(
            &name,
            &description,
            &valid_triggers,
            &category,
            &instructions,
            capabilities_used,
            &parameters,
        )
        .await
    }

    // AI 驱动的改进

    /// 使用 AI 生成改进后的 Skill 指令
    async fn ai_improve_skill(
        evolution: &Arc<SkillEvolution>,
        skill_id: &str,
        old_instructions: &str,
        failure_reason: Option<&str>,
    ) -> Result<(), String> {
        use crate::config::ModelTier;
        use crate::services::ai::create_ai_analyzer_for_tier;

        let analyzer = create_ai_analyzer_for_tier(ModelTier::Standard)
            .await
            .ok_or("AI analyzer not available")?;

        let reason_ctx = failure_reason
            .map(|r| format!("\nRecent failure reason: {}", r))
            .unwrap_or_default();

        let prompt = format!(
            "You improve Skill instructions. The Skill below has been failing often.\n\n\
            Current instructions:\n{}\n{}\n\n\
            Rewrite them to be more robust and precise:\n\
            1. Keep the original intent\n\
            2. Add error handling and edge-case checks\n\
            3. Make parameter matching more exact\n\
            4. Output the improved instruction text only, no explanation\n",
            old_instructions, reason_ctx
        );

        let new_instructions = analyzer
            .analyze(&prompt)
            .await
            .map_err(|error| skill_ai_failed("Skill AI generation failed", error))?;

        if new_instructions.len() < 20 {
            return Err("AI generated instructions too short".to_string());
        }

        // 绕过冷却检查：调用方 on_execution_complete 已做过冷却判定，
        // 并且刚写入了"改进中"时间戳标记——不绕过的话这里会永远撞上自己的标记
        evolution
            .improve_skill_inner(skill_id, &new_instructions, true)
            .await?;

        tracing::info!(
            skill_id = skill_id,
            "[SkillEvolution] AI improvement completed"
        );
        Ok(())
    }

    // 自动创建

    /// 自动创建 Skill（带参数声明）
    #[allow(clippy::too_many_arguments)]
    pub async fn auto_create_skill_with_params(
        &self,
        name: &str,
        description: &str,
        triggers: &[String],
        category: &str,
        instructions: &str,
        required_capabilities: &[String],
        parameters: &[String],
    ) -> Result<Skill, String> {
        self.auto_create_skill_inner(
            name,
            description,
            triggers,
            category,
            instructions,
            required_capabilities,
            parameters,
        )
        .await
    }

    /// 自动创建 Skill 内部实现
    #[allow(clippy::too_many_arguments)]
    async fn auto_create_skill_inner(
        &self,
        name: &str,
        description: &str,
        triggers: &[String],
        category: &str,
        instructions: &str,
        required_capabilities: &[String],
        parameters: &[String],
    ) -> Result<Skill, String> {
        // 检查每日限额（在 daily_date mutex 保护下检查，避免 TOCTOU）
        {
            let today = Utc::now().date_naive();
            let mut date = self.daily_date.lock().await;
            if *date != today {
                *date = today;
                self.daily_create_count.store(0, Ordering::SeqCst);
            }
            let current = self.daily_create_count.load(Ordering::SeqCst);
            if current >= DAILY_AUTO_CREATE_LIMIT {
                return Err(format!(
                    "Daily auto-create limit reached ({}/{})",
                    current, DAILY_AUTO_CREATE_LIMIT
                ));
            }
            // 在 mutex 保护下递增，确保原子性
            self.daily_create_count.fetch_add(1, Ordering::SeqCst);
        }

        // 验证所需能力是否存在
        if let Err(e) = self.validate_capabilities(required_capabilities).await {
            self.daily_create_count.fetch_sub(1, Ordering::SeqCst);
            return Err(e);
        }

        // 去重检查：如果已有高度相似的 Skill，跳过创建或改进已有 Skill
        if let Some(registry) = get_skill_registry() {
            // 用 name+描述+触发词构建查询文本
            let query_text = format!("{} {} {}", name, description, triggers.join(" "));
            let existing_matches = registry.get_relevant_skills(&query_text, 3).await;

            for m in &existing_matches {
                if m.relevance > 1.5 {
                    if m.skill.origin != SkillOrigin::Manual {
                        tracing::info!(
                            existing = %m.skill.name,
                            relevance = m.relevance,
                            "[SkillEvolution] Similar skill exists, improving instead of creating"
                        );
                        // 改进已有 skill 的指令
                        let _ = self.improve_skill(&m.skill.id, instructions).await;
                        self.daily_create_count.fetch_sub(1, Ordering::SeqCst);
                        return Err(format!(
                            "Similar skill '{}' already exists (relevance={:.2}), improved it instead",
                            m.skill.name, m.relevance
                        ));
                    } else {
                        tracing::info!(
                            existing = %m.skill.name,
                            "[SkillEvolution] Similar manual skill exists, skipping auto-create"
                        );
                        self.daily_create_count.fetch_sub(1, Ordering::SeqCst);
                        return Err(format!(
                            "Similar manual skill '{}' already exists, skipping creation",
                            m.skill.name
                        ));
                    }
                }
            }
        }

        // 生成安全文件名
        let safe_name = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect::<String>();
        let file_name = format!("_auto_{}.md", safe_name);
        let file_path = self.skills_dir.join(&file_name);

        // 对 YAML 字符串值做转义（防止 AI 输出含换行/引号破坏 frontmatter 结构）
        let escape_yaml = |s: &str| -> String {
            s.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', " ")
                .replace('\r', "")
        };

        let name_safe = escape_yaml(name);
        let desc_safe = escape_yaml(description);
        let category_safe = escape_yaml(category);

        // 构建 YAML frontmatter
        let triggers_yaml = triggers
            .iter()
            .map(|t| format!("\"{}\"", escape_yaml(t)))
            .collect::<Vec<_>>()
            .join(", ");
        let caps_yaml = required_capabilities
            .iter()
            .map(|c| format!("\"{}\"", escape_yaml(c)))
            .collect::<Vec<_>>()
            .join(", ");

        let params_yaml = if parameters.is_empty() {
            String::new()
        } else {
            let items = parameters
                .iter()
                .map(|p| format!("\"{}\"", escape_yaml(p)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("\nparameters: [{}]", items)
        };

        let content = format!(
            r#"---
name: {name_safe}
description: "{desc_safe}"
category: {category_safe}
triggers: [{triggers_yaml}]{params_yaml}
tier_hint: standard
gating:
  capabilities: [{caps_yaml}]
origin: agent_generated
---

{instructions}
"#
        );

        // 原子写入（失败时回滚 daily_create_count）
        let tmp_path = self.skills_dir.join(format!(".tmp_{}", file_name));
        if let Err(e) = tokio::fs::write(&tmp_path, &content).await {
            self.daily_create_count.fetch_sub(1, Ordering::SeqCst);
            return Err(format!("Failed to write skill file: {}", e));
        }
        if let Err(e) = tokio::fs::rename(&tmp_path, &file_path).await {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            self.daily_create_count.fetch_sub(1, Ordering::SeqCst);
            return Err(format!("Failed to rename skill file: {}", e));
        }

        // （daily_create_count 已在入口处原子递增，无需再次增加）

        // 重新加载 Skills
        if let Some(registry) = get_skill_registry() {
            registry.reload().await;
        }

        tracing::info!(
            name = name,
            file = %file_path.display(),
            "[SkillEvolution] Auto-created skill"
        );

        let skill = Skill {
            id: format!("_auto_{}", safe_name),
            name: name.to_string(),
            description: description.to_string(),
            full_instructions: instructions.to_string(),
            triggers: triggers.to_vec(),
            category: category.to_string(),
            gating: super::skill::SkillGating {
                platforms: Vec::new(),
                capabilities: required_capabilities.to_vec(),
            },
            tier_hint: Some(super::skill::ModelTierHint::Standard),
            origin: SkillOrigin::AgentGenerated,
            parameters: parameters.to_vec(),
            file_path,
            loaded_at: Some(std::time::Instant::now()),
        };

        Ok(skill)
    }

    // 改进

    /// 改进 Skill：更新指令内容（仅限 Agent 生成的 Skill），带冷却检查
    pub async fn improve_skill(
        &self,
        skill_id: &str,
        new_instructions: &str,
    ) -> Result<(), String> {
        self.improve_skill_inner(skill_id, new_instructions, false)
            .await
    }

    /// 改进 Skill 内部实现
    ///
    /// `bypass_cooldown`: AI 自动改进路径由 on_execution_complete 统一做冷却判定，
    /// 且已写入"改进中"时间戳防并发，此处必须跳过冷却检查。
    async fn improve_skill_inner(
        &self,
        skill_id: &str,
        new_instructions: &str,
        bypass_cooldown: bool,
    ) -> Result<(), String> {
        let registry = get_skill_registry().ok_or("Skill registry not initialized")?;
        let skill = registry
            .get(skill_id)
            .await
            .ok_or_else(|| format!("Skill not found: {}", skill_id))?;

        if skill.origin == SkillOrigin::Manual {
            return Err("Cannot modify manual skills".to_string());
        }

        // 检查冷却时间
        if !bypass_cooldown {
            let stats = self.stats.lock().await;
            if let Some(stat) = stats.get(skill_id) {
                if let Some(last) = stat.last_improved_at {
                    let elapsed = (Utc::now() - last).num_seconds();
                    if elapsed < IMPROVE_COOLDOWN_SECS {
                        return Err(format!(
                            "Skill improvement on cooldown ({} seconds remaining)",
                            IMPROVE_COOLDOWN_SECS - elapsed
                        ));
                    }
                }
            }
        }

        // 备份原文件
        let bak_path = skill.file_path.with_extension("md.bak");
        if skill.file_path.exists() {
            tokio::fs::copy(&skill.file_path, &bak_path)
                .await
                .map_err(|error| skill_io_failed("Failed to backup skill", error))?;
        }

        // 读取原文件，替换 body 部分，保留 frontmatter
        let original = tokio::fs::read_to_string(&skill.file_path)
            .await
            .map_err(|error| skill_io_failed("Failed to read skill file", error))?;

        let new_content = if let Some(idx) = original.find("\n---\n") {
            let frontmatter = &original[..idx];
            // 逐行处理：只改 origin 行（避免误伤 name/description 中的同名文本）；
            // 丢弃过期的 parameters 行，重载时会从新指令的 ${} 槽位重新提取
            let updated_fm = frontmatter
                .lines()
                .filter(|l| !l.trim_start().starts_with("parameters:"))
                .map(|l| {
                    if l.trim_start().starts_with("origin:") {
                        "origin: agent_improved"
                    } else {
                        l
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("{}\n---\n\n{}\n", updated_fm, new_instructions)
        } else {
            return Err("Invalid skill file format".to_string());
        };

        // 原子写入
        let tmp_path = skill.file_path.with_extension("md.tmp");
        tokio::fs::write(&tmp_path, &new_content)
            .await
            .map_err(|error| skill_io_failed("Failed to write skill file", error))?;
        tokio::fs::rename(&tmp_path, &skill.file_path)
            .await
            .map_err(|error| skill_io_failed("Failed to replace skill file", error))?;

        // 更新统计
        {
            let mut stats = self.stats.lock().await;
            let entry = stats.entry(skill_id.to_string()).or_default();
            entry.last_improved_at = Some(Utc::now());
            entry.consecutive_failures = 0;
            self.mark_stats_dirty();
        }

        // 重新加载
        if let Some(registry) = get_skill_registry() {
            registry.reload().await;
        }

        tracing::info!(skill_id = skill_id, "[SkillEvolution] Improved skill");
        Ok(())
    }

    // 能力缺口检测

    /// 能力缺口检测：当用户请求无法被任何能力/Skill 满足时
    ///
    /// 由 `process_work` 在 Planner 返回 `PlannerStatus::Unsupported` 时调用。
    pub async fn detect_capability_gap(
        &self,
        user_request: &str,
        missing_description: &str,
    ) -> CapabilityGap {
        let has_http_fetch = {
            let registry = capability::get_registry().await;
            registry.get("http.fetch").is_some()
        };

        let workaround = if has_http_fetch {
            Some("Try implementing it by calling an external API with http.fetch".to_string())
        } else {
            None
        };

        let mut gaps = self.capability_gaps.lock().await;

        // 查找已有的相同缺口
        let existing = gaps
            .iter_mut()
            .find(|g| g.missing_capability == missing_description);

        if let Some(existing) = existing {
            existing.report_count += 1;
            existing.confidence = (existing.confidence + 0.2).min(1.0);
            self.mark_gaps_dirty();
            return existing.clone();
        }

        // 新缺口
        let gap = CapabilityGap {
            description: user_request.to_string(),
            missing_capability: missing_description.to_string(),
            workaround,
            suggestion: format!("Suggest adding a new handler for: {missing_description}"),
            confidence: 0.3,
            first_seen: Utc::now().to_rfc3339(),
            report_count: 1,
        };
        gaps.push(gap.clone());

        // 保留最近 50 个缺口记录
        if gaps.len() > 50 {
            let excess = gaps.len() - 50;
            gaps.drain(0..excess);
        }

        self.mark_gaps_dirty();
        gap
    }

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
                let lookup_id = skill_id.strip_prefix("skill:").unwrap_or(&skill_id);
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
    pub async fn delete_skill(&self, skill_id: &str) -> Result<(), String> {
        let registry = get_skill_registry().ok_or("Skill registry not initialized")?;
        let skill = registry
            .get(skill_id)
            .await
            .ok_or_else(|| format!("Skill not found: {}", skill_id))?;

        if skill.origin == SkillOrigin::Manual {
            return Err("Cannot delete manual skills".to_string());
        }

        if skill.file_path.exists() {
            soft_delete_skill_file(&skill).await?;
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

    // 内部方法

    /// 验证所需能力是否全部存在
    async fn validate_capabilities(&self, required: &[String]) -> Result<(), String> {
        let registry = capability::get_registry().await;
        for cap_id in required {
            if registry.get(cap_id).is_none() {
                return Err(format!("Required capability not found: {}", cap_id));
            }
        }
        Ok(())
    }
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

/// 从 AI 响应中提取 JSON 块（支持 ```json 包裹和裸 JSON）
fn extract_json_from_response(response: &str) -> Option<String> {
    // 尝试 ```json ... ``` 包裹
    if let Some(start) = response.find("```json") {
        let after = &response[start + 7..];
        if let Some(end) = after.find("```") {
            return Some(after[..end].trim().to_string());
        }
    }
    // 尝试 ``` ... ``` 包裹
    if let Some(start) = response.find("```") {
        let after = &response[start + 3..];
        if let Some(end) = after.find("```") {
            let inner = after[..end].trim();
            if inner.starts_with('{') {
                return Some(inner.to_string());
            }
        }
    }
    // 尝试裸 JSON
    if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            if end > start {
                return Some(response[start..=end].to_string());
            }
        }
    }
    None
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

    // 后台定时任务：定期 flush + 每日 prune
    tokio::spawn(async move {
        let flush_interval = tokio::time::Duration::from_secs(5 * 60); // 5 min
        let prune_interval_ticks = 288; // 288 * 5min = 24h
        let mut tick_count: u64 = 0;

        loop {
            tokio::time::sleep(flush_interval).await;
            tick_count += 1;

            // 每 5 分钟 flush
            evolution.flush().await;

            // 每 24 小时 prune
            if tick_count.is_multiple_of(prune_interval_ticks) {
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
    });
}

/// 获取全局 SkillEvolution
pub fn get_skill_evolution() -> Option<&'static Arc<SkillEvolution>> {
    SKILL_EVOLUTION.get()
}
