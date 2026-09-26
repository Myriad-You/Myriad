//! Shared private-chat Work: session, pending, cursor, outbound ledger.
//!
//! Transport adapters only adapt send/receive. Rules stay in
//! `myriad_agent_rules::channel`.

use crate::services::channel_platform::ChannelPlatform;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use myriad_agent_rules::channel::{
    CHANNEL_HELP_REPLY, CHANNEL_IMAGE_LIMIT, CHANNEL_NEW_SESSION_REPLY, CHANNEL_STOP_REPLY,
    ChannelCommand, ChannelEvent, ChannelImageRef, DISCORD_TEXT_LIMIT, DeliveryContext,
    DeliveryPlan, FEISHU_TEXT_LIMIT, PANEL_REQUIRED_REPLY, PENDING_STALE_REPLY, PendingDecision,
    PendingKind, PendingOption, PendingPrompt, QQ_TEXT_LIMIT, TELEGRAM_TEXT_LIMIT,
    TelegramCallbackAction, channel_can_finish, clarify_base_input, clarify_followup,
    collect_channel_image_urls, decide_pending_reply, discord_dm_capabilities,
    discord_reply_markup, ensure_pending_id, feishu_dm_capabilities, feishu_reply_markup,
    format_channel_result, format_pending_prompt, panel_entry_reply, parse_channel_command,
    pending_prompt_from_model_json, plan_delivery, qq_c2c_capabilities, should_deliver_sequence,
    split_channel_text, task_started_reply, telegram_callback_action, telegram_dm_capabilities,
    telegram_force_reply_markup, telegram_reply_markup,
};
use myriad_agent_rules::{is_cancellable_task_status, session_id_from_lane_id};
use once_cell::sync::Lazy;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement, Value as SeaValue,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::api::agent::{ProcessContext, ProcessRequest};
use crate::middleware::auth::{Claims, mint_session_claims};
use crate::services::agent::run_hub::AgentRun;
use crate::services::agent::{Agent, AgentInteractionMode, AgentProgressEvent};
use crate::services::runtime_registry::{self as shared_registry, RegistryIdentity};

const BINDING_TTL_SECS: i64 = 30 * 24 * 60 * 60;
const TYPING_REFRESH: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSession {
    session_id: String,
    #[serde(default)]
    last_run_id: Option<String>,
    #[serde(default)]
    last_event_seq: u64,
    #[serde(default)]
    original_input: String,
    #[serde(default)]
    binding: Option<ChannelBinding>,
    #[serde(default)]
    address: Option<transport::ChannelAddress>,
    /// Her chat with them, apart from the Work she hands off.
    #[serde(default)]
    chat_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPending {
    prompt: PendingPrompt,
    #[serde(default)]
    last_event_seq: u64,
    #[serde(default)]
    expected_user_id: Option<i32>,
}

mod chat;
mod transport;
use crate::services::channel_pairing::ChannelBinding;
pub use transport::ChannelTransport;
use transport::cache_inbound_images;

#[derive(Clone)]
struct ChannelSink {
    transport: ChannelTransport,
    binding: ChannelBinding,
    db: DatabaseConnection,
}

impl ChannelSink {
    fn platform(&self) -> ChannelPlatform {
        self.transport.platform()
    }
    fn capabilities(&self) -> myriad_agent_rules::channel::ChannelCapabilities {
        self.transport.capabilities()
    }
    fn text_limit(&self) -> usize {
        self.transport.text_limit()
    }
    fn delivery_context(&self) -> DeliveryContext {
        self.transport.delivery_context()
    }
    async fn authorized(&self) -> bool {
        self.binding.is_current(&self.db).await
    }
    async fn check(&self) -> Result<(), String> {
        if self.authorized().await {
            Ok(())
        } else {
            Err("channel binding revoked".into())
        }
    }
    async fn send_typing(&self) {
        if self.authorized().await {
            self.transport.send_typing().await;
        }
    }
    async fn send_text(&self, text: &str) -> Result<(), String> {
        self.check().await?;
        self.transport.send_text(text).await
    }
    async fn send_prompt(&self, text: &str, prompt: &PendingPrompt) -> Result<(), String> {
        self.check().await?;
        self.transport.send_prompt(text, prompt).await
    }
    async fn send_force_reply(&self, text: &str) -> Result<(), String> {
        self.check().await?;
        self.transport.send_force_reply(text).await
    }
}

fn identity(user_id: i32) -> RegistryIdentity<'static> {
    RegistryIdentity {
        subject_id: Some(user_id),
        owner_id: Some(user_id),
        tapp_id: None,
        runtime_id: None,
    }
}

