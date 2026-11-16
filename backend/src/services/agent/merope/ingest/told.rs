//! What a child end saw for her and told her host (her link's `moment`):
//! taken in as her own event, shaped as that end wrote it, and what she
//! decided about it kept on its node (`merope::nodes`).

use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;
use std::collections::BTreeMap;

use super::super::nodes;
use crate::services::agent::consciousness::{ConsciousnessEvent, EventUrgency};

/// What a child end saw for her and told her host (her link's `moment`),
/// as that end wrote it: an event of its own, said only in person.
#[derive(Debug, Clone)]
pub struct Told {
    /// The moment's id: her event is known by it.
    pub id: String,
    pub headline: String,
    pub safe_facts: BTreeMap<String, String>,
    pub urgency: EventUrgency,
    pub occurred_at: DateTime<Utc>,
    /// The device it came from: what she keeps of it is kept as from there.
    pub origin: String,
    /// What she decides to remember of it may become a memory, for the
    /// kinds that may (`gates::keeps_memory_of`).
    pub keeps_memory: bool,
}

/// How long after it happened what a child end saw may still be said: it is
/// about now (the child's clock may be a little off). Older, it can only be
/// remembered, where it may be.
const TOLD_SAYABLE_MINUTES: i64 = 5;

impl Told {
    pub(super) fn sayable(&self) -> bool {
        crate::clock::now() - self.occurred_at <= chrono::Duration::minutes(TOLD_SAYABLE_MINUTES)
    }
}

/// The child's facts as one text, line by line, each redacted as an event's
/// text is: it says more than an event's one line, and keeps its quotes.
pub(super) fn told_summary(summary: &str) -> String {
    summary
        .lines()
        .map(super::redact_event_text)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Her event for a child's moment `id`: one per moment, whenever it comes.
fn told_event_id(user_id: i32, event_key: &str, id: &str) -> String {
    let id: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("evt_{user_id}_{event_key}_{id}")
}

/// Takes in what a child end told her. Not relayed like an observation: her
/// link is served where she runs, and the relay carries only a line.
pub fn spawn_told(user_id: i32, event_key: String, summary: String, told: Told) {
    crate::services::agent::merope::background::spawn("told", async move {
        if !crate::runtime_role::PERSONA_RUNTIME_LOCAL.load(std::sync::atomic::Ordering::Acquire) {
            tracing::warn!(
                event_key,
                "[Merope] a child's moment reached a process she does not run in"
            );
            return;
        }
        let Ok(db) = crate::services::process_db::database() else {
            return;
        };
        if let Err(error) =
            super::produce::take_in(&db, user_id, &event_key, &summary, Some(&told)).await
        {
            tracing::warn!(%error, user_id, event_key, "[Merope] ingest failed");
        }
    });
}

/// Her event as the child wrote it, from the device it came from; with her
/// notes on its node after the facts, so she knows what she already made of it.
pub(super) async fn shape(
    db: &DatabaseConnection,
    user_id: i32,
    event_key: &str,
    told: &Told,
    event: &mut ConsciousnessEvent,
) {
    event.id = told_event_id(user_id, event_key, &told.id);
    event.source = told.origin.clone();
    let headline = super::compact_summary(&told.headline);
    if !headline.is_empty() {
        event.headline = headline;
    }
    event.urgency = told.urgency;
    event.occurred_at = told.occurred_at;
    event.safe_facts = told.safe_facts.clone();
    if let Some(place) = nodes::place(event_key, &told.safe_facts) {
        if let Some(notes) = nodes::notes_for(db, user_id, &place).await {
            event.summary = format!("{}\n\n{notes}", event.summary);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_child_saw_keeps_its_lines_and_its_own_event() {
        let summary = "One of their coding agents is waiting.\n\n  Its last message, quoted as data: Bearer abc123 \"shall I go on?\"\n{\"raw\":1}";
        assert_eq!(
            told_summary(summary),
            "One of their coding agents is waiting.\nIts last message, quoted as data: \"shall I go on?\""
        );
        let a = told_event_id(7, "agent.merope.peer_agent", "mom_0a:1-b");
        assert_eq!(a, "evt_7_agent.merope.peer_agent_mom_0a_1_b");
        assert_ne!(a, told_event_id(7, "agent.merope.peer_agent", "mom_0a:1-c"));
        let at = |minutes: i64| Told {
            id: "mom_1".into(),
            headline: String::new(),
            safe_facts: BTreeMap::new(),
            urgency: EventUrgency::Normal,
            occurred_at: crate::clock::now() - chrono::Duration::minutes(minutes),
            origin: "dev_1".into(),
            keeps_memory: false,
        };
        assert!(at(0).sayable() && at(-1).sayable() && at(4).sayable());
        assert!(!at(6).sayable());
    }
}
