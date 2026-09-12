//! One owner per MCP transport. Control/inspection never waits on tool I/O.
use std::{sync::Arc, time::Duration};

use serde_json::Value;
use tokio::sync::{mpsc, oneshot, watch, Mutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

use super::{config::McpServerConfig, protocol::McpToolDef, server::McpServer};

const CALL_BUDGET: Duration = Duration::from_secs(35);
const START_BUDGET: Duration = Duration::from_secs(35);
const RETRY_DELAY: Duration = Duration::from_secs(60);
static STARTS: Semaphore = Semaphore::const_new(4);

#[derive(Clone, Default)]
pub(super) struct Snapshot {
    pub healthy: bool,
    pub tools: Vec<McpToolDef>,
    pub state: &'static str,
}

struct Call {
    tool: String,
    arguments: Value,
    deadline: Instant,
    response: oneshot::Sender<Result<String, String>>,
    _bytes: OwnedSemaphorePermit,
}

struct Inner {
    config: McpServerConfig,
    calls: mpsc::Sender<Call>,
    bytes: Arc<Semaphore>,
    snapshot: watch::Receiver<Snapshot>,
    stop: watch::Sender<bool>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

#[derive(Clone)]
pub(super) struct ServerHandle(Arc<Inner>);

impl ServerHandle {
    pub fn spawn(config: McpServerConfig, reporter: crate::StatusReporter) -> Self {
        let (calls, receiver) = mpsc::channel(16);
        let (snapshot_tx, snapshot) = watch::channel(Snapshot {
            state: "starting",
            ..Default::default()
        });
        let (stop, stopped) = watch::channel(false);
        let task = tokio::spawn(run(
            config.clone(),
            receiver,
            snapshot_tx,
            stopped,
            reporter,
        ));
        Self(Arc::new(Inner {
            config,
            calls,
            bytes: Arc::new(Semaphore::new(super::transport::MAX_MCP_LINE_BYTES)),
            snapshot,
            stop,
            task: Mutex::new(Some(task)),
        }))
    }

    pub fn config(&self) -> &McpServerConfig {
        &self.0.config
    }

    pub fn snapshot(&self) -> Snapshot {
        if self.0.snapshot.has_changed().is_err() {
            return Snapshot {
                state: "stopped",
                ..Default::default()
            };
        }
        self.0.snapshot.borrow().clone()
    }

    pub async fn call(&self, tool: &str, arguments: Value) -> Result<String, String> {
        if *self.0.stop.borrow() {
            return Err("MCP server stopped".into());
        }
        let size = argument_size(&arguments)?;
        let bytes = self
            .0
            .bytes
            .clone()
            .try_acquire_many_owned(size.max(1) as u32)
            .map_err(|_| "MCP pending request byte budget exhausted".to_string())?;
        let deadline = Instant::now() + CALL_BUDGET;
        let (response, result) = oneshot::channel();
        // Never accumulate unbounded tasks waiting to enter a slow server.
        self.0
            .calls
            .try_send(Call {
                tool: tool.into(),
                arguments,
                deadline,
                response,
                _bytes: bytes,
            })
            .map_err(|_| "MCP server busy or stopped".to_string())?;
        tokio::time::timeout_at(deadline, result)
            .await
            .map_err(|_| "MCP call timed out (including queue wait)".to_string())?
            .map_err(|_| "MCP server stopped".to_string())?
    }

    pub async fn shutdown(&self) {
        let _ = self.0.stop.send(true);
        if let Some(mut task) = self.0.task.lock().await.take() {
            if tokio::time::timeout(Duration::from_secs(2), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = task.await;
            }
        }
    }
}

async fn run(
    config: McpServerConfig,
    mut calls: mpsc::Receiver<Call>,
    snapshot: watch::Sender<Snapshot>,
    mut stop: watch::Receiver<bool>,
    reporter: crate::StatusReporter,
) {
    let mut server = McpServer::new(config.clone());
    let mut retry_count = 0;
    let mut retry_at = Instant::now();
    let mut first_start = true;
    let mut maintenance = tokio::time::interval(Duration::from_secs(1));
    maintenance.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if *stop.borrow() {
            break;
        }
        if !server.is_healthy()
            && Instant::now() >= retry_at
            && (first_start || (config.auto_restart && retry_count < config.max_restart_attempts))
        {
            if !first_start {
                retry_count += 1;
            }
            first_start = false;
            // Dropping the previous transport also terminates interrupted writes.
            server = McpServer::new(config.clone());
            snapshot.send_replace(Snapshot {
                state: "starting",
                ..Default::default()
            });
            let result = tokio::select! {
                biased;
                _ = stop.changed() => break,
                result = async {
                    let _permit = STARTS.acquire().await.map_err(|_| "MCP startup stopped".to_string())?;
                    tokio::time::timeout(START_BUDGET, server.start()).await
                        .map_err(|_| "MCP initialization timed out".to_string())?
                } => result,
            };
            let healthy = result.is_ok();
            if !healthy {
                server = McpServer::new(config.clone());
            }
            retry_at = Instant::now() + RETRY_DELAY;
            snapshot.send_replace(Snapshot {
                healthy,
                tools: if healthy {
                    server.tools().to_vec()
                } else {
                    vec![]
                },
                state: if healthy { "ready" } else { "failed" },
            });
            reporter(config.id.clone(), healthy);
            if let Err(error) = result {
                tracing::warn!(server = %config.id, %error, "MCP initialization failed");
            }
        }
        tokio::select! {
            biased;
            _ = stop.changed() => break,
            call = calls.recv() => {
                let Some(mut call) = call else { break; };
                if call.response.is_closed() { continue; }
                if call.deadline <= Instant::now() {
                    let _ = call.response.send(Err("MCP call expired in queue".into()));
                    continue;
                }
                if !server.is_healthy() {
                    let _ = call.response.send(Err("MCP server is not ready".into()));
                    continue;
                }
                if !server.tools().iter().any(|t| t.name == call.tool) {
                    let _ = call.response.send(Err("MCP tool is no longer available".into()));
                    continue;
                }
                let result = tokio::select! {
                    biased;
                    _ = stop.changed() => break,
                    _ = call.response.closed() => Err("MCP caller cancelled".into()),
                    result = tokio::time::timeout_at(call.deadline, server.call_tool(&call.tool, call.arguments)) =>
                        result.unwrap_or_else(|_| Err("MCP call timed out".into())),
                };
                if result.is_err() {
                    // A cancelled exchange cannot safely be resumed, and the
                    // external program may still be performing side effects.
                    server = McpServer::new(config.clone());
                    retry_at = Instant::now() + RETRY_DELAY;
                    snapshot.send_replace(Snapshot { state: "failed", ..Default::default() });
                } else {
                    retry_count = 0;
                }
                let _ = call.response.send(result);
            },
            _ = maintenance.tick() => {
                if !server.is_healthy() {
                    snapshot.send_modify(|s| { s.healthy = false; s.tools.clear(); s.state = "failed"; });
                }
            },
        }
    }
    // Immediate transport destruction: never wait for an untrusted process to
    // accept a shutdown notification before revoking its execution.
    server.terminate().await;
    drop(server);
    snapshot.send_replace(Snapshot {
        state: "stopped",
        ..Default::default()
    });
}

// Count encoded bytes without allocating a second copy of a potentially large
// argument. The permit covers queued AND executing calls for this server.
fn argument_size(value: &Value) -> Result<usize, String> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            if self.0 > super::transport::MAX_MCP_LINE_BYTES - 1024 {
                return Err(std::io::Error::other("MCP arguments too large"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value)
        .map_err(|_| "MCP arguments too large".to_string())?;
    Ok(counter.0)
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    #[test]
    fn budget_counts_json_escaping_and_unicode() {
        let value = serde_json::json!({"message": "你好\n\t"});
        assert_eq!(
            argument_size(&value).unwrap(),
            serde_json::to_vec(&value).unwrap().len()
        );
        assert!(argument_size(&Value::String(
            "x".repeat(super::super::transport::MAX_MCP_LINE_BYTES)
        ))
        .is_err());
    }
}
