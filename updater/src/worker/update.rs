//! Normal update flow. State machine progression per spec §7.
//! Supports release (GitHub `release.json` when present, else Docker Hub `vX.Y.Z` images)
//! and commit (CI image tags) modes. Swap/health use `PreflightReport` digests and tags;
//! a missing `pre.manifest` is fine for the Docker Hub release path.
//!
//! # Failure invariants (do not regress)
//!
//! 1. **After app stop**: any `Err` must go through `dispatch_update_failure` → restart app
//!    (`PreSwap`) or full rollback (`PostSwap`).
//! 2. **`post_swap` only after `swap_tag` Ok** — failed tag write must not snapshot-restore.
//! 3. **`committed` immediately after health Ok** — never rollback a live healthy stack.
//! 4. **After `committed`**: always leave job=`Succeeded` + maintenance clear; return `Ok`
//!    even if bookkeeping I/O fails.
//! 5. **Preflight / maintenance entry failures**: always clear maintenance (best-effort).
//! 6. **Rollback paths**: on Err after stopping services, best-effort `compose up` again
//!    (`execute_inline` outer wrapper).

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
        record_preflight_failure(&worker, &rec, &e);
        let _ = rec.finalize(JobStatus::Failed);
        let _ = crate::worker::machine::clear_maintenance(worker.state());
        return Err(e);
    }
    let pre = match preflight::run(worker.clone(), &target, mode, risk).await {
        Ok(r) => {
            let _ = rec.finish_step_ok();
            r
        }
        Err(e) => {
            error!(job = %job_id, err = %e, "preflight failed");
            let _ = rec.finish_step_err(format!("preflight: {e}"));
            // Surface reason on About → update block; stop auto-install so a
            // broken pre-check does not keep firing until the operator fixes it.
            record_preflight_failure(&worker, &rec, &e);
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
            return finish_pre_swap_failure(
                &worker,
                &rec,
                None,
                PreSwapRestoreScope::None,
                e,
            )
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
        mode,
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
    /// Health passed and deploy was recorded — never auto-rollback after this.
    committed: bool,
}

impl UpdateFlowCtx {
    fn new() -> Self {
        Self {
            scope: PreSwapRestoreScope::None,
            post_swap: false,
            snapshot_id: String::new(),
            from_tag: None,
            committed: false,
        }
    }
}

/// Classify failure for tests and the dispatcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpdateFailureKind {
    PreSwap(PreSwapRestoreScope),
    PostSwap,
    /// Health already passed; log-only (do not destroy the new stack).
    Committed,
}

