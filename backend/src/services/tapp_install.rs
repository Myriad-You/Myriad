//! Pure install/update request decisions for Tapp packages.
//!
//! HTTP handlers keep Claims, DB, filesystem, and role-config permission
//! filtering. Domain owns:
//! - install/update source mode parsing (`direct` | `store`)
//! - direct-mode CSS channel routing (declared styles vs generated sidecars)
//! - approved-permission selection (manifest ∩ request / previous)
//! - install/update DB column snapshots (paths, permissions JSON, default status)
//! - multipart `.tapp` upload field classification + archive size gate
//!
//! Canonical install-owner / conflict-owner namespaces live in
//! [`crate::services::tapp_ownership`].

/// Package provenance for install and update endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallSource {
    /// Client-supplied manifest + resources (or multipart archive path).
    Direct,
    /// Backend fetches from a configured store catalog.
    Store,
}

/// Unknown or empty `source` field on install/update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidInstallSource;

impl InvalidInstallSource {
    pub fn message(self) -> &'static str {
        "Invalid source, must be 'direct' or 'store'"
    }

    pub fn status_hint(self) -> u16 {
        400
    }
}

/// Parse the install/update `source` string (`direct` | `store`).
pub fn parse_install_source(source: &str) -> Result<InstallSource, InvalidInstallSource> {
    match source.trim() {
        "direct" => Ok(InstallSource::Direct),
        "store" => Ok(InstallSource::Store),
        _ => Err(InvalidInstallSource),
    }
}

/// Where client `widgetCss` / `pageCss` bodies should land for direct install.
///
/// Prefer declared `manifest.widgetStyles` / `pageStyles` paths when present
/// (cssMode=separated). Otherwise treat the bodies as generated
/// `widget.css` / `page.css` sidecars.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DirectCssChannels {
    pub widget_styles: Option<String>,
    pub generated_widget_css: Option<String>,
    pub page_styles: Option<String>,
    pub generated_page_css: Option<String>,
}

/// Route optional widget/page CSS bodies according to manifest declarations.
pub fn map_direct_css_channels(
    manifest_declares_widget_styles: bool,
    manifest_declares_page_styles: bool,
    widget_css: Option<String>,
    page_css: Option<String>,
) -> DirectCssChannels {
    let (widget_styles, generated_widget_css) = if manifest_declares_widget_styles {
        (widget_css, None)
    } else {
        (None, widget_css)
    };
    let (page_styles, generated_page_css) = if manifest_declares_page_styles {
        (page_css, None)
    } else {
        (None, page_css)
    };
    DirectCssChannels {
        widget_styles,
        generated_widget_css,
        page_styles,
        generated_page_css,
    }
}

/// Select approved permissions for a **new** install.
///
/// Product contract (matches API comment「可选，默认全部授权」and file-install
/// callers that omit `permissions`):
///
/// - Empty / omitted `requested` → **all** `manifest_permissions` (default full grant).
/// - Non-empty → intersection of manifest declarations with the request
/// (unknown / undeclared names are dropped).
pub fn select_install_approved_permissions(
    manifest_permissions: &[String],
    requested: &[String],
) -> Vec<String> {
    if requested.is_empty() {
        manifest_permissions.to_vec()
    } else {
        manifest_permissions
            .iter()
            .filter(|p| requested.iter().any(|r| r == *p))
            .cloned()
            .collect()
    }
}

/// Select approved permissions for an **update**.
///
/// - `requested == None` → keep previous approvals that still exist in the new
/// manifest (drop permissions the new version no longer declares).
/// - `requested == Some([])` → **all** new manifest permissions (default full grant,
/// same empty-list product semantics as install).
/// - `requested == Some(list)` → intersection with the new manifest.
pub fn select_update_approved_permissions(
    manifest_permissions: &[String],
    requested: Option<&[String]>,
    previous_approved: &[String],
) -> Vec<String> {
    match requested {
        Some(perms) if perms.is_empty() => manifest_permissions.to_vec(),
        Some(perms) => manifest_permissions
            .iter()
            .filter(|p| perms.iter().any(|r| r == *p))
            .cloned()
            .collect(),
        None => manifest_permissions
            .iter()
            .filter(|p| previous_approved.iter().any(|r| r == *p))
            .cloned()
            .collect(),
    }
}

/// Whether the installation owner namespace is the public site-owner row.
pub fn is_public_installation_namespace(installation_owner_id: i32, site_owner_id: i32) -> bool {
    installation_owner_id == site_owner_id
}

