//! Rig generation jobs as rows (`migrations/rig_jobs.sql`): what a job was
//! asked for and by whom, whose it is (`Owner`) and what its steps run from,
//! where it stands, how it ended, where its result is
//! kept, every step it ran with its time and compute, the ledger id its paid
//! calls are written under, and the callers' keys for their submissions.
//!
//! A job is claimed in one transaction under one lock: the caller's key gives
//! the job it gave before (unless that job failed or was cancelled, which
//! lets the key go), a running job of the same subject is joined (and takes
//! the key), and at most `at_once` jobs run at a time. Keys last `KEY_TTL`;
//! ended jobs are kept `RETENTION`.
//!
//! The process running a job renews its lease (`heartbeat`); a running job
//! whose lease has lapsed is one whose process went away, and is failed as
//! interrupted (`fail_interrupted`): always before a claim, and at most every
//! `SWEEP_EVERY` before a read. A process never fails the jobs it still runs
//! itself (`hold`): their lease lapsing means its database was out of reach,
//! not that they stopped. A job failed as interrupted by another process
//! while its own lived on takes its lease back when nothing has taken its
//! place (`retake`).
//!
//! A process told to stop claims nothing more (`begin_stopping`), and lets
//! its jobs go once it has stopped answering requests (`release_at_exit`):
//! until then a page polling one of them reads it running, so that it does
//! not start it again on the process that is going away.

use std::{
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, DbErr, FromQueryResult, Statement,
    TransactionTrait, Value as SeaValue,
};
use serde_json::Value;

#[path = "rig_job_owner.rs"]
mod owner;
pub use owner::{Owner, Storage};

/// How long a running job's lease lasts without being renewed.
pub const LEASE: Duration = Duration::from_secs(45);

/// How long a caller's key gives the job it gave.
pub const KEY_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// How long an ended job is kept.
pub const RETENTION: Duration = Duration::from_secs(90 * 24 * 60 * 60);

/// How often a read fails the jobs whose lease lapsed.
const SWEEP_EVERY: Duration = Duration::from_secs(5);

/// Ended jobs removed at most per claim.
const RETENTION_BATCH: i64 = 100;

/// Longest `backend_target` and `result_ref` kept (their columns' width).
const TARGET_MAX: usize = 255;

/// Longest `error_code` kept (its column's width).
const CODE_MAX: usize = 64;

/// The error code of a job whose process went away while it ran.
pub const INTERRUPTED: &str = "job_interrupted";

/// How long letting this process's jobs go may hold up its exit.
const RELEASE_WITHIN: Duration = Duration::from_secs(5);

/// This process was told to stop: it claims nothing more.
static STOPPING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// This process's jobs were let go at its exit.
static RELEASED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The jobs this process runs and still holds, by id: no sweep of its own fails them.
static HELD: LazyLock<Mutex<std::collections::HashSet<String>>> = LazyLock::new(Default::default);

/// This process stops claiming jobs: it is going away.
pub fn begin_stopping() {
    STOPPING.store(true, std::sync::atomic::Ordering::Release);
}

pub fn stopping() -> bool {
    STOPPING.load(std::sync::atomic::Ordering::Acquire)
}

/// A job this process runs, held until `let_go`: its own sweeps leave it be.
pub fn hold(id: &str) {
    HELD.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(id.to_owned());
}

pub fn let_go(id: &str) {
    HELD.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(id);
}

fn held() -> Vec<String> {
    HELD.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .cloned()
        .collect()
}

/// This process, as the runner of the jobs it claims.
static RUNNER: LazyLock<String> =
    LazyLock::new(|| format!("proc-{}", uuid::Uuid::new_v4().simple()));

