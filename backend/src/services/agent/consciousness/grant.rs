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
    if !is_personal_addressee(actor_user_id) {
        return AutonomyVerdict::RequireUserReview;
    }
    let Some(grant) = grant else {
        return AutonomyVerdict::RequireUserReview;
    };
    if grant.revoked || grant.user_id != actor_user_id || !is_personal_addressee(grant.user_id) {
        return AutonomyVerdict::RequireUserReview;
    }

    let granted_permissions: Vec<String> = grant
        .allowed_permissions
        .iter()
        .filter(|permission| {
            current_granted_permissions
                .iter()
                .any(|granted| granted == *permission)
        })
        .cloned()
        .collect();
    if granted_permissions.is_empty() {
        return AutonomyVerdict::RequireUserReview;
    }
    AutonomyVerdict::AllowPersonalWork {
        granted_permissions,
    }
}

pub fn skips_user_review(verdict: &AutonomyVerdict) -> bool {
    matches!(verdict, AutonomyVerdict::AllowPersonalWork { .. })
}

/// Action-time check used by `validate_intention_work_request`.
///
/// A live grant must still `AllowPersonalWork` after re-filtering current
/// granted permissions. `RequireUserReview` (revoked, dropped permissions,
/// heartbeat) must not enter Work. A user-accepted proposal with no grant
/// row may proceed — that path already went through the proposal card.
pub fn intention_may_enter_work(
    actor_user_id: i32,
    grant: Option<&AutonomyGrantView>,
    current_granted_permissions: &[String],
    accept_source: super::AcceptSource,
) -> bool {
    if !is_personal_addressee(actor_user_id) {
        return false;
    }
    match accept_source {
        super::AcceptSource::User => true,
        super::AcceptSource::Autonomy => matches!(
            evaluate_autonomy_grant(actor_user_id, grant, current_granted_permissions),
            AutonomyVerdict::AllowPersonalWork { .. }
        ),
    }
}

/// Autonomy-capped execute: the grant row must still `AllowPersonalWork`
/// after re-filtering current granted permissions, and the entry-time cap
/// remains an extra ceiling.
pub fn autonomy_cap_still_allows(
    user_id: i32,
    grant: Option<&AutonomyGrantView>,
    current_granted_permissions: &[String],
    autonomy_permission_cap: Option<&[String]>,
) -> bool {
    let Some(cap) = autonomy_permission_cap else {
        return true;
    };
    matches!(
        evaluate_autonomy_grant(user_id, grant, current_granted_permissions),
        AutonomyVerdict::AllowPersonalWork { .. }
    ) && !effective_granted_permissions(current_granted_permissions, Some(cap)).is_empty()
}

/// Runtime granted names intersected with an optional autonomy cap.
pub fn effective_granted_permissions(
    current_granted_permissions: &[String],
    autonomy_permission_cap: Option<&[String]>,
) -> Vec<String> {
    let Some(cap) = autonomy_permission_cap else {
        return current_granted_permissions.to_vec();
    };
    current_granted_permissions
        .iter()
        .filter(|permission| cap.iter().any(|allowed| allowed == *permission))
        .cloned()
        .collect()
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
    if autonomy_permission_cap.is_some()
        && !autonomy_cap_still_allows(
            user_id,
            grant,
            current_granted_permissions,
            autonomy_permission_cap,
        )
    {
        return Some("Personal autonomy is no longer granted".into());
    }
    if required_permissions.is_empty() {
        return None;
    }
    let effective =
        effective_granted_permissions(current_granted_permissions, autonomy_permission_cap);
    required_permissions.iter().find_map(|perm| {
        if effective.iter().any(|granted| granted == perm) {
            None
        } else {
            Some(format!(
                "权限不足：执行 '{capability_id}' 需要 '{perm}' 权限"
            ))
        }
    })
}

