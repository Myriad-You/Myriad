//! The site's side of the grant rules in `myriad_agent_rules::grants`: who
//! is a person here (an account id above the heartbeat's), what an empty
//! request covers (`max_user_agent_permissions`), and the messages.

use myriad_agent_rules::grants::{self, Actor, AutonomyGrant, Check, Verdict, WriteRefusal};
use serde::Serialize;

use crate::services::agent::SYSTEM_USER_ID;

/// Durable personal autonomy grant as seen by the decision check.
///
/// `allowed_permissions` is a claimed subset only. Action time must still
/// intersect it with the current runtime grant set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomyGrantView {
    pub user_id: i32,
    pub allowed_permissions: Vec<String>,
    pub revoked: bool,
}

/// Outcome of a personal-autonomy check. Consciousness never receives an
/// execute variant: even `AllowPersonalWork` only means Work may proceed as
/// this user after the existing Planner / confirmation path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutonomyVerdict {
    /// Missing, revoked, heartbeat, or empty after re-filter. The proposal
    /// card stays; Work must not start on its own.
    RequireUserReview,
    /// Bound to the addressee. Permissions listed here are the intersection
    /// of the grant and current granted permissions — never declared or
    /// approved permissions that the runtime filter dropped.
    AllowPersonalWork { granted_permissions: Vec<String> },
}

/// Check a consciousness-originated Work request.
///
/// Heartbeat `SYSTEM_USER_ID` is never a personal grant. A missing or revoked
/// grant cannot skip user review. A live grant is re-filtered against current
/// granted permissions at this call — not against declared or approved sets.
pub fn evaluate_autonomy_grant(
    actor_user_id: i32,
    grant: Option<&AutonomyGrantView>,
    current_granted_permissions: &[String],
) -> AutonomyVerdict {
    match grants::evaluate(
        &actor(actor_user_id),
        rule_grant(grant).as_ref(),
        current_granted_permissions,
    ) {
        Verdict::RequireUserReview => AutonomyVerdict::RequireUserReview,
        Verdict::AllowPersonalWork { granted } => AutonomyVerdict::AllowPersonalWork {
            granted_permissions: granted,
        },
    }
}

pub fn skips_user_review(verdict: &AutonomyVerdict) -> bool {
    matches!(verdict, AutonomyVerdict::AllowPersonalWork { .. })
}

/// Action-time check used by `validate_intention_work_request`.
///
/// Heartbeat is blocked for every source. `RequireUserReview` (revoked, dropped
/// permissions, missing grant) blocks Autonomy entry only; User-accepted may
/// proceed with missing/revoked grant (proposal card already used).
pub fn intention_may_enter_work(
    actor_user_id: i32,
    grant: Option<&AutonomyGrantView>,
    current_granted_permissions: &[String],
    accept_source: super::AcceptSource,
) -> bool {
    grants::may_enter_work(
        &actor(actor_user_id),
        rule_grant(grant).as_ref(),
        current_granted_permissions,
        source(accept_source),
    )
}

/// Runtime granted names intersected with an optional autonomy cap.
pub fn effective_granted_permissions(
    current_granted_permissions: &[String],
    autonomy_permission_cap: Option<&[String]>,
) -> Vec<String> {
    grants::effective(current_granted_permissions, autonomy_permission_cap)
}

/// Execute-time permission check. Autonomy-capped Work re-reads the live grant
/// and current granted permissions; a revoke or empty intersection fails closed.
/// User-accepted Work (`autonomy_permission_cap` is `None`) only checks current
/// granted permissions against the capability.
pub fn autonomy_execute_permission_error(
    user_id: i32,
    grant: Option<&AutonomyGrantView>,
    current_granted_permissions: &[String],
    autonomy_permission_cap: Option<&[String]>,
    capability_id: &str,
    required_permissions: &[String],
) -> Option<String> {
    match grants::check(
        &actor(user_id),
        rule_grant(grant).as_ref(),
        current_granted_permissions,
        autonomy_permission_cap,
        required_permissions,
    ) {
        Check::Allowed => None,
        Check::AutonomyWithdrawn => Some("Personal autonomy is no longer granted".into()),
        Check::Missing { missing } => missing
            .first()
            .map(|perm| format!("权限不足：执行 '{capability_id}' 需要 '{perm}' 权限")),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutonomyGrantWriteError {
    HeartbeatIdentity,
    EmptyAfterFilter,
}

impl AutonomyGrantWriteError {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HeartbeatIdentity => "Personal autonomy cannot run as the heartbeat identity",
            Self::EmptyAfterFilter => {
                "Personal autonomy requires at least one currently granted permission"
            }
        }
    }
}

impl From<WriteRefusal> for AutonomyGrantWriteError {
    fn from(refusal: WriteRefusal) -> Self {
        match refusal {
            WriteRefusal::HeartbeatIdentity => Self::HeartbeatIdentity,
            WriteRefusal::EmptyAfterFilter => Self::EmptyAfterFilter,
        }
    }
}