/// Claim one inbound message id; `false` means it was already handled (or
/// the claim could not be recorded, which is treated the same way).
pub(crate) async fn claim_inbound(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    user_id: Option<i32>,
    inbound_id: &str,
) -> bool {
    match shared_registry::put_if_absent(
        db,
        platform.inbound_ns(),
        inbound_id,
        user_id.map_or(
            RegistryIdentity {
                subject_id: None,
                owner_id: None,
                tapp_id: None,
                runtime_id: None,
            },
            identity,
        ),
        &Value::Bool(true),
        (Utc::now() + ChronoDuration::hours(24)).timestamp(),
    )
    .await
    {
        Ok(true) => true,
        Ok(false) => {
            info!(inbound_id, %platform, "channel duplicate inbound ignored");
            false
        }
        Err(error) => {
            warn!(%error, %platform, "channel duplicate check failed");
            false
        }
    }
}

pub async fn handle_text_with_images(
    db: &DatabaseConnection,
    user_id: i32,
    sender_id: &str,
    chat_id: &str,
    input: &str,
    images: &[ChannelImageRef],
    session_key: &str,
    transport: ChannelTransport,
) {
    // The private-text entry has already claimed this message id.
    let platform = transport.platform();
    let binding = match ChannelBinding::resolve(db, platform, user_id, sender_id).await {
        Ok(Some(binding)) => binding,
        _ => return,
    };
    let session_key = binding.session_key(session_key);
    let sink = ChannelSink {
        transport,
        binding,
        db: db.clone(),
    };
    let session_key = session_key.to_string();
    let chat_id = chat_id.to_string();
    let input = input.to_string();
    let images = images.to_vec();
    let db = db.clone();
    let lock_key = session_key.clone();
    with_chat_lock(&lock_key, async move {
        if !sink.authorized().await {
            return;
        }
        continue_text(&db, user_id, &chat_id, &input, &images, &session_key, sink).await;
    })
    .await;
}

pub async fn handle_callback(
    db: &DatabaseConnection,
    user_id: i32,
    sender_id: &str,
    chat_id: &str,
    data: &str,
    session_key: &str,
    inbound_id: &str,
    transport: ChannelTransport,
) {
    let platform = transport.platform();
    let binding = match ChannelBinding::resolve(db, platform, user_id, sender_id).await {
        Ok(Some(binding)) => binding,
        _ => return,
    };
    let session_key = binding.session_key(session_key);
    let sink = ChannelSink {
        transport,
        binding,
        db: db.clone(),
    };
    let platform = sink.platform();
    let session_key = session_key.to_string();
    let chat_id = chat_id.to_string();
    let data = data.to_string();
    let inbound_id = inbound_id.to_string();
    let db = db.clone();
    let lock_key = session_key.clone();
    with_chat_lock(&lock_key, async move {
        if !sink.authorized().await
            || !claim_inbound(
                &db,
                platform,
                Some(user_id),
                &format!("{session_key}:{inbound_id}"),
            )
            .await
        {
            return;
        }
        let Some(pending) = load_pending(&db, platform, &session_key).await else {
            return;
        };
        match telegram_callback_action(&pending.prompt, &data) {
            TelegramCallbackAction::RequestInput => {
                let _ = sink.send_force_reply(&pending.prompt.question).await;
            }
            TelegramCallbackAction::Resume(answer) => {
                continue_text(&db, user_id, &chat_id, &answer, &[], &session_key, sink).await;
            }
            TelegramCallbackAction::Stale => {
                let _ = sink.send_text(PENDING_STALE_REPLY).await;
            }
            TelegramCallbackAction::Unknown => {
                let _ = sink
                    .send_prompt(&format_pending_prompt(&pending.prompt), &pending.prompt)
                    .await;
            }
        }
    })
    .await;
}

