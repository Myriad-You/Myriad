//! Shared private-chat Work: session, pending, cursor, outbound ledger.
//!
//! Transport adapters only adapt send/receive. Rules stay in
//! `myriad_agent_rules::channel`.

use crate::services::channel_platform::ChannelPlatform;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};

#[cfg(test)]
use myriad_agent_rules::channel::task_started_reply;
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
    split_channel_text, telegram_callback_action, telegram_dm_capabilities,
    telegram_force_reply_markup, telegram_reply_markup,
};
use myriad_agent_rules::{is_cancellable_task_status, session_id_from_lane_id};
use once_cell::sync::Lazy;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::services::agent::run::{ProcessContext, ProcessRequest};
use crate::services::agent::run_hub::AgentRun;
use crate::services::agent::{Agent, AgentInteractionMode, AgentProgressEvent};
use crate::services::runtime_registry::{self as shared_registry, RegistryIdentity};

const BINDING_TTL_SECS: i64 = 30 * 24 * 60 * 60;
const TYPING_REFRESH: Duration = Duration::from_secs(4);

mod chat;
mod chat_images;
mod chat_waiting;
mod transport;
use crate::services::channel_pairing::ChannelBinding;
pub use transport::ChannelTransport;
use transport::cache_inbound_images;

mod commands;
mod first_word;
mod inbound;
mod session;
mod work_run;

use commands::*;
pub(crate) use first_word::*;
pub use inbound::*;
use session::*;
use work_run::*;

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
