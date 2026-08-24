use chrono::{DateTime, Utc};

const CHAT_ACTIVE_SECS: i64 = 90;

pub fn is_chatting(
    last_active_at: Option<DateTime<Utc>>,
    has_open_run: bool,
    now: DateTime<Utc>,
) -> bool {
    if has_open_run {
        return true;
    }
    last_active_at.is_some_and(|at| (now - at).num_seconds() < CHAT_ACTIVE_SECS)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestDecision {
    pub allow_model: bool,
    pub notify: bool,
    pub reason: &'static str,
}

const VALUABLE: &[&str] = &[
    "agent.task_completed",
    "agent.task_failed",
    "agent.task_cancelled",
    "agent.clarification",
    "brew.source_error",
    "platform.sync.failed",
    "agent.merope.platform_activity",
];

pub fn is_valuable_event(event_key: &str) -> bool {
    VALUABLE.contains(&event_key) || event_key.contains("platform") && event_key.contains("error")
}

pub fn is_task_outcome(event_key: &str) -> bool {
    matches!(
        event_key,
        "agent.task_completed"
            | "agent.task_failed"
            | "agent.task_cancelled"
            | "agent.clarification"
    )
}

pub fn decide_ingest(
    event_key: &str,
    do_not_disturb: bool,
    chatting: bool,
    working: bool,
) -> IngestDecision {
    if chatting {
        return IngestDecision {
            allow_model: false,
            notify: false,
            reason: "chatting",
        };
    }
    if working && !is_task_outcome(event_key) {
        return IngestDecision {
            allow_model: false,
            notify: false,
            reason: "working",
        };
    }
    if do_not_disturb {
        return IngestDecision {
            allow_model: false,
            notify: false,
            reason: "dnd",
        };
    }
    let valuable = is_valuable_event(event_key);
    IngestDecision {
        allow_model: true,
        notify: valuable,
        reason: if valuable { "valuable" } else { "ambient" },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn chatting_is_recent_activity_or_open_run() {
        let now = Utc::now();
        assert!(is_chatting(None, true, now));
        assert!(is_chatting(Some(now - Duration::seconds(10)), false, now));
        assert!(!is_chatting(Some(now - Duration::seconds(120)), false, now));
    }

    #[test]
    fn chatting_blocks_valuable_notify() {
        let decision = decide_ingest("agent.task_failed", false, true, false);
        assert!(!decision.allow_model);
        assert!(!decision.notify);
        assert_eq!(decision.reason, "chatting");
    }

    #[test]
    fn valuable_notifies_when_idle() {
        let decision = decide_ingest("agent.task_failed", false, false, false);
        assert!(decision.allow_model);
        assert!(decision.notify);
    }

    #[test]
    fn ambient_does_not_notify() {
        let decision = decide_ingest("agent.merope.greeting", false, false, false);
        assert!(decision.allow_model);
        assert!(!decision.notify);
    }

    #[test]
    fn platform_sync_failure_is_valuable() {
        assert!(is_valuable_event("platform.sync.failed"));
        let decision = decide_ingest("platform.sync.failed", false, false, false);
        assert!(decision.notify);
    }

    #[test]
    fn dnd_blocks_speech_and_notify() {
        let decision = decide_ingest("agent.task_failed", true, false, false);
        assert!(!decision.allow_model);
        assert!(!decision.notify);
        assert_eq!(decision.reason, "dnd");
        assert!(!is_valuable_event("federation.new_follower"));
        assert!(!is_valuable_event("brew.starred"));
    }

    #[test]
    fn working_still_allows_task_outcome() {
        let decision = decide_ingest("agent.task_failed", false, false, true);
        assert!(decision.allow_model);
        assert!(decision.notify);
        let blocked = decide_ingest("brew.source_error", false, false, true);
        assert!(!blocked.allow_model);
        assert_eq!(blocked.reason, "working");
    }
}
