//! Normal update flow. State machine progression per spec §7.
//! Supports release (GitHub `release.json` when present, else Docker Hub `vX.Y.Z` images)
//! and commit (CI image tags) modes. Swap/health use `PreflightReport` digests and tags;
//! a missing `pre.manifest` is fine for the Docker Hub release path.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tracing::{error, info, warn};

use crate::docker::{ComposeRunner, ROLLBACK_IMAGE_TAG};
use crate::env_file::EnvFile;
use crate::error::{Result, UpdaterError};
use crate::probe::compose::ComposeBinary;
use crate::snapshot::SnapshotManager;
use crate::state::{JobStatus, Phase, UpdaterStateFile};
use crate::version::{DeployTag, MyriadVersion, UpdateMode};
use crate::worker::{machine::PhaseRecorder, preflight, rollback, Worker};

pub async fn run(
    worker: Arc<Worker>,
    job_id: String,
    target: DeployTag,
    mode: UpdateMode,
    risk: preflight::RiskFlags,
    actor: Option<String>,
) -> Result<()> {
    info!(
        job = %job_id,
        target = %target,
        ?mode,
        allow_downgrade = risk.allow_downgrade,
        allow_diverged = risk.allow_diverged,
        allow_unknown = risk.allow_unknown,
        allow_irreversible = risk.allow_irreversible,
        actor = actor.as_deref().unwrap_or("-"),
        "update flow starting"
    );

    let rec = PhaseRecorder {
        state: worker.state(),
        job_id: job_id.clone(),
        from_version: worker.state().read_updater()?.current_version.clone(),
        to_version: Some(target.clone()),
    };

    // ============================================================
    // Pre-swap phase: any failure cleans up without touching prod.
    // ============================================================
    // Structured audit line before any side effects (operator risk acknowledgements).
    let actor_suffix = actor
        .as_deref()
        .map(|a| format!(" actor={a}"))
        .unwrap_or_default();
    let audit = format!(
        "audit: update_request job={} target={} mode={} \
         allow_downgrade={} allow_diverged={} allow_unknown={} allow_irreversible={}{}",
        job_id,
        target.as_str(),
        mode.as_str(),
        risk.allow_downgrade,
        risk.allow_diverged,
        risk.allow_unknown,
        risk.allow_irreversible,
        actor_suffix,
    );
    worker.state().append_history(&audit)?;
    let _ = worker.state().append_audit(&audit);

    rec.enter(Phase::Preflight, "updater.phase.preflight")?;
    let pre = match preflight::run(worker.clone(), &target, mode, risk).await {
        Ok(r) => {
            rec.finish_step_ok()?;
            r
        }
        Err(e) => {
            error!(job = %job_id, err = %e, "preflight failed");
            rec.finish_step_err(format!("preflight: {e}"))?;
            rec.finalize(JobStatus::Failed)?;
            crate::worker::machine::clear_maintenance(worker.state())?;
            return Err(e);
        }
    };

    // Commit mode normalizes branch tips → dev-<sha>; use pre.target for the rest of the flow.
    let target = pre.target.clone();
    // Keep job.to_version in sync with the effective tag (not the raw request).
    {
        let mut job = worker.state().read_job(&job_id)?;
        job.to_version = Some(target.clone());
        worker.state().write_job(&job)?;
    }
    // Refresh recorder to_version to the effective tag.
    let rec = PhaseRecorder {
        state: worker.state(),
        job_id: job_id.clone(),
        from_version: rec.from_version.clone(),
        to_version: Some(target.clone()),
    };

    // Maintenance ON.
    rec.enter(Phase::MaintenanceOn, "updater.phase.maintenance_on")?;
    rec.finish_step_ok()?;

    let compose = build_compose_runner(&worker).await?;

    // Stop business containers.
    rec.enter(Phase::Stopping, "updater.phase.stopping")?;
    let out = compose
        .stop(&["frontend", "backend"], 30)
        .await
        .map_err(|e| {
            rec.finish_step_err(format!("stop frontend/backend: {e}"))
                .ok();
            e
        })?;
    if !out.ok() {
        let err = format!("compose stop failed: {}", out.error_summary());
        rec.finish_step_err(&err)?;
        rec.finalize(JobStatus::Failed)?;
        crate::worker::machine::clear_maintenance(worker.state())?;
        return Err(UpdaterError::Internal(anyhow::anyhow!(err)));
    }
    rec.finish_step_ok()?;

    // Snapshot pgdata.
    rec.enter(Phase::Snapshotting, "updater.phase.snapshotting")?;
    let stop_pg = compose.stop(&["postgres"], 60).await?;
    if !stop_pg.ok() {
        let err = format!("stop postgres failed: {}", stop_pg.error_summary());
        rec.finish_step_err(&err)?;
        rec.finalize(JobStatus::Failed)?;
        crate::worker::machine::clear_maintenance(worker.state())?;
        return Err(UpdaterError::Internal(anyhow::anyhow!(err)));
    }

    let snap = SnapshotManager {
        state: worker.state(),
        pgdata: worker.cli().pgdata.clone(),
    };
    let snapshot_id = format!("snap-{}", job_id);
    let source_version = pre.from_version.clone().or_else(|| {
        EnvFile::load(&worker.cli().env_file)
            .ok()
            .and_then(|e| e.get("MYRIAD_TAG").map(|s| s.to_string()))
            .and_then(|t| DeployTag::parse(&t).ok())
    });
    let _ = snap
        .create(&snapshot_id, source_version)
        .await
        .map_err(|e| {
            rec.finish_step_err(format!("snapshot: {e}")).ok();
            e
        })?;

    let start_pg = compose.start(&["postgres"]).await?;
    if !start_pg.ok() {
        let err = format!("restart postgres failed: {}", start_pg.error_summary());
        rec.finish_step_err(&err)?;
        rec.finalize(JobStatus::Failed)?;
        crate::worker::machine::clear_maintenance(worker.state())?;
        return Err(UpdaterError::Internal(anyhow::anyhow!(err)));
    }
    {
        let mut job = worker.state().read_job(&job_id)?;
        job.snapshot_id = Some(snapshot_id.clone());
        worker.state().write_job(&job)?;
    }
    rec.finish_step_ok()?;

    // ============================================================
    // Swap tag. Beyond here, any failure triggers automated rollback.
    // ============================================================
    rec.enter(Phase::SwapTag, "updater.phase.swap_tag")?;
    let from_tag_hint = rec
        .from_version
        .as_ref()
        .map(|v| v.to_string())
        .or_else(|| pre.from_version.as_ref().map(|v| v.to_string()));
    let from_tag_backup = match swap_tag(&worker, target.as_str()) {
        Ok(prev) => prev,
        Err(e) => {
            rec.finish_step_err(format!("swap_tag: {e}"))?;
            return finish_with_rollback(
                &worker,
                &rec,
                &compose,
                &snap,
                &snapshot_id,
                from_tag_hint.as_deref(),
                e,
            )
            .await;
        }
    };
    rec.finish_step_ok()?;

    match pin_rollback_images(&worker, &from_tag_backup).await {
        Ok(true) => {
            if let Ok(v) = DeployTag::parse(&from_tag_backup) {
                let mut st = worker.state().read_updater()?;
                st.rollback_version = Some(v);
                let _ = worker.state().write_updater(&st);
            }
        }
        Ok(false) => {
            tracing::warn!(
                prev = %from_tag_backup,
                "pre-start rollback image pin skipped because the complete image pair is not local"
            );
        }
        Err(e) => {
            tracing::warn!(err = %e, prev = %from_tag_backup, "pre-start rollback image pin failed");
        }
    }

    rec.enter(Phase::StartingNew, "updater.phase.starting_new")?;
    let volume_init = match compose.init_backend_volumes().await {
        Ok(output) => output,
        Err(error) => {
            let err = format!("backend volume ownership initialization failed: {error}");
            rec.finish_step_err(&err)?;
            return finish_with_rollback(
                &worker,
                &rec,
                &compose,
                &snap,
                &snapshot_id,
                Some(&from_tag_backup),
                UpdaterError::Internal(anyhow::anyhow!(err)),
            )
            .await;
        }
    };
    if !volume_init.ok() {
        let err = format!(
            "backend volume ownership initialization failed: {}",
            volume_init.error_summary()
        );
        rec.finish_step_err(&err)?;
        return finish_with_rollback(
            &worker,
            &rec,
            &compose,
            &snap,
            &snapshot_id,
            Some(&from_tag_backup),
            UpdaterError::Internal(anyhow::anyhow!(err)),
        )
        .await;
    }
    tracing::info!(
        output = %volume_init.stdout_tail.trim(),
        diagnostics = %volume_init.stderr_tail.trim(),
        "backend volume ownership and write verification completed"
    );
    let up = compose.up_detached(&["backend", "frontend"]).await?;
    if !up.ok() {
        let err = format!("compose up new failed: {}", up.error_summary());
        rec.finish_step_err(&err)?;
        return finish_with_rollback(
            &worker,
            &rec,
            &compose,
            &snap,
            &snapshot_id,
            Some(&from_tag_backup),
            UpdaterError::Internal(anyhow::anyhow!(err)),
        )
        .await;
    }
    rec.finish_step_ok()?;

    rec.enter(Phase::HealthProbing, "updater.phase.health_probing")?;
    let deadline = Duration::from_secs(300u64.max((pre.estimated_seconds as u64) * 3));
    let probe_result = health_probe_phased(&worker, &target, deadline).await;
    if let Err(e) = probe_result {
        rec.finish_step_err(format!("health: {e}"))?;
        return finish_with_rollback(
            &worker,
            &rec,
            &compose,
            &snap,
            &snapshot_id,
            Some(&from_tag_backup),
            e,
        )
        .await;
    }
    rec.finish_step_ok()?;

    rec.enter(Phase::SwappingProxy, "updater.phase.swapping_proxy")?;
    let mut st = worker.state().read_updater()?;
    record_successful_deploy(&mut st, target.clone(), pre.target_commit_sha.clone());
    worker.state().write_updater(&st)?;
    rec.finish_step_ok()?;

    rec.enter(Phase::Finalize, "updater.phase.finalize")?;
    // Keep `*:myriad-rollback` and `rollback_version` on the build that was running
    // before this update. The next update will advance the slot to this build before
    // it starts its own target.
    rec.finish_step_ok()?;

    let _ = snap.prune(3);

    rec.finalize(JobStatus::Succeeded)?;
    // Maintenance may already be inactive after frontend probe phase; full clear resets job pointer.
    crate::worker::machine::clear_maintenance(worker.state())?;
    worker
        .state()
        .append_history(&format!("job {job_id}: SUCCESS {target} ({mode})"))?;
    let _ = worker.state().append_audit(&format!(
        "audit: update_succeeded job={job_id} target={} mode={}",
        target.as_str(),
        mode.as_str()
    ));
    info!(job = %job_id, %target, ?mode, "update succeeded");
    Ok(())
}

