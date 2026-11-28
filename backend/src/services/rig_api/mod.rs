//! The rig API's customers (`migrations/rig_api.sql`).
//!
//! A tenant is an account of the API, owned by a site user ([`tenants`]).
//! It calls with keys that are shown once and kept only as a keyed hash
//! ([`keys`]), and it is held to its quotas ([`quota`]): jobs running at
//! once, jobs started per period, and requests per minute. A limit a tenant
//! has none of its own for is the site's default (the `rig_api_*` settings),
//! read from the database when it is needed, so every backend process holds
//! a tenant to the same limits.
//!
//! None of this is a site session: a key is not a JWT, so no site route
//! takes one, and the rig API's routes take nothing else.

pub mod admission;
pub mod keys;
pub mod quota;
pub mod tenants;

#[cfg(test)]
mod tests;

use chrono::{DateTime, Datelike, TimeZone, Utc};
use sea_orm::{DatabaseBackend, Statement};
use serde::{Deserialize, Serialize};

/// What a tenant's jobs-per-period quota counts over: a UTC day or a UTC
/// calendar month.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Period {
    Day,
    Month,
}

impl Period {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Month => "month",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "day" => Some(Self::Day),
            "month" => Some(Self::Month),
            _ => None,
        }
    }

    /// The start of the period `at` falls in.
    pub fn start(self, at: DateTime<Utc>) -> DateTime<Utc> {
        let day = at.date_naive();
        let first = match self {
            Self::Day => day,
            Self::Month => day.with_day(1).expect("every month has a first day"),
        };
        Utc.from_utc_datetime(&first.and_hms_opt(0, 0, 0).expect("midnight exists"))
    }

    /// The start of the period after the one starting at `start`.
    pub fn next(self, start: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            Self::Day => start + chrono::Duration::days(1),
            Self::Month => start
                .checked_add_months(chrono::Months::new(1))
                .expect("a month after a period start is representable"),
        }
    }
}

fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values)
}

/// `columns` of the table aliased `table`, each selected as `<table>_<column>`.
fn aliased(columns: &str, table: &str) -> String {
    columns
        .split(',')
        .map(str::trim)
        .map(|column| format!("{table}.{column} AS {table}_{column}"))
        .collect::<Vec<_>>()
        .join(", ")
}
