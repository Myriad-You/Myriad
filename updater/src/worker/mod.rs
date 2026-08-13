//! Update worker: serializes update/rollback jobs through a single-slot state machine.
//!
//! Concurrency model:
//! - The HTTP API never executes long-running work directly.
//! - It enqueues commands into a bounded MPSC channel consumed by a single worker task.
//! - The state machine ensures at most one job is in flight; further `update` requests get 409.

pub mod machine;
pub mod preflight;
pub mod preflight_env;
pub mod proxy_update;
pub mod rollback;
pub mod self_update;
pub mod update;

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::config::{Channel, Config};
use crate::docker::DockerClient;
use crate::error::{Result, UpdaterError};
use crate::release::{
    commit_upgrade_direction_ex, is_cross_kind_deploy, pushed_at_for_tag,
    select_dev_channel_tip_for, DockerBuild, DockerHubClient, GithubClient, Manifest,
};
use crate::state::{Job, JobKind, JobStatus, LatestAvailable, MaintenanceFile, Phase, StateDir};
use crate::version::{
    commit_branch_for_channel, DeployTag, DeployTagKind, MyriadVersion, UpdateMode,
};

/// CLI parameters shared with the worker.
#[derive(Debug, Clone)]
pub struct WorkerCli {
    pub state_dir: std::path::PathBuf,
    pub compose_dir: std::path::PathBuf,
    pub env_file: std::path::PathBuf,
    pub pgdata: std::path::PathBuf,
    pub listen: String,
    /// Resolved `MYRIAD_DB_MODE` (`bundled` default). Controls pgdata snapshot/restore.
    pub db_mode: crate::config::DbMode,
}

#[derive(Debug)]
pub enum Command {
    Update {
        target: DeployTag,
        mode: UpdateMode,
        /// Required when target is older than current; UI must confirm first.
        allow_downgrade: bool,
        /// Umbrella for diverged / unknown / irreversible (see also granular flags).
        allow_risk: bool,
        allow_diverged: Option<bool>,
        allow_unknown: Option<bool>,
        allow_irreversible: Option<bool>,
        idempotency_key: Option<String>,
        /// Optional admin actor from backend (`X-Update-Actor`), for audit only.
        actor: Option<String>,
        reply: tokio::sync::oneshot::Sender<Result<String>>,
    },
    ListCommits {
        branch: String,
        limit: u32,
        reply: tokio::sync::oneshot::Sender<Result<Vec<crate::release::CommitInfo>>>,
    },
    ListBuilds {
        limit: u32,
        reply: tokio::sync::oneshot::Sender<Result<Vec<DockerBuild>>>,
    },
    ListReleases {
        channel: Option<String>,
        limit: u32,
        reply: tokio::sync::oneshot::Sender<Result<Vec<crate::release::github::Release>>>,
    },
    Compare {
        from: Option<String>,
        to: String,
        reply: tokio::sync::oneshot::Sender<Result<crate::release::Freshness>>,
    },
    Rollback {
        snapshot_id: String,
        actor: Option<String>,
        reply: tokio::sync::oneshot::Sender<Result<String>>,
    },
    CheckUpdates {
        /// Ephemeral overrides — do NOT persist prefs (use SetPrefs for that).
        /// Availability cache is refreshed only when the resolved request still
        /// matches the saved prefs.
        channel: Option<String>,
        mode: Option<UpdateMode>,
        reply: tokio::sync::oneshot::Sender<Result<Option<AvailableInfo>>>,
    },
    SetPrefs {
        channel: Option<String>,
        mode: Option<UpdateMode>,
        check_interval_secs: Option<Option<u64>>,
        auto_install: Option<bool>,
        /// Toggle auto-prune of pgdata snapshots.
        snapshot_limit_enabled: Option<bool>,
        /// Max non-keep / non-protected snapshots to retain when limit is enabled (1..=20).
        snapshot_limit: Option<u32>,
        reply: tokio::sync::oneshot::Sender<Result<Prefs>>,
    },
    SelfUpdate {
        actor: Option<String>,
        reply: tokio::sync::oneshot::Sender<Result<self_update::SelfUpdateReport>>,
    },
    /// Manual proxy image upgrade (not part of business auto-update).
    ProxyUpdate {
        actor: Option<String>,
        /// When set, fetch this release's proxy image; otherwise latest for channel.
        explicit_tag: Option<String>,
        reply: tokio::sync::oneshot::Sender<Result<proxy_update::ProxyUpdateReport>>,
    },
    Shutdown,
}

/// Unified "what's available" for both release and commit modes.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum AvailableInfo {
    Release(Manifest),
    Commit {
        tag: DeployTag,
        full_sha: String,
        message: String,
        branch: String,
        notes_url: String,
        source: String,
        /// Ancestry of branch tip vs currently running deploy (if resolvable).
        freshness: Option<crate::release::Freshness>,
        /// Final upgrade direction (push-time and/or ancestry). Used by status/UI/auto_install.
        is_upgrade: Option<bool>,
        is_downgrade: Option<bool>,
        relation: Option<String>,
    },
}

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

pub struct Worker {
    state: Arc<StateDir>,
    docker: Arc<DockerClient>,
    config: Config,
    cli: WorkerCli,
    tx: mpsc::Sender<Command>,
    rx: Mutex<Option<mpsc::Receiver<Command>>>,
    /// Recent idempotency keys → job id.
    idempotency: Mutex<std::collections::VecDeque<(String, String)>>,
}

#[derive(Debug, Clone)]
pub enum RecoveryReport {
    Idle,
    ResumedRollback(String),
    NeedsManual {
        job_id: String,
        phase: Phase,
        reason: String,
    },
    ClearedPreSwap,
    NoChange,
}

/// Pure recovery decision (unit-tested). See [`Worker::recover_or_idle`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrashRecoveryPlan {
    Idle,
    /// Pre-swap stop/snapshot interrupted — clear maint and restart previous stack.
    ClearPreSwap {
        job_id: String,
        phase: Phase,
    },
    /// Post-swap / rollback / needs_manual — never auto-destructive; operator rescue.
    NeedsManual {
        job_id: String,
        phase: Phase,
        reason: String,
    },
    /// `maintenance.active` with no job id and no `job.current`.
    ClearOrphanMaintenance,
}

/// Decide crash recovery without I/O (except the optional `env_myriad_tag` snapshot).
///
/// Inputs:
/// - `maint` — on-disk maintenance.json (may have `active=false` during live frontend probe)
/// - `current_job_id` — job.current
/// - `job` — loaded job file when either id is known
/// - `env_myriad_tag` — current `MYRIAD_TAG` from `.env` (if readable); used to tell
///   pre-write `SwapTag` (tag still old → ClearPreSwap) from post-write (tag matches
///   `job.to_version` → NeedsManual)
pub fn plan_crash_recovery(
    maint: &MaintenanceFile,
    current_job_id: Option<&str>,
    job: Option<&Job>,
    env_myriad_tag: Option<&str>,
) -> CrashRecoveryPlan {
    let job_id = maint
        .job_id
        .as_deref()
        .or(current_job_id)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let job_inflight = job.is_some_and(|j| {
        matches!(
            j.status,
            JobStatus::Running | JobStatus::Pending | JobStatus::NeedsManual
        )
    });
    let last_step_phase = job.and_then(|j| j.steps.last().map(|s| s.phase));

    // Effective phase: prefer maintenance phase when it still carries work context;
    // otherwise fall back to the job's last step (covers active=false health probe).
    let effective_phase = {
        let mp = maint.phase;
        if mp.is_post_swap()
            || mp.is_rollback()
            || matches!(mp, Phase::NeedsManual | Phase::SwapTag)
        {
            mp
        } else if let Some(lp) = last_step_phase {
            if lp.is_post_swap()
                || lp.is_rollback()
                || matches!(lp, Phase::NeedsManual | Phase::SwapTag)
            {
                lp
            } else if maint.active {
                mp
            } else if job_inflight {
                lp
            } else {
                mp
            }
        } else {
            mp
        }
    };

    // SwapTag is entered *before* writing MYRIAD_TAG. Only treat as post-swap when
    // .env already shows the job's target tag.
    if matches!(effective_phase, Phase::SwapTag) {
        let to = job.and_then(|j| j.to_version.as_ref().map(|t| t.as_str()));
        let post_write = matches!(
            (to, env_myriad_tag),
            (Some(t), Some(e)) if t == e
        );
        if let Some(ref id) = job_id {
            if post_write {
                return CrashRecoveryPlan::NeedsManual {
                    job_id: id.clone(),
                    phase: Phase::SwapTag,
                    reason: format!(
                        "recovered mid-SwapTag after MYRIAD_TAG already matches target \
                         ({}); manual intervention required",
                        to.unwrap_or("?")
                    ),
                };
            }
            if job_inflight || maint.active {
                return CrashRecoveryPlan::ClearPreSwap {
                    job_id: id.clone(),
                    phase: Phase::SwapTag,
                };
            }
        } else if maint.active {
            return CrashRecoveryPlan::ClearOrphanMaintenance;
        }
    }

    // Post-swap / rollback / already needs_manual with a recoverable job identity.
    if effective_phase.is_post_swap()
        || effective_phase.is_rollback()
        || matches!(effective_phase, Phase::NeedsManual)
    {
        // Even when maintenance was lifted (active=false) for frontend probe, or
        // job is only referenced via job.current — never silently Idle.
        if let Some(ref id) = job_id {
            if job_inflight
                || maint.active
                || matches!(effective_phase, Phase::NeedsManual)
                || job.is_some_and(|j| matches!(j.status, JobStatus::NeedsManual))
            {
                return CrashRecoveryPlan::NeedsManual {
                    job_id: id.clone(),
                    phase: effective_phase,
                    reason: format!(
                        "recovered into post-swap/rollback phase {:?} (maint.active={}, job_status={:?}); manual intervention required",
                        effective_phase,
                        maint.active,
                        job.map(|j| j.status)
                    ),
                };
            }
        } else if maint.active {
            return CrashRecoveryPlan::ClearOrphanMaintenance;
        }
    }

    // Pre-swap work in flight: services may be stopped.
    if let Some(ref id) = job_id {
        let pre_swap_phase = matches!(
            effective_phase,
            Phase::Preflight
                | Phase::MaintenanceOn
                | Phase::Stopping
                | Phase::Snapshotting
                | Phase::Cleanup
        ) || matches!(
            last_step_phase,
            Some(
                Phase::Preflight
                    | Phase::MaintenanceOn
                    | Phase::Stopping
                    | Phase::Snapshotting
                    | Phase::Cleanup
            )
        );
        if job_inflight && (maint.active || pre_swap_phase) {
            return CrashRecoveryPlan::ClearPreSwap {
                job_id: id.clone(),
                phase: effective_phase,
            };
        }
        // Active maintenance with terminal job should still clear the flag.
        if maint.active && !job_inflight {
            return CrashRecoveryPlan::ClearPreSwap {
                job_id: id.clone(),
                phase: effective_phase,
            };
        }
    } else if maint.active {
        return CrashRecoveryPlan::ClearOrphanMaintenance;
    }

    CrashRecoveryPlan::Idle
}