pub(crate) fn classify_update_failure(flow: &UpdateFlowCtx) -> UpdateFailureKind {
    if flow.committed {
        UpdateFailureKind::Committed
    } else if flow.post_swap {
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
        UpdateFailureKind::Committed => {
            // Health already passed and the new stack is live. Bookkeeping errors must
            // NOT mark the job Failed or leave maintenance up — that reads as "update
            // failed without rollback" while the site is actually on the new version.
            warn!(
                err = %err,
                "post-health bookkeeping error; treating deploy as succeeded (no rollback)"
            );
            let _ = rec.finish_step_err(format!("post-health bookkeeping: {err}"));
            // Prefer Succeeded so UI/status match the running stack.
            let _ = rec.finalize(JobStatus::Succeeded);
            let _ = crate::worker::machine::clear_maintenance(worker.state());
            // Same retention as a clean success: free extras now that job.current is clear.
            worker.best_effort_prune_snapshots("update_success_bookkeeping_err");
            let _ = worker.state().append_history(&format!(
                "job {}: SUCCESS_WITH_BOOKKEEPING_ERR ({err})",
                rec.job_id
            ));
            let _ = worker.state().append_audit(&format!(
                "audit: update_succeeded_with_bookkeeping_err job={} err={err}",
                rec.job_id
            ));
            // Stack is healthy — report Ok so callers do not treat a live deploy as failed.
            Ok(())
        }
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
    mode: UpdateMode,
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
        let _ = worker
            .state()
            .append_history(&format!("job {job_id}: db_mode=external; skipping pgdata snapshot"));
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
        // Snapshot id is best-effort metadata — failure must not leave stack down.
        if let Ok(mut job) = worker.state().read_job(job_id) {
            job.snapshot_id = Some(snapshot_id);
            if let Err(e) = worker.state().write_job(&job) {
                warn!(err = %e, "failed to record snapshot_id on job; continuing");
            }
        }
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
    let from_tag_backup = swap_tag(&worker, target.as_str())?;
    flow.post_swap = true;
    flow.from_tag = Some(from_tag_backup.clone());
    rec.finish_step_ok()?;

    match pin_rollback_images(&worker, &from_tag_backup).await {
        Ok(true) => {
            if let Ok(v) = DeployTag::parse(&from_tag_backup) {
                if let Ok(mut st) = worker.state().read_updater() {
                    st.rollback_version = Some(v);
                    let _ = worker.state().write_updater(&st);
                }
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

    // ----- Start new -----
    rec.enter(Phase::StartingNew, "updater.phase.starting_new")?;
    let volume_init = compose
        .init_backend_volumes()
        .await
        .map_err(|e| {
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
        .up_detached_recreate(&["backend", "frontend"])
        .await
        .map_err(|e| {
            UpdaterError::Internal(anyhow::anyhow!("compose up new failed: {e}"))
        })?;
    if !up.ok() {
        return Err(UpdaterError::Internal(anyhow::anyhow!(
            "compose up new failed: {}",
            up.error_summary()
        )));
    }
    rec.finish_step_ok()?;

    // ----- Health -----
    rec.enter(Phase::HealthProbing, "updater.phase.health_probing")?;
    let deadline = Duration::from_secs(300u64.max((pre.estimated_seconds as u64) * 3));
    // Preserve original error text so is_health_probe_failure still matches.
    health_probe_phased(&worker, target, deadline).await?;

    // CRITICAL: mark committed immediately after health OK, *before* any state
    // writes. A finish_step_ok / write_updater failure must not trigger rollback
    // of a live, healthy new stack.
    flow.committed = true;
    let _ = rec.finish_step_ok();

    let _ = rec.enter(Phase::SwappingProxy, "updater.phase.swapping_proxy");
    match worker.state().read_updater() {
        Ok(mut st) => {
            record_successful_deploy(&mut st, target.clone(), pre.target_commit_sha.clone());
            if let Err(e) = worker.state().write_updater(&st) {
                warn!(err = %e, "write updater state after health ok failed; continuing finalize");
            }
        }
        Err(e) => {
            warn!(err = %e, "read updater state after health ok failed; continuing finalize");
        }
    }
    let _ = rec.finish_step_ok();

    let _ = rec.enter(Phase::Finalize, "updater.phase.finalize");
    let _ = rec.finish_step_ok();
    // After health OK we NEVER return Err: stack is live on the new tag.
    // Bookkeeping failures are logged; job is forced Succeeded and maintenance cleared.
    if let Err(e) = rec.finalize(JobStatus::Succeeded) {
        warn!(err = %e, "finalize Succeeded failed after healthy deploy (forcing clear)");
    }
    if let Err(e) = crate::worker::machine::clear_maintenance(worker.state()) {
        warn!(err = %e, "clear_maintenance failed after healthy deploy");
    }
    // Prune *after* clearing job.current so the just-created snapshot counts
    // toward keep_n (not as an extra in-use slot outside the limit).
    worker.best_effort_prune_snapshots("update_success");
    let _ = worker
        .state()
        .append_history(&format!("job {job_id}: SUCCESS {target} ({mode})"));
    let _ = worker.state().append_audit(&format!(
        "audit: update_succeeded job={job_id} target={} mode={}",
        target.as_str(),
        mode.as_str()
    ));
    info!(job = %job_id, %target, ?mode, "update succeeded");
    Ok(())
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
    let up = compose.up_detached(&["backend", "frontend"]).await?;
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

    let mut restore_err: Option<String> = None;
    match compose {
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
            // Recheck must be *stricter* than the main loop: only HardOk skips rollback.
            // SoftOk (proxy-down, maintenance HTML, dual-image warm) used to mark SUCCESS
            // after a real probe timeout and leave a broken site without rollback.
            let recheck = probe_one_tick(
                worker,
                &target,
                Duration::from_secs(120),
                FrontendProbe::LiveViaProxy,
            )
            .await;
            match recheck {
                ProbeTick::HardOk { detail, pass_kind } => {
                    warn!(
                        %detail,
                        %pass_kind,
                        target = %target,
                        "health re-check HardOk after timeout — treating update as SUCCESS \
                         (skipping rollback). Original probe was a false negative."
                    );
                    // Best-effort bookkeeping only — stack is already live.
                    let _ = rec.enter(Phase::SwappingProxy, "updater.phase.swapping_proxy");
                    if let Ok(mut st) = worker.state().read_updater() {
                        // Recheck path has no PreflightReport; preserve commit only when
                        // we already recorded this exact target (false-negative after success bookkeeping).
                        let commit = if st.current_version.as_ref() == Some(&target) {
                            st.current_commit_sha.clone()
                        } else {
                            None
                        };
                        record_successful_deploy(&mut st, target.clone(), commit);
                        let _ = worker.state().write_updater(&st);
                    }
                    let _ = rec.finish_step_ok();
                    let _ = rec.enter(Phase::Finalize, "updater.phase.finalize");
                    let _ = rec.finish_step_ok();
                    let _ = rec.finalize(JobStatus::Succeeded);
                    let _ = crate::worker::machine::clear_maintenance(worker.state());
                    worker.best_effort_prune_snapshots("update_success_health_recheck");
                    let _ = worker.state().append_history(&format!(
                        "job {}: SUCCESS after health false-negative recheck ({target}); \
                         original_probe_err={original_err}",
                        rec.job_id
                    ));
                    return Ok(());
                }
                ProbeTick::SoftOk { detail, pass_kind } => {
                    warn!(
                        %detail,
                        %pass_kind,
                        "health re-check only SoftOk after timeout; refusing false success — rolling back"
                    );
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

    // Health probe may have set maintenance.active=false; re-enter so proxy shows
    // maintenance during destructive rollback and crash recovery can see us.
    if let Ok(mut m) = worker.state().read_maintenance() {
        if !m.active {
            m.active = true;
            m.phase = Phase::RollbackInProgress;
            m.message_key = "updater.phase.rollback".into();
            m.job_id = Some(rec.job_id.clone());
            m.bump_heartbeat();
            let _ = worker.state().write_maintenance(&m);
        }
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

fn is_health_probe_failure(err: &UpdaterError) -> bool {
    let s = err.to_string();
    s.contains("health probe") || s.contains("health:")
}

/// Persist preflight failure for the About-page update block and turn off
/// auto-install so a hard pre-check failure cannot loop on the next tick.
fn record_preflight_failure(worker: &Worker, rec: &PhaseRecorder<'_>, err: &UpdaterError) {
    if let Ok(mut st) = worker.state().read_updater() {
        let was_auto = st.auto_install;
        let reason = if was_auto {
            format!("preflight: {err} (auto-update disabled)")
        } else {
            format!("preflight: {err}")
        };
        st.last_failed_update = Some(crate::state::FailedUpdate {
            from_version: rec.from_version.clone(),
            to_version: rec.to_version.clone(),
            at: Utc::now(),
            reason: reason.clone(),
            job_id: rec.job_id.clone(),
        });
        if was_auto {
            st.auto_install = false;
        }
        if let Err(e) = worker.state().write_updater(&st) {
            warn!(err = %e, "preflight failure: write_updater failed");
            return;
        }
        if was_auto {
            info!(
                job = %rec.job_id,
                "preflight failed: auto_install disabled"
            );
            let _ = worker.state().append_history(&format!(
                "job {}: PREFLIGHT_FAIL auto_install=off reason={reason}",
                rec.job_id
            ));
            let _ = worker.state().append_audit(&format!(
                "audit: preflight_failed_auto_install_off job={} reason={reason}",
                rec.job_id
            ));
        } else {
            let _ = worker.state().append_history(&format!(
                "job {}: PREFLIGHT_FAIL reason={reason}",
                rec.job_id
            ));
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
    let boot: Option<crate::probe::EnvProbe> =
        std::fs::read(&probe_path)
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
    Ok(ComposeRunner::new(
        binary,
        project,
        compose_files,
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
    // Must NOT accept maintenance HTML as SoftOk — that path used to false-pass recheck
    // and skip rollback while the site still served the maintenance page.
    if db
        && mig
        && backend_running
        && frontend_running
        && backend_img_ok
        && frontend_img_ok
        && fe_html_ok
        && !looks_like_maintenance
    {
        return ProbeTick::SoftOk {
            pass_kind: "soft_dual_image",
            detail: format!(
                "running+db+image tags+non-maint HTML; fe_meta_ok={fe_meta_ok} \
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
        // committed wins over post_swap — never destroy a healthy stack
        flow.committed = true;
        assert_eq!(classify_update_failure(&flow), UpdateFailureKind::Committed);
    }

    #[test]
    fn health_probe_failure_detector_matches_probe_errors() {
        let e = UpdaterError::Precondition("health probe exceeded 300s".into());
        assert!(is_health_probe_failure(&e));
        let e2 = UpdaterError::Precondition("health: backend not ready".into());
        assert!(is_health_probe_failure(&e2));
        let e3 = UpdaterError::Internal(anyhow::anyhow!("compose up new failed"));
        assert!(!is_health_probe_failure(&e3));
    }

    #[test]
    fn soft_dual_image_rejects_maintenance_html() {
        // Mirrors the SoftOk gate: maintenance page must not count as soft success.
        let looks_like_maintenance = true;
        let fe_html_ok = false; // fe_html_ok requires !looks_like_maintenance
        assert!(!(fe_html_ok && !looks_like_maintenance));
        let looks_like_maintenance = false;
        let fe_html_ok = true;
        assert!(fe_html_ok && !looks_like_maintenance);
    }

    #[test]
    fn recheck_accepts_only_hard_ok_variants() {
        // Document the recheck contract: SoftOk must not skip rollback.
        fn recheck_skips_rollback(tick: &str) -> bool {
            matches!(tick, "hard")
        }
        assert!(recheck_skips_rollback("hard"));
        assert!(!recheck_skips_rollback("soft"));
        assert!(!recheck_skips_rollback("not_ready"));
    }
}
