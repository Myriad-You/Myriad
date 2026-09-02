//! 动态 Skill 系统（语义匹配 + 参数化模板）
//!
//! Skills 是 Markdown 文件定义的能力编排模板。
//! 支持热加载、语义匹配、参数化执行、以及 Agent 自动创建。
//!
//! 当前实现：
//! - **语义模糊匹配**：trigger 不再要求精确子串，支持 TF-IDF 关键词重叠
//! - **参数化模板**：instructions 支持 ${param} 槽位，AI 从用户输入填充
//! - **质量评分**：Skill 索引带执行统计，AI 优先选高质量 Skill
//! - **紧凑索引优化**：仅注入 top-K 相关 Skill，减少 token 浪费

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::RwLock;

use crate::config::ModelTier;

/// Skill 定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// 唯一 ID（即文件名去掉 .md，frontmatter 中可省略）
    #[serde(default)]
    pub id: String,
    /// 显示名称
    #[serde(default)]
    pub name: String,
    /// 一句话描述（用于 compact index）
    #[serde(default)]
    pub description: String,
    /// 完整 Markdown 指令内容（按需加载）
    #[serde(skip)]
    pub full_instructions: String,
    /// 触发关键词（支持语义模糊匹配）
    #[serde(default)]
    pub triggers: Vec<String>,
    /// 分类
    #[serde(default)]
    pub category: String,
    /// 前置条件
    #[serde(default)]
    pub gating: SkillGating,
    /// 建议使用的模型层级
    #[serde(default)]
    pub tier_hint: Option<ModelTierHint>,
    /// 来源
    #[serde(default)]
    pub origin: SkillOrigin,
    /// 参数槽位定义（如 ["character_name", "style", "count"]）
    #[serde(default)]
    pub parameters: Vec<String>,
    /// 文件路径
    #[serde(skip)]
    pub file_path: PathBuf,
    /// 加载时间
    #[serde(skip)]
    pub loaded_at: Option<Instant>,
}

/// Skill 前置条件
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillGating {
    /// 需要用户已绑定的平台
    #[serde(default)]
    pub platforms: Vec<String>,
    /// 依赖的能力 ID
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// Whether this skill's gating capabilities are all in the grant set.
///
/// Empty gating stays visible (expansion is still grant-filtered at execute).
/// Unknown gating ids fail closed. `granted = None` is unfiltered.
pub async fn skill_covered_by_grants(
    skill: &Skill,
    granted: Option<&std::collections::HashSet<String>>,
) -> bool {
    let Some(granted) = granted else {
        return true;
    };
    if skill.gating.capabilities.is_empty() {
        return true;
    }
    let registry = super::capability::get_registry().await;
    for cap_id in &skill.gating.capabilities {
        let Some(cap) = registry.get(cap_id) else {
            return false;
        };
        if !super::capability::capability_covered_by_grants(cap, Some(granted)) {
            return false;
        }
    }
    true
}

/// Skill 来源
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SkillOrigin {
    /// 开发者手写
    #[default]
    Manual,
    /// Agent 自动创建
    AgentGenerated,
    /// Agent 基于反馈改进
    AgentImproved,
}

/// ModelTier 提示（序列化友好版）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelTierHint {
    Pro,
    Standard,
}

impl ModelTierHint {
    pub fn to_model_tier(&self) -> ModelTier {
        match self {
            ModelTierHint::Pro => ModelTier::Pro,
            ModelTierHint::Standard => ModelTier::Standard,
        }
    }
}

/// Skill 匹配结果（带相关性分数）
#[derive(Debug, Clone)]
pub struct SkillMatch {
    pub skill: Skill,
    pub relevance: f32,
}

/// Skill 注册表
pub struct SkillRegistry {
    /// id -> Skill
    skills: RwLock<HashMap<String, Skill>>,
    /// skills 目录路径
    skills_dir: PathBuf,
}

impl SkillRegistry {
    /// 创建并从目录加载所有 Skills
    pub async fn new(skills_dir: PathBuf) -> Self {
        // 确保目录存在，否则技能自动创建 / 统计持久化会全部写盘失败
        if let Err(e) = tokio::fs::create_dir_all(&skills_dir).await {
            tracing::warn!(
                "[SkillRegistry] Failed to create skills dir {}: {}",
                skills_dir.display(),
                e
            );
        }
        let registry = Self {
            skills: RwLock::new(HashMap::new()),
            skills_dir,
        };
        registry.load_all().await;
        registry
    }