/// Build a persistable grant for the addressee. Heartbeat identity cannot hold
/// one. Requested names are intersected with current granted permissions.
///
/// An empty request is the panel toggle: current granted permissions
/// intersected with the non-admin candidate set. Admin-only names such as
/// `system:admin` stay out unless the caller lists them explicitly.
pub fn prepare_personal_grant(
    user_id: i32,
    requested_permissions: &[String],
    current_granted_permissions: &[String],
) -> Result<AutonomyGrantView, AutonomyGrantWriteError> {
    let candidates: Vec<String> = crate::services::agent::max_user_agent_permissions()
        .into_iter()
        .collect();
    let grant = grants::prepare(
        &actor(user_id),
        requested_permissions,
        current_granted_permissions,
        &candidates,
    )?;
    Ok(view(grant))
}

/// Mark a stored grant revoked. Heartbeat identity cannot hold the result.
pub fn revoke_personal_grant(
    grant: &AutonomyGrantView,
) -> Result<AutonomyGrantView, AutonomyGrantWriteError> {
    let revoked = grants::revoke(&rule_grant_of(grant), is_personal_addressee(grant.user_id))?;
    Ok(view(revoked))
}

pub(super) fn actor(user_id: i32) -> Actor<i32> {
    Actor {
        id: user_id,
        personal: is_personal_addressee(user_id),
    }
}

pub(super) fn rule_grant(grant: Option<&AutonomyGrantView>) -> Option<AutonomyGrant<i32>> {
    grant.map(rule_grant_of)
}

fn rule_grant_of(grant: &AutonomyGrantView) -> AutonomyGrant<i32> {
    AutonomyGrant {
        actor: grant.user_id,
        allowed: grant.allowed_permissions.clone(),
        revoked: grant.revoked,
    }
}

fn view(grant: AutonomyGrant<i32>) -> AutonomyGrantView {
    AutonomyGrantView {
        user_id: grant.actor,
        allowed_permissions: grant.allowed,
        revoked: grant.revoked,
    }
}

pub(super) fn source(accept_source: super::AcceptSource) -> grants::Source {
    match accept_source {
        super::AcceptSource::User => grants::Source::User,
        super::AcceptSource::Autonomy => grants::Source::Autonomy,
    }
}

fn is_personal_addressee(user_id: i32) -> bool {
    user_id != SYSTEM_USER_ID && user_id > 0
}

#[cfg(test)]
mod tests {
    // The rules themselves are tested in `myriad_agent_rules::grants`; these
    // cover what the site adds: who is a person, the candidates, the words.
    use super::*;

    fn grant(user_id: i32, permissions: &[&str], revoked: bool) -> AutonomyGrantView {
        AutonomyGrantView {
            user_id,
            allowed_permissions: permissions
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            revoked,
        }
    }

    #[test]
    fn the_heartbeat_and_non_positive_ids_are_not_people() {
        let current = vec!["calendar:read".to_string()];
        for id in [SYSTEM_USER_ID, -3] {
            assert_eq!(
                evaluate_autonomy_grant(id, Some(&grant(id, &["calendar:read"], false)), &current),
                AutonomyVerdict::RequireUserReview
            );
            assert_eq!(
                prepare_personal_grant(id, &current, &current),
                Err(AutonomyGrantWriteError::HeartbeatIdentity)
            );
            assert_eq!(
                revoke_personal_grant(&grant(id, &["calendar:read"], false)),
                Err(AutonomyGrantWriteError::HeartbeatIdentity)
            );
        }
        assert_eq!(
            evaluate_autonomy_grant(7, Some(&grant(7, &["calendar:read"], false)), &current),
            AutonomyVerdict::AllowPersonalWork {
                granted_permissions: current.clone(),
            }
        );
    }

    #[test]
    fn an_empty_request_drops_admin_only_permissions() {
        let admin_current = vec![
            "system:admin".to_string(),
            "phantasi:admin".to_string(),
            "http:fetch".to_string(),
            "scheduler:write".to_string(),
        ];
        let prepared = prepare_personal_grant(7, &[], &admin_current).unwrap();
        assert_eq!(
            prepared.allowed_permissions,
            vec!["http:fetch".to_string(), "scheduler:write".to_string()]
        );
        assert_eq!(
            prepare_personal_grant(7, &["system:admin".into()], &admin_current)
                .unwrap()
                .allowed_permissions,
            vec!["system:admin".to_string()]
        );
        assert_eq!(
            prepare_personal_grant(7, &["mail:send".into()], &admin_current),
            Err(AutonomyGrantWriteError::EmptyAfterFilter)
        );
    }

    #[test]
    fn execute_errors_keep_their_words() {
        let live = grant(7, &["calendar:read"], false);
        let cap = vec!["calendar:read".to_string()];
        let current = vec!["calendar:read".to_string(), "mail:send".to_string()];
        assert_eq!(
            autonomy_execute_permission_error(
                7,
                Some(&grant(7, &["calendar:read"], true)),
                &current,
                Some(&cap),
                "calendar.read",
                &["calendar:read".into()],
            )
            .as_deref(),
            Some("Personal autonomy is no longer granted")
        );
        assert_eq!(
            autonomy_execute_permission_error(
                7,
                Some(&live),
                &current,
                Some(&cap),
                "mail.send",
                &["mail:send".into(), "http:fetch".into()],
            )
            .as_deref(),
            Some("权限不足：执行 'mail.send' 需要 'mail:send' 权限")
        );
        assert_eq!(
            autonomy_execute_permission_error(7, None, &current, None, "x", &[]),
            None
        );
    }
}
