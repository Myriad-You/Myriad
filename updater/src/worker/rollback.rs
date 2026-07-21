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
//! still leaves `.env` pointing at the rollback version. If snapshot restore fails we still attempt
//! to start services so operators are not stuck with everything stopped.

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
    rec.enter(Phase::RollbackInProgress, "updater.phase.rollback")?;
    rec.finish_step_ok()?;

    let compose = super::update::build_compose_runner_pub(&worker).await?;
    let snap = SnapshotManager {
        state: worker.state(),
        pgdata: worker.cli().pgdata.clone(),
    };

    // Resolve from snapshot / rollback version — never skip tag restore on standalone rollback
    // when metadata is available.
    match execute_inline(worker.clone(), &rec, &compose, &snap, &snapshot_id, None).await {
        Ok(restored) => {
            if let Some(v) = restored {
                let mut job = worker.state().read_job(&job_id)?;
                job.to_version = Some(v);
                worker.state().write_job(&job)?;
            }
            rec.finalize(JobStatus::Succeeded)?;
            crate::worker::machine::clear_maintenance(worker.state())?;
            Ok(())
        }
        Err(e) => {
            error!(err = %e, "standalone rollback failed");
            rec.finalize(JobStatus::NeedsManual)?;
            let mut m = worker.state().read_maintenance()?;
            m.phase = Phase::NeedsManual;
            m.bump_heartbeat();
            worker.state().write_maintenance(&m)?;
            Err(e)
        }
    }
}

/// Execute the core rollback steps.
///
/// Returns the version tag that was restored into `MYRIAD_TAG` (when known).
pub async fn execute_inline(
    worker: Arc<Worker>,
    rec: &PhaseRecorder<'_>,
    compose: &ComposeRunner,
    snap: &SnapshotManager<'_>,
    snapshot_id: &str,
    swap_back_tag: Option<&str>,
) -> Result<Option<DeployTag>> {
    info!(snapshot = snapshot_id, "rollback: stopping new containers");
    rec.enter(Phase::StopNew, "updater.phase.stop_new")?;
    let stop_app = compose.stop(&["frontend", "backend"], 30).await?;
    if !stop_app.ok() {
        warn!(
            summary = %stop_app.error_summary(),
            "compose stop frontend/backend non-zero; forcing container stop"
        );
        for name in ["myriad-frontend", "frontend", "myriad-backend", "backend"] {
            let _ = worker.docker().force_stop_container(name).await;
        }
    }
    rec.finish_step_ok()?;

    // --- Resolve + restore MYRIAD_TAG BEFORE snapshot work ---
    // So any later failure (EBUSY restore, etc.) still leaves env at the rollback version.
    let prev_tag = resolve_previous_tag(worker.state(), snapshot_id, swap_back_tag)?;
    let restored_version = match &prev_tag {
        Some(tag) => {
            rec.enter(Phase::SwapTagBack, "updater.phase.swap_tag_back")?;
            let parsed = DeployTag::parse(tag).ok();
            if let Some(ref version) = parsed {
                if let Err(e) = materialize_pinned_rollback_images(worker.as_ref(), version).await {
                    // The immutable version tag may still be local or pullable by Compose. Keep
                    // the normal rollback path available, but make the lost local fallback loud.
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
            rec.finish_step_ok()?;
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

    rec.enter(Phase::RestoreSnapshot, "updater.phase.restore_snapshot")?;
    // Missing pgdata is non-fatal at startup; restore still needs the path to exist.
    if let Err(e) = crate::probe::filesystem::require_pgdata(&worker.cli().pgdata) {
        let msg = e.to_string();
        let _ = rec.finish_step_err(&msg);
        return Err(e);
    }
    let stop_pg = compose.stop(&["postgres"], 60).await?;
    if !stop_pg.ok() {
        warn!(
            summary = %stop_pg.error_summary(),
            "compose stop postgres non-zero; forcing stop"
        );
    }
    // Hard guarantee: no process may hold open files under pgdata.
    for name in ["myriad-postgres", "postgres"] {
        if let Err(e) = worker.docker().force_stop_container(name).await {
            warn!(%name, err = %e, "force_stop postgres attempt");
        }
    }
    // Brief settle so the kernel releases bind-mount file handles.
    tokio::time::sleep(Duration::from_secs(1)).await;

    let mut restore_failed: Option<String> = None;
    if let Err(e) = snap.restore(snapshot_id).await {
        // Do NOT abort the whole rollback here — tag is already restored; try to bring
        // services back so the operator is not left with a fully stopped stack.
        warn!(
            err = %e,
            snapshot = snapshot_id,
            "rollback: snapshot restore failed; continuing to start rollback images \
             (pgdata may still be post-upgrade)"
        );
        restore_failed = Some(e.to_string());
        let _ = rec.finish_step_err(format!("restore snapshot (continuing): {e}"));
    } else {
        rec.finish_step_ok()?;
    }

    let start_pg = compose.start(&["postgres"]).await?;
    if !start_pg.ok() {
        // Try compose up for postgres if start failed (container removed).
        let up_pg = compose.up_detached(&["postgres"]).await?;
        if !up_pg.ok() {
            let err = format!(
                "post-restore start postgres failed: start={} up={}",
                start_pg.error_summary(),
                up_pg.error_summary()
            );
            rec.finish_step_err(&err)?;
            return Err(UpdaterError::Internal(anyhow::anyhow!(err)));
        }
    }

    rec.enter(Phase::StartOld, "updater.phase.start_old")?;
    let up = compose.up_detached(&["backend", "frontend"]).await?;
    if !up.ok() {
        let err = format!("start old failed: {}", up.error_summary());
        rec.finish_step_err(&err)?;
        return Err(UpdaterError::Internal(anyhow::anyhow!(err)));
    }
    rec.finish_step_ok()?;

    // Liveness only (no version stamp required — old image may still be pulling).
    match rollback_health_wait(worker.as_ref(), Duration::from_secs(180)).await {
        Ok(()) => {
            if let Some(ref v) = restored_version {
                let mut st = worker.state().read_updater()?;
                st.current_version = Some(v.clone());
                st.current_commit_sha = None;
                worker.state().write_updater(&st)?;
                if let Err(e) = worker.reconcile_current_deploy().await {
                    warn!(err = %e, "rollback restored version but commit reconciliation failed");
                }
            }
            if let Some(ref e) = restore_failed {
                // Services up but data not restored — still NeedsManual signal via error.
                return Err(UpdaterError::Precondition(format!(
                    "rollback brought services up on the rollback tag, but pgdata restore failed: {e}"
                )));
            }
            Ok(restored_version)
        }
        Err(e) => {
            if let Some(ref re) = restore_failed {
                Err(UpdaterError::Precondition(format!(
                    "rollback health failed ({e}); also pgdata restore failed: {re}"
                )))
            } else {
                Err(e)
            }
        }
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

#[cfg(test)]
mod pair_integrity_tests {
    use super::*;

    #[test]
    fn incomplete_pair_blocks_materialize() {
        assert!(!should_materialize_rollback_pair(&["frontend".into()]));
        assert!(!should_materialize_rollback_pair(&[
            "backend".into(),
            "frontend".into()
        ]));
        assert!(should_materialize_rollback_pair(&[]));
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
