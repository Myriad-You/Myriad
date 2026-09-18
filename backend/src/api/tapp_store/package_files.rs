//! Filesystem staging, recovery, package resources and archive boundaries.

use super::{
    TappManifest, decode_asset_base64, lock_tapp_lifecycle, validate_asset_path, validate_tapp_id,
};
use crate::error::HttpError;
use axum::http::StatusCode;
use myriad_error::AppError;
use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, TransactionTrait};
use std::path::{Path as FsPath, PathBuf};
use tokio::fs;

#[cfg(test)]
static ACTIVATE_RENAME_FAILURES: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<PathBuf, std::io::ErrorKind>>,
> = std::sync::OnceLock::new();

#[cfg(test)]
pub(crate) fn fail_next_activation_rename(from: &FsPath) {
    fail_next_activation_rename_with_kind(from, std::io::ErrorKind::Other);
}

#[cfg(test)]
pub(crate) fn fail_next_activation_rename_with_kind(from: &FsPath, kind: std::io::ErrorKind) {
    ACTIVATE_RENAME_FAILURES
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(from.to_path_buf(), kind);
}

async fn rename_activation_path(from: &FsPath, to: &FsPath) -> Result<(), std::io::Error> {
    #[cfg(test)]
    {
        let failure_kind = ACTIVATE_RENAME_FAILURES
            .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(from);
        if let Some(kind) = failure_kind {
            return Err(std::io::Error::new(
                kind,
                "injected Tapp activation rename failure",
            ));
        }
    }
    fs::rename(from, to).await
}

use crate::models::entities::tapps;
use crate::services::data_paths::paths;
use crate::services::tapp_package_fs::{
    RecoveryPlan, archive_entry_relative_path, filesystem_error_message,
    filesystem_error_status_hint, install_generation_matches_micros, install_generation_payload,
    is_lifecycle_artifact_filename, is_staging_artifact_filename, lifecycle_artifact_dir_name,
    looks_like_tapp_installation_from_markers, orphan_tapp_key_if_unowned,
    parse_tapp_owner_dir_name, plan_tapp_directory_recovery,
    recovery_artifacts_to_remove_after_promote, recovery_discard_artifact_name,
    recovery_plan_mutates_live, resource_relative_path, sandbox_path_matches_relative,
    should_log_filesystem_permission_context, should_preserve_orphan_path,
    sort_recovery_artifact_paths,
};
use myriad_tapp_contract::contract_rules::ASSET_DIRECTORY;

// Path-stable re-exports for manifest_tests / parent imports.
pub(crate) use crate::services::tapp_package_fs::{
    MANIFEST_JSON, TAPP_INSTALL_STATE_FILE, has_reinstall_orphan_state,
};

pub(crate) fn tapp_dir_for(user_id: i32, tapp_id: &str) -> Result<PathBuf, String> {
    validate_tapp_id(tapp_id)?;
    Ok(paths().tapp_user_dir(user_id).join(tapp_id))
}

pub(crate) struct TappDirStage {
    path: PathBuf,
}

impl TappDirStage {
    pub(super) async fn create(final_path: &FsPath) -> Result<Self, std::io::Error> {
        let parent = final_path.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Tapp directory has no parent",
            )
        })?;
        fs::create_dir_all(parent).await?;
        let name = final_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("tapp");
        let path = parent.join(lifecycle_artifact_dir_name(
            name,
            "staging",
            &uuid::Uuid::new_v4().simple().to_string(),
        ));
        fs::create_dir(&path).await?;
        Ok(Self { path })
    }

    pub(super) fn path(&self) -> &FsPath {
        &self.path
    }

    pub(super) async fn activate(
        self,
        final_path: &FsPath,
    ) -> Result<ActivatedTappDir, std::io::Error> {
        let final_occupied = match fs::symlink_metadata(final_path).await {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => {
                tracing::error!(
                    path = %final_path.display(),
                    kind = ?error.kind(),
                    %error,
                    "Failed to inspect final Tapp path before activate"
                );
                return Err(error);
            }
        };

        let backup_path = if final_occupied {
            let parent = final_path.parent().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Tapp directory has no parent",
                )
            })?;
            let name = final_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("tapp");
            let backup = parent.join(lifecycle_artifact_dir_name(
                name,
                "backup",
                &uuid::Uuid::new_v4().simple().to_string(),
            ));
            match rename_activation_path(final_path, &backup).await {
                Ok(()) => Some(backup),
                Err(rename_error) => {
                    tracing::error!(
                        from = %final_path.display(),
                        to = %backup.display(),
                        kind = ?rename_error.kind(),
                        %rename_error,
                        "Failed to rename live Tapp dir to backup during activate; preserving live version"
                    );
                    // A failed backup rename means ownership of the live path
                    // was never transferred. Deleting it here would turn a
                    // recoverable activation error into loss of the last
                    // known-good installation.
                    return Err(rename_error);
                }
            }
        } else {
            None
        };

        if let Err(error) = rename_activation_path(&self.path, final_path).await {
            tracing::error!(
                from = %self.path.display(),
                to = %final_path.display(),
                kind = ?error.kind(),
                %error,
                "Failed to rename staging Tapp dir to final path"
            );
            if let Some(backup) = &backup_path {
                if let Err(restore_error) = rename_activation_path(backup, final_path).await {
                    tracing::error!(
                        from = %backup.display(),
                        to = %final_path.display(),
                        kind = ?restore_error.kind(),
                        %restore_error,
                        "Failed to restore backup after staging activate failure"
                    );
                }
            }
            return Err(error);
        }
        Ok(ActivatedTappDir {
            final_path: final_path.to_path_buf(),
            backup_path,
        })
    }
}

