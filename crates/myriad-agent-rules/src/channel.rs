//! Pure channel transport rules. No I/O.
//!
//! QQ C2C and Telegram DM share ingest, pairing replies, delivery plans, and
//! Transient/Permanent classification. Platform-specific parse helpers stay here
//! so workers only do HTTP.
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

/// Reply when this Telegram user id is already bound to a different site user.
pub const TELEGRAM_PAIRING_TAKEN_REPLY: &str =
    "这个 Telegram 号已经绑过别人。请先在原账号解除，或换一个号。";

/// Reply when this Discord user id is already bound to a different site user.
pub const DISCORD_PAIRING_TAKEN_REPLY: &str =
    "这个 Discord 号已经绑过别人。请先在原账号解除，或换一个号。";

/// Reply when this Feishu open id is already bound to a different site user.
pub const FEISHU_PAIRING_TAKEN_REPLY: &str =
    "这个飞书号已经绑过别人。请先在原账号解除，或换一个号。";

/// Telegram `sendMessage` text cap. Counted after entity parse; first cut
/// sends plain text so Unicode scalars are the conservative bound.
pub const TELEGRAM_TEXT_LIMIT: usize = 4096;

/// Reply when confirmation or a browser-only page action arrives on QQ.
pub const PANEL_REQUIRED_REPLY: &str = "请到站点面板完成这一步。";

/// Reply when a pending prompt cannot be parsed.
pub const PENDING_REASK_REPLY: &str = "没看懂。请回复选项编号，或按提示回答。";

/// Reply when a confirmation or question has expired.
pub const PENDING_EXPIRED_REPLY: &str = "这一步已经过期。请重新发一句，或到站点面板继续。";

/// Reply when a button belongs to an older question.
pub const PENDING_STALE_REPLY: &str = "这一步已经换过了。请回答上面最新的问题。";

/// Reply after a stop command cancels the current Work turn.
pub const CHANNEL_STOP_REPLY: &str = "已停止当前办事。再发一句就是新的请求。";

/// Reply after opening a fresh Work session on this chat.
pub const CHANNEL_NEW_SESSION_REPLY: &str = "已开新对话。之前的待答作废。";

/// Reply to `/start` / `/help`. Telegram sends `/start` on its own when a
/// person opens the bot; it must not reach the Planner as Work input.
pub const CHANNEL_HELP_REPLY: &str = "在这里发文字或图片，就是在站点办事，结果回到这个聊天。\n「当前任务」看进度；「停止」取消正在办的事；「新对话」重开一段。\n站点面板才能做的一步，会告诉你去哪继续。";

/// One-line notice when a multi-step task starts on a chat that has no
/// typing indicator. Without it a long task looks dead.
pub fn task_started_reply(total_steps: u32) -> String {
    if total_steps > 1 {
        format!("已开始办事，共 {total_steps} 步，完成后在这里回复。发「当前任务」看进度，「停止」取消。")
    } else {
        "已开始办事，完成后在这里回复。发「当前任务」看进度，「停止」取消。".to_string()
    }
}

/// First-cut QQ C2C text cap. Conservative so a long result can be split.
pub const QQ_TEXT_LIMIT: usize = 2000;

/// Discord `content` cap. Official Create Message limit.
pub const DISCORD_TEXT_LIMIT: usize = 2000;

/// Feishu text-message cap. The API caps the whole request body at 150 KB;
/// this conservative scalar bound keeps a split result well under it.
pub const FEISHU_TEXT_LIMIT: usize = 4000;

/// Feishu events pushed over the long-connection WebSocket.
pub const FEISHU_MESSAGE_RECEIVE_V1: &str = "im.message.receive_v1";
pub const FEISHU_CARD_ACTION_TRIGGER: &str = "card.action.trigger";

/// Shared inbound/outbound image cap for private-chat Work.
pub const CHANNEL_IMAGE_LIMIT: usize = 4;

/// Discord channel type: 1:1 DM. Only this type is ingested.
/// <https://discord.com/developers/docs/resources/channel#channel-object-channel-types>
pub const DISCORD_CHANNEL_TYPE_DM: i64 = 1;

/// Discord channel type: Group DM. Also arrives on `DIRECT_MESSAGES` intent.
pub const DISCORD_CHANNEL_TYPE_GROUP_DM: i64 = 3;

/// `DIRECT_MESSAGES` intent. First-cut Identify only sends this bit.
/// <https://discord.com/developers/docs/topics/gateway#gateway-intents>
pub const DISCORD_DIRECT_MESSAGES: u32 = 1 << 12;

/// How to answer a yes/no confirmation in chat.
pub const CONFIRM_HINT: &str = "回复「是」确认，或「否」取消。";

/// Marker used when a clarify resume used to be concatenated into one user turn.
/// New turns must not prepend this; strip it if an old pending still has it.
const CLARIFY_FOLLOWUP_MARK: &str = "\n补充说明：";

/// Inline button that opens Telegram's reply composer for free-text pending.
pub const TELEGRAM_INPUT_BUTTON: &str = "输入";

/// `callback_data` for [`TELEGRAM_INPUT_BUTTON`].
pub const TELEGRAM_CALLBACK_INPUT: &str = "i";

/// `callback_data` for confirmation yes.
pub const TELEGRAM_CALLBACK_YES: &str = "y";

/// `callback_data` for confirmation no.
pub const TELEGRAM_CALLBACK_NO: &str = "n";

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

/// Telegram DM: text and images in, final text plus images out, numbered options plus inline buttons.
/// Typing is a transport hint (`sendChatAction`), not a capability bit.
/// Edit / streaming draft stay off.
pub fn telegram_dm_capabilities() -> ChannelCapabilities {
    ChannelCapabilities {
        inbound_text: true,
        inbound_media: true,
        inbound_callback: true,
        outbound_final_text: true,
        outbound_markdown: false,
        outbound_image: true,
        outbound_edit: false,
        outbound_streaming_draft: false,
        interactive: true,
        frontend_action: false,
        performance: false,
        outfit: false,
    }
}

/// Discord DM: text and images in, final text plus images out, numbered options plus component buttons.
/// Typing is `POST /channels/{id}/typing` (10s), not a capability bit.
/// Edit / streaming draft stay off. Message Content Intent is not required.
pub fn discord_dm_capabilities() -> ChannelCapabilities {
    telegram_dm_capabilities()
}

/// C2C: text and images in, final text plus images out, numbered options plus yes/no. No buttons.
pub fn qq_c2c_capabilities() -> ChannelCapabilities {
    ChannelCapabilities {
        inbound_text: true,
        inbound_media: true,
        inbound_callback: false,
        outbound_final_text: true,
        outbound_markdown: false,
        outbound_image: true,
        outbound_edit: false,
        outbound_streaming_draft: false,
        interactive: true,
        frontend_action: false,
        performance: false,
        outfit: false,
    }
}

/// Feishu p2p: text and images in, final text plus images out, numbered options
/// plus interactive-card buttons. There is no typing indicator; edit / streaming
/// draft stay off.
pub fn feishu_dm_capabilities() -> ChannelCapabilities {
    ChannelCapabilities {
        inbound_text: true,
        inbound_media: true,
        inbound_callback: true,
        outbound_final_text: true,
        outbound_markdown: false,
        outbound_image: true,
        outbound_edit: false,
        outbound_streaming_draft: false,
        interactive: true,
        frontend_action: false,
        performance: false,
        outfit: false,
    }
}

/// Whether this chat can finish a mapped event without sending the person
/// to the site panel. `frontend_action` is never completable here.
pub fn channel_can_finish(caps: &ChannelCapabilities, event: &ChannelEvent) -> bool {
    match event {
        ChannelEvent::FrontendAction => false,
        ChannelEvent::ConfirmationRequired => caps.interactive,
        ChannelEvent::Answer { .. } | ChannelEvent::Error { .. } => caps.outbound_final_text,
        ChannelEvent::ThinkingToken
        | ChannelEvent::StepStarted
        | ChannelEvent::StepCompleted
        | ChannelEvent::Progress
        | ChannelEvent::TaskStarted { .. } => true,
    }
}

/// Visible panel entry when the chat window cannot finish the step.
pub fn panel_entry_reply(session_id: Option<&str>, task_id: Option<&str>) -> String {
    match (
        session_id.filter(|id| !id.is_empty()),
        task_id.filter(|id| !id.is_empty()),
    ) {
        (Some(session), Some(task)) => {
            format!("请到站点打开这次办事继续。会话 {session}，任务 {task}。")
        }
        (Some(session), None) => format!("请到站点打开这次办事继续。会话 {session}。"),
        (None, Some(task)) => format!("请到站点打开这次办事继续。任务 {task}。"),
        (None, None) => PANEL_REQUIRED_REPLY.to_string(),
    }
}

/// Resume only delivers envelopes after this sequence. History replay stays
/// on the same run; already-sent confirmations must not go out again.
pub fn should_deliver_sequence(last_delivered: Option<u64>, sequence: u64) -> bool {
    last_delivered.is_none_or(|seen| sequence > seen)
}

/// Private-chat command. Exact token after trim; extra words stay ordinary Work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelCommand {
    Stop,
    NewConversation,
    Status,
    Help,
}