// ── Persist path + column snapshots ─────────────────────────────────────────

use std::path::Path;

use chrono::{DateTime, FixedOffset};
use myriad_tapp_contract::manifest::TappManifest;

use crate::services::tapp_package_fs::MANIFEST_JSON;

/// Absolute path strings stored on the install row after activate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPathPair {
    /// `…/manifest.json` under the live install directory.
    pub file_path: String,
    /// Live path to the package main entry (`manifest.main`).
    pub code_path: String,
}

/// Build `file_path` / `code_path` for a live install directory.
pub fn install_path_pair(final_tapp_dir: &Path, main: &str) -> InstallPathPair {
    InstallPathPair {
        file_path: final_tapp_dir
            .join(MANIFEST_JSON)
            .to_string_lossy()
            .into_owned(),
        code_path: final_tapp_dir.join(main).to_string_lossy().into_owned(),
    }
}

/// Serialize optional manifest author for the `tapps.author` JSON column.
pub fn manifest_author_json(
    author: &Option<myriad_tapp_contract::manifest::TappAuthor>,
) -> Option<serde_json::Value> {
    author
        .as_ref()
        .and_then(|a| serde_json::to_value(a).ok())
}

/// Pre-SeaORM column snapshot for a **new** install insert.
///
/// New installs always start `Running` so public dashboard widgets render
/// without a manual Start click; operators can still Stop from the list UI.
#[derive(Debug, Clone, PartialEq)]
pub struct NewInstallPersist {
    pub tapp_id: String,
    pub user_id: i32,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<serde_json::Value>,
    pub icon: Option<String>,
    pub theme_color: Option<String>,
    pub manifest: serde_json::Value,
    pub start_running: bool,
    pub granted_permissions: serde_json::Value,
    pub approved_permissions: serde_json::Value,
    pub file_path: String,
    pub code_path: String,
    pub installed_at: DateTime<FixedOffset>,
    pub last_run_at: DateTime<FixedOffset>,
    pub updated_at: DateTime<FixedOffset>,
}

/// Project manifest + approvals into new-install DB columns.
pub fn build_new_install_persist(
    manifest: &TappManifest,
    installation_owner_id: i32,
    granted: &[String],
    approved: &[String],
    final_tapp_dir: &Path,
    now: DateTime<FixedOffset>,
) -> Result<NewInstallPersist, String> {
    let paths = install_path_pair(final_tapp_dir, &manifest.main);
    let manifest_json =
        serde_json::to_value(manifest).map_err(|e| format!("Failed to serialize manifest: {e}"))?;
    Ok(NewInstallPersist {
        tapp_id: manifest.id.clone(),
        user_id: installation_owner_id,
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        author: manifest_author_json(&manifest.author),
        icon: manifest.icon.clone(),
        theme_color: manifest.theme_color.clone(),
        manifest: manifest_json,
        start_running: true,
        granted_permissions: serde_json::to_value(granted).unwrap_or_default(),
        approved_permissions: serde_json::to_value(approved).unwrap_or_default(),
        file_path: paths.file_path,
        code_path: paths.code_path,
        installed_at: now,
        last_run_at: now,
        updated_at: now,
    })
}

/// Pre-SeaORM column snapshot for an **update** (preserves install identity).
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateInstallPersist {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<serde_json::Value>,
    pub icon: Option<String>,
    pub theme_color: Option<String>,
    pub manifest: serde_json::Value,
    pub granted_permissions: serde_json::Value,
    pub approved_permissions: serde_json::Value,
    pub code_path: String,
    pub updated_at: DateTime<FixedOffset>,
}

/// Project updated package + approvals into update DB columns.
pub fn build_update_install_persist(
    manifest: &TappManifest,
    granted: &[String],
    approved: &[String],
    final_tapp_dir: &Path,
    now: DateTime<FixedOffset>,
) -> Result<UpdateInstallPersist, String> {
    let paths = install_path_pair(final_tapp_dir, &manifest.main);
    let manifest_json =
        serde_json::to_value(manifest).map_err(|e| format!("Failed to serialize manifest: {e}"))?;
    Ok(UpdateInstallPersist {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        author: manifest_author_json(&manifest.author),
        icon: manifest.icon.clone(),
        theme_color: manifest.theme_color.clone(),
        manifest: manifest_json,
        granted_permissions: serde_json::to_value(granted).unwrap_or_default(),
        approved_permissions: serde_json::to_value(approved).unwrap_or_default(),
        code_path: paths.code_path,
        updated_at: now,
    })
}