impl Worker {
    pub fn new(
        state: Arc<StateDir>,
        docker: Arc<DockerClient>,
        config: Config,
        cli: WorkerCli,
    ) -> Self {
        let (tx, rx) = mpsc::channel(16);
        Self {
            state,
            docker,
            config,
            cli,
            tx,
            rx: Mutex::new(Some(rx)),
            idempotency: Mutex::new(std::collections::VecDeque::with_capacity(100)),
        }
    }

    /// Block business updates while the stack is in a stuck rescue state.
    ///
    /// `NeedsManual` clears `job.current` when the spawn ends, so conflict alone
    /// is not enough — operators (or auto_install) could start another update
    /// and overwrite maintenance while services are still half-down.
    pub(crate) fn refuse_update_if_stuck(&self) -> Result<()> {
        let maint = self
            .state
            .read_maintenance()
            .unwrap_or_else(|_| crate::state::MaintenanceFile::inactive());
        if matches!(maint.phase, Phase::NeedsManual)
            || (maint.active
                && (maint.phase.is_post_swap()
                    || maint.phase.is_rollback()
                    || matches!(maint.phase, Phase::NeedsManual)))
        {
            return Err(UpdaterError::Conflict);
        }
        // Job file left in NeedsManual even if maintenance was partially cleared.
        if let Ok(Some(id)) = self.state.read_current_job() {
            if let Ok(job) = self.state.read_job(&id) {
                if matches!(job.status, JobStatus::NeedsManual) {
                    return Err(UpdaterError::Conflict);
                }
            }
        }
        Ok(())
    }

    pub fn cli(&self) -> &WorkerCli {
        &self.cli
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn state(&self) -> &Arc<StateDir> {
        &self.state
    }

    pub fn github_client(&self) -> Result<GithubClient> {
        use crate::release::CosignPolicy;
        let policy = CosignPolicy::from_env(Some(&self.config.cosign_verify));
        GithubClient::new(
            self.config.github_repo.clone(),
            self.config.github_token.clone(),
            self.state.cache_dir(),
            policy,
        )
    }

    /// Commit-mode GitHub metadata (branch tip, ancestry) needs a token for private repos.
    /// Without it we deliberately use Docker Hub image tags only — not an error condition.
    pub fn github_commit_metadata_enabled(&self) -> bool {
        self.config.github_token_present()
    }

    pub fn dockerhub_client(&self) -> Result<DockerHubClient> {
        DockerHubClient::new()
    }

    /// Image repositories are explicit deployment inputs. Shared by commit-mode preflight,
    /// release-mode Docker Hub fallback (when `release.json` is unavailable), and Hub discovery.
    pub fn image_repos_required(&self) -> Result<(String, String)> {
        let env = crate::env_file::EnvFile::load(&self.cli.env_file)?;
        let backend = env.get("BACKEND_IMAGE").map(str::to_owned).ok_or_else(|| {
            UpdaterError::Precondition(
                "BACKEND_IMAGE missing in .env; required for commit-mode and release Docker Hub \
                     image pulls. Add e.g. BACKEND_IMAGE=docker.io/<org>/myriad-backend (no tag) \
                     or re-run scripts/docker/deploy.sh to bootstrap defaults."
                    .into(),
            )
        })?;
        let frontend = env
            .get("FRONTEND_IMAGE")
            .map(str::to_owned)
            .ok_or_else(|| {
                UpdaterError::Precondition(
                    "FRONTEND_IMAGE missing in .env; required for commit-mode and release Docker \
                     Hub image pulls. Add e.g. FRONTEND_IMAGE=docker.io/<org>/myriad-frontend \
                     (no tag) or re-run scripts/docker/deploy.sh to bootstrap defaults."
                        .into(),
                )
            })?;
        if backend.trim().is_empty() || frontend.trim().is_empty() {
            return Err(UpdaterError::Precondition(
                "BACKEND_IMAGE / FRONTEND_IMAGE must be non-empty (image repo without tag)".into(),
            ));
        }
        Ok((backend, frontend))
    }

    /// Proxy image repository (no tag). Prefer `.env` `PROXY_IMAGE`; else compose default.
    pub fn proxy_image_repo(&self) -> Result<String> {
        self.optional_image_repo("PROXY_IMAGE", "docker.io/somekawahitomi/myriad-proxy")
    }

    /// Updater image repository (no tag). Prefer `.env` `UPDATER_IMAGE`; else compose default.
    pub fn updater_image_repo(&self) -> Result<String> {
        self.optional_image_repo("UPDATER_IMAGE", "docker.io/somekawahitomi/myriad-updater")
    }

    fn optional_image_repo(&self, key: &str, default: &str) -> Result<String> {
        let env = crate::env_file::EnvFile::load(&self.cli.env_file)?;
        let raw = env
            .get(key)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(default);
        // Normalize: strip accidental `:tag` so we never emit `repo:tag:other`.
        // Host:port is preserved (e.g. localhost:5000/ns/name) — only strip the
        // last `:` segment when a path is present and it is not a bare host:port.
        let value = strip_image_repo_tag(raw);
        if value.is_empty() {
            return Err(UpdaterError::Precondition(format!(
                "{key} must be an image repository without tag, got {raw}"
            )));
        }
        // Bare "v1.2.3" or similar without a path
        if value.contains(':') && !value.contains('/') {
            return Err(UpdaterError::Precondition(format!(
                "{key} must be an image repository without tag, got {raw}"
            )));
        }
        Ok(value)
    }

    pub fn docker(&self) -> &Arc<DockerClient> {
        &self.docker
    }

    /// After crash recovery clears a pre-swap job, restart app services that may
    /// still be stopped (frontend/backend, and postgres if bundled). Does not
    /// rewrite `MYRIAD_TAG` or restore snapshots.
    pub async fn restore_stack_after_pre_swap(self: &Arc<Self>) -> Result<()> {
        let compose = update::build_compose_runner_pub(self).await?;
        // Conservative: always try postgres (no-op-ish if already up / external skip via scope)
        // AppAndPostgres is safe for external mode — restore_previous_stack skips pg.
        update::restore_previous_stack(self, &compose, update::PreSwapRestoreScope::AppAndPostgres)
            .await
    }

    /// Pull an image, applying REGISTRY_MIRROR rewriting if configured. Returns the digest
    /// of the pulled image (`sha256:...`).
    pub async fn docker_pull_with_mirror(&self, image_ref: &str) -> Result<String> {
        let actual_ref = match &self.config.registry_mirror {
            Some(mirror) => rewrite_with_mirror(image_ref, mirror),
            None => image_ref.to_string(),
        };
        let digest = self.docker.pull(&actual_ref, None).await?;
        // Strip "<image>@" prefix, keep only "sha256:..."
        Ok(digest.split('@').next_back().unwrap_or(&digest).to_string())
    }

    pub fn sender(&self) -> mpsc::Sender<Command> {
        self.tx.clone()
    }

    /// Reconcile persisted state with the version embedded in the running backend image.
    ///
    /// This runs once at daemon startup. The backend build stamp is the strongest source
    /// because mutable branch tags may have advanced since the currently running image was
    /// pulled. Older images do not expose `commit_sha`, so we fall back to resolving the
    /// deploy tag through GitHub. If the backend is not ready yet, MYRIAD_TAG from the managed
    /// `.env` still prevents a fresh/cleared state directory from reporting "unknown".
    pub async fn reconcile_current_deploy(&self) -> Result<()> {
        let mut st = self.state.read_updater()?;
        let previous_version = st.current_version.clone();

        let runtime = self.probe_runtime_identity().await;
        let env_version = crate::env_file::EnvFile::load(&self.cli.env_file)
            .ok()
            .and_then(|env| env.get("MYRIAD_TAG").map(str::to_owned))
            .and_then(|raw| match DeployTag::parse(&raw) {
                Ok(tag) => Some(tag),
                Err(e) => {
                    warn!(value = %raw, err = %e, "ignoring invalid MYRIAD_TAG during startup reconciliation");
                    None
                }
            });

        let (version, embedded_sha, source) = match runtime {
            Some((version, sha)) => (Some(version), sha, "backend-health"),
            None => match env_version {
                Some(version) => (Some(version), None, "env-file"),
                None => (previous_version.clone(), None, "persisted-state"),
            },
        };
        let Some(version) = version else {
            warn!("current deploy version remains unknown after startup reconciliation");
            return Ok(());
        };

        let version_changed = previous_version.as_ref() != Some(&version);
        let mut commit_sha = embedded_sha.or_else(|| {
            version
                .commit_sha()
                .filter(|sha| sha.len() == 40)
                .map(str::to_owned)
        });

        if commit_sha.is_none() && version.kind() != DeployTagKind::Branch {
            // Private repos without GITHUB_TOKEN always 404; backend /health already
            // supplies commit_sha for stamped images — skip noisy GitHub calls.
            if self.github_commit_metadata_enabled() {
                let git_ref = crate::release::deploy_tag_to_git_ref(&version);
                match self.github_client() {
                    Ok(gh) => match gh.resolve_commit(&git_ref).await {
                        Ok(info) => commit_sha = Some(info.sha),
                        Err(e) => {
                            if GithubClient::is_expected_unauthenticated_failure(&e) {
                                info!(
                                    tag = %version,
                                    "GitHub commit resolve skipped/failed (private or no access); using runtime identity only"
                                );
                            } else {
                                warn!(tag = %version, err = %e, "could not resolve current deploy commit during startup");
                            }
                        }
                    },
                    Err(e) => {
                        warn!(tag = %version, err = %e, "GitHub client unavailable during startup reconciliation")
                    }
                }
            }
            if commit_sha.is_none() && !version_changed {
                commit_sha = st.current_commit_sha.clone();
            }
        } else if commit_sha.is_none() && version.kind() == DeployTagKind::Branch {
            // A mutable branch name only identifies the image that would be pulled now, not the
            // image already running. Wait for backend /health rather than persisting a false SHA.
            commit_sha = None;
        }

        let state_changed =
            version_changed || st.current_commit_sha != commit_sha || st.current_version.is_none();
        st.current_version = Some(version.clone());
        st.current_commit_sha = commit_sha.clone();
        if state_changed {
            self.state.write_updater(&st)?;
        }
        info!(
            version = %version,
            commit_sha = ?commit_sha,
            %source,
            "current deploy identity reconciled"
        );
        Ok(())
    }

    async fn probe_runtime_identity(&self) -> Option<(DeployTag, Option<String>)> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .ok()?;
        let response = client
            .get("http://backend:1103/health")
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        let body: serde_json::Value = response.json().await.ok()?;
        runtime_identity_from_json(&body)
    }

