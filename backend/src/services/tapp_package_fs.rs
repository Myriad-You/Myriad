//! Tapp package filesystem pure rules (lifecycle artifacts, orphans, errors).
//!
//! Path naming, orphan selection helpers, and permission-class IO error
//! messaging live here so install/uninstall recovery does not own contract
//! strings in the API layer. Handlers still perform directory IO.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::services::tapp_validation::{validate_resource_path, validate_tapp_id};

/// Install generation marker written next to `manifest.json`.
pub const TAPP_INSTALL_STATE_FILE: &str = ".myriad-install-state.json";

/// Canonical manifest filename inside an install directory.
pub const MANIFEST_JSON: &str = "manifest.json";

/// Lifecycle artifact kind suffixes (without leading/trailing separators).
pub const LIFECYCLE_ARTIFACT_KINDS: &[&str] =
    &["staging", "backup", "uninstall", "recovery-discard"];

/// Directory name prefixes for lifecycle artifacts of one live install name.
///
/// Example for `com.example.app`:
/// - `.com.example.app.staging-`
/// - `.com.example.app.backup-`
/// - `.com.example.app.uninstall-`
/// - `.com.example.app.recovery-discard-`
pub fn lifecycle_artifact_prefixes(tapp_dir_name: &str) -> [String; 4] {
    std::array::from_fn(|index| {
        format!(".{tapp_dir_name}.{}-", LIFECYCLE_ARTIFACT_KINDS[index])
    })
}

/// Build a lifecycle artifact directory name for `tapp_dir_name` + kind + nonce.
pub fn lifecycle_artifact_dir_name(tapp_dir_name: &str, kind: &str, nonce: &str) -> String {
    format!(".{tapp_dir_name}.{kind}-{nonce}")
}

/// Whether `filename` is a lifecycle artifact for the live install basename.
pub fn is_lifecycle_artifact_filename(filename: &str, tapp_dir_name: &str) -> bool {
    lifecycle_artifact_prefixes(tapp_dir_name)
        .iter()
        .any(|prefix| filename.starts_with(prefix.as_str()))
}

/// Whether a lifecycle filename refers to a staging directory.
pub fn is_staging_artifact_filename(filename: &str) -> bool {
    filename.contains(".staging-")
}

/// Parse the Tapp id from a lifecycle artifact filename (e.g.
/// `.com.example.app.staging-<32hex>`).
///
/// Returns `None` when the name is not a recognized lifecycle artifact or the
/// extracted id fails [`validate_tapp_id`].
pub fn lifecycle_artifact_tapp_id(filename: &str) -> Option<&str> {
    let stem = filename.strip_prefix('.')?;
    let (prefix, nonce) = stem.rsplit_once('-')?;
    if nonce.len() != 32 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    [".staging", ".backup", ".uninstall", ".recovery-discard"]
        .into_iter()
        .find_map(|kind| prefix.strip_suffix(kind))
        .filter(|tapp_id| validate_tapp_id(tapp_id).is_ok())
}

/// Marker files that identify a Tapp install directory.
pub fn tapp_installation_marker_names() -> [&'static str; 2] {
    [MANIFEST_JSON, TAPP_INSTALL_STATE_FILE]
}

/// Whether `paths` from reinstall-orphan enumeration indicate leftover state.
pub fn has_reinstall_orphan_state(paths: &[PathBuf]) -> bool {
    !paths.is_empty()
}

/// Skip removing a candidate when it is the active staging directory.
///
/// Used by reinstall orphan cleanup under the lifecycle lock so the current
/// `TappDirStage` path is never deleted mid-install.
pub fn should_preserve_orphan_path(candidate: &Path, preserve: Option<&Path>) -> bool {
    preserve.is_some_and(|keep| keep == candidate)
}

/// Permission / read-only volume failures that should surface as 503.
pub fn is_storage_unwritable_error(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem
    )
}