async fn continue_text(
    db: &DatabaseConnection,
    user_id: i32,
    _chat_id: &str,
    input: &str,
    images: &[ChannelImageRef],
    session_key: &str,
    sink: ChannelSink,
) {
    let platform = sink.platform();
    if !sink.authorized().await {
        return;
    }
    let claims = match claims_for_user(db, user_id).await {
        Ok(value) => value,
        Err(error) => {
            warn!(%error, user_id, "channel claims lookup failed");
            return;
        }
    };
    let session_id = match bind_session(db, platform, user_id, session_key).await {
        Ok(stored) => stored.session_id,
        Err(error) => {
            warn!(%error, "channel session bind failed");
            let _ = sink.send_text("无法保存通道会话，请稍后重试。").await;
            return;
        }
    };

    if let Some(command) = parse_channel_command(input) {
        handle_command(
            db,
            user_id,
            platform,
            session_key,
            &session_id,
            command,
            &sink,
        )
        .await;
        return;
    }

    if runtime::recover_session(db, session_key, Some(sink.clone())).await {
        let _ = sink.send_text("正在恢复上一件事的回复，请稍后再试。").await;
        return;
    }
    if let Some(pending) = load_pending(db, platform, session_key).await {
        if pending
            .expected_user_id
            .is_some_and(|expected| expected != user_id)
        {
            let _ = sink.send_text(PENDING_STALE_REPLY).await;
            return;
        }
        match decide_pending_reply(&pending.prompt, input, Utc::now().timestamp()) {
            PendingDecision::Reask { reply } => {
                let _ = sink.send_prompt(&reply, &pending.prompt).await;
                return;
            }
            PendingDecision::Expired { reply } => {
                clear_pending(db, platform, session_key).await;
                let _ = sink.send_text(&reply).await;
                return;
            }
            PendingDecision::Resume { kind, answer, .. } => {
                let taken = take_pending_if_id(db, platform, session_key, &pending.prompt.id).await;
                if taken.is_none() {
                    let _ = sink.send_text(PENDING_STALE_REPLY).await;
                    return;
                }
                resume_pending(
                    db.clone(),
                    claims,
                    session_id,
                    session_key,
                    user_id,
                    kind,
                    answer,
                    sink,
                    input,
                    pending,
                )
                .await;
                return;
            }
        }
    }

    // She answers as herself; Work is what she hands off.
    chat::start_chat_turn(
        db.clone(),
        claims,
        user_id,
        session_id,
        input,
        images,
        sink,
        session_key,
    )
    .await;
}

async fn handle_command(
    db: &DatabaseConnection,
    user_id: i32,
    platform: ChannelPlatform,
    session_key: &str,
    session_id: &str,
    command: ChannelCommand,
    sink: &ChannelSink,
) {
    match command {
        ChannelCommand::Stop => {
            stop_delivery(session_key).await;
            cancel_session_tasks(db, user_id, session_id).await;
            clear_pending(db, platform, session_key).await;
            clear_outbound(db, platform, session_key).await;
            if let Some(mut stored) = load_session(db, platform, session_key).await {
                stored.last_run_id = None;
                stored.last_event_seq = 0;
                let _ = put_session(db, platform, user_id, session_key, stored).await;
            }
            let _ = sink.send_text(CHANNEL_STOP_REPLY).await;
        }
        ChannelCommand::NewConversation => {
            stop_delivery(session_key).await;
            cancel_session_tasks(db, user_id, session_id).await;
            clear_pending(db, platform, session_key).await;
            clear_outbound(db, platform, session_key).await;
            match crate::api::agent::ensure_session(db, None, user_id, AgentInteractionMode::Work)
                .await
            {
                Ok(new_id) => {
                    let _ = put_session(
                        db,
                        platform,
                        user_id,
                        session_key,
                        StoredSession {
                            session_id: new_id,
                            last_run_id: None,
                            last_event_seq: 0,
                            original_input: String::new(),
                            binding: None,
                            address: None,
                            chat_session_id: None,
                        },
                    )
                    .await;
                    let _ = sink.send_text(CHANNEL_NEW_SESSION_REPLY).await;
                }
                Err(error) => {
                    warn!(%error, "channel new session failed");
                    let _ = sink.send_text("没能开新对话，请稍后再试。").await;
                }
            }
        }
        ChannelCommand::Status => {
            let reply = status_reply(db, user_id, platform, session_key, session_id).await;
            let _ = sink.send_text(&reply).await;
        }
        ChannelCommand::Help => {
            let _ = sink.send_text(CHANNEL_HELP_REPLY).await;
        }
    }
}

