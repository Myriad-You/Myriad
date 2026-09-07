//! ModelTier 智能路由
//!
//! 根据能力类别和动作类型，决定使用 Standard 还是 Pro 模型。
//! Pro 模型用于复杂推理、规划和创造性任务；Standard 模型用于数据获取和常规操作。

use crate::config::ModelTier;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 任务复杂度等级
#[derive(Debug, Clone, PartialEq)]
pub enum TaskComplexity {
    /// 数据读取、格式化、简单查询 → Standard
    Simple,
    /// 内容总结、模式匹配、条件判断 → Standard (失败时可 fallback Pro)
    Medium,
    /// 多步推理、创意生成、复杂分析 → Pro
    Complex,
    /// 规划、决策、评估、纠错 → 强制 Pro
    Critical,
}

impl TaskComplexity {
    /// 转换为对应的 ModelTier
    pub fn to_tier(&self) -> ModelTier {
        match self {
            TaskComplexity::Simple => ModelTier::Standard,
            TaskComplexity::Medium => ModelTier::Standard,
            TaskComplexity::Complex => ModelTier::Pro,
            TaskComplexity::Critical => ModelTier::Pro,
        }
    }

    /// Simple 级别的能力不使用 LLM，不需要 tier 标注和计量
    pub fn requires_llm(&self) -> bool {
        !matches!(self, TaskComplexity::Simple)
    }
}

/// ModelTier 路由器
///
/// 根据能力 ID 和动作推断应使用的模型层级。
/// 支持显式覆盖（Recipe 步骤可指定 tier）。
pub struct TierRouter;

impl TierRouter {
    /// 显式登记的复杂度规则；`None` 表示这个能力没有被登记。
    ///
    /// **顺序敏感**：精确分支必须排在同前缀的通配分支之前。`brew.generateReadingList`
    /// 原先排在 `brew.` 前缀规则之后，永远被判成 Simple——这条确实要跑 AI 的能力
    /// 因此从未计入 LLM 用量。
    ///
    /// 动态命名空间（`skill:` / `mcp.`）也在此登记：它们不在能力注册表里，但同样
    /// 会走 `suggest_tier`，落到兜底分支就会被无声地当成消耗 LLM。
    fn complexity_rule(capability_id: &str) -> Option<TaskComplexity> {
        let complexity = match capability_id {
            // Critical：强制 Pro
            // 复杂分析和对比类
            "ai.analyze" | "compare.content" | "ai.recommend" => TaskComplexity::Critical,

            // Complex：Pro
            // 创造性生成
            "ai.chat" | "prompt.generate" | "tapp.generate" => TaskComplexity::Complex,
            // 代码理解
            "code.explain" => TaskComplexity::Complex,

            // Medium：Standard (可升级 Pro)
            // 总结和注释类（定式化 AI 任务）
            "ai.summarize" | "brewlia.annotate" | "brewlia.podcast" | "translate.text" => {
                TaskComplexity::Medium
            }
            // 智能过滤和搜索
            "smart.filter" | "ai.webSearch" | "ai.groundingSearch" => TaskComplexity::Medium,
            // 图标推荐
            "icon.recommend" => TaskComplexity::Medium,

            // brew：要跑 AI 的两条必须排在 `brew.` 前缀规则之前
            "brew.discover" | "brew.generateReadingList" => TaskComplexity::Medium,
            id if id.starts_with("brew.") => TaskComplexity::Simple,
            // 所有 platform 数据读取与写入
            id if id.starts_with("platform.") => TaskComplexity::Simple,
            // tapp 交互和理解需要 AI（排在其余 tapp 规则之前）
            "tapp.ui" | "tapp.understand" | "tapp.interact" => TaskComplexity::Medium,
            // tapp 查询 / 安装 / 存储 / 窗口操作都不消耗 LLM
            "tapp.list" | "tapp.page" | "tapp.widget" | "tapp.windows" | "tapp.pageContent"
            | "tapp.install" | "tapp.storage" => TaskComplexity::Simple,
            id if id.starts_with("tapp.window.") => TaskComplexity::Simple,
            // 音乐控制和状态
            id if id.starts_with("music.") => TaskComplexity::Simple,
            // 网易云音乐查询
            id if id.starts_with("netease.") => TaskComplexity::Simple,
            // 路由和页面
            "router.navigate" | "router.state" => TaskComplexity::Simple,
            "page.content" => TaskComplexity::Simple,
            "page.interact" | "page.understand" => TaskComplexity::Medium,
            // 搜索
            "search.global" | "search.fuzzy" => TaskComplexity::Simple,
            // 系统和数据操作
            "data.transform" | "export.data" | "cache.status" | "cache.clear"
            | "system.metrics" | "stats.overview" | "profile.summary" | "task.status"
            | "task.submit" | "image.cache" | "scheduler.list" | "scheduler.create"
            | "scheduler.trigger" | "heartbeat.list" | "heartbeat.create" | "heartbeat.update"
            | "heartbeat.delete" | "heartbeat.toggle" | "setup.status" | "auth.status"
            | "time.info" | "config.get" | "metadata.history" | "rsshub.instances"
            | "rsshub.healthcheck" | "context.reference" => TaskComplexity::Simple,
            // 外部集成（纯数据获取）
            "http.fetch"
            | "web.scrape"
            | "hitokoto.get"
            | "weather.get"
            | "proxy.image"
            | "bilibili.user"
            | "bilibili.video"
            | "bilibili.bangumi"
            | "bangumi.user"
            | "bangumi.collections"
            | "steam.user"
            | "steam.game"
            | "steam.wishlist"
            | "github.repos" => TaskComplexity::Simple,
            // Notion 查询
            "notion.query" => TaskComplexity::Simple,
            // 数据写入
            "storage.set" | "content.write" => TaskComplexity::Simple,
            // 资源创建（需要一定 AI 能力）
            "report.create" | "note.create" | "bookmark.save" | "reminder.create" => {
                TaskComplexity::Medium
            }
            // TTS（调用外部 TTS API，不使用 LLM）
            "speech.tts" => TaskComplexity::Simple,
            // AI 图像生成（调用外部图像 API，不使用 LLM）
            "ai.image" => TaskComplexity::Simple,
            // Tripo 3D：外部 API，能力表里 requires_ai = false
            id if id.starts_with("model3d.") => TaskComplexity::Simple,
            // 数据库查询
            id if id.starts_with("database.") => TaskComplexity::Simple,
            // 其余只读查询
            "random.content" | "permission.check" | "report.list" => TaskComplexity::Simple,

            // Skill 执行会先跑一次 AI 把 instructions 翻译成调用序列
            id if id.starts_with("skill:") => TaskComplexity::Medium,
            // MCP 工具是外部进程调用，不消耗本地模型预算
            id if id.starts_with("mcp.") => TaskComplexity::Simple,

            _ => return None,
        };
        Some(complexity)
    }

