//! Crash recovery: decide whether to idle, restore a pre-swap stack, or freeze
//! into `needs_manual`. Pure planning is unit-tested; I/O lives in
//! [`Worker::recover_or_idle_state`].

use std::sync::Arc;

use chrono::Utc;
use tracing::{info, warn};

use crate::docker::DockerClient;
use crate::error::Result;
use crate::state::{Job, JobStatus, MaintenanceFile, Phase, StateDir};
use crate::worker::Worker;

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
}

#[cfg(test)]
mod recovery_plan_tests {
    use super::*;
    use crate::state::{JobKind, JobStep};
    use crate::version::DeployTag;

    fn job(status: JobStatus, phase: Phase) -> Job {
        Job {
            trust: None,
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