async fn status_reply(
    db: &DatabaseConnection,
    user_id: i32,
    platform: ChannelPlatform,
    session_key: &str,
    session_id: &str,
) -> String {
    if let Some(pending) = load_pending(db, platform, session_key).await {
        return format!("当前待答：\n{}", format_pending_prompt(&pending.prompt));
    }
    let agent = Agent::new(db.clone()).await;
    let tasks = agent.get_user_tasks(user_id).await;
    let live = tasks.into_iter().find(|task| {
        is_cancellable_task_status(&task.status)
            && session_id_from_lane_id(task.lane_id.as_deref()).as_deref() == Some(session_id)
    });
    match live {
        Some(task) => format!(
            "正在办：{}（{}%）\n任务 {}",
            task_status_label(&task.status),
            task.progress,
            task.task_id
        ),
        None => {
            if session_id.is_empty() {
                "现在没有进行中的任务。".to_string()
            } else {
                format!("现在没有进行中的任务。会话 {session_id}。")
            }
        }
    }
}

fn task_status_label(status: &myriad_agent_rules::TaskStatus) -> &'static str {
    match status {
        myriad_agent_rules::TaskStatus::Pending => "排队中",
        myriad_agent_rules::TaskStatus::Running => "执行中",
        myriad_agent_rules::TaskStatus::WaitingForInput => "等待回答",
        myriad_agent_rules::TaskStatus::Paused => "已暂停",
        myriad_agent_rules::TaskStatus::Completed => "已完成",
        myriad_agent_rules::TaskStatus::Failed => "失败",
        myriad_agent_rules::TaskStatus::Cancelled => "已取消",
    }
}

async fn cancel_session_tasks(db: &DatabaseConnection, user_id: i32, session_id: &str) {
    if session_id.is_empty() {
        return;
    }
    let agent = Agent::new(db.clone()).await;
    for task in agent.get_user_tasks(user_id).await {
        if !is_cancellable_task_status(&task.status) {
            continue;
        }
        if session_id_from_lane_id(task.lane_id.as_deref()).as_deref() != Some(session_id) {
            continue;
        }
        let _ = crate::api::agent::cancel_task_and_wake(db, user_id, &task.task_id).await;
    }
}

/// Start their Work with images already cached: what they asked for, or what
/// she handed off for them.
async fn start_work_run(
    db: DatabaseConnection,
    claims: Claims,
    user_id: i32,
    session_id: String,
    input: &str,
    custom_data: Option<Value>,
    sink: ChannelSink,
    session_key: &str,
) {
    let input = if input.trim().is_empty() && custom_data.is_some() {
        "请查看这张图片。"
    } else {
        input
    };
    let run = match crate::api::agent::start_process_run(
        db.clone(),
        claims,
        ProcessRequest {
            input: input.to_string(),
            context: Some(ProcessContext {
                mode: Some(AgentInteractionMode::Work),
                session_id: (!session_id.is_empty()).then_some(session_id),
                current_route: None,
                active_platforms: None,
                conversation_history: None,
                custom_data: custom_data.clone(),
                intention_id: None,
                autonomy_permission_cap: None,
                rig_state: None,
                group: None,
                channel_chat: None,
            }),
        },
    )
    .await
    {
        Ok(run) => run,
        Err(error) => {
            let body = error.0.to_json();
            let message = body
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| body.get("error").and_then(Value::as_str))
                .unwrap_or("办事没能开始。")
                .to_string();
            warn!(error = %message, "channel Work start failed");
            let _ = sink.send_text(&message).await;
            return;
        }
    };
    start_delivery(
        run,
        db,
        user_id,
        session_key.to_string(),
        sink,
        input.to_string(),
    )
    .await;
}

