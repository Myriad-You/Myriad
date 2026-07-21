//! 能力注册表模块
//!
//! 管理系统所有可用能力的注册、查询和匹配

pub mod definitions;
mod utils;

pub use utils::*;

use super::types::*;
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 全局能力注册表
static CAPABILITY_REGISTRY: Lazy<Arc<RwLock<CapabilityRegistry>>> =
    Lazy::new(|| Arc::new(RwLock::new(CapabilityRegistry::new())));

/// 能力注册表
pub struct CapabilityRegistry {
    /// 能力 ID -> 能力定义
    capabilities: HashMap<String, Capability>,
    /// 类别 -> 能力 ID 列表
    by_category: HashMap<CapabilityCategory, Vec<String>>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            capabilities: HashMap::new(),
            by_category: HashMap::new(),
        };
        // 注册内置能力
        definitions::register_all(&mut registry);
        registry
    }

    /// 注册一个能力
    pub fn register(&mut self, capability: Capability) {
        let id = capability.id.clone();

        // 更新类别索引
        self.by_category
            .entry(capability.category.clone())
            .or_default()
            .push(id.clone());

        self.capabilities.insert(id, capability);
    }

    /// 根据 ID 获取能力
    pub fn get(&self, id: &str) -> Option<&Capability> {
        self.capabilities.get(id)
    }

    /// 获取所有能力
    pub fn get_all(&self) -> Vec<&Capability> {
        self.capabilities.values().collect()
    }
}

impl Default for CapabilityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============ 公共 API ============

/// 获取全局能力注册表（只读）
pub async fn get_registry() -> tokio::sync::RwLockReadGuard<'static, CapabilityRegistry> {
    CAPABILITY_REGISTRY.read().await
}

/// 异步版本：检查能力是否需要确认（从注册表读取）
pub async fn capability_requires_confirmation_async(
    capability_id: &str,
) -> Option<(String, RiskLevel)> {
    if let Some(cap) = get_capability_by_id(capability_id).await {
        if cap.requires_confirmation || cap.risk_level != RiskLevel::None {
            let message = cap
                .confirmation_message
                .clone()
                .unwrap_or_else(|| crate::services::agent::response_agent::will_execute(&cap.name));
            return Some((message, cap.risk_level));
        }
    }
    get_sensitive_capabilities()
        .get(capability_id)
        .map(|(msg, risk)| (msg.to_string(), *risk))
}

/// 获取能力摘要（用于 AI 提示）
pub async fn get_capability_summary() -> Value {
    let registry = get_registry().await;

    let mut by_category: std::collections::HashMap<String, Vec<Value>> =
        std::collections::HashMap::new();

    for cap in registry.get_all() {
        let usage_hint = get_capability_usage_hint(&cap.id);
        let category = get_capability_category_name(&cap.category);

        let cap_info = json!({
            "id": cap.id,
            "name": cap.name,
            "hint": usage_hint,
            "ai": cap.requires_ai
        });

        by_category.entry(category).or_default().push(cap_info);
    }

    json!({
        "total": registry.get_all().len(),
        "byCategory": by_category,
        "quickReference": get_quick_reference()
    })
}

/// 获取能力紧凑索引（用于 AI 提示的渐进式披露）
///
/// 返回仅包含 ID + 一句话 hint 的轻量列表，大幅减少 prompt token 用量。
/// AI 根据此索引选出 `suggested_capabilities`，后续再按需加载完整 schema。
pub async fn get_compact_index() -> Value {
    let registry = get_registry().await;

    // Gemini Key 缺失时标记联网搜索能力不可用，避免 Planner 选中后必然失败
    let gemini_available = {
        let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        config
            .gemini_api_key
            .as_ref()
            .is_some_and(|k| !k.trim().is_empty())
            || config
                .pro_gemini_api_key
                .as_ref()
                .is_some_and(|k| !k.trim().is_empty())
    };

    let mut by_category: std::collections::HashMap<String, Vec<Value>> =
        std::collections::HashMap::new();

    for cap in registry.get_all() {
        let base_hint = get_capability_usage_hint(&cap.id);
        let category = get_capability_category_name(&cap.category);

        let needs_gemini = cap.id == "ai.webSearch" || cap.id == "ai.groundingSearch";
        let hint = if needs_gemini && !gemini_available {
            format!("{}（不可用：Gemini API Key 未配置）", base_hint)
        } else {
            base_hint.to_string()
        };

        // 提取必需参数名（帮助 AI 正确构建 params）
        let mut entry = json!({ "id": cap.id, "h": hint });
        if needs_gemini && !gemini_available {
            entry
                .as_object_mut()
                .unwrap()
                .insert("unavailable".to_string(), json!(true));
        }
        if let Some(required) = cap.input_schema.get("required").and_then(|v| v.as_array()) {
            let param_names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
            if !param_names.is_empty() {
                entry
                    .as_object_mut()
                    .unwrap()
                    .insert("p".to_string(), json!(param_names));
            }
        }

        by_category.entry(category).or_default().push(entry);
    }

    // 合并动态 Skills 到索引
    let mut total = registry.get_all().len();
    if let Some(skill_registry) = super::skill::get_skill_registry() {
        let skill_index = skill_registry.get_compact_index().await;
        if !skill_index.is_empty() {
            total += skill_index.len();
            by_category
                .entry("动态技能".to_string())
                .or_default()
                .extend(skill_index);
        }
    }

    // 合并 MCP 工具到索引
    if let Some(mcp_manager) = super::mcp::get_mcp_manager() {
        let mcp_tools = mcp_manager.list_tools().await;
        if !mcp_tools.is_empty() {
            total += mcp_tools.len();
            let mcp_entries: Vec<Value> = mcp_tools
                .iter()
                .map(|(server_id, tool)| {
                    json!({
                        "id": format!("mcp.{}.{}", server_id, tool.name),
                        "h": if tool.description.is_empty() {
                            format!("MCP tool from {}", server_id)
                        } else {
                            tool.description.chars().take(80).collect::<String>()
                        }
                    })
                })
                .collect();
            by_category
                .entry("MCP 工具".to_string())
                .or_default()
                .extend(mcp_entries);
        }
    }

    json!({
        "total": total,
        "caps": by_category
    })
}

