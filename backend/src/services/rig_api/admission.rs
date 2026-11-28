//! Letting a tenant's job start: a place among its concurrent jobs and a
//! count in its period, taken in one transaction under the tenant's row
//! lock ([`admit_job`]), and given back for a job that was not started after
//! all ([`release_job`]). See [`super::quota`] for how places are held.
//!
//! No route calls these: the rig API does not take jobs.
#![cfg_attr(
    not(test),
    expect(dead_code, reason = "the rig API does not take jobs")
)]

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr, TransactionTrait};

use super::quota::{HELD, count_held, grace_start, jobs_in, limits_for};
use super::statement;
use super::tenants::{Overrides, Tenant};

/// A place and a count a job was given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Admission {
    pub tenant_id: String,
    pub job_id: String,
    /// The period the job was counted in.
    pub period_start: DateTime<Utc>,
}

/// Why a job was not let start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// `limit` jobs hold their places already.
    Concurrency {
        limit: i32,
        running: i64,
    },
    /// `limit` jobs were started this period already.
    Period {
        limit: i32,
        used: i32,
        resets_at: DateTime<Utc>,
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

/// Lets `job_id` of `tenant_id`'s start at `now` if the tenant's limits
/// allow it: a place among its concurrent jobs and a count in this period
/// are taken for it. The same job let start before gets the admission it
/// got, and nothing is taken again.
///
/// The job is then claimed in `rig_jobs` under `job_id` within
/// [`CLAIM_GRACE`](super::quota::CLAIM_GRACE); a job that is not claimed after all is given back with
/// [`release_job`].
pub async fn admit_job(
    db: &DatabaseConnection,
    tenant_id: &str,
    job_id: &str,
    now: DateTime<Utc>,
) -> Result<Admission, AdmitError> {
    let txn = db.begin().await?;
    let overrides = lock_tenant(&txn, tenant_id).await?;
    let earlier = txn
        .query_one_raw(statement(
            "SELECT period_start FROM rig_api_job_slots WHERE tenant_id = $1 AND job_id = $2",
            vec![tenant_id.into(), job_id.into()],
        ))
        .await?;
    if let Some(row) = earlier {
        let period_start = row.try_get("", "period_start")?;
        txn.commit().await?;
        return Ok(Admission {
            tenant_id: tenant_id.to_owned(),
            job_id: job_id.to_owned(),
            period_start,
        });
    }
    let limits = limits_for(&txn, overrides).await?;
    txn.execute_raw(statement(
        &format!("DELETE FROM rig_api_job_slots s WHERE s.tenant_id = $1 AND NOT {HELD}"),
        vec![tenant_id.into(), grace_start(now).into()],
    ))
    .await?;
    let running = count_held(&txn, tenant_id, now).await?;
    if running >= i64::from(limits.concurrent_jobs) {
        txn.commit().await?;
        return Err(AdmitError::Refused(Refusal::Concurrency {
            limit: limits.concurrent_jobs,
            running,
        }));
    }
    let period_start = limits.job_period.start(now);
    let used = jobs_in(&txn, tenant_id, period_start).await?;
    if used >= limits.jobs_per_period {
        txn.commit().await?;
        return Err(AdmitError::Refused(Refusal::Period {
            limit: limits.jobs_per_period,
            used,
            resets_at: limits.job_period.next(period_start),
        }));
    }
    txn.execute_raw(statement(
        "INSERT INTO rig_api_job_usage (tenant_id, period_start, jobs) VALUES ($1, $2, 1) \
         ON CONFLICT (tenant_id, period_start) DO UPDATE \
         SET jobs = rig_api_job_usage.jobs + 1",
        vec![tenant_id.into(), period_start.into()],
    ))
    .await?;
    txn.execute_raw(statement(
        "INSERT INTO rig_api_job_slots (tenant_id, job_id, period_start, reserved_at) \
         VALUES ($1, $2, $3, $4)",
        vec![
            tenant_id.into(),
            job_id.into(),
            period_start.into(),
            now.into(),
        ],
    ))
    .await?;
    txn.commit().await?;
    Ok(Admission {
        tenant_id: tenant_id.to_owned(),
        job_id: job_id.to_owned(),
        period_start,
    })
}

/// Gives back what `admission` took, for a job that was not started after
/// all: its place, and its count in its period. A job that is in `rig_jobs`
/// is not given back (its place frees itself when it ends): false.
pub async fn release_job(db: &DatabaseConnection, admission: &Admission) -> Result<bool, DbErr> {
    let txn = db.begin().await?;
    match lock_tenant(&txn, &admission.tenant_id).await {
        Ok(_) | Err(AdmitError::Refused(Refusal::TenantDisabled)) => {}
        Err(AdmitError::Refused(_)) => return Ok(false),
        Err(AdmitError::Db(error)) => return Err(error),
    }
    let released = txn
        .execute_raw(statement(
            "DELETE FROM rig_api_job_slots s WHERE s.tenant_id = $1 AND s.job_id = $2 \
             AND NOT EXISTS (SELECT 1 FROM rig_jobs j WHERE j.id = s.job_id)",
            vec![
                admission.tenant_id.clone().into(),
                admission.job_id.clone().into(),
            ],
        ))
        .await?
        .rows_affected()
        > 0;
    if released {
        txn.execute_raw(statement(
            "UPDATE rig_api_job_usage SET jobs = GREATEST(jobs - 1, 0) \
             WHERE tenant_id = $1 AND period_start = $2",
            vec![
                admission.tenant_id.clone().into(),
                admission.period_start.into(),
            ],
        ))
        .await?;
    }
    txn.commit().await?;
    Ok(released)
}

/// Takes the tenant's row lock; its own limits.
async fn lock_tenant<C: ConnectionTrait>(db: &C, tenant_id: &str) -> Result<Overrides, AdmitError> {
    let row = db
        .query_one_raw(statement(
            &format!(
                "SELECT {} FROM rig_api_tenants WHERE id = $1 FOR UPDATE",
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
    Ok(tenant.overrides)
}
