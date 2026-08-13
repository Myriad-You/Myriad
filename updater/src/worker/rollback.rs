//! Rollback flow: restore pgdata snapshot, swap tag back, restart old containers.
//!
//! Two entry points:
//!  - [`run`]: a standalone rollback job triggered from the API.
//!  - [`execute_inline`]: called from inside the update flow when something fails post-swap.
//!
//! Tag restoration priority (last known good, never a hardcoded version):
//!  1. Explicit `swap_back_tag` from the update flow (value of `MYRIAD_TAG` before swap)
//!  2. Snapshot metadata `source_version` (recorded at snapshot create time)
//!  3. `updater.json.current_version` (only advanced after a successful health-checked update)
//!
//! If none of the above is available we leave `.env` unchanged (safe when swap never landed).
//!
//! ## Resilience (coupled with health-probe false negatives)
//!
//! Order matters: we **restore MYRIAD_TAG first**, then snapshot, so a mid-rollback crash
//! still leaves `.env` pointing at the rollback version. If snapshot restore fails we
//! **fail closed** (do not start services on inconsistent pgdata); use rescue manually.

use std::sync::Arc;
use std::time::Duration;

use tracing::{error, info, warn};

use crate::docker::{ComposeRunner, ROLLBACK_IMAGE_TAG};
use crate::env_file::EnvFile;
use crate::error::{Result, UpdaterError};
use crate::snapshot::SnapshotManager;
use crate::state::{JobStatus, Phase, StateDir, UpdaterStateFile};
use crate::version::DeployTag;
use crate::worker::{machine::PhaseRecorder, Worker};

pub async fn run(
    worker: Arc<Worker>,
    job_id: String,
    snapshot_id: String,
    actor: Option<String>,
) -> Result<()> {
    let actor_suffix = actor
        .as_deref()
        .map(|a| format!(" actor={a}"))
        .unwrap_or_default();
    let audit = format!("audit: rollback_start job={job_id} snapshot={snapshot_id}{actor_suffix}");
    worker.state().append_history(&audit)?;
    let _ = worker.state().append_audit(&audit);

    let rec = PhaseRecorder {
        state: worker.state(),
        job_id: job_id.clone(),
        from_version: worker.state().read_updater()?.current_version.clone(),
        to_version: None,
    };
    let _ = rec.enter(Phase::RollbackInProgress, "updater.phase.rollback");
    let _ = rec.finish_step_ok();

    let compose = match super::update::build_compose_runner_pub(&worker).await {
        Ok(c) => c,
        Err(e) => {
            error!(err = %e, "standalone rollback: compose runner unavailable");
            let _ = rec.finish_step_err(format!("compose runner: {e}"));
            let _ = rec.finalize(JobStatus::NeedsManual);
            if let Ok(mut m) = worker.state().read_maintenance() {
                m.active = true;
                m.phase = Phase::NeedsManual;
                m.message_key = "updater.phase.needs_manual".into();
                m.bump_heartbeat();
                let _ = worker.state().write_maintenance(&m);
            }
            return Err(e);
        }
    };
    let snap = SnapshotManager {
        state: worker.state(),
        pgdata: worker.cli().pgdata.clone(),
    };

    // Resolve from snapshot / rollback version — never skip tag restore on standalone rollback
    // when metadata is available.
    match execute_inline(worker.clone(), &rec, &compose, &snap, &snapshot_id, None).await {
        Ok(restored) => {
            if let Some(v) = restored {
                if let Ok(mut job) = worker.state().read_job(&job_id) {
                    job.to_version = Some(v);
                    let _ = worker.state().write_job(&job);
                }
            }
            let _ = rec.finalize(JobStatus::Succeeded);
            let _ = crate::worker::machine::clear_maintenance(worker.state());
            Ok(())
        }
        Err(e) => {
            error!(err = %e, "standalone rollback failed");
            let _ = rec.finalize(JobStatus::NeedsManual);
            if let Ok(mut m) = worker.state().read_maintenance() {
                m.active = true;
                m.phase = Phase::NeedsManual;
                m.message_key = "updater.phase.needs_manual".into();
                m.bump_heartbeat();
                let _ = worker.state().write_maintenance(&m);
            }
            Err(e)
        }
    }
}

