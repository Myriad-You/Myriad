//! pgdata file-level snapshots. See spec §9.
//!
//! Steps (caller must ensure postgres is stopped before invoking `create`):
//!
//! 1. `sync -f pgdata`
//! 2. `cp -a --reflink=auto pgdata snapshots/<id>.tmp`
//! 3. fsync the snapshot
//! 4. rename to `snapshots/<id>`
//! 5. fsync snapshots dir
//! 6. record SnapshotMeta in snapshots.json
//!
//! Restore is the reverse: stop postgres, then put the snapshot back into `pgdata`.
//!
//! Production overlays writable pgdata below a read-only deployment-root bind.
//! Mount-point / read-only-parent rename failures use staged in-place replacement;
//! installations with a writable parent can still use the rename path.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;

#[cfg(test)]
use chrono::DateTime;
use chrono::Utc;
use sha2::{Digest, Sha256};
use tokio::process::Command;
use tracing::{info, warn};
use walkdir::WalkDir;

use crate::error::{Result, UpdaterError};
use crate::state::{SnapshotMeta, StateDir};
use crate::version::DeployTag;

pub struct SnapshotManager<'a> {
    pub state: &'a StateDir,
    pub pgdata: PathBuf,
}

/// Counts used by API diagnostics (`GET /snapshots`, `POST /prefs`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetentionCounts {
    pub total_count: u32,
    /// Non-keep, non-in-use snapshots (subject to keep-N).
    pub eligible_count: u32,
    /// `keep=true` and/or in-use/rescue-protected snapshots.
    pub protected_count: u32,
}

/// Safe directory names under `state/snapshots/` that orphan sweep may remove.
///
/// Allows opaque ids, create leftovers (`{id}.tmp`), and in-place restore
/// staging names (`broken-inplace-*`, `restore-stage-*`). Rejects anything
/// that could escape the directory via `..` or separators.
pub fn is_safe_snapshot_dir_name(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    // Create leftovers: `{id}.tmp` where id itself is a valid opaque token.
    if let Some(base) = name.strip_suffix(".tmp") {
        return !base.is_empty()
            && base
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// Reject any snapshot id that could influence path resolution.
///
/// Every snapshot id is an opaque, updater-generated token; it is only ever
/// joined onto `state/snapshots/`. Restricting it to `[A-Za-z0-9_-]` means
/// `Path::join` can never escape that directory — no `..`, no absolute paths,
/// no separators, no NUL.
///
/// `delete` has always enforced this. `restore` did not: it took the id
/// straight from the rollback request body and only checked whether the joined
/// path existed, so an authenticated caller could point a restore at any
/// directory on the host and have its contents copied over `pgdata`.
pub fn validate_snapshot_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err(UpdaterError::InvalidInput(format!(
            "invalid snapshot id: {id:?}"
        )));
    }
    Ok(())
}

impl<'a> SnapshotManager<'a> {
    /// Snapshot pgdata. Returns the snapshot id (matches caller-supplied job id for traceability).
    /// Caller is responsible for stopping postgres beforehand.
    pub async fn create(
        &self,
        snapshot_id: &str,
        source_version: Option<DeployTag>,
    ) -> Result<SnapshotMeta> {
        validate_snapshot_id(snapshot_id)?;
        crate::probe::filesystem::require_pgdata(&self.pgdata)?;
        let snapshots_dir = self.state.snapshots_dir();
        std::fs::create_dir_all(&snapshots_dir)?;

        let tmp = snapshots_dir.join(format!("{snapshot_id}.tmp"));
        let final_path = snapshots_dir.join(snapshot_id);

        if crate::probe::filesystem::path_is_present(&final_path)? {
            return Err(UpdaterError::Precondition(format!(
                "snapshot {snapshot_id} already exists"
            )));
        }
        if crate::probe::filesystem::path_is_present(&tmp)? {
            std::fs::remove_dir_all(&tmp)?;
        }

        // 1. sync -f pgdata: best-effort.
        let _ = Command::new("sync")
            .arg("-f")
            .arg(&self.pgdata)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;

        // 2. cp -a (--reflink=auto on Linux; plain -a elsewhere / on fallback)
        if let Err(e) = copy_tree(&self.pgdata, &tmp).await {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(UpdaterError::Internal(anyhow::anyhow!(
                "cp pgdata → snapshot failed: {e}"
            )));
        }

        // 3 + 4. fsync tmp dir then rename.
        fsync_dir(&tmp)?;
        std::fs::rename(&tmp, &final_path)?;
        // 5. fsync snapshots dir.
        fsync_dir(&snapshots_dir)?;

        // 6. record metadata.
        let (size, count, sample) = measure_and_sample(&final_path)?;
        let meta = SnapshotMeta {
            id: snapshot_id.to_string(),
            created_at: Utc::now(),
            source_version,
            size_bytes: size,
            file_count: count,
            keep: false,
            sample_sha256: Some(sample),
        };
        let mut sf = self.state.read_snapshots()?;
        sf.items.push(meta.clone());
        self.state.write_snapshots(&sf)?;
        info!(snapshot = %snapshot_id, size, count, "snapshot created");
        Ok(meta)
    }

