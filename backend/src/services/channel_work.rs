//! Shared private-chat Work: session, pending, cursor, outbound ledger.
//!
//! QQ and Telegram only adapt send/receive. Rules stay in
//! `myriad_agent_rules::channel`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use myriad_agent_rules::channel::{
    channel_can_finish, clarify_base_input, clarify_followup, collect_channel_image_urls,
    decide_pending_reply, discord_dm_capabilities, discord_reply_markup, ensure_pending_id,
    format_channel_result, format_pending_prompt, panel_entry_reply, parse_channel_command,
    pending_prompt_from_model_json, plan_delivery, qq_c2c_capabilities, should_deliver_sequence,
    split_channel_text, telegram_callback_action, telegram_dm_capabilities,
    telegram_force_reply_markup, telegram_reply_markup, ChannelCommand, ChannelEvent,
    ChannelImageRef, DeliveryContext, DeliveryPlan, PendingDecision, PendingKind, PendingOption,
    PendingPrompt, TelegramCallbackAction, CHANNEL_NEW_SESSION_REPLY, CHANNEL_STOP_REPLY,
    DISCORD_TEXT_LIMIT, PANEL_REQUIRED_REPLY, PENDING_STALE_REPLY, QQ_TEXT_LIMIT,
    TELEGRAM_TEXT_LIMIT,
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

