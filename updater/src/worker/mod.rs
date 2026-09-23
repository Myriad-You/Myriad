//! Update worker: serializes update/rollback jobs through a single-slot state machine.
//!
//! Concurrency model:
//! - The HTTP API never executes long-running work directly.
//! - It enqueues commands into a bounded MPSC channel consumed by a single worker task.
//! - The state machine ensures at most one job is in flight; further `update` requests get 409.

pub mod backend_health;
pub mod machine;
pub mod preflight;
pub mod preflight_env;
pub mod rollback;
pub mod self_update;
pub mod update;

pub mod check;
pub mod commands;
pub mod prefs;
pub mod recovery;

pub use check::auto_install_latest_ok;
pub use prefs::{
    CHECK_INTERVAL_PRESETS, Prefs, SnapshotListDiagnostics, validate_check_interval_secs,
    validate_snapshot_limit,
};
pub use recovery::{
    CrashRecoveryPlan, RecoveryReport, commit_pre_swap_stack_restored, plan_crash_recovery,
    record_recovery_failure,
};

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::docker::DockerClient;
use crate::error::{Result, UpdaterError};
use crate::release::{DockerHubClient, GithubClient};
use crate::state::{Job, JobKind, JobStatus, Phase, StateDir};
use crate::version::{DeployTag, DeployTagKind, MyriadVersion, UpdateMode};

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
        allow_compose_override: Option<bool>,
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
        reply: tokio::sync::oneshot::Sender<Result<Vec<crate::release::DockerBuild>>>,
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
    /// Clear the persisted last-failed banner after the operator acknowledges it.
    DismissLastFailed {
        reply: tokio::sync::oneshot::Sender<Result<()>>,
    },
    /// Clear durable TCB self-update last-outcome (`self-update-last.json`).
    DismissSelfUpdateLast {
        reply: tokio::sync::oneshot::Sender<Result<()>>,
    },
    SelfUpdate {
        actor: Option<String>,
        reply: tokio::sync::oneshot::Sender<Result<self_update::SelfUpdateReport>>,
    },
    Shutdown,
}

/// Unified "what's available" for both release and commit modes.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum AvailableInfo {
    Release(crate::release::Manifest),
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

