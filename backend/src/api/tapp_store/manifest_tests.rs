use super::{
    append_directory_to_zip, archive_entry_path, canonical_installation_owner_id,
    cleanup_reinstall_orphans, fail_next_activation_rename, fail_next_activation_rename_with_kind,
    has_reinstall_orphan_state, installation_conflict_owner_ids, orphaned_tapp_directories,
    recover_tapp_directory, reinstall_orphan_paths, tapp_dir_for, tapp_filesystem_error_message,
    tapp_filesystem_error_status, tapp_setting_value_is_valid, uninstall_post_commit_cleanup_path,
    validate_asset_path, validate_installed_resources, validate_resource_path,
    validate_store_manifest_category, validate_tapp_archive, validate_tapp_id,
    validate_tapp_manifest, validate_widget_template_contents, widget_template_path,
    write_install_generation, RegisterWidgetRequest, TappCategory, TappDirStage, TappManifest,
    TappSettingDef, TappStorageAccess, TappWidgetCategory, TappWidgetDef, WidgetTemplateContents,
};
use crate::models::entities::{tapp_widgets, tapps};
use crate::services::permission_service::UserRole;
use axum::http::StatusCode;
use serde_json::json;
use std::path::PathBuf;

#[test]
fn permission_errors_return_actionable_service_unavailable() {
    let error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
    assert_eq!(
        tapp_filesystem_error_status(&error),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let message = tapp_filesystem_error_message("create staging", &error);
    assert!(message.contains("storage is not writable"));
    assert!(message.contains("ownership/permissions"));
}

use crate::services::tapp_lifecycle::{select_uninstall_target, UninstallTarget};

#[test]
fn every_admin_operates_the_canonical_public_owner_namespace() {
    assert_eq!(canonical_installation_owner_id(UserRole::Admin, 9, 1), 1);
    assert_eq!(canonical_installation_owner_id(UserRole::User, 9, 1), 9);
    assert_eq!(canonical_installation_owner_id(UserRole::Guest, -9, 1), -9);
}

#[test]
fn public_and_private_installations_only_conflict_with_their_own_namespace() {
    // Publishing cannot be blocked by another user's private copy.
    assert_eq!(
        installation_conflict_owner_ids(UserRole::Admin, 9, 1),
        vec![1]
    );
    assert_eq!(
        installation_conflict_owner_ids(UserRole::User, 42, 1),
        vec![42]
    );
    assert_eq!(
        installation_conflict_owner_ids(UserRole::Guest, -5, 1),
        vec![-5]
    );
}

#[test]
fn uninstall_prefers_own_install_when_public_coexists() {
    // Dual install: non-admin must hit own row (no admin required), not public 403 path.
    assert_eq!(
        select_uninstall_target(true, true),
        UninstallTarget::OwnInstall
    );
    // Private only.
    assert_eq!(
        select_uninstall_target(true, false),
        UninstallTarget::OwnInstall
    );
    // Public only: admin gate, then uninstall public.
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
fn uninstall_post_commit_prefers_quarantine_then_live_dir() {
    let live = PathBuf::from("/data/tapps/1/com.example.app");
    let quarantine = PathBuf::from("/data/tapps/1/.com.example.app.uninstall-deadbeef");

    // Rename succeeded: always clean quarantine, even if live path is gone.
    assert_eq!(
        uninstall_post_commit_cleanup_path(Some(quarantine.clone()), live.clone(), false),
        Some(quarantine.clone())
    );
    // Rename failed but live dir still present: best-effort delete live.
    assert_eq!(
        uninstall_post_commit_cleanup_path(None, live.clone(), true),
        Some(live.clone())
    );
    // Nothing on disk after commit: no filesystem work.
    assert_eq!(uninstall_post_commit_cleanup_path(None, live, false), None);
}

#[test]
fn reinstall_orphan_paths_selects_live_and_lifecycle_artifacts() {
    let root = std::env::temp_dir().join(format!(
        "myriad-tapp-reinstall-orphan-select-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let live = root.join("com.example.app");
    let staging = root.join(format!(
        ".com.example.app.staging-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let backup = root.join(format!(
        ".com.example.app.backup-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let uninstall = root.join(format!(
        ".com.example.app.uninstall-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let recovery = root.join(format!(
        ".com.example.app.recovery-discard-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let unrelated = root.join("com.example.other");
    for directory in [&live, &staging, &backup, &uninstall, &recovery, &unrelated] {
        std::fs::create_dir_all(directory).unwrap();
    }

    let selected = reinstall_orphan_paths(&live).unwrap();
    let selected_set = selected
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    assert!(has_reinstall_orphan_state(
        &selected_set.iter().cloned().collect::<Vec<_>>()
    ));
    assert_eq!(
        selected_set,
        std::collections::HashSet::from([live.clone(), staging, backup, uninstall, recovery])
    );
    assert!(!selected_set.contains(&unrelated));

    // Empty owner dir → no orphan state.
    let empty = root.join("missing-app");
    assert!(!has_reinstall_orphan_state(
        &reinstall_orphan_paths(&empty).unwrap()
    ));

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cleanup_reinstall_orphans_removes_live_and_artifacts_preserving_staging() {
    let root = std::env::temp_dir().join(format!(
        "myriad-tapp-reinstall-orphan-clean-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let live = root.join("com.example.app");
    let preserve = root.join(format!(
        ".com.example.app.staging-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let leftover_staging = root.join(format!(
        ".com.example.app.staging-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let uninstall = root.join(format!(
        ".com.example.app.uninstall-{}",
        uuid::Uuid::new_v4().simple()
    ));
    for directory in [&live, &preserve, &leftover_staging, &uninstall] {
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join("marker"), "x").unwrap();
    }

    let removed = cleanup_reinstall_orphans(&live, "com.example.app", 1, 1, Some(&preserve));
    assert!(removed >= 3);
    assert!(!live.exists());
    assert!(!leftover_staging.exists());
    assert!(!uninstall.exists());
    assert!(preserve.exists());
    assert!(!has_reinstall_orphan_state(
        &reinstall_orphan_paths(&live)
            .unwrap()
            .into_iter()
            .filter(|path| path != &preserve)
            .collect::<Vec<_>>()
    ));

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn activate_replaces_a_cleaned_or_renameable_live_directory() {
    let root = std::env::temp_dir().join(format!(
        "myriad-tapp-activate-orphan-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let live = root.join("com.example.app");
    tokio::fs::create_dir_all(&live).await.unwrap();
    tokio::fs::write(live.join("main.js"), "orphan")
        .await
        .unwrap();

    // Pre-activate cleanup path selection (install uses this before staging).
    let orphans = reinstall_orphan_paths(&live).unwrap();
    assert!(has_reinstall_orphan_state(&orphans));
    cleanup_reinstall_orphans(&live, "com.example.app", 1, 1, None);
    assert!(!live.exists());

    let stage = TappDirStage::create(&live).await.unwrap();
    tokio::fs::write(stage.path().join("main.js"), "fresh")
        .await
        .unwrap();
    stage.activate(&live).await.unwrap().commit().await;
    assert_eq!(
        tokio::fs::read_to_string(live.join("main.js"))
            .await
            .unwrap(),
        "fresh"
    );

    // activate also replaces an existing live dir via backup rename.
    let stage2 = TappDirStage::create(&live).await.unwrap();
    tokio::fs::write(stage2.path().join("main.js"), "updated")
        .await
        .unwrap();
    stage2.activate(&live).await.unwrap().commit().await;
    assert_eq!(
        tokio::fs::read_to_string(live.join("main.js"))
            .await
            .unwrap(),
        "updated"
    );
    assert!(super::lifecycle_artifact_directories(&live)
        .unwrap()
        .is_empty());

    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn backup_rename_failure_preserves_the_last_known_good_live_version() {
    let root = std::env::temp_dir().join(format!(
        "myriad-tapp-backup-rename-failure-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let live = root.join("com.example.app");
    tokio::fs::create_dir_all(&live).await.unwrap();
    tokio::fs::write(live.join("main.js"), "known-good")
        .await
        .unwrap();
    let stage = TappDirStage::create(&live).await.unwrap();
    let stage_path = stage.path().to_path_buf();
    tokio::fs::write(stage.path().join("main.js"), "candidate")
        .await
        .unwrap();

    fail_next_activation_rename(&live);
    let error = match stage.activate(&live).await {
        Ok(_) => panic!("backup rename failure must abort activation"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), std::io::ErrorKind::Other);
    assert_eq!(
        tokio::fs::read_to_string(live.join("main.js"))
            .await
            .unwrap(),
        "known-good"
    );
    assert!(!stage_path.exists());
    assert!(super::lifecycle_artifact_directories(&live)
        .unwrap()
        .is_empty());
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn staging_rename_failure_restores_the_previous_live_version() {
    let root = std::env::temp_dir().join(format!(
        "myriad-tapp-stage-rename-failure-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let live = root.join("com.example.app");
    tokio::fs::create_dir_all(&live).await.unwrap();
    tokio::fs::write(live.join("main.js"), "known-good")
        .await
        .unwrap();
    let stage = TappDirStage::create(&live).await.unwrap();
    let stage_path = stage.path().to_path_buf();
    tokio::fs::write(stage.path().join("main.js"), "candidate")
        .await
        .unwrap();

    fail_next_activation_rename(&stage_path);
    let error = match stage.activate(&live).await {
        Ok(_) => panic!("staging rename failure must abort activation"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), std::io::ErrorKind::Other);
    assert_eq!(
        tokio::fs::read_to_string(live.join("main.js"))
            .await
            .unwrap(),
        "known-good"
    );
    assert!(!stage_path.exists());
    assert!(super::lifecycle_artifact_directories(&live)
        .unwrap()
        .is_empty());
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn storage_full_during_candidate_switch_restores_the_previous_live_version() {
    let root = std::env::temp_dir().join(format!(
        "myriad-tapp-stage-storage-full-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let live = root.join("com.example.app");
    tokio::fs::create_dir_all(&live).await.unwrap();
    tokio::fs::write(live.join("main.js"), "known-good")
        .await
        .unwrap();
    let stage = TappDirStage::create(&live).await.unwrap();
    let stage_path = stage.path().to_path_buf();
    tokio::fs::write(stage.path().join("main.js"), "candidate")
        .await
        .unwrap();

    fail_next_activation_rename_with_kind(&stage_path, std::io::ErrorKind::StorageFull);
    let error = match stage.activate(&live).await {
        Ok(_) => panic!("storage exhaustion must abort activation"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), std::io::ErrorKind::StorageFull);
    assert_eq!(
        tokio::fs::read_to_string(live.join("main.js"))
            .await
            .unwrap(),
        "known-good"
    );
    assert!(!stage_path.exists());
    assert!(super::lifecycle_artifact_directories(&live)
        .unwrap()
        .is_empty());
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn rollback_restore_failure_preserves_both_generations_for_startup_recovery() {
    let root = std::env::temp_dir().join(format!(
        "myriad-tapp-rollback-restore-failure-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let live = root.join("com.example.app");
    let old_manifest = json!({
        "id": "com.example.app",
        "version": "1.0.0",
        "main": "main.js"
    });
    let new_manifest = json!({
        "id": "com.example.app",
        "version": "2.0.0",
        "main": "main.js"
    });
    let old_generation = chrono::Utc::now().fixed_offset();
    let new_generation = old_generation + chrono::Duration::seconds(1);

    tokio::fs::create_dir_all(&live).await.unwrap();
    tokio::fs::write(live.join("main.js"), "known-good")
        .await
        .unwrap();
    tokio::fs::write(
        live.join("manifest.json"),
        serde_json::to_vec(&old_manifest).unwrap(),
    )
    .await
    .unwrap();
    write_install_generation(&live, old_generation).unwrap();

    let stage = TappDirStage::create(&live).await.unwrap();
    tokio::fs::write(stage.path().join("main.js"), "candidate")
        .await
        .unwrap();
    tokio::fs::write(
        stage.path().join("manifest.json"),
        serde_json::to_vec(&new_manifest).unwrap(),
    )
    .await
    .unwrap();
    write_install_generation(stage.path(), new_generation).unwrap();
    let activated = stage.activate(&live).await.unwrap();
    let backup = activated.backup_path.clone().unwrap();

    fail_next_activation_rename(&backup);
    activated.rollback().await;

    assert_eq!(
        tokio::fs::read_to_string(live.join("main.js"))
            .await
            .unwrap(),
        "candidate"
    );
    assert_eq!(
        tokio::fs::read_to_string(backup.join("main.js"))
            .await
            .unwrap(),
        "known-good"
    );

    assert!(recover_tapp_directory(&live, &old_manifest, old_generation).unwrap());
    assert_eq!(
        tokio::fs::read_to_string(live.join("main.js"))
            .await
            .unwrap(),
        "known-good"
    );
    assert!(super::lifecycle_artifact_directories(&live)
        .unwrap()
        .is_empty());
    assert!(!recover_tapp_directory(&live, &old_manifest, old_generation).unwrap());

    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn ambiguous_database_commit_can_recover_the_preserved_candidate_generation() {
    let root = std::env::temp_dir().join(format!(
        "myriad-tapp-ambiguous-commit-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let live = root.join("com.example.app");
    let old_manifest = json!({
        "id": "com.example.app",
        "version": "1.0.0",
        "main": "main.js"
    });
    let new_manifest = json!({
        "id": "com.example.app",
        "version": "2.0.0",
        "main": "main.js"
    });
    let old_generation = chrono::Utc::now().fixed_offset();
    let new_generation = old_generation + chrono::Duration::seconds(1);

    tokio::fs::create_dir_all(&live).await.unwrap();
    tokio::fs::write(live.join("main.js"), "known-good")
        .await
        .unwrap();
    tokio::fs::write(
        live.join("manifest.json"),
        serde_json::to_vec(&old_manifest).unwrap(),
    )
    .await
    .unwrap();
    write_install_generation(&live, old_generation).unwrap();

    let stage = TappDirStage::create(&live).await.unwrap();
    tokio::fs::write(stage.path().join("main.js"), "candidate")
        .await
        .unwrap();
    tokio::fs::write(
        stage.path().join("manifest.json"),
        serde_json::to_vec(&new_manifest).unwrap(),
    )
    .await
    .unwrap();
    write_install_generation(stage.path(), new_generation).unwrap();
    let activated = stage.activate(&live).await.unwrap();

    // Simulate the caller receiving an error from COMMIT: rollback restores
    // the old live path but must retain the candidate until DB truth is known.
    activated.rollback_after_commit_error().await;
    assert_eq!(
        tokio::fs::read_to_string(live.join("main.js"))
            .await
            .unwrap(),
        "known-good"
    );
    assert_eq!(
        super::lifecycle_artifact_directories(&live).unwrap().len(),
        1
    );

    // If COMMIT actually succeeded, startup reconciliation promotes the
    // preserved candidate selected by its generation marker.
    assert!(recover_tapp_directory(&live, &new_manifest, new_generation).unwrap());
    assert_eq!(
        tokio::fs::read_to_string(live.join("main.js"))
            .await
            .unwrap(),
        "candidate"
    );
    assert!(super::lifecycle_artifact_directories(&live)
        .unwrap()
        .is_empty());
    assert!(!recover_tapp_directory(&live, &new_manifest, new_generation).unwrap());

    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn install_can_activate_when_orphan_contents_cannot_be_deleted() {
    use std::os::unix::fs::PermissionsExt;

    let root = std::env::temp_dir().join(format!(
        "myriad-tapp-permission-orphan-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let live = root.join("com.example.app");
    let protected = live.join("protected");
    tokio::fs::create_dir_all(&protected).await.unwrap();
    tokio::fs::write(protected.join("root-owned.js"), "orphan")
        .await
        .unwrap();
    tokio::fs::set_permissions(&protected, std::fs::Permissions::from_mode(0o000))
        .await
        .unwrap();

    // Recursive cleanup cannot traverse the old contents, but that must
    // not abort installation: activate only needs rename permission on
    // the owner directory to quarantine the occupied live path.
    assert_eq!(
        cleanup_reinstall_orphans(&live, "com.example.app", 1, 1, None),
        0
    );
    assert!(live.exists());

    let stage = TappDirStage::create(&live).await.unwrap();
    tokio::fs::write(stage.path().join("main.js"), "fresh")
        .await
        .unwrap();
    let activated = stage.activate(&live).await.unwrap();
    let backup = activated.backup_path.clone().unwrap();
    assert_eq!(
        tokio::fs::read_to_string(live.join("main.js"))
            .await
            .unwrap(),
        "fresh"
    );

    // Restore permissions only so the test can verify deferred cleanup.
    tokio::fs::set_permissions(
        backup.join("protected"),
        std::fs::Permissions::from_mode(0o700),
    )
    .await
    .unwrap();
    activated.commit().await;
    assert!(!backup.exists());

    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[test]
fn private_storage_follows_subject_while_settings_follow_installation() {
    let viewer_of_admin = TappStorageAccess::from_owner_and_subject(1, 42);
    let second_viewer = TappStorageAccess::from_owner_and_subject(1, 43);
    assert_eq!(viewer_of_admin.private_storage_namespace(), 42);
    assert_eq!(second_viewer.private_storage_namespace(), 43);
    assert_eq!(viewer_of_admin.installation_namespace(), 1);
    assert_eq!(second_viewer.installation_namespace(), 1);
    assert!(!viewer_of_admin.can_manage_installation());
    assert!(viewer_of_admin.require_installation_write().is_err());

    let private_owner = TappStorageAccess::from_owner_and_subject(42, 42);
    assert_eq!(private_owner.private_storage_namespace(), 42);
    assert_eq!(private_owner.installation_namespace(), 42);
    assert!(private_owner.can_manage_installation());
    assert!(private_owner.require_installation_write().is_ok());

    let site_owner = TappStorageAccess::from_owner_and_subject(1, 1);
    assert_eq!(site_owner.private_storage_namespace(), 1);
    assert_eq!(site_owner.installation_namespace(), 1);
    assert!(site_owner.can_manage_installation());
}

#[test]
fn installation_settings_allow_owner_or_current_admin_only() {
    let public_viewer = TappStorageAccess::from_owner_and_subject(1, 42);
    assert!(!super::can_write_installation_settings(
        public_viewer,
        false
    ));
    assert!(super::can_write_installation_settings(public_viewer, true));

    let private_owner = TappStorageAccess::from_owner_and_subject(42, 42);
    assert!(super::can_write_installation_settings(private_owner, false));
}

#[test]
fn sandbox_storage_rejects_host_managed_key_prefixes() {
    for key in [
        "_settings.theme",
        "_component:theme:midnight",
        "_shortcut:open",
        "_report:weekly",
    ] {
        assert!(super::validate_sandbox_storage_key(key).is_err(), "{key}");
    }
    assert!(super::validate_sandbox_storage_key("user.preferences").is_ok());
}

#[test]
fn preserves_background_requirements_during_manifest_round_trip() {
    let manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.background",
        "name": "Background app",
        "version": "1.0.0",
        "main": "main.js",
        "permissions": ["scheduler:register"],
        "backgroundRequirements": ["scheduler", "sync"]
    }))
    .expect("manifest should deserialize");

    let value = serde_json::to_value(manifest).expect("manifest should serialize");
    assert_eq!(
        value["backgroundRequirements"],
        json!(["scheduler", "sync"])
    );
}

#[tokio::test]
async fn staged_directory_rollback_restores_previous_install() {
    let root = std::env::temp_dir().join(format!(
        "myriad-tapp-stage-rollback-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let live = root.join("com.example.app");
    tokio::fs::create_dir_all(&live).await.unwrap();
    tokio::fs::write(live.join("main.js"), "old").await.unwrap();

    let stage = TappDirStage::create(&live).await.unwrap();
    tokio::fs::write(stage.path().join("main.js"), "new")
        .await
        .unwrap();
    let activated = stage.activate(&live).await.unwrap();
    assert_eq!(
        tokio::fs::read_to_string(live.join("main.js"))
            .await
            .unwrap(),
        "new"
    );

    activated.rollback().await;
    assert_eq!(
        tokio::fs::read_to_string(live.join("main.js"))
            .await
            .unwrap(),
        "old"
    );
    assert!(super::lifecycle_artifact_directories(&live)
        .unwrap()
        .is_empty());
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn staged_directory_commit_keeps_only_new_install() {
    let root = std::env::temp_dir().join(format!(
        "myriad-tapp-stage-commit-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let live = root.join("com.example.app");
    tokio::fs::create_dir_all(&live).await.unwrap();
    tokio::fs::write(live.join("main.js"), "old").await.unwrap();

    let stage = TappDirStage::create(&live).await.unwrap();
    tokio::fs::write(stage.path().join("main.js"), "new")
        .await
        .unwrap();
    stage.activate(&live).await.unwrap().commit().await;

    assert_eq!(
        tokio::fs::read_to_string(live.join("main.js"))
            .await
            .unwrap(),
        "new"
    );
    let mut entries = tokio::fs::read_dir(&root).await.unwrap();
    let mut names = Vec::new();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        names.push(entry.file_name().to_string_lossy().to_string());
    }
    assert_eq!(names, vec!["com.example.app"]);
    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[tokio::test]
async fn startup_recovery_restores_database_generation_after_interrupted_update() {
    let root = std::env::temp_dir().join(format!(
        "myriad-tapp-stage-recovery-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let live = root.join("com.example.app");
    let old_manifest = json!({
        "id": "com.example.app",
        "version": "1.0.0",
        "main": "main.js"
    });
    let new_manifest = json!({
        "id": "com.example.app",
        "version": "1.0.0",
        "main": "main.js"
    });
    let old_generation = chrono::Utc::now().fixed_offset();
    let new_generation = old_generation + chrono::Duration::seconds(1);

    tokio::fs::create_dir_all(&live).await.unwrap();
    tokio::fs::write(live.join("main.js"), "old").await.unwrap();
    tokio::fs::write(
        live.join("manifest.json"),
        serde_json::to_vec(&old_manifest).unwrap(),
    )
    .await
    .unwrap();
    write_install_generation(&live, old_generation).unwrap();

    let stage = TappDirStage::create(&live).await.unwrap();
    tokio::fs::write(stage.path().join("main.js"), "new")
        .await
        .unwrap();
    tokio::fs::write(
        stage.path().join("manifest.json"),
        serde_json::to_vec(&new_manifest).unwrap(),
    )
    .await
    .unwrap();
    write_install_generation(stage.path(), new_generation).unwrap();
    let _interrupted = stage.activate(&live).await.unwrap();

    assert_eq!(
        tokio::fs::read_to_string(live.join("main.js"))
            .await
            .unwrap(),
        "new"
    );
    assert!(recover_tapp_directory(&live, &old_manifest, old_generation).unwrap());
    assert_eq!(
        tokio::fs::read_to_string(live.join("main.js"))
            .await
            .unwrap(),
        "old"
    );
    assert!(super::lifecycle_artifact_directories(&live)
        .unwrap()
        .is_empty());
    assert!(!recover_tapp_directory(&live, &old_manifest, old_generation).unwrap());

    tokio::fs::remove_dir_all(root).await.unwrap();
}

#[test]
fn startup_recovery_can_restore_an_interrupted_recovery_discard() {
    let root = std::env::temp_dir().join(format!(
        "myriad-tapp-recovery-discard-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let live = root.join("com.example.app");
    let discard = root.join(format!(
        ".com.example.app.recovery-discard-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let manifest = json!({
        "id": "com.example.app",
        "version": "1.0.0",
        "main": "main.js"
    });
    let expected_generation = chrono::Utc::now().fixed_offset();
    std::fs::create_dir_all(&live).unwrap();
    std::fs::create_dir_all(&discard).unwrap();
    std::fs::write(live.join("main.js"), "incomplete").unwrap();
    std::fs::write(discard.join("main.js"), "expected").unwrap();
    std::fs::write(
        discard.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    write_install_generation(&discard, expected_generation).unwrap();

    assert!(recover_tapp_directory(&live, &manifest, expected_generation).unwrap());
    assert_eq!(
        std::fs::read_to_string(live.join("main.js")).unwrap(),
        "expected"
    );
    assert!(!discard.exists());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn startup_recovery_discovers_only_unowned_live_and_artifact_directories() {
    let root = std::env::temp_dir().join(format!(
        "myriad-tapp-orphan-discovery-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let owner = root.join("1");
    let keep = owner.join("com.example.keep");
    let orphan = owner.join("com.example.orphan");
    let orphan_artifact = owner.join(format!(
        ".com.example.orphan.uninstall-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let keep_artifact = owner.join(format!(
        ".com.example.keep.recovery-discard-{}",
        uuid::Uuid::new_v4().simple()
    ));
    for directory in [&keep, &orphan, &orphan_artifact, &keep_artifact] {
        std::fs::create_dir_all(directory).unwrap();
    }
    std::fs::write(keep.join("manifest.json"), "{}").unwrap();
    std::fs::write(orphan.join("manifest.json"), "{}").unwrap();

    let installed = std::collections::HashSet::from([(1, "com.example.keep".to_string())]);
    let candidates = orphaned_tapp_directories(&root, &installed).unwrap();
    let candidate_paths = candidates
        .into_iter()
        .map(|(_, _, path)| path)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        candidate_paths,
        std::collections::HashSet::from([orphan, orphan_artifact])
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn preserves_and_validates_data_exchange_during_manifest_round_trip() {
    let manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.exchange",
        "name": "Exchange app",
        "version": "1.0.0",
        "main": "main.js",
        "category": "utility",
        "permissions": ["storage:read"],
        "dataExchange": {
            "exports": [{
                "id": "playlist.current",
                "description": "Current playlist",
                "maxBytes": 262144,
                "maxRecords": 200,
                "schema": {
                    "type": "array",
                    "items": { "type": "string" }
                }
            }],
            "imports": [{
                "tappId": "com.example.player",
                "exportId": "playlist.current"
            }]
        }
    }))
    .expect("manifest should deserialize");

    validate_tapp_manifest(&manifest).expect("exchange declaration should validate");
    let value = serde_json::to_value(manifest).expect("manifest should serialize");
    assert_eq!(
        value["dataExchange"]["exports"][0]["id"],
        "playlist.current"
    );
    assert_eq!(
        value["dataExchange"]["imports"][0]["tappId"],
        "com.example.player"
    );
}

#[test]
fn validates_manifest_locales_overrides() {
    let manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.i18n",
        "name": "我的应用",
        "version": "1.0.0",
        "main": "main.js",
        "category": "utility",
        "description": "中文描述",
        "locales": {
            "en-US": { "name": "My App", "description": "English description" },
            "ja-JP": { "description": "日本語の説明" }
        }
    }))
    .expect("manifest should deserialize");
    validate_tapp_manifest(&manifest).expect("locales declaration should validate");
    let value = serde_json::to_value(manifest).expect("manifest should serialize");
    assert_eq!(value["locales"]["en-US"]["name"], "My App");

    let bad_tag: TappManifest = serde_json::from_value(json!({
        "id": "com.example.i18n",
        "name": "App",
        "version": "1.0.0",
        "main": "main.js",
        "category": "utility",
        "locales": { "not a tag": { "name": "X" } }
    }))
    .expect("manifest should deserialize");
    let error = validate_tapp_manifest(&bad_tag).expect_err("invalid tag should fail");
    assert!(error.contains("BCP-47"));

    let blank_name: TappManifest = serde_json::from_value(json!({
        "id": "com.example.i18n",
        "name": "App",
        "version": "1.0.0",
        "main": "main.js",
        "category": "utility",
        "locales": { "en-US": { "name": "   " } }
    }))
    .expect("manifest should deserialize");
    let error = validate_tapp_manifest(&blank_name).expect_err("blank override should fail");
    assert!(error.contains("locales['en-US'].name"));

    assert!(serde_json::from_value::<TappManifest>(json!({
        "id": "com.example.i18n",
        "name": "App",
        "version": "1.0.0",
        "main": "main.js",
        "category": "utility",
        "locales": { "en-US": { "name": "X", "unknown": true } }
    }))
    .is_err());
}

#[test]
fn preserves_and_validates_ai_contract_during_manifest_round_trip() {
    let manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.ai",
        "name": "AI app",
        "version": "1.0.0",
        "main": "main.js",
        "category": "ai",
        "permissions": ["ai:generate", "platform:read"],
        "ai": {
            "protocolVersion": 2,
            "operations": ["generate"],
            "modelTier": "standard",
            "contextSources": ["platform", "custom"],
            "outputFormats": ["text", "json"]
        }
    }))
    .expect("manifest should deserialize");

    validate_tapp_manifest(&manifest).expect("AI declaration should validate");
    let value = serde_json::to_value(manifest).expect("manifest should serialize");
    assert_eq!(value["ai"]["protocolVersion"], 2);
    assert_eq!(value["ai"]["operations"], json!(["generate"]));
}

#[test]
fn ai_builtin_requires_matching_manifest_declaration() {
    let base = json!({
        "id": "com.example.ai-builtin",
        "name": "AI builtin app",
        "version": "1.0.0",
        "main": "main.js",
        "category": "ai",
        "permissions": ["ai:generate"],
        "apis": {
            "summary": {
                "type": "builtin",
                "builtin": "ai:generate"
            }
        }
    });
    let missing: TappManifest = serde_json::from_value(base.clone()).unwrap();
    assert!(validate_tapp_manifest(&missing).is_err());

    let mut declared = base;
    declared["ai"] = json!({
        "protocolVersion": 2,
        "operations": ["generate"],
        "modelTier": "standard",
        "contextSources": [],
        "outputFormats": ["text"]
    });
    let declared: TappManifest = serde_json::from_value(declared).unwrap();
    validate_tapp_manifest(&declared).unwrap();
}

#[test]
fn rejects_ai_operation_without_matching_permission() {
    let manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.ai",
        "name": "AI app",
        "version": "1.0.0",
        "main": "main.js",
        "category": "ai",
        "permissions": [],
        "ai": {
            "protocolVersion": 2,
            "operations": ["chat"],
            "modelTier": "standard",
            "contextSources": [],
            "outputFormats": ["text"]
        }
    }))
    .expect("manifest should deserialize");

    assert!(validate_tapp_manifest(&manifest).is_err());
}

#[test]
fn rejects_removed_media_control_with_replacement_names() {
    let manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.legacy-media",
        "name": "Legacy media app",
        "version": "1.0.0",
        "main": "main.js",
        "category": "utility",
        "permissions": ["media:control"]
    }))
    .expect("manifest should deserialize");

    let error = validate_tapp_manifest(&manifest).expect_err("media:control must fail validation");
    assert!(
        error.contains("media:control"),
        "error should name the removed permission: {error}"
    );
    assert!(error.contains("media:playback"), "error should list media:playback: {error}");
    assert!(error.contains("media:volume"), "error should list media:volume: {error}");
    assert!(error.contains("media:queue"), "error should list media:queue: {error}");
}

#[test]
fn accepts_split_media_permissions() {
    let manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.split-media",
        "name": "Split media app",
        "version": "1.0.0",
        "main": "main.js",
        "category": "utility",
        "permissions": ["media:playback", "media:volume", "media:queue", "media:read"]
    }))
    .expect("manifest should deserialize");

    validate_tapp_manifest(&manifest).expect("split media permissions should validate");
}

#[test]
fn preserves_and_validates_event_topics() {
    let manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.player",
        "name": "Event app",
        "version": "1.0.0",
        "main": "main.js",
        "category": "media",
        "permissions": ["event:publish", "event:subscribe"],
        "events": {
            "publish": ["tapp.com.example.player.track.changed"],
            "subscribe": ["system.theme.changed", "tapp.com.example.other.invalidated"]
        }
    }))
    .expect("manifest should deserialize");

    validate_tapp_manifest(&manifest).expect("event declaration should validate");
    let value = serde_json::to_value(manifest).expect("manifest should serialize");
    assert_eq!(
        value["events"]["publish"],
        json!(["tapp.com.example.player.track.changed"])
    );
}

#[test]
fn rejects_event_publish_topic_outside_tapp_namespace() {
    let manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.player",
        "name": "Event app",
        "version": "1.0.0",
        "main": "main.js",
        "category": "media",
        "permissions": ["event:publish"],
        "events": {
            "publish": ["tapp.com.example.other.track.changed"]
        }
    }))
    .expect("manifest should deserialize");

    assert!(validate_tapp_manifest(&manifest).is_err());
}

#[test]
fn preserves_and_validates_agent_manifest() {
    let manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.reporter",
        "name": "Agent app",
        "version": "1.0.0",
        "main": "main.js",
        "category": "productivity",
        "permissions": [],
        "agent": {
            "protocolVersion": 2,
            "interactions": [{
                "type": "report.compose",
                "inputSchema": "schemas/report-input.json",
                "resultSchema": "schemas/report-result.json"
            }],
            "intents": ["ui.open", "report.create"]
        }
    }))
    .expect("manifest should deserialize");

    validate_tapp_manifest(&manifest).expect("Agent declaration should validate");
    let value = serde_json::to_value(manifest).expect("manifest should serialize");
    assert_eq!(value["agent"]["protocolVersion"], 2);
    assert_eq!(value["agent"]["interactions"][0]["type"], "report.compose");
}

#[test]
fn rejects_external_data_exchange_schema_references() {
    let manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.exchange",
        "name": "Exchange app",
        "version": "1.0.0",
        "main": "main.js",
        "category": "data",
        "permissions": [],
        "dataExchange": {
            "exports": [{
                "id": "unsafe",
                "maxBytes": 1024,
                "schema": { "$ref": "https://example.com/schema.json" }
            }]
        }
    }))
    .expect("manifest should deserialize");

    assert!(validate_tapp_manifest(&manifest).is_err());
}

#[test]
fn preserves_widget_metadata_during_manifest_round_trip() {
    let manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.widget",
        "name": "Widget app",
        "version": "1.0.0",
        "main": "main.js",
        "category": "utility",
        "permissions": ["widget:register"],
        "widgets": [{
            "id": "summary",
            "name": "Summary",
            "description": "Daily summary",
            "icon": "chart",
            "defaultSize": "2x2",
            "sizes": ["2x2", "4x2"],
            "category": "stats",
            "templates": {
                "2x2": "templates/widget-2x2.html",
                "4x2": "templates/widget-4x2.html"
            },
            "settings": [{
                "key": "compact",
                "label": "Compact layout",
                "type": "toggle",
                "defaultValue": false
            }],
            "refreshPolicy": {
                "mode": "interval",
                "intervalSeconds": 60,
                "refreshOnVisible": true
            }
        }]
    }))
    .expect("manifest should deserialize");

    validate_tapp_manifest(&manifest).expect("widget metadata should validate");
    let value = serde_json::to_value(manifest).expect("manifest should serialize");
    let widget = &value["widgets"][0];
    assert_eq!(widget["description"], json!("Daily summary"));
    assert_eq!(widget["icon"], json!("chart"));
    assert_eq!(widget["category"], json!("stats"));
    assert_eq!(
        widget["templates"]["4x2"],
        json!("templates/widget-4x2.html")
    );
    assert_eq!(widget["settings"][0]["key"], json!("compact"));
    assert_eq!(widget["refreshPolicy"]["mode"], json!("interval"));
    assert_eq!(widget["refreshPolicy"]["intervalSeconds"], json!(60));
    let parsed: TappManifest = serde_json::from_value(value).unwrap();
    assert_eq!(
        widget_template_path(&parsed, "summary", "4x2"),
        Some("templates/widget-4x2.html")
    );
}

#[test]
fn rejects_invalid_widget_settings_and_refresh_policy() {
    let parse = |widget: serde_json::Value| {
        serde_json::from_value::<TappManifest>(json!({
            "id": "com.example.invalid-widget",
            "name": "Invalid widget",
            "version": "1.0.0",
            "main": "main.js",
            "category": "utility",
            "permissions": ["widget:register"],
            "widgets": [widget]
        }))
        .unwrap()
    };

    let too_frequent = parse(json!({
        "id": "summary",
        "name": "Summary",
        "defaultSize": "2x2",
        "sizes": ["2x2"],
        "refreshPolicy": { "mode": "interval", "intervalSeconds": 5 }
    }));
    assert!(validate_tapp_manifest(&too_frequent).is_err());

    let invalid_setting = parse(json!({
        "id": "summary",
        "name": "Summary",
        "defaultSize": "2x2",
        "sizes": ["2x2"],
        "settings": [{ "key": "mode", "label": "Mode", "type": "select" }]
    }));
    assert!(validate_tapp_manifest(&invalid_setting).is_err());
}

#[test]
fn host_setting_values_follow_manifest_type_and_range() {
    let setting: TappSettingDef = serde_json::from_value(json!({
        "key": "volume",
        "label": "Volume",
        "type": "number",
        "defaultValue": 50,
        "min": 0,
        "max": 100
    }))
    .unwrap();

    assert!(tapp_setting_value_is_valid(&setting, &json!(75)));
    assert!(!tapp_setting_value_is_valid(&setting, &json!(101)));
    assert!(!tapp_setting_value_is_valid(&setting, &json!("75")));

    let select: TappSettingDef = serde_json::from_value(json!({
        "key": "theme",
        "label": "Theme",
        "type": "select",
        "options": [
            { "value": "light", "label": "Light" },
            { "value": "dark", "label": "Dark" }
        ]
    }))
    .unwrap();
    assert!(tapp_setting_value_is_valid(&select, &json!("dark")));
    assert!(!tapp_setting_value_is_valid(&select, &json!("system")));
}

#[test]
fn enforces_minimum_system_version_on_install_and_update_validation() {
    let mut manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.system-version",
        "name": "System version gate",
        "version": "1.0.0",
        "main": "main.js",
        "category": "utility",
        "permissions": [],
        "minSystemVersion": env!("CARGO_PKG_VERSION")
    }))
    .unwrap();

    validate_tapp_manifest(&manifest).unwrap();
    let serialized = serde_json::to_value(&manifest).unwrap();
    assert_eq!(
        serialized["minSystemVersion"],
        json!(env!("CARGO_PKG_VERSION"))
    );

    manifest.min_system_version = Some("999.0.0".to_string());
    assert!(validate_tapp_manifest(&manifest).is_err());

    manifest.min_system_version = Some("not-a-version".to_string());
    assert!(validate_tapp_manifest(&manifest).is_err());
}

#[test]
fn validates_author_contact_fields() {
    let parse = |author: serde_json::Value| {
        serde_json::from_value::<TappManifest>(json!({
            "id": "com.example.author",
            "name": "Author metadata",
            "version": "1.0.0",
            "main": "main.js",
            "category": "utility",
            "permissions": [],
            "author": author
        }))
        .unwrap()
    };

    let valid = parse(json!({
        "name": "Example Team",
        "email": "team@example.com",
        "url": "https://example.com/team"
    }));
    validate_tapp_manifest(&valid).unwrap();

    assert!(validate_tapp_manifest(&parse(json!({ "name": "" }))).is_err());
    assert!(validate_tapp_manifest(&parse(json!({
        "name": "Example Team",
        "email": "not-an-email"
    })))
    .is_err());
    assert!(validate_tapp_manifest(&parse(json!({
        "name": "Example Team",
        "url": "javascript:alert(1)"
    })))
    .is_err());
}

#[test]
fn validates_manifest_metadata_and_declared_capability_permissions() {
    let base = || {
        serde_json::from_value::<TappManifest>(json!({
            "id": "com.example.metadata",
            "name": "Metadata app",
            "version": "1.0.0-beta.1",
            "description": "Valid metadata",
            "main": "main.js",
            "category": "utility",
            "permissions": [],
            "themeColor": "#12ABef",
            "homepage": "https://example.com/app",
            "repository": "https://github.com/example/app"
        }))
        .unwrap()
    };
    validate_tapp_manifest(&base()).unwrap();

    let mut invalid = base();
    invalid.name = " ".to_string();
    assert!(validate_tapp_manifest(&invalid).is_err());

    let mut invalid = base();
    invalid.version = "latest".to_string();
    assert!(validate_tapp_manifest(&invalid).is_err());

    let mut invalid = base();
    invalid.theme_color = Some("red".to_string());
    assert!(validate_tapp_manifest(&invalid).is_err());

    let mut invalid = base();
    invalid.repository = Some("javascript:alert(1)".to_string());
    assert!(validate_tapp_manifest(&invalid).is_err());

    let mut invalid = base();
    invalid.background_requirements = Some(vec!["widget".to_string()]);
    assert!(validate_tapp_manifest(&invalid).is_err());

    let widget_without_permission: TappManifest = serde_json::from_value(json!({
        "id": "com.example.widget-permission",
        "name": "Widget permission",
        "version": "1.0.0",
        "main": "main.js",
        "category": "utility",
        "permissions": [],
        "widgets": [{
            "id": "summary",
            "name": "Summary",
            "defaultSize": "2x2",
            "sizes": ["2x2"]
        }]
    }))
    .unwrap();
    assert!(validate_tapp_manifest(&widget_without_permission).is_err());

    let protected_api_without_permission: TappManifest = serde_json::from_value(json!({
        "id": "com.example.api-permission",
        "name": "API permission",
        "version": "1.0.0",
        "main": "main.js",
        "permissions": [],
        "apis": {
            "protected": {
                "endpoint": "https://example.com/data"
            }
        }
    }))
    .unwrap();
    assert!(validate_tapp_manifest(&protected_api_without_permission).is_err());
}

#[test]
fn normalizes_legacy_tapp_categories_and_rejects_unknown_values() {
    let parse = |category: Option<&str>| {
        let mut value = json!({
            "id": "com.example.category",
            "name": "Category contract",
            "version": "1.0.0",
            "main": "main.js",
            "permissions": []
        });
        if let Some(category) = category {
            value["category"] = json!(category);
        }
        serde_json::from_value::<TappManifest>(value)
    };

    let legacy_game = parse(Some("games")).unwrap();
    assert_eq!(legacy_game.category, Some(TappCategory::Game));
    assert_eq!(
        serde_json::to_value(legacy_game).unwrap()["category"],
        json!("game")
    );

    let legacy_tool = parse(Some("tool")).unwrap();
    assert_eq!(legacy_tool.category, Some(TappCategory::Utility));
    assert_eq!(
        serde_json::to_value(legacy_tool).unwrap()["category"],
        json!("utility")
    );

    let missing = parse(None).unwrap();
    assert_eq!(missing.category, None);
    assert!(validate_tapp_manifest(&missing).is_err());
    assert!(serde_json::to_value(missing)
        .unwrap()
        .get("category")
        .is_none());

    assert!(parse(Some("uncategorized")).is_err());
}

#[test]
fn normalizes_and_restricts_widget_categories_across_manifest_and_runtime() {
    let legacy_manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.widget-category",
        "name": "Widget category",
        "version": "1.0.0",
        "main": "main.js",
        "category": "utility",
        "permissions": ["widget:register"],
        "category": "utility",
        "widgets": [{
            "id": "summary",
            "name": "Summary",
            "defaultSize": "2x2",
            "sizes": ["2x2"],
            "category": "tool"
        }]
    }))
    .unwrap();
    assert_eq!(
        legacy_manifest.widgets.as_ref().unwrap()[0].category,
        Some(TappWidgetCategory::Utility)
    );
    assert_eq!(
        serde_json::to_value(legacy_manifest).unwrap()["widgets"][0]["category"],
        json!("utility")
    );

    let runtime_payload = json!({
        "id": "summary",
        "name": "Summary",
        "default_size": "2x2",
        "sizes": ["2x2"],
        "category": "activity"
    });
    let runtime: RegisterWidgetRequest = serde_json::from_value(runtime_payload.clone()).unwrap();
    assert_eq!(runtime.category, Some(TappWidgetCategory::Activity));

    let mut invalid_runtime = runtime_payload;
    invalid_runtime["category"] = json!("media");
    assert!(serde_json::from_value::<RegisterWidgetRequest>(invalid_runtime).is_err());

    let invalid_manifest = json!({
        "id": "com.example.invalid-widget-category",
        "name": "Invalid Widget category",
        "version": "1.0.0",
        "main": "main.js",
        "permissions": ["widget:register"],
        "category": "utility",
        "widgets": [{
            "id": "summary",
            "name": "Summary",
            "defaultSize": "2x2",
            "sizes": ["2x2"],
            "category": "media"
        }]
    });
    assert!(serde_json::from_value::<TappManifest>(invalid_manifest).is_err());
}

#[test]
fn runtime_widget_owner_binding_prevents_cross_installation_reuse() {
    let now = chrono::Utc::now().fixed_offset();
    let widget = |subject_id: i32, config: serde_json::Value| tapp_widgets::Model {
        id: 1,
        widget_id: "tapp.com.example.shared.dynamic".to_string(),
        tapp_id: "com.example.shared".to_string(),
        user_id: subject_id,
        name: "Dynamic".to_string(),
        description: None,
        icon: None,
        default_size: "2x2".to_string(),
        sizes: json!(["2x2"]),
        category: None,
        config,
        registered_at: now,
    };

    let public_widget = widget(9, json!({ "source": "runtime", "installationOwnerId": 1 }));
    assert!(super::runtime_widget_belongs_to_installation(
        &public_widget,
        9,
        1
    ));
    assert!(!super::runtime_widget_belongs_to_installation(
        &public_widget,
        9,
        9
    ));

    let legacy_private = widget(9, json!({ "source": "runtime" }));
    assert!(super::runtime_widget_belongs_to_installation(
        &legacy_private,
        9,
        9
    ));
    assert!(!super::runtime_widget_belongs_to_installation(
        &legacy_private,
        9,
        1
    ));
}

#[test]
fn requires_store_index_and_manifest_categories_to_match() {
    let manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.store-category",
        "name": "Store category",
        "version": "1.0.0",
        "main": "main.js",
        "permissions": [],
        "category": "media"
    }))
    .unwrap();

    assert!(validate_store_manifest_category(&json!({ "category": "music" }), &manifest).is_ok());
    assert!(
        validate_store_manifest_category(&json!({ "category": "productivity" }), &manifest)
            .is_err()
    );
    assert!(validate_store_manifest_category(&json!({}), &manifest).is_err());
}

