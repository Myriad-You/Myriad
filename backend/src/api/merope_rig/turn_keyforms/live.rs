//! 本进程正在跑的任务：管线同步地改它们的进度（`update`），跑任务的那一路每秒把变了的进度和
//! 步骤写进任务记录（`services::rig_jobs`），每十秒续一次租约，顺带看有没有人要取消。取消时
//! 整个任务的 future 被丢掉，手上还在等的生图和拆层请求随之断开；已经做完的生图与拆层在
//! `TurnCache` 里，再提交一次不必重做。
//!
//! 同时能跑几个任务由 `MYRIAD_RIG_JOBS_AT_ONCE` 决定，默认一个。

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};

use sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use tokio::sync::Notify;

use super::{ApiResult, ArchiveMeta, Job, Made, finish};
use crate::services::{ai_cost_ledger::AiLedgerAttribution, rig_jobs};

/// The jobs this process runs, by id: their progress as the pipeline reports
/// it, and the results of those that ended until the next job is claimed.
pub(super) static JOBS: LazyLock<Mutex<HashMap<String, Job>>> = LazyLock::new(Default::default);

/// This process, as the runner of the jobs it claims.
pub(super) static RUNNER: LazyLock<String> =
    LazyLock::new(|| format!("proc-{}", uuid::Uuid::new_v4().simple()));

/// How many jobs run at once, across processes: `MYRIAD_RIG_JOBS_AT_ONCE`, one by default.
pub(super) fn at_once() -> u32 {
    at_once_from(std::env::var("MYRIAD_RIG_JOBS_AT_ONCE").ok().as_deref())
}

fn at_once_from(value: Option<&str>) -> u32 {
    value
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(1)
}

const WRITE_EVERY: Duration = Duration::from_secs(1);
const RENEW_EVERY: Duration = Duration::from_secs(10);

/// Where a job's steps run.
#[derive(Clone, Debug, Default)]
pub(super) struct Targets {
    /// The See-through Space's name.
    pub space: Option<String>,
    /// The image provider and model, as `provider/model`.
    pub image: Option<String>,
}

/// A step of a job as this process has it, and whether it changed since it was written.
#[derive(Clone, Debug)]
pub(super) struct Step {
    pub row: rig_jobs::StepRow,
    began: Instant,
    version: u64,
    written: u64,
}

tokio::task_local! {
    /// The step running in this future: its job and its place among the job's steps.
    static STEP: (String, usize);
    /// The ledger task id of the job this future works for.
    static LEDGER: String;
}

/// Steps whose work is drawing; every other step's runs on the Space.
fn backend_of(name: &str, targets: &Targets) -> (&'static str, Option<String>) {
    if name.ends_with("-drawings") {
        ("image_provider", targets.image.clone())
    } else {
        ("hf_space", targets.space.clone())
    }
}

impl Job {
    /// A step begun now: its place among the job's steps.
    pub(super) fn step_began(&mut self, name: &str, began: Instant) -> usize {
        let seq = self.steps.len();
        let (backend, target) = backend_of(name, &self.targets);
        let started_at =
            crate::clock::now() - chrono::Duration::from_std(began.elapsed()).unwrap_or_default();
        self.steps.push(Step {
            row: rig_jobs::StepRow {
                seq: seq as i32,
                name: name.to_owned(),
                state: rig_jobs::State::Running,
                attempts: 1,
                backend: backend.to_owned(),
                backend_target: target,
                started_at,
                ended_at: None,
                wall_seconds: None,
                gpu_seconds: None,
            },
            began,
            version: 1,
            written: 0,
        });
        seq
    }

    pub(super) fn step_ended(&mut self, seq: usize, ok: bool) {
        if let Some(step) = self.steps.get_mut(seq) {
            step.row.state = if ok {
                rig_jobs::State::Succeeded
            } else {
                rig_jobs::State::Failed
            };
            step.row.ended_at = Some(crate::clock::now());
            step.row.wall_seconds = Some(step.began.elapsed().as_secs_f64());
            step.version += 1;
        }
    }

    /// Where the job stands, as its record keeps it.
    pub(super) fn progress(&self) -> Value {
        json!({
            "stage": self.stage.as_str(),
            "phase": self.phase.as_str(),
            "body": self.body.map(|body| json!({
                "motion": body.motion,
                "stage": body.stage.as_str(),
                "done": body.done,
                "total": body.total,
            })),
            "done": self.done,
            "total": self.total,
        })
    }

    /// Steps changed since they were written, with the version each is at.
    fn unwritten_steps(&self) -> Vec<(usize, u64, rig_jobs::StepRow)> {
        self.steps
            .iter()
            .enumerate()
            .filter(|(_, step)| step.version != step.written)
            .map(|(seq, step)| (seq, step.version, step.row.clone()))
            .collect()
    }
}

/// `read` of the job, when this process runs it.
pub(super) fn with_job<T>(id: &str, read: impl FnOnce(&Job) -> T) -> Option<T> {
    let jobs = JOBS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    jobs.get(id).map(read)
}

