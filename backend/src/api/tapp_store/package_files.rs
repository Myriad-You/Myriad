//! Filesystem staging, recovery, package resources and archive boundaries.

use super::{
    decode_asset_base64, is_safe_path_component, lock_tapp_lifecycle, validate_asset_path,
    validate_inline_data_schema, validate_resource_path, validate_tapp_id, TappManifest,
    MAX_AGENT_SCHEMA_RESOURCE_BYTES, MAX_TAPP_ARCHIVE_FILES, MAX_TAPP_ARCHIVE_UNCOMPRESSED_BYTES,
    MAX_TAPP_ASSETS_TOTAL_BYTES, MAX_TAPP_ASSET_BYTES, MAX_TAPP_I18N_FILES,
    MAX_TAPP_I18N_RESOURCE_BYTES, MAX_TAPP_RESOURCE_BYTES,
};
use axum::http::StatusCode;
use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, TransactionTrait};
use std::path::{Path as FsPath, PathBuf};
use tokio::fs;

use crate::models::entities::tapps;
use crate::services::data_paths::paths;

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
        let path = parent.join(format!(".{name}.staging-{}", uuid::Uuid::new_v4().simple()));
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
            let backup = parent.join(format!(".{name}.backup-{}", uuid::Uuid::new_v4().simple()));
            match fs::rename(final_path, &backup).await {
                Ok(()) => Some(backup),
                Err(rename_error) => {
                    tracing::error!(
                        from = %final_path.display(),
                        to = %backup.display(),
                        kind = ?rename_error.kind(),
                        %rename_error,
                        "Failed to rename live Tapp dir to backup during activate; attempting remove"
                    );
                    // Orphan leftover after failed uninstall quarantine, or a
                    // non-renameable live dir: free the final path so staging
                    // can take its place. Prefer remove over leaving install stuck.
                    match remove_path_best_effort(final_path).await {
                        Ok(()) => None,
                        Err(remove_error) => {
                            tracing::error!(
                                path = %final_path.display(),
                                rename_kind = ?rename_error.kind(),
                                %rename_error,
                                remove_kind = ?remove_error.kind(),
                                %remove_error,
                                "Cannot free final Tapp path for activate"
                            );
                            return Err(std::io::Error::new(
                                rename_error.kind(),
                                format!(
                                    "cannot free final Tapp path {}: rename failed ({rename_error}); remove failed ({remove_error})",
                                    final_path.display()
                                ),
                            ));
                        }
                    }
                }
            }
        } else {
            None
        };

        if let Err(error) = fs::rename(&self.path, final_path).await {
            tracing::error!(
                from = %self.path.display(),
                to = %final_path.display(),
                kind = ?error.kind(),
                %error,
                "Failed to rename staging Tapp dir to final path"
            );
            if let Some(backup) = &backup_path {
                if let Err(restore_error) = fs::rename(backup, final_path).await {
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

pub(crate) const TAPP_INSTALL_STATE_FILE: &str = ".myriad-install-state.json";

impl ActivatedTappDir {
    pub(super) async fn commit(mut self) {
        if let Some(backup) = self.backup_path.take() {
            if let Err(error) = fs::remove_dir_all(&backup).await {
                tracing::warn!(path = %backup.display(), %error, "Failed to remove old Tapp backup");
            }
        }
    }

    pub(super) async fn rollback(mut self) {
        let _ = fs::remove_dir_all(&self.final_path).await;
        if let Some(backup) = self.backup_path.take() {
            let _ = fs::rename(backup, &self.final_path).await;
        }
    }
}

pub(crate) fn directory_manifest_matches(directory: &FsPath, expected: &serde_json::Value) -> bool {
    let Ok(content) = std::fs::read_to_string(directory.join("manifest.json")) else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(&content).is_ok_and(|value| value == *expected)
}

pub(crate) fn write_install_generation(
    directory: &FsPath,
    updated_at: chrono::DateTime<chrono::FixedOffset>,
) -> Result<(), std::io::Error> {
    let value = serde_json::json!({ "updatedAtMicros": updated_at.timestamp_micros() });
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
            .and_then(|value| {
                value
                    .get("updatedAtMicros")
                    .and_then(serde_json::Value::as_i64)
            })
            == Some(expected_updated_at.timestamp_micros());
    }
    // Compatibility for installations created before generation markers.
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
    let prefixes = [
        format!(".{name}.staging-"),
        format!(".{name}.backup-"),
        format!(".{name}.uninstall-"),
        format!(".{name}.recovery-discard-"),
    ];
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
        if prefixes.iter().any(|prefix| filename.starts_with(prefix)) {
            artifacts.push(entry.path());
        }
    }
    Ok(artifacts)
}

/// Live install dir and lifecycle artifacts that should not remain when the DB
/// has no row for this owner/tapp_id (post-uninstall orphans, partial activate).
///
/// Pure path-selection helper used by reinstall cleanup and unit tests.
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

/// Whether `paths` from [`reinstall_orphan_paths`] indicate orphan filesystem state.
pub(crate) fn has_reinstall_orphan_state(paths: &[PathBuf]) -> bool {
    !paths.is_empty()
}

/// Best-effort remove leftover live dir and lifecycle artifacts before install
/// staging/activate. Caller must ensure there is no conflicting DB install row.
///
/// Cleanup is deliberately non-fatal. Older deployments may have created Tapp
/// contents as another UID, making recursive deletion fail even though the
/// owner directory still permits an atomic rename. `TappDirStage::activate`
/// can quarantine that live path by renaming it, so refusing to stage here
/// would turn recoverable ownership drift into a permanent install failure.
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
        if preserve.is_some_and(|keep| keep == path.as_path()) {
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
    match error.kind() {
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub(crate) fn tapp_filesystem_error_message(action: &str, error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem => format!(
            "Tapp storage is not writable by the backend service account; repair the backend data volume ownership/permissions and retry ({action}: {error})"
        ),
        _ => format!("{action}: {error}"),
    }
}

/// Add owner/mode context for storage failures without following symlinks.
pub(crate) fn log_tapp_filesystem_access(path: &FsPath, error: &std::io::Error) {
    if !matches!(
        error.kind(),
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
    ) {
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
pub(crate) fn recover_tapp_directory(
    final_path: &FsPath,
    expected_manifest: &serde_json::Value,
    expected_updated_at: chrono::DateTime<chrono::FixedOffset>,
) -> Result<bool, std::io::Error> {
    let mut artifacts = lifecycle_artifact_directories(final_path)?;
    // A backup/uninstall quarantine is the authoritative pre-transaction
    // generation. Consider staging only after those recovery sources.
    artifacts.sort_by_key(|path| {
        path.file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.contains(".staging-"))
    });
    if directory_generation_matches(final_path, expected_manifest, expected_updated_at) {
        for artifact in artifacts {
            std::fs::remove_dir_all(artifact)?;
        }
        return Ok(false);
    }

    let Some(recovery_source) = artifacts
        .iter()
        .find(|path| directory_generation_matches(path, expected_manifest, expected_updated_at))
        .cloned()
    else {
        return Ok(false);
    };

    let discard_path = final_path.with_file_name(format!(
        ".{}.recovery-discard-{}",
        final_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("tapp"),
        uuid::Uuid::new_v4().simple()
    ));
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
    for artifact in artifacts {
        if artifact != recovery_source {
            let _ = std::fs::remove_dir_all(artifact);
        }
    }
    Ok(true)
}

pub(crate) fn lifecycle_artifact_tapp_id(filename: &str) -> Option<&str> {
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

pub(crate) fn looks_like_tapp_installation(directory: &FsPath) -> bool {
    ["manifest.json", TAPP_INSTALL_STATE_FILE]
        .into_iter()
        .any(|name| std::fs::symlink_metadata(directory.join(name)).is_ok())
}

/// Remove filesystem generations that cannot belong to any database row.
/// Artifacts for an installed key are deliberately retained when normal
/// recovery cannot identify the expected generation, avoiding destructive
/// guesses in the presence of partial/manual damage.
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
        let Some(owner_name) = owner_entry.file_name().to_str().map(String::from) else {
            continue;
        };
        let Ok(owner_id) = owner_name.parse::<i32>() else {
            continue;
        };
        if owner_id < 0 || owner_id.to_string() != owner_name {
            continue;
        }
        for entry in std::fs::read_dir(owner_entry.path())? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }
            let Some(filename) = entry.file_name().to_str().map(String::from) else {
                continue;
            };
            let tapp_id = lifecycle_artifact_tapp_id(&filename).map(String::from);
            let live_tapp_id = if tapp_id.is_none()
                && validate_tapp_id(&filename).is_ok()
                && looks_like_tapp_installation(&entry.path())
            {
                Some(filename.clone())
            } else {
                None
            };
            let Some(tapp_id) = tapp_id.or(live_tapp_id) else {
                continue;
            };
            if installed.contains(&(owner_id, tapp_id.clone())) {
                continue;
            }
            candidates.push((owner_id, tapp_id, entry.path()));
        }
    }
    Ok(candidates)
}

