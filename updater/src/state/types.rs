//! On-disk state schemas. Stable; bump `schema_version` if breaking changes are needed.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::version::{DeployTag, MyriadVersion, UpdateMode};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdaterStateFile {
    #[serde(default = "default_schema")]
    pub schema_version: u32,

    /// Currently running business deploy tag (release or commit/branch).
    pub current_version: Option<DeployTag>,
    /// Git commit backing the currently running business deploy, when resolved.
    #[serde(default)]
    pub current_commit_sha: Option<String>,
    /// Updater binary's own release version (always v-semver when known).
    pub updater_version: Option<MyriadVersion>,
    pub last_checked_at: Option<DateTime<Utc>>,
    /// Preferred release channel (`stable`/`preview`) or commit branch
    /// (`main`/`preview`) depending on [`Self::update_mode`].
    pub channel: String,

    /// Release vs commit consumption mode. Default: release.
    #[serde(default)]
    pub update_mode: UpdateMode,

    /// Periodic update check interval in seconds.
    /// `None` = fall back to `CHECK_INTERVAL_SECS` env; `Some(0)` = off;
    /// otherwise one of the UI presets (3600 / 21600 / 43200 / 86400).
    #[serde(default)]
    pub check_interval_secs: Option<u64>,

    /// When true, clear upgrades on the **current** channel/mode are installed
    /// automatically. Default OFF. Applies to stable, preview, and commit/dev;
    /// never auto-installs downgrade / diverged / unknown / irreversible targets.
    #[serde(default)]
    pub auto_install: bool,

    /// When true, successful updates auto-prune pgdata snapshots so that, among
    /// non-`keep` / non-in-use backups (any age), only the most recent
    /// [`Self::snapshot_limit`] are retained. Default ON (historical `prune(3)`).
    #[serde(default = "default_true")]
    pub snapshot_limit_enabled: bool,

    /// Max number of auto-retained non-keep / non-protected snapshots when
    /// [`Self::snapshot_limit_enabled`] is true. Default 3; valid range 1..=20.
    #[serde(default = "default_snapshot_limit")]
    pub snapshot_limit: u32,

    #[serde(default)]
    pub last_failed_update: Option<FailedUpdate>,

    /// Cached result of the most recent successful release/commit lookup.
    #[serde(default)]
    pub latest_available: Option<LatestAvailable>,

    /// Previous known-good version whose images are pinned as `*:myriad-rollback`.
    /// The alias migrates state written by updater versions that used the internal
    /// `last_good_version` name.
    #[serde(default, alias = "last_good_version")]
    pub rollback_version: Option<DeployTag>,
}

fn default_true() -> bool {
    true
}

/// Matches the historical hard-coded `prune(3)` retention count.
pub const SNAPSHOT_LIMIT_DEFAULT: u32 = 3;
pub const SNAPSHOT_LIMIT_MIN: u32 = 1;
pub const SNAPSHOT_LIMIT_MAX: u32 = 20;

fn default_snapshot_limit() -> u32 {
    SNAPSHOT_LIMIT_DEFAULT
}

impl Default for UpdaterStateFile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            current_version: None,
            current_commit_sha: None,
            updater_version: None,
            last_checked_at: None,
            channel: "stable".into(),
            update_mode: UpdateMode::Release,
            check_interval_secs: None,
            auto_install: false,
            snapshot_limit_enabled: true,
            snapshot_limit: SNAPSHOT_LIMIT_DEFAULT,
            last_failed_update: None,
            latest_available: None,
            rollback_version: None,
        }
    }
}

