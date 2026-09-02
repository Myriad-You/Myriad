//! Turn identity and Chat supersession.
//!
//! `runId` is the durable root of a Chat/Work turn. Do not invent a second
//! `turnId`. `generation` lives only in memory: a newer Chat request replaces
//! the previous Chat run's text, speech, and motion. Work is never cancelled
//! by Chat. SSE disconnect unsubscribes; it does not cancel the run.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use once_cell::sync::Lazy;
use serde_json::json;
use tokio::sync::{oneshot, Mutex};

use super::types::AgentProgressEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventPlane {
    /// Lifecycle and semantic events. These may use the run hub.
    Control,
    /// Volume, visemes, spectrum, VAD, phonemes. Must never enter the run hub.
    Data,
}

/// Exhaustive: a new `AgentProgressEvent` variant must pick a plane.
pub fn event_plane(event: &AgentProgressEvent) -> EventPlane {
    match event {
        AgentProgressEvent::RunStarted { .. }
        | AgentProgressEvent::TaskCreated { .. }
        | AgentProgressEvent::TaskAssigned { .. }
        | AgentProgressEvent::StepStarted { .. }
        | AgentProgressEvent::StepCompleted { .. }
        | AgentProgressEvent::Progress { .. }
        | AgentProgressEvent::StepRetrying { .. }
        | AgentProgressEvent::TaskCompleted { .. }
        | AgentProgressEvent::WaitingForInput { .. }
        | AgentProgressEvent::SessionCreated { .. }
        | AgentProgressEvent::SessionTitleUpdated { .. }
        | AgentProgressEvent::SummaryToken { .. }
        | AgentProgressEvent::ThinkingToken { .. }
        | AgentProgressEvent::PerformancePlan { .. }
        | AgentProgressEvent::MeropeStateChanged { .. }
        | AgentProgressEvent::Error { .. }
        | AgentProgressEvent::PlannerDecision { .. }
        | AgentProgressEvent::StepDebug { .. } => EventPlane::Control,
    }
}

pub const TURN_SUPERSEDED_CODE: &str = "TURN_SUPERSEDED";

/// Chat completions are spoken lines, not Work outcomes. They must not become
/// persona events, mood bumps, or task notifications.
pub fn is_chat_turn_completion(response: &serde_json::Value) -> bool {
    if response.get("code").and_then(|value| value.as_str()) == Some(TURN_SUPERSEDED_CODE) {
        return true;
    }
    let Some(data) = response.get("data") else {
        return false;
    };
    data.get("mode").and_then(|value| value.as_str()) == Some("chat")
        || data.get("type").and_then(|value| value.as_str()) == Some("chat")
}

struct ChatTurnSlot {
    tx: oneshot::Sender<()>,
    slot_id: u64,
}

static CHAT_TURNS: Lazy<Mutex<HashMap<(i32, String), ChatTurnSlot>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static NEXT_CHAT_SLOT: AtomicU64 = AtomicU64::new(1);

/// Register this Chat run as the live turn for the session. The previous Chat
/// turn, if any, is cancelled. Work must not call this.
pub async fn claim_chat_turn(user_id: i32, session_id: &str) -> (oneshot::Receiver<()>, u64) {
    let (tx, rx) = oneshot::channel();
    let slot_id = NEXT_CHAT_SLOT.fetch_add(1, Ordering::Relaxed);
    let mut slots = CHAT_TURNS.lock().await;
    if let Some(previous) = slots.insert(
        (user_id, session_id.to_string()),
        ChatTurnSlot { tx, slot_id },
    ) {
        let _ = previous.tx.send(());
    }
    (rx, slot_id)
}

/// Drop this Chat slot after it finishes, if a newer claim has not replaced it.
pub async fn finish_chat_turn(user_id: i32, session_id: &str, slot_id: u64) {
    let mut slots = CHAT_TURNS.lock().await;
    let key = (user_id, session_id.to_string());
    if slots.get(&key).is_some_and(|slot| slot.slot_id == slot_id) {
        slots.remove(&key);
    }
}

/// Stop the live Chat turn without starting a replacement. Work must not call this.
pub async fn cancel_chat_turn(user_id: i32, session_id: &str) -> bool {
    let mut slots = CHAT_TURNS.lock().await;
    if !session_id.is_empty() {
        if let Some(previous) = slots.remove(&(user_id, session_id.to_string())) {
            let _ = previous.tx.send(());
            return true;
        }
        return false;
    }
    let keys: Vec<_> = slots
        .keys()
        .filter(|(uid, _)| *uid == user_id)
        .cloned()
        .collect();
    let mut cancelled = false;
    for key in keys {
        if let Some(previous) = slots.remove(&key) {
            let _ = previous.tx.send(());
            cancelled = true;
        }
    }
    cancelled
}