// ── Multipart .tapp upload ──────────────────────────────────────────────────

/// Recognized multipart field names on `POST /tapp/install/file`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMultipartField {
    File,
    Permissions,
    Ignore,
}

/// Classify a multipart field name for archive install.
pub fn classify_install_multipart_field(name: &str) -> InstallMultipartField {
    match name {
        "file" => InstallMultipartField::File,
        "permissions" => InstallMultipartField::Permissions,
        _ => InstallMultipartField::Ignore,
    }
}

/// Whether accepting `chunk_len` more bytes would exceed the archive cap.
pub fn archive_upload_would_exceed(
    current_len: usize,
    chunk_len: usize,
    max_bytes: usize,
) -> bool {
    current_len.saturating_add(chunk_len) > max_bytes
}

/// User-facing message when a `.tapp` upload exceeds the contract limit.
pub fn archive_upload_too_large_message(max_bytes: usize) -> String {
    format!(".tapp file exceeds {max_bytes} bytes")
}

// ── Concurrent install capacity (MYR-025) ───────────────────────────────────

/// Max concurrent Tapp install handlers (archive extract / stage / DB).
///
/// Generous but finite: prevents unbounded zip buffers + blocking extract work
/// from exhausting memory and the async runtime under parallel uploads.
pub const MAX_CONCURRENT_INSTALLS: usize = 4;

/// How long a request may wait for an install slot before 503.
pub const INSTALL_ACQUIRE_TIMEOUT_SECS: u64 = 2;

/// User-facing overload body when install concurrency is saturated.
pub fn install_overloaded_message() -> &'static str {
    "Too many Tapp installs in progress. Please try again shortly."
}