pub struct Worker {
    state: Arc<StateDir>,
    docker: Arc<DockerClient>,
    config: Config,
    cli: WorkerCli,
    tx: mpsc::Sender<Command>,
    rx: Mutex<Option<mpsc::Receiver<Command>>>,
    component_task: std::sync::Mutex<Option<JoinHandle<()>>>,
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
            component_task: std::sync::Mutex::new(None),
        }
    }

    /// Block business updates while the stack is in a stuck rescue state.
    ///
    /// Maintenance remains authoritative even when no current job is recorded.
    pub(crate) fn refuse_update_if_stuck(&self) -> Result<()> {
        refuse_update_if_stuck_in(&self.state)
    }

    fn require_no_component_update(&self) -> Result<()> {
        if self
            .component_task
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            return Err(UpdaterError::Conflict);
        }
        // `component_task` is the only executor of a component update in this
        // process; a self-update is handed to Guard, which keeps its own gate and
        // refuses Docker mutations while it runs. With no task here, an outcome
        // record we cannot parse describes history, not a running task: converge
        // it so a corrupt file cannot refuse every mutation — including the
        // dismiss that would clear it — for the lifetime of the system. A readable
        // `Pending` record still refuses below.
        self_update::converge_unreadable_outcome(&self.state)?;
        self_update::require_no_pending_handoff(&self.state)
    }

    fn require_idle_mutation(&self) -> Result<()> {
        self.require_no_component_update()?;
        if self.state.read_current_job()?.is_some() {
            return Err(UpdaterError::Conflict);
        }
        self.refuse_update_if_stuck()
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

    async fn component_target(&self, repo: &str, requested: Option<String>) -> Result<String> {
        if let Some(tag) = requested {
            crate::version::validate_image_tag(&tag).map_err(UpdaterError::InvalidInput)?;
            return Ok(tag);
        }
        let tags = self
            .dockerhub_client()?
            .list_immutable_tags(repo, 25)
            .await?;
        crate::release::select_component_tip(&tags, self.effective_mode()? == UpdateMode::Release)
            .map(|target| target.tag.clone())
            .ok_or_else(|| {
                UpdaterError::Precondition(format!("No component image available in {repo}"))
            })
    }

    /// Image repositories are explicit deployment inputs. Shared by commit-mode preflight,
    /// release-mode Docker Hub fallback (when `release.json` is unavailable), and Hub discovery.
    pub fn image_repos_required(&self) -> Result<(String, String)> {
        let env = crate::env_file::EnvFile::load(&self.cli.env_file)?;
        let backend = env.get("BACKEND_IMAGE").map(str::to_owned).ok_or_else(|| {
            UpdaterError::Precondition(
                "BACKEND_IMAGE missing in .env; required for commit-mode and release Docker Hub \
                     image pulls. Add e.g. BACKEND_IMAGE=docker.io/<org>/myriad-backend (no tag) \
                     or re-run scripts/extra/deploy.sh to bootstrap defaults."
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
                     (no tag) or re-run scripts/extra/deploy.sh to bootstrap defaults."
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

    /// After crash recovery classifies a pre-swap job, restart app services that may
    /// still be stopped (frontend/backend, and postgres if bundled). Does not
    /// rewrite `MYRIAD_TAG` or restore snapshots. Clears job.current / maintenance
    /// only after restore succeeds.
    pub async fn restore_stack_after_pre_swap(self: &Arc<Self>) -> Result<()> {
        let job_id = self.state.read_current_job()?;
        if let Some(id) = &job_id {
            update::restore_compose(&self.state, id)?;
        }
        let compose = update::build_compose_runner_pub(self).await?;
        update::restore_previous_stack(self, &compose, update::PreSwapRestoreScope::AppAndPostgres)
            .await?;
        commit_pre_swap_stack_restored(&self.state)
    }

    /// Keep the official reference unchanged. Docker's registry mirrors are transparent.
    pub async fn pull_image(&self, image_ref: &str) -> Result<String> {
        let digest = self.docker.pull(image_ref, None).await?;
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
        // An in-flight update owns its version; probing partially started services
        // must not publish a new current_version before that update commits.
        if self.state.read_current_job()?.is_some() {
            return Ok(());
        }
        let mut st = self.state.read_updater()?;
        let updater_identity_changed = self.heal_running_updater_identity(&mut st);
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
            if updater_identity_changed {
                self.state.write_updater(&st)?;
            }
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

        let state_changed = version_changed
            || updater_identity_changed
            || st.current_commit_sha != commit_sha
            || st.current_version.is_none();
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

    /// Persist the running binary version. Identity is the image stamp, not
    /// `UPDATER_TAG` — that key selects the deployment target, not runtime identity.
    fn heal_running_updater_identity(&self, st: &mut crate::state::UpdaterStateFile) -> bool {
        let running = MyriadVersion::parse(crate::self_version()).ok();
        if st.updater_version == running {
            return false;
        }
        st.updater_version = running;
        true
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
        self_update::resume_pending(self.clone());

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
                    allow_compose_override,
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
                            allow_compose_override,
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
                    let worker = self.clone();
                    tokio::spawn(async move {
                        let _ = reply.send(worker.handle_list_commits(branch, limit).await);
                    });
                }
                Command::ListBuilds { limit, reply } => {
                    let worker = self.clone();
                    tokio::spawn(async move {
                        let _ = reply.send(worker.handle_list_builds(limit).await);
                    });
                }
                Command::ListReleases {
                    channel,
                    limit,
                    reply,
                } => {
                    let worker = self.clone();
                    tokio::spawn(async move {
                        let _ = reply.send(worker.handle_list_releases(channel, limit).await);
                    });
                }
                Command::Compare { from, to, reply } => {
                    let worker = self.clone();
                    tokio::spawn(async move {
                        let _ = reply.send(worker.handle_compare(from, to).await);
                    });
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
                Command::DismissLastFailed { reply } => {
                    let res = self.clone().handle_dismiss_last_failed();
                    let _ = reply.send(res);
                }
                Command::DismissSelfUpdateLast { reply } => {
                    let res = self.clone().handle_dismiss_state_file(
                        "self-update-last.json",
                        "audit: self_update_last_dismissed",
                    );
                    let _ = reply.send(res);
                }
                Command::SelfUpdate { actor, reply } => {
                    let res = match self.require_idle_mutation() {
                        Ok(()) => self_update::schedule(self.clone(), actor),
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(res);
                }
            }
        }
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
        allow_compose_override: Option<bool>,
        idempotency_key: Option<String>,
        actor: Option<String>,
    ) -> Result<String> {
        let fingerprint = update_request_fingerprint(
            &target,
            mode,
            allow_downgrade,
            allow_risk,
            allow_diverged,
            allow_unknown,
            allow_irreversible,
            allow_compose_override,
        );
        // A keyed job the executor system already accepted replays its id. A keyed
        // job whose admission never committed (Pending, never started, not
        // `job.current`) finishes the same admission below instead of being
        // reported as accepted while nothing runs it.
        let unadmitted = match &idempotency_key {
            Some(k) => match replay_idempotent_update(&self.state, k, &fingerprint)? {
                Some(IdempotentReplay::Accepted(jid)) => return Ok(jid),
                Some(IdempotentReplay::Unadmitted(job)) => Some(job),
                None => None,
            },
            None => None,
        };

        self.require_no_component_update()?;
        if let Some(_existing) = self.state.read_current_job()? {
            return Err(UpdaterError::Conflict);
        }
        // NeedsManual / sticky post-swap maintenance must not start another business update.
        // Rollback and rescue APIs remain available.
        self.refuse_update_if_stuck()?;

        let from_version = self.state.read_updater()?.current_version;
        let job = match unadmitted {
            Some(mut job) => {
                job.from_version = from_version;
                job
            }
            None => Job {
                id: uuid::Uuid::new_v4().simple().to_string(),
                kind: JobKind::Update,
                created_at: Utc::now(),
                finished_at: None,
                from_version,
                to_version: Some(target.clone()),
                snapshot_id: None,
                status: JobStatus::Pending,
                steps: Vec::new(),
                idempotency_key: idempotency_key.clone(),
                idempotency_fingerprint: idempotency_key.as_ref().map(|_| fingerprint),
            },
        };
        let job_id = job.id.clone();
        // Admission commit point: `job.current` naming this job. The job file is
        // written first so the pointer never names a missing job; a failure or
        // exit between the two leaves an unadmitted Pending job that a replay of
        // the same key completes (see `replay_idempotent_update`).
        self.state.write_job(&job)?;
        self.state.set_current_job(Some(&job_id))?;

        let job_id_clone = job_id.clone();
        let me = self.clone();
        let risk = preflight::RiskFlags::from_api(
            allow_downgrade,
            allow_risk,
            allow_diverged,
            allow_unknown,
            allow_irreversible,
            allow_compose_override,
        );
        tokio::spawn(async move {
            if let Err(e) =
                update::run(me.clone(), job_id_clone.clone(), target, mode, risk, actor).await
            {
                error!(job = %job_id_clone, err = %e, "update flow exited with error");
            }
        });

        Ok(job_id)
    }

    async fn handle_rollback(
        self: Arc<Self>,
        snapshot_id: String,
        actor: Option<String>,
    ) -> Result<String> {
        self.require_no_component_update()?;
        if let Some(existing) = self.state.read_current_job()?
            && self.state.read_job(&existing)?.status != JobStatus::NeedsManual
        {
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
            idempotency_fingerprint: None,
        };
        self.state.write_job(&job)?;
        self.state.set_current_job(Some(&job_id))?;

        let me = self.clone();
        let id = job_id.clone();
        tokio::spawn(async move {
            if let Err(e) = rollback::run(me.clone(), id.clone(), snapshot_id, actor).await {
                error!(job = %id, err = %e, "rollback flow exited with error");
            }
        });
        Ok(job_id)
    }
}

/// I/O and parse errors are errors. Missing files are already `Ok(inactive)` / `Ok(None)`.
pub(crate) fn refuse_update_if_stuck_in(state: &StateDir) -> Result<()> {
    let maint = state.read_maintenance()?;
    if maint.active || matches!(maint.phase, Phase::NeedsManual) {
        return Err(UpdaterError::Conflict);
    }
    match state.read_current_job()? {
        None => Ok(()),
        Some(id) => match state.read_job(&id) {
            Ok(job) if matches!(job.status, JobStatus::NeedsManual) => Err(UpdaterError::Conflict),
            Ok(_) => Ok(()),
            Err(UpdaterError::NotFound(_)) => Err(UpdaterError::Conflict),
            Err(e) => Err(e),
        },
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update_request_fingerprint(
    target: &DeployTag,
    mode: UpdateMode,
    allow_downgrade: bool,
    allow_risk: bool,
    allow_diverged: Option<bool>,
    allow_unknown: Option<bool>,
    allow_irreversible: Option<bool>,
    allow_compose_override: Option<bool>,
) -> String {
    fn flag(v: Option<bool>) -> &'static str {
        match v {
            Some(true) => "true",
            Some(false) => "false",
            None => "unset",
        }
    }
    format!(
        "update|{}|{}|downgrade={}|risk={}|diverged={}|unknown={}|irreversible={}|compose_override={}",
        target.as_str(),
        mode.as_str(),
        allow_downgrade,
        allow_risk,
        flag(allow_diverged),
        flag(allow_unknown),
        flag(allow_irreversible),
        flag(allow_compose_override),
    )
}

/// Durable Idempotency-Key lookup result. Job files are the only authority.
#[derive(Debug)]
pub(crate) enum IdempotentReplay {
    /// Admitted (`job.current`), started, or terminal: replay the id, never rerun.
    Accepted(String),
    /// Written but never admitted: Pending, no step, not `job.current`.
    Unadmitted(Job),
}

/// A job the executor never touched: `PhaseRecorder` flips Pending to Running
/// together with the first step, so Pending with no step means no side effect.
pub(crate) fn job_never_started(job: &Job) -> bool {
    job.status == JobStatus::Pending && job.steps.is_empty()
}

pub(crate) fn replay_idempotent_update(
    state: &StateDir,
    key: &str,
    fingerprint: &str,
) -> Result<Option<IdempotentReplay>> {
    let mut found: Option<Job> = None;
    for id in state.list_jobs()? {
        let job = match state.read_job(&id) {
            Ok(job) => job,
            Err(UpdaterError::NotFound(_)) => continue,
            Err(e) => return Err(e),
        };
        if job.idempotency_key.as_deref() != Some(key) {
            continue;
        }
        if let Some(prev) = found.replace(job) {
            return Err(UpdaterError::State(format!(
                "duplicate Idempotency-Key on jobs {} and {id}",
                prev.id
            )));
        }
    }
    match found {
        None => Ok(None),
        Some(job) => match job.idempotency_fingerprint.as_deref() {
            Some(fp) if fp == fingerprint => {
                let admitted = state.read_current_job()?.as_deref() == Some(job.id.as_str());
                if job_never_started(&job) && !admitted {
                    Ok(Some(IdempotentReplay::Unadmitted(job)))
                } else {
                    Ok(Some(IdempotentReplay::Accepted(job.id)))
                }
            }
            Some(_) | None => Err(UpdaterError::InvalidInput(
                "Idempotency-Key was already used for a different update request".into(),
            )),
        },
    }
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
mod stuck_and_idempotency_tests {
    use super::*;
    use crate::state::{Job, JobKind, JobStatus, MaintenanceFile, Phase};
    use chrono::Utc;

    fn sample_job(id: &str, key: Option<&str>, fingerprint: Option<&str>) -> Job {
        Job {
            id: id.into(),
            kind: JobKind::Update,
            created_at: Utc::now(),
            finished_at: None,
            from_version: None,
            to_version: Some(DeployTag::parse("v1.2.3").unwrap()),
            snapshot_id: None,
            status: JobStatus::Succeeded,
            steps: Vec::new(),
            idempotency_key: key.map(str::to_string),
            idempotency_fingerprint: fingerprint.map(str::to_string),
        }
    }

    fn accepted(replay: Option<IdempotentReplay>) -> Option<String> {
        match replay {
            Some(IdempotentReplay::Accepted(id)) => Some(id),
            Some(IdempotentReplay::Unadmitted(job)) => panic!("unexpected unadmitted {}", job.id),
            None => None,
        }
    }

    fn pending_keyed(id: &str) -> Job {
        let mut job = sample_job(id, Some("k"), Some("fp"));
        job.status = JobStatus::Pending;
        job
    }

    /// `write_job` succeeded, `set_current_job` failed (or the process exited
    /// between them): the replay must resume admission, not report success.
    #[test]
    fn unadmitted_pending_job_is_resumed_not_replayed() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        state.write_job(&pending_keyed("orphan")).unwrap();
        assert!(matches!(
            replay_idempotent_update(&state, "k", "fp").unwrap(),
            Some(IdempotentReplay::Unadmitted(job)) if job.id == "orphan"
        ));
        // Same answer from a fresh process: job files are the only authority.
        let restarted = StateDir::open_readonly(dir.path()).unwrap();
        assert!(matches!(
            replay_idempotent_update(&restarted, "k", "fp").unwrap(),
            Some(IdempotentReplay::Unadmitted(_))
        ));
        // Admission resumed with different parameters is still refused.
        assert!(matches!(
            replay_idempotent_update(&state, "k", "other"),
            Err(UpdaterError::InvalidInput(_))
        ));
    }

    /// Once admitted or started, a replay returns the id and never reruns.
    #[test]
    fn admitted_or_started_job_replays_id_only() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        state.write_job(&pending_keyed("admitted")).unwrap();
        state.set_current_job(Some("admitted")).unwrap();
        assert_eq!(
            accepted(replay_idempotent_update(&state, "k", "fp").unwrap()).as_deref(),
            Some("admitted")
        );

        let mut running = pending_keyed("admitted");
        running.status = JobStatus::Running;
        running.steps.push(crate::state::JobStep::start(Phase::Preflight));
        state.write_job(&running).unwrap();
        state.set_current_job(None).unwrap();
        assert_eq!(
            accepted(replay_idempotent_update(&state, "k", "fp").unwrap()).as_deref(),
            Some("admitted")
        );
    }

    #[test]
    fn refuse_update_propagates_maintenance_io() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        std::fs::create_dir(dir.path().join("maintenance.json")).unwrap();
        let err = refuse_update_if_stuck_in(&state).unwrap_err();
        assert!(
            !matches!(err, UpdaterError::Conflict),
            "I/O must not look like 'not stuck', got {err}"
        );
    }

    #[test]
    fn refuse_update_propagates_job_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        state.set_current_job(Some("j1")).unwrap();
        std::fs::write(dir.path().join("job.j1.json"), b"{not-json").unwrap();
        let err = refuse_update_if_stuck_in(&state).unwrap_err();
        assert!(
            matches!(err, UpdaterError::Json(_)),
            "corrupt current job must fail closed, got {err}"
        );
    }

    #[test]
    fn refuse_update_blocks_active_maintenance() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        let mut m = MaintenanceFile::inactive();
        m.active = true;
        m.phase = Phase::Stopping;
        state.write_maintenance(&m).unwrap();
        assert!(matches!(
            refuse_update_if_stuck_in(&state),
            Err(UpdaterError::Conflict)
        ));
    }

    #[test]
    fn fingerprint_changes_with_target_and_flags() {
        let a = update_request_fingerprint(
            &DeployTag::parse("v1.0.0").unwrap(),
            UpdateMode::Release,
            false,
            false,
            None,
            None,
            None,
            None,
        );
        let b = update_request_fingerprint(
            &DeployTag::parse("v1.0.1").unwrap(),
            UpdateMode::Release,
            false,
            false,
            None,
            None,
            None,
            None,
        );
        let c = update_request_fingerprint(
            &DeployTag::parse("v1.0.0").unwrap(),
            UpdateMode::Release,
            true,
            false,
            None,
            None,
            None,
            None,
        );
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(
            a,
            update_request_fingerprint(
                &DeployTag::parse("v1.0.0").unwrap(),
                UpdateMode::Release,
                false,
                false,
                None,
                None,
                None,
                None,
            )
        );
    }

    #[test]
    fn idempotent_replay_requires_matching_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        let fp = update_request_fingerprint(
            &DeployTag::parse("v1.2.3").unwrap(),
            UpdateMode::Release,
            false,
            false,
            None,
            None,
            None,
            None,
        );
        state
            .write_job(&sample_job("job-a", Some("k1"), Some(&fp)))
            .unwrap();

        assert_eq!(
            accepted(replay_idempotent_update(&state, "k1", &fp).unwrap()).as_deref(),
            Some("job-a")
        );
        let other = update_request_fingerprint(
            &DeployTag::parse("v9.9.9").unwrap(),
            UpdateMode::Release,
            false,
            false,
            None,
            None,
            None,
            None,
        );
        let err = replay_idempotent_update(&state, "k1", &other).unwrap_err();
        assert!(
            matches!(err, UpdaterError::InvalidInput(_)),
            "different fingerprint must not replay, got {err}"
        );
    }

    #[test]
    fn idempotent_lookup_survives_process_restart_via_job_files() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        let fp = "update|v1.2.3|release|downgrade=false|risk=false|diverged=unset|unknown=unset|irreversible=unset|compose_override=unset";
        state
            .write_job(&sample_job("durable", Some("restart-key"), Some(fp)))
            .unwrap();
        // New StateDir handle = new process with no in-memory queue.
        let state2 = StateDir::open_readonly(dir.path()).unwrap();
        assert_eq!(
            accepted(replay_idempotent_update(&state2, "restart-key", fp).unwrap()).as_deref(),
            Some("durable")
        );
    }

    #[test]
    fn idempotent_lookup_fails_closed_on_corrupt_job() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        std::fs::write(dir.path().join("job.bad.json"), b"{not-json").unwrap();
        let err = replay_idempotent_update(&state, "k", "fp").unwrap_err();
        assert!(
            matches!(err, UpdaterError::Json(_)),
            "corrupt job during idempotency scan must fail closed, got {err}"
        );
    }
}
