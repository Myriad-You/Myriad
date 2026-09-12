/// Where the addressee is, right now. Axes are independent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IngestSight {
    /// The tab is visible.
    pub on_page: bool,
    /// The Agent panel is open (may still be true after the tab hides).
    pub panel_open: bool,
    /// A Chat/Work run currently owns the floor.
    pub executing: bool,
    /// Persona activity is busy (thinking / talking / working).
    pub working: bool,
    pub do_not_disturb: bool,
}

impl IngestSight {
    pub fn looking(&self) -> bool {
        self.on_page && self.panel_open
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestDecision {
    pub allow_model: bool,
    pub notify: bool,
    /// Speak on the face. Independent of `notify`.
    pub live: bool,
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
    "agent.merope.report_ready",
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

pub fn worth_notifying(event_key: &str) -> bool {
    if event_key == "agent.merope.touch" {
        return false;
    }
    is_valuable_event(event_key) || event_key.starts_with("agent.merope.")
}

/// On the page → she may speak on the face.
/// Looking at the panel → no toast.
/// Not looking at the panel → notify when `worth_notifying`.
pub fn decide_ingest(event_key: &str, sight: &IngestSight) -> IngestDecision {
    if event_key == "agent.merope.touch" && !sight.on_page {
        return IngestDecision {
            allow_model: false,
            notify: false,
            live: false,
            reason: "touch_not_present",
        };
    }
    if sight.executing && !is_task_outcome(event_key) {
        return IngestDecision {
            allow_model: false,
            notify: false,
            live: false,
            reason: "executing",
        };
    }
    if sight.working && !is_task_outcome(event_key) {
        return IngestDecision {
            allow_model: false,
            notify: false,
            live: false,
            reason: "working",
        };
    }
    if sight.do_not_disturb {
        return IngestDecision {
            allow_model: false,
            notify: false,
            live: false,
            reason: "dnd",
        };
    }
    let looking = sight.looking();
    let valuable = worth_notifying(event_key);
    IngestDecision {
        allow_model: true,
        notify: valuable && !looking,
        live: sight.on_page,
        reason: if looking {
            "looking"
        } else if sight.on_page {
            "on_page"
        } else if valuable {
            "notify"
        } else {
            "ambient"
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_only_speaks_in_person_and_never_notifies() {
        let event = "agent.merope.touch";
        assert!(!worth_notifying(event));
        assert!(!decide_ingest(event, &IngestSight::default()).allow_model);
        let here = IngestSight {
            on_page: true,
            ..Default::default()
        };
        assert!(decide_ingest(event, &here).live);
        assert!(!decide_ingest(event, &here).notify);
        for busy in [
            IngestSight {
                executing: true,
                ..here.clone()
            },
            IngestSight {
                working: true,
                ..here.clone()
            },
            IngestSight {
                do_not_disturb: true,
                ..here
            },
        ] {
            assert!(!decide_ingest(event, &busy).allow_model);
        }
    }

    fn looking() -> IngestSight {
        IngestSight {
            on_page: true,
            panel_open: true,
            ..Default::default()
        }
    }

    #[test]
    fn looking_at_the_panel_speaks_without_notify() {
        let decision = decide_ingest("agent.task_failed", &looking());
        assert!(decision.allow_model);
        assert!(decision.live);
        assert!(!decision.notify);
        assert_eq!(decision.reason, "looking");
    }

    #[test]
    fn on_page_with_panel_closed_is_live_and_notifies() {
        let sight = IngestSight {
            on_page: true,
            ..Default::default()
        };
        let decision = decide_ingest("agent.merope.greeting", &sight);
        assert!(decision.allow_model);
        assert!(decision.live);
        assert!(decision.notify);
        assert_eq!(decision.reason, "on_page");
    }

    #[test]
    fn executing_blocks_non_outcome_speech() {
        let sight = IngestSight {
            executing: true,
            ..looking()
        };
        let decision = decide_ingest("agent.merope.platform_activity", &sight);
        assert!(!decision.allow_model);
        assert!(!decision.notify);
        assert!(!decision.live);
        assert_eq!(decision.reason, "executing");
    }

    #[test]
    fn valuable_notifies_when_away() {
        let decision = decide_ingest("agent.task_failed", &IngestSight::default());
        assert!(decision.allow_model);
        assert!(decision.notify);
        assert!(!decision.live);
    }

    #[test]
    fn ambient_does_not_notify() {
        let decision = decide_ingest("brew.starred", &IngestSight::default());
        assert!(decision.allow_model);
        assert!(!decision.notify);
        assert!(!decision.live);
    }

    #[test]
    fn greeting_notifies_when_away() {
        let decision = decide_ingest("agent.merope.greeting", &IngestSight::default());
        assert!(decision.allow_model);
        assert!(decision.notify);
        assert!(!decision.live);
    }

    #[test]
    fn greeting_is_live_without_notify_when_looking() {
        let decision = decide_ingest("agent.merope.greeting", &looking());
        assert!(decision.allow_model);
        assert!(decision.live);
        assert!(!decision.notify);
    }

    #[test]
    fn platform_sync_failure_is_valuable() {
        assert!(is_valuable_event("platform.sync.failed"));
        let decision = decide_ingest("platform.sync.failed", &IngestSight::default());
        assert!(decision.notify);
    }

    #[test]
    fn report_ready_is_valuable() {
        assert!(is_valuable_event("agent.merope.report_ready"));
        let decision = decide_ingest("agent.merope.report_ready", &IngestSight::default());
        assert!(decision.allow_model);
        assert!(decision.notify);
    }

    #[test]
    fn dnd_blocks_speech_and_notify() {
        let sight = IngestSight {
            do_not_disturb: true,
            ..looking()
        };
        let decision = decide_ingest("agent.task_failed", &sight);
        assert!(!decision.allow_model);
        assert!(!decision.notify);
        assert_eq!(decision.reason, "dnd");
        assert!(!is_valuable_event("federation.new_follower"));
        assert!(!is_valuable_event("brew.starred"));
    }

    #[test]
    fn working_still_allows_task_outcome() {
        let working = IngestSight {
            working: true,
            ..Default::default()
        };
        let decision = decide_ingest("agent.task_failed", &working);
        assert!(decision.allow_model);
        assert!(decision.notify);
        let blocked = decide_ingest("brew.source_error", &working);
        assert!(!blocked.allow_model);
        assert_eq!(blocked.reason, "working");
    }

    #[test]
    fn task_outcome_may_speak_during_an_executing_run() {
        let sight = IngestSight {
            executing: true,
            working: true,
            ..looking()
        };
        let decision = decide_ingest("agent.task_completed", &sight);
        assert!(decision.allow_model);
        assert!(decision.live);
        assert!(!decision.notify);
    }
}
