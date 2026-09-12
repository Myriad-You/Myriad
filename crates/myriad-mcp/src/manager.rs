//! MCP registry. Each server owns its transport in a cancellable, bounded actor.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::SystemTime;
use tokio::sync::{Mutex, RwLock};

use super::actor::ServerHandle;
use super::config::{load_config, save_config, validate_config, McpServersConfig};
use super::protocol::McpToolDef;

pub struct McpManager {
    config_path: PathBuf,
    servers: RwLock<BTreeMap<String, ServerHandle>>,
    loaded_mtime: Mutex<Option<SystemTime>>,
    reload_lock: Mutex<()>,
    stopped: AtomicBool,
    reporter: crate::StatusReporter,
}

impl McpManager {
    pub async fn init(config_path: &Path) -> Arc<Self> {
        Self::init_with_reporter(config_path, Arc::new(|_, _| {})).await
    }

    pub async fn init_with_reporter(
        config_path: &Path,
        reporter: crate::StatusReporter,
    ) -> Arc<Self> {
        let manager = Arc::new(Self {
            config_path: config_path.to_path_buf(),
            servers: RwLock::new(BTreeMap::new()),
            loaded_mtime: Mutex::new(None),
            reload_lock: Mutex::new(()),
            stopped: AtomicBool::new(false),
            reporter,
        });
        if let Err(error) = manager.reload_from_disk().await {
            tracing::warn!(%error, "MCP configuration rejected at startup");
        }
        let weak = Arc::downgrade(&manager);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                let Some(manager) = weak.upgrade() else {
                    break;
                };
                if manager.stopped.load(Ordering::Acquire) {
                    break;
                }
                if let Err(error) = manager.reload_if_config_changed().await {
                    tracing::warn!(%error, "MCP configuration reload failed");
                }
            }
        });
        manager
    }

    pub async fn reload_if_config_changed(&self) -> Result<bool, String> {
        if file_mtime(&self.config_path).await == *self.loaded_mtime.lock().await {
            return Ok(false);
        }
        self.reload_from_disk().await?;
        Ok(true)
    }

    pub async fn reload_from_disk(&self) -> Result<(), String> {
        let _guard = self.reload_lock.lock().await;
        let observed_mtime = file_mtime(&self.config_path).await;
        let config = load_config(&self.config_path).await?;
        self.apply(config).await?;
        *self.loaded_mtime.lock().await = observed_mtime;
        Ok(())
    }

    pub async fn read_config(&self) -> Result<McpServersConfig, String> {
        load_config(&self.config_path).await
    }

    pub fn config_path_display(&self) -> String {
        self.config_path.display().to_string()
    }

    pub async fn replace_config(
        &self,
        config: McpServersConfig,
    ) -> Result<McpServersConfig, String> {
        let config = validate_config(config)?;
        let _guard = self.reload_lock.lock().await;
        if self.stopped.load(Ordering::Acquire) {
            return Err("MCP manager stopped".into());
        }
        save_config(&self.config_path, &config).await?;
        let observed_mtime = file_mtime(&self.config_path).await;
        self.apply(config.clone()).await?;
        *self.loaded_mtime.lock().await = observed_mtime;
        Ok(config)
    }

    async fn apply(&self, config: McpServersConfig) -> Result<(), String> {
        if self.stopped.load(Ordering::Acquire) {
            return Err("MCP manager stopped".into());
        }
        let desired: BTreeMap<_, _> = config
            .servers
            .into_iter()
            .filter(|s| s.enabled)
            .map(|s| (s.id.clone(), s))
            .collect();
        let retired = {
            let mut servers = self.servers.write().await;
            let ids: Vec<_> = servers
                .iter()
                .filter(|(id, handle)| desired.get(*id) != Some(handle.config()))
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter()
                .filter_map(|id| servers.remove(&id))
                .collect::<Vec<_>>()
        };
        // Remove routing first, then revoke all old handles, including callers
        // which obtained a handle before this reload. Unchanged servers continue.
        futures::future::join_all(retired.iter().map(ServerHandle::shutdown)).await;
        {
            let mut servers = self.servers.write().await;
            for (id, config) in desired {
                servers
                    .entry(id)
                    .or_insert_with(|| ServerHandle::spawn(config, self.reporter.clone()));
            }
        }
        Ok(())
    }

    pub async fn call_tool(
        &self,
        server_id: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let handle = self
            .servers
            .read()
            .await
            .get(server_id)
            .cloned()
            .ok_or_else(|| "MCP server not found".to_string())?;
        let text = handle.call(tool_name, arguments).await?;
        Ok(parse_mcp_tool_result(&text))
    }

    pub async fn list_tools(&self) -> Vec<(String, McpToolDef)> {
        self.servers
            .read()
            .await
            .iter()
            .flat_map(|(id, handle)| {
                handle
                    .snapshot()
                    .tools
                    .into_iter()
                    .map(|tool| (id.clone(), tool))
            })
            .collect()
    }

    pub async fn server_trusts_annotations(&self, server_id: &str) -> bool {
        self.servers
            .read()
            .await
            .get(server_id)
            .is_some_and(|s| s.config().trust_annotations)
    }

    pub async fn list_server_status(&self) -> Vec<McpServerStatus> {
        self.servers
            .read()
            .await
            .iter()
            .map(|(id, handle)| {
                let snapshot = handle.snapshot();
                McpServerStatus {
                    id: id.clone(),
                    healthy: snapshot.healthy,
                    tool_count: snapshot.tools.len(),
                    auto_restart: handle.config().auto_restart,
                    state: snapshot.state,
                }
            })
            .collect()
    }

    pub async fn shutdown_all(&self) {
        let _guard = self.reload_lock.lock().await;
        self.stopped.store(true, Ordering::Release);
        let servers = std::mem::take(&mut *self.servers.write().await);
        futures::future::join_all(servers.values().map(ServerHandle::shutdown)).await;
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct McpServerStatus {
    pub id: String,
    pub healthy: bool,
    pub tool_count: usize,
    pub auto_restart: bool,
    pub state: &'static str,
}

/// 工具结果：若为 JSON 对象/数组则解析为结构化 Value，否则保留字符串。
fn parse_mcp_tool_result(text: &str) -> serde_json::Value {
    let trimmed = text.trim();
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        if let Ok(value) = serde_json::from_str(trimmed) {
            return value;
        }
    }
    serde_json::Value::String(text.to_string())
}

