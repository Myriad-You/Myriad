//! Helpers used by update/rollback flows to record phase transitions atomically.

use chrono::Utc;
use tracing::info;

use crate::error::Result;
use crate::state::{JobStatus, JobStep, MaintenanceFile, Phase, StateDir};
use crate::version::DeployTag;

pub struct PhaseRecorder<'a> {
    pub state: &'a StateDir,
    pub job_id: String,
    pub from_version: Option<DeployTag>,
    pub to_version: Option<DeployTag>,
}

impl<'a> PhaseRecorder<'a> {
    pub fn enter(&self, phase: Phase, message_key: &str) -> Result<()> {
        // Preflight must keep active=false so a failed pre-check never strand
        // the SPA on the maintenance page or block /api via the proxy.
        let m = MaintenanceFile {
            schema_version: 1,
            active: phase.takes_site_offline(),
            phase,
            from_version: self.from_version.clone(),
            to_version: self.to_version.clone(),
            started_at: Some(Utc::now()),
            updated_at: Utc::now(),
            job_id: Some(self.job_id.clone()),
            message_key: message_key.to_string(),
        };
        self.state.write_maintenance(&m)?;

        let mut job = self.state.read_job(&self.job_id)?;
        if !matches!(
            job.steps.last().map(|s| s.phase),
            Some(p) if p == phase && job.steps.last().is_some_and(|s| s.finished_at.is_none())
        ) {
            job.steps.push(JobStep::start(phase));
        }
        if matches!(job.status, JobStatus::Pending) {
            job.status = JobStatus::Running;
        }
        self.state.write_job(&job)?;
        info!(job = %self.job_id, ?phase, "phase enter");
        let _ = self
            .state
            .append_history(&format!("job {}: -> {:?}", self.job_id, phase));
        Ok(())
    }

    pub fn finish_step_ok(&self) -> Result<()> {
        let mut job = self.state.read_job(&self.job_id)?;
        if let Some(step) = job.steps.last_mut() {
            step.finish_ok();
        }
        self.state.write_job(&job)
    }

    pub fn finish_step_err(&self, err: impl Into<String>) -> Result<()> {
        let err = err.into();
        let mut job = self.state.read_job(&self.job_id)?;
        if let Some(step) = job.steps.last_mut() {
            step.finish_err(err.clone());
        }
        self.state.write_job(&job)?;
        let _ = self
            .state
            .append_history(&format!("job {}: ERR {}", self.job_id, err));
        Ok(())
    }

    pub fn finalize(&self, status: JobStatus) -> Result<()> {
        let mut job = self.state.read_job(&self.job_id)?;
        job.status = status;
        job.finished_at = Some(Utc::now());
        self.state.write_job(&job)?;
        let _ = self
            .state
            .append_history(&format!("job {}: finalize {:?}", self.job_id, status));
        let _ = self.state.append_audit(&format!(
            "audit: job_terminal job={} status={:?}",
            self.job_id, status
        ));
        Ok(())
    }
}

pub fn clear_maintenance(state: &StateDir) -> Result<()> {
    state.clear_maintenance()?;
    state.set_current_job(None)?;
    Ok(())
}

pub fn heartbeat(state: &StateDir) -> Result<()> {
    let mut m = state.read_maintenance()?;
    if m.active {
        m.bump_heartbeat();
        state.write_maintenance(&m)?;
    }
    Ok(())
}