#[test]
fn rejects_removed_or_unknown_manifest_fields() {
    let removed_top_level = serde_json::from_value::<TappManifest>(json!({
        "id": "com.example.legacy",
        "name": "Legacy app",
        "version": "1.0.0",
        "main": "main.js",
        "permissions": [],
        "optionalPermissions": ["network:fetch"]
    }));
    assert!(removed_top_level.is_err());

    let removed_widget_field = serde_json::from_value::<TappManifest>(json!({
        "id": "com.example.legacy-widget",
        "name": "Legacy widget",
        "version": "1.0.0",
        "main": "main.js",
        "permissions": ["widget:register"],
        "widgets": [{
            "id": "summary",
            "name": "Summary",
            "defaultSize": "2x2",
            "sizes": ["2x2"],
            "refreshInterval": 60000
        }]
    }));
    assert!(removed_widget_field.is_err());

    let removed_min_refresh_interval = serde_json::from_value::<TappManifest>(json!({
        "id": "com.example.legacy-widget",
        "name": "Legacy widget",
        "version": "1.0.0",
        "main": "main.js",
        "permissions": ["widget:register"],
        "widgets": [{
            "id": "summary",
            "name": "Summary",
            "defaultSize": "2x2",
            "sizes": ["2x2"],
            "minRefreshInterval": 1000
        }]
    }));
    assert!(removed_min_refresh_interval.is_err());
}

