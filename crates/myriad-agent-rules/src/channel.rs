//! Pure QQ C2C transport rules. No I/O.
//!
//! Capability names, session-key colon sanitizing, passive `msg_seq`,
//! outbound idempotency, and Transient/Permanent classification follow the
//! shape of Easybot's QQ adapter. Copied files keep GPL-3 headers; this
//! module is original AGPL-3 host code.

/// Reply when an unpaired C2C text arrives and is not a pairing code.
pub const PAIRING_REQUIRED_REPLY: &str = "请先去站点配对后再发消息。";

/// Reply after a pairing code binds this openid to a site user.
pub const PAIRING_OK_REPLY: &str = "配对成功。之后在这里发消息就是在站点办事。";

/// Reply when the inbound text looks like a code but is missing, expired, or used.
pub const PAIRING_INVALID_REPLY: &str = "配对码无效或已过期，请回站点重新生成。";

/// Reply when this openid is already bound to a different site user.
pub const PAIRING_TAKEN_REPLY: &str = "这个 QQ 号已经绑过别人。请先在原账号解除，或换一个号。";

/// Reply when confirmation or a browser-only page action arrives on QQ.
pub const PANEL_REQUIRED_REPLY: &str = "请到站点面板完成这一步。";

/// Crockford Base32 without checksum. I/L → 1, O → 0. Eight characters = 40 bits.
pub const PAIRING_CODE_ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Encode 5 random bytes as an 8-character pairing code.
pub fn encode_pairing_code(bytes: [u8; 5]) -> String {
    let mut n = 0u64;
    for byte in bytes {
        n = (n << 8) | u64::from(byte);
    }
    let mut chars = [0u8; 8];
    for slot in chars.iter_mut().rev() {
        *slot = PAIRING_CODE_ALPHABET[(n & 31) as usize];
        n >>= 5;
    }
    String::from_utf8(chars.to_vec()).expect("Crockford alphabet is ASCII")
}

/// Display form `ABCD-EFGH`. Unknown shapes pass through.
pub fn format_pairing_code(code: &str) -> String {
    if code.len() == 8 && code.bytes().all(|b| PAIRING_CODE_ALPHABET.contains(&b)) {
        format!("{}-{}", &code[..4], &code[4..])
    } else {
        code.to_string()
    }
}

/// Whole inbound text is a pairing code: optional hyphen/spaces, Crockford letters.
/// Extra words are ordinary chat, not a code.
pub fn extract_pairing_code(content: &str) -> Option<String> {
    let mut out = String::new();
    for ch in content.trim().chars() {
        if ch == '-' || ch.is_ascii_whitespace() {
            continue;
        }
        let mapped = match ch.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            upper if PAIRING_CODE_ALPHABET.contains(&(upper as u8)) => upper,
            _ => return None,
        };
        out.push(mapped);
        if out.len() > 8 {
            return None;
        }
    }
    (out.len() == 8).then_some(out)
}

/// First-cut QQ C2C capability bitmap. Undeclared capabilities are unsupported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelCapabilities {
    pub inbound_text: bool,
    pub inbound_media: bool,
    pub inbound_callback: bool,
    pub outbound_final_text: bool,
    pub outbound_markdown: bool,
    pub outbound_image: bool,
    pub outbound_edit: bool,
    pub outbound_streaming_draft: bool,
    pub interactive: bool,
    pub frontend_action: bool,
    pub performance: bool,
    pub outfit: bool,
}

/// First-cut C2C: text in, final text out. Everything else is false.
pub fn qq_c2c_capabilities() -> ChannelCapabilities {
    ChannelCapabilities {
        inbound_text: true,
        inbound_media: false,
        inbound_callback: false,
        outbound_final_text: true,
        outbound_markdown: false,
        outbound_image: false,
        outbound_edit: false,
        outbound_streaming_draft: false,
        interactive: false,
        frontend_action: false,
        performance: false,
        outfit: false,
    }
}