/// Cached snapshot of "what would `/available` return right now".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestAvailable {
    pub version: DeployTag,
    pub channel: String,
    #[serde(default)]
    pub mode: UpdateMode,
    /// Metadata provider used to discover this target (`github` or `dockerhub`).
    #[serde(default)]
    pub source: Option<String>,
    pub seen_at: DateTime<Utc>,
    /// Full git sha when mode=commit (optional; short tag is in `version`).
    #[serde(default)]
    pub commit_sha: Option<String>,
    /// Running deploy resolved to a git sha (for commit freshness).
    #[serde(default)]
    pub current_commit_sha: Option<String>,
    /// Ancestry of target vs current: ahead | behind | identical | diverged | unknown.
    #[serde(default)]
    pub relation: Option<String>,
    #[serde(default)]
    pub ahead_by: Option<u32>,
    #[serde(default)]
    pub behind_by: Option<u32>,
    /// True when target is an upgrade relative to current (ancestry/semver).
    #[serde(default)]
    pub is_upgrade: Option<bool>,
    /// True when target is older than current (explicit downgrade path).
    #[serde(default)]
    pub is_downgrade: Option<bool>,
    #[serde(default)]
    pub requires_self_update: bool,
    #[serde(default)]
    pub min_updater_version: Option<MyriadVersion>,
    #[serde(default)]
    pub notes_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedUpdate {
    pub from_version: Option<DeployTag>,
    pub to_version: Option<DeployTag>,
    pub at: DateTime<Utc>,
    pub reason: String,
    pub job_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceFile {
    #[serde(default = "default_schema")]
    pub schema_version: u32,
    pub active: bool,
    pub phase: Phase,
    pub from_version: Option<DeployTag>,
    pub to_version: Option<DeployTag>,
    pub started_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub job_id: Option<String>,
    pub message_key: String,
}

impl MaintenanceFile {
    pub fn inactive() -> Self {
        Self {
            schema_version: 1,
            active: false,
            phase: Phase::Idle,
            from_version: None,
            to_version: None,
            started_at: None,
            updated_at: Utc::now(),
            job_id: None,
            message_key: "updater.phase.idle".into(),
        }
    }

    pub fn bump_heartbeat(&mut self) {
        self.updated_at = Utc::now();
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Checking,
    Ready,
    Preflight,
    MaintenanceOn,
    Stopping,
    Snapshotting,
    SwapTag,
    StartingNew,
    HealthProbing,
    SwappingProxy,
    Finalize,
    RollbackInProgress,
    StopNew,
    RestoreSnapshot,
    SwapTagBack,
    StartOld,
    NeedsManual,
    Cleanup,
}

impl Phase {
    /// Whether the proxy should take the site offline (`maintenance.active=true`).
    ///
    /// Preflight / Checking / Ready run with all services still up — they must
    /// **not** flip `active` or the proxy serves maintenance.html and the admin
    /// SPA navigates away. Failures in those phases clear the job without ever
    /// having stopped the stack (see `preflight.rs` module docs).
    pub fn takes_site_offline(self) -> bool {
        use Phase::*;
        !matches!(self, Idle | Checking | Ready | Preflight)
    }

    /// Phases after `MYRIAD_TAG` has been rewritten (destructive zone).
    ///
    /// **`SwapTag` is excluded**: the phase is entered *before* the tag write.
    /// Crash recovery must disambiguate pre-write vs post-write via `.env`
    /// (see `plan_crash_recovery` + `env_myriad_tag`).
    pub fn is_post_swap(self) -> bool {
        use Phase::*;
        matches!(
            self,
            StartingNew | HealthProbing | SwappingProxy | Finalize
        )
    }

    pub fn is_rollback(self) -> bool {
        use Phase::*;
        matches!(
            self,
            RollbackInProgress | StopNew | RestoreSnapshot | SwapTagBack | StartOld
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub kind: JobKind,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub from_version: Option<DeployTag>,
    pub to_version: Option<DeployTag>,
    pub snapshot_id: Option<String>,
    pub status: JobStatus,
    pub steps: Vec<JobStep>,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Update,
    Rollback,
    SelfUpdate,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    NeedsManual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStep {
    pub phase: Phase,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub ok: Option<bool>,
    pub log_tail: String,
    pub error: Option<String>,
}

impl JobStep {
    pub fn start(phase: Phase) -> Self {
        Self {
            phase,
            started_at: Utc::now(),
            finished_at: None,
            ok: None,
            log_tail: String::new(),
            error: None,
        }
    }

    pub fn finish_ok(&mut self) {
        self.finished_at = Some(Utc::now());
        self.ok = Some(true);
    }

    pub fn finish_err(&mut self, err: impl Into<String>) {
        self.finished_at = Some(Utc::now());
        self.ok = Some(false);
        self.error = Some(err.into());
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapshotsFile {
    #[serde(default = "default_schema")]
    pub schema_version: u32,
    #[serde(default)]
    pub items: Vec<SnapshotMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub source_version: Option<DeployTag>,
    pub size_bytes: u64,
    pub file_count: u64,
    pub keep: bool,
    pub sample_sha256: Option<String>,
}

fn default_schema() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollback_version_reads_legacy_state_name_and_writes_the_new_name() {
        let mut value = serde_json::to_value(UpdaterStateFile::default()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("rollback_version");
        object.insert(
            "last_good_version".into(),
            serde_json::Value::String("v0.2.2".into()),
        );

        let state: UpdaterStateFile = serde_json::from_value(value).unwrap();
        assert_eq!(
            state.rollback_version.as_ref().map(DeployTag::as_str),
            Some("v0.2.2")
        );
        let migrated = serde_json::to_value(state).unwrap();
        assert_eq!(migrated["rollback_version"], "v0.2.2");
        assert!(migrated.get("last_good_version").is_none());
    }

    #[test]
    fn preflight_and_idle_do_not_take_site_offline() {
        // Preflight failures must never put the proxy into maintenance mode.
        for phase in [Phase::Idle, Phase::Checking, Phase::Ready, Phase::Preflight] {
            assert!(
                !phase.takes_site_offline(),
                "{phase:?} must keep maintenance.active=false"
            );
        }
        assert!(Phase::MaintenanceOn.takes_site_offline());
        assert!(Phase::Stopping.takes_site_offline());
        assert!(Phase::NeedsManual.takes_site_offline());
    }
}
