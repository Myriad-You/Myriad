//! Admin-only Discord bot status and saved-credential probe.

use super::*;
use crate::error::HttpError;
use crate::services::discord_bot::{self, DiscordBotPhase};
use myriad_agent_rules::channel::ConnectFailureKind;

/// GET /api/agent/discord/status
pub async fn get_discord_bot_status(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    let status = discord_bot::current_status().await;
    Ok(Json(json!({
        "phase": status.phase,
        "enabled": status.enabled,
        "hasToken": status.has_token,
        "botUsername": status.bot_username,
        "botName": status.bot_name,
        "botUserId": status.bot_user_id,
        "lastInboundAt": status.last_inbound_at,
    })))
}

/// POST /api/agent/discord/test
pub async fn post_discord_bot_test(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    match discord_bot::test_saved_credentials().await {
        Ok(identity) => Ok(Json(json!({
            "success": true,
            "phase": DiscordBotPhase::Online,
            "botUsername": identity.username,
            "botName": identity.global_name.as_deref().unwrap_or(&identity.username),
            "botUserId": identity.id,
        }))),
        Err(ConnectFailureKind::Permanent) => Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "Discord credentials were rejected",
                "code": "discord_credentials_rejected"
            })),
        ))),
        Err(ConnectFailureKind::Transient) => Err(HttpError::from((
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "success": false,
                "error": "Discord API is temporarily unreachable",
                "code": "discord_api_unreachable"
            })),
        ))),
    }
}
