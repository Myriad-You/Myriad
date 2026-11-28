//! A tenant's quotas: jobs running at once, jobs started per period, and
//! requests per minute.
//!
//! Every count is a row in the database, every check that moves one is made
//! where the database serializes it, and every time it is counted against is
//! the database's clock, so the limits hold however many backend processes
//! serve the API and whatever their clocks say:
//!
//! - A request is counted by one statement that adds it to the tenant's row
//!   for the minute only while the row is under the limit
//!   ([`count_request`]). The minutes are fixed windows: a tenant may make
//!   its limit's worth at the end of one minute and again at the start of
//!   the next.
//! - A job is let start by [`admit_job_in`](super::admission::admit_job_in),
//!   in the transaction that inserts the job, holding the tenant's row lock.
//!
//! A tenant's running jobs are its `rig_jobs` rows queued or running, found
//! by their caller ([`tenant_caller`]). Jobs started are counted per UTC day
//! and a period is the sum of its days.
//!
//! Limits a tenant has none of its own for are the site's defaults
//! ([`site_defaults`]), read from the database.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use sea_orm::{ConnectionTrait, DbErr, QueryResult};
use serde::Serialize;

use super::tenants::{Overrides, Tenant};
use super::{Period, statement};
use crate::config::DynamicConfig;
use crate::services::config_service::ConfigService;

/// The settings the site's defaults are kept in.
const DEFAULT_KEYS: [&str; 4] = [
    "rig_api_concurrent_jobs",
    "rig_api_jobs_per_period",
    "rig_api_job_period",
    "rig_api_requests_per_minute",
];

/// Days of job counts kept: a year and a month.
const USAGE_DAYS_KEPT: i32 = 400;

/// What "now" is for the quotas: the database's clock, the one every
/// process shares. A test gives a time of its own instead, which the same
/// statements take in place of `now()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clock {
    Database,
    #[cfg(test)]
    At(DateTime<Utc>),
}

impl Clock {
    /// The statement parameter that `COALESCE($n::timestamptz, now())` reads.
    pub(super) fn param(self) -> sea_orm::Value {
        match self {
            Self::Database => Option::<DateTime<Utc>>::None.into(),
            #[cfg(test)]
            Self::At(at) => Some(at).into(),
        }
    }
}

/// The caller a tenant's jobs are recorded under in `rig_jobs`.
pub fn tenant_caller(tenant_id: &str) -> String {
    format!("tenant:{tenant_id}")
}

/// The limits a tenant is held to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    pub concurrent_jobs: i32,
    pub jobs_per_period: i32,
    pub job_period: Period,
    pub requests_per_minute: i32,
}

impl Limits {
    /// The site's defaults as `config` has them. A negative number is none
    /// allowed; a period it does not know is a month.
    pub fn from_config(config: &DynamicConfig) -> Self {
        Self {
            concurrent_jobs: config.rig_api_concurrent_jobs.max(0),
            jobs_per_period: config.rig_api_jobs_per_period.max(0),
            job_period: Period::parse(&config.rig_api_job_period).unwrap_or(Period::Month),
            requests_per_minute: config.rig_api_requests_per_minute.max(0),
        }
    }

    /// A tenant's own limits, and `defaults` for the ones it has none of.
    pub fn with(overrides: Overrides, defaults: Limits) -> Self {
        Self {
            concurrent_jobs: overrides
                .concurrent_jobs
                .unwrap_or(defaults.concurrent_jobs),
            jobs_per_period: overrides
                .jobs_per_period
                .unwrap_or(defaults.jobs_per_period),
            job_period: overrides.job_period.unwrap_or(defaults.job_period),
            requests_per_minute: overrides
                .requests_per_minute
                .unwrap_or(defaults.requests_per_minute),
        }
    }
}

fn config_error(error: anyhow::Error) -> DbErr {
    DbErr::Custom(format!("rig API defaults: {error:#}"))
}

/// The site's default limits, as the database has them now.
pub async fn site_defaults<C: ConnectionTrait>(db: &C) -> Result<Limits, DbErr> {
    let stored = ConfigService::load_values_on(db, &DEFAULT_KEYS)
        .await
        .map_err(config_error)?;
    Ok(Limits::from_config(&ConfigService::parse_config(stored)))
}

/// The limits `overrides` comes to: the site's defaults are read only when
/// some limit is not the tenant's own.
pub async fn limits_for<C: ConnectionTrait>(db: &C, overrides: Overrides) -> Result<Limits, DbErr> {
    if let (Some(concurrent_jobs), Some(jobs_per_period), Some(job_period), Some(requests)) = (
        overrides.concurrent_jobs,
        overrides.jobs_per_period,
        overrides.job_period,
        overrides.requests_per_minute,
    ) {
        return Ok(Limits {
            concurrent_jobs,
            jobs_per_period,
            job_period,
            requests_per_minute: requests,
        });
    }
    Ok(Limits::with(overrides, site_defaults(db).await?))
}