#[test]
fn validates_declared_api_shape_and_inject_aliases() {
    let valid: TappManifest = serde_json::from_value(json!({
        "id": "com.example.api",
        "name": "API app",
        "version": "1.0.0",
        "main": "main.js",
        "category": "developer",
        "permissions": ["network:fetch"],
        "apis": {
            "weather.current": {
                "type": "http",
                "endpoint": "https://example.com/weather?city={{city}}",
                "inject": { "city": "{{geo.city}}" }
            }
        }
    }))
    .unwrap();
    validate_tapp_manifest(&valid).unwrap();
    assert_eq!(
        valid
            .apis
            .as_ref()
            .unwrap()
            .get("weather.current")
            .unwrap()
            .body_mode,
        super::TappHttpBodyMode::Json
    );
    assert!(
        serde_json::to_value(&valid).unwrap()["apis"]["weather.current"]
            .get("bodyMode")
            .is_none()
    );

    let body_modes: TappManifest = serde_json::from_value(json!({
        "id": "com.example.api-body-modes",
        "name": "API body modes",
        "version": "1.0.0",
        "main": "main.js",
        "category": "developer",
        "permissions": ["network:fetch"],
        "apis": {
            "submit.raw": {
                "type": "http",
                "endpoint": "https://example.com/raw",
                "method": "POST",
                "bodyMode": "raw",
                "headers": { "Content-Type": "text/plain; charset=utf-8" },
                "body": "{{params.body}}"
            },
            "token.form": {
                "type": "http",
                "endpoint": "https://example.com/oauth/token",
                "method": "POST",
                "bodyMode": "form",
                "body": {
                    "grant_type": "client_credentials",
                    "scope": "{{params.scope}}"
                }
            }
        }
    }))
    .unwrap();
    validate_tapp_manifest(&body_modes).unwrap();
    assert_eq!(
        serde_json::to_value(&body_modes).unwrap()["apis"]["submit.raw"]["bodyMode"],
        "raw"
    );

    let unknown_body_mode = serde_json::from_value::<TappManifest>(json!({
        "id": "com.example.api-unknown-body-mode",
        "name": "Unknown API body mode",
        "version": "1.0.0",
        "main": "main.js",
        "permissions": ["network:fetch"],
        "apis": {
            "submit": {
                "endpoint": "https://example.com/raw",
                "method": "POST",
                "bodyMode": "binary",
                "body": "payload"
            }
        }
    }));
    assert!(unknown_body_mode.is_err());

    let mut invalid_raw_method = body_modes.clone();
    invalid_raw_method
        .apis
        .as_mut()
        .unwrap()
        .get_mut("submit.raw")
        .unwrap()
        .method = "GET".to_string();
    assert!(validate_tapp_manifest(&invalid_raw_method).is_err());

    let mut invalid_raw_body = body_modes.clone();
    invalid_raw_body
        .apis
        .as_mut()
        .unwrap()
        .get_mut("submit.raw")
        .unwrap()
        .body = Some(json!({ "not": "raw" }));
    assert!(validate_tapp_manifest(&invalid_raw_body).is_err());

    let mut invalid_form_body = body_modes.clone();
    invalid_form_body
        .apis
        .as_mut()
        .unwrap()
        .get_mut("token.form")
        .unwrap()
        .body = Some(json!({ "scope": ["read", "write"] }));
    assert!(validate_tapp_manifest(&invalid_form_body).is_err());

    let mut public_without_network = valid.clone();
    public_without_network.permissions.clear();
    public_without_network
        .apis
        .as_mut()
        .unwrap()
        .get_mut("weather.current")
        .unwrap()
        .access = super::TappApiAccess::Public;
    assert!(validate_tapp_manifest(&public_without_network).is_err());

    let mut reserved_alias = valid.clone();
    reserved_alias
        .apis
        .as_mut()
        .unwrap()
        .get_mut("weather.current")
        .unwrap()
        .inject = Some(std::collections::HashMap::from([(
        "user.id".to_string(),
        "{{geo.city}}".to_string(),
    )]));
    assert!(validate_tapp_manifest(&reserved_alias).is_err());

    let mut secret_template = valid.clone();
    secret_template
        .apis
        .as_mut()
        .unwrap()
        .get_mut("weather.current")
        .unwrap()
        .headers = Some(std::collections::HashMap::from([(
        "Authorization".to_string(),
        "Bearer {{secrets.OPENWEATHER_KEY}}".to_string(),
    )]));
    assert!(validate_tapp_manifest(&secret_template).is_err());

    let removed_api_url = serde_json::from_value::<TappManifest>(json!({
        "id": "com.example.legacy-api",
        "name": "Legacy API",
        "version": "1.0.0",
        "main": "main.js",
        "permissions": ["network:fetch"],
        "apis": {
            "weather": {
                "url": "https://example.com/weather"
            }
        }
    }));
    assert!(removed_api_url.is_err());

    // Methods come from the fixed allow-list shared with the offline CLI:
    // RFC 7230 extension tokens and lowercase spellings are rejected.
    for (method, expected_ok) in [("POST", true), ("PURGE", false), ("get", false)] {
        let mut with_method = valid.clone();
        with_method
            .apis
            .as_mut()
            .unwrap()
            .get_mut("weather.current")
            .unwrap()
            .method = method.to_string();
        let result = validate_tapp_manifest(&with_method);
        assert_eq!(result.is_ok(), expected_ok, "method {method}");
        if let Err(message) = result {
            // The rejection lists the allowed methods so LLM repair loops can
            // fix the manifest without guessing.
            assert!(
                message.contains("GET"),
                "message must list methods: {message}"
            );
        }
    }
}