async fn resume_pending(
    db: DatabaseConnection,
    claims: Claims,
    session_id: String,
    session_key: &str,
    user_id: i32,
    kind: PendingKind,
    answer: String,
    sink: ChannelSink,
    latest_input: &str,
    parked: StoredPending,
) {
    sink.send_typing().await;
    let sid = (!session_id.is_empty()).then_some(session_id);
    let mut next_original = latest_input.to_string();
    let run = match kind {
        PendingKind::Clarify { original_input } => {
            let (input, parked_original) = clarify_followup(&original_input, &answer);
            next_original = parked_original;
            crate::api::agent::start_process_run(
                db.clone(),
                claims,
                ProcessRequest {
                    input,
                    context: Some(ProcessContext {
                        mode: Some(AgentInteractionMode::Work),
                        session_id: sid,
                        current_route: None,
                        active_platforms: None,
                        conversation_history: None,
                        custom_data: None,
                        intention_id: None,
                        autonomy_permission_cap: None,
                        rig_state: None,
                        group: None,
                        channel_chat: None,
                    }),
                },
            )
            .await
            .map_err(|error| error.0.to_json())
        }
        // Recipe-level confirmations are gone; only prompts stored before
        // that change can still carry this kind.
        PendingKind::Confirm { .. } => Err(myriad_error::AppError::public_json(
            "This confirmation is no longer available. Please send the request again.",
        )),
        PendingKind::Answer {
            task_id,
            question_id,
            ..
        } => crate::api::agent::start_answer_run(
            db.clone(),
            claims,
            task_id,
            question_id,
            answer,
            sid,
        )
        .await
        .map_err(|error| error.0.to_json()),
    };

    match run {
        Ok(run) => {
            start_delivery(
                run,
                db,
                user_id,
                session_key.to_string(),
                sink,
                next_original,
            )
            .await;
        }
        Err(body) => {
            if sink.authorized().await {
                if let Err(error) = shared_registry::put(
                    &db,
                    sink.platform().pending_ns(),
                    session_key,
                    identity(user_id),
                    &parked,
                    parked
                        .prompt
                        .expires_at_unix
                        .unwrap_or_else(|| (Utc::now() + ChronoDuration::days(2)).timestamp()),
                )
                .await
                {
                    warn!(%error, "cannot restore unaccepted channel answer");
                }
            }
            let message = body
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| body.get("error").and_then(Value::as_str))
                .unwrap_or("这一步没能继续。")
                .to_string();
            warn!(error = %message, "channel resume failed");
            let _ = sink.send_text(&message).await;
        }
    }
}

/// Platforms where she can write first: a bot there may start a message.
/// QQ only answers a message it received; Feishu waits for later.
const FIRST_WORD_PLATFORMS: [ChannelPlatform; 2] =
    [ChannelPlatform::Telegram, ChannelPlatform::Discord];

/// People she could write to first: paired, with a private chat she has had
/// with them on a platform where she can.
pub(crate) async fn reachable_people(db: &DatabaseConnection) -> Vec<i32> {
    let mut people = std::collections::BTreeSet::new();
    for platform in FIRST_WORD_PLATFORMS {
        for row in shared_registry::list(db, platform.session_ns(), None, None)
            .await
            .unwrap_or_default()
        {
            if let Ok(stored) = serde_json::from_value::<StoredSession>(row.payload) {
                if let (Some(binding), Some(_)) = (stored.binding, stored.address) {
                    people.insert(binding.user_id);
                }
            }
        }
    }
    people.into_iter().collect()
}

/// Her first line to a paired person, in the private chat they last used
/// with her: sent, and kept in her chat with them there, so their answer
/// carries on from it. Where it went, if it went.
pub(crate) async fn say_first(
    db: &DatabaseConnection,
    user_id: i32,
    text: &str,
) -> Option<ChannelPlatform> {
    for platform in FIRST_WORD_PLATFORMS {
        let rows = shared_registry::list(db, platform.session_ns(), Some(user_id), None)
            .await
            .unwrap_or_default();
        // Newest last.
        for row in rows.into_iter().rev() {
            let Ok(stored) = serde_json::from_value::<StoredSession>(row.payload) else {
                continue;
            };
            let (Some(binding), Some(address)) = (stored.binding, stored.address) else {
                continue;
            };
            if binding.user_id != user_id || !binding.is_current(db).await {
                continue;
            }
            let Some(transport) = address.connect(db).await else {
                continue;
            };
            let sink = ChannelSink {
                transport,
                binding,
                db: db.clone(),
            };
            let session_key = row.record_id;
            let sent = runtime::with_chat_lock(&session_key, async {
                for chunk in split_channel_text(text, sink.text_limit()) {
                    if sink.send_text(&chunk).await.is_err() {
                        return false;
                    }
                }
                true
            })
            .await;
            if !sent {
                return None;
            }
            if let Some(chat) = chat::chat_session(db, platform, user_id, &session_key).await {
                if let Err(error) = crate::api::agent::persist_assistant_message(
                    db,
                    &chat,
                    None,
                    text,
                    Some(serde_json::json!({ "unprompted": true })),
                )
                .await
                {
                    warn!(%error, "her first line was sent but not kept in the chat");
                }
            }
            return Some(platform);
        }
    }
    None
}

