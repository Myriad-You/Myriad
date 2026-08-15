//! MCP 服务器配置
//!
//! 从 `data/agent/mcp_servers.json` 加载 / 保存 MCP 服务器定义。

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// MCP 服务器配置集合
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct McpServersConfig {
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

/// 单个 MCP 服务器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// 服务器 ID（用于日志和引用）
    pub id: String,
    /// 启动命令（如 "npx", "python", "node"）
    pub command: String,
    /// 命令参数
    #[serde(default)]
    pub args: Vec<String>,
    /// 额外环境变量
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 崩溃后自动重启
    #[serde(default = "default_true")]
    pub auto_restart: bool,
    /// 最大重启次数
    #[serde(default = "default_max_restarts")]
    pub max_restart_attempts: u32,
    /// 采信该服务器工具自述的 annotations（`readOnlyHint` 等）来判定风险等级。
    ///
    /// **默认关闭。** annotations 由服务器进程自己提供，MCP 规范明确要求客户端
    /// 不要基于不可信服务器的 annotations 做安全决策——否则等于让外部进程自行
    /// 声明「我无害」来关掉确认框。为某个服务器打开这个开关，是运维对「它的自述
    /// 可信」的显式表态。关闭时该服务器的所有工具一律按高风险处理。
    #[serde(default)]
    pub trust_annotations: bool,
}

fn default_true() -> bool {
    true
}
fn default_max_restarts() -> u32 {
    3
}

/// 从文件加载配置
pub async fn load_config(path: &Path) -> McpServersConfig {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
            tracing::warn!("[MCP] Failed to parse config: {}", e);
            McpServersConfig::default()
        }),
        Err(_) => {
            tracing::debug!("[MCP] Config file not found: {}", path.display());
            McpServersConfig::default()
        }
    }
}

/// Validate and normalize config before persistence.
///
/// - Trims ids/commands; rejects empty or duplicate ids
/// - Id charset: `[A-Za-z0-9._-]` (max 64)
/// - Caps restart attempts at 50
/// - Drops empty env keys
pub fn validate_config(mut config: McpServersConfig) -> Result<McpServersConfig, String> {
    let mut seen = HashSet::new();
    for server in &mut config.servers {
        server.id = server.id.trim().to_string();
        server.command = server.command.trim().to_string();
        if server.id.is_empty() {
            return Err("server id must not be empty".into());
        }
        if server.id.len() > 64 {
            return Err(format!("server id '{}' is too long (max 64)", server.id));
        }
        if !server
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            return Err(format!(
                "server id '{}' may only contain A–Z, a–z, 0–9, '.', '_', '-'",
                server.id
            ));
        }
        if !seen.insert(server.id.clone()) {
            return Err(format!("duplicate server id '{}'", server.id));
        }
        if server.command.is_empty() {
            return Err(format!("server '{}': command must not be empty", server.id));
        }
        if server.command.len() > 512 {
            return Err(format!("server '{}': command is too long", server.id));
        }
        server.args = server
            .args
            .iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .collect();
        if server.args.len() > 64 {
            return Err(format!("server '{}': too many args (max 64)", server.id));
        }
        for a in &server.args {
            if a.len() > 1024 {
                return Err(format!("server '{}': arg is too long", server.id));
            }
        }
        // Normalize env: drop empty keys; cap size.
        let mut env = HashMap::new();
        for (k, v) in server.env.drain() {
            let key = k.trim().to_string();
            if key.is_empty() {
                continue;
            }
            if key.len() > 128 || v.len() > 8192 {
                return Err(format!(
                    "server '{}': env entry too large (key or value)",
                    server.id
                ));
            }
            if env.len() >= 64 {
                return Err(format!("server '{}': too many env entries (max 64)", server.id));
            }
            env.insert(key, v);
        }
        server.env = env;
        if server.max_restart_attempts > 50 {
            server.max_restart_attempts = 50;
        }
    }
    // Keep in lockstep with `transport::MAX_MCP_CHILDREN` (MYR-009).
    if config.servers.len() > 32 {
        return Err("too many MCP servers (max 32)".into());
    }
    Ok(config)
}

/// Atomically write config JSON (pretty) to `path`.
pub async fn save_config(path: &Path, config: &McpServersConfig) -> Result<(), String> {
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("serialize mcp config: {e}"))?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|e| format!("create mcp config dir: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    tokio::fs::write(&tmp, json.as_bytes())
        .await
        .map_err(|e| format!("write mcp config temp: {e}"))?;
    tokio::fs::rename(&tmp, path)
        .await
        .map_err(|e| format!("replace mcp config: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicate_and_bad_id() {
        let cfg = McpServersConfig {
            servers: vec![
                McpServerConfig {
                    id: "a".into(),
                    command: "npx".into(),
                    args: vec![],
                    env: HashMap::new(),
                    enabled: true,
                    auto_restart: true,
                    max_restart_attempts: 3,
                    trust_annotations: false,
                },
                McpServerConfig {
                    id: "a".into(),
                    command: "npx".into(),
                    args: vec![],
                    env: HashMap::new(),
                    enabled: true,
                    auto_restart: true,
                    max_restart_attempts: 3,
                    trust_annotations: false,
                },
            ],
        };
        assert!(validate_config(cfg).is_err());

        let bad = McpServersConfig {
            servers: vec![McpServerConfig {
                id: "has space".into(),
                command: "npx".into(),
                args: vec![],
                env: HashMap::new(),
                enabled: true,
                auto_restart: true,
                max_restart_attempts: 3,
                trust_annotations: false,
            }],
        };
        assert!(validate_config(bad).is_err());
    }

    #[test]
    fn accepts_normal_server() {
        let cfg = McpServersConfig {
            servers: vec![McpServerConfig {
                id: "github".into(),
                command: "npx".into(),
                args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
                env: HashMap::from([("GITHUB_TOKEN".into(), "x".into())]),
                enabled: true,
                auto_restart: true,
                max_restart_attempts: 3,
                trust_annotations: false,
            }],
        };
        assert!(validate_config(cfg).is_ok());
    }
}
