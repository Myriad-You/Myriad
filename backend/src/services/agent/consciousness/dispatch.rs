//! Build the Work request an autonomy-accepted proposal must enter.
//!
//! Consciousness still does not execute. The API tick claims a row and then
//! calls `Agent::process_with_progress` as the addressee.

use chrono::Utc;

use crate::services::agent::{AgentInteractionMode, RequestContext, UserRequest, SYSTEM_USER_ID};

use super::{AcceptSource, AutonomyGrantView};

/// Whether this tick may CAS-claim an Accepted intention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutonomyClaim {
    /// Leave Accepted. User click can still start Work.
    Skip,
    Claim {
        cap: Vec<String>,
    },
}

/// Revoked grants, heartbeat identity, user-accepted rows, and empty
/// re-filter stay Accepted so a later user click can still start Work.
pub fn autonomy_claim_decision(
    user_id: i32,
    accept_source: AcceptSource,
    grant: Option<&AutonomyGrantView>,
    current_granted: &[String],
) -> AutonomyClaim {
    if accept_source != AcceptSource::Autonomy {
        return AutonomyClaim::Skip;
    }
    match autonomy_cap_from_grant(user_id, grant, current_granted) {
        Some(cap) if user_id != SYSTEM_USER_ID && user_id > 0 && !cap.is_empty() => {
            AutonomyClaim::Claim { cap }
        }
        _ => AutonomyClaim::Skip,
    }
}

/// Assemble the Work turn for an autonomy-accepted proposal.
///
/// Returns `None` when the actor is the heartbeat identity — that path is
/// forbidden even if a grant row exists.
pub fn build_autonomy_work_request(
    user_id: i32,
    instruction: &str,
    intent_id: &str,
    session_id: String,
    cap: Vec<String>,
) -> Option<UserRequest> {
    if user_id == SYSTEM_USER_ID || user_id <= 0 || cap.is_empty() {
        return None;
    }
    Some(UserRequest {
        raw_input: instruction.to_string(),
        timestamp: Utc::now(),
        user_id,
        context: Some(RequestContext {
            interaction_mode: AgentInteractionMode::Work,
            session_id: Some(session_id),
            source_intent_id: Some(intent_id.to_string()),
            autonomy_permission_cap: Some(cap),
            ..Default::default()
        }),
    })
}

pub fn autonomy_cap_from_grant(
    user_id: i32,
    grant: Option<&AutonomyGrantView>,
    current_granted: &[String],
) -> Option<Vec<String>> {
    match super::evaluate_autonomy_grant(user_id, grant, current_granted) {
        super::AutonomyVerdict::AllowPersonalWork {
            granted_permissions,
        } if !granted_permissions.is_empty() => Some(granted_permissions),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent::consciousness::AutonomyGrantView;

    #[test]
    fn autonomy_work_request_is_never_heartbeat() {
        assert!(build_autonomy_work_request(
            SYSTEM_USER_ID,
            "do it",
            "int_1",
            "ses_1".into(),
            vec!["calendar:read".into()],
        )
        .is_none());
        let request = build_autonomy_work_request(
            7,
            "整理报告",
            "int_1",
            "ses_1".into(),
            vec!["calendar:read".into()],
        )
        .expect("addressee work request");
        assert_eq!(request.user_id, 7);
        assert_ne!(request.user_id, SYSTEM_USER_ID);
        let ctx = request.context.unwrap();
        assert_eq!(ctx.interaction_mode, AgentInteractionMode::Work);
        assert_eq!(ctx.source_intent_id.as_deref(), Some("int_1"));
        assert_eq!(
            ctx.autonomy_permission_cap.as_deref(),
            Some(["calendar:read".to_string()].as_slice())
        );
        assert_eq!(request.raw_input, "整理报告");
    }

    #[test]
    fn cap_from_grant_refilters() {
        let grant = AutonomyGrantView {
            user_id: 7,
            allowed_permissions: vec!["calendar:read".into(), "mcp:execute".into()],
            revoked: false,
        };
        let cap = autonomy_cap_from_grant(7, Some(&grant), &["calendar:read".into()]).unwrap();
        assert_eq!(cap, vec!["calendar:read".to_string()]);
        assert!(autonomy_cap_from_grant(7, Some(&grant), &[]).is_none());
    }

    #[test]
    fn claim_is_skipped_after_revoke_or_user_accept() {
        let live = AutonomyGrantView {
            user_id: 7,
            allowed_permissions: vec!["calendar:read".into()],
            revoked: false,
        };
        let revoked = AutonomyGrantView {
            user_id: 7,
            allowed_permissions: vec!["calendar:read".into()],
            revoked: true,
        };
        let granted = vec!["calendar:read".to_string()];
        assert_eq!(
            autonomy_claim_decision(7, AcceptSource::Autonomy, Some(&live), &granted),
            AutonomyClaim::Claim {
                cap: vec!["calendar:read".into()]
            }
        );
        assert_eq!(
            autonomy_claim_decision(7, AcceptSource::Autonomy, Some(&revoked), &granted),
            AutonomyClaim::Skip
        );
        assert_eq!(
            autonomy_claim_decision(7, AcceptSource::Autonomy, Some(&live), &[]),
            AutonomyClaim::Skip
        );
        assert_eq!(
            autonomy_claim_decision(7, AcceptSource::User, Some(&live), &granted),
            AutonomyClaim::Skip
        );
        assert_eq!(
            autonomy_claim_decision(
                SYSTEM_USER_ID,
                AcceptSource::Autonomy,
                Some(&AutonomyGrantView {
                    user_id: SYSTEM_USER_ID,
                    allowed_permissions: granted.clone(),
                    revoked: false,
                }),
                &granted,
            ),
            AutonomyClaim::Skip
        );
    }
}
