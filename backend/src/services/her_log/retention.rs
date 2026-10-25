//! What the server keeps of her log, and for how long.
//!
//! - A forgotten row's payload and its object's key go the moment the
//!   forget is taken ([`super::store::take`]); the stub and the ledger stay,
//!   so a side that comes back with old rows is answered with the forget
//!   first.
//! - Download links for a first full sync go once used or expired.
//! - When the owner turns syncing off, everything of her log on the server
//!   (rows, payloads, keys, ledger, counts, the lease and what it knew of
//!   each end) is deleted after the retention period
//!   ([`super::Settings::retention_days`]). The account row stays, stopped,
//!   with its region.
//!
//! The database's own backups are the deployment's and are not reached
//! from here: a payload in an old backup does not open once its key is
//! gone, but the backup itself lasts as long as the deployment keeps it.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};

use super::{Error, Settings};

/// What one sweep deleted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Swept {
    pub links: u64,
    /// Whether her whole log on the server went (syncing off for longer
    /// than the retention period).
    pub log: bool,
}

/// One sweep at `now`.
pub async fn sweep(
    db: &DatabaseConnection,
    settings: &Settings,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Swept, Error> {
    let links = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM agent_log_full_links WHERE used_at IS NOT NULL OR expires_at <= $1",
            [now.into()],
        ))
        .await?
        .rows_affected();
    let cutoff = now - chrono::Duration::days(settings.retention_days);
    // One owner, one her: her log goes when every account is off past the
    // period, and only then.
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) FILTER (WHERE stopped_at IS NULL OR stopped_at > $1) AS kept, \
                    COUNT(*) AS accounts \
             FROM agent_log_accounts",
            [cutoff.into()],
        ))
        .await?;
    let (kept, accounts) = match row {
        Some(row) => (
            row.try_get::<i64>("", "kept")?,
            row.try_get::<i64>("", "accounts")?,
        ),
        None => (0, 0),
    };
    let mut swept = Swept { links, log: false };
    if accounts > 0 && kept == 0 {
        let txn = db.begin().await?;
        // The grants refuse deletion row by row (they are only ever added);
        // TRUNCATE is the one way to let them all go at once.
        txn.execute_unprepared(
            "DELETE FROM agent_log_payloads; \
             DELETE FROM agent_log_rows; \
             DELETE FROM agent_log_object_keys; \
             DELETE FROM agent_log_audience_keys; \
             DELETE FROM agent_log_ledger; \
             DELETE FROM agent_log_counters; \
             DELETE FROM agent_log_clock; \
             DELETE FROM agent_log_full_links; \
             DELETE FROM agent_lease_ends; \
             TRUNCATE agent_lease_grants;",
        )
        .await?;
        txn.commit().await?;
        swept.log = true;
        tracing::info!(
            "[her-log] syncing was off past the retention period: her log on the server was deleted"
        );
    }
    Ok(swept)
}

/// How often the sweep and the takeover check run.
pub const EVERY: std::time::Duration = std::time::Duration::from_secs(15);

/// Runs the sweep (every hour) and the takeover check (every [`EVERY`])
/// while her log on the server is on. Nothing starts when it is off.
pub fn start(db: DatabaseConnection) {
    let settings = super::settings();
    if !settings.enabled {
        return;
    }
    tracing::info!("[her-log] her log on the server is on");
    crate::services::jobs::jobs().supervised(
        "her log retention",
        crate::services::jobs::Backoff::new(EVERY, std::time::Duration::from_secs(60)),
        move || run(db.clone(), settings),
    );
}

async fn run(db: DatabaseConnection, settings: &Settings) {
    let mut ticks: u64 = 0;
    let mut interval = tokio::time::interval(EVERY);
    loop {
        interval.tick().await;
        let now = super::now();
        if let Err(error) = super::lease::watch(&db, settings, now).await {
            tracing::warn!(%error, "[her-log] the takeover check failed");
        }
        if ticks % 240 == 0
            && let Err(error) = sweep(&db, settings, crate::clock::now()).await
        {
            tracing::warn!(%error, "[her-log] the retention sweep failed");
        }
        ticks = ticks.wrapping_add(1);
    }
}
