//! MCP JSON-RPC 2.0 协议类型
//!
//! 实现 Model Context Protocol 的最小子集：
//! - JSON-RPC 2.0 请求/响应
//! - MCP initialize, tools/list, tools/call

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 请求
#[derive(Debug, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: u64, method: &str, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        }
    }
}

/// JSON-RPC 2.0 响应
///
/// `id` 允许 number / string（部分 MCP server 用字符串 id），反序列化时尽量宽松。
#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse {
    #[allow(dead_code)]
    pub jsonrpc: String,
    #[allow(dead_code)]
    #[serde(default)]
    pub id: Option<Value>,
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 错误
#[derive(Debug, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[allow(dead_code)]
    pub data: Option<Value>,
}

/// MCP Tool 定义（从 tools/list 返回）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_schema")]
    pub input_schema: Value,
    /// 工具自述的行为提示；老 server 不返回时为 `None`
    #[serde(default)]
    pub annotations: Option<McpToolAnnotations>,
}

/// MCP `ToolAnnotations`：工具对自身行为的自述。
///
/// 全部字段用 `Option` 而不是带默认值的 `bool`，是为了区分「服务器明确声明为
/// false」和「服务器没说」——两者在风险判定上不同：规范给 `destructiveHint` 的
/// 默认值是 `true`，即沉默应当按「可能有破坏性」处理。
///
/// 规范同时明确这些只是**提示**，客户端不应基于不可信服务器的 annotations 做
/// 安全决策。因此只有配置里显式标记 `trust_annotations` 的服务器，其自述才会
/// 被用来降低风险等级。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolAnnotations {
    /// 人类可读标题（仅展示用）
    #[serde(default)]
    pub title: Option<String>,
    /// 不修改任何环境状态。规范默认 false
    #[serde(default)]
    pub read_only_hint: Option<bool>,
    /// 可能执行破坏性更新。**规范默认 true**，仅在 `read_only_hint` 为假时有意义
    #[serde(default)]
    pub destructive_hint: Option<bool>,
    /// 相同参数重复调用没有额外副作用。规范默认 false
    #[serde(default)]
    pub idempotent_hint: Option<bool>,
    /// 与外部实体交互。规范默认 true
    #[serde(default)]
    pub open_world_hint: Option<bool>,
}

fn default_schema() -> Value {
    serde_json::json!({"type": "object"})
}

/// MCP initialize 结果
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpInitializeResult {
    pub protocol_version: String,
    #[allow(dead_code)]
    pub capabilities: Option<Value>,
    #[allow(dead_code)]
    pub server_info: Option<Value>,
}

/// MCP tools/call 的 content 项
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct McpToolResultContent {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub data: Option<String>,
}

/// MCP tools/call 结果
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolCallResult {
    pub content: Vec<McpToolResultContent>,
    #[serde(default)]
    pub is_error: bool,
}
