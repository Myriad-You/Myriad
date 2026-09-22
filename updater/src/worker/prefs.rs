//! Channel / mode / interval prefs and snapshot retention.

use std::sync::Arc;

use tracing::{info, warn};

use crate::error::{Result, UpdaterError};
use crate::version::UpdateMode;
use crate::worker::Worker;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Prefs {
    pub channel: String,
    pub mode: UpdateMode,
    /// Effective check interval (env fallback already applied when state is unset).
    pub check_interval_secs: u64,
    /// Raw prefs value: null when unset (using env fallback).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_interval_secs_pref: Option<u64>,
    pub auto_install: bool,
    /// Auto-prune backups to a max count (see [`Prefs::snapshot_limit`]).
    pub snapshot_limit_enabled: bool,
    /// Max non-keep / non-protected snapshots retained when limit is enabled.
    pub snapshot_limit: u32,
    /// Ids removed when prefs change triggered an immediate prune (empty otherwise).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pruned_snapshot_ids: Vec<String>,
    /// Non-keep / non-in-use snapshot count after prune (diagnostics).
    pub eligible_count: u32,
    /// keep=true and/or in-use/rescue-protected snapshot count after prune.
    pub protected_count: u32,
    /// Total snapshots in metadata after prune.
    pub total_count: u32,
}

/// Snapshot list response extras (retention diagnostics + self-heal prune result).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SnapshotListDiagnostics {
    pub snapshot_limit_enabled: bool,
    pub snapshot_limit: u32,
    pub eligible_count: u32,
    pub protected_count: u32,
    pub total_count: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pruned_snapshot_ids: Vec<String>,
}

/// Allowed UI values for the check-interval preference (seconds).
/// `0` = off; others are 1h / 6h / 12h / 24h.
pub const CHECK_INTERVAL_PRESETS: &[u64] = &[0, 3600, 21600, 43200, 86400];

pub fn validate_check_interval_secs(secs: u64) -> Result<u64> {
    if CHECK_INTERVAL_PRESETS.contains(&secs) {
        Ok(secs)
    } else {
        Err(UpdaterError::InvalidInput(format!(
            "check_interval_secs must be one of {CHECK_INTERVAL_PRESETS:?}, got {secs}"
        )))
    }
}

pub fn validate_snapshot_limit(n: u32) -> Result<u32> {
    use crate::state::{SNAPSHOT_LIMIT_MAX, SNAPSHOT_LIMIT_MIN};
    if (SNAPSHOT_LIMIT_MIN..=SNAPSHOT_LIMIT_MAX).contains(&n) {
        Ok(n)
    } else {
        Err(UpdaterError::InvalidInput(format!(
            "snapshot_limit must be {SNAPSHOT_LIMIT_MIN}..={SNAPSHOT_LIMIT_MAX}, got {n}"
        )))
    }
}

fn validate_channel_for_mode(channel: &str, mode: UpdateMode) -> Result<()> {
    if !matches!(channel, "stable" | "preview") {
        return Err(UpdaterError::InvalidInput(format!(
            "channel must be stable|preview, got {channel}"
        )));
    }
    match mode {
        UpdateMode::Release => Ok(()),
        // Commit tracking is only offered on the preview track.
        UpdateMode::Commit if channel == "preview" => Ok(()),
        UpdateMode::Commit => Err(UpdaterError::InvalidInput(format!(
            "commit mode is only allowed when channel=preview, got channel={channel}"
        ))),
    }
}