    /// 该能力是否有显式登记的复杂度规则。
    ///
    /// 供测试断言「注册表里的每个能力都被登记过」——这张表和能力定义是两份互不
    /// 校验的真相，漏登记不会报错，只会静默按兜底值计费。
    #[cfg(test)]
    pub fn has_explicit_rule(capability_id: &str) -> bool {
        Self::complexity_rule(capability_id).is_some()
    }

    /// 根据能力 ID 推断任务复杂度
    pub fn assess_complexity(capability_id: &str) -> TaskComplexity {
        Self::complexity_rule(capability_id).unwrap_or_else(|| {
            tracing::debug!(
                capability_id = capability_id,
                "[TierRouter] Unknown capability, defaulting to Medium"
            );
            TaskComplexity::Medium
        })
    }

    /// 根据能力 ID 推断 ModelTier
    pub fn resolve_tier(capability_id: &str) -> ModelTier {
        Self::assess_complexity(capability_id).to_tier()
    }

    /// 该能力是否需要 LLM（不需要 LLM 的能力不参与 tier 标注和计量）
    pub fn requires_llm(capability_id: &str) -> bool {
        Self::assess_complexity(capability_id).requires_llm()
    }

    /// 带显式覆盖的 tier 解析
    ///
    /// 如果 `explicit_tier` 有值，直接使用（Recipe 步骤可指定）；
    /// 否则根据能力 ID 自动推断。
    pub fn resolve_with_override(
        capability_id: &str,
        explicit_tier: Option<ModelTier>,
    ) -> ModelTier {
        if let Some(tier) = explicit_tier {
            return tier;
        }
        Self::resolve_tier(capability_id)
    }
}

// 熔断器

/// 熔断器状态
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum CircuitState {
    /// 正常运行
    Closed = 0,
    /// 熔断开启，拒绝请求
    Open = 1,
    /// 半开，允许试探性请求
    HalfOpen = 2,
}

/// 简易熔断器
///
/// 每个 (provider, tier) 组合维护一个熔断器。
/// Pro 熔断 → 降级到 Standard；Standard 也熔断 → 返回错误。
pub struct CircuitBreaker {
    /// 连续失败次数
    failure_count: AtomicU32,
    /// 最后失败时间（unix ms）
    last_failure: AtomicU64,
    /// 当前状态
    state: AtomicU8,
    /// 连续失败 N 次后熔断
    threshold: u32,
    /// 熔断恢复时间（毫秒）
    recovery_ms: u64,
}