async fn pin_rollback_images(worker: &Arc<Worker>, previous_tag: &str) -> Result<bool> {
    let env = EnvFile::load(&worker.cli().env_file)?;
    let backend = env
        .get("BACKEND_IMAGE")
        .map(|s| s.to_string())
        .ok_or_else(|| {
            UpdaterError::Precondition("BACKEND_IMAGE missing; cannot pin rollback image".into())
        })?;
    let frontend = env
        .get("FRONTEND_IMAGE")
        .map(|s| s.to_string())
        .ok_or_else(|| {
            UpdaterError::Precondition("FRONTEND_IMAGE missing; cannot pin rollback image".into())
        })?;
    let pairs = [("backend", backend), ("frontend", frontend)];

    // Do not create a split rollback slot where backend and frontend point at
    // different versions. Verify the complete pair before changing either alias.
    for (comp, repo) in &pairs {
        let source = format!("{repo}:{previous_tag}");
        if !worker.docker().image_exists_local(&source).await {
            tracing::warn!(%comp, %source, "rollback pin skipped: image not local");
            return Ok(false);
        }
    }

    for (comp, repo) in pairs {
        let source = format!("{repo}:{previous_tag}");
        worker
            .docker()
            .tag_image(&source, &repo, ROLLBACK_IMAGE_TAG)
            .await
            .map_err(|e| UpdaterError::Docker(format!("pin rollback {comp} ({source}): {e}")))?;
        info!(%comp, %source, pin = %format!("{repo}:{ROLLBACK_IMAGE_TAG}"), "pinned rollback image");
    }
    Ok(true)
}

