//! 能力注册表模块
//!
//! 管理系统所有可用能力的注册、查询和匹配

pub mod definitions;
mod output_contract;
mod utils;

pub use output_contract::{check_output_contract, declared_output_fields, ContractViolation};
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

// 公共 API

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

/// 获取能力摘要（用于 AI 提示 + GET /api/agent/capabilities）
///
/// 契约（FE agentApi.getCapabilities 依赖）：
/// - `capabilities[]`：扁平列表（id/name/description/category/actions/requiresAi）
/// - `total` / `totalCount`：数量
/// - `byCategory` / `quickReference`：AI 提示用紧凑视图（保留兼容）
///
/// When `include_admin` is false, capabilities that require `system:admin`
/// (e.g. system.metrics) are omitted from discovery — execute-time still gates.
pub async fn get_capability_summary() -> Value {
    get_capability_summary_filtered(true).await
}

pub async fn get_capability_summary_filtered(include_admin: bool) -> Value {
    let registry = get_registry().await;
    let all = registry.get_all();

    let mut by_category: std::collections::HashMap<String, Vec<Value>> =
        std::collections::HashMap::new();
    let mut capabilities: Vec<Value> = Vec::with_capacity(all.len());

    for cap in all {
        if !include_admin
            && cap
                .required_permissions
                .iter()
                .any(|p| p == "system:admin")
        {
            continue;
        }
        let usage_hint = resolve_capability_hint(cap);
        let category = get_capability_category_name(&cap.category);

        let cap_info = json!({
            "id": cap.id,
            "name": cap.name,
            "hint": usage_hint,
            "ai": cap.requires_ai
        });
        by_category
            .entry(category.clone())
            .or_default()
            .push(cap_info);

        // FE Capability.actions is string[]; IntentAction unit variants already
        // serde as snake_case strings — project explicitly so newtype variants
        // never leak as objects into the public list.
        let mut actions: Vec<String> = cap
            .supported_actions
            .iter()
            .filter_map(|a| {
                let v = serde_json::to_value(a).ok()?;
                if let Some(s) = v.as_str() {
                    return Some(s.to_string());
                }
                // e.g. Unknown("x") → {"unknown":"x"}
                v.as_object().and_then(|o| {
                    let (k, val) = o.iter().next()?;
                    match val {
                        serde_json::Value::String(s) if !s.is_empty() => {
                            Some(format!("{k}:{s}"))
                        }
                        _ => Some(k.clone()),
                    }
                })
            })
            .collect();
        // Stable order for clients/tests
        actions.sort();
        actions.dedup();

        capabilities.push(json!({
            "id": cap.id,
            "name": cap.name,
            "description": cap.description,
            "category": category,
            "actions": actions,
            "requiresAi": cap.requires_ai,
        }));
    }

    let total = capabilities.len();
    json!({
        "total": total,
        "totalCount": total,
        "capabilities": capabilities,
        "byCategory": by_category,
        "quickReference": get_quick_reference()
    })
}

