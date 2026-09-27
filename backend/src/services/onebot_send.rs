//! OneBot outbound over the worker's live WebSocket.
//!
//! NapCat accepts a connection and then speaks actions on that same socket.
//! A second connection would not replace it, so delivery cannot open its own.
//! The worker installs a sender while the socket is up and removes it on close.
//!
//! Each action gets an `echo`. The matching response completes that delivery.
//! Closing the socket fails every action still waiting, instead of leaving it
//! pending. The pure encoder stays free of `echo`; correlation lives here.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use myriad_agent_rules::channel::ConnectFailureKind;
use myriad_agent_rules::onebot::rules::classify_onebot_handshake;
use serde_json::{Value, json};
use tokio::sync::{Mutex, oneshot};
use uuid::Uuid;

type WriteFn = Arc<dyn Fn(String) -> Result<(), String> + Send + Sync>;

struct Live {
    write: WriteFn,
    pending: HashMap<String, oneshot::Sender<Result<(), String>>>,
}

static SLOT: OnceLock<Mutex<Option<Live>>> = OnceLock::new();

const ACTION_TIMEOUT: Duration = Duration::from_secs(15);

fn slot() -> &'static Mutex<Option<Live>> {
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Install the function that writes one text frame on the current socket.
pub async fn install(write: WriteFn) {
    let mut guard = slot().lock().await;
    if let Some(previous) = guard.take() {
        fail_all(previous, "onebot socket closed");
    }
    *guard = Some(Live {
        write,
        pending: HashMap::new(),
    });
}

/// Drop the sender and fail every action still waiting.
pub async fn clear() {
    let mut guard = slot().lock().await;
    if let Some(previous) = guard.take() {
        fail_all(previous, "onebot socket closed");
    }
}

fn fail_all(live: Live, reason: &str) {
    for (_, sender) in live.pending {
        let _ = sender.send(Err(reason.to_string()));
    }
}

/// Send one action and wait for its `echo`.
///
/// A missing socket, a write failure, a timeout, or a close is transient.
/// `retcode` 1400..=1404 is permanent for this action. The caller still uses
/// that range to close the socket when the frame is a handshake refusal.
pub async fn send_action(mut action: Value) -> Result<(), String> {
    let echo = Uuid::new_v4().to_string();
    if let Some(object) = action.as_object_mut() {
        object.insert("echo".into(), json!(echo));
    }
    let text = serde_json::to_string(&action).map_err(|error| error.to_string())?;
    let (tx, rx) = oneshot::channel();
    {
        let mut guard = slot().lock().await;
        let Some(live) = guard.as_mut() else {
            return Err("onebot socket is not connected".to_string());
        };
        live.write.as_ref()(text).map_err(|_| "onebot socket is not connected".to_string())?;
        live.pending.insert(echo, tx);
    }
    match tokio::time::timeout(ACTION_TIMEOUT, rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err("onebot socket closed".to_string()),
        Err(_) => Err("onebot action timed out".to_string()),
    }
}

/// Finish the action whose `echo` came back.
///
/// A matched echo never closes the socket. A handshake refusal has no echo
/// and is classified by the worker, not here.
pub async fn complete_echo(echo: &Value, retcode: i64) {
    let Some(key) = echo.as_str() else {
        return;
    };
    let kind = classify_onebot_handshake(None, Some(retcode));
    let mut guard = slot().lock().await;
    let Some(live) = guard.as_mut() else {
        return;
    };
    let Some(sender) = live.pending.remove(key) else {
        return;
    };
    let result = if kind == ConnectFailureKind::Permanent {
        Err(format!("onebot action refused: {retcode}"))
    } else if retcode == 0 {
        Ok(())
    } else {
        Err(format!("onebot action failed: {retcode}"))
    };
    let _ = sender.send(result);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex as StdMutex;

    // The live socket is one process-wide slot. Parallel tests would install
    // over each other and fail the wrong waiter.
    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    #[tokio::test]
    async fn a_missing_socket_fails_before_any_frame_is_written() {
        let _lock = TEST_LOCK.lock().expect("test lock");
        clear().await;
        let missing = send_action(json!({"action": "send_private_msg"})).await;
        assert!(missing.is_err());
    }

    #[tokio::test]
    async fn echo_completes_the_matching_action_and_close_fails_the_rest() {
        let _lock = TEST_LOCK.lock().expect("test lock");
        clear().await;
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let capture = seen.clone();
        install(Arc::new(move |text| {
            capture.lock().expect("frames").push(text);
            Ok(())
        }))
        .await;

        let send = tokio::spawn(async { send_action(json!({"action": "send_private_msg"})).await });
        let echo = wait_for_echo(&seen).await;
        complete_echo(&json!(echo), 0).await;
        assert!(send.await.expect("join").is_ok());

        let pending =
            tokio::spawn(async { send_action(json!({"action": "set_input_status"})).await });
        let _ = wait_for_echo(&seen).await;
        clear().await;
        let closed = pending.await.expect("join");
        assert!(closed.is_err());
    }

    #[tokio::test]
    async fn a_refused_retcode_fails_that_action_and_is_permanent() {
        let _lock = TEST_LOCK.lock().expect("test lock");
        clear().await;
        install(Arc::new(|_| Ok(()))).await;
        let send = tokio::spawn(async { send_action(json!({"action": "send_private_msg"})).await });
        tokio::task::yield_now().await;
        {
            let guard = slot().lock().await;
            let echo = guard
                .as_ref()
                .expect("live")
                .pending
                .keys()
                .next()
                .cloned()
                .expect("echo");
            drop(guard);
            complete_echo(&json!(echo), 1403).await;
        }
        assert!(send.await.expect("join").is_err());
        clear().await;
    }

    async fn wait_for_echo(seen: &StdMutex<Vec<String>>) -> String {
        for _ in 0..50 {
            if let Some(text) = seen.lock().expect("frames").last() {
                let value: Value = serde_json::from_str(text).expect("json");
                if let Some(echo) = value.get("echo").and_then(Value::as_str) {
                    return echo.to_string();
                }
            }
            tokio::task::yield_now().await;
        }
        panic!("echo was not written");
    }
}
