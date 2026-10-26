//! What a child end saw for her and told her host (her link's `moment`):
//! taken in as her own event, shaped as that end wrote it, and what she
//! decided about it kept on its node (`merope::nodes`).

use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;
use std::collections::BTreeMap;

use super::super::nodes;
use crate::services::agent::consciousness::{
    ConsciousnessAction, ConsciousnessDecision, ConsciousnessEvent, EventUrgency,
};

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

/// What she decided about it, kept on its node: the line she would say (not
/// said yet) or what she noted; a decision without either is still kept.
pub(super) async fn file(
    db: &DatabaseConnection,
    user_id: i32,
    event_key: &str,
    told: &Told,
    event: &ConsciousnessEvent,
    decision: &ConsciousnessDecision,
) {
    let Some(place) = nodes::place(event_key, &told.safe_facts) else {
        return;
    };
    if let Err(error) = nodes::decided(db, user_id, &place, filed(event, decision, told)).await {
        tracing::warn!(%error, event_key, "[Merope] could not keep a decision on its node");
    }
}

/// The decision as its node keeps it.
pub(super) fn filed<'a>(
    event: &'a ConsciousnessEvent,
    decision: &'a ConsciousnessDecision,
    told: &'a Told,
) -> nodes::Decided<'a> {
    let action = match decision.action {
        ConsciousnessAction::Ignore => "ignore",
        ConsciousnessAction::Remember => "remember",
        ConsciousnessAction::Speak => "speak",
        ConsciousnessAction::Ask => "ask",
        ConsciousnessAction::ProposeWork => "propose_work",
    };
    let line = match decision.action {
        ConsciousnessAction::Speak => decision.speech.clone(),
        ConsciousnessAction::Ask => decision.question.clone(),
        ConsciousnessAction::Remember => decision.memory.clone(),
        ConsciousnessAction::Ignore | ConsciousnessAction::ProposeWork => None,
    }
    .map(|line| line.trim().to_string())
    .filter(|line| !line.is_empty());
    nodes::Decided {
        event: &event.id,
        happened: told
            .safe_facts
            .get("happened")
            .map(String::as_str)
            .unwrap_or(""),
        action,
        reason: &decision.reason_code,
        origin: &told.origin,
        note: line.map(|line| nodes::Note {
            event: event.id.clone(),
            happened: told.safe_facts.get("happened").cloned().unwrap_or_default(),
            action: action.to_string(),
            line,
            at: crate::clock::now(),
            // What she noted is kept as hers; what she would say waits to be said.
            said: decision.action == ConsciousnessAction::Remember,
        }),
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

    /// What she decided is kept as her line waiting to be said, as what she
    /// noted, or as the decision alone.
    #[test]
    fn a_decision_is_kept_on_its_node_as_she_made_it() {
        use crate::services::agent::consciousness::{ConsciousnessAction, ConsciousnessDecision};
        let told = Told {
            id: "mom_1".into(),
            headline: String::new(),
            safe_facts: [("happened".to_string(), "waiting".to_string())].into(),
            urgency: EventUrgency::Soon,
            occurred_at: crate::clock::now(),
            origin: "dev_mac".into(),
            keeps_memory: false,
        };
        let event = ConsciousnessEvent {
            id: "evt_1_x".into(),
            source: "dev_mac".into(),
            kind: "agent.merope.peer_agent".into(),
            headline: String::new(),
            summary: "s".into(),
            addressee_user_id: 1,
            urgency: EventUrgency::Soon,
            occurred_at: crate::clock::now(),
            parent_event_id: None,
            safe_facts: told.safe_facts.clone(),
            keeps_memory: false,
        };
        let decision = |action, memory: Option<&str>, speech: Option<&str>| ConsciousnessDecision {
            action,
            reason_code: "r".into(),
            confidence: 0.9,
            memory: memory.map(Into::into),
            speech: speech.map(Into::into),
            question: None,
            work_proposal: None,
        };
        let speak = decision(ConsciousnessAction::Speak, Some("x"), Some(" 它在等你。 "));
        let kept = filed(&event, &speak, &told);
        assert_eq!(
            (kept.action, kept.happened, kept.origin),
            ("speak", "waiting", "dev_mac")
        );
        let note = kept.note.unwrap();
        assert_eq!((note.line.as_str(), note.said), ("它在等你。", false));
        let remember = decision(ConsciousnessAction::Remember, Some("他们在改测试"), None);
        let note = filed(&event, &remember, &told).note.unwrap();
        assert_eq!((note.action.as_str(), note.said), ("remember", true));
        let ignore = decision(ConsciousnessAction::Ignore, None, None);
        let kept = filed(&event, &ignore, &told);
        assert_eq!((kept.action, kept.note.is_none()), ("ignore", true));
        let empty = decision(ConsciousnessAction::Speak, None, Some("  "));
        assert!(filed(&event, &empty, &told).note.is_none());
    }
}
