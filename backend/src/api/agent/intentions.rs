//! User review endpoints for consciousness Work proposals.

use super::*;
use crate::error::HttpError;
use crate::services::agent::consciousness::{
    AcceptSource, AutonomyGrantStore, AutonomyGrantWriteError, IntentStatus, IntentStore,
    prepare_personal_grant, revoke_personal_grant,
};
use myriad_error::AppError;

fn grant_write_error(error: AutonomyGrantWriteError) -> HttpError {
    let status = match error {
        AutonomyGrantWriteError::HeartbeatIdentity => StatusCode::FORBIDDEN,
        AutonomyGrantWriteError::EmptyAfterFilter => StatusCode::BAD_REQUEST,
    };
    HttpError::from((status, Json(AppError::public_json(error.as_str()))))
}

/// GET /api/agent/autonomy
pub async fn get_autonomy_grant(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    let grant = AutonomyGrantStore::new(db)
        .find(user_id)
        .await
        .map_err(intention_error)?;
    Ok(Json(json!({ "grant": grant })))
}

/// PUT /api/agent/autonomy
pub async fn put_autonomy_grant(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<AutonomyGrantBody>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    let granted: Vec<String> = crate::services::agent::get_user_permissions(&db, user_id)
        .await
        .into_iter()
        .collect();
    let prepared = prepare_personal_grant(user_id, &body.allowed_permissions, &granted)
        .map_err(grant_write_error)?;
    let grant = AutonomyGrantStore::new(db)
        .upsert(&prepared)
        .await
        .map_err(intention_error)?;
    Ok(Json(json!({ "grant": grant })))
}

/// DELETE /api/agent/autonomy
pub async fn delete_autonomy_grant(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    let store = AutonomyGrantStore::new(db);
    let current = store.find(user_id).await.map_err(intention_error)?;
    let Some(current) = current else {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Personal autonomy grant not found")),
        )));
    };
    let revoked = revoke_personal_grant(&current).map_err(grant_write_error)?;
    let grant = store.upsert(&revoked).await.map_err(intention_error)?;
    Ok(Json(json!({ "grant": grant })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomyGrantBody {
    #[serde(default)]
    pub allowed_permissions: Vec<String>,
}

/// GET /api/agent/intentions
pub async fn list_intentions(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    let intentions = IntentStore::new(db)
        .actionable(user_id, 20)
        .await
        .map_err(intention_error)?;
    Ok(Json(json!({ "intentions": intentions })))
}

/// POST /api/agent/intentions/:id/accept
///
/// Acceptance still does not execute here. The returned Work input must be sent
/// through `/process/stream` with `mode=work`, preserving Planner, current
/// granted-permission filtering, and normal confirmations.
pub async fn accept_intention(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(intent_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    let store = IntentStore::new(db);
    store.expire_stale(user_id).await.map_err(intention_error)?;
    let accepted = store
        .mark_accepted(&intent_id, user_id, AcceptSource::User)
        .await
        .map_err(intention_error)?;
    let work_input = accepted.proposal.instruction.clone();
    Ok(Json(json!({
        "intention": accepted,
        "work": {
            "mode": "work",
            "input": work_input,
        }
    })))
}

/// POST /api/agent/intentions/:id/dismiss
pub async fn dismiss_intention(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(intent_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    let dismissed = IntentStore::new(db)
        .transition(
            &intent_id,
            user_id,
            IntentStatus::Abandoned,
            None,
            None,
            None,
        )
        .await
        .map_err(intention_error)?;
    Ok(Json(json!({ "intention": dismissed })))
}

#[cfg(test)]
mod work_grant_tests {
    use super::validate_intention_work_grant;
    use crate::services::agent::SYSTEM_USER_ID;
    use crate::services::agent::consciousness::{AcceptSource, AutonomyGrantView};

    fn grant(revoked: bool, allowed: &[&str]) -> AutonomyGrantView {
        AutonomyGrantView {
            user_id: 7,
            allowed_permissions: allowed.iter().map(|value| (*value).to_string()).collect(),
            revoked,
        }
    }

    #[test]
    fn shipped_grant_then_revoke_refuses_work_entry() {
        use crate::services::agent::consciousness::{
            prepare_personal_grant, revoke_personal_grant,
        };

        let current = vec!["calendar:read".to_string()];
        let live = prepare_personal_grant(7, &["calendar:read".into()], &current).unwrap();
        assert!(
            validate_intention_work_grant(7, Some(&live), &current, AcceptSource::Autonomy,)
                .is_ok()
        );

        let revoked = revoke_personal_grant(&live).unwrap();
        assert!(
            validate_intention_work_grant(7, Some(&revoked), &current, AcceptSource::Autonomy,)
                .is_err()
        );
        assert!(
            validate_intention_work_grant(7, Some(&live), &[], AcceptSource::Autonomy).is_err()
        );
        assert!(
            validate_intention_work_grant(7, Some(&revoked), &current, AcceptSource::User,).is_ok()
        );
        assert!(
            validate_intention_work_grant(
                SYSTEM_USER_ID,
                Some(&live),
                &current,
                AcceptSource::User,
            )
            .is_err()
        );
        assert!(prepare_personal_grant(SYSTEM_USER_ID, &current, &current).is_err());
    }

    #[test]
    fn require_user_review_cannot_enter_work_after_revoke_or_permission_drop() {
        let revoked = grant(true, &["calendar:read"]);
        assert!(
            validate_intention_work_grant(
                7,
                Some(&revoked),
                &["calendar:read".into()],
                AcceptSource::Autonomy,
            )
            .is_err()
        );
        let live = grant(false, &["calendar:read"]);
        assert!(
            validate_intention_work_grant(7, Some(&live), &[], AcceptSource::Autonomy).is_err()
        );
        assert!(
            validate_intention_work_grant(SYSTEM_USER_ID, None, &[], AcceptSource::User).is_err()
        );
        assert!(validate_intention_work_grant(7, None, &[], AcceptSource::User).is_ok());
        assert!(
            validate_intention_work_grant(
                7,
                Some(&live),
                &["calendar:read".into()],
                AcceptSource::Autonomy,
            )
            .is_ok()
        );
    }
}
