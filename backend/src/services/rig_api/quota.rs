//! A tenant's quotas: jobs running at once, jobs started per period, and
//! requests per minute.
//!
//! Every count is a row in the database, and every check that moves one is
//! made where the database serializes it, so the limits hold however many
//! backend processes serve the API:
//!
//! - A request is counted by one statement that adds it to the tenant's row
//!   for the minute only while the row is under the limit
//!   ([`count_request`]).
//! - A job is let start by [`admit_job`](super::admission::admit_job), in one
//!   transaction holding the tenant's row lock: it frees the places of jobs
//!   that ended, counts the places held and the jobs started in the period,
//!   and takes a place and a count when both are under their limits. Admissions of one tenant wait
//!   on one another; other tenants' do not.
//!
//! A place is held while `rig_jobs` has the job queued or running, and,
//! before the job is there, for [`CLAIM_GRACE`] after it was taken; the job
//! ending frees it with no call from the job's runner. A job that was not
//! started after all (its claim refused, or another job joined in its
//! place) gives its place and count back
//! ([`release_job`](super::admission::release_job)).
//!
//! Limits a tenant has none of its own for are the site's defaults
//! ([`site_defaults`]), read from the database.

use chrono::{DateTime, DurationRound, TimeDelta, Utc};
use sea_orm::{ConnectionTrait, DbErr};
use serde::Serialize;

use super::tenants::{Overrides, Tenant};
use super::{Period, statement};
use crate::config::DynamicConfig;
use crate::services::config_service::ConfigService;

/// How long a place taken for a job holds before the job is in `rig_jobs`.
pub const CLAIM_GRACE: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// The settings the site's defaults are kept in.
const DEFAULT_KEYS: [&str; 4] = [
    "rig_api_concurrent_jobs",
    "rig_api_jobs_per_period",
    "rig_api_job_period",
    "rig_api_requests_per_minute",
];

/// A place is held: its job is queued or running, or not there yet and
/// taken since `$2`.
pub(super) const HELD: &str = "(EXISTS (SELECT 1 FROM rig_jobs j WHERE j.id = s.job_id \
                       AND j.state IN ('queued', 'running')) \
                    OR (NOT EXISTS (SELECT 1 FROM rig_jobs j WHERE j.id = s.job_id) \
                        AND s.reserved_at >= $2))";

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

/// The minute `now` falls in.
fn minute_of(now: DateTime<Utc>) -> DateTime<Utc> {
    now.duration_trunc(TimeDelta::minutes(1))
        .expect("a time truncates to its minute")
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
    },
    /// Over the limit, and not counted.
    Limited {
        limit: i32,
        resets_at: DateTime<Utc>,
        retry_after_secs: u64,
    },
}

