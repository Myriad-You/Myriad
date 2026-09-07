//! 联邦体积上限的单一事实源
//!
//! # 为什么要集中
//!
//! 这些数字此前散在 5 个文件里（`channel.rs`、`room.rs`、`content.rs`、
//! `file_transfer.rs`、`main.rs` 的路由层），彼此靠注释「与 X 对齐」维持一致。
//! 改一个忘一个的结果不是编译错误，而是**运行期的静默不匹配**：本地允许发出
//! 的消息，对端 inbox 拒收。
//!
//! # 这条链
//!
//! ```text
//! 用户提交 payload  ──►  活动 = payload + 信封
//! │
//! ├─► 本地路由 DefaultBodyLimit   （我们控制）
//! └─► 对端 inbox DefaultBodyLimit （对方控制）
//! ```
//!
//! 实际可用体积 = 链上的**最小值**。所以 [`INBOX_BODY_LIMIT`] 必须显著大于
//! [`MESSAGE_PAYLOAD_LIMIT`]，否则本实例发得出、收不进自己的消息。
//!
//! 注意跨实例时对端可能是旧版本或别的实现，它的 inbox 上限我们无从得知 ——
//! 放宽本端只在双方都升级后才完全生效。
//!
//! # 内存与解析上界
//!
//! inbox 是**远端可达**的表面。一个请求最坏会同时持有：原始字节
//! （≤ [`INBOX_BODY_LIMIT`]）+ 解析后的 `serde_json::Value`（结构化开销通常是
//! 原始大小的 2–3 倍）。[`INBOX_PARSE_CONCURRENCY`] 限制同时持有完整解析树的
//! 请求数；[`validate_inbox_json_budget`] 在构造 `Value` 前拒绝过深、结构项过多或
//! 单字符串过大的 JSON。
//!
//! 大文件必须走分块传输端点，不能借公共 inbox 的 JSON 上限绕过资源预算。
//!
//! 缓解措施：
//! 1. `inbox::verify_preparse_gate` 在解析 JSON **之前**先校验签名头、
//!    Date 新鲜度与 Digest（对原始字节逐字节比对），攻击者要让我们开始解析就得先
//!    算出正确的 SHA-256，不能靠一坨随机字节撑爆内存。
//! 2. [`INBOX_INFLIGHT_RAW_BUDGET`] 限制**并发**缓冲的原始 body 总量；预算耗尽
//!    返回 429。
//! 3. [`INBOX_PARSE_CONCURRENCY`] 限制完整 JSON 树的并发存活数；没有排队持有大
//!    body，预算耗尽直接返回 429。

use std::sync::atomic::{AtomicUsize, Ordering};

/// 单条消息载荷上限（channel 与 room 共用）。
///
/// 按 `payload.to_string().len()` 度量，覆盖小文件内联 base64 与 Tapp 安装包
/// 分享；更大的附件走 [`file_transfer`](crate::federation::file_transfer) 分块。
///
/// 公共 inbox 只承载活动与小型内联数据；大文件走分块传输。4 MiB 仍为普通消息、
/// 加密信封和小图保留余量，同时不再让单条公共 JSON 承担媒体传输职责。
pub const MESSAGE_PAYLOAD_LIMIT: usize = 4 * 1024 * 1024;

/// Live message payload cap (default or memory-saver).
#[inline]
pub fn message_payload_limit() -> usize {
    crate::services::memory_profile::message_payload_limit()
}

/// inbox 请求体上限（`/inbox` 与 `/users/{u}/inbox`）。
///
/// 必须容纳 [`MESSAGE_PAYLOAD_LIMIT`] 加上活动信封、JSON 转义膨胀与
/// base64 分块的 4/3 放大。
///
/// 8 MiB 是独立于全局 body limit 的远端硬上限。它可容纳 4 MiB payload、加密/
/// base64 膨胀和活动信封；更大内容必须走分块传输。
/// Active process value: [`inbox_body_limit`].
pub const INBOX_BODY_LIMIT: usize = 8 * 1024 * 1024;

/// Live inbox body cap (default or memory-saver).
#[inline]
pub fn inbox_body_limit() -> usize {
    crate::services::memory_profile::inbox_body_limit()
}

/// 已认证用户提交内容的路由上限（发布、房间/频道消息、媒体上传）。
///
/// 比 inbox 略宽：这些请求来自已登录的本地用户，不是任意远端。
/// 本地已认证写路径保留额外 16 MiB 余量；公开 inbox 不继承该宽限。
/// Active: [`authenticated_body_limit`].
pub const AUTHENTICATED_BODY_LIMIT: usize = INBOX_BODY_LIMIT + 16 * 1024 * 1024;

/// Live authenticated body cap (default or memory-saver).
#[inline]
pub fn authenticated_body_limit() -> usize {
    crate::services::memory_profile::authenticated_body_limit()
}

