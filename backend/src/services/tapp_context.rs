//! Tapp runtime context payloads and subject role projection.
//!
//! Pure builders for `/api/tapp/context/*` so HTTP handlers only resolve Claims,
//! DB rows, filesystem timestamps, and wire JSON. Keeps response shape and
//! role rules testable without Axum.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::services::permission_service::UserRole;

/// Resolve the capability role for an authenticated or soft-guest subject id.
///
/// - Current admin → [`UserRole::Admin`]
/// - `user_id <= 0` (guest tokens / anonymous) → [`UserRole::Guest`]
/// - otherwise → [`UserRole::User`]
pub fn role_for_subject(user_id: i32, is_current_admin: bool) -> UserRole {
    if is_current_admin {
        UserRole::Admin
    } else if user_id <= 0 {
        UserRole::Guest
    } else {
        UserRole::User
    }
}

/// Same rules as [`role_for_subject`] for optional JWT subjects (catalog paths).
///
/// Missing or negative ids are guests; non-negative authenticated ids are users
/// unless elevated to admin.
pub fn role_for_optional_subject(user_id: Option<i32>, is_current_admin: bool) -> UserRole {
    if is_current_admin {
        UserRole::Admin
    } else if user_id.is_some_and(|id| id >= 0) {
        UserRole::User
    } else {
        UserRole::Guest
    }
}

/// `GET /api/tapp/context/app` body (version + features).
///
/// `locale` is the **host UI** locale (from request headers), not a hard-coded
/// `zh-CN` — otherwise Tapps keep Chinese after the user switches language.
pub fn context_app_payload(
    version: &str,
    ai_enabled: bool,
    platforms: &[String],
    locale: &str,
) -> Value {
    json!({
        "version": version,
        "locale": locale,
        "theme": "system",
        "features": {
            "aiEnabled": ai_enabled,
            "platforms": platforms
        }
    })
}

/// `GET /api/tapp/context/user` body from resolved identity fields.
///
/// `language` / `timezone` come from the host (request headers), not hard-coded
/// Asia/Shanghai — matches the user's actual UI locale and browser TZ.
pub fn context_user_payload(
    user_id: i32,
    username: &str,
    display_name: Option<String>,
    avatar_url: Option<String>,
    is_current_admin: bool,
    connected_platforms: &[String],
    language: &str,
    timezone: &str,
) -> Value {
    let role = role_for_subject(user_id, is_current_admin);
    json!({
        "id": format!("user_{}", user_id),
        "username": username,
        "display_name": display_name,
        "avatar": avatar_url.clone(),
        "avatar_url": avatar_url,
        "isAdmin": is_current_admin,
        "role": role.as_str(),
        // Aro / soft-guest paths treat this as a strong "not guest" signal.
        "authenticated": user_id > 0,
        "connectedPlatforms": connected_platforms,
        "preferences": {
            "language": language,
            "timezone": timezone
        }
    })
}

/// Idle player stub — real-time state is delivered via TappBridge events.
pub fn idle_player_context() -> Value {
    json!({
        "isPlaying": false,
        "isPaused": false,
        "currentTrack": null,
        "progress": { "current": 0, "duration": 0, "percentage": 0 },
        "playlist": null,
        "mode": "sequence",
        "volume": 80,
        "muted": false,
        "_note": "Real-time player state is provided via TappBridge events"
    })
}

/// Idle navigation stub — real-time path is delivered via TappBridge events.
pub fn idle_navigation_context() -> Value {
    json!({
        "currentPath": "/",
        "previousPath": null,
        "history": [],
        "availableRoutes": [
            { "path": "/", "name": "home", "icon": "home" },
            { "path": "/library", "name": "library", "icon": "book" },
            { "path": "/brew", "name": "brew", "icon": "book-open" },
            { "path": "/reports", "name": "reports", "icon": "bar-chart" },
            { "path": "/tapp", "name": "tapp", "icon": "grid" },
            { "path": "/config", "name": "config", "icon": "settings" }
        ],
        "tappPages": [],
        "_note": "Real-time navigation state is provided via TappBridge events"
    })
}