impl CircuitBreaker {
    /// 创建新的熔断器
    pub fn new(threshold: u32, recovery_ms: u64) -> Self {
        Self {
            failure_count: AtomicU32::new(0),
            last_failure: AtomicU64::new(0),
            state: AtomicU8::new(CircuitState::Closed as u8),
            threshold,
            recovery_ms,
        }
    }

    /// 默认配置：连续 3 次失败后熔断，60 秒恢复
    pub fn default() -> Self {
        Self::new(3, 60_000)
    }

    /// 检查是否允许请求
    pub fn is_allowed(&self) -> bool {
        let state = self.get_state();
        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // 检查是否到了恢复时间
                let now = Self::now_ms();
                let last = self.last_failure.load(Ordering::Acquire);
                if now - last >= self.recovery_ms {
                    // 切换到半开状态
                    self.state
                        .store(CircuitState::HalfOpen as u8, Ordering::Release);
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true, // 允许试探
        }
    }

    /// 记录成功
    pub fn record_success(&self) {
        self.failure_count.store(0, Ordering::Release);
        self.state
            .store(CircuitState::Closed as u8, Ordering::Release);
    }

    /// 记录失败
    pub fn record_failure(&self) {
        let count = self.failure_count.fetch_add(1, Ordering::AcqRel) + 1;
        self.last_failure.store(Self::now_ms(), Ordering::Release);

        if count >= self.threshold {
            let prev = self.state.load(Ordering::Acquire);
            if prev == CircuitState::HalfOpen as u8 || count >= self.threshold {
                self.state
                    .store(CircuitState::Open as u8, Ordering::Release);
                tracing::warn!(
                    failures = count,
                    "[CircuitBreaker] Circuit opened after {} consecutive failures",
                    count
                );
            }
        }
    }