    /// 从目录加载所有 .md 文件
    async fn load_all(&self) {
        if let Some(new_skills) = self.load_all_into_map().await {
            let mut skills = self.skills.write().await;
            *skills = new_skills;
        }
    }

    /// 加载所有 Skill 文件到独立 HashMap（不写入 self.skills）
    ///
    /// 返回 `None` 表示目录不可读（IO 错误）；`Some(空 map)` 表示目录合法但没有技能。
    async fn load_all_into_map(&self) -> Option<HashMap<String, Skill>> {
        let dir = &self.skills_dir;
        let mut result = HashMap::new();

        let mut entries = match tokio::fs::read_dir(dir).await {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!("[SkillRegistry] Failed to read skills dir: {}", e);
                return None;
            }
        };

        let mut count = 0;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if fname.starts_with('_') && !fname.starts_with("_auto_") {
                    continue;
                }
                if let Some(skill) = Self::parse_skill_file(&path).await {
                    tracing::debug!("[SkillRegistry] Loaded skill: {}", skill.id);
                    result.insert(skill.id.clone(), skill);
                    count += 1;
                }
            }
        }

        tracing::info!(
            "[SkillRegistry] Loaded {} skills from {}",
            count,
            dir.display()
        );
        Some(result)
    }

    /// 解析单个 Skill 文件（YAML frontmatter + Markdown body）
    async fn parse_skill_file(path: &Path) -> Option<Skill> {
        let content = tokio::fs::read_to_string(path).await.ok()?;
        let id = path.file_stem()?.to_str()?.to_string();

        // 解析 YAML frontmatter（--- 分隔）
        let (frontmatter, body) = Self::split_frontmatter(&content)?;

        // 反序列化 frontmatter
        let mut skill: Skill = match serde_yaml::from_str(&frontmatter) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    "[SkillRegistry] Failed to parse frontmatter in {}: {}",
                    path.display(),
                    e
                );
                return None;
            }
        };

        skill.id = id;
        skill.full_instructions = body;
        skill.file_path = path.to_path_buf();
        skill.loaded_at = Some(Instant::now());

        // 从 instructions 中自动提取参数槽位 ${param_name}
        if skill.parameters.is_empty() {
            skill.parameters = Self::extract_parameter_slots(&skill.full_instructions);
        }

        Some(skill)
    }

    /// 分割 YAML frontmatter 和 Markdown body
    fn split_frontmatter(content: &str) -> Option<(String, String)> {
        let trimmed = content.trim_start();
        if !trimmed.starts_with("---") {
            return None;
        }

        // 跳过第一个 ---
        let after_first = &trimmed[3..];
        let end_idx = after_first.find("\n---")?;
        let frontmatter = after_first[..end_idx].trim().to_string();
        let body = after_first[end_idx + 4..].trim().to_string();

        Some((frontmatter, body))
    }

    /// 从 instructions 文本中提取 ${param} 槽位
    fn extract_parameter_slots(instructions: &str) -> Vec<String> {
        let mut params = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut chars = instructions.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '$' && chars.peek() == Some(&'{') {
                chars.next(); // consume '{'
                let mut param = String::new();
                let mut found_closing = false;
                // 限制参数名长度，防止未闭合 ${ 消耗整个文档
                for c in chars.by_ref().take(100) {
                    if c == '}' {
                        found_closing = true;
                        break;
                    }
                    param.push(c);
                }
                if found_closing && !param.is_empty() && seen.insert(param.clone()) {
                    params.push(param);
                }
            }
        }
        params
    }

    /// 获取所有 Skill 的紧凑索引（用于 AI 提示）
    pub async fn get_compact_index(&self) -> Vec<Value> {
        let skills = self.skills.read().await;
        skills
            .values()
            .map(|s| {
                let mut entry = json!({
                    "id": format!("skill:{}", s.id),
                    "h": s.description,
                });
                // 添加参数提示
                if !s.parameters.is_empty() {
                    entry
                        .as_object_mut()
                        .unwrap()
                        .insert("params".to_string(), json!(s.parameters));
                }
                entry
            })
            .collect()
    }

    /// 获取与用户输入最相关的 Skill（语义模糊匹配）
    ///
    /// 相比 get_compact_index() 的全量返回，这个方法用 TF-IDF 关键词重叠
    /// 预过滤出 top-K 最相关的 Skill，减少 Planner 的 token 消耗。
    pub async fn get_relevant_skills(&self, user_input: &str, limit: usize) -> Vec<SkillMatch> {
        let input_lower = user_input.to_lowercase();
        let input_tokens = Self::simple_tokenize(&input_lower);
        let skills = self.skills.read().await;

        // 获取 skill 统计数据（成功率），用于质量加权
        let stats_map = if let Some(evo) = super::skill_evolution::get_skill_evolution() {
            evo.get_all_stats().await
        } else {
            std::collections::HashMap::new()
        };

        let mut scored: Vec<SkillMatch> = skills
            .values()
            .filter_map(|skill| {
                let mut relevance =
                    Self::compute_skill_relevance(skill, &input_lower, &input_tokens);
                if relevance > 0.05 {
                    // 质量加权：使用 Wilson score lower bound，对小样本更宽容
                    // 参考: https://en.wikipedia.org/wiki/Binomial_proportion_confidence_interval#Wilson_score_interval
                    if let Some(stats) = stats_map.get(&skill.id) {
                        let total = stats.success_count + stats.failure_count;
                        if total >= 3 {
                            let n = total as f32;
                            let p = stats.success_count as f32 / n;
                            // z = 1.0 (较低置信度，对新 skill 更宽容)
                            let z = 1.0_f32;
                            let z2 = z * z;
                            // Wilson score lower bound
                            let wilson_lower = (p + z2 / (2.0 * n)
                                - z * ((p * (1.0 - p) + z2 / (4.0 * n)) / n).sqrt())
                                / (1.0 + z2 / n);
                            // 映射到 [0.6, 1.0] 范围，避免惩罚过重
                            // wilson_lower(z=1.0): n=3,s=2 → ≈0.38 → ×0.75
                            // n=5,s=4 → ≈0.57 → ×0.83, n=10,s=9 → ≈0.74 → ×0.90
                            relevance *= 0.6 + 0.4 * wilson_lower.max(0.0);
                        }
                    }
                    Some(SkillMatch {
                        skill: skill.clone(),
                        relevance,
                    })
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|a, b| {
            b.relevance
                .partial_cmp(&a.relevance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(limit);
        scored
    }

    /// 计算 Skill 与用户输入的相关性分数
    fn compute_skill_relevance(skill: &Skill, input_lower: &str, input_tokens: &[String]) -> f32 {
        let mut score: f32 = 0.0;

        // 1. Trigger 精确命中（最高权重）
        for trigger in &skill.triggers {
            let trigger_lower = trigger.to_lowercase();
            if input_lower.contains(&trigger_lower) {
                score += 1.0;
            } else {
                // Trigger 中的关键词部分命中
                let trigger_tokens = Self::simple_tokenize(&trigger_lower);
                let overlap = trigger_tokens
                    .iter()
                    .filter(|t| input_tokens.contains(t))
                    .count();
                if !trigger_tokens.is_empty() {
                    let overlap_ratio = overlap as f32 / trigger_tokens.len() as f32;
                    score += overlap_ratio * 0.6;
                }
            }
        }

        // 2. 描述关键词重叠
        let desc_tokens = Self::simple_tokenize(&skill.description.to_lowercase());
        if !desc_tokens.is_empty() {
            let desc_overlap = desc_tokens
                .iter()
                .filter(|t| input_tokens.contains(t))
                .count();
            let desc_ratio = desc_overlap as f32 / desc_tokens.len().max(1) as f32;
            score += desc_ratio * 0.3;
        }

        // 3. 分类命中
        if !skill.category.is_empty() && input_lower.contains(&skill.category.to_lowercase()) {
            score += 0.2;
        }

        score
    }

    /// 简单分词（中文单字 + 英文空格分词）
    fn simple_tokenize(text: &str) -> Vec<String> {
        let mut tokens = Vec::new();
        let mut latin_buf = String::new();

        for ch in text.chars() {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                latin_buf.push(ch);
            } else {
                if latin_buf.len() >= 2 {
                    tokens.push(latin_buf.clone());
                }
                latin_buf.clear();

                if is_cjk(ch) {
                    tokens.push(ch.to_string());
                }
            }
        }
        if latin_buf.len() >= 2 {
            tokens.push(latin_buf);
        }
        tokens
    }

    /// 根据 ID 获取完整 Skill
    pub async fn get(&self, id: &str) -> Option<Skill> {
        let skills = self.skills.read().await;
        skills.get(id).cloned()
    }

    /// 获取所有 Skills
    pub async fn get_all(&self) -> Vec<Skill> {
        let skills = self.skills.read().await;
        skills.values().cloned().collect()
    }

    /// 重新加载所有 Skills
    ///
    /// 先加载到临时容器，成功后再替换。仅当目录不可读（IO 错误）时保留旧注册表；
    /// 目录合法为空时正常清空，否则被删除/淘汰的技能会以"僵尸"形式残留在内存中。
    pub async fn reload(&self) {
        match self.load_all_into_map().await {
            Some(new_skills) => {
                let mut skills = self.skills.write().await;
                *skills = new_skills;
            }
            None => {
                tracing::warn!(
                    "[SkillRegistry] reload failed to read dir, keeping existing skills"
                );
            }
        }
    }
}

/// CJK unicode 范围检测
fn is_cjk(ch: char) -> bool {
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}'
        | '\u{3400}'..='\u{4DBF}'
        | '\u{3040}'..='\u{309F}'
        | '\u{30A0}'..='\u{30FF}'
        | '\u{AC00}'..='\u{D7AF}'
    )
}

/// 全局 Skill 注册表
static SKILL_REGISTRY: once_cell::sync::OnceCell<Arc<SkillRegistry>> =
    once_cell::sync::OnceCell::new();

/// 初始化全局 Skill 注册表
pub async fn init_skills(skills_dir: PathBuf) {
    let registry = Arc::new(SkillRegistry::new(skills_dir).await);
    let _ = SKILL_REGISTRY.set(registry);
}

/// 获取全局 Skill 注册表
pub fn get_skill_registry() -> Option<&'static Arc<SkillRegistry>> {
    SKILL_REGISTRY.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_skill(id: &str, triggers: &[&str], desc: &str, category: &str) -> Skill {
        Skill {
            id: id.to_string(),
            name: id.to_string(),
            description: desc.to_string(),
            full_instructions: String::new(),
            triggers: triggers.iter().map(|s| s.to_string()).collect(),
            category: category.to_string(),
            gating: SkillGating::default(),
            tier_hint: None,
            origin: SkillOrigin::Manual,
            parameters: Vec::new(),
            file_path: PathBuf::new(),
            loaded_at: None,
        }
    }

    #[test]
    fn test_skill_relevance_exact_trigger() {
        let skill = make_skill("img", &["生成图片"], "AI 图片生成", "creative");
        let input = "帮我生成图片";
        let input_lower = input.to_lowercase();
        let tokens = SkillRegistry::simple_tokenize(&input_lower);
        let score = SkillRegistry::compute_skill_relevance(&skill, &input_lower, &tokens);
        assert!(
            score >= 1.0,
            "exact trigger match should score >= 1.0, got {score}"
        );
    }

    #[test]
    fn test_skill_relevance_partial_trigger() {
        let skill = make_skill(
            "img",
            &["generate beautiful image"],
            "image generation",
            "creative",
        );
        let input = "generate image";
        let input_lower = input.to_lowercase();
        let tokens = SkillRegistry::simple_tokenize(&input_lower);
        let score = SkillRegistry::compute_skill_relevance(&skill, &input_lower, &tokens);
        assert!(
            score > 0.0,
            "partial trigger overlap should score > 0, got {score}"
        );
        assert!(
            score < 1.0,
            "partial trigger should score < 1.0, got {score}"
        );
    }

    #[test]
    fn test_skill_relevance_no_match() {
        let skill = make_skill("music", &["播放音乐"], "音乐播放", "media");
        let input = "generate an image";
        let input_lower = input.to_lowercase();
        let tokens = SkillRegistry::simple_tokenize(&input_lower);
        let score = SkillRegistry::compute_skill_relevance(&skill, &input_lower, &tokens);
        assert!(
            score < 0.1,
            "unrelated skill should score near 0, got {score}"
        );
    }

    #[test]
    fn test_skill_relevance_category_boost() {
        let skill = make_skill("img", &[], "image generation", "creative");
        let input = "creative image";
        let input_lower = input.to_lowercase();
        let tokens = SkillRegistry::simple_tokenize(&input_lower);
        let score = SkillRegistry::compute_skill_relevance(&skill, &input_lower, &tokens);
        assert!(
            score >= 0.2,
            "category match should contribute >= 0.2, got {score}"
        );
    }

    #[test]
    fn test_wilson_score_bounds() {
        // Wilson score lower bound for p=1.0, n=3, z=1.0
        let n = 3.0_f32;
        let p = 1.0_f32;
        let z = 1.0_f32;
        let z2 = z * z;
        let wilson = (p + z2 / (2.0 * n) - z * ((p * (1.0 - p) + z2 / (4.0 * n)) / n).sqrt())
            / (1.0 + z2 / n);
        // All success: wilson should be high
        let multiplier = 0.6 + 0.4 * wilson.max(0.0);
        assert!(
            multiplier >= 0.9,
            "all-success should give multiplier >= 0.9, got {multiplier}"
        );

        // All failure: p=0.0
        let p = 0.0_f32;
        let wilson = (p + z2 / (2.0 * n) - z * ((p * (1.0 - p) + z2 / (4.0 * n)) / n).sqrt())
            / (1.0 + z2 / n);
        let multiplier = 0.6 + 0.4 * wilson.max(0.0);
        assert!(
            multiplier <= 0.7,
            "all-failure should give multiplier <= 0.7, got {multiplier}"
        );
    }

    #[test]
    fn test_extract_parameter_slots() {
        let instructions = "Generate ${character_name} in ${style} style, count: ${count}";
        let params = SkillRegistry::extract_parameter_slots(instructions);
        assert_eq!(params, vec!["character_name", "style", "count"]);
    }

    #[test]
    fn test_extract_parameter_slots_unclosed() {
        // 未闭合的 ${ 不应产出参数
        let instructions = "Hello ${name} and ${broken";
        let params = SkillRegistry::extract_parameter_slots(instructions);
        assert_eq!(params, vec!["name"]);
    }

    #[test]
    fn test_extract_parameter_slots_dedup() {
        let instructions = "${x} and ${y} and ${x} again";
        let params = SkillRegistry::extract_parameter_slots(instructions);
        assert_eq!(params, vec!["x", "y"]);
    }

    #[test]
    fn test_simple_tokenize_cjk() {
        let tokens = SkillRegistry::simple_tokenize("生成图片");
        // CJK 单字 token
        assert!(tokens.contains(&"生".to_string()));
        assert!(tokens.contains(&"成".to_string()));
        assert!(tokens.contains(&"图".to_string()));
        assert!(tokens.contains(&"片".to_string()));
    }

    #[test]
    fn test_simple_tokenize_mixed() {
        let tokens = SkillRegistry::simple_tokenize("use openai to 生成");
        assert!(tokens.contains(&"use".to_string()));
        assert!(tokens.contains(&"openai".to_string()));
        assert!(tokens.contains(&"生".to_string()));
        assert!(tokens.contains(&"成".to_string()));
        // "to" 有 2 个字符，满足 >= 2 阈值，会保留
        assert!(tokens.contains(&"to".to_string()));
    }

    #[test]
    fn test_split_frontmatter() {
        let content = "---\nname: test\ntriggers: [hello]\n---\n\nInstructions here.";
        let (fm, body) = SkillRegistry::split_frontmatter(content).unwrap();
        assert!(fm.contains("name: test"));
        assert_eq!(body, "Instructions here.");
    }

    #[test]
    fn test_split_frontmatter_missing() {
        let content = "No frontmatter here";
        assert!(SkillRegistry::split_frontmatter(content).is_none());
    }

    /// Smoke: seeded manual skills under data/agent/skills must load via SkillRegistry.
    #[tokio::test]
    async fn test_load_seeded_brew_skills_from_data_dir() {
        let skills_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/agent/skills");
        assert!(
            skills_dir.is_dir(),
            "expected skills dir at {}",
            skills_dir.display()
        );

        let registry = SkillRegistry::new(skills_dir).await;
        let all = registry.get_all().await;
        let ids: Vec<String> = all.iter().map(|s| s.id.clone()).collect();

        for required in [
            "brew-friend-links",
            "brew-source-latest",
            "brew-latest-articles",
            "platform-status",
        ] {
            assert!(
                ids.iter().any(|id| id == required),
                "missing seeded skill {required}; loaded: {ids:?}"
            );
        }

        // Friend links: category filter on brew.sources, no fake caps
        let friend = registry
            .get("brew-friend-links")
            .await
            .expect("friend skill");
        assert_eq!(friend.origin, SkillOrigin::Manual);
        assert!(
            friend
                .gating
                .capabilities
                .contains(&"brew.sources".to_string()),
            "brew-friend-links must gate on brew.sources"
        );
        assert!(
            !friend.full_instructions.is_empty(),
            "instructions body required"
        );
        assert!(
            friend
                .triggers
                .iter()
                .any(|t| t.contains("友情链接") || t.contains("友链")),
            "zh triggers required"
        );

        // Source latest: sourceId handoff + parameter slot
        let source_latest = registry
            .get("brew-source-latest")
            .await
            .expect("source-latest skill");
        assert!(
            source_latest.parameters.iter().any(|p| p == "source_name"),
            "source_name parameter required, got {:?}",
            source_latest.parameters
        );
        assert!(
            source_latest
                .gating
                .capabilities
                .contains(&"brew.items".to_string()),
            "brew-source-latest must gate on brew.items"
        );
        let body_lower = source_latest.full_instructions.to_lowercase();
        assert!(
            body_lower.contains("sourceid"),
            "instructions must require passing sourceId from list step"
        );
        assert!(
            source_latest.full_instructions.contains("webSearch")
                || source_latest.full_instructions.contains("联网"),
            "instructions must mention forbidding web search for local lists"
        );

        // Latest articles: local-only
        let latest = registry
            .get("brew-latest-articles")
            .await
            .expect("latest skill");
        assert!(
            latest
                .gating
                .capabilities
                .contains(&"brew.items".to_string()),
            "brew-latest-articles must gate on brew.items"
        );
        assert!(
            latest
                .gating
                .capabilities
                .iter()
                .all(|c| c.starts_with("brew.") || c.starts_with("platform.")),
            "gating must list real brew/platform IDs only, got {:?}",
            latest.gating.capabilities
        );

        // Platform status: platform.read only
        let platform = registry
            .get("platform-status")
            .await
            .expect("platform skill");
        assert_eq!(
            platform.gating.capabilities,
            vec!["platform.read".to_string()]
        );
    }

    fn stub_skill(capabilities: Vec<String>) -> Skill {
        Skill {
            id: "stub".into(),
            name: "stub".into(),
            description: String::new(),
            full_instructions: String::new(),
            triggers: vec![],
            category: String::new(),
            gating: SkillGating {
                platforms: vec![],
                capabilities,
            },
            tier_hint: None,
            origin: SkillOrigin::Manual,
            parameters: vec![],
            file_path: PathBuf::new(),
            loaded_at: None,
        }
    }

    #[tokio::test]
    async fn skill_gating_follows_capability_grants() {
        use std::collections::HashSet;

        let skill = stub_skill(vec!["speech.tts".into()]);
        let without = HashSet::from(["ai:chat".to_string()]);
        assert!(
            !skill_covered_by_grants(&skill, Some(&without)).await,
            "speech.tts gating must not pass without speech:tts"
        );
        let with = HashSet::from(["speech:tts".to_string()]);
        assert!(skill_covered_by_grants(&skill, Some(&with)).await);
        assert!(skill_covered_by_grants(&skill, None).await);

        let open = stub_skill(vec![]);
        assert!(skill_covered_by_grants(&open, Some(&without)).await);

        let unknown = stub_skill(vec!["not.a.capability".into()]);
        assert!(!skill_covered_by_grants(&unknown, Some(&with)).await);
    }
}
