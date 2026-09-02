//! Wait for the browser to run query_windows / music_get_status and send the
//! live snapshot back before the next recipe step reads `_music_status` /
//! `_window_state`.
//!
//! This is an in-flight oneshot, not WaitingForInput — the user never sees a
//! question card. No SSE listener (sync `process`) skips the wait.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use once_cell::sync::Lazy;
use serde_json::{json, Value};
use tokio::sync::oneshot;

use crate::services::agent::types::ExecutionContext;

use super::events::StepEventEmitter;

const ACK_TIMEOUT: Duration = Duration::from_secs(5);

static PENDING: Lazy<Mutex<HashMap<String, oneshot::Sender<Value>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn ack_key(task_id: &str, step_id: &str) -> String {
    format!("{task_id}\0{step_id}")
}

fn lock_pending() -> std::sync::MutexGuard<'static, HashMap<String, oneshot::Sender<Value>>> {
    PENDING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn actions_need_snapshot_ack(output: &Value) -> bool {
    crate::services::agent::collect_step_frontend_actions(std::iter::once(output))
        .iter()
        .any(|action| {
            matches!(
                action.get("type").and_then(Value::as_str),
                Some("query_windows" | "music_get_status")
            )
        })
}

pub fn begin_if_needed(
    task_id: &str,
    step_id: &str,
    output: &Value,
) -> Option<oneshot::Receiver<Value>> {
    if !actions_need_snapshot_ack(output) {
        return None;
    }
    let (tx, rx) = oneshot::channel();
    lock_pending().insert(ack_key(task_id, step_id), tx);
    Some(rx)
}

pub fn submit(task_id: &str, step_id: &str, payload: Value) -> bool {
    lock_pending()
        .remove(&ack_key(task_id, step_id))
        .is_some_and(|tx| tx.send(payload).is_ok())
}

async fn finish(
    pending: Option<oneshot::Receiver<Value>>,
    task_id: &str,
    step_id: &str,
    capability_id: &str,
    mut output: Value,
    context: &mut ExecutionContext,
) -> Value {
    let Some(rx) = pending else {
        return output;
    };
    let ack = match tokio::time::timeout(ACK_TIMEOUT, rx).await {
        Ok(Ok(value)) => value,
        _ => {
            lock_pending().remove(&ack_key(task_id, step_id));
            return output;
        }
    };
    merge_ack(capability_id, &mut output, &ack, context);
    output
}

fn merge_ack(capability_id: &str, output: &mut Value, ack: &Value, context: &mut ExecutionContext) {
    if let Some(music) = ack.get("musicStatus").filter(|value| value.is_object()) {
        context
            .variables
            .insert("_music_status".to_string(), music.clone());
        if capability_id == "music.status" {
            if let (Some(object), Some(status)) = (output.as_object_mut(), music.as_object()) {
                for (key, value) in status {
                    object.insert(key.clone(), value.clone());
                }
                object.insert("available".to_string(), json!(true));
            }
        }
    }
    if let Some(windows) = ack.get("windowState").filter(|value| !value.is_null()) {
        context
            .variables
            .insert("_window_state".to_string(), windows.clone());
        if capability_id == "tapp.windows" {
            let available = windows
                .get("available")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            if let Some(object) = output.as_object_mut() {
                object.insert("available".to_string(), json!(available));
                if available {
                    if let Some(list) = windows.get("windows") {
                        object.insert("windows".to_string(), list.clone());
                    }
                    if let Some(active) = windows.get("activeWindowId") {
                        object.insert("activeWindowId".to_string(), active.clone());
                    }
                    if let Some(count) = windows.get("windowCount") {
                        object.insert("windowCount".to_string(), count.clone());
                    }
                    object.insert("message".to_string(), Value::Null);
                }
            }
        }
    }
}

/// Register → emit step_completed → wait for the browser snapshot → merge.
pub async fn publish_and_await_snapshots(
    emitter: &StepEventEmitter,
    task_id: &str,
    step_id: &str,
    capability_id: &str,
    step_index: u32,
    duration_ms: u64,
    output: Value,
    context: &mut ExecutionContext,
    send_visible: bool,
) -> Value {
    let pending = if emitter.is_live() {
        begin_if_needed(task_id, step_id, &output)
    } else {
        None
    };
    if send_visible {
        emitter
            .step_output_succeeded(step_id, step_index, duration_ms, &output)
            .await;
    }
    let merged = finish(pending, task_id, step_id, capability_id, output, context).await;
    context.add_output(step_id, merged.clone());
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_and_music_status_need_ack() {
        assert!(actions_need_snapshot_ack(&json!({
            "frontendAction": { "type": "music_get_status", "timestamp": 1 }
        })));
        assert!(actions_need_snapshot_ack(&json!({
            "frontendAction": { "type": "query_windows", "timestamp": 1 }
        })));
        assert!(!actions_need_snapshot_ack(&json!({
            "frontendAction": { "type": "navigate", "path": "/brew", "timestamp": 1 }
        })));
    }

    #[test]
    fn merge_writes_live_music_into_status_output() {
        let mut output = json!({
            "available": false,
            "frontendAction": { "type": "music_get_status" }
        });
        let mut context = ExecutionContext::default();
        merge_ack(
            "music.status",
            &mut output,
            &json!({ "musicStatus": { "isPlaying": true, "isEnabled": true } }),
            &mut context,
        );
        assert_eq!(output["available"], true);
        assert_eq!(output["isPlaying"], true);
        assert_eq!(context.variables["_music_status"]["isPlaying"], true);
    }
}