pub(crate) async fn cleanup_orphaned_tapp_directories(
    db: &DatabaseConnection,
    installed: &std::collections::HashSet<(i32, String)>,
) -> Result<usize, DbErr> {
    let candidates = orphaned_tapp_directories(&paths().tapps, installed)
        .map_err(|error| DbErr::Custom(format!("Failed to inspect Tapp resources: {error}")))?;
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
                    )))
                }
            }
        }
        txn.commit().await?;
    }
    Ok(removed)
}

/// Startup recovery for filesystem/DB transactions interrupted between the
/// atomic directory rename and the PostgreSQL commit.
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

pub(crate) fn installed_tapp_dir(tapp: &tapps::Model) -> Result<PathBuf, StatusCode> {
    tapp_dir_for(tapp.user_id, &tapp.tapp_id).map_err(|_| StatusCode::BAD_REQUEST)
}

pub(crate) fn installed_code_path(tapp: &tapps::Model) -> Result<PathBuf, StatusCode> {
    // 新安装遵循 Manifest 的 main。旧安装可能曾把任意入口统一写为根目录
    // main.js/index.js，因此仅在 Manifest 路径不存在时回退持久化元数据。
    if let Some(main) = tapp.manifest.get("main").and_then(|value| value.as_str()) {
        if let Some(path) = regular_resource_path(&installed_tapp_dir(tapp)?, main) {
            return Ok(path);
        }
    }

    let stored_code_path = PathBuf::from(&tapp.code_path);
    let filename = stored_code_path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| matches!(*value, "main.js" | "index.js"))
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    regular_resource_path(&installed_tapp_dir(tapp)?, filename)
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)
}

