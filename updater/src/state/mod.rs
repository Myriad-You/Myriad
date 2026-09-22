//! Persistent state below the bind-mounted deployment root (production: /host/compose/state).
//!
//! Layout (see docs/updater-spec.md §6):
//!   state/
//!     updater.json
//!     maintenance.json
//!     job.current
//!     job.<id>.json
//!     prepared.<id>.json    (preflight report + Compose before/after for one job)
//!     lock                  (flock-style process lock)
//!     manual-override       (touch to enable rescue endpoints)
//!     snapshots/            (pgdata snapshots)
//!     snapshots.json
//!     history.log           (append-only)
//!     audit.log             (append-only security/ops audit trail)
//!     env-probe.json
//!     cache/                (release.json ETag/cache)
//!
//! All structured writes go through [`atomic::write_atomic`] which writes to a temp file in the
//! same directory, fsyncs, renames, then fsyncs the directory.

pub mod atomic;
pub mod audit;
pub mod history;
pub mod lock;
pub mod types;

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::error::{Result, UpdaterError};

pub(crate) fn read_existing(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Optional component outcome, not the execution lock. Invalid history must not
/// take down /status; running tasks and Guard retain execution ownership.
pub(crate) fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    let Some(bytes) = read_existing(path)? else {
        return Ok(None);
    };
    match serde_json::from_slice(&bytes) {
        Ok(value) => Ok(Some(value)),
        Err(error) => {
            tracing::warn!(%error, "ignoring unreadable component outcome");
            Ok(None)
        }
    }
}

pub use types::*;

/// Owned handle to the state directory. The updater daemon holds an exclusive process lock
/// for its entire lifetime via `open()`. The rescue CLI can also need to read state while
/// the daemon is still running (e.g. `myriad-rescue status` for diagnostics), in which case
/// it uses `open_readonly()` to skip the lock.
pub struct StateDir {
    root: PathBuf,
    _lock: Option<lock::ProcessLock>,
}

impl StateDir {
    /// Open the state directory with an exclusive process lock. Used by the main updater
    /// daemon — fails fast if another instance already holds the lock.
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        for sub in ["snapshots", "cache"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let lock = lock::ProcessLock::acquire(&root.join("lock"))?;
        Ok(Self {
            root: root.to_path_buf(),
            _lock: Some(lock),
        })
    }