/// True when every required permission is inside the autonomy ceiling.
///
/// `None` means this turn is not autonomy-capped (user-accepted Work).
/// Empty `required` is always allowed; execute time still re-reads current
/// granted permissions.
pub fn required_permissions_within_cap(
    required_permissions: &[String],
    autonomy_permission_cap: Option<&[String]>,
) -> bool {
    let Some(cap) = autonomy_permission_cap else {
        return true;
    };
    required_permissions
        .iter()
        .all(|permission| cap.iter().any(|allowed| allowed == permission))
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

/// Build a persistable grant for the addressee. Heartbeat identity cannot hold
/// one. Requested names are intersected with current granted permissions.
pub fn prepare_personal_grant(
    user_id: i32,
    requested_permissions: &[String],
    current_granted_permissions: &[String],
) -> Result<AutonomyGrantView, AutonomyGrantWriteError> {
    if !is_personal_addressee(user_id) {
        return Err(AutonomyGrantWriteError::HeartbeatIdentity);
    }
    let requested = if requested_permissions.is_empty() {
        current_granted_permissions
    } else {
        requested_permissions
    };
    let allowed_permissions: Vec<String> = requested
        .iter()
        .filter(|permission| {
            current_granted_permissions
                .iter()
                .any(|granted| granted == *permission)
        })
        .cloned()
        .collect();
    if allowed_permissions.is_empty() {
        return Err(AutonomyGrantWriteError::EmptyAfterFilter);
    }
    Ok(AutonomyGrantView {
        user_id,
        allowed_permissions,
        revoked: false,
    })
}

/// Mark a stored grant revoked. Heartbeat identity cannot hold the result.
pub fn revoke_personal_grant(
    grant: &AutonomyGrantView,
) -> Result<AutonomyGrantView, AutonomyGrantWriteError> {
    if !is_personal_addressee(grant.user_id) {
        return Err(AutonomyGrantWriteError::HeartbeatIdentity);
    }
    Ok(AutonomyGrantView {
        user_id: grant.user_id,
        allowed_permissions: grant.allowed_permissions.clone(),
        revoked: true,
    })
}

fn is_personal_addressee(user_id: i32) -> bool {
    user_id != SYSTEM_USER_ID && user_id > 0
}

#[cfg(test)]
mod tests {
    use super::super::AcceptSource;
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
    fn heartbeat_system_user_is_never_a_personal_grant() {
        let heartbeat = grant(SYSTEM_USER_ID, &["calendar:read"], false);
        assert_eq!(
            evaluate_autonomy_grant(SYSTEM_USER_ID, Some(&heartbeat), &["calendar:read".into()]),
            AutonomyVerdict::RequireUserReview
        );
        assert!(!skips_user_review(&evaluate_autonomy_grant(
            SYSTEM_USER_ID,
            Some(&heartbeat),
            &["calendar:read".into()],
        )));
        assert_eq!(
            evaluate_autonomy_grant(7, Some(&heartbeat), &["calendar:read".into()]),
            AutonomyVerdict::RequireUserReview
        );
    }

    #[test]
    fn missing_or_revoked_grant_cannot_skip_user_review() {
        assert_eq!(
            evaluate_autonomy_grant(7, None, &["calendar:read".into()]),
            AutonomyVerdict::RequireUserReview
        );
        let revoked = grant(7, &["calendar:read"], true);
        assert_eq!(
            evaluate_autonomy_grant(7, Some(&revoked), &["calendar:read".into()]),
            AutonomyVerdict::RequireUserReview
        );
        assert!(!skips_user_review(&evaluate_autonomy_grant(
            7,
            Some(&revoked),
            &["calendar:read".into()],
        )));
    }

    #[test]
    fn valid_grant_is_refiltered_by_current_granted_permissions() {
        let stored = grant(7, &["calendar:read", "mcp:execute", "mail:send"], false);
        // mcp:execute is on the grant row but not currently granted — drop it.
        let verdict = evaluate_autonomy_grant(
            7,
            Some(&stored),
            &["calendar:read".into(), "mail:send".into()],
        );
        assert_eq!(
            verdict,
            AutonomyVerdict::AllowPersonalWork {
                granted_permissions: vec!["calendar:read".into(), "mail:send".into()],
            }
        );
        assert!(skips_user_review(&verdict));

        let declared_only = evaluate_autonomy_grant(
            7,
            Some(&stored),
            &[], // runtime grant filter produced nothing
        );
        assert_eq!(declared_only, AutonomyVerdict::RequireUserReview);
        assert!(!skips_user_review(&declared_only));
    }

    #[test]
    fn require_user_review_after_revoke_or_permission_drop_cannot_enter_work() {
        let revoked = grant(7, &["calendar:read"], true);
        assert!(!intention_may_enter_work(
            7,
            Some(&revoked),
            &["calendar:read".into()],
            AcceptSource::Autonomy,
        ));
        let live = grant(7, &["calendar:read", "mcp:execute"], false);
        assert!(!intention_may_enter_work(
            7,
            Some(&live),
            &[],
            AcceptSource::Autonomy,
        ));
        assert!(!intention_may_enter_work(
            SYSTEM_USER_ID,
            Some(&grant(SYSTEM_USER_ID, &["calendar:read"], false)),
            &["calendar:read".into()],
            AcceptSource::User,
        ));
        assert!(intention_may_enter_work(
            7,
            None,
            &["calendar:read".into()],
            AcceptSource::User,
        ));
        assert!(intention_may_enter_work(
            7,
            Some(&live),
            &["calendar:read".into()],
            AcceptSource::Autonomy,
        ));
        assert!(intention_may_enter_work(
            7,
            Some(&revoked),
            &["calendar:read".into()],
            AcceptSource::User,
        ));
    }

    #[test]
    fn grant_then_revoke_and_permission_drop_go_through_work_entry_check() {
        let current = vec!["calendar:read".to_string(), "mail:send".to_string()];
        let live = prepare_personal_grant(7, &["calendar:read".into()], &current).unwrap();
        assert!(!live.revoked);
        assert_eq!(live.allowed_permissions, vec!["calendar:read".to_string()]);
        assert!(intention_may_enter_work(
            7,
            Some(&live),
            &current,
            AcceptSource::Autonomy,
        ));

        let revoked = revoke_personal_grant(&live).unwrap();
        assert!(revoked.revoked);
        assert!(!intention_may_enter_work(
            7,
            Some(&revoked),
            &current,
            AcceptSource::Autonomy,
        ));
        assert!(!intention_may_enter_work(
            7,
            Some(&live),
            &[],
            AcceptSource::Autonomy,
        ));

        assert_eq!(
            prepare_personal_grant(SYSTEM_USER_ID, &["calendar:read".into()], &current),
            Err(AutonomyGrantWriteError::HeartbeatIdentity)
        );
        assert_eq!(
            revoke_personal_grant(&grant(SYSTEM_USER_ID, &["calendar:read"], false)),
            Err(AutonomyGrantWriteError::HeartbeatIdentity)
        );
    }

    #[test]
    fn effective_granted_permissions_intersect_the_autonomy_cap() {
        let granted = vec!["calendar:read".into(), "mail:send".into()];
        let cap = vec!["calendar:read".into()];
        assert_eq!(
            effective_granted_permissions(&granted, Some(&cap)),
            vec!["calendar:read".to_string()]
        );
        assert!(!effective_granted_permissions(&granted, Some(&cap))
            .iter()
            .any(|p| p == "mail:send"));
        assert_eq!(effective_granted_permissions(&granted, None), granted);
        assert!(required_permissions_within_cap(
            &["calendar:read".into()],
            Some(&cap)
        ));
        assert!(!required_permissions_within_cap(
            &["mail:send".into()],
            Some(&cap)
        ));
        assert!(required_permissions_within_cap(&["mail:send".into()], None));
        assert!(required_permissions_within_cap(&[], Some(&cap)));
    }

    #[test]
    fn autonomy_cap_still_allows_fails_after_revoke_or_permission_drop() {
        let live = grant(7, &["calendar:read"], false);
        let current = vec!["calendar:read".to_string(), "mail:send".to_string()];
        let cap = vec!["calendar:read".to_string()];
        assert!(autonomy_cap_still_allows(
            7,
            Some(&live),
            &current,
            Some(&cap)
        ));
        assert!(autonomy_cap_still_allows(7, Some(&live), &current, None));
        let revoked = grant(7, &["calendar:read"], true);
        assert!(!autonomy_cap_still_allows(
            7,
            Some(&revoked),
            &current,
            Some(&cap)
        ));
        assert!(!autonomy_cap_still_allows(7, Some(&live), &[], Some(&cap)));
        assert!(!autonomy_cap_still_allows(
            7,
            Some(&live),
            &["mail:send".into()],
            Some(&cap)
        ));
    }

    #[test]
    fn verdict_never_means_consciousness_may_execute() {
        let stored = grant(7, &["calendar:read"], false);
        let verdict = evaluate_autonomy_grant(7, Some(&stored), &["calendar:read".into()]);
        match verdict {
            AutonomyVerdict::RequireUserReview | AutonomyVerdict::AllowPersonalWork { .. } => {}
        }
    }

    #[test]
    fn execute_time_revoke_or_permission_drop_fails_closed() {
        let live = grant(7, &["calendar:read"], false);
        let cap = vec!["calendar:read".to_string()];
        let current = vec!["calendar:read".to_string(), "mail:send".to_string()];
        assert_eq!(
            autonomy_execute_permission_error(
                7,
                Some(&live),
                &current,
                Some(&cap),
                "calendar.read",
                &["calendar:read".into()],
            ),
            None
        );

        let revoked = grant(7, &["calendar:read"], true);
        assert_eq!(
            autonomy_execute_permission_error(
                7,
                Some(&revoked),
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
                &["mail:send".into()],
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
                &["mail:send".into()],
            )
            .as_deref(),
            Some("权限不足：执行 'mail.send' 需要 'mail:send' 权限")
        );

        assert_eq!(
            autonomy_execute_permission_error(
                7,
                None,
                &current,
                None,
                "mail.send",
                &["mail:send".into()],
            ),
            None
        );
        assert_eq!(
            autonomy_execute_permission_error(
                7,
                None,
                &["calendar:read".into()],
                None,
                "mail.send",
                &["mail:send".into()],
            )
            .as_deref(),
            Some("权限不足：执行 'mail.send' 需要 'mail:send' 权限")
        );
    }
}
