//! Agent 运行状态中心。
//!
//! 后端 run 独立于 HTTP/SSE 连接存在：执行器写入服务端通道，任意前端连接只订阅
//! 可重放的事件快照。页面刷新、切路由和网络断线不会取消任务。

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use chrono::Utc;
use once_cell::sync::Lazy;
use sea_orm::{ConnectionTrait, DbBackend, FromQueryResult, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};

use crate::services::tapp_registry as shared_registry;

use super::AgentProgressEvent;
use super::notifications::get_notification_manager;

/// 单 run 内存事件环：加长以减少超长任务 re-subscribe 丢中间步骤
const EVENT_HISTORY_LIMIT: usize = 512;
const PERSISTENCE_QUEUE_LIMIT: usize = 128;
const COMPLETED_RUN_CACHE_LIMIT: usize = 256;
const RUN_REGISTRY_NAMESPACE: &str = "agent_run";
const RUN_EVENT_REGISTRY_NAMESPACE: &str = "agent_run_event";
const RUN_RETENTION_HOURS: i64 = 24;

#[derive(Clone, Serialize, Deserialize)]
pub struct AgentRunEnvelope {
    pub sequence: u64,
    pub event: AgentProgressEvent,
}

struct AgentRunState {
    next_sequence: u64,
    events: VecDeque<AgentRunEnvelope>,
    task_id: Option<String>,
    status: String,
    progress: u8,
    message: String,
    completed: bool,
    updated_at: chrono::DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
struct PersistedAgentRun {
    run_id: String,
    user_id: i32,
    session_id: Option<String>,
    created_at: chrono::DateTime<Utc>,
    next_sequence: u64,
    task_id: Option<String>,
    status: String,
    progress: u8,
    message: String,
    completed: bool,
    updated_at: chrono::DateTime<Utc>,
}

#[derive(FromQueryResult)]
struct PersistedAgentRunEventRow {
    payload: Value,
}

pub struct AgentRun {
    execution: std::sync::Mutex<Option<tokio::task::AbortHandle>>,
    pub(crate) playback_direction: super::playback_direction::PlaybackDirection,
    run_id: String,
    user_id: i32,
    session_id: Option<String>,
    created_at: chrono::DateTime<Utc>,
    state: Mutex<AgentRunState>,
    publish_order: Mutex<()>,
    persistence_tx: mpsc::Sender<(AgentRunEnvelope, PersistedAgentRun)>,
    events_tx: broadcast::Sender<AgentRunEnvelope>,
}

static AGENT_RUNS: Lazy<RwLock<HashMap<String, Arc<AgentRun>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

impl AgentRun {
    fn new(run_id: String, user_id: i32, session_id: Option<String>) -> Arc<Self> {
        let (events_tx, _) = broadcast::channel(EVENT_HISTORY_LIMIT);
        Arc::new(Self {
            execution: std::sync::Mutex::new(None),
            publish_order: Mutex::new(()),
            persistence_tx: Self::persistence_channel(),
            playback_direction: Default::default(),
            run_id,
            user_id,
            session_id,
            created_at: Utc::now(),
            state: Mutex::new(AgentRunState {
                next_sequence: 1,
                events: VecDeque::with_capacity(EVENT_HISTORY_LIMIT),
                task_id: None,
                status: "running".to_string(),
                progress: 0,
                message: "Task submitted, waiting to run".to_string(),
                completed: false,
                updated_at: Utc::now(),
            }),
            events_tx,
        })
    }

    /// One ordered writer per run. The receiver owns snapshots, never the run,
    /// and drains accepted events before exiting when the last sender is dropped.
    fn persistence_channel() -> mpsc::Sender<(AgentRunEnvelope, PersistedAgentRun)> {
        let (tx, mut rx) =
            mpsc::channel::<(AgentRunEnvelope, PersistedAgentRun)>(PERSISTENCE_QUEUE_LIMIT);
        tokio::spawn(async move {
            while let Some((envelope, snapshot)) = rx.recv().await {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    Self::persist(&envelope, &snapshot),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        tracing::error!(
                            run_id = %snapshot.run_id,
                            sequence = envelope.sequence,
                            %error,
                            "[Agent Run] Failed to persist shared run snapshot"
                        );
                    }
                    Err(_) => {
                        tracing::error!(
                            run_id = %snapshot.run_id,
                            sequence = envelope.sequence,
                            "[Agent Run] Persistence timed out"
                        );
                    }
                }
            }
        });
        tx
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(run_id: impl Into<String>, user_id: i32) -> Arc<Self> {
        Self::new(run_id.into(), user_id, None)
    }

