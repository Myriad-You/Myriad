//! OneBot outbound over the worker's live WebSocket.
//!
//! NapCat accepts a connection and then speaks actions on that same socket.
//! A second connection would not replace it, so delivery cannot open its own.
//! The worker installs a sender while the socket is up and removes it on close.

use std::sync::{Arc, OnceLock};

use serde_json::Value;
use tokio::sync::Mutex;

type SendFn = Arc<dyn Fn(Value) -> Result<(), String> + Send + Sync>;

static SENDER: OnceLock<Mutex<Option<SendFn>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<SendFn>> {
    SENDER.get_or_init(|| Mutex::new(None))
}

/// Install the function that writes one action frame on the current socket.
pub async fn install(send: SendFn) {
    *slot().lock().await = Some(send);
}

/// Drop the sender. Later deliveries fail as transient until the next connect.
pub async fn clear() {
    *slot().lock().await = None;
}

/// Send one action. Missing sender is transient: the worker may reconnect.
pub async fn send_action(action: Value) -> Result<(), String> {
    let guard = slot().lock().await;
    let Some(send) = guard.as_ref() else {
        return Err("onebot socket is not connected".to_string());
    };
    send(action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex as StdMutex;

    #[tokio::test]
    async fn a_missing_socket_fails_and_an_installed_sender_receives_the_action() {
        clear().await;
        let missing = send_action(json!({"action": "send_private_msg"})).await;
        assert!(missing.is_err());

        let seen = Arc::new(StdMutex::new(None));
        let capture = seen.clone();
        install(Arc::new(move |action| {
            *capture.lock().expect("sender lock") = Some(action);
            Ok(())
        }))
        .await;
        send_action(json!({"action": "send_private_msg", "echo": "1"}))
            .await
            .expect("connected");
        assert_eq!(
            seen.lock()
                .expect("seen")
                .as_ref()
                .and_then(|v| v.get("echo")),
            Some(&json!("1"))
        );
        clear().await;
        assert!(send_action(json!({})).await.is_err());
    }
}
