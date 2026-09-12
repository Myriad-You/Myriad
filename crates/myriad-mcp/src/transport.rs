//! MCP Stdio Transport
//!
//! 通过 stdin/stdout 与 MCP 服务器子进程通信。
//! 协议：每行一个 JSON-RPC 2.0 消息（line-delimited JSON）。
//!
//! - Cap each stdio line / JSON-RPC message at [`MAX_MCP_LINE_BYTES`].
//! - Cap live child processes at [`MAX_MCP_CHILDREN`] (aligned with config max).
//! - MCP children share the host process UID/namespace.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use super::config::McpServerConfig;
use super::protocol::{JsonRpcRequest, JsonRpcResponse};

/// Max bytes for a single MCP stdio line (one JSON-RPC message).
///
/// Tool results can be large, but multi-megabyte-per-line floods are abuse.
/// 4 MiB is generous for normal tool I/O and rejects unbounded read_line growth.
pub const MAX_MCP_LINE_BYTES: usize = 4 * 1024 * 1024;

/// Max concurrent live MCP child processes process-wide.
///
/// Matches `validate_config` server cap so a healthy config can start every
/// enabled server, while still bounding spawn storms / reload races.
pub const MAX_MCP_CHILDREN: usize = 32;

fn mcp_io_failed(action: &str, error: std::io::Error) -> String {
    tracing::warn!(%error, action, "MCP io failed");
    match error.kind() {
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => {
            format!("{action}: storage is not writable")
        }
        ErrorKind::BrokenPipe | ErrorKind::NotConnected | ErrorKind::UnexpectedEof => {
            format!("{action}: MCP server closed")
        }
        ErrorKind::TimedOut => format!("{action}: timed out"),
        ErrorKind::StorageFull => format!("{action}: not enough disk space"),
        _ => action.to_string(),
    }
}

fn mcp_json_failed(action: &str, error: impl std::fmt::Display) -> String {
    tracing::warn!(%error, action, "MCP json failed");
    action.to_string()
}

fn mcp_rpc_error(code: i64, message: &str) -> String {
    let keep = message.trim();
    if keep.is_empty()
        || keep.starts_with('{')
        || keep.contains("at line ")
        || keep.contains("missing field")
        || keep.len() > 160
    {
        return format!("MCP error ({code})");
    }
    format!("MCP error ({code}): {keep}")
}

/// Process-wide count of live MCP children (includes slots held during spawn).
static MCP_LIVE_CHILDREN: AtomicUsize = AtomicUsize::new(0);