impl Worker {
    /// Effective channel: state preference, else config default.
    pub fn effective_channel(&self) -> String {
        self.state
            .read_updater()
            .ok()
            .map(|s| s.channel)
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| self.config.channel.to_string())
    }

    pub fn effective_mode(&self) -> Result<UpdateMode> {
        Ok(self.state.read_updater()?.update_mode)
    }

    /// Effective periodic check interval: prefs when set, else `CHECK_INTERVAL_SECS` env.
    pub fn effective_check_interval_secs(&self) -> u64 {
        self.state
            .read_updater()
            .ok()
            .and_then(|s| s.check_interval_secs)
            .unwrap_or(self.config.check_interval_secs)
    }

    pub fn auto_install_enabled(&self) -> bool {
        self.state
            .read_updater()
            .ok()
            .map(|s| s.auto_install)
            .unwrap_or(false)
    }

    /// Effective snapshot retention: `Ok(Some(keep_n))` when limit is enabled,
    /// `Ok(None)` when disabled, `Err` when updater state cannot be read.
    /// Falls back to historical default of 3 when enabled but the stored value
    /// is out of range.
    pub fn effective_snapshot_limit(&self) -> Result<Option<usize>> {
        let st = self.state.read_updater()?;
        if !st.snapshot_limit_enabled {
            return Ok(None);
        }
        let n = validate_snapshot_limit(st.snapshot_limit)
            .unwrap_or(crate::state::SNAPSHOT_LIMIT_DEFAULT);
        Ok(Some(n as usize))
    }

    /// Best-effort prune using current prefs.
    ///
    /// - Limit disabled → still sweep orphan dirs under `snapshots/`; return `Ok([])`.
    /// - State read failure → `Err` + warn (callers must not treat this as
    ///   "disabled"; previously `ok()?` swallowed the error as a silent no-op).
    /// - Limit enabled → run prune and log kept/removed counts.
    pub fn maybe_prune_snapshots(&self) -> Result<Vec<String>> {
        let snap = crate::snapshot::SnapshotManager {
            state: &self.state,
            pgdata: self.cli.pgdata.clone(),
        };
        let keep_n = match self.effective_snapshot_limit() {
            Ok(Some(n)) => n,
            Ok(None) => {
                tracing::debug!("snapshot prune skipped: limit disabled");
                // Orphans still waste disk even when count limit is off.
                let _ = snap.sweep_orphan_snapshot_dirs();
                return Ok(Vec::new());
            }
            Err(e) => {
                warn!(
                    err = %e,
                    "snapshot prune skipped: failed to read updater state (not the same as limit off)"
                );
                return Err(e);
            }
        };
        let removed = snap.prune(keep_n)?;
        if removed.is_empty() {
            info!(keep_n, "snapshot prune ran: nothing to remove");
        } else {
            info!(
                keep_n,
                removed = removed.len(),
                ids = %removed.join(","),
                "snapshot prune ran: removed backups"
            );
        }
        Ok(removed)
    }

    /// Like [`Self::maybe_prune_snapshots`] but never fails the caller: logs and
    /// optionally records history/audit when removals happen. Use on update
    /// success/failure cleanup paths. Also sweeps preflight reports whose job and
    /// snapshot are both gone, so retention covers them too.
    pub fn best_effort_prune_snapshots(&self, reason: &str) {
        match self.maybe_prune_snapshots() {
            Ok(ids) if !ids.is_empty() => {
                let _ = self.state.append_history(&format!(
                    "snapshot prune ({reason}): removed {} ({})",
                    ids.len(),
                    ids.join(",")
                ));
                let _ = self.state.append_audit(&format!(
                    "audit: snapshot_prune reason={reason} count={} ids={}",
                    ids.len(),
                    ids.join(",")
                ));
            }
            Ok(_) => {}
            Err(e) => {
                warn!(err = %e, %reason, "snapshot prune failed (best-effort)");
            }
        }
        self.sweep_stale_prepared_reports(reason);
    }

    /// A preflight report holds one job's Compose before/after; it stays useful only
    /// while that job runs or its pgdata snapshot exists, so it follows snapshot
    /// retention. Without this they accumulate one file per update, forever.
    fn sweep_stale_prepared_reports(&self, reason: &str) {
        match self.state.sweep_prepared_reports() {
            Ok(ids) if !ids.is_empty() => {
                let _ = self.state.append_history(&format!(
                    "prepared report sweep ({reason}): removed {} ({})",
                    ids.len(),
                    ids.join(",")
                ));
                let _ = self.state.append_audit(&format!(
                    "audit: prepared_report_sweep reason={reason} count={} ids={}",
                    ids.len(),
                    ids.join(",")
                ));
            }
            Ok(_) => {}
            Err(e) => {
                warn!(err = %e, %reason, "prepared report sweep failed (best-effort)");
            }
        }
    }

    /// Self-heal retention when listing backups: prune if over limit, always
    /// report diagnostics so the UI can show truth (including old-updater gap
    /// when these fields are missing from status).
    pub fn heal_and_list_snapshot_diagnostics(
        &self,
    ) -> Result<(crate::state::SnapshotsFile, SnapshotListDiagnostics)> {
        let pruned = match self.maybe_prune_snapshots() {
            Ok(ids) => {
                if !ids.is_empty() {
                    let _ = self.state.append_history(&format!(
                        "snapshot prune (list_snapshots): removed {} ({})",
                        ids.len(),
                        ids.join(",")
                    ));
                    let _ = self.state.append_audit(&format!(
                        "audit: snapshot_prune reason=list_snapshots count={} ids={}",
                        ids.len(),
                        ids.join(",")
                    ));
                }
                ids
            }
            Err(e) => {
                warn!(err = %e, "snapshot prune on list failed (returning current list)");
                Vec::new()
            }
        };

        let st = self.state.read_updater()?;
        let snap = crate::snapshot::SnapshotManager {
            state: &self.state,
            pgdata: self.cli.pgdata.clone(),
        };
        let counts = snap.retention_counts().unwrap_or_default();
        let file = self.state.read_snapshots()?;
        Ok((
            file,
            SnapshotListDiagnostics {
                snapshot_limit_enabled: st.snapshot_limit_enabled,
                snapshot_limit: st.snapshot_limit,
                eligible_count: counts.eligible_count,
                protected_count: counts.protected_count,
                total_count: counts.total_count,
                pruned_snapshot_ids: pruned,
            },
        ))
    }

    fn snapshot_retention_diagnostics_after_prune(
        &self,
        pruned: Vec<String>,
    ) -> (Vec<String>, crate::snapshot::RetentionCounts) {
        let snap = crate::snapshot::SnapshotManager {
            state: &self.state,
            pgdata: self.cli.pgdata.clone(),
        };
        let counts = snap.retention_counts().unwrap_or_default();
        (pruned, counts)
    }

    pub(crate) async fn handle_set_prefs(
        self: Arc<Self>,
        channel: Option<String>,
        mode: Option<UpdateMode>,
        check_interval_secs: Option<Option<u64>>,
        auto_install: Option<bool>,
        snapshot_limit_enabled: Option<bool>,
        snapshot_limit: Option<u32>,
    ) -> Result<Prefs> {
        let mut st = self.state.read_updater()?;
        let mut channel_or_mode_changed = false;
        let mut retention_changed = false;
        if let Some(ch) = channel {
            let ch = ch.trim().to_ascii_lowercase();
            let mode_now = mode.unwrap_or(st.update_mode);
            validate_channel_for_mode(&ch, mode_now)?;
            st.channel = ch.clone();
            channel_or_mode_changed = true;
            // Persist to .env so restarts keep the preference.
            if let Ok(mut env) = crate::env_file::EnvFile::load(&self.cli.env_file) {
                let _ = env.set("CHANNEL", &ch);
                let _ = env.save();
            }
        }
        if let Some(m) = mode {
            // Re-validate channel under new mode.
            validate_channel_for_mode(&st.channel, m)?;
            st.update_mode = m;
            channel_or_mode_changed = true;
        }
        if let Some(interval) = check_interval_secs {
            match interval {
                None => st.check_interval_secs = None,
                Some(secs) => {
                    validate_check_interval_secs(secs)?;
                    st.check_interval_secs = Some(secs);
                }
            }
        }
        if let Some(ai) = auto_install {
            st.auto_install = ai;
        }
        if let Some(enabled) = snapshot_limit_enabled {
            if st.snapshot_limit_enabled != enabled {
                retention_changed = true;
            }
            st.snapshot_limit_enabled = enabled;
        }
        if let Some(limit) = snapshot_limit {
            let limit = validate_snapshot_limit(limit)?;
            if st.snapshot_limit != limit {
                retention_changed = true;
            }
            st.snapshot_limit = limit;
        }
        // Clear stale availability cache when channel/mode change.
        if channel_or_mode_changed {
            st.latest_available = None;
        }
        self.state.write_updater(&st)?;

        // When retention fields are present or the limit is on/changed, prune
        // immediately so multi-day-old extras free without waiting for an update.
        let pruned_snapshot_ids = if st.snapshot_limit_enabled
            && (retention_changed || snapshot_limit_enabled.is_some() || snapshot_limit.is_some())
        {
            match self.maybe_prune_snapshots() {
                Ok(ids) => {
                    if !ids.is_empty() {
                        let _ = self.state.append_history(&format!(
                            "prefs: snapshot prune removed {} ({})",
                            ids.len(),
                            ids.join(",")
                        ));
                        let _ = self.state.append_audit(&format!(
                            "audit: snapshot_prune reason=prefs count={} ids={}",
                            ids.len(),
                            ids.join(",")
                        ));
                    } else {
                        info!(
                            snapshot_limit = st.snapshot_limit,
                            "prefs: snapshot prune ran with nothing to remove"
                        );
                    }
                    ids
                }
                Err(e) => {
                    warn!(err = %e, "snapshot prune after prefs change failed");
                    Vec::new()
                }
            }
        } else {
            if snapshot_limit_enabled.is_some() || snapshot_limit.is_some() {
                info!(
                    enabled = st.snapshot_limit_enabled,
                    "prefs: snapshot prune not run (limit disabled or fields not applied)"
                );
            }
            // Still sweep orphans when operator touches retention prefs.
            if snapshot_limit_enabled.is_some() || snapshot_limit.is_some() {
                let snap = crate::snapshot::SnapshotManager {
                    state: &self.state,
                    pgdata: self.cli.pgdata.clone(),
                };
                let _ = snap.sweep_orphan_snapshot_dirs();
            }
            Vec::new()
        };

        let (pruned_snapshot_ids, counts) =
            self.snapshot_retention_diagnostics_after_prune(pruned_snapshot_ids);

        let prefs = Prefs {
            channel: st.channel.clone(),
            mode: st.update_mode,
            check_interval_secs: st
                .check_interval_secs
                .unwrap_or(self.config.check_interval_secs),
            check_interval_secs_pref: st.check_interval_secs,
            auto_install: st.auto_install,
            snapshot_limit_enabled: st.snapshot_limit_enabled,
            snapshot_limit: st.snapshot_limit,
            pruned_snapshot_ids,
            eligible_count: counts.eligible_count,
            protected_count: counts.protected_count,
            total_count: counts.total_count,
        };
        self.state.append_history(&format!(
            "prefs: channel={} mode={} check_interval_secs={:?} auto_install={} \
             snapshot_limit_enabled={} snapshot_limit={}",
            prefs.channel,
            prefs.mode,
            prefs.check_interval_secs_pref,
            prefs.auto_install,
            prefs.snapshot_limit_enabled,
            prefs.snapshot_limit
        ))?;
        Ok(prefs)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn effective_mode_does_not_guess_release_on_read_error() {
        let src = include_str!("prefs.rs");
        let mode = src
            .split("pub fn effective_mode")
            .nth(1)
            .and_then(|rest| rest.split("pub fn effective_check_interval_secs").next())
            .expect("effective_mode");
        assert!(mode.contains("read_updater()?"));
        assert!(!mode.contains("unwrap_or(UpdateMode::Release)"));
        let set = src
            .split("pub(crate) async fn handle_set_prefs")
            .nth(1)
            .and_then(|rest| rest.split("self.state.write_updater").next())
            .expect("handle_set_prefs");
        assert!(!set.contains("UPDATE_MODE"));
    }
}