pub fn this_process() -> &'static str {
    &RUNNER
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "succeeded" => Self::Succeeded,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }

    pub fn ended(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// A job to claim.
#[derive(Clone, Debug)]
pub struct NewJob {
    pub id: String,
    pub kind: String,
    /// `user:<id>`.
    pub caller: String,
    /// Whose it is: whose storage its results are kept in.
    pub owner: Owner,
    pub idempotency_key: Option<String>,
    /// SHA-256 hex of what was asked (`digest`).
    pub request_digest: String,
    /// A running job of the same subject is joined instead of started again.
    pub subject: Option<String>,
    /// What was asked, in the caller's terms (`request_digest` is of it).
    pub input: Value,
    /// What the job's steps run from: its picture and what is made of it.
    pub pipeline_input: Value,
    pub progress: Value,
    pub ledger_task_id: String,
    /// The process that will run it.
    pub runner: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Claimed {
    /// The job claimed is a new one, and the caller runs it.
    New(String),
    /// The job given for the same key, or the running job of the same subject.
    Existing(String),
}

#[derive(Debug)]
pub enum ClaimError {
    /// `at_once` jobs are running already.
    Busy,
    /// The key was used before for something else.
    KeyReused,
    /// This process is stopping.
    Stopping,
    Db(DbErr),
}

impl From<DbErr> for ClaimError {
    fn from(error: DbErr) -> Self {
        Self::Db(error)
    }
}

/// SHA-256 hex of a job's input, to tell one submission from another.
pub fn digest(input: &Value) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(input.to_string().as_bytes()))
}

/// The caller of a site account.
pub fn user_caller(user_id: i32) -> String {
    format!("user:{user_id}")
}

/// The ledger task id a job's paid calls are written under.
pub fn ledger_task_id(job_id: &str) -> String {
    format!("rig-job:{job_id}")
}

/// `target` cut to what its column keeps.
pub fn backend_target(target: &str) -> String {
    cut(target, TARGET_MAX)
}

/// `value` cut to `chars` characters.
fn cut(value: &str, chars: usize) -> String {
    match value.char_indices().nth(chars) {
        Some((at, _)) => value[..at].to_owned(),
        None => value.to_owned(),
    }
}

/// Whether the database refused a statement for what it says (bad data, a
/// broken constraint, a statement it cannot run), so that the same
/// statement fails the same way however often it is tried; a dropped
/// connection or an exhausted pool is not.
pub fn refused_for_good(error: &DbErr) -> bool {
    let (DbErr::Exec(sea_orm::RuntimeErr::SqlxError(error))
    | DbErr::Query(sea_orm::RuntimeErr::SqlxError(error))) = error
    else {
        return false;
    };
    let sea_orm::sqlx::Error::Database(error) = error.as_ref() else {
        return false;
    };
    error.code().is_some_and(|code| {
        ["22", "23", "42"]
            .iter()
            .any(|class| code.starts_with(class))
    })
}

const CLAIM_LOCK: &str = "rig_jobs:claim";

/// What a caller's key gave: its job, what was asked under it, and how the job stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keyed {
    pub job_id: String,
    pub request_digest: String,
    pub state: State,
}

impl Keyed {
    /// The key still gives its job: the job has not failed or been cancelled.
    /// A key that no longer holds is free, for any request.
    pub fn holds(&self) -> bool {
        !matches!(self.state, State::Failed | State::Cancelled)
    }
}

/// Claims `job`: the job a key gave before, the running job of the same
/// subject, or `job` itself when fewer than `at_once` run.
pub async fn claim(
    db: &DatabaseConnection,
    job: NewJob,
    at_once: u32,
) -> Result<Claimed, ClaimError> {
    claim_unless(db, job, at_once, &STOPPING).await
}

