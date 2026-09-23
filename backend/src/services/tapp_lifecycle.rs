//! Tapp install lifecycle pure decisions and projections.
//!
//! Covers start/stop/uninstall branch selection, post-uninstall filesystem
//! cleanup path choice, runtime-widget ownership binding, and recent-activity
//! list projection. HTTP handlers keep DB/fs/grant side effects and map
//! outcomes to status codes.

use serde::Serialize;

use crate::services::tapp_catalog::{icon_svg_from_manifest, manifest_locales};

// ── Uninstall ───────────────────────────────────────────────────────────────

/// Pure mirror of `uninstall_tapp` branch order (own → public+admin → not found).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UninstallTarget {
    OwnInstall,
    PublicRequiresAdmin,
    NotFound,
}

/// Select which install row the uninstall handler should act on.
pub fn select_uninstall_target(has_own_install: bool, has_public_install: bool) -> UninstallTarget {
    if has_own_install {
        UninstallTarget::OwnInstall
    } else if has_public_install {
        UninstallTarget::PublicRequiresAdmin
    } else {
        UninstallTarget::NotFound
    }
}

/// Directory name used when quarantining a live install before DB cleanup.
pub fn uninstall_quarantine_dir_name(tapp_id: &str, uuid_simple: &str) -> String {
    format!(".{tapp_id}.uninstall-{uuid_simple}")
}

// ── Start / stop ────────────────────────────────────────────────────────────

/// Outcome of a start request after presence flags are known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOutcome {
    /// Mutate the subject's private install row to Running + activity.
    MutatePrivate,
    /// Mutate the site-owner public install (admin only) + activity.
    MutatePublic,
    /// Non-admin on a public install that is already Running: activity only.
    RecordActivityOnly,
    /// Non-admin on a public install that is not Running.
    Forbidden,
    NotFound,
}

/// Resolve start behavior (private-first; public session rules for non-admins).
///
/// `has_private` is only set when the handler actually queried a private copy
/// (subject is not the site owner). `public_is_running` is ignored unless the
/// public branch is taken for a non-admin.
pub fn resolve_start_outcome(
    has_private: bool,
    has_public: bool,
    is_current_admin: bool,
    public_is_running: bool,
) -> StartOutcome {
    if has_private {
        return StartOutcome::MutatePrivate;
    }
    if has_public {
        if is_current_admin {
            return StartOutcome::MutatePublic;
        }
        if public_is_running {
            return StartOutcome::RecordActivityOnly;
        }
        return StartOutcome::Forbidden;
    }
    StartOutcome::NotFound
}

/// Outcome of a stop request after presence flags are known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopOutcome {
    /// Mutate private row to Installed + revoke grants for subject.
    MutatePrivate,
    /// Admin mutates public row + revoke subject's grants.
    MutatePublic,
    /// Non-admin on public: revoke subject's grants only (no public row write).
    RevokeOnly,
    NotFound,
}

pub fn resolve_stop_outcome(
    has_private: bool,
    has_public: bool,
    is_current_admin: bool,
) -> StopOutcome {
    if has_private {
        return StopOutcome::MutatePrivate;
    }
    if has_public {
        if is_current_admin {
            return StopOutcome::MutatePublic;
        }
        return StopOutcome::RevokeOnly;
    }
    StopOutcome::NotFound
}

// ── Runtime widgets ─────────────────────────────────────────────────────────

/// `config.source` string when present.
pub fn widget_source(config: &serde_json::Value) -> Option<&str> {
    config.get("source").and_then(serde_json::Value::as_str)
}

