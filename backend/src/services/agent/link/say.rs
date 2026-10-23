//! What she says unprompted, said on the app the person is using her on:
//! the `say` frame. It is reliable: kept for the device until the child's
//! ack covers it, sent again on the device's next connection, and dropped
//! once older than `timing.sayTtlMs` (they did not come back while it still
//! mattered; by then they are not present either).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::connection::Outgoing;
use super::contract::{CONTRACT, valid_id};

/// Which of her lines it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SayKind {
    /// They have just come to her.
    Greeting,
    /// She writes to them first.
    Proactive,
    /// Something she says to them in person.
    FaceToFace,
}

impl SayKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            SayKind::Greeting => "greeting",
            SayKind::Proactive => "proactive",
            SayKind::FaceToFace => "face-to-face",
        }
    }
}

/// One line for the app.
#[derive(Debug, Clone)]
pub(crate) struct Say {
    /// Its idempotency id: the same across resends.
    pub id: String,
    pub kind: SayKind,
    pub text: String,
    pub session_id: Option<String>,
    /// How she acts it (a PerformanceDirective).
    pub perform: Option<Value>,
    pub event_key: Option<String>,
}

struct Pending {
    id: String,
    body: Value,
    until: Instant,
}

/// Lines not yet acknowledged, by device.
static OUTBOX: LazyLock<Mutex<HashMap<String, Vec<Pending>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn ttl() -> Duration {
    Duration::from_millis(CONTRACT.timing.say_ttl_ms)
}

pub(super) fn body(say: &Say, said_at: i64) -> Value {
    let mut body = json!({
        "text": say.text,
        "kind": say.kind.as_str(),
        "saidAt": said_at,
    });
    if let Some(session_id) = &say.session_id {
        body["sessionId"] = session_id.clone().into();
    }
    if let Some(perform) = &say.perform {
        body["perform"] = perform.clone();
    }
    if let Some(event_key) = &say.event_key {
        body["eventKey"] = event_key.clone().into();
    }
    body
}

/// The owner's app `device_id` has a link open that takes `say`.
pub(crate) fn can_say(user_id: i32, device_id: &str) -> bool {
    super::say_sender(user_id, device_id).is_some()
}

/// Says the line on the app; false when no link there takes it (nothing is
/// kept then).
pub(crate) fn say(user_id: i32, device_id: &str, say: Say) -> bool {
    let Some(sender) = super::say_sender(user_id, device_id) else {
        return false;
    };
    if !valid_id(&say.id) || say.text.trim().is_empty() {
        return false;
    }
    let body = body(&say, crate::clock::now().timestamp_millis());
    if let Ok(mut outbox) = OUTBOX.lock() {
        let kept = outbox.entry(device_id.to_string()).or_default();
        let now = Instant::now();
        kept.retain(|pending| pending.until > now && pending.id != say.id);
        kept.push(Pending {
            id: say.id.clone(),
            body: body.clone(),
            until: now + ttl(),
        });
    }
    // A full queue leaves it to the next heartbeat; it is kept either way.
    let _ = sender.try_send(Outgoing::reliable("say", say.id, body));
    true
}

/// The device's lines still waiting for an ack, oldest first; expired ones
/// are dropped here.
pub(super) fn pending(device_id: &str) -> Vec<(String, Value)> {
    let Ok(mut outbox) = OUTBOX.lock() else {
        return Vec::new();
    };
    let Some(kept) = outbox.get_mut(device_id) else {
        return Vec::new();
    };
    let now = Instant::now();
    kept.retain(|pending| pending.until > now);
    let waiting = kept
        .iter()
        .map(|pending| (pending.id.clone(), pending.body.clone()))
        .collect();
    if kept.is_empty() {
        outbox.remove(device_id);
    }
    waiting
}

/// The child has it.
pub(super) fn acked(device_id: &str, id: &str) {
    if let Ok(mut outbox) = OUTBOX.lock() {
        if let Some(kept) = outbox.get_mut(device_id) {
            kept.retain(|pending| pending.id != id);
            if kept.is_empty() {
                outbox.remove(device_id);
            }
        }
    }
}

/// Ages every line kept for the device past its time.
#[cfg(test)]
pub(super) fn age_all(device_id: &str) {
    if let Ok(mut outbox) = OUTBOX.lock() {
        for pending in outbox.get_mut(device_id).into_iter().flatten() {
            pending.until = Instant::now();
        }
    }
}

/// The device is not one any more: nothing is kept for it.
pub(super) fn forget_device(device_id: &str) {
    if let Ok(mut outbox) = OUTBOX.lock() {
        outbox.remove(device_id);
    }
}
