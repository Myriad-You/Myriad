use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::services::agent::AgentInteractionMode;

/// Priority supplied by the trusted event adapter, not inferred from raw text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventUrgency {
    Immediate,
    Soon,
    #[default]
    Normal,
    Low,
}

/// A normalized event safe to expose to the consciousness model.
///
/// Adapters must reduce raw payloads to short, non-secret facts. Credentials and
/// arbitrary application payloads do not belong in this contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsciousnessEvent {
    pub id: String,
    pub source: String,
    pub kind: String,
    pub headline: String,
    pub summary: String,
    pub addressee_user_id: i32,
    #[serde(default)]
    pub urgency: EventUrgency,
    pub occurred_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub safe_facts: BTreeMap<String, String>,
    /// Of a kind said only in person, what she decides to remember of it
    /// may still become a memory of them (`gates::keeps_memory_of`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keeps_memory: bool,
}

/// Compact prior intention exposed to the decision model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentIntent {
    pub id: String,
    pub summary: String,
    pub status: IntentStatus,
    pub updated_at: DateTime<Utc>,
}

/// Which kind of end a live observation came from: a page of the site, or a
/// native child on her link. Observational only, never a grant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LiveSurface {
    #[default]
    Web,
    Native,
}

/// Instant live-face / speech facts. Memory-only; never a grant or a tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct SelfLivePresence {
    /// Page-instance lease key, never a user identity or an authorization fact.
    #[serde(skip)]
    pub instance_id: Option<String>,
    /// Where it was observed. Everything that reads presence as "on the site"
    /// reads only the web surface.
    #[serde(skip)]
    pub surface: LiveSurface,
    pub speaking: bool,
    pub face_visible: bool,
    /// Document is visible in this browser. Independent of the face widget.
    pub page_visible: bool,
    /// Agent panel is open in this browser. Independent of being on the page.
    pub panel_visible: bool,
    pub speech_interruptible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motion_intent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speech_intent: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub perception: Vec<String>,
    /// Sanitized source-shaped observations for another trusted Chat ingress.
    /// The decision snapshot uses the compact reader text above.
    #[serde(skip)]
    pub perception_payload: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub music_status: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rig_state: Option<myriad_merope::RigStateSummary>,
    /// When this observation was written. `None` is treated as expired.
    #[serde(default)]
    pub captured_at: Option<DateTime<Utc>>,
    /// When they last came to this page or app, or did something there: its
    /// visibility or panel changed, or they said something. Unchanged
    /// reports keep it. Which surface they are using is the latest one.
    #[serde(skip)]
    pub changed_at: Option<DateTime<Utc>>,
}

/// Trusted runtime facts describing who the Agent is speaking to and what it
/// can actually do at this instant.
///
/// `granted_permissions` is the runtime-filtered grant set. Declared or
/// installation-approved permissions must never be substituted here.
/// `live` is observational only and never expands what this layer may execute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelfSnapshot {
    pub persona_name: String,
    pub addressee_user_id: i32,
    pub interaction_mode: AgentInteractionMode,
    pub mood: f64,
    pub activity: String,
    pub do_not_disturb: bool,
    pub has_active_work: bool,
    #[serde(default)]
    pub granted_permissions: Vec<String>,
    #[serde(default)]
    pub recent_intents: Vec<RecentIntent>,
    /// Persona-chosen facts about this addressee. Not Work memory.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remembered: Vec<String>,
    pub captured_at: DateTime<Utc>,
    #[serde(default)]
    pub live: SelfLivePresence,
    #[serde(default)]
    pub attention: Option<super::attention::AttentionSegment>,
    /// Facts about her own day, not toward this person. How they make her
    /// feel is the decision model's judgment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub myself: Option<SelfFacts>,
    /// Who they are to her, as she calls them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addressee_name: Option<String>,
    /// Minutes since she last spoke up to them unasked, if she has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minutes_since_you_spoke_unasked: Option<f64>,
    /// What she said to them lately unasked, newest first: what they have
    /// already heard from her, and whether they wrote back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub said_unasked_lately: Vec<SaidUnasked>,
    /// How she was after her last exchange with them, if a moment ago.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inner: Option<String>,
    /// The owner's Macs linked now that let the site ask them to act: a
    /// proposal may name `mac` to run there.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macs_for_work: Vec<String>,
}

/// A line she said to them unasked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaidUnasked {
    pub minutes_ago: i64,
    /// Her words, as she said them.
    pub line: String,
    /// They have written to her in private since.
    pub they_wrote_since: bool,
}

