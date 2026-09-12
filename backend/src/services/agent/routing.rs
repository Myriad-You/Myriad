//! 多 Agent 路由模块
//!
//! 按能力前缀把步骤分到 `AgentRole`（Data / Content / Creative / System）。
//! Orchestrator 前缀为空，`route_capability` 不会选中它。
//! `default_tier` 是展示用；实际模型选择走 `TierRouter`。

use crate::config::ModelTier;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Agent Profile

/// Agent 角色类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    /// 编排者角色（前缀为空，`route_capability` 不会选出）
    Orchestrator,
    /// 数据工作者：平台数据获取、Brew 读取、API 调用
    DataWorker,
    /// 内容工作者：总结、分析、过滤、搜索
    ContentWorker,
    /// 创意工作者：自由对话、代码生成、Tapp 生成、图片生成
    CreativeWorker,
    /// 系统工作者：路由导航、音乐控制、缓存操作
    SystemWorker,
}

impl AgentRole {
    /// 获取角色的友好名称
    pub fn display_name(&self) -> &'static str {
        match self {
            AgentRole::Orchestrator => "Orchestrator",
            AgentRole::DataWorker => "Data Worker",
            AgentRole::ContentWorker => "Content Worker",
            AgentRole::CreativeWorker => "Creative Worker",
            AgentRole::SystemWorker => "System Worker",
        }
    }

    /// 获取角色的默认 ModelTier
    pub fn default_tier(&self) -> ModelTier {
        match self {
            AgentRole::Orchestrator => ModelTier::Pro,
            AgentRole::DataWorker => ModelTier::Standard,
            AgentRole::ContentWorker => ModelTier::Standard,
            AgentRole::CreativeWorker => ModelTier::Pro,
            AgentRole::SystemWorker => ModelTier::Standard,
        }
    }

    /// 获取角色的 emoji 标识
    pub fn icon(&self) -> &'static str {
        match self {
            AgentRole::Orchestrator => "🧠",
            AgentRole::DataWorker => "📊",
            AgentRole::ContentWorker => "📝",
            AgentRole::CreativeWorker => "🎨",
            AgentRole::SystemWorker => "⚙️",
        }
    }
}

/// Agent 身份配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    /// Agent ID
    pub id: String,
    /// Agent 角色
    pub role: AgentRole,
    /// 该 Agent 可调用的能力子集（前缀匹配）
    pub capability_prefixes: Vec<String>,
    /// 默认模型层级
    pub default_tier: ModelTier,
    /// 最大并发数
    pub max_concurrency: usize,
    /// 简短描述
    pub description: String,
}

// Agent Router

/// 多 Agent 路由器
///
/// 根据能力 ID 将任务路由到合适的 `AgentRole`。
/// `default_tier` 仅展示；实际模型选择走 `TierRouter`。
pub struct AgentRouter {
    /// 角色 → Agent 配置
    profiles: HashMap<AgentRole, AgentProfile>,
    /// 能力前缀 → 角色 映射缓存
    prefix_cache: HashMap<String, AgentRole>,
}