    fn from_persisted(
        persisted: PersistedAgentRun,
        events: VecDeque<AgentRunEnvelope>,
    ) -> Arc<Self> {
        let (events_tx, _) = broadcast::channel(EVENT_HISTORY_LIMIT);
        let playback_direction = super::playback_direction::PlaybackDirection::default();
        playback_direction.close(); // Restored history must never revive a live director.
        Arc::new(Self {
            execution: std::sync::Mutex::new(None),
            publish_order: Mutex::new(()),
            persistence_tx: Self::persistence_channel(),
            playback_direction,
            run_id: persisted.run_id,
            user_id: persisted.user_id,
            session_id: persisted.session_id,
            created_at: persisted.created_at,
            state: Mutex::new(AgentRunState {
                next_sequence: persisted.next_sequence,
                events,
                task_id: persisted.task_id,
                status: persisted.status,
                progress: persisted.progress,
                message: persisted.message,
                completed: persisted.completed,
                updated_at: persisted.updated_at,
            }),
            events_tx,
        })
    }

    /// Explicit user cancellation also covers queueing and planning before a task exists.
    /// Subscribers disconnecting must never call this.
    pub(crate) fn register_execution(&self, handle: tokio::task::AbortHandle) {
        *self.execution.lock().unwrap() = Some(handle);
    }

    pub(crate) async fn abort_execution(self: &Arc<Self>) {
        if let Some(handle) = self.execution.lock().unwrap().take() {
            handle.abort();
        }
        self.publish(AgentProgressEvent::Error {
            task_id: None,
            message: "任务已取消".into(),
            code: "CANCELLED".into(),
        })
        .await;
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AgentRunEnvelope> {
        self.events_tx.subscribe()
    }

    pub async fn snapshot(&self) -> (Vec<AgentRunEnvelope>, u64, bool) {
        let state = self.state.lock().await;
        (
            state.events.iter().cloned().collect(),
            state.events.back().map(|event| event.sequence).unwrap_or(0),
            state.completed,
        )
    }

    pub(crate) async fn is_executing(&self) -> bool {
        let state = self.state.lock().await;
        !state.completed && state.status != "waiting_for_input"
    }

    #[cfg(test)]
    async fn persisted_snapshot(&self) -> PersistedAgentRun {
        let state = self.state.lock().await;
        self.snapshot_from_state(&state)
    }

    fn snapshot_from_state(&self, state: &AgentRunState) -> PersistedAgentRun {
        PersistedAgentRun {
            run_id: self.run_id.clone(),
            user_id: self.user_id,
            session_id: self.session_id.clone(),
            created_at: self.created_at,
            next_sequence: state.next_sequence,
            task_id: state.task_id.clone(),
            status: state.status.clone(),
            progress: state.progress,
            message: state.message.clone(),
            completed: state.completed,
            updated_at: state.updated_at,
        }
    }

    async fn load_persisted_events(
        db: &impl ConnectionTrait,
        run_id: &str,
    ) -> Result<VecDeque<AgentRunEnvelope>, sea_orm::DbErr> {
        let rows = PersistedAgentRunEventRow::find_by_statement(Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"
SELECT payload
FROM (
    SELECT record_id, payload
    FROM tapp_runtime_registry
    WHERE namespace = $1
      AND runtime_id = $2
      AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT
    ORDER BY record_id DESC
    LIMIT $3
) AS recent
ORDER BY record_id ASC
"#,
            vec![
                RUN_EVENT_REGISTRY_NAMESPACE.into(),
                run_id.to_string().into(),
                (EVENT_HISTORY_LIMIT as i64).into(),
            ],
        ))
        .all(db)
        .await?;
        rows.into_iter()
            .map(|row| {
                serde_json::from_value(row.payload)
                    .map_err(|error| sea_orm::DbErr::Json(error.to_string()))
            })
            .collect()
    }