    /// Try to recover from prior crash. Per spec §7.1 we are conservative: anything
    /// post-`swap_tag` becomes `needs_manual` unless we were specifically in a rollback flow.
    ///
    /// **Critical**: health probe phase 2 sets `maintenance.active=false` while the job is
    /// still running post-swap. Recovery must NOT treat that as Idle (old bug: crash mid
    /// probe → silent no-op, stack left on the new tag with no rollback / no needs_manual).
    pub async fn recover_or_idle(
        state: Arc<StateDir>,
        _docker: Arc<DockerClient>,
        env_file: Option<&std::path::Path>,
    ) -> Result<RecoveryReport> {
        Self::recover_or_idle_state(state, env_file).await
    }

    /// State-only recovery (unit/integration tests; no Docker required).
    pub async fn recover_or_idle_state(
        state: Arc<StateDir>,
        env_file: Option<&std::path::Path>,
    ) -> Result<RecoveryReport> {
        let maint = state.read_maintenance()?;
        let current_job_id = state.read_current_job()?;
        let job_id = maint
            .job_id
            .clone()
            .or_else(|| current_job_id.clone())
            .filter(|s| !s.is_empty());
        let job = match job_id.as_deref() {
            Some(id) => state.read_job(id).ok(),
            None => None,
        };

        let env_tag = env_file.and_then(|p| {
            crate::env_file::EnvFile::load(p)
                .ok()
                .and_then(|e| e.get("MYRIAD_TAG").map(|s| s.to_string()))
        });
        let plan = plan_crash_recovery(&maint, job_id.as_deref(), job.as_ref(), env_tag.as_deref());
        match plan {
            CrashRecoveryPlan::Idle => Ok(RecoveryReport::Idle),
            CrashRecoveryPlan::NeedsManual {
                job_id,
                phase,
                reason,
            } => {
                if let Ok(mut job) = state.read_job(&job_id) {
                    job.status = JobStatus::NeedsManual;
                    job.steps
                        .push(crate::state::JobStep::start(Phase::NeedsManual));
                    if let Some(step) = job.steps.last_mut() {
                        step.finish_err(reason.clone());
                    }
                    if job.finished_at.is_none() {
                        job.finished_at = Some(Utc::now());
                    }
                    let _ = state.write_job(&job);
                }
                let mut m = maint;
                m.active = true;
                m.phase = Phase::NeedsManual;
                m.job_id = Some(job_id.clone());
                m.message_key = "updater.phase.needs_manual".into();
                m.bump_heartbeat();
                state.write_maintenance(&m)?;
                // Keep job.current so status/UI see the stuck job and rescue hint.
                let _ = state.set_current_job(Some(&job_id));
                state.append_history(&format!(
                    "recovery: job {job_id} stuck at {phase:?}; needs_manual ({reason})"
                ))?;
                let _ = state.append_audit(&format!(
                    "audit: recovery_needs_manual job={job_id} phase={phase:?}"
                ));
                Ok(RecoveryReport::NeedsManual {
                    job_id,
                    phase,
                    reason,
                })
            }
            CrashRecoveryPlan::ClearPreSwap { job_id, phase } => {
                info!(%job_id, ?phase, "recovery: clearing pre-swap maintenance state");
                if let Ok(mut job) = state.read_job(&job_id) {
                    if matches!(job.status, JobStatus::Running | JobStatus::Pending) {
                        job.status = JobStatus::Failed;
                        job.finished_at = Some(Utc::now());
                        if let Some(step) = job.steps.last_mut() {
                            if step.finished_at.is_none() {
                                step.finish_err(
                                    "updater restarted during pre-swap; stack restore will be attempted",
                                );
                            }
                        }
                    }
                    let _ = state.write_job(&job);
                }
                state.clear_maintenance()?;
                state.set_current_job(None)?;
                state.append_history(&format!(
                    "recovery: pre-swap cleanup for job {job_id} phase={phase:?} (will restore app stack)"
                ))?;
                let _ = state.append_audit(&format!(
                    "audit: recovery_pre_swap_clear job={job_id} phase={phase:?}"
                ));
                Ok(RecoveryReport::ClearedPreSwap)
            }
            CrashRecoveryPlan::ClearOrphanMaintenance => {
                warn!("maintenance active but no recoverable job; clearing");
                state.clear_maintenance()?;
                state.set_current_job(None)?;
                state.append_history("recovery: cleared orphan maintenance (no job)")?;
                Ok(RecoveryReport::ClearedPreSwap)
            }
        }
    }

    /// Spawn the worker loop AND the periodic update checker (interval from prefs / env).
    /// The returned handle resolves when the main loop exits.
    pub fn spawn(self: Arc<Self>) -> JoinHandle<()> {
        let rx = self
            .rx
            .try_lock()
            .expect("spawn() called twice")
            .take()
            .expect("worker rx already taken");
        let me = self.clone();

        // Periodic poller. Interval is re-read each cycle so prefs hot-reload without restart.
        // Each tick enqueues CheckUpdates on the single-slot worker channel.
        let ticker_worker = me.clone();
        tokio::spawn(async move {
            // Initial delay so we don't hammer GitHub on a crash-loop restart.
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            loop {
                let interval_secs = ticker_worker.effective_check_interval_secs();
                if interval_secs == 0 {
                    // Checks disabled — re-read prefs periodically so enabling hot-reloads.
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    continue;
                }

                let (tx, rx) = tokio::sync::oneshot::channel();
                if ticker_worker
                    .tx
                    .send(Command::CheckUpdates {
                        channel: None,
                        mode: None,
                        reply: tx,
                    })
                    .await
                    .is_err()
                {
                    // Worker dropped — exit the ticker too.
                    break;
                }
                match rx.await {
                    Ok(Ok(Some(AvailableInfo::Release(m)))) => {
                        tracing::info!(
                            target_version = %m.version,
                            channel = %m.channel,
                            "periodic check: release available"
                        );
                        if let Err(e) = ticker_worker.clone().maybe_auto_install_release(&m).await {
                            tracing::warn!(err = %e, "periodic auto_install skipped/failed");
                        }
                    }
                    Ok(Ok(Some(AvailableInfo::Commit {
                        tag,
                        branch,
                        relation,
                        is_upgrade,
                        ..
                    }))) => {
                        tracing::info!(
                            target = %tag,
                            %branch,
                            relation = relation.as_deref().unwrap_or("?"),
                            is_upgrade = ?is_upgrade,
                            "periodic check: commit tip available"
                        );
                        if let Err(e) = ticker_worker.clone().maybe_auto_install_commit(&tag).await
                        {
                            tracing::warn!(err = %e, "periodic auto_install skipped/failed");
                        }
                    }
                    Ok(Ok(None)) => {
                        tracing::debug!("periodic check: nothing available for channel");
                    }
                    Ok(Err(e)) => {
                        tracing::warn!(err = %e, "periodic check: github lookup failed");
                    }
                    Err(_) => break, // worker shutdown
                }

                // Self-heal backup pile without waiting for UI open or another update.
                ticker_worker.best_effort_prune_snapshots("periodic_check");

                // Re-read interval after the check so a prefs change takes effect promptly.
                let sleep_secs = ticker_worker.effective_check_interval_secs().max(1);
                tokio::time::sleep(std::time::Duration::from_secs(sleep_secs)).await;
            }
        });

        tokio::spawn(async move {
            me.run(rx).await;
        })
    }

