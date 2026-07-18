//! Agent 智能记忆系统 (v3 — 语义记忆 + AI 提取)
//!
//! 三级记忆架构：
//! - **ShortTerm**：当前对话上下文（高衰减，session 结束后归档）
//! - **MediumTerm**：会话摘要、近期交互模式（天级留存）
//! - **LongTerm**：用户偏好、重要事实、关键决策（永久）
//!
//! v3 改进：
//! - **AI 驱动的记忆提取**：任务完成后用 AI 从对话+执行结果中提取有价值记忆
//! - **结构化记忆类型**：偏好、知识纠错、执行教训、会话洞察
//! - **纠错学习**：检测用户纠错模式，自动提取实体知识
//! - **失败教训**：执行失败时记录教训，下次规划时规避已知坑
//! - **记忆去重与合并**：相同主题的记忆自动合并
//! - **容量管理**：上限 500 条，LRU 衰减淘汰
//!
//! 搜索采用 TF-IDF 余弦相似度 + 时间衰减 + 重要性权重的复合评分。
//!
//! 持久化：
//! - `memory.md`：人类可读的长期记忆（向后兼容）
//! - `memory_index.json`：完整索引状态（快速恢复）
//! - `YYYY-MM-DD.md`：每日交互日志

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::RwLock;

/// 记忆容量上限
const MAX_MEMORY_ENTRIES: usize = 500;

/// 记忆去重——TF-IDF 相似度超过此值认为重复
const DEDUP_SIMILARITY_THRESHOLD: f32 = 0.85;

/// 合并——TF-IDF 相似度超过此值认为可合并
const MERGE_SIMILARITY_THRESHOLD: f32 = 0.70;

// ==================== 类型定义 ====================

/// 记忆条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// 唯一 ID（内容 hash）
    pub id: String,
    /// 所属用户（None = 遗留/全局，仅系统可读；新写入必须带 user_id）
    #[serde(default)]
    pub user_id: Option<i32>,
    /// 记忆类型
    pub memory_type: MemoryType,
    /// 记忆层级
    #[serde(default = "default_tier")]
    pub tier: MemoryTier,
    /// 记忆内容
    pub content: String,
    /// 来源描述
    pub source: Option<String>,
    /// 重要性 (0.0 - 1.0)
    #[serde(default = "default_importance")]
    pub importance: f32,
    /// 访问次数
    #[serde(default)]
    pub access_count: u32,
    /// 创建时间
    pub created_at: String,
    /// 最后访问时间
    #[serde(default)]
    pub last_accessed_at: Option<String>,
    /// 关联实体（人物、平台、作品等）
    #[serde(default)]
    pub entities: Vec<String>,
    /// 关联能力 ID（该记忆涉及哪些 capability）
    #[serde(default)]
    pub related_capabilities: Vec<String>,
}

/// 条目是否对用户可见（严格：仅自己的；系统 user_id=0 可见全部）
fn entry_visible_to(entry: &MemoryEntry, user_id: i32) -> bool {
    if user_id == 0 {
        return true;
    }
    entry.user_id == Some(user_id)
}

fn default_tier() -> MemoryTier {
    MemoryTier::LongTerm
}
fn default_importance() -> f32 {
    0.5
}

/// 记忆类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    /// 用户偏好（"用户喜欢ACG风格"、"用户常用中文"）
    Preference,
    /// 实体知识（角色名纠错、作品关联等 — "昔涟是星穹铁道角色，不是希格雯"）
    EntityKnowledge,
    /// 执行教训（什么有效、什么失败 — "ai.image 不接受 loli 关键词"）
    ExecutionLesson,
    /// 有效参数模式（"生成角色图片时 category=anime 效果好"）
    EffectivePattern,
    /// 事实性记忆（向后兼容）
    Fact,
    /// 交互记录（向后兼容，新逻辑不再生成此类型）
    Interaction,
    /// 决策记录
    Decision,
    /// 会话洞察（从整个会话提炼的关键信息）
    SessionInsight,
    /// 会话摘要（向后兼容）
    SessionSummary,
}

/// 记忆层级
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub enum MemoryTier {
    /// 当前对话上下文（ephemeral）
    ShortTerm,
    /// 会话摘要、近期模式（天级留存）
    MediumTerm,
    /// 永久事实、偏好、决策
    LongTerm,
}

/// AI 提取的结构化记忆
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedMemory {
    /// 记忆内容
    pub content: String,
    /// 记忆类型
    pub memory_type: String,
    /// 重要性 0.0-1.0
    pub importance: f32,
    /// 关联实体
    #[serde(default)]
    pub entities: Vec<String>,
    /// 关联能力
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// AI 记忆提取结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryExtractionResult {
    pub memories: Vec<ExtractedMemory>,
}

// ==================== TF-IDF 搜索索引 ====================

/// 轻量级 TF-IDF 索引
///
/// 对中英文混合文本做 token 化，支持 CJK bigram + 拉丁单词分词。
/// 设计目标：百级文档集上的毫秒级搜索，零外部依赖。
///
/// IDF 采用惰性重建策略：add/remove 仅标记脏位，实际重建推迟到搜索时执行，
/// 避免批量写入时 O(n) × k 的重复计算。
struct TfIdfIndex {
    /// doc_id → { term → tf }
    tf: HashMap<String, HashMap<String, f32>>,
    /// term → idf
    idf: HashMap<String, f32>,
    /// 文档总数
    doc_count: usize,
    /// IDF 是否过期（需要重建）
    idf_dirty: bool,
}

impl TfIdfIndex {
    fn new() -> Self {
        Self {
            tf: HashMap::new(),
            idf: HashMap::new(),
            doc_count: 0,
            idf_dirty: false,
        }
    }