    /// 获取当前状态
    pub fn get_state(&self) -> CircuitState {
        match self.state.load(Ordering::Acquire) {
            0 => CircuitState::Closed,
            1 => CircuitState::Open,
            2 => CircuitState::HalfOpen,
            _ => CircuitState::Closed,
        }
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

/// 全局熔断器（三个模型层级各一个）
static PRO_BREAKER: once_cell::sync::Lazy<CircuitBreaker> =
    once_cell::sync::Lazy::new(CircuitBreaker::default);
static STANDARD_BREAKER: once_cell::sync::Lazy<CircuitBreaker> =
    once_cell::sync::Lazy::new(CircuitBreaker::default);
static LITE_BREAKER: once_cell::sync::Lazy<CircuitBreaker> =
    once_cell::sync::Lazy::new(CircuitBreaker::default);

/// 获取指定 tier 的熔断器
pub fn get_circuit_breaker(tier: ModelTier) -> &'static CircuitBreaker {
    match tier {
        ModelTier::Pro => &PRO_BREAKER,
        ModelTier::Standard => &STANDARD_BREAKER,
        ModelTier::Lite => &LITE_BREAKER,
    }
}

/// 带熔断器的 tier 解析
///
/// 如果目标 tier 已熔断，自动降级：
/// - Pro 熔断 → 降级到 Standard
/// - Standard 熔断 → 返回 None（调用方应显示错误）
pub fn resolve_with_circuit_breaker(
    capability_id: &str,
    explicit_tier: Option<ModelTier>,
) -> Option<ModelTier> {
    let tier = TierRouter::resolve_with_override(capability_id, explicit_tier);
    let breaker = get_circuit_breaker(tier);

    if breaker.is_allowed() {
        Some(tier)
    } else {
        // 降级
        match tier {
            ModelTier::Pro => {
                tracing::warn!(
                    capability_id = capability_id,
                    "[CircuitBreaker] Pro tier breaker open, degrading to Standard"
                );
                let std_breaker = get_circuit_breaker(ModelTier::Standard);
                if std_breaker.is_allowed() {
                    Some(ModelTier::Standard)
                } else {
                    tracing::error!(
                        capability_id = capability_id,
                        "[CircuitBreaker] Both tiers breaker open, rejecting request"
                    );
                    None
                }
            }
            ModelTier::Standard => {
                tracing::error!(
                    capability_id = capability_id,
                    "[CircuitBreaker] Standard tier breaker open, rejecting request"
                );
                None
            }
            ModelTier::Lite => {
                tracing::error!(
                    capability_id = capability_id,
                    "[CircuitBreaker] Lite tier breaker open, rejecting request"
                );
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_critical_capabilities_use_pro() {
        assert_eq!(TierRouter::resolve_tier("ai.analyze"), ModelTier::Pro);
        assert_eq!(TierRouter::resolve_tier("ai.recommend"), ModelTier::Pro);
        assert_eq!(TierRouter::resolve_tier("compare.content"), ModelTier::Pro);
    }

    #[tokio::test]
    async fn every_registered_capability_has_an_explicit_rule() {
        // This table and the capability registry are two independent sources of
        // truth with nothing keeping them in sync. A missing row does not fail
        // anything — it silently bills a plain data read as an LLM step.
        let registry = crate::services::agent::capability::get_registry().await;
        let unregistered: Vec<&str> = registry
            .get_all()
            .iter()
            .map(|cap| cap.id.as_str())
            .filter(|id| !TierRouter::has_explicit_rule(id))
            .collect();
        assert!(
            unregistered.is_empty(),
            "capabilities with no tier rule fall through to the Medium default \
             and are counted as LLM usage: {unregistered:?}"
        );
    }

    #[test]
    fn ai_dependent_brew_rules_win_over_the_brew_prefix() {
        // Regression: `brew.generateReadingList` sat after `starts_with("brew.")`,
        // so it resolved to Simple and never counted as LLM usage.
        assert!(TierRouter::requires_llm("brew.generateReadingList"));
        assert!(TierRouter::requires_llm("brew.discover"));
        assert!(!TierRouter::requires_llm("brew.items"));
        assert!(!TierRouter::requires_llm("brew.subscribe"));
    }

    #[test]
    fn ai_dependent_tapp_rules_win_over_the_tapp_rules_below_them() {
        assert!(TierRouter::requires_llm("tapp.understand"));
        assert!(TierRouter::requires_llm("tapp.interact"));
        assert!(!TierRouter::requires_llm("tapp.list"));
        assert!(!TierRouter::requires_llm("tapp.install"));
    }

    #[test]
    fn non_llm_capabilities_are_not_billed_as_llm_steps() {
        for id in [
            "image.cache",
            "task.submit",
            "web.scrape",
            "tapp.window.open",
            "tapp.window.close",
            "tapp.window.focus",
            "model3d.status",
            "model3d.generate",
            "model3d.rig",
            "model3d.retarget",
        ] {
            assert!(
                TierRouter::has_explicit_rule(id),
                "{id} must not rely on the default"
            );
            assert!(!TierRouter::requires_llm(id), "{id} does not call a model");
        }
    }

    #[test]
    fn dynamic_namespaces_are_classified_explicitly() {
        // Skills run an AI pass to turn their instructions into a call sequence.
        assert!(TierRouter::requires_llm("skill:_auto_daily_digest"));
        // MCP tools execute in an external process; they cost no local tokens.
        assert!(!TierRouter::requires_llm("mcp.docs.lookup"));
    }

    #[test]
    fn tapp_windows_query_is_not_matched_by_the_window_action_prefix() {
        assert!(TierRouter::has_explicit_rule("tapp.windows"));
        assert!(!TierRouter::requires_llm("tapp.windows"));
    }

    #[test]
    fn test_simple_capabilities_use_standard() {
        assert_eq!(
            TierRouter::resolve_tier("platform.read"),
            ModelTier::Standard
        );
        assert_eq!(TierRouter::resolve_tier("brew.items"), ModelTier::Standard);
        assert_eq!(
            TierRouter::resolve_tier("music.status"),
            ModelTier::Standard
        );
        assert_eq!(TierRouter::resolve_tier("http.fetch"), ModelTier::Standard);
    }

    #[test]
    fn test_medium_capabilities_use_standard() {
        assert_eq!(
            TierRouter::resolve_tier("ai.summarize"),
            ModelTier::Standard
        );
        assert_eq!(
            TierRouter::resolve_tier("smart.filter"),
            ModelTier::Standard
        );
    }

    #[test]
    fn test_complex_capabilities_use_pro() {
        assert_eq!(TierRouter::resolve_tier("ai.chat"), ModelTier::Pro);
        assert_eq!(TierRouter::resolve_tier("tapp.generate"), ModelTier::Pro);
    }

    #[test]
    fn test_explicit_override() {
        // Override should take precedence
        assert_eq!(
            TierRouter::resolve_with_override("platform.read", Some(ModelTier::Pro)),
            ModelTier::Pro
        );
        assert_eq!(
            TierRouter::resolve_with_override("ai.chat", Some(ModelTier::Standard)),
            ModelTier::Standard
        );
        // None falls back to auto
        assert_eq!(
            TierRouter::resolve_with_override("ai.chat", None),
            ModelTier::Pro
        );
    }
}