    pub async fn shutdown(&self) {
        let _ = self.tx.send(Command::Shutdown).await;
    }

    async fn run(self: Arc<Self>, mut rx: mpsc::Receiver<Command>) {
        info!("worker loop started");
        while let Some(cmd) = rx.recv().await {
            match cmd {
                Command::Shutdown => {
                    info!("worker shutting down");
                    break;
                }
                Command::Update {
                    target,
                    mode,
                    allow_downgrade,
                    allow_risk,
                    allow_diverged,
                    allow_unknown,
                    allow_irreversible,
                    idempotency_key,
                    actor,
                    reply,
                } => {
                    let res = self
                        .clone()
                        .handle_update(
                            target,
                            mode,
                            allow_downgrade,
                            allow_risk,
                            allow_diverged,
                            allow_unknown,
                            allow_irreversible,
                            idempotency_key,
                            actor,
                        )
                        .await;
                    let _ = reply.send(res);
                }
                Command::ListCommits {
                    branch,
                    limit,
                    reply,
                } => {
                    let res = self.clone().handle_list_commits(branch, limit).await;
                    let _ = reply.send(res);
                }
                Command::ListBuilds { limit, reply } => {
                    let res = self.clone().handle_list_builds(limit).await;
                    let _ = reply.send(res);
                }
                Command::ListReleases {
                    channel,
                    limit,
                    reply,
                } => {
                    let res = self.clone().handle_list_releases(channel, limit).await;
                    let _ = reply.send(res);
                }
                Command::Compare { from, to, reply } => {
                    let res = self.clone().handle_compare(from, to).await;
                    let _ = reply.send(res);
                }
                Command::Rollback {
                    snapshot_id,
                    actor,
                    reply,
                } => {
                    let res = self.clone().handle_rollback(snapshot_id, actor).await;
                    let _ = reply.send(res);
                }
                Command::CheckUpdates {
                    channel,
                    mode,
                    reply,
                } => {
                    let res = self.clone().handle_check_updates(channel, mode).await;
                    let _ = reply.send(res);
                }
                Command::SetPrefs {
                    channel,
                    mode,
                    check_interval_secs,
                    auto_install,
                    snapshot_limit_enabled,
                    snapshot_limit,
                    reply,
                } => {
                    let res = self
                        .clone()
                        .handle_set_prefs(
                            channel,
                            mode,
                            check_interval_secs,
                            auto_install,
                            snapshot_limit_enabled,
                            snapshot_limit,
                        )
                        .await;
                    let _ = reply.send(res);
                }
                Command::SelfUpdate { actor, reply } => {
                    let res = self_update::run(self.clone(), actor).await;
                    let _ = reply.send(res);
                }
                Command::ProxyUpdate {
                    actor,
                    explicit_tag,
                    reply,
                } => {
                    let res = proxy_update::run(self.clone(), actor, explicit_tag).await;
                    let _ = reply.send(res);
                }
            }
        }
    }

