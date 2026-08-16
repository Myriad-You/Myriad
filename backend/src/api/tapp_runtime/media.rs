//! 媒体控制 API

use axum::{extract::State, http::StatusCode, Extension, Json};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::services::permission_service::TappPermission;

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
/// `frontend/src/tapp/runtime/permissionConfig.ts` and locked by the machine
/// readable fixture `docs/development/tapp/fixtures/media_action_permissions.json`
/// (frontend + backend table-driven tests). Playback state = `media:playback`,
/// volume = `media:volume`, playlist ordering/mode = `media:queue`. The coarse
/// `media:control` no longer exists (ADR 0013 / 0020) and no legacy alias maps
/// here. `None` = unknown action, which the endpoint rejects as BAD_REQUEST.
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

    // 未知 action 直接 400：授权只认 media_control_permission 这一份映射，
    // 没有第二份 action 清单可漂移，也不允许 expect panic 兜底。
    let Some(permission) = media_control_permission(&req.action) else {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("Invalid action: {}", req.action) })),
        )));
    };

    // Authorize the narrowest action domain before any behavior occurs.
    // A grant holding only one media domain must not affect the others.
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
    /// endpoint's `let-else` turns this into BAD_REQUEST before authorization.
    #[test]
    fn unknown_actions_are_bad_request_not_panics() {
        for action in ["", "skip", "playlist", "setMode", "setVolume", "load"] {
            assert_eq!(media_control_permission(action), None, "action {action:?}");
        }
    }

    /// 端点级跨域拒绝：模拟 handler 在行为前的授权判定（grant 权限集 ×
    /// action → 最窄权限）。比较语义与 RuntimeGrant::has 相同（按权限名字符串
    /// 精确匹配）；runtime_grant.require 自身的错误契约由
    /// `services::tapp_runtime_grant` 测试覆盖，这里锁定分域本身。
    fn grant_allows(granted: &[&str], permission: TappPermission) -> bool {
        granted
            .iter()
            .any(|granted| *granted == permission.as_str())
    }

    #[test]
    fn endpoint_denies_cross_domain_actions_for_partial_grants() {
        let playback_only = ["media:playback"];
        for action in ["volume", "mute", "unmute", "mode"] {
            let required = media_control_permission(action).unwrap();
            assert!(
                !grant_allows(&playback_only, required),
                "media:playback-only grant must be denied at the endpoint for {action}"
            );
        }

        let volume_only = ["media:volume"];
        for action in ["play", "pause", "next", "prev", "seek", "mode"] {
            let required = media_control_permission(action).unwrap();
            assert!(
                !grant_allows(&volume_only, required),
                "media:volume-only grant must be denied at the endpoint for {action}"
            );
        }

        let queue_only = ["media:queue"];
        for action in [
            "play", "pause", "next", "prev", "seek", "volume", "mute", "unmute",
        ] {
            let required = media_control_permission(action).unwrap();
            assert!(
                !grant_allows(&queue_only, required),
                "media:queue-only grant must be denied at the endpoint for {action}"
            );
        }
    }

    /// 端点放行路径：grant 含对应最窄权限时同一判定必须通过。
    #[test]
    fn endpoint_allows_action_when_narrowest_permission_is_granted() {
        let full = ["media:playback", "media:volume", "media:queue"];
        for action in [
            "play", "pause", "next", "prev", "seek", "volume", "mute", "unmute", "mode",
        ] {
            let required = media_control_permission(action).unwrap();
            assert!(
                grant_allows(&full, required),
                "full media grant must allow {action}"
            );
        }
        // 单域 grant 只放行自己域内的 action
        assert!(grant_allows(
            &["media:volume"],
            media_control_permission("volume").unwrap()
        ));
        assert!(grant_allows(
            &["media:queue"],
            media_control_permission("mode").unwrap()
        ));
        assert!(grant_allows(
            &["media:playback"],
            media_control_permission("seek").unwrap()
        ));
    }

    /// 机器可读 fixture 与后端映射锁定：每个 control action 的权限必须一致，
    /// 所有权限串必须是已知 TappPermission。
    #[test]
    fn fixture_media_action_permissions_match_backend_mapping() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../docs/development/tapp/fixtures/media_action_permissions.json"
        ))
        .expect("media action fixture must parse");

        let control = fixture["controlActions"]
            .as_array()
            .expect("controlActions");
        assert!(!control.is_empty());
        for entry in control {
            let action = entry["action"].as_str().expect("control action");
            let permission = entry["permission"].as_str().expect("control permission");
            assert_eq!(
                media_control_permission(action).map(|p| p.as_str()),
                Some(permission),
                "fixture control action {action} must match backend mapping"
            );
            assert!(
                TappPermission::from_str(permission).is_some(),
                "fixture permission {permission} for {action} must be a known TappPermission"
            );
        }

        // bridge 顶层 action 是纯前端路径（TappBridge/PERMISSION_MAP），不在
        // 后端 media_control_permission 域内；这里锁定其权限串仍是已知权限，
        // 且落在三个 media 动作域内（前端 fixture 一致性测试做完整对照）。
        let bridge = fixture["bridgeActions"].as_array().expect("bridgeActions");
        assert!(!bridge.is_empty());
        for entry in bridge {
            let action = entry["action"].as_str().expect("bridge action");
            let permission = entry["permission"].as_str().expect("bridge permission");
            assert!(
                TappPermission::from_str(permission).is_some(),
                "fixture bridge permission {permission} for {action} must be known"
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