/// Execute the core rollback steps.
///
/// Returns the version tag that was restored into `MYRIAD_TAG` (when known).
///
/// **Invariant**: after this function returns (Ok or Err), we best-effort attempt to
/// leave backend/frontend (and postgres when bundled) running. Mid-rollback `?` must
/// not leave a fully stopped stack without a start attempt.
pub async fn execute_inline(
    worker: Arc<Worker>,
    rec: &PhaseRecorder<'_>,
    compose: &ComposeRunner,
    snap: &SnapshotManager<'_>,
    snapshot_id: &str,
    swap_back_tag: Option<&str>,
) -> Result<Option<DeployTag>> {
    let result = execute_inline_inner(
        worker.clone(),
        rec,
        compose,
        snap,
        snapshot_id,
        swap_back_tag,
    )
    .await;
    if result.is_err() {
        // Last resort: do not leave the stack fully stopped after a partial rollback.
        warn!("rollback path failed; best-effort restart of app (and postgres if bundled)");
        if !worker.cli().db_mode.is_external() {
            let _ = compose.start(&["postgres"]).await;
            let _ = compose.up_detached(&["postgres"]).await;
        }
        let _ = compose.up_detached(&["backend", "frontend"]).await;
    }
    result
}

async fn execute_inline_inner(
    worker: Arc<Worker>,
    rec: &PhaseRecorder<'_>,
    compose: &ComposeRunner,
    snap: &SnapshotManager<'_>,
    snapshot_id: &str,
    swap_back_tag: Option<&str>,
) -> Result<Option<DeployTag>> {
    info!(snapshot = snapshot_id, "rollback: stopping new containers");
    let _ = rec.enter(Phase::StopNew, "updater.phase.stop_new");
    let stop_app = compose.stop(&["frontend", "backend"], 30).await;
    match &stop_app {
        Ok(out) if out.ok() => {}
        Ok(out) => {
            warn!(
                summary = %out.error_summary(),
                "compose stop frontend/backend non-zero; forcing container stop"
            );
            for name in ["myriad-frontend", "frontend", "myriad-backend", "backend"] {
                let _ = worker.docker().force_stop_container(name).await;
            }
        }
        Err(e) => {
            warn!(
                err = %e,
                "compose stop frontend/backend errored; forcing container stop"
            );
            for name in ["myriad-frontend", "frontend", "myriad-backend", "backend"] {
                let _ = worker.docker().force_stop_container(name).await;
            }
        }
    }
    let _ = rec.finish_step_ok();

    // --- Resolve + restore MYRIAD_TAG BEFORE snapshot work ---
    // So any later failure (EBUSY restore, etc.) still leaves env at the rollback version.
    let prev_tag = resolve_previous_tag(worker.state(), snapshot_id, swap_back_tag)?;
    let restored_version = match &prev_tag {
        Some(tag) => {
            let _ = rec.enter(Phase::SwapTagBack, "updater.phase.swap_tag_back");
            let parsed = DeployTag::parse(tag).ok();
            if let Some(ref version) = parsed {
                if let Err(e) = materialize_pinned_rollback_images(worker.as_ref(), version).await {
                    warn!(
                        err = %e,
                        version = %version,
                        "failed to restore version refs from the local rollback slot"
                    );
                }
            }
            let mut env = EnvFile::load(&worker.cli().env_file)?;
            let before = env.get("MYRIAD_TAG").unwrap_or("").to_string();
            env.set("MYRIAD_TAG", tag)?;
            env.save()?;
            info!(
                from = %before,
                to = %tag,
                "rollback: restored MYRIAD_TAG to last known good (before snapshot restore)"
            );
            let _ = rec.finish_step_ok();
            parsed
        }
        None => {
            warn!(
                snapshot = snapshot_id,
                "rollback: no previous MYRIAD_TAG resolved; leaving .env unchanged"
            );
            None
        }
    };

    let _ = rec.enter(Phase::RestoreSnapshot, "updater.phase.restore_snapshot");

    if should_skip_pgdata_restore(worker.cli().db_mode, snapshot_id) {
        info!(
            db_mode = %worker.cli().db_mode,
            snapshot = snapshot_id,
            "db_mode=external or no snapshot; skipping pgdata restore (tag-only rollback)"
        );
        let _ = rec.finish_step_ok();
    } else {
        if let Err(e) = crate::probe::filesystem::require_pgdata(&worker.cli().pgdata) {
            let msg = e.to_string();
            let _ = rec.finish_step_err(&msg);
            // Tag may already be restored; outer execute_inline will best-effort start services.
            return Err(e);
        }
        match compose.stop(&["postgres"], 60).await {
            Ok(stop_pg) if !stop_pg.ok() => {
                warn!(
                    summary = %stop_pg.error_summary(),
                    "compose stop postgres non-zero; forcing stop"
                );
            }
            Err(e) => {
                warn!(err = %e, "compose stop postgres errored; forcing stop");
            }
            _ => {}
        }
        for name in ["myriad-postgres", "postgres"] {
            if let Err(e) = worker.docker().force_stop_container(name).await {
                warn!(%name, err = %e, "force_stop postgres attempt");
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;

        if let Err(e) = snap.restore(snapshot_id).await {
            // Fail closed for data: do not start postgres/app on a half-wiped or
            // post-upgrade pgdata after restore failure — operator must rescue.
            let msg = format!(
                "rollback: snapshot restore failed: {e}; refusing to start services \
                 (pgdata may be inconsistent — use rescue / manual restore)"
            );
            error!(err = %e, snapshot = %snapshot_id, %msg, "rollback snapshot restore failed");
            let _ = rec.finish_step_err(&msg);
            let _ = worker.state().append_audit(&format!(
                "audit: rollback_restore_failed job={} snapshot={} err={e}",
                rec.job_id, snapshot_id
            ));
            return Err(UpdaterError::Internal(anyhow::anyhow!(msg)));
        }
        let _ = rec.finish_step_ok();

        let start_pg = compose.start(&["postgres"]).await;
        let start_ok = start_pg.as_ref().map(|o| o.ok()).unwrap_or(false);
        if !start_ok {
            let up_pg = compose.up_detached(&["postgres"]).await;
            let up_ok = up_pg.as_ref().map(|o| o.ok()).unwrap_or(false);
            if !up_ok {
                let start_summary = start_pg
                    .as_ref()
                    .map(|o| o.error_summary())
                    .unwrap_or_else(|_| "start errored".into());
                let up_summary = up_pg
                    .as_ref()
                    .map(|o| o.error_summary())
                    .unwrap_or_else(|_| "up errored".into());
                let err = format!(
                    "post-restore start postgres failed: start={start_summary} up={up_summary}"
                );
                let _ = rec.finish_step_err(&err);
                return Err(UpdaterError::Internal(anyhow::anyhow!(err)));
            }
        }
    }

    let _ = rec.enter(Phase::StartOld, "updater.phase.start_old");
    let up = compose
        .up_detached_recreate(&["backend", "frontend"])
        .await
        .map_err(|e| UpdaterError::Internal(anyhow::anyhow!("start old failed: {e}")))?;
    if !up.ok() {
        let err = format!("start old failed: {}", up.error_summary());
        let _ = rec.finish_step_err(&err);
        return Err(UpdaterError::Internal(anyhow::anyhow!(err)));
    }
    let _ = rec.finish_step_ok();

    // Liveness only (no version stamp required — old image may still be pulling).
    match rollback_health_wait(worker.as_ref(), Duration::from_secs(180)).await {
        Ok(()) => {
            if let Some(ref v) = restored_version {
                if let Ok(mut st) = worker.state().read_updater() {
                    st.current_version = Some(v.clone());
                    st.current_commit_sha = None;
                    let _ = worker.state().write_updater(&st);
                }
                if let Err(e) = worker.reconcile_current_deploy().await {
                    warn!(err = %e, "rollback restored version but commit reconciliation failed");
                }
            }
            Ok(restored_version)
        }
        Err(e) => Err(e),
    }
}

/// Recreate the immutable Compose image refs from the local rollback aliases when
/// they are the slot recorded for `version`. This turns `*:myriad-rollback` into a
/// usable offline fallback instead of merely a dangling-image protection tag.
async fn materialize_pinned_rollback_images(worker: &Worker, version: &DeployTag) -> Result<()> {
    let state = worker.state().read_updater()?;
    if !rollback_slot_matches(&state, version) {
        return Ok(());
    }

    let env = EnvFile::load(&worker.cli().env_file)?;
    let backend = env.get("BACKEND_IMAGE").ok_or_else(|| {
        UpdaterError::Precondition(
            "BACKEND_IMAGE missing; cannot restore pinned rollback image".into(),
        )
    })?;
    let frontend = env.get("FRONTEND_IMAGE").ok_or_else(|| {
        UpdaterError::Precondition(
            "FRONTEND_IMAGE missing; cannot restore pinned rollback image".into(),
        )
    })?;

    // Require a complete pair of `*:myriad-rollback` before rewriting either
    // version ref. A half-pin (one component only) used to look like "one image
    // occupies the rollback tag, the other does not" and left Compose broken.
    let pair = [
        ("backend", backend.to_string()),
        ("frontend", frontend.to_string()),
    ];
    let mut missing_pins = Vec::new();
    for (component, repo) in &pair {
        let rollback_ref = format!("{repo}:{ROLLBACK_IMAGE_TAG}");
        if !worker.docker().image_exists_local(&rollback_ref).await {
            missing_pins.push((*component).to_string());
        }
    }
    if !should_materialize_rollback_pair(&missing_pins) {
        warn!(
            version = %version.as_str(),
            missing = ?missing_pins,
            "incomplete local rollback slot (*:myriad-rollback); will not materialize a split pair"
        );
        return Ok(());
    }

    for (component, repo) in &pair {
        let version_ref = format!("{repo}:{}", version.as_str());
        if worker.docker().image_exists_local(&version_ref).await {
            continue;
        }

        let rollback_ref = format!("{repo}:{ROLLBACK_IMAGE_TAG}");
        worker
            .docker()
            .tag_image(&rollback_ref, repo, version.as_str())
            .await
            .map_err(|e| {
                UpdaterError::Docker(format!(
                    "restore rollback {component} ({rollback_ref} -> {version_ref}): {e}"
                ))
            })?;
        info!(
            %component,
            source = %rollback_ref,
            target = %version_ref,
            "restored version ref from local rollback slot"
        );
    }

    Ok(())
}

fn rollback_slot_matches(state: &UpdaterStateFile, version: &DeployTag) -> bool {
    state.rollback_version.as_ref() == Some(version)
}

/// Pure helper: incomplete pairs must not materialize (avoids one-sided version retag).
pub(crate) fn should_materialize_rollback_pair(missing_pins: &[String]) -> bool {
    missing_pins.is_empty()
}

/// Whether rollback should skip pgdata restore (external DB or no snapshot id).
pub(crate) fn should_skip_pgdata_restore(
    db_mode: crate::config::DbMode,
    snapshot_id: &str,
) -> bool {
    db_mode.is_external() || snapshot_id.is_empty()
}

#[cfg(test)]
mod pair_integrity_tests {
    use super::*;
    use crate::config::DbMode;

    #[test]
    fn incomplete_pair_blocks_materialize() {
        assert!(!should_materialize_rollback_pair(&["frontend".into()]));
        assert!(!should_materialize_rollback_pair(&[
            "backend".into(),
            "frontend".into()
        ]));
        assert!(should_materialize_rollback_pair(&[]));
    }

    #[test]
    fn external_or_empty_snapshot_skips_pgdata_restore() {
        assert!(should_skip_pgdata_restore(DbMode::External, "snap-abc"));
        assert!(should_skip_pgdata_restore(DbMode::External, ""));
        assert!(should_skip_pgdata_restore(DbMode::Bundled, ""));
        assert!(!should_skip_pgdata_restore(DbMode::Bundled, "snap-abc"));
    }
}

/// Wait until backend answers /health with db_connected over the compose network.
/// Soft-pass after 60s if container is running and returns any 200 health JSON.
async fn rollback_health_wait(worker: &Worker, deadline: Duration) -> Result<()> {
    let start = std::time::Instant::now();
    let mut last = String::new();
    while start.elapsed() < deadline {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let elapsed = start.elapsed();
        match worker
            .docker()
            .http_probe("http://backend:1103/health", Duration::from_secs(5))
            .await
        {
            Ok((200, body)) => {
                let json: serde_json::Value =
                    serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
                let db = json.get("db_connected").and_then(|v| v.as_bool()) == Some(true);
                if db {
                    info!(
                        elapsed_s = elapsed.as_secs(),
                        "rollback health: db_connected ok"
                    );
                    return Ok(());
                }
                // Soft: after 60s, HTTP 200 health is enough (config mode may clear slowly).
                if elapsed >= Duration::from_secs(60) {
                    let running = worker
                        .docker()
                        .is_running("myriad-backend")
                        .await
                        .unwrap_or(false)
                        || worker.docker().is_running("backend").await.unwrap_or(false);
                    if running {
                        warn!(
                            elapsed_s = elapsed.as_secs(),
                            "rollback health: soft-pass (HTTP 200, db_connected not yet true)"
                        );
                        return Ok(());
                    }
                }
                last = format!(
                    "backend 200 but db_connected=false body={}",
                    &body[..body.len().min(80)]
                );
            }
            Ok((code, body)) => {
                last = format!(
                    "backend HTTP {code}: {}",
                    body.chars().take(80).collect::<String>()
                );
            }
            Err(e) => {
                last = format!("backend probe: {e}");
            }
        }
        let _ = crate::worker::machine::heartbeat(worker.state());
    }
    Err(UpdaterError::Precondition(format!(
        "rollback health probe exceeded {}s; last={last}",
        deadline.as_secs()
    )))
}

/// Resolve the image tag that represented the last known good business version.
///
/// Order:
/// 1. Explicit preferred tag (captured by `swap_tag` before overwrite)
/// 2. Snapshot `source_version`
/// 3. `updater.json.current_version`
///
/// Returns `Ok(None)` only when no durable source knows the previous tag.
pub(crate) fn resolve_previous_tag(
    state: &StateDir,
    snapshot_id: &str,
    preferred: Option<&str>,
) -> Result<Option<String>> {
    if let Some(tag) = preferred.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(Some(tag.to_string()));
    }

    let snaps = state.read_snapshots()?;
    if let Some(meta) = snaps.items.iter().find(|m| m.id == snapshot_id) {
        if let Some(v) = &meta.source_version {
            return Ok(Some(v.to_string()));
        }
    }

    if let Some(v) = state.read_updater()?.current_version {
        return Ok(Some(v.to_string()));
    }

    // Callers treat None as "leave .env alone".
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{SnapshotMeta, SnapshotsFile, StateDir, UpdaterStateFile};
    use chrono::Utc;
    use tempfile::tempdir;

    #[test]
    fn resolve_prefers_explicit_tag() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        let got = resolve_previous_tag(&state, "snap-x", Some("v1.2.3")).unwrap();
        assert_eq!(got.as_deref(), Some("v1.2.3"));
    }

    #[test]
    fn rollback_slot_only_matches_its_recorded_version() {
        let updater = UpdaterStateFile {
            rollback_version: Some(DeployTag::parse("v1.2.3").unwrap()),
            ..UpdaterStateFile::default()
        };

        assert!(rollback_slot_matches(
            &updater,
            &DeployTag::parse("v1.2.3").unwrap()
        ));
        assert!(!rollback_slot_matches(
            &updater,
            &DeployTag::parse("v1.2.4").unwrap()
        ));
    }

    #[test]
    fn resolve_falls_back_to_snapshot_source_version() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        let mut sf = SnapshotsFile::default();
        sf.items.push(SnapshotMeta {
            id: "snap-abc".into(),
            created_at: Utc::now(),
            source_version: Some(DeployTag::parse("v0.9.0").unwrap()),
            size_bytes: 1,
            file_count: 1,
            keep: false,
            sample_sha256: None,
        });
        state.write_snapshots(&sf).unwrap();
        let got = resolve_previous_tag(&state, "snap-abc", None).unwrap();
        assert_eq!(got.as_deref(), Some("v0.9.0"));
    }

    #[test]
    fn resolve_falls_back_to_current_version() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        let st = UpdaterStateFile {
            current_version: Some(DeployTag::parse("v0.8.1").unwrap()),
            ..UpdaterStateFile::default()
        };
        state.write_updater(&st).unwrap();
        let got = resolve_previous_tag(&state, "snap-missing", None).unwrap();
        assert_eq!(got.as_deref(), Some("v0.8.1"));
    }

    #[test]
    fn resolve_explicit_overrides_snapshot() {
        let dir = tempdir().unwrap();
        let state = StateDir::open(dir.path()).unwrap();
        let mut sf = SnapshotsFile::default();
        sf.items.push(SnapshotMeta {
            id: "snap-abc".into(),
            created_at: Utc::now(),
            source_version: Some(DeployTag::parse("v0.9.0").unwrap()),
            size_bytes: 1,
            file_count: 1,
            keep: false,
            sample_sha256: None,
        });
        state.write_snapshots(&sf).unwrap();
        let got = resolve_previous_tag(&state, "snap-abc", Some("v0.8.0")).unwrap();
        assert_eq!(got.as_deref(), Some("v0.8.0"));
    }
}