impl AgentRouter {
    /// 创建带默认 Agent 配置的路由器
    pub fn new() -> Self {
        let mut profiles = HashMap::new();
        let mut prefix_cache = HashMap::new();

        // Orchestrator
        profiles.insert(
            AgentRole::Orchestrator,
            AgentProfile {
                id: "orchestrator".to_string(),
                role: AgentRole::Orchestrator,
                capability_prefixes: vec![], // Orchestrator 不直接执行能力
                default_tier: ModelTier::Pro,
                max_concurrency: 1,
                description: "Planning, decisions, and summaries".to_string(),
            },
        );

        // Data Worker
        let data_prefixes = vec![
            "platform.".to_string(),
            "brew.".to_string(),
            "steam.".to_string(),
            "bilibili.".to_string(),
            "bangumi.".to_string(),
            "github.".to_string(),
            "netease.".to_string(),
            "http.fetch".to_string(),
            "notion.".to_string(),
            "weather.".to_string(),
            "hitokoto.".to_string(),
            "data.".to_string(),
            "export.".to_string(),
            "database.".to_string(),
            "rsshub.".to_string(),
            "web.".to_string(),
            "mcp.".to_string(),
        ];
        for p in &data_prefixes {
            prefix_cache.insert(p.clone(), AgentRole::DataWorker);
        }
        profiles.insert(
            AgentRole::DataWorker,
            AgentProfile {
                id: "data-worker".to_string(),
                role: AgentRole::DataWorker,
                capability_prefixes: data_prefixes,
                default_tier: ModelTier::Standard,
                max_concurrency: 4,
                description: "Platform data, API calls, and transforms".to_string(),
            },
        );

        // Content Worker
        let content_prefixes = vec![
            "ai.summarize".to_string(),
            "ai.analyze".to_string(),
            "ai.recommend".to_string(),
            "ai.webSearch".to_string(),
            "ai.groundingSearch".to_string(),
            "smart.".to_string(),
            "search.".to_string(),
            "fuzzy.".to_string(),
            "translate.".to_string(),
            "brewlia.".to_string(),
            "compare.".to_string(),
            "icon.".to_string(),
        ];
        for p in &content_prefixes {
            prefix_cache.insert(p.clone(), AgentRole::ContentWorker);
        }
        profiles.insert(
            AgentRole::ContentWorker,
            AgentProfile {
                id: "content-worker".to_string(),
                role: AgentRole::ContentWorker,
                capability_prefixes: content_prefixes,
                default_tier: ModelTier::Standard,
                max_concurrency: 3,
                description: "Summarize, analyze, filter, and search".to_string(),
            },
        );

        // Creative Worker
        let creative_prefixes = vec![
            "ai.chat".to_string(),
            "ai.image".to_string(),
            "tapp.generate".to_string(),
            "prompt.".to_string(),
            "report.create".to_string(),
            "code.".to_string(),
            "speech.".to_string(),
        ];
        for p in &creative_prefixes {
            prefix_cache.insert(p.clone(), AgentRole::CreativeWorker);
        }
        profiles.insert(
            AgentRole::CreativeWorker,
            AgentProfile {
                id: "creative-worker".to_string(),
                role: AgentRole::CreativeWorker,
                capability_prefixes: creative_prefixes,
                default_tier: ModelTier::Pro,
                max_concurrency: 2,
                description: "Creative generation, chat, code, and images".to_string(),
            },
        );

        // System Worker
        let system_prefixes = vec![
            "router.".to_string(),
            "page.".to_string(),
            "music.".to_string(),
            "tapp.list".to_string(),
            "tapp.page".to_string(),
            "tapp.widget".to_string(),
            "tapp.windows".to_string(),
            "tapp.ui".to_string(),
            "tapp.interact".to_string(),
            "tapp.understand".to_string(),
            "tapp.storage".to_string(),
            "cache.".to_string(),
            "scheduler.".to_string(),
            "heartbeat.".to_string(),
            "config.".to_string(),
            "auth.".to_string(),
            "system.".to_string(),
            "task.".to_string(),
            "profile.".to_string(),
            "setup.".to_string(),
            "permission.".to_string(),
            "metadata.".to_string(),
            "context.".to_string(),
            "time.".to_string(),
            "stats.".to_string(),
            "storage.".to_string(),
            "content.write".to_string(),
            "brew.subscribe".to_string(),
            "brew.mark".to_string(),
            "brew.schedule".to_string(),
            "note.".to_string(),
            "bookmark.".to_string(),
            "reminder.".to_string(),
            "random.".to_string(),
            "proxy.".to_string(),
        ];
        for p in &system_prefixes {
            prefix_cache.insert(p.clone(), AgentRole::SystemWorker);
        }
        profiles.insert(
            AgentRole::SystemWorker,
            AgentProfile {
                id: "system-worker".to_string(),
                role: AgentRole::SystemWorker,
                capability_prefixes: system_prefixes,
                default_tier: ModelTier::Standard,
                max_concurrency: 4,
                description: "Routing, UI control, and system operations".to_string(),
            },
        );

        Self {
            profiles,
            prefix_cache,
        }
    }

    /// 根据能力 ID 路由到合适的 Agent 角色
    pub fn route_capability(&self, capability_id: &str) -> AgentRole {
        // 精确匹配
        if let Some(role) = self.prefix_cache.get(capability_id) {
            return *role;
        }

        // 前缀匹配（最长前缀优先，避免 "brew." 抢占 "brew.subscribe" 等特化前缀）
        let best = self
            .prefix_cache
            .iter()
            .filter(|(prefix, _)| capability_id.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len());
        if let Some((_, role)) = best {
            return *role;
        }

        // 默认路由到 ContentWorker（最通用）
        tracing::debug!(
            capability_id = capability_id,
            "[AgentRouter] No matching agent, routing to ContentWorker"
        );
        AgentRole::ContentWorker
    }

    /// 分析一组 Recipe Steps 的 Agent 分布
    pub fn analyze_distribution(
        &self,
        capability_ids: &[String],
    ) -> HashMap<AgentRole, Vec<String>> {
        let mut distribution: HashMap<AgentRole, Vec<String>> = HashMap::new();

        for cap_id in capability_ids {
            let role = self.route_capability(cap_id);
            distribution.entry(role).or_default().push(cap_id.clone());
        }

        distribution
    }

