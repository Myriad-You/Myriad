//! Shared private-chat Work: session, pending, cursor, outbound ledger.
//!
//! Transport adapters only adapt send/receive. Rules stay in
//! `myriad_agent_rules::channel`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use myriad_agent_rules::channel::{
    channel_can_finish, clarify_base_input, clarify_followup, collect_channel_image_urls,
    decide_pending_reply, discord_dm_capabilities, discord_reply_markup, ensure_pending_id,
    feishu_dm_capabilities, feishu_reply_markup, format_channel_result, format_pending_prompt,
    panel_entry_reply, parse_channel_command, pending_prompt_from_model_json, plan_delivery,
    qq_c2c_capabilities, should_deliver_sequence, split_channel_text, task_started_reply,
    telegram_callback_action, telegram_dm_capabilities, telegram_force_reply_markup,
    telegram_reply_markup, ChannelCommand, ChannelEvent, ChannelImageRef, DeliveryContext,
    DeliveryPlan, PendingDecision, PendingKind, PendingOption, PendingPrompt,
    TelegramCallbackAction, CHANNEL_HELP_REPLY, CHANNEL_IMAGE_LIMIT, CHANNEL_NEW_SESSION_REPLY,
    CHANNEL_STOP_REPLY, DISCORD_TEXT_LIMIT, FEISHU_TEXT_LIMIT, PANEL_REQUIRED_REPLY,
    PENDING_STALE_REPLY, QQ_TEXT_LIMIT, TELEGRAM_TEXT_LIMIT,
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
use crate::middleware::auth::{mint_session_claims, Claims};
use crate::services::agent::run_hub::AgentRun;
use crate::services::agent::{Agent, AgentInteractionMode, AgentProgressEvent};
use crate::services::tapp_registry::{self as shared_registry, RegistryIdentity};

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPending {
    prompt: PendingPrompt,
    #[serde(default)]
    last_event_seq: u64,
    #[serde(default)]
    expected_user_id: Option<i32>,
}

mod transport;
use crate::services::channel_pairing::ChannelBinding;
use transport::cache_inbound_images;
pub use transport::ChannelTransport;

#[derive(Clone)]
struct ChannelSink {
    transport: ChannelTransport,
    binding: ChannelBinding,
    db: DatabaseConnection,
}

impl ChannelSink {
    fn platform(&self) -> &'static str {
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

fn session_ns(platform: &str) -> &'static str {
    match platform {
        "telegram" => "telegram_dm_session",
        "discord" => "discord_dm_session",
        "feishu" => "feishu_p2p_session",
        _ => "qq_c2c_session",
    }
}

fn pending_ns(platform: &str) -> &'static str {
    match platform {
        "telegram" => "telegram_dm_pending",
        "discord" => "discord_dm_pending",
        "feishu" => "feishu_p2p_pending",
        _ => "qq_c2c_pending",
    }
}

fn outbound_ns(platform: &str) -> &'static str {
    match platform {
        "telegram" => "telegram_dm_outbound",
        "discord" => "discord_dm_outbound",
        "feishu" => "feishu_p2p_outbound",
        _ => "qq_c2c_outbound",
    }
}