/// Whole seconds from `now` until `at`, at least one.
pub fn seconds_until(now: DateTime<Utc>, at: DateTime<Utc>) -> u64 {
    let millis = (at - now).num_milliseconds().max(0);
    u64::try_from((millis + 999) / 1000).unwrap_or(0).max(1)
}

/// How a request was counted against the per-minute limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateCount {
    /// Counted: the requests of this minute, this one included.
    Counted {
        used: i32,
        limit: i32,
        resets_at: DateTime<Utc>,
        retry_after_secs: u64,
    },
    /// Over the limit, and not counted. A limit of none never frees, so it
    /// has no time to come back at.
    Limited {
        limit: i32,
        resets_at: Option<DateTime<Utc>>,
        retry_after_secs: Option<u64>,
    },
}

/// Counts a request of `tenant_id`'s, if this minute's requests are under
/// `limit`. One statement adds it or leaves the row as it is, so requests
/// made at once in any number of processes never go past the limit; the
/// minute is the database's.
pub async fn count_request<C: ConnectionTrait>(
    db: &C,
    tenant_id: &str,
    limit: i32,
    clock: Clock,
) -> Result<RateCount, DbErr> {
    if limit <= 0 {
        return Ok(RateCount::Limited {
            limit,
            resets_at: None,
            retry_after_secs: None,
        });
    }
    let row = db
        .query_one_raw(statement(
            "WITH t AS (SELECT COALESCE($3::timestamptz, now()) AS at), \
             up AS ( \
                 INSERT INTO rig_api_request_windows (tenant_id, window_start, requests) \
                 SELECT $1, date_trunc('minute', t.at), 1 FROM t \
                 ON CONFLICT (tenant_id, window_start) DO UPDATE \
                 SET requests = rig_api_request_windows.requests + 1 \
                 WHERE rig_api_request_windows.requests < $2 \
                 RETURNING requests) \
             SELECT t.at, date_trunc('minute', t.at) + INTERVAL '1 minute' AS resets_at, \
                    (SELECT requests FROM up) AS requests \
             FROM t",
            vec![tenant_id.into(), limit.into(), clock.param()],
        ))
        .await?
        .ok_or(DbErr::RecordNotFound("the database's time".into()))?;
    let at: DateTime<Utc> = row.try_get("", "at")?;
    let resets_at: DateTime<Utc> = row.try_get("", "resets_at")?;
    let retry_after_secs = seconds_until(at, resets_at);
    let Some(used) = row.try_get::<Option<i32>>("", "requests")? else {
        return Ok(RateCount::Limited {
            limit,
            resets_at: Some(resets_at),
            retry_after_secs: Some(retry_after_secs),
        });
    };
    if used == 1 {
        // The first request of a new minute: the minutes before it are done with.
        db.execute_raw(statement(
            "DELETE FROM rig_api_request_windows \
             WHERE tenant_id = $1 AND window_start < $2::timestamptz - INTERVAL '1 minute'",
            vec![tenant_id.into(), resets_at.into()],
        ))
        .await?;
    }
    Ok(RateCount::Counted {
        used,
        limit,
        resets_at,
        retry_after_secs,
    })
}

/// A tenant's jobs queued or running now.
pub(super) async fn running_jobs<C: ConnectionTrait>(
    db: &C,
    tenant_id: &str,
) -> Result<i64, DbErr> {
    Ok(db
        .query_one_raw(statement(
            "SELECT COUNT(*)::BIGINT AS n FROM rig_jobs \
             WHERE caller = $1 AND state IN ('queued', 'running')",
            vec![tenant_caller(tenant_id).into()],
        ))
        .await?
        .map(|row| row.try_get::<i64>("", "n"))
        .transpose()?
        .unwrap_or(0))
}

/// The period `clock`'s now falls in, and what a tenant started in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PeriodUse {
    /// The database's now the period was read at.
    pub at: DateTime<Utc>,
    pub start: DateTime<Utc>,
    pub resets_at: DateTime<Utc>,
    /// Today's UTC date, the row a new job is counted in.
    pub today: NaiveDate,
    pub used: i64,
}

fn midnight(day: NaiveDate) -> DateTime<Utc> {
    Utc.from_utc_datetime(&day.and_hms_opt(0, 0, 0).expect("midnight exists"))
}

pub(super) async fn period_use<C: ConnectionTrait>(
    db: &C,
    tenant_id: &str,
    period: Period,
    clock: Clock,
) -> Result<PeriodUse, DbErr> {
    let row: QueryResult = db
        .query_one_raw(statement(
            "WITH t AS (SELECT COALESCE($3::timestamptz, now()) AT TIME ZONE 'UTC' AS at), \
             p AS (SELECT date_trunc($2, t.at)::date AS start, \
                          (date_trunc($2, t.at) + ('1 ' || $2)::interval)::date AS next, \
                          t.at::date AS today FROM t) \
             SELECT COALESCE($3::timestamptz, now()) AS now, p.start, p.next, p.today, \
                    COALESCE((SELECT SUM(u.jobs) FROM rig_api_job_usage u \
                              WHERE u.tenant_id = $1 AND u.day >= p.start AND u.day < p.next), \
                             0)::BIGINT AS used \
             FROM p",
            vec![tenant_id.into(), period.as_str().into(), clock.param()],
        ))
        .await?
        .ok_or(DbErr::RecordNotFound("the database's time".into()))?;
    Ok(PeriodUse {
        at: row.try_get("", "now")?,
        start: midnight(row.try_get("", "start")?),
        resets_at: midnight(row.try_get("", "next")?),
        today: row.try_get("", "today")?,
        used: row.try_get("", "used")?,
    })
}