/// Remove a leftover path that occupies a Tapp live or lifecycle location.
pub(crate) async fn remove_path_best_effort(path: &FsPath) -> Result<(), std::io::Error> {
    match fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(dir_error) => match fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(file_error) if file_error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(file_error) => {
                // Prefer reporting the directory error when the path was a dir.
                if dir_error.kind() != std::io::ErrorKind::NotADirectory {
                    Err(dir_error)
                } else {
                    Err(file_error)
                }
            }
        },
    }
}

impl Drop for TappDirStage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub(crate) struct ActivatedTappDir {
    pub(super) final_path: PathBuf,
    pub(super) backup_path: Option<PathBuf>,
}

impl ActivatedTappDir {
    pub(super) async fn commit(mut self) {
        if let Some(backup) = self.backup_path.take() {
            if let Err(error) = fs::remove_dir_all(&backup).await {
                tracing::warn!(path = %backup.display(), %error, "Failed to remove old Tapp backup");
            }
        }
    }

    pub(super) async fn rollback(mut self) {
        self.rollback_with_candidate_policy(false).await;
    }

    pub(super) async fn rollback_after_commit_error(mut self) {
        self.rollback_with_candidate_policy(true).await;
    }

    async fn rollback_with_candidate_policy(&mut self, preserve_candidate: bool) {
        if let Some(backup) = self.backup_path.take() {
            let name = self
                .final_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("tapp");
            let discard = self
                .final_path
                .with_file_name(recovery_discard_artifact_name(
                    name,
                    &uuid::Uuid::new_v4().simple().to_string(),
                ));
            let candidate_exists = fs::symlink_metadata(&self.final_path).await.is_ok();
            if candidate_exists {
                if let Err(error) = rename_activation_path(&self.final_path, &discard).await {
                    tracing::error!(
                        from = %self.final_path.display(),
                        to = %discard.display(),
                        %error,
                        "Failed to quarantine candidate Tapp during rollback; preserving both generations"
                    );
                    return;
                }
            }

            if let Err(error) = rename_activation_path(&backup, &self.final_path).await {
                tracing::error!(
                    from = %backup.display(),
                    to = %self.final_path.display(),
                    %error,
                    "Failed to restore previous Tapp during rollback; preserving backup"
                );
                if candidate_exists {
                    if let Err(restore_error) =
                        rename_activation_path(&discard, &self.final_path).await
                    {
                        tracing::error!(
                            from = %discard.display(),
                            to = %self.final_path.display(),
                            %restore_error,
                            "Failed to restore candidate Tapp after rollback failure"
                        );
                    }
                }
                return;
            }

            if candidate_exists && !preserve_candidate {
                let _ = remove_path_best_effort(&discard).await;
            }
        } else if !preserve_candidate {
            // Definite first-install failure: drop the candidate. Ambiguous
            // COMMIT errors must keep the files so a committed row cannot be
            // left without a live directory.
            let _ = remove_path_best_effort(&self.final_path).await;
        }
    }
}

pub(crate) fn directory_manifest_matches(directory: &FsPath, expected: &serde_json::Value) -> bool {
    let Ok(content) = std::fs::read_to_string(directory.join(MANIFEST_JSON)) else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(&content).is_ok_and(|value| value == *expected)
}