/// RAII slot against [`MCP_LIVE_CHILDREN`].
struct ChildSlot(&'static AtomicUsize);

impl ChildSlot {
    fn try_acquire() -> Result<Self, String> {
        Self::from_counter(&MCP_LIVE_CHILDREN)
    }

    fn from_counter(counter: &'static AtomicUsize) -> Result<Self, String> {
        loop {
            let cur = counter.load(Ordering::Relaxed);
            if cur >= MAX_MCP_CHILDREN {
                return Err(format!(
                    "Too many concurrent MCP child processes (max {MAX_MCP_CHILDREN})"
                ));
            }
            if counter
                .compare_exchange_weak(cur, cur + 1, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return Ok(Self(counter));
            }
        }
    }
}

impl Drop for ChildSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Read one line from `reader` without exceeding `max_bytes` (excluding newline).
///
/// Returns the line **including** the trailing `\n` when present. Rejects when
/// the line body alone would exceed `max_bytes` (protects against OOM).
/// On overflow, **does not consume** the buffered bytes past the cap so callers
/// can drain with [`drain_until_newline`] if they want to continue reading.
async fn read_line_limited<R: AsyncBufReadExt + Unpin>(
    reader: &mut R,
    max_bytes: usize,
) -> Result<String, String> {
    let mut out: Vec<u8> = Vec::new();
    loop {
        let available = reader.fill_buf().await.map_err(|e| {
            tracing::warn!(error = %e, "Failed to read from MCP server");
            "Failed to read from MCP server".to_string()
        })?;
        if available.is_empty() {
            if out.is_empty() {
                return Err("MCP server closed stdout (process exited)".into());
            }
            break;
        }
        if let Some(pos) = available.iter().position(|&b| b == b'\n') {
            let take = pos + 1;
            // Body length excludes the trailing newline.
            if out.len().saturating_add(pos) > max_bytes {
                return Err(format!(
                    "MCP message exceeds max line length ({max_bytes} bytes)"
                ));
            }
            out.extend_from_slice(&available[..take]);
            reader.consume(take);
            break;
        }
        if out.len().saturating_add(available.len()) > max_bytes {
            return Err(format!(
                "MCP message exceeds max line length ({max_bytes} bytes)"
            ));
        }
        let n = available.len();
        out.extend_from_slice(available);
        reader.consume(n);
    }
    String::from_utf8(out).map_err(|e| {
        tracing::warn!(error = %e, "MCP line is not valid UTF-8");
        "MCP line is not valid UTF-8".to_string()
    })
}

/// Discard bytes until a newline (or EOF). Used after an oversized-line reject.
async fn drain_until_newline<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> Result<(), String> {
    loop {
        let available = reader.fill_buf().await.map_err(|e| {
            tracing::warn!(error = %e, "Failed to drain MCP stream");
            "Failed to drain MCP stream".to_string()
        })?;
        if available.is_empty() {
            return Ok(());
        }
        if let Some(pos) = available.iter().position(|&b| b == b'\n') {
            reader.consume(pos + 1);
            return Ok(());
        }
        let n = available.len();
        reader.consume(n);
    }
}

/// 允许透传给 MCP 子进程的环境变量。
///
/// 只放"进程要跑起来"必需的东西。任何凭据类变量都不在这里 ——
/// server 自己需要的密钥由 `mcp_servers.json` 的 `env` 显式声明，
/// 这样每个 server 拿到什么是可审计的，而不是默认继承一切。
const ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    "TZ",
    "LANG",
    "LC_ALL",
    "TERM",
    // Node/Python 运行时定位自身依赖所需
    "NODE_PATH",
    "NVM_DIR",
    "PYTHONPATH",
    "PYTHONHOME",
    // 代理设置：MCP server 常需联网，且这些不是凭据
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    // TLS / 企业自签 CA：路径指向证书束，不是密钥。
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "REQUESTS_CA_BUNDLE",
    "CURL_CA_BUNDLE",
    "NODE_EXTRA_CA_CERTS",
    "AWS_CA_BUNDLE",
    // Windows 上进程创建的基本要求
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "COMSPEC",
    "PATHEXT",
    "APPDATA",
    "LOCALAPPDATA",
    "USERPROFILE",
];

/// Stdio 双向传输通道
pub struct StdioTransport {
    child: Option<Child>,
    process_group: Option<u32>,
    stderr_task: Option<tokio::task::JoinHandle<()>>,
    usable: bool,
    stdin: Option<BufWriter<ChildStdin>>,
    stdout: BufReader<ChildStdout>,
    next_id: AtomicU64,
    /// Held for the lifetime of this transport so child counts stay accurate.
    child_slot: Option<ChildSlot>,
}

impl StdioTransport {
    /// 启动 MCP 服务器子进程
    pub async fn spawn(config: &McpServerConfig) -> Result<Self, String> {
        // Admit child slot before spawn so reload races cannot pile up.
        let child_slot = ChildSlot::try_acquire()?;

        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped()); // stderr 用于服务器日志

        // env_clear 必须在任何 cmd.env() 之前。
        // Command 默认**继承父进程的整个环境**。MCP server 是第三方代码，不该看到
        // JWT_SECRET、DATABASE_URL（含 POSTGRES_PASSWORD）、UPDATER_GATEWAY_SECRET。
        cmd.env_clear();

        // 显式白名单：只给运行时真正需要的变量。
        for key in ENV_ALLOWLIST {
            if let Ok(value) = std::env::var(key) {
                cmd.env(key, value);
            }
        }

        // 该 server 在配置里声明的环境变量（它自己的 API key 等）。
        // 放在白名单之后，允许显式覆盖 PATH 这类值。
        for (k, v) in &config.env {
            cmd.env(k, v);
        }