    /// Restore pgdata from snapshot. Caller must stop postgres first.
    ///
    /// Strategy:
    /// 1. Prefer renaming the existing pgdata directory aside (fast, clean).
    /// 2. If rename fails with EBUSY or EROFS (mount point / read-only parent),
    ///    fall back to in-place content replace: safety-copy current contents under
    ///    `state/snapshots/`, wipe children of the mount, then copy snapshot contents in.
    ///
    /// The snapshot itself is always *copied* (never renamed away) so retry remains possible.
    pub async fn restore(&self, snapshot_id: &str) -> Result<()> {
        // Opaque-id rule first: the id must not be able to steer path resolution.
        validate_snapshot_id(snapshot_id)?;

        // Then require it to be a snapshot we actually took. `exists()` alone
        // would accept any directory that happens to sit under snapshots/.
        if !self
            .state
            .read_snapshots()?
            .items
            .iter()
            .any(|m| m.id == snapshot_id)
        {
            return Err(UpdaterError::NotFound(format!(
                "snapshot {snapshot_id} is not present in snapshots.json"
            )));
        }

        crate::probe::filesystem::require_pgdata(&self.pgdata)?;
        let snap_path = self.state.snapshots_dir().join(snapshot_id);
        if !crate::probe::filesystem::path_is_present(&snap_path)? {
            return Err(UpdaterError::NotFound(format!(
                "snapshot {snapshot_id} does not exist on disk"
            )));
        }

        let ts = Utc::now().format("%Y%m%dT%H%M%SZ");
        let broken_sibling = self.pgdata.with_extension(format!("broken.{ts}"));

        if crate::probe::filesystem::path_is_present(&self.pgdata)? {
            match std::fs::rename(&self.pgdata, &broken_sibling) {
                Ok(()) => {
                    info!(
                        from = %self.pgdata.display(),
                        to = %broken_sibling.display(),
                        "pgdata moved aside via rename"
                    );
                    if let Err(e) = copy_tree(&snap_path, &self.pgdata).await {
                        // Best-effort undo of the rename.
                        if crate::probe::filesystem::path_is_present(&broken_sibling).unwrap_or(false)
                            && matches!(
                                crate::probe::filesystem::path_is_present(&self.pgdata),
                                Ok(false)
                            )
                        {
                            let _ = std::fs::rename(&broken_sibling, &self.pgdata);
                        }
                        return Err(e);
                    }
                    fsync_dir(&self.pgdata)?;
                    if let Err(error) =
                        self.adopt_aside_directory(&broken_sibling, &format!("aside-{ts}"))
                    {
                        warn!(
                            err = %error,
                            aside = %broken_sibling.display(),
                            "pgdata restored but aside copy was not registered as a protected snapshot"
                        );
                    }
                    info!(snapshot = %snapshot_id, "pgdata restored from snapshot (rename path)");
                    return Ok(());
                }
                Err(e) if requires_in_place_restore(&e) => {
                    warn!(
                        err = %e,
                        path = %self.pgdata.display(),
                        "rename of pgdata failed (likely bind-mount point); using in-place restore"
                    );
                    return self
                        .restore_in_place(&snap_path, snapshot_id, &ts.to_string())
                        .await;
                }
                Err(e) => {
                    return Err(UpdaterError::Internal(anyhow::anyhow!(
                        "rename pgdata aside failed: {e}"
                    )));
                }
            }
        }

        // No existing pgdata — just materialize the snapshot.
        copy_tree(&snap_path, &self.pgdata).await?;
        fsync_dir(&self.pgdata)?;
        info!(snapshot = %snapshot_id, "pgdata restored from snapshot (empty target)");
        Ok(())
    }

    /// In-place restore when `pgdata` cannot be renamed (mount point / read-only parent).
    async fn restore_in_place(&self, snap_path: &Path, snapshot_id: &str, ts: &str) -> Result<()> {
        std::fs::create_dir_all(&self.pgdata)?;

        // Safety copy of current (possibly half-upgraded) contents so operators can recover.
        let safety = self
            .state
            .snapshots_dir()
            .join(format!("broken-inplace-{ts}"));
        if crate::probe::filesystem::path_is_present(&safety)? {
            std::fs::remove_dir_all(&safety)?;
        }
        if dir_has_entries(&self.pgdata)? {
            info!(
                safety = %safety.display(),
                "copying current pgdata contents aside before in-place restore"
            );
            if let Err(e) = copy_tree(&self.pgdata, &safety).await {
                let _ = std::fs::remove_dir_all(&safety);
                // Fail closed: never wipe pgdata without a safety copy of non-empty data.
                return Err(UpdaterError::Internal(anyhow::anyhow!(
                    "safety copy of current pgdata failed before in-place restore: {e}; \
                     refusing to clear pgdata (snapshot {snapshot_id})"
                )));
            }
        }

        // Stage into a temp dir under snapshots, then swap contents — if stage fails,
        // existing pgdata is still intact.
        let stage = self
            .state
            .snapshots_dir()
            .join(format!("restore-stage-{ts}"));
        if crate::probe::filesystem::path_is_present(&stage)? {
            std::fs::remove_dir_all(&stage)?;
        }
        std::fs::create_dir_all(&stage)?;
        if let Err(e) = copy_tree_into(snap_path, &stage).await {
            let _ = std::fs::remove_dir_all(&stage);
            return Err(UpdaterError::Internal(anyhow::anyhow!(
                "staged snapshot copy failed (pgdata untouched): {e}"
            )));
        }

        // Wipe children of the mount point (cannot remove the mount itself).
        clear_dir_contents(&self.pgdata)?;

        // `cp -a stage/. dest/` into the existing mount directory.
        if let Err(e) = copy_tree_into(&stage, &self.pgdata).await {
            // Attempt to put safety copy back if we have one.
            if crate::probe::filesystem::path_is_present(&safety).unwrap_or(false) {
                let _ = clear_dir_contents(&self.pgdata);
                let _ = copy_tree_into(&safety, &self.pgdata).await;
            }
            let _ = std::fs::remove_dir_all(&stage);
            return Err(UpdaterError::Internal(anyhow::anyhow!(
                "in-place restore into pgdata failed after wipe: {e}"
            )));
        }
        let _ = std::fs::remove_dir_all(&stage);
        fsync_dir(&self.pgdata)?;
        if crate::probe::filesystem::path_is_present(&safety).unwrap_or(false)
            && let Err(error) = self.adopt_aside_directory(&safety, &format!("aside-{ts}"))
        {
            warn!(
                err = %error,
                aside = %safety.display(),
                "in-place restore succeeded but safety copy was not registered as a protected snapshot"
            );
        }
        info!(
            snapshot = %snapshot_id,
            safety = %safety.display(),
            "pgdata restored from snapshot (in-place / mount-point path)"
        );
        Ok(())
    }