fn record_successful_deploy(
    state: &mut UpdaterStateFile,
    target: DeployTag,
    target_commit_sha: Option<String>,
) {
    state.current_version = Some(target);
    state.current_commit_sha = target_commit_sha;
    state.updater_version = MyriadVersion::parse(crate::self_version()).ok();
    // `rollback_version` deliberately remains unchanged: it identifies the
    // previous known-good build pinned immediately before this deploy started.
}

async fn finish_with_rollback(
    worker: &Arc<Worker>,
    rec: &PhaseRecorder<'_>,
    compose: &ComposeRunner,
    snap: &SnapshotManager<'_>,
    snapshot_id: &str,
    from_tag: Option<&str>,
    original_err: UpdaterError,
) -> Result<()> {
    // Health-probe false negatives used to destroy a working stack via rollback.
    // Before any destructive step, re-check with the hardened multi-path probe under
    // soft-pass timing (treat elapsed as already past soft threshold).
    if is_health_probe_failure(&original_err) {
        if let Some(target) = rec.to_version.clone() {
            info!(
                err = %original_err,
                target = %target,
                "health failure: re-checking before destructive rollback"
            );
            // Force soft-pass window open (90s+) for this recheck; prefer live frontend via proxy.
            let recheck = probe_one_tick(
                worker,
                &target,
                Duration::from_secs(120),
                FrontendProbe::LiveViaProxy,
            )
            .await;
            match recheck {
                ProbeTick::HardOk { detail, pass_kind } | ProbeTick::SoftOk { detail, pass_kind } => {
                    warn!(
                        %detail,
                        %pass_kind,
                        target = %target,
                        "health re-check succeeded after timeout — treating update as SUCCESS \
                         (skipping rollback). Original probe was a false negative."
                    );
                    rec.enter(Phase::SwappingProxy, "updater.phase.swapping_proxy")?;
                    let mut st = worker.state().read_updater()?;
                    record_successful_deploy(&mut st, target.clone(), None);
                    worker.state().write_updater(&st)?;
                    rec.finish_step_ok()?;
                    rec.enter(Phase::Finalize, "updater.phase.finalize")?;
                    rec.finish_step_ok()?;
                    let _ = snap.prune(3);
                    rec.finalize(JobStatus::Succeeded)?;
                    crate::worker::machine::clear_maintenance(worker.state())?;
                    worker.state().append_history(&format!(
                        "job {}: SUCCESS after health false-negative recheck ({target}); \
                         original_probe_err={original_err}",
                        rec.job_id
                    ))?;
                    return Ok(());
                }
                ProbeTick::NotReady { detail } => {
                    warn!(
                        %detail,
                        "health re-check still not ready; proceeding with rollback"
                    );
                }
            }
        }
    }

    error!(err = %original_err, "rollback triggered");
    let rb_result =
        rollback::execute_inline(worker.clone(), rec, compose, snap, snapshot_id, from_tag).await;
    match rb_result {
        Ok(restored) => {
            rec.finalize(JobStatus::Failed)?;
            let mut st = worker.state().read_updater()?;
            if let Some(v) = restored.or_else(|| rec.from_version.clone()) {
                let version_changed = st.current_version.as_ref() != Some(&v);
                st.current_version = Some(v);
                if version_changed {
                    st.current_commit_sha = None;
                }
            }
            st.last_failed_update = Some(crate::state::FailedUpdate {
                from_version: rec.from_version.clone(),
                to_version: rec.to_version.clone(),
                at: Utc::now(),
                reason: original_err.to_string(),
                job_id: rec.job_id.clone(),
            });
            worker.state().write_updater(&st)?;
            crate::worker::machine::clear_maintenance(worker.state())?;
            let rb_ok = format!("job {}: ROLLBACK_OK ({original_err})", rec.job_id);
            worker.state().append_history(&rb_ok)?;
            let _ = worker.state().append_audit(&format!(
                "audit: auto_rollback_ok job={} err={original_err}",
                rec.job_id
            ));
            Err(original_err)
        }
        Err(rb_err) => {
            rec.finish_step_err(format!("rollback failed: {rb_err}"))?;
            rec.finalize(JobStatus::NeedsManual)?;
            let mut m = worker.state().read_maintenance()?;
            m.phase = Phase::NeedsManual;
            m.message_key = "updater.phase.needs_manual".into();
            m.bump_heartbeat();
            worker.state().write_maintenance(&m)?;
            worker.state().append_history(&format!(
                "job {}: NEEDS_MANUAL — original={original_err}; rollback={rb_err}",
                rec.job_id
            ))?;
            // Prefer returning the original health/update error when rollback also failed,
            // but keep rollback detail in the message for operators.
            Err(UpdaterError::Precondition(format!(
                "update failed ({original_err}); rollback also failed ({rb_err})"
            )))
        }
    }
}

