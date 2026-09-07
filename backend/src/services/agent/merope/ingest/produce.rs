//! Event in → speak intent out. No sentence, no notice, no motion.

use chrono::Utc;
use sea_orm::DatabaseConnection;
use std::collections::BTreeMap;

use super::super::gates::{decide_ingest, is_valuable_event};
use super::super::store::{
    affect_from_state, get_or_create_state, insert_diary, recently_spoke_event, update_affect,
    DIARY_SOURCE_EVENT,
};
use super::super::{apply_task_outcome, is_extremely_low, is_logged_in_addressee};
use super::{
    compact_summary, current_sight, is_enabled, is_trivial_line, log_skip,
    persist_persona_remember, SAME_EVENT_MINUTES,
};
use crate::services::agent::consciousness::{
    consider_event, enqueue_speak_intent, is_work_outcome, new_speak_intent, ConsciousnessAction,
    ConsciousnessEvent, EventUrgency, IntentStore,
};

pub fn work_outcome_parent(
    event_key: &str,
    latest_work_source_event: Option<String>,
) -> Option<String> {
    if is_work_outcome(event_key) {
        latest_work_source_event
    } else {
        None
    }
}

pub fn stable_consciousness_event_id(user_id: i32, event_key: &str, summary: &str) -> String {
    let key: String = event_key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let distinguisher = if is_work_outcome(event_key) {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        summary.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    } else {
        (Utc::now().timestamp() / (SAME_EVENT_MINUTES * 60)).to_string()
    };
    format!("evt_{user_id}_{key}_{distinguisher}")
}

pub fn spawn_diary(user_id: i32, summary: impl Into<String>) {
    let summary = compact_summary(&summary.into());
    if summary.is_empty() {
        return;
    }
    tokio::spawn(async move {
        if !is_logged_in_addressee(user_id) || !is_enabled().await {
            return;
        }
        let Ok(db) = crate::services::tapp_registry::database().await else {
            return;
        };
        if let Err(error) = insert_diary(&db, user_id, &summary, DIARY_SOURCE_EVENT).await {
            tracing::debug!(%error, user_id, "[Merope] diary write failed");
        }
    });
}

pub fn spawn_presence(user_id: i32) {
    if !is_logged_in_addressee(user_id) {
        return;
    }
    tokio::spawn(async move {
        if !is_enabled().await {
            return;
        }
        let Ok(db) = crate::services::tapp_registry::database().await else {
            return;
        };
        let Ok(state) = get_or_create_state(&db, user_id).await else {
            return;
        };
        let gap_hours = state
            .last_user_message_at
            .map(|at| (Utc::now() - at.with_timezone(&Utc)).num_minutes() as f64 / 60.0)
            .unwrap_or(24.0);
        let first_today = state
            .last_user_message_at
            .is_none_or(|at| at.with_timezone(&Utc).date_naive() != Utc::now().date_naive());
        if !first_today && gap_hours < 12.0 {
            return;
        }
        if recently_spoke_event(&db, user_id, "agent.merope.greeting", 12 * 60)
            .await
            .unwrap_or(true)
        {
            return;
        }
        let summary = if first_today {
            "这个人今天第一次来了"
        } else {
            "这个人隔了很久又来了"
        };
        if let Err(error) = ingest(&db, user_id, "agent.merope.greeting", summary).await {
            tracing::debug!(%error, user_id, "[Merope] presence ingest failed");
        }
    });
}

pub fn spawn(user_id: i32, event_key: impl Into<String>, summary: impl Into<String>) {
    let event_key = event_key.into();
    let summary = summary.into();
    tokio::spawn(async move {
        let Ok(db) = crate::services::tapp_registry::database().await else {
            return;
        };
        if let Err(error) = ingest(&db, user_id, &event_key, &summary).await {
            tracing::warn!(
                %error,
                user_id,
                event_key,
                "[Merope] ingest failed"
            );
        }
    });
}