/// `config.installationOwnerId` when present and in i32 range.
pub fn widget_installation_owner(config: &serde_json::Value) -> Option<i32> {
    config
        .get("installationOwnerId")
        .and_then(serde_json::Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

/// Whether a runtime-sourced widget row belongs to the given installation.
///
/// Runtime widgets bind to `installationOwnerId` when set; rows without
/// the field bind to the subject when subject == installation owner.
pub fn runtime_widget_belongs_to_installation(
    config: &serde_json::Value,
    subject_id: i32,
    installation_owner_id: i32,
) -> bool {
    widget_source(config) == Some("runtime")
        && (widget_installation_owner(config) == Some(installation_owner_id)
            || (widget_installation_owner(config).is_none() && subject_id == installation_owner_id))
}

/// Canonical registry id: `tapp.{tapp_id}.{local_id}`.
pub fn format_tapp_widget_id(tapp_id: &str, local_id: &str) -> String {
    format!("tapp.{tapp_id}.{local_id}")
}

/// Resolve a path/body widget id to the full `tapp.{tapp_id}.…` form.
///
/// Accepts either a local id or a full id that already carries the expected
/// prefix. Rejects full ids scoped to a different tapp.
pub fn resolve_full_widget_id(tapp_id: &str, widget_id: &str) -> Result<String, ()> {
    if widget_id.starts_with("tapp.") {
        let expected_prefix = format!("tapp.{tapp_id}.");
        if !widget_id.starts_with(&expected_prefix) {
            return Err(());
        }
        Ok(widget_id.to_string())
    } else {
        Ok(format_tapp_widget_id(tapp_id, widget_id))
    }
}

/// Shape checks for runtime `POST …/widgets` body (before settings/policy).
///
/// Returns `false` when the request must map to HTTP 400.
pub fn runtime_widget_register_shape_ok(
    id: &str,
    name: &str,
    sizes: &[String],
    default_size: &str,
) -> bool {
    use crate::services::tapp_validation::{is_safe_path_component, is_valid_widget_size};

    is_safe_path_component(id)
        && !name.is_empty()
        && name.len() <= 255
        && !sizes.is_empty()
        && sizes.len() <= 10
        && sizes.iter().all(|size| is_valid_widget_size(size))
        && sizes.iter().any(|size| size == default_size)
}

/// Full widget ids from a previous raw manifest JSON `widgets` array.
///
/// During reconcile a row is manifest-sourced when `config.source` is
/// `"manifest"` or its id is one of these legacy (source-less) ids.
pub fn legacy_manifest_widget_ids(
    tapp_id: &str,
    previous_manifest: Option<&serde_json::Value>,
) -> std::collections::HashSet<String> {
    previous_manifest
        .and_then(|value| value.get("widgets"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|widget| widget.get("id").and_then(serde_json::Value::as_str))
        .map(|id| format_tapp_widget_id(tapp_id, id))
        .collect()
}

/// Local widget id stripped of the `tapp.{tapp_id}.` prefix when present.
pub fn local_widget_id_from_full(tapp_id: &str, full_widget_id: &str) -> String {
    let prefix = format!("tapp.{tapp_id}.");
    full_widget_id
        .strip_prefix(&prefix)
        .unwrap_or(full_widget_id)
        .to_string()
}

/// Whether the installer's total widget count may accept one more runtime widget.
pub fn runtime_widget_slot_available(
    manifest_widget_count: usize,
    runtime_widget_count: usize,
    max_widgets: usize,
) -> bool {
    manifest_widget_count + runtime_widget_count < max_widgets
}

/// Whether a local widget id collides with a declared manifest widget.
pub fn manifest_declares_local_widget_id(manifest: &serde_json::Value, local_id: &str) -> bool {
    manifest
        .get("widgets")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|widgets| {
            widgets.iter().any(|widget| {
                widget.get("id").and_then(serde_json::Value::as_str) == Some(local_id)
            })
        })
}

// ── Recent activity list ────────────────────────────────────────────────────

/// Recent-use list item for `GET /api/tapps/recent`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecentTappItem {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub icon_svg: Option<String>,
    pub theme_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locales: Option<serde_json::Value>,
    pub last_run_at: String,
    pub run_count: i32,
}

/// Clamp the recent-list limit to the API contract (1..=50).
pub fn clamp_recent_limit(limit: i32) -> usize {
    limit.clamp(1, 50) as usize
}

/// Project activity + install metadata into a recent list row.
pub fn recent_tapp_item(
    tapp_id: &str,
    name: &str,
    icon: Option<String>,
    theme_color: Option<String>,
    manifest: &serde_json::Value,
    last_run_at_rfc3339: String,
    run_count: i32,
) -> RecentTappItem {
    RecentTappItem {
        id: tapp_id.to_string(),
        name: name.to_string(),
        icon,
        icon_svg: icon_svg_from_manifest(manifest),
        theme_color,
        locales: manifest_locales(manifest),
        last_run_at: last_run_at_rfc3339,
        run_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn uninstall_prefers_own_install_when_public_coexists() {
        assert_eq!(
            select_uninstall_target(true, true),
            UninstallTarget::OwnInstall
        );
        assert_eq!(
            select_uninstall_target(true, false),
            UninstallTarget::OwnInstall
        );
        assert_eq!(
            select_uninstall_target(false, true),
            UninstallTarget::PublicRequiresAdmin
        );
        assert_eq!(
            select_uninstall_target(false, false),
            UninstallTarget::NotFound
        );
    }

    #[test]
    fn quarantine_dir_name_is_stable() {
        assert_eq!(
            uninstall_quarantine_dir_name("com.example.app", "deadbeef"),
            ".com.example.app.uninstall-deadbeef"
        );
    }

    #[test]
    fn start_outcome_private_first_and_public_session_rules() {
        assert_eq!(
            resolve_start_outcome(true, true, false, false),
            StartOutcome::MutatePrivate
        );
        assert_eq!(
            resolve_start_outcome(false, true, true, false),
            StartOutcome::MutatePublic
        );
        assert_eq!(
            resolve_start_outcome(false, true, false, true),
            StartOutcome::RecordActivityOnly
        );
        assert_eq!(
            resolve_start_outcome(false, true, false, false),
            StartOutcome::Forbidden
        );
        assert_eq!(
            resolve_start_outcome(false, false, true, false),
            StartOutcome::NotFound
        );
    }

    #[test]
    fn stop_outcome_private_first_and_public_revoke_rules() {
        assert_eq!(
            resolve_stop_outcome(true, true, false),
            StopOutcome::MutatePrivate
        );
        assert_eq!(
            resolve_stop_outcome(false, true, true),
            StopOutcome::MutatePublic
        );
        assert_eq!(
            resolve_stop_outcome(false, true, false),
            StopOutcome::RevokeOnly
        );
        assert_eq!(
            resolve_stop_outcome(false, false, true),
            StopOutcome::NotFound
        );
    }

    #[test]
    fn runtime_widget_owner_binding_prevents_cross_installation_reuse() {
        let public = json!({ "source": "runtime", "installationOwnerId": 1 });
        assert!(runtime_widget_belongs_to_installation(&public, 9, 1));
        assert!(!runtime_widget_belongs_to_installation(&public, 9, 9));

        let legacy_private = json!({ "source": "runtime" });
        assert!(runtime_widget_belongs_to_installation(
            &legacy_private,
            9,
            9
        ));
        assert!(!runtime_widget_belongs_to_installation(
            &legacy_private,
            9,
            1
        ));

        let manifest_widget = json!({ "source": "manifest" });
        assert!(!runtime_widget_belongs_to_installation(
            &manifest_widget,
            9,
            9
        ));
    }

    #[test]
    fn clamp_recent_limit_contract() {
        assert_eq!(clamp_recent_limit(0), 1);
        assert_eq!(clamp_recent_limit(10), 10);
        assert_eq!(clamp_recent_limit(100), 50);
        assert_eq!(clamp_recent_limit(-3), 1);
    }

    #[test]
    fn recent_item_pulls_manifest_extras() {
        let item = recent_tapp_item(
            "com.example.app",
            "App",
            Some("icon.png".into()),
            Some("#abc".into()),
            &json!({
                "iconSvg": "<svg/>",
                "locales": { "en": { "name": "App" } }
            }),
            "2026-01-01T00:00:00+00:00".into(),
            3,
        );
        assert_eq!(item.id, "com.example.app");
        assert_eq!(item.icon_svg.as_deref(), Some("<svg/>"));
        assert!(item.locales.unwrap().is_object());
        assert_eq!(item.run_count, 3);
        assert_eq!(item.theme_color.as_deref(), Some("#abc"));
    }

    #[test]
    fn widget_id_format_resolve_and_register_shape() {
        assert_eq!(
            format_tapp_widget_id("com.example.app", "card"),
            "tapp.com.example.app.card"
        );
        assert_eq!(
            resolve_full_widget_id("com.example.app", "card").unwrap(),
            "tapp.com.example.app.card"
        );
        assert_eq!(
            resolve_full_widget_id("com.example.app", "tapp.com.example.app.card").unwrap(),
            "tapp.com.example.app.card"
        );
        assert!(resolve_full_widget_id("com.example.app", "tapp.other.card").is_err());
        assert_eq!(
            local_widget_id_from_full("com.example.app", "tapp.com.example.app.card"),
            "card"
        );

        assert!(runtime_widget_register_shape_ok(
            "card",
            "Card",
            &["2x2".into(), "4x2".into()],
            "2x2"
        ));
        assert!(!runtime_widget_register_shape_ok(
            "../evil",
            "Card",
            &["2x2".into()],
            "2x2"
        ));
        assert!(!runtime_widget_register_shape_ok(
            "card",
            "",
            &["2x2".into()],
            "2x2"
        ));
        assert!(!runtime_widget_register_shape_ok(
            "card",
            "Card",
            &["2x2".into()],
            "4x2"
        ));
        assert!(!runtime_widget_register_shape_ok(
            "card",
            "Card",
            &[],
            "2x2"
        ));
    }

    #[test]
    fn reconcile_and_slot_helpers() {
        let legacy =
            legacy_manifest_widget_ids("com.ex", Some(&json!({ "widgets": [{ "id": "old" }] })));
        assert!(legacy.contains("tapp.com.ex.old"));

        assert!(runtime_widget_slot_available(1, 1, 3));
        assert!(!runtime_widget_slot_available(2, 1, 3));
        assert!(manifest_declares_local_widget_id(
            &json!({ "widgets": [{ "id": "card" }] }),
            "card"
        ));
        assert!(!manifest_declares_local_widget_id(
            &json!({ "widgets": [{ "id": "card" }] }),
            "list"
        ));
    }
}
