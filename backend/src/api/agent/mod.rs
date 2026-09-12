//! Agent API handlers and routes.
//!
//! Mechanical split of the former monolithic `agent.rs`.

//! Agent API 端点
//!
//! 提供 AI Agent 自然语言任务编排的 HTTP 接口

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    Extension, Json,
};
use chrono::Utc;
use futures::stream::Stream;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
pub(crate) mod touch;
use tokio_stream::StreamExt;

use crate::middleware::auth::Claims;
use crate::models::entities::{agent_messages, agent_sessions, agent_task_presets};
use crate::services::agent::queue::LaneQueue;
use crate::services::agent::run_hub::{create_run, get_run_for_user, AgentRun};
use crate::services::agent::{
    Agent, AgentProgressEvent, AgentResponse, AgentResponseType, RequestContext, TaskState,
    UserAnswer, UserRequest, LANE_QUEUE,
};

/// 等待用户回答的任务上下文
/// `spawn_restored_wait_loop` 注册后等待 oneshot；answer / cancel / interrupt 都可 send `done_tx`
struct WaitingTaskCtx {
    /// 任务所有者；take 时必须匹配，防止跨用户抢 oneshot
    user_id: i32,
    /// 后端 run 的进度 sender；answer 阶段继续写入同一个 run hub
    progress_tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
    /// 单次信号：answer 处理完成后将最终 response Value 发送至此
    done_tx: tokio::sync::oneshot::Sender<serde_json::Value>,
    /// 会话 ID（用于持久化用户的问答消息到 agent_messages）
    session_id: String,
}

static WAITING_TASKS: once_cell::sync::Lazy<
    tokio::sync::RwLock<std::collections::HashMap<String, WaitingTaskCtx>>,
> = once_cell::sync::Lazy::new(|| tokio::sync::RwLock::new(std::collections::HashMap::new()));

/// Channel stop / HTTP cancel share this so a waiting run does not hang.
pub(crate) async fn cancel_task_and_wake(
    db: &DatabaseConnection,
    user_id: i32,
    task_id: &str,
) -> bool {
    let agent = Agent::new(db.clone()).await;
    if !agent.cancel_task_for_user(task_id, user_id).await {
        return false;
    }
    if let Some(waiting) = take_waiting_task(task_id, user_id).await {
        let _ = waiting.done_tx.send(json!({
            "success": false,
            "responseType": "error",
            "message": "任务已取消",
            "streamTerminal": true,
            "task": {
                "taskId": task_id,
                "status": "cancelled",
                "progress": 0
            }
        }));
    }
    true
}

/// 仅任务所有者可取出 waiting 上下文；错误用户不 remove，避免抢 oneshot
async fn take_waiting_task(task_id: &str, user_id: i32) -> Option<WaitingTaskCtx> {
    let mut map = WAITING_TASKS.write().await;
    match map.get(task_id) {
        Some(ctx) if ctx.user_id != user_id => {
            tracing::warn!(
                task_id = %task_id,
                caller = user_id,
                owner = ctx.user_id,
                "[Agent API] WAITING_TASKS ownership mismatch"
            );
            None
        }
        Some(_) => map.remove(task_id),
        None => None,
    }
}

fn agent_run_event_is_terminal(event: &AgentProgressEvent) -> bool {
    match event {
        AgentProgressEvent::TaskCompleted { response, .. } => {
            response.get("streamTerminal").and_then(Value::as_bool) == Some(true)
                || response.pointer("/task/status").and_then(Value::as_str)
                    != Some("waiting_for_input")
        }
        AgentProgressEvent::Error { .. } => true,
        _ => false,
    }
}

/// Terminal payload when the wait-loop oneshot is dropped without a normal answer.
/// Re-subscribers must not hang forever on a non-completed run.
fn wait_loop_channel_dropped_response(task_id: &str) -> Value {
    json!({
        "success": false,
        "responseType": "error",
        "message": "The wait channel closed",
        "code": "wait_channel_closed",
        "streamTerminal": true,
        "task": {
            "taskId": task_id,
            "status": "failed",
            "progress": 0
        }
    })
}