    async fn persist(
        envelope: &AgentRunEnvelope,
        snapshot: &PersistedAgentRun,
    ) -> Result<(), String> {
        let db = shared_registry::database().map_err(|error| error.to_string())?;
        let expires_at =
            (snapshot.updated_at + chrono::Duration::hours(RUN_RETENTION_HOURS)).timestamp();
        let snapshot_payload = serde_json::to_value(&snapshot)
            .map_err(|error| format!("Failed to serialize run snapshot: {error}"))?;
        let event_payload = serde_json::to_value(envelope)
            .map_err(|error| format!("Failed to serialize run event: {error}"))?;
        let event_record_id = format!("{}:{:020}", snapshot.run_id, envelope.sequence);
        let transaction = db
            .begin()
            .await
            .map_err(|error| format!("Failed to begin persistence transaction: {error}"))?;
        let result = async {
            transaction
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
                    vec![format!("agent_run:{}", snapshot.run_id).into()],
                ))
                .await?;
            transaction
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    r#"
INSERT INTO tapp_runtime_registry
    (namespace, record_id, subject_id, owner_id, runtime_id, payload, expires_at, updated_at)
VALUES ($1, $2, $3, $3, $4, $5, $6, NOW())
ON CONFLICT (namespace, record_id) DO NOTHING
"#,
                    vec![
                        RUN_EVENT_REGISTRY_NAMESPACE.into(),
                        event_record_id.into(),
                        snapshot.user_id.into(),
                        snapshot.run_id.clone().into(),
                        event_payload.into(),
                        expires_at.into(),
                    ],
                ))
                .await?;
            transaction
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    r#"
INSERT INTO tapp_runtime_registry
    (namespace, record_id, subject_id, owner_id, payload, expires_at, updated_at)
VALUES ($1, $2, $3, $3, $4, $5, NOW())
ON CONFLICT (namespace, record_id) DO UPDATE SET
    subject_id = EXCLUDED.subject_id,
    owner_id = EXCLUDED.owner_id,
    payload = EXCLUDED.payload,
    expires_at = EXCLUDED.expires_at,
    updated_at = NOW()
WHERE COALESCE((tapp_runtime_registry.payload ->> 'next_sequence')::BIGINT, 0)
      <= (EXCLUDED.payload ->> 'next_sequence')::BIGINT
"#,
                    vec![
                        RUN_REGISTRY_NAMESPACE.into(),
                        snapshot.run_id.clone().into(),
                        snapshot.user_id.into(),
                        snapshot_payload.into(),
                        expires_at.into(),
                    ],
                ))
                .await?;
            transaction
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    r#"
DELETE FROM tapp_runtime_registry
WHERE namespace = $1 AND runtime_id = $2
  AND record_id NOT IN (
      SELECT record_id
      FROM tapp_runtime_registry
      WHERE namespace = $1 AND runtime_id = $2
      ORDER BY record_id DESC
      LIMIT $3
  )
