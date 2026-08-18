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
use tokio::sync::{broadcast, Mutex, RwLock};

use crate::services::tapp_registry as shared_registry;

use super::notifications::get_notification_manager;
use super::AgentProgressEvent;

/// 单 run 内存事件环：加长以减少超长任务 re-subscribe 丢中间步骤
const EVENT_HISTORY_LIMIT: usize = 512;
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
    run_id: String,
    user_id: i32,
    session_id: Option<String>,
    created_at: chrono::DateTime<Utc>,
    state: Mutex<AgentRunState>,
    events_tx: broadcast::Sender<AgentRunEnvelope>,
}

static AGENT_RUNS: Lazy<RwLock<HashMap<String, Arc<AgentRun>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

impl AgentRun {
    fn new(run_id: String, user_id: i32, session_id: Option<String>) -> Arc<Self> {
        let (events_tx, _) = broadcast::channel(EVENT_HISTORY_LIMIT);
        Arc::new(Self {
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
                message: "任务已提交，等待执行".to_string(),
                completed: false,
                updated_at: Utc::now(),
            }),
            events_tx,
        })
    }

    fn from_persisted(
        persisted: PersistedAgentRun,
        events: VecDeque<AgentRunEnvelope>,
    ) -> Arc<Self> {
        let (events_tx, _) = broadcast::channel(EVENT_HISTORY_LIMIT);
        Arc::new(Self {
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

    async fn is_executing(&self) -> bool {
        let state = self.state.lock().await;
        !state.completed && state.status != "waiting_for_input"
    }

    async fn is_open(&self) -> bool {
        !self.state.lock().await.completed
    }

    async fn updated_at(&self) -> chrono::DateTime<Utc> {
        self.state.lock().await.updated_at
    }

    async fn persisted_snapshot(&self) -> PersistedAgentRun {
        let state = self.state.lock().await;
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

    async fn persist(&self, envelope: &AgentRunEnvelope) {
        let Ok(db) = shared_registry::database().await else {
            tracing::warn!(run_id = %self.run_id, "[Agent Run] Database unavailable; run snapshot remains local");
            return;
        };
        let snapshot = self.persisted_snapshot().await;
        let expires_at =
            (snapshot.updated_at + chrono::Duration::hours(RUN_RETENTION_HOURS)).timestamp();
        let snapshot_payload = match serde_json::to_value(&snapshot) {
            Ok(payload) => payload,
            Err(error) => {
                tracing::warn!(run_id = %self.run_id, %error, "[Agent Run] Failed to serialize run snapshot");
                return;
            }
        };
        let event_payload = match serde_json::to_value(envelope) {
            Ok(payload) => payload,
            Err(error) => {
                tracing::warn!(run_id = %self.run_id, %error, "[Agent Run] Failed to serialize run event");
                return;
            }
        };
        let event_record_id = format!("{}:{:020}", self.run_id, envelope.sequence);
        let transaction = match db.begin().await {
            Ok(transaction) => transaction,
            Err(error) => {
                tracing::warn!(run_id = %self.run_id, %error, "[Agent Run] Failed to begin persistence transaction");
                return;
            }
        };
        let result = async {
            transaction
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Postgres,
                    "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
                    vec![format!("agent_run:{}", self.run_id).into()],
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
                        self.user_id.into(),
                        self.run_id.clone().into(),
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
                        self.run_id.clone().into(),
                        self.user_id.into(),
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
                        self.run_id.clone().into(),
                        (EVENT_HISTORY_LIMIT as i64).into(),
                    ],
                ))
                .await?;
            transaction.commit().await
        }
        .await;
        if let Err(error) = result {
            tracing::warn!(run_id = %self.run_id, %error, "[Agent Run] Failed to persist shared run snapshot");
        } else {
            shared_registry::maybe_cleanup(&db).await;
        }
    }

    /// Merge a newer shared snapshot and return events not present locally.
    pub async fn refresh_from_registry(&self) -> Vec<AgentRunEnvelope> {
        let Ok(db) = shared_registry::database().await else {
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
    /// Live SSE must not wait on registry DB writes: a slow persist would fill the
    /// mpsc forwarder and freeze step progress. Persistence is best-effort async.
    pub async fn publish(self: &Arc<Self>, event: AgentProgressEvent) {
        let mut notify = false;
        let (envelope, task_id, status, progress, message, success) = {
            let mut state = self.state.lock().await;
            let success = match &event {
                AgentProgressEvent::RunStarted { .. } => {
                    notify = true;
                    state.status = "running".to_string();
                    state.message = "任务已提交，等待执行".to_string();
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
                    notify = true;
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
                            "任务已完成"
                        } else {
                            "任务执行失败"
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
                state.task_id.clone(),
                state.status.clone(),
                state.progress,
                state.message.clone(),
                success,
            )
        };

        // Live path: broadcast immediately so SSE subscribers never wait on DB.
        let _ = self.events_tx.send(envelope.clone());

        // Durable path: best-effort, off the hot path.
        let this = Arc::clone(self);
        let envelope_for_persist = envelope;
        tokio::spawn(async move {
            this.persist(&envelope_for_persist).await;
        });

        if notify {
            if let Some(manager) = get_notification_manager() {
                let title = match status.as_str() {
                    "completed" => "任务完成",
                    "failed" => "任务失败",
                    "cancelled" => "任务已取消",
                    "waiting_for_input" => "任务等待你的回答",
                    _ => "Arael 正在执行任务",
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

pub async fn create_run(user_id: i32, session_id: Option<String>) -> Arc<AgentRun> {
    let cutoff = Utc::now() - chrono::Duration::hours(24);
    let candidates = {
        let runs = AGENT_RUNS.read().await;
        runs.iter()
            .map(|(id, run)| (id.clone(), run.clone()))
            .collect::<Vec<_>>()
    };
    let mut stale_ids = Vec::new();
    for (id, run) in candidates {
        if run.updated_at().await < cutoff {
            stale_ids.push(id);
        }
    }
    if !stale_ids.is_empty() {
        let mut runs = AGENT_RUNS.write().await;
        for id in stale_ids {
            runs.remove(&id);
        }
    }

    let run_id = format!("run_{}", uuid::Uuid::new_v4().simple());
    let run = AgentRun::new(run_id.clone(), user_id, session_id.clone());
    AGENT_RUNS.write().await.insert(run_id.clone(), run.clone());
    run.publish(AgentProgressEvent::RunStarted { run_id, session_id })
        .await;
    run
}

pub async fn user_has_executing_run(user_id: i32) -> bool {
    let runs = AGENT_RUNS.read().await;
    for run in runs.values() {
        if run.user_id == user_id && run.is_executing().await {
            return true;
        }
    }
    false
}

/// Unfinished run, including `waiting_for_input`. Heartbeat uses SYSTEM_USER_ID.
pub async fn user_has_open_run(user_id: i32) -> bool {
    let runs = AGENT_RUNS.read().await;
    for run in runs.values() {
        if run.user_id == user_id && run.is_open().await {
            return true;
        }
    }
    false
}

pub async fn get_run_for_user(run_id: &str, user_id: i32) -> Option<Arc<AgentRun>> {
    if let Some(run) = AGENT_RUNS
        .read()
        .await
        .get(run_id)
        .filter(|run| run.user_id == user_id)
        .cloned()
    {
        return Some(run);
    }

    let db = shared_registry::database().await.ok()?;
    let persisted = shared_registry::get::<PersistedAgentRun>(&db, RUN_REGISTRY_NAMESPACE, run_id)
        .await
        .ok()??;
    if persisted.user_id != user_id || persisted.run_id != run_id {
        return None;
    }
    let events = AgentRun::load_persisted_events(&db, run_id).await.ok()?;
    let run = AgentRun::from_persisted(persisted, events);
    let mut runs = AGENT_RUNS.write().await;
    Some(
        runs.entry(run_id.to_string())
            .or_insert_with(|| run.clone())
            .clone(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