/// Recognize stop / new conversation / current-task / help. Case-insensitive ASCII.
pub fn parse_channel_command(text: &str) -> Option<ChannelCommand> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let folded = trimmed.to_ascii_lowercase();
    match folded.as_str() {
        "停止" | "/stop" | "stop" => Some(ChannelCommand::Stop),
        "新对话" | "/new" | "new" => Some(ChannelCommand::NewConversation),
        "当前任务" | "查看当前任务" | "/status" | "status" => {
            Some(ChannelCommand::Status)
        }
        "帮助" | "/help" | "help" | "/start" | "start" => Some(ChannelCommand::Help),
        _ => None,
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

/// One inbound image the worker can download and cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelImageRef {
    pub url: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
}

/// Inbound C2C text after Gateway decoding. Images are platform URLs, not bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundC2cText {
    pub msg_id: String,
    pub user_openid: String,
    pub content: String,
    pub images: Vec<ChannelImageRef>,
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
    pairing_bind_reply_for(result, "qq")
}

/// Pairing consume reply for a channel. Only the taken-openid copy differs.
pub fn pairing_bind_reply_for(result: PairingBindResult, platform: &str) -> &'static str {
    match result {
        PairingBindResult::Bound { .. } => PAIRING_OK_REPLY,
        PairingBindResult::InvalidOrExpired => PAIRING_INVALID_REPLY,
        PairingBindResult::OpenidTaken if platform == "telegram" => TELEGRAM_PAIRING_TAKEN_REPLY,
        PairingBindResult::OpenidTaken if platform == "discord" || platform == "discord_dm" => {
            DISCORD_PAIRING_TAKEN_REPLY
        }
        PairingBindResult::OpenidTaken if platform == "feishu" => FEISHU_PAIRING_TAKEN_REPLY,
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
    ingest_channel_text(event, pairing, already_seen, "qq", &event.user_openid)
}

/// Same decision tree as [`ingest_c2c_text`], with an explicit platform and
/// session chat id. Telegram uses `update_id` as `msg_id` and `chat.id` here.
pub fn ingest_channel_text(
    event: &InboundC2cText,
    pairing: PairingLookup,
    already_seen: bool,
    platform: &str,
    chat_id: &str,
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
            session_key: session_key(platform, chat_id),
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
    /// A multi-step task was created; the turn is still running.
    TaskStarted {
        total_steps: u32,
    },
    Answer {
        message: String,
        image_urls: Vec<String>,
    },
    Error {
        message: String,
    },
    ConfirmationRequired,
    FrontendAction,
}

/// Passive window leftover from the inbound C2C message, plus whether the
/// transport can show a typing indicator while the turn runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryContext {
    pub inbound_msg_id: Option<String>,
    pub passive_window_open: bool,
    pub remaining_passive_replies: u8,
    pub typing: bool,
}

/// What to send, swallow, or fail visibly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryPlan {
    Drop,
    PassiveText {
        content: String,
        msg_id: String,
        image_urls: Vec<String>,
    },
    ActiveText {
        content: String,
        image_urls: Vec<String>,
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

fn send_text(content: String, image_urls: Vec<String>, ctx: &DeliveryContext) -> DeliveryPlan {
    if can_reply_passively(ctx) {
        DeliveryPlan::PassiveText {
            content,
            msg_id: ctx.inbound_msg_id.clone().expect("checked"),
            image_urls,
        }
    } else {
        DeliveryPlan::ActiveText {
            content,
            image_urls,
        }
    }
}

/// Map a finished-turn event onto a delivery plan. Process events are dropped;
/// a task start is one short notice only where typing cannot stand in for it.
/// Images ride with the final answer; confirmation / frontend-action failures stay text.
pub fn plan_delivery(event: &ChannelEvent, ctx: &DeliveryContext) -> DeliveryPlan {
    match event {
        ChannelEvent::ThinkingToken
        | ChannelEvent::StepStarted
        | ChannelEvent::StepCompleted
        | ChannelEvent::Progress => DeliveryPlan::Drop,
        ChannelEvent::TaskStarted { total_steps } => {
            if ctx.typing {
                DeliveryPlan::Drop
            } else {
                send_text(task_started_reply(*total_steps), Vec::new(), ctx)
            }
        }
        ChannelEvent::Answer {
            message,
            image_urls,
        } => send_text(message.clone(), image_urls.clone(), ctx),
        ChannelEvent::Error { message } => send_text(message.clone(), Vec::new(), ctx),
        ChannelEvent::ConfirmationRequired | ChannelEvent::FrontendAction => {
            let content = panel_entry_reply(None, None);
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

/// What the next inbound text should resume, after a prompt was sent.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PendingKind {
    Clarify {
        original_input: String,
    },
    Confirm {
        confirmation_id: String,
    },
    Answer {
        task_id: String,
        question_id: String,
        question_type: String,
    },
}

/// One visible choice. `value` is what resume APIs consume; `label` is what
/// the person sees.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PendingOption {
    pub value: String,
    pub label: String,
}

/// A question parked on a channel until the next inbound text.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PendingPrompt {
    /// Short id bound into buttons. Empty means a pre-id stored prompt.
    #[serde(default)]
    pub id: String,
    pub kind: PendingKind,
    pub question: String,
    pub options: Vec<PendingOption>,
    pub expires_at_unix: Option<i64>,
}

/// 8 hex chars from 4 bytes. Fits Telegram's 64-byte `callback_data`.
pub fn encode_pending_id(bytes: [u8; 4]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(8);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 15) as usize] as char);
    }
    out
}

/// Fill a missing prompt id from kind + question so old constructors stay valid.
pub fn ensure_pending_id(prompt: &mut PendingPrompt) {
    if !prompt.id.trim().is_empty() {
        return;
    }
    prompt.id = pending_id_from_parts(&prompt.kind, &prompt.question);
}

fn pending_id_from_parts(kind: &PendingKind, question: &str) -> String {
    let seed = match kind {
        PendingKind::Confirm { confirmation_id } => confirmation_id.as_str(),
        PendingKind::Answer { question_id, .. } => question_id.as_str(),
        PendingKind::Clarify { original_input } => original_input.as_str(),
    };
    let mut n: u32 = 0x811c_9dc5;
    for byte in seed.bytes().chain(question.bytes()) {
        n ^= u32::from(byte);
        n = n.wrapping_mul(0x0100_0193);
    }
    encode_pending_id(n.to_be_bytes())
}

/// How the next inbound text is consumed against a parked prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingDecision {
    Resume {
        kind: PendingKind,
        answer: String,
        confirmed: Option<bool>,
    },
    Reask {
        reply: String,
    },
    Expired {
        reply: String,
    },
}

/// Numbered options plus a yes/no hint for confirmation.
pub fn format_pending_prompt(prompt: &PendingPrompt) -> String {
    let mut lines = Vec::new();
    let question = prompt.question.trim();
    if !question.is_empty() {
        lines.push(question.to_string());
    }
    if !prompt.options.is_empty() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        for (index, option) in prompt.options.iter().enumerate() {
            let label = if option.label.trim().is_empty() {
                option.value.as_str()
            } else {
                option.label.as_str()
            };
            lines.push(format!("{}. {}", index + 1, label.trim()));
        }
    }
    if matches!(prompt_question_type(prompt), "confirmation" | "confirm") {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(CONFIRM_HINT.to_string());
    }
    if lines.is_empty() {
        PENDING_REASK_REPLY.to_string()
    } else {
        lines.join("\n")
    }
}

/// Parse the next inbound text against a parked prompt. Unknown text re-asks;
/// confirmation never defaults to yes.
pub fn decide_pending_reply(prompt: &PendingPrompt, text: &str, now_unix: i64) -> PendingDecision {
    if prompt
        .expires_at_unix
        .is_some_and(|expires| now_unix >= expires)
    {
        return PendingDecision::Expired {
            reply: PENDING_EXPIRED_REPLY.to_string(),
        };
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return reask(prompt);
    }
    match prompt_question_type(prompt) {
        "confirmation" | "confirm" => match parse_yes_no(trimmed) {
            Some(true) => PendingDecision::Resume {
                kind: prompt.kind.clone(),
                answer: "是".to_string(),
                confirmed: Some(true),
            },
            Some(false) => PendingDecision::Resume {
                kind: prompt.kind.clone(),
                answer: "否".to_string(),
                confirmed: Some(false),
            },
            None => reask(prompt),
        },
        "single_choice" => match match_one_option(prompt, trimmed) {
            Some(option) => PendingDecision::Resume {
                kind: prompt.kind.clone(),
                answer: option,
                confirmed: None,
            },
            None => reask(prompt),
        },
        "multiple_choice" => match match_many_options(prompt, trimmed) {
            Some(answer) => PendingDecision::Resume {
                kind: prompt.kind.clone(),
                answer,
                confirmed: None,
            },
            None => reask(prompt),
        },
        _ => {
            if !prompt.options.is_empty() {
                if let Some(option) = match_one_option(prompt, trimmed) {
                    return PendingDecision::Resume {
                        kind: prompt.kind.clone(),
                        answer: option,
                        confirmed: None,
                    };
                }
                return reask(prompt);
            }
            PendingDecision::Resume {
                kind: prompt.kind.clone(),
                answer: trimmed.to_string(),
                confirmed: None,
            }
        }
    }
}

