//! 本进程正在跑的任务：管线同步地改它们的进度（`update`）。每个任务旁边有一个单独的看守任务
//! （`keep`）：每秒把变了的进度和步骤写进任务记录（`services::rig_jobs`），每十秒续一次租约，
//! 顺带看有没有人要取消。续租和写步骤互不牵连：步骤写不进去只记一行、下一秒再写；看守在自己
//! 的任务里跑，数据库慢不会拖住管线的请求，管线里长的同步计算也拖不住续租。任务做完到结果
//! 写进记录（核对主立绘、存档、写结束）这段时间租约一直续着。
//!
//! 取消时整个任务的 future 被丢掉，等着的拆层请求随之断开；已经发出去的生图不断开，在后台
//! 画完，照常记进账本（记在这个任务名下），图不留（`detached`）。已经做完的生图与拆层在
//! `TurnCache` 里，再提交一次不必重做。
//!
//! 本进程不会把自己还在跑的任务记成中断。租约丢了（数据库有一阵连不上，别的进程把任务记成
//! 中断）时，没有别的任务顶替它、上限也允许，就把租约拿回来接着跑；否则让这一路停下，免得
//! 和页面重开的那个一起花钱。任务的记录没了（账号被删）时任务停下。任务最多跑
//! `JOB_DEADLINE`，过了就记成超时失败，不让卡住的任务一直占着位置。
//!
//! 任务记录没写成结束之前，页面看到的一直是做完前的最后一个阶段，不会先看到「done」再去取
//! 还没交出来的结果。
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

/// The process running the jobs it claims.
pub(super) fn runner() -> String {
    rig_jobs::this_process().to_owned()
}

/// The jobs this process runs, by id: their progress as the pipeline reports
/// it, and the results of those that ended until the next job is claimed.
pub(super) static JOBS: LazyLock<Mutex<HashMap<String, Job>>> = LazyLock::new(Default::default);

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
/// The longest a job runs before it is failed as timed out: well past the
/// tens of minutes a figure with its body takes, short of holding the place
/// for good.
const JOB_DEADLINE: Duration = Duration::from_secs(3 * 60 * 60);

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

/// Where a step's work runs, by the step: the drawings on the image provider;
/// the judging and the fit on the Space, with a turn drawn again on the image
/// provider when it asks for one; every other step on the Space.
fn backend_of(name: &str, targets: &Targets) -> (&'static str, Option<String>) {
    match name {
        "head-drawings" | "body-drawings" => ("image_provider", targets.image.clone()),
        "head-judging" | "head-fit" => (
            "hf_space+image_provider",
            match (&targets.space, &targets.image) {
                (Some(space), Some(image)) => Some(format!("{space} + {image}")),
                (space, image) => space.clone().or_else(|| image.clone()),
            },
        ),
        _ => ("hf_space", targets.space.clone()),
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
                backend_target: target.as_deref().map(rig_jobs::backend_target),
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
            "stage": self.shown.as_str(),
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

/// A paid call run on its own task, under `attribution`: once sent, it is
/// seen to its end and written to the ledger even when the job waiting for
/// it is cancelled and stops waiting.
pub(super) async fn detached<T: Send + 'static>(
    attribution: AiLedgerAttribution,
    call: impl Future<Output = T> + Send + 'static,
) -> Option<T> {
    let call = crate::services::ai_cost_ledger::with_ai_ledger_attribution(attribution, call);
    tokio::spawn(call).await.ok()
}

/// What was last written of a job.
#[derive(Default)]
struct Written {
    version: u64,
    renewed: Option<Instant>,
}

/// What the keeper found the job is to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Keep {
    Go,
    /// Cancelled, or its record is gone: it stops.
    Stop,
}

/// The lease renewed when anything changed or it is due, with the progress;
/// then changed steps written. A step that cannot be written is logged and
/// written again next time; it never holds back the lease.
async fn write(db: &DatabaseConnection, id: &str, written: &mut Written) -> Keep {
    let Some((version, progress, ending)) =
        with_job(id, |job| (job.version, job.progress(), job.ending))
    else {
        return Keep::Stop;
    };
    let changed = version != written.version;
    let due = written
        .renewed
        .is_none_or(|renewed| renewed.elapsed() >= RENEW_EVERY);
    let keep = if changed || due {
        renew(
            db,
            id,
            written,
            version,
            changed.then_some(&progress),
            ending,
        )
        .await
    } else {
        Keep::Go
    };
    let steps = with_job(id, Job::unwritten_steps).unwrap_or_default();
    if !steps.is_empty() {
        let rows: Vec<_> = steps.iter().map(|(_, _, row)| row.clone()).collect();
        match rig_jobs::save_steps(db, id, &rows).await {
            Ok(()) => super::update_quietly(id, |job| {
                for (seq, at, _) in &steps {
                    if let Some(step) = job.steps.get_mut(*seq) {
                        step.written = *at;
                    }
                }
            }),
            Err(error) => {
                tracing::warn!(%error, job = %id, "turn keyforms steps could not be written; trying again")
            }
        }
    }
    keep
}