pub fn superseded_turn_event() -> AgentProgressEvent {
    AgentProgressEvent::TaskCompleted {
        task_id: String::new(),
        success: false,
        response: Box::new(json!({
            "success": false,
            "responseType": "error",
            "message": "Replaced by a newer Chat turn",
            "streamTerminal": true,
            "code": TURN_SUPERSEDED_CODE,
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn current_progress_events_are_control_plane() {
        let events = [
            AgentProgressEvent::RunStarted {
                run_id: "run_1".into(),
                session_id: None,
            },
            AgentProgressEvent::SummaryToken {
                token: "hi".into(),
                done: false,
            },
            superseded_turn_event(),
        ];
        for event in &events {
            assert_eq!(event_plane(event), EventPlane::Control);
        }
        let encoded = serde_json::to_string(&events[0]).unwrap();
        for forbidden in [
            "viseme",
            "articulation",
            "spectrum",
            "vad",
            "phoneme",
            "volume",
        ] {
            assert!(
                !encoded.contains(forbidden),
                "control event leaked data-plane field {forbidden}"
            );
        }
    }

    #[test]
    fn chat_turn_completion_is_not_a_persona_event() {
        assert!(is_chat_turn_completion(&json!({
            "success": true,
            "message": "你好。",
            "data": { "reply": "你好。", "type": "chat", "mode": "chat" }
        })));
        assert!(is_chat_turn_completion(&json!({
            "success": false,
            "message": "Replaced by a newer Chat turn",
            "streamTerminal": true,
            "code": TURN_SUPERSEDED_CODE,
        })));
        assert!(!is_chat_turn_completion(&json!({
            "success": true,
            "message": "The task finished",
            "data": { "type": "work" }
        })));
    }

    #[test]
    fn superseded_event_is_detectable_and_terminal_shaped() {
        let event = superseded_turn_event();
        match &event {
            AgentProgressEvent::TaskCompleted {
                success, response, ..
            } => {
                assert!(!*success);
                assert_eq!(response["streamTerminal"], true);
                assert_eq!(response["code"], TURN_SUPERSEDED_CODE);
            }
            other => panic!("expected task completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn cancel_chat_turn_fires_the_live_slot_without_a_replacement() {
        let (first, _) = claim_chat_turn(9, "chat-session").await;
        assert!(cancel_chat_turn(9, "chat-session").await);
        assert!(first.await.is_ok());
        assert!(!cancel_chat_turn(9, "chat-session").await);
    }

    #[tokio::test]
    async fn finish_chat_turn_drops_only_the_matching_slot() {
        let (first, first_id) = claim_chat_turn(8, "chat-session").await;
        let (second, second_id) = claim_chat_turn(8, "chat-session").await;
        assert!(first.await.is_ok());
        finish_chat_turn(8, "chat-session", first_id).await;
        finish_chat_turn(8, "other-session", second_id).await;
        assert!(cancel_chat_turn(8, "chat-session").await);
        drop(second);
        let (_, live_id) = claim_chat_turn(8, "chat-session").await;
        finish_chat_turn(8, "chat-session", live_id).await;
        assert!(!cancel_chat_turn(8, "chat-session").await);
    }

    #[tokio::test]
    async fn newer_chat_cancels_the_previous_chat_once() {
        let (first, _) = claim_chat_turn(12, "chat-session").await;
        let (second, _) = claim_chat_turn(12, "chat-session").await;
        assert!(first.await.is_ok());
        let (third, _) = claim_chat_turn(12, "chat-session").await;
        assert!(second.await.is_ok());
        drop(third);
    }

    #[tokio::test]
    async fn different_sessions_do_not_cancel_each_other() {
        let (mut chat, _) = claim_chat_turn(11, "chat-session").await;
        let _work = claim_chat_turn(11, "work-session").await;
        assert!(tokio::time::timeout(Duration::from_millis(30), &mut chat)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn repeated_claim_is_idempotent_for_the_latest_turn() {
        let (first, _) = claim_chat_turn(13, "s").await;
        let _second = claim_chat_turn(13, "s").await;
        let _again = claim_chat_turn(13, "s").await;
        assert!(first.await.is_ok());
    }
}
