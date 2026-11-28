//! Letting a tenant's job start ([`admit_job_in`]): in the transaction that
//! inserts the job's `rig_jobs` row, under the tenant's row lock, so that
//! the count of its running jobs and of the jobs it started this period
//! cannot change between the check and the insert. Nothing is held for a
//! job before its row is there and nothing needs giving back: a transaction
//! that does not commit leaves no count behind. A job counted that is not
//! to be held against the tenant is given back with [`refund_job_in`].
//!
//! No route calls this: the rig API does not take jobs.
#![cfg_attr(
    not(test),
    expect(dead_code, reason = "the rig API does not take jobs")
)]

use chrono::{DateTime, NaiveDate, Utc};
use sea_orm::{ConnectionTrait, DatabaseTransaction, DbErr};

use super::quota::{
    Clock, count_job, limits_for, period_use, running_jobs, seconds_until, tenant_caller,
};
use super::statement;
use super::tenants::Tenant;

/// A job let start: the caller it is to be inserted under, and the period
/// it was counted in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Admission {
    pub tenant_id: String,
    /// `rig_jobs.caller` for the job ([`tenant_caller`]): what the tenant's
    /// running jobs are counted by.
    pub caller: String,
    pub period_start: DateTime<Utc>,
    /// The UTC day the job was counted on: the date of its `created_at`
    /// when it is inserted in the admitting transaction, whose `now()` that
    /// is. What [`refund_job_in`] takes.
    pub counted_on: NaiveDate,
}

/// Why a job was not let start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// `limit` jobs are queued or running already.
    Concurrency {
        limit: i32,
        running: i64,
    },
    /// `limit` jobs were started this period already. A limit of none has
    /// no period end to wait for.
    Period {
        limit: i32,
        used: i64,
        resets_at: Option<DateTime<Utc>>,
        retry_after_secs: Option<u64>,
    },
    TenantDisabled,
    TenantMissing,
}

#[derive(Debug)]
pub enum AdmitError {
    Refused(Refusal),
    Db(DbErr),
}

impl From<DbErr> for AdmitError {
    fn from(error: DbErr) -> Self {
        Self::Db(error)
    }
}

/// Lets a job of `tenant_id`'s start, in `txn`, if the tenant's limits
/// allow it, and counts it in this period.
///
/// `txn` is the transaction that then inserts the job into `rig_jobs` under
/// [`Admission::caller`], queued or running, and commits. Until it ends it
/// holds the tenant's row lock, so admissions of one tenant, from any
/// process, wait on one another (other tenants' do not). Jobs whose lease
/// lapsed are failed first (`rig_jobs::fail_interrupted`, as a claim does),
/// so a job whose process went away holds no place.
pub async fn admit_job_in(
    txn: &DatabaseTransaction,
    tenant_id: &str,
    clock: Clock,
) -> Result<Admission, AdmitError> {
    let row = txn
        .query_one_raw(statement(
            &format!(
                "SELECT {} FROM rig_api_tenants WHERE id = $1 FOR NO KEY UPDATE",
                super::tenants::COLUMNS
            ),
            vec![tenant_id.into()],
        ))
        .await?
        .ok_or(AdmitError::Refused(Refusal::TenantMissing))?;
    let tenant = Tenant::from_row(&row, "")?;
    if tenant.disabled() {
        return Err(AdmitError::Refused(Refusal::TenantDisabled));
    }
    let limits = limits_for(txn, tenant.overrides).await?;
    crate::services::rig_jobs::fail_interrupted(txn).await?;
    let running = running_jobs(txn, tenant_id).await?;
    if running >= i64::from(limits.concurrent_jobs) {
        return Err(AdmitError::Refused(Refusal::Concurrency {
            limit: limits.concurrent_jobs,
            running,
        }));
    }
    let period = period_use(txn, tenant_id, limits.job_period, clock).await?;
    if period.used >= i64::from(limits.jobs_per_period) {
        let frees = limits.jobs_per_period > 0;
        return Err(AdmitError::Refused(Refusal::Period {
            limit: limits.jobs_per_period,
            used: period.used,
            resets_at: frees.then_some(period.resets_at),
            retry_after_secs: frees.then(|| seconds_until(period.at, period.resets_at)),
        }));
    }
    count_job(txn, tenant_id, period.today).await?;
    Ok(Admission {
        tenant_id: tenant_id.to_owned(),
        caller: tenant_caller(tenant_id),
        period_start: period.start,
        counted_on: period.today,
    })
}

/// Gives back the count of a job of `tenant_id`'s counted on `day`, for a
/// job that is not to be held against the tenant (one that failed for the
/// platform's own reasons). False when that day has nothing to give back.
/// Giving back the same job once is the caller's to see to: the count does
/// not know which jobs it holds.
pub async fn refund_job_in<C: ConnectionTrait>(
    db: &C,
    tenant_id: &str,
    day: NaiveDate,
) -> Result<bool, DbErr> {
    Ok(db
        .execute_raw(statement(
            "UPDATE rig_api_job_usage SET jobs = jobs - 1 \
             WHERE tenant_id = $1 AND day = $2 AND jobs > 0",
            vec![tenant_id.into(), day.into()],
        ))
        .await?
        .rows_affected()
        > 0)
}