pub(crate) fn write_install_generation(
    directory: &FsPath,
    updated_at: chrono::DateTime<chrono::FixedOffset>,
) -> Result<(), std::io::Error> {
    let value = install_generation_payload(updated_at.timestamp_micros());
    let encoded = serde_json::to_vec(&value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    std::fs::write(directory.join(TAPP_INSTALL_STATE_FILE), encoded)
}

pub(crate) fn directory_generation_matches(
    directory: &FsPath,
    expected_manifest: &serde_json::Value,
    expected_updated_at: chrono::DateTime<chrono::FixedOffset>,
) -> bool {
    let state_path = directory.join(TAPP_INSTALL_STATE_FILE);
    if state_path.exists() {
        return std::fs::read(state_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .is_some_and(|value| {
                install_generation_matches_micros(&value, expected_updated_at.timestamp_micros())
            });
    }
    // 无 generation marker 时回退 `directory_manifest_matches`
    directory_manifest_matches(directory, expected_manifest)
}

pub(crate) fn lifecycle_artifact_directories(
    final_path: &FsPath,
) -> Result<Vec<PathBuf>, std::io::Error> {
    let Some(parent) = final_path.parent() else {
        return Ok(Vec::new());
    };
    let Some(name) = final_path.file_name().and_then(|value| value.to_str()) else {
        return Ok(Vec::new());
    };
    let mut artifacts = Vec::new();
    if !parent.is_dir() {
        return Ok(artifacts);
    }
    for entry in std::fs::read_dir(parent)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let filename = entry.file_name();
        let Some(filename) = filename.to_str() else {
            continue;
        };
        if is_lifecycle_artifact_filename(filename, name) {
            artifacts.push(entry.path());
        }
    }
    Ok(artifacts)
}

/// Live install dir and lifecycle artifacts that should not remain when the DB
/// has no row for this owner/tapp_id (post-uninstall orphans, partial activate).
///
/// Path-selection helper used by reinstall cleanup and unit tests (prefix rules
/// in services::tapp_package_fs).
pub(crate) fn reinstall_orphan_paths(final_path: &FsPath) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut paths = Vec::new();
    match std::fs::symlink_metadata(final_path) {
        Ok(_) => paths.push(final_path.to_path_buf()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    paths.extend(lifecycle_artifact_directories(final_path)?);
    Ok(paths)
}

/// Best-effort remove leftover live dir and lifecycle artifacts before install
/// staging/activate. Caller must ensure there is no conflicting DB install row.
///
/// Cleanup is deliberately non-fatal. Recursive delete can fail when the live
/// tree is another UID, even if the owner directory still allows rename.
/// `TappDirStage::activate` quarantines that path by renaming it; refusing to
/// stage here would turn recoverable ownership drift into a permanent failure.
pub(crate) fn cleanup_reinstall_orphans(
    final_path: &FsPath,
    tapp_id: &str,
    owner_id: i32,
    user_id: i32,
    preserve: Option<&FsPath>,
) -> usize {
    let candidates = match reinstall_orphan_paths(final_path) {
        Ok(candidates) => candidates,
        Err(error) => {
            tracing::warn!(
                tapp_id,
                owner_id,
                user_id,
                path = %final_path.display(),
                kind = ?error.kind(),
                %error,
                "Unable to enumerate orphan Tapp paths; continuing install"
            );
            return 0;
        }
    };
    if !has_reinstall_orphan_state(&candidates) {
        return 0;
    }
    tracing::warn!(
        tapp_id,
        owner_id,
        user_id,
        path = %final_path.display(),
        orphan_count = candidates.len(),
        "Cleaning orphan Tapp filesystem state before install"
    );
    let mut removed = 0;
    for path in candidates {
        if should_preserve_orphan_path(&path, preserve) {
            continue;
        }
        // Staging dirs are created before the lifecycle lock. Deleting them
        // here can wipe another concurrent install of the same tapp_id.
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(is_staging_artifact_filename)
        {
            continue;
        }
        match std::fs::remove_dir_all(&path) {
            Ok(()) => {
                tracing::warn!(
                    tapp_id,
                    owner_id,
                    path = %path.display(),
                    "Removed orphan Tapp directory"
                );
                removed += 1;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(dir_error) => match std::fs::remove_file(&path) {
                Ok(()) => {
                    tracing::warn!(
                        tapp_id,
                        owner_id,
                        path = %path.display(),
                        "Removed orphan Tapp file occupying install path"
                    );
                    removed += 1;
                }
                Err(file_error) if file_error.kind() == std::io::ErrorKind::NotFound => {}
                Err(file_error) => {
                    tracing::warn!(
                        tapp_id,
                        owner_id,
                        user_id,
                        path = %path.display(),
                        dir_kind = ?dir_error.kind(),
                        %dir_error,
                        file_kind = ?file_error.kind(),
                        %file_error,
                        "Unable to remove orphan Tapp path; activate will quarantine it"
                    );
                }
            },
        }
    }
    removed
}

pub(crate) fn log_install_failure(
    step: &str,
    tapp_id: &str,
    user_id: i32,
    owner_id: i32,
    path: Option<&FsPath>,
    error: &dyn std::fmt::Display,
) {
    match path {
        Some(path) => tracing::error!(
            step,
            tapp_id,
            user_id,
            owner_id,
            path = %path.display(),
            error = %error,
            "Tapp install failed"
        ),
        None => tracing::error!(
            step,
            tapp_id,
            user_id,
            owner_id,
            error = %error,
            "Tapp install failed"
        ),
    }
}

pub(crate) fn tapp_filesystem_error_status(error: &std::io::Error) -> StatusCode {
    StatusCode::from_u16(filesystem_error_status_hint(error.kind()))
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

pub(crate) fn tapp_filesystem_error_message(action: &str, error: &std::io::Error) -> String {
    filesystem_error_message(action, error.kind(), &error.to_string())
}

/// Add owner/mode context for storage failures without following symlinks.
pub(crate) fn log_tapp_filesystem_access(path: &FsPath, error: &std::io::Error) {
    if !should_log_filesystem_permission_context(error.kind()) {
        return;
    }
    for candidate in [Some(path), path.parent()].into_iter().flatten() {
        match std::fs::symlink_metadata(candidate) {
            Ok(metadata) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    tracing::error!(
                        path = %candidate.display(),
                        uid = metadata.uid(),
                        gid = metadata.gid(),
                        mode = format_args!("{:04o}", metadata.mode() & 0o7777),
                        "Tapp filesystem permission context"
                    );
                }
                #[cfg(not(unix))]
                tracing::error!(
                    path = %candidate.display(),
                    readonly = metadata.permissions().readonly(),
                    "Tapp filesystem permission context"
                );
            }
            Err(metadata_error) => tracing::error!(
                path = %candidate.display(),
                kind = ?metadata_error.kind(),
                %metadata_error,
                "Unable to inspect Tapp filesystem permission context"
            ),
        }
    }
}

/// Reconcile one live resource directory with the database Manifest after an
/// interrupted install/update/uninstall lifecycle transaction.
///
/// Decision table lives in [`plan_tapp_directory_recovery`]; this function only
/// probes generations and performs renames/deletes.
pub(crate) fn recover_tapp_directory(
    final_path: &FsPath,
    expected_manifest: &serde_json::Value,
    expected_updated_at: chrono::DateTime<chrono::FixedOffset>,
) -> Result<bool, std::io::Error> {
    let mut artifacts = lifecycle_artifact_directories(final_path)?;
    // A backup/uninstall quarantine is the authoritative pre-transaction
    // generation. Consider staging only after those recovery sources.
    sort_recovery_artifact_paths(&mut artifacts);

    let live_matches =
        directory_generation_matches(final_path, expected_manifest, expected_updated_at);
    let artifact_matches: Vec<bool> = artifacts
        .iter()
        .map(|path| directory_generation_matches(path, expected_manifest, expected_updated_at))
        .collect();

    let plan = plan_tapp_directory_recovery(live_matches, &artifact_matches);
    match plan {
        RecoveryPlan::DiscardArtifactsOnly => {
            for artifact in artifacts {
                std::fs::remove_dir_all(artifact)?;
            }
            Ok(recovery_plan_mutates_live(plan))
        }
        RecoveryPlan::NoOp => Ok(recovery_plan_mutates_live(plan)),
        RecoveryPlan::PromoteArtifact { source_index } => {
            let recovery_source = artifacts[source_index].clone();
            let discard_name = recovery_discard_artifact_name(
                final_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("tapp"),
                &uuid::Uuid::new_v4().simple().to_string(),
            );
            let discard_path = final_path.with_file_name(discard_name);
            let had_live_path = final_path.exists();
            if had_live_path {
                std::fs::rename(final_path, &discard_path)?;
            }
            if let Err(error) = std::fs::rename(&recovery_source, final_path) {
                if had_live_path {
                    let _ = std::fs::rename(&discard_path, final_path);
                }
                return Err(error);
            }
            if had_live_path {
                let _ = std::fs::remove_dir_all(&discard_path);
            }
            for artifact in recovery_artifacts_to_remove_after_promote(&artifacts, &recovery_source)
            {
                let _ = std::fs::remove_dir_all(artifact);
            }
            Ok(recovery_plan_mutates_live(plan))
        }
    }
}

pub(crate) fn looks_like_tapp_installation(directory: &FsPath) -> bool {
    looks_like_tapp_installation_from_markers(|name| {
        std::fs::symlink_metadata(directory.join(name)).is_ok()
    })
}

/// List filesystem generations that cannot belong to any database row.
pub(crate) fn orphaned_tapp_directories(
    root: &FsPath,
    installed: &std::collections::HashSet<(i32, String)>,
) -> Result<Vec<(i32, String, PathBuf)>, std::io::Error> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut candidates = Vec::new();
    for owner_entry in std::fs::read_dir(root)? {
        let owner_entry = owner_entry?;
        let owner_type = owner_entry.file_type()?;
        if !owner_type.is_dir() || owner_type.is_symlink() {
            continue;
        }
        let owner_name = owner_entry.file_name();
        let Some(owner_name) = owner_name.to_str() else {
            continue;
        };
        let Some(owner_id) = parse_tapp_owner_dir_name(owner_name) else {
            continue;
        };
        for entry in std::fs::read_dir(owner_entry.path())? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }
            let Some(filename) = entry.file_name().to_str().map(String::from) else {
                continue;
            };
            let looks_like = looks_like_tapp_installation(&entry.path());
            let Some((owner_id, tapp_id)) =
                orphan_tapp_key_if_unowned(owner_id, &filename, looks_like, installed)
            else {
                continue;
            };
            candidates.push((owner_id, tapp_id, entry.path()));
        }
    }
    Ok(candidates)
}

