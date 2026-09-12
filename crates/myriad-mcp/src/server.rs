//! MCP Server 生命周期管理
//!
//! 管理单个 MCP 服务器子进程的完整生命周期：
//! spawn → initialize handshake → tools/list → ready
//! 重启预算、取消和调用排队由 actor 管理。

use super::config::McpServerConfig;
use super::protocol::{McpInitializeResult, McpToolCallResult, McpToolDef};
use super::transport::StdioTransport;

/// MCP 服务器状态
#[derive(Debug, Clone, PartialEq)]
pub enum ServerState {
    Stopped,
    Starting,
    Ready,
    Failed(String),
}

/// 单个 MCP 服务器实例
pub struct McpServer {
    pub config: McpServerConfig,
    state: ServerState,
    transport: Option<StdioTransport>,
    tools: Vec<McpToolDef>,
}

impl McpServer {
    pub fn new(config: McpServerConfig) -> Self {
        Self {
            config,
            state: ServerState::Stopped,
            transport: None,
            tools: Vec::new(),
        }
    }

    /// 启动服务器并完成 MCP 初始化握手
    pub async fn start(&mut self) -> Result<(), String> {
        if self.state == ServerState::Ready {
            return Ok(());
        }

        self.state = ServerState::Starting;
        tracing::info!(server = %self.config.id, "Starting MCP server");

        // 1. Spawn 子进程
        let transport = StdioTransport::spawn(&self.config).await.inspect_err(|e| {
            self.state = ServerState::Failed(e.clone());
        })?;
        self.transport = Some(transport);

        // 2. Initialize 握手
        self.do_initialize().await.inspect_err(|e| {
            self.state = ServerState::Failed(e.clone());
        })?;

        // 3. 发现工具列表
        self.do_tools_list().await.inspect_err(|e| {
            self.state = ServerState::Failed(e.clone());
        })?;

        self.state = ServerState::Ready;
        tracing::info!(
            server = %self.config.id,
            tools = self.tools.len(),
            "MCP server ready"
        );

        Ok(())
    }

    /// MCP initialize 握手
    async fn do_initialize(&mut self) -> Result<(), String> {
        let transport = self.transport.as_mut().ok_or("No transport")?;

        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "myriad-agent",
                "version": "1.0.0"
            }
        });

        let result = transport.send_request("initialize", Some(params)).await?;

        let init_result: McpInitializeResult = serde_json::from_value(result).map_err(|error| {
            tracing::warn!(%error, "Invalid MCP initialize response");
            "Invalid MCP initialize response".to_string()
        })?;

        tracing::debug!(
            server = %self.config.id,
            protocol = %init_result.protocol_version,
            "MCP initialize OK"
        );

        // 发送 initialized 通知
        transport
            .send_notification("notifications/initialized", None)
            .await?;

        Ok(())
    }

    /// 发现服务器工具列表
    async fn do_tools_list(&mut self) -> Result<(), String> {
        let transport = self.transport.as_mut().ok_or("No transport")?;

        let result = transport.send_request("tools/list", None).await?;

        #[derive(serde::Deserialize)]
        struct ToolsListResult {
            tools: Vec<McpToolDef>,
        }

        let tools_result: ToolsListResult = serde_json::from_value(result).map_err(|error| {
            tracing::warn!(%error, "Invalid MCP tools/list response");
            "Invalid MCP tools/list response".to_string()
        })?;

        tracing::debug!(
            server = %self.config.id,
            count = tools_result.tools.len(),
            tools = ?tools_result.tools.iter().map(|t| &t.name).collect::<Vec<_>>(),
            "Discovered MCP tools"
        );

        self.tools = tools_result.tools;
        Ok(())
    }

    /// 调用工具
    pub async fn call_tool(
        &mut self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<String, String> {
        if self.state != ServerState::Ready {
            let state = match &self.state {
                ServerState::Stopped => "stopped",
                ServerState::Starting => "starting",
                ServerState::Ready => "ready",
                ServerState::Failed(_) => "failed",
            };
            return Err(format!(
                "MCP server '{}' is not ready ({state})",
                self.config.id
            ));
        }

        let transport = self.transport.as_mut().ok_or("No transport")?;

        let params = serde_json::json!({
            "name": tool_name,
            "arguments": arguments
        });

        let result = transport.send_request("tools/call", Some(params)).await?;

        let call_result: McpToolCallResult = serde_json::from_value(result).map_err(|error| {
            tracing::warn!(%error, "Invalid MCP tools/call response");
            "Invalid MCP tools/call response".to_string()
        })?;

        if call_result.is_error {
            let error_text = call_result
                .content
                .iter()
                .filter_map(|c| c.text.as_deref())
                .collect::<Vec<_>>()
                .join("\n");
            let keep = error_text.trim();
            if keep.is_empty()
                || keep.starts_with('{')
                || keep.contains("at line ")
                || keep.len() > 160
            {
                return Err("MCP tool failed".to_string());
            }
            return Err(format!("MCP tool failed: {keep}"));
        }

        // 提取文本内容
        let text = call_result
            .content
            .iter()
            .filter_map(|c| c.text.as_deref())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(text)
    }

    /// 获取工具列表
    pub fn tools(&self) -> &[McpToolDef] {
        &self.tools
    }

    /// 健康检查（检查子进程是否存活）
    pub fn is_healthy(&mut self) -> bool {
        if self.state != ServerState::Ready {
            return false;
        }
        match self.transport.as_mut() {
            Some(t) => t.is_alive(),
            None => false,
        }
    }

    pub async fn terminate(&mut self) {
        if let Some(transport) = self.transport.as_mut() {
            transport.terminate_and_reap().await;
        }
        self.transport = None;
        self.tools.clear();
        self.state = ServerState::Stopped;
    }

    /// 优雅关闭
    pub async fn shutdown(&mut self) {
        if let Some(ref mut transport) = self.transport {
            transport.shutdown().await;
        }
        self.transport = None;
        self.tools.clear();
        self.state = ServerState::Stopped;
    }
}