/// 小型联邦控制端点的请求体上限（信任策略、房间创建、邀请等）。
///
/// 这些端点的 body 只有几百字节到几 KiB。放宽到 1 MiB 足够容纳异常大的
/// 名称/描述，同时避免一个只需要 JSON 小对象的端点可以缓冲整个路由上限。
pub const SMALL_CONTROL_BODY_LIMIT: usize = 256 * 1024;

/// 同时持有完整 inbox JSON 树的最大请求数。
pub const INBOX_PARSE_CONCURRENCY: usize = 4;

/// inbox JSON 最大嵌套层数。ActivityPub/MFP 合法信封通常远低于此值。
pub const INBOX_JSON_MAX_DEPTH: usize = 32;

/// 所有数组/对象结构项的全局预算（逗号、键值分隔符和容器起点）。
pub const INBOX_JSON_MAX_STRUCTURAL_ITEMS: usize = 65_536;

/// 单个 JSON 字符串的最大原始编码字节数。
/// 6 MiB 可容纳 4 MiB payload 的 base64/加密膨胀，但拒绝占满整个 body 的单值。
pub const INBOX_JSON_MAX_STRING_BYTES: usize = 6 * 1024 * 1024;

/// 文件分块传输的单块大小。
pub const TRANSFER_CHUNK_SIZE: i64 = 4 * 1024 * 1024;

/// 分块上传端点的请求体上限。
///
/// 块内容以 base64 承载（4/3 膨胀），再加 JSON 键，故显著大于
/// [`TRANSFER_CHUNK_SIZE`]。
pub const TRANSFER_CHUNK_BODY_LIMIT: usize = 16 * 1024 * 1024;

/// 单个文件传输的总大小上限。
///
/// Product decision: large media is in-scope for self-hosted chat. Concurrent
/// amplification is bounded separately (MYR-008) rather than shrinking this.
pub const MAX_FILE_SIZE: i64 = 20 * 1024 * 1024 * 1024;

// ── MYR-008: transfer amplification admission (generous budgets) ───────────
//
// Prefer budgets + admission control over removing chunked transfer. Numbers
// are intentionally loose for normal multi-file use; they only stop absurd
// concurrent pile-ups (dozens of max-size files / unbounded in-flight chunks).

/// Max concurrent open transfers (`pending` + `in-progress`) process-wide.
pub const MAX_CONCURRENT_TRANSFERS: i64 = 64;

/// Max concurrent open transfers attributed to one local user (initiator/owner).
pub const MAX_CONCURRENT_TRANSFERS_PER_USER: i64 = 16;

/// Sum of declared `file_size` across open transfers must stay under this.
///
/// ≈ 3 × [`MAX_FILE_SIZE`]: a few full-size uploads at once, or many smaller.
pub const MAX_CONCURRENT_TRANSFER_BYTES: i64 = 64 * 1024 * 1024 * 1024; // 64 GiB

/// Max decoded chunk payload bytes held concurrently across upload/inbound handlers.
///
/// At [`TRANSFER_CHUNK_SIZE`] (4 MiB), this allows ~32 simultaneous chunk ops (**default**).
/// Active cap: [`max_in_flight_chunk_bytes`].
pub const MAX_IN_FLIGHT_CHUNK_BYTES: usize = 128 * 1024 * 1024; // 128 MiB

/// Live transfer chunk inflight byte budget.
#[inline]
pub fn max_in_flight_chunk_bytes() -> usize {
    crate::services::memory_profile::max_in_flight_chunk_bytes()
}

/// Live note image cap.
#[inline]
pub fn note_image_limit() -> usize {
    crate::services::memory_profile::note_image_limit()
}

/// Live note video cap.
#[inline]
pub fn note_video_limit() -> usize {
    crate::services::memory_profile::note_video_limit()
}

/// 单条 Note 的附件数量上限。
pub const NOTE_ATTACHMENT_COUNT_LIMIT: usize = 32;

/// 单条 Note 的正文字符数上限。
pub const NOTE_TEXT_CHAR_LIMIT: usize = 100_000;

// ---------------------------------------------------------------------------
// In-flight inbox raw-body budget
// ---------------------------------------------------------------------------
//
// Per-request body limit still allows a full-size delivery. This budget only
// caps *concurrent* buffering so remote requests cannot pile up parse-sized
// allocations inside the backend container.
//
// Chosen numbers (document + keep in sync with asserts below):
//
// | quantity                         | value        | rationale                          |
// |----------------------------------|--------------|------------------------------------|
// | INBOX_BODY_LIMIT (single)        | 8 MiB        | public JSON envelope               |
// | single-request parse peak (≈3×)  | ~24 MiB      | raw + serde_json::Value            |
// | INBOX_INFLIGHT_RAW_BUDGET        | 32 MiB       | at most 4 full raw bodies          |
// | INBOX_PARSE_CONCURRENCY          | 4            | at most 4 complete JSON trees      |
// | bounded parse-tree peak (≈3×)    | ~96 MiB      | independent of global body limit   |
//
// Exhausted budget → HTTP **429** (not a lower body limit / 413).

