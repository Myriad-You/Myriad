//! Role-aware Tapp catalog / detail projection.
//!
//! Lives in services so install lists, detail views, and agent/UI surfaces share
//! one permission-filtered projection without importing `crate::api::tapp_store`.
//! HTTP handlers only load models and wrap [`TappListItem`] / [`TappDetail`] in
//! API envelopes.

use serde::Serialize;

use crate::config::DynamicConfig;
use crate::models::entities::tapps;
use crate::services::permission_service::{TappPermissionService, UserRole};

/// Compact list row for catalog / install responses.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TappListItem {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    /// 内联 SVG 图标代码（优先于 icon）
    pub icon_svg: Option<String>,
    /// manifest.locales 透传：语言标签 → { name?, description? }
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locales: Option<serde_json::Value>,
    pub status: String,
    pub installed_at: String,
    pub last_run_at: Option<String>,
    /// 是否为临时安装（普通用户安装的 Tapp）
    #[serde(default)]
    pub is_temporary: bool,
    /// 是否为管理员/站点的公开 Tapp
    #[serde(default)]
    pub is_admin_tapp: bool,
    /// 公开安装可见性：`all` | `admin`（私有安装始终仅本人）
    #[serde(default = "default_tapp_visibility")]
    pub visibility: String,
    #[serde(default)]
    pub needs_reauthorization: bool,
}

/// Full detail projection for catalog detail endpoints.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TappDetail {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<serde_json::Value>,
    pub icon: Option<String>,
    pub theme_color: Option<String>,
    pub manifest: serde_json::Value,
    pub status: String,
    pub granted_permissions: Vec<String>,
    #[serde(default)]
    pub needs_reauthorization: bool,
    pub installed_at: String,
    pub last_run_at: Option<String>,
    /// 当前用户角色: "guest" | "user" | "admin"
    pub user_role: String,
    #[serde(default)]
    pub is_temporary: bool,
    #[serde(default)]
    pub is_admin_tapp: bool,
    /// 公开安装可见性：`all` | `admin`
    #[serde(default = "default_tapp_visibility")]
    pub visibility: String,
}

fn default_tapp_visibility() -> String {
    crate::services::tapp_ownership::TAPP_VISIBILITY_ALL.to_string()
}

/// Catalog namespace flags for a row.
///
/// - Site-owner / public install → `(is_temporary=false, is_admin_tapp=true)`
/// - Subject private install → `(is_temporary=true, is_admin_tapp=false)`
pub fn catalog_install_flags(is_site_owner_install: bool) -> (bool, bool) {
    if is_site_owner_install {
        (false, true)
    } else {
        (true, false)
    }
}

/// Lowercase install status label preserved by the API contract.
pub fn install_status_label(status: &tapps::TappStatus) -> String {
    format!("{status:?}").to_lowercase()
}

/// Extract `manifest.locales` when it is a JSON object; otherwise `None`.
pub fn manifest_locales(manifest: &serde_json::Value) -> Option<serde_json::Value> {
    manifest.get("locales").filter(|v| v.is_object()).cloned()
}

/// Inline SVG from `manifest.iconSvg` when present.
pub fn icon_svg_from_manifest(manifest: &serde_json::Value) -> Option<String> {
    manifest
        .get("iconSvg")
        .and_then(serde_json::Value::as_str)
        .map(String::from)
}

/// Project a DB install row into a list item with namespace flags.
pub fn tapp_list_item_from_model(
    tapp: tapps::Model,
    is_temporary: bool,
    is_admin_tapp: bool,
) -> TappListItem {
    let approved_permissions: Vec<String> =
        serde_json::from_value(tapp.approved_permissions.clone()).unwrap_or_default();
    let needs_reauthorization = approved_permissions.iter().any(|permission| {
        crate::services::permission_service::TappPermission::from_str(permission).is_none()
    });
    let icon_svg = icon_svg_from_manifest(&tapp.manifest);
    let locales = manifest_locales(&tapp.manifest);
    let visibility =
        crate::services::tapp_ownership::normalize_tapp_visibility(&tapp.visibility).to_string();
    TappListItem {
        id: tapp.tapp_id,
        name: tapp.name,
        version: tapp.version,
        description: tapp.description,
        icon: tapp.icon,
        icon_svg,
        locales,
        status: install_status_label(&tapp.status),
        installed_at: tapp.installed_at.to_rfc3339(),
        last_run_at: tapp.last_run_at.map(|date| date.to_rfc3339()),
        is_temporary,
        is_admin_tapp,
        visibility,
        needs_reauthorization,
    }
}