fn is_health_probe_failure(err: &UpdaterError) -> bool {
    let s = err.to_string();
    s.contains("health probe") || s.contains("health:")
}

pub(crate) async fn build_compose_runner_pub(worker: &Arc<Worker>) -> Result<ComposeRunner> {
    build_compose_runner(worker).await
}

async fn build_compose_runner(worker: &Arc<Worker>) -> Result<ComposeRunner> {
    let probe_path = worker.state().root().join("env-probe.json");
    let probe: crate::probe::EnvProbe = serde_json::from_slice(&std::fs::read(&probe_path)?)?;
    let binary: ComposeBinary = probe
        .compose
        .binary
        .ok_or_else(|| UpdaterError::Internal(anyhow::anyhow!("compose binary unavailable")))?;
    let project = std::env::var("COMPOSE_PROJECT_NAME").unwrap_or_else(|_| "myriad".into());
    let compose_base = probe
        .compose
        .compose_files
        .first()
        .and_then(|file| file.parent())
        .unwrap_or(&worker.cli().compose_dir);
    let host_project_directory = worker
        .docker()
        .resolve_host_bind_source(compose_base)
        .await?;
    Ok(ComposeRunner::new(
        binary,
        project,
        probe.compose.compose_files,
        worker.cli().env_file.clone(),
        worker.cli().compose_dir.clone(),
        host_project_directory,
    ))
}