/// 根据 ID 列表获取完整能力定义（渐进式披露第二阶段）
pub async fn get_capabilities_by_ids(ids: &[String]) -> Vec<Capability> {
    let mut capabilities = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(capability) = get_capability_by_id(id).await {
            capabilities.push(capability);
        }
    }
    capabilities
}

/// Resolve either a built-in capability or a currently advertised MCP tool.
pub async fn get_capability_by_id(id: &str) -> Option<Capability> {
    if let Some(capability) = {
        let registry = get_registry().await;
        registry.get(id).cloned()
    } {
        return Some(capability);
    }

    if !id.starts_with("mcp.") {
        return None;
    }

    let manager = super::mcp::get_mcp_manager()?;
    manager
        .list_tools()
        .await
        .into_iter()
        .find(|(server_id, tool)| id == format!("mcp.{}.{}", server_id, tool.name))
        .map(|(server_id, tool)| mcp_capability(&server_id, &tool))
}

fn mcp_capability(server_id: &str, tool: &super::mcp::protocol::McpToolDef) -> Capability {
    Capability {
        id: format!("mcp.{}.{}", server_id, tool.name),
        name: format!("MCP: {}", tool.name),
        description: tool.description.clone(),
        category: CapabilityCategory::ExternalIntegration,
        supported_actions: vec![],
        input_schema: tool.input_schema.clone(),
        output_schema: json!({ "type": "string" }),
        required_permissions: vec!["mcp:execute".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(30_000),
        // MCP tools are arbitrary external integrations. Require an explicit
        // confirmation unless the system-user policy blocks them earlier.
        requires_confirmation: true,
        confirmation_message: Some(format!(
            "将调用外部 MCP 服务 '{}' 的工具 '{}'",
            server_id, tool.name
        )),
        risk_level: RiskLevel::High,
    }
}

/// 获取能力类别的友好名称
fn get_capability_category_name(category: &CapabilityCategory) -> String {
    match category {
        CapabilityCategory::DataRead => "数据读取".to_string(),
        CapabilityCategory::DataWrite => "数据写入".to_string(),
        CapabilityCategory::AiProcess => "AI处理".to_string(),
        CapabilityCategory::ResourceCreate => "资源创建".to_string(),
        CapabilityCategory::SystemOp => "系统操作".to_string(),
        CapabilityCategory::ExternalIntegration => "外部集成".to_string(),
        CapabilityCategory::UiControl => "界面控制".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_initialization() {
        let registry = CapabilityRegistry::new();
        assert!(!registry.capabilities.is_empty());
        assert!(registry.get("platform.read").is_some());
        assert!(registry.get("ai.summarize").is_some());
    }

    #[test]
    fn mcp_capability_is_qualified_and_sensitive() {
        let tool = super::super::mcp::protocol::McpToolDef {
            name: "lookup".to_string(),
            description: "Look up external data".to_string(),
            input_schema: json!({"type": "object"}),
        };
        let capability = mcp_capability("docs", &tool);
        assert_eq!(capability.id, "mcp.docs.lookup");
        assert_eq!(capability.required_permissions, vec!["mcp:execute"]);
        assert!(capability.requires_confirmation);
        assert_eq!(capability.risk_level, RiskLevel::High);
    }
}
