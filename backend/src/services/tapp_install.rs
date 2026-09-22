//! Pure install/update request decisions for Tapp packages.
//!
//! HTTP handlers keep Claims, DB, filesystem, and role-config permission
//! filtering. Domain owns:
//! - install/update source mode parsing (`direct` | `store`)
//! - approved-permission selection (manifest ∩ request / previous)
//! - install/update DB column snapshots (paths, permissions JSON, default status)
//! - multipart `.tapp` upload field classification + archive size gate
//!
//! Canonical install-owner / conflict-owner namespaces live in
//! [`crate::services::tapp_ownership`].

/// Package provenance for install and update endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallSource {
    /// Client-supplied manifest + resources.
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

/// Select approved permissions for a **new** install.
///
/// Product contract (`InstallTappRequest.permissions`: 可选；缺省则批准全部声明权限; file-install omits the field → empty vec):
///
/// - Empty / omitted `requested` → **all** `manifest_permissions` (default full approval).
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
/// - `requested == Some([])` → **all** new manifest permissions (default full approval,
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

/// Select approved permissions for an **overwrite** (re-install of an existing id).
///
/// Keeps every previously approved permission the new manifest still declares,
/// and adds only the newly declared permissions the operator explicitly
/// accepted. Removed declarations drop out; a fresh declaration is never
/// silently approved.
pub fn select_overwrite_approved_permissions(
    manifest_permissions: &[String],
    accepted_new: &[String],
    previous_approved: &[String],
) -> Vec<String> {
    manifest_permissions
        .iter()
        .filter(|p| {
            previous_approved.iter().any(|r| r == *p) || accepted_new.iter().any(|r| r == *p)
        })
        .cloned()
        .collect()
}

/// Whether the installation owner namespace is the public site-owner row.
pub fn is_public_installation_namespace(installation_owner_id: i32, site_owner_id: i32) -> bool {
    installation_owner_id == site_owner_id
} // ── Persist path + column snapshots ─────────────────────────────────────────

use std::path::Path;

use chrono::{DateTime, FixedOffset};
use myriad_tapp_contract::manifest::TappManifest;

use crate::services::tapp_package_fs::MANIFEST_JSON;

/// Absolute path strings stored on the install row after activate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPathPair {
    /// `…/manifest.json` under the live install directory.
    pub file_path: String,
    /// Live path to the package's primary layer entry.
    ///
    /// Kept as a diagnostic/column value only. Serving resolves entries from
    /// the stored manifest layers, never from this column, so a package whose
    /// layers declare no JS entry falls back to the manifest path here.
    pub code_path: String,
}

/// Build `file_path` / `code_path` for a live install directory.
pub fn install_path_pair(final_tapp_dir: &Path, primary_entry: Option<&str>) -> InstallPathPair {
    let file_path = final_tapp_dir.join(MANIFEST_JSON);
    InstallPathPair {
        code_path: primary_entry
            .map(|entry| final_tapp_dir.join(entry))
            .unwrap_or_else(|| file_path.clone())
            .to_string_lossy()
            .into_owned(),
        file_path: file_path.to_string_lossy().into_owned(),
    }
}

/// Serialize optional manifest author for the `tapps.author` JSON column.
pub fn manifest_author_json(
    author: &Option<myriad_tapp_contract::manifest::TappAuthor>,
) -> Option<serde_json::Value> {
    author.as_ref().and_then(|a| serde_json::to_value(a).ok())
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
    pub approved_permissions: serde_json::Value,
    /// Re-authorization marker. Always `false` here: only the upgrade
    /// migration sets `true`, and every successful install/update is an
    /// explicit re-authorization that clears it.
    pub needs_reauthorization: bool,
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
    approved: &[String],
    final_tapp_dir: &Path,
    now: DateTime<FixedOffset>,
) -> Result<NewInstallPersist, String> {
    let paths = install_path_pair(final_tapp_dir, manifest.layer_entries().first().copied());
    let manifest_json = serde_json::to_value(manifest).map_err(|e| {
        tracing::error!(error = %e, "Failed to serialize manifest");
        "Failed to serialize manifest".to_string()
    })?;
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
        approved_permissions: serde_json::to_value(approved).unwrap_or_default(),
        needs_reauthorization: false,
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
    pub approved_permissions: serde_json::Value,
    /// Re-authorization marker. Always `false` here: only the upgrade
    /// migration sets `true`, and every successful install/update is an
    /// explicit re-authorization that clears it.
    pub needs_reauthorization: bool,
    pub code_path: String,
    pub updated_at: DateTime<FixedOffset>,
}

/// Project updated package + approvals into update DB columns.
pub fn build_update_install_persist(
    manifest: &TappManifest,
    approved: &[String],
    final_tapp_dir: &Path,
    now: DateTime<FixedOffset>,
) -> Result<UpdateInstallPersist, String> {
    let paths = install_path_pair(final_tapp_dir, manifest.layer_entries().first().copied());
    let manifest_json = serde_json::to_value(manifest).map_err(|e| {
        tracing::error!(error = %e, "Failed to serialize manifest");
        "Failed to serialize manifest".to_string()
    })?;
    Ok(UpdateInstallPersist {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        author: manifest_author_json(&manifest.author),
        icon: manifest.icon.clone(),
        theme_color: manifest.theme_color.clone(),
        manifest: manifest_json,
        approved_permissions: serde_json::to_value(approved).unwrap_or_default(),
        needs_reauthorization: false,
        code_path: paths.code_path,
        updated_at: now,
    })
}

// ── Multipart .tapp upload ──────────────────────────────────────────────────