"#,
                    vec![
                        RUN_EVENT_REGISTRY_NAMESPACE.into(),
                        snapshot.run_id.clone().into(),
                        (EVENT_HISTORY_LIMIT as i64).into(),
                    ],
                ))
                .await?;
            transaction.commit().await
        }
        .await;
        match result {
            Ok(()) => {
                shared_registry::maybe_cleanup(&db).await;
                Ok(())
            }
            Err(error) => Err(error.to_string()),
        }
    }

    /// Merge a newer shared snapshot and return events not present locally.
    pub async fn refresh_from_registry(&self) -> Vec<AgentRunEnvelope> {
        let Ok(db) = shared_registry::database() else {
            return Vec::new();
        };
        let persisted = match shared_registry::get::<PersistedAgentRun>(
            &db,
            RUN_REGISTRY_NAMESPACE,
            &self.run_id,
        )
        .await
        {
            Ok(Some(run)) if run.user_id == self.user_id => run,
            Ok(_) => return Vec::new(),
            Err(error) => {
                tracing::warn!(run_id = %self.run_id, %error, "[Agent Run] Failed to refresh shared run snapshot");
                return Vec::new();
            }
        };
        let persisted_events = match Self::load_persisted_events(&db, &self.run_id).await {
            Ok(events) => events,
            Err(error) => {
                tracing::warn!(run_id = %self.run_id, %error, "[Agent Run] Failed to refresh shared run events");
                return Vec::new();
            }
        };

        let mut state = self.state.lock().await;
        if persisted.next_sequence <= state.next_sequence {
            return Vec::new();
        }
        let last_local_sequence = state.next_sequence.saturating_sub(1);
        let new_events = persisted_events
            .iter()
            .filter(|event| event.sequence > last_local_sequence)
            .cloned()
            .collect::<Vec<_>>();
        state.next_sequence = persisted.next_sequence;
        state.events = persisted_events;
        state.task_id = persisted.task_id;
        state.status = persisted.status;
        state.progress = persisted.progress;
        state.message = persisted.message;
        state.completed = persisted.completed;
        state.updated_at = persisted.updated_at;
        new_events
    }

    /// Publish a progress event to live subscribers first, then durable storage.
    ///
    /// Live SSE precedes its DB write. Sustained DB slowdown applies backpressure
    /// once the bounded persistence queue is full; accepted events remain ordered.
    /// Data-plane frames (visemes, spectrum, VAD) must never reach this method.
    pub async fn publish(self: &Arc<Self>, event: AgentProgressEvent) {
        if matches!(
            &event,
            AgentProgressEvent::Error { .. }
                | AgentProgressEvent::TaskCompleted { success: false, .. }
        ) {
            // Cleanup from an old producer may race a successful terminal.
            // Check under the same lock as that transition before closing its
            // playback, while still closing active failures before DB backpressure.
            let state = self.state.lock().await;
            if !state.completed {
                self.playback_direction.close();
            }
        }
        if super::turn::event_plane(&event) == super::turn::EventPlane::Data {
            tracing::error!("[Agent Run] data-plane event dropped from run hub");
            return;
        }
        let _order = self.publish_order.lock().await;
        // Reserve before mutating/broadcasting: a cancelled publisher cannot
        // leave a visible event without its matching persistence obligation.
        let persistence = self.persistence_tx.reserve().await.ok();
        let mut notify = false;
        let (envelope, snapshot, task_id, status, progress, message, success) = {
            let mut state = self.state.lock().await;
            // A terminal envelope is the run's hard boundary. SSE consumers
            // close on it, so accepting anything later only creates durable
            // events no live rig can ever observe and can even revive status.
            if state.completed {
                tracing::warn!(
                    run_id = %self.run_id,
                    "[Agent Run] post-terminal event dropped"
                );
                return;
            }
            let success = match &event {
                AgentProgressEvent::RunStarted { .. } => {
                    notify = true;
                    state.status = "running".to_string();
                    state.message = "Task submitted, waiting to run".to_string();
                    None
                }
                AgentProgressEvent::TaskCreated {
                    task_id, message, ..
                } => {
                    notify = true;
                    state.task_id = Some(task_id.clone());
                    state.status = "running".to_string();
                    state.progress = state.progress.max(5);
                    state.message = message.clone();
                    None
                }
                AgentProgressEvent::TaskAssigned {
                    task_id,
                    assignment,
                } => {
                    notify = true;
                    state.task_id = Some(task_id.clone());
                    state.status = "running".to_string();
                    state.message = format!(
                        "已分配给 {} 个 Agent: {}",
                        assignment.total_agents,
                        assignment
                            .agents
                            .iter()
                            .map(|agent| agent.display_name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    None
                }
                AgentProgressEvent::Progress {
                    progress, message, ..
                } => {
                    notify = true;
                    state.status = "running".to_string();
                    state.progress = *progress;
                    state.message = message.clone();
                    None
                }
                AgentProgressEvent::WaitingForInput {
                    task_id, question, ..
                } => {
                    notify = true;
                    state.task_id = Some(task_id.clone());
                    state.status = "waiting_for_input".to_string();
                    state.message = question.clone();
                    None
                }
                AgentProgressEvent::TaskCompleted {
                    task_id,
                    success,
                    response,
                } => {
                    notify = !super::turn::is_chat_turn_completion(response);
                    if !task_id.is_empty() {
                        state.task_id = Some(task_id.clone());
                    }
                    let force_terminal =
                        response.get("streamTerminal").and_then(Value::as_bool) == Some(true);
                    let response_status = response.pointer("/task/status").and_then(Value::as_str);
                    state.status = match response_status {
                        Some("cancelled") => "cancelled",
                        Some("waiting_for_input") if !force_terminal => "waiting_for_input",
                        _ if *success => "completed",
                        _ => "failed",
                    }
                    .to_string();
                    if state.status != "waiting_for_input" {
                        state.progress = 100;
                    }
                    state.message = response
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or(if *success {
                            "The task finished"
                        } else {
                            "Processing failed"
                        })
                        .to_string();
                    state.completed = state.status != "waiting_for_input";
                    Some(*success)
                }
                AgentProgressEvent::Error {
                    task_id, message, ..
                } => {
                    notify = true;
                    if task_id.is_some() {
                        state.task_id = task_id.clone();
                    }
                    state.status = "failed".to_string();
                    state.message = message.clone();
                    state.completed = true;
                    Some(false)
                }
                _ => None,
            };

            let envelope = AgentRunEnvelope {
                sequence: state.next_sequence,
                event,
            };
            state.next_sequence += 1;
            if state.events.len() >= EVENT_HISTORY_LIMIT {
                state.events.pop_front();
            }
            state.events.push_back(envelope.clone());
            state.updated_at = Utc::now();
            (
                envelope,
                self.snapshot_from_state(&state),
                state.task_id.clone(),
                state.status.clone(),
                state.progress,
                state.message.clone(),
                success,
            )
        };

        // Live path: broadcast immediately so SSE subscribers never wait on DB.
        let _ = self.events_tx.send(envelope.clone());

        if let Some(permit) = persistence {
            permit.send((envelope, snapshot));
        } else {
            tracing::error!(run_id = %self.run_id, "[Agent Run] Persistence worker unavailable");
        }
        drop(_order);

        if notify {
            if let Some(manager) = get_notification_manager() {
                let title = match status.as_str() {
                    "completed" => "The task finished",
                    "failed" => "The task failed",
                    "cancelled" => "The task was cancelled",
                    "waiting_for_input" => "The task needs your reply",
                    _ => "Agent is working",
                };
                manager
                    .notify_task_status(
                        &self.run_id,
                        task_id.as_deref(),
                        self.user_id,
                        self.session_id.as_deref(),
                        title,
                        &message.chars().take(160).collect::<String>(),
                        progress,
                        &status,
                        success,
                    )
                    .await;
            }
        }
    }
}

/// Reclaim completed replay caches and expired cached replicas during idle periods.
/// Never evict a locally executing run merely because its progress is quiet;
/// removing a stale cached replica neither aborts Work nor deletes durable state.
pub(crate) async fn cleanup_retained_runs() {
    let cutoff = Utc::now() - chrono::Duration::hours(RUN_RETENTION_HOURS);
    let mut runs = AGENT_RUNS.write().await;
    let mut completed = Vec::new();
    let mut stale_replicas = Vec::new();
    for (id, run) in runs.iter() {
        let state = run.state.lock().await;
        if state.completed {
            completed.push((state.updated_at, id.clone()));
        } else if state.updated_at < cutoff
            && !run
                .execution
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|handle| !handle.is_finished())
        {
            stale_replicas.push(id.clone());
        }
    }
    for id in stale_replicas {
        runs.remove(&id);
    }
    completed.sort_unstable();
    let surplus = completed.len().saturating_sub(COMPLETED_RUN_CACHE_LIMIT);
    for (index, (updated_at, id)) in completed.into_iter().enumerate() {
        if updated_at < cutoff || index < surplus {
            runs.remove(&id);
        }
    }
}

