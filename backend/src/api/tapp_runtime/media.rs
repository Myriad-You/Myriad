//! 媒体控制 API

use axum::{extract::State, http::StatusCode, Extension, Json};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::middleware::auth::Claims;
use crate::services::permission_service::TappPermission;
use crate::error::HttpError;

use super::common::authorize_tapp_permission;
use super::runtime_grant::RuntimeGrantContext;

#[derive(Debug, Deserialize)]
pub struct MediaControlRequest {
    pub tapp_id: String,
    pub action: String,
    pub value: Option<Value>,
}

/// Accepted `MediaControlRequest.action` values → narrowest required permission.
///
/// Mirrored by frontend `MEDIA_ACTION_PERMISSIONS` in
/// `frontend/src/tapp/runtime/permissionConfig.ts`; both sides enforce the same
/// split. Playback state = `media:playback`, volume = `media:volume`, playlist
/// ordering/mode = `media:queue`. The coarse `media:control` no longer exists
/// (ADR 0013 / 0020) and no legacy alias maps here.
pub fn media_control_permission(action: &str) -> Option<TappPermission> {
    match action {
        "play" | "pause" | "next" | "prev" | "seek" => Some(TappPermission::MediaPlayback),
        "volume" | "mute" | "unmute" => Some(TappPermission::MediaVolume),
        "mode" => Some(TappPermission::MediaQueue),
        _ => None,
    }
}

/// POST /api/tapp/media/control
pub async fn media_control(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Json(req): Json<MediaControlRequest>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&req.tapp_id)?;

    let valid_actions = [
        "play", "pause", "next", "prev", "seek", "volume", "mode", "mute", "unmute",
    ];
    if !valid_actions.contains(&req.action.as_str()) {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("Invalid action: {}", req.action) })),
        )));
    }

    // Authorize the narrowest action domain before any behavior occurs.
    // A grant holding only one media domain must not affect the others.
    let permission = media_control_permission(&req.action)
        .expect("validated action always maps to a media permission");
    runtime_grant.require(permission)?;
    authorize_tapp_permission(&db, &claims, &req.tapp_id, permission, &dynamic_config).await?;

    tracing::info!(
        "[TAPP] media_control - User: {}, Tapp: {}, Action: {}",
        claims.username,
        req.tapp_id,
        req.action
    );

    let valid_actions = [
        "play", "pause", "next", "prev", "seek", "volume", "mode", "mute", "unmute",
    ];
    if !valid_actions.contains(&req.action.as_str()) {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("Invalid action: {}", req.action) })),
        )));
    }

    match req.action.as_str() {
        "seek" => {
            if req.value.is_none() {
                return Err(HttpError::from((
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "Seek action requires a position value" })),
                )));
            }
        }
        "volume" => {
            if let Some(val) = &req.value {
                if let Some(v) = val.as_f64() {
                    if !(0.0..=100.0).contains(&v) {
                        return Err(HttpError::from((
                            StatusCode::BAD_REQUEST,
                            Json(json!({ "error": "Volume must be between 0 and 100" })),
                        )));
                    }
                }
            }
        }
        "mode" => {
            if let Some(val) = &req.value {
                let valid_modes = ["sequence", "loop", "shuffle", "single"];
                if let Some(mode) = val.as_str() {
                    if !valid_modes.contains(&mode) {
                        return Err(HttpError::from((
                            StatusCode::BAD_REQUEST,
                            Json(json!({ "error": format!("Invalid mode: {}", mode) })),
                        )));
                    }
                }
            }
        }
        _ => {}
    }

    Ok(Json(json!({
        "success": true,
        "action": req.action,
        "value": req.value,
        "_note": "Media control is handled by TappBridge on the frontend"
    })))
}

/// GET /api/tapp/media/status
pub async fn media_status(
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require(TappPermission::MediaRead)?;
    tracing::debug!("[TAPP] media_status - User: {}", claims.username);

    Ok(Json(json!({
        "success": true,
        "status": {
            "isPlaying": false,
            "isPaused": false,
            "currentTrack": null,
            "progress": { "current": 0, "duration": 0, "percentage": 0 },
            "playlist": null,
            "mode": "sequence",
            "volume": 80,
            "muted": false
        },
        "_note": "Real-time status is provided via TappBridge"
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every accepted `MediaControlRequest.action` maps to the narrowest
    /// permission domain that covers its real host effect.
    #[test]
    fn every_accepted_action_maps_to_narrowest_permission() {
        let cases: &[(&str, TappPermission)] = &[
            // 播放状态域
            ("play", TappPermission::MediaPlayback),
            ("pause", TappPermission::MediaPlayback),
            ("next", TappPermission::MediaPlayback),
            ("prev", TappPermission::MediaPlayback),
            ("seek", TappPermission::MediaPlayback),
            // 音量域
            ("volume", TappPermission::MediaVolume),
            ("mute", TappPermission::MediaVolume),
            ("unmute", TappPermission::MediaVolume),
            // 队列/模式域
            ("mode", TappPermission::MediaQueue),
        ];
        for (action, expected) in cases {
            assert_eq!(
                media_control_permission(action),
                Some(*expected),
                "action {action} must require {}",
                expected.as_str()
            );
        }
    }

    /// Unknown or rejected actions must not map to any permission — the
    /// endpoint rejects them as BAD_REQUEST before authorization is consulted.
    #[test]
    fn unknown_actions_have_no_permission_mapping() {
        for action in ["", "skip", "playlist", "setMode", "setVolume", "load"] {
            assert_eq!(media_control_permission(action), None, "action {action:?}");
        }
    }

    /// Cross-domain negative cases: a grant holding only one of the three
    /// media domains must never authorize an action from another domain.
    #[test]
    fn cross_domain_grants_do_not_cover_other_action_domains() {
        let playback_only = [TappPermission::MediaPlayback];
        for action in ["volume", "mute", "unmute", "mode"] {
            let required = media_control_permission(action).unwrap();
            assert!(
                !playback_only.contains(&required),
                "media:playback-only grant must not authorize {action}"
            );
        }

        let volume_only = [TappPermission::MediaVolume];
        for action in ["play", "pause", "next", "prev", "seek", "mode"] {
            let required = media_control_permission(action).unwrap();
            assert!(
                !volume_only.contains(&required),
                "media:volume-only grant must not authorize {action}"
            );
        }

        let queue_only = [TappPermission::MediaQueue];
        for action in [
            "play", "pause", "next", "prev", "seek", "volume", "mute", "unmute",
        ] {
            let required = media_control_permission(action).unwrap();
            assert!(
                !queue_only.contains(&required),
                "media:queue-only grant must not authorize {action}"
            );
        }
    }

    /// All accepted actions stay inside the three media domains — nothing may
    /// leak back to the removed coarse permission or to read-only media:read.
    #[test]
    fn no_action_maps_to_removed_or_read_only_permissions() {
        for action in [
            "play", "pause", "next", "prev", "seek", "volume", "mode", "mute", "unmute",
        ] {
            let required = media_control_permission(action).unwrap();
            assert_ne!(required.as_str(), "media:control");
            assert_ne!(required.as_str(), "media:read");
            assert_ne!(required.as_str(), "media:audio");
        }
    }
}
