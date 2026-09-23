//! Normal update flow. State machine progression per spec §7.
//! Supports release (GitHub `release.json` when present, else Docker Hub `vX.Y.Z` images)
//! and commit (CI image tags) modes. Swap/health use the local image IDs selected during preflight;
//! The selected images and Compose changes are persisted before stopping services.
//!
//! # Failure invariants (do not regress)
//!
//! 1. **After app stop**: any `Err` must go through `dispatch_update_failure` → restart app
//!    (`PreSwap`) or full rollback (`PostSwap`).
//! 2. **`post_swap` only after `swap_tag` Ok** — failed tag write must not snapshot-restore.
//! 3. **After health Ok**: finish state writes without returning to the rollback path.
//! 4. Durable success allows restart recovery to finish the same state writes.
//! 5. **Preflight / maintenance entry failures**: always clear maintenance (best-effort).
//! 6. **Rollback paths**: failed restoration keeps writers stopped and maintenance
//!    active for explicit recovery; never start services on uncertain pgdata.

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
use crate::worker::backend_health::{
    backend_business_ready, backend_routes_full, backend_storage_writable,
};
use crate::worker::{Worker, machine::PhaseRecorder, preflight, rollback};

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
    // Pre-swap phase: any failure must restore the previous stack
    // (frontend/backend, and postgres if we stopped it) then leave idle.
    // Historically cleanup only cleared maintenance — services stayed down
    // and some Err paths never finalized the job (stuck maintenance).
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
    let _ = worker.state().append_history(&audit);
    let _ = worker.state().append_audit(&audit);

    if let Err(e) = rec.enter(Phase::Preflight, "updater.phase.preflight") {
        record_preflight_failure(worker.state(), &rec, &e);
        let _ = rec.finalize(JobStatus::Failed);
        let _ = crate::worker::machine::clear_maintenance(worker.state());
        return Err(e);
    }
    let pre = match preflight::run(worker.clone(), &target, mode, risk)
        .await
        .and_then(|r| {
            crate::state::atomic::write_atomic_json(
                &worker.state().prepared_report_path(&job_id),
                &r,
            )?;
            Ok(r)
        }) {
        Ok(r) => {
            let _ = rec.finish_step_ok();
            r
        }
        Err(e) => {
            error!(job = %job_id, err = %e, "preflight failed");
            let _ = rec.finish_step_err(format!("preflight: {e}"));
            // Surface the failure; the next scheduled attempt keeps the user preference.
            record_preflight_failure(worker.state(), &rec, &e);
            let _ = rec.finalize(JobStatus::Failed);
            let _ = crate::worker::machine::clear_maintenance(worker.state());
            return Err(e);
        }
    };

    // Commit mode normalizes branch tips → dev-<sha>; use pre.target for the rest of the flow.
    let target = pre.target.clone();
    // Keep job.to_version in sync with the effective tag (not the raw request).
    if let Ok(mut job) = worker.state().read_job(&job_id) {
        job.to_version = Some(target.clone());
        let _ = worker.state().write_job(&job);
    }
    // Refresh recorder to_version to the effective tag.
    let rec = PhaseRecorder {
        state: worker.state(),
        job_id: job_id.clone(),
        from_version: rec.from_version.clone(),
        to_version: Some(target.clone()),
    };

    // Maintenance ON. From here, every error is routed through UpdateFlowCtx so we
    // never leave services stopped or a swapped tag without cleanup.
    //
    // enter/finish failures must not leave maintenance.active stuck without a dispatcher.
    if let Err(e) = rec.enter(Phase::MaintenanceOn, "updater.phase.maintenance_on") {
        let _ = rec.finalize(JobStatus::Failed);
        let _ = crate::worker::machine::clear_maintenance(worker.state());
        return Err(e);
    }
    if let Err(e) = rec.finish_step_ok() {
        let _ = rec.finalize(JobStatus::Failed);
        let _ = crate::worker::machine::clear_maintenance(worker.state());
        return Err(e);
    }

    let compose = match build_compose_runner(&worker).await {
        Ok(c) => c,
        Err(e) => {
            let _ = rec.finish_step_err(format!("compose runner: {e}"));
            return finish_pre_swap_failure(&worker, &rec, None, PreSwapRestoreScope::None, e)
                .await;
        }
    };

    let snap = SnapshotManager {
        state: worker.state(),
        pgdata: worker.cli().pgdata.clone(),
    };
    let mut flow = UpdateFlowCtx::new();

    match run_update_body(
        worker.clone(),
        &rec,
        &compose,
        &snap,
        &mut flow,
        &pre,
        &target,
        &job_id,
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(e) => dispatch_update_failure(&worker, &rec, &compose, &snap, &flow, e).await,
    }
}

/// Tracks how far the update progressed so a single dispatcher can clean up.
#[derive(Debug, Clone)]
pub(crate) struct UpdateFlowCtx {
    /// What to restart if we fail before a successful tag swap.
    scope: PreSwapRestoreScope,
    /// True once we attempt / complete writing the new MYRIAD_TAG (destructive zone).
    post_swap: bool,
    /// Snapshot id for post-swap rollback (empty string = tag-only / external DB).
    snapshot_id: String,
    /// Previous MYRIAD_TAG to restore on rollback.
    from_tag: Option<String>,
}

impl UpdateFlowCtx {
    fn new() -> Self {
        Self {
            scope: PreSwapRestoreScope::None,
            post_swap: false,
            snapshot_id: String::new(),
            from_tag: None,
        }
    }
}