pub(crate) fn resource_path(tapp_dir: &FsPath, relative: &str) -> Option<PathBuf> {
    validate_resource_path(relative).ok()?;
    Some(tapp_dir.join(relative))
}

/// Resolve an installed resource only when every path component remains under
/// the canonical Tapp directory and the target is a regular file. This rejects
/// both final and intermediate symlinks inserted after installation.
pub(crate) fn regular_resource_path(tapp_dir: &FsPath, relative: &str) -> Option<PathBuf> {
    let joined = resource_path(tapp_dir, relative)?;
    let canonical_root = std::fs::canonicalize(tapp_dir).ok()?;
    let canonical_path = std::fs::canonicalize(joined).ok()?;
    if canonical_path != canonical_root.join(relative) {
        return None;
    }
    let metadata = std::fs::symlink_metadata(&canonical_path).ok()?;
    metadata.file_type().is_file().then_some(canonical_path)
}

pub(crate) fn regular_resource_directory(tapp_dir: &FsPath, relative: &str) -> Option<PathBuf> {
    let joined = resource_path(tapp_dir, relative)?;
    let canonical_root = std::fs::canonicalize(tapp_dir).ok()?;
    let canonical_path = std::fs::canonicalize(joined).ok()?;
    if canonical_path != canonical_root.join(relative) {
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
    let declared: std::collections::HashSet<&str> = manifest
        .assets
        .as_ref()
        .map(|list| list.iter().map(String::as_str).collect())
        .unwrap_or_default();
    if declared.is_empty() && !assets.is_empty() {
        return Err("assets payload requires manifest.assets declarations".to_string());
    }
    let mut total: u64 = 0;
    for (relative, encoded) in assets {
        validate_asset_path(relative)?;
        if !declared.contains(relative.as_str()) {
            return Err(format!(
                "Asset path is not declared in manifest.assets: {relative}"
            ));
        }
        let bytes = decode_asset_base64(encoded)?;
        let size = bytes.len() as u64;
        if size > MAX_TAPP_ASSET_BYTES {
            return Err(format!(
                "Tapp asset exceeds {MAX_TAPP_ASSET_BYTES} bytes: {relative}"
            ));
        }
        total = total
            .checked_add(size)
            .ok_or_else(|| "Tapp assets total size overflow".to_string())?;
        if total > MAX_TAPP_ASSETS_TOTAL_BYTES {
            return Err(format!(
                "Tapp assets total size exceeds {MAX_TAPP_ASSETS_TOTAL_BYTES} bytes"
            ));
        }
        write_tapp_resource(tapp_dir, relative, &bytes)
            .await
            .map_err(|_| format!("Failed to save asset: {relative}"))?;
    }
    Ok(())
}

pub(crate) async fn copy_regular_tapp_directory(
    source: &FsPath,
    destination: &FsPath,
) -> Result<(), std::io::Error> {
    let mut pending = vec![(source.to_path_buf(), destination.to_path_buf())];
    while let Some((source_dir, destination_dir)) = pending.pop() {
        let mut entries = fs::read_dir(&source_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            if file_type.is_symlink() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "Tapp resource directory contains a symbolic link",
                ));
            }
            let destination_path = destination_dir.join(entry.file_name());
            if file_type.is_dir() {
                fs::create_dir(&destination_path).await?;
                pending.push((entry.path(), destination_path));
            } else if file_type.is_file() {
                fs::copy(entry.path(), destination_path).await?;
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Tapp resource directory contains a non-regular entry",
                ));
            }
        }
    }
    Ok(())
}