/// `claim`, refused once `stopping` is set.
pub(crate) async fn claim_unless(
    db: &DatabaseConnection,
    job: NewJob,
    at_once: u32,
    stopping: &std::sync::atomic::AtomicBool,
) -> Result<Claimed, ClaimError> {
    if stopping.load(std::sync::atomic::Ordering::Acquire) {
        return Err(ClaimError::Stopping);
    }
    let txn = db.begin().await?;
    lock_claims(&txn).await?;
    fail_interrupted(&txn).await?;
    tidy(&txn).await?;
    if let Some(key) = &job.idempotency_key
        && let Some(keyed) = by_key(&txn, &job.caller, key).await?
    {
        if keyed.holds() {
            if keyed.request_digest != job.request_digest {
                return Err(ClaimError::KeyReused);
            }
            txn.commit().await?;
            return Ok(Claimed::Existing(keyed.job_id));
        }
        // Its job failed or was cancelled: the key is let go, free for any request.
        txn.execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "DELETE FROM rig_job_keys WHERE caller = $1 AND idempotency_key = $2",
            [job.caller.clone().into(), key.clone().into()],
        ))
        .await?;
    }
    if let Some(subject) = &job.subject {
        let joined = txn
            .query_one_raw(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT id FROM rig_jobs WHERE caller = $1 AND subject = $2 \
                 AND state IN ('queued', 'running') ORDER BY created_at DESC LIMIT 1",
                [job.caller.clone().into(), subject.clone().into()],
            ))
            .await?;
        if let Some(row) = joined {
            let id: String = row.try_get("", "id")?;
            keep_key(&txn, &job, &id).await?;
            txn.commit().await?;
            return Ok(Claimed::Existing(id));
        }
    }
    let active = txn
        .query_one_raw(Statement::from_string(
            DbBackend::Postgres,
            "SELECT COUNT(*) AS n FROM rig_jobs WHERE state IN ('queued', 'running')",
        ))
        .await?
        .map(|row| row.try_get::<i64>("", "n"))
        .transpose()?
        .unwrap_or(0);
    if active >= i64::from(at_once.max(1)) {
        return Err(ClaimError::Busy);
    }
    txn.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO rig_jobs (id, kind, caller, owner_user_id, owner_scope, subject, input, \
         pipeline_input, state, progress, attempts, ledger_task_id, runner, heartbeat_at, \
         started_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'running', $9, 1, $10, $11, NOW(), NOW())",
        [
            job.id.clone().into(),
            job.kind.clone().into(),
            job.caller.clone().into(),
            job.owner.user_id().into(),
            job.owner.scope().into(),
            job.subject.clone().into(),
            job.input.clone().into(),
            job.pipeline_input.clone().into(),
            job.progress.clone().into(),
            job.ledger_task_id.clone().into(),
            job.runner.clone().into(),
        ],
    ))
    .await?;
    keep_key(&txn, &job, &job.id).await?;
    txn.commit().await?;
    Ok(Claimed::New(job.id))
}

/// Claims wait on one another: one lock for every claim and retake.
async fn lock_claims(db: &impl ConnectionTrait) -> Result<(), DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        [CLAIM_LOCK.into()],
    ))
    .await?;
    Ok(())
}

/// A job of this process's, failed as interrupted by a sweep while it ran on,
/// taken back as running: only while no other job has taken the place of it
/// (none active for its subject) and the limit allows it, under the claim
/// lock. False: it stays as it ended.
pub async fn retake(db: &DatabaseConnection, id: &str, at_once: u32) -> Result<bool, DbErr> {
    retake_unless(db, id, at_once, &STOPPING).await
}

async fn retake_unless(
    db: &DatabaseConnection,
    id: &str,
    at_once: u32,
    stopping: &std::sync::atomic::AtomicBool,
) -> Result<bool, DbErr> {
    if stopping.load(std::sync::atomic::Ordering::Acquire) {
        return Ok(false);
    }
    let txn = db.begin().await?;
    lock_claims(&txn).await?;
    let row = txn
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT \
             (SELECT COUNT(*) FROM rig_jobs WHERE state IN ('queued', 'running')) AS active, \
             EXISTS (SELECT 1 FROM rig_jobs o, rig_jobs j WHERE j.id = $1 AND o.id <> j.id \
               AND o.caller = j.caller AND o.subject IS NOT DISTINCT FROM j.subject \
               AND o.state IN ('queued', 'running')) AS replaced",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| DbErr::Custom("retake answered nothing".into()))?;
    let active: i64 = row.try_get("", "active")?;
    let replaced: bool = row.try_get("", "replaced")?;
    if replaced || active >= i64::from(at_once.max(1)) {
        return Ok(false);
    }
    let taken = txn
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "UPDATE rig_jobs SET state = 'running', error_code = NULL, error_message = NULL, \
             ended_at = NULL, runner = $2, heartbeat_at = NOW(), updated_at = NOW() \
             WHERE id = $1 AND state = 'failed' AND error_code = $3",
            [id.into(), this_process().into(), INTERRUPTED.into()],
        ))
        .await?
        .rows_affected()
        == 1;
    txn.commit().await?;
    Ok(taken)
}

