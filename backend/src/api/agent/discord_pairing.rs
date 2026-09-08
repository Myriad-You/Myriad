//! Login-user Discord DM pairing: issue a one-time code, show status, unbind.

use super::*;
use crate::error::HttpError;
use crate::services::discord_pairing::{self, IssuedPairingCode, PairingStatus};

/// GET /api/agent/discord/pairing
pub async fn get_discord_pairing(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    let status = discord_pairing::status_for_user(&db, user_id)
        .await
        .map_err(pairing_db_error)?;
    Ok(Json(json!({ "pairing": pairing_json(&status) })))
}

/// POST /api/agent/discord/pairing
pub async fn post_discord_pairing(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    let current = discord_pairing::status_for_user(&db, user_id)
        .await
        .map_err(pairing_db_error)?;
    if current.paired {
        return Err(HttpError::from((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Already paired",
                "code": "discord_already_paired",
                "pairing": pairing_json(&current)
            })),
        )));
    }
    let issued: IssuedPairingCode = discord_pairing::mint_code(&db, user_id)
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

/// DELETE /api/agent/discord/pairing
pub async fn delete_discord_pairing(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;
    let unbound = discord_pairing::unpair(&db, user_id)
        .await
        .map_err(pairing_db_error)?;
    if !unbound {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Not paired", "code": "discord_not_paired" })),
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
    tracing::error!(%error, "Discord pairing database error");
    HttpError::from((
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "Pairing is temporarily unavailable",
            "code": "discord_pairing_failed"
        })),
    ))
}
