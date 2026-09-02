use std::{error::Error, fmt};

use super::{ConsciousnessAction, ConsciousnessDecision, SelfSnapshot};

#[derive(Debug, PartialEq, Eq)]
pub enum DecisionPolicyError {
    InvalidConfidence,
    EmptyReasonCode,
    PayloadMismatch,
    EmptyWorkProposal,
    DoNotDisturb,
}

impl fmt::Display for DecisionPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidConfidence => {
                "decision confidence must be finite and between zero and one"
            }
            Self::EmptyReasonCode => "decision reason_code must not be empty",
            Self::PayloadMismatch => "decision payload does not match its selected action",
            Self::EmptyWorkProposal => "work proposal fields must not be empty",
            Self::DoNotDisturb => "do-not-disturb blocks proactive speech and questions",
        })
    }
}

impl Error for DecisionPolicyError {}

/// Validate untrusted model output against deterministic runtime policy.
///
/// This is intentionally stricter than prompting: a malformed or disallowed
/// decision is rejected before any persistence, transcript, notification, or
/// Work hand-off occurs.
pub fn validate_decision(
    decision: &ConsciousnessDecision,
    snapshot: &SelfSnapshot,
) -> Result<(), DecisionPolicyError> {
    if !decision.confidence.is_finite() || !(0.0..=1.0).contains(&decision.confidence) {
        return Err(DecisionPolicyError::InvalidConfidence);
    }
    if decision.reason_code.trim().is_empty() {
        return Err(DecisionPolicyError::EmptyReasonCode);
    }

    let memory = nonempty(decision.memory.as_deref());
    let speech = nonempty(decision.speech.as_deref());
    let question = nonempty(decision.question.as_deref());
    let proposal = decision.work_proposal.as_ref();

    let payload_matches = match decision.action {
        ConsciousnessAction::Ignore => {
            memory.is_none() && speech.is_none() && question.is_none() && proposal.is_none()
        }
        ConsciousnessAction::Remember => {
            memory.is_some() && speech.is_none() && question.is_none() && proposal.is_none()
        }
        ConsciousnessAction::Speak => speech.is_some() && question.is_none() && proposal.is_none(),
        ConsciousnessAction::ProposeWork => {
            memory.is_none() && speech.is_none() && question.is_none() && proposal.is_some()
        }
        ConsciousnessAction::Ask => speech.is_none() && question.is_some() && proposal.is_none(),
    };
    if !payload_matches {
        return Err(DecisionPolicyError::PayloadMismatch);
    }

    if let Some(proposal) = proposal {
        if proposal.title.trim().is_empty()
            || proposal.instruction.trim().is_empty()
            || proposal.expected_outcome.trim().is_empty()
            || proposal.source_event_id.trim().is_empty()
        {
            return Err(DecisionPolicyError::EmptyWorkProposal);
        }
    }

    if snapshot.do_not_disturb
        && matches!(
            decision.action,
            ConsciousnessAction::Speak | ConsciousnessAction::Ask
        )
    {
        return Err(DecisionPolicyError::DoNotDisturb);
    }

    Ok(())
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::services::agent::{consciousness::WorkProposal, AgentInteractionMode};

    fn snapshot(do_not_disturb: bool) -> SelfSnapshot {
        SelfSnapshot {
            persona_name: "Arael".into(),
            addressee_user_id: 7,
            interaction_mode: AgentInteractionMode::Chat,
            mood: 70.0,
            activity: "idle".into(),
            do_not_disturb,
            has_active_work: false,
            granted_permissions: vec![],
            recent_intents: vec![],
            remembered: vec![],
            captured_at: Utc::now(),
            live: Default::default(),
            attention: None,
        }
    }

    fn decision(action: ConsciousnessAction) -> ConsciousnessDecision {
        ConsciousnessDecision {
            action,
            reason_code: "test".into(),
            confidence: 0.8,
            memory: None,
            speech: None,
            question: None,
            work_proposal: None,
        }
    }

    #[test]
    fn ignore_must_be_side_effect_free() {
        let mut value = decision(ConsciousnessAction::Ignore);
        value.speech = Some("hello".into());
        assert_eq!(
            validate_decision(&value, &snapshot(false)),
            Err(DecisionPolicyError::PayloadMismatch)
        );
    }

    #[test]
    fn do_not_disturb_blocks_speech_but_not_memory() {
        let mut speech = decision(ConsciousnessAction::Speak);
        speech.speech = Some("hello".into());
        assert_eq!(
            validate_decision(&speech, &snapshot(true)),
            Err(DecisionPolicyError::DoNotDisturb)
        );
        let mut memory = decision(ConsciousnessAction::Remember);
        memory.memory = Some("The user prefers quiet hours.".into());
        assert_eq!(validate_decision(&memory, &snapshot(true)), Ok(()));
    }

    #[test]
    fn live_presence_does_not_bypass_do_not_disturb() {
        let mut state = snapshot(true);
        state.live.speaking = true;
        state.live.speech_interruptible = true;
        state.granted_permissions = vec!["agent.execute".into()];
        let mut speech = decision(ConsciousnessAction::Speak);
        speech.speech = Some("hello".into());
        assert_eq!(
            validate_decision(&speech, &state),
            Err(DecisionPolicyError::DoNotDisturb)
        );
    }

    #[test]
    fn work_proposal_carries_no_execution_authority() {
        let mut value = decision(ConsciousnessAction::ProposeWork);
        value.work_proposal = Some(WorkProposal {
            title: "Review the new event".into(),
            instruction: "Review the event and prepare a summary.".into(),
            expected_outcome: "A reviewable summary.".into(),
            source_event_id: "event-1".into(),
        });
        assert_eq!(validate_decision(&value, &snapshot(false)), Ok(()));
        value.memory = Some("the user likes quiet hours".into());
        assert_eq!(
            validate_decision(&value, &snapshot(false)),
            Err(DecisionPolicyError::PayloadMismatch)
        );
    }

    #[test]
    fn speak_and_ask_may_carry_optional_persona_memory() {
        let mut speak = decision(ConsciousnessAction::Speak);
        speak.speech = Some("晚上再聊。".into());
        assert_eq!(validate_decision(&speak, &snapshot(false)), Ok(()));
        speak.memory = Some("晚上想打独立游戏".into());
        assert_eq!(validate_decision(&speak, &snapshot(false)), Ok(()));
        speak.question = Some("现在方便吗？".into());
        assert_eq!(
            validate_decision(&speak, &snapshot(false)),
            Err(DecisionPolicyError::PayloadMismatch)
        );

        let mut ask = decision(ConsciousnessAction::Ask);
        ask.question = Some("今晚还打吗？".into());
        assert_eq!(validate_decision(&ask, &snapshot(false)), Ok(()));
        ask.memory = Some("晚上想打独立游戏".into());
        assert_eq!(validate_decision(&ask, &snapshot(false)), Ok(()));

        let mut ignore = decision(ConsciousnessAction::Ignore);
        ignore.memory = Some("晚上想打独立游戏".into());
        assert_eq!(
            validate_decision(&ignore, &snapshot(false)),
            Err(DecisionPolicyError::PayloadMismatch)
        );
    }
}