/// A job this process has just claimed, tracked from now; the jobs that
/// ended before it are let go.
pub(super) fn track(job: Job) {
    let mut jobs = JOBS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    jobs.retain(|_, job| !job.ended());
    jobs.insert(job.id.clone(), job);
}

/// Cancel told to the job, when this process runs it.
pub(super) fn tell_cancel(id: &str) {
    if let Some(cancel) = with_job(id, |job| job.cancel.clone()) {
        cancel.notify_one();
    }
}

/// One more call made for the step running in this future, when it tries again.
pub(in crate::api::merope_rig) fn note_retry() {
    let _ = STEP.try_with(|(id, seq)| {
        super::update(id, |job| {
            if let Some(step) = job.steps.get_mut(*seq) {
                step.row.attempts += 1;
                step.version += 1;
            }
        });
    });
}

/// `work` run as the job's step `seq`.
pub(super) async fn as_step<T>(id: &str, seq: usize, work: impl Future<Output = T>) -> T {
    STEP.scope((id.to_owned(), seq), work).await
}

/// The ledger attribution of a turned drawing: the job's ledger id when it is
/// drawn for a job, so that the job's spend adds up.
pub(super) fn drawing_attribution(user_id: i32) -> AiLedgerAttribution {
    let site = AiLedgerAttribution::site(user_id, "merope", "turn-reference");
    match LEDGER.try_with(Clone::clone) {
        Ok(task_id) => AiLedgerAttribution { task_id, ..site },
        Err(_) => site,
    }
}

/// `work` run with the job's ledger id for the paid calls inside it.
pub(super) async fn for_ledger<T>(id: &str, work: impl Future<Output = T>) -> T {
    LEDGER.scope(rig_jobs::ledger_task_id(id), work).await
}

/// What was last written of a job.
#[derive(Default)]
struct Written {
    version: u64,
    renewed: Option<Instant>,
}

/// Progress and changed steps written when anything changed, the lease
/// renewed when due. Whether the job is to stop: cancelled, or no longer
/// this runner's (failed as interrupted while its lease could not be renewed).
async fn write(db: &DatabaseConnection, id: &str, written: &mut Written) -> bool {
    let Some((version, progress, steps)) = with_job(id, |job| {
        (job.version, job.progress(), job.unwritten_steps())
    }) else {
        return true;
    };
    let changed = version != written.version;
    let due = written
        .renewed
        .is_none_or(|renewed| renewed.elapsed() >= RENEW_EVERY);
    if !changed && !due {
        return false;
    }
    let rows: Vec<_> = steps.iter().map(|(_, _, row)| row.clone()).collect();
    let beat = async {
        rig_jobs::save_steps(db, id, &rows).await?;
        rig_jobs::heartbeat(db, id, &RUNNER, changed.then_some(&progress)).await
    }
    .await;
    match beat {
        Ok(beat) => {
            written.version = version;
            written.renewed = Some(Instant::now());
            super::update_quietly(id, |job| {
                for (seq, at, _) in &steps {
                    if let Some(step) = job.steps.get_mut(*seq) {
                        step.written = *at;
                    }
                }
            });
            if !beat.running {
                tracing::warn!(job = %id, "turn keyforms job is no longer this process's; stopping it");
            }
            !beat.running || beat.cancel_requested
        }
        Err(error) => {
            tracing::warn!(%error, job = %id, "turn keyforms progress could not be written");
            false
        }
    }
}

/// The job's steps as they stand, written.
pub(super) async fn write_steps(db: &DatabaseConnection, id: &str) {
    let Some(steps) = with_job(id, Job::unwritten_steps) else {
        return;
    };
    let rows: Vec<_> = steps.into_iter().map(|(_, _, row)| row).collect();
    if let Err(error) = rig_jobs::save_steps(db, id, &rows).await {
        tracing::warn!(%error, job = %id, "turn keyforms steps could not be written");
    }
}

/// Runs a claimed job's `work` to its end, or until it is cancelled, writing
/// its progress meanwhile; then ends it (`finish`). Cancelled, the work is
/// dropped where it stands: what it was waiting on is let go.
pub(super) async fn run_job(
    db: DatabaseConnection,
    id: String,
    work: impl Future<Output = ApiResult<Made>>,
    meta: ArchiveMeta,
) {
    let Some(cancel) = with_job(&id, |job| job.cancel.clone()) else {
        return;
    };
    let mut work = Box::pin(for_ledger(&id, work));
    let mut tick = tokio::time::interval(WRITE_EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut written = Written::default();
    let result = loop {
        tokio::select! {
            result = &mut work => break Some(result),
            () = cancel.notified() => break None,
            _ = tick.tick() => {
                if write(&db, &id, &mut written).await {
                    break None;
                }
            }
        }
    };
    drop(work);
    match result {
        Some(result) => finish(&db, &id, result, meta).await,
        None => super::cancelled(&db, &id).await,
    }
}

/// A notifier for a job's cancel.
pub(super) fn cancel_notifier() -> Arc<Notify> {
    Arc::new(Notify::new())
}

#[cfg(test)]
#[path = "live_tests.rs"]
pub(super) mod tests;