static CHAT_LOCKS: Lazy<Mutex<HashMap<String, Arc<Mutex<()>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSession {
    session_id: String,
    #[serde(default)]
    last_run_id: Option<String>,
    #[serde(default)]
    last_event_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPending {
    prompt: PendingPrompt,
    #[serde(default)]
    last_event_seq: u64,
    #[serde(default)]
    expected_user_id: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredOutbound {
    chunks: Vec<String>,
    #[serde(default)]
    next_index: usize,
    prompt: Option<PendingPrompt>,
}

#[derive(Debug, Clone)]
pub enum ChannelSink {
    Telegram {
        token: String,
        chat_id: String,
    },
    Discord {
        token: String,
        channel_id: String,
    },
    Qq {
        db: DatabaseConnection,
        auth_header: String,
        openid: String,
        inbound_msg_id: Option<String>,
    },
}

impl ChannelSink {
    fn platform(&self) -> &'static str {
        match self {
            Self::Telegram { .. } => "telegram",
            Self::Discord { .. } => "discord",
            Self::Qq { .. } => "qq",
        }
    }

    fn capabilities(&self) -> myriad_agent_rules::channel::ChannelCapabilities {
        match self {
            Self::Telegram { .. } => telegram_dm_capabilities(),
            Self::Discord { .. } => discord_dm_capabilities(),
            Self::Qq { .. } => qq_c2c_capabilities(),
        }
    }

    fn text_limit(&self) -> usize {
        match self {
            Self::Telegram { .. } => TELEGRAM_TEXT_LIMIT,
            Self::Discord { .. } => DISCORD_TEXT_LIMIT,
            Self::Qq { .. } => QQ_TEXT_LIMIT,
        }
    }

    fn delivery_context(&self) -> DeliveryContext {
        match self {
            Self::Telegram { .. } | Self::Discord { .. } => DeliveryContext {
                inbound_msg_id: None,
                passive_window_open: false,
                remaining_passive_replies: 0,
            },
            Self::Qq { inbound_msg_id, .. } => DeliveryContext {
                inbound_msg_id: inbound_msg_id.clone(),
                passive_window_open: inbound_msg_id.as_ref().is_some_and(|id| !id.is_empty()),
                remaining_passive_replies: 4,
            },
        }
    }

    async fn send_typing(&self) {
        match self {
            Self::Telegram { token, chat_id } => {
                if let Err(error) = crate::services::telegram_bot::send_typing(token, chat_id).await
                {
                    warn!(?error, "channel typing failed");
                }
            }
            Self::Discord { token, channel_id } => {
                if let Err(error) =
                    crate::services::discord_bot::send_typing(token, channel_id).await
                {
                    warn!(?error, "channel typing failed");
                }
            }
            Self::Qq { .. } => {}
        }
    }

    async fn send_force_reply(&self, placeholder: &str) -> Result<(), String> {
        match self {
            Self::Telegram { token, chat_id } => crate::services::telegram_bot::send_outbound(
                token,
                chat_id,
                "请在这里输入。",
                Some(telegram_force_reply_markup(placeholder)),
            )
            .await
            .map_err(|error| format!("{error:?}")),
            Self::Discord { .. } | Self::Qq { .. } => self.send_text("请直接回复这一问。").await,
        }
    }

    async fn send_text(&self, content: &str) -> Result<(), String> {
        self.send_chunks(&[content.to_string()], &[], None).await
    }

    async fn send_prompt(&self, content: &str, prompt: &PendingPrompt) -> Result<(), String> {
        self.send_chunks(&[content.to_string()], &[], Some(prompt))
            .await
    }

    async fn send_chunks(
        &self,
        chunks: &[String],
        image_urls: &[String],
        prompt: Option<&PendingPrompt>,
    ) -> Result<(), String> {
        let images = if self.capabilities().outbound_image {
            image_urls
        } else {
            &[]
        };
        if chunks.is_empty() && images.is_empty() {
            return Ok(());
        }
        let last_text = chunks.len().saturating_sub(1);
        for (index, chunk) in chunks.iter().enumerate() {
            if chunk.trim().is_empty() {
                continue;
            }
            let markup = prompt
                .filter(|_| images.is_empty() && index == last_text)
                .and_then(|prompt| match self {
                    Self::Telegram { .. } => telegram_reply_markup(prompt),
                    Self::Discord { .. } => discord_reply_markup(prompt),
                    Self::Qq { .. } => None,
                });
            self.send_text_chunk(chunk, markup).await?;
        }
        for (index, url) in images.iter().enumerate() {
            let markup =
                prompt
                    .filter(|_| index + 1 == images.len())
                    .and_then(|prompt| match self {
                        Self::Telegram { .. } => telegram_reply_markup(prompt),
                        Self::Discord { .. } => discord_reply_markup(prompt),
                        Self::Qq { .. } => None,
                    });
            self.send_image_chunk(url, markup).await?;
        }
        Ok(())
    }

    async fn send_text_chunk(&self, chunk: &str, markup: Option<Value>) -> Result<(), String> {
        match self {
            Self::Telegram { token, chat_id } => {
                crate::services::telegram_bot::send_outbound(token, chat_id, chunk, markup)
                    .await
                    .map_err(|error| format!("{error:?}"))
            }
            Self::Discord { token, channel_id } => {
                crate::services::discord_bot::send_outbound(token, channel_id, chunk, markup)
                    .await
                    .map_err(|error| format!("{error:?}"))
            }
            Self::Qq {
                db,
                auth_header,
                openid,
                inbound_msg_id,
            } => {
                crate::services::qq_work::send_c2c(
                    db,
                    auth_header,
                    openid,
                    chunk,
                    inbound_msg_id.as_deref(),
                )
                .await
            }
        }
    }

    async fn send_image_chunk(&self, url: &str, markup: Option<Value>) -> Result<(), String> {
        let image = load_channel_image_bytes(url).await?;
        match self {
            Self::Telegram { token, chat_id } => crate::services::telegram_bot::send_photo(
                token,
                chat_id,
                &image.bytes,
                &image.mime,
                markup,
            )
            .await
            .map_err(|error| format!("{error:?}")),
            Self::Discord { token, channel_id } => crate::services::discord_bot::send_photo(
                token,
                channel_id,
                &image.bytes,
                &image.mime,
                markup,
            )
            .await
            .map_err(|error| format!("{error:?}")),
            Self::Qq {
                db,
                auth_header,
                openid,
                inbound_msg_id,
            } => {
                crate::services::qq_work::send_c2c_image(
                    db,
                    auth_header,
                    openid,
                    &image.bytes,
                    inbound_msg_id.as_deref(),
                )
                .await
            }
        }
    }
}

async fn load_channel_image_bytes(url: &str) -> Result<ChannelImageBytes, String> {
    let cache = crate::services::image_cache::ImageCacheService::new();
    if cache.local_path_for_public_url(url).is_some() {
        let (bytes, mime) = cache.read_local_public_url(url).await?;
        return Ok(ChannelImageBytes { bytes, mime });
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("imageUrl is not a sendable path".to_string());
    }
    let cached = cache.cache_image(url).await?;
    let (bytes, mime) = cache.read_local_public_url(&cached).await?;
    Ok(ChannelImageBytes { bytes, mime })
}

struct ChannelImageBytes {
    bytes: Vec<u8>,
    mime: String,
}

async fn cache_inbound_images(sink: &ChannelSink, images: &[ChannelImageRef]) -> Option<Value> {
    if images.is_empty() || !sink.capabilities().inbound_media {
        return None;
    }
    let cache = crate::services::image_cache::ImageCacheService::new();
    let mut attachments = Vec::new();
    for image in images.iter().take(4) {
        match resolve_inbound_image(sink, image, &cache).await {
            Ok((url, mime, size, name)) => attachments.push(serde_json::json!({
                "name": name,
                "mime": mime,
                "size": size,
                "url": url,
            })),
            Err(error) => warn!(%error, "channel inbound image cache failed"),
        }
    }
    (!attachments.is_empty()).then(|| serde_json::json!({ "attachments": attachments }))
}

async fn resolve_inbound_image(
    sink: &ChannelSink,
    image: &ChannelImageRef,
    cache: &crate::services::image_cache::ImageCacheService,
) -> Result<(String, String, usize, String), String> {
    if cache.local_path_for_public_url(&image.url).is_some() {
        let (bytes, mime) = cache.read_local_public_url(&image.url).await?;
        return Ok((image.url.clone(), mime, bytes.len(), image.name.clone()));
    }
    if let ChannelSink::Telegram { token, .. } = sink {
        if let Some(file_id) = image.url.strip_prefix("tg:") {
            let (bytes, mime) =
                crate::services::telegram_bot::download_file_bytes(token, file_id).await?;
            let stored = cache.store_bytes_with_status(&bytes, &mime).await?;
            return Ok((
                stored.url,
                if image.mime.starts_with("image/") {
                    image.mime.clone()
                } else {
                    mime
                },
                bytes.len(),
                image.name.clone(),
            ));
        }
    }
    let cached = cache.cache_image(&image.url).await?;
    finish_cached(cache, image, cached).await
}

async fn finish_cached(
    cache: &crate::services::image_cache::ImageCacheService,
    image: &ChannelImageRef,
    cached: String,
) -> Result<(String, String, usize, String), String> {
    let (bytes, mime) = cache.read_local_public_url(&cached).await?;
    Ok((
        cached,
        if image.mime.starts_with("image/") {
            image.mime.clone()
        } else {
            mime
        },
        bytes.len(),
        image.name.clone(),
    ))
}

fn session_ns(platform: &str) -> &'static str {
    match platform {
        "telegram" => "telegram_dm_session",
        "discord" => "discord_dm_session",
        _ => "qq_c2c_session",
    }
}