pub(crate) async fn claims_for_user(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Claims, DbErr> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT username, COALESCE(is_admin, false) AS is_admin, \
                    COALESCE(is_owner, false) AS is_owner, \
                    COALESCE(token_version, 0) AS token_version \
             FROM users WHERE id = $1 LIMIT 1",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await?
        .ok_or_else(|| DbErr::RecordNotFound("paired user missing".into()))?;
    let username: String = row.try_get("", "username").unwrap_or_default();
    let is_admin: bool = row.try_get("", "is_admin").unwrap_or(false);
    let is_owner: bool = row.try_get("", "is_owner").unwrap_or(false);
    let token_version = crate::middleware::auth::row_session_epoch(&row)?;
    Ok(mint_session_claims(
        user_id,
        username,
        is_admin,
        is_owner,
        token_version,
    ))
}

async fn bind_session(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    user_id: i32,
    session_key: &str,
) -> Result<StoredSession, DbErr> {
    if let Some(stored) =
        shared_registry::get::<StoredSession>(db, platform.session_ns(), session_key).await?
    {
        if !stored.session_id.is_empty() {
            return Ok(stored);
        }
    }
    let session_id =
        crate::api::agent::ensure_session(db, None, user_id, AgentInteractionMode::Work)
            .await
            .map_err(DbErr::Custom)?;
    let stored = StoredSession {
        session_id,
        last_run_id: None,
        last_event_seq: 0,
        original_input: String::new(),
        binding: None,
        address: None,
        chat_session_id: None,
    };
    put_session(db, platform, user_id, session_key, stored.clone()).await?;
    Ok(stored)
}

async fn put_session(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    user_id: i32,
    session_key: &str,
    stored: StoredSession,
) -> Result<(), DbErr> {
    shared_registry::put(
        db,
        platform.session_ns(),
        session_key,
        identity(user_id),
        &stored,
        (Utc::now() + ChronoDuration::seconds(BINDING_TTL_SECS)).timestamp(),
    )
    .await
}

async fn load_session(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    session_key: &str,
) -> Option<StoredSession> {
    shared_registry::get::<StoredSession>(db, platform.session_ns(), session_key)
        .await
        .ok()
        .flatten()
}

async fn load_pending(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    session_key: &str,
) -> Option<StoredPending> {
    if session_key.is_empty() {
        return None;
    }
    match shared_registry::get::<StoredPending>(db, platform.pending_ns(), session_key).await {
        Ok(value) => value,
        Err(error) => {
            warn!(%error, "channel pending load failed");
            None
        }
    }
}

async fn take_pending_if_id(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    session_key: &str,
    expected_id: &str,
) -> Option<StoredPending> {
    let taken = match shared_registry::take::<StoredPending>(db, platform.pending_ns(), session_key)
        .await
    {
        Ok(value) => value,
        Err(error) => {
            warn!(%error, "channel pending take failed");
            return None;
        }
    };
    let pending = taken?;
    let mut prompt = pending.prompt.clone();
    ensure_pending_id(&mut prompt);
    if !expected_id.is_empty() && prompt.id != expected_id {
        let _ = shared_registry::put(
            db,
            platform.pending_ns(),
            session_key,
            RegistryIdentity {
                subject_id: pending.expected_user_id,
                owner_id: pending.expected_user_id,
                tapp_id: None,
                runtime_id: None,
            },
            &pending,
            (Utc::now() + ChronoDuration::days(2)).timestamp(),
        )
        .await;
        return None;
    }
    Some(pending)
}

async fn clear_pending(db: &DatabaseConnection, platform: ChannelPlatform, session_key: &str) {
    if session_key.is_empty() {
        return;
    }
    if let Err(error) =
        shared_registry::take::<StoredPending>(db, platform.pending_ns(), session_key).await
    {
        warn!(%error, "channel pending clear failed");
    }
}

mod presentation;
use presentation::map_progress;

mod runtime;
use runtime::{is_active, owns_run, start_delivery, stop_delivery, with_chat_lock};
pub(crate) use runtime::{revoke_pairing, run_recovery_worker};
mod delivery;
use delivery::{clear_outbound, deliver_run, flush_outbound};

/// Cancels a channel session watcher when its owning connection is stopped.
pub(crate) struct AbortTask<T>(pub tokio::task::JoinHandle<T>);
impl<T> Drop for AbortTask<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}