fn swap_tag(worker: &Arc<Worker>, new_tag: &str) -> Result<String> {
    let mut env = EnvFile::load(&worker.cli().env_file)?;
    let prev = env
        .get("MYRIAD_TAG")
        .map(|s| s.to_string())
        .ok_or_else(|| UpdaterError::Precondition("MYRIAD_TAG missing in .env".into()))?;
    env.set("MYRIAD_TAG", new_tag)?;
    env.save()?;
    Ok(prev)
}

/// Two-phase health probe after starting new backend/frontend.
///
/// 1. **Backend-only** (maintenance still active): direct `http://backend:1103/health`
///    so DB/migrations/version identity is verified without relying on proxy.
/// 2. **Lift maintenance** (keep job id) so proxy serves real frontend HTML.
/// 3. **Live frontend** via `http://proxy:80/`: prefer `myriad-version` / commit meta;
///    soft-pass still allows image-tag match if stamps lag.
///
/// No Docker exec / probe containers. Needs **2** consecutive OK ticks per phase.
async fn health_probe_phased(
    worker: &Arc<Worker>,
    target: &DeployTag,
    deadline: Duration,
) -> Result<()> {
    let start = std::time::Instant::now();
    // Spec §11.3 initial wait before first probe.
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Phase 1 — backend hard identity while users still see maintenance page.
    let phase1_budget = deadline.mul_f32(0.55).max(Duration::from_secs(60));
    run_probe_loop(
        worker,
        target,
        start,
        phase1_budget,
        FrontendProbe::BackendOnly,
        "phase1_backend",
    )
    .await?;

    // Lift maintenance so proxy forwards to real frontend (keep job/phase for rollback).
    deactivate_maintenance_for_frontend_probe(worker.state())?;
    info!(
        target = %target,
        "health: maintenance inactive for live frontend probe via proxy"
    );
    let _ = worker.state().append_history(&format!(
        "health: lift maintenance for frontend probe (target={})",
        target.as_str()
    ));

    // Brief settle so proxy cache of maintenance.json expires (proxy caches ~1s).
    tokio::time::sleep(Duration::from_secs(2)).await;

    let remaining = deadline.saturating_sub(start.elapsed());
    if remaining < Duration::from_secs(20) {
        return Err(UpdaterError::Precondition(format!(
            "health probe: insufficient time left for frontend phase ({}s)",
            remaining.as_secs()
        )));
    }

    run_probe_loop(
        worker,
        target,
        start,
        deadline,
        FrontendProbe::LiveViaProxy,
        "phase2_frontend",
    )
    .await
}

/// Set `active=false` without clearing job id so rollback can re-enter maintenance.
fn deactivate_maintenance_for_frontend_probe(state: &crate::state::StateDir) -> Result<()> {
    let mut m = state.read_maintenance()?;
    m.active = false;
    m.message_key = "updater.phase.health_probing_live".into();
    m.bump_heartbeat();
    state.write_maintenance(&m)?;
    Ok(())
}

async fn run_probe_loop(
    worker: &Arc<Worker>,
    target: &DeployTag,
    start: std::time::Instant,
    deadline: Duration,
    mode: FrontendProbe,
    phase_label: &str,
) -> Result<()> {
    const OK_STREAK_NEED: u32 = 2;
    const SOFT_PASS_AFTER: Duration = Duration::from_secs(90);

    let mut ok_streak = 0u32;
    let mut soft_streak = 0u32;
    let mut last_diag = String::new();
    let mut attempts: u32 = 0;

    while start.elapsed() < deadline {
        tokio::time::sleep(Duration::from_secs(2)).await;
        attempts += 1;
        let elapsed = start.elapsed();

        let tick = probe_one_tick(worker, target, elapsed, mode).await;
        let diag = match &tick {
            ProbeTick::HardOk { detail, pass_kind } => {
                ok_streak += 1;
                soft_streak = 0;
                if ok_streak >= OK_STREAK_NEED {
                    info!(
                        target = %target,
                        phase = phase_label,
                        pass_kind = %pass_kind,
                        attempts,
                        elapsed_s = elapsed.as_secs(),
                        detail = %detail,
                        "health probe passed (hard)"
                    );
                    let _ = worker.state().append_history(&format!(
                        "health: pass hard phase={phase_label} kind={pass_kind} target={} detail={detail}",
                        target.as_str()
                    ));
                    return Ok(());
                }
                format!("hard ok streak={ok_streak}/{OK_STREAK_NEED} kind={pass_kind} {detail}")
            }
            ProbeTick::SoftOk { detail, pass_kind } if elapsed >= SOFT_PASS_AFTER => {
                soft_streak += 1;
                ok_streak = 0;
                if soft_streak >= OK_STREAK_NEED {
                    warn!(
                        target = %target,
                        phase = phase_label,
                        pass_kind = %pass_kind,
                        attempts,
                        elapsed_s = elapsed.as_secs(),
                        detail = %detail,
                        "health probe passed (soft)"
                    );
                    let _ = worker.state().append_history(&format!(
                        "health: pass soft phase={phase_label} kind={pass_kind} target={} detail={detail}",
                        target.as_str()
                    ));
                    return Ok(());
                }
                format!("soft ok streak={soft_streak}/{OK_STREAK_NEED} kind={pass_kind} {detail}")
            }
            ProbeTick::SoftOk { detail, pass_kind } => {
                ok_streak = 0;
                soft_streak = 0;
                format!(
                    "soft-eligible kind={pass_kind} (wait {}s): {detail}",
                    SOFT_PASS_AFTER.as_secs()
                )
            }
            ProbeTick::NotReady { detail } => {
                ok_streak = 0;
                soft_streak = 0;
                detail.clone()
            }
        };

        if diag != last_diag {
            warn!(
                target = %target,
                phase = phase_label,
                attempt = attempts,
                elapsed_s = elapsed.as_secs(),
                %diag,
                "health probe not ready"
            );
            last_diag = diag;
        }
        let _ = crate::worker::machine::heartbeat(worker.state());
    }
    Err(UpdaterError::Precondition(format!(
        "health probe deadline ({}s) exceeded phase={phase_label}; last={last_diag}",
        deadline.as_secs()
    )))
}