/// List-item projection for a successful **install** response.
///
/// Uses [`catalog_install_flags`] for temporary/public flags. The HTTP contract
/// reports `status = "installed"` and omits `last_run_at` even when the DB row
/// was inserted as Running with a timestamp (clients treat install as not yet
/// "started" from the list UI).
pub fn install_response_list_item(tapp: tapps::Model, is_site_owner_install: bool) -> TappListItem {
    let (is_temporary, is_admin_tapp) = catalog_install_flags(is_site_owner_install);
    let mut item = tapp_list_item_from_model(tapp, is_temporary, is_admin_tapp);
    item.status = "installed".to_string();
    item.last_run_at = None;
    item
}

/// List-item projection for a successful **update** response.
///
/// Preserves live status / last_run_at from the DB row; flags follow
/// [`catalog_install_flags`].
pub fn update_response_list_item(tapp: tapps::Model, is_site_owner_install: bool) -> TappListItem {
    let (is_temporary, is_admin_tapp) = catalog_install_flags(is_site_owner_install);
    tapp_list_item_from_model(tapp, is_temporary, is_admin_tapp)
}

/// Project a DB install row into a role-filtered detail DTO.
///
/// `granted_permissions` is the intersection of approved install permissions
/// with the current role's capability policy (not the legacy DB snapshot field).
pub fn tapp_detail_from_model(
    tapp: tapps::Model,
    role: UserRole,
    is_temporary: bool,
    is_admin_tapp: bool,
    config: &DynamicConfig,
) -> TappDetail {
    let approved_permissions: Vec<String> =
        serde_json::from_value(tapp.approved_permissions.clone()).unwrap_or_default();
    let (granted_permissions, needs_reauthorization) =
        match TappPermissionService::filter_permissions_for_role(
            config,
            role,
            &approved_permissions,
        ) {
            Ok(granted_permissions) => (granted_permissions, false),
            Err(_) => (Vec::new(), true),
        };
    let visibility =
        crate::services::tapp_ownership::normalize_tapp_visibility(&tapp.visibility).to_string();
    TappDetail {
        id: tapp.tapp_id,
        name: tapp.name,
        version: tapp.version,
        description: tapp.description,
        author: tapp.author,
        icon: tapp.icon,
        theme_color: tapp.theme_color,
        manifest: tapp.manifest,
        status: install_status_label(&tapp.status),
        granted_permissions,
        needs_reauthorization,
        installed_at: tapp.installed_at.to_rfc3339(),
        last_run_at: tapp.last_run_at.map(|date| date.to_rfc3339()),
        user_role: role.as_str().to_string(),
        is_temporary,
        is_admin_tapp,
        visibility,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;

    fn sample_model(approved: serde_json::Value) -> tapps::Model {
        let now = Utc::now().fixed_offset();
        tapps::Model {
            id: 1,
            tapp_id: "com.example.detail".to_string(),
            user_id: 7,
            name: "Detail".to_string(),
            version: "1.0.0".to_string(),
            description: Some("desc".to_string()),
            author: Some(json!({"name": "Ada"})),
            icon: Some("icon.png".to_string()),
            theme_color: Some("#fff".to_string()),
            manifest: json!({
                "id": "com.example.detail",
                "name": "Detail",
                "version": "1.0.0",
                "main": "main.js",
                "iconSvg": "<svg/>",
                "locales": { "zh-CN": { "name": "详情" } },
                "permissions": ["storage:read", "brew:write", "ai:generate"]
            }),
            status: tapps::TappStatus::Installed,
            granted_permissions: json!(["storage:read"]),
            approved_permissions: approved,
            file_path: "manifest.json".to_string(),
            code_path: "main.js".to_string(),
            installed_at: now,
            last_run_at: None,
            updated_at: now,
            error_message: None,
            visibility: "all".to_string(),
        }
    }

    #[test]
    fn catalog_install_flags_private_vs_public() {
        assert_eq!(catalog_install_flags(false), (true, false));
        assert_eq!(catalog_install_flags(true), (false, true));
    }

    #[test]
    fn install_status_label_is_lowercase_debug() {
        assert_eq!(
            install_status_label(&tapps::TappStatus::Installed),
            "installed"
        );
        assert_eq!(install_status_label(&tapps::TappStatus::Running), "running");
        assert_eq!(install_status_label(&tapps::TappStatus::Error), "error");
    }

    #[test]
    fn manifest_locales_requires_object() {
        assert!(manifest_locales(&json!({"locales": {"en": {}}})).is_some());
        assert!(manifest_locales(&json!({"locales": "en"})).is_none());
        assert!(manifest_locales(&json!({})).is_none());
    }

    #[test]
    fn icon_svg_from_manifest_reads_string_only() {
        assert_eq!(
            icon_svg_from_manifest(&json!({"iconSvg": "<svg/>"})),
            Some("<svg/>".into())
        );
        assert!(icon_svg_from_manifest(&json!({"iconSvg": 1})).is_none());
        assert!(icon_svg_from_manifest(&json!({})).is_none());
    }

    #[test]
    fn list_item_projection_carries_manifest_extras_and_flags() {
        let item = tapp_list_item_from_model(sample_model(json!([])), true, false);
        assert_eq!(item.id, "com.example.detail");
        assert_eq!(item.icon_svg.as_deref(), Some("<svg/>"));
        assert!(item.locales.unwrap().is_object());
        assert_eq!(item.status, "installed");
        assert!(item.is_temporary);
        assert!(!item.is_admin_tapp);
        assert!(item.last_run_at.is_none());
        assert!(!item.needs_reauthorization);
    }

    #[test]
    fn install_response_list_item_forces_installed_and_clears_last_run() {
        let mut model = sample_model(json!([]));
        model.status = tapps::TappStatus::Running;
        model.last_run_at = Some(Utc::now().fixed_offset());
        // Site-owner public install.
        let public = install_response_list_item(model.clone(), true);
        assert_eq!(public.status, "installed");
        assert!(public.last_run_at.is_none());
        assert!(!public.is_temporary);
        assert!(public.is_admin_tapp);
        assert_eq!(public.icon_svg.as_deref(), Some("<svg/>"));
        // Private install.
        let private = install_response_list_item(model, false);
        assert!(private.is_temporary);
        assert!(!private.is_admin_tapp);
        assert_eq!(private.status, "installed");
    }

    #[test]
    fn update_response_list_item_preserves_live_status_and_last_run() {
        let mut model = sample_model(json!([]));
        model.status = tapps::TappStatus::Running;
        let now = Utc::now().fixed_offset();
        model.last_run_at = Some(now);
        let item = update_response_list_item(model, true);
        assert_eq!(item.status, "running");
        assert!(item.last_run_at.is_some());
        assert!(!item.is_temporary);
        assert!(item.is_admin_tapp);
    }

    #[test]
    fn detail_mapping_applies_current_role_and_capability_rules() {
        let config = DynamicConfig {
            user_perm_ai_generate: true,
            ..Default::default()
        };
        let detail = tapp_detail_from_model(
                sample_model(json!(["storage:read", "brew:write", "ai:generate"])),
            UserRole::User,
            true,
            false,
            &config,
        );

        assert_eq!(detail.user_role, "user");
        assert!(detail.is_temporary);
        assert!(!detail.is_admin_tapp);
        assert_eq!(
            detail.granted_permissions,
            vec!["storage:read", "brew:write", "ai:generate"]
        );
        assert_eq!(detail.status, "installed");
        assert_eq!(detail.theme_color.as_deref(), Some("#fff"));
        assert!(!detail.needs_reauthorization);
    }

    #[test]
    fn detail_mapping_filters_capabilities_denied_to_role() {
        // Without user AI generate, ai:generate must not appear for User role.
        let config = DynamicConfig {
            user_perm_ai_generate: false,
            ..Default::default()
        };
        let detail = tapp_detail_from_model(
            sample_model(json!(["storage:read", "ai:generate"])),
            UserRole::User,
            false,
            true,
            &config,
        );
        assert!(detail.granted_permissions.contains(&"storage:read".to_string()));
        assert!(!detail
            .granted_permissions
            .iter()
            .any(|p| p == "ai:generate"));
        assert!(!detail.is_temporary);
        assert!(detail.is_admin_tapp);
    }

    #[test]
    fn guest_detail_uses_guest_role_string() {
        let config = DynamicConfig::default();
        let detail = tapp_detail_from_model(
            sample_model(json!(["storage:read"])),
            UserRole::Guest,
            false,
            true,
            &config,
        );
        assert_eq!(detail.user_role, "guest");
    }

    #[test]
    fn unknown_approved_permission_marks_reauthorization_without_breaking_projection() {
        let model = sample_model(json!(["storage:read", "legacy:unknown", "ui:theme:read"]));
        let item = tapp_list_item_from_model(model.clone(), false, true);
        let detail = tapp_detail_from_model(
            model,
            UserRole::Admin,
            false,
            true,
            &DynamicConfig::default(),
        );

        assert!(item.needs_reauthorization);
        assert!(detail.needs_reauthorization);
        assert!(detail.granted_permissions.is_empty());
    }
}