    /// Effective channel: state preference, else config default.
    pub fn effective_channel(&self) -> String {
        self.state
            .read_updater()
            .ok()
            .map(|s| s.channel)
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| self.config.channel.to_string())
    }

    pub fn effective_mode(&self) -> UpdateMode {
        self.state
            .read_updater()
            .ok()
            .map(|s| s.update_mode)
            .unwrap_or(UpdateMode::Release)
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
    /// success/failure cleanup paths.
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

    /// Shared safety gate for auto-install: only clear upgrades on the current
    /// channel/mode. Downgrade / diverged / irreversible need human confirm.
    ///
    /// **Commit/dev mode**: `relation=unknown` does **not** block auto-install when
    /// `is_upgrade` is true (build-time newer is enough). Release channels still
    /// reject unknown. Applies to **all** channels (stable / preview / commit).
    ///
    /// Dev channel may surface a formal `vX.Y.Z` tip; that tip is cached/installed
    /// via the release path even though prefs `update_mode` remains `commit`.
    fn auto_install_target_ok(
        &self,
        install_mode: UpdateMode,
        irreversible: bool,
    ) -> Result<Option<DeployTag>> {
        if !self.auto_install_enabled() {
            return Ok(None);
        }
        if self.state.read_current_job()?.is_some() {
            return Ok(None);
        }
        if self.refuse_update_if_stuck().is_err() {
            return Ok(None);
        }
        let st = self.state.read_updater()?;
        let Some(la) = st.latest_available.as_ref() else {
            return Ok(None);
        };
        if la.mode != install_mode {
            return Ok(None);
        }
        let effective = self.effective_mode();
        let prefs_allow = match effective {
            // Release-channel prefs only auto-install release tips.
            UpdateMode::Release => install_mode == UpdateMode::Release,
            // Dev/commit prefs: commit tips, or formal release tips discovered as tip.
            UpdateMode::Commit => {
                install_mode == UpdateMode::Commit
                    || (install_mode == UpdateMode::Release && la.version.is_release())
            }
        };
        if !prefs_allow {
            return Ok(None);
        }
        if !auto_install_latest_ok(
            install_mode,
            la.is_upgrade,
            la.is_downgrade,
            la.relation.as_deref(),
            la.requires_self_update,
            irreversible,
        ) {
            return Ok(None);
        }
        let target = la.version.clone();
        if st.current_version.as_ref() == Some(&target) {
            return Ok(None);
        }
        Ok(Some(target))
    }

    async fn dispatch_auto_install(
        self: &Arc<Self>,
        target: DeployTag,
        mode: UpdateMode,
    ) -> Result<()> {
        info!(
            target = %target,
            mode = %mode,
            channel = %self.effective_channel(),
            "auto_install: dispatching clear upgrade"
        );
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(Command::Update {
                target: target.clone(),
                mode,
                allow_downgrade: false,
                allow_risk: false,
                allow_diverged: None,
                allow_unknown: None,
                allow_irreversible: None,
                idempotency_key: Some(format!(
                    "auto-install-{}-{}",
                    mode.as_str(),
                    target.as_str()
                )),
                actor: Some("auto-install".into()),
                reply: tx,
            })
            .await
            .map_err(|_| UpdaterError::Conflict)?;
        let _ = rx.await.map_err(|_| {
            UpdaterError::Precondition("worker dropped auto_install reply".into())
        })??;
        Ok(())
    }

    /// Auto-install a clear release upgrade on the current channel (stable or preview).
    async fn maybe_auto_install_release(self: Arc<Self>, manifest: &Manifest) -> Result<()> {
        let Some(target) =
            self.auto_install_target_ok(UpdateMode::Release, manifest.migrations.irreversible)?
        else {
            return Ok(());
        };
        // Prefer the just-fetched manifest version when it matches the cache.
        let target = if target.as_str() == manifest.version.as_str() {
            DeployTag::from_release(manifest.version.clone())
        } else {
            target
        };
        self.dispatch_auto_install(target, UpdateMode::Release)
            .await
    }

    /// Auto-install a clear commit/dev tip upgrade on the current channel.
    /// Formal release tips discovered under commit prefs install via the release path
    /// (preflight/API also re-resolve mode from `target.is_release()`).
    async fn maybe_auto_install_commit(self: Arc<Self>, tag: &DeployTag) -> Result<()> {
        let install_mode = if tag.is_release() {
            UpdateMode::Release
        } else {
            UpdateMode::Commit
        };
        let Some(target) = self.auto_install_target_ok(install_mode, false)? else {
            return Ok(());
        };
        // Prefer the tip just reported by the check when it matches cache.
        let target = if target.as_str() == tag.as_str() {
            tag.clone()
        } else {
            target
        };
        self.dispatch_auto_install(target, install_mode).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_update(
        self: Arc<Self>,
        target: DeployTag,
        mode: UpdateMode,
        allow_downgrade: bool,
        allow_risk: bool,
        allow_diverged: Option<bool>,
        allow_unknown: Option<bool>,
        allow_irreversible: Option<bool>,
        idempotency_key: Option<String>,
        actor: Option<String>,
    ) -> Result<String> {
        if let Some(k) = &idempotency_key {
            let cache = self.idempotency.lock().await;
            if let Some((_, jid)) = cache.iter().find(|(kk, _)| kk == k) {
                return Ok(jid.clone());
            }
        }

        if let Some(_existing) = self.state.read_current_job()? {
            return Err(UpdaterError::Conflict);
        }
        // NeedsManual / sticky post-swap maintenance must not start another business update.
        // Rollback and rescue APIs remain available.
        self.refuse_update_if_stuck()?;

        let job_id = uuid::Uuid::new_v4().simple().to_string();
        let from_version = self.state.read_updater()?.current_version;
        let job = Job {
            id: job_id.clone(),
            kind: JobKind::Update,
            created_at: Utc::now(),
            finished_at: None,
            from_version,
            to_version: Some(target.clone()),
            snapshot_id: None,
            status: JobStatus::Pending,
            steps: Vec::new(),
            idempotency_key: idempotency_key.clone(),
        };
        self.state.write_job(&job)?;
        self.state.set_current_job(Some(&job_id))?;

        if let Some(k) = idempotency_key {
            let mut cache = self.idempotency.lock().await;
            cache.push_back((k, job_id.clone()));
            if cache.len() > 100 {
                cache.pop_front();
            }
        }

        let job_id_clone = job_id.clone();
        let me = self.clone();
        let risk = preflight::RiskFlags::from_api(
            allow_downgrade,
            allow_risk,
            allow_diverged,
            allow_unknown,
            allow_irreversible,
        );
        tokio::spawn(async move {
            if let Err(e) =
                update::run(me.clone(), job_id_clone.clone(), target, mode, risk, actor).await
            {
                error!(job = %job_id_clone, err = %e, "update flow exited with error");
            }
            let _ = me.state.set_current_job(None);
        });

        Ok(job_id)
    }

    async fn handle_list_commits(
        self: Arc<Self>,
        branch: String,
        limit: u32,
    ) -> Result<Vec<crate::release::CommitInfo>> {
        let branch = if branch.trim().is_empty() {
            commit_branch_for_channel(&self.effective_channel()).to_string()
        } else {
            commit_branch_for_channel(branch.trim()).to_string()
        };
        // Empty list → API/UI falls through to Docker Hub /builds (private repo, no token).
        if !self.github_commit_metadata_enabled() {
            info!(
                %branch,
                "commit list: GITHUB_TOKEN unset; returning empty (use Docker Hub builds)"
            );
            return Ok(Vec::new());
        }
        let gh = self.github_client()?;
        match gh.list_commits(&branch, limit).await {
            Ok(items) => Ok(items),
            Err(e) if GithubClient::is_expected_unauthenticated_failure(&e) => {
                info!(
                    err = %e,
                    %branch,
                    "commit list: GitHub unavailable; returning empty for Docker Hub fallback"
                );
                Ok(Vec::new())
            }
            Err(e) => Err(e),
        }
    }

    async fn handle_list_builds(self: Arc<Self>, limit: u32) -> Result<Vec<DockerBuild>> {
        let (backend, frontend) = self.image_repos_required()?;
        self.dockerhub_client()?
            .list_common_builds(&backend, &frontend, limit)
            .await
    }

    async fn handle_list_releases(
        self: Arc<Self>,
        channel: Option<String>,
        limit: u32,
    ) -> Result<Vec<crate::release::github::Release>> {
        let ch_name = channel.filter(|c| !c.trim().is_empty()).unwrap_or_else(|| {
            crate::version::release_channel_name_for_self_update(&self.effective_channel())
                .to_string()
        });
        let ch: Channel = ch_name.parse().unwrap_or(self.config.channel);
        let gh = self.github_client()?;
        gh.list_releases_for_channel(ch, limit).await
    }

    async fn handle_compare(
        self: Arc<Self>,
        from: Option<String>,
        to: String,
    ) -> Result<crate::release::Freshness> {
        let gh = self.github_client()?;
        let current = match from {
            Some(s) if !s.trim().is_empty() => Some(DeployTag::parse(s.trim())?),
            _ => self.state.read_updater()?.current_version,
        };
        gh.compare_deploy_to_ref(current.as_ref(), to.trim())
            .await?
            .ok_or_else(|| {
                UpdaterError::Precondition(
                    "could not resolve current deploy tag to a git commit for comparison".into(),
                )
            })
    }

    async fn handle_set_prefs(
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
            if let Ok(mut env) = crate::env_file::EnvFile::load(&self.cli.env_file) {
                let _ = env.set("UPDATE_MODE", m.as_str());
                let _ = env.save();
            }
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

    async fn handle_rollback(
        self: Arc<Self>,
        snapshot_id: String,
        actor: Option<String>,
    ) -> Result<String> {
        if let Some(_existing) = self.state.read_current_job()? {
            return Err(UpdaterError::Conflict);
        }
        let job_id = uuid::Uuid::new_v4().simple().to_string();
        let job = Job {
            id: job_id.clone(),
            kind: JobKind::Rollback,
            created_at: Utc::now(),
            finished_at: None,
            from_version: self.state.read_updater()?.current_version,
            to_version: None,
            snapshot_id: Some(snapshot_id.clone()),
            status: JobStatus::Pending,
            steps: Vec::new(),
            idempotency_key: None,
        };
        self.state.write_job(&job)?;
        self.state.set_current_job(Some(&job_id))?;

        let me = self.clone();
        let id = job_id.clone();
        tokio::spawn(async move {
            if let Err(e) = rollback::run(me.clone(), id.clone(), snapshot_id, actor).await {
                error!(job = %id, err = %e, "rollback flow exited with error");
            }
            let _ = me.state.set_current_job(None);
        });
        Ok(job_id)
    }

    async fn handle_check_updates(
        self: Arc<Self>,
        channel_override: Option<String>,
        mode_override: Option<UpdateMode>,
    ) -> Result<Option<AvailableInfo>> {
        // Ephemeral overrides for this check only — never write prefs here.
        // A caller may redundantly send the currently saved values; that is still
        // the canonical check and must refresh status(). Only a genuinely different
        // preview request is kept out of the shared availability cache.
        let saved_channel = self.effective_channel();
        let saved_mode = self.effective_mode();
        let (channel, mode, persist_cache) =
            resolve_check_request(&saved_channel, saved_mode, channel_override, mode_override);
        match mode {
            UpdateMode::Release => self.check_release_available(&channel, persist_cache).await,
            UpdateMode::Commit => self.check_commit_available(&channel, persist_cache).await,
        }
    }

    async fn check_release_available(
        self: Arc<Self>,
        channel: &str,
        persist_cache: bool,
    ) -> Result<Option<AvailableInfo>> {
        let gh = self.github_client()?;
        let ch: Channel = channel.parse().unwrap_or(self.config.channel);
        let Some(rel) = gh.latest_for_channel(ch).await? else {
            if persist_cache {
                let mut st = self.state.read_updater()?;
                st.last_checked_at = Some(Utc::now());
                st.latest_available = None;
                self.state.write_updater(&st)?;
            }
            return Ok(None);
        };
        let manifest = gh.fetch_manifest(&rel.tag_name).await?;

        let self_v = MyriadVersion::parse(crate::self_version()).ok();
        let requires_self_update = manifest.updater.self_update_required
            || self_v
                .as_ref()
                .is_some_and(|v| v.older_than(&manifest.updater.min_updater_version));
        let target_tag = DeployTag::from_release(manifest.version.clone());
        let current_state = self.state.read_updater().ok();
        let current = current_state
            .as_ref()
            .and_then(|s| s.current_version.clone());
        // Prefer semver when both are releases; otherwise git ancestry.
        let (is_upgrade, is_downgrade, relation) = match (
            current.as_ref().and_then(|c| c.as_release()),
            target_tag.as_release(),
        ) {
            (None, _) => (Some(true), Some(false), Some("ahead".into())),
            (Some(c), Some(tgt)) if c.as_str() == tgt.as_str() => {
                (Some(false), Some(false), Some("identical".into()))
            }
            (Some(c), Some(tgt)) if c.older_than(&tgt) => {
                (Some(true), Some(false), Some("ahead".into()))
            }
            (Some(c), Some(tgt)) if tgt.older_than(&c) => {
                (Some(false), Some(true), Some("behind".into()))
            }
            _ => {
                // Cross-mode or non-orderable: try git compare.
                match gh
                    .compare_deploy_to_ref(current.as_ref(), target_tag.as_str())
                    .await
                {
                    Ok(Some(f)) => (
                        Some(f.is_upgrade()),
                        Some(f.is_downgrade()),
                        Some(f.relation.as_str().to_string()),
                    ),
                    _ => (None, None, Some("unknown".into())),
                }
            }
        };
        let target_commit_sha = match manifest.commit_sha.clone() {
            some @ Some(_) => some,
            None => gh
                .resolve_commit(&rel.tag_name)
                .await
                .ok()
                .map(|info| info.sha),
        };
        let cached = LatestAvailable {
            version: target_tag,
            channel: manifest.channel.clone(),
            mode: UpdateMode::Release,
            source: Some("github".to_string()),
            seen_at: Utc::now(),
            commit_sha: target_commit_sha,
            current_commit_sha: current_state.and_then(|s| s.current_commit_sha),
            relation,
            ahead_by: None,
            behind_by: None,
            is_upgrade,
            is_downgrade,
            requires_self_update,
            min_updater_version: Some(manifest.updater.min_updater_version.clone()),
            notes_url: manifest.notes_url.clone(),
        };

        if persist_cache {
            let mut st = self.state.read_updater()?;
            st.last_checked_at = Some(Utc::now());
            st.latest_available = Some(cached);
            self.state.write_updater(&st)?;
        }
        Ok(Some(AvailableInfo::Release(manifest)))
    }

    async fn check_commit_available(
        self: Arc<Self>,
        channel: &str,
        persist_cache: bool,
    ) -> Result<Option<AvailableInfo>> {
        let branch = commit_branch_for_channel(channel);

        // Dev channel tip = newest Docker Hub common build by pushed_at among BOTH
        // dev-* commits and formal vX.Y.Z releases. Time wins; no prefer-dev filter.
        match self.clone().handle_list_builds(25).await {
            Ok(builds) if !builds.is_empty() => {
                return self
                    .finish_dev_channel_tip_from_builds(branch, builds, persist_cache)
                    .await;
            }
            Ok(_) => {
                info!(
                    %branch,
                    "commit check: Docker Hub has no common builds; falling back to GitHub branch tip"
                );
            }
            Err(docker_error) => {
                if !self.github_commit_metadata_enabled() {
                    return Err(UpdaterError::DockerHub(format!(
                        "Docker Hub commit discovery failed ({docker_error}); \
                         GITHUB_TOKEN not set for branch-tip fallback"
                    )));
                }
                warn!(
                    err = %docker_error,
                    %branch,
                    "commit check: Docker Hub list failed; falling back to GitHub branch tip"
                );
            }
        }

        // No usable Docker Hub tip — GitHub branch tip only (when token present).
        if !self.github_commit_metadata_enabled() {
            return Err(UpdaterError::DockerHub(
                "Docker Hub has no common immutable frontend/backend build and \
                 GITHUB_TOKEN is unset (cannot resolve branch tip)"
                    .into(),
            ));
        }
        self.check_github_branch_tip_available(branch, persist_cache)
            .await
    }

    /// Finish availability for a Docker Hub tip (commit or formal release).
    async fn finish_dev_channel_tip_from_builds(
        self: Arc<Self>,
        branch: &str,
        builds: Vec<DockerBuild>,
        persist_cache: bool,
    ) -> Result<Option<AvailableInfo>> {
        let state_now = self.state.read_updater()?;
        let current = state_now.current_version.as_ref().map(|c| c.as_str());
        let current_commit_sha = state_now.current_commit_sha.as_deref();

        // Skip tip when it is the same artifact as the running deploy (e.g. v0.3.21
        // vs dev-<same-sha>, or short vs full dev-sha). Otherwise a re-tagged sibling
        // becomes "tip" and either thrash-upgrades or blocks seeing a later build.
        let Some(build) = select_dev_channel_tip_for(&builds, current, current_commit_sha) else {
            if persist_cache {
                let mut state = state_now;
                state.last_checked_at = Some(Utc::now());
                state.latest_available = None;
                self.state.write_updater(&state)?;
            }
            // List was non-empty but every common build is the running identity.
            return Ok(None);
        };
        // Clone tip fields we need past the shared builds borrow.
        let tip_tag = build.tag.clone();
        let tip_kind = build.kind;
        let tip_pushed = build.pushed_at.clone();
        let tip_short_sha = build.short_sha.clone();
        let tip_backend_url = build.backend_url.clone();

        let tag = DeployTag::parse(&tip_tag)?;
        let current_pushed = current.and_then(|c| pushed_at_for_tag(&builds, c));

        // Cross-kind: push time / semver is primary (no ancestry). Same-kind commits may use git.
        let cross_kind = is_cross_kind_deploy(tag.as_str(), current);
        let freshness = if !cross_kind && !tag.is_release() && self.github_commit_metadata_enabled()
        {
            match self.github_client() {
                Ok(gh) => {
                    // Always pass a git-resolvable ref (strip dev- prefix); bare
                    // `dev-<sha>` 404s on the GitHub commits API and drops ancestry.
                    let target_ref = crate::release::deploy_tag_to_git_ref(&tag);
                    match gh
                        .compare_deploy_to_ref(state_now.current_version.as_ref(), &target_ref)
                        .await
                    {
                        Ok(f) => f,
                        Err(e) => {
                            warn!(err = %e, "commit freshness compare failed");
                            None
                        }
                    }
                }
                Err(e) => {
                    warn!(err = %e, "github client unavailable for ancestry");
                    None
                }
            }
        } else {
            None
        };

        let direction = commit_upgrade_direction_ex(
            tag.as_str(),
            current,
            tip_pushed.as_deref(),
            current_pushed,
            if cross_kind { None } else { freshness.as_ref() },
            current_commit_sha,
            Some(tip_short_sha.as_str()),
        );

        info!(
            target = %tag,
            kind = tip_kind,
            %branch,
            is_upgrade = direction.is_upgrade,
            is_downgrade = direction.is_downgrade,
            relation = direction.relation,
            target_pushed = ?tip_pushed,
            current_pushed = ?current_pushed,
            cross_kind,
            "dev-channel tip from Docker Hub (push-time; commits + formal releases)"
        );

        if !direction.is_upgrade && !direction.is_downgrade {
            if persist_cache {
                let mut state = state_now;
                state.last_checked_at = Some(Utc::now());
                state.latest_available = None;
                self.state.write_updater(&state)?;
            }
            return Ok(None);
        }

        // Formal release tip → release path (manifest / auto-install / preflight).
        if tag.is_release() {
            return self
                .finish_dev_channel_release_tip(
                    branch,
                    tag,
                    tip_backend_url.as_str(),
                    &direction,
                    state_now,
                    persist_cache,
                )
                .await;
        }

        // Commit tip: optional GitHub metadata enrichment.
        let mut full_sha = tip_short_sha.clone();
        let mut message = "Docker Hub common frontend/backend build".to_string();
        let mut notes_url = tip_backend_url.clone();
        let source = "dockerhub";
        if self.github_commit_metadata_enabled() {
            if let Ok(gh) = self.github_client() {
                if let Ok(info) = gh.resolve_commit(&tip_short_sha).await {
                    full_sha = info.sha;
                    message = info.message;
                    notes_url = info.html_url;
                }
            }
        }

        let cached = LatestAvailable {
            version: tag.clone(),
            channel: branch.to_string(),
            mode: UpdateMode::Commit,
            source: Some(source.to_string()),
            seen_at: Utc::now(),
            commit_sha: Some(full_sha.clone()),
            current_commit_sha: freshness
                .as_ref()
                .and_then(|f| f.current_sha.clone())
                .or_else(|| state_now.current_commit_sha.clone()),
            relation: Some(direction.relation.to_string()),
            ahead_by: freshness.as_ref().map(|f| f.ahead_by),
            behind_by: freshness.as_ref().map(|f| f.behind_by),
            is_upgrade: Some(direction.is_upgrade),
            is_downgrade: Some(direction.is_downgrade),
            requires_self_update: false,
            min_updater_version: None,
            notes_url: notes_url.clone(),
        };
        if persist_cache {
            let mut state = state_now;
            state.last_checked_at = Some(Utc::now());
            state.latest_available = Some(cached);
            self.state.write_updater(&state)?;
        }

        Ok(Some(AvailableInfo::Commit {
            tag,
            full_sha,
            message,
            branch: branch.to_string(),
            notes_url,
            source: source.to_string(),
            freshness,
            is_upgrade: Some(direction.is_upgrade),
            is_downgrade: Some(direction.is_downgrade),
            relation: Some(direction.relation.to_string()),
        }))
    }

    /// Formal release discovered as dev-channel tip: cache + AvailableInfo use release path.
    async fn finish_dev_channel_release_tip(
        self: Arc<Self>,
        branch: &str,
        tag: DeployTag,
        tip_backend_url: &str,
        direction: &crate::release::CommitUpgradeDirection,
        state_now: crate::state::UpdaterStateFile,
        persist_cache: bool,
    ) -> Result<Option<AvailableInfo>> {
        // Prefer full release.json when GitHub is reachable.
        if let Ok(gh) = self.github_client() {
            match gh.fetch_manifest(tag.as_str()).await {
                Ok(manifest) => {
                    let self_v = MyriadVersion::parse(crate::self_version()).ok();
                    let requires_self_update = manifest.updater.self_update_required
                        || self_v
                            .as_ref()
                            .is_some_and(|v| v.older_than(&manifest.updater.min_updater_version));
                    let target_commit_sha = match manifest.commit_sha.clone() {
                        some @ Some(_) => some,
                        None => gh
                            .resolve_commit(tag.as_str())
                            .await
                            .ok()
                            .map(|info| info.sha),
                    };
                    let cached = LatestAvailable {
                        version: tag,
                        channel: branch.to_string(),
                        mode: UpdateMode::Release,
                        source: Some("dockerhub".to_string()),
                        seen_at: Utc::now(),
                        commit_sha: target_commit_sha,
                        current_commit_sha: state_now.current_commit_sha.clone(),
                        relation: Some(direction.relation.to_string()),
                        ahead_by: None,
                        behind_by: None,
                        is_upgrade: Some(direction.is_upgrade),
                        is_downgrade: Some(direction.is_downgrade),
                        requires_self_update,
                        min_updater_version: Some(manifest.updater.min_updater_version.clone()),
                        notes_url: manifest.notes_url.clone(),
                    };
                    if persist_cache {
                        let mut state = state_now;
                        state.last_checked_at = Some(Utc::now());
                        state.latest_available = Some(cached);
                        self.state.write_updater(&state)?;
                    }
                    return Ok(Some(AvailableInfo::Release(manifest)));
                }
                Err(e) => {
                    warn!(
                        err = %e,
                        target = %tag,
                        "dev-channel release tip: fetch_manifest failed; caching release tag only"
                    );
                }
            }
        }

        // No manifest: still surface the formal release tip with release-mode cache so
        // status / auto-install use the release path (install re-resolves from tag).
        let cached = LatestAvailable {
            version: tag.clone(),
            channel: branch.to_string(),
            mode: UpdateMode::Release,
            source: Some("dockerhub".to_string()),
            seen_at: Utc::now(),
            commit_sha: None,
            current_commit_sha: state_now.current_commit_sha.clone(),
            relation: Some(direction.relation.to_string()),
            ahead_by: None,
            behind_by: None,
            is_upgrade: Some(direction.is_upgrade),
            is_downgrade: Some(direction.is_downgrade),
            requires_self_update: false,
            min_updater_version: None,
            notes_url: tip_backend_url.to_string(),
        };
        if persist_cache {
            let mut state = state_now;
            state.last_checked_at = Some(Utc::now());
            state.latest_available = Some(cached);
            self.state.write_updater(&state)?;
        }
        // Synthetic commit-shaped payload so /available still returns a tip without
        // release.json; mode field in cache remains Release for auto-install.
        Ok(Some(AvailableInfo::Commit {
            tag,
            full_sha: String::new(),
            message: "Docker Hub formal release build (dev-channel tip)".to_string(),
            branch: branch.to_string(),
            notes_url: tip_backend_url.to_string(),
            source: "dockerhub".to_string(),
            freshness: None,
            is_upgrade: Some(direction.is_upgrade),
            is_downgrade: Some(direction.is_downgrade),
            relation: Some(direction.relation.to_string()),
        }))
    }

    /// GitHub branch tip only — used when Docker Hub has no common builds.
    async fn check_github_branch_tip_available(
        self: Arc<Self>,
        branch: &str,
        persist_cache: bool,
    ) -> Result<Option<AvailableInfo>> {
        let gh = self.github_client()?;
        let info = match gh.latest_commit_on_branch(branch).await {
            Ok(i) => i,
            Err(e) => {
                if GithubClient::is_expected_unauthenticated_failure(&e) {
                    info!(
                        err = %e,
                        %branch,
                        "commit lookup: GitHub access denied/not found; no Docker Hub tip either"
                    );
                } else {
                    warn!(err = %e, %branch, "commit lookup failed");
                }
                return Err(e);
            }
        };
        let tag = DeployTag::parse(&format!("dev-{}", info.short_sha))?;
        let notes_url = info.html_url.clone();

        let st_now = self.state.read_updater()?;
        let freshness = match gh
            .compare_deploy_to_ref(st_now.current_version.as_ref(), branch)
            .await
        {
            Ok(f) => f,
            Err(e) => {
                warn!(err = %e, "commit freshness compare failed");
                None
            }
        };
        if let Some(ref f) = freshness {
            info!(
                relation = f.relation.as_str(),
                ahead = f.ahead_by,
                behind = f.behind_by,
                current = ?f.current_sha,
                target = ?f.target_sha,
                "commit freshness vs branch tip (no Docker Hub builds)"
            );
        }

        let direction = commit_upgrade_direction_ex(
            tag.as_str(),
            st_now.current_version.as_ref().map(|c| c.as_str()),
            None,
            None,
            freshness.as_ref(),
            st_now.current_commit_sha.as_deref(),
            Some(info.sha.as_str()),
        );
        info!(
            target = %tag,
            is_upgrade = direction.is_upgrade,
            is_downgrade = direction.is_downgrade,
            relation = direction.relation,
            "commit check: GitHub branch tip only (no Docker Hub common builds)"
        );

        if !direction.is_upgrade && !direction.is_downgrade {
            if persist_cache {
                let mut st = self.state.read_updater()?;
                st.last_checked_at = Some(Utc::now());
                st.latest_available = None;
                self.state.write_updater(&st)?;
            }
            return Ok(None);
        }

        let cached = LatestAvailable {
            version: tag.clone(),
            channel: branch.to_string(),
            mode: UpdateMode::Commit,
            source: Some("github".to_string()),
            seen_at: Utc::now(),
            commit_sha: Some(info.sha.clone()),
            current_commit_sha: freshness
                .as_ref()
                .and_then(|f| f.current_sha.clone())
                .or_else(|| st_now.current_commit_sha.clone()),
            relation: Some(direction.relation.to_string()),
            ahead_by: freshness.as_ref().map(|f| f.ahead_by),
            behind_by: freshness.as_ref().map(|f| f.behind_by),
            is_upgrade: Some(direction.is_upgrade),
            is_downgrade: Some(direction.is_downgrade),
            requires_self_update: false,
            min_updater_version: None,
            notes_url: notes_url.clone(),
        };
        if persist_cache {
            let mut st = self.state.read_updater()?;
            st.last_checked_at = Some(Utc::now());
            st.latest_available = Some(cached);
            self.state.write_updater(&st)?;
        }
        Ok(Some(AvailableInfo::Commit {
            tag,
            full_sha: info.sha,
            message: info.message,
            branch: branch.to_string(),
            notes_url,
            source: "github".to_string(),
            freshness,
            is_upgrade: Some(direction.is_upgrade),
            is_downgrade: Some(direction.is_downgrade),
            relation: Some(direction.relation.to_string()),
        }))
    }
}

fn resolve_check_request(
    saved_channel: &str,
    saved_mode: UpdateMode,
    channel_override: Option<String>,
    mode_override: Option<UpdateMode>,
) -> (String, UpdateMode, bool) {
    let channel = channel_override
        .map(|channel| channel.trim().to_ascii_lowercase())
        .filter(|channel| !channel.is_empty())
        .unwrap_or_else(|| saved_channel.to_string());
    let mode = mode_override.unwrap_or(saved_mode);
    let persist_cache = channel == saved_channel && mode == saved_mode;
    (channel, mode, persist_cache)
}

fn runtime_identity_from_json(body: &serde_json::Value) -> Option<(DeployTag, Option<String>)> {
    let version = DeployTag::parse(body.get("version")?.as_str()?).ok()?;
    let commit_sha = body
        .get("commit_sha")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|sha| sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(|sha| sha.to_ascii_lowercase());
    Some((version, commit_sha))
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

/// Pure auto-install gate used by the worker (and unit tests).
///
/// - **Release**: only clear, low-risk upgrades (`is_upgrade`, relation not
///   unknown/diverged/behind/identical).
/// - **Commit/dev**: `is_upgrade` is enough; `relation=unknown` is allowed
///   (build publish time / different tip). Still blocks behind/diverged/identical.
pub fn auto_install_latest_ok(
    mode: UpdateMode,
    is_upgrade: Option<bool>,
    is_downgrade: Option<bool>,
    relation: Option<&str>,
    requires_self_update: bool,
    irreversible: bool,
) -> bool {
    if irreversible {
        return false;
    }
    if requires_self_update {
        return false;
    }
    if is_upgrade != Some(true) || is_downgrade == Some(true) {
        return false;
    }
    match mode {
        UpdateMode::Release => !matches!(
            relation,
            Some("diverged") | Some("unknown") | Some("behind") | Some("identical")
        ),
        UpdateMode::Commit => {
            // unknown is explicitly allowed for commit/dev (Docker Hub / no ancestry).
            !matches!(
                relation,
                Some("diverged") | Some("behind") | Some("identical")
            )
        }
    }
}

#[cfg(test)]
mod check_request_tests {
    use super::*;

    #[test]
    fn matching_explicit_values_refresh_the_canonical_cache() {
        let (channel, mode, persist_cache) = resolve_check_request(
            "preview",
            UpdateMode::Commit,
            Some("preview".to_string()),
            Some(UpdateMode::Commit),
        );

        assert_eq!(channel, "preview");
        assert_eq!(mode, UpdateMode::Commit);
        assert!(persist_cache);
    }

    #[test]
    fn genuine_override_does_not_replace_saved_availability() {
        let (_, _, persist_cache) = resolve_check_request(
            "stable",
            UpdateMode::Release,
            Some("preview".to_string()),
            Some(UpdateMode::Commit),
        );

        assert!(!persist_cache);
    }

    #[test]
    fn omitted_values_use_and_refresh_saved_preferences() {
        let (channel, mode, persist_cache) =
            resolve_check_request("stable", UpdateMode::Release, None, None);

        assert_eq!(channel, "stable");
        assert_eq!(mode, UpdateMode::Release);
        assert!(persist_cache);
    }
}

#[cfg(test)]
mod auto_install_gate_tests {
    use super::*;

    #[test]
    fn commit_mode_allows_upgrade_with_unknown_relation() {
        assert!(auto_install_latest_ok(
            UpdateMode::Commit,
            Some(true),
            Some(false),
            Some("unknown"),
            false,
            false,
        ));
    }

    #[test]
    fn commit_mode_allows_upgrade_with_ahead_from_push_time() {
        assert!(auto_install_latest_ok(
            UpdateMode::Commit,
            Some(true),
            Some(false),
            Some("ahead"),
            false,
            false,
        ));
    }

    #[test]
    fn commit_mode_blocks_downgrade_and_diverged() {
        assert!(!auto_install_latest_ok(
            UpdateMode::Commit,
            Some(false),
            Some(true),
            Some("behind"),
            false,
            false,
        ));
        assert!(!auto_install_latest_ok(
            UpdateMode::Commit,
            Some(true),
            Some(false),
            Some("diverged"),
            false,
            false,
        ));
    }

    #[test]
    fn release_mode_still_blocks_unknown() {
        assert!(!auto_install_latest_ok(
            UpdateMode::Release,
            Some(true),
            Some(false),
            Some("unknown"),
            false,
            false,
        ));
        assert!(auto_install_latest_ok(
            UpdateMode::Release,
            Some(true),
            Some(false),
            Some("ahead"),
            false,
            false,
        ));
    }
}

/// Strip a trailing image tag from a repository reference.
///
/// - `docker.io/org/name:v0.3.6` → `docker.io/org/name`
/// - `localhost:5000/org/name:dev-abc` → `localhost:5000/org/name`
/// - `localhost:5000/org/name` → unchanged (host:port kept)
/// - `registry:5000/ns/img` → unchanged (colon is host:port, after has `/`)
fn strip_image_repo_tag(raw: &str) -> String {
    let s = raw.trim();
    let Some((before, after)) = s.rsplit_once(':') else {
        return s.to_string();
    };
    // Tag form: path present before last `:`, and after has no `/` (not host:port/path).
    if before.contains('/') && !after.is_empty() && !after.contains('/') {
        return before.to_string();
    }
    s.to_string()
}

fn rewrite_with_mirror(image_ref: &str, mirror: &str) -> String {
    // image_ref looks like "docker.io/foo/bar:v1". Replace the registry host with `mirror`.
    let mirror = mirror.trim_end_matches('/');
    match image_ref.split_once('/') {
        Some((_host, rest)) => format!("{mirror}/{rest}"),
        None => format!("{mirror}/{image_ref}"),
    }
}

/// Helper used by the maintenance/state-machine layer to update maintenance.json.
#[allow(dead_code)]
pub(crate) fn set_phase(
    state: &StateDir,
    job_id: &str,
    from: Option<&DeployTag>,
    to: Option<&DeployTag>,
    phase: Phase,
    message_key: &str,
) -> Result<()> {
    let m = MaintenanceFile {
        schema_version: 1,
        active: phase.takes_site_offline(),
        phase,
        from_version: from.cloned(),
        to_version: to.cloned(),
        started_at: Some(Utc::now()),
        updated_at: Utc::now(),
        job_id: Some(job_id.to_string()),
        message_key: message_key.to_string(),
    };
    state.write_maintenance(&m)
}

#[cfg(test)]
mod image_repo_tests {
    use super::*;

    #[test]
    fn strip_tag_from_full_repo() {
        assert_eq!(
            strip_image_repo_tag("docker.io/org/name:v0.3.6"),
            "docker.io/org/name"
        );
        assert_eq!(
            strip_image_repo_tag("localhost:5000/org/name:dev-abc"),
            "localhost:5000/org/name"
        );
    }

    #[test]
    fn preserves_host_port_without_tag() {
        assert_eq!(
            strip_image_repo_tag("localhost:5000/org/name"),
            "localhost:5000/org/name"
        );
        assert_eq!(
            strip_image_repo_tag("registry:5000/ns/img"),
            "registry:5000/ns/img"
        );
    }

    #[test]
    fn leaves_unpathed_values() {
        assert_eq!(strip_image_repo_tag("v0.3.6"), "v0.3.6");
        assert_eq!(strip_image_repo_tag("myriad-backend"), "myriad-backend");
    }
}

#[cfg(test)]
mod runtime_identity_tests {
    use super::*;

    #[test]
    fn parses_release_with_embedded_commit() {
        let body = serde_json::json!({
            "version": "v0.1.0",
            "commit_sha": "ABCDEF0123456789ABCDEF0123456789ABCDEF01"
        });
        let (version, sha) = runtime_identity_from_json(&body).expect("runtime identity");
        assert_eq!(version.as_str(), "v0.1.0");
        assert_eq!(
            sha.as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef01")
        );
    }

    #[test]
    fn keeps_version_when_old_health_has_no_commit() {
        let body = serde_json::json!({ "version": "dev-103a5ec" });
        let (version, sha) = runtime_identity_from_json(&body).expect("runtime identity");
        assert_eq!(version.as_str(), "dev-103a5ec");
        assert_eq!(sha, None);
    }

    #[test]
    fn ignores_invalid_commit_sha() {
        let body = serde_json::json!({
            "version": "v0.1.0",
            "commit_sha": "not-a-commit"
        });
        let (_, sha) = runtime_identity_from_json(&body).expect("runtime identity");
        assert_eq!(sha, None);
    }
}

#[cfg(test)]
mod recovery_plan_tests {
    use super::*;
    use crate::state::JobStep;

    fn job(status: JobStatus, phase: Phase) -> Job {
        Job {
            id: "j1".into(),
            kind: JobKind::Update,
            created_at: Utc::now(),
            finished_at: None,
            from_version: None,
            to_version: None,
            snapshot_id: Some("snap-j1".into()),
            status,
            steps: vec![JobStep::start(phase)],
            idempotency_key: None,
        }
    }

    fn maint(active: bool, phase: Phase, job_id: Option<&str>) -> MaintenanceFile {
        MaintenanceFile {
            schema_version: 1,
            active,
            phase,
            from_version: None,
            to_version: None,
            started_at: Some(Utc::now()),
            updated_at: Utc::now(),
            job_id: job_id.map(|s| s.to_string()),
            message_key: "test".into(),
        }
    }

    #[test]
    fn idle_when_nothing_in_flight() {
        let m = MaintenanceFile::inactive();
        assert_eq!(
            plan_crash_recovery(&m, None, None, None),
            CrashRecoveryPlan::Idle
        );
    }

    #[test]
    fn health_probe_lifted_maintenance_is_needs_manual_not_idle() {
        // Phase 2 frontend probe sets active=false while job still running post-swap.
        let m = maint(false, Phase::HealthProbing, Some("j1"));
        let j = job(JobStatus::Running, Phase::HealthProbing);
        match plan_crash_recovery(&m, Some("j1"), Some(&j), None) {
            CrashRecoveryPlan::NeedsManual { phase, .. } => {
                assert_eq!(phase, Phase::HealthProbing);
            }
            other => panic!("expected NeedsManual, got {other:?}"),
        }
    }

    #[test]
    fn job_current_alone_with_post_swap_step_is_needs_manual() {
        let m = MaintenanceFile::inactive();
        let j = job(JobStatus::Running, Phase::StartingNew);
        match plan_crash_recovery(&m, Some("j1"), Some(&j), None) {
            CrashRecoveryPlan::NeedsManual { phase, .. } => {
                assert_eq!(phase, Phase::StartingNew);
            }
            other => panic!("expected NeedsManual, got {other:?}"),
        }
    }

    #[test]
    fn pre_swap_stopping_clears_for_stack_restore() {
        let m = maint(true, Phase::Stopping, Some("j1"));
        let j = job(JobStatus::Running, Phase::Stopping);
        match plan_crash_recovery(&m, Some("j1"), Some(&j), None) {
            CrashRecoveryPlan::ClearPreSwap { phase, .. } => {
                assert_eq!(phase, Phase::Stopping);
            }
            other => panic!("expected ClearPreSwap, got {other:?}"),
        }
    }

    #[test]
    fn swap_tag_pre_write_clears_for_stack_restore() {
        let m = maint(true, Phase::SwapTag, Some("j1"));
        let mut j = job(JobStatus::Running, Phase::SwapTag);
        j.to_version = Some(DeployTag::parse("v0.2.3").unwrap());
        j.from_version = Some(DeployTag::parse("v0.2.2").unwrap());
        // .env still on old tag → pre-write crash
        match plan_crash_recovery(&m, Some("j1"), Some(&j), Some("v0.2.2")) {
            CrashRecoveryPlan::ClearPreSwap { phase, .. } => {
                assert_eq!(phase, Phase::SwapTag);
            }
            other => panic!("expected ClearPreSwap for pre-write SwapTag, got {other:?}"),
        }
    }

    #[test]
    fn swap_tag_post_write_is_needs_manual() {
        let m = maint(true, Phase::SwapTag, Some("j1"));
        let mut j = job(JobStatus::Running, Phase::SwapTag);
        j.to_version = Some(DeployTag::parse("v0.2.3").unwrap());
        match plan_crash_recovery(&m, Some("j1"), Some(&j), Some("v0.2.3")) {
            CrashRecoveryPlan::NeedsManual { phase, .. } => {
                assert_eq!(phase, Phase::SwapTag);
            }
            other => panic!("expected NeedsManual for post-write SwapTag, got {other:?}"),
        }
    }

    #[test]
    fn active_rollback_phase_is_needs_manual() {
        let m = maint(true, Phase::RestoreSnapshot, Some("j1"));
        let j = job(JobStatus::Running, Phase::RestoreSnapshot);
        assert!(matches!(
            plan_crash_recovery(&m, Some("j1"), Some(&j), None),
            CrashRecoveryPlan::NeedsManual { .. }
        ));
    }

    #[test]
    fn orphan_active_maintenance_clears() {
        let m = maint(true, Phase::MaintenanceOn, None);
        assert_eq!(
            plan_crash_recovery(&m, None, None, None),
            CrashRecoveryPlan::ClearOrphanMaintenance
        );
    }

    /// End-to-end recovery against a real state dir (no docker).
    #[tokio::test]
    async fn recover_or_idle_health_probe_lifted_writes_needs_manual() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(StateDir::open(dir.path()).unwrap());

        let job = job(JobStatus::Running, Phase::HealthProbing);
        state.write_job(&job).unwrap();
        state.set_current_job(Some("j1")).unwrap();
        state
            .write_maintenance(&maint(false, Phase::HealthProbing, Some("j1")))
            .unwrap();

        let report = Worker::recover_or_idle_state(state.clone(), None)
            .await
            .expect("recover");
        match report {
            RecoveryReport::NeedsManual { job_id, phase, .. } => {
                assert_eq!(job_id, "j1");
                assert_eq!(phase, Phase::HealthProbing);
            }
            other => panic!("expected NeedsManual, got {other:?}"),
        }
        let m = state.read_maintenance().unwrap();
        assert!(m.active);
        assert_eq!(m.phase, Phase::NeedsManual);
        let j = state.read_job("j1").unwrap();
        assert_eq!(j.status, JobStatus::NeedsManual);
    }

    #[tokio::test]
    async fn recover_or_idle_pre_swap_stopping_clears() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(StateDir::open(dir.path()).unwrap());
        let job = job(JobStatus::Running, Phase::Stopping);
        state.write_job(&job).unwrap();
        state.set_current_job(Some("j1")).unwrap();
        state
            .write_maintenance(&maint(true, Phase::Stopping, Some("j1")))
            .unwrap();

        let report = Worker::recover_or_idle_state(state.clone(), None)
            .await
            .expect("recover");
        assert!(matches!(report, RecoveryReport::ClearedPreSwap));
        let m = state.read_maintenance().unwrap();
        assert!(!m.active);
        let j = state.read_job("j1").unwrap();
        assert_eq!(j.status, JobStatus::Failed);
        assert!(state.read_current_job().unwrap().is_none());
    }

    #[tokio::test]
    async fn corrupt_maintenance_with_job_current_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        state.set_current_job(Some("j1")).unwrap();
        std::fs::write(dir.path().join("maintenance.json"), b"{not-json").unwrap();
        let m = state.read_maintenance().unwrap();
        assert!(m.active);
        assert_eq!(m.phase, Phase::NeedsManual);
        assert_eq!(m.job_id.as_deref(), Some("j1"));
    }
}
