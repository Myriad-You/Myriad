//! Tenants: the rig API's customer accounts. Each belongs to a site user and
//! goes with that user's account. A tenant may carry limits of its own; one
//! it does not carry is the site's default ([`super::quota`]). A disabled
//! tenant's keys are refused until it is enabled again.

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DbErr, QueryResult};
use serde::{Deserialize, Serialize};

use super::{Period, statement};

/// Longest tenant name kept (its column's width).
pub const MAX_NAME_CHARS: usize = 120;

/// Highest limit a tenant may be given.
pub const MAX_LIMIT: i32 = 1_000_000;

/// The limits a tenant has of its own. `None` is the site's default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Overrides {
    pub concurrent_jobs: Option<i32>,
    pub jobs_per_period: Option<i32>,
    pub job_period: Option<Period>,
    pub requests_per_minute: Option<i32>,
}

impl Overrides {
    /// Whether every limit given is within `0..=MAX_LIMIT`.
    pub fn valid(&self) -> bool {
        [
            self.concurrent_jobs,
            self.jobs_per_period,
            self.requests_per_minute,
        ]
        .into_iter()
        .flatten()
        .all(|limit| (0..=MAX_LIMIT).contains(&limit))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tenant {
    /// `ten_<uuid simple>`.
    pub id: String,
    pub owner_user_id: i32,
    pub name: String,
    pub overrides: Overrides,
    pub disabled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub(super) const COLUMNS: &str = "id, owner_user_id, name, max_concurrent_jobs, \
     max_jobs_per_period, job_period, requests_per_minute, disabled_at, created_at, updated_at";

impl Tenant {
    pub(super) fn from_row(row: &QueryResult, prefix: &str) -> Result<Self, DbErr> {
        let col = |name: &str| format!("{prefix}{name}");
        let period: Option<String> = row.try_get("", &col("job_period"))?;
        Ok(Self {
            id: row.try_get("", &col("id"))?,
            owner_user_id: row.try_get("", &col("owner_user_id"))?,
            name: row.try_get("", &col("name"))?,
            overrides: Overrides {
                concurrent_jobs: row.try_get("", &col("max_concurrent_jobs"))?,
                jobs_per_period: row.try_get("", &col("max_jobs_per_period"))?,
                job_period: period.as_deref().and_then(Period::parse),
                requests_per_minute: row.try_get("", &col("requests_per_minute"))?,
            },
            disabled_at: row.try_get("", &col("disabled_at"))?,
            created_at: row.try_get("", &col("created_at"))?,
            updated_at: row.try_get("", &col("updated_at"))?,
        })
    }

    pub fn disabled(&self) -> bool {
        self.disabled_at.is_some()
    }
}

#[derive(Debug)]
pub enum TenantError {
    /// The owner given is no site account.
    OwnerMissing,
    /// The name is empty or too long.
    InvalidName,
    /// A limit is out of range.
    InvalidLimit,
    Db(DbErr),
}

impl From<DbErr> for TenantError {
    fn from(error: DbErr) -> Self {
        Self::Db(error)
    }
}

/// `name` as kept: trimmed, and neither empty nor longer than the column.
pub fn clean_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty() && name.chars().count() <= MAX_NAME_CHARS).then(|| name.to_owned())
}

/// A new tenant of `owner_user_id`'s.
pub async fn create<C: ConnectionTrait>(
    db: &C,
    owner_user_id: i32,
    name: &str,
    overrides: Overrides,
    now: DateTime<Utc>,
) -> Result<Tenant, TenantError> {
    let name = clean_name(name).ok_or(TenantError::InvalidName)?;
    if !overrides.valid() {
        return Err(TenantError::InvalidLimit);
    }
    let owner = db
        .query_one_raw(statement(
            "SELECT 1 AS one FROM users WHERE id = $1",
            vec![owner_user_id.into()],
        ))
        .await?;
    if owner.is_none() || owner_user_id <= 0 {
        return Err(TenantError::OwnerMissing);
    }
    let id = format!("ten_{}", uuid::Uuid::new_v4().simple());
    let row = db
        .query_one_raw(statement(
            &format!(
                "INSERT INTO rig_api_tenants (id, owner_user_id, name, max_concurrent_jobs, \
                 max_jobs_per_period, job_period, requests_per_minute, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8) RETURNING {COLUMNS}"
            ),
            vec![
                id.into(),
                owner_user_id.into(),
                name.into(),
                overrides.concurrent_jobs.into(),
                overrides.jobs_per_period.into(),
                overrides
                    .job_period
                    .map(|period| period.as_str().to_owned())
                    .into(),
                overrides.requests_per_minute.into(),
                now.into(),
            ],
        ))
        .await?
        .ok_or(DbErr::RecordNotInserted)?;
    Ok(Tenant::from_row(&row, "")?)
}

pub async fn get<C: ConnectionTrait>(db: &C, id: &str) -> Result<Option<Tenant>, DbErr> {
    db.query_one_raw(statement(
        &format!("SELECT {COLUMNS} FROM rig_api_tenants WHERE id = $1"),
        vec![id.into()],
    ))
    .await?
    .map(|row| Tenant::from_row(&row, ""))
    .transpose()
}

/// Every tenant, newest first.
pub async fn list<C: ConnectionTrait>(db: &C) -> Result<Vec<Tenant>, DbErr> {
    db.query_all_raw(statement(
        &format!("SELECT {COLUMNS} FROM rig_api_tenants ORDER BY created_at DESC, id"),
        vec![],
    ))
    .await?
    .iter()
    .map(|row| Tenant::from_row(row, ""))
    .collect()
}

/// Renames or disables (or enables) a tenant. `None` when there is no such
/// tenant. Disabling one already disabled keeps when it was disabled.
pub async fn update<C: ConnectionTrait>(
    db: &C,
    id: &str,
    name: Option<&str>,
    disabled: Option<bool>,
    now: DateTime<Utc>,
) -> Result<Option<Tenant>, TenantError> {
    let name = name
        .map(|name| clean_name(name).ok_or(TenantError::InvalidName))
        .transpose()?;
    let row = db
        .query_one_raw(statement(
            &format!(
                "UPDATE rig_api_tenants SET \
                 name = COALESCE($2, name), \
                 disabled_at = CASE WHEN $3::BOOLEAN IS NULL THEN disabled_at \
                                    WHEN $3 THEN COALESCE(disabled_at, $4) \
                                    ELSE NULL END, \
                 updated_at = $4 \
                 WHERE id = $1 RETURNING {COLUMNS}"
            ),
            vec![id.into(), name.into(), disabled.into(), now.into()],
        ))
        .await?;
    Ok(row.map(|row| Tenant::from_row(&row, "")).transpose()?)
}

/// Replaces a tenant's own limits; `None` in `overrides` gives that limit
/// back to the site's default. `None` when there is no such tenant.
pub async fn set_overrides<C: ConnectionTrait>(
    db: &C,
    id: &str,
    overrides: Overrides,
    now: DateTime<Utc>,
) -> Result<Option<Tenant>, TenantError> {
    if !overrides.valid() {
        return Err(TenantError::InvalidLimit);
    }
    let row = db
        .query_one_raw(statement(
            &format!(
                "UPDATE rig_api_tenants SET max_concurrent_jobs = $2, max_jobs_per_period = $3, \
                 job_period = $4, requests_per_minute = $5, updated_at = $6 \
                 WHERE id = $1 RETURNING {COLUMNS}"
            ),
            vec![
                id.into(),
                overrides.concurrent_jobs.into(),
                overrides.jobs_per_period.into(),
                overrides
                    .job_period
                    .map(|period| period.as_str().to_owned())
                    .into(),
                overrides.requests_per_minute.into(),
                now.into(),
            ],
        ))
        .await?;
    Ok(row.map(|row| Tenant::from_row(&row, "")).transpose()?)
}