    /// Move a restore safety copy under `snapshots/` and pin it (`keep=true`)
    /// so retention cannot drop the only pre-restore pgdata until an operator
    /// clears the pin.
    fn adopt_aside_directory(&self, src: &Path, id: &str) -> Result<()> {
        validate_snapshot_id(id)?;
        if !crate::probe::filesystem::path_is_present(src)? {
            return Ok(());
        }
        let dest = self.state.snapshots_dir().join(id);
        if src != dest.as_path() {
            if crate::probe::filesystem::path_is_present(&dest)? {
                return Err(UpdaterError::Precondition(format!(
                    "aside snapshot {id} already exists"
                )));
            }
            std::fs::create_dir_all(self.state.snapshots_dir())?;
            std::fs::rename(src, &dest).map_err(|e| {
                UpdaterError::Internal(anyhow::anyhow!(
                    "move pgdata aside into snapshots/{id} failed: {e}"
                ))
            })?;
            fsync_dir(&dest)?;
            fsync_dir(&self.state.snapshots_dir())?;
        }
        let (size, count, sample) = measure_and_sample(&dest)?;
        let mut sf = self.state.read_snapshots()?;
        if !sf.items.iter().any(|m| m.id == id) {
            sf.items.push(SnapshotMeta {
                id: id.to_string(),
                created_at: Utc::now(),
                source_version: None,
                size_bytes: size,
                file_count: count,
                keep: true,
                sample_sha256: Some(sample),
            });
            self.state.write_snapshots(&sf)?;
        }
        Ok(())
    }

    /// Classify snapshots for retention diagnostics (eligible vs protected).
    ///
    /// - **eligible**: non-`keep` and not in-use/rescue — subject to keep-N
    /// - **protected**: `keep=true` and/or referenced by current job / rescue
    ///
    /// A pin that is also in-use counts once toward `protected_count`.
    pub fn retention_counts(&self) -> Result<RetentionCounts> {
        let sf = self.state.read_snapshots()?;
        let mut eligible = 0u32;
        let mut protected = 0u32;
        for m in &sf.items {
            let in_use = self.in_use_reason(&m.id)?.is_some();
            if m.keep || in_use {
                protected += 1;
            } else {
                eligible += 1;
            }
        }
        Ok(RetentionCounts {
            total_count: sf.items.len() as u32,
            eligible_count: eligible,
            protected_count: protected,
        })
    }

    /// Apply the keep-N retention policy described in spec §9.3.
    ///
    /// Always retained (never auto-deleted):
    /// - `keep=true` (operator permanent pin)
    /// - currently referenced by an in-flight job or rescue / needs_manual recovery
    ///
    /// Among all other snapshots (regardless of age), keep the most recent `keep_n`
    /// by `created_at` and delete the rest. Age is not a free pass past the limit.
    ///
    /// `keep_n == 0` deletes all eligible non-keep / non-protected snapshots (used only
    /// when callers intentionally pass zero; the prefs path clamps to ≥1). When
    /// `keep_n >= 1`, at least `min(keep_n, eligible)` auto-managed backups remain, so
    /// prune never wipes the last eligible backup solely because the set is small.
    ///
    /// Disk delete failures are **not** silent: the id stays in `snapshots.json` so the
    /// list remains honest, and an error is logged. After meta prune, orphan dirs under
    /// `state/snapshots/` (safe name patterns only) are swept.
    pub fn prune(&self, keep_n: usize) -> Result<Vec<String>> {
        let mut sf = self.state.read_snapshots()?;

        // Collect ids that must never be auto-deleted (in-use / rescue).
        let mut protected: std::collections::HashSet<String> = std::collections::HashSet::new();
        for m in &sf.items {
            if self.in_use_reason(&m.id)?.is_some() {
                protected.insert(m.id.clone());
            }
        }

        // Pins and in-use/rescue are always kept; among the rest, keep newest N.
        let mut keepers: Vec<&SnapshotMeta> = sf
            .items
            .iter()
            .filter(|m| m.keep || protected.contains(&m.id))
            .collect();
        let mut others: Vec<&SnapshotMeta> = sf
            .items
            .iter()
            .filter(|m| !m.keep && !protected.contains(&m.id))
            .collect();
        others.sort_by_key(|m| std::cmp::Reverse(m.created_at));
        keepers.extend(others.iter().take(keep_n));
        let keep_ids: std::collections::HashSet<String> =
            keepers.iter().map(|m| m.id.clone()).collect();

        let mut removed = Vec::new();
        let mut disk_failures = Vec::new();
        // Collect candidates first — we cannot call fallible I/O inside retain.
        let drop_ids: Vec<String> = sf
            .items
            .iter()
            .filter(|m| !keep_ids.contains(&m.id))
            .map(|m| m.id.clone())
            .collect();

        for id in drop_ids {
            let p = self.state.snapshots_dir().join(&id);
            match crate::probe::filesystem::path_is_present(&p) {
                Ok(false) => {
                    removed.push(id);
                    continue;
                }
                Err(e) => {
                    warn!(
                        snapshot = %id,
                        path = %p.display(),
                        err = %e,
                        "snapshot prune: cannot inspect dir; keeping id in snapshots.json"
                    );
                    disk_failures.push(id);
                    continue;
                }
                Ok(true) => {}
            }
            if let Err(e) = std::fs::remove_dir_all(&p)
            {
                warn!(
                    snapshot = %id,
                    path = %p.display(),
                    err = %e,
                    "snapshot prune: disk delete failed; keeping id in snapshots.json"
                );
                disk_failures.push(id);
                continue;
            }
            removed.push(id);
        }

        if !removed.is_empty() {
            let drop_set: std::collections::HashSet<&str> =
                removed.iter().map(String::as_str).collect();
            sf.items.retain(|m| !drop_set.contains(m.id.as_str()));
            self.state.write_snapshots(&sf)?;
            info!(
                keep_n,
                removed = removed.len(),
                disk_failures = disk_failures.len(),
                "snapshot prune removed older backups"
            );
        } else if !disk_failures.is_empty() {
            warn!(
                keep_n,
                failed = disk_failures.len(),
                "snapshot prune: all candidate deletes failed on disk; metadata unchanged"
            );
        }

        // Always sweep untracked leftover dirs (tmp / restore stages / deleted-but-left).
        let orphaned = self.sweep_orphan_snapshot_dirs()?;
        if !orphaned.is_empty() {
            info!(
                count = orphaned.len(),
                names = %orphaned.join(","),
                "snapshot prune: removed orphan dirs under snapshots/"
            );
        }

        Ok(removed)
    }