/// The submission's key kept as giving `job_id`.
async fn keep_key(db: &impl ConnectionTrait, job: &NewJob, job_id: &str) -> Result<(), DbErr> {
    let Some(key) = &job.idempotency_key else {
        return Ok(());
    };
    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO rig_job_keys (caller, idempotency_key, job_id, request_digest) \
         VALUES ($1, $2, $3, $4)",
        [
            job.caller.clone().into(),
            key.clone().into(),
            job_id.into(),
            job.request_digest.clone().into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Expired keys and ended jobs past their retention removed, a batch at a time.
async fn tidy(db: &impl ConnectionTrait) -> Result<(), DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "DELETE FROM rig_job_keys WHERE created_at < NOW() - $1::interval",
        [format!("{} seconds", KEY_TTL.as_secs()).into()],
    ))
    .await?;
    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "DELETE FROM rig_jobs WHERE id IN (SELECT id FROM rig_jobs \
         WHERE ended_at < NOW() - $1::interval ORDER BY ended_at LIMIT $2)",
        [
            format!("{} seconds", RETENTION.as_secs()).into(),
            RETENTION_BATCH.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// What a caller's key, unexpired, gave.
pub async fn by_key(
    db: &impl ConnectionTrait,
    caller: &str,
    key: &str,
) -> Result<Option<Keyed>, DbErr> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT k.job_id, k.request_digest, j.state FROM rig_job_keys k \
             JOIN rig_jobs j ON j.id = k.job_id \
             WHERE k.caller = $1 AND k.idempotency_key = $2 \
             AND k.created_at >= NOW() - $3::interval",
            [
                caller.into(),
                key.into(),
                format!("{} seconds", KEY_TTL.as_secs()).into(),
            ],
        ))
        .await?;
    row.map(|row| {
        Ok(Keyed {
            job_id: row.try_get("", "job_id")?,
            request_digest: row.try_get("", "request_digest")?,
            state: State::parse(&row.try_get::<String>("", "state")?),
        })
    })
    .transpose()
}

/// Running jobs whose lease lapsed, failed as interrupted, with their running
/// steps; never those this process still runs itself (`hold`). How many
/// there were.
pub async fn fail_interrupted(db: &impl ConnectionTrait) -> Result<u64, DbErr> {
    interrupt(
        db,
        "state IN ('queued', 'running') \
         AND (heartbeat_at IS NULL OR heartbeat_at < NOW() - $2::interval) \
         AND NOT (runner IS NOT DISTINCT FROM $3 AND id = ANY($4))",
        vec![
            format!("{} seconds", LEASE.as_secs()).into(),
            this_process().into(),
            held().into(),
        ],
    )
    .await
}

/// This process's running jobs failed as interrupted at once, so that they do
/// not hold their places until their leases lapse. Once, at its exit, after
/// it stopped answering requests; given at most `RELEASE_WITHIN`.
pub async fn release_at_exit(db: &impl ConnectionTrait) {
    release_once(db, &RELEASED).await;
}

/// `release_at_exit`, done once per `released`.
pub(crate) async fn release_once(
    db: &impl ConnectionTrait,
    released: &std::sync::atomic::AtomicBool,
) {
    if released.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    let released = tokio::time::timeout(
        RELEASE_WITHIN,
        interrupt(
            db,
            "state IN ('queued', 'running') AND runner = $2",
            vec![this_process().into()],
        ),
    )
    .await;
    match released {
        Ok(Ok(0)) => {}
        Ok(Ok(released)) => tracing::info!(released, "rig jobs let go as the process stops"),
        Ok(Err(error)) => {
            tracing::warn!(%error, "rig jobs could not be let go as the process stops")
        }
        Err(_) => {
            tracing::warn!("rig jobs not let go in time as the process stops; their leases lapse")
        }
    }
}

async fn interrupt(
    db: &impl ConnectionTrait,
    which: &str,
    values: Vec<SeaValue>,
) -> Result<u64, DbErr> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            format!(
                "UPDATE rig_jobs SET state = 'failed', error_code = $1, \
                 error_message = 'The job stopped when the process running it went away', \
                 ended_at = NOW(), updated_at = NOW(), runner = NULL \
                 WHERE {which} RETURNING id"
            ),
            std::iter::once(SeaValue::from(INTERRUPTED))
                .chain(values)
                .collect::<Vec<_>>(),
        ))
        .await?;
    let ids = rows
        .iter()
        .map(|row| row.try_get::<String>("", "id"))
        .collect::<Result<Vec<_>, _>>()?;
    if !ids.is_empty() {
        tracing::warn!(jobs = ?ids, "rig jobs interrupted: their process went away");
        db.execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "UPDATE rig_job_steps SET state = 'failed', ended_at = NOW() \
             WHERE job_id = ANY($1) AND state = 'running'",
            [ids.clone().into()],
        ))
        .await?;
    }
    Ok(ids.len() as u64)
}