pub async fn create_run(user_id: i32, session_id: Option<String>) -> Arc<AgentRun> {
    cleanup_retained_runs().await;
    let run_id = format!("run_{}", uuid::Uuid::new_v4().simple());
    let run = AgentRun::new(run_id.clone(), user_id, session_id.clone());
    AGENT_RUNS.write().await.insert(run_id.clone(), run.clone());
    run.publish(AgentProgressEvent::RunStarted { run_id, session_id })
        .await;
    run
}

/// Run still doing work. `waiting_for_input` is excluded: that run is waiting on
/// the person, not occupying them. Heartbeat uses SYSTEM_USER_ID.
pub async fn user_has_executing_run(user_id: i32) -> bool {
    user_executing_run_count(user_id).await > 0
}

pub async fn user_executing_run_count(user_id: i32) -> usize {
    let candidates = AGENT_RUNS
        .read()
        .await
        .values()
        .filter(|run| run.user_id == user_id)
        .cloned()
        .collect::<Vec<_>>();
    let mut count = 0;
    for run in candidates {
        if run.is_executing().await {
            count += 1;
        }
    }
    count
}

pub async fn get_run_for_user(run_id: &str, user_id: i32) -> Result<Option<Arc<AgentRun>>, String> {
    if let Some(run) = AGENT_RUNS
        .read()
        .await
        .get(run_id)
        .filter(|run| run.user_id == user_id)
        .cloned()
    {
        return Ok(Some(run));
    }

    let db = shared_registry::database().map_err(|error| error.to_string())?;
    let persisted = match shared_registry::get::<PersistedAgentRun>(
        &db,
        RUN_REGISTRY_NAMESPACE,
        run_id,
    )
    .await
    {
        Ok(Some(persisted)) => persisted,
        Ok(None) => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if persisted.user_id != user_id || persisted.run_id != run_id {
        return Ok(None);
    }
    let events = AgentRun::load_persisted_events(&db, run_id)
        .await
        .map_err(|error| error.to_string())?;
    let run = AgentRun::from_persisted(persisted, events);
    let mut runs = AGENT_RUNS.write().await;
    Ok(Some(
        runs.entry(run_id.to_string())
            .or_insert_with(|| run.clone())
            .clone(),
    ))
}

/// Live-only lookup for transient playback; no disk rehydration or model call.
pub(crate) async fn get_live_run_for_user(run_id: &str, user_id: i32) -> Option<Arc<AgentRun>> {
    AGENT_RUNS
        .read()
        .await
        .get(run_id)
        .filter(|run| run.user_id == user_id)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn persistence_queue_applies_backpressure_without_losing_order_or_terminal() {
        let mut run = AgentRun::new("bounded-writer".into(), 702, None);
        let (tx, mut rx) = mpsc::channel(2);
        Arc::get_mut(&mut run).unwrap().persistence_tx = tx;
        for token in ["one", "two"] {
            run.publish(AgentProgressEvent::SummaryToken {
                token: token.into(),
                done: false,
            })
            .await;
        }
        let producer_run = run.clone();
        let mut producer = tokio::spawn(async move {
            producer_run
                .publish(AgentProgressEvent::SummaryToken {
                    token: "three".into(),
                    done: false,
                })
                .await;
        });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut producer)
                .await
                .is_err()
        );
        assert_eq!(run.state.lock().await.next_sequence, 3);
        let (first, snapshot) = rx.recv().await.unwrap();
        assert_eq!(first.sequence, 1);
        assert_eq!(snapshot.next_sequence, 2);
        producer.await.unwrap();
        for sequence in [2, 3] {
            let (event, snapshot) = rx.recv().await.unwrap();
            assert_eq!(event.sequence, sequence);
            assert_eq!(snapshot.next_sequence, sequence + 1);
        }
        run.publish(AgentProgressEvent::TaskCompleted {
            task_id: "task".into(),
            success: true,
            response: Box::new(serde_json::json!({"message":"done"})),
        })
        .await;
        let (terminal, snapshot) = rx.recv().await.unwrap();
        assert_eq!(terminal.sequence, 4);
        assert!(snapshot.completed);
    }

    #[tokio::test]
    async fn run_cleanup_keeps_live_execution_and_reclaims_stale_replicas() {
        let run_id = format!("old-active-{}", uuid::Uuid::new_v4());
        let run = AgentRun::new(run_id.clone(), 709, None);
        run.state.lock().await.updated_at = Utc::now() - chrono::Duration::hours(25);
        let execution = tokio::spawn(std::future::pending::<()>());
        run.register_execution(execution.abort_handle());
        AGENT_RUNS.write().await.insert(run_id.clone(), run.clone());
        let trigger = create_run(709, None).await;
        assert!(get_live_run_for_user(&run_id, 709).await.is_some());
        execution.abort();
        let _ = execution.await;
        cleanup_retained_runs().await;
        assert!(get_live_run_for_user(&run_id, 709).await.is_none());
        // A caller's existing run handle remains valid; cleanup did not abort it.
        assert!(!run.snapshot().await.2);
        AGENT_RUNS.write().await.remove(&run_id);
        AGENT_RUNS.write().await.remove(trigger.run_id());
    }

    #[tokio::test]
    async fn event_burst_keeps_persistence_workers_bounded() {
        // This current-thread burst gives persistence no scheduler time, just as
        // a slow database can keep older writes pending while events arrive.
        let run = AgentRun::new("persistence-burst".into(), 701, None);
        for _ in 0..2_000 {
            run.publish(AgentProgressEvent::SummaryToken {
                token: "chunk".into(),
                done: false,
            })
            .await;
        }
        assert!(
            Arc::strong_count(&run) <= 2,
            "one persistence worker per run, not one per event"
        );
        assert_eq!(run.snapshot().await.0.len(), 512);
    }

    #[tokio::test]
    async fn events_survive_subscriber_disconnect_and_replay() {
        let run = AgentRun::new("run_test".to_string(), 7, Some("session_test".to_string()));
        let mut first_subscriber = run.subscribe();
        run.publish(AgentProgressEvent::RunStarted {
            run_id: "run_test".to_string(),
            session_id: Some("session_test".to_string()),
        })
        .await;

        assert_eq!(run.snapshot().await.0.len(), 1);
        assert!(first_subscriber.recv().await.is_ok());
        drop(first_subscriber);

        run.publish(AgentProgressEvent::Progress {
            progress: 45,
            completed_steps: 1,
            total_steps: 2,
            message: "still running".to_string(),
        })
        .await;
        run.publish(AgentProgressEvent::TaskCompleted {
            task_id: "task_test".to_string(),
            success: true,
            response: Box::new(serde_json::json!({
                "success": true,
                "message": "done"
            })),
        })
        .await;

        let (history, _, completed) = run.snapshot().await;
        assert_eq!(history.len(), 3);
        assert!(completed);
        assert!(matches!(
            history.last().map(|event| &event.event),
            Some(AgentProgressEvent::TaskCompleted { .. })
        ));

        let encoded = serde_json::to_value(run.persisted_snapshot().await).unwrap();
        let persisted: PersistedAgentRun = serde_json::from_value(encoded).unwrap();
        let restored = AgentRun::from_persisted(persisted, history.into());
        let (restored_history, restored_sequence, restored_completed) = restored.snapshot().await;
        assert_eq!(restored_history.len(), 3);
        assert_eq!(restored_sequence, 3);
        assert!(restored_completed);
    }

    #[tokio::test]
    async fn persist_fails_when_database_unavailable() {
        if shared_registry::database().is_ok() {
            return;
        }
        let snapshot = PersistedAgentRun {
            run_id: "run_persist_err".into(),
            user_id: 1,
            session_id: None,
            created_at: Utc::now(),
            next_sequence: 2,
            task_id: None,
            status: "running".into(),
            progress: 0,
            message: "x".into(),
            completed: false,
            updated_at: Utc::now(),
        };
        let envelope = AgentRunEnvelope {
            sequence: 1,
            event: AgentProgressEvent::RunStarted {
                run_id: "run_persist_err".into(),
                session_id: None,
            },
        };
        assert!(
            AgentRun::persist(&envelope, &snapshot).await.is_err(),
            "missing DB must not look like a successful durable write"
        );
    }

    #[tokio::test]
    async fn rehydrate_error_is_not_missing_run() {
        if shared_registry::database().is_ok() {
            return;
        }
        let result = get_run_for_user("run_does_not_exist", 42).await;
        assert!(
            result.is_err(),
            "registry/db error must not look like a missing run"
        );
    }

    #[tokio::test]
    async fn live_run_lookup_does_not_require_registry() {
        let run = AgentRun::new("run_live_lookup".into(), 77, None);
        AGENT_RUNS
            .write()
            .await
            .insert(run.run_id().to_string(), run.clone());
        let found = get_run_for_user(run.run_id(), 77)
            .await
            .unwrap()
            .expect("live run");
        assert_eq!(found.run_id(), run.run_id());
        AGENT_RUNS.write().await.remove(run.run_id());
    }

    #[tokio::test]
    async fn publish_delivers_to_subscribers_without_waiting_on_persist() {
        // Live broadcast must complete even when durable registry is unavailable.
        let run = AgentRun::new("run_live".to_string(), 1, None);
        let mut sub = run.subscribe();
        run.publish(AgentProgressEvent::Progress {
            progress: 10,
            completed_steps: 0,
            total_steps: 1,
            message: "hot path".to_string(),
        })
        .await;
        let envelope = tokio::time::timeout(std::time::Duration::from_millis(200), sub.recv())
            .await
            .expect("live subscriber must receive without DB")
            .expect("channel open");
        assert!(matches!(
            envelope.event,
            AgentProgressEvent::Progress { progress: 10, .. }
        ));
    }

    #[tokio::test]
    async fn terminal_event_is_the_last_event_in_a_run() {
        let run = AgentRun::new("run_terminal".to_string(), 1, None);
        run.publish(AgentProgressEvent::TaskCompleted {
            task_id: "task_terminal".to_string(),
            success: true,
            response: Box::new(serde_json::json!({
                "success": true,
                "message": "done"
            })),
        })
        .await;
        run.publish(AgentProgressEvent::Progress {
            progress: 5,
            completed_steps: 0,
            total_steps: 1,
            message: "too late".to_string(),
        })
        .await;

        let (history, sequence, completed) = run.snapshot().await;
        assert_eq!(history.len(), 1);
        assert_eq!(sequence, 1);
        assert!(completed);
        assert!(matches!(
            history.last().map(|event| &event.event),
            Some(AgentProgressEvent::TaskCompleted { .. })
        ));
    }

    #[tokio::test]
    async fn late_failure_does_not_close_successful_playback() {
        let run = AgentRun::new_for_test("run_late_failure", 7433);
        run.publish(AgentProgressEvent::TaskCompleted {
            task_id: "task".into(),
            success: true,
            response: Box::new(serde_json::json!({"success": true})),
        })
        .await;
        run.publish(AgentProgressEvent::Error {
            task_id: None,
            message: "late producer cleanup".into(),
            code: "AUTONOMY_DISPATCH_FAILED".into(),
        })
        .await;
        run.playback_direction
            .observe(super::super::playback_direction::PlaybackObservation {
                upcoming_text: "still playing".into(),
                rig: serde_json::json!({}),
            });
        assert!(
            run.playback_direction.observation().is_some(),
            "late error closed successful playback"
        );
        assert_eq!(run.snapshot().await.0.len(), 1);
    }

    #[tokio::test]
    async fn playback_direction_after_terminal_is_live_only_and_owner_scoped() {
        let run_id = "playback_direction_owner_test";
        let run = AgentRun::new(run_id.into(), 701, None);
        AGENT_RUNS.write().await.insert(run_id.into(), run.clone());
        assert!(get_live_run_for_user(run_id, 702).await.is_none());
        assert!(get_live_run_for_user(run_id, 701).await.is_some());
        run.publish(AgentProgressEvent::TaskCompleted {
            task_id: String::new(),
            success: true,
            response: Box::new(serde_json::json!({"success":true,"data":{"mode":"chat"}})),
        })
        .await;
        let performance = super::super::merope::local_directive(
            &super::super::motion_overlay::motion_refinement_tests::context(),
        )
        .unwrap();
        assert!(run.playback_direction.publish(performance));
        assert_eq!(run.playback_direction.read_after(0).await.version, 1);
        let (history, sequence, completed) = run.snapshot().await;
        assert!(completed);
        assert_eq!(sequence, 1);
        assert_eq!(history.len(), 1, "playback must not append after terminal");
        let persisted = serde_json::to_value(run.persisted_snapshot().await).unwrap();
        assert!(persisted.get("playback_direction").is_none());
        run.playback_direction.close();
        assert!(
            run.playback_direction
                .read_after(0)
                .await
                .performance
                .is_none()
        );
        AGENT_RUNS.write().await.remove(run_id);
    }
}

#[cfg(test)]
mod execution_cancellation_tests {
    use super::*;
    #[tokio::test]
    async fn explicit_cancel_drops_planning_before_task_creation() {
        let run = AgentRun::new("channel-cancel-test".into(), 777, Some("session".into()));
        let (committed, result) = tokio::sync::oneshot::channel::<()>();
        let execution = tokio::spawn(async move {
            std::future::pending::<()>().await;
            let _ = committed.send(());
        });
        run.register_execution(execution.abort_handle());
        run.abort_execution().await;
        assert!(execution.await.unwrap_err().is_cancelled());
        assert!(result.await.is_err());
        let (events, _, completed) = run.snapshot().await;
        assert!(completed);
        assert!(events.iter().any(|event| matches!(&event.event, AgentProgressEvent::Error { code, .. } if code == "CANCELLED")));
    }
}
