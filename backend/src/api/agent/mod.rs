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
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
pub(crate) mod touch;
use tokio_stream::StreamExt;

use crate::middleware::auth::Claims;
use crate::models::entities::{agent_messages, agent_sessions, agent_task_presets};
use crate::services::agent::queue::LaneQueue;
use crate::services::agent::run_hub::{AgentRun, AgentRunEnvelope, create_run, get_run_for_user};
use crate::services::agent::{
    Agent, AgentProgressEvent, AgentResponse, AgentResponseType, LANE_QUEUE, RequestContext,
    TaskState, UserAnswer, UserRequest,
};

/// 等待用户回答的任务上下文
/// `spawn_restored_wait_loop` 注册后等待 oneshot；answer / cancel / interrupt 都可 send `done_tx`
struct WaitingTaskCtx {
    registration: Arc<()>,
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
    std::sync::Mutex<std::collections::HashMap<String, WaitingTaskCtx>>,
> = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Each polling round owns exactly its context, including during task abort.
/// A synchronous lock makes Drop cleanup immediate; no map lock crosses await.
struct WaitingTaskRegistration {
    task_id: String,
    identity: Arc<()>,
}

impl WaitingTaskRegistration {
    fn insert(task_id: &str, context: WaitingTaskCtx) -> Self {
        let identity = context.registration.clone();
        WAITING_TASKS
            .lock()
            .unwrap()
            .insert(task_id.to_owned(), context);
        Self {
            task_id: task_id.to_owned(),
            identity,
        }
    }
}

impl Drop for WaitingTaskRegistration {
    fn drop(&mut self) {
        let mut tasks = WAITING_TASKS.lock().unwrap();
        if tasks
            .get(&self.task_id)
            .is_some_and(|context| Arc::ptr_eq(&context.registration, &self.identity))
        {
            tasks.remove(&self.task_id);
        }
    }
}

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
    let mut map = WAITING_TASKS.lock().unwrap();
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
    futures::stream::unfold(Some(RunEnvelopeFeed::Boot(run)), |feed| async move {
        let feed = feed?;
        return run_envelope_step(feed).await;
    })
}

enum RunEnvelopeFeed {
    Boot(Arc<AgentRun>),
    Live(RunEnvelopeLive),
}

struct RunEnvelopeLive {
    run: Arc<AgentRun>,
    receiver: tokio::sync::broadcast::Receiver<AgentRunEnvelope>,
    last_sequence: u64,
    registry_poll: tokio::time::Interval,
    pending: VecDeque<AgentRunEnvelope>,
    stop_after_pending: bool,
}

