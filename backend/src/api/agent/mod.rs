//! Agent API handlers and routes.
//!
//! Mechanical split of the former monolithic `agent.rs`.

//! Agent API 端点
//!
//! 提供 AI Agent 自然语言任务编排的 HTTP 接口

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
};
use chrono::Utc;
use futures::stream::Stream;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
pub(crate) mod touch;
use tokio_stream::StreamExt;

use crate::middleware::auth::Claims;
use crate::models::entities::{agent_messages, agent_sessions, agent_task_presets};
use crate::services::agent::queue::LaneQueue;
use crate::services::agent::run_hub::{AgentRun, get_run_for_user};
use crate::services::agent::{
    Agent, AgentProgressEvent, LANE_QUEUE, RequestContext, UserAnswer, UserRequest};

/// 等待用户回答的任务上下文
/// `spawn_restored_wait_loop` 注册后等待 oneshot；answer / cancel / interrupt 都可 send `done_tx`
fn agent_run_event_stream(run: Arc<AgentRun>) -> impl Stream<Item = Result<Event, Infallible>> {
    agent_run_envelopes(run).map(|envelope| {
        let data = serde_json::to_string(&envelope.event).unwrap_or_else(|_| "{}".to_string());
        Ok(Event::default()
            .id(envelope.sequence.to_string())
            .data(data))
    })
}

mod autonomy_dispatch;
mod discord_pairing;
mod discord_status;
mod feishu_pairing;
mod feishu_status;
mod heartbeat_mcp;
mod helpers;
mod intentions;
mod notifications;
mod persona;
mod playback_direction;
mod presence;
mod presets;
mod process;
mod qq_pairing;
mod qq_status;
mod routes;
mod sessions;
mod telegram_pairing;
mod telegram_status;

pub(crate) use crate::services::agent::run::*;
pub use autonomy_dispatch::*;
pub use discord_pairing::*;
pub use discord_status::*;
pub use feishu_pairing::*;
pub use feishu_status::*;
pub use heartbeat_mcp::*;
pub use helpers::*;
pub use intentions::*;
pub use notifications::*;
pub use presence::*;
pub use presets::*;
pub use process::*;
pub use qq_pairing::*;
pub use qq_status::*;
pub use routes::*;
pub use sessions::*;
pub use telegram_pairing::*;
pub use telegram_status::*;