    /// 中文高频停用词（过滤以提升 TF-IDF 质量）
    const CJK_STOP_CHARS: &'static [char] = &[
        '的', '了', '是', '在', '和', '与', '也', '就', '都', '而', '及', '着', '或', '一', '不',
        '有', '这', '那', '个', '为', '以', '到', '对', '被', '从', '把', '让', '给', '向', '但',
        '又', '要', '会', '能', '可', '很',
    ];

    /// 对文本做 token 化
    ///
    /// 策略：拉丁文按空格分词并 lowercase，CJK 按字符 bigram 切分，过滤停用词
    fn tokenize(text: &str) -> Vec<String> {
        let text_lower = text.to_lowercase();
        let mut tokens = Vec::new();
        let mut latin_buf = String::new();
        let stop_set: std::collections::HashSet<char> =
            Self::CJK_STOP_CHARS.iter().cloned().collect();

        for ch in text_lower.chars() {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                latin_buf.push(ch);
            } else {
                // flush latin
                if latin_buf.len() >= 2 {
                    tokens.push(latin_buf.clone());
                }
                latin_buf.clear();

                // CJK character → 单字 token（过滤停用词）
                if is_cjk(ch) && !stop_set.contains(&ch) {
                    tokens.push(ch.to_string());
                }
            }
        }
        if latin_buf.len() >= 2 {
            tokens.push(latin_buf);
        }

        // CJK bigram：从原始 CJK 字符连续序列生成（不过滤停用词），
        // 确保存储和查询两侧对称。例如 "生成图像" → bigrams ["生成","成图","图像"]
        // 停用词仅影响上面的 unigram，不影响 bigram 生成
        let mut cjk_run: Vec<char> = Vec::new();
        for ch in text_lower.chars() {
            if is_cjk(ch) {
                cjk_run.push(ch);
            } else {
                // flush CJK run as bigrams
                for window in cjk_run.windows(2) {
                    tokens.push(format!("{}{}", window[0], window[1]));
                }
                cjk_run.clear();
            }
        }
        // flush trailing CJK run
        for window in cjk_run.windows(2) {
            tokens.push(format!("{}{}", window[0], window[1]));
        }

        tokens
    }

    /// 添加文档到索引（不立即重建 IDF，标记为脏）
    fn add_document(&mut self, doc_id: &str, text: &str) {
        // 截断过长文本，避免超大文档污染 TF-IDF 权重
        let truncated: String = text.chars().take(1000).collect();
        let tokens = Self::tokenize(&truncated);
        if tokens.is_empty() {
            return;
        }

        let total = tokens.len() as f32;
        let mut term_freq: HashMap<String, f32> = HashMap::new();
        for token in &tokens {
            *term_freq.entry(token.clone()).or_default() += 1.0;
        }
        // normalize TF
        for v in term_freq.values_mut() {
            *v /= total;
        }

        // 覆盖已有文档时不增加计数，防止 doc_count 膨胀导致 IDF 失真
        if self.tf.insert(doc_id.to_string(), term_freq).is_none() {
            self.doc_count += 1;
        }
        self.idf_dirty = true;
    }

    /// 移除文档
    fn remove_document(&mut self, doc_id: &str) {
        if self.tf.remove(doc_id).is_some() {
            self.doc_count = self.doc_count.saturating_sub(1);
            self.idf_dirty = true;
        }
    }

    /// 重建 IDF（在批量 add/remove 之后调用，或搜索前惰性触发）
    fn rebuild_idf(&mut self) {
        let n = self.doc_count.max(1) as f32;
        let mut df: HashMap<String, u32> = HashMap::new();

        for term_freqs in self.tf.values() {
            for term in term_freqs.keys() {
                *df.entry(term.clone()).or_default() += 1;
            }
        }

        self.idf.clear();
        for (term, count) in df {
            // IDF = ln(N / df) + 1 (smoothed)
            self.idf.insert(term, (n / count as f32).ln() + 1.0);
        }
        self.idf_dirty = false;
    }

    /// 确保 IDF 索引是最新的（惰性重建）
    fn ensure_idf_fresh(&mut self) {
        if self.idf_dirty {
            self.rebuild_idf();
        }
    }

    /// 计算 query 与文档的 TF-IDF 余弦相似度
    fn similarity(&self, query_tokens: &[String], doc_id: &str) -> f32 {
        let Some(doc_tf) = self.tf.get(doc_id) else {
            return 0.0;
        };

        // query TF
        let q_total = query_tokens.len().max(1) as f32;
        let mut query_tf: HashMap<&str, f32> = HashMap::new();
        for t in query_tokens {
            *query_tf.entry(t.as_str()).or_default() += 1.0 / q_total;
        }

        // dot product + magnitudes
        let mut dot = 0.0f32;
        let mut q_mag = 0.0f32;
        let mut d_mag = 0.0f32;

        // 收集所有出现的 term
        let mut all_terms: Vec<&str> = Vec::new();
        for t in query_tf.keys() {
            all_terms.push(t);
        }
        for t in doc_tf.keys() {
            if !query_tf.contains_key(t.as_str()) {
                all_terms.push(t.as_str());
            }
        }

        for term in all_terms {
            let idf = self.idf.get(term).copied().unwrap_or(1.0);
            let q_w = query_tf.get(term).copied().unwrap_or(0.0) * idf;
            let d_w = doc_tf.get(term).copied().unwrap_or(0.0) * idf;

            dot += q_w * d_w;
            q_mag += q_w * q_w;
            d_mag += d_w * d_w;
        }

        let magnitude = (q_mag * d_mag).sqrt();
        if magnitude < 1e-10 {
            return 0.0;
        }
        dot / magnitude
    }

    /// 搜索：返回 (doc_id, similarity_score) 降序
    fn search(&mut self, query: &str, limit: usize) -> Vec<(String, f32)> {
        self.ensure_idf_fresh();

        let tokens = Self::tokenize(query);
        if tokens.is_empty() {
            return Vec::new();
        }

        let mut results: Vec<(String, f32)> = self
            .tf
            .keys()
            .map(|doc_id| {
                let score = self.similarity(&tokens, doc_id);
                (doc_id.clone(), score)
            })
            .filter(|(_, score)| *score > 0.0)
            .collect();

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        results
    }
}

/// CJK unicode 范围检测
fn is_cjk(ch: char) -> bool {
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}'   // CJK Unified Ideographs
        | '\u{3400}'..='\u{4DBF}' // CJK Extension A
        | '\u{3040}'..='\u{309F}' // Hiragana
        | '\u{30A0}'..='\u{30FF}' // Katakana
        | '\u{AC00}'..='\u{D7AF}' // Hangul
    )
}

// ==================== 复合评分 ====================

/// 召回查询参数
pub struct RecallQuery {
    pub query: String,
    pub limit: usize,
    /// 仅搜索指定层级（None = 全部）
    pub tier_filter: Option<Vec<MemoryTier>>,
    /// 仅搜索指定类型（None = 全部）
    pub type_filter: Option<Vec<MemoryType>>,
    /// 仅召回该用户的记忆（必填于多租户路径）
    pub user_id: Option<i32>,
    /// 语义相似度权重
    pub similarity_weight: f32,
    /// 时间衰减权重
    pub recency_weight: f32,
    /// 重要性权重
    pub importance_weight: f32,
}

impl Default for RecallQuery {
    fn default() -> Self {
        Self {
            query: String::new(),
            limit: 5,
            tier_filter: None,
            type_filter: None,
            user_id: None,
            similarity_weight: 0.5,
            recency_weight: 0.3,
            importance_weight: 0.2,
        }
    }
}

/// 计算时间衰减分：越新越高
fn recency_score(created_at: &str) -> f32 {
    let hours_ago = chrono::DateTime::parse_from_rfc3339(created_at)
        .map(|dt| (Utc::now() - dt.with_timezone(&Utc)).num_hours().max(0) as f32)
        .unwrap_or(720.0); // 默认 30 天前
    1.0 / (1.0 + hours_ago / 24.0)
}

// ==================== AgentMemory 主结构 ====================

/// Agent 记忆管理器 (v3 — 智能记忆)
pub struct AgentMemory {
    /// 记忆文件目录
    memory_dir: PathBuf,
    /// 全部记忆条目 (id → entry)
    entries: RwLock<HashMap<String, MemoryEntry>>,
    /// TF-IDF 搜索索引
    index: RwLock<TfIdfIndex>,
    /// 有低优先级变更（访问计数等）尚未落盘，由后台维护任务批量 flush
    dirty: std::sync::atomic::AtomicBool,
}

/// 索引文件名
const INDEX_FILE: &str = "memory_index.json";

/// 用户纠错模式的关键词
const CORRECTION_PATTERNS_ZH: &[&str] = &[
    "不是",
    "错了",
    "搞错",
    "弄错",
    "你搞混",
    "你搞反",
    "画错",
    "识别错",
    "认错",
    "不对",
    "应该是",
    "其实是",
    "实际上是",
];
const CORRECTION_PATTERNS_EN: &[&str] = &[
    "that's wrong",
    "that is wrong",
    "you're wrong",
    "you are wrong",
    "incorrect",
    "mistake",
    "actually is",
    "actually it's",
    "should be",
    "confused with",
    "mixed up",
];

impl AgentMemory {
    /// 创建记忆管理器（异步加载持久化状态）
    pub async fn new(memory_dir: PathBuf) -> Self {
        let _ = tokio::fs::create_dir_all(&memory_dir).await;

        let entries = Self::load_entries(&memory_dir).await;
        let mut idx = TfIdfIndex::new();
        for (id, entry) in &entries {
            let mut index_text = entry.content.clone();
            if !entry.entities.is_empty() {
                index_text.push(' ');
                index_text.push_str(&entry.entities.join(" "));
            }
            if !entry.related_capabilities.is_empty() {
                index_text.push(' ');
                index_text.push_str(&entry.related_capabilities.join(" "));
            }
            idx.add_document(id, &index_text);
        }
        idx.rebuild_idf();

        let count = entries.len();
        let manager = Self {
            memory_dir,
            entries: RwLock::new(entries),
            index: RwLock::new(idx),
            dirty: std::sync::atomic::AtomicBool::new(false),
        };

        if count > 0 {
            tracing::info!("[AgentMemory] Loaded {} memories (TF-IDF indexed)", count);
        }

        manager
    }

    // ==================== 写入 ====================