    /// 生成任务分配摘要（用于日志和前端展示）
    pub fn summarize_assignment(&self, capability_ids: &[String]) -> TaskAssignment {
        let distribution = self.analyze_distribution(capability_ids);

        let agents: Vec<AgentAssignment> = distribution
            .iter()
            .map(|(role, caps)| AgentAssignment {
                role: *role,
                agent_id: self
                    .profiles
                    .get(role)
                    .map(|p| p.id.clone())
                    .unwrap_or_default(),
                display_name: role.display_name().to_string(),
                icon: role.icon().to_string(),
                tier: role.default_tier(),
                capabilities: caps.clone(),
            })
            .collect();

        let total_agents = agents.len();
        let has_pro = agents.iter().any(|a| a.tier == ModelTier::Pro);
        let has_standard = agents.iter().any(|a| a.tier == ModelTier::Standard);

        TaskAssignment {
            agents,
            total_agents,
            is_multi_agent: total_agents > 1,
            tier_mix: if has_pro && has_standard {
                "mixed".to_string()
            } else if has_pro {
                "pro".to_string()
            } else {
                "standard".to_string()
            },
        }
    }
}

impl Default for AgentRouter {
    fn default() -> Self {
        Self::new()
    }
}

// 任务分配结果

/// 任务分配结果（可序列化，用于 SSE 和 API）
///
/// Field names use camelCase for FE (`totalAgents`, `isMultiAgent`, …).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAssignment {
    /// 参与的 Agent 列表
    pub agents: Vec<AgentAssignment>,
    /// 参与的 Agent 数量
    #[serde(rename = "totalAgents", alias = "total_agents")]
    pub total_agents: usize,
    /// 是否多 Agent 协作
    #[serde(rename = "isMultiAgent", alias = "is_multi_agent")]
    pub is_multi_agent: bool,
    /// Tier 分布: "pro" | "standard" | "mixed"
    #[serde(rename = "tierMix", alias = "tier_mix")]
    pub tier_mix: String,
}

/// 单个 Agent 的任务分配
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAssignment {
    /// Agent 角色
    pub role: AgentRole,
    #[serde(rename = "agentId", alias = "agent_id")]
    pub agent_id: String,
    /// 显示名称
    #[serde(rename = "displayName", alias = "display_name")]
    pub display_name: String,
    pub icon: String,
    /// 使用的模型层级
    pub tier: ModelTier,
    /// 分配的能力列表
    pub capabilities: Vec<String>,
}

// 全局路由器

use once_cell::sync::Lazy;

static AGENT_ROUTER: Lazy<AgentRouter> = Lazy::new(AgentRouter::new);

/// 获取全局 Agent 路由器
pub fn get_router() -> &'static AgentRouter {
    &AGENT_ROUTER
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_worker_routing() {
        let router = AgentRouter::new();
        assert_eq!(
            router.route_capability("platform.read"),
            AgentRole::DataWorker
        );
        assert_eq!(router.route_capability("brew.items"), AgentRole::DataWorker);
        assert_eq!(router.route_capability("http.fetch"), AgentRole::DataWorker);
        assert_eq!(router.route_capability("steam.user"), AgentRole::DataWorker);
        assert_eq!(
            router.route_capability("bangumi.collections"),
            AgentRole::DataWorker
        );
    }

    #[test]
    fn test_content_worker_routing() {
        let router = AgentRouter::new();
        assert_eq!(
            router.route_capability("ai.summarize"),
            AgentRole::ContentWorker
        );
        assert_eq!(
            router.route_capability("ai.analyze"),
            AgentRole::ContentWorker
        );
        assert_eq!(
            router.route_capability("ai.recommend"),
            AgentRole::ContentWorker
        );
        assert_eq!(
            router.route_capability("search.global"),
            AgentRole::ContentWorker
        );
    }

    #[test]
    fn test_creative_worker_routing() {
        let router = AgentRouter::new();
        assert_eq!(
            router.route_capability("ai.chat"),
            AgentRole::CreativeWorker
        );
        assert_eq!(
            router.route_capability("ai.image"),
            AgentRole::CreativeWorker
        );
        assert_eq!(
            router.route_capability("tapp.generate"),
            AgentRole::CreativeWorker
        );
    }

    #[test]
    fn test_system_worker_routing() {
        let router = AgentRouter::new();
        assert_eq!(
            router.route_capability("router.navigate"),
            AgentRole::SystemWorker
        );
        assert_eq!(
            router.route_capability("music.control"),
            AgentRole::SystemWorker
        );
        assert_eq!(
            router.route_capability("cache.clear"),
            AgentRole::SystemWorker
        );
    }

    #[test]
    fn test_multi_agent_distribution() {
        let router = AgentRouter::new();
        let caps = vec![
            "platform.read".to_string(),
            "ai.summarize".to_string(),
            "ai.image".to_string(),
        ];
        let dist = router.analyze_distribution(&caps);

        assert!(dist.contains_key(&AgentRole::DataWorker));
        assert!(dist.contains_key(&AgentRole::ContentWorker));
        assert!(dist.contains_key(&AgentRole::CreativeWorker));
    }

    #[test]
    fn test_task_assignment_summary() {
        let router = AgentRouter::new();
        let caps = vec!["platform.read".to_string(), "ai.summarize".to_string()];
        let assignment = router.summarize_assignment(&caps);

        assert_eq!(assignment.total_agents, 2);
        assert!(assignment.is_multi_agent);
    }
}