pub(crate) async fn cleanup_orphaned_tapp_directories(
    db: &DatabaseConnection,
    installed: &std::collections::HashSet<(i32, String)>,
) -> Result<usize, DbErr> {
    let candidates = orphaned_tapp_directories(&paths().tapps, installed).map_err(|error| {
        tracing::error!(%error, "Failed to inspect Tapp resources");
        DbErr::Custom("Failed to inspect Tapp resources".to_string())
    })?;
    let mut removed = 0;
    for (owner_id, tapp_id, directory) in candidates {
        // Recovery also runs after a live database reconfiguration. Serialize
        // with install/update/uninstall and re-check under the lock so a newly
        // activated, not-yet-committed generation is never mistaken for an
        // orphan.
        let txn = db.begin().await?;
        lock_tapp_lifecycle(&txn, &tapp_id).await?;
        let exists = tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(owner_id))
            .filter(tapps::Column::TappId.eq(&tapp_id))
            .one(&txn)
            .await?
            .is_some();
        if !exists {
            match fs::symlink_metadata(&directory).await {
                Ok(metadata)
                    if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() =>
                {
                    fs::remove_dir_all(&directory).await.map_err(|error| {
                        DbErr::Custom(format!(
                            "Failed to remove orphaned Tapp directory {}: {error}",
                            directory.display()
                        ))
                    })?;
                    removed += 1;
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(DbErr::Custom(format!(
                        "Failed to inspect orphaned Tapp directory {}: {error}",
                        directory.display()
                    )));
                }
            }
        }
        txn.commit().await?;
    }
    Ok(removed)
}