/// Session key `platform:chatId`. Colons inside each part become `_` so
/// `qq` + `user:1` cannot collide with `qq:user` + `1`.
pub fn session_key(platform: &str, chat_id: &str) -> String {
    format!(
        "{}:{}",
        sanitize_key_part(platform),
        sanitize_key_part(chat_id)
    )
}

fn sanitize_key_part(value: &str) -> String {
    value.replace(':', "_")
}

/// Inbound C2C text after Gateway decoding. Attachments are ignored in the first cut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundC2cText {
    pub msg_id: String,
    pub user_openid: String,
    pub content: String,
}

/// Whether this openid already maps to a Myriad user. Lookup itself is I/O;
/// the adapter only consumes the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingLookup {
    Unpaired,
    Paired { user_id: i32 },
}

/// What ingest decides. Never silently act as the site owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundDecision {
    PairingRequired {
        user_openid: String,
        reply: String,
        msg_id: String,
    },
    ConsumePairingCode {
        user_openid: String,
        code: String,
        msg_id: String,
    },
    StartWork {
        user_id: i32,
        input: String,
        mode: String,
        session_key: String,
        msg_id: String,
    },
    Duplicate {
        msg_id: String,
    },
}

/// Result of consuming a pairing code against stored identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingBindResult {
    Bound { user_id: i32 },
    InvalidOrExpired,
    OpenidTaken,
}

/// Text to send after a pairing-code consume. Binding I/O stays outside this crate.
pub fn pairing_bind_reply(result: PairingBindResult) -> &'static str {
    match result {
        PairingBindResult::Bound { .. } => PAIRING_OK_REPLY,
        PairingBindResult::InvalidOrExpired => PAIRING_INVALID_REPLY,
        PairingBindResult::OpenidTaken => PAIRING_TAKEN_REPLY,
    }
}

/// Turn a C2C text + pairing result into a Work request, pairing-code consume,
/// or a pairing prompt. `already_seen` is whether this `msg_id` already opened a run.
pub fn ingest_c2c_text(
    event: &InboundC2cText,
    pairing: PairingLookup,
    already_seen: bool,
) -> InboundDecision {
    if already_seen {
        return InboundDecision::Duplicate {
            msg_id: event.msg_id.clone(),
        };
    }
    match pairing {
        PairingLookup::Unpaired => {
            if let Some(code) = extract_pairing_code(&event.content) {
                InboundDecision::ConsumePairingCode {
                    user_openid: event.user_openid.clone(),
                    code,
                    msg_id: event.msg_id.clone(),
                }
            } else {
                InboundDecision::PairingRequired {
                    user_openid: event.user_openid.clone(),
                    reply: PAIRING_REQUIRED_REPLY.to_string(),
                    msg_id: event.msg_id.clone(),
                }
            }
        }
        PairingLookup::Paired { user_id } => InboundDecision::StartWork {
            user_id,
            input: event.content.clone(),
            mode: "work".to_string(),
            session_key: session_key("qq", &event.user_openid),
            msg_id: event.msg_id.clone(),
        },
    }
}

/// Normalized Agent progress / final response for the first-cut deliverer.
/// Thinking and step events collapse to unit variants so tests do not mock
/// the Planner stream shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelEvent {
    ThinkingToken,
    StepStarted,
    StepCompleted,
    Progress,
    Answer { message: String },
    Error { message: String },
    ConfirmationRequired,
    FrontendAction,
}

/// Passive window leftover from the inbound C2C message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryContext {
    pub inbound_msg_id: Option<String>,
    pub passive_window_open: bool,
    pub remaining_passive_replies: u8,
}

/// What to send, swallow, or fail visibly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryPlan {
    Drop,
    PassiveText {
        content: String,
        msg_id: String,
    },
    ActiveText {
        content: String,
    },
    FailVisible {
        content: String,
        msg_id: Option<String>,
        passive: bool,
    },
}

fn can_reply_passively(ctx: &DeliveryContext) -> bool {
    ctx.passive_window_open
        && ctx.remaining_passive_replies > 0
        && ctx.inbound_msg_id.as_ref().is_some_and(|id| !id.is_empty())
}