/// Recognized multipart field names on POST /api/tapps/install-file.
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
pub fn archive_upload_would_exceed(current_len: usize, chunk_len: usize, max_bytes: usize) -> bool {
    current_len.saturating_add(chunk_len) > max_bytes
}

/// User-facing message when a `.tapp` upload exceeds the contract limit.
pub fn archive_upload_too_large_message(max_bytes: usize) -> String {
    format!(".tapp file exceeds {max_bytes} bytes")
}

// ── Concurrent install capacity ───────────────────────────────────

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
        assert_eq!(
            parse_install_source("direct").unwrap(),
            InstallSource::Direct
        );
        assert_eq!(parse_install_source("store").unwrap(), InstallSource::Store);
        assert_eq!(
            parse_install_source(" store ").unwrap(),
            InstallSource::Store
        );
        assert!(parse_install_source("").is_err());
        assert!(parse_install_source("file").is_err());
        assert_eq!(
            InvalidInstallSource.message(),
            "Invalid source, must be 'direct' or 'store'"
        );
        assert_eq!(InvalidInstallSource.status_hint(), 400);
    }

    #[test]
    fn install_concurrency_limits_are_generous_but_finite() {
        // a few concurrent installs, not unbounded; fail fast when full.
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
        let manifest = vec!["storage:read".into(), "network".into(), "ai".into()];
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
        let manifest = vec!["storage:read".into(), "network".into(), "ai".into()];
        let previous = vec!["storage:read".into(), "network".into(), "legacy".into()];

        // No request: keep previous that still exist in new manifest.
        assert_eq!(
            select_update_approved_permissions(&manifest, None, &previous),
            vec!["storage:read".to_string(), "network".to_string()]
        );
        // Empty list: default full approval of new manifest (same as install).
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
    fn overwrite_approved_permissions_keep_old_and_add_accepted_new() {
        let manifest = vec!["storage:read".into(), "network".into(), "ai".into()];
        let previous = vec!["storage:read".into(), "legacy".into()];

        // Only accepted new permissions are added on top of the kept old set.
        assert_eq!(
            select_overwrite_approved_permissions(&manifest, &["ai".into()], &previous),
            vec!["storage:read".to_string(), "ai".to_string()]
        );
        // Accepting nothing keeps only the overlap with the previous approvals.
        assert_eq!(
            select_overwrite_approved_permissions(&manifest, &[], &previous),
            vec!["storage:read".to_string()]
        );
        // Accepted names not declared by the new manifest never enter the set.
        assert_eq!(
            select_overwrite_approved_permissions(&manifest, &["gone".into()], &previous),
            vec!["storage:read".to_string()]
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
            "core": { "entry": "src/core.js" },
            "category": "utility",
            "permissions": ["storage:read"],
            "icon": "icon.png",
            "themeColor": "#abc",
            "author": { "name": "Ada" }
        }))
        .unwrap()
    }

    #[test]
    fn install_path_pair_joins_manifest_and_primary_entry() {
        let root = Path::new("/data/tapps/1/com.example.app");
        let paths = install_path_pair(root, Some("src/core.js"));
        assert_eq!(paths.file_path, root.join(MANIFEST_JSON).to_string_lossy());
        assert_eq!(paths.code_path, root.join("src/core.js").to_string_lossy());
    }

    /// 没有任何 JS 层入口的包（例如纯模板 Page）仍要有个非空 code_path。
    #[test]
    fn install_path_pair_falls_back_to_manifest_without_entry() {
        let root = Path::new("/data/tapps/1/com.example.app");
        let paths = install_path_pair(root, None);
        assert_eq!(paths.code_path, paths.file_path);
    }

    #[test]
    fn new_install_persist_starts_running_with_paths_and_perms() {
        let now = DateTime::parse_from_rfc3339("2026-01-02T03:04:05+00:00").unwrap();
        let snap = build_new_install_persist(
            &sample_manifest(),
            7,
            &["storage:read".into()],
            Path::new("/data/tapps/7/com.example.app"),
            now,
        )
        .unwrap();
        assert_eq!(snap.tapp_id, "com.example.app");
        assert_eq!(snap.user_id, 7);
        assert_eq!(snap.version, "1.2.3");
        assert!(snap.start_running);
        let root = Path::new("/data/tapps/7/com.example.app");
        assert_eq!(snap.file_path, root.join(MANIFEST_JSON).to_string_lossy());
        assert_eq!(snap.code_path, root.join("src/core.js").to_string_lossy());
        assert_eq!(snap.installed_at, now);
        assert_eq!(snap.last_run_at, now);
        assert_eq!(snap.approved_permissions, json!(["storage:read"]));
        assert!(
            !snap.needs_reauthorization,
            "new installs are never pre-flagged"
        );
        assert_eq!(snap.author.as_ref().unwrap()["name"], "Ada");
    }

    #[test]
    fn update_install_persist_refreshes_code_path_and_perms() {
        let now = DateTime::parse_from_rfc3339("2026-02-01T00:00:00+00:00").unwrap();
        let snap = build_update_install_persist(
            &sample_manifest(),
            &["storage:read".into()],
            Path::new("/data/tapps/1/com.example.app"),
            now,
        )
        .unwrap();
        assert_eq!(snap.version, "1.2.3");
        assert_eq!(
            snap.code_path,
            Path::new("/data/tapps/1/com.example.app")
                .join("src/core.js")
                .to_string_lossy()
        );
        assert_eq!(snap.updated_at, now);
        assert_eq!(snap.approved_permissions, json!(["storage:read"]));
        assert!(
            !snap.needs_reauthorization,
            "successful update re-authorizes the install"
        );
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