/// Counts a job started today for `tenant_id`; the first count of a day
/// removes the days no period reaches back to.
pub(super) async fn count_job<C: ConnectionTrait>(
    db: &C,
    tenant_id: &str,
    today: NaiveDate,
) -> Result<(), DbErr> {
    let jobs: i32 = db
        .query_one_raw(statement(
            "INSERT INTO rig_api_job_usage (tenant_id, day, jobs) VALUES ($1, $2, 1) \
             ON CONFLICT (tenant_id, day) DO UPDATE SET jobs = rig_api_job_usage.jobs + 1 \
             RETURNING jobs",
            vec![tenant_id.into(), today.into()],
        ))
        .await?
        .ok_or(DbErr::RecordNotInserted)?
        .try_get("", "jobs")?;
    if jobs == 1 {
        db.execute_raw(statement(
            "DELETE FROM rig_api_job_usage WHERE tenant_id = $1 AND day < $2::date - $3::INTEGER",
            vec![tenant_id.into(), today.into(), USAGE_DAYS_KEPT.into()],
        ))
        .await?;
    }
    Ok(())
}

/// What a tenant has used of its limits now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub concurrent_jobs: i64,
    pub jobs_this_period: i64,
    pub period_start: DateTime<Utc>,
    pub period_resets_at: DateTime<Utc>,
    pub requests_this_minute: i32,
    pub minute_resets_at: DateTime<Utc>,
}

pub async fn usage<C: ConnectionTrait>(
    db: &C,
    tenant: &Tenant,
    limits: &Limits,
    clock: Clock,
) -> Result<Usage, DbErr> {
    let period = period_use(db, &tenant.id, limits.job_period, clock).await?;
    let minute = db
        .query_one_raw(statement(
            "WITH t AS (SELECT date_trunc('minute', COALESCE($2::timestamptz, now())) AS w) \
             SELECT t.w + INTERVAL '1 minute' AS resets_at, \
                    COALESCE((SELECT requests FROM rig_api_request_windows \
                              WHERE tenant_id = $1 AND window_start = t.w), 0) AS requests \
             FROM t",
            vec![tenant.id.clone().into(), clock.param()],
        ))
        .await?
        .ok_or(DbErr::RecordNotFound("the database's time".into()))?;
    Ok(Usage {
        concurrent_jobs: running_jobs(db, &tenant.id).await?,
        jobs_this_period: period.used,
        period_start: period.start,
        period_resets_at: period.resets_at,
        requests_this_minute: minute.try_get("", "requests")?,
        minute_resets_at: minute.try_get("", "resets_at")?,
    })
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use chrono::TimeDelta;

    #[test]
    fn a_tenants_own_limits_win_and_the_rest_are_the_sites() {
        let defaults = Limits::from_config(&DynamicConfig::default());
        let own = Limits::with(
            Overrides {
                concurrent_jobs: Some(9),
                job_period: Some(Period::Day),
                ..Overrides::default()
            },
            defaults,
        );
        assert_eq!(own.concurrent_jobs, 9);
        assert_eq!(own.job_period, Period::Day);
        assert_eq!(own.jobs_per_period, defaults.jobs_per_period);
        assert_eq!(own.requests_per_minute, defaults.requests_per_minute);
    }

    #[test]
    fn site_defaults_never_go_below_none_and_an_unknown_period_is_a_month() {
        let config = DynamicConfig {
            rig_api_concurrent_jobs: -3,
            rig_api_job_period: "week".into(),
            ..DynamicConfig::default()
        };
        let limits = Limits::from_config(&config);
        assert_eq!(limits.concurrent_jobs, 0);
        assert_eq!(limits.job_period, Period::Month);
    }

    #[test]
    fn retry_after_is_whole_seconds_and_at_least_one() {
        let at =
            Utc.with_ymd_and_hms(2026, 11, 28, 10, 0, 59).unwrap() + TimeDelta::milliseconds(400);
        let next = Utc.with_ymd_and_hms(2026, 11, 28, 10, 1, 0).unwrap();
        assert_eq!(seconds_until(at, next), 1);
        let start = Utc.with_ymd_and_hms(2026, 11, 28, 10, 0, 0).unwrap();
        assert_eq!(seconds_until(start, next), 60);
        assert_eq!(seconds_until(next, start), 1);
    }
}