/// Classify failure for tests and the dispatcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpdateFailureKind {
    PreSwap(PreSwapRestoreScope),
    PostSwap,
}

pub(crate) fn classify_update_failure(flow: &UpdateFlowCtx) -> UpdateFailureKind {
    if flow.post_swap {
        UpdateFailureKind::PostSwap
    } else {
        UpdateFailureKind::PreSwap(flow.scope)
    }
}

async fn dispatch_update_failure(
    worker: &Arc<Worker>,
    rec: &PhaseRecorder<'_>,
    compose: &ComposeRunner,
    snap: &SnapshotManager<'_>,
    flow: &UpdateFlowCtx,
    err: UpdaterError,
) -> Result<()> {
    let kind = classify_update_failure(flow);
    match kind {
        UpdateFailureKind::PostSwap => {
            let _ = rec.finish_step_err(err.to_string());
            finish_with_rollback(
                worker,
                rec,
                compose,
                snap,
                &flow.snapshot_id,
                flow.from_tag.as_deref(),
                err,
            )
            .await
        }
        UpdateFailureKind::PreSwap(scope) => {
            let _ = rec.finish_step_err(err.to_string());
            finish_pre_swap_failure(worker, rec, Some(compose), scope, err).await
        }
    }
}

/// Body after maintenance_on + compose ready. Any `Err` is handled by
/// [`dispatch_update_failure`] — do not call finish_* helpers here.
///
/// 参数多是这条流程的固有形态：一次更新需要 worker/recorder/compose/快照/
/// 版本等全部上下文，打包成结构体只是把同样的耦合换个地方放。
#[allow(clippy::too_many_arguments)]
async fn run_update_body(
    worker: Arc<Worker>,
    rec: &PhaseRecorder<'_>,
    compose: &ComposeRunner,
    snap: &SnapshotManager<'_>,
    flow: &mut UpdateFlowCtx,
    pre: &preflight::PreflightReport,
    target: &DeployTag,
    job_id: &str,
) -> Result<()> {
    // ----- Stop app -----
    flow.scope = PreSwapRestoreScope::App;
    rec.enter(Phase::Stopping, "updater.phase.stopping")?;
    let out = compose
        .stop(&["frontend", "backend"], 30)
        .await
        .map_err(|e| UpdaterError::Internal(anyhow::anyhow!("stop frontend/backend: {e}")))?;
    if !out.ok() {
        return Err(UpdaterError::Internal(anyhow::anyhow!(
            "compose stop failed: {}",
            out.error_summary()
        )));
    }
    rec.finish_step_ok()?;

    // ----- Snapshot -----
    rec.enter(Phase::Snapshotting, "updater.phase.snapshotting")?;
    if worker.cli().db_mode.is_external() {
        info!("db_mode=external; skipping pgdata snapshot");
        let _ = worker.state().append_history(&format!(
            "job {job_id}: db_mode=external; skipping pgdata snapshot"
        ));
        flow.snapshot_id.clear();
    } else {
        crate::probe::filesystem::require_pgdata(&worker.cli().pgdata)?;
        flow.scope = PreSwapRestoreScope::AppAndPostgres;
        let stop_pg = compose
            .stop(&["postgres"], 60)
            .await
            .map_err(|e| UpdaterError::Internal(anyhow::anyhow!("stop postgres: {e}")))?;
        if !stop_pg.ok() {
            return Err(UpdaterError::Internal(anyhow::anyhow!(
                "stop postgres failed: {}",
                stop_pg.error_summary()
            )));
        }

        let snapshot_id = format!("snap-{job_id}");
        let source_version = pre.from_version.clone().or_else(|| {
            EnvFile::load(&worker.cli().env_file)
                .ok()
                .and_then(|e| e.get("MYRIAD_TAG").map(|s| s.to_string()))
                .and_then(|t| DeployTag::parse(&t).ok())
        });
        snap.create(&snapshot_id, source_version).await?;

        let start_pg = compose
            .start(&["postgres"])
            .await
            .map_err(|e| UpdaterError::Internal(anyhow::anyhow!("restart postgres: {e}")))?;
        if !start_pg.ok() {
            let up_pg = compose.up_detached(&["postgres"]).await;
            let up_ok = up_pg.as_ref().map(|o| o.ok()).unwrap_or(false);
            if !up_ok {
                let up_summary = up_pg
                    .as_ref()
                    .map(|o| o.error_summary())
                    .unwrap_or_else(|_| "compose up errored".into());
                return Err(UpdaterError::Internal(anyhow::anyhow!(
                    "restart postgres failed: start={}; up={}",
                    start_pg.error_summary(),
                    up_summary
                )));
            }
        }
        // Postgres is back; only app containers remain down.
        flow.scope = PreSwapRestoreScope::App;
        flow.snapshot_id = snapshot_id.clone();
        // Recovery must know the snapshot before any new-version writer can start.
        let mut job = worker.state().read_job(job_id)?;
        job.snapshot_id = Some(snapshot_id);
        worker.state().write_job(&job)?;
    }
    rec.finish_step_ok()?;

    // ----- Swap tag (destructive zone starts only after tag is written) -----
    rec.enter(Phase::SwapTag, "updater.phase.swap_tag")?;
    let from_tag_hint = rec
        .from_version
        .as_ref()
        .map(|v| v.to_string())
        .or_else(|| pre.from_version.as_ref().map(|v| v.to_string()));
    flow.from_tag = from_tag_hint.clone();
    // Keep post_swap=false until swap_tag succeeds so a failed tag write does not
    // trigger snapshot restore (postgres stop) — only app restart via PreSwap.
    pre.compose.install()?;
    let from_tag_backup = swap_tag(&worker, target.as_str())?;
    // The proxy keeps its own PROXY_TAG cadence: swap it only when the target
    // release actually shipped a proxy image. It is written before post_swap so
    // a crash still leaves both tags pointing at the new stack.
    if let Some(proxy_tag) = pre.proxy_target_tag.as_deref() {
        swap_proxy_tag(&worker, proxy_tag)?;
    }
    flow.post_swap = true;
    flow.from_tag = Some(from_tag_backup.clone());
    rec.finish_step_ok()?;

    match pin_rollback_images(&worker, &from_tag_backup).await {
        Ok(true) => {
            if let Ok(v) = DeployTag::parse(&from_tag_backup)
                && let Ok(mut st) = worker.state().read_updater()
            {
                st.rollback_version = Some(v);
                let _ = worker.state().write_updater(&st);
            }
        }
        Ok(false) => {
            tracing::warn!(
                prev = %from_tag_backup,
                "pre-start rollback image pin skipped because the complete image pair is not local"
            );
            let _ = worker.state().append_audit(&format!(
                "audit: rollback_pin_incomplete prev={from_tag_backup} \
                 (auto-rollback may fail if previous images are GC'd)"
            ));
            let _ = worker.state().append_history(&format!(
                "WARNING: rollback image pin incomplete for {from_tag_backup}; \
                 offline rollback may need manual image restore"
            ));
        }
        Err(e) => {
            tracing::warn!(err = %e, prev = %from_tag_backup, "pre-start rollback image pin failed");
            let _ = worker.state().append_audit(&format!(
                "audit: rollback_pin_failed prev={from_tag_backup} err={e}"
            ));
            let _ = worker.state().append_history(&format!(
                "WARNING: rollback image pin failed for {from_tag_backup}: {e}"
            ));
        }
    }

    start_and_finish(&worker, rec, compose, pre).await
}

