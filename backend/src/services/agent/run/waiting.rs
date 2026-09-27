//! Tasks waiting on the person to answer a question: who may answer, where
//! the answer goes, and cancelling one so its wait ends.

use std::sync::Arc;

use serde_json::{Value, json};

use sea_orm::DatabaseConnection;

use crate::services::agent::{Agent, AgentProgressEvent};

pub(crate) struct WaitingTaskCtx {
    pub(crate) registration: Arc<()>,
    /// 任务所有者；take 时必须匹配，防止跨用户抢 oneshot
    pub(crate) user_id: i32,
    /// 后端 run 的进度 sender；answer 阶段继续写入同一个 run hub
    pub(crate) progress_tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
    /// 单次信号：answer 处理完成后将最终 response Value 发送至此
    pub(crate) done_tx: tokio::sync::oneshot::Sender<serde_json::Value>,
    /// 会话 ID（用于持久化用户的问答消息到 agent_messages）
    pub(crate) session_id: String,
}

pub(crate) static WAITING_TASKS: once_cell::sync::Lazy<
    std::sync::Mutex<std::collections::HashMap<String, WaitingTaskCtx>>,
> = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Each polling round owns exactly its context, including during task abort.
/// A synchronous lock makes Drop cleanup immediate; no map lock crosses await.
pub(crate) struct WaitingTaskRegistration {
    pub(crate) task_id: String,
    pub(crate) identity: Arc<()>,
}

impl WaitingTaskRegistration {
    pub(crate) fn insert(task_id: &str, context: WaitingTaskCtx) -> Self {
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
pub(crate) async fn take_waiting_task(task_id: &str, user_id: i32) -> Option<WaitingTaskCtx> {
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

/// Terminal payload when the wait-loop oneshot is dropped without a normal answer.
/// Re-subscribers must not hang forever on a non-completed run.
pub(crate) fn wait_loop_channel_dropped_response(task_id: &str) -> Value {
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
pub(crate) fn wait_loop_channel_dropped_event(task_id: &str) -> AgentProgressEvent {
    AgentProgressEvent::TaskCompleted {
        task_id: task_id.to_string(),
        success: false,
        response: Box::new(wait_loop_channel_dropped_response(task_id)),
    }
}