#[test]
fn rejects_unbounded_widget_manifests() {
    let mut manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.too-many-widgets",
        "name": "Too many widgets",
        "version": "1.0.0",
        "main": "main.js",
        "category": "utility",
        "permissions": ["widget:register"]
    }))
    .unwrap();
    manifest.widgets = Some(
        (0..65)
            .map(|index| TappWidgetDef {
                id: format!("widget-{index}"),
                name: format!("Widget {index}"),
                description: None,
                icon: None,
                default_size: "2x2".to_string(),
                sizes: vec!["2x2".to_string()],
                category: None,
                templates: None,
                settings: Vec::new(),
                refresh_policy: None,
            })
            .collect(),
    );

    assert!(validate_tapp_manifest(&manifest).is_err());
}

#[test]
fn batch_detail_mapping_applies_current_role_and_brew_capability_rules() {
    let now = chrono::Utc::now().fixed_offset();
    let tapp = tapps::Model {
        id: 1,
        tapp_id: "com.example.detail".to_string(),
        user_id: 7,
        name: "Detail".to_string(),
        version: "1.0.0".to_string(),
        description: None,
        author: None,
        icon: None,
        theme_color: None,
        manifest: json!({
            "id": "com.example.detail",
            "name": "Detail",
            "version": "1.0.0",
            "main": "main.js",
            "permissions": ["storage:read", "brew:write", "ai:generate"]
        }),
        status: tapps::TappStatus::Installed,
        granted_permissions: json!(["storage:read", "brew:write"]),
        approved_permissions: json!(["storage:read", "brew:write", "ai:generate"]),
        file_path: "manifest.json".to_string(),
        code_path: "main.js".to_string(),
        installed_at: now,
        last_run_at: None,
        updated_at: now,
        error_message: None,
        visibility: "all".to_string(),
    };

    let config = crate::config::DynamicConfig {
        user_perm_ai_generate: true,
        ..Default::default()
    };
    let detail = super::tapp_detail_from_model(tapp, UserRole::User, true, false, &config);

    assert_eq!(detail.user_role, "user");
    assert!(detail.is_temporary);
    assert!(!detail.is_admin_tapp);
    assert_eq!(
        detail.granted_permissions,
        vec!["storage:read", "brew:write", "ai:generate"]
    );
}