/// Concurrent raw-body reservation budget for inbox handlers (**default** profile).
///
/// Active process budget is [`inbox_inflight_raw_budget`] (memory-saver may lower it).
/// See the module comment block above for the full sizing rationale.
pub const INBOX_INFLIGHT_RAW_BUDGET: usize = 32 * 1024 * 1024;

/// Live inbox concurrent raw-body budget (default or memory-saver).
#[inline]
pub fn inbox_inflight_raw_budget() -> usize {
    crate::services::memory_profile::inbox_inflight_raw_budget()
}

/// Currently reserved raw body bytes across in-flight inbox handlers.
static INBOX_INFLIGHT_RAW_BYTES: AtomicUsize = AtomicUsize::new(0);

/// Complete inbox JSON trees currently admitted for parsing/dispatch.
static INBOX_ACTIVE_PARSES: AtomicUsize = AtomicUsize::new(0);

/// RAII permit for concurrent inbox body buffering. Releases on drop.
#[derive(Debug)]
pub struct InboxInflightPermit {
    bytes: usize,
}

impl InboxInflightPermit {
    /// Bytes this permit holds against [`INBOX_INFLIGHT_RAW_BUDGET`].
    #[cfg(test)]
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

impl Drop for InboxInflightPermit {
    fn drop(&mut self) {
        if self.bytes > 0 {
            INBOX_INFLIGHT_RAW_BYTES.fetch_sub(self.bytes, Ordering::AcqRel);
            self.bytes = 0;
        }
    }
}

/// RAII admission for one complete inbox JSON parse tree.
#[derive(Debug)]
pub struct InboxParsePermit;

impl Drop for InboxParsePermit {
    fn drop(&mut self) {
        INBOX_ACTIVE_PARSES.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Current complete inbox JSON trees retained by handlers.
#[cfg(test)]
pub fn inbox_active_parses() -> usize {
    INBOX_ACTIVE_PARSES.load(Ordering::Acquire)
}

/// Refuse work instead of queueing already-buffered remote bodies.
pub fn try_acquire_inbox_parse() -> Option<InboxParsePermit> {
    loop {
        let current = INBOX_ACTIVE_PARSES.load(Ordering::Acquire);
        if current >= INBOX_PARSE_CONCURRENCY {
            return None;
        }
        match INBOX_ACTIVE_PARSES.compare_exchange_weak(
            current,
            current + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return Some(InboxParsePermit),
            Err(_) => continue,
        }
    }
}

/// Lexically enforce JSON resource budgets before `serde_json::Value` allocates
/// the complete tree. This is intentionally not a second JSON parser: it only
/// recognizes strings and container delimiters needed for resource accounting;
/// `serde_json` remains the syntax authority.
pub fn validate_inbox_json_budget(input: &[u8]) -> Result<(), &'static str> {
    let mut stack = [0_u8; INBOX_JSON_MAX_DEPTH];
    let mut depth = 0_usize;
    let mut structural_items = 0_usize;
    let mut string_bytes = 0_usize;
    let mut in_string = false;
    let mut escaped = false;

    for &byte in input {
        if in_string {
            string_bytes = string_bytes.saturating_add(1);
            if string_bytes > INBOX_JSON_MAX_STRING_BYTES {
                return Err("JSON string exceeds inbox budget");
            }
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }

        match byte {
            b'"' => {
                in_string = true;
                string_bytes = 0;
            }
            b'{' | b'[' => {
                if depth >= INBOX_JSON_MAX_DEPTH {
                    return Err("JSON nesting exceeds inbox budget");
                }
                stack[depth] = byte;
                depth += 1;
                structural_items = structural_items.saturating_add(1);
            }
            b'}' | b']' => {
                let expected = if byte == b'}' { b'{' } else { b'[' };
                if depth == 0 || stack[depth - 1] != expected {
                    return Err("JSON containers are unbalanced");
                }
                depth -= 1;
            }
            b',' | b':' => {
                structural_items = structural_items.saturating_add(1);
            }
            _ => {}
        }

        if structural_items > INBOX_JSON_MAX_STRUCTURAL_ITEMS {
            return Err("JSON collection size exceeds inbox budget");
        }
    }

    if in_string || depth != 0 {
        return Err("JSON is structurally incomplete");
    }
    Ok(())
}

/// Snapshot of currently reserved in-flight raw body bytes (tests / metrics).
pub fn inbox_inflight_raw_bytes() -> usize {
    INBOX_INFLIGHT_RAW_BYTES.load(Ordering::Acquire)
}

/// Try to reserve `bytes` against the concurrent inbox raw-body budget.
///
/// `bytes` is clamped to `1..=INBOX_BODY_LIMIT` so a single full-size request
/// is always representable, and zero-length reservations still take a slot of 1.
///
/// Returns `None` when the budget cannot admit `bytes` without exceeding
/// the active [`inbox_inflight_raw_budget`] (caller should respond **429**).
pub fn try_acquire_inbox_inflight(bytes: usize) -> Option<InboxInflightPermit> {
    let body_cap = inbox_body_limit();
    let bytes = bytes.clamp(1, body_cap);
    let budget = inbox_inflight_raw_budget();
    loop {
        let current = INBOX_INFLIGHT_RAW_BYTES.load(Ordering::Acquire);
        let new = current.checked_add(bytes)?;
        if new > budget {
            return None;
        }
        match INBOX_INFLIGHT_RAW_BYTES.compare_exchange_weak(
            current,
            new,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return Some(InboxInflightPermit { bytes }),
            Err(_) => continue,
        }
    }
}

/// Reserve concurrent budget from `Content-Length` (or full inbox limit if
/// absent / unparseable), then buffer the request body up to [`INBOX_BODY_LIMIT`].
///
/// Acquire happens **before** buffering so concurrent peaks are accounted for
/// prior to allocating the body. The permit must be held until processing
/// finishes (drop releases the reservation).
pub async fn buffer_inbox_body(
    request: axum::http::Request<axum::body::Body>,
) -> Result<
    (axum::body::Bytes, InboxInflightPermit),
    (axum::http::StatusCode, axum::Json<serde_json::Value>),
> {
    use axum::http::{header, StatusCode};
    use axum::Json;
    use serde_json::json;

    let body_cap = inbox_body_limit();
    let content_length = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok());
    // Reject oversized Content-Length before buffering (memory-saver live cap).
    if let Some(cl) = content_length {
        if cl > body_cap {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(json!({
                    "error": format!(
                        "Inbox body exceeds limit: {cl} bytes (max {body_cap})"
                    )
                })),
            ));
        }
    }
    let reserve = content_length.unwrap_or(body_cap).clamp(1, body_cap);

    let permit = try_acquire_inbox_inflight(reserve).ok_or_else(|| {
        tracing::warn!(
            reserve_bytes = reserve,
            inflight = inbox_inflight_raw_bytes(),
            budget = inbox_inflight_raw_budget(),
            "inbox concurrent memory budget exhausted"
        );
        (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": "Inbox concurrent memory budget exhausted; retry later"
            })),
        )
    })?;

    let body = axum::body::to_bytes(request.into_body(), body_cap)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "failed to read federation inbox body");
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Failed to read body"})),
            )
        })?;

    Ok((body, permit))
}

