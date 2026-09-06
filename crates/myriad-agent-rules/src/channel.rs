//! Pure QQ C2C transport rules. No I/O.
//!
//! Capability names, session-key colon sanitizing, passive `msg_seq`,
//! outbound idempotency, and Transient/Permanent classification follow the
//! shape of Easybot's QQ adapter. Copied files keep GPL-3 headers; this
//! module is original AGPL-3 host code.

/// Reply when an unpaired C2C text arrives. First-cut copy; pairing UI is later.
pub const PAIRING_REQUIRED_REPLY: &str = "请先去站点配对后再发消息。";

/// Reply when confirmation or a browser-only page action arrives on QQ.
pub const PANEL_REQUIRED_REPLY: &str = "请到站点面板完成这一步。";

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
    format!("{}:{}", sanitize_key_part(platform), sanitize_key_part(chat_id))
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

/// Turn a C2C text + pairing result into a Work request or a pairing reply.
/// `already_seen` is whether this `msg_id` already opened a run.
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
        PairingLookup::Unpaired => InboundDecision::PairingRequired {
            user_openid: event.user_openid.clone(),
            reply: PAIRING_REQUIRED_REPLY.to_string(),
            msg_id: event.msg_id.clone(),
        },
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
    PassiveText { content: String, msg_id: String },
    ActiveText { content: String },
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

/// 401/403, gateway auth close codes, and token rejection are permanent.
/// Network jitter and 5xx stay transient so a configured bot is not treated
/// as "never configured".
pub fn classify_connect_failure(failure: &ConnectFailure<'_>) -> ConnectFailureKind {
    match failure {
        ConnectFailure::Transport => ConnectFailureKind::Transient,
        ConnectFailure::AuthRejected { close_code } => {
            if matches!(*close_code, 4004 | 4014) {
                ConnectFailureKind::Permanent
            } else {
                ConnectFailureKind::Transient
            }
        }
        ConnectFailure::TokenRejected { .. } => ConnectFailureKind::Permanent,
        ConnectFailure::HttpStatus { status, body } => {
            if *status == 401 || *status == 403 {
                return ConnectFailureKind::Permanent;
            }
            if body.contains("\"code\":100001") || body.contains("code\":100001") {
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