/// `fail_interrupted`, when this process has not swept for `SWEEP_EVERY`.
async fn sweep(db: &impl ConnectionTrait) -> Result<(), DbErr> {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    {
        let mut last = LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if last.is_some_and(|at| at.elapsed() < SWEEP_EVERY) {
            return Ok(());
        }
        *last = Some(Instant::now());
    }
    fail_interrupted(db).await.map(|_| ())
}

/// A job as its row has it.
#[derive(Clone, Debug, FromQueryResult)]
pub struct JobRow {
    pub id: String,
    pub kind: String,
    pub caller: String,
    pub owner_user_id: Option<i32>,
    /// `site` for the site's own jobs (`Owner::scope`).
    pub owner_scope: String,
    pub input: Value,
    /// None: claimed before jobs kept it, by a process that ran it from what it held.
    pub pipeline_input: Option<Value>,
    state: String,
    pub progress: Value,
    pub attempts: i32,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub error_detail: Option<Value>,
    pub result_ref: Option<String>,
    pub ledger_task_id: String,
    pub runner: Option<String>,
    pub cancel_requested_at: Option<DateTime<Utc>>,
    pub delivered: Value,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
}

impl JobRow {
    pub fn state(&self) -> State {
        State::parse(&self.state)
    }

    /// Whose the job is, as its record names it (`Owner::of_record`).
    pub fn owner(&self) -> Option<Owner> {
        Owner::of_record(&self.owner_scope, self.owner_user_id)
    }

    pub fn interrupted(&self) -> bool {
        self.state() == State::Failed && self.error_code.as_deref() == Some(INTERRUPTED)
    }

    /// Whether the caller has had `part` of the job's end.
    pub fn delivered(&self, part: &str) -> bool {
        self.delivered[part].as_bool().unwrap_or(false)
    }
}

const JOB_COLUMNS: &str = "id, kind, caller, owner_user_id, owner_scope, input, pipeline_input, \
    state, progress, attempts, error_code, error_message, error_detail, result_ref, \
    ledger_task_id, runner, cancel_requested_at, delivered, created_at, started_at, ended_at";

/// The job, the jobs whose lease lapsed failed first (`sweep`).
pub async fn get(db: &DatabaseConnection, id: &str) -> Result<Option<JobRow>, DbErr> {
    sweep(db).await?;
    JobRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        format!("SELECT {JOB_COLUMNS} FROM rig_jobs WHERE id = $1"),
        [id.into()],
    ))
    .one(db)
    .await
}

/// The caller's job claimed last, the jobs whose lease lapsed failed first (`sweep`).
pub async fn latest(db: &DatabaseConnection, caller: &str) -> Result<Option<JobRow>, DbErr> {
    sweep(db).await?;
    JobRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        format!(
            "SELECT {JOB_COLUMNS} FROM rig_jobs WHERE caller = $1 \
             ORDER BY created_at DESC, id DESC LIMIT 1"
        ),
        [caller.into()],
    ))
    .one(db)
    .await
}

/// What a lease renewal found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Beat {
    /// Still this runner's and running: renewed.
    Renewed { cancel_requested: bool },
    /// The job is there but no longer this runner's running job: it was
    /// failed as interrupted while its lease could not be renewed.
    Lost,
    /// The job is gone: its account was deleted.
    Gone,
}

/// The lease renewed by its runner, with the progress when given.
pub async fn heartbeat(
    db: &impl ConnectionTrait,
    id: &str,
    runner: &str,
    progress: Option<&Value>,
) -> Result<Beat, DbErr> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "WITH renewed AS (UPDATE rig_jobs SET heartbeat_at = NOW(), updated_at = NOW(), \
             progress = COALESCE($3, progress) \
             WHERE id = $1 AND runner = $2 AND state = 'running' \
             RETURNING cancel_requested_at IS NOT NULL AS cancel) \
             SELECT (SELECT cancel FROM renewed) AS cancel, \
             EXISTS (SELECT 1 FROM rig_jobs WHERE id = $1) AS there",
            [id.into(), runner.into(), progress.cloned().into()],
        ))
        .await?
        .ok_or_else(|| DbErr::Custom("lease renewal answered nothing".into()))?;
    Ok(match row.try_get::<Option<bool>>("", "cancel")? {
        Some(cancel_requested) => Beat::Renewed { cancel_requested },
        None if row.try_get::<bool>("", "there")? => Beat::Lost,
        None => Beat::Gone,
    })
}