/// Whether install handlers should probe uid/gid when logging FS failures.
///
/// Same gate as unwritable storage: only permission/read-only errors gain
/// ownership context (avoids noise on NotFound / other kinds).
pub fn should_log_filesystem_permission_context(kind: ErrorKind) -> bool {
    is_storage_unwritable_error(kind)
}

/// HTTP status hint for Tapp filesystem failures (503 vs 500).
pub fn filesystem_error_status_hint(kind: ErrorKind) -> u16 {
    if is_storage_unwritable_error(kind) {
        503
    } else {
        500
    }
}

/// Human-readable filesystem failure message preserved by the install API.
pub fn filesystem_error_message(action: &str, kind: ErrorKind, error_display: &str) -> String {
    if is_storage_unwritable_error(kind) {
        format!(
            "Tapp storage is not writable by the backend service account; repair the backend data volume ownership/permissions and retry ({action}: {error_display})"
        )
    } else {
        format!("{action}: {error_display}")
    }
}

/// JSON body for [`.myriad-install-state.json`](TAPP_INSTALL_STATE_FILE).
pub fn install_generation_payload(updated_at_micros: i64) -> Value {
    json!({ "updatedAtMicros": updated_at_micros })
}

/// Whether a generation marker payload matches the expected timestamp micros.
pub fn install_generation_matches_micros(value: &Value, expected_micros: i64) -> bool {
    value
        .get("updatedAtMicros")
        .and_then(Value::as_i64)
        == Some(expected_micros)
}

/// Join a validated relative resource path under `tapp_dir`.
pub fn resource_relative_path(tapp_dir: &Path, relative: &str) -> Result<PathBuf, String> {
    validate_resource_path(relative)?;
    Ok(tapp_dir.join(relative))
}

/// Archive entry → install path (same rules as resource paths; trailing `/` stripped).
pub fn archive_entry_relative_path(tapp_dir: &Path, entry_name: &str) -> Result<PathBuf, String> {
    let relative = entry_name.trim_end_matches('/');
    resource_relative_path(tapp_dir, relative)
}

// ── Orphan scan / code-path pure rules ──────────────────────────────────────

/// Parse a user-namespace directory name under `tapps/` (`"42"` → 42).
///
/// Rejects negatives, non-integers, and padded forms (`"042"`).
pub fn parse_tapp_owner_dir_name(name: &str) -> Option<i32> {
    let owner_id = name.parse::<i32>().ok()?;
    if owner_id < 0 || owner_id.to_string() != name {
        return None;
    }
    Some(owner_id)
}

/// How an entry under `tapps/{owner}/` is classified during orphan recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TappDirEntryClass {
    /// Lifecycle artifact (staging/backup/uninstall/recovery-discard) for a tapp id.
    LifecycleArtifact { tapp_id: String },
    /// Live install directory that looks like a Tapp package.
    LiveInstall { tapp_id: String },
}

/// Classify a directory name under an owner namespace for orphan cleanup.
///
/// `looks_like_installation` is provided by the IO layer (marker file presence).
pub fn classify_tapp_directory_entry(
    filename: &str,
    looks_like_installation: bool,
) -> Option<TappDirEntryClass> {
    if let Some(tapp_id) = lifecycle_artifact_tapp_id(filename) {
        return Some(TappDirEntryClass::LifecycleArtifact {
            tapp_id: tapp_id.to_string(),
        });
    }
    if validate_tapp_id(filename).is_ok() && looks_like_installation {
        return Some(TappDirEntryClass::LiveInstall {
            tapp_id: filename.to_string(),
        });
    }
    None
}

/// Tapp id associated with a classified entry (artifact or live).
pub fn tapp_id_from_dir_entry_class(class: &TappDirEntryClass) -> &str {
    match class {
        TappDirEntryClass::LifecycleArtifact { tapp_id }
        | TappDirEntryClass::LiveInstall { tapp_id } => tapp_id,
    }
}