fn inbound_ns(platform: &str) -> &'static str {
    match platform {
        "telegram" => "telegram_dm_update",
        "discord" => "discord_dm_msg",
        "feishu" => "feishu_p2p_msg",
        _ => "qq_c2c_msg",
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

async fn claim_inbound(
    db: &DatabaseConnection,
    platform: &str,
    user_id: i32,
    inbound_id: &str,
) -> bool {
    match shared_registry::put_if_absent(
        db,
        inbound_ns(platform),
        inbound_id,
        identity(user_id),
        &Value::Bool(true),
        (Utc::now() + ChronoDuration::hours(24)).timestamp(),
    )
    .await
    {
        Ok(true) => true,
        Ok(false) => {
            info!(inbound_id, platform, "channel duplicate inbound ignored");
            false
        }
        Err(error) => {
            warn!(%error, platform, "channel duplicate check failed");
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
    let input = input.to_string();
    let inbound_id = inbound_id.to_string();
    let images = images.to_vec();
    let db = db.clone();
    let lock_key = session_key.clone();
    with_chat_lock(&lock_key, async move {
        if !sink.authorized().await
            || !claim_inbound(
                &db,
                platform,
                user_id,
                &format!("{session_key}:{inbound_id}"),
            )
            .await
        {
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
                user_id,
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

    if is_active(session_key).await {
        let _ = sink
            .send_text("上一件事还在处理中；可发送 /status 查看，或 /stop 停止。")
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
            PendingDecision::Resume {
                kind,
                answer,
                confirmed,
            } => {
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
                    confirmed,
                    sink,
                    input,
                    pending,
                )
                .await;
                return;
            }
        }
    }

    start_new_work(
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
    platform: &str,
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
    platform: &str,
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
    if let Err(error) = agent
        .revoke_session_confirmations(user_id, session_id)
        .await
    {
        warn!(%error, "channel confirmation revocation failed");
    }
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

async fn start_new_work(
    db: DatabaseConnection,
    claims: Claims,
    user_id: i32,
    session_id: String,
    input: &str,
    images: &[ChannelImageRef],
    sink: ChannelSink,
    session_key: &str,
) {
    sink.send_typing().await;
    let custom_data = match cache_inbound_images(&sink.transport, images).await {
        Ok(data) => data,
        Err(message) => {
            let _ = sink.send_text(&message).await;
            return;
        }
    };
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
                custom_data,
                intention_id: None,
                autonomy_permission_cap: None,
                rig_state: None,
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
    confirmed: Option<bool>,
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
                    }),
                },
            )
            .await
            .map_err(|error| error.0.to_json())
        }
        PendingKind::Confirm { confirmation_id } => crate::api::agent::start_confirm_run(
            db.clone(),
            claims,
            confirmation_id,
            confirmed.unwrap_or(false),
            None,
        )
        .await
        .map_err(|error| error.0.to_json()),
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
                    pending_ns(sink.platform()),
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

async fn claims_for_user(db: &DatabaseConnection, user_id: i32) -> Result<Claims, DbErr> {
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
    let token_version: i64 = row
        .try_get::<i32>("", "token_version")
        .ok()
        .map(i64::from)
        .or_else(|| row.try_get::<i64>("", "token_version").ok())
        .unwrap_or(0);
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
    platform: &str,
    user_id: i32,
    session_key: &str,
) -> Result<StoredSession, DbErr> {
    if let Some(stored) =
        shared_registry::get::<StoredSession>(db, session_ns(platform), session_key).await?
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
    };
    put_session(db, platform, user_id, session_key, stored.clone()).await?;
    Ok(stored)
}

async fn put_session(
    db: &DatabaseConnection,
    platform: &str,
    user_id: i32,
    session_key: &str,
    stored: StoredSession,
) -> Result<(), DbErr> {
    shared_registry::put(
        db,
        session_ns(platform),
        session_key,
        identity(user_id),
        &stored,
        (Utc::now() + ChronoDuration::seconds(BINDING_TTL_SECS)).timestamp(),
    )
    .await
}

async fn load_session(
    db: &DatabaseConnection,
    platform: &str,
    session_key: &str,
) -> Option<StoredSession> {
    shared_registry::get::<StoredSession>(db, session_ns(platform), session_key)
        .await
        .ok()
        .flatten()
}

async fn load_pending(
    db: &DatabaseConnection,
    platform: &str,
    session_key: &str,
) -> Option<StoredPending> {
    if session_key.is_empty() {
        return None;
    }
    match shared_registry::get::<StoredPending>(db, pending_ns(platform), session_key).await {
        Ok(value) => value,
        Err(error) => {
            warn!(%error, "channel pending load failed");
            None
        }
    }
}

async fn take_pending_if_id(
    db: &DatabaseConnection,
    platform: &str,
    session_key: &str,
    expected_id: &str,
) -> Option<StoredPending> {
    let taken =
        match shared_registry::take::<StoredPending>(db, pending_ns(platform), session_key).await {
            Ok(value) => value,
            Err(error) => {
                warn!(%error, "channel pending take failed");
                return None;
            }
        };
    let Some(pending) = taken else {
        return None;
    };
    let mut prompt = pending.prompt.clone();
    ensure_pending_id(&mut prompt);
    if !expected_id.is_empty() && prompt.id != expected_id {
        let _ = shared_registry::put(
            db,
            pending_ns(platform),
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

async fn clear_pending(db: &DatabaseConnection, platform: &str, session_key: &str) {
    if session_key.is_empty() {
        return;
    }
    if let Err(error) =
        shared_registry::take::<StoredPending>(db, pending_ns(platform), session_key).await
    {
        warn!(%error, "channel pending clear failed");
    }
}

mod presentation;
use presentation::map_progress;

mod runtime;
use runtime::{is_active, owns_run, start_delivery, stop_delivery, with_chat_lock};
pub(crate) use runtime::{revoke_pairing, spawn_recovery_worker};
mod delivery;
use delivery::{clear_outbound, deliver_run, flush_outbound};