/// A step of a job, as it is written.
#[derive(Clone, Debug, PartialEq)]
pub struct StepRow {
    pub seq: i32,
    pub name: String,
    pub state: State,
    pub attempts: i32,
    pub backend: String,
    pub backend_target: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub wall_seconds: Option<f64>,
    pub gpu_seconds: Option<f64>,
}

/// Steps written as they stand now.
pub async fn save_steps(
    db: &impl ConnectionTrait,
    job_id: &str,
    steps: &[StepRow],
) -> Result<(), DbErr> {
    for step in steps {
        db.execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO rig_job_steps (job_id, seq, name, state, attempts, backend, \
             backend_target, started_at, ended_at, wall_seconds, gpu_seconds) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
             ON CONFLICT (job_id, seq) DO UPDATE SET state = EXCLUDED.state, \
             attempts = EXCLUDED.attempts, ended_at = EXCLUDED.ended_at, \
             wall_seconds = EXCLUDED.wall_seconds, gpu_seconds = EXCLUDED.gpu_seconds",
            [
                job_id.into(),
                step.seq.into(),
                step.name.clone().into(),
                step.state.as_str().into(),
                step.attempts.into(),
                step.backend.clone().into(),
                step.backend_target.as_deref().map(backend_target).into(),
                step.started_at.into(),
                step.ended_at.into(),
                step.wall_seconds.into(),
                step.gpu_seconds.into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

/// A job's steps in the order they began.
pub async fn steps(db: &impl ConnectionTrait, job_id: &str) -> Result<Vec<StepRow>, DbErr> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT seq, name, state, attempts, backend, backend_target, started_at, ended_at, \
             wall_seconds, gpu_seconds FROM rig_job_steps WHERE job_id = $1 ORDER BY seq",
            [job_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            Ok(StepRow {
                seq: row.try_get("", "seq")?,
                name: row.try_get("", "name")?,
                state: State::parse(&row.try_get::<String>("", "state")?),
                attempts: row.try_get("", "attempts")?,
                backend: row.try_get("", "backend")?,
                backend_target: row.try_get("", "backend_target")?,
                started_at: row.try_get("", "started_at")?,
                ended_at: row.try_get("", "ended_at")?,
                wall_seconds: row.try_get("", "wall_seconds")?,
                gpu_seconds: row.try_get("", "gpu_seconds")?,
            })
        })
        .collect()
}

/// How a job ended.
#[derive(Clone, Debug)]
pub enum Ending {
    Succeeded {
        result_ref: Option<String>,
    },
    Failed {
        code: Option<String>,
        message: Option<String>,
        /// The failure as the API answered it.
        detail: Value,
    },
    Cancelled,
}

/// How writing a job's end went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    /// Written over the running job.
    Written,
    /// A success written over the job failed as interrupted while it ran on.
    Reconciled,
    /// Not written: the job had ended already, or is gone.
    Already,
}

/// The job's end written, with its progress as it ended; steps still running
/// end with it. A success is written over a job failed as interrupted too:
/// the process went on and the result was kept.
pub async fn finish(
    db: &DatabaseConnection,
    id: &str,
    ending: Ending,
    progress: &Value,
) -> Result<Ended, DbErr> {
    let (state, code, message, detail, result_ref) = match ending {
        Ending::Succeeded { result_ref } => (State::Succeeded, None, None, None, result_ref),
        Ending::Failed {
            code,
            message,
            detail,
        } => (State::Failed, code, message, Some(detail), None),
        Ending::Cancelled => (State::Cancelled, None, None, None, None),
    };
    // Written within their columns, so that a long value never keeps the end from its record.
    let code = code.map(|code| cut(&code, CODE_MAX));
    let result_ref = result_ref.map(|result_ref| cut(&result_ref, TARGET_MAX));
    let txn = db.begin().await?;
    let was = txn
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT state, error_code FROM rig_jobs WHERE id = $1 FOR UPDATE",
            [id.into()],
        ))
        .await?
        .map(|row| {
            Ok::<_, DbErr>((
                State::parse(&row.try_get::<String>("", "state")?),
                row.try_get::<Option<String>>("", "error_code")?,
            ))
        })
        .transpose()?;
    let ended = match was {
        Some((State::Queued | State::Running, _)) => Ended::Written,
        Some((State::Failed, Some(code))) if code == INTERRUPTED && state == State::Succeeded => {
            Ended::Reconciled
        }
        _ => Ended::Already,
    };
    if ended != Ended::Already {
        txn.execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "UPDATE rig_jobs SET state = $2, error_code = $3, error_message = $4, \
             error_detail = $5, result_ref = $6, progress = $7, ended_at = NOW(), \
             updated_at = NOW(), runner = NULL WHERE id = $1",
            [
                id.into(),
                state.as_str().into(),
                code.into(),
                message.into(),
                detail.into(),
                result_ref.into(),
                progress.clone().into(),
            ],
        ))
        .await?;
    }
    // A step still running when the job succeeded is one it went on without.
    let step_state = if state == State::Succeeded {
        State::Cancelled
    } else {
        state
    };
    txn.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "UPDATE rig_job_steps SET state = $2, ended_at = NOW() \
         WHERE job_id = $1 AND state = 'running'",
        [id.into(), step_state.as_str().into()],
    ))
    .await?;
    txn.commit().await?;
    Ok(ended)
}

