
use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

/// 每个用户的记忆容量上限。
///
/// 此前这是一个 **全局** 上限：淘汰在全体条目上打分，跨用户竞争同一份配额，
/// 一个活跃用户可以把别人的记忆全部挤掉，而被清空的一方毫无感知。现在配额按
/// 用户独立结算，淘汰只在超额用户自己的桶里进行。
///
/// 数值保持 500 不变：单用户站点升级后条目数不会突然缩水。代价是总量随用户数
/// 线性增长（上限 = 用户数 × 500），多租户部署需要留意——真正的全局上限属于
/// 「记忆搬进 Postgres」那件事，不适合用跨用户淘汰来凑。
pub(crate) const MAX_MEMORY_ENTRIES_PER_USER: usize = 500;

/// 记忆去重——TF-IDF 相似度超过此值认为重复
pub(crate) const DEDUP_SIMILARITY_THRESHOLD: f32 = 0.85;

/// 合并——TF-IDF 相似度超过此值认为可合并
pub(crate) const MERGE_SIMILARITY_THRESHOLD: f32 = 0.70;

// 类型定义

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
pub(crate) fn entry_visible_to(entry: &MemoryEntry, user_id: i32) -> bool {
    if user_id == 0 {
        return true;
    }
    entry.user_id == Some(user_id)
}

pub(crate) fn default_tier() -> MemoryTier {
    MemoryTier::LongTerm
}
pub(crate) fn default_importance() -> f32 {
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

// TF-IDF 搜索索引

/// 轻量级 TF-IDF 索引
///
/// 对中英文混合文本做 token 化，支持 CJK bigram + 拉丁单词分词。
/// 设计目标：百级文档集上的毫秒级搜索，零外部依赖。
///
/// IDF 采用惰性重建策略：add/remove 仅标记脏位，实际重建推迟到搜索时执行，
/// 避免批量写入时 O(n) × k 的重复计算。
pub struct TfIdfIndex {
    /// doc_id → { term → tf }
    pub(crate) tf: HashMap<String, HashMap<String, f32>>,
    /// term → idf
    pub(crate) idf: HashMap<String, f32>,
    /// 文档总数
    pub(crate) doc_count: usize,
    /// IDF 是否过期（需要重建）
    pub(crate) idf_dirty: bool,
}

impl TfIdfIndex {
    pub(crate) fn new() -> Self {
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
    pub(crate) fn tokenize(text: &str) -> Vec<String> {
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
    pub(crate) fn add_document(&mut self, doc_id: &str, text: &str) {
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
    pub(crate) fn remove_document(&mut self, doc_id: &str) {
        if self.tf.remove(doc_id).is_some() {
            self.doc_count = self.doc_count.saturating_sub(1);
            self.idf_dirty = true;
        }
    }

    /// 重建 IDF（在批量 add/remove 之后调用，或搜索前惰性触发）
    pub(crate) fn rebuild_idf(&mut self) {
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
    pub(crate) fn ensure_idf_fresh(&mut self) {
        if self.idf_dirty {
            self.rebuild_idf();
        }
    }

    /// 计算 query 与文档的 TF-IDF 余弦相似度
    pub(crate) fn similarity(&self, query_tokens: &[String], doc_id: &str) -> f32 {
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
    ///
    /// 只读。IDF 的重建由写入方在释放写锁前完成（见 [`Self::ensure_idf_fresh`]），
    /// 否则召回就必须拿写锁——而一次规划要打 3 次召回、执行阶段还有 1 次，
    /// 全站的召回会因此彼此串行。
    pub(crate) fn search(&self, query: &str, limit: usize) -> Vec<(String, f32)> {
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
pub(crate) fn is_cjk(ch: char) -> bool {
    matches!(ch,
        '\u{4E00}'..='\u{9FFF}'   // CJK Unified Ideographs
        | '\u{3400}'..='\u{4DBF}' // CJK Extension A
        | '\u{3040}'..='\u{309F}' // Hiragana
        | '\u{30A0}'..='\u{30FF}' // Katakana
        | '\u{AC00}'..='\u{D7AF}' // Hangul
    )
}

// 复合评分

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
pub(crate) fn recency_score(created_at: &str) -> f32 {
    let hours_ago = chrono::DateTime::parse_from_rfc3339(created_at)
        .map(|dt| (Utc::now() - dt.with_timezone(&Utc)).num_hours().max(0) as f32)
        .unwrap_or(720.0); // 默认 30 天前
    1.0 / (1.0 + hours_ago / 24.0)
}

// AgentMemory 主结构

/// Agent 记忆管理器 (v3 — 智能记忆)
pub struct AgentMemory {
    /// 记忆文件目录
    pub(crate) memory_dir: PathBuf,
    /// 全部记忆条目 (id → entry)
    pub(crate) entries: RwLock<HashMap<String, MemoryEntry>>,
    /// 按用户分片的 TF-IDF 搜索索引（key 即 `MemoryEntry::user_id`）
    ///
    /// 分片而不是单表，是因为召回、去重都先按相似度取 top-N **再**做用户过滤：
    /// 单表时用户 A 的候选会被用户 B 的高分文档挤出候选池，表现为「明明存了却
    /// 召不回」，且没有任何日志能把它和「根本没记住」区分开。顺带也让 IDF 不再
    /// 被其他用户的语料污染。
    ///
    /// key 用 `Option<i32>`：遗留 markdown 导入的条目 `user_id` 为 `None`，它们
    /// 对普通用户不可见，单独成片后就不再干扰任何人的词权重。
    pub(crate) indexes: RwLock<HashMap<Option<i32>, TfIdfIndex>>,
    /// 有低优先级变更（访问计数等）尚未落盘，由后台维护任务批量 flush
    pub(crate) dirty: std::sync::atomic::AtomicBool,
    /// 串行化全量快照，避免较旧的并发保存覆盖较新的状态
    pub(crate) persist_lock: Mutex<()>,
}

/// 索引文件名
pub(crate) const INDEX_FILE: &str = "memory_index.json";

/// 用户纠错模式的关键词
pub(crate) const CORRECTION_PATTERNS_ZH: &[&str] = &[
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
pub(crate) const CORRECTION_PATTERNS_EN: &[&str] = &[
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