#[test]
fn validates_all_declared_install_resources() {
    let manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.resources",
        "name": "Resources",
        "version": "1.0.0",
        "main": "src/main.js",
        "category": "utility",
        "permissions": [],
        "styles": "css/shared.css",
        "pageModules": ["index.js"],
        "widgets": [{
            "id": "summary",
            "name": "Summary",
            "defaultSize": "2x2",
            "sizes": ["2x2"],
            "templates": { "2x2": "templates/summary.html" }
        }]
    }))
    .unwrap();

    let unique = format!(
        "myriad-tapp-resource-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let root = std::env::temp_dir().join(unique);
    for relative in [
        "src/main.js",
        "css/shared.css",
        "page/index.js",
        "templates/summary.html",
    ] {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "test").unwrap();
    }

    assert!(validate_installed_resources(&manifest, &root).is_ok());

    std::fs::write(root.join("src/main.js"), [0xff, 0xfe]).unwrap();
    assert!(validate_installed_resources(&manifest, &root).is_err());
    std::fs::write(root.join("src/main.js"), "test").unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let template = root.join("templates/summary.html");
        std::fs::remove_file(&template).unwrap();
        std::fs::write(root.join("outside.html"), "outside").unwrap();
        symlink(root.join("outside.html"), &template).unwrap();
        assert!(validate_installed_resources(&manifest, &root).is_err());
        std::fs::remove_file(&template).unwrap();
        std::fs::write(&template, "test").unwrap();
    }

    std::fs::remove_file(root.join("templates/summary.html")).unwrap();
    assert!(validate_installed_resources(&manifest, &root).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn validates_declared_package_assets_allow_binary() {
    assert!(validate_asset_path("assets/sprite.png").is_ok());
    assert!(validate_asset_path("sprite.png").is_err());
    assert!(validate_asset_path("assets/hack.js").is_err());
    assert!(validate_asset_path("../assets/x.png").is_err());

    let manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.assets",
        "name": "Assets",
        "version": "1.0.0",
        "main": "main.js",
        "category": "game",
        "permissions": ["media:audio"],
        "assets": ["assets/pixel.png", "assets/level.json"]
    }))
    .unwrap();
    assert!(validate_tapp_manifest(&manifest).is_ok());

    let unique = format!(
        "myriad-tapp-assets-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let root = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(root.join("assets")).unwrap();
    std::fs::write(root.join("main.js"), "export {};").unwrap();
    std::fs::write(root.join("assets/pixel.png"), [0x89, 0x50, 0x4e, 0x47]).unwrap();
    std::fs::write(root.join("assets/level.json"), r#"{"ok":true}"#).unwrap();
    assert!(validate_installed_resources(&manifest, &root).is_ok());

    std::fs::remove_file(root.join("assets/pixel.png")).unwrap();
    assert!(validate_installed_resources(&manifest, &root).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn validates_agent_schema_and_i18n_contents_at_install_time() {
    let manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.validated-content",
        "name": "Validated content",
        "version": "1.0.0",
        "main": "main.js",
        "category": "productivity",
        "permissions": [],
        "agent": {
            "protocolVersion": 2,
            "interactions": [{
                "type": "report.compose",
                "inputSchema": "schemas/input.json"
            }]
        }
    }))
    .unwrap();
    let unique = format!(
        "myriad-tapp-content-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let root = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(root.join("schemas")).unwrap();
    std::fs::create_dir_all(root.join("i18n")).unwrap();
    std::fs::write(root.join("main.js"), "export {};").unwrap();
    std::fs::write(
        root.join("schemas/input.json"),
        r#"{"type":"object","properties":{"title":{"type":"string"}}}"#,
    )
    .unwrap();
    std::fs::write(root.join("i18n/en-US.json"), r#"{"title":"Title"}"#).unwrap();
    assert!(validate_installed_resources(&manifest, &root).is_ok());

    std::fs::write(root.join("schemas/input.json"), "not-json").unwrap();
    assert!(validate_installed_resources(&manifest, &root)
        .unwrap_err()
        .contains("not valid JSON"));

    std::fs::write(root.join("schemas/input.json"), r#"{"$ref":"remote.json"}"#).unwrap();
    assert!(validate_installed_resources(&manifest, &root)
        .unwrap_err()
        .contains("does not support $ref"));

    std::fs::write(root.join("schemas/input.json"), r#"{"type":"object"}"#).unwrap();
    std::fs::write(root.join("i18n/en-US.json"), r#"["not","an","object"]"#).unwrap();
    assert!(validate_installed_resources(&manifest, &root)
        .unwrap_err()
        .contains("must contain a JSON object"));

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn exports_nested_resource_paths_without_flattening() {
    let unique = format!(
        "myriad-tapp-export-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let root = std::env::temp_dir().join(unique);
    let nested = root.join("templates/dashboard/widget.html");
    std::fs::create_dir_all(nested.parent().unwrap()).unwrap();
    std::fs::write(root.join("manifest.json"), "{}").unwrap();
    std::fs::write(root.join(super::TAPP_INSTALL_STATE_FILE), "internal").unwrap();
    std::fs::write(&nested, "<main>nested</main>").unwrap();

    let cursor = std::io::Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    append_directory_to_zip(&mut writer, &root, &root, options).unwrap();
    let bytes = writer.finish().unwrap().into_inner();

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut names = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_string())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        vec![
            "manifest.json".to_string(),
            "templates/dashboard/widget.html".to_string()
        ]
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_colliding_archive_entries() {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default();
    writer.add_directory("templates/", options).unwrap();
    writer.start_file("templates", options).unwrap();
    std::io::Write::write_all(&mut writer, b"collision").unwrap();
    let bytes = writer.finish().unwrap().into_inner();

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert!(validate_tapp_archive(&mut archive).is_err());
}

#[test]
fn rejects_tapp_ids_and_resource_paths_that_escape_the_sandbox() {
    for invalid in ["", ".", "..", "../escape", "/tmp/escape", ".hidden"] {
        assert!(validate_tapp_id(invalid).is_err(), "accepted {invalid}");
    }
    assert!(validate_tapp_id("com.myriad.safe-app_2").is_ok());
    assert!(tapp_dir_for(7, "../../tmp/escape").is_err());

    for invalid in ["../secret", "/etc/passwd", "page/../../secret", ".env"] {
        assert!(
            validate_resource_path(invalid).is_err(),
            "accepted {invalid}"
        );
    }
    assert!(validate_resource_path("page/state.js").is_ok());
    let root = std::path::Path::new("/tmp/tapps/com.example.safe");
    assert_eq!(
        archive_entry_path(root, "templates/widget-2x2.html").unwrap(),
        root.join("templates/widget-2x2.html")
    );
    assert!(archive_entry_path(root, "../outside.html").is_err());
}

#[test]
fn validates_manifest_paths_before_install_or_update() {
    let mut manifest: TappManifest = serde_json::from_value(json!({
        "id": "com.example.safe",
        "name": "Safe app",
        "version": "1.0.0",
        "main": "main.js",
        "category": "utility",
        "permissions": [],
        "pageModules": ["state.js"]
    }))
    .unwrap();
    assert!(validate_tapp_manifest(&manifest).is_ok());

    manifest.main = "main.txt".to_string();
    assert!(validate_tapp_manifest(&manifest).is_err());
    manifest.main = "main.js".to_string();

    manifest.styles = Some("styles.txt".to_string());
    assert!(validate_tapp_manifest(&manifest).is_err());
    manifest.styles = None;

    manifest.page_template = Some("page.txt".to_string());
    assert!(validate_tapp_manifest(&manifest).is_err());

    manifest.page_template = Some("../../outside.html".to_string());
    assert!(validate_tapp_manifest(&manifest).is_err());

    manifest.page_template = None;
    manifest.page_modules = Some(vec!["nested/index.js".to_string()]);
    assert!(validate_tapp_manifest(&manifest).is_err());

    let manifest_with_escaping_widget_template: TappManifest = serde_json::from_value(json!({
        "id": "com.example.unsafe-widget",
        "name": "Unsafe widget",
        "version": "1.0.0",
        "main": "main.js",
        "category": "utility",
        "permissions": [],
        "widgets": [{
            "id": "unsafe",
            "name": "Unsafe",
            "defaultSize": "2x2",
            "sizes": ["2x2"],
            "templates": { "2x2": "../outside.html" }
        }]
    }))
    .unwrap();
    assert!(validate_tapp_manifest(&manifest_with_escaping_widget_template).is_err());

    let same_size_templates: TappManifest = serde_json::from_value(json!({
        "id": "com.example.conflicting-widgets",
        "name": "Conflicting widgets",
        "version": "1.0.0",
        "main": "main.js",
        "category": "utility",
        "permissions": ["widget:register"],
        "widgets": [
            {
                "id": "one",
                "name": "One",
                "defaultSize": "2x2",
                "sizes": ["2x2"],
                "templates": { "2x2": "templates/one.html" }
            },
            {
                "id": "two",
                "name": "Two",
                "defaultSize": "2x2",
                "sizes": ["2x2"],
                "templates": { "2x2": "templates/two.html" }
            }
        ]
    }))
    .unwrap();
    assert!(validate_tapp_manifest(&same_size_templates).is_ok());

    let contents = WidgetTemplateContents::from([
        (
            "one".to_string(),
            std::collections::HashMap::from([("2x2".to_string(), "one template".to_string())]),
        ),
        (
            "two".to_string(),
            std::collections::HashMap::from([("2x2".to_string(), "two template".to_string())]),
        ),
    ]);
    assert!(validate_widget_template_contents(&same_size_templates, &contents).is_ok());

    let unknown_widget = WidgetTemplateContents::from([(
        "missing".to_string(),
        std::collections::HashMap::from([("2x2".to_string(), "template".to_string())]),
    )]);
    assert!(validate_widget_template_contents(&same_size_templates, &unknown_widget).is_err());
}
