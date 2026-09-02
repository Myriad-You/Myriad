//! Tapp package filesystem pure rules (lifecycle artifacts, orphans, errors).
//!
//! Path naming, orphan selection helpers, and permission-class IO error
//! messaging live here so install/uninstall recovery does not own contract
//! strings in the API layer. Handlers still perform directory IO.

pub use myriad_tapp_rules::{
    archive_entry_relative_path, classify_tapp_directory_entry, filesystem_error_message,
    filesystem_error_status_hint, has_reinstall_orphan_state, install_generation_matches_micros,
    install_generation_payload, is_lifecycle_artifact_filename, is_staging_artifact_filename,
    is_storage_unwritable_error, lifecycle_artifact_dir_name, lifecycle_artifact_prefixes,
    lifecycle_artifact_tapp_id, looks_like_tapp_installation_from_markers,
    orphan_tapp_key_if_unowned, parse_tapp_owner_dir_name, plan_tapp_directory_recovery,
    recovery_artifact_sort_key, recovery_artifacts_to_remove_after_promote,
    recovery_discard_artifact_name, recovery_plan_mutates_live, resource_relative_path,
    sandbox_path_matches_relative, should_log_filesystem_permission_context,
    should_preserve_orphan_path, sort_recovery_artifact_paths, tapp_id_from_dir_entry_class,
    tapp_installation_marker_names, RecoveryPlan, TappDirEntryClass, LIFECYCLE_ARTIFACT_KINDS,
    MANIFEST_JSON, TAPP_INSTALL_STATE_FILE,
};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::ErrorKind;
    use std::path::{Path, PathBuf};

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
        let denied =
            filesystem_error_message("create staging", ErrorKind::PermissionDenied, "denied");
        assert!(denied.contains("storage is not writable"));
        assert!(denied.contains("ownership/permissions"));
        assert_eq!(
            filesystem_error_status_hint(ErrorKind::PermissionDenied),
            503
        );
        assert_eq!(filesystem_error_status_hint(ErrorKind::Other), 500);
        let other = filesystem_error_message("activate", ErrorKind::Other, "boom");
        assert_eq!(other, "activate failed.");
        assert!(!denied.contains("denied"));
        assert!(denied.contains("create staging"));
        let full = filesystem_error_message(
            "create staging",
            ErrorKind::StorageFull,
            "No space left on device (os error 28)",
        );
        assert_eq!(full, "create staging: not enough disk space.");
        assert!(!full.contains("os error"));
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
        assert_eq!(
            classify_tapp_directory_entry("com.example.app", false),
            None
        );
        assert_eq!(classify_tapp_directory_entry("not a tapp", true), None);
        assert_eq!(
            tapp_id_from_dir_entry_class(&TappDirEntryClass::LiveInstall {
                tapp_id: "x".into()
            }),
            "x"
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
        let mut names = [
            ".app.staging-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ".app.backup-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ];
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