        cmd.kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                drop(child_slot);
                return Err(format!("Failed to spawn MCP server '{}': {}", config.id, e));
            }
        };

        let stdin = match child.stdin.take() {
            Some(s) => s,
            None => {
                drop(child_slot);
                let _ = child.kill().await;
                return Err("Failed to capture MCP server stdin".into());
            }
        };
        let stdout = match child.stdout.take() {
            Some(s) => s,
            None => {
                drop(child_slot);
                let _ = child.kill().await;
                return Err("Failed to capture MCP server stdout".into());
            }
        };

        // 后台转发 stderr 到 tracing（bounded lines）
        let stderr_task = child.stderr.take().map(|stderr| {
            let server_id = config.id.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                loop {
                    match read_line_limited(&mut reader, MAX_MCP_LINE_BYTES).await {
                        Ok(line) => {
                            let trimmed = line.trim();
                            if !trimmed.is_empty() {
                                let preview: String = trimmed.chars().take(500).collect();
                                tracing::debug!(server = %server_id, "[MCP stderr] {}", preview);
                            }
                        }
                        Err(e) if e.contains("exceeds max line length") => {
                            tracing::warn!(
                                server = %server_id,
                                "[MCP stderr] dropped oversized line; draining to newline"
                            );
                            // Drain remaining bytes of this line so subsequent lines can be logged.
                            if drain_until_newline(&mut reader).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
        });

        Ok(Self {
            process_group: child.id(),
            child: Some(child),
            stderr_task,
            usable: true,
            stdin: Some(BufWriter::new(stdin)),
            stdout: BufReader::new(stdout),
            next_id: AtomicU64::new(1),
            child_slot: Some(child_slot),
        })
    }

    /// 发送 JSON-RPC 请求并等待**匹配 id** 的响应
    ///
    /// 跳过 server 推送的 notification（无 id）以及 id 不匹配的消息，
    /// 避免把通知或乱序行当成工具结果。
    pub async fn send_request(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, String> {
        self.send_request_with_timeout(method, params, Duration::from_secs(30))
            .await
    }

    async fn send_request_with_timeout(
        &mut self,
        method: &str,
        params: Option<Value>,
        budget: Duration,
    ) -> Result<Value, String> {
        if !self.usable {
            self.terminate();
            return Err("MCP transport is closed or interrupted".into());
        }
        // Cancellation can interrupt a partial write/read. Never reuse that stream.
        self.usable = false;
        let result = tokio::time::timeout(budget, self.request_inner(method, params))
            .await
            .unwrap_or_else(|_| Err(format!("MCP request timed out for method '{method}'")));
        self.usable = result.is_ok();
        if !self.usable {
            self.terminate();
        }
        result
    }

    async fn request_inner(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = JsonRpcRequest::new(id, method, params);

        // 序列化 + 换行
        let mut payload = serde_json::to_string(&request)
            .map_err(|error| mcp_json_failed("Failed to serialize MCP request", error))?;
        // Reject oversized outbound messages before write.
        if payload.len() > MAX_MCP_LINE_BYTES {
            return Err(format!(
                "MCP request exceeds max message length ({} bytes)",
                MAX_MCP_LINE_BYTES
            ));
        }
        payload.push('\n');

        // 写入 stdin
        let stdin = self.stdin.as_mut().ok_or("MCP stdin is closed")?;
        stdin
            .write_all(payload.as_bytes())
            .await
            .map_err(|error| mcp_io_failed("Failed to write to MCP server", error))?;
        stdin
            .flush()
            .await
            .map_err(|error| mcp_io_failed("Failed to flush MCP stdin", error))?;

        loop {
            let mut line = String::new();
            self.read_response_line(&mut line).await?;

            let trimmed = line.trim();
            let value: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(e) => {
                    tracing::debug!(
                        "[MCP] skip non-JSON line: {} | raw: {}",
                        e,
                        trimmed.chars().take(120).collect::<String>()
                    );
                    continue;
                }
            };

            // Notification: has method, no result/error pair as response — skip
            if value.get("method").is_some()
                && value.get("result").is_none()
                && value.get("error").is_none()
            {
                tracing::debug!(
                    method = %value.get("method").and_then(|m| m.as_str()).unwrap_or("?"),
                    "[MCP] skip server notification while waiting for response"
                );
                continue;
            }

            // Match request id (number or string form of number)
            let resp_id = match value.get("id") {
                Some(Value::Number(n)) => n.as_u64(),
                Some(Value::String(s)) => s.parse::<u64>().ok(),
                _ => None,
            };
            if resp_id != Some(id) {
                tracing::debug!(
                    expected = id,
                    got = ?resp_id,
                    "[MCP] skip response with mismatched id"
                );
                continue;
            }

            let response: JsonRpcResponse = serde_json::from_value(value)
                .map_err(|error| mcp_json_failed("Invalid JSON-RPC response", error))?;

            if let Some(err) = response.error {
                return Err(mcp_rpc_error(err.code, &err.message));
            }

            return response
                .result
                .ok_or_else(|| "MCP response has no result".to_string());
        }
    }

    /// 发送 JSON-RPC 通知（无 id，不期望响应）
    pub async fn send_notification(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), String> {
        if !self.usable {
            self.terminate();
            return Err("MCP transport is closed or interrupted".into());
        }
        self.usable = false;
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            self.notification_inner(method, params),
        )
        .await
        .unwrap_or_else(|_| Err("MCP notification timed out".into()));
        self.usable = result.is_ok();
        if !self.usable {
            self.terminate();
        }
        result
    }

    async fn notification_inner(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(), String> {
        // 通知没有 id 字段
        let mut map = HashMap::new();
        map.insert("jsonrpc", Value::String("2.0".to_string()));
        map.insert("method", Value::String(method.to_string()));
        if let Some(p) = params {
            map.insert("params", p);
        }

        let mut payload = serde_json::to_string(&map)
            .map_err(|error| mcp_json_failed("Failed to serialize MCP notification", error))?;
        if payload.len() > MAX_MCP_LINE_BYTES {
            return Err(format!(
                "MCP notification exceeds max message length ({} bytes)",
                MAX_MCP_LINE_BYTES
            ));
        }
        payload.push('\n');

        let stdin = self.stdin.as_mut().ok_or("MCP stdin is closed")?;
        stdin
            .write_all(payload.as_bytes())
            .await
            .map_err(|error| mcp_io_failed("Failed to write MCP notification", error))?;
        stdin
            .flush()
            .await
            .map_err(|error| mcp_io_failed("Failed to flush MCP stdin", error))?;

        Ok(())
    }

    /// 从 stdout 读取一行 JSON 对象（跳过空行与非 JSON 前缀）
    ///
    /// Lines longer than [`MAX_MCP_LINE_BYTES`] are rejected (no unbounded growth).
    async fn read_response_line(&mut self, buf: &mut String) -> Result<(), String> {
        loop {
            let line = read_line_limited(&mut self.stdout, MAX_MCP_LINE_BYTES).await?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // 确保是 JSON 对象
            if trimmed.starts_with('{') {
                *buf = line;
                return Ok(());
            }

            // 非 JSON 行（可能是服务器启动消息），跳过
            tracing::debug!(
                "[MCP stdout skip] {}",
                trimmed.chars().take(100).collect::<String>()
            );
        }
    }

    pub fn is_alive(&mut self) -> bool {
        self.usable
            && self
                .child
                .as_mut()
                .is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }

    fn terminate(&mut self) {
        self.usable = false;
        #[cfg(unix)]
        if let Some(group) = self.process_group.take() {
            // The child starts its own process group, never the backend's group.
            // This cleans up cooperative descendants; OS/container isolation must
            // additionally contain children that create their own sessions.
            unsafe {
                libc::kill(-(group as i32), libc::SIGKILL);
            }
        }
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
        if let Some(task) = self.stderr_task.take() {
            task.abort();
        }
    }

    /// Revocation kills first; retain the admission permit until wait confirms
    /// exit so replacement servers cannot race unreaped old processes.
    pub async fn terminate_and_reap(&mut self) {
        self.terminate();
        if let Some(child) = self.child.as_mut() {
            if matches!(
                tokio::time::timeout(Duration::from_secs(1), child.wait()).await,
                Ok(Ok(_))
            ) {
                self.child_slot.take();
            }
        }
    }

    /// Closing stdin is the stdio shutdown signal. Discard any partial request;
    /// a server that never reads must not prevent revocation or process exit.
    pub async fn shutdown(&mut self) {
        self.stdin.take();
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            if let Some(child) = self.child.as_mut() {
                let _ = child.wait().await;
            }
        })
        .await;
        self.terminate();
        if let Some(child) = self.child.as_mut() {
            let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
        }
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        self.terminate();
        // Retain the admission slot until the direct child has actually exited.
        // Dropping an in-flight request must not create zombies or free capacity
        // while the old process is still alive.
        if let (Some(mut child), Some(slot)) = (self.child.take(), self.child_slot.take()) {
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    let _ = child.wait().await;
                    drop(slot);
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tokio::io::BufReader;

    #[test]
    fn response_id_matches_number_and_string() {
        let id = 3u64;
        let num = serde_json::json!({"jsonrpc":"2.0","id":3,"result":{}});
        let s = serde_json::json!({"jsonrpc":"2.0","id":"3","result":{}});
        let wrong = serde_json::json!({"jsonrpc":"2.0","id":4,"result":{}});
        let notif = serde_json::json!({"jsonrpc":"2.0","method":"notifications/progress"});

        let extract = |v: &Value| match v.get("id") {
            Some(Value::Number(n)) => n.as_u64(),
            Some(Value::String(s)) => s.parse::<u64>().ok(),
            _ => None,
        };
        assert_eq!(extract(&num), Some(id));
        assert_eq!(extract(&s), Some(id));
        assert_ne!(extract(&wrong), Some(id));
        assert!(notif.get("method").is_some() && notif.get("result").is_none());
    }

    #[test]
    fn mcp_rpc_error_keeps_short_phrase_and_drops_dumps() {
        assert_eq!(
            mcp_rpc_error(-32601, "Method not found"),
            "MCP error (-32601): Method not found"
        );
        assert_eq!(
            mcp_rpc_error(-32700, "{\"stack\":\"boom\"}"),
            "MCP error (-32700)"
        );
        assert_eq!(
            mcp_rpc_error(-32602, "missing field `name` at line 1 column 2"),
            "MCP error (-32602)"
        );
    }

    /// MCP server 是第三方代码。这条断言锁住"宿主凭据不进子进程环境"。
    ///
    /// 断言这些名不在 ENV_ALLOWLIST。config.env 仍会在 allowlist 之后原样写入。
    #[test]
    fn env_allowlist_excludes_host_credentials() {
        for leaked in [
            "JWT_SECRET",
            "DATABASE_URL",
            "POSTGRES_PASSWORD",
            "UPDATER_GATEWAY_SECRET",
            "UPDATE_TOKEN",
            "MYRIAD_DATA_KEY",
            "OAUTH_STATE_SECRET",
        ] {
            assert!(
                !ENV_ALLOWLIST.contains(&leaked),
                "{leaked} must never be inherited by MCP subprocesses"
            );
        }
    }

    #[test]
    fn env_allowlist_keeps_what_runtimes_need() {
        for needed in [
            "PATH",
            "HOME",
            "NODE_PATH",
            "HTTPS_PROXY",
            "SSL_CERT_FILE",
            "NODE_EXTRA_CA_CERTS",
            "REQUESTS_CA_BUNDLE",
        ] {
            assert!(
                ENV_ALLOWLIST.contains(&needed),
                "{needed} should pass through"
            );
        }
    }

    /// 白名单本身不能出现凭据形状的名字 —— 防止将来有人顺手往里加。
    ///
    /// 证书*路径*（`*_CA_*` / `SSL_CERT_*`）允许：它们是文件系统路径，不是密钥。
    #[test]
    fn env_allowlist_has_no_credential_shaped_names() {
        for name in ENV_ALLOWLIST {
            let lower = name.to_ascii_lowercase();
            let is_cert_path = lower.contains("ssl_cert")
                || lower.contains("ca_bundle")
                || lower.contains("ca_certs")
                || lower.contains("extra_ca");
            if is_cert_path {
                continue;
            }
            assert!(
                !(lower.contains("secret")
                    || lower.contains("token")
                    || lower.contains("password")
                    || lower.contains("api_key")),
                "{name} looks like a credential; it must not be allowlisted"
            );
        }
    }

    #[test]
    fn myr009_limits_are_documented_product_values() {
        assert_eq!(MAX_MCP_LINE_BYTES, 4 * 1024 * 1024);
        assert_eq!(MAX_MCP_CHILDREN, 32);
    }

    #[test]
    fn child_slot_caps_concurrent_processes() {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let mut slots = Vec::new();
        for _ in 0..MAX_MCP_CHILDREN {
            slots.push(ChildSlot::from_counter(&COUNTER).expect("slot within cap"));
        }
        assert!(
            ChildSlot::from_counter(&COUNTER).is_err(),
            "must reject when child cap is full"
        );
        drop(slots);
        let again = ChildSlot::from_counter(&COUNTER).expect("slot after release");
        drop(again);
        assert_eq!(COUNTER.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn read_line_limited_accepts_normal_line() {
        let mut reader = BufReader::new(Cursor::new(b"{\"ok\":true}\n".as_slice()));
        let line = read_line_limited(&mut reader, 1024).await.unwrap();
        assert!(line.starts_with('{'));
        assert!(line.ends_with('\n'));
    }

    #[tokio::test]
    async fn read_line_limited_rejects_oversized_line() {
        let mut body = vec![b'x'; 64];
        body.push(b'\n');
        let mut reader = BufReader::new(Cursor::new(body));
        let err = read_line_limited(&mut reader, 16).await.unwrap_err();
        assert!(
            err.contains("exceeds max line length"),
            "unexpected err: {err}"
        );
    }

    #[tokio::test]
    async fn read_line_limited_rejects_oversized_without_newline_yet() {
        // No newline: fill buffer past max while still streaming.
        let body = vec![b'y'; 100];
        let mut reader = BufReader::new(Cursor::new(body));
        let err = read_line_limited(&mut reader, 32).await.unwrap_err();
        assert!(
            err.contains("exceeds max line length"),
            "unexpected err: {err}"
        );
    }
}

#[cfg(all(test, unix))]
mod process_tests {
    use super::*;
    use crate::test_support::server_config;

    #[tokio::test]
    async fn timeout_covers_a_child_that_never_reads_stdin() {
        let mut config = server_config("blocked", false);
        config.args = vec!["-c".into(), "exec sleep 60".into()];
        let mut transport = StdioTransport::spawn(&config).await.unwrap();
        let error = tokio::time::timeout(
            Duration::from_secs(2),
            transport.send_request_with_timeout(
                "tools/call",
                Some(serde_json::json!({"data": "x".repeat(2 * 1024 * 1024)})),
                Duration::from_millis(100),
            ),
        )
        .await
        .expect("write must have a deadline")
        .unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        assert!(!transport.is_alive());
        assert!(transport
            .send_notification("notifications/initialized", None)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn cancellation_poisoning_prevents_stream_reuse() {
        let mut config = server_config("cancelled", false);
        config.args = vec!["-c".into(), "exec sleep 60".into()];
        let mut transport = StdioTransport::spawn(&config).await.unwrap();
        assert!(tokio::time::timeout(
            Duration::from_millis(50),
            transport.send_request("initialize", None)
        )
        .await
        .is_err());
        assert!(transport
            .send_request("initialize", None)
            .await
            .unwrap_err()
            .contains("interrupted"));
        tokio::time::timeout(Duration::from_secs(5), transport.shutdown())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn spawn_uses_a_separate_process_group_and_drop_terminates_it() {
        let mut config = server_config("group", false);
        config.args = vec![
            "-c".into(),
            "sleep 60 & child=$!; printf '%s\\n' \"$child\"; wait".into(),
        ];
        let mut transport = StdioTransport::spawn(&config).await.unwrap();
        let descendant: i32 = tokio::time::timeout(
            Duration::from_secs(2),
            read_line_limited(&mut transport.stdout, 32),
        )
        .await
        .unwrap()
        .unwrap()
        .trim()
        .parse()
        .unwrap();
        let pid = transport.child.as_ref().unwrap().id().unwrap() as i32;
        assert_eq!(unsafe { libc::getpgid(pid) }, pid);
        assert_ne!(unsafe { libc::getpgrp() }, pid);
        assert_eq!(unsafe { libc::getpgid(descendant) }, pid);
        drop(transport);
        tokio::time::timeout(Duration::from_secs(3), async {
            while unsafe { libc::kill(-pid, 0) } == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("process group must be reclaimed");
    }
}