fn prompt_question_type(prompt: &PendingPrompt) -> &str {
    match &prompt.kind {
        PendingKind::Confirm { .. } => "confirmation",
        PendingKind::Answer { question_type, .. } => question_type.as_str(),
        PendingKind::Clarify { .. } => {
            if prompt.options.is_empty() {
                "free_text"
            } else {
                "single_choice"
            }
        }
    }
}

/// First user turn of a clarify, without stacked `补充说明` tails.
pub fn clarify_base_input(text: &str) -> &str {
    text.split(CLARIFY_FOLLOWUP_MARK)
        .next()
        .unwrap_or(text)
        .trim()
}

/// Visible follow-up vs the parked original. The reply is the next user turn;
/// the original stays the first intent and is not concatenated again.
pub fn clarify_followup(original_input: &str, answer: &str) -> (String, String) {
    (
        answer.trim().to_string(),
        clarify_base_input(original_input).to_string(),
    )
}

fn reask(prompt: &PendingPrompt) -> PendingDecision {
    PendingDecision::Reask {
        reply: format_pending_prompt(prompt),
    }
}

/// One inline button. `callback_data` must stay within Telegram's 64-byte cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramInlineButton {
    pub text: String,
    pub callback_data: String,
}

/// What a private-chat callback should do against the parked prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelegramCallbackAction {
    Resume(String),
    RequestInput,
    Stale,
    Unknown,
}