/// Counts a request of `tenant_id`'s made at `now`, if the minute's requests
/// are under `limit`. One statement adds it or leaves the row as it is, so
/// requests made at once in any number of processes never go past the limit.
pub async fn count_request<C: ConnectionTrait>(
    db: &C,
    tenant_id: &str,
    limit: i32,
    now: DateTime<Utc>,
) -> Result<RateCount, DbErr> {
    let window = minute_of(now);
    let resets_at = window + TimeDelta::minutes(1);
    let limited = RateCount::Limited {
        limit,
        resets_at,
        retry_after_secs: seconds_until(now, resets_at),
    };
    if limit <= 0 {
        return Ok(limited);
    }
    let row = db
        .query_one_raw(statement(
            "INSERT INTO rig_api_request_windows (tenant_id, window_start, requests) \
             VALUES ($1, $2, 1) \
             ON CONFLICT (tenant_id, window_start) DO UPDATE \
             SET requests = rig_api_request_windows.requests + 1 \
             WHERE rig_api_request_windows.requests < $3 \
             RETURNING requests",
            vec![tenant_id.into(), window.into(), limit.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(limited);
    };
    let used: i32 = row.try_get("", "requests")?;
    if used == 1 {
        // The first request of a new minute: the minutes before it are done with.
        db.execute_raw(statement(
            "DELETE FROM rig_api_request_windows WHERE tenant_id = $1 AND window_start < $2",
            vec![tenant_id.into(), window.into()],
        ))
        .await?;
    }
    Ok(RateCount::Counted {
        used,
        limit,
        resets_at,
    })
}

pub(super) fn grace_start(now: DateTime<Utc>) -> DateTime<Utc> {
    now - TimeDelta::from_std(CLAIM_GRACE).expect("the grace is a few minutes")
}

pub(super) async fn count_held<C: ConnectionTrait>(
    db: &C,
    tenant_id: &str,
    now: DateTime<Utc>,
) -> Result<i64, DbErr> {
    Ok(db
        .query_one_raw(statement(
            &format!(
                "SELECT COUNT(*)::BIGINT AS n FROM rig_api_job_slots s \
                 WHERE s.tenant_id = $1 AND {HELD}"
            ),
            vec![tenant_id.into(), grace_start(now).into()],
        ))
        .await?
        .map(|row| row.try_get::<i64>("", "n"))
        .transpose()?
        .unwrap_or(0))
}

pub(super) async fn jobs_in<C: ConnectionTrait>(
    db: &C,
    tenant_id: &str,
    period_start: DateTime<Utc>,
) -> Result<i32, DbErr> {
    Ok(db
        .query_one_raw(statement(
            "SELECT jobs FROM rig_api_job_usage WHERE tenant_id = $1 AND period_start = $2",
            vec![tenant_id.into(), period_start.into()],
        ))
        .await?
        .map(|row| row.try_get::<i32>("", "jobs"))
        .transpose()?
        .unwrap_or(0))
}

/// What a tenant has used of its limits at `now`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub concurrent_jobs: i64,
    pub jobs_this_period: i32,
    pub period_start: DateTime<Utc>,
    pub period_resets_at: DateTime<Utc>,
    pub requests_this_minute: i32,
    pub minute_resets_at: DateTime<Utc>,
}

pub async fn usage<C: ConnectionTrait>(
    db: &C,
    tenant: &Tenant,
    limits: &Limits,
    now: DateTime<Utc>,
) -> Result<Usage, DbErr> {
    let period_start = limits.job_period.start(now);
    let minute = minute_of(now);
    let requests = db
        .query_one_raw(statement(
            "SELECT requests FROM rig_api_request_windows \
             WHERE tenant_id = $1 AND window_start = $2",
            vec![tenant.id.clone().into(), minute.into()],
        ))
        .await?
        .map(|row| row.try_get::<i32>("", "requests"))
        .transpose()?
        .unwrap_or(0);
    Ok(Usage {
        concurrent_jobs: count_held(db, &tenant.id, now).await?,
        jobs_this_period: jobs_in(db, &tenant.id, period_start).await?,
        period_start,
        period_resets_at: limits.job_period.next(period_start),
        requests_this_minute: requests,
        minute_resets_at: minute + TimeDelta::minutes(1),
    })
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use chrono::TimeZone;

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
    fn periods_start_and_end_on_utc_days_and_months() {
        let at = Utc.with_ymd_and_hms(2026, 12, 31, 23, 59, 59).unwrap();
        let day = Period::Day.start(at);
        assert_eq!(day, Utc.with_ymd_and_hms(2026, 12, 31, 0, 0, 0).unwrap());
        assert_eq!(
            Period::Day.next(day),
            Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap()
        );
        let month = Period::Month.start(at);
        assert_eq!(month, Utc.with_ymd_and_hms(2026, 12, 1, 0, 0, 0).unwrap());
        assert_eq!(
            Period::Month.next(month),
            Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap()
        );
        let january = Utc.with_ymd_and_hms(2027, 1, 31, 12, 0, 0).unwrap();
        assert_eq!(
            Period::Month.next(Period::Month.start(january)),
            Utc.with_ymd_and_hms(2027, 2, 1, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn retry_after_is_whole_seconds_to_the_next_minute() {
        let at =
            Utc.with_ymd_and_hms(2026, 11, 28, 10, 0, 59).unwrap() + TimeDelta::milliseconds(400);
        let next = minute_of(at) + TimeDelta::minutes(1);
        assert_eq!(seconds_until(at, next), 1);
        let start = Utc.with_ymd_and_hms(2026, 11, 28, 10, 0, 0).unwrap();
        assert_eq!(seconds_until(start, next), 60);
    }
}