pub(crate) type WidgetTemplateContents =
    std::collections::HashMap<String, std::collections::HashMap<String, String>>;

pub(crate) fn widget_template_path<'a>(
    manifest: &'a TappManifest,
    widget_id: &str,
    size: &str,
) -> Option<&'a str> {
    manifest
        .widgets
        .as_ref()
        .into_iter()
        .flatten()
        .find(|widget| widget.id == widget_id)
        .and_then(|widget| widget.templates.as_ref())
        .and_then(|templates| templates.get(size))
        .map(String::as_str)
}

pub(crate) fn validate_widget_template_contents(
    manifest: &TappManifest,
    contents: &WidgetTemplateContents,
) -> Result<(), String> {
    for (widget_id, templates) in contents {
        let widget = manifest
            .widgets
            .as_ref()
            .and_then(|widgets| widgets.iter().find(|widget| widget.id == *widget_id))
            .ok_or_else(|| format!("Widget template references unknown Widget: {widget_id}"))?;
        for size in templates.keys() {
            if !widget.sizes.contains(size)
                || widget_template_path(manifest, widget_id, size).is_none()
            {
                return Err(format!(
                    "Widget template content has no matching Manifest path: {widget_id}/{size}"
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_installed_resources(
    manifest: &TappManifest,
    tapp_dir: &FsPath,
) -> Result<(), String> {
    let mut resources = vec![manifest.main.as_str()];
    resources.extend(
        [
            manifest.styles.as_deref(),
            manifest.widget_styles.as_deref(),
            manifest.page_styles.as_deref(),
            manifest.page_template.as_deref(),
        ]
        .into_iter()
        .flatten(),
    );
    if let Some(widgets) = &manifest.widgets {
        for widget in widgets {
            if let Some(templates) = &widget.templates {
                resources.extend(templates.values().map(String::as_str));
            }
        }
    }
    for relative in resources {
        let path = regular_resource_path(tapp_dir, relative)
            .ok_or_else(|| format!("Declared Tapp resource is not a regular file: {relative}"))?;
        let bytes = std::fs::read(path)
            .map_err(|_| format!("Declared Tapp resource not found: {relative}"))?;
        std::str::from_utf8(&bytes)
            .map_err(|_| format!("Declared Tapp resource is not UTF-8 text: {relative}"))?;
    }

    if let Some(modules) = &manifest.page_modules {
        for module in modules {
            let relative = format!("page/{module}");
            let path = regular_resource_path(tapp_dir, &relative).ok_or_else(|| {
                format!("Declared Tapp resource is not a regular file: {relative}")
            })?;
            let bytes = std::fs::read(path)
                .map_err(|_| format!("Declared Tapp resource not found: {relative}"))?;
            std::str::from_utf8(&bytes)
                .map_err(|_| format!("Declared Tapp resource is not UTF-8 text: {relative}"))?;
        }
    }
    if let Some(agent) = &manifest.agent {
        for interaction in &agent.interactions {
            for relative in [
                interaction.input_schema.as_deref(),
                interaction.result_schema.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                let path = regular_resource_path(tapp_dir, relative).ok_or_else(|| {
                    format!("Declared Agent schema is not a regular file: {relative}")
                })?;
                let bytes = std::fs::read(path)
                    .map_err(|_| format!("Declared Agent schema not found: {relative}"))?;
                if bytes.len() > MAX_AGENT_SCHEMA_RESOURCE_BYTES {
                    return Err(format!(
                        "Agent schema exceeds {MAX_AGENT_SCHEMA_RESOURCE_BYTES} bytes: {relative}"
                    ));
                }
                let schema = serde_json::from_slice::<serde_json::Value>(&bytes)
                    .map_err(|_| format!("Agent schema is not valid JSON: {relative}"))?;
                validate_inline_data_schema(&schema)
                    .map_err(|error| format!("Invalid Agent schema {relative}: {error}"))?;
            }
        }
    }
    if let Some(assets) = &manifest.assets {
        let mut total: u64 = 0;
        for relative in assets {
            validate_asset_path(relative)?;
            let path = regular_resource_path(tapp_dir, relative)
                .ok_or_else(|| format!("Declared Tapp asset is not a regular file: {relative}"))?;
            let bytes = std::fs::read(&path)
                .map_err(|_| format!("Declared Tapp asset not found: {relative}"))?;
            let size = bytes.len() as u64;
            if size > MAX_TAPP_ASSET_BYTES {
                return Err(format!(
                    "Tapp asset exceeds {MAX_TAPP_ASSET_BYTES} bytes: {relative}"
                ));
            }
            total = total
                .checked_add(size)
                .ok_or_else(|| "Tapp assets total size overflow".to_string())?;
            if total > MAX_TAPP_ASSETS_TOTAL_BYTES {
                return Err(format!(
                    "Tapp assets total size exceeds {MAX_TAPP_ASSETS_TOTAL_BYTES} bytes"
                ));
            }
        }
    }
    validate_installed_i18n_resources(tapp_dir)?;
    Ok(())
}

pub(crate) fn validate_installed_i18n_resources(tapp_dir: &FsPath) -> Result<(), String> {
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
    if entries.len() > MAX_TAPP_I18N_FILES {
        return Err(format!(
            "Tapp i18n accepts at most {MAX_TAPP_I18N_FILES} locale files"
        ));
    }
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|_| "Failed to inspect Tapp i18n resource".to_string())?;
        let filename = entry
            .file_name()
            .into_string()
            .map_err(|_| "Tapp i18n filename must be UTF-8".to_string())?;
        let Some(locale) = filename.strip_suffix(".json") else {
            return Err(format!(
                "Tapp i18n resource must be a JSON file: {filename}"
            ));
        };
        if !file_type.is_file()
            || file_type.is_symlink()
            || !is_safe_path_component(&filename)
            || !is_safe_path_component(locale)
        {
            return Err(format!("Invalid Tapp i18n resource: {filename}"));
        }
        let relative = format!("i18n/{filename}");
        let path = regular_resource_path(tapp_dir, &relative)
            .ok_or_else(|| format!("Invalid Tapp i18n resource: {filename}"))?;
        let bytes = std::fs::read(path)
            .map_err(|_| format!("Failed to read Tapp i18n resource: {filename}"))?;
        if bytes.len() > MAX_TAPP_I18N_RESOURCE_BYTES {
            return Err(format!(
                "Tapp i18n resource exceeds {MAX_TAPP_I18N_RESOURCE_BYTES} bytes: {filename}"
            ));
        }
        let value = serde_json::from_slice::<serde_json::Value>(&bytes)
            .map_err(|_| format!("Tapp i18n resource is not valid JSON: {filename}"))?;
        if !value.is_object() {
            return Err(format!(
                "Tapp i18n locale must contain a JSON object: {filename}"
            ));
        }
    }
    Ok(())
}

pub(crate) fn archive_entry_path(tapp_dir: &FsPath, entry_name: &str) -> Result<PathBuf, String> {
    let relative = entry_name.trim_end_matches('/');
    validate_resource_path(relative)?;
    Ok(tapp_dir.join(relative))
}

pub(crate) fn validate_tapp_archive<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<(), String> {
    if archive.len() > MAX_TAPP_ARCHIVE_FILES {
        return Err(format!(
            "Tapp archive contains too many entries (max {MAX_TAPP_ARCHIVE_FILES})"
        ));
    }

    let mut total_size = 0_u64;
    let mut paths = std::collections::HashSet::new();
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|error| format!("Invalid Tapp archive entry: {error}"))?;
        let name = file.name().trim_end_matches('/');
        validate_resource_path(name)?;
        if !paths.insert(name.to_string()) {
            return Err(format!("Duplicate Tapp archive entry: {name}"));
        }
        if file.is_dir() {
            continue;
        }
        if file.size() > MAX_TAPP_RESOURCE_BYTES {
            return Err(format!(
                "Tapp archive entry is too large: {name} (max {MAX_TAPP_RESOURCE_BYTES} bytes)"
            ));
        }
        total_size = total_size
            .checked_add(file.size())
            .ok_or_else(|| "Tapp archive size overflow".to_string())?;
        if total_size > MAX_TAPP_ARCHIVE_UNCOMPRESSED_BYTES {
            return Err(format!(
                "Tapp archive expands beyond {MAX_TAPP_ARCHIVE_UNCOMPRESSED_BYTES} bytes"
            ));
        }
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
