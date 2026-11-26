//! Rig generation jobs as rows (`migrations/rig_jobs.sql`): what a job was
//! asked for and by whom, where it stands, how it ended, where its result is
//! kept, every step it ran with its time and compute, and the ledger id its
//! paid calls are written under.
//!
//! A job is claimed in one transaction under one lock: the caller's key for
//! the submission gives the job it gave before, a running job of the same
//! subject is joined, and at most `at_once` jobs run at a time. The process
//! running a job renews its lease (`heartbeat`); a running job whose lease
//! has lapsed is one whose process went away, and is failed as interrupted
//! (`fail_interrupted`) before anything reads or claims jobs.

use std::time::Duration;

use chrono::{DateTime, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, DbErr, FromQueryResult, Statement,
    TransactionTrait, Value as SeaValue,
};
use serde_json::Value;

/// How long a running job's lease lasts without being renewed.
pub const LEASE: Duration = Duration::from_secs(45);

/// The error code of a job whose process went away while it ran.
pub const INTERRUPTED: &str = "job_interrupted";

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
    pub owner_user_id: Option<i32>,
    pub idempotency_key: Option<String>,
    /// SHA-256 hex of what was asked (`digest`).
    pub request_digest: String,
    /// A running job of the same subject is joined instead of started again.
    pub subject: Option<String>,
    pub input: Value,
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

const CLAIM_LOCK: &str = "rig_jobs:claim";

/// Claims `job`: the job a key gave before, the running job of the same
/// subject, or `job` itself when fewer than `at_once` run.
pub async fn claim(
    db: &DatabaseConnection,
    job: NewJob,
    at_once: u32,
) -> Result<Claimed, ClaimError> {
    let txn = db.begin().await?;
    txn.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        [CLAIM_LOCK.into()],
    ))
    .await?;
    fail_interrupted(&txn).await?;
    if let Some(key) = &job.idempotency_key
        && let Some((id, digest)) = by_key(&txn, &job.caller, key).await?
    {
        if digest != job.request_digest {
            return Err(ClaimError::KeyReused);
        }
        txn.commit().await?;
        return Ok(Claimed::Existing(id));
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
        "INSERT INTO rig_jobs (id, kind, caller, owner_user_id, idempotency_key, request_digest, \
         subject, input, state, progress, attempts, ledger_task_id, runner, heartbeat_at, \
         started_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'running', $9, 1, $10, $11, NOW(), NOW())",
        [
            job.id.clone().into(),
            job.kind.into(),
            job.caller.into(),
            job.owner_user_id.into(),
            job.idempotency_key.into(),
            job.request_digest.into(),
            job.subject.into(),
            job.input.into(),
            job.progress.into(),
            job.ledger_task_id.into(),
            job.runner.into(),
        ],
    ))
    .await?;
    txn.commit().await?;
    Ok(Claimed::New(job.id))
}

/// The job a caller's key gave, and the digest of what it was asked.
pub async fn by_key(
    db: &impl ConnectionTrait,
    caller: &str,
    key: &str,
) -> Result<Option<(String, String)>, DbErr> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT id, request_digest FROM rig_jobs WHERE caller = $1 AND idempotency_key = $2",
            [caller.into(), key.into()],
        ))
        .await?;
    row.map(|row| Ok((row.try_get("", "id")?, row.try_get("", "request_digest")?)))
        .transpose()
}