async fn file_mtime(path: &Path) -> Option<SystemTime> {
    tokio::fs::metadata(path).await.ok()?.modified().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_mcp_tool_result_object() {
        let v = parse_mcp_tool_result(r#"{"ok":true,"n":1}"#);
        assert_eq!(v, json!({"ok": true, "n": 1}));
    }

    #[test]
    fn parse_mcp_tool_result_plain_string() {
        let v = parse_mcp_tool_result("hello");
        assert_eq!(v, json!("hello"));
    }
}

#[cfg(all(test, unix))]
mod lifecycle_tests {
    use super::super::test_support::server_config;
    use super::*;
    use std::time::Duration;

    async fn ready(manager: &McpManager, count: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while manager.list_tools().await.len() != count {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture servers must initialize");
    }

    #[tokio::test]
    async fn stalled_io_does_not_block_inspection_or_revocation() {
        let path = std::env::temp_dir().join(format!("myriad-mcp-{}.json", uuid::Uuid::new_v4()));
        let manager = McpManager::init(&path).await;
        let slow = server_config("a", true);
        let fast = server_config("a.b", false);
        manager
            .replace_config(McpServersConfig {
                servers: vec![slow, fast.clone()],
            })
            .await
            .unwrap();
        ready(&manager, 2).await;
        let call = {
            let manager = manager.clone();
            tokio::spawn(async move {
                manager
                    .call_tool(
                        "a",
                        "echo",
                        serde_json::json!({"data": "x".repeat(2 * 1024 * 1024)}),
                    )
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(100)).await;
        tokio::time::timeout(Duration::from_millis(500), async {
            assert_eq!(manager.list_tools().await.len(), 2);
            assert_eq!(manager.list_server_status().await.len(), 2);
            assert!(!manager.server_trusts_annotations("a").await);
            assert_eq!(
                manager
                    .call_tool("a.b", "echo", serde_json::json!({}))
                    .await
                    .unwrap(),
                serde_json::json!("ok")
            );
        })
        .await
        .expect("unrelated server and status must stay responsive");
        tokio::time::timeout(
            Duration::from_secs(3),
            manager.replace_config(McpServersConfig {
                servers: vec![fast],
            }),
        )
        .await
        .expect("revocation must interrupt the stalled exchange")
        .unwrap();
        assert!(call.await.unwrap().is_err());
        assert_eq!(manager.list_tools().await[0].0, "a.b");
        assert_eq!(
            manager
                .call_tool("a.b", "echo", serde_json::json!({}))
                .await
                .unwrap(),
            serde_json::json!("ok")
        );
        tokio::fs::write(&path, b"{broken").await.unwrap();
        assert!(manager.reload_from_disk().await.is_err());
        assert!(manager.read_config().await.is_err());
        assert_eq!(
            manager
                .call_tool("a.b", "echo", serde_json::json!({}))
                .await
                .unwrap(),
            serde_json::json!("ok")
        );
        manager.shutdown_all().await;
        assert!(manager.list_tools().await.is_empty());
        tokio::fs::remove_file(path).await.unwrap();
    }
}