/// The persona's own day, as plain facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SelfFacts {
    /// Her local time, `HH:MM`.
    #[serde(default)]
    pub local_time: String,
    /// Different people who talked with her in the last few hours.
    #[serde(default)]
    pub people_recently: usize,
    #[serde(default)]
    pub hours_since_anyone: Option<f64>,
    #[serde(default)]
    pub hours_since_learned: Option<f64>,
    /// How what happened today has left her, in words; none when even.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub today_left_you: Option<String>,
}

/// Side-effect class selected by the consciousness model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsciousnessAction {
    Ignore,
    Remember,
    Speak,
    ProposeWork,
    Ask,
}

/// Natural-language work suggestion. It intentionally contains no tool,
/// capability, permission, or pre-authorized parameter selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkProposal {
    pub title: String,
    pub instruction: String,
    pub expected_outcome: String,
    pub source_event_id: String,
    /// Where accepting it runs the Work; none is the site. She is offered
    /// `mac` only while a Mac of the owner's that takes requests is linked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<WorkPlace>,
    /// The Mac a `mac` proposal is for, chosen by the host when it is made
    /// (whatever the model wrote here is replaced).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
}

impl WorkProposal {
    /// A proposal to run on the owner's Mac.
    pub fn on_mac(&self) -> bool {
        self.place == Some(WorkPlace::Mac)
    }
}

/// Where a proposal's Work runs once accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkPlace {
    /// On the site, her host.
    Site,
    /// On the owner's Mac, a child of her host (contract `act`).
    Mac,
}

impl WorkPlace {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Site => "site",
            Self::Mac => "mac",
        }
    }
}

/// Structured output accepted from the strict Lite decision call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsciousnessDecision {
    pub action: ConsciousnessAction,
    /// Stable machine-readable reason, suitable for tracing and tests.
    pub reason_code: String,
    /// Model confidence in the selected action, constrained to 0..=1.
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speech: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_proposal: Option<WorkProposal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentStatus {
    Proposed,
    Accepted,
    Running,
    Waiting,
    Completed,
    Failed,
    Abandoned,
    Expired,
}

impl IntentStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Accepted => "accepted",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Abandoned => "abandoned",
            Self::Expired => "expired",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "proposed" => Some(Self::Proposed),
            "accepted" => Some(Self::Accepted),
            "running" => Some(Self::Running),
            "waiting" => Some(Self::Waiting),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "abandoned" => Some(Self::Abandoned),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Abandoned | Self::Expired
        )
    }

    pub const fn can_transition_to(self, next: Self) -> bool {
        match self {
            Self::Proposed => matches!(next, Self::Accepted | Self::Abandoned | Self::Expired),
            Self::Accepted => matches!(next, Self::Running | Self::Abandoned | Self::Expired),
            Self::Running => matches!(
                next,
                Self::Waiting | Self::Completed | Self::Failed | Self::Abandoned | Self::Accepted
            ),
            Self::Waiting => matches!(
                next,
                Self::Running | Self::Completed | Self::Failed | Self::Abandoned | Self::Expired
            ),
            Self::Completed | Self::Failed | Self::Abandoned | Self::Expired => false,
        }
    }
}

/// Durable intention state. The repository is responsible for enforcing
/// `IntentStatus::can_transition_to` when updating this record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentRecord {
    pub id: String,
    pub user_id: i32,
    pub source_event_id: String,
    pub summary: String,
    pub reason_code: String,
    pub status: IntentStatus,
    pub proposal: WorkProposal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    /// Who accepted this proposal: `user` or `autonomy`.
    pub accept_source: AcceptSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AcceptSource {
    #[default]
    User,
    Autonomy,
}

impl AcceptSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Autonomy => "autonomy",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "autonomy" => Self::Autonomy,
            _ => Self::User,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::IntentStatus;

    #[test]
    fn only_non_terminal_intents_can_advance() {
        assert!(IntentStatus::Proposed.can_transition_to(IntentStatus::Accepted));
        assert!(IntentStatus::Accepted.can_transition_to(IntentStatus::Running));
        assert!(IntentStatus::Accepted.can_transition_to(IntentStatus::Expired));
        assert!(IntentStatus::Running.can_transition_to(IntentStatus::Waiting));
        assert!(IntentStatus::Running.can_transition_to(IntentStatus::Accepted));
        assert!(IntentStatus::Waiting.can_transition_to(IntentStatus::Completed));
        assert!(IntentStatus::Waiting.can_transition_to(IntentStatus::Running));
        assert!(!IntentStatus::Proposed.can_transition_to(IntentStatus::Running));
        assert!(!IntentStatus::Completed.can_transition_to(IntentStatus::Running));
        assert!(IntentStatus::Failed.is_terminal());
        assert_eq!(
            IntentStatus::from_str(IntentStatus::Waiting.as_str()),
            Some(IntentStatus::Waiting)
        );
    }
}
