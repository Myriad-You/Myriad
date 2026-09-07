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

/// Telegram `sendMessage` text cap. Counted after entity parse; first cut
/// sends plain text so Unicode scalars are the conservative bound.
pub const TELEGRAM_TEXT_LIMIT: usize = 4096;

/// Reply when confirmation or a browser-only page action arrives on QQ.
pub const PANEL_REQUIRED_REPLY: &str = "请到站点面板完成这一步。";

/// Reply when a pending prompt cannot be parsed.
pub const PENDING_REASK_REPLY: &str = "没看懂。请回复选项编号，或按提示回答。";

/// Reply when a confirmation or question has expired.
pub const PENDING_EXPIRED_REPLY: &str = "这一步已经过期。请重新发一句，或到站点面板继续。";

/// How to answer a yes/no confirmation in chat.
pub const CONFIRM_HINT: &str = "回复「是」确认，或「否」取消。";

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

/// Telegram DM: text in, final text out, numbered options plus inline buttons.
/// Typing is a transport hint (`sendChatAction`), not a capability bit.
/// Edit / streaming draft stay off.
pub fn telegram_dm_capabilities() -> ChannelCapabilities {
    ChannelCapabilities {
        inbound_text: true,
        inbound_media: false,
        inbound_callback: true,
        outbound_final_text: true,
        outbound_markdown: false,
        outbound_image: false,
        outbound_edit: false,
        outbound_streaming_draft: false,
        interactive: true,
        frontend_action: false,
        performance: false,
        outfit: false,
    }
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
    pairing_bind_reply_for(result, "qq")
}

/// Pairing consume reply for a channel. Only the taken-openid copy differs.
pub fn pairing_bind_reply_for(result: PairingBindResult, platform: &str) -> &'static str {
    match result {
        PairingBindResult::Bound { .. } => PAIRING_OK_REPLY,
        PairingBindResult::InvalidOrExpired => PAIRING_INVALID_REPLY,
        PairingBindResult::OpenidTaken if platform == "telegram" => TELEGRAM_PAIRING_TAKEN_REPLY,
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
    pub kind: PendingKind,
    pub question: String,
    pub options: Vec<PendingOption>,
    pub expires_at_unix: Option<i64>,
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
pub fn telegram_inline_keyboard(prompt: &PendingPrompt) -> Vec<Vec<TelegramInlineButton>> {
    match prompt_question_type(prompt) {
        "confirmation" | "confirm" => vec![vec![
            TelegramInlineButton {
                text: "是".to_string(),
                callback_data: TELEGRAM_CALLBACK_YES.to_string(),
            },
            TelegramInlineButton {
                text: "否".to_string(),
                callback_data: TELEGRAM_CALLBACK_NO.to_string(),
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
                    callback_data: format!("o:{index}"),
                }]
            })
            .collect(),
        _ => vec![vec![TelegramInlineButton {
            text: TELEGRAM_INPUT_BUTTON.to_string(),
            callback_data: TELEGRAM_CALLBACK_INPUT.to_string(),
        }]],
    }
}

/// Map `callback_data` onto the parked prompt. Indexes stay 0-based (`o:0`).
pub fn telegram_callback_action(prompt: &PendingPrompt, data: &str) -> TelegramCallbackAction {
    match data.trim() {
        TELEGRAM_CALLBACK_INPUT => TelegramCallbackAction::RequestInput,
        TELEGRAM_CALLBACK_YES => TelegramCallbackAction::Resume("是".to_string()),
        TELEGRAM_CALLBACK_NO => TelegramCallbackAction::Resume("否".to_string()),
        other => match other.strip_prefix("o:") {
            Some(index) => index
                .parse::<usize>()
                .ok()
                .and_then(|index| prompt.options.get(index))
                .map(|option| TelegramCallbackAction::Resume(option.value.clone()))
                .unwrap_or(TelegramCallbackAction::Unknown),
            None => TelegramCallbackAction::Unknown,
        },
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
    Some(PendingPrompt {
        kind: PendingKind::Clarify {
            original_input: original_input.to_string(),
        },
        question,
        options,
        expires_at_unix: None,
    })
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
    let text = message
        .get("text")
        .and_then(|value| value.as_str())?
        .to_string();
    let message_id = json_i64(message.get("message_id")?)?;
    Some(TelegramPrivateText {
        update_id,
        message_id,
        from_id,
        chat_id,
        text,
    })
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
    let count = text.chars().count();
    if count <= TELEGRAM_TEXT_LIMIT {
        return text.to_string();
    }
    text.chars().take(TELEGRAM_TEXT_LIMIT).collect()
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