fn send_text(content: String, ctx: &DeliveryContext) -> DeliveryPlan {
    if can_reply_passively(ctx) {
        DeliveryPlan::PassiveText {
            content,
            msg_id: ctx.inbound_msg_id.clone().expect("checked"),
        }
    } else {
        DeliveryPlan::ActiveText { content }
    }
}

/// Map a finished-turn event onto a delivery plan. Process events are dropped.
pub fn plan_delivery(event: &ChannelEvent, ctx: &DeliveryContext) -> DeliveryPlan {
    match event {
        ChannelEvent::ThinkingToken
        | ChannelEvent::StepStarted
        | ChannelEvent::StepCompleted
        | ChannelEvent::Progress => DeliveryPlan::Drop,
        ChannelEvent::Answer { message } | ChannelEvent::Error { message } => {
            send_text(message.clone(), ctx)
        }
        ChannelEvent::ConfirmationRequired | ChannelEvent::FrontendAction => {
            let content = PANEL_REQUIRED_REPLY.to_string();
            if can_reply_passively(ctx) {
                DeliveryPlan::FailVisible {
                    content,
                    msg_id: ctx.inbound_msg_id.clone(),
                    passive: true,
                }
            } else {
                DeliveryPlan::FailVisible {
                    content,
                    msg_id: None,
                    passive: false,
                }
            }
        }
    }
}

/// First unused `msg_seq` for a passive reply. QQ starts at 1.
pub fn next_passive_seq(last: Option<u32>) -> u32 {
    last.unwrap_or(0).saturating_add(1).max(1)
}

/// Outbound idempotency key. Colons in chat id are sanitized the same way as
/// session keys so two openids cannot share a delivery slot.
pub fn outbound_idempotency_key(
    platform: &str,
    chat_id: &str,
    msg_id: Option<&str>,
    seq: u32,
) -> String {
    let slot = msg_id
        .filter(|id| !id.is_empty())
        .map(sanitize_key_part)
        .unwrap_or_else(|| "active".to_string());
    format!(
        "{}:{}:{}:{seq}",
        sanitize_key_part(platform),
        sanitize_key_part(chat_id),
        slot
    )
}

/// Whether the Gateway worker should be running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerIntent {
    Run,
    Stop,
}

/// Switch on and both credentials present → run; anything else → stop.
pub fn worker_intent(enabled: bool, app_id: &str, has_app_secret: bool) -> WorkerIntent {
    if enabled && !app_id.trim().is_empty() && has_app_secret {
        WorkerIntent::Run
    } else {
        WorkerIntent::Stop
    }
}

/// Official `GROUP_AND_C2C_EVENT`. One bit covers C2C and group events.
/// First-cut worker only needs C2C; do not add guild intents (4014 on 公域).
/// <https://bot.q.qq.com/wiki/develop/api-v2/dev-prepare/interface-framework/event-emit.html>
pub const GROUP_AND_C2C_EVENT: u32 = 1 << 25;