    /// 记住一条记忆
    pub async fn remember(&self, content: &str, memory_type: MemoryType, user_id: i32) {
        self.remember_with_tier(content, memory_type, MemoryTier::LongTerm, 0.5, user_id)
            .await;
    }

    /// 记住一条记忆（带层级和重要性）
    pub async fn remember_with_tier(
        &self,
        content: &str,
        memory_type: MemoryType,
        tier: MemoryTier,
        importance: f32,
        user_id: i32,
    ) {
        self.remember_full(
            content,
            memory_type,
            tier,
            importance,
            Vec::new(),
            Vec::new(),
            user_id,
        )
        .await;
    }

    /// 记住一条记忆（完整参数，带实体和能力关联）
    // This is the single internal boundary that expands all memory metadata.
    // Keep the call shape stable until the Agent memory API moves to a request object.
    #[allow(clippy::too_many_arguments)]
    pub async fn remember_full(
        &self,
        content: &str,
        memory_type: MemoryType,
        tier: MemoryTier,
        importance: f32,
        entities: Vec<String>,
        related_capabilities: Vec<String>,
        user_id: i32,
    ) {
        // 去重检查：如果已有高度相似的记忆，跳过或合并（仅同用户）
        if self
            .should_dedup_or_merge(
                content,
                &memory_type,
                &entities,
                &related_capabilities,
                importance,
                user_id,
            )
            .await
        {
            return;
        }

        let id = Self::make_id(&format!("{user_id}:{content}"));

        // 构建索引文本（需要在 move 之前）
        let mut index_text = content.to_string();
        if !entities.is_empty() {
            index_text.push(' ');
            index_text.push_str(&entities.join(" "));
        }
        if !related_capabilities.is_empty() {
            index_text.push(' ');
            index_text.push_str(&related_capabilities.join(" "));
        }

        let entry = MemoryEntry {
            id: id.clone(),
            user_id: Some(user_id),
            memory_type,
            tier,
            content: content.to_string(),
            source: Some("agent".to_string()),
            importance: importance.clamp(0.0, 1.0),
            access_count: 0,
            created_at: Utc::now().to_rfc3339(),
            last_accessed_at: None,
            entities,
            related_capabilities,
        };

        // 更新索引（包含实体和能力关键词以提升语义召回率）
        {
            let mut idx = self.index.write().await;
            idx.add_document(&id, &index_text);
            // IDF 将在下次搜索时惰性重建
        }

        // 更新条目
        {
            let mut entries = self.entries.write().await;
            entries.insert(id, entry);
        }

        // 容量控制
        self.enforce_capacity_limit().await;

        // 持久化
        self.save_all().await;
    }

    // ==================== AI 驱动的智能记忆提取 ====================