/// Cancel asked of the caller's job. The job as it stands; None: no such
/// job of the caller's.
pub async fn request_cancel(
    db: &DatabaseConnection,
    caller: &str,
    id: &str,
) -> Result<Option<JobRow>, DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "UPDATE rig_jobs SET cancel_requested_at = COALESCE(cancel_requested_at, NOW()), \
         updated_at = NOW() WHERE id = $1 AND caller = $2 AND state IN ('queued', 'running')",
        [id.into(), caller.into()],
    ))
    .await?;
    Ok(get(db, id).await?.filter(|job| job.caller == caller))
}

/// `part` of the job's end counted as had by its caller.
pub async fn deliver(db: &DatabaseConnection, id: &str, part: &str) -> Result<(), DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "UPDATE rig_jobs SET delivered = delivered || jsonb_build_object($2::text, true), \
         updated_at = NOW() WHERE id = $1",
        [id.into(), part.into()],
    ))
    .await?;
    Ok(())
}

/// Paid calls of one outcome, as the ledger has them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Calls {
    pub calls: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// What the calls that reported a cost cost, in micro-dollars.
    pub priced_micro_usd: i64,
    /// Calls with no cost reported (their tokens may be estimates).
    pub unpriced_calls: i64,
}

/// What a job's paid calls cost: those that completed, and apart from them
/// those that failed (a provider may or may not bill a failed call).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Spend {
    pub completed: Calls,
    pub failed: Calls,
}

/// The ledger rows written under the job's ledger id since it was claimed.
pub async fn spend(db: &DatabaseConnection, job: &JobRow) -> Result<Spend, DbErr> {
    let (filter, values): (&str, Vec<SeaValue>) = match job.owner_user_id {
        // The ledger is indexed by subject and time.
        Some(owner) => (
            "subject_id = $3 AND",
            vec![
                job.ledger_task_id.clone().into(),
                job.created_at.into(),
                owner.into(),
            ],
        ),
        None => (
            "",
            vec![job.ledger_task_id.clone().into(), job.created_at.into()],
        ),
    };
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            format!(
                "SELECT status = 'completed' AS completed, COUNT(*) AS calls, \
                 COALESCE(SUM(input_tokens), 0)::BIGINT AS input_tokens, \
                 COALESCE(SUM(output_tokens), 0)::BIGINT AS output_tokens, \
                 COALESCE(SUM(cost_micro_usd), 0)::BIGINT AS priced, \
                 COUNT(*) FILTER (WHERE cost_micro_usd IS NULL) AS unpriced \
                 FROM ai_cost_ledger WHERE {filter} task_id = $1 AND occurred_at >= $2 \
                 GROUP BY 1"
            ),
            values,
        ))
        .await?;
    let mut spend = Spend::default();
    for row in rows {
        let calls = Calls {
            calls: row.try_get("", "calls")?,
            input_tokens: row.try_get("", "input_tokens")?,
            output_tokens: row.try_get("", "output_tokens")?,
            priced_micro_usd: row.try_get("", "priced")?,
            unpriced_calls: row.try_get("", "unpriced")?,
        };
        if row.try_get::<bool>("", "completed")? {
            spend.completed = calls;
        } else {
            spend.failed = calls;
        }
    }
    Ok(spend)
}

#[cfg(test)]
#[path = "rig_jobs_tests.rs"]
mod tests;