/// The lease renewed. Lost while the job still works (a sweep elsewhere took
/// it for interrupted), it is taken back when nothing took its place;
/// otherwise the job stops, so that it does not run beside the one a page
/// started again. While its end is being written the job only holds on.
async fn renew(
    db: &DatabaseConnection,
    id: &str,
    written: &mut Written,
    version: u64,
    progress: Option<&Value>,
    ending: bool,
) -> Keep {
    match rig_jobs::heartbeat(db, id, rig_jobs::this_process(), progress).await {
        Ok(rig_jobs::Beat::Renewed { cancel_requested }) => {
            written.version = version;
            written.renewed = Some(Instant::now());
            if cancel_requested && !ending {
                Keep::Stop
            } else {
                Keep::Go
            }
        }
        Ok(rig_jobs::Beat::Lost) if ending => Keep::Go,
        Ok(rig_jobs::Beat::Lost) => match rig_jobs::retake(db, id, at_once()).await {
            Ok(true) => {
                tracing::warn!(job = %id, "turn keyforms lease was lost while the job ran; taken back");
                // Its steps were ended with it: written again as they stand.
                super::update_quietly(id, |job| {
                    for step in &mut job.steps {
                        step.written = 0;
                    }
                });
                written.renewed = None;
                Keep::Go
            }
            Ok(false) => {
                tracing::warn!(job = %id, "turn keyforms lease was lost and its place taken; stopping the job");
                Keep::Stop
            }
            Err(error) => {
                tracing::warn!(%error, job = %id, "turn keyforms lease could not be taken back");
                Keep::Go
            }
        },
        Ok(rig_jobs::Beat::Gone) if ending => Keep::Go,
        Ok(rig_jobs::Beat::Gone) => {
            tracing::warn!(job = %id, "turn keyforms job's record is gone; stopping it");
            Keep::Stop
        }
        Err(error) => {
            tracing::warn!(%error, job = %id, "turn keyforms lease could not be renewed");
            Keep::Go
        }
    }
}

/// The job's steps as they stand, written.
pub(super) async fn write_steps(db: &DatabaseConnection, id: &str) -> Result<(), sea_orm::DbErr> {
    let Some(steps) = with_job(id, Job::unwritten_steps) else {
        return Ok(());
    };
    let rows: Vec<_> = steps.into_iter().map(|(_, _, row)| row).collect();
    rig_jobs::save_steps(db, id, &rows).await
}

/// Keeps a job's record up while it runs and ends, on its own task, until
/// `done`: writes it (`write`) once a second, and tells the job to stop when
/// it is cancelled or its record is gone.
async fn keep(db: DatabaseConnection, id: String, mut done: tokio::sync::watch::Receiver<bool>) {
    let Some(cancel) = with_job(&id, |job| job.cancel.clone()) else {
        return;
    };
    let mut tick = tokio::time::interval(WRITE_EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut written = Written::default();
    let mut told = false;
    loop {
        tokio::select! {
            _ = done.changed() => return,
            _ = tick.tick() => {
                if write(&db, &id, &mut written).await == Keep::Stop && !told {
                    told = true;
                    cancel.notify_one();
                }
            }
        }
    }
}

/// Runs a claimed job's `work` to its end, or until it is cancelled, its
/// record kept up meanwhile by a keeper on its own task (`keep`); then ends
/// it (`finish`) with the lease still kept. Cancelled, the work is dropped
/// where it stands: what it was waiting on is let go. Past `JOB_DEADLINE` it
/// fails as timed out.
pub(super) async fn run_job(
    db: DatabaseConnection,
    id: String,
    work: impl Future<Output = ApiResult<Made>>,
    meta: ArchiveMeta,
) {
    run_job_within(db, id, work, meta, JOB_DEADLINE).await
}

/// How a job's run ended.
enum Ran {
    Made(Box<ApiResult<Made>>),
    Stopped,
    TimedOut,
}

pub(super) async fn run_job_within(
    db: DatabaseConnection,
    id: String,
    work: impl Future<Output = ApiResult<Made>>,
    meta: ArchiveMeta,
    deadline: Duration,
) {
    let Some(cancel) = with_job(&id, |job| job.cancel.clone()) else {
        return;
    };
    rig_jobs::hold(&id);
    let (done, watching) = tokio::sync::watch::channel(false);
    let keeper = tokio::spawn(keep(db.clone(), id.clone(), watching));
    let work = for_ledger(&id, work);
    let ran = tokio::select! {
        result = work => Ran::Made(Box::new(result)),
        () = cancel.notified() => Ran::Stopped,
        () = tokio::time::sleep(deadline) => Ran::TimedOut,
    };
    super::update_quietly(&id, |job| job.ending = true);
    match ran {
        Ran::Made(result) => finish(&db, &id, *result, meta).await,
        Ran::Stopped => super::cancelled(&db, &id).await,
        Ran::TimedOut => {
            tracing::warn!(job = %id, ?deadline, "turn keyforms job ran past its deadline");
            finish(&db, &id, Err(timed_out()), meta).await
        }
    }
    let _ = done.send(true);
    let _ = keeper.await;
    rig_jobs::let_go(&id);
}

/// The failure of a job that ran past its deadline.
fn timed_out() -> super::ApiError {
    (
        axum::http::StatusCode::GATEWAY_TIMEOUT,
        axum::Json(json!({
            "error": "The turn keys job took too long",
            "code": "turn_keyforms_timed_out"
        })),
    )
}

/// A notifier for a job's cancel.
pub(super) fn cancel_notifier() -> Arc<Notify> {
    Arc::new(Notify::new())
}

#[cfg(test)]
#[path = "live_tests.rs"]
pub(super) mod tests;