fn pending_ns(platform: &str) -> &'static str {
    match platform {
        "telegram" => "telegram_dm_pending",
        "discord" => "discord_dm_pending",
        _ => "qq_c2c_pending",
    }
}

fn outbound_ns(platform: &str) -> &'static str {
    match platform {
        "telegram" => "telegram_dm_outbound",
        "discord" => "discord_dm_outbound",
        _ => "qq_c2c_outbound",
    }
}

fn inbound_ns(platform: &str) -> &'static str {
    match platform {
        "telegram" => "telegram_dm_update",
        "discord" => "discord_dm_msg",
        _ => "qq_c2c_msg",
    }
}

async fn with_chat_lock<T>(session_key: &str, fut: impl std::future::Future<Output = T>) -> T {
    let lock = {
        let mut locks = CHAT_LOCKS.lock().await;
        locks
            .entry(session_key.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    let _guard = lock.lock().await;
    fut.await
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
    chat_id: &str,
    input: &str,
    images: &[ChannelImageRef],
    session_key: &str,
    inbound_id: &str,
    sink: ChannelSink,
) {
    let platform = sink.platform();
    let session_key = session_key.to_string();
    let chat_id = chat_id.to_string();
    let input = input.to_string();
    let inbound_id = inbound_id.to_string();
    let images = images.to_vec();
    let db = db.clone();
    let lock_key = session_key.clone();
    with_chat_lock(&lock_key, async move {
        if !claim_inbound(&db, platform, user_id, &inbound_id).await {
            return;
        }
        continue_text(&db, user_id, &chat_id, &input, &images, &session_key, sink).await;
    })
    .await;
}

pub async fn handle_callback(
    db: &DatabaseConnection,
    user_id: i32,
    chat_id: &str,
    data: &str,
    session_key: &str,
    inbound_id: &str,
    sink: ChannelSink,
) {
    let platform = sink.platform();
    let session_key = session_key.to_string();
    let chat_id = chat_id.to_string();
    let data = data.to_string();
    let inbound_id = inbound_id.to_string();
    let db = db.clone();
    let lock_key = session_key.clone();
    with_chat_lock(&lock_key, async move {
        if !claim_inbound(&db, platform, user_id, &inbound_id).await {
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
    flush_outbound(db, user_id, session_key, &sink).await;
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
            String::new()
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
            cancel_session_tasks(db, user_id, session_id).await;
            clear_pending(db, platform, session_key).await;
            clear_outbound(db, platform, session_key).await;
            let _ = sink.send_text(CHANNEL_STOP_REPLY).await;
        }
        ChannelCommand::NewConversation => {
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
    let custom_data = cache_inbound_images(&sink, images).await;
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
    deliver_run(
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
            deliver_run(
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

async fn save_cursor(
    db: &DatabaseConnection,
    platform: &str,
    user_id: i32,
    session_key: &str,
    run_id: &str,
    sequence: u64,
) {
    let mut stored = load_session(db, platform, session_key)
        .await
        .unwrap_or(StoredSession {
            session_id: String::new(),
            last_run_id: None,
            last_event_seq: 0,
        });
    stored.last_run_id = Some(run_id.to_string());
    stored.last_event_seq = sequence;
    if let Err(error) = put_session(db, platform, user_id, session_key, stored).await {
        warn!(%error, "channel cursor save failed");
    }
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

async fn save_pending(
    db: &DatabaseConnection,
    platform: &str,
    user_id: i32,
    session_key: &str,
    mut prompt: PendingPrompt,
    last_event_seq: u64,
) {
    if session_key.is_empty() {
        return;
    }
    ensure_pending_id(&mut prompt);
    if let Err(error) = shared_registry::put(
        db,
        pending_ns(platform),
        session_key,
        identity(user_id),
        &StoredPending {
            prompt,
            last_event_seq,
            expected_user_id: Some(user_id),
        },
        (Utc::now() + ChronoDuration::days(2)).timestamp(),
    )
    .await
    {
        warn!(%error, "channel pending save failed");
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

async fn save_outbound(
    db: &DatabaseConnection,
    platform: &str,
    user_id: i32,
    session_key: &str,
    chunks: Vec<String>,
    prompt: Option<PendingPrompt>,
) {
    if session_key.is_empty() || chunks.is_empty() {
        return;
    }
    let _ = shared_registry::put(
        db,
        outbound_ns(platform),
        session_key,
        identity(user_id),
        &StoredOutbound {
            chunks,
            next_index: 0,
            prompt,
        },
        (Utc::now() + ChronoDuration::days(2)).timestamp(),
    )
    .await;
}

async fn clear_outbound(db: &DatabaseConnection, platform: &str, session_key: &str) {
    let _ = shared_registry::take::<StoredOutbound>(db, outbound_ns(platform), session_key).await;
}

async fn flush_outbound(
    db: &DatabaseConnection,
    user_id: i32,
    session_key: &str,
    sink: &ChannelSink,
) {
    let platform = sink.platform();
    let Some(stored) =
        shared_registry::get::<StoredOutbound>(db, outbound_ns(platform), session_key)
            .await
            .ok()
            .flatten()
    else {
        return;
    };
    if stored.next_index >= stored.chunks.len() {
        clear_outbound(db, platform, session_key).await;
        return;
    }
    let remaining = stored.chunks[stored.next_index..].to_vec();
    let prompt = stored.prompt.clone();
    match sink.send_chunks(&remaining, &[], prompt.as_ref()).await {
        Ok(()) => clear_outbound(db, platform, session_key).await,
        Err(error) => {
            warn!(%error, "channel outbound flush failed");
            let _ = shared_registry::put(
                db,
                outbound_ns(platform),
                session_key,
                identity(user_id),
                &stored,
                (Utc::now() + ChronoDuration::days(2)).timestamp(),
            )
            .await;
        }
    }
}

async fn deliver_prepared(
    db: &DatabaseConnection,
    user_id: i32,
    session_key: &str,
    sink: &ChannelSink,
    content: &str,
    image_urls: &[String],
    prompt: Option<PendingPrompt>,
) {
    let platform = sink.platform();
    let chunks = split_channel_text(content, sink.text_limit());
    save_outbound(
        db,
        platform,
        user_id,
        session_key,
        chunks.clone(),
        prompt.clone(),
    )
    .await;
    match sink.send_chunks(&chunks, image_urls, prompt.as_ref()).await {
        Ok(()) => clear_outbound(db, platform, session_key).await,
        Err(error) => warn!(%error, "channel send failed; outbound kept for retry"),
    }
}

async fn deliver_run(
    run: Arc<AgentRun>,
    db: DatabaseConnection,
    user_id: i32,
    session_key: String,
    sink: ChannelSink,
    original_input: String,
) {
    let platform = sink.platform();
    sink.send_typing().await;
    let run_id = run.run_id().to_string();
    let cursor = load_session(&db, platform, &session_key).await;
    let after = if cursor
        .as_ref()
        .and_then(|stored| stored.last_run_id.as_deref())
        == Some(run_id.as_str())
    {
        cursor.as_ref().map(|stored| stored.last_event_seq)
    } else {
        None
    };
    let mut envelopes = Box::pin(crate::api::agent::agent_run_envelopes(run));
    let mut refresh = tokio::time::interval(TYPING_REFRESH);
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    refresh.tick().await;
    loop {
        tokio::select! {
            envelope = futures::StreamExt::next(&mut envelopes) => {
                let Some(envelope) = envelope else {
                    return;
                };
                if !should_deliver_sequence(after, envelope.sequence) {
                    continue;
                }
                if let Some((event, parked)) = map_progress(&envelope.event, &original_input, &sink.capabilities()) {
                    save_cursor(&db, platform, user_id, &session_key, &run_id, envelope.sequence).await;
                    if let Some(prompt) = parked.as_ref() {
                        save_pending(
                            &db,
                            platform,
                            user_id,
                            &session_key,
                            prompt.clone(),
                            envelope.sequence,
                        )
                        .await;
                    } else {
                        clear_pending(&db, platform, &session_key).await;
                    }
                    match plan_delivery(&event, &sink.delivery_context()) {
                        DeliveryPlan::Drop => {}
                        DeliveryPlan::ActiveText {
                            content,
                            image_urls,
                        }
                        | DeliveryPlan::PassiveText {
                            content,
                            image_urls,
                            ..
                        } => {
                            deliver_prepared(
                                &db,
                                user_id,
                                &session_key,
                                &sink,
                                &content,
                                &image_urls,
                                parked,
                            )
                            .await;
                            return;
                        }
                        DeliveryPlan::FailVisible { content, .. } => {
                            deliver_prepared(
                                &db,
                                user_id,
                                &session_key,
                                &sink,
                                &content,
                                &[],
                                parked,
                            )
                            .await;
                            return;
                        }
                    }
                }
            }
            _ = refresh.tick() => {
                sink.send_typing().await;
            }
        }
    }
}

pub(crate) fn map_progress(
    event: &AgentProgressEvent,
    original_input: &str,
    caps: &myriad_agent_rules::channel::ChannelCapabilities,
) -> Option<(ChannelEvent, Option<PendingPrompt>)> {
    match event {
        AgentProgressEvent::WaitingForInput {
            task_id,
            question_id,
            question_type,
            question,
            options,
            ..
        } => {
            if !caps.interactive {
                return Some((ChannelEvent::ConfirmationRequired, None));
            }
            let mut prompt = PendingPrompt {
                id: String::new(),
                kind: PendingKind::Answer {
                    task_id: task_id.clone(),
                    question_id: question_id.clone(),
                    question_type: question_type.clone(),
                },
                question: question.clone(),
                options: options
                    .as_ref()
                    .map(|rows| {
                        rows.iter()
                            .map(|row| PendingOption {
                                value: row.value.clone(),
                                label: if row.label.trim().is_empty() {
                                    row.value.clone()
                                } else {
                                    row.label.clone()
                                },
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                expires_at_unix: None,
            };
            ensure_pending_id(&mut prompt);
            Some((
                ChannelEvent::Answer {
                    message: format_pending_prompt(&prompt),
                    image_urls: Vec::new(),
                },
                Some(prompt),
            ))
        }
        AgentProgressEvent::Error { message, .. } => Some((
            ChannelEvent::Error {
                message: message.clone(),
            },
            None,
        )),
        AgentProgressEvent::TaskCompleted { response, .. } => {
            Some(map_completed(response, original_input, caps))
        }
        AgentProgressEvent::StepCompleted {
            frontend_actions, ..
        } if !frontend_actions.is_empty() => Some((ChannelEvent::FrontendAction, None)),
        AgentProgressEvent::ThinkingToken { .. }
        | AgentProgressEvent::StepStarted { .. }
        | AgentProgressEvent::StepCompleted { .. }
        | AgentProgressEvent::Progress { .. }
        | AgentProgressEvent::StepRetrying { .. }
        | AgentProgressEvent::RunStarted { .. }
        | AgentProgressEvent::SessionCreated { .. }
        | AgentProgressEvent::SessionTitleUpdated { .. }
        | AgentProgressEvent::SummaryToken { .. }
        | AgentProgressEvent::TaskCreated { .. }
        | AgentProgressEvent::TaskAssigned { .. }
        | AgentProgressEvent::PlannerDecision { .. }
        | AgentProgressEvent::StepDebug { .. }
        | AgentProgressEvent::PerformancePlan { .. }
        | AgentProgressEvent::MeropeStateChanged { .. }
        | AgentProgressEvent::OutfitOverlay { .. }
        | AgentProgressEvent::MusicControl { .. } => None,
    }
}

pub(crate) fn map_completed(
    response: &Value,
    original_input: &str,
    caps: &myriad_agent_rules::channel::ChannelCapabilities,
) -> (ChannelEvent, Option<PendingPrompt>) {
    if response.get("frontendAction").is_some() {
        let session_id = response.get("sessionId").and_then(Value::as_str);
        let task_id = response
            .get("task")
            .and_then(|task| task.get("taskId"))
            .and_then(Value::as_str);
        return (
            ChannelEvent::Error {
                message: panel_entry_reply(session_id, task_id),
            },
            None,
        );
    }
    let response_type = response
        .get("responseType")
        .and_then(Value::as_str)
        .unwrap_or("");
    if response_type == "confirmation_required" || response.get("confirmation").is_some() {
        if !caps.interactive {
            return (ChannelEvent::ConfirmationRequired, None);
        }
        if let Some(prompt) = confirmation_prompt(response) {
            return (
                ChannelEvent::Answer {
                    message: format_pending_prompt(&prompt),
                    image_urls: Vec::new(),
                },
                Some(prompt),
            );
        }
        return (ChannelEvent::ConfirmationRequired, None);
    }
    if response_type == "clarification" {
        if !caps.interactive {
            return (ChannelEvent::ConfirmationRequired, None);
        }
        let prompt = clarification_prompt(response, original_input);
        return (
            ChannelEvent::Answer {
                message: format_pending_prompt(&prompt),
                image_urls: Vec::new(),
            },
            Some(prompt),
        );
    }
    let message = response
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if let Some(prompt) = pending_prompt_from_model_json(message, original_input) {
        if caps.interactive {
            return (
                ChannelEvent::Answer {
                    message: format_pending_prompt(&prompt),
                    image_urls: Vec::new(),
                },
                Some(prompt),
            );
        }
    }
    let image_urls = if caps.outbound_image {
        collect_channel_image_urls(response)
    } else {
        Vec::new()
    };
    let data = if image_urls.is_empty() || response.get("dataDisplay").is_some() {
        response.get("data")
    } else {
        None
    };
    let rendered = format_channel_result(message, data, response.get("dataDisplay"));
    if response.get("success").and_then(Value::as_bool) == Some(false) || response_type == "error" {
        return (
            ChannelEvent::Error {
                message: if rendered.is_empty() {
                    "办事失败。".into()
                } else {
                    rendered
                },
            },
            None,
        );
    }
    let event = ChannelEvent::Answer {
        message: if rendered.is_empty() && image_urls.is_empty() {
            PANEL_REQUIRED_REPLY.to_string()
        } else {
            rendered
        },
        image_urls,
    };
    if !channel_can_finish(caps, &event) {
        return (ChannelEvent::FrontendAction, None);
    }
    (event, None)
}

fn confirmation_prompt(response: &Value) -> Option<PendingPrompt> {
    let confirmation = response.get("confirmation")?;
    let confirmation_id = confirmation
        .get("confirmationId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())?;
    let question = response
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let step = confirmation
        .get("pendingSteps")
        .and_then(Value::as_array)
        .and_then(|steps| steps.first())
        .and_then(|step| step.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let question = if question.is_empty() {
        step.to_string()
    } else if step.is_empty() || question.contains(step) {
        question.to_string()
    } else {
        format!("{question}\n{step}")
    };
    let expires_in = confirmation
        .get("expiresInSeconds")
        .and_then(Value::as_i64)
        .filter(|secs| *secs > 0);
    let mut prompt = PendingPrompt {
        id: String::new(),
        kind: PendingKind::Confirm {
            confirmation_id: confirmation_id.to_string(),
        },
        question,
        options: Vec::new(),
        expires_at_unix: expires_in.map(|secs| Utc::now().timestamp().saturating_add(secs)),
    };
    ensure_pending_id(&mut prompt);
    Some(prompt)
}

fn clarification_prompt(response: &Value, original_input: &str) -> PendingPrompt {
    let question = response
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let mut options = json_string_options(response.get("suggestions"));
    if options.is_empty() {
        options = json_string_options(
            response
                .get("clarification")
                .and_then(|value| value.get("options")),
        );
    }
    let mut prompt = PendingPrompt {
        id: String::new(),
        kind: PendingKind::Clarify {
            original_input: clarify_base_input(original_input).to_string(),
        },
        question: question.clone(),
        options,
        expires_at_unix: None,
    };
    if prompt.options.is_empty() {
        if let Some(from_json) = pending_prompt_from_model_json(&question, original_input) {
            return from_json;
        }
    }
    ensure_pending_id(&mut prompt);
    prompt
}

fn json_string_options(value: Option<&Value>) -> Vec<PendingOption> {
    value
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let text = row.as_str().or_else(|| {
                        row.get("label")
                            .and_then(Value::as_str)
                            .or_else(|| row.get("value").and_then(Value::as_str))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn telegram_caps() -> myriad_agent_rules::channel::ChannelCapabilities {
        telegram_dm_capabilities()
    }

    #[test]
    fn completed_answer_uses_message() {
        let (event, parked) = map_completed(
            &serde_json::json!({
                "success": true,
                "responseType": "answer",
                "message": "票已订好"
            }),
            "订票",
            &telegram_caps(),
        );
        assert_eq!(
            event,
            ChannelEvent::Answer {
                message: "票已订好".into(),
                image_urls: Vec::new(),
            }
        );
        assert!(parked.is_none());
    }

    #[test]
    fn completed_image_keeps_message_and_urls() {
        let (event, parked) = map_completed(
            &serde_json::json!({
                "success": true,
                "responseType": "answer",
                "message": "图片已经生成好了",
                "data": {
                    "format": "image",
                    "value": {
                        "url": "/api/brew/image-cache/ab/abcd.png",
                        "width": 1,
                        "height": 1
                    }
                },
                "task": {
                    "stepHistory": [{ "imageUrl": "https://cdn.example/a.png" }]
                }
            }),
            "画一只猫",
            &telegram_caps(),
        );
        let ChannelEvent::Answer {
            message,
            image_urls,
        } = event
        else {
            panic!("{event:?}");
        };
        assert_eq!(message, "图片已经生成好了");
        assert!(!message.contains("format"));
        assert_eq!(
            image_urls,
            vec![
                "/api/brew/image-cache/ab/abcd.png",
                "https://cdn.example/a.png",
            ]
        );
        assert!(parked.is_none());
    }

    #[test]
    fn completed_image_without_message_does_not_send_panel_entry() {
        let (event, parked) = map_completed(
            &serde_json::json!({
                "success": true,
                "responseType": "answer",
                "message": "",
                "data": {
                    "format": "image",
                    "value": { "url": "/api/brew/image-cache/ab/abcd.png" }
                }
            }),
            "画一只猫",
            &telegram_caps(),
        );
        let ChannelEvent::Answer {
            message,
            image_urls,
        } = event
        else {
            panic!("{event:?}");
        };
        assert!(message.is_empty());
        assert_eq!(image_urls, vec!["/api/brew/image-cache/ab/abcd.png"]);
        assert!(parked.is_none());
    }

    #[test]
    fn table_result_is_appended() {
        let (event, parked) = map_completed(
            &serde_json::json!({
                "success": true,
                "responseType": "answer",
                "message": "查到了",
                "data": [{"name": "A"}],
                "dataDisplay": {
                    "type": "table",
                    "columns": [{"field": "name", "title": "名称"}]
                }
            }),
            "查",
            &telegram_caps(),
        );
        let ChannelEvent::Answer {
            message,
            image_urls,
        } = event
        else {
            panic!("{event:?}");
        };
        assert!(image_urls.is_empty());
        assert!(message.contains("查到了"));
        assert!(message.contains("名称"));
        assert!(message.contains("A"));
        assert!(parked.is_none());
    }

    #[test]
    fn confirmation_asks_yes_or_no() {
        let (event, parked) = map_completed(
            &serde_json::json!({
                "success": true,
                "responseType": "confirmation_required",
                "message": "要删掉这篇文章吗？",
                "confirmation": {
                    "confirmationId": "c1",
                    "expiresInSeconds": 60
                }
            }),
            "删除文章",
            &telegram_caps(),
        );
        let ChannelEvent::Answer {
            message,
            image_urls,
        } = event
        else {
            panic!("{event:?}");
        };
        assert!(image_urls.is_empty());
        assert!(message.contains("要删掉这篇文章吗？"));
        let prompt = parked.expect("confirmation parks");
        assert!(!prompt.id.is_empty());
        assert!(matches!(
            prompt.kind,
            PendingKind::Confirm { confirmation_id } if confirmation_id == "c1"
        ));
    }

    #[test]
    fn frontend_action_points_at_the_session() {
        let (event, parked) = map_completed(
            &serde_json::json!({
                "success": true,
                "responseType": "answer",
                "message": "打开页面",
                "sessionId": "ses_1",
                "task": {"taskId": "t9"},
                "frontendAction": {"type": "navigate"}
            }),
            "打开",
            &telegram_caps(),
        );
        let ChannelEvent::Error { message } = event else {
            panic!("{event:?}");
        };
        assert!(message.contains("ses_1"));
        assert!(message.contains("t9"));
        assert!(parked.is_none());
    }
}