/// Inline keyboard (and optional ForceReply) for a parked prompt.
///
/// Choice and confirm become `callback_data` buttons. Free text gets an
/// 「输入」 button; the caller sends `ForceReply` when that is pressed.
pub fn telegram_reply_markup(prompt: &PendingPrompt) -> Option<serde_json::Value> {
    let rows = telegram_inline_keyboard(prompt);
    if rows.is_empty() {
        return None;
    }
    Some(serde_json::json!({
        "inline_keyboard": rows
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|button| {
                        serde_json::json!({
                            "text": button.text,
                            "callback_data": button.callback_data,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    }))
}

/// Discord Action Rows for a parked prompt. Same `custom_id` encoding as
/// Telegram `callback_data` so [`telegram_callback_action`] can consume it.
pub fn discord_reply_markup(prompt: &PendingPrompt) -> Option<serde_json::Value> {
    let rows = telegram_inline_keyboard(prompt);
    if rows.is_empty() {
        return None;
    }
    Some(serde_json::Value::Array(
        rows.into_iter()
            .map(|row| {
                serde_json::json!({
                    "type": 1,
                    "components": row
                        .into_iter()
                        .map(|button| {
                            let style = match button.callback_data.split(':').next() {
                                Some(TELEGRAM_CALLBACK_YES) => 3,
                                Some(TELEGRAM_CALLBACK_NO) => 4,
                                _ => 2,
                            };
                            serde_json::json!({
                                "type": 2,
                                "style": style,
                                "label": button.text,
                                "custom_id": button.callback_data,
                            })
                        })
                        .collect::<Vec<_>>(),
                })
            })
            .collect(),
    ))
}

/// Feishu interactive-card elements for a parked prompt. Same callback encoding
/// as Telegram `callback_data`, wrapped in `value.data` so
/// [`telegram_callback_action`] can consume it.
pub fn feishu_reply_markup(prompt: &PendingPrompt) -> Option<serde_json::Value> {
    let rows = telegram_inline_keyboard(prompt);
    if rows.is_empty() {
        return None;
    }
    let elements = rows
        .into_iter()
        .map(|row| {
            let buttons: Vec<serde_json::Value> = row
                .into_iter()
                .map(|button| {
                    serde_json::json!({
                        "tag": "button",
                        "text": {
                            "tag": "plain_text",
                            "content": button.text,
                        },
                        "value": { "data": button.callback_data },
                    })
                })
                .collect();
            if buttons.len() == 1 {
                buttons.into_iter().next().expect("single button")
            } else {
                serde_json::json!({ "tag": "action", "actions": buttons })
            }
        })
        .collect::<Vec<_>>();
    Some(serde_json::json!({ "elements": elements }))
}

/// Force the reply composer. Placeholder is capped at 64 characters.
pub fn telegram_force_reply_markup(placeholder: &str) -> serde_json::Value {
    let placeholder: String = placeholder.chars().take(64).collect();
    if placeholder.is_empty() {
        serde_json::json!({ "force_reply": true })
    } else {
        serde_json::json!({
            "force_reply": true,
            "input_field_placeholder": placeholder,
        })
    }
}

/// Button rows for a parked prompt. Confirm is yes/no; options are one per row.
/// `callback_data` always includes the prompt id so an old button cannot
/// answer a newer question.
pub fn telegram_inline_keyboard(prompt: &PendingPrompt) -> Vec<Vec<TelegramInlineButton>> {
    let mut prompt = prompt.clone();
    ensure_pending_id(&mut prompt);
    let id = prompt.id.as_str();
    match prompt_question_type(&prompt) {
        "confirmation" | "confirm" => vec![vec![
            TelegramInlineButton {
                text: "是".to_string(),
                callback_data: format!("{TELEGRAM_CALLBACK_YES}:{id}"),
            },
            TelegramInlineButton {
                text: "否".to_string(),
                callback_data: format!("{TELEGRAM_CALLBACK_NO}:{id}"),
            },
        ]],
        _ if !prompt.options.is_empty() => prompt
            .options
            .iter()
            .enumerate()
            .map(|(index, option)| {
                let label = if option.label.trim().is_empty() {
                    option.value.as_str()
                } else {
                    option.label.as_str()
                };
                vec![TelegramInlineButton {
                    text: truncate_button_label(label),
                    callback_data: format!("o:{id}:{index}"),
                }]
            })
            .collect(),
        _ => vec![vec![TelegramInlineButton {
            text: TELEGRAM_INPUT_BUTTON.to_string(),
            callback_data: format!("{TELEGRAM_CALLBACK_INPUT}:{id}"),
        }]],
    }
}

/// Parsed private-chat callback. `prompt_id` is empty for legacy unbound data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramCallbackBinding {
    pub prompt_id: String,
    pub kind: TelegramBoundKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelegramBoundKind {
    Yes,
    No,
    Input,
    Option(usize),
}

/// Parse `callback_data`. Bound form is `y:{id}` / `n:{id}` / `i:{id}` / `o:{id}:{index}`.
/// Legacy `y` / `n` / `i` / `o:0` stay parseable so they can be rejected as stale.
pub fn parse_telegram_callback(data: &str) -> Option<TelegramCallbackBinding> {
    let data = data.trim();
    if data.is_empty() {
        return None;
    }
    if let Some(rest) = data.strip_prefix("o:") {
        if let Some((id, index)) = rest.rsplit_once(':') {
            if !id.is_empty() && id != TELEGRAM_CALLBACK_YES && id != TELEGRAM_CALLBACK_NO {
                let index = index.parse::<usize>().ok()?;
                return Some(TelegramCallbackBinding {
                    prompt_id: id.to_string(),
                    kind: TelegramBoundKind::Option(index),
                });
            }
        }
        let index = rest.parse::<usize>().ok()?;
        return Some(TelegramCallbackBinding {
            prompt_id: String::new(),
            kind: TelegramBoundKind::Option(index),
        });
    }
    if let Some((action, id)) = data.split_once(':') {
        let kind = match action {
            TELEGRAM_CALLBACK_YES => TelegramBoundKind::Yes,
            TELEGRAM_CALLBACK_NO => TelegramBoundKind::No,
            TELEGRAM_CALLBACK_INPUT => TelegramBoundKind::Input,
            _ => return None,
        };
        return Some(TelegramCallbackBinding {
            prompt_id: id.to_string(),
            kind,
        });
    }
    let kind = match data {
        TELEGRAM_CALLBACK_YES => TelegramBoundKind::Yes,
        TELEGRAM_CALLBACK_NO => TelegramBoundKind::No,
        TELEGRAM_CALLBACK_INPUT => TelegramBoundKind::Input,
        _ => return None,
    };
    Some(TelegramCallbackBinding {
        prompt_id: String::new(),
        kind,
    })
}

/// Map `callback_data` onto the parked prompt. A missing or other id is stale.
pub fn telegram_callback_action(prompt: &PendingPrompt, data: &str) -> TelegramCallbackAction {
    let Some(binding) = parse_telegram_callback(data) else {
        return TelegramCallbackAction::Unknown;
    };
    let mut prompt = prompt.clone();
    ensure_pending_id(&mut prompt);
    if binding.prompt_id != prompt.id {
        return TelegramCallbackAction::Stale;
    }
    match binding.kind {
        TelegramBoundKind::Input => TelegramCallbackAction::RequestInput,
        TelegramBoundKind::Yes => TelegramCallbackAction::Resume("是".to_string()),
        TelegramBoundKind::No => TelegramCallbackAction::Resume("否".to_string()),
        TelegramBoundKind::Option(index) => prompt
            .options
            .get(index)
            .map(|option| TelegramCallbackAction::Resume(option.value.clone()))
            .unwrap_or(TelegramCallbackAction::Unknown),
    }
}

/// Lift a model JSON blob in `message` into a parked clarify prompt.
///
/// Only succeeds when at least one option is present. Unknown wrappers
/// (`intent`, `clarifications_needed`) are read; they are not a contract.
pub fn pending_prompt_from_model_json(text: &str, original_input: &str) -> Option<PendingPrompt> {
    let value = extract_json_object(text)?;
    let (question, options) = clarification_fields(&value);
    if options.is_empty() {
        return None;
    }
    let question = if question.is_empty() {
        "请选择：".to_string()
    } else {
        question
    };
    let mut prompt = PendingPrompt {
        id: String::new(),
        kind: PendingKind::Clarify {
            original_input: clarify_base_input(original_input).to_string(),
        },
        question,
        options,
        expires_at_unix: None,
    };
    ensure_pending_id(&mut prompt);
    Some(prompt)
}

fn extract_json_object(text: &str) -> Option<serde_json::Value> {
    let trimmed = text.trim();
    let start = trimmed.find('{')?;
    if start > 0 && !trimmed[..start].trim().is_empty() && !trimmed.starts_with("```") {
        return None;
    }
    let end = trimmed.rfind('}')?;
    if end < start {
        return None;
    }
    let parsed = serde_json::from_str::<serde_json::Value>(&trimmed[start..=end]).ok()?;
    parsed.as_object().is_some().then_some(parsed)
}

fn clarification_fields(value: &serde_json::Value) -> (String, Vec<PendingOption>) {
    let needed = value
        .get("clarifications_needed")
        .and_then(|row| row.as_array())
        .and_then(|rows| rows.first());
    let question = needed
        .and_then(|row| row.get("question"))
        .and_then(|row| row.as_str())
        .or_else(|| value.get("question").and_then(|row| row.as_str()))
        .or_else(|| {
            value
                .get("clarification")
                .and_then(|row| row.get("message"))
                .and_then(|row| row.as_str())
        })
        .or_else(|| value.get("message").and_then(|row| row.as_str()))
        .unwrap_or("")
        .trim()
        .to_string();
    let mut options = json_choice_options(needed.and_then(|row| row.get("suggestions")));
    if options.is_empty() {
        options = json_choice_options(needed.and_then(|row| row.get("options")));
    }
    if options.is_empty() {
        options = json_choice_options(value.get("suggestions"));
    }
    if options.is_empty() {
        options = json_choice_options(value.get("options"));
    }
    if options.is_empty() {
        options = json_choice_options(
            value
                .get("clarification")
                .and_then(|row| row.get("options")),
        );
    }
    (question, options)
}

fn json_choice_options(value: Option<&serde_json::Value>) -> Vec<PendingOption> {
    value
        .and_then(|row| row.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let text = row.as_str().or_else(|| {
                        row.get("label")
                            .and_then(|item| item.as_str())
                            .or_else(|| row.get("value").and_then(|item| item.as_str()))
                    })?;
                    let trimmed = text.trim();
                    (!trimmed.is_empty()).then(|| PendingOption {
                        value: trimmed.to_string(),
                        label: trimmed.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn truncate_button_label(label: &str) -> String {
    let trimmed = label.trim();
    let truncated: String = trimmed.chars().take(64).collect();
    if truncated.is_empty() {
        "选项".to_string()
    } else {
        truncated
    }
}

fn parse_yes_no(text: &str) -> Option<bool> {
    let folded = text.trim().to_ascii_lowercase();
    match folded.as_str() {
        "是" | "确认" | "同意" | "好的" | "好" | "yes" | "y" | "ok" => Some(true),
        "否" | "取消" | "不同意" | "不" | "no" | "n" => Some(false),
        _ => None,
    }
}

fn match_one_option(prompt: &PendingPrompt, text: &str) -> Option<String> {
    if let Some(index) = parse_option_index(text, prompt.options.len()) {
        return Some(prompt.options[index].value.clone());
    }
    let needle = normalize_choice(text);
    prompt.options.iter().find_map(|option| {
        let label = normalize_choice(&option.label);
        let value = normalize_choice(&option.value);
        (needle == label || needle == value).then(|| option.value.clone())
    })
}

fn match_many_options(prompt: &PendingPrompt, text: &str) -> Option<String> {
    let parts: Vec<&str> = text
        .split(|ch: char| ch == ',' || ch == '，' || ch.is_ascii_whitespace())
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        return None;
    }
    let mut values = Vec::new();
    for part in parts {
        let Some(value) = match_one_option(prompt, part) else {
            return None;
        };
        if !values.contains(&value) {
            values.push(value);
        }
    }
    (!values.is_empty()).then_some(values.join(","))
}

fn parse_option_index(text: &str, count: usize) -> Option<usize> {
    let cleaned = text
        .trim()
        .trim_end_matches(|ch: char| matches!(ch, '.' | '、' | ')' | '）'));
    let index: usize = cleaned.parse().ok()?;
    (index >= 1 && index <= count).then_some(index - 1)
}

fn normalize_choice(text: &str) -> String {
    text.trim().to_lowercase()
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

/// Telegram: switch on and a non-empty bot token → run.
pub fn telegram_worker_intent(enabled: bool, token: &str) -> WorkerIntent {
    worker_intent(enabled, token, !token.trim().is_empty())
}

/// Discord: switch on and a non-empty bot token → run.
pub fn discord_worker_intent(enabled: bool, token: &str) -> WorkerIntent {
    telegram_worker_intent(enabled, token)
}

/// Feishu: same shape as QQ — switch on, AppID, and AppSecret.
pub fn feishu_worker_intent(enabled: bool, app_id: &str, has_app_secret: bool) -> WorkerIntent {
    worker_intent(enabled, app_id, has_app_secret)
}

/// Tenant-token endpoint credential codes (Easybot Feishu adapter).
/// 99991663/64/65, 20013, 20005, 4001 are AppID / AppSecret rejects.
pub fn classify_feishu_token_code(code: i64) -> ConnectFailureKind {
    match code {
        99991663 | 99991664 | 99991665 | 20013 | 20005 | 4001 => ConnectFailureKind::Permanent,
        _ => ConnectFailureKind::Transient,
    }
}

/// Token expired / illegal on a later OpenAPI call. Refresh once, then retry.
/// 99991664 is app_access_token illegal — not tenant — so it is excluded.
pub fn feishu_token_needs_refresh(code: i64) -> bool {
    matches!(code, 99991663 | 99991665 | 20013 | 20005)
}

/// Parse `POST /open-apis/auth/v3/tenant_access_token/internal`.
/// Never returns the secret; errors are kinds only.
pub fn parse_feishu_tenant_token(
    status: u16,
    body: &str,
) -> Result<(String, u64), ConnectFailureKind> {
    if status == 0 || status >= 500 {
        return Err(ConnectFailureKind::Transient);
    }
    if status == 401 || status == 403 {
        return Err(ConnectFailureKind::Permanent);
    }
    if !(200..300).contains(&status) {
        return Err(ConnectFailureKind::Transient);
    }
    let data: serde_json::Value =
        serde_json::from_str(body).map_err(|_| ConnectFailureKind::Transient)?;
    let code = data.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 {
        return Err(classify_feishu_token_code(code));
    }
    let token = data
        .get("tenant_access_token")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(ConnectFailureKind::Permanent)?;
    let expire = data.get("expire").and_then(|v| v.as_u64()).unwrap_or(7200);
    Ok((token.to_string(), expire.max(1)))
}

/// Parse `POST /callback/ws/endpoint`. Returns `(ws_url, service_id, ping_interval)`.
pub fn parse_feishu_ws_endpoint(
    status: u16,
    body: &str,
) -> Result<(String, i32, u64), ConnectFailureKind> {
    if status == 0 || status >= 500 {
        return Err(ConnectFailureKind::Transient);
    }
    if status == 401 || status == 403 {
        return Err(ConnectFailureKind::Permanent);
    }
    if !(200..300).contains(&status) {
        return Err(ConnectFailureKind::Transient);
    }
    let data: serde_json::Value =
        serde_json::from_str(body).map_err(|_| ConnectFailureKind::Transient)?;
    let code = data.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 {
        return Err(classify_feishu_token_code(code));
    }
    let inner = data.get("data").ok_or(ConnectFailureKind::Transient)?;
    let url = inner
        .get("URL")
        .or_else(|| inner.get("url"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(ConnectFailureKind::Transient)?;
    let service_id = url
        .split('?')
        .nth(1)
        .and_then(|qs| {
            qs.split('&').find_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                (k == "service_id").then(|| v.parse::<i32>().ok()).flatten()
            })
        })
        .unwrap_or(0);
    let ping_interval = inner
        .get("ClientConfig")
        .or_else(|| inner.get("client_config"))
        .and_then(|cfg| cfg.get("PingInterval").or_else(|| cfg.get("ping_interval")))
        .and_then(|v| v.as_u64())
        .filter(|n| *n > 0)
        .unwrap_or(120);
    Ok((url.to_string(), service_id, ping_interval))
}

/// Handshake-Status on a non-101 WebSocket upgrade.
/// 403 and 514 (except connection-limit 1000040350) are credential / app-state.
pub fn classify_feishu_handshake(status: i32, auth_err_code: i32) -> ConnectFailureKind {
    match status {
        403 => ConnectFailureKind::Permanent,
        514 if auth_err_code == 1_000_040_350 => ConnectFailureKind::Transient,
        514 => ConnectFailureKind::Permanent,
        _ => ConnectFailureKind::Transient,
    }
}

/// Decode a long-connection event payload. Returns `(event_type, event_id, event)`.
pub fn parse_feishu_event_envelope(payload: &[u8]) -> Option<(String, String, serde_json::Value)> {
    let body: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let header = body.get("header")?;
    let event_type = header
        .get("event_type")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let event_id = header
        .get("event_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let event = body
        .get("event")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    Some((event_type, event_id, event))
}

/// OpenAPI JSON `code` after a send / upload / download. 0 is success.
pub fn parse_feishu_api_code(
    status: u16,
    body: &str,
) -> Result<serde_json::Value, ConnectFailureKind> {
    if status == 0 || status >= 500 {
        return Err(ConnectFailureKind::Transient);
    }
    if status == 401 || status == 403 {
        return Err(ConnectFailureKind::Permanent);
    }
    let data: serde_json::Value =
        serde_json::from_str(body).map_err(|_| ConnectFailureKind::Transient)?;
    let code = data.get("code").and_then(|v| v.as_i64()).unwrap_or(0);
    if code != 0 {
        // Token-endpoint credential codes stay Permanent. The same numbers on a
        // later OpenAPI call mean the cached tenant token expired — Transient
        // so the worker can refresh once.
        return Err(if feishu_token_needs_refresh(code) {
            ConnectFailureKind::Transient
        } else {
            classify_feishu_token_code(code)
        });
    }
    if !(200..300).contains(&status) {
        return Err(ConnectFailureKind::Transient);
    }
    Ok(data)
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
        4004 | 4010 | 4011 | 4012 | 4013 | 4014 | 4914 | 4915 => ConnectFailureKind::Permanent,
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
            if *status >= 500 || *status == 429 || *status == 409 {
                ConnectFailureKind::Transient
            } else if *status >= 400 {
                ConnectFailureKind::Permanent
            } else {
                ConnectFailureKind::Transient
            }
        }
    }
}

/// Private-chat text extracted from a Telegram `Update`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramPrivateText {
    pub update_id: i64,
    pub message_id: i64,
    pub from_id: i64,
    pub chat_id: i64,
    pub text: String,
    pub images: Vec<ChannelImageRef>,
}

/// Private-chat inline-button press. `data` is raw `callback_data`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramPrivateCallback {
    pub update_id: i64,
    pub message_id: i64,
    pub from_id: i64,
    pub chat_id: i64,
    pub callback_query_id: String,
    pub data: String,
}

/// Private-chat inbound after `getUpdates`. Groups and empty `from` drop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelegramPrivateInbound {
    Text(TelegramPrivateText),
    Callback(TelegramPrivateCallback),
}

impl TelegramPrivateText {
    pub fn inbound(&self) -> InboundC2cText {
        InboundC2cText {
            msg_id: self.update_id.to_string(),
            user_openid: self.from_id.to_string(),
            content: self.text.clone(),
            images: self.images.clone(),
        }
    }

    pub fn chat_id_key(&self) -> String {
        self.chat_id.to_string()
    }
}

impl TelegramPrivateCallback {
    pub fn chat_id_key(&self) -> String {
        self.chat_id.to_string()
    }

    pub fn msg_id(&self) -> String {
        self.update_id.to_string()
    }
}

/// Feishu p2p text after long-connection decode. `open_id` is preferred;
/// `user_id` is kept as an alias so mobile payloads that omit `open_id` still
/// match a pairing bound under the other id. Both missing drops the event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundFeishuText {
    pub event_id: String,
    pub message_id: String,
    pub open_id: String,
    pub identity_keys: Vec<String>,
    pub chat_id: String,
    pub content: String,
    pub images: Vec<ChannelImageRef>,
}

/// Feishu interactive-card button press. `data` is `action.value.data`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeishuCardCallback {
    pub event_id: String,
    pub open_id: String,
    pub identity_keys: Vec<String>,
    pub chat_id: String,
    pub data: String,
}

/// Pairing lookup keys for a Feishu sender. `open_id` first, then `user_id` if
/// it is a different non-empty value. Empty after trim is skipped.
pub fn feishu_identity_keys(open_id: Option<&str>, user_id: Option<&str>) -> Vec<String> {
    let mut keys = Vec::new();
    for raw in [open_id, user_id] {
        let Some(value) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
            continue;
        };
        if !keys.iter().any(|existing| existing == value) {
            keys.push(value.to_string());
        }
    }
    keys
}