/// Recover filesystem/DB generations (startup and after live DB reconnect).
pub(crate) async fn recover_tapp_filesystem_state(db: &DatabaseConnection) -> Result<usize, DbErr> {
    let installed = tapps::Entity::find().all(db).await?;
    let installed_keys = installed
        .iter()
        .map(|tapp| (tapp.user_id, tapp.tapp_id.clone()))
        .collect::<std::collections::HashSet<_>>();
    let mut recovered = 0;
    for tapp in installed {
        let Ok(final_path) = installed_tapp_dir(&tapp) else {
            tracing::error!(tapp_id = %tapp.tapp_id, "Invalid installed Tapp path during recovery");
            continue;
        };
        match recover_tapp_directory(&final_path, &tapp.manifest, tapp.updated_at) {
            Ok(true) => {
                recovered += 1;
                tracing::warn!(tapp_id = %tapp.tapp_id, owner_id = tapp.user_id, "Recovered interrupted Tapp filesystem transaction");
            }
            Ok(false) => {}
            Err(error) => tracing::error!(
                tapp_id = %tapp.tapp_id,
                owner_id = tapp.user_id,
                %error,
                "Failed to recover interrupted Tapp filesystem transaction"
            ),
        }
    }
    let removed = cleanup_orphaned_tapp_directories(db, &installed_keys).await?;
    if removed > 0 {
        tracing::warn!(removed, "Removed orphaned Tapp filesystem generations");
    }
    recovered += removed;
    Ok(recovered)
}