/// `GET /api/tapp/context/system` body after IO (ping + cache mtimes) is resolved.
pub fn context_system_payload(
    server_connected: bool,
    version: &str,
    last_fetch: &HashMap<String, Option<String>>,
) -> Value {
    json!({
        "online": true,
        "serverConnected": server_connected,
        "version": version,
        "backgroundTasks": Value::Array(vec![]),
        "lastFetch": last_fetch
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_for_subject_admin_user_guest() {
        assert_eq!(role_for_subject(9, true), UserRole::Admin);
        assert_eq!(role_for_subject(9, false), UserRole::User);
        assert_eq!(role_for_subject(0, false), UserRole::Guest);
        assert_eq!(role_for_subject(-1, false), UserRole::Guest);
        // Admin flag wins even for non-positive ids (should not happen in practice).
        assert_eq!(role_for_subject(-1, true), UserRole::Admin);
    }

    #[test]
    fn role_for_optional_subject_matches_catalog_rules() {
        assert_eq!(
            role_for_optional_subject(Some(42), false),
            UserRole::User
        );
        assert_eq!(
            role_for_optional_subject(Some(0), false),
            UserRole::User
        );
        assert_eq!(role_for_optional_subject(None, false), UserRole::Guest);
        assert_eq!(
            role_for_optional_subject(Some(-5), false),
            UserRole::Guest
        );
        assert_eq!(
            role_for_optional_subject(Some(42), true),
            UserRole::Admin
        );
    }

    #[test]
    fn context_app_payload_exposes_ai_and_platforms() {
        let platforms = vec!["steam".into(), "github".into()];
        let body = context_app_payload("0.3.18", true, &platforms, "en-US");
        assert_eq!(body["version"], "0.3.18");
        assert_eq!(body["features"]["aiEnabled"], true);
        assert_eq!(body["features"]["platforms"][0], "steam");
        assert_eq!(body["locale"], "en-US");
    }

    #[test]
    fn context_user_payload_marks_guest_unauthenticated() {
        let body = context_user_payload(
            -1,
            "guest",
            None,
            None,
            false,
            &[],
            "ja-JP",
            "Asia/Tokyo",
        );
        assert_eq!(body["role"], "guest");
        assert_eq!(body["authenticated"], false);
        assert_eq!(body["isAdmin"], false);
        assert_eq!(body["id"], "user_-1");
        assert_eq!(body["preferences"]["language"], "ja-JP");
        assert_eq!(body["preferences"]["timezone"], "Asia/Tokyo");
    }

    #[test]
    fn context_user_payload_admin_and_avatar_aliases() {
        let body = context_user_payload(
            1,
            "owner",
            Some("Owner".into()),
            Some("https://cdn.example/a.png".into()),
            true,
            &["steam".into()],
            "zh-CN",
            "Asia/Shanghai",
        );
        assert_eq!(body["role"], "admin");
        assert_eq!(body["authenticated"], true);
        assert_eq!(body["isAdmin"], true);
        assert_eq!(body["avatar"], "https://cdn.example/a.png");
        assert_eq!(body["avatar_url"], "https://cdn.example/a.png");
        assert_eq!(body["connectedPlatforms"][0], "steam");
        assert_eq!(body["preferences"]["language"], "zh-CN");
    }

    #[test]
    fn idle_player_and_navigation_preserve_bridge_notes() {
        let player = idle_player_context();
        assert_eq!(player["isPlaying"], false);
        assert_eq!(player["volume"], 80);
        assert!(player["_note"].as_str().unwrap().contains("TappBridge"));

        let nav = idle_navigation_context();
        assert_eq!(nav["currentPath"], "/");
        assert_eq!(nav["availableRoutes"][0]["path"], "/");
        let paths: Vec<&str> = nav["availableRoutes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r["path"].as_str())
            .collect();
        assert!(paths.contains(&"/brew"));
        assert!(paths.contains(&"/reports"));
        assert!(paths.contains(&"/tapp"));
        assert!(paths.contains(&"/config"));
        assert!(!paths.contains(&"/life"));
        assert!(!paths.contains(&"/settings"));
        assert!(nav["_note"].as_str().unwrap().contains("TappBridge"));
    }

    #[test]
    fn context_system_payload_embeds_last_fetch() {
        let mut last = HashMap::new();
        last.insert("steam".into(), Some("2026-01-01T00:00:00Z".into()));
        last.insert("github".into(), None);
        let body = context_system_payload(true, "0.3.18", &last);
        assert_eq!(body["serverConnected"], true);
        assert_eq!(body["online"], true);
        assert_eq!(body["version"], "0.3.18");
        assert_eq!(body["lastFetch"]["steam"], "2026-01-01T00:00:00Z");
        assert!(body["lastFetch"]["github"].is_null());
        assert!(body["backgroundTasks"].as_array().unwrap().is_empty());
    }
}