/// Build the TaskCompleted event published when a wait-loop oneshot is dropped.
fn wait_loop_channel_dropped_event(task_id: &str) -> AgentProgressEvent {
    AgentProgressEvent::TaskCompleted {
        task_id: task_id.to_string(),
        success: false,
        response: Box::new(wait_loop_channel_dropped_response(task_id)),
    }
}

/// Ensure session-message metadata always carries top-level run/task ids for reattach.
/// Merges into an existing JSON object (e.g. ApiResponse value) without dropping fields.
pub(crate) fn session_metadata_with_run_identity(
    base: Option<Value>,
    run_id: &str,
    task_id: &str,
) -> Value {
    let mut meta = match base {
        Some(Value::Object(map)) => Value::Object(map),
        Some(other) => json!({ "data": other }),
        None => json!({}),
    };
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("runId".to_string(), json!(run_id));
        obj.insert("taskId".to_string(), json!(task_id));
        // snake_case aliases `run_id` / `task_id`
        obj.insert("run_id".to_string(), json!(run_id));
        obj.insert("task_id".to_string(), json!(task_id));
        if !obj.contains_key("task") {
            obj.insert(
                "task".to_string(),
                json!({ "taskId": task_id, "status": "running" }),
            );
        }
    }
    meta
}

fn agent_run_event_stream(run: Arc<AgentRun>) -> impl Stream<Item = Result<Event, Infallible>> {
    agent_run_envelopes(run).map(|envelope| {
        let data = serde_json::to_string(&envelope.event).unwrap_or_else(|_| "{}".to_string());
        Ok(Event::default()
            .id(envelope.sequence.to_string())
            .data(data))
    })
}

pub(crate) fn agent_run_envelopes(
    run: Arc<AgentRun>,
) -> impl Stream<Item = crate::services::agent::run_hub::AgentRunEnvelope> {
    async_stream::stream! {
        // 先订阅再读取快照；sequence 去重消除两者之间的竞态。
        // mut: Lagged 时会重新 subscribe 同一 run。
        let mut receiver = run.subscribe();
        let (history, mut last_sequence, already_completed) = run.snapshot().await;

        if !history.iter().any(|envelope| matches!(
            &envelope.event,
            AgentProgressEvent::RunStarted { .. }
        )) {
            let event = AgentProgressEvent::RunStarted {
                run_id: run.run_id().to_string(),
                session_id: run.session_id().map(str::to_string),
            };
            yield crate::services::agent::run_hub::AgentRunEnvelope { sequence: 0, event };
        }

        for envelope in history {
            yield envelope;
        }
        if already_completed {
            return;
        }

        let mut registry_poll = tokio::time::interval(Duration::from_secs(2));
        registry_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                received = receiver.recv() => match received {
                    Ok(envelope) if envelope.sequence > last_sequence => {
                        last_sequence = envelope.sequence;
                        let terminal = agent_run_event_is_terminal(&envelope.event);
                        yield envelope;
                        if terminal {
                            return;
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // 不关流：从内存快照补发遗漏事件，避免前端必须重开连接。
                        receiver = run.subscribe();
                        let (history, snap_seq, completed) = run.snapshot().await;
                        for envelope in history {
                            if envelope.sequence <= last_sequence {
                                continue;
                            }
                            last_sequence = envelope.sequence;
                            let terminal = agent_run_event_is_terminal(&envelope.event);
                            yield envelope;
                            if terminal {
                                return;
                            }
                        }
                        last_sequence = last_sequence.max(snap_seq);
                        if completed {
                            return;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                },
                _ = registry_poll.tick() => {
                    for envelope in run.refresh_from_registry().await {
                        if envelope.sequence <= last_sequence {
                            continue;
                        }
                        last_sequence = envelope.sequence;
                        let terminal = agent_run_event_is_terminal(&envelope.event);
                        yield envelope;
                        if terminal {
                            return;
                        }
                    }
                }
            }
        }
    }
}

mod autonomy_dispatch;
mod boot;
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
mod types;

pub use autonomy_dispatch::*;
pub use boot::*;
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
pub use types::*;