    /// Open the state directory **without** taking the process lock. Suitable for rescue
    /// CLI read-only operations (`status`, `diagnose`, `clean-snapshots`). Writes are still
    /// physically possible but the caller is responsible for ensuring no conflicting daemon
    /// is mutating state concurrently (typically: stop the updater container first).
    pub fn open_readonly(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        for sub in ["snapshots", "cache"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        Ok(Self {
            root: root.to_path_buf(),
            _lock: None,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn snapshots_dir(&self) -> PathBuf {
        self.root.join("snapshots")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn manual_override_enabled(&self) -> Result<bool> {
        crate::probe::filesystem::path_is_present(&self.root.join("manual-override"))
    }

    pub fn read_updater(&self) -> Result<UpdaterStateFile> {
        let path = self.root.join("updater.json");
        let Some(bytes) = read_existing(&path)? else {
            return Ok(UpdaterStateFile::default());
        };
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn write_updater(&self, st: &UpdaterStateFile) -> Result<()> {
        atomic::write_atomic_json(&self.root.join("updater.json"), st)
    }

    pub fn read_maintenance(&self) -> Result<MaintenanceFile> {
        let path = self.root.join("maintenance.json");
        let Some(bytes) = read_existing(&path)? else {
            return Ok(MaintenanceFile::inactive());
        };
        match serde_json::from_slice(&bytes) {
            Ok(m) => Ok(m),
            Err(e) => {
                // Corrupt file: if a job is in flight, fail *closed* into needs_manual so
                // crash recovery / UI do not silently treat a mid-update as idle.
                tracing::warn!(error = %e, "maintenance.json corrupt");
                match self.read_current_job() {
                    Ok(Some(id)) if !id.is_empty() => {
                        let mut m = MaintenanceFile::inactive();
                        m.active = true;
                        m.phase = crate::state::Phase::NeedsManual;
                        m.job_id = Some(id);
                        m.message_key = "updater.phase.needs_manual".into();
                        m.bump_heartbeat();
                        Ok(m)
                    }
                    Ok(_) => Err(e.into()),
                    Err(job_err) => Err(job_err),
                }
            }
        }
    }

    pub fn write_maintenance(&self, m: &MaintenanceFile) -> Result<()> {
        atomic::write_atomic_json(&self.root.join("maintenance.json"), m)
    }

    pub fn clear_maintenance(&self) -> Result<()> {
        self.write_maintenance(&MaintenanceFile::inactive())
    }

    pub fn read_current_job(&self) -> Result<Option<String>> {
        let path = self.root.join("job.current");
        let Some(bytes) = read_existing(&path)? else {
            return Ok(None);
        };
        let s = String::from_utf8_lossy(&bytes);
        Ok(Some(s.trim().to_string()).filter(|s| !s.is_empty()))
    }

    pub fn set_current_job(&self, id: Option<&str>) -> Result<()> {
        let path = self.root.join("job.current");
        match id {
            Some(id) => atomic::write_atomic_bytes(&path, id.as_bytes()),
            None => match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            },
        }
    }

    pub fn read_job(&self, id: &str) -> Result<Job> {
        let path = self.job_path(id);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(UpdaterError::NotFound(format!("job {id}")));
            }
            Err(e) => return Err(e.into()),
        };
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn write_job(&self, job: &Job) -> Result<()> {
        atomic::write_atomic_json(&self.job_path(&job.id), job)
    }

    pub fn list_jobs(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(id) = name
                .strip_prefix("job.")
                .and_then(|s| s.strip_suffix(".json"))
                && id != "current"
            {
                out.push(id.to_string());
            }
        }
        Ok(out)
    }

    fn job_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("job.{id}.json"))
    }

    /// Preflight report (selected images, Compose before/after) persisted before the
    /// destructive zone so a crash can resume or roll back this exact job.
    pub fn prepared_report_path(&self, job_id: &str) -> PathBuf {
        self.root.join(format!("prepared.{job_id}.json"))
    }

    /// Drop persisted preflight reports that can no longer be used.
    ///
    /// A report belongs to one job and carries that job's Compose before/after. It is
    /// live only while the job is in flight or while `snap-<job>` still exists, because
    /// the rollback path restores the pgdata snapshot and the Compose snapshot together.
    /// Without this sweep they accumulate one file per update, forever.
    pub fn sweep_prepared_reports(&self) -> Result<Vec<String>> {
        let current = self.read_current_job()?;
        let snapshots: std::collections::HashSet<String> = self
            .read_snapshots()?
            .items
            .into_iter()
            .map(|m| m.id)
            .collect();
        let mut removed = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(job_id) = name
                .strip_prefix("prepared.")
                .and_then(|rest| rest.strip_suffix(".json"))
            else {
                continue;
            };
            if current.as_deref() == Some(job_id) || snapshots.contains(&format!("snap-{job_id}")) {
                continue;
            }
            match std::fs::remove_file(entry.path()) {
                Ok(()) => {
                    tracing::info!(job = %job_id, "removed stale preflight report");
                    removed.push(job_id.to_string());
                }
                Err(error) => {
                    tracing::warn!(job = %job_id, %error, "failed to remove stale preflight report");
                }
            }
        }
        Ok(removed)
    }