pub async fn ingest(
    db: &DatabaseConnection,
    user_id: i32,
    event_key: &str,
    summary: &str,
) -> Result<(), anyhow::Error> {
    if !is_logged_in_addressee(user_id) {
        return Ok(());
    }
    if !is_enabled().await {
        log_skip(user_id, event_key, "disabled");
        return Ok(());
    }
    let summary = compact_summary(summary);
    if summary.is_empty() {
        log_skip(user_id, event_key, "empty_summary");
        return Ok(());
    }

    let state = get_or_create_state(db, user_id).await?;
    let sight = current_sight(user_id, &state).await;
    let decision = decide_ingest(event_key, &sight);
    // Whether this is a Chat completion is a property of the event, and it is
    // already filtered twice: `run_hub` stops publishing one, and the match in
    // `apply_task_mood` ignores every key but the three task outcomes. Whether
    // the addressee happens to be chatting right now is a different question,
    // and gating on it meant a real Work task that finished inside the chat
    // window never counted — success or failure — for good.
    apply_task_mood(db, user_id, event_key).await;

    if !decision.allow_model {
        log_skip(user_id, event_key, decision.reason);
        let _ = insert_diary(db, user_id, &summary, DIARY_SOURCE_EVENT).await;
        return Ok(());
    }

    let parent_event_id = work_outcome_parent(
        event_key,
        IntentStore::new(db.clone())
            .latest_work_source_event(user_id)
            .await
            .ok()
            .flatten(),
    );
    let conscious_event = ConsciousnessEvent {
        id: stable_consciousness_event_id(user_id, event_key, &summary),
        source: "merope".into(),
        kind: event_key.to_string(),
        headline: summary.clone(),
        summary: summary.clone(),
        addressee_user_id: user_id,
        urgency: if is_valuable_event(event_key) {
            EventUrgency::Soon
        } else {
            EventUrgency::Normal
        },
        occurred_at: Utc::now(),
        parent_event_id,
        safe_facts: BTreeMap::new(),
    };
    let consideration = match consider_event(db, &conscious_event).await {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%error, event_key, "[Merope] consciousness decision failed");
            None
        }
    };

    if let Some(value) = consideration.as_ref() {
        match value.decision.action {
            ConsciousnessAction::Ignore => {
                return Ok(());
            }
            ConsciousnessAction::Remember => {
                persist_persona_remember(db, user_id, value.decision.memory.as_deref()).await;
                return Ok(());
            }
            ConsciousnessAction::Speak | ConsciousnessAction::Ask => {
                persist_persona_remember(db, user_id, value.decision.memory.as_deref()).await;
            }
            ConsciousnessAction::ProposeWork => {}
        }
    }

    let gist = consideration
        .as_ref()
        .and_then(|value| match value.decision.action {
            ConsciousnessAction::Speak => value.decision.speech.clone(),
            ConsciousnessAction::Ask => value.decision.question.clone(),
            ConsciousnessAction::ProposeWork => value
                .intent
                .as_ref()
                .map(|intent| format!("我注意到{}。要不要交给我处理？", intent.proposal.title)),
            ConsciousnessAction::Ignore | ConsciousnessAction::Remember => None,
        })
        .filter(|text| !is_trivial_line(text));
    if let Some(gist) = gist {
        let work_intent_id = consideration
            .as_ref()
            .and_then(|value| value.intent.as_ref())
            .map(|intent| intent.id.clone());
        enqueue_speak_intent(new_speak_intent(
            user_id,
            conscious_event.id.clone(),
            event_key.to_string(),
            gist,
            conscious_event.urgency,
            work_intent_id,
        ));
        let speak_db = db.clone();
        tokio::spawn(async move {
            super::tick_speak_intents(speak_db).await;
        });
    }

    let _ = insert_diary(db, user_id, &summary, DIARY_SOURCE_EVENT).await;
    Ok(())
}

/// Whether an event is a task outcome, and whether it went well.
///
/// This is the only thing that decides if mood moves — not what the addressee
/// happened to be doing when the event arrived.
fn task_mood_outcome(event_key: &str) -> Option<bool> {
    match event_key {
        "agent.task_completed" => Some(true),
        "agent.task_failed" | "agent.task_cancelled" => Some(false),
        _ => None,
    }
}

