//! A run that is the work on one of her own proposals (an intention): whether
//! it may start, marking it begun, how far it may act on its own, and moving
//! it along as the run goes.

use super::*;

pub(crate) async fn validate_intention_work_request(
    db: &DatabaseConnection,
    intent_id: Option<&str>,
    user_id: i32,
    input: &str,
) -> Result<(), AppError> {
    let Some(intent_id) = intent_id else {
        return Ok(());
    };
    let intent = IntentStore::new(db.clone())
        .find(intent_id, user_id)
        .await
        .map_err(intention_error)?;
    if intent.status != IntentStatus::Accepted {
        return Err(AppError::conflict("The intention is not awaiting Work"));
    }
    if intent.proposal.instruction.trim() != input.trim() {
        return Err(AppError::bad_request(
            "Work input does not match the accepted intention",
        ));
    }
    let grant = AutonomyGrantStore::new(db.clone())
        .find(user_id)
        .await
        .map_err(intention_error)?;
    let granted: Vec<String> = crate::services::agent::get_user_permissions(db, user_id)
        .await
        .into_iter()
        .collect();
    validate_intention_work_grant(user_id, grant.as_ref(), &granted, intent.accept_source)
}

pub(crate) async fn begin_intention_work(
    db: &DatabaseConnection,
    intent_id: Option<&str>,
    user_id: i32,
    session_id: Option<String>,
    run_id: Option<String>,
) -> Result<(), AppError> {
    let Some(intent_id) = intent_id else {
        return Ok(());
    };
    IntentStore::new(db.clone())
        .transition(
            intent_id,
            user_id,
            IntentStatus::Running,
            session_id,
            run_id,
            None,
        )
        .await
        .map_err(intention_error)?;
    Ok(())
}

pub(crate) async fn autonomy_cap_for_intention(
    db: &DatabaseConnection,
    intent_id: Option<&str>,
    user_id: i32,
) -> Result<Option<Vec<String>>, AppError> {
    let Some(intent_id) = intent_id else {
        return Ok(None);
    };
    let intent = IntentStore::new(db.clone())
        .find(intent_id, user_id)
        .await
        .map_err(intention_error)?;
    if intent.accept_source != AcceptSource::Autonomy {
        return Ok(None);
    }
    let grant = AutonomyGrantStore::new(db.clone())
        .find(user_id)
        .await
        .map_err(intention_error)?;
    let granted: Vec<String> = crate::services::agent::get_user_permissions(db, user_id)
        .await
        .into_iter()
        .collect();
    match evaluate_autonomy_grant(user_id, grant.as_ref(), &granted) {
        AutonomyVerdict::AllowPersonalWork {
            granted_permissions,
        } => Ok(Some(granted_permissions)),
        AutonomyVerdict::RequireUserReview => Err(AppError::forbidden(
            "Personal autonomy is no longer granted; review is required",
        )),
    }
}

pub(crate) async fn advance_intention_work(
    db: &DatabaseConnection,
    intent_id: Option<&str>,
    user_id: i32,
    status: IntentStatus,
    result_summary: Option<String>,
) {
    let Some(intent_id) = intent_id else {
        return;
    };
    if let Err(error) = IntentStore::new(db.clone())
        .transition(intent_id, user_id, status, None, None, result_summary)
        .await
    {
        tracing::warn!(%error, intent_id, ?status, "[Agent API] failed to advance intention");
    }
}

pub(crate) fn intention_error(error: sea_orm::DbErr) -> AppError {
    tracing::warn!(%error, "[Agent] intention operation failed");
    match error {
        sea_orm::DbErr::RecordNotFound(_) => AppError::not_found("Intention not found"),
        sea_orm::DbErr::Custom(_) => {
            AppError::conflict("Intention state changed; refresh and try again")
        }
        _ => AppError::internal("Intention service is temporarily unavailable"),
    }
}

/// Grant re-filter at Work entry. Extracted so tests drive the same function
/// `/process` uses after the intention is Accepted.
pub(crate) fn validate_intention_work_grant(
    user_id: i32,
    grant: Option<&crate::services::agent::consciousness::AutonomyGrantView>,
    current_granted_permissions: &[String],
    accept_source: AcceptSource,
) -> Result<(), AppError> {
    if user_id == crate::services::agent::SYSTEM_USER_ID || user_id <= 0 {
        return Err(AppError::forbidden(
            "Personal autonomy cannot run as the heartbeat identity",
        ));
    }
    if !intention_may_enter_work(user_id, grant, current_granted_permissions, accept_source) {
        return Err(AppError::forbidden(
            "Personal autonomy is no longer granted; review is required",
        ));
    }
    Ok(())
}