    pub fn read_snapshots(&self) -> Result<SnapshotsFile> {
        let path = self.root.join("snapshots.json");
        let Some(bytes) = read_existing(&path)? else {
            return Ok(SnapshotsFile::default());
        };
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn write_snapshots(&self, s: &SnapshotsFile) -> Result<()> {
        atomic::write_atomic_json(&self.root.join("snapshots.json"), s)
    }

    pub fn write_env_probe(&self, p: &crate::probe::EnvProbe) -> Result<()> {
        atomic::write_atomic_json(&self.root.join("env-probe.json"), p)
    }

    pub fn append_history(&self, line: &str) -> Result<()> {
        history::append(&self.root.join("history.log"), line)
    }

    /// Append a security/ops audit line to `state/audit.log` (fsync'd).
    /// Prefer the `audit: …` line style used in history for machine grepping.
    pub fn append_audit(&self, line: &str) -> Result<()> {
        audit::append(&self.root.join("audit.log"), line)
    }

    /// Record one operational line in both history and audit. Unlike a bare
    /// `let _ =`, a failure is logged: this trail is the only record of what an
    /// update did to a host, so it must not vanish silently.
    pub fn record_operation(&self, line: &str) {
        if let Err(error) = self.append_history(line) {
            tracing::warn!(%error, "history append failed");
        }
        if let Err(error) = self.append_audit(line) {
            tracing::warn!(%error, "audit append failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_job_missing_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        let err = state.read_job("no-such").unwrap_err();
        assert!(
            matches!(err, UpdaterError::NotFound(_)),
            "missing job must be NotFound, got {err}"
        );
    }

    #[test]
    fn read_job_io_error_is_not_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        // A directory at the job path is readable-as-file failure, not NotFound.
        std::fs::create_dir(dir.path().join("job.j1.json")).unwrap();
        let err = state.read_job("j1").unwrap_err();
        assert!(
            !matches!(err, UpdaterError::NotFound(_)),
            "I/O on job file must not look like a missing job, got {err}"
        );
    }

    #[test]
    fn read_job_parse_error_is_not_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        std::fs::write(dir.path().join("job.j1.json"), b"{not-json").unwrap();
        let err = state.read_job("j1").unwrap_err();
        assert!(
            matches!(err, UpdaterError::Json(_)),
            "corrupt job must be Json, got {err}"
        );
    }

    #[test]
    fn missing_state_files_are_not_found_not_permission_errors() {
        let src = include_str!("mod.rs");
        let override_fn = src
            .split("pub fn manual_override_enabled")
            .nth(1)
            .and_then(|rest| rest.split("pub fn read_updater").next())
            .expect("manual_override_enabled");
        assert!(!override_fn.contains("path.exists()"));
        assert!(override_fn.contains("path_is_present"));
        for name in [
            "read_updater",
            "read_maintenance",
            "read_current_job",
            "read_snapshots",
        ] {
            let start = src.find(&format!("pub fn {name}")).expect(name);
            let body = &src[start..];
            let end = body[1..]
                .find(
                    "
    pub fn ",
                )
                .map(|i| i + 1)
                .unwrap_or(body.len());
            let fn_src = &body[..end];
            assert!(
                !fn_src.contains("path.exists()"),
                "{name} must not treat exists() false as absence"
            );
        }
        assert!(src.contains("ErrorKind::NotFound"));
    }

    #[test]
    fn corrupt_maintenance_without_job_is_error_not_inactive() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        std::fs::write(dir.path().join("maintenance.json"), b"{not-json").unwrap();
        let err = state.read_maintenance().unwrap_err();
        assert!(
            matches!(err, UpdaterError::Json(_)),
            "corrupt maintenance with no job pointer must not become idle, got {err}"
        );
    }

    fn plant_prepared(state: &StateDir, job_id: &str) {
        std::fs::write(state.prepared_report_path(job_id), b"{}").unwrap();
    }

    fn plant_snapshot(state: &StateDir, id: &str) {
        let mut sf = state.read_snapshots().unwrap();
        sf.items.push(SnapshotMeta {
            id: id.to_string(),
            created_at: chrono::Utc::now(),
            source_version: None,
            size_bytes: 1,
            file_count: 1,
            keep: false,
        });
        state.write_snapshots(&sf).unwrap();
    }

    /// Regression: reports used to be kept forever, one per update.
    #[test]
    fn sweep_prepared_reports_keeps_in_flight_and_snapshot_backed_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        plant_prepared(&state, "live");
        plant_prepared(&state, "rollbackable");
        plant_prepared(&state, "orphan");
        plant_snapshot(&state, "snap-rollbackable");
        state.set_current_job(Some("live")).unwrap();

        let removed = state.sweep_prepared_reports().unwrap();

        assert_eq!(removed, vec!["orphan".to_string()]);
        assert!(state.prepared_report_path("live").is_file());
        assert!(state.prepared_report_path("rollbackable").is_file());
        assert!(!state.prepared_report_path("orphan").exists());
    }

    /// Once the job is no longer current and its snapshot is gone, the report follows.
    #[test]
    fn sweep_prepared_reports_drops_finished_jobs_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        std::fs::write(state.root().join("job.done.json"), b"{}").unwrap();
        plant_prepared(&state, "done");

        assert_eq!(
            state.sweep_prepared_reports().unwrap(),
            vec!["done".to_string()]
        );
        assert!(state.sweep_prepared_reports().unwrap().is_empty());
        assert!(state.root().join("job.done.json").is_file());
    }
}
