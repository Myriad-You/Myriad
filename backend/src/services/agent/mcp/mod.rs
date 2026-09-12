//! MCP (Model Context Protocol) Client 模块
//!
//! 通过 stdio 管理多个 MCP 服务器子进程，
//! 提供统一的工具发现和调用接口。
//!
//! # Security
//!
//! - Env inheritance allowlist (excludes host secret variables)
//! - Config validation (id charset, arg/env caps, max 32 servers)
//! - Stdio line / JSON-RPC message cap ([`transport::MAX_MCP_LINE_BYTES`])
//! - Concurrent live child process cap ([`transport::MAX_MCP_CHILDREN`])

pub use myriad_mcp::{config, manager, protocol, server, transport};

use std::path::Path;
use std::sync::{Arc, OnceLock};

use manager::McpManager;

static MCP_MANAGER: OnceLock<Arc<McpManager>> = OnceLock::new();

/// 初始化 MCP 管理器（在 main.rs 启动时调用）
pub async fn init_mcp(config_path: &Path) {
    let reporter: myriad_mcp::StatusReporter = Arc::new(|id, healthy| {
        let Some(notifications) =
            crate::services::agent::notifications::get_notification_manager().cloned()
        else {
            return;
        };
        tokio::spawn(async move {
            let detail = if healthy {
                "MCP server ready"
            } else {
                "MCP server unavailable"
            };
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                notifications.notify_mcp_server_status(&id, healthy, detail),
            )
            .await;
        });
    });
    let manager = McpManager::init_with_reporter(config_path, reporter).await;
    let _ = MCP_MANAGER.set(manager);
    tracing::info!("[MCP] Manager initialized");
}

/// 获取全局 MCP 管理器实例
pub fn get_mcp_manager() -> Option<&'static Arc<McpManager>> {
    MCP_MANAGER.get()
}

/// 关闭所有 MCP 子进程（graceful shutdown）
pub async fn shutdown_mcp() {
    if let Some(manager) = MCP_MANAGER.get() {
        manager.shutdown_all().await;
    }
}

/// 强制从磁盘重载 MCP 配置（管理 API）
pub async fn reload_mcp() -> Result<(), String> {
    let manager = MCP_MANAGER
        .get()
        .ok_or_else(|| "MCP manager not initialized".to_string())?;
    manager.reload_from_disk().await
}
