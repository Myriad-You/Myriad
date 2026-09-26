//! Event-driven autonomy contracts.
//!
//! This layer may decide that something is worth remembering, saying, asking,
//! or proposing as Work. It deliberately cannot select tools, grant
//! permissions, or execute a proposal; accepted work re-enters the existing
//! Planner / Executor path.

mod attention;
mod authorize;
mod dispatch;
mod engine;
mod grant;
mod grant_store;
mod policy;
mod presence;
mod snapshot;
mod speak_intent;
mod store;
mod types;

#[cfg(test)]
pub(crate) fn semantic_probe_contract(soul: &str, kind: &str) -> (String, serde_json::Value) {
    (
        engine::decision_system_prompt(soul),
        engine::decision_schema_for_event(kind),
    )
}

#[cfg(test)]
pub use attention::AttentionSegment;
pub use attention::{last_attention, touch_attention};
#[cfg(test)]
pub use authorize::missing_permission;
pub use authorize::{authorize_capability, effective_granted};
pub use dispatch::{AutonomyClaim, autonomy_claim_decision, build_autonomy_work_request};
#[cfg(test)]
pub use engine::{ConsciousnessGate, forbids_propose_work, pre_gate};
pub use engine::{consider_event, is_work_outcome};
pub use grant::{
    AutonomyGrantView, AutonomyGrantWriteError, AutonomyVerdict, autonomy_execute_permission_error,
    effective_granted_permissions, evaluate_autonomy_grant, intention_may_enter_work,
    prepare_personal_grant, revoke_personal_grant, skips_user_review,
};
pub use grant_store::AutonomyGrantStore;
pub use policy::validate_decision;
pub use presence::{
    last_live_presence, live_presence_from_custom_data, live_presence_from_request,
    live_presence_panel_open, present_users, remember_live_presence,
};
pub use snapshot::capture_self_snapshot;
pub use speak_intent::{SpeakIntent, drain_speak_intents, enqueue_speak_intent, new_speak_intent};
pub use store::IntentStore;
pub use types::{
    AcceptSource, ConsciousnessAction, ConsciousnessDecision, ConsciousnessEvent, EventUrgency,
    IntentRecord, IntentStatus, RecentIntent, SelfFacts, SelfLivePresence, SelfSnapshot,
    WorkProposal,
};

#[cfg(test)]
mod scratch_state_tests {
    /// Speak intent, live presence and the attention segment stay in process
    /// memory on purpose.
    ///
    /// All three are 暂存, not 记忆: they expire on their own, and none of them
    /// is an authorization premise, so nothing about them survives a restart by
    /// design. Writing them to a table would turn a stated intention to speak
    /// and an observed window into 人设记忆 the user never agreed to keep.
    ///
    /// The cost of that choice is a single-process assumption — with more than
    /// one backend instance each holds its own copy. That is a deployment
    /// question, not a reason to persist them; a shared cache would be the
    /// answer, and it would still not be a table.
    #[test]
    fn scratch_state_never_reaches_a_database() {
        for (name, source) in [
            ("speak_intent.rs", include_str!("speak_intent.rs")),
            ("presence.rs", include_str!("presence.rs")),
            ("attention.rs", include_str!("attention.rs")),
        ] {
            for forbidden in ["DatabaseConnection", "sea_orm", "entities::"] {
                assert!(
                    !source.contains(forbidden),
                    "{name} reaches persistence via `{forbidden}`; scratch state must expire, not be stored"
                );
            }
            assert!(
                source.contains("static ") && source.contains("RwLock"),
                "{name} no longer holds its state in process memory"
            );
        }
    }
}

pub(crate) use attention::{cleanup_attention, clear_user_attention};
