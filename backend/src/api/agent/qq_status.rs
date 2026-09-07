//! Admin-only QQ bot Gateway status and saved-credential probe.

use super::*;
use crate::error::HttpError;
use crate::services::qq_bot::{self, QqBotPhase};
use myriad_agent_rules::channel::ConnectFailureKind;

/// GET /api/agent/qq/status
pub async fn get_qq_bot_status(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    let status = qq_bot::current_status().await;
    Ok(Json(json!({
        "phase": status.phase,
        "enabled": status.enabled,
        "hasAppId": status.has_app_id,
        "hasSecret": status.has_secret,
    })))
}

/// POST /api/agent/qq/test
pub async fn post_qq_bot_test(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    match qq_bot::test_saved_credentials().await {
        Ok(()) => Ok(Json(json!({
            "success": true,
            "phase": QqBotPhase::Online,
        }))),
        Err(ConnectFailureKind::Permanent) => Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "QQ credentials were rejected",
                "code": "qq_credentials_rejected"
            })),
        ))),
        Err(ConnectFailureKind::Transient) => Err(HttpError::from((
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "success": false,
                "error": "QQ Gateway is temporarily unreachable",
                "code": "qq_gateway_unreachable"
            })),
        ))),
    }
}