pub(crate) fn installed_tapp_dir(tapp: &tapps::Model) -> Result<PathBuf, HttpError> {
    tapp_dir_for(tapp.user_id, &tapp.tapp_id)
        .map_err(|_| HttpError(AppError::bad_request("Bad request")))
}

/// 已安装包的结构不符合当前契约时的失败。
///
/// 不能用 5xx：这不是宿主故障。不能用 404：getTappResources 对任何 !ok 都抛错，没有旧 `/code` 回退。
pub(crate) fn unsupported_package_structure(reason: &str) -> HttpError {
    HttpError(AppError::conflict("Tapp package is not usable").with_message(reason))
}

pub(crate) fn resource_path(tapp_dir: &FsPath, relative: &str) -> Option<PathBuf> {
    resource_relative_path(tapp_dir, relative).ok()
}

/// Resolve an installed resource only when every path component remains under
/// the canonical Tapp directory and the target is a regular file. This rejects
/// both final and intermediate symlinks inserted after installation.
pub(crate) fn regular_resource_path(tapp_dir: &FsPath, relative: &str) -> Option<PathBuf> {
    let joined = resource_path(tapp_dir, relative)?;
    let canonical_root = std::fs::canonicalize(tapp_dir).ok()?;
    let canonical_path = std::fs::canonicalize(joined).ok()?;
    if !sandbox_path_matches_relative(&canonical_root, &canonical_path, relative) {
        return None;
    }
    let metadata = std::fs::symlink_metadata(&canonical_path).ok()?;
    metadata.file_type().is_file().then_some(canonical_path)
}

pub(crate) fn regular_resource_directory(tapp_dir: &FsPath, relative: &str) -> Option<PathBuf> {
    let joined = resource_path(tapp_dir, relative)?;
    let canonical_root = std::fs::canonicalize(tapp_dir).ok()?;
    let canonical_path = std::fs::canonicalize(joined).ok()?;
    if !sandbox_path_matches_relative(&canonical_root, &canonical_path, relative) {
        return None;
    }
    let metadata = std::fs::symlink_metadata(&canonical_path).ok()?;
    metadata.file_type().is_dir().then_some(canonical_path)
}

pub(crate) async fn read_tapp_text_resource(
    tapp_dir: &FsPath,
    relative: &str,
) -> Result<String, std::io::Error> {
    let path = regular_resource_path(tapp_dir, relative).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Tapp resource is not a regular in-sandbox file",
        )
    })?;
    fs::read_to_string(path).await
}

pub(crate) async fn write_tapp_resource(
    tapp_dir: &FsPath,
    relative: &str,
    content: impl AsRef<[u8]>,
) -> Result<PathBuf, std::io::Error> {
    let path = resource_path(tapp_dir, relative).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid Tapp resource path",
        )
    })?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(&path, content).await?;
    Ok(path)
}

/// Write base64-encoded package assets for direct install/update.
pub(crate) async fn write_install_assets(
    tapp_dir: &FsPath,
    manifest: &TappManifest,
    assets: &std::collections::HashMap<String, String>,
) -> Result<(), String> {
    use crate::services::tapp_install_resources::validate_write_assets_declaration;

    validate_write_assets_declaration(manifest.assets.as_deref(), assets.keys())?;
    let mut total: u64 = 0;
    for (relative, encoded) in assets {
        validate_asset_path(relative)?;
        let bytes = decode_asset_base64(encoded)?;
        total = crate::services::tapp_install_resources::validate_asset_resource_bytes_with(
            relative,
            bytes.len() as u64,
            total,
            crate::services::tapp_install_resources::AssetBudget::for_manifest(manifest),
        )?;
        write_tapp_resource(tapp_dir, relative, &bytes)
            .await
            .map_err(|_| format!("Failed to save asset: {relative}"))?;
    }
    Ok(())
}

// Domain: services::tapp_prepared_package (path-stable re-export).
pub(crate) use crate::services::tapp_prepared_package::{
    WidgetTemplateContents, validate_widget_template_contents, widget_template_path,
};