/// GET `/api/federation/public/limits` — live message/media caps (no auth).
pub async fn public_limits() -> axum::Json<serde_json::Value> {
    axum::Json(crate::services::memory_profile::public_limits_snapshot())
}

// ── Live DefaultBodyLimit (memory profile hot-reload) ───────────────────────
//
// Axum's `DefaultBodyLimit::max(N)` freezes N when the router is built. Memory
// saver can change after config save without rebuilding routes. These layers
// re-read the active profile **per request**: Content-Length precheck + stream
// cap via `http_body_util::Limited`.

/// Which live budget a route should enforce at the HTTP body layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveBodyLimitKind {
    /// Shared / user inbox (remote deliveries).
    Inbox,
    /// Authenticated federation write surface (messages, follow, …).
    Authenticated,
    /// Freeform Note media upload (video cap + envelope).
    NoteMedia,
    /// Chunked file transfer upload.
    TransferChunk,
    /// Small control JSON (trust, room meta, …).
    SmallControl,
}

impl LiveBodyLimitKind {
    #[inline]
    pub fn limit_bytes(self) -> usize {
        match self {
            Self::Inbox => inbox_body_limit(),
            Self::Authenticated => authenticated_body_limit(),
            Self::NoteMedia => note_video_limit().saturating_add(16 * 1024 * 1024),
            Self::TransferChunk => TRANSFER_CHUNK_BODY_LIMIT,
            Self::SmallControl => SMALL_CONTROL_BODY_LIMIT,
        }
    }
}

async fn live_body_limit_middleware(
    kind: LiveBodyLimitKind,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::body::Body;
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;
    use axum::Json;
    use serde_json::json;

    let limit = kind.limit_bytes();
    if let Some(cl) = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
    {
        if cl > limit {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(json!({
                    "error": format!("Request body too large: {cl} bytes (max {limit})")
                })),
            )
                .into_response();
        }
    }

    let (parts, body) = request.into_parts();
    // Stream cap without Content-Length (Content-Length already checked above).
    let limited_body = http_body_util::Limited::new(body, limit);
    let request = axum::extract::Request::from_parts(parts, Body::new(limited_body));
    next.run(request).await
}