/// HTTP status when install concurrency is saturated (retryable overload).
pub fn install_overloaded_status() -> u16 {
    503
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn parse_install_source_accepts_direct_and_store() {
        assert_eq!(parse_install_source("direct").unwrap(), InstallSource::Direct);
        assert_eq!(parse_install_source("store").unwrap(), InstallSource::Store);
        assert_eq!(parse_install_source(" store ").unwrap(), InstallSource::Store);
        assert!(parse_install_source("").is_err());
        assert!(parse_install_source("file").is_err());
        assert_eq!(
            InvalidInstallSource.message(),
            "Invalid source, must be 'direct' or 'store'"
        );
        assert_eq!(InvalidInstallSource.status_hint(), 400);
    }

    #[test]
    fn map_direct_css_prefers_declared_styles_channels() {
        let declared = map_direct_css_channels(
            true,
            true,
            Some("w-body".into()),
            Some("p-body".into()),
        );
        assert_eq!(declared.widget_styles.as_deref(), Some("w-body"));
        assert!(declared.generated_widget_css.is_none());
        assert_eq!(declared.page_styles.as_deref(), Some("p-body"));
        assert!(declared.generated_page_css.is_none());

        let sidecars = map_direct_css_channels(
            false,
            false,
            Some("w-body".into()),
            Some("p-body".into()),
        );
        assert!(sidecars.widget_styles.is_none());
        assert_eq!(sidecars.generated_widget_css.as_deref(), Some("w-body"));
        assert!(sidecars.page_styles.is_none());
        assert_eq!(sidecars.generated_page_css.as_deref(), Some("p-body"));

        let mixed = map_direct_css_channels(true, false, Some("w".into()), Some("p".into()));
        assert_eq!(mixed.widget_styles.as_deref(), Some("w"));
        assert!(mixed.generated_widget_css.is_none());
        assert!(mixed.page_styles.is_none());
        assert_eq!(mixed.generated_page_css.as_deref(), Some("p"));
    }

    #[test]
    fn install_concurrency_limits_are_generous_but_finite() {
        // MYR-025: a few concurrent installs, not unbounded; fail fast when full.
        const {
            assert!(MAX_CONCURRENT_INSTALLS >= 2);
            assert!(MAX_CONCURRENT_INSTALLS <= 8);
            assert!(INSTALL_ACQUIRE_TIMEOUT_SECS >= 1);
            assert!(INSTALL_ACQUIRE_TIMEOUT_SECS <= 15);
        }
        assert_eq!(install_overloaded_status(), 503);
        assert!(!install_overloaded_message().is_empty());
    }

    #[test]
    fn install_approved_permissions_empty_defaults_to_full_manifest() {
        let manifest = vec!["storage".into(), "network".into(), "ai".into()];
        // Product: omit / empty request → full manifest (file install without permissions field).
        assert_eq!(
            select_install_approved_permissions(&manifest, &[]),
            manifest
        );
        assert_eq!(
            select_install_approved_permissions(&manifest, &["network".into(), "evil".into()]),
            vec!["network".to_string()]
        );
        assert!(select_install_approved_permissions(&manifest, &["evil".into()]).is_empty());
    }

    #[test]
    fn update_approved_permissions_keeps_overlap_or_replaces() {
        let manifest = vec!["storage".into(), "network".into(), "ai".into()];
        let previous = vec!["storage".into(), "network".into(), "legacy".into()];

        // No request: keep previous that still exist in new manifest.
        assert_eq!(
            select_update_approved_permissions(&manifest, None, &previous),
            vec!["storage".to_string(), "network".to_string()]
        );
        // Empty list: default full grant of new manifest (same as install).
        assert_eq!(
            select_update_approved_permissions(&manifest, Some(&[]), &previous),
            manifest
        );
        // Explicit list: intersection with new manifest.
        assert_eq!(
            select_update_approved_permissions(
                &manifest,
                Some(&["ai".into(), "gone".into()]),
                &previous
            ),
            vec!["ai".to_string()]
        );
    }

    #[test]
    fn public_namespace_matches_site_owner() {
        assert!(is_public_installation_namespace(1, 1));
        assert!(!is_public_installation_namespace(42, 1));
    }

    fn sample_manifest() -> TappManifest {
        serde_json::from_value(json!({
            "id": "com.example.app",
            "name": "App",
            "version": "1.2.3",
            "description": "demo",
            "main": "src/main.js",
            "category": "utility",
            "permissions": ["storage"],
            "icon": "icon.png",
            "themeColor": "#abc",
            "author": { "name": "Ada" }
        }))
        .unwrap()
    }

    #[test]
    fn install_path_pair_joins_manifest_and_main() {
        let paths = install_path_pair(Path::new("/data/tapps/1/com.example.app"), "src/main.js");
        assert_eq!(
            paths.file_path,
            "/data/tapps/1/com.example.app/manifest.json"
        );
        assert_eq!(
            paths.code_path,
            "/data/tapps/1/com.example.app/src/main.js"
        );
    }

    #[test]
    fn new_install_persist_starts_running_with_paths_and_perms() {
        let now = DateTime::parse_from_rfc3339("2026-01-02T03:04:05+00:00").unwrap();
        let snap = build_new_install_persist(
            &sample_manifest(),
            7,
            &["storage".into()],
            &["storage".into()],
            Path::new("/data/tapps/7/com.example.app"),
            now,
        )
        .unwrap();
        assert_eq!(snap.tapp_id, "com.example.app");
        assert_eq!(snap.user_id, 7);
        assert_eq!(snap.version, "1.2.3");
        assert!(snap.start_running);
        assert_eq!(
            snap.file_path,
            "/data/tapps/7/com.example.app/manifest.json"
        );
        assert_eq!(snap.code_path, "/data/tapps/7/com.example.app/src/main.js");
        assert_eq!(snap.installed_at, now);
        assert_eq!(snap.last_run_at, now);
        assert_eq!(snap.granted_permissions, json!(["storage"]));
        assert_eq!(snap.author.as_ref().unwrap()["name"], "Ada");
    }

    #[test]
    fn update_install_persist_refreshes_code_path_and_perms() {
        let now = DateTime::parse_from_rfc3339("2026-02-01T00:00:00+00:00").unwrap();
        let snap = build_update_install_persist(
            &sample_manifest(),
            &["storage".into()],
            &["storage".into()],
            Path::new("/data/tapps/1/com.example.app"),
            now,
        )
        .unwrap();
        assert_eq!(snap.version, "1.2.3");
        assert_eq!(
            snap.code_path,
            "/data/tapps/1/com.example.app/src/main.js"
        );
        assert_eq!(snap.updated_at, now);
        assert_eq!(snap.approved_permissions, json!(["storage"]));
    }

    #[test]
    fn multipart_field_and_archive_size_gate() {
        assert_eq!(
            classify_install_multipart_field("file"),
            InstallMultipartField::File
        );
        assert_eq!(
            classify_install_multipart_field("permissions"),
            InstallMultipartField::Permissions
        );
        assert_eq!(
            classify_install_multipart_field("other"),
            InstallMultipartField::Ignore
        );
        assert!(!archive_upload_would_exceed(10, 5, 20));
        assert!(archive_upload_would_exceed(16, 5, 20));
        assert!(archive_upload_too_large_message(100).contains("100"));
        let _ = PathBuf::from(".");
    }
}