/// Post-stage check: every declared resource is a regular in-sandbox file with
/// valid content. Domain path collection + content rules live in
/// [`crate::services::tapp_install_resources`].
pub(crate) fn validate_installed_resources(
    manifest: &TappManifest,
    tapp_dir: &FsPath,
) -> Result<(), String> {
    use crate::services::tapp_install_resources::{
        DeclaredResourceKind, agent_schema_not_found, agent_schema_not_regular, asset_not_found,
        asset_not_regular, collect_declared_install_resources, invalid_declared_path,
        missing_after_install, not_regular_in_sandbox, resource_not_found,
        validate_agent_schema_bytes, validate_text_resource_bytes,
    };

    let mut asset_total: u64 = 0;
    for declared in collect_declared_install_resources(manifest) {
        let relative = declared.relative.as_str();
        match declared.kind {
            DeclaredResourceKind::Text => {
                let joined = resource_path(tapp_dir, relative)
                    .ok_or_else(|| invalid_declared_path(relative))?;
                if !joined.is_file() {
                    return Err(missing_after_install(relative));
                }
                let path = regular_resource_path(tapp_dir, relative)
                    .ok_or_else(|| not_regular_in_sandbox(relative))?;
                let bytes = std::fs::read(path).map_err(|_| resource_not_found(relative))?;
                validate_text_resource_bytes(relative, &bytes)?;
            }
            DeclaredResourceKind::AgentSchema => {
                let path = regular_resource_path(tapp_dir, relative)
                    .ok_or_else(|| agent_schema_not_regular(relative))?;
                let bytes = std::fs::read(path).map_err(|_| agent_schema_not_found(relative))?;
                validate_agent_schema_bytes(relative, &bytes)?;
            }
            DeclaredResourceKind::Asset => {
                let path = regular_resource_path(tapp_dir, relative)
                    .ok_or_else(|| asset_not_regular(relative))?;
                let bytes = std::fs::read(&path).map_err(|_| asset_not_found(relative))?;
                asset_total =
                    crate::services::tapp_install_resources::validate_asset_resource_bytes_with(
                        relative,
                        bytes.len() as u64,
                        asset_total,
                        crate::services::tapp_install_resources::AssetBudget::for_manifest(
                            manifest,
                        ),
                    )?;
            }
        }
    }

    validate_installed_i18n_resources(tapp_dir)?;
    validate_installed_package_modules(tapp_dir)?;
    Ok(())
}

/// 递归列出安装目录内可被 require 的模块相对路径。
///
/// 跳过 `assets/`、不安全的路径分量与符号链接。这里只收集候选文件，层归属
/// 由 manifest 入口加 require 闭包决定，目录名不参与。分发与安装校验共用这一份
/// 遍历，避免同一套扫描实现两遍。
pub(crate) fn collect_package_module_paths(tapp_dir: &FsPath) -> Vec<String> {
    use crate::services::tapp_validation::is_safe_path_component;

    let mut found = Vec::new();
    let mut pending = vec![String::new()];

    while let Some(prefix) = pending.pop() {
        let directory = if prefix.is_empty() {
            Some(tapp_dir.to_path_buf())
        } else {
            regular_resource_directory(tapp_dir, &prefix)
        };
        let Some(directory) = directory else { continue };
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if !is_safe_path_component(&name) || name == ASSET_DIRECTORY {
                continue;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if file_type.is_dir() {
                pending.push(relative);
            } else if file_type.is_file() && name.ends_with(".js") {
                found.push(relative);
            }
        }
    }

    found.sort();
    found
}

/// 包内 `.js` 不需要逐个在 Manifest 里声明，但仍要受检：必须是沙箱内的普通 UTF-8
/// 文件，且 `require` 的目标真实存在。体积上限不在本函数。
pub(crate) fn validate_installed_package_modules(tapp_dir: &FsPath) -> Result<(), String> {
    use crate::services::tapp_install_resources::{
        extract_require_requests, require_target_missing, resolve_require_against_modules,
        validate_text_resource_bytes,
    };

    let paths = collect_package_module_paths(tapp_dir);
    let mut sources: Vec<(String, String)> = Vec::with_capacity(paths.len());
    for relative in paths {
        let path = regular_resource_path(tapp_dir, &relative)
            .ok_or_else(|| format!("Tapp module is not an in-sandbox file: {relative}"))?;
        let bytes =
            std::fs::read(path).map_err(|_| format!("Failed to read Tapp module: {relative}"))?;
        validate_text_resource_bytes(&relative, &bytes)?;
        let source =
            String::from_utf8(bytes).map_err(|_| "Tapp module must be UTF-8".to_string())?;
        sources.push((relative, source));
    }

    let known: std::collections::HashSet<String> =
        sources.iter().map(|(path, _)| path.clone()).collect();
    for (relative, source) in &sources {
        for request in extract_require_requests(source) {
            resolve_require_against_modules(relative, &request, &known)
                .ok_or_else(|| require_target_missing(relative, &request))?;
        }
    }

    Ok(())
}

