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
//! Production reaches pgdata below the single `/host/compose` deployment-root bind, so directory
//! rename rollback remains available. The in-place replacement path is retained as a filesystem
//! compatibility fallback.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;

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

impl<'a> SnapshotManager<'a> {
    /// Snapshot pgdata. Returns the snapshot id (matches caller-supplied job id for traceability).
    /// Caller is responsible for stopping postgres beforehand.
    pub async fn create(
        &self,
        snapshot_id: &str,
        source_version: Option<DeployTag>,
    ) -> Result<SnapshotMeta> {
        let snapshots_dir = self.state.snapshots_dir();
        std::fs::create_dir_all(&snapshots_dir)?;

        let tmp = snapshots_dir.join(format!("{snapshot_id}.tmp"));
        let final_path = snapshots_dir.join(snapshot_id);

        if final_path.exists() {
            return Err(UpdaterError::Precondition(format!(
                "snapshot {snapshot_id} already exists"
            )));
        }
        if tmp.exists() {
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
    /// 2. If rename fails with EBUSY (typical for bind-mount points like `/host/pgdata`),
    ///    fall back to in-place content replace: safety-copy current contents under
    ///    `state/snapshots/`, wipe children of the mount, then copy snapshot contents in.
    ///
    /// The snapshot itself is always *copied* (never renamed away) so retry remains possible.
    pub async fn restore(&self, snapshot_id: &str) -> Result<()> {
        let snap_path = self.state.snapshots_dir().join(snapshot_id);
        if !snap_path.exists() {
            return Err(UpdaterError::NotFound(format!(
                "snapshot {snapshot_id} does not exist on disk"
            )));
        }

        let ts = Utc::now().format("%Y%m%dT%H%M%SZ");
        let broken_sibling = self.pgdata.with_extension(format!("broken.{ts}"));

        if self.pgdata.exists() {
            match std::fs::rename(&self.pgdata, &broken_sibling) {
                Ok(()) => {
                    info!(
                        from = %self.pgdata.display(),
                        to = %broken_sibling.display(),
                        "pgdata moved aside via rename"
                    );
                    if let Err(e) = copy_tree(&snap_path, &self.pgdata).await {
                        // Best-effort undo of the rename.
                        if broken_sibling.exists() && !self.pgdata.exists() {
                            let _ = std::fs::rename(&broken_sibling, &self.pgdata);
                        }
                        return Err(e);
                    }
                    fsync_dir(&self.pgdata)?;
                    info!(snapshot = %snapshot_id, "pgdata restored from snapshot (rename path)");
                    return Ok(());
                }
                Err(e) if is_busy(&e) => {
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

    /// In-place restore when `pgdata` cannot be renamed (mount point / EBUSY).
    async fn restore_in_place(&self, snap_path: &Path, snapshot_id: &str, ts: &str) -> Result<()> {
        std::fs::create_dir_all(&self.pgdata)?;

        // Safety copy of current (possibly half-upgraded) contents so operators can recover.
        let safety = self
            .state
            .snapshots_dir()
            .join(format!("broken-inplace-{ts}"));
        if safety.exists() {
            std::fs::remove_dir_all(&safety)?;
        }
        if dir_has_entries(&self.pgdata)? {
            info!(
                safety = %safety.display(),
                "copying current pgdata contents aside before in-place restore"
            );
            if let Err(e) = copy_tree(&self.pgdata, &safety).await {
                warn!(err = %e, "safety copy of current pgdata failed; continuing with restore");
                let _ = std::fs::remove_dir_all(&safety);
            }
        }

        // Wipe children of the mount point (cannot remove the mount itself).
        clear_dir_contents(&self.pgdata)?;

        // `cp -a snap/. dest/` copies *contents* into the existing mount directory.
        copy_tree_into(snap_path, &self.pgdata).await?;
        fsync_dir(&self.pgdata)?;
        info!(
            snapshot = %snapshot_id,
            safety = %safety.display(),
            "pgdata restored from snapshot (in-place / mount-point path)"
        );
        Ok(())
    }

    /// Apply the keep-N retention policy described in spec §9.3.
    pub fn prune(&self, keep_n: usize) -> Result<Vec<String>> {
        let mut sf = self.state.read_snapshots()?;
        let cutoff = Utc::now() - chrono::Duration::hours(24);
        let mut keepers: Vec<&SnapshotMeta> = sf
            .items
            .iter()
            .filter(|m| m.keep || m.created_at >= cutoff)
            .collect();
        // Among the rest, keep the most recent N.
        let mut others: Vec<&SnapshotMeta> = sf
            .items
            .iter()
            .filter(|m| !m.keep && m.created_at < cutoff)
            .collect();
        others.sort_by_key(|m| std::cmp::Reverse(m.created_at));
        keepers.extend(others.iter().take(keep_n));
        let keep_ids: std::collections::HashSet<String> =
            keepers.iter().map(|m| m.id.clone()).collect();

        let mut removed = Vec::new();
        sf.items.retain(|m| {
            if keep_ids.contains(&m.id) {
                true
            } else {
                let p = self.state.snapshots_dir().join(&m.id);
                let _ = std::fs::remove_dir_all(&p);
                removed.push(m.id.clone());
                false
            }
        });
        self.state.write_snapshots(&sf)?;
        Ok(removed)
    }

    /// Delete a single snapshot by id.
    ///
    /// - Not in `snapshots.json` → `NotFound` (orphan dir under `state/snapshots/<id>` is
    ///   still cleaned if present).
    /// - In use by the current job, or required for rescue / needs_manual recovery →
    ///   `Precondition`.
    /// - Otherwise removes `state/snapshots/<id>` and updates `snapshots.json` atomically,
    ///   then appends history + audit lines.
    pub fn delete(&self, id: &str) -> Result<()> {
        if id.is_empty()
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        {
            return Err(UpdaterError::InvalidInput(format!(
                "invalid snapshot id: {id:?}"
            )));
        }

        let path = self.state.snapshots_dir().join(id);
        let mut sf = self.state.read_snapshots()?;
        let in_meta = sf.items.iter().any(|m| m.id == id);

        if !in_meta {
            // Best-effort orphan cleanup when metadata already dropped the entry.
            if path.exists() {
                let _ = std::fs::remove_dir_all(&path);
                info!(snapshot = %id, "removed orphan snapshot dir (not in snapshots.json)");
            }
            return Err(UpdaterError::NotFound(format!(
                "snapshot {id} not found in snapshots.json"
            )));
        }

        if let Some(reason) = self.in_use_reason(id)? {
            return Err(UpdaterError::Precondition(reason));
        }

        if path.exists() {
            std::fs::remove_dir_all(&path).map_err(|e| {
                UpdaterError::Internal(anyhow::anyhow!(
                    "remove snapshot dir {}: {e}",
                    path.display()
                ))
            })?;
        }

        sf.items.retain(|m| m.id != id);
        self.state.write_snapshots(&sf)?;

        let line = format!("audit: snapshot_delete id={id}");
        self.state.append_history(&line)?;
        let _ = self.state.append_audit(&line);
        info!(snapshot = %id, "snapshot deleted");
        Ok(())
    }

    /// Returns a human-readable refusal reason when `id` must not be deleted.
    fn in_use_reason(&self, id: &str) -> Result<Option<String>> {
        // Current in-flight job (job.current).
        if let Some(job_id) = self.state.read_current_job()? {
            if let Ok(job) = self.state.read_job(&job_id) {
                if job.snapshot_id.as_deref() == Some(id) {
                    return Ok(Some(format!(
                        "snapshot {id} is in use by current job {job_id}"
                    )));
                }
            }
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
            if let Some(job_id) = job_id {
                if let Ok(job) = self.state.read_job(&job_id) {
                    if job.snapshot_id.as_deref() == Some(id) {
                        return Ok(Some(format!(
                            "snapshot {id} is required for rescue (job {job_id}, phase {:?})",
                            maint.phase
                        )));
                    }
                }
            }
        }

        Ok(None)
    }
}

fn is_busy(e: &std::io::Error) -> bool {
    // Linux: EBUSY = 16. Also accept ErrorKind::ResourceBusy / Other with "busy" text
    // for portability across libc wrappers.
    e.raw_os_error() == Some(16)
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
        // Safety copy retained.
        let safety = state.snapshots_dir().join("broken-inplace-testts");
        assert!(safety.join("extra").exists());
    }

    #[test]
    fn is_busy_detects_ebusy() {
        let e = std::io::Error::from_raw_os_error(16);
        assert!(is_busy(&e));
        let e2 = std::io::Error::other("Device or resource busy");
        assert!(is_busy(&e2));
        let e3 = std::io::Error::new(ErrorKind::NotFound, "no such file");
        assert!(!is_busy(&e3));
    }

    fn plant_snapshot_meta(state: &StateDir, id: &str) {
        std::fs::create_dir_all(state.snapshots_dir().join(id)).unwrap();
        std::fs::write(state.snapshots_dir().join(id).join("marker"), b"x").unwrap();
        let mut sf = state.read_snapshots().unwrap();
        sf.items.push(SnapshotMeta {
            id: id.to_string(),
            created_at: Utc::now(),
            source_version: None,
            size_bytes: 1,
            file_count: 1,
            keep: false,
            sample_sha256: None,
        });
        state.write_snapshots(&sf).unwrap();
    }

    #[test]
    fn delete_removes_meta_and_dir() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        plant_snapshot_meta(&state, "snap-del");

        let mgr = SnapshotManager {
            state: &state,
            pgdata: dir.path().join("pgdata"),
        };
        mgr.delete("snap-del").unwrap();

        assert!(!state.snapshots_dir().join("snap-del").exists());
        assert!(state.read_snapshots().unwrap().items.is_empty());
    }

    #[test]
    fn delete_missing_returns_not_found() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
        let mgr = SnapshotManager {
            state: &state,
            pgdata: dir.path().join("pgdata"),
        };
        let err = mgr.delete("no-such").unwrap_err();
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
        let err = mgr.delete("orphan-only").unwrap_err();
        assert!(matches!(err, UpdaterError::NotFound(_)));
        assert!(!orphan.exists());
    }

    #[test]
    fn delete_refuses_in_use_by_current_job() {
        use crate::state::{Job, JobKind, JobStatus};

        let dir = tempdir().unwrap();
        let state = StateDir::open(&dir.path().join("state")).unwrap();
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
        };
        state.write_job(&job).unwrap();
        state.set_current_job(Some("job-1")).unwrap();

        let mgr = SnapshotManager {
            state: &state,
            pgdata: dir.path().join("pgdata"),
        };
        let err = mgr.delete("snap-busy").unwrap_err();
        assert!(matches!(err, UpdaterError::Precondition(_)), "{err:?}");
        assert!(state.snapshots_dir().join("snap-busy").exists());
        assert_eq!(state.read_snapshots().unwrap().items.len(), 1);
    }
}