    /// Remove directories under `state/snapshots/` that are not listed in
    /// `snapshots.json`, when the name matches a safe pattern only:
    /// - opaque snapshot id (`[A-Za-z0-9_-]+`)
    /// - create leftover `{id}.tmp`
    /// - restore staging `restore-stage-*`
    ///
    /// Safety copies (`aside-*`, `broken-inplace-*`) are never swept.
    ///
    /// Never touches names with path separators or other characters.
    pub fn sweep_orphan_snapshot_dirs(&self) -> Result<Vec<String>> {
        let sf = self.state.read_snapshots()?;
        let known: std::collections::HashSet<&str> =
            sf.items.iter().map(|m| m.id.as_str()).collect();
        let dir = self.state.snapshots_dir();
        if !dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut removed = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let ft = entry.file_type()?;
            if !ft.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !is_safe_snapshot_dir_name(name) {
                continue;
            }
            // Keep meta-tracked snapshot dirs and unregistered restore safety copies.
            if known.contains(name)
                || name.starts_with("aside-")
                || name.starts_with("broken-inplace-")
            {
                continue;
            }
            let path = entry.path();
            match std::fs::remove_dir_all(&path) {
                Ok(()) => {
                    info!(name, "removed orphan snapshot dir");
                    removed.push(name.to_string());
                }
                Err(e) => {
                    warn!(
                        name,
                        path = %path.display(),
                        err = %e,
                        "failed to remove orphan snapshot dir"
                    );
                }
            }
        }
        Ok(removed)
    }

    /// Delete a single snapshot by id.
    ///
    /// - Not in `snapshots.json` → `NotFound` (orphan dir under `state/snapshots/<id>` is
    ///   still cleaned if present).
    /// - In use by the current job, or required for rescue / needs_manual recovery →
    ///   `Precondition`.
    /// - `keep=true` → `Precondition` (no force path; operators must clear keep first).
    /// - Last remaining snapshot in metadata → `Precondition`.
    /// - Otherwise removes `state/snapshots/<id>` and updates `snapshots.json` atomically,
    ///   then appends history + audit lines (timestamped; `actor` when provided).
    pub fn delete(&self, id: &str, actor: Option<&str>) -> Result<()> {
        validate_snapshot_id(id)?;

        let path = self.state.snapshots_dir().join(id);
        let mut sf = self.state.read_snapshots()?;
        let meta = sf.items.iter().find(|m| m.id == id).cloned();

        let Some(meta) = meta else {
            // Best-effort orphan cleanup when metadata already dropped the entry.
            if crate::probe::filesystem::path_is_present(&path).unwrap_or(false) {
                let _ = std::fs::remove_dir_all(&path);
                info!(snapshot = %id, "removed orphan snapshot dir (not in snapshots.json)");
            }
            return Err(UpdaterError::NotFound(format!(
                "snapshot {id} not found in snapshots.json"
            )));
        };

        if let Some(reason) = self.in_use_reason(id)? {
            return Err(UpdaterError::Precondition(reason));
        }

        if meta.keep {
            return Err(UpdaterError::Precondition(format!(
                "snapshot {id} is marked keep=true and cannot be deleted"
            )));
        }

        if sf.items.len() <= 1 {
            return Err(UpdaterError::Precondition(format!(
                "refusing to delete the last remaining snapshot ({id})"
            )));
        }

        if crate::probe::filesystem::path_is_present(&path)? {
            std::fs::remove_dir_all(&path).map_err(|e| {
                UpdaterError::Internal(anyhow::anyhow!(
                    "remove snapshot dir {}: {e}",
                    path.display()
                ))
            })?;
        }

        sf.items.retain(|m| m.id != id);
        self.state.write_snapshots(&sf)?;

        // history/audit prepend RFC3339 timestamps; include actor when known.
        let line = match actor.map(str::trim).filter(|s| !s.is_empty()) {
            Some(a) => format!("audit: snapshot_delete id={id} actor={a}"),
            None => format!("audit: snapshot_delete id={id}"),
        };
        self.state.append_history(&line)?;
        let _ = self.state.append_audit(&line);
        info!(snapshot = %id, actor = actor.unwrap_or("-"), "snapshot deleted");
        Ok(())
    }

    /// Returns a human-readable refusal reason when `id` must not be deleted.
    fn in_use_reason(&self, id: &str) -> Result<Option<String>> {
        // Current in-flight job (job.current).
        if let Some(job_id) = self.state.read_current_job()?
            && let Ok(job) = self.state.read_job(&job_id)
            && job.snapshot_id.as_deref() == Some(id)
        {
            return Ok(Some(format!(
                "snapshot {id} is in use by current job {job_id}"
            )));
        }

        // Rescue / needs_manual: protect the snapshot the operator would roll back to.
        let maint = self.state.read_maintenance()?;
        use crate::state::Phase;
        let stuck = matches!(maint.phase, Phase::NeedsManual)
            || (maint.active && (maint.phase.is_post_swap() || maint.phase.is_rollback()));
        if stuck {
            let job_id = maint
                .job_id
                .clone()
                .or_else(|| self.state.read_current_job().ok().flatten());
            if let Some(job_id) = job_id
                && let Ok(job) = self.state.read_job(&job_id)
                && job.snapshot_id.as_deref() == Some(id)
            {
                return Ok(Some(format!(
                    "snapshot {id} is required for rescue (job {job_id}, phase {:?})",
                    maint.phase
                )));
            }
        }

        Ok(None)
    }
}

fn requires_in_place_restore(e: &std::io::Error) -> bool {
    // The official read-only compose root overlays a writable pgdata mount.
    // Renaming that mount can fail with EROFS (parent) as well as EBUSY (mount).
    // The existing staged in-place path still requires pgdata itself to be writable.
    matches!(e.raw_os_error(), Some(16 | 30))
        || e.kind() == ErrorKind::ResourceBusy
        || e.to_string().to_ascii_lowercase().contains("busy")
}

fn dir_has_entries(dir: &Path) -> Result<bool> {
    let mut rd = std::fs::read_dir(dir)?;
    Ok(rd.next().is_some())
}