pub(crate) fn validate_installed_i18n_resources(tapp_dir: &FsPath) -> Result<(), String> {
    use crate::services::tapp_install_resources::{
        validate_i18n_file_bytes, validate_i18n_file_count, validate_i18n_filename,
    };

    let joined = tapp_dir.join("i18n");
    let metadata = match std::fs::symlink_metadata(&joined) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("Failed to inspect Tapp i18n directory".to_string()),
    };
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err("Tapp i18n must be an in-sandbox directory".to_string());
    }
    let directory = regular_resource_directory(tapp_dir, "i18n")
        .ok_or_else(|| "Tapp i18n must be an in-sandbox directory".to_string())?;
    let entries = std::fs::read_dir(directory)
        .map_err(|_| "Failed to read Tapp i18n directory".to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Failed to read Tapp i18n directory".to_string())?;
    validate_i18n_file_count(entries.len())?;
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|_| "Failed to inspect Tapp i18n resource".to_string())?;
        let filename = entry
            .file_name()
            .into_string()
            .map_err(|_| "Tapp i18n filename must be UTF-8".to_string())?;
        validate_i18n_filename(&filename)?;
        if !file_type.is_file() || file_type.is_symlink() {
            return Err(format!("Invalid Tapp i18n resource: {filename}"));
        }
        let relative = format!("i18n/{filename}");
        let path = regular_resource_path(tapp_dir, &relative)
            .ok_or_else(|| format!("Invalid Tapp i18n resource: {filename}"))?;
        let bytes = std::fs::read(path)
            .map_err(|_| format!("Failed to read Tapp i18n resource: {filename}"))?;
        validate_i18n_file_bytes(&filename, &bytes)?;
    }
    Ok(())
}

pub(crate) fn archive_entry_path(tapp_dir: &FsPath, entry_name: &str) -> Result<PathBuf, String> {
    archive_entry_relative_path(tapp_dir, entry_name)
}

pub(crate) fn validate_tapp_archive<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<(), String> {
    validate_tapp_archive_with(
        archive,
        crate::services::tapp_install_resources::ArchiveBudget::ceiling(),
    )
}

pub(crate) fn validate_tapp_archive_with<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    budget: crate::services::tapp_install_resources::ArchiveBudget,
) -> Result<(), String> {
    use crate::services::tapp_install_resources::{
        validate_archive_entry_count_with, validate_archive_entry_with,
    };

    validate_archive_entry_count_with(archive.len(), budget)?;

    let mut total_size = 0_u64;
    let mut paths = std::collections::HashSet::new();
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|error| {
            tracing::error!(%error, "Invalid Tapp archive entry");
            "Invalid Tapp archive".to_string()
        })?;
        let name = file.name().trim_end_matches('/');
        total_size = validate_archive_entry_with(
            name,
            file.is_dir(),
            file.size(),
            &mut paths,
            total_size,
            budget,
        )?;
    }

    Ok(())
}

pub(crate) fn append_directory_to_zip<W: std::io::Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    root: &FsPath,
    directory: &FsPath,
    options: zip::write::SimpleFileOptions,
) -> Result<(), std::io::Error> {
    use std::io::{Read, Write};

    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        // Installed resources are regular files. Never follow a manually inserted
        // symlink while exporting, because it may point outside the Tapp directory.
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            append_directory_to_zip(zip, root, &path, options)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }

        let relative = path.strip_prefix(root).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Tapp export path escaped root",
            )
        })?;
        if relative == FsPath::new(TAPP_INSTALL_STATE_FILE) {
            continue;
        }
        let filename = relative.to_string_lossy().replace('\\', "/");
        let mut file = std::fs::File::open(&path)?;
        let mut content = Vec::new();
        file.read_to_end(&mut content)?;
        zip.start_file(filename, options)?;
        zip.write_all(&content)?;
    }

    Ok(())
}
