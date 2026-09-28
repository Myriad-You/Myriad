//! Admin-only OneBot status. There is no separate credential probe:
//! a bad token is only visible after NapCat accepts the socket and
//! then closes it.

use super::*;
use crate::error::HttpError;
use crate::services::onebot_bot;

/// GET /api/agent/onebot/status
pub async fn get_onebot_bot_status(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    let status = onebot_bot::current_status().await;
    Ok(Json(json!({
        "phase": status.phase,
        "enabled": status.enabled,
        "hasUrl": status.has_url,
        "hasToken": status.has_token,
        "wsUrl": status.ws_url,
        "lastInboundAt": status.last_inbound_at,
    })))
}