/// Running jobs whose lease lapsed, failed as interrupted, with their running
/// steps. How many there were.
pub async fn fail_interrupted(db: &impl ConnectionTrait) -> Result<u64, DbErr> {
    let lapsed = format!("{} seconds", LEASE.as_secs());
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "UPDATE rig_jobs SET state = 'failed', error_code = $1, \
             error_message = 'The job stopped when the process running it went away', \
             ended_at = NOW(), updated_at = NOW(), runner = NULL \
             WHERE state IN ('queued', 'running') \
             AND (heartbeat_at IS NULL OR heartbeat_at < NOW() - $2::interval) RETURNING id",
            [INTERRUPTED.into(), lapsed.into()],
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

/// A job as its row has it.
#[derive(Clone, Debug, FromQueryResult)]
pub struct JobRow {
    pub id: String,
    pub kind: String,
    pub caller: String,
    pub owner_user_id: Option<i32>,
    pub idempotency_key: Option<String>,
    pub input: Value,
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

    pub fn interrupted(&self) -> bool {
        self.state() == State::Failed && self.error_code.as_deref() == Some(INTERRUPTED)
    }

    /// Whether the caller has had `part` of the job's end.
    pub fn delivered(&self, part: &str) -> bool {
        self.delivered[part].as_bool().unwrap_or(false)
    }
}

const JOB_COLUMNS: &str = "id, kind, caller, owner_user_id, idempotency_key, input, \
    state, progress, attempts, error_code, error_message, error_detail, result_ref, \
    ledger_task_id, runner, cancel_requested_at, delivered, created_at, started_at, ended_at";

/// The job, after failing those interrupted.
pub async fn get(db: &DatabaseConnection, id: &str) -> Result<Option<JobRow>, DbErr> {
    fail_interrupted(db).await?;
    JobRow::find_by_statement(Statement::from_sql_and_values(
        DbBackend::Postgres,
        format!("SELECT {JOB_COLUMNS} FROM rig_jobs WHERE id = $1"),
        [id.into()],
    ))
    .one(db)
    .await
}

/// The caller's job claimed last, after failing those interrupted.
pub async fn latest(db: &DatabaseConnection, caller: &str) -> Result<Option<JobRow>, DbErr> {
    fail_interrupted(db).await?;
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
pub struct Beat {
    /// The job is still this runner's and still running.
    pub running: bool,
    pub cancel_requested: bool,
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
            "UPDATE rig_jobs SET heartbeat_at = NOW(), updated_at = NOW(), \
             progress = COALESCE($3, progress) \
             WHERE id = $1 AND runner = $2 AND state = 'running' \
             RETURNING cancel_requested_at IS NOT NULL AS cancel",
            [id.into(), runner.into(), progress.cloned().into()],
        ))
        .await?;
    Ok(match row {
        Some(row) => Beat {
            running: true,
            cancel_requested: row.try_get("", "cancel")?,
        },
        None => Beat {
            running: false,
            cancel_requested: false,
        },
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
                step.backend_target.clone().into(),
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

/// The job's end written, with its progress as it ended; steps still running
/// end with it. False: the job had ended already (failed as interrupted
/// while its process could not renew its lease).
pub async fn finish(
    db: &DatabaseConnection,
    id: &str,
    ending: Ending,
    progress: &Value,
) -> Result<bool, DbErr> {
    let (state, code, message, detail, result_ref) = match ending {
        Ending::Succeeded { result_ref } => (State::Succeeded, None, None, None, result_ref),
        Ending::Failed {
            code,
            message,
            detail,
        } => (State::Failed, code, message, Some(detail), None),
        Ending::Cancelled => (State::Cancelled, None, None, None, None),
    };
    let txn = db.begin().await?;
    let ended = txn
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "UPDATE rig_jobs SET state = $2, error_code = $3, error_message = $4, \
             error_detail = $5, result_ref = $6, progress = $7, ended_at = NOW(), \
             updated_at = NOW(), runner = NULL \
             WHERE id = $1 AND state IN ('queued', 'running')",
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
        .await?
        .rows_affected()
        == 1;
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
        "UPDATE rig_jobs SET delivered = delivered || jsonb_build_object($2::text, true) \
         WHERE id = $1",
        [id.into(), part.into()],
    ))
    .await?;
    Ok(())
}

/// What a job's paid calls cost, as the ledger has them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Spend {
    pub calls: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// What the calls that reported a cost cost, in micro-dollars.
    pub priced_micro_usd: i64,
    /// Calls with no cost reported (their tokens may be estimates).
    pub unpriced_calls: i64,
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
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            format!(
                "SELECT COUNT(*) AS calls, \
                 COALESCE(SUM(input_tokens), 0)::BIGINT AS input_tokens, \
                 COALESCE(SUM(output_tokens), 0)::BIGINT AS output_tokens, \
                 COALESCE(SUM(cost_micro_usd), 0)::BIGINT AS priced, \
                 COUNT(*) FILTER (WHERE cost_micro_usd IS NULL) AS unpriced \
                 FROM ai_cost_ledger WHERE {filter} task_id = $1 AND occurred_at >= $2"
            ),
            values,
        ))
        .await?;
    let Some(row) = row else {
        return Ok(Spend::default());
    };
    Ok(Spend {
        calls: row.try_get("", "calls")?,
        input_tokens: row.try_get("", "input_tokens")?,
        output_tokens: row.try_get("", "output_tokens")?,
        priced_micro_usd: row.try_get("", "priced")?,
        unpriced_calls: row.try_get("", "unpriced")?,
    })
}

#[cfg(test)]
#[path = "rig_jobs_tests.rs"]
mod tests;