/// Named middleware entry points (axum `from_fn` needs plain async fns).
pub async fn live_inbox_body_limit(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    live_body_limit_middleware(LiveBodyLimitKind::Inbox, request, next).await
}

pub async fn live_authenticated_body_limit(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    live_body_limit_middleware(LiveBodyLimitKind::Authenticated, request, next).await
}

pub async fn live_note_media_body_limit(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    live_body_limit_middleware(LiveBodyLimitKind::NoteMedia, request, next).await
}

pub async fn live_transfer_chunk_body_limit(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    live_body_limit_middleware(LiveBodyLimitKind::TransferChunk, request, next).await
}

pub async fn live_small_control_body_limit(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    live_body_limit_middleware(LiveBodyLimitKind::SmallControl, request, next).await
}

// 编译期不变量
//
// 这些关系是常量之间的，没有理由等到跑测试才发现 —— 违反它们直接编译失败。
// 想调大某个上限时，这里会先把连带约束顶出来。

/// 本实例发得出的东西，本实例必须收得进。
///
/// 违反它的表现不是错误信息，而是消息在对端被 413 静默拒收。
const _: () = assert!(
    INBOX_BODY_LIMIT > MESSAGE_PAYLOAD_LIMIT,
    "INBOX_BODY_LIMIT must exceed MESSAGE_PAYLOAD_LIMIT to fit the activity envelope"
);

/// 信封与 JSON 转义膨胀至少要有 25% 余量。
const _: () = assert!(
    INBOX_BODY_LIMIT - MESSAGE_PAYLOAD_LIMIT >= MESSAGE_PAYLOAD_LIMIT / 4,
    "not enough envelope headroom between MESSAGE_PAYLOAD_LIMIT and INBOX_BODY_LIMIT"
);

/// 本地用户提交的内容最终要能作为活动投递出去。
const _: () = assert!(
    AUTHENTICATED_BODY_LIMIT >= INBOX_BODY_LIMIT,
    "AUTHENTICATED_BODY_LIMIT must not be narrower than INBOX_BODY_LIMIT"
);

/// 分块请求体要容得下 base64 的 4/3 膨胀。
const _: () = assert!(
    TRANSFER_CHUNK_BODY_LIMIT > (TRANSFER_CHUNK_SIZE as usize) * 4 / 3,
    "TRANSFER_CHUNK_BODY_LIMIT cannot hold a base64-encoded TRANSFER_CHUNK_SIZE"
);

/// 控制类端点不该能缓冲和消息端点一样多的数据。
const _: () = assert!(
    SMALL_CONTROL_BODY_LIMIT < MESSAGE_PAYLOAD_LIMIT / 8,
    "SMALL_CONTROL_BODY_LIMIT is too close to the bulk message limit"
);

/// Concurrent transfer budget must be at least one full max-size file.
const _: () = assert!(
    MAX_CONCURRENT_TRANSFER_BYTES >= MAX_FILE_SIZE,
    "MAX_CONCURRENT_TRANSFER_BYTES must admit at least one MAX_FILE_SIZE transfer"
);

/// Per-user concurrent transfer cap must not exceed the global cap.
const _: () = assert!(
    MAX_CONCURRENT_TRANSFERS_PER_USER <= MAX_CONCURRENT_TRANSFERS,
    "per-user transfer cap cannot exceed the global concurrent transfer cap"
);

/// In-flight chunk budget must hold at least one full chunk.
const _: () = assert!(
    MAX_IN_FLIGHT_CHUNK_BYTES >= TRANSFER_CHUNK_SIZE as usize,
    "MAX_IN_FLIGHT_CHUNK_BYTES must hold at least one TRANSFER_CHUNK_SIZE"
);

/// 容器内存预算哨兵。
///
/// 单个满额 inbox 请求峰值约 `3 × INBOX_BODY_LIMIT`（原始字节 + 解析后的 Value）。
/// compose 给 backend 2 GiB；要求单请求峰值不超过其 1/4，好让若干并发投递
/// 不至于把容器打爆。
///
/// **这条先失败就是提醒：调高上限前先抬高 docker-compose.yml 里的 `memory:`。**
const _: () = assert!(
    INBOX_BODY_LIMIT * 3 <= (2 * 1024 * 1024 * 1024_usize) / 4,
    "a max-size inbox request would peak past a quarter of the 2 GiB container limit; \
     raise `memory:` in docker-compose.yml before raising INBOX_BODY_LIMIT"
);

/// 并发 raw 预算必须能放下至少一次满额投递，否则单请求也会 429。
const _: () = assert!(
    INBOX_INFLIGHT_RAW_BUDGET >= INBOX_BODY_LIMIT,
    "INBOX_INFLIGHT_RAW_BUDGET must admit at least one full-size inbox body"
);