fn json_trimmed_str<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

impl InboundFeishuText {
    pub fn inbound(&self) -> InboundC2cText {
        InboundC2cText {
            msg_id: self.message_id.clone(),
            user_openid: self.open_id.clone(),
            content: self.content.clone(),
            images: self.images.clone(),
        }
    }

    pub fn chat_id_key(&self) -> String {
        self.chat_id.clone()
    }
}

impl FeishuCardCallback {
    pub fn chat_id_key(&self) -> String {
        self.chat_id.clone()
    }

    pub fn msg_id(&self) -> String {
        self.event_id.clone()
    }
}

/// Text from a Feishu `text` message `content` (`{"text":"..."}`). Non-JSON or
/// missing `text` keeps the raw content so a malformed payload is not silent.
pub fn feishu_text_from_content(content: &str) -> String {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .and_then(|value| {
            value
                .get("text")
                .and_then(|text| text.as_str())
                .map(str::to_string)
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| content.to_string())
}

/// Image refs from a Feishu message. `image_key` is not a public URL — the
/// worker downloads via `/im/v1/images/{key}` before caching, so `url` carries
/// the `feishu:`-prefixed key.
fn parse_feishu_images(message: &serde_json::Value) -> Vec<ChannelImageRef> {
    let Some(content) = message.get("content").and_then(|value| value.as_str()) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) else {
        return Vec::new();
    };
    let Some(key) = parsed
        .get("image_key")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|key| !key.is_empty())
    else {
        return Vec::new();
    };
    vec![ChannelImageRef {
        url: format!("feishu:{key}"),
        name: "image.jpg".to_string(),
        mime: "image/jpeg".to_string(),
        size: 0,
    }]
}