fn clear_dir_contents(dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Copy `src` directory to a new `dst` path (`cp -a src dst`).
/// Tries `--reflink=auto` first (cheap on btrfs/xfs); falls back to plain `-a`
/// for macOS / filesystems that reject the flag.
async fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    cp_a(&[src.as_os_str()], dst).await
}

/// Copy *contents* of `src` into existing directory `dst` (`cp -a src/. dst/`).
async fn copy_tree_into(src: &Path, dst: &Path) -> Result<()> {
    let src_dot = src.join(".");
    cp_a(&[src_dot.as_os_str()], dst).await
}

async fn cp_a(srcs: &[&std::ffi::OsStr], dst: &Path) -> Result<()> {
    // Prefer reflink when available.
    let mut args: Vec<std::ffi::OsString> = vec!["-a".into(), "--reflink=auto".into()];
    for s in srcs {
        args.push((*s).to_os_string());
    }
    args.push(dst.as_os_str().to_os_string());
    let status = Command::new("cp")
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .await
        .map_err(|e| UpdaterError::Internal(anyhow::anyhow!("spawn cp: {e}")))?;
    if status.success() {
        return Ok(());
    }

    // Fallback without reflink (macOS BSD cp, older coreutils, etc.).
    let mut args: Vec<std::ffi::OsString> = vec!["-a".into()];
    for s in srcs {
        args.push((*s).to_os_string());
    }
    args.push(dst.as_os_str().to_os_string());
    let status = Command::new("cp")
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .await
        .map_err(|e| UpdaterError::Internal(anyhow::anyhow!("spawn cp: {e}")))?;
    if !status.success() {
        return Err(UpdaterError::Internal(anyhow::anyhow!(
            "cp → {} failed: {:?}",
            dst.display(),
            status
        )));
    }
    Ok(())
}

fn fsync_dir(p: &Path) -> Result<()> {
    let f = std::fs::File::open(p)?;
    f.sync_all()?;
    Ok(())
}