/// Whether this tick must verify real frontend HTML via proxy (post-maintenance).
#[derive(Debug, Clone, Copy)]
enum FrontendProbe {
    /// Maintenance may still be active — only backend HTTP + image/running matter.
    BackendOnly,
    /// Maintenance lifted — proxy must serve real frontend (meta preferred).
    LiveViaProxy,
}

enum ProbeTick {
    HardOk { detail: String, pass_kind: &'static str },
    SoftOk { detail: String, pass_kind: &'static str },
    NotReady { detail: String },
}

fn backend_storage_writable(health: &serde_json::Value) -> bool {
    // Missing means an older backend from before the storage-preflight field;
    // preserve rollback/upgrade compatibility for those images.
    match health.get("storage_writable") {
        None => true,
        Some(value) => value.as_bool().unwrap_or(false),
    }
}

async fn probe_one_tick(
    worker: &Arc<Worker>,
    target: &DeployTag,
    elapsed: Duration,
    mode: FrontendProbe,
) -> ProbeTick {
    const LOOSE_FRONTEND_AFTER: Duration = Duration::from_secs(45);

    let docker = worker.docker();
    let backend_running = docker.is_running("myriad-backend").await.unwrap_or(false)
        || docker.is_running("backend").await.unwrap_or(false);
    let frontend_running = docker.is_running("myriad-frontend").await.unwrap_or(false)
        || docker.is_running("frontend").await.unwrap_or(false);

    let backend_image = match docker.container_image_ref("myriad-backend").await {
        Ok(s) if !s.is_empty() => s,
        _ => docker
            .container_image_ref("backend")
            .await
            .unwrap_or_default(),
    };
    let frontend_image = match docker.container_image_ref("myriad-frontend").await {
        Ok(s) if !s.is_empty() => s,
        _ => docker
            .container_image_ref("frontend")
            .await
            .unwrap_or_default(),
    };
    let backend_img_ok = image_ref_matches_target(&backend_image, target);
    let frontend_img_ok = image_ref_matches_target(&frontend_image, target);

    let be = docker
        .http_probe("http://backend:1103/health", Duration::from_secs(10))
        .await;
    let (be_code, be_body) = match be {
        Ok(v) => v,
        Err(e) => {
            return ProbeTick::NotReady {
                detail: format!(
                    "backend unreachable ({e}); running={backend_running} image={backend_image}"
                ),
            };
        }
    };
    if be_code != 200 {
        return ProbeTick::NotReady {
            detail: format!(
                "backend HTTP {be_code}: {} | running={backend_running} image={backend_image}",
                be_body
                    .chars()
                    .take(120)
                    .collect::<String>()
                    .replace('\n', " ")
            ),
        };
    }

    let json: serde_json::Value = serde_json::from_str(&be_body).unwrap_or(serde_json::Value::Null);
    let version = json.get("version").and_then(|v| v.as_str()).unwrap_or("");
    let commit_sha = json.get("commit_sha").and_then(|c| c.as_str());
    let db = json
        .get("db_connected")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mig = json
        .get("migrations_applied")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    // Older backends do not expose this field; retain upgrade compatibility.
    // New backends only start after a real uid-1000 storage write probe.
    let storage = backend_storage_writable(&json);
    let version_ok =
        target.matches_runtime_version(version) || commit_matches_target(target, commit_sha);
    let backend_identity_ok = version_ok || backend_img_ok;

    if !db || !mig || !storage {
        return ProbeTick::NotReady {
            detail: format!(
                "backend up but db_connected={db} migrations_applied={mig} storage_writable={storage} \
                 version={version:?} commit={commit_sha:?} image={backend_image}"
            ),
        };
    }

    // --- Phase 1: backend only ---
    if matches!(mode, FrontendProbe::BackendOnly) {
        if backend_identity_ok {
            return ProbeTick::HardOk {
                pass_kind: "hard_backend",
                detail: format!(
                    "backend identity ok version={version:?} commit={commit_sha:?} \
                     be_img_ok={backend_img_ok} fe_running={frontend_running} fe_img_ok={frontend_img_ok}"
                ),
            };
        }
        if backend_running && backend_img_ok {
            return ProbeTick::SoftOk {
                pass_kind: "soft_backend_image",
                detail: format!(
                    "backend running+image tag; version stamp weak version={version:?} want={}",
                    target.as_str()
                ),
            };
        }
        return ProbeTick::NotReady {
            detail: format!(
                "backend identity incomplete: version={version:?} commit={commit_sha:?} \
                 ver_ok={version_ok} be_img_ok={backend_img_ok} want={}",
                target.as_str()
            ),
        };
    }

    // --- Phase 2: live frontend via proxy (maintenance inactive) ---
    // Updater is on admin-net; proxy is dual-homed — do not use frontend:1102.
    let fe = docker
        .http_probe("http://proxy:80/", Duration::from_secs(10))
        .await;
    let (fe_code, fe_body) = match fe {
        Ok(v) => v,
        Err(e) => {
            if backend_identity_ok && backend_running && frontend_img_ok && frontend_running {
                return ProbeTick::SoftOk {
                    pass_kind: "soft_backend_fe_img_proxy_down",
                    detail: format!(
                        "backend OK but proxy unreachable ({e}); \
                         fe_running={frontend_running} image={frontend_image}"
                    ),
                };
            }
            return ProbeTick::NotReady {
                detail: format!(
                    "frontend via proxy unreachable ({e}); backend identity ok={backend_identity_ok}"
                ),
            };
        }
    };

    let looks_like_maintenance = fe_body.contains("更新维护中")
        || fe_body.contains("maintenance")
        || fe_body.contains("updater.phase");
    let fe_html_ok = fe_code == 200
        && !looks_like_maintenance
        && (fe_body.contains("<html")
            || fe_body.contains("<!DOCTYPE")
            || fe_body.contains("myriad")
            || !fe_body.is_empty());
    let fe_meta_ok = fe_code == 200
        && !looks_like_maintenance
        && (fe_body.contains(&format!(
            r#"name="myriad-version" content="{}""#,
            target.as_str()
        )) || frontend_meta_matches(&fe_body, target));

    // Hard: real page meta, or image match + non-maintenance HTML after grace.
    let frontend_hard_ok = fe_meta_ok
        || (elapsed >= LOOSE_FRONTEND_AFTER
            && fe_html_ok
            && frontend_img_ok
            && backend_identity_ok);

    if backend_identity_ok && frontend_hard_ok {
        let pass_kind = if fe_meta_ok {
            "hard_fe_meta"
        } else {
            "hard_fe_html_image"
        };
        return ProbeTick::HardOk {
            pass_kind,
            detail: format!(
                "version={version:?} commit={commit_sha:?} \
                 be_img_ok={backend_img_ok} fe_meta_ok={fe_meta_ok} fe_img_ok={frontend_img_ok} \
                 fe_code={fe_code} maint_html={looks_like_maintenance}"
            ),
        };
    }

    // Soft: containers + DB + both image tags; page may still be warming.
    if db
        && mig
        && backend_running
        && frontend_running
        && backend_img_ok
        && frontend_img_ok
        && (fe_html_ok || looks_like_maintenance)
    {
        return ProbeTick::SoftOk {
            pass_kind: "soft_dual_image",
            detail: format!(
                "running+db+image tags; fe_meta_ok={fe_meta_ok} maint_html={looks_like_maintenance} \
                 version={version:?} want={}",
                target.as_str()
            ),
        };
    }

    ProbeTick::NotReady {
        detail: format!(
            "frontend identity incomplete: version={version:?} commit={commit_sha:?} \
             ver_ok={version_ok} be_img_ok={backend_img_ok} fe_code={fe_code} \
             fe_meta_ok={fe_meta_ok} fe_img_ok={frontend_img_ok} maint_html={looks_like_maintenance} \
             want={}",
            target.as_str()
        ),
    }
}

/// True when container image ref is clearly the target deploy tag
/// (`…:dev-abc1234` or `…:v1.2.3`).
fn image_ref_matches_target(image_ref: &str, target: &DeployTag) -> bool {
    if image_ref.is_empty() {
        return false;
    }
    let tag = target.as_str();
    // Exact tag suffix: repo:tag or registry/repo:tag
    if image_ref.ends_with(&format!(":{tag}")) || image_ref.ends_with(&format!("/{tag}")) {
        return true;
    }
    // Digest-only refs cannot prove tag; still allow short sha substring for commit tags.
    if let Some(sha) = target.commit_sha() {
        if image_ref.contains(sha) {
            return true;
        }
    }
    image_ref.contains(tag)
}

fn frontend_meta_matches(html: &str, target: &DeployTag) -> bool {
    // Parse content="..." of myriad-version meta loosely.
    let marker = r#"name="myriad-version" content=""#;
    if let Some(idx) = html.find(marker) {
        let rest = &html[idx + marker.len()..];
        if let Some(end) = rest.find('"') {
            let reported = &rest[..end];
            if target.matches_runtime_version(reported) {
                return true;
            }
        }
    }
    // Also accept myriad-commit meta matching the target sha (commit-mode stamps).
    if let Some(sha) = target.commit_sha() {
        let marker = r#"name="myriad-commit" content=""#;
        if let Some(idx) = html.find(marker) {
            let rest = &html[idx + marker.len()..];
            if let Some(end) = rest.find('"') {
                let reported = &rest[..end];
                if reported.starts_with(sha) || sha.starts_with(reported) {
                    return true;
                }
            }
        }
    }
    false
}

fn commit_matches_target(target: &DeployTag, commit_sha: Option<&str>) -> bool {
    let Some(want) = target.commit_sha() else {
        return false;
    };
    let Some(got) = commit_sha.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    // Full or prefix either way (short tag vs full stamp).
    got.eq_ignore_ascii_case(want)
        || got
            .to_ascii_lowercase()
            .starts_with(&want.to_ascii_lowercase())
        || want
            .to_ascii_lowercase()
            .starts_with(&got.to_ascii_lowercase())
}

#[cfg(test)]
mod health_match_tests {
    use super::*;
    use crate::version::DeployTag;

    #[test]
    fn commit_sha_matches_short_target() {
        let t = DeployTag::parse("dev-133d1bb").unwrap();
        assert!(commit_matches_target(
            &t,
            Some("133d1bb0123456789abcdef0123456789abcdef0")
        ));
        assert!(commit_matches_target(&t, Some("133d1bb")));
        assert!(!commit_matches_target(&t, Some("deadbeef")));
        assert!(!commit_matches_target(&t, None));
    }

    #[test]
    fn frontend_meta_matches_version_and_commit() {
        let t = DeployTag::parse("dev-133d1bb").unwrap();
        let html = r#"<meta name="myriad-version" content="dev-133d1bb0123456789abcdef0123456789abcdef0" />"#;
        assert!(frontend_meta_matches(html, &t));
        let html2 =
            r#"<meta name="myriad-commit" content="133d1bb0123456789abcdef0123456789abcdef0" />"#;
        assert!(frontend_meta_matches(html2, &t));
    }

    #[test]
    fn image_ref_matches_tag_suffix_and_sha() {
        let t = DeployTag::parse("dev-133d1bb").unwrap();
        assert!(image_ref_matches_target(
            "docker.io/somekawahitomi/myriad-backend:dev-133d1bb",
            &t
        ));
        assert!(image_ref_matches_target(
            "somekawahitomi/myriad-backend:dev-133d1bb",
            &t
        ));
        assert!(!image_ref_matches_target(
            "docker.io/somekawahitomi/myriad-backend:v0.2.2",
            &t
        ));
        let r = DeployTag::parse("v0.2.2").unwrap();
        assert!(image_ref_matches_target(
            "docker.io/x/myriad-backend:v0.2.2",
            &r
        ));
    }

    #[test]
    fn backend_storage_health_is_strict_when_field_is_present() {
        assert!(backend_storage_writable(&serde_json::json!({})));
        assert!(backend_storage_writable(
            &serde_json::json!({ "storage_writable": true })
        ));
        assert!(!backend_storage_writable(
            &serde_json::json!({ "storage_writable": false })
        ));
        assert!(!backend_storage_writable(
            &serde_json::json!({ "storage_writable": "invalid-old-shape" })
        ));
    }

    #[test]
    fn successful_deploy_preserves_previous_rollback_slot() {
        let previous = DeployTag::parse("v0.2.2").unwrap();
        let target = DeployTag::parse("v0.2.3").unwrap();
        let mut state = UpdaterStateFile {
            rollback_version: Some(previous.clone()),
            ..UpdaterStateFile::default()
        };

        record_successful_deploy(&mut state, target.clone(), Some("0123456789abcdef".into()));

        assert_eq!(state.current_version.as_ref(), Some(&target));
        assert_eq!(state.rollback_version.as_ref(), Some(&previous));
        assert_eq!(
            state.current_commit_sha.as_deref(),
            Some("0123456789abcdef")
        );
    }
}
