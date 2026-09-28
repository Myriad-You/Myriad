//! Login-user OneBot pairing: issue a one-time code, show status, unbind.

use super::*;
use crate::error::HttpError;
use crate::services::channel_pairing::{self, IssuedPairingCode, ONEBOT, PairingStatus};

/// GET /api/agent/onebot/pairing
pub async fn get_onebot_pairing(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_pairing_user_id(&claims)?;
    let status = channel_pairing::status_for_user(&db, ONEBOT, user_id)
        .await
        .map_err(pairing_db_error)?;
    Ok(Json(json!({ "pairing": pairing_json(&status) })))
}

/// POST /api/agent/onebot/pairing
pub async fn post_onebot_pairing(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    let current = channel_pairing::status_for_user(&db, ONEBOT, user_id)
        .await
        .map_err(pairing_db_error)?;
    if current.paired {
        return Err(HttpError::from((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Already paired",
                "code": "onebot_already_paired",
                "pairing": pairing_json(&current)
            })),
        )));
    }
    let issued: IssuedPairingCode = channel_pairing::mint_code(&db, ONEBOT, user_id)
        .await
        .map_err(pairing_db_error)?;
    Ok(Json(json!({
        "code": issued.display,
        "expiresAt": issued.expires_at,
        "pairing": pairing_json(&PairingStatus {
            paired: false,
            identity_id: None,
            openid_masked: None,
            linked_at: None,
            pending_code: Some(issued.display.clone()),
            pending_expires_at: Some(issued.expires_at.clone()),
        })
    })))
}

/// DELETE /api/agent/onebot/pairing
pub async fn delete_onebot_pairing(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_pairing_user_id(&claims)?;
    let unbound = channel_pairing::unpair(&db, ONEBOT, user_id)
        .await
        .map_err(pairing_db_error)?;
    if !unbound {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Not paired", "code": "onebot_not_paired" })),
        )));
    }
    Ok(Json(json!({ "success": true })))
}

fn pairing_json(status: &PairingStatus) -> Value {
    json!({
        "paired": status.paired,
        "identityId": status.identity_id,
        "openidMasked": status.openid_masked,
        "linkedAt": status.linked_at,
        "pendingCode": status.pending_code,
        "pendingExpiresAt": status.pending_expires_at,
    })
}

fn pairing_db_error(error: sea_orm::DbErr) -> HttpError {
    tracing::error!(%error, "OneBot pairing database error");
    HttpError::from((
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "Pairing is temporarily unavailable",
            "code": "onebot_pairing_failed"
        })),
    ))
}