/// Walk the snapshot tree to compute size, file count, and a sample digest covering the
/// first 4KB of up to 64 deterministic paths.
fn measure_and_sample(root: &Path) -> Result<(u64, u64, String)> {
    let mut size: u64 = 0;
    let mut count: u64 = 0;
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            count += 1;
            if let Ok(meta) = entry.metadata() {
                size += meta.len();
            }
            paths.push(entry.path().to_path_buf());
        }
    }
    paths.sort();
    let mut hasher = Sha256::new();
    let sample_step = (paths.len().max(1) / 64).max(1);
    for p in paths.iter().step_by(sample_step).take(64) {
        if let Ok(bytes) = std::fs::read(p) {
            let head = &bytes[..bytes.len().min(4096)];
            hasher.update(p.to_string_lossy().as_bytes());
            hasher.update(b"\0");
            hasher.update(head);
        }
    }
    Ok((size, count, hex::encode(hasher.finalize())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StateDir;
    use tempfile::tempdir;

    fn write_file(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    #[tokio::test]
    async fn restore_via_rename_when_possible() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        let pgdata = dir.path().join("pgdata");
        write_file(&pgdata.join("PG_VERSION"), "18\n");
        write_file(&pgdata.join("base/1"), "live\n");

        let mgr = SnapshotManager {
            state: &state,
            pgdata: pgdata.clone(),
        };
        mgr.create("snap-a", None).await.unwrap();

        // Mutate live data after snapshot.
        write_file(&pgdata.join("base/1"), "mutated\n");

        mgr.restore("snap-a").await.unwrap();
        assert_eq!(
            std::fs::read_to_string(pgdata.join("base/1")).unwrap(),
            "live\n"
        );
        let asides: Vec<_> = state
            .read_snapshots()
            .unwrap()
            .items
            .into_iter()
            .filter(|m| m.id.starts_with("aside-") && m.keep)
            .collect();
        assert_eq!(asides.len(), 1, "pre-restore pgdata must be a pinned snapshot");
        assert!(
            state.snapshots_dir().join(&asides[0].id).join("base/1").exists(),
            "aside snapshot must contain the mutated live tree"
        );
    }

    #[tokio::test]
    async fn restore_in_place_when_rename_busy() {
        // Call restore_in_place directly after planting a snapshot.
        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        let pgdata = dir.path().join("pgdata");
        write_file(&pgdata.join("PG_VERSION"), "18\n");
        write_file(&pgdata.join("base/1"), "live\n");

        let mgr = SnapshotManager {
            state: &state,
            pgdata: pgdata.clone(),
        };
        mgr.create("snap-b", None).await.unwrap();
        write_file(&pgdata.join("base/1"), "mutated\n");
        write_file(&pgdata.join("extra"), "should-go\n");

        let snap = state.snapshots_dir().join("snap-b");
        mgr.restore_in_place(&snap, "snap-b", "testts")
            .await
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(pgdata.join("base/1")).unwrap(),
            "live\n"
        );
        assert!(!pgdata.join("extra").exists());
        let safety = state.snapshots_dir().join("aside-testts");
        assert!(safety.join("extra").exists());
        let meta = state
            .read_snapshots()
            .unwrap()
            .items
            .into_iter()
            .find(|m| m.id == "aside-testts")
            .expect("safety copy registered");
        assert!(meta.keep, "pre-restore copy must be pinned until health is confirmed");
    }

    #[test]
    fn validate_snapshot_id_rejects_path_influencing_ids() {
        for bad in [
            "",
            "..",
            "../../etc",
            "a/b",
            "a\\b",
            "/absolute",
            "with space",
            "nul\0byte",
            "dot.dot",
        ] {
            assert!(validate_snapshot_id(bad).is_err(), "should reject {bad:?}");
        }
        for good in ["snap-1", "job_2026", "AbC123", "a"] {
            assert!(validate_snapshot_id(good).is_ok(), "should accept {good:?}");
        }
    }

    /// Regression: `restore` used to accept any id whose joined path existed,
    /// so a rollback request could aim the restore outside `state/snapshots/`
    /// and copy an arbitrary host directory over pgdata.
    #[tokio::test]
    async fn restore_rejects_traversal_and_unregistered_ids() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        let pgdata = dir.path().join("pgdata");
        write_file(&pgdata.join("PG_VERSION"), "18\n");
        write_file(&pgdata.join("base/1"), "live\n");

        let mgr = SnapshotManager {
            state: &state,
            pgdata: pgdata.clone(),
        };

        // Traversal id — refused on the opaque-id rule, before any filesystem work.
        let err = mgr.restore("../../etc").await.unwrap_err();
        assert!(
            matches!(err, UpdaterError::InvalidInput(_)),
            "expected InvalidInput, got {err:?}"
        );

        // Well-formed id, directory planted on disk, but never registered in
        // snapshots.json — still refused.
        std::fs::create_dir_all(state.snapshots_dir().join("not-ours")).unwrap();
        let err = mgr.restore("not-ours").await.unwrap_err();
        assert!(
            matches!(err, UpdaterError::NotFound(_)),
            "expected NotFound, got {err:?}"
        );

        // pgdata untouched by either attempt.
        assert!(pgdata.join("base/1").exists());
    }

    #[test]
    fn mount_or_readonly_parent_requires_in_place_restore() {
        let e = std::io::Error::from_raw_os_error(16);
        assert!(requires_in_place_restore(&e));
        assert!(requires_in_place_restore(
            &std::io::Error::from_raw_os_error(30)
        ));
        assert!(!requires_in_place_restore(
            &std::io::Error::from_raw_os_error(13)
        ));
        let e2 = std::io::Error::other("Device or resource busy");
        assert!(requires_in_place_restore(&e2));
        let e3 = std::io::Error::new(ErrorKind::NotFound, "no such file");
        assert!(!requires_in_place_restore(&e3));
    }

    fn plant_snapshot_meta(state: &StateDir, id: &str) {
        plant_snapshot_meta_keep(state, id, false);
    }

    fn plant_snapshot_meta_keep(state: &StateDir, id: &str, keep: bool) {
        std::fs::create_dir_all(state.snapshots_dir().join(id)).unwrap();
        std::fs::write(state.snapshots_dir().join(id).join("marker"), b"x").unwrap();
        let mut sf = state.read_snapshots().unwrap();
        sf.items.push(SnapshotMeta {
            id: id.to_string(),
            created_at: Utc::now(),
            source_version: None,
            size_bytes: 1,
            file_count: 1,
            keep,
            sample_sha256: None,
        });
        state.write_snapshots(&sf).unwrap();
    }

    #[test]
    fn delete_removes_meta_and_dir() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        // Need ≥2 items so last-snapshot protection does not fire.
        plant_snapshot_meta(&state, "snap-keep-other");
        plant_snapshot_meta(&state, "snap-del");

        let mgr = SnapshotManager {
            state: &state,
            pgdata: dir.path().join("pgdata"),
        };
        mgr.delete("snap-del", Some("admin:1:test")).unwrap();

        assert!(!state.snapshots_dir().join("snap-del").exists());
        let remaining = state.read_snapshots().unwrap().items;
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "snap-keep-other");
    }

    #[test]
    fn delete_missing_returns_not_found() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        let mgr = SnapshotManager {
            state: &state,
            pgdata: dir.path().join("pgdata"),
        };
        let err = mgr.delete("no-such", None).unwrap_err();
        assert!(matches!(err, UpdaterError::NotFound(_)));
    }

    #[test]
    fn delete_missing_cleans_orphan_dir() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        let orphan = state.snapshots_dir().join("orphan-only");
        std::fs::create_dir_all(&orphan).unwrap();
        std::fs::write(orphan.join("x"), b"y").unwrap();

        let mgr = SnapshotManager {
            state: &state,
            pgdata: dir.path().join("pgdata"),
        };
        let err = mgr.delete("orphan-only", None).unwrap_err();
        assert!(matches!(err, UpdaterError::NotFound(_)));
        assert!(!orphan.exists());
    }

    #[test]
    fn delete_refuses_in_use_by_current_job() {
        use crate::state::{Job, JobKind, JobStatus};

        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        plant_snapshot_meta(&state, "snap-other");
        plant_snapshot_meta(&state, "snap-busy");

        let job = Job {
            id: "job-1".into(),
            kind: JobKind::Update,
            created_at: Utc::now(),
            finished_at: None,
            from_version: None,
            to_version: None,
            snapshot_id: Some("snap-busy".into()),
            status: JobStatus::Running,
            steps: vec![],
            idempotency_key: None,
            idempotency_fingerprint: None,
        };
        state.write_job(&job).unwrap();
        state.set_current_job(Some("job-1")).unwrap();

        let mgr = SnapshotManager {
            state: &state,
            pgdata: dir.path().join("pgdata"),
        };
        let err = mgr.delete("snap-busy", None).unwrap_err();
        assert!(matches!(err, UpdaterError::Precondition(_)), "{err:?}");
        assert!(err.to_string().contains("in use"));
        assert!(state.snapshots_dir().join("snap-busy").exists());
        assert_eq!(state.read_snapshots().unwrap().items.len(), 2);
    }

    #[test]
    fn delete_refuses_last_remaining_snapshot() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        plant_snapshot_meta(&state, "snap-only");

        let mgr = SnapshotManager {
            state: &state,
            pgdata: dir.path().join("pgdata"),
        };
        let err = mgr.delete("snap-only", None).unwrap_err();
        assert!(matches!(err, UpdaterError::Precondition(_)), "{err:?}");
        assert!(err.to_string().contains("last remaining"));
        assert_eq!(state.read_snapshots().unwrap().items.len(), 1);
        assert!(state.snapshots_dir().join("snap-only").exists());
    }

    #[test]
    fn delete_refuses_keep_flag() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        plant_snapshot_meta(&state, "snap-other");
        plant_snapshot_meta_keep(&state, "snap-kept", true);

        let mgr = SnapshotManager {
            state: &state,
            pgdata: dir.path().join("pgdata"),
        };
        let err = mgr.delete("snap-kept", None).unwrap_err();
        assert!(matches!(err, UpdaterError::Precondition(_)), "{err:?}");
        assert!(err.to_string().contains("keep=true"));
        assert!(state.snapshots_dir().join("snap-kept").exists());
        assert_eq!(state.read_snapshots().unwrap().items.len(), 2);
    }

    fn plant_snapshot_meta_at(state: &StateDir, id: &str, created_at: DateTime<Utc>, keep: bool) {
        std::fs::create_dir_all(state.snapshots_dir().join(id)).unwrap();
        std::fs::write(state.snapshots_dir().join(id).join("marker"), b"x").unwrap();
        let mut sf = state.read_snapshots().unwrap();
        sf.items.push(SnapshotMeta {
            id: id.to_string(),
            created_at,
            source_version: None,
            size_bytes: 1,
            file_count: 1,
            keep,
            sample_sha256: None,
        });
        state.write_snapshots(&sf).unwrap();
    }

    /// Regression: multi-day-old extras must be deleted under keep_n.
    /// Under the old 24h rule these were already "eligible"; if they still piled up,
    /// the bug was missing prune invocation — this asserts the algorithm itself
    /// removes 48h+ non-keep snapshots past N.
    #[test]
    fn prune_all_at_least_48h_old_keeps_only_latest_n() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        let base = Utc::now() - chrono::Duration::hours(48);
        // Five non-keep snapshots, all ≥48h old (staggered further back).
        plant_snapshot_meta_at(&state, "d5", base - chrono::Duration::hours(40), false);
        plant_snapshot_meta_at(&state, "d4", base - chrono::Duration::hours(30), false);
        plant_snapshot_meta_at(&state, "d3", base - chrono::Duration::hours(20), false);
        plant_snapshot_meta_at(&state, "d2", base - chrono::Duration::hours(10), false);
        plant_snapshot_meta_at(&state, "d1", base, false);

        let mgr = SnapshotManager {
            state: &state,
            pgdata: dir.path().join("pgdata"),
        };
        let removed = mgr.prune(2).unwrap();
        assert_eq!(
            removed.len(),
            3,
            "must remove three oldest among five ≥48h backups"
        );
        let ids: std::collections::HashSet<_> = state
            .read_snapshots()
            .unwrap()
            .items
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains("d1"), "newest of the old set must remain");
        assert!(ids.contains("d2"));
        assert!(!ids.contains("d3"));
        assert!(!ids.contains("d4"));
        assert!(!ids.contains("d5"));
        // Disk dirs removed too.
        assert!(!state.snapshots_dir().join("d5").exists());
        assert!(state.snapshots_dir().join("d1").exists());
    }

    #[test]
    fn prune_keeps_latest_n_among_mixed_ages_and_protects_keep() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        let now = Utc::now();
        let old = now - chrono::Duration::hours(48);
        // 5 non-keep snapshots of mixed ages (including <24h). keep_n=2 → only 2 newest.
        plant_snapshot_meta_at(&state, "old-a", old - chrono::Duration::hours(4), false);
        plant_snapshot_meta_at(&state, "old-b", old - chrono::Duration::hours(3), false);
        plant_snapshot_meta_at(&state, "mid", now - chrono::Duration::hours(12), false);
        plant_snapshot_meta_at(&state, "fresh-a", now - chrono::Duration::hours(2), false);
        plant_snapshot_meta_at(
            &state,
            "fresh-b",
            now - chrono::Duration::minutes(30),
            false,
        );
        // Permanent pin survives even when older than all others.
        plant_snapshot_meta_at(
            &state,
            "kept-forever",
            old - chrono::Duration::hours(10),
            true,
        );

        let mgr = SnapshotManager {
            state: &state,
            pgdata: dir.path().join("pgdata"),
        };
        let removed = mgr.prune(2).unwrap();
        assert_eq!(removed.len(), 3);
        let ids: std::collections::HashSet<_> = state
            .read_snapshots()
            .unwrap()
            .items
            .into_iter()
            .map(|m| m.id)
            .collect();
        // Newest two non-keep + the pin.
        assert!(ids.contains("fresh-a"));
        assert!(ids.contains("fresh-b"));
        assert!(ids.contains("kept-forever"));
        assert!(!ids.contains("old-a"));
        assert!(!ids.contains("old-b"));
        assert!(!ids.contains("mid"));
        // Age no longer exempts: older-than-N non-keep are gone even if <24h.
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn prune_keep_n_one_among_several_fresh() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        let now = Utc::now();
        plant_snapshot_meta_at(&state, "f1", now - chrono::Duration::hours(3), false);
        plant_snapshot_meta_at(&state, "f2", now - chrono::Duration::hours(2), false);
        plant_snapshot_meta_at(&state, "f3", now - chrono::Duration::hours(1), false);
        plant_snapshot_meta_at(&state, "f4", now - chrono::Duration::minutes(10), false);

        let mgr = SnapshotManager {
            state: &state,
            pgdata: dir.path().join("pgdata"),
        };
        let removed = mgr.prune(1).unwrap();
        assert_eq!(removed.len(), 3);
        let ids: std::collections::HashSet<_> = state
            .read_snapshots()
            .unwrap()
            .items
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, std::collections::HashSet::from(["f4".to_string()]));
    }

    #[test]
    fn prune_protects_in_use_even_when_oldest() {
        use crate::state::{Job, JobKind, JobStatus};

        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        let now = Utc::now();
        let old = now - chrono::Duration::hours(72);
        plant_snapshot_meta_at(&state, "busy-old", old, false);
        plant_snapshot_meta_at(&state, "n1", now - chrono::Duration::hours(3), false);
        plant_snapshot_meta_at(&state, "n2", now - chrono::Duration::hours(2), false);
        plant_snapshot_meta_at(&state, "n3", now - chrono::Duration::hours(1), false);

        let job = Job {
            id: "job-prune".into(),
            kind: JobKind::Update,
            created_at: now,
            finished_at: None,
            from_version: None,
            to_version: None,
            snapshot_id: Some("busy-old".into()),
            status: JobStatus::Running,
            steps: vec![],
            idempotency_key: None,
            idempotency_fingerprint: None,
        };
        state.write_job(&job).unwrap();
        state.set_current_job(Some("job-prune")).unwrap();

        let mgr = SnapshotManager {
            state: &state,
            pgdata: dir.path().join("pgdata"),
        };
        let removed = mgr.prune(2).unwrap();
        assert_eq!(removed.len(), 1);
        let ids: std::collections::HashSet<_> = state
            .read_snapshots()
            .unwrap()
            .items
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert!(ids.contains("busy-old"));
        assert!(ids.contains("n2"));
        assert!(ids.contains("n3"));
        assert!(!ids.contains("n1"));
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn prune_keeps_sole_eligible_when_keep_n_at_least_one() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        plant_snapshot_meta_at(&state, "only", Utc::now(), false);

        let mgr = SnapshotManager {
            state: &state,
            pgdata: dir.path().join("pgdata"),
        };
        let removed = mgr.prune(1).unwrap();
        assert!(removed.is_empty());
        assert_eq!(state.read_snapshots().unwrap().items.len(), 1);
        assert_eq!(state.read_snapshots().unwrap().items[0].id, "only");
    }

    /// Disk delete failure must not drop the id from snapshots.json (list stays honest).
    #[test]
    fn prune_disk_delete_failure_keeps_meta() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        let now = Utc::now();
        // Two normal dirs + one "broken" path that is a file, so remove_dir_all fails.
        plant_snapshot_meta_at(&state, "ok-new", now, false);
        plant_snapshot_meta_at(&state, "ok-mid", now - chrono::Duration::hours(1), false);
        // Meta for bad-old, but plant a file instead of a directory.
        let mut sf = state.read_snapshots().unwrap();
        sf.items.push(SnapshotMeta {
            id: "bad-old".into(),
            created_at: now - chrono::Duration::hours(2),
            source_version: None,
            size_bytes: 1,
            file_count: 1,
            keep: false,
            sample_sha256: None,
        });
        state.write_snapshots(&sf).unwrap();
        std::fs::write(state.snapshots_dir().join("bad-old"), b"not-a-dir").unwrap();

        let mgr = SnapshotManager {
            state: &state,
            pgdata: dir.path().join("pgdata"),
        };
        // keep_n=1 → should try to drop mid + bad-old; mid goes, bad-old stays in JSON.
        let removed = mgr.prune(1).unwrap();
        assert!(
            removed.contains(&"ok-mid".to_string()),
            "removed={removed:?}"
        );
        assert!(
            !removed.contains(&"bad-old".to_string()),
            "failed disk delete must not appear in removed: {removed:?}"
        );
        let ids: std::collections::HashSet<_> = state
            .read_snapshots()
            .unwrap()
            .items
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert!(ids.contains("ok-new"));
        assert!(
            ids.contains("bad-old"),
            "meta must stay when disk delete fails"
        );
        assert!(!ids.contains("ok-mid"));
        assert!(state.snapshots_dir().join("bad-old").is_file());
    }

    #[test]
    fn prune_sweeps_orphan_dirs_with_safe_names() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        plant_snapshot_meta_at(&state, "tracked", Utc::now(), false);

        let snaps = state.snapshots_dir();
        // Safe orphans.
        std::fs::create_dir_all(snaps.join("orphan-id")).unwrap();
        std::fs::create_dir_all(snaps.join("leftover.tmp")).unwrap();
        std::fs::create_dir_all(snaps.join("broken-inplace-20260101T000000Z")).unwrap();
        std::fs::create_dir_all(snaps.join("restore-stage-20260101T000000Z")).unwrap();
        // Unsafe name must not be touched.
        std::fs::create_dir_all(snaps.join("weird.name")).unwrap();

        let mgr = SnapshotManager {
            state: &state,
            pgdata: dir.path().join("pgdata"),
        };
        // keep_n large enough that tracked is not deleted; still sweeps orphans.
        let removed = mgr.prune(5).unwrap();
        assert!(removed.is_empty());
        assert!(snaps.join("tracked").exists());
        assert!(!snaps.join("orphan-id").exists());
        assert!(!snaps.join("leftover.tmp").exists());
        assert!(
            snaps.join("broken-inplace-20260101T000000Z").exists(),
            "unregistered restore safety copies must not be swept"
        );
        assert!(!snaps.join("restore-stage-20260101T000000Z").exists());
        assert!(
            snaps.join("weird.name").exists(),
            "unsafe names must not be auto-deleted"
        );
    }

    #[test]
    fn is_safe_snapshot_dir_name_accepts_known_patterns() {
        assert!(is_safe_snapshot_dir_name("abc123"));
        assert!(is_safe_snapshot_dir_name("job-uuid_01"));
        assert!(is_safe_snapshot_dir_name("snap.tmp"));
        assert!(is_safe_snapshot_dir_name("broken-inplace-20260101T000000Z"));
        assert!(is_safe_snapshot_dir_name("restore-stage-x"));
        assert!(!is_safe_snapshot_dir_name(""));
        assert!(!is_safe_snapshot_dir_name(".."));
        assert!(!is_safe_snapshot_dir_name("a/b"));
        assert!(!is_safe_snapshot_dir_name("weird.name"));
        assert!(!is_safe_snapshot_dir_name(".tmp"));
    }

    #[test]
    fn retention_counts_splits_eligible_and_protected() {
        use crate::state::{Job, JobKind, JobStatus};

        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        let now = Utc::now();
        plant_snapshot_meta_at(&state, "e1", now, false);
        plant_snapshot_meta_at(&state, "e2", now, false);
        plant_snapshot_meta_at(&state, "pin", now, true);
        plant_snapshot_meta_at(&state, "busy", now, false);

        let job = Job {
            id: "job-rc".into(),
            kind: JobKind::Update,
            created_at: now,
            finished_at: None,
            from_version: None,
            to_version: None,
            snapshot_id: Some("busy".into()),
            status: JobStatus::Running,
            steps: vec![],
            idempotency_key: None,
            idempotency_fingerprint: None,
        };
        state.write_job(&job).unwrap();
        state.set_current_job(Some("job-rc")).unwrap();

        let mgr = SnapshotManager {
            state: &state,
            pgdata: dir.path().join("pgdata"),
        };
        let c = mgr.retention_counts().unwrap();
        assert_eq!(c.total_count, 4);
        assert_eq!(c.eligible_count, 2);
        assert_eq!(c.protected_count, 2);
    }
}