/// Relative code-path candidates for runtime open, in preference order.
///
/// 1. Manifest `main` when present
/// 2. Legacy stored basename only when it is exactly `main.js` or `index.js`
/// (and not already covered by `main`)
pub fn preferred_code_path_candidates(
    manifest_main: Option<&str>,
    stored_code_path: &str,
) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(main) = manifest_main.filter(|value| !value.is_empty()) {
        candidates.push(main.to_string());
    }
    if let Some(filename) = Path::new(stored_code_path)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| matches!(*value, "main.js" | "index.js"))
    {
        if !candidates.iter().any(|path| path == filename) {
            candidates.push(filename.to_string());
        }
    }
    candidates
}

/// After canonicalize, ensure the resolved path is exactly `root/relative`.
///
/// Rejects symlink escapes where canonicalize lands outside or renames components.
pub fn sandbox_path_matches_relative(
    canonical_root: &Path,
    canonical_path: &Path,
    relative: &str,
) -> bool {
    canonical_path == canonical_root.join(relative)
}

// ── Interrupted lifecycle recovery decision table ───────────────────────────

/// Pure recovery plan for one live install directory + sibling artifacts.
///
/// Callers still perform FS probes (`directory_generation_matches`) and renames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryPlan {
    /// Live generation already matches DB; only delete leftover artifacts.
    DiscardArtifactsOnly,
    /// Promote `artifacts[source_index]` to the live path, then clean others.
    PromoteArtifact { source_index: usize },
    /// No matching generation found; leave the filesystem alone.
    NoOp,
}

/// Sort key so backup/uninstall quarantines sort before staging directories.
///
/// Lower keys come first (`sort_by_key`); staging is deprioritized as a recovery
/// source because backup/uninstall hold the pre-transaction generation.
pub fn recovery_artifact_sort_key(filename: &str) -> u8 {
    u8::from(is_staging_artifact_filename(filename))
}

/// Decide recovery action given live/artifact generation match flags.
///
/// `artifact_matches` must already be ordered with
/// [`recovery_artifact_sort_key`] (non-staging first). The first `true` wins.
pub fn plan_tapp_directory_recovery(
    live_matches: bool,
    artifact_matches: &[bool],
) -> RecoveryPlan {
    if live_matches {
        return RecoveryPlan::DiscardArtifactsOnly;
    }
    if let Some(source_index) = artifact_matches.iter().position(|matched| *matched) {
        return RecoveryPlan::PromoteArtifact { source_index };
    }
    RecoveryPlan::NoOp
}

/// Sort sibling lifecycle artifact paths by recovery priority (non-staging first).
pub fn sort_recovery_artifact_paths(artifacts: &mut [PathBuf]) {
    artifacts.sort_by_key(|path| {
        path.file_name()
            .and_then(|value| value.to_str())
            .map(recovery_artifact_sort_key)
            .unwrap_or(0)
    });
}

/// Directory name used to quarantine a live path while promoting a recovery source.
pub fn recovery_discard_artifact_name(tapp_dir_name: &str, nonce: &str) -> String {
    lifecycle_artifact_dir_name(tapp_dir_name, "recovery-discard", nonce)
}

/// Whether a recovery plan rewrites the live install directory.
///
/// `true` only for promote (caller increments recovered counters).
pub fn recovery_plan_mutates_live(plan: RecoveryPlan) -> bool {
    matches!(plan, RecoveryPlan::PromoteArtifact { .. })
}

/// Artifact paths to best-effort delete after a successful promote (all except source).
pub fn recovery_artifacts_to_remove_after_promote<'a>(
    artifacts: &'a [PathBuf],
    recovery_source: &Path,
) -> Vec<&'a PathBuf> {
    artifacts
        .iter()
        .filter(|path| path.as_path() != recovery_source)
        .collect()
}

/// Classify one owner-namespace directory entry as an orphan cleanup candidate.
///
/// Returns `(owner_id, tapp_id)` when the entry looks like a Tapp install or
/// lifecycle artifact that is **not** covered by a live DB install row.
pub fn orphan_tapp_key_if_unowned(
    owner_id: i32,
    filename: &str,
    looks_like_installation: bool,
    installed: &std::collections::HashSet<(i32, String)>,
) -> Option<(i32, String)> {
    let class = classify_tapp_directory_entry(filename, looks_like_installation)?;
    let tapp_id = tapp_id_from_dir_entry_class(&class).to_string();
    if installed.contains(&(owner_id, tapp_id.clone())) {
        None
    } else {
        Some((owner_id, tapp_id))
    }
}