    /// 从执行结果中 AI 提取有价值的记忆
    ///
    /// 在任务完成后调用，用 Standard AI 从对话历史+执行结果中提取：
    /// - 用户偏好
    /// - 实体知识（纠错）
    /// - 执行教训（成功/失败模式）
    /// - 有效参数模式
    pub async fn extract_memories_from_execution(
        &self,
        user_input: &str,
        conversation_history: Option<&[Value]>,
        execution_results: &HashMap<String, Value>,
        success: bool,
        capabilities_used: &[String],
        user_id: i32,
    ) {
        // Short-circuit: 极简交互不需要触发 AI 提取
        let input_chars: usize = user_input.chars().count();
        if input_chars < 6
            && success
            && execution_results.len() <= 1
            && capabilities_used.is_empty()
        {
            tracing::debug!("[Memory] Skipping AI extraction for trivial interaction");
            return;
        }

        // 构建上下文摘要
        let mut context_parts = Vec::new();
        context_parts.push(format!("用户请求: {}", user_input));

        if let Some(history) = conversation_history {
            let recent: Vec<String> = history
                .iter()
                .rev()
                .take(6)
                .map(|msg| {
                    let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("?");
                    let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    if content.len() > 200 {
                        format!(
                            "{}: {}...",
                            role,
                            content.chars().take(200).collect::<String>()
                        )
                    } else {
                        format!("{}: {}", role, content)
                    }
                })
                .collect();
            if !recent.is_empty() {
                context_parts.push(format!("对话历史:\n{}", recent.join("\n")));
            }
        }

        // 执行结果摘要
        let mut results_summary = Vec::new();
        for (step_id, output) in execution_results {
            let summary = summarize_value_for_memory(output);
            results_summary.push(format!("  {}: {}", step_id, summary));
        }
        if !results_summary.is_empty() {
            context_parts.push(format!(
                "执行结果 ({}):\n{}",
                if success { "成功" } else { "失败" },
                results_summary.join("\n")
            ));
        }

        context_parts.push(format!("使用的能力: {}", capabilities_used.join(", ")));

        let context = context_parts.join("\n\n");

        // 1. 检测纠错模式（规则匹配，不需要 AI）
        self.detect_and_store_corrections(user_input, conversation_history, user_id)
            .await;

        // 2. 如果失败，记录执行教训（规则匹配）
        if !success {
            self.record_failure_lesson(user_input, execution_results, capabilities_used, user_id)
                .await;
        }

        // 3. AI 提取深层记忆
        let existing_memories = self.get_existing_summary(user_id).await;
        let prompt = format!(
            r#"你是记忆提取引擎。从以下对话和执行记录中提取**值得长期记住**的信息。

## 已有记忆（避免重复）
{existing}

## 本次交互
{ctx}

## 提取规则
1. **用户偏好** (preference): 用户表达的喜好/习惯/风格偏好（"喜欢ACG风格"、"常用日语"、"经常画初音未来"）
2. **实体知识** (entity_knowledge): 角色/作品/人物的关联知识纠错（"芙芙=芙宁娜/原神水神"、"昔涟=星穹铁道角色"）
3. **执行教训** (execution_lesson): 什么参数有效/无效、什么策略成功/失败（"生成角色图时详细描述外观效果更好"）
4. **有效模式** (effective_pattern): 可复用的参数组合或执行策略（"动漫角色图片 category=anime 效果好"）

只输出有价值的新信息，不重复已有记忆。如果没有值得记住的，返回空数组。

输出 JSON：{{"memories": [{{"content": "...", "memory_type": "preference|entity_knowledge|execution_lesson|effective_pattern", "importance": 0.0-1.0, "entities": ["相关实体"], "capabilities": ["相关能力ID"]}}]}}"#,
            existing = existing_memories,
            ctx = context,
        );

        // 使用 Standard tier AI
        let analyzer = match crate::services::ai::create_ai_analyzer_for_tier(
            crate::config::ModelTier::Standard,
        )
        .await
        {
            Some(a) => a,
            None => {
                tracing::debug!("[Memory] No AI analyzer available for memory extraction");
                return;
            }
        };

        match analyzer.analyze(&prompt).await {
            Ok(response) => {
                if let Some(result) = parse_extraction_result(&response) {
                    let count = result.memories.len();
                    for mem in result.memories {
                        let memory_type = match mem.memory_type.as_str() {
                            "preference" => MemoryType::Preference,
                            "entity_knowledge" => MemoryType::EntityKnowledge,
                            "execution_lesson" => MemoryType::ExecutionLesson,
                            "effective_pattern" => MemoryType::EffectivePattern,
                            _ => continue,
                        };
                        self.remember_full(
                            &mem.content,
                            memory_type,
                            MemoryTier::LongTerm,
                            mem.importance.clamp(0.3, 1.0),
                            mem.entities,
                            mem.capabilities,
                            user_id,
                        )
                        .await;
                    }
                    if count > 0 {
                        tracing::info!(
                            count = count,
                            "[Memory] AI extracted {} valuable memories",
                            count
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "[Memory] AI memory extraction failed");
            }
        }
    }

    /// 检测用户纠错模式并存储为实体知识
    async fn detect_and_store_corrections(
        &self,
        user_input: &str,
        conversation_history: Option<&[Value]>,
        user_id: i32,
    ) {
        let input_lower = user_input.to_lowercase();

        let is_correction = CORRECTION_PATTERNS_ZH
            .iter()
            .any(|p| input_lower.contains(p))
            || CORRECTION_PATTERNS_EN
                .iter()
                .any(|p| input_lower.contains(p));

        if !is_correction {
            return;
        }

        // 纠错内容直接记录为高重要性 EntityKnowledge
        let correction_content = if let Some(history) = conversation_history {
            // 取最近一轮对话作为上下文
            let recent_context: Vec<String> = history
                .iter()
                .rev()
                .take(2)
                .filter_map(|msg| {
                    msg.get("content")
                        .and_then(|c| c.as_str())
                        .map(String::from)
                })
                .collect();
            format!(
                "用户纠正: {} (上下文: {})",
                user_input,
                recent_context.join(" → ")
            )
        } else {
            format!("用户纠正: {}", user_input)
        };

        self.remember_full(
            &correction_content,
            MemoryType::EntityKnowledge,
            MemoryTier::LongTerm,
            0.9, // 纠错信息高重要性
            Vec::new(),
            Vec::new(),
            user_id,
        )
        .await;

        tracing::info!("[Memory] Detected user correction, stored as EntityKnowledge");
    }

    /// 记录执行失败教训
    async fn record_failure_lesson(
        &self,
        user_input: &str,
        execution_results: &HashMap<String, Value>,
        capabilities_used: &[String],
        user_id: i32,
    ) {
        for (step_id, output) in execution_results {
            let error = output.get("error").and_then(|e| e.as_str()).or_else(|| {
                // 检查是否是失败结果
                if output.get("success") == Some(&json!(false)) {
                    output.get("message").and_then(|m| m.as_str())
                } else {
                    None
                }
            });

            if let Some(error_msg) = error {
                // 取第一个 capability（或用 step_id 作兜底），而不是用 step_id 去反向匹配
                // step_id 通常是 "step1"/"search" 等名字，不含 capability ID
                let cap_id = capabilities_used
                    .first()
                    .cloned()
                    .unwrap_or_else(|| step_id.clone());

                let lesson = format!(
                    "执行 {} 时失败: {} (用户请求: {})",
                    cap_id,
                    error_msg,
                    user_input.chars().take(50).collect::<String>()
                );

                self.remember_full(
                    &lesson,
                    MemoryType::ExecutionLesson,
                    MemoryTier::LongTerm,
                    0.8,
                    Vec::new(),
                    vec![cap_id],
                    user_id,
                )
                .await;
            }
        }
    }

    /// 获取已有记忆摘要（用于 AI 提取时避免重复）
    async fn get_existing_summary(&self, user_id: i32) -> String {
        let entries = self.entries.read().await;
        // 按重要性降序取最高价值的记忆，给 AI 提取时避免重复
        let mut filtered: Vec<&MemoryEntry> = entries
            .values()
            .filter(|e| {
                entry_visible_to(e, user_id)
                    && e.tier == MemoryTier::LongTerm
                    && e.memory_type != MemoryType::Interaction
            })
            .collect();
        filtered.sort_by(|a, b| {
            b.importance
                .partial_cmp(&a.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let summaries: Vec<String> = filtered
            .into_iter()
            .take(15)
            .map(|e| {
                let type_str = match e.memory_type {
                    MemoryType::Preference => "偏好",
                    MemoryType::EntityKnowledge => "知识",
                    MemoryType::ExecutionLesson => "教训",
                    MemoryType::EffectivePattern => "模式",
                    MemoryType::SessionInsight => "会话洞察",
                    MemoryType::Fact => "事实",
                    MemoryType::Decision => "决策",
                    MemoryType::SessionSummary => "会话摘要",
                    _ => "其他",
                };
                format!("- [{}] {}", type_str, e.content)
            })
            .collect();

        if summaries.is_empty() {
            "（暂无已有记忆）".to_string()
        } else {
            summaries.join("\n")
        }
    }

    // ==================== 去重与合并 ====================

    /// 检查是否应该去重或合并（返回 true 表示跳过写入）
    ///
    /// 合并策略：同类型且相似度 > 0.70 时，**用新内容覆盖旧内容**并提升重要性，
    /// 同时并入新记忆的实体/能力关联，保证纠错/更新信息能正确替换过时记忆。
    async fn should_dedup_or_merge(
        &self,
        new_content: &str,
        new_type: &MemoryType,
        new_entities: &[String],
        new_capabilities: &[String],
        new_importance: f32,
        user_id: i32,
    ) -> bool {
        let mut idx = self.index.write().await;
        let similar = idx.search(new_content, 3);
        drop(idx);

        if similar.is_empty() {
            return false;
        }

        let entries = self.entries.read().await;

        for (id, score) in &similar {
            if let Some(existing) = entries.get(id) {
                // 跨用户不去重
                if !entry_visible_to(existing, user_id) {
                    continue;
                }
                // 完全重复：跳过
                if *score > DEDUP_SIMILARITY_THRESHOLD {
                    tracing::debug!(
                        score = score,
                        existing = %existing.content.chars().take(50).collect::<String>(),
                        "[Memory] Dedup: skipping duplicate memory"
                    );
                    return true;
                }

                // 可合并：同类型且高度相似 — 用新内容替换旧内容并提升重要性
                if *score > MERGE_SIMILARITY_THRESHOLD && existing.memory_type == *new_type {
                    let id_clone = id.clone();
                    let new_id = Self::make_id(&format!("{user_id}:{new_content}"));
                    drop(entries);

                    // 在 entries 写锁内完成合并 + 提取索引数据，避免 drop 后竞态
                    let (entry_entities, entry_capabilities) = {
                        let mut entries = self.entries.write().await;
                        if let Some(entry) = entries.get_mut(&id_clone) {
                            tracing::info!(
                                score = score,
                                old = %entry.content.chars().take(60).collect::<String>(),
                                new = %new_content.chars().take(60).collect::<String>(),
                                "[Memory] Merge: replacing old content with updated version"
                            );
                            entry.content = new_content.to_string();
                            // 重要性取「旧值+0.1」与新记忆重要性的较大者，
                            // 避免高重要性纠错（0.9）合并进旧记忆后被压低
                            entry.importance =
                                (entry.importance + 0.1).max(new_importance).min(1.0);
                            entry.access_count += 1;
                            entry.last_accessed_at = Some(Utc::now().to_rfc3339());
                            // 并入新记忆的实体/能力关联（旧逻辑直接丢弃新关联）
                            for ent in new_entities {
                                if !entry.entities.contains(ent) {
                                    entry.entities.push(ent.clone());
                                }
                            }
                            for cap in new_capabilities {
                                if !entry.related_capabilities.contains(cap) {
                                    entry.related_capabilities.push(cap.clone());
                                }
                            }

                            // 在锁内提取索引所需数据
                            let entities = entry.entities.clone();
                            let capabilities = entry.related_capabilities.clone();

                            // 如果内容 hash 变了，需要重新映射 id
                            if new_id != id_clone {
                                let mut updated = entry.clone();
                                updated.id = new_id.clone();
                                entries.remove(&id_clone);
                                entries.insert(new_id.clone(), updated);
                            }

                            (entities, capabilities)
                        } else {
                            (Vec::new(), Vec::new())
                        }
                    };

                    // 更新搜索索引（entries 锁已释放，不会死锁）
                    {
                        let mut idx = self.index.write().await;
                        idx.remove_document(&id_clone);
                        let mut index_text = new_content.to_string();
                        if !entry_entities.is_empty() {
                            index_text.push(' ');
                            index_text.push_str(&entry_entities.join(" "));
                        }
                        if !entry_capabilities.is_empty() {
                            index_text.push(' ');
                            index_text.push_str(&entry_capabilities.join(" "));
                        }
                        idx.add_document(&new_id, &index_text);
                    }

                    // 合并后持久化（remember_full 会提前返回，不会调用 save_all）
                    self.save_all().await;
                    return true;
                }
            }
        }

        false
    }

    // ==================== 容量管理 ====================

    /// 强制容量上限，淘汰低价值记忆
    ///
    /// 锁顺序：先 index.write()，再 entries.write()（与其他所有路径一致，避免死锁）
    async fn enforce_capacity_limit(&self) {
        // Step 1: 快照当前条目并评分（用 read 锁，不持锁做后续操作）
        let scored: Vec<(String, f32)> = {
            let entries = self.entries.read().await;
            if entries.len() <= MAX_MEMORY_ENTRIES {
                return;
            }

            let mut v: Vec<(String, f32)> = entries
                .iter()
                .map(|(id, e)| {
                    let recency = recency_score(&e.created_at);
                    let access_boost = 1.0 + (e.access_count as f32 * 0.1).min(1.0);
                    let type_bonus = match e.memory_type {
                        MemoryType::Preference | MemoryType::EntityKnowledge => 0.2,
                        MemoryType::ExecutionLesson | MemoryType::EffectivePattern => 0.1,
                        MemoryType::SessionInsight | MemoryType::Decision => 0.1,
                        _ => 0.0,
                    };
                    let score = (e.importance + type_bonus) * access_boost * (0.3 + 0.7 * recency);
                    (id.clone(), score)
                })
                .collect();

            // 按分数升序排列（最低分的先淘汰）
            v.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            v
        };
        // entries read lock dropped here

        // Step 2: 计算要删除多少
        let current_len = {
            let entries = self.entries.read().await;
            entries.len()
        };
        if current_len <= MAX_MEMORY_ENTRIES {
            return;
        }
        let to_remove = current_len - MAX_MEMORY_ENTRIES;
        let ids_to_remove: Vec<String> = scored
            .into_iter()
            .take(to_remove)
            .map(|(id, _)| id)
            .collect();

        // Step 3: 按一致的顺序获取锁（index 先，entries 后）
        {
            let mut idx = self.index.write().await;
            let mut entries = self.entries.write().await;
            for id in &ids_to_remove {
                entries.remove(id);
                idx.remove_document(id);
            }
        }

        tracing::info!(
            removed = ids_to_remove.len(),
            "[Memory] Capacity enforcement: removed {} low-value memories",
            ids_to_remove.len()
        );
    }

    /// 按实体召回相关记忆
    pub async fn recall_by_entity(
        &self,
        entity: &str,
        limit: usize,
        user_id: i32,
    ) -> Vec<MemoryEntry> {
        if entity.trim().is_empty() {
            return Vec::new();
        }

        // 将输入拆分为有意义的片段进行匹配，而不是用整个句子匹配
        let tokens = Self::tokenize_for_entity_match(entity);
        if tokens.is_empty() {
            return Vec::new();
        }

        let entries = self.entries.read().await;
        let mut scored: Vec<(&MemoryEntry, f32)> = entries
            .values()
            .filter(|e| entry_visible_to(e, user_id))
            .filter_map(|e| {
                let content_lower = e.content.to_lowercase();
                let entity_names: Vec<String> =
                    e.entities.iter().map(|ent| ent.to_lowercase()).collect();

                let mut match_score: f32 = 0.0;
                for token in &tokens {
                    // 实体名精确匹配（高权重）
                    if entity_names
                        .iter()
                        .any(|ent| ent.contains(token) || token.contains(ent.as_str()))
                    {
                        match_score += 2.0;
                    }
                    // 内容包含匹配（低权重）
                    if content_lower.contains(token) {
                        match_score += 1.0;
                    }
                }

                if match_score > 0.0 {
                    Some((e, match_score * e.importance))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored.into_iter().map(|(e, _)| e.clone()).collect()
    }

    /// 将用户输入拆分为用于实体匹配的有意义片段
    fn tokenize_for_entity_match(input: &str) -> Vec<String> {
        let input_lower = input.to_lowercase();

        // 停用词（高频虚词 + 动作元词，避免匹配噪声）
        let stop_words: &[&str] = &[
            // 中文虚词
            "的",
            "了",
            "是",
            "在",
            "和",
            "有",
            "我",
            "你",
            "他",
            "她",
            "它",
            "这",
            "那",
            "就",
            "也",
            "都",
            "要",
            "会",
            "可以",
            "不",
            "很",
            "吗",
            "呢",
            "吧",
            "啊",
            "哦",
            "嗯",
            "一下",
            "一个",
            "什么",
            "看看",
            "帮我",
            "给我",
            "告诉我",
            "介绍",
            "关于",
            "最近",
            "最新",
            "最热",
            "并",
            "然后",
            "以及",
            "或者",
            "还有",
            // 中文动作/元动词（不构成实体信息）
            "想",
            "想要",
            "需要",
            "需",
            "做",
            "能",
            "能否",
            "是否",
            "哪",
            "哪个",
            "谁",
            "怎样",
            "怎么",
            "如何",
            "为什么",
            "因为",
            "请",
            "让",
            "把",
            "被",
            "从",
            "向",
            "对",
            "用",
            "去",
            "来",
            "生成",
            "搜索",
            "查找",
            "查看",
            "打开",
            "执行",
            // English stop words
            "the",
            "a",
            "an",
            "is",
            "are",
            "was",
            "were",
            "be",
            "been",
            "and",
            "or",
            "but",
            "in",
            "on",
            "at",
            "to",
            "for",
            "of",
            "with",
            "by",
            "from",
            "about",
            "into",
            "what",
            "how",
            "show",
            "me",
            "please",
            "find",
            "get",
            "give",
            "tell",
            "do",
            "can",
            "will",
            "would",
            "could",
            "should",
            "try",
            "let",
            "make",
            "run",
            "use",
            "help",
            "want",
            "need",
        ];

        let mut tokens = Vec::new();

        // 按空格/标点分割，过滤停用词和过短的 token
        for segment in input_lower.split(|c: char| {
            c.is_whitespace() || c == '，' || c == '。' || c == '、' || c == '！' || c == '？'
        }) {
            let segment = segment.trim();
            if segment.is_empty() {
                continue;
            }

            // 对于纯 ASCII（英文），按空格继续拆分
            if segment.is_ascii() {
                for word in segment.split_whitespace() {
                    let word = word.trim_matches(|c: char| !c.is_alphanumeric());
                    if word.len() >= 2 && !stop_words.contains(&word) {
                        tokens.push(word.to_string());
                    }
                }
            } else {
                // 中文：过滤停用词后保留整个片段，同时提取连续的中文字符子串（2-4字的组合）
                let cleaned: String = {
                    let mut s = segment.to_string();
                    for sw in stop_words {
                        s = s.replace(sw, "");
                    }
                    s
                };
                let cleaned = cleaned.trim();
                if !cleaned.is_empty() {
                    // 保留清理后的完整片段
                    tokens.push(cleaned.to_string());
                    // 如果片段较长（>4字），提取连续的2-4字组合作为子 token
                    let chars: Vec<char> = cleaned.chars().collect();
                    if chars.len() > 4 {
                        for window_size in 2..=4 {
                            for window in chars.windows(window_size) {
                                let sub: String = window.iter().collect();
                                if !stop_words.contains(&sub.as_str()) {
                                    tokens.push(sub);
                                }
                            }
                        }
                    }
                }
            }
        }

        tokens.sort();
        tokens.dedup();
        tokens
    }

    /// 追加今日日志
    pub async fn log_daily(&self, user_id: i32, entry: &str) {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let log_path = self.memory_dir.join(format!("{}.md", today));

        let timestamp = Utc::now().format("%H:%M:%S").to_string();
        let log_line = format!("\n- [{}] user:{} — {}\n", timestamp, user_id, entry);

        if !log_path.exists() {
            let header = format!("# Agent Daily Log - {}\n", today);
            let _ = tokio::fs::write(&log_path, header).await;
        }

        use tokio::io::AsyncWriteExt;
        if let Ok(mut file) = tokio::fs::OpenOptions::new()
            .append(true)
            .open(&log_path)
            .await
        {
            let _ = file.write_all(log_line.as_bytes()).await;
        }
    }

    /// 归档会话洞察到 LongTerm 记忆
    ///
    /// 在会话结束或切换时调用，用 AI 从会话历史中提炼关键信息。
    pub async fn consolidate_session(&self, summary: &str, user_id: i32) {
        if summary.trim().is_empty() {
            return;
        }
        self.remember_with_tier(
            summary,
            MemoryType::SessionInsight,
            MemoryTier::MediumTerm,
            0.6,
            user_id,
        )
        .await;
    }

    /// 提升高频访问的 MediumTerm 记忆到 LongTerm
    ///
    /// 阈值：access_count >= 3 的 MediumTerm 条目自动升级。
    pub async fn promote_memories(&self) -> usize {
        let mut promoted = 0;
        {
            let mut entries = self.entries.write().await;
            for entry in entries.values_mut() {
                if entry.tier == MemoryTier::MediumTerm && entry.access_count >= 3 {
                    entry.tier = MemoryTier::LongTerm;
                    entry.importance = (entry.importance + 0.1).min(1.0);
                    promoted += 1;
                }
            }
        }
        if promoted > 0 {
            tracing::info!("[AgentMemory] Promoted {} memories to LongTerm", promoted);
            self.save_all().await;
        }
        promoted
    }

    // ==================== 列举 ====================

    /// 列出最近的记忆条目（按创建时间降序，排除 ShortTerm，按用户隔离）
    pub async fn list_recent(&self, limit: usize, user_id: i32) -> Vec<MemoryEntry> {
        let entries = self.entries.read().await;
        let mut recent: Vec<&MemoryEntry> = entries
            .values()
            .filter(|e| entry_visible_to(e, user_id) && e.tier != MemoryTier::ShortTerm)
            .collect();
        recent.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        recent.truncate(limit);
        recent.into_iter().cloned().collect()
    }

    // ==================== 管理操作 ====================

    /// 删除指定 ID 的记忆条目（仅所有者或系统用户）
    ///
    /// 锁顺序：index 先，entries 后（与其他所有路径一致，避免死锁）
    pub async fn remove_memory(&self, memory_id: &str, user_id: i32) -> bool {
        // 先检查是否存在且归属正确（read 锁）
        let allowed = {
            let entries = self.entries.read().await;
            entries
                .get(memory_id)
                .is_some_and(|e| entry_visible_to(e, user_id))
        };
        if !allowed {
            return false;
        }
        // 按正确顺序获取写锁
        {
            let mut idx = self.index.write().await;
            let mut entries = self.entries.write().await;
            // 再验一次归属
            if !entries
                .get(memory_id)
                .is_some_and(|e| entry_visible_to(e, user_id))
            {
                return false;
            }
            idx.remove_document(memory_id);
            entries.remove(memory_id);
        }
        self.save_all().await;
        tracing::info!(id = memory_id, user_id, "[Memory] Removed memory entry");
        true
    }

    /// 更新指定 ID 的记忆内容（仅所有者）
    ///
    /// 锁顺序：index 先，entries 后（与其他所有路径一致，避免死锁）
    pub async fn update_memory(&self, memory_id: &str, new_content: &str, user_id: i32) -> bool {
        let allowed = {
            let entries = self.entries.read().await;
            entries
                .get(memory_id)
                .is_some_and(|e| entry_visible_to(e, user_id))
        };
        if !allowed {
            return false;
        }
        let new_id = Self::make_id(&format!("{user_id}:{new_content}"));
        {
            let mut idx = self.index.write().await;
            let mut entries = self.entries.write().await;
            if !entries
                .get(memory_id)
                .is_some_and(|e| entry_visible_to(e, user_id))
            {
                return false;
            }
            idx.remove_document(memory_id);
            if let Some(mut entry) = entries.remove(memory_id) {
                entry.content = new_content.to_string();
                entry.id = new_id.clone();
                entry.user_id = Some(user_id);
                entry.last_accessed_at = Some(Utc::now().to_rfc3339());
                // 索引文本包含实体和能力关键词，与其他写入路径保持一致
                let mut index_text = new_content.to_string();
                if !entry.entities.is_empty() {
                    index_text.push(' ');
                    index_text.push_str(&entry.entities.join(" "));
                }
                if !entry.related_capabilities.is_empty() {
                    index_text.push(' ');
                    index_text.push_str(&entry.related_capabilities.join(" "));
                }
                idx.add_document(&new_id, &index_text);
                entries.insert(new_id.clone(), entry);
            }
        }
        self.save_all().await;
        tracing::info!(old_id = memory_id, new_id = %new_id, user_id, "[Memory] Updated memory entry");
        true
    }

    // ==================== 搜索 ====================

    /// 召回相关记忆（完整参数）
    pub async fn recall_with_params(&self, params: RecallQuery) -> Vec<MemoryEntry> {
        if params.query.is_empty() {
            return Vec::new();
        }

        // TF-IDF 搜索 — 拿多一些候选做后续过滤
        let tfidf_results = {
            let mut idx = self.index.write().await;
            idx.search(&params.query, params.limit * 3)
        };

        if tfidf_results.is_empty() {
            tracing::debug!(
                query = %params.query,
                "[Memory] TF-IDF recall returned 0 candidates for non-empty query"
            );
        }

        let entries = self.entries.read().await;

        let mut scored: Vec<(f32, String)> = tfidf_results
            .into_iter()
            .filter_map(|(id, sim_score)| {
                let entry = entries.get(&id)?;

                // 用户隔离
                if let Some(uid) = params.user_id {
                    if !entry_visible_to(entry, uid) {
                        return None;
                    }
                }

                // 层级过滤
                if let Some(ref tiers) = params.tier_filter {
                    if !tiers.contains(&entry.tier) {
                        return None;
                    }
                }
                // 类型过滤
                if let Some(ref types) = params.type_filter {
                    if !types.contains(&entry.memory_type) {
                        return None;
                    }
                }

                // 复合评分（LongTerm 记忆降低时间衰减权重，确保持久知识不因时间被低估）
                let r_score = recency_score(&entry.created_at);
                let (sim_w, rec_w, imp_w) = if entry.tier == MemoryTier::LongTerm {
                    (0.6, 0.1, 0.3)
                } else {
                    (
                        params.similarity_weight,
                        params.recency_weight,
                        params.importance_weight,
                    )
                };
                // 权重归一化：防止自定义调用方传入非 1.0 总和的权重
                let w_sum = sim_w + rec_w + imp_w;
                let (sim_w, rec_w, imp_w) = if w_sum > 0.0 && (w_sum - 1.0).abs() > 0.01 {
                    (sim_w / w_sum, rec_w / w_sum, imp_w / w_sum)
                } else {
                    (sim_w, rec_w, imp_w)
                };

                // 重要性时间衰减：基于最后访问时间，越久不访问重要性越低
                let importance = {
                    // 使用 last_accessed_at（如有），否则使用 created_at
                    let reference_time = entry
                        .last_accessed_at
                        .as_deref()
                        .unwrap_or(&entry.created_at);
                    let days_since = chrono::DateTime::parse_from_rfc3339(reference_time)
                        .map(|dt| {
                            (chrono::Utc::now() - dt.with_timezone(&chrono::Utc))
                                .num_days()
                                .max(0) as f32
                        })
                        .unwrap_or(365.0);

                    // 访问越多衰减越慢：半衰期 = 90 天 * ln(access_count + 1)
                    // access_count=0: 90天(floor), =5: 161天, =10: 215天
                    let half_life = 90.0 * (entry.access_count as f32).ln_1p();
                    let half_life = half_life.max(90.0); // 最低 90 天
                    let decay = (0.3_f32).max((-0.693 * days_since / half_life).exp());
                    entry.importance * decay
                };

                let final_score = sim_w * sim_score + rec_w * r_score + imp_w * importance;

                Some((final_score, id))
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(params.limit);

        // 更新 access_count (不阻塞返回)
        let hit_ids: Vec<String> = scored.iter().map(|(_, id)| id.clone()).collect();
        drop(entries);

        // 收集结果
        let entries = self.entries.read().await;
        let results: Vec<MemoryEntry> = scored
            .iter()
            .filter_map(|(_, id)| entries.get(id).cloned())
            .collect();
        drop(entries);

        // 更新 access_count（promote_memories 的晋升阈值依赖此计数）
        if !hit_ids.is_empty() {
            {
                let mut entries = self.entries.write().await;
                let now = Utc::now().to_rfc3339();
                for id in &hit_ids {
                    if let Some(entry) = entries.get_mut(id) {
                        entry.access_count += 1;
                        entry.last_accessed_at = Some(now.clone());
                    }
                }
            }
            // 只是访问计数变更，不在召回热路径上全量重写两份持久化文件
            // （每次规划会触发 2 次召回）；标记脏位，由后台维护任务批量落盘
            self.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
        }

        results
    }

    /// 若有未落盘的低优先级变更（访问计数、短期记忆清理）则写盘
    pub async fn flush_if_dirty(&self) {
        if self.dirty.swap(false, std::sync::atomic::Ordering::Relaxed) {
            self.save_all().await;
        }
    }

    // ==================== 清理 ====================

    /// 清理旧日志（保留最近 N 天）
    pub async fn cleanup_old_logs(&self, keep_days: i64) {
        let cutoff = Utc::now().date_naive() - chrono::Duration::days(keep_days);

        let mut dir = match tokio::fs::read_dir(&self.memory_dir).await {
            Ok(d) => d,
            Err(_) => return,
        };

        while let Ok(Some(entry)) = dir.next_entry().await {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.len() == 13 && file_name.ends_with(".md") {
                if let Ok(date) = NaiveDate::parse_from_str(&file_name[..10], "%Y-%m-%d") {
                    if date < cutoff {
                        let _ = tokio::fs::remove_file(entry.path()).await;
                    }
                }
            }
        }
    }

    /// 清理过期的 ShortTerm 记忆（超过 2 小时）
    pub async fn cleanup_short_term(&self) -> usize {
        let cutoff = Utc::now() - chrono::Duration::hours(2);
        let mut removed = Vec::new();

        {
            let entries = self.entries.read().await;
            for (id, entry) in entries.iter() {
                if entry.tier == MemoryTier::ShortTerm {
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&entry.created_at) {
                        if dt.with_timezone(&Utc) < cutoff {
                            removed.push(id.clone());
                        }
                    }
                }
            }
        }

        let count = removed.len();
        if !removed.is_empty() {
            {
                // 锁顺序：index 先，entries 后（与其他所有路径一致，避免死锁）
                let mut idx = self.index.write().await;
                let mut entries = self.entries.write().await;
                for id in &removed {
                    entries.remove(id);
                    idx.remove_document(id);
                }
            }
            // 标记脏位，让后台维护任务把删除结果落盘（否则重启后过期条目复活）
            self.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        count
    }

    // ==================== 持久化 ====================

    /// 生成确定性 ID（纯内容 hash，相同内容产生相同 ID，支持幂等去重）
    fn make_id(content: &str) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        content.hash(&mut hasher);
        format!("mem_{:016x}", hasher.finish())
    }

    /// 加载全部记忆条目
    async fn load_entries(memory_dir: &Path) -> HashMap<String, MemoryEntry> {
        // 优先从 memory_index.json 快速恢复
        let index_path = memory_dir.join(INDEX_FILE);
        if let Ok(content) = tokio::fs::read_to_string(&index_path).await {
            if let Ok(entries) = serde_json::from_str::<Vec<MemoryEntry>>(&content) {
                return entries.into_iter().map(|e| (e.id.clone(), e)).collect();
            }
        }

        // fallback: 从 memory.md 迁移
        let memory_path = memory_dir.join("memory.md");
        if let Ok(content) = tokio::fs::read_to_string(&memory_path).await {
            let entries = Self::parse_memory_md(&content);
            if !entries.is_empty() {
                tracing::info!(
                    "[AgentMemory] Migrated {} entries from memory.md",
                    entries.len()
                );
            }
            return entries.into_iter().map(|e| (e.id.clone(), e)).collect();
        }

        HashMap::new()
    }

    /// 保存全部状态（memory_index.json + memory.md）
    async fn save_all(&self) {
        // 全量写盘会带上所有未落盘变更，脏位可以一并清除
        self.dirty
            .store(false, std::sync::atomic::Ordering::Relaxed);
        // 快照数据后立即释放读锁，避免持锁做 I/O
        let (json_opt, md) = {
            let entries = self.entries.read().await;

            // 1. 序列化全量 JSON
            let all_entries: Vec<&MemoryEntry> = entries.values().collect();
            let json_opt = serde_json::to_string_pretty(&all_entries).ok();

            // 2. 构建 memory.md（仅 LongTerm）
            let mut md = String::from("# Agent Long-term Memory\n\n");
            let mut long_term: Vec<&MemoryEntry> = entries
                .values()
                .filter(|e| e.tier == MemoryTier::LongTerm)
                .collect();
            long_term.sort_by(|a, b| a.created_at.cmp(&b.created_at));

            for entry in long_term {
                let type_str = match entry.memory_type {
                    MemoryType::Preference => "preference",
                    MemoryType::EntityKnowledge => "knowledge",
                    MemoryType::ExecutionLesson => "lesson",
                    MemoryType::EffectivePattern => "pattern",
                    MemoryType::Fact => "fact",
                    MemoryType::Interaction => "interaction",
                    MemoryType::Decision => "decision",
                    MemoryType::SessionInsight => "insight",
                    MemoryType::SessionSummary => "session",
                };
                md.push_str(&format!(
                    "- [{}] [{}] {}\n",
                    entry.created_at, type_str, entry.content
                ));
            }

            (json_opt, md)
        }; // 读锁在此释放

        // 写文件（无锁状态）
        if let Some(json) = json_opt {
            let index_path = self.memory_dir.join(INDEX_FILE);
            if let Err(e) = tokio::fs::write(&index_path, json).await {
                tracing::error!(error = %e, "[Memory] Failed to persist memory_index.json");
            }
        }

        let memory_path = self.memory_dir.join("memory.md");
        if let Err(e) = tokio::fs::write(&memory_path, md).await {
            tracing::error!(error = %e, "[Memory] Failed to persist memory.md");
        }
    }

    /// 解析旧版 memory.md 格式（迁移用）
    fn parse_memory_md(content: &str) -> Vec<MemoryEntry> {
        content
            .lines()
            .filter(|line| line.starts_with("- ["))
            .filter_map(|line| {
                let rest = line.strip_prefix("- [")?;
                let ts_end = rest.find(']')?;
                let created_at = rest[..ts_end].to_string();
                let rest = rest[ts_end + 1..].trim_start();

                let rest = rest.strip_prefix('[')?;
                let type_end = rest.find(']')?;
                let type_str = &rest[..type_end];
                let content = rest[type_end + 1..].trim().to_string();

                let memory_type = match type_str {
                    "preference" => MemoryType::Preference,
                    "knowledge" | "entity_knowledge" => MemoryType::EntityKnowledge,
                    "lesson" | "execution_lesson" => MemoryType::ExecutionLesson,
                    "pattern" | "effective_pattern" => MemoryType::EffectivePattern,
                    "fact" => MemoryType::Fact,
                    "interaction" => MemoryType::Interaction,
                    "decision" => MemoryType::Decision,
                    "insight" | "session_insight" => MemoryType::SessionInsight,
                    "session" => MemoryType::SessionSummary,
                    _ => MemoryType::Fact,
                };

                let id = Self::make_id(&content);

                Some(MemoryEntry {
                    id,
                    user_id: None, // 遗留 markdown 导入，不归属具体用户
                    memory_type,
                    tier: MemoryTier::LongTerm,
                    content,
                    source: Some("file".to_string()),
                    importance: 0.5,
                    access_count: 0,
                    created_at,
                    last_accessed_at: None,
                    entities: Vec::new(),
                    related_capabilities: Vec::new(),
                })
            })
            .collect()
    }
}

// ==================== 辅助函数 ====================

/// 解析 AI 返回的记忆提取结果
fn parse_extraction_result(response: &str) -> Option<MemoryExtractionResult> {
    let text = response.trim();
    // 尝试找到 JSON 块
    let json_str = if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            &text[start..=end]
        } else {
            text
        }
    } else {
        text
    };
    serde_json::from_str(json_str).ok()
}

/// 将 Value 摘要为紧凑字符串（用于记忆提取 prompt）
pub fn summarize_value_for_memory(value: &Value) -> String {
    match value {
        Value::String(s) => {
            if s.len() > 100 {
                format!("\"{}...\"", s.chars().take(100).collect::<String>())
            } else {
                format!("\"{}\"", s)
            }
        }
        Value::Array(arr) => format!("[{} items]", arr.len()),
        Value::Object(obj) => {
            if let Some(error) = obj.get("error").and_then(|e| e.as_str()) {
                format!("{{error: \"{}\"}}", error)
            } else if let Some(msg) = obj.get("message").and_then(|m| m.as_str()) {
                format!(
                    "{{message: \"{}\"}}",
                    msg.chars().take(80).collect::<String>()
                )
            } else {
                format!("{{{} fields}}", obj.len())
            }
        }
        Value::Bool(b) => format!("{}", b),
        Value::Number(n) => format!("{}", n),
        Value::Null => "null".to_string(),
    }
}

// ==================== 全局实例 ====================

static AGENT_MEMORY: once_cell::sync::OnceCell<Arc<AgentMemory>> = once_cell::sync::OnceCell::new();

/// 初始化全局记忆管理器（含后台维护 worker）
pub async fn init_memory(memory_dir: PathBuf) {
    let memory = Arc::new(AgentMemory::new(memory_dir).await);
    let _ = AGENT_MEMORY.set(memory.clone());

    // 后台维护：每 10 分钟清理过期短期记忆 + 提升高频记忆 + 批量落盘访问计数
    tokio::spawn(async move {
        let interval = tokio::time::Duration::from_secs(10 * 60);
        loop {
            tokio::time::sleep(interval).await;
            let cleaned = memory.cleanup_short_term().await;
            let promoted = memory.promote_memories().await;
            memory.flush_if_dirty().await;
            if cleaned > 0 || promoted > 0 {
                tracing::info!(
                    cleaned = cleaned,
                    promoted = promoted,
                    "[Memory] Background maintenance completed"
                );
            }
        }
    });
}

/// 获取全局记忆管理器
pub fn get_memory() -> Option<&'static Arc<AgentMemory>> {
    AGENT_MEMORY.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ======== TF-IDF 分词测试 ========

    #[test]
    fn test_tokenize_english() {
        let tokens = TfIdfIndex::tokenize("Generate an image of sunset");
        assert!(tokens.contains(&"generate".to_string()));
        assert!(tokens.contains(&"image".to_string()));
        assert!(tokens.contains(&"sunset".to_string()));
        // "an" 有 2 个字符，满足 >= 2 阈值，会保留
        assert!(tokens.contains(&"an".to_string()));
        // 单字母 "a" 应被过滤
        assert!(!tokens.contains(&"a".to_string()));
    }

    #[test]
    fn test_tokenize_cjk_unigram_filters_stopwords() {
        let tokens = TfIdfIndex::tokenize("生成图像");
        // 单字 token 应包含非停用词
        assert!(tokens.contains(&"生".to_string()));
        assert!(tokens.contains(&"成".to_string()));
        assert!(tokens.contains(&"图".to_string()));
        assert!(tokens.contains(&"像".to_string()));
    }

    #[test]
    fn test_tokenize_cjk_bigram_generation() {
        let tokens = TfIdfIndex::tokenize("生成图像");
        // bigram 应从连续 CJK 序列生成
        assert!(tokens.contains(&"生成".to_string()));
        assert!(tokens.contains(&"成图".to_string()));
        assert!(tokens.contains(&"图像".to_string()));
    }

    #[test]
    fn test_tokenize_cjk_bigram_includes_stopwords() {
        // bigram 不过滤停用词，确保存储和查询对称
        let tokens = TfIdfIndex::tokenize("我的图像");
        // "的" 是停用词，不应出现在 unigram
        let unigrams: Vec<&String> = tokens.iter().filter(|t| t.chars().count() == 1).collect();
        assert!(!unigrams.iter().any(|t| t.as_str() == "的"));
        // 但 bigram 应包含 "我的" 和 "的图"
        assert!(tokens.contains(&"我的".to_string()));
        assert!(tokens.contains(&"的图".to_string()));
    }

    #[test]
    fn test_tokenize_mixed_cjk_latin() {
        let tokens = TfIdfIndex::tokenize("使用PixAI生成");
        assert!(tokens.contains(&"pixai".to_string()));
        assert!(tokens.contains(&"使用".to_string()));
        // "使用" 和 "生成" 被 "PixAI" 隔断，不应生成 cross-boundary bigram
        assert!(!tokens.contains(&"用生".to_string()));
    }

    #[test]
    fn test_tokenize_single_cjk_char_no_bigram() {
        // 单个 CJK 字符不应生成 bigram
        let tokens = TfIdfIndex::tokenize("a 图 b");
        let bigrams: Vec<&String> = tokens.iter().filter(|t| t.chars().count() == 2).collect();
        assert!(bigrams.is_empty());
    }

    // ======== TF-IDF 检索测试 ========

    #[test]
    fn test_tfidf_similarity_basic() {
        let mut idx = TfIdfIndex::new();
        idx.add_document("doc1", "生成一张猫咪的图片");
        idx.add_document("doc2", "搜索最新的新闻");
        idx.add_document("doc3", "生成一张狗的图片");
        idx.rebuild_idf();

        let query_tokens = TfIdfIndex::tokenize("生成图片");
        let s1 = idx.similarity(&query_tokens, "doc1");
        let s2 = idx.similarity(&query_tokens, "doc2");
        let s3 = idx.similarity(&query_tokens, "doc3");
        assert!(
            s1 > s2,
            "doc1 ({s1}) should be more relevant than doc2 ({s2})"
        );
        assert!(
            s3 > s2,
            "doc3 ({s3}) should be more relevant than doc2 ({s2})"
        );
    }

    // ======== 重要性衰减测试 ========

    #[test]
    fn test_importance_decay_formula() {
        // access_count=0: ln_1p(0) = ln(1) = 0 → half_life = 0 → max(90) = 90
        let hl0 = 90.0_f32 * (0.0_f32).ln_1p();
        assert_eq!(hl0, 0.0);
        assert_eq!(hl0.max(90.0), 90.0);

        // access_count=5: ln_1p(5) = ln(6) ≈ 1.792 → half_life ≈ 161
        let hl5 = 90.0 * (5.0_f32).ln_1p();
        assert!(
            (hl5 - 161.2).abs() < 1.0,
            "half_life for access_count=5: {hl5}"
        );

        // access_count=10: ln_1p(10) = ln(11) ≈ 2.398 → half_life ≈ 215
        let hl10 = 90.0 * (10.0_f32).ln_1p();
        assert!(
            (hl10 - 215.8).abs() < 1.0,
            "half_life for access_count=10: {hl10}"
        );
    }

    #[test]
    fn test_importance_decay_floor() {
        // 衰减永远不低于 0.3
        let days_since = 10000.0_f32; // 极端：10000 天
        let half_life = 90.0_f32;
        let decay = (0.3_f32).max((-0.693 * days_since / half_life).exp());
        assert!(
            (decay - 0.3).abs() < f32::EPSILON,
            "decay should floor at 0.3, got {decay}"
        );
    }

    #[test]
    fn entry_visible_to_isolates_users() {
        let a = MemoryEntry {
            id: "a".into(),
            user_id: Some(1),
            memory_type: MemoryType::Preference,
            tier: MemoryTier::LongTerm,
            content: "user1".into(),
            source: None,
            importance: 0.5,
            access_count: 0,
            created_at: Utc::now().to_rfc3339(),
            last_accessed_at: None,
            entities: vec![],
            related_capabilities: vec![],
        };
        let legacy = MemoryEntry {
            user_id: None,
            content: "legacy".into(),
            ..a.clone()
        };
        assert!(entry_visible_to(&a, 1));
        assert!(!entry_visible_to(&a, 2));
        assert!(entry_visible_to(&a, 0)); // system sees all
        assert!(!entry_visible_to(&legacy, 1)); // orphan legacy not visible to users
        assert!(entry_visible_to(&legacy, 0));
    }
}