/// Raw reservation and complete-tree admission describe the same full-size
/// concurrency ceiling; neither budget may silently outrun the other.
const _: () = assert!(
    INBOX_INFLIGHT_RAW_BUDGET <= INBOX_BODY_LIMIT * INBOX_PARSE_CONCURRENCY,
    "raw inbox budget exceeds complete JSON parse concurrency budget"
);

const _: () = assert!(
    INBOX_JSON_MAX_STRING_BYTES < INBOX_BODY_LIMIT,
    "a single inbox string must not consume the entire public body budget"
);

/// 并发 raw 预算对应的解析峰值（按 3× 粗算）应落在 2 GiB 容器内。
const _: () = assert!(
    INBOX_INFLIGHT_RAW_BUDGET * 3 <= 2 * 1024 * 1024 * 1024_usize,
    "INBOX_INFLIGHT_RAW_BUDGET × 3 would exceed the 2 GiB container memory limit"
);

#[cfg(test)]
mod tests {

    /// 实证 axum 的 layer 覆盖语义。
    ///
    /// 联邦路由的 body 上限现在**大于**外层的全局上限，这依赖「离 handler 更近的
    /// `DefaultBodyLimit` 覆盖更外层的」。这条语义如果不成立，联邦上传会被外层
    /// 静默夹到 50 MB —— 表现为大文件传输莫名 413，且没有任何编译期信号。
    ///
    /// 用真实的 Router 走一遍请求来证明，而不是靠读文档假设。
    #[tokio::test]
    async fn inner_body_limit_overrides_the_outer_one() {
        use axum::http::{Request, StatusCode};
        use axum::{body::Body, extract::DefaultBodyLimit, routing::post, Router};
        use tower::ServiceExt;

        const OUTER: usize = 1024;
        const INNER: usize = 8 * 1024;

        async fn echo(body: axum::body::Bytes) -> String {
            body.len().to_string()
        }

        // 内层（联邦子路由）比外层（全局）更宽，与 main.rs 的结构一致
        let inner = Router::new()
            .route("/wide", post(echo))
            .layer(DefaultBodyLimit::max(INNER));
        let app = Router::new()
            .route("/narrow", post(echo))
            .merge(inner)
            .layer(DefaultBodyLimit::max(OUTER));

        let payload = vec![b'x'; 4 * 1024]; // 大于 OUTER，小于 INNER

        let wide = app
            .clone()
            .oneshot(
                Request::post("/wide")
                    .body(Body::from(payload.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            wide.status(),
            StatusCode::OK,
            "inner DefaultBodyLimit must widen past the outer one; if this fails, \
             federation uploads are silently clamped to the global limit"
        );

        // 同一个 body 打到只受外层约束的路由上，必须被拒 —— 证明外层确实生效，
        // 上面的成功不是因为两个 limit 都没起作用
        let narrow = app
            .oneshot(Request::post("/narrow").body(Body::from(payload)).unwrap())
            .await
            .unwrap();
        assert_eq!(narrow.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}

#[cfg(test)]
mod constant_tests {
    use super::*;

    #[test]
    fn public_inbox_has_independent_bounded_json_budget() {
        assert_eq!(MESSAGE_PAYLOAD_LIMIT, 4 * 1024 * 1024);
        assert_eq!(INBOX_BODY_LIMIT, 8 * 1024 * 1024);
        assert_eq!(
            AUTHENTICATED_BODY_LIMIT,
            INBOX_BODY_LIMIT + 16 * 1024 * 1024
        );
        assert_eq!(AUTHENTICATED_BODY_LIMIT, 24 * 1024 * 1024);
        assert_eq!(INBOX_INFLIGHT_RAW_BUDGET, 32 * 1024 * 1024);
        let max_full = INBOX_INFLIGHT_RAW_BUDGET / INBOX_BODY_LIMIT;
        assert_eq!(max_full, INBOX_PARSE_CONCURRENCY);
    }

    /// Product single-request limits stay large (media); concurrent raw budget is
    /// the 1 GiB-host tension (512 MiB reserved buffering alone). Changing these
    /// is a product/profile decision — this test fails loudly if they drift.
    #[test]
    fn public_inbox_peak_stays_bounded_on_1g_host() {
        const ONE_GIB: usize = 1024 * 1024 * 1024;
        const {
            assert!(INBOX_BODY_LIMIT >= MESSAGE_PAYLOAD_LIMIT);
            assert!(
                INBOX_INFLIGHT_RAW_BUDGET * 3 <= ONE_GIB / 4,
                "estimated inbox parse peak must stay below one quarter of 1 GiB"
            );
            assert!(
                MAX_IN_FLIGHT_CHUNK_BYTES <= 128 * 1024 * 1024,
                "chunk inflight must stay documented upper bound"
            );
        }
        assert_eq!(MAX_IN_FLIGHT_CHUNK_BYTES, 128 * 1024 * 1024);
    }

    #[test]
    fn live_body_limit_kinds_track_memory_profile() {
        let _g = crate::services::memory_profile::test_profile_lock();
        crate::services::memory_profile::apply(
            crate::services::memory_profile::MemoryProfile::Default,
        );
        assert_eq!(LiveBodyLimitKind::Inbox.limit_bytes(), INBOX_BODY_LIMIT);
        assert_eq!(
            LiveBodyLimitKind::Authenticated.limit_bytes(),
            AUTHENTICATED_BODY_LIMIT
        );

        crate::services::memory_profile::apply(
            crate::services::memory_profile::MemoryProfile::Saver,
        );
        assert_eq!(
            LiveBodyLimitKind::Inbox.limit_bytes(),
            crate::services::memory_profile::SAVER_INBOX_BODY_LIMIT
        );
        assert_eq!(
            LiveBodyLimitKind::Authenticated.limit_bytes(),
            crate::services::memory_profile::SAVER_AUTHENTICATED_BODY_LIMIT
        );
        assert!(
            LiveBodyLimitKind::NoteMedia.limit_bytes()
                < crate::services::memory_profile::DEFAULT_NOTE_VIDEO_LIMIT + 16 * 1024 * 1024
        );
        // Restore default for other tests in this process.
        crate::services::memory_profile::apply(
            crate::services::memory_profile::MemoryProfile::Default,
        );
    }
}

#[cfg(test)]
mod inflight_budget_tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    /// Global counter is process-wide; serialize these tests so they do not race.
    fn budget_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn single_full_size_always_fits_when_idle() {
        let _guard = budget_test_lock();
        assert_eq!(
            inbox_inflight_raw_bytes(),
            0,
            "test isolation: inflight counter must start at 0"
        );
        let p = try_acquire_inbox_inflight(INBOX_BODY_LIMIT).expect("full-size when idle");
        assert_eq!(p.bytes(), INBOX_BODY_LIMIT);
        assert_eq!(inbox_inflight_raw_bytes(), INBOX_BODY_LIMIT);
        drop(p);
        assert_eq!(inbox_inflight_raw_bytes(), 0);
    }

    #[test]
    fn exhausted_budget_returns_none_then_recovers_on_drop() {
        let _guard = budget_test_lock();
        assert_eq!(inbox_inflight_raw_bytes(), 0);

        // Fill the budget with full-size permits.
        let n = INBOX_INFLIGHT_RAW_BUDGET / INBOX_BODY_LIMIT;
        let mut permits = Vec::with_capacity(n);
        for _ in 0..n {
            permits.push(try_acquire_inbox_inflight(INBOX_BODY_LIMIT).expect("slot"));
        }
        assert_eq!(inbox_inflight_raw_bytes(), n * INBOX_BODY_LIMIT);

        // Next full-size must fail (429 path).
        assert!(
            try_acquire_inbox_inflight(INBOX_BODY_LIMIT).is_none(),
            "budget exhausted must reject another full-size reservation"
        );

        // A tiny request may still fit if remainder allows — only assert when
        // remainder is strictly less than 1 (always after exact fill).
        let remainder = INBOX_INFLIGHT_RAW_BUDGET - n * INBOX_BODY_LIMIT;
        if remainder == 0 {
            assert!(try_acquire_inbox_inflight(1).is_none());
        }

        drop(permits);
        assert_eq!(inbox_inflight_raw_bytes(), 0);
        let recovered = try_acquire_inbox_inflight(INBOX_BODY_LIMIT);
        assert!(recovered.is_some());
        drop(recovered);
        assert_eq!(inbox_inflight_raw_bytes(), 0);
    }

    #[test]
    fn clamps_reservation_to_inbox_body_limit() {
        let _guard = budget_test_lock();
        assert_eq!(inbox_inflight_raw_bytes(), 0);
        let p = try_acquire_inbox_inflight(INBOX_BODY_LIMIT * 2).expect("clamped");
        assert_eq!(p.bytes(), INBOX_BODY_LIMIT);
        drop(p);
        assert_eq!(inbox_inflight_raw_bytes(), 0);
    }

    #[test]
    fn parse_admission_refuses_the_fifth_complete_tree() {
        let _guard = budget_test_lock();
        assert_eq!(inbox_active_parses(), 0);
        let permits: Vec<_> = (0..INBOX_PARSE_CONCURRENCY)
            .map(|_| try_acquire_inbox_parse().expect("parse slot"))
            .collect();
        assert!(try_acquire_inbox_parse().is_none());
        drop(permits);
        assert_eq!(inbox_active_parses(), 0);
    }

    #[test]
    fn json_budget_accepts_normal_activity_and_escaped_delimiters() {
        let body =
            br#"{"type":"Create","actor":"https://peer.test/u/a","object":{"content":"[]{}\\\""}}"#;
        assert_eq!(validate_inbox_json_budget(body), Ok(()));
        let _: serde_json::Value = serde_json::from_slice(body).expect("valid control JSON");
    }

    #[test]
    fn json_budget_rejects_depth_before_value_allocation() {
        let mut body = vec![b'['; INBOX_JSON_MAX_DEPTH + 1];
        body.extend(std::iter::repeat_n(b']', INBOX_JSON_MAX_DEPTH + 1));
        assert_eq!(
            validate_inbox_json_budget(&body),
            Err("JSON nesting exceeds inbox budget")
        );
    }

    #[test]
    fn json_budget_rejects_large_collection_and_string() {
        let mut collection = Vec::from("[0".as_bytes());
        for _ in 0..=INBOX_JSON_MAX_STRUCTURAL_ITEMS {
            collection.extend_from_slice(b",0");
        }
        collection.push(b']');
        assert_eq!(
            validate_inbox_json_budget(&collection),
            Err("JSON collection size exceeds inbox budget")
        );

        let mut string = Vec::with_capacity(INBOX_JSON_MAX_STRING_BYTES + 3);
        string.push(b'"');
        string.extend(std::iter::repeat_n(b'x', INBOX_JSON_MAX_STRING_BYTES + 1));
        string.push(b'"');
        assert_eq!(
            validate_inbox_json_budget(&string),
            Err("JSON string exceeds inbox budget")
        );
    }
}

#[cfg(test)]
mod optional_json_tests {
    /// `Option<Json<T>>` 对「空 body」与「畸形 JSON」的实际行为。
    ///
    /// `federation_join_room` 的 body 是可选的，原实现是
    /// `from_slice(..).unwrap_or_default()` —— 空 body 和畸形 JSON 都回落到默认值。
    /// 换成 `Option<Json<T>>` 之后这两种情况是否仍然一致，必须实测，
    /// 不能靠读文档猜。
    #[tokio::test]
    async fn optional_json_distinguishes_absent_from_malformed() {
        use axum::http::{Request, StatusCode};
        use axum::{body::Body, routing::post, Json, Router};
        use tower::ServiceExt;

        #[derive(Default, serde::Deserialize)]
        struct Payload {
            home_server: Option<String>,
        }

        async fn handler(body: Option<Json<Payload>>) -> String {
            let p = body.map(|Json(v)| v).unwrap_or_default();
            p.home_server.unwrap_or_else(|| "DEFAULT".into())
        }

        let app = Router::new().route("/j", post(handler));

        // 1) 完全没有 body → 走默认值（join_room 的正常用法）
        let empty = app
            .clone()
            .oneshot(Request::post("/j").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            empty.status(),
            StatusCode::OK,
            "empty body must be accepted"
        );

        // 2) 合法 body → 正常解析
        let ok = app
            .clone()
            .oneshot(
                Request::post("/j")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"home_server":"h"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ok.status(), StatusCode::OK);

        // 3) 畸形 JSON → 这里与原实现**有差异**：原来 unwrap_or_default() 静默
        // 回落，现在 axum 会拒绝。对一个"加入房间"的请求，显式报错比静默
        // 忽略用户传错的 home_server 更好，所以接受这个变化并在此记录。
        let malformed = app
            .oneshot(
                Request::post("/j")
                    .header("content-type", "application/json")
                    .body(Body::from("{not json"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            malformed.status(),
            StatusCode::BAD_REQUEST,
            "malformed JSON is now rejected instead of silently defaulting"
        );
    }
}

#[cfg(test)]
mod rejection_tests {
    /// 超限 body 必须仍然拿到带分块传输指引的 413。
    ///
    /// `send_room_message` 原本手工 `Bytes::from_request` 才能返回这句提示；
    /// 换成 `Json<T>` 提取器后，如果不接管 `JsonRejection`，用户会收到 axum 的
    /// 纯文本 413，看不到"更大的文件走分块传输"这个下一步动作。
    #[tokio::test]
    async fn oversized_body_keeps_the_chunked_transfer_hint() {
        use axum::extract::rejection::JsonRejection;
        use axum::http::{Request, StatusCode};
        use axum::{body::Body, extract::DefaultBodyLimit, routing::post, Json, Router};
        use tower::ServiceExt;

        #[derive(serde::Deserialize)]
        struct P {
            _x: Option<String>,
        }

        async fn handler(payload: Result<Json<P>, JsonRejection>) -> axum::response::Response {
            match payload {
                Ok(_) => "ok".into_response(),
                Err(e) => {
                    crate::api::federation::json_rejection_response(e, Some("use chunked transfer"))
                }
            }
        }
        use axum::response::IntoResponse;

        let app = Router::new()
            .route("/m", post(handler))
            .layer(DefaultBodyLimit::max(64));

        let resp = app
            .oneshot(
                Request::post("/m")
                    .header("content-type", "application/json")
                    .body(Body::from(vec![b'x'; 4096]))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains("use chunked transfer"),
            "the operator-facing hint must survive the extractor switch; got: {text}"
        );
    }
}
