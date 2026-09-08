//! Admin-only Telegram bot status and saved-credential probe.

use super::*;
use crate::error::HttpError;
use crate::services::telegram_bot::{self, TelegramBotPhase};
use myriad_agent_rules::channel::ConnectFailureKind;

/// GET /api/agent/telegram/status
pub async fn get_telegram_bot_status(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    let status = telegram_bot::current_status().await;
    Ok(Json(json!({
        "phase": status.phase,
        "enabled": status.enabled,
        "hasToken": status.has_token,
        "botUsername": status.bot_username,
        "botName": status.bot_name,
        "lastInboundAt": status.last_inbound_at,
    })))
}

/// POST /api/agent/telegram/test
pub async fn post_telegram_bot_test(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    match telegram_bot::test_saved_credentials().await {
        Ok(identity) => Ok(Json(json!({
            "success": true,
            "phase": TelegramBotPhase::Online,
            "botUsername": identity.username,
            "botName": identity.first_name,
        }))),
        Err(ConnectFailureKind::Permanent) => Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "Telegram credentials were rejected",
                "code": "telegram_credentials_rejected"
            })),
        ))),
        Err(ConnectFailureKind::Transient) => Err(HttpError::from((
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "success": false,
                "error": "Telegram API is temporarily unreachable",
                "code": "telegram_api_unreachable"
            })),
        ))),
    }
}