async fn run_envelope_step(
    feed: RunEnvelopeFeed,
) -> Option<(AgentRunEnvelope, Option<RunEnvelopeFeed>)> {
    let mut live = match feed {
        RunEnvelopeFeed::Boot(run) => {
            // 先订阅再读取快照；sequence 去重消除两者之间的竞态。
            let receiver = run.subscribe();
            let (history, last_sequence, already_completed) = run.snapshot().await;
            let mut pending = VecDeque::new();
            if !history
                .iter()
                .any(|envelope| matches!(&envelope.event, AgentProgressEvent::RunStarted { .. }))
            {
                pending.push_back(AgentRunEnvelope {
                    sequence: 0,
                    event: AgentProgressEvent::RunStarted {
                        run_id: run.run_id().to_string(),
                        session_id: run.session_id().map(str::to_string),
                    },
                });
            }
            pending.extend(history);
            let mut registry_poll = tokio::time::interval(Duration::from_secs(2));
            registry_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            RunEnvelopeLive {
                run,
                receiver,
                last_sequence,
                registry_poll,
                pending,
                stop_after_pending: already_completed,
            }
        }
        RunEnvelopeFeed::Live(live) => live,
    };
    if let Some(envelope) = live.pending.pop_front() {
        let terminal = agent_run_event_is_terminal(&envelope.event);
        if terminal {
            return Some((envelope, None));
        }
        return Some((envelope, Some(RunEnvelopeFeed::Live(live))));
    }
    if live.stop_after_pending {
        return None;
    }
    loop {
        tokio::select! {
            received = live.receiver.recv() => match received {
                Ok(envelope) if envelope.sequence > live.last_sequence => {
                    live.last_sequence = envelope.sequence;
                    let terminal = agent_run_event_is_terminal(&envelope.event);
                    return Some((
                        envelope,
                        if terminal {
                            None
                        } else {
                            Some(RunEnvelopeFeed::Live(live))
                        },
                    ));
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // 不关流：从内存快照补发遗漏事件，避免前端必须重开连接。
                    live.receiver = live.run.subscribe();
                    let snapshot = live.run.snapshot().await;
                    let (history, snap_seq, completed) = snapshot;
                    for envelope in history {
                        if envelope.sequence <= live.last_sequence {
                            continue;
                        }
                        live.last_sequence = envelope.sequence;
                        let terminal = agent_run_event_is_terminal(&envelope.event);
                        live.pending.push_back(envelope);
                        if terminal {
                            live.stop_after_pending = true;
                            break;
                        }
                    }
                    live.last_sequence = live.last_sequence.max(snap_seq);
                    if completed {
                        live.stop_after_pending = true;
                    }
                    if let Some(envelope) = live.pending.pop_front() {
                        let terminal = agent_run_event_is_terminal(&envelope.event);
                        return Some((
                            envelope,
                            if terminal || (live.pending.is_empty() && live.stop_after_pending)
                            {
                                None
                            } else {
                                Some(RunEnvelopeFeed::Live(live))
                            },
                        ));
                    }
                    if live.stop_after_pending {
                        return None;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            },
            _ = live.registry_poll.tick() => {
                let refreshed = live.run.refresh_from_registry().await;
                for envelope in refreshed {
                    if envelope.sequence <= live.last_sequence {
                        continue;
                    }
                    live.last_sequence = envelope.sequence;
                    let terminal = agent_run_event_is_terminal(&envelope.event);
                    live.pending.push_back(envelope);
                    if terminal {
                        live.stop_after_pending = true;
                        break;
                    }
                }
                if let Some(envelope) = live.pending.pop_front() {
                    let terminal = agent_run_event_is_terminal(&envelope.event);
                    return Some((
                        envelope,
                        if terminal {
                            None
                        } else {
                            Some(RunEnvelopeFeed::Live(live))
                        },
                    ));
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

#[cfg(test)]
mod envelope_stream_tests {
    use super::*;
    use crate::services::agent::run_hub::create_run_for_test as create_run;

    fn progress(message: &str) -> AgentProgressEvent {
        AgentProgressEvent::Progress {
            progress: 10,
            completed_steps: 0,
            total_steps: 1,
            message: message.to_string(),
        }
    }

    fn completed(message: &str) -> AgentProgressEvent {
        AgentProgressEvent::TaskCompleted {
            task_id: "task".to_string(),
            success: true,
            response: Box::new(json!({ "success": true, "message": message })),
        }
    }

    #[test]
    fn task_completed_is_terminal_unless_waiting_for_input() {
        assert!(agent_run_event_is_terminal(&completed("done")));
        assert!(!agent_run_event_is_terminal(
            &AgentProgressEvent::TaskCompleted {
                task_id: "task".into(),
                success: true,
                response: Box::new(json!({
                    "success": true,
                    "task": { "status": "waiting_for_input" }
                })),
            }
        ));
        assert!(agent_run_event_is_terminal(&AgentProgressEvent::Error {
            task_id: None,
            message: "boom".into(),
            code: "test".into(),
        }));
        assert!(!agent_run_event_is_terminal(&progress("working")));
    }

    async fn collect_until_end(run: Arc<AgentRun>) -> Vec<AgentRunEnvelope> {
        let mut stream = std::pin::pin!(agent_run_envelopes(run));
        let mut out = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            match tokio::time::timeout_at(deadline, stream.next()).await {
                Ok(Some(envelope)) => out.push(envelope),
                Ok(None) => return out,
                Err(_) => panic!(
                    "agent_run_envelopes did not end; got {} event(s)",
                    out.len()
                ),
            }
        }
    }

    #[tokio::test]
    async fn completed_history_replays_then_ends() {
        let run = create_run(8101, Some("envelope-completed".into())).await;
        run.publish(progress("working")).await;
        run.publish(completed("done")).await;

        let events = collect_until_end(run).await;
        assert!(
            matches!(
                events.first().map(|e| &e.event),
                Some(AgentProgressEvent::RunStarted { .. })
            ),
            "first event: {:?}",
            events.first().map(|e| &e.event)
        );
        assert!(
            matches!(
                events.last().map(|e| &e.event),
                Some(AgentProgressEvent::TaskCompleted { .. })
            ),
            "last event: {:?}",
            events.last().map(|e| &e.event)
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(&e.event, AgentProgressEvent::Progress { message, .. } if message == "working"))
        );
        let sequences: Vec<u64> = events.iter().map(|e| e.sequence).collect();
        let mut sorted = sequences.clone();
        sorted.sort_unstable();
        assert_eq!(sequences, sorted, "replay must keep sequence order");
    }

    #[tokio::test]
    async fn missing_run_started_is_synthesized_then_history_follows() {
        let run = AgentRun::new_for_test("envelope-synth", 8102);
        run.publish(progress("no-start")).await;
        run.publish(completed("done")).await;

        let events = collect_until_end(run).await;
        assert!(matches!(
            &events[0].event,
            AgentProgressEvent::RunStarted {
                run_id,
                ..
            } if run_id == "envelope-synth"
        ));
        assert_eq!(events[0].sequence, 0);
        assert!(
            events
                .iter()
                .any(|e| matches!(&e.event, AgentProgressEvent::Progress { message, .. } if message == "no-start"))
        );
        assert!(matches!(
            events.last().map(|e| &e.event),
            Some(AgentProgressEvent::TaskCompleted { .. })
        ));
    }

    #[tokio::test]
    async fn terminal_event_closes_the_stream() {
        let run = create_run(8103, Some("envelope-terminal".into())).await;
        let mut stream = std::pin::pin!(agent_run_envelopes(run.clone()));
        let started = stream.next().await.expect("RunStarted");
        assert!(matches!(
            started.event,
            AgentProgressEvent::RunStarted { .. }
        ));

        run.publish(completed("stop")).await;
        let terminal = tokio::time::timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("terminal should arrive")
            .expect("stream open for terminal");
        assert!(matches!(
            terminal.event,
            AgentProgressEvent::TaskCompleted { .. }
        ));

        let ended = tokio::time::timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("stream must end after terminal, not wait on the 2s registry poll");
        assert!(ended.is_none());
    }

    #[tokio::test]
    async fn waiting_for_input_does_not_close_the_stream() {
        let run = create_run(8104, Some("envelope-wait".into())).await;
        let mut stream = std::pin::pin!(agent_run_envelopes(run.clone()));
        let _ = stream.next().await;

        run.publish(AgentProgressEvent::TaskCompleted {
            task_id: "task".into(),
            success: true,
            response: Box::new(json!({
                "success": true,
                "message": "need input",
                "task": { "status": "waiting_for_input" }
            })),
        })
        .await;
        let waiting = tokio::time::timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("waiting event")
            .expect("stream stays open");
        assert!(matches!(
            waiting.event,
            AgentProgressEvent::TaskCompleted { .. }
        ));

        let next = tokio::time::timeout(Duration::from_millis(200), stream.next()).await;
        assert!(
            next.is_err(),
            "waiting_for_input must keep the live select open"
        );

        run.publish(completed("after-wait")).await;
        let terminal = tokio::time::timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("completion after wait")
            .expect("stream still open");
        assert!(matches!(
            terminal.event,
            AgentProgressEvent::TaskCompleted { success: true, .. }
        ));
    }

    #[tokio::test]
    async fn lagged_subscriber_replays_from_snapshot() {
        let run = create_run(8105, Some("envelope-lag".into())).await;
        let mut stream = std::pin::pin!(agent_run_envelopes(run.clone()));
        let started = stream.next().await.expect("RunStarted");
        assert!(matches!(
            started.event,
            AgentProgressEvent::RunStarted { .. }
        ));
        let last_before_flood = started.sequence;

        for i in 0..520 {
            run.publish(progress(&format!("lag-{i}"))).await;
        }

        let mut caught_up = 0usize;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            match tokio::time::timeout_at(deadline, stream.next()).await {
                Ok(Some(envelope)) => {
                    assert!(
                        envelope.sequence > last_before_flood,
                        "catch-up must skip already-yielded sequence {}",
                        envelope.sequence
                    );
                    if matches!(
                        &envelope.event,
                        AgentProgressEvent::Progress { message, .. } if message.starts_with("lag-")
                    ) {
                        caught_up += 1;
                    }
                    if caught_up >= 100 {
                        break;
                    }
                }
                Ok(None) => panic!("stream ended during lag catch-up after {caught_up} events"),
                Err(_) => panic!("lag catch-up stalled after {caught_up} events"),
            }
        }
        assert!(caught_up >= 100);
    }

    #[tokio::test]
    async fn error_event_closes_the_stream() {
        let run = create_run(8106, Some("envelope-error".into())).await;
        let mut stream = std::pin::pin!(agent_run_envelopes(run.clone()));
        let _ = stream.next().await;

        run.publish(AgentProgressEvent::Error {
            task_id: None,
            message: "boom".into(),
            code: "test".into(),
        })
        .await;
        let error = tokio::time::timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("error should arrive")
            .expect("stream open for error");
        assert!(matches!(error.event, AgentProgressEvent::Error { .. }));

        let ended = tokio::time::timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("stream must end after Error");
        assert!(ended.is_none());
    }
}