async fn start_and_finish(
    worker: &Arc<Worker>,
    rec: &PhaseRecorder<'_>,
    compose: &ComposeRunner,
    pre: &preflight::PreflightReport,
) -> Result<()> {
    let target = &pre.target;
    let job_id = &rec.job_id;
    // ----- Start new -----
    rec.enter(Phase::StartingNew, "updater.phase.starting_new")?;
    // Do not run migrations/initializers from a different image than preflight.
    let model = compose.config_json().await?;
    for (service, expected) in [
        ("backend", &pre.backend_image_id),
        ("frontend", &pre.frontend_image_id),
        ("backend-volume-init", &pre.backend_image_id),
    ] {
        let reference = model
            .pointer(&format!("/services/{service}/image"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| UpdaterError::Precondition(format!("missing {service} image")))?;
        if worker.docker().image_id(reference).await? != *expected {
            return Err(UpdaterError::Precondition(format!(
                "{service} does not select the preflight image"
            )));
        }
    }
    if let Some(expected) = &pre.proxy_image_id {
        let reference = model
            .pointer("/services/proxy/image")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| UpdaterError::Precondition("missing proxy image".into()))?;
        if worker.docker().image_id(reference).await? != *expected {
            return Err(UpdaterError::Precondition(
                "proxy does not select the preflight image".into(),
            ));
        }
    }
    let already_ready = matches!(
        probe_one_tick(worker, [&pre.backend_image_id, &pre.frontend_image_id]).await,
        ProbeTick::HardOk { .. }
    );
    if !already_ready {
        let volume_init = compose.init_backend_volumes().await.map_err(|e| {
            UpdaterError::Internal(anyhow::anyhow!(
                "backend volume ownership initialization failed: {e}"
            ))
        })?;
        if !volume_init.ok() {
            return Err(UpdaterError::Internal(anyhow::anyhow!(
                "backend volume ownership initialization failed: {}",
                volume_init.error_summary()
            )));
        }
        tracing::info!(
            output = %volume_init.stdout_tail.trim(),
            diagnostics = %volume_init.stderr_tail.trim(),
            "backend volume ownership and write verification completed"
        );
        let up = compose
            .up_detached_recreate(&["backend", "frontend", "proxy"])
            .await
            .map_err(|e| UpdaterError::Internal(anyhow::anyhow!("compose up new failed: {e}")))?;
        if !up.ok() {
            return Err(UpdaterError::Internal(anyhow::anyhow!(
                "compose up new failed: {}",
                up.error_summary()
            )));
        }
    }
    rec.finish_step_ok()?;

    // ----- Health -----
    rec.enter(Phase::HealthProbing, "updater.phase.health_probing")?;
    let deadline = Duration::from_secs(300u64.max((pre.estimated_seconds as u64) * 3));
    health_probe(
        worker,
        target,
        [&pre.backend_image_id, &pre.frontend_image_id],
        deadline,
    )
    .await?;

    // The selected stack is healthy. Retry only bookkeeping; never restore old
    // data because a state write was temporarily unavailable.
    loop {
        match finish_successful_deploy(worker.state(), &rec.job_id, pre.target_commit_sha.clone()) {
            Ok(()) => break,
            Err(error) => {
                warn!(%error, "deployment is healthy; retrying finalization");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
    // Prune *after* clearing job.current so the just-created snapshot counts
    // toward keep_n (not as an extra in-use slot outside the limit).
    worker.best_effort_prune_snapshots("update_success");
    let _ = worker
        .state()
        .append_history(&format!("job {job_id}: SUCCESS {target}"));
    let _ = worker.state().append_audit(&format!(
        "audit: update_succeeded job={job_id} target={}",
        target.as_str(),
    ));
    info!(job = %job_id, %target, "update succeeded");
    Ok(())
}

/// Restart at the first repeatable operation after the tag switch. Never take a
/// second snapshot of a database that may already have run the new migrations.
pub async fn resume(worker: Arc<Worker>, job_id: &str, rollback: bool) -> Result<()> {
    let mut job = worker.state().read_job(job_id)?;
    if job.kind == crate::state::JobKind::Rollback {
        return rollback::run(
            worker.clone(),
            job_id.into(),
            job.snapshot_id.unwrap_or_default(),
            None,
        )
        .await;
    }
    job.status = JobStatus::Running;
    job.finished_at = None;
    worker.state().write_job(&job)?;
    let rec = PhaseRecorder {
        state: worker.state(),
        job_id: job_id.into(),
        from_version: job.from_version.clone(),
        to_version: job.to_version.clone(),
    };
    let compose = build_compose_runner(&worker).await?;
    let snap = SnapshotManager {
        state: worker.state(),
        pgdata: worker.cli().pgdata.clone(),
    };
    let path = worker.state().prepared_report_path(job_id);
    // v0.5.3 did not persist preflight results. Recover that interrupted update
    // with its existing snapshot instead; remove after the first refactor release.
    let error = if !rollback && path.try_exists()? {
        let attempt = async {
            let pre: preflight::PreflightReport = serde_json::from_slice(&std::fs::read(path)?)?;
            pre.compose.install()?;
            start_and_finish(&worker, &rec, &compose, &pre).await
        }
        .await;
        match attempt {
            Ok(()) => return Ok(()),
            Err(error) => error,
        }
    } else {
        UpdaterError::Precondition("resuming interrupted rollback".into())
    };
    finish_with_rollback(
        &worker,
        &rec,
        &compose,
        &snap,
        job.snapshot_id.as_deref().unwrap_or(""),
        job.from_version.as_ref().map(|v| v.as_str()),
        error,
    )
    .await
}

/// Pin both backend and frontend `:{previous_tag}` images as `*:myriad-rollback`.
///
/// **Pair integrity**: never leave one component pinned and the other not. Both
/// sources must exist first; after tagging, both aliases are re-inspected. A mid-way
/// tag failure aborts without updating `rollback_version` (caller only writes state
/// on `Ok(true)`), and attempts to re-pin the whole pair from `previous_tag` so the
/// slot does not mix two versions.
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
    let pairs: [(&str, String); 2] = [("backend", backend), ("frontend", frontend)];

    // Phase 1: both version tags must exist locally (avoid half-pin).
    for (comp, repo) in &pairs {
        let source = format!("{repo}:{previous_tag}");
        if !worker.docker().image_exists_local(&source).await {
            tracing::warn!(%comp, %source, "rollback pin skipped: image not local");
            return Ok(false);
        }
    }

    // Phase 2: tag both. On any error, re-tag both from previous_tag to heal split slots.
    for (comp, repo) in &pairs {
        let source = format!("{repo}:{previous_tag}");
        if let Err(e) = worker
            .docker()
            .tag_image(&source, repo, ROLLBACK_IMAGE_TAG)
            .await
        {
            tracing::error!(
                %comp,
                %source,
                err = %e,
                "rollback pin failed mid-pair; re-pinning full pair from previous tag"
            );
            let _ = heal_rollback_pair_from_version(worker, &pairs, previous_tag).await;
            return Err(UpdaterError::Docker(format!(
                "pin rollback {comp} ({source}): {e}"
            )));
        }
        info!(
            %comp,
            %source,
            pin = %format!("{repo}:{ROLLBACK_IMAGE_TAG}"),
            "pinned rollback image"
        );
    }

    // Phase 3: both aliases must resolve (detect docker/tag oddities).
    if let Err(missing) = verify_rollback_pair_local(worker, &pairs).await {
        tracing::error!(
            ?missing,
            prev = %previous_tag,
            "rollback pin incomplete after tag; healing from previous tag"
        );
        let _ = heal_rollback_pair_from_version(worker, &pairs, previous_tag).await;
        return Err(UpdaterError::Docker(format!(
            "rollback pin incomplete after tag: missing {missing:?}"
        )));
    }

    Ok(true)
}

/// Ensure both `repo:myriad-rollback` refs exist locally. Returns missing component names.
async fn verify_rollback_pair_local(
    worker: &Arc<Worker>,
    pairs: &[(&str, String); 2],
) -> std::result::Result<(), Vec<String>> {
    let mut missing = Vec::new();
    for (comp, repo) in pairs {
        let pin = format!("{repo}:{ROLLBACK_IMAGE_TAG}");
        if !worker.docker().image_exists_local(&pin).await {
            missing.push((*comp).to_string());
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing)
    }
}

/// Re-tag both components' `previous_tag` → `myriad-rollback` to repair a split slot.
async fn heal_rollback_pair_from_version(
    worker: &Arc<Worker>,
    pairs: &[(&str, String); 2],
    previous_tag: &str,
) -> Result<()> {
    for (comp, repo) in pairs {
        let source = format!("{repo}:{previous_tag}");
        if !worker.docker().image_exists_local(&source).await {
            warn!(%comp, %source, "heal rollback pair: source still missing");
            continue;
        }
        worker
            .docker()
            .tag_image(&source, repo, ROLLBACK_IMAGE_TAG)
            .await
            .map_err(|e| {
                UpdaterError::Docker(format!("heal rollback pin {comp} ({source}): {e}"))
            })?;
        info!(%comp, %source, "healed rollback pin from version tag");
    }
    Ok(())
}

pub(crate) fn restore_compose(state: &crate::state::StateDir, job_id: &str) -> Result<()> {
    let path = state.prepared_report_path(job_id);
    if let Some(bytes) = crate::state::read_existing(&path)? {
        let pre: preflight::PreflightReport = serde_json::from_slice(&bytes)?;
        pre.compose.restore()?;
    }
    Ok(())
}

/// Restore `PROXY_TAG` to the value it had before the update, when the update
/// swapped it. The previous value is persisted in the preflight report written
/// before the swap, so this also works across a crash.
pub(crate) fn restore_proxy_tag(
    state: &crate::state::StateDir,
    job_id: &str,
    env_file: &std::path::Path,
) -> Result<()> {
    let path = state.prepared_report_path(job_id);
    if let Some(bytes) = crate::state::read_existing(&path)? {
        let pre: preflight::PreflightReport = serde_json::from_slice(&bytes)?;
        if let Some(previous) = pre.previous_proxy_tag.as_deref().filter(|s| !s.is_empty()) {
            let mut env = EnvFile::load(env_file)?;
            env.set("PROXY_TAG", previous)?;
            env.save()?;
        }
    }
    Ok(())
}

/// Used by the live flow and startup recovery after durable success.
pub(crate) fn finish_successful_deploy(
    state: &crate::state::StateDir,
    job_id: &str,
    commit_sha: Option<String>,
) -> Result<()> {
    let mut job = state.read_job(job_id)?;
    let target = job
        .to_version
        .clone()
        .ok_or_else(|| UpdaterError::State("successful update has no target".into()))?;
    if job.status != JobStatus::Succeeded {
        if let Some(step) = job.steps.last_mut() {
            step.finish_ok();
        }
        let mut step = crate::state::JobStep::start(Phase::Finalize);
        step.finish_ok();
        job.steps.push(step);
        job.status = JobStatus::Succeeded;
        job.finished_at = Some(Utc::now());
        state.write_job(&job)?;
    }
    let mut current = state.read_updater()?;
    let commit_sha = commit_sha.or_else(|| {
        (current.current_version.as_ref() == Some(&target))
            .then(|| current.current_commit_sha.clone())
            .flatten()
    });
    record_successful_deploy(&mut current, target, commit_sha);
    state.write_updater(&current)?;
    crate::worker::machine::clear_maintenance(state)
}

fn record_successful_deploy(
    state: &mut UpdaterStateFile,
    target: DeployTag,
    target_commit_sha: Option<String>,
) {
    state.current_version = Some(target);
    state.current_commit_sha = target_commit_sha;
    state.updater_version = MyriadVersion::parse(crate::self_version()).ok();
    // A later success supersedes the unacknowledged failure banner.
    state.last_failed_update = None;
    // `rollback_version` deliberately remains unchanged: it identifies the
    // previous known-good build pinned immediately before this deploy started.
}

/// What the pre-swap abort path must restart after a failed update step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreSwapRestoreScope {
    /// Maintenance only; compose stack was not touched.
    None,
    /// frontend/backend were stopped (postgres still running or external).
    App,
    /// postgres was also stopped for snapshot (bundled DB).
    AppAndPostgres,
}

pub(crate) fn pre_swap_needs_postgres(scope: PreSwapRestoreScope, db_external: bool) -> bool {
    matches!(scope, PreSwapRestoreScope::AppAndPostgres) && !db_external
}

/// Best-effort restart of the stack that was running before the update.
///
/// Used by in-flow pre-swap abort and by crash recovery after clearing a stuck
/// pre-swap job. Does not change `MYRIAD_TAG` (still the previous tag).
pub(crate) async fn restore_previous_stack(
    worker: &Arc<Worker>,
    compose: &ComposeRunner,
    scope: PreSwapRestoreScope,
) -> Result<()> {
    if matches!(scope, PreSwapRestoreScope::None) {
        return Ok(());
    }

    let need_pg = pre_swap_needs_postgres(scope, worker.cli().db_mode.is_external());
    if need_pg {
        info!("pre-swap restore: ensuring postgres is up");
        let start_pg = compose.start(&["postgres"]).await;
        let start_ok = start_pg.as_ref().map(|o| o.ok()).unwrap_or(false);
        if !start_ok {
            let up_pg = compose.up_detached(&["postgres"]).await?;
            if !up_pg.ok() {
                let start_summary = start_pg
                    .as_ref()
                    .map(|o| o.error_summary())
                    .unwrap_or_else(|_| "start errored".into());
                return Err(UpdaterError::Internal(anyhow::anyhow!(
                    "pre-swap restore postgres failed: start={start_summary}; up={}",
                    up_pg.error_summary()
                )));
            }
        }
    }

    info!("pre-swap restore: bringing backend/frontend back");
    let up = compose.up_detached(&["backend", "frontend", "proxy"]).await?;
    if !up.ok() {
        return Err(UpdaterError::Internal(anyhow::anyhow!(
            "pre-swap restore app failed: {}",
            up.error_summary()
        )));
    }
    Ok(())
}

/// Pre-swap failure cleanup (spec §7: cleanup → idle).
///
/// Unlike post-swap `finish_with_rollback`, this does **not** restore a pgdata snapshot
/// or rewrite tags — the previous tag is still current. It must still:
/// 1. restart any services we stopped
/// 2. finalize the job
/// 3. clear maintenance (or leave `needs_manual` if restore itself fails)
async fn finish_pre_swap_failure(
    worker: &Arc<Worker>,
    rec: &PhaseRecorder<'_>,
    compose: Option<&ComposeRunner>,
    scope: PreSwapRestoreScope,
    original_err: UpdaterError,
) -> Result<()> {
    error!(
        err = %original_err,
        ?scope,
        "pre-swap failure; restoring previous stack before leaving update"
    );
    let _ = worker.state().append_history(&format!(
        "job {}: PRE_SWAP_FAIL scope={scope:?} err={original_err}",
        rec.job_id
    ));

    let mut restore_err = restore_compose(worker.state(), &rec.job_id)
        .err()
        .map(|e| e.to_string());
    match compose {
        _ if restore_err.is_some() => {}
        Some(c) => {
            if let Err(e) = restore_previous_stack(worker, c, scope).await {
                error!(err = %e, "pre-swap stack restore failed");
                restore_err = Some(e.to_string());
            }
        }
        None if !matches!(scope, PreSwapRestoreScope::None) => {
            restore_err = Some("compose runner unavailable; cannot restart services".into());
        }
        None => {}
    }

    // Record failure context for UI / ops (same shape as post-swap auto-rollback).
    if let Ok(mut st) = worker.state().read_updater() {
        st.last_failed_update = Some(crate::state::FailedUpdate {
            from_version: rec.from_version.clone(),
            to_version: rec.to_version.clone(),
            at: Utc::now(),
            reason: original_err.to_string(),
            job_id: rec.job_id.clone(),
        });
        let _ = worker.state().write_updater(&st);
    }

    if let Some(re) = restore_err {
        let _ = rec.finish_step_err(format!("pre-swap restore failed: {re}"));
        let _ = rec.finalize(JobStatus::NeedsManual);
        if let Ok(mut m) = worker.state().read_maintenance() {
            m.active = true;
            m.phase = Phase::NeedsManual;
            m.message_key = "updater.phase.needs_manual".into();
            m.job_id = Some(rec.job_id.clone());
            m.bump_heartbeat();
            let _ = worker.state().write_maintenance(&m);
        }
        // Free old extras; rescue/in-use snap stays protected by needs_manual.
        worker.best_effort_prune_snapshots("pre_swap_needs_manual");
        let _ = worker.state().append_history(&format!(
            "job {}: NEEDS_MANUAL — original={original_err}; pre_swap_restore={re}",
            rec.job_id
        ));
        let _ = worker.state().append_audit(&format!(
            "audit: pre_swap_restore_failed job={} err={original_err} restore={re}",
            rec.job_id
        ));
        return Err(UpdaterError::Precondition(format!(
            "update failed ({original_err}); restoring previous stack also failed ({re})"
        )));
    }

    let _ = rec.finalize(JobStatus::Failed);
    if let Err(e) = crate::worker::machine::clear_maintenance(worker.state()) {
        warn!(err = %e, "pre-swap cleanup: clear_maintenance failed after stack restored");
    }
    // Snapshot may already exist after mid-update failure; without prune here,
    // failed updates pile up unbounded backups until a later success/prefs save.
    worker.best_effort_prune_snapshots("pre_swap_fail");
    let _ = worker.state().append_history(&format!(
        "job {}: PRE_SWAP_CLEANUP_OK ({original_err})",
        rec.job_id
    ));
    let _ = worker.state().append_audit(&format!(
        "audit: pre_swap_cleanup_ok job={} err={original_err}",
        rec.job_id
    ));
    Err(original_err)
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
    // Preserve maintenance throughout destructive rollback, including recovery
    // of jobs created by an older updater that probed with maintenance disabled.
    if let Ok(mut m) = worker.state().read_maintenance()
        && !m.active
    {
        m.active = true;
        m.phase = Phase::RollbackInProgress;
        m.message_key = "updater.phase.rollback".into();
        m.job_id = Some(rec.job_id.clone());
        m.bump_heartbeat();
        let _ = worker.state().write_maintenance(&m);
    }

    error!(err = %original_err, "rollback triggered");
    let rb_result =
        rollback::execute_inline(worker.clone(), rec, compose, snap, snapshot_id, from_tag).await;
    match rb_result {
        Ok(restored) => {
            // Best-effort state updates: services are already on the previous stack.
            // A failed write_updater must not leave maintenance active forever.
            let _ = rec.finalize(JobStatus::Failed);
            if let Ok(mut st) = worker.state().read_updater() {
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
                if let Err(e) = worker.state().write_updater(&st) {
                    warn!(err = %e, "auto-rollback: write_updater failed after services restored");
                }
            }
            if let Err(e) = crate::worker::machine::clear_maintenance(worker.state()) {
                warn!(err = %e, "auto-rollback: clear_maintenance failed");
            }
            // Rollback restored pgdata; prune extras so failed updates cannot
            // leave unbounded snapshot piles (rescue snap no longer in-use).
            worker.best_effort_prune_snapshots("post_swap_rollback_ok");
            let rb_ok = format!("job {}: ROLLBACK_OK ({original_err})", rec.job_id);
            let _ = worker.state().append_history(&rb_ok);
            let _ = worker.state().append_audit(&format!(
                "audit: auto_rollback_ok job={} err={original_err}",
                rec.job_id
            ));
            Err(original_err)
        }
        Err(rb_err) => {
            let _ = rec.finish_step_err(format!("rollback failed: {rb_err}"));
            let _ = rec.finalize(JobStatus::NeedsManual);
            if let Ok(mut m) = worker.state().read_maintenance() {
                m.active = true;
                m.phase = Phase::NeedsManual;
                m.message_key = "updater.phase.needs_manual".into();
                m.job_id = Some(rec.job_id.clone());
                m.bump_heartbeat();
                let _ = worker.state().write_maintenance(&m);
            }
            // Persist last_failed even when rollback itself failed.
            if let Ok(mut st) = worker.state().read_updater() {
                st.last_failed_update = Some(crate::state::FailedUpdate {
                    from_version: rec.from_version.clone(),
                    to_version: rec.to_version.clone(),
                    at: Utc::now(),
                    reason: format!("{original_err}; rollback also failed: {rb_err}"),
                    job_id: rec.job_id.clone(),
                });
                let _ = worker.state().write_updater(&st);
            }
            // Still free unrelated old backups; rescue snapshot stays protected
            // via needs_manual / in_use_reason.
            worker.best_effort_prune_snapshots("post_swap_needs_manual");
            let _ = worker.state().append_history(&format!(
                "job {}: NEEDS_MANUAL — original={original_err}; rollback={rb_err}",
                rec.job_id
            ));
            Err(UpdaterError::Precondition(format!(
                "update failed ({original_err}); rollback also failed ({rb_err})"
            )))
        }
    }
}

/// Keep the failure visible without changing the user's update preference.
fn record_preflight_failure(
    store: &crate::state::StateDir,
    rec: &PhaseRecorder<'_>,
    err: &UpdaterError,
) {
    if let Ok(mut state) = store.read_updater() {
        state.last_failed_update = Some(crate::state::FailedUpdate {
            from_version: rec.from_version.clone(),
            to_version: rec.to_version.clone(),
            at: Utc::now(),
            reason: format!("preflight: {err}"),
            job_id: rec.job_id.clone(),
        });
        if let Err(error) = store.write_updater(&state) {
            warn!(%error, "cannot record preflight failure");
        }
    }
}

pub(crate) async fn build_compose_runner_pub(worker: &Arc<Worker>) -> Result<ComposeRunner> {
    build_compose_runner(worker).await
}

async fn build_compose_runner(worker: &Arc<Worker>) -> Result<ComposeRunner> {
    // Re-discover compose files at use time so panel renames after boot are visible.
    // Binary preference still falls back to startup env-probe when live detect fails.
    let live = crate::probe::compose::probe(&worker.cli().compose_dir).await;
    let probe_path = worker.state().root().join("env-probe.json");
    let boot: Option<crate::probe::EnvProbe> = std::fs::read(&probe_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());

    let binary: ComposeBinary = live
        .binary
        .or_else(|| boot.as_ref().and_then(|p| p.compose.binary))
        .ok_or_else(|| UpdaterError::Internal(anyhow::anyhow!("compose binary unavailable")))?;

    let compose_files = if !live.compose_files.is_empty() {
        live.compose_files
    } else if let Some(p) = boot.as_ref() {
        p.compose.compose_files.clone()
    } else {
        return Err(UpdaterError::Internal(anyhow::anyhow!(
            "no compose files found under {}",
            worker.cli().compose_dir.display()
        )));
    };

    let project = std::env::var("COMPOSE_PROJECT_NAME").unwrap_or_else(|_| "myriad".into());
    let compose_base = compose_files
        .first()
        .and_then(|file| file.parent())
        .unwrap_or(&worker.cli().compose_dir);
    let host_project_directory = worker
        .docker()
        .resolve_host_bind_source(compose_base)
        .await?;
    // Release builds resolve this to the fixed read-only secret mount. Only
    // debug builds accept the dev-compose override.
    let guard_env_file = crate::docker::compose::guard_env_file_path();
    if !guard_env_file.is_absolute() || !guard_env_file.is_file() {
        return Err(UpdaterError::Precondition(format!(
            "invalid host-owned Guard policy file: {}",
            guard_env_file.display()
        )));
    }
    crate::docker::compose::validate_guard_policy_file(&guard_env_file)?;
    Ok(ComposeRunner::new(
        binary,
        project,
        compose_files,
        worker.cli().env_file.clone(),
        guard_env_file,
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

fn swap_proxy_tag(worker: &Arc<Worker>, new_tag: &str) -> Result<()> {
    let mut env = EnvFile::load(&worker.cli().env_file)?;
    env.set("PROXY_TAG", new_tag)?;
    env.save()
}

/// Private health probe after starting new backend/frontend.
///
/// Keep public maintenance active until both images and their local healthchecks
/// pass. Frontend health is observed through Docker, so no public bypass or new
/// proxy protocol is required for older deployments.
async fn health_probe(
    worker: &Arc<Worker>,
    target: &DeployTag,
    images: [&str; 2],
    deadline: Duration,
) -> Result<()> {
    let start = std::time::Instant::now();
    const OK_STREAK_NEED: u32 = 2;
    let phase_label = "private_health";

    let mut ok_streak = 0u32;
    let mut last_diag = String::new();
    let mut attempts: u32 = 0;

    while start.elapsed() < deadline {
        tokio::time::sleep(Duration::from_secs(2)).await;
        attempts += 1;
        let elapsed = start.elapsed();

        let tick = probe_one_tick(worker, images).await;
        let diag = match &tick {
            ProbeTick::HardOk { detail, pass_kind } => {
                ok_streak += 1;
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
            ProbeTick::NotReady { detail } => {
                ok_streak = 0;
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

enum ProbeTick {
    HardOk {
        detail: String,
        pass_kind: &'static str,
    },
    NotReady {
        detail: String,
    },
}

async fn probe_one_tick(worker: &Arc<Worker>, images: [&str; 2]) -> ProbeTick {
    match worker.docker().workers_healthy().await {
        Ok(true) => {}
        Ok(false) => {
            return ProbeTick::NotReady {
                detail: "worker health/image not ready".into(),
            };
        }
        Err(error) => {
            return ProbeTick::NotReady {
                detail: format!("worker probe: {error}"),
            };
        }
    }
    let docker = worker.docker();
    for (name, image) in ["myriad-backend", "myriad-frontend"]
        .into_iter()
        .zip(images)
    {
        match docker.container_ready(name, image).await {
            Ok(true) => (),
            other => {
                return ProbeTick::NotReady {
                    detail: format!("{name}: expected image {image}, health={other:?}"),
                };
            }
        }
    }

    let be = docker
        .http_probe("http://backend:1103/health", Duration::from_secs(10))
        .await;
    let (be_code, be_body) = match be {
        Ok(v) => v,
        Err(e) => {
            return ProbeTick::NotReady {
                detail: format!("backend unreachable ({e})"),
            };
        }
    };
    if be_code != 200 {
        return ProbeTick::NotReady {
            detail: format!(
                "backend HTTP {be_code}: {}",
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
    let routes = backend_routes_full(&json);
    if !backend_business_ready(&json) {
        return ProbeTick::NotReady {
            detail: format!(
                "backend up but db_connected={db} migrations_applied={mig} routes_full={routes} \
                 storage_writable={storage} version={version:?} commit={commit_sha:?}"
            ),
        };
    }

    match docker.http_probe("http://proxy:80/healthz", Duration::from_secs(5)).await {
        Ok((200, _)) => ProbeTick::HardOk { detail: "target images, local healthchecks, backend readiness and proxy liveness verified; maintenance retained".into(), pass_kind: "private_health" },
        other => ProbeTick::NotReady { detail: format!("proxy liveness: {other:?}") },
    }
}

#[cfg(test)]
mod health_match_tests {
    use super::*;
    use crate::version::DeployTag;

    #[test]
    fn preflight_failure_keeps_auto_install_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let state = crate::state::StateDir::open(dir.path()).unwrap();
        state
            .write_updater(&UpdaterStateFile {
                auto_install: true,
                ..Default::default()
            })
            .unwrap();
        let rec = PhaseRecorder {
            state: &state,
            job_id: "attempt".into(),
            from_version: None,
            to_version: DeployTag::parse("v1.2.3").ok(),
        };
        record_preflight_failure(
            &state,
            &rec,
            &UpdaterError::Docker("registry unavailable".into()),
        );
        let saved = state.read_updater().unwrap();
        assert!(saved.auto_install);
        assert_eq!(saved.last_failed_update.unwrap().job_id, "attempt");
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
        assert!(state.last_failed_update.is_none());
    }

    #[test]
    fn successful_deploy_clears_last_failed_banner() {
        let mut state = UpdaterStateFile {
            last_failed_update: Some(crate::state::FailedUpdate {
                from_version: DeployTag::parse("v0.3.32").ok(),
                to_version: DeployTag::parse("v0.3.33").ok(),
                at: Utc::now(),
                reason: "preflight".into(),
                job_id: "job-1".into(),
            }),
            ..UpdaterStateFile::default()
        };
        record_successful_deploy(&mut state, DeployTag::parse("v0.3.34").unwrap(), None);
        assert!(state.last_failed_update.is_none());
    }

    #[test]
    fn pre_swap_restore_scope_includes_postgres_only_when_needed() {
        assert!(!pre_swap_needs_postgres(PreSwapRestoreScope::None, false));
        assert!(!pre_swap_needs_postgres(PreSwapRestoreScope::App, false));
        assert!(pre_swap_needs_postgres(
            PreSwapRestoreScope::AppAndPostgres,
            false
        ));
        assert!(!pre_swap_needs_postgres(
            PreSwapRestoreScope::AppAndPostgres,
            true
        ));
    }

    #[test]
    fn classify_update_failure_respects_flow_ctx() {
        let mut flow = UpdateFlowCtx::new();
        assert_eq!(
            classify_update_failure(&flow),
            UpdateFailureKind::PreSwap(PreSwapRestoreScope::None)
        );
        flow.scope = PreSwapRestoreScope::AppAndPostgres;
        assert_eq!(
            classify_update_failure(&flow),
            UpdateFailureKind::PreSwap(PreSwapRestoreScope::AppAndPostgres)
        );
        // swap_tag not yet successful → still pre-swap even if scope is App
        flow.scope = PreSwapRestoreScope::App;
        assert_eq!(
            classify_update_failure(&flow),
            UpdateFailureKind::PreSwap(PreSwapRestoreScope::App)
        );
        flow.post_swap = true;
        assert_eq!(classify_update_failure(&flow), UpdateFailureKind::PostSwap);
    }
}
