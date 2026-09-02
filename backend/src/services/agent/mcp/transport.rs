//! MCP Stdio Transport
//!
//! 通过 stdin/stdout 与 MCP 服务器子进程通信。
//! 协议：每行一个 JSON-RPC 2.0 消息（line-delimited JSON）。
//!
//! # MYR-009 (first practical cut)
//!
//! - Cap each stdio line / JSON-RPC message at [`MAX_MCP_LINE_BYTES`].
//! - Cap live child processes at [`MAX_MCP_CHILDREN`] (aligned with config max).
//! - Residual: MCP children still share the host process UID/namespace. Full OS
//!   sandbox / seccomp / landlock is intentionally future work (multi-week).

use std::collections::HashMap;
use std::io::ErrorKind;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

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
struct ChildSlot;

impl ChildSlot {
    fn try_acquire() -> Result<Self, String> {
        loop {
            let cur = MCP_LIVE_CHILDREN.load(Ordering::Relaxed);
            if cur >= MAX_MCP_CHILDREN {
                return Err(format!(
                    "Too many concurrent MCP child processes (max {MAX_MCP_CHILDREN})"
                ));
            }
            if MCP_LIVE_CHILDREN
                .compare_exchange_weak(cur, cur + 1, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return Ok(Self);
            }
        }
    }
}

impl Drop for ChildSlot {
    fn drop(&mut self) {
        MCP_LIVE_CHILDREN.fetch_sub(1, Ordering::AcqRel);
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
    // 缺了这些时 Node/Python/curl 在企业代理环境会 TLS handshake 失败。
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
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: AtomicU64,
    /// Held for the lifetime of this transport so child counts stay accurate.
    _child_slot: ChildSlot,
}

impl StdioTransport {
    /// 启动 MCP 服务器子进程
    pub async fn spawn(config: &McpServerConfig) -> Result<Self, String> {
        // MYR-009: admit child slot before spawn so reload races cannot pile up.
        let child_slot = ChildSlot::try_acquire()?;

        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped()); // stderr 用于服务器日志

        // env_clear 必须在任何 cmd.env() 之前。
        //
        // Command 默认**继承父进程的整个环境**。之前这里只是"再设一遍"
        // PATH/HOME，看着像白名单，实际上每个 MCP server 子进程都拿到了
        // JWT_SECRET、DATABASE_URL（含 POSTGRES_PASSWORD）、
        // UPDATER_GATEWAY_SECRET —— 一个 `cat /proc/self/environ` 全都有。
        //
        // MCP server 是第三方代码（npx 拉取的包、社区实现），不该看到宿主凭据。
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

        // 后台转发 stderr 到 tracing（bounded lines — MYR-009）
        if let Some(stderr) = child.stderr.take() {
            let server_id = config.id.clone();
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr);
                loop {
                    match read_line_limited(&mut reader, MAX_MCP_LINE_BYTES).await {
                        Ok(line) => {
                            let trimmed = line.trim();
                            if !trimmed.is_empty() {
                                let preview = &trimmed[..trimmed.len().min(500)];
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
            });
        }

        Ok(Self {
            child,
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
            next_id: AtomicU64::new(1),
            _child_slot: child_slot,
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
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = JsonRpcRequest::new(id, method, params);

        // 序列化 + 换行
        let mut payload = serde_json::to_string(&request)
            .map_err(|error| mcp_json_failed("Failed to serialize MCP request", error))?;
        // MYR-009: reject oversized outbound messages before write
        if payload.len() > MAX_MCP_LINE_BYTES {
            return Err(format!(
                "MCP request exceeds max message length ({} bytes)",
                MAX_MCP_LINE_BYTES
            ));
        }
        payload.push('\n');

        // 写入 stdin
        self.stdin
            .write_all(payload.as_bytes())
            .await
            .map_err(|error| mcp_io_failed("Failed to write to MCP server", error))?;
        self.stdin
            .flush()
            .await
            .map_err(|error| mcp_io_failed("Failed to flush MCP stdin", error))?;

        // 在总超时内读到匹配 id 的响应
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(format!("MCP server timeout (30s) for method '{}'", method));
            }

            let mut line = String::new();
            let read_result =
                tokio::time::timeout(remaining, self.read_response_line(&mut line)).await;
            match read_result {
                Err(_) => {
                    return Err(format!("MCP server timeout (30s) for method '{}'", method));
                }
                Ok(Err(e)) => return Err(e),
                Ok(Ok(())) => {}
            }

            let trimmed = line.trim();
            let value: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(e) => {
                    tracing::debug!(
                        "[MCP] skip non-JSON line: {} | raw: {}",
                        e,
                        &trimmed[..trimmed.len().min(120)]
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

        self.stdin
            .write_all(payload.as_bytes())
            .await
            .map_err(|error| mcp_io_failed("Failed to write MCP notification", error))?;
        self.stdin
            .flush()
            .await
            .map_err(|error| mcp_io_failed("Failed to flush MCP stdin", error))?;

        Ok(())
    }

    /// 从 stdout 读取一行 JSON 对象（跳过空行与非 JSON 前缀）
    ///
    /// MYR-009: lines longer than [`MAX_MCP_LINE_BYTES`] are rejected (no unbounded growth).
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
            tracing::debug!("[MCP stdout skip] {}", &trimmed[..trimmed.len().min(100)]);
        }
    }

    /// 检查子进程是否存活
    pub fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(None) => true,     // 仍在运行
            Ok(Some(_)) => false, // 已退出
            Err(_) => false,
        }
    }

    /// 优雅关闭
    pub async fn shutdown(&mut self) {
        // 尝试发送 shutdown 通知
        let _ = self
            .send_notification("notifications/cancelled", None)
            .await;
        // 等待 2 秒后强制 kill
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), self.child.wait()).await;
        let _ = self.child.kill().await;
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        // 尽力 kill — 非 async，不能等待
        let _ = self.child.start_kill();
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
    /// 修复前 `Command` 默认继承整个父环境，这些变量全都泄给了每个 MCP server。
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
        let mut slots = Vec::new();
        for _ in 0..MAX_MCP_CHILDREN {
            slots.push(ChildSlot::try_acquire().expect("slot within cap"));
        }
        assert!(
            ChildSlot::try_acquire().is_err(),
            "must reject when child cap is full"
        );
        drop(slots);
        let again = ChildSlot::try_acquire().expect("slot after release");
        drop(again);
        assert_eq!(MCP_LIVE_CHILDREN.load(Ordering::Relaxed), 0);
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
