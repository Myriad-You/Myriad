//! Admin-only Feishu bot status and saved-credential probe.

use super::*;
use crate::error::HttpError;
use crate::services::feishu_bot::{self, FeishuBotPhase};
use myriad_agent_rules::channel::ConnectFailureKind;

/// GET /api/agent/feishu/status
pub async fn get_feishu_bot_status(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    let status = feishu_bot::current_status().await;
    Ok(Json(json!({
        "phase": status.phase,
        "enabled": status.enabled,
        "hasAppId": status.has_app_id,
        "hasSecret": status.has_secret,
        "appId": status.app_id,
        "lastInboundAt": status.last_inbound_at,
    })))
}

/// POST /api/agent/feishu/test
pub async fn post_feishu_bot_test(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    require_current_admin(&claims, &db).await?;
    match feishu_bot::test_saved_credentials().await {
        Ok(()) => Ok(Json(json!({
            "success": true,
            "phase": FeishuBotPhase::Online,
        }))),
        Err(ConnectFailureKind::Permanent) => Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "Feishu credentials were rejected",
                "code": "feishu_credentials_rejected"
            })),
        ))),
        Err(ConnectFailureKind::Transient) => Err(HttpError::from((
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "success": false,
                "error": "Feishu API is temporarily unreachable",
                "code": "feishu_api_unreachable"
            })),
        ))),
    }
}