/// Transport-level failure after attempting to connect or refresh a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectFailure<'a> {
    HttpStatus { status: u16, body: &'a str },
    AuthRejected { close_code: u16 },
    TokenRejected { body: &'a str },
    Transport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectFailureKind {
    Permanent,
    Transient,
}

/// Unified-platform token endpoint JSON `code` values (EasyBot QQ adapter).
/// 100001 is rate-limit; 100016 / 100007 / 10004 are credential / bot-state.
pub fn classify_access_token_code(code: i64) -> ConnectFailureKind {
    match code {
        100001 => ConnectFailureKind::Transient,
        100016 | 100007 | 10004 => ConnectFailureKind::Permanent,
        _ => ConnectFailureKind::Permanent,
    }
}

/// Token is expired or the auth service asked for a refresh.
/// 401 / 11244 are expire; 11242 is "retry once" (EasyBot `is_qq_token_invalid_response`).
pub fn qq_token_needs_refresh(status: u16, body: &str) -> bool {
    status == 401
        || body.contains("11244")
        || body.contains("token not exist or expire")
        || body.contains("11242")
}

/// Official Node SDK close codes that must not reconnect.
/// 4004 token; 4013/4014 intents; 4914 delisted; 4915 banned.
/// <https://github.com/tencent-connect/bot-node-sdk/blob/main/src/types/websocket-types.ts>
pub fn classify_gateway_close(close_code: u16) -> ConnectFailureKind {
    match close_code {
        4004 | 4013 | 4014 | 4914 | 4915 => ConnectFailureKind::Permanent,
        _ => ConnectFailureKind::Transient,
    }
}

/// Parse `POST /app/getAppAccessToken`. Never returns the secret; errors are kinds only.
pub fn parse_access_token_response(
    status: u16,
    body: &str,
) -> Result<(String, u64), ConnectFailureKind> {
    let data = match serde_json::from_str::<serde_json::Value>(body) {
        Ok(value) => value,
        Err(_) => {
            return Err(classify_connect_failure(&ConnectFailure::HttpStatus {
                status,
                body,
            }));
        }
    };
    if let Some(code) = json_code(&data) {
        return Err(classify_access_token_code(code));
    }
    if !(200..300).contains(&status) {
        return Err(classify_connect_failure(&ConnectFailure::HttpStatus {
            status,
            body,
        }));
    }
    let access_token = data
        .get("access_token")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ConnectFailureKind::Permanent)?;
    let expires_in = data.get("expires_in").and_then(json_u64).unwrap_or(7200);
    Ok((access_token.to_string(), expires_in))
}

/// Parse `GET /gateway/bot` `{ "url": "wss://..." }`.
/// 401 / 11244 stop; 11242 is retry-once so the worker can refresh the token.
pub fn parse_gateway_url_response(status: u16, body: &str) -> Result<String, ConnectFailureKind> {
    if body.contains("11242") {
        return Err(ConnectFailureKind::Transient);
    }
    if qq_token_needs_refresh(status, body) {
        return Err(ConnectFailureKind::Permanent);
    }
    if !(200..300).contains(&status) {
        return Err(classify_connect_failure(&ConnectFailure::HttpStatus {
            status,
            body,
        }));
    }
    let data: serde_json::Value =
        serde_json::from_str(body).map_err(|_| ConnectFailureKind::Transient)?;
    data.get("url")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|url| url.starts_with("ws://") || url.starts_with("wss://"))
        .map(str::to_string)
        .ok_or(ConnectFailureKind::Transient)
}

fn json_code(data: &serde_json::Value) -> Option<i64> {
    data.get("code").and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_u64().map(|n| n as i64))
            .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
    })
}

fn json_u64(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|n| u64::try_from(n).ok()))
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

/// 401/403, gateway auth close codes, and token rejection are permanent.
/// Network jitter and 5xx stay transient so a configured bot is not treated
/// as "never configured".
pub fn classify_connect_failure(failure: &ConnectFailure<'_>) -> ConnectFailureKind {
    match failure {
        ConnectFailure::Transport => ConnectFailureKind::Transient,
        ConnectFailure::AuthRejected { close_code } => classify_gateway_close(*close_code),
        ConnectFailure::TokenRejected { .. } => ConnectFailureKind::Permanent,
        ConnectFailure::HttpStatus { status, body } => {
            if body.contains("11242") {
                return ConnectFailureKind::Transient;
            }
            if *status == 401
                || *status == 403
                || body.contains("11244")
                || body.contains("token not exist or expire")
            {
                return ConnectFailureKind::Permanent;
            }
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(body) {
                if let Some(code) = json_code(&data) {
                    return classify_access_token_code(code);
                }
            } else if body.contains("\"code\":100001") || body.contains("code\":100001") {
                return ConnectFailureKind::Transient;
            }
            if *status >= 500 || *status == 429 {
                ConnectFailureKind::Transient
            } else if *status >= 400 {
                ConnectFailureKind::Permanent
            } else {
                ConnectFailureKind::Transient
            }
        }
    }
}