/// Parse `im.message.receive_v1`. Drops bots, groups, unknown `message_type`,
/// and empty text without images. Only `text` and `image` enter Work. Missing
/// `open_id` falls back to `user_id`; both are kept on `identity_keys` so
/// pairing can alias them. `event_id` comes from the event header for dedup.
pub fn parse_feishu_message_receive(
    event_id: &str,
    event_data: &serde_json::Value,
) -> Option<InboundFeishuText> {
    let sender = event_data.get("sender")?;
    if sender.get("sender_type").and_then(|value| value.as_str()) == Some("app") {
        return None;
    }
    let sender_id = sender.get("sender_id")?;
    let identity_keys = feishu_identity_keys(
        json_trimmed_str(sender_id, "open_id"),
        json_trimmed_str(sender_id, "user_id"),
    );
    let open_id = identity_keys.first()?.clone();
    let message = event_data.get("message")?;
    if message.get("chat_type").and_then(|value| value.as_str()) != Some("p2p") {
        return None;
    }
    let message_id = message
        .get("message_id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let chat_id = message
        .get("chat_id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let msg_type = message
        .get("message_type")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let raw_content = message
        .get("content")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let content = match msg_type {
        "text" => feishu_text_from_content(raw_content),
        "image" => String::new(),
        _ => return None,
    };
    let images = if msg_type == "image" {
        parse_feishu_images(message)
    } else {
        Vec::new()
    };
    if content.trim().is_empty() && images.is_empty() {
        return None;
    }
    Some(InboundFeishuText {
        event_id: event_id.to_string(),
        message_id: message_id.to_string(),
        open_id,
        identity_keys,
        chat_id: chat_id.to_string(),
        content,
        images,
    })
}

/// Parse `card.action.trigger`. Drops payloads without operator, chat, or data.
/// Operator `open_id` is preferred; `user_id` is kept as an alias.
pub fn parse_feishu_card_callback(
    event_id: &str,
    event_data: &serde_json::Value,
) -> Option<FeishuCardCallback> {
    let operator = event_data.get("operator")?;
    let identity_keys = feishu_identity_keys(
        json_trimmed_str(operator, "open_id"),
        json_trimmed_str(operator, "user_id"),
    );
    let open_id = identity_keys.first()?.clone();
    let chat_id = event_data
        .get("context")
        .and_then(|value| value.get("open_chat_id"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let data = event_data
        .get("action")
        .and_then(|value| value.get("value"))
        .and_then(|value| value.get("data"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some(FeishuCardCallback {
        event_id: event_id.to_string(),
        open_id,
        identity_keys,
        chat_id: chat_id.to_string(),
        data: data.to_string(),
    })
}

/// Parse `getUpdates` / `getMe` / `sendMessage` JSON. Never returns the token.
pub fn parse_telegram_ok_payload(
    status: u16,
    body: &str,
) -> Result<serde_json::Value, ConnectFailureKind> {
    if let Some(kind) = telegram_failure_kind(status, body) {
        return Err(kind);
    }
    let data: serde_json::Value =
        serde_json::from_str(body).map_err(|_| ConnectFailureKind::Transient)?;
    if data.get("ok") != Some(&serde_json::Value::Bool(true)) {
        return Err(telegram_failure_kind(status, body).unwrap_or(ConnectFailureKind::Transient));
    }
    Ok(data
        .get("result")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

/// Public bot name from a `getMe` result. Never includes the token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramBotIdentity {
    pub id: i64,
    pub first_name: String,
    pub username: Option<String>,
}

pub fn parse_telegram_bot_identity(result: &serde_json::Value) -> Option<TelegramBotIdentity> {
    let id = result.get("id")?.as_i64()?;
    let first_name = result
        .get("first_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let username = result
        .get("username")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if first_name.is_empty() && username.is_none() {
        return None;
    }
    Some(TelegramBotIdentity {
        id,
        first_name,
        username,
    })
}

/// Private-chat texts from a `getUpdates` body. Groups, edits, and empty `from` drop.
pub fn parse_telegram_private_texts(
    status: u16,
    body: &str,
) -> Result<Vec<TelegramPrivateText>, ConnectFailureKind> {
    Ok(parse_telegram_private_inbounds(status, body)?
        .into_iter()
        .filter_map(|inbound| match inbound {
            TelegramPrivateInbound::Text(text) => Some(text),
            TelegramPrivateInbound::Callback(_) => None,
        })
        .collect())
}

/// Private-chat texts and callback presses from a `getUpdates` body.
pub fn parse_telegram_private_inbounds(
    status: u16,
    body: &str,
) -> Result<Vec<TelegramPrivateInbound>, ConnectFailureKind> {
    let result = parse_telegram_ok_payload(status, body)?;
    let updates = result.as_array().cloned().unwrap_or_default();
    Ok(updates
        .iter()
        .filter_map(telegram_private_inbound_from_update)
        .collect())
}

fn telegram_private_inbound_from_update(
    update: &serde_json::Value,
) -> Option<TelegramPrivateInbound> {
    if let Some(text) = telegram_private_text_from_update(update) {
        return Some(TelegramPrivateInbound::Text(text));
    }
    telegram_private_callback_from_update(update).map(TelegramPrivateInbound::Callback)
}

fn telegram_private_callback_from_update(
    update: &serde_json::Value,
) -> Option<TelegramPrivateCallback> {
    let update_id = json_i64(update.get("update_id")?)?;
    let query = update.get("callback_query")?;
    let from = query.get("from")?;
    let from_id = json_i64(from.get("id")?)?;
    let message = query.get("message")?;
    let chat = message.get("chat")?;
    if chat.get("type").and_then(|value| value.as_str()) != Some("private") {
        return None;
    }
    let chat_id = json_i64(chat.get("id")?)?;
    let message_id = json_i64(message.get("message_id")?)?;
    let callback_query_id = query.get("id")?.as_str()?.to_string();
    if callback_query_id.is_empty() {
        return None;
    }
    let data = query.get("data")?.as_str()?.to_string();
    if data.is_empty() {
        return None;
    }
    Some(TelegramPrivateCallback {
        update_id,
        message_id,
        from_id,
        chat_id,
        callback_query_id,
        data,
    })
}

fn telegram_private_text_from_update(update: &serde_json::Value) -> Option<TelegramPrivateText> {
    let update_id = json_i64(update.get("update_id")?)?;
    let message = update.get("message")?;
    let chat = message.get("chat")?;
    if chat.get("type").and_then(|value| value.as_str()) != Some("private") {
        return None;
    }
    let from = message.get("from")?;
    let from_id = json_i64(from.get("id")?)?;
    let chat_id = json_i64(chat.get("id")?)?;
    let images = parse_telegram_photos(message);
    let text = message
        .get("text")
        .or_else(|| message.get("caption"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    if text.trim().is_empty() && images.is_empty() {
        return None;
    }
    let message_id = json_i64(message.get("message_id")?)?;
    Some(TelegramPrivateText {
        update_id,
        message_id,
        from_id,
        chat_id,
        text,
        images,
    })
}

fn parse_telegram_photos(message: &serde_json::Value) -> Vec<ChannelImageRef> {
    let Some(photos) = message.get("photo").and_then(|value| value.as_array()) else {
        return Vec::new();
    };
    let Some(largest) = photos.iter().max_by_key(|photo| {
        photo.get("width").and_then(json_i64).unwrap_or(0)
            * photo.get("height").and_then(json_i64).unwrap_or(0)
    }) else {
        return Vec::new();
    };
    let Some(file_id) = largest
        .get("file_id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Vec::new();
    };
    vec![ChannelImageRef {
        url: format!("tg:{file_id}"),
        name: "photo.jpg".to_string(),
        mime: "image/jpeg".to_string(),
        size: largest.get("file_size").and_then(json_u64).unwrap_or(0),
    }]
}

/// `getFile` result `file_path`. Empty or non-ok → Transient.
pub fn parse_telegram_file_path(status: u16, body: &str) -> Result<String, ConnectFailureKind> {
    let result = parse_telegram_ok_payload(status, body)?;
    result
        .get("file_path")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.contains(".."))
        .map(str::to_string)
        .ok_or(ConnectFailureKind::Transient)
}

/// `parameters.retry_after` seconds on 429. Missing → none, caller uses default backoff.
pub fn telegram_retry_after(body: &str) -> Option<u64> {
    let data: serde_json::Value = serde_json::from_str(body).ok()?;
    data.get("parameters")
        .and_then(|value| value.get("retry_after"))
        .and_then(json_u64)
}

/// Largest `update_id` in a successful `getUpdates` body, including dropped types.
pub fn telegram_max_update_id(status: u16, body: &str) -> Result<Option<i64>, ConnectFailureKind> {
    let result = parse_telegram_ok_payload(status, body)?;
    let updates = result.as_array().cloned().unwrap_or_default();
    Ok(updates
        .iter()
        .filter_map(|update| update.get("update_id").and_then(json_i64))
        .max())
}

/// First-cut outbound: trim to [`TELEGRAM_TEXT_LIMIT`] Unicode scalars.
pub fn truncate_telegram_text(text: &str) -> String {
    split_channel_text(text, TELEGRAM_TEXT_LIMIT)
        .into_iter()
        .next()
        .unwrap_or_default()
}

/// First-cut outbound: trim to [`FEISHU_TEXT_LIMIT`] Unicode scalars.
pub fn truncate_feishu_text(text: &str) -> String {
    split_channel_text(text, FEISHU_TEXT_LIMIT)
        .into_iter()
        .next()
        .unwrap_or_default()
}

/// Split on paragraph / line / word boundaries so a long result is complete.
pub fn split_channel_text(text: &str, limit: usize) -> Vec<String> {
    if limit == 0 {
        return if text.is_empty() {
            Vec::new()
        } else {
            vec![text.to_string()]
        };
    }
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if trimmed.chars().count() <= limit {
        return vec![trimmed.to_string()];
    }
    let chars: Vec<char> = trimmed.chars().collect();
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        if chars.len() - start <= limit {
            let rest: String = chars[start..].iter().collect();
            let rest = rest.trim().to_string();
            if !rest.is_empty() {
                chunks.push(rest);
            }
            break;
        }
        let mut end = start + limit;
        let window = &chars[start..end];
        if let Some(rel) = window.iter().rposition(|ch| *ch == '\n') {
            if rel > 0 {
                end = start + rel;
            }
        } else if let Some(rel) = window.iter().rposition(|ch| ch.is_whitespace()) {
            if rel > 0 {
                end = start + rel;
            }
        }
        let chunk: String = chars[start..end].iter().collect();
        let chunk = chunk.trim().to_string();
        if !chunk.is_empty() {
            chunks.push(chunk);
        }
        start = end;
        while start < chars.len() && chars[start].is_whitespace() {
            start += 1;
        }
    }
    if chunks.is_empty() {
        vec![trimmed.to_string()]
    } else {
        chunks
    }
}

/// Render `message` plus independent `data` / `dataDisplay` as readable text.
pub fn format_channel_result(
    message: &str,
    data: Option<&serde_json::Value>,
    data_display: Option<&serde_json::Value>,
) -> String {
    let message = message.trim();
    // Chat answers already put the user-facing text in `message` (and mirror it
    // under `data.reply`). Dumping the envelope would produce:
    //   pong
    //
    //   reply：pong
    //   type：chat
    let message = if message.is_empty() {
        chat_envelope_reply(data).unwrap_or("")
    } else {
        message
    };
    let extra = format_data_display(data, data_display);
    match (message.is_empty(), extra.is_empty()) {
        (true, true) => String::new(),
        (false, true) => message.to_string(),
        (true, false) => extra,
        (false, false) => format!("{message}\n\n{extra}"),
    }
}

/// Site chat payload: `{ "reply": "...", "type": "chat", ... }`. Not a table.
fn chat_envelope_reply(data: Option<&serde_json::Value>) -> Option<&str> {
    let obj = data?.as_object()?;
    if obj.get("type").and_then(|value| value.as_str()) != Some("chat") {
        return None;
    }
    obj.get("reply")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn format_data_display(
    data: Option<&serde_json::Value>,
    display: Option<&serde_json::Value>,
) -> String {
    // Do not pretty-print chat envelopes — `message` / `data.reply` already hold
    // the line the user should see.
    if chat_envelope_reply(data).is_some()
        || data
            .and_then(|value| value.get("type"))
            .and_then(|value| value.as_str())
            == Some("chat")
    {
        return String::new();
    }
    let Some(display) = display else {
        return format_value_preview(data, 12);
    };
    let display_type = display
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let rows = display_rows(data, display);
    match display_type {
        "table" => format_table(display, &rows),
        "chart" => format_chart(display, &rows),
        "card_list" => format_card_list(display, &rows),
        "timeline" => format_timeline(display, &rows),
        "key_value" => format_key_value(&rows, data),
        "markdown" => data
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .unwrap_or("")
            .to_string(),
        "raw" => format_value_preview(data, 20),
        _ => format_value_preview(data, 12),
    }
}

fn display_rows<'a>(
    data: Option<&'a serde_json::Value>,
    display: &serde_json::Value,
) -> Vec<&'a serde_json::Value> {
    let Some(data) = data else {
        return Vec::new();
    };
    if let Some(path) = display.get("dataPath").and_then(|value| value.as_str()) {
        if let Some(found) = json_path(data, path).and_then(|value| value.as_array()) {
            return found.iter().collect();
        }
    }
    match data {
        serde_json::Value::Array(rows) => rows.iter().collect(),
        serde_json::Value::Object(map) => {
            for key in ["items", "rows", "data", "list", "records"] {
                if let Some(serde_json::Value::Array(rows)) = map.get(key) {
                    return rows.iter().collect();
                }
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn json_path<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for part in path.split('.').filter(|part| !part.is_empty()) {
        current = current.get(part)?;
    }
    Some(current)
}

fn format_table(display: &serde_json::Value, rows: &[&serde_json::Value]) -> String {
    let columns = display
        .get("columns")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    if columns.is_empty() {
        return format_value_preview(
            Some(&serde_json::Value::Array(
                rows.iter().map(|row| (*row).clone()).collect(),
            )),
            16,
        );
    }
    let headers: Vec<String> = columns
        .iter()
        .map(|column| {
            column
                .get("title")
                .or_else(|| column.get("field"))
                .and_then(|value| value.as_str())
                .unwrap_or("列")
                .to_string()
        })
        .collect();
    let mut lines = vec![headers.join(" | ")];
    for row in rows.iter().take(16) {
        let cells: Vec<String> = columns
            .iter()
            .map(|column| {
                let field = column
                    .get("field")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                display_cell(row.get(field))
            })
            .collect();
        lines.push(cells.join(" | "));
    }
    if rows.len() > 16 {
        lines.push(format!(
            "……还有 {} 行，完整结果请到站点查看。",
            rows.len() - 16
        ));
    }
    lines.join("\n")
}

fn format_chart(display: &serde_json::Value, rows: &[&serde_json::Value]) -> String {
    let chart_type = display
        .get("chartType")
        .or_else(|| display.get("chart_type"))
        .and_then(|value| value.as_str())
        .unwrap_or("chart");
    let x_field = display
        .get("xField")
        .or_else(|| display.get("x_field"))
        .and_then(|value| value.as_str())
        .unwrap_or("x");
    let y_field = display
        .get("yField")
        .or_else(|| display.get("y_field"))
        .and_then(|value| value.as_str())
        .unwrap_or("y");
    let mut lines = vec![format!("图表（{chart_type}）：{x_field} / {y_field}")];
    for row in rows.iter().take(12) {
        lines.push(format!(
            "{}：{}",
            display_cell(row.get(x_field)),
            display_cell(row.get(y_field))
        ));
    }
    if rows.len() > 12 {
        lines.push("完整图表请到站点查看。".to_string());
    }
    lines.join("\n")
}

fn format_card_list(display: &serde_json::Value, rows: &[&serde_json::Value]) -> String {
    let title_field = display
        .get("titleField")
        .or_else(|| display.get("title_field"))
        .and_then(|value| value.as_str())
        .unwrap_or("title");
    let description_field = display
        .get("descriptionField")
        .or_else(|| display.get("description_field"))
        .and_then(|value| value.as_str());
    let mut lines = Vec::new();
    for (index, row) in rows.iter().take(12).enumerate() {
        let title = display_cell(row.get(title_field));
        if let Some(field) = description_field {
            let desc = display_cell(row.get(field));
            if desc.is_empty() {
                lines.push(format!("{}. {title}", index + 1));
            } else {
                lines.push(format!("{}. {title} — {desc}", index + 1));
            }
        } else {
            lines.push(format!("{}. {title}", index + 1));
        }
    }
    if rows.len() > 12 {
        lines.push("完整列表请到站点查看。".to_string());
    }
    lines.join("\n")
}

fn format_timeline(display: &serde_json::Value, rows: &[&serde_json::Value]) -> String {
    let time_field = display
        .get("timeField")
        .or_else(|| display.get("time_field"))
        .and_then(|value| value.as_str())
        .unwrap_or("time");
    let content_field = display
        .get("contentField")
        .or_else(|| display.get("content_field"))
        .and_then(|value| value.as_str())
        .unwrap_or("content");
    let mut lines = Vec::new();
    for row in rows.iter().take(16) {
        lines.push(format!(
            "{} — {}",
            display_cell(row.get(time_field)),
            display_cell(row.get(content_field))
        ));
    }
    if rows.len() > 16 {
        lines.push("完整时间线请到站点查看。".to_string());
    }
    lines.join("\n")
}

fn format_key_value(rows: &[&serde_json::Value], data: Option<&serde_json::Value>) -> String {
    if !rows.is_empty() {
        return rows
            .iter()
            .take(20)
            .filter_map(|row| {
                let key = row
                    .get("key")
                    .or_else(|| row.get("label"))
                    .or_else(|| row.get("name"))
                    .and_then(|value| value.as_str())?;
                let value = row.get("value").or_else(|| row.get("content"));
                Some(format!("{key}：{}", display_cell(value)))
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    match data {
        Some(serde_json::Value::Object(map)) => map
            .iter()
            .take(20)
            .map(|(key, value)| format!("{key}：{}", display_cell(Some(value))))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn format_value_preview(data: Option<&serde_json::Value>, max_lines: usize) -> String {
    match data {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(serde_json::Value::String(text)) => text.trim().to_string(),
        Some(serde_json::Value::Array(rows)) => rows
            .iter()
            .take(max_lines)
            .map(|row| display_cell(Some(row)))
            .collect::<Vec<_>>()
            .join("\n"),
        Some(serde_json::Value::Object(map)) => map
            .iter()
            .take(max_lines)
            .map(|(key, value)| format!("{key}：{}", display_cell(Some(value))))
            .collect::<Vec<_>>()
            .join("\n"),
        Some(other) => other.to_string(),
    }
}

fn display_cell(value: Option<&serde_json::Value>) -> String {
    match value {
        None | Some(serde_json::Value::Null) => "—".to_string(),
        Some(serde_json::Value::String(text)) => text.trim().to_string(),
        Some(serde_json::Value::Bool(flag)) => {
            if *flag {
                "是".to_string()
            } else {
                "否".to_string()
            }
        }
        Some(serde_json::Value::Number(number)) => number.to_string(),
        Some(serde_json::Value::Array(rows)) => format!("{} 项", rows.len()),
        Some(serde_json::Value::Object(_)) => "…".to_string(),
    }
}

fn telegram_failure_kind(status: u16, body: &str) -> Option<ConnectFailureKind> {
    let data: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let ok = data
        .as_ref()
        .and_then(|value| value.get("ok"))
        .and_then(|value| value.as_bool());
    let error_code = data
        .as_ref()
        .and_then(|value| value.get("error_code"))
        .and_then(json_u64)
        .map(|code| code as u16);
    let effective = error_code.unwrap_or(status);
    if ok == Some(true) && (200..300).contains(&status) {
        return None;
    }
    if ok == Some(true) {
        return None;
    }
    Some(classify_connect_failure(&ConnectFailure::HttpStatus {
        status: effective,
        body,
    }))
}

fn json_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|n| i64::try_from(n).ok()))
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

fn json_snowflake(value: Option<&serde_json::Value>) -> Option<String> {
    let value = value?;
    if let Some(text) = value.as_str() {
        let trimmed = text.trim();
        return (!trimmed.is_empty()).then(|| trimmed.to_string());
    }
    value
        .as_u64()
        .map(|n| n.to_string())
        .or_else(|| value.as_i64().map(|n| n.to_string()))
}

/// Private-chat text extracted from a Discord `MESSAGE_CREATE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordPrivateText {
    pub message_id: String,
    pub author_id: String,
    pub channel_id: String,
    pub text: String,
    pub images: Vec<ChannelImageRef>,
}

impl DiscordPrivateText {
    pub fn inbound(&self) -> InboundC2cText {
        InboundC2cText {
            msg_id: self.message_id.clone(),
            user_openid: self.author_id.clone(),
            content: self.text.clone(),
            images: self.images.clone(),
        }
    }
}

/// Private-chat button press from `INTERACTION_CREATE` type 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordPrivateComponent {
    pub interaction_id: String,
    pub interaction_token: String,
    pub message_id: String,
    pub author_id: String,
    pub channel_id: String,
    pub custom_id: String,
}

/// Bot identity from `GET /users/@me` or Ready `user`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordBotIdentity {
    pub id: String,
    pub username: String,
    pub global_name: Option<String>,
}

pub fn parse_discord_bot_identity(result: &serde_json::Value) -> Option<DiscordBotIdentity> {
    let id = json_snowflake(result.get("id"))?;
    let username = result
        .get("username")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("")
        .to_string();
    let global_name = result
        .get("global_name")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if username.is_empty() && global_name.is_none() {
        return None;
    }
    Some(DiscordBotIdentity {
        id,
        username,
        global_name,
    })
}

/// `GET /gateway/bot` `{ "url": "wss://..." }`.
pub fn parse_discord_gateway_url(status: u16, body: &str) -> Result<String, ConnectFailureKind> {
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
        .filter(|url| url.starts_with("wss://") || url.starts_with("ws://"))
        .map(str::to_string)
        .ok_or(ConnectFailureKind::Transient)
}

/// Remaining Identify budget from `GET /gateway/bot`. Missing → none.
pub fn discord_session_starts_remaining(body: &str) -> Option<u64> {
    let data: serde_json::Value = serde_json::from_str(body).ok()?;
    data.get("session_start_limit")
        .and_then(|value| value.get("remaining"))
        .and_then(json_u64)
}

/// JSON `retry_after` seconds on Discord 429 (float allowed).
pub fn discord_retry_after(body: &str) -> Option<u64> {
    let data: serde_json::Value = serde_json::from_str(body).ok()?;
    data.get("retry_after")
        .and_then(|value| {
            value
                .as_f64()
                .map(|secs| secs.ceil() as u64)
                .or_else(|| json_u64(value))
        })
        .filter(|secs| *secs > 0)
}

/// Discord REST `code` field.
pub fn discord_json_code(body: &str) -> Option<i64> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .as_ref()
        .and_then(json_code)
}

/// Worker-level REST classification. `50007` / `50278` / `40003` stay Transient
/// so one blocked DM does not stop the Gateway worker.
pub fn classify_discord_rest(status: u16, body: &str) -> ConnectFailureKind {
    if let Some(code) = discord_json_code(body) {
        if matches!(code, 50007 | 50278 | 40003 | 20009) {
            return ConnectFailureKind::Transient;
        }
    }
    classify_connect_failure(&ConnectFailure::HttpStatus { status, body })
}

/// Channel type from Gateway/Interaction payloads (`channel_type` or `channel.type`).
pub fn discord_channel_type(data: &serde_json::Value) -> Option<i64> {
    data.get("channel_type").and_then(json_i64).or_else(|| {
        data.get("channel")
            .and_then(|ch| ch.get("type"))
            .and_then(json_i64)
    })
}

/// True only for 1:1 DM. Missing type is not a DM (fail-closed for Group DM).
pub fn is_discord_dm_channel(data: &serde_json::Value) -> bool {
    discord_channel_type(data) == Some(DISCORD_CHANNEL_TYPE_DM)
}

/// `type` field from `GET /channels/{id}` JSON.
pub fn parse_discord_channel_type(body: &str) -> Option<i64> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()?
        .get("type")
        .and_then(json_i64)
}

/// DM `MESSAGE_CREATE`. Drops guild / Group DM / unknown channel type, bots,
/// and empty content without images. Gateway often omits type — the worker must
/// resolve and inject `channel_type` before calling this.
pub fn discord_private_text_from_create(
    data: &serde_json::Value,
    bot_user_id: &str,
) -> Option<DiscordPrivateText> {
    if data.get("guild_id").is_some() {
        return None;
    }
    if !is_discord_dm_channel(data) {
        return None;
    }
    let author = data.get("author")?;
    if author.get("bot").and_then(|value| value.as_bool()) == Some(true) {
        return None;
    }
    let author_id = json_snowflake(author.get("id"))?;
    if !bot_user_id.is_empty() && author_id == bot_user_id {
        return None;
    }
    let text = data
        .get("content")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let images = parse_http_image_attachments(data.get("attachments"));
    if text.trim().is_empty() && images.is_empty() {
        return None;
    }
    Some(DiscordPrivateText {
        message_id: json_snowflake(data.get("id"))?,
        author_id,
        channel_id: json_snowflake(data.get("channel_id"))?,
        text,
        images,
    })
}

/// QQ C2C `attachments` that look like images. Non-image / javascript URLs drop.
pub fn parse_qq_c2c_images(data: &serde_json::Value) -> Vec<ChannelImageRef> {
    parse_http_image_attachments(data.get("attachments"))
}

fn parse_http_image_attachments(value: Option<&serde_json::Value>) -> Vec<ChannelImageRef> {
    let Some(rows) = value.and_then(|value| value.as_array()) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let url = row
                .get("url")
                .or_else(|| row.get("proxy_url"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|url| url.starts_with("http://") || url.starts_with("https://"))?;
            let name = row
                .get("filename")
                .or_else(|| row.get("name"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or("photo.png");
            let mime = row
                .get("content_type")
                .or_else(|| row.get("contentType"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .unwrap_or("");
            let looks_image = mime.starts_with("image/")
                || name.rsplit('.').next().is_some_and(|ext| {
                    matches!(
                        ext.to_ascii_lowercase().as_str(),
                        "png" | "jpg" | "jpeg" | "gif" | "webp"
                    )
                });
            if !looks_image {
                return None;
            }
            Some(ChannelImageRef {
                url: url.to_string(),
                name: name.to_string(),
                mime: if mime.starts_with("image/") {
                    mime.to_string()
                } else {
                    "image/png".to_string()
                },
                size: row.get("size").and_then(json_u64).unwrap_or(0),
            })
        })
        .collect()
}

/// Component interaction in a DM. Interaction type must be 3 (`MESSAGE_COMPONENT`).
/// Same channel-type gate as text: only `channel.type` / `channel_type == 1`.
pub fn discord_private_component_from_create(
    data: &serde_json::Value,
) -> Option<DiscordPrivateComponent> {
    if data.get("type").and_then(json_i64) != Some(3) {
        return None;
    }
    if data.get("guild_id").is_some() {
        return None;
    }
    if !is_discord_dm_channel(data) {
        return None;
    }
    let user = data
        .get("user")
        .or_else(|| data.get("member").and_then(|member| member.get("user")))?;
    let author_id = json_snowflake(user.get("id"))?;
    let custom_id = data
        .get("data")
        .and_then(|row| row.get("custom_id"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let interaction_id = json_snowflake(data.get("id"))?;
    let interaction_token = data
        .get("token")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let message_id = json_snowflake(data.get("message").and_then(|row| row.get("id")))
        .unwrap_or_else(|| interaction_id.clone());
    Some(DiscordPrivateComponent {
        interaction_id,
        interaction_token,
        message_id,
        author_id,
        channel_id: json_snowflake(data.get("channel_id"))?,
        custom_id,
    })
}

/// Final-turn image URLs from `data` plus `task.stepHistory`.
/// Only site image-cache paths and `http(s)` URLs; `javascript:` / data URLs drop.
/// Capped at [`CHANNEL_IMAGE_LIMIT`] to mirror inbound attachment take.
pub fn collect_channel_image_urls(response: &serde_json::Value) -> Vec<String> {
    let mut urls = Vec::new();
    push_channel_image_url(&mut urls, extract_channel_image_url(response.get("data")));
    if urls.len() >= CHANNEL_IMAGE_LIMIT {
        return urls;
    }
    if let Some(steps) = response
        .get("task")
        .and_then(|task| task.get("stepHistory"))
        .and_then(|value| value.as_array())
    {
        for step in steps {
            if urls.len() >= CHANNEL_IMAGE_LIMIT {
                break;
            }
            push_channel_image_url(
                &mut urls,
                step.get("imageUrl").and_then(|value| value.as_str()),
            );
        }
    }
    urls
}

fn extract_channel_image_url(data: Option<&serde_json::Value>) -> Option<&str> {
    let inner = crate::task_inner_value(data?);
    inner
        .get("url")
        .or_else(|| inner.get("imageUrl"))
        .and_then(|value| value.as_str())
        .or_else(|| data?.get("imageUrl").and_then(|value| value.as_str()))
}

fn push_channel_image_url(urls: &mut Vec<String>, candidate: Option<&str>) {
    let Some(url) = candidate.map(str::trim).filter(|url| !url.is_empty()) else {
        return;
    };
    if !(url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("/api/brew/image-cache/"))
    {
        return;
    }
    if urls.iter().any(|seen| seen == url) {
        return;
    }
    urls.push(url.to_string());
}

/// `file_info` from QQ C2C / group file upload JSON. Empty or non-2xx → Transient.
pub fn parse_qq_file_info(status: u16, body: &str) -> Result<String, ConnectFailureKind> {
    if !(200..300).contains(&status) {
        return Err(classify_connect_failure(&ConnectFailure::HttpStatus {
            status,
            body,
        }));
    }
    let data: serde_json::Value =
        serde_json::from_str(body).map_err(|_| ConnectFailureKind::Transient)?;
    data.get("file_info")
        .or_else(|| data.get("data").and_then(|value| value.get("file_info")))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or(ConnectFailureKind::Transient)
}