/// 获取能力紧凑索引（用于 AI 提示的渐进式披露）
///
/// 返回仅包含 ID + 一句话 hint 的轻量列表，大幅减少 prompt token 用量。
/// AI 根据此索引选出 `suggested_capabilities`，后续再按需加载完整 schema。
///
/// 每个条目的字段：`id` / `h` 用途 / `p` 必需入参 / `o` 声明的输出字段。
/// `o` 让 Planner 能写出 `"dataFrom": "search.results"` 这类精确引用，
/// 而不是只引用整个步骤输出再由执行层猜哪个字段有用。
///
/// Note: AI 能力（含 ai.webSearch）始终保持注册与可规划；缺失 API Key 时由执行层
/// 返回非重试错误，而不是在索引中降级/隐藏能力。
pub async fn get_compact_index() -> Value {
    let registry = get_registry().await;

    let mut by_category: std::collections::HashMap<String, Vec<Value>> =
        std::collections::HashMap::new();

    for cap in registry.get_all() {
        let hint = resolve_capability_hint(cap);
        let category = get_capability_category_name(&cap.category);

        // 提取必需参数名（帮助 AI 正确构建 params）
        let mut entry = json!({ "id": cap.id, "h": hint });
        if let Some(required) = cap.input_schema.get("required").and_then(|v| v.as_array()) {
            let param_names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
            if !param_names.is_empty() {
                entry
                    .as_object_mut()
                    .unwrap()
                    .insert("p".to_string(), json!(param_names));
            }
        }
        // 声明的输出字段：供 Planner 做 `"xxxFrom": "step_id.字段"` 的精确引用
        let output_fields = declared_output_fields(&cap.output_schema);
        if !output_fields.is_empty() {
            entry
                .as_object_mut()
                .unwrap()
                .insert("o".to_string(), json!(output_fields));
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
    let (server_id, tool) = manager
        .list_tools()
        .await
        .into_iter()
        .find(|(server_id, tool)| id == format!("mcp.{}.{}", server_id, tool.name))?;
    let trusted = manager.server_trusts_annotations(&server_id).await;
    Some(mcp_capability(&server_id, &tool, trusted))
}

/// Risk classification for an MCP tool.
///
/// Every tool used to be `High` + always-confirm. That is safe in isolation but
/// corrosive in aggregate: a read-only lookup and a destructive write raise the
/// same dialog, so users learn to dismiss it and the confirmation stops carrying
/// information by the time a genuinely dangerous call arrives.
///
/// A tool's own `annotations` can tell the two apart, but only for a server the
/// operator has marked `trust_annotations` — the MCP spec is explicit that these
/// are hints and that clients must not base security decisions on annotations
/// from untrusted servers. Without that opt-in, nothing changes.
///
/// Spec defaults are load-bearing here: `destructiveHint` defaults to *true*, so
/// silence means "assume destructive", never "assume safe".
fn mcp_tool_risk(
    annotations: Option<&super::mcp::protocol::McpToolAnnotations>,
    trusted: bool,
) -> (RiskLevel, bool) {
    if !trusted {
        return (RiskLevel::High, true);
    }
    let Some(annotations) = annotations else {
        // Trusted server, but the tool declares nothing: no basis to downgrade.
        return (RiskLevel::High, true);
    };

    if annotations.read_only_hint == Some(true) {
        return (RiskLevel::None, false);
    }
    // Writes. Only an explicit `destructiveHint: false` earns the lower tier;
    // an unset hint keeps the spec default of "may be destructive".
    if annotations.destructive_hint == Some(false) {
        return (RiskLevel::Medium, true);
    }
    (RiskLevel::High, true)
}

fn mcp_capability(
    server_id: &str,
    tool: &super::mcp::protocol::McpToolDef,
    trust_annotations: bool,
) -> Capability {
    let (risk_level, requires_confirmation) =
        mcp_tool_risk(tool.annotations.as_ref(), trust_annotations);
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
        requires_confirmation,
        confirmation_message: requires_confirmation
            .then(|| format!("将调用外部 MCP 服务 '{}' 的工具 '{}'", server_id, tool.name)),
        risk_level,
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
    use super::super::mcp::protocol::{McpToolAnnotations, McpToolDef};
    use super::*;

    #[test]
    fn test_registry_initialization() {
        let registry = CapabilityRegistry::new();
        assert!(!registry.capabilities.is_empty());
        assert!(registry.get("platform.read").is_some());
        assert!(registry.get("ai.summarize").is_some());
    }

    #[tokio::test]
    async fn compact_index_exposes_declared_output_fields() {
        let index = get_compact_index().await;
        let caps = index
            .get("caps")
            .and_then(Value::as_object)
            .expect("compact index must carry caps");
        let entry = caps
            .values()
            .filter_map(Value::as_array)
            .flatten()
            .find(|entry| entry.get("id").and_then(Value::as_str) == Some("ai.summarize"))
            .expect("ai.summarize must be indexed");

        let outputs: Vec<&str> = entry
            .get("o")
            .and_then(Value::as_array)
            .expect("declared output fields must reach the planner index")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(outputs.contains(&"summary"), "got {outputs:?}");
        assert!(outputs.contains(&"keyPoints"), "got {outputs:?}");
    }

    #[tokio::test]
    async fn no_capability_is_indexed_without_a_description() {
        // An entry with an empty `h` reaches the planner as a bare ID, which
        // makes the capability effectively unselectable. 20 capabilities were in
        // that state before `resolve_capability_hint` fell back to description.
        let index = get_compact_index().await;
        let blank: Vec<String> = index
            .get("caps")
            .and_then(Value::as_object)
            .expect("caps")
            .values()
            .filter_map(Value::as_array)
            .flatten()
            .filter(|entry| {
                entry
                    .get("h")
                    .and_then(Value::as_str)
                    .is_none_or(|hint| hint.trim().is_empty())
            })
            .filter_map(|entry| entry.get("id").and_then(Value::as_str).map(String::from))
            .collect();
        assert!(
            blank.is_empty(),
            "capabilities indexed with no description at all: {blank:?}"
        );
    }

    #[tokio::test]
    async fn hint_falls_back_to_the_capability_description() {
        // `translate.text` has no curated hint; it must still describe itself.
        let registry = get_registry().await;
        let cap = registry.get("translate.text").expect("translate.text");
        assert!(get_capability_usage_hint(&cap.id).is_empty());
        assert_eq!(resolve_capability_hint(cap), cap.description);
        assert!(!cap.description.is_empty());
    }

    #[tokio::test]
    async fn curated_hint_wins_over_the_description() {
        let registry = get_registry().await;
        let cap = registry.get("ai.summarize").expect("ai.summarize");
        assert_eq!(
            resolve_capability_hint(cap),
            get_capability_usage_hint("ai.summarize")
        );
    }

    #[tokio::test]
    async fn compact_index_omits_o_when_nothing_is_declared() {
        // `Capability::default()` leaves output_schema empty; such entries must
        // not emit an empty `o` list that the planner would read as "no output".
        let index = get_compact_index().await;
        let entries: Vec<&Value> = index
            .get("caps")
            .and_then(Value::as_object)
            .expect("caps")
            .values()
            .filter_map(Value::as_array)
            .flatten()
            .collect();
        assert!(!entries.is_empty());
        for entry in entries {
            if let Some(outputs) = entry.get("o") {
                assert!(
                    outputs.as_array().is_some_and(|list| !list.is_empty()),
                    "entry {entry} carries an empty output field list"
                );
            }
        }
    }

    fn mcp_tool(annotations: Option<McpToolAnnotations>) -> McpToolDef {
        McpToolDef {
            name: "lookup".to_string(),
            description: "Look up external data".to_string(),
            input_schema: json!({"type": "object"}),
            annotations,
        }
    }

    fn read_only() -> McpToolAnnotations {
        McpToolAnnotations {
            read_only_hint: Some(true),
            ..Default::default()
        }
    }

    #[test]
    fn mcp_capability_is_qualified_and_sensitive() {
        let capability = mcp_capability("docs", &mcp_tool(None), false);
        assert_eq!(capability.id, "mcp.docs.lookup");
        assert_eq!(capability.required_permissions, vec!["mcp:execute"]);
        assert!(capability.requires_confirmation);
        assert_eq!(capability.risk_level, RiskLevel::High);
    }

    #[test]
    fn annotations_from_an_untrusted_server_never_lower_risk() {
        // The whole point of the opt-in: a server must not be able to switch its
        // own confirmation off by declaring itself harmless.
        let capability = mcp_capability("docs", &mcp_tool(Some(read_only())), false);
        assert!(capability.requires_confirmation);
        assert_eq!(capability.risk_level, RiskLevel::High);
    }

    #[test]
    fn read_only_tools_on_a_trusted_server_skip_confirmation() {
        let capability = mcp_capability("docs", &mcp_tool(Some(read_only())), true);
        assert!(!capability.requires_confirmation);
        assert_eq!(capability.risk_level, RiskLevel::None);
        assert!(capability.confirmation_message.is_none());
    }

    #[test]
    fn a_trusted_server_that_declares_nothing_stays_high_risk() {
        let capability = mcp_capability("docs", &mcp_tool(None), true);
        assert!(capability.requires_confirmation);
        assert_eq!(capability.risk_level, RiskLevel::High);
    }

    #[test]
    fn unset_destructive_hint_keeps_the_spec_default_of_destructive() {
        // Spec default for destructiveHint is true, so a write tool that says
        // nothing must not be downgraded.
        let writes_silently = McpToolAnnotations {
            read_only_hint: Some(false),
            ..Default::default()
        };
        let capability = mcp_capability("docs", &mcp_tool(Some(writes_silently)), true);
        assert_eq!(capability.risk_level, RiskLevel::High);

        let writes_safely = McpToolAnnotations {
            read_only_hint: Some(false),
            destructive_hint: Some(false),
            ..Default::default()
        };
        let capability = mcp_capability("docs", &mcp_tool(Some(writes_safely)), true);
        assert_eq!(capability.risk_level, RiskLevel::Medium);
        assert!(
            capability.requires_confirmation,
            "non-destructive writes still confirm"
        );
    }

    #[test]
    fn destructive_tools_stay_high_even_on_a_trusted_server() {
        let destructive = McpToolAnnotations {
            read_only_hint: Some(false),
            destructive_hint: Some(true),
            ..Default::default()
        };
        let capability = mcp_capability("docs", &mcp_tool(Some(destructive)), true);
        assert_eq!(capability.risk_level, RiskLevel::High);
        assert!(capability.requires_confirmation);
    }

    #[test]
    fn tools_list_without_annotations_still_deserializes() {
        // Servers predating the annotations field must keep working.
        let tool: McpToolDef = serde_json::from_value(json!({
            "name": "lookup",
            "description": "d",
            "inputSchema": { "type": "object" }
        }))
        .expect("legacy tool definition");
        assert!(tool.annotations.is_none());
    }

    #[test]
    fn annotations_deserialize_from_the_wire_shape() {
        let tool: McpToolDef = serde_json::from_value(json!({
            "name": "lookup",
            "inputSchema": { "type": "object" },
            "annotations": {
                "title": "Look up",
                "readOnlyHint": true,
                "openWorldHint": false
            }
        }))
        .expect("annotated tool definition");
        let annotations = tool.annotations.expect("annotations parsed");
        assert_eq!(annotations.title.as_deref(), Some("Look up"));
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.open_world_hint, Some(false));
        // Absent hints stay `None` so the spec defaults can be applied.
        assert_eq!(annotations.destructive_hint, None);
    }
}