/// Whether marker presence probes indicate a Tapp installation directory.
///
/// IO layer supplies `marker_exists(name)`; domain owns which names count.
pub fn looks_like_tapp_installation_from_markers(
    marker_exists: impl FnMut(&str) -> bool,
) -> bool {
    tapp_installation_marker_names()
        .into_iter()
        .any(marker_exists)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn lifecycle_prefixes_cover_all_kinds() {
        let prefixes = lifecycle_artifact_prefixes("com.example.app");
        assert_eq!(prefixes[0], ".com.example.app.staging-");
        assert_eq!(prefixes[1], ".com.example.app.backup-");
        assert_eq!(prefixes[2], ".com.example.app.uninstall-");
        assert_eq!(prefixes[3], ".com.example.app.recovery-discard-");
        assert!(is_lifecycle_artifact_filename(
            ".com.example.app.staging-deadbeefdeadbeefdeadbeefdeadbeef",
            "com.example.app"
        ));
        assert!(!is_lifecycle_artifact_filename(
            "com.example.other",
            "com.example.app"
        ));
        assert!(is_staging_artifact_filename(
            ".com.example.app.staging-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
        assert!(!is_staging_artifact_filename(
            ".com.example.app.backup-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
    }

    #[test]
    fn lifecycle_artifact_tapp_id_parses_valid_names() {
        let nonce = "a".repeat(32);
        let staging = format!(".com.example.app.staging-{nonce}");
        assert_eq!(
            lifecycle_artifact_tapp_id(&staging),
            Some("com.example.app")
        );
        let recovery = format!(".com.example.app.recovery-discard-{nonce}");
        assert_eq!(
            lifecycle_artifact_tapp_id(&recovery),
            Some("com.example.app")
        );
        assert!(lifecycle_artifact_tapp_id("com.example.app").is_none());
        assert!(lifecycle_artifact_tapp_id(&format!(".bad.staging-{}", "z".repeat(32))).is_none());
        // Nonce must be 32 hex chars.
        assert!(lifecycle_artifact_tapp_id(".com.example.app.staging-short").is_none());
    }

    #[test]
    fn has_reinstall_orphan_state_is_nonempty() {
        assert!(!has_reinstall_orphan_state(&[]));
        assert!(has_reinstall_orphan_state(&[PathBuf::from("/x")]));
    }

    #[test]
    fn filesystem_error_messages_match_api_contract() {
        let denied = filesystem_error_message("create staging", ErrorKind::PermissionDenied, "denied");
        assert!(denied.contains("storage is not writable"));
        assert!(denied.contains("ownership/permissions"));
        assert_eq!(
            filesystem_error_status_hint(ErrorKind::PermissionDenied),
            503
        );
        assert_eq!(
            filesystem_error_status_hint(ErrorKind::Other),
            500
        );
        let other = filesystem_error_message("activate", ErrorKind::Other, "boom");
        assert_eq!(other, "activate: boom");
    }

    #[test]
    fn install_generation_payload_roundtrip() {
        let payload = install_generation_payload(1_700_000_000_000_000);
        assert!(install_generation_matches_micros(
            &payload,
            1_700_000_000_000_000
        ));
        assert!(!install_generation_matches_micros(&payload, 1));
        assert!(!install_generation_matches_micros(&json!({}), 1));
    }

    #[test]
    fn resource_and_archive_paths_reject_escapes() {
        let root = Path::new("/data/tapps/1/com.example.app");
        assert_eq!(
            resource_relative_path(root, "page/state.js").unwrap(),
            root.join("page/state.js")
        );
        assert!(resource_relative_path(root, "../escape.js").is_err());
        assert_eq!(
            archive_entry_relative_path(root, "templates/widget-2x2.html/").unwrap(),
            root.join("templates/widget-2x2.html")
        );
        assert!(archive_entry_relative_path(root, "../outside.html").is_err());
    }

    #[test]
    fn lifecycle_artifact_dir_name_is_stable() {
        assert_eq!(
            lifecycle_artifact_dir_name("com.example.app", "staging", "deadbeef"),
            ".com.example.app.staging-deadbeef"
        );
    }

    #[test]
    fn installation_marker_names_include_manifest_and_state() {
        let markers = tapp_installation_marker_names();
        assert!(markers.contains(&MANIFEST_JSON));
        assert!(markers.contains(&TAPP_INSTALL_STATE_FILE));
    }

    #[test]
    fn orphan_preserve_and_permission_log_gates() {
        let staging = PathBuf::from("/data/tapps/1/.com.ex.staging-abc");
        let live = PathBuf::from("/data/tapps/1/com.ex");
        assert!(should_preserve_orphan_path(&staging, Some(&staging)));
        assert!(!should_preserve_orphan_path(&live, Some(&staging)));
        assert!(!should_preserve_orphan_path(&live, None));
        assert!(has_reinstall_orphan_state(&[live]));
        assert!(!has_reinstall_orphan_state(&[]));

        assert!(should_log_filesystem_permission_context(
            ErrorKind::PermissionDenied
        ));
        assert!(should_log_filesystem_permission_context(
            ErrorKind::ReadOnlyFilesystem
        ));
        assert!(!should_log_filesystem_permission_context(
            ErrorKind::NotFound
        ));
        assert!(is_storage_unwritable_error(ErrorKind::PermissionDenied));
    }

    #[test]
    fn parse_tapp_owner_dir_name_rejects_padding_and_negatives() {
        assert_eq!(parse_tapp_owner_dir_name("42"), Some(42));
        assert_eq!(parse_tapp_owner_dir_name("0"), Some(0));
        assert_eq!(parse_tapp_owner_dir_name("042"), None);
        assert_eq!(parse_tapp_owner_dir_name("-1"), None);
        assert_eq!(parse_tapp_owner_dir_name("user"), None);
        assert_eq!(parse_tapp_owner_dir_name(""), None);
    }

    #[test]
    fn classify_tapp_directory_entry_prefers_lifecycle_then_live() {
        let nonce = "b".repeat(32);
        let staging = format!(".com.example.app.staging-{nonce}");
        assert_eq!(
            classify_tapp_directory_entry(&staging, false),
            Some(TappDirEntryClass::LifecycleArtifact {
                tapp_id: "com.example.app".into()
            })
        );
        assert_eq!(
            classify_tapp_directory_entry("com.example.app", true),
            Some(TappDirEntryClass::LiveInstall {
                tapp_id: "com.example.app".into()
            })
        );
        // Valid id but no install markers → ignore (not an orphan candidate).
        assert_eq!(classify_tapp_directory_entry("com.example.app", false), None);
        assert_eq!(classify_tapp_directory_entry("not a tapp", true), None);
        assert_eq!(
            tapp_id_from_dir_entry_class(&TappDirEntryClass::LiveInstall {
                tapp_id: "x".into()
            }),
            "x"
        );
    }

    #[test]
    fn preferred_code_path_candidates_manifest_first_legacy_fallback() {
        assert_eq!(
            preferred_code_path_candidates(Some("src/main.js"), "/data/old/main.js"),
            vec!["src/main.js".to_string(), "main.js".to_string()]
        );
        // Same basename as main → no duplicate fallback.
        assert_eq!(
            preferred_code_path_candidates(Some("main.js"), "/data/old/main.js"),
            vec!["main.js".to_string()]
        );
        // Non-legacy stored path → no fallback.
        assert_eq!(
            preferred_code_path_candidates(None, "/data/old/custom.js"),
            Vec::<String>::new()
        );
        assert_eq!(
            preferred_code_path_candidates(None, "/data/old/index.js"),
            vec!["index.js".to_string()]
        );
    }

    #[test]
    fn sandbox_path_matches_relative_is_exact() {
        let root = Path::new("/data/tapps/1/com.example.app");
        assert!(sandbox_path_matches_relative(
            root,
            &root.join("page/a.js"),
            "page/a.js"
        ));
        assert!(!sandbox_path_matches_relative(
            root,
            Path::new("/elsewhere/page/a.js"),
            "page/a.js"
        ));
    }

    #[test]
    fn recovery_artifact_sort_key_deprioritizes_staging() {
        assert_eq!(
            recovery_artifact_sort_key(".com.example.app.backup-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            0
        );
        assert_eq!(
            recovery_artifact_sort_key(
                ".com.example.app.uninstall-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            ),
            0
        );
        assert_eq!(
            recovery_artifact_sort_key(".com.example.app.staging-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            1
        );
        let mut names = [".app.staging-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ".app.backup-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"];
        names.sort_by_key(|name| recovery_artifact_sort_key(name));
        assert!(names[0].contains(".backup-"));
        assert!(names[1].contains(".staging-"));
    }

    #[test]
    fn plan_tapp_directory_recovery_decision_table() {
        assert_eq!(
            plan_tapp_directory_recovery(true, &[false, true]),
            RecoveryPlan::DiscardArtifactsOnly
        );
        assert_eq!(
            plan_tapp_directory_recovery(false, &[false, true, false]),
            RecoveryPlan::PromoteArtifact { source_index: 1 }
        );
        // First match wins (non-staging should already be ordered first).
        assert_eq!(
            plan_tapp_directory_recovery(false, &[true, true]),
            RecoveryPlan::PromoteArtifact { source_index: 0 }
        );
        assert_eq!(
            plan_tapp_directory_recovery(false, &[false, false]),
            RecoveryPlan::NoOp
        );
        assert_eq!(plan_tapp_directory_recovery(false, &[]), RecoveryPlan::NoOp);

        assert!(!recovery_plan_mutates_live(RecoveryPlan::NoOp));
        assert!(!recovery_plan_mutates_live(
            RecoveryPlan::DiscardArtifactsOnly
        ));
        assert!(recovery_plan_mutates_live(RecoveryPlan::PromoteArtifact {
            source_index: 0
        }));
    }

    #[test]
    fn recovery_path_helpers_and_orphan_keys() {
        let mut artifacts = vec![
            PathBuf::from("/data/.app.staging-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            PathBuf::from("/data/.app.backup-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ];
        sort_recovery_artifact_paths(&mut artifacts);
        assert!(artifacts[0].to_string_lossy().contains(".backup-"));
        assert!(artifacts[1].to_string_lossy().contains(".staging-"));

        assert_eq!(
            recovery_discard_artifact_name("com.example.app", "deadbeef"),
            ".com.example.app.recovery-discard-deadbeef"
        );

        let source = PathBuf::from("/data/.app.backup-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let to_remove = recovery_artifacts_to_remove_after_promote(&artifacts, &source);
        assert_eq!(to_remove.len(), 1);
        assert!(to_remove[0].to_string_lossy().contains(".staging-"));

        let mut installed = std::collections::HashSet::new();
        installed.insert((1, "com.example.app".to_string()));
        assert!(orphan_tapp_key_if_unowned(1, "com.example.app", true, &installed).is_none());
        assert_eq!(
            orphan_tapp_key_if_unowned(1, "com.other.app", true, &installed),
            Some((1, "com.other.app".to_string()))
        );
        let nonce = "c".repeat(32);
        let staging = format!(".com.other.app.staging-{nonce}");
        assert_eq!(
            orphan_tapp_key_if_unowned(2, &staging, false, &installed),
            Some((2, "com.other.app".to_string()))
        );

        assert!(looks_like_tapp_installation_from_markers(|name| {
            name == MANIFEST_JSON
        }));
        assert!(!looks_like_tapp_installation_from_markers(|_| false));
    }
}