async fn apply_task_mood(db: &DatabaseConnection, user_id: i32, event_key: &str) {
    let Some(succeeded) = task_mood_outcome(event_key) else {
        return;
    };
    let Ok((previous, saved)) = update_affect(db, user_id, false, |affect| {
        apply_task_outcome(affect, succeeded);
    })
    .await
    else {
        return;
    };
    let after = affect_from_state(&saved);
    if !is_extremely_low(previous.mood) && is_extremely_low(after.mood) {
        spawn(
            user_id,
            "agent.merope.mood_floor",
            "跟这个人的心情掉到了极低",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_writes_persona_memory_not_event_ledger() {
        let src = include_str!("produce.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(src.contains("ConsciousnessAction::Remember"));
        assert!(src.contains("persist_persona_remember"));
        assert!(src.contains("ConsciousnessAction::Speak | ConsciousnessAction::Ask"));
        assert!(!src.contains("insert_diary(db, user_id, memory, \"event\")"));
        assert!(!src.contains("insert_diary(db, user_id, &memory, \"event\")"));
        assert!(src.contains("ConsciousnessAction::Ignore =>") && src.contains("return Ok(());"));
        let speak_arm = src
            .find("ConsciousnessAction::Speak | ConsciousnessAction::Ask")
            .expect("speak/ask arm");
        let persist_in_arm = src[speak_arm..]
            .find("persist_persona_remember")
            .expect("persist in speak/ask arm");
        let persist_at = speak_arm + persist_in_arm;
        let enqueue_at = speak_arm
            + src[speak_arm..]
                .find("enqueue_speak_intent")
                .expect("produce enqueues a speak intent");
        assert!(
            persist_at < enqueue_at,
            "Speak/Ask memory must persist before enqueue"
        );
        let produce = src
            .split("pub async fn ingest(")
            .nth(1)
            .expect("produce path");
        assert!(produce.contains("enqueue_speak_intent"));
        assert!(!produce.contains("insert_proactive"));
        assert!(!produce.contains("emit_speech_notification"));
        assert!(!produce.contains("direct_motion"));
    }

    #[test]
    fn work_outcome_event_ids_are_stable_for_the_same_summary() {
        let a = stable_consciousness_event_id(7, "agent.task_completed", "报告写好了");
        let b = stable_consciousness_event_id(7, "agent.task_completed", "报告写好了");
        let c = stable_consciousness_event_id(7, "agent.task_completed", "另一件事");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("evt_7_agent.task_completed_"));
    }

    #[test]
    fn ordinary_event_ids_share_a_time_bucket() {
        let a = stable_consciousness_event_id(7, "brew.source_error", "feed failed");
        let b = stable_consciousness_event_id(7, "brew.source_error", "feed failed again");
        assert_eq!(a, b);
    }

    #[test]
    fn work_outcomes_chain_parent_to_the_proposal_source() {
        assert_eq!(
            work_outcome_parent(
                "agent.task_completed",
                Some("evt_7_brew.source_error_1".into())
            ),
            Some("evt_7_brew.source_error_1".into())
        );
        assert_eq!(
            work_outcome_parent(
                "brew.source_error",
                Some("evt_7_brew.source_error_1".into())
            ),
            None
        );
    }

    /// Mood follows what happened, not what the addressee was doing when it
    /// happened. Chat completions are filtered by event kind — in `run_hub`,
    /// and again by the match in `apply_task_mood` — so a Work outcome must
    /// still count while the addressee is mid-conversation.
    #[test]
    fn task_mood_follows_the_event_kind_not_the_addressees_activity() {
        let src = include_str!("produce.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let apply = src
            .find("apply_task_mood(db")
            .expect("apply_task_mood call");
        let guard = src[..apply].rfind("if !chatting");
        assert!(
            guard.is_none_or(|at| apply - at > 400),
            "a Work outcome must not be dropped because the addressee is chatting"
        );
        assert!(src.contains("current_sight(user_id, &state)"));
    }

    #[test]
    fn only_task_outcomes_move_mood() {
        for key in [
            "agent.task_progress",
            "agent.clarification",
            "agent.chat_completed",
            "merope.diary",
        ] {
            assert_eq!(task_mood_outcome(key), None, "{key} must not move mood");
        }
        assert_eq!(task_mood_outcome("agent.task_completed"), Some(true));
        assert_eq!(task_mood_outcome("agent.task_failed"), Some(false));
        assert_eq!(task_mood_outcome("agent.task_cancelled"), Some(false));
    }
}
