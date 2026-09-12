//! Process-local health snapshot.
//!
//! `/health` stays a liveness probe (always HTTP 200). Database and storage
//! fields come from the last probe, not from “a handle exists”. `/ready`
//! refreshes the snapshot with a timed live check.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const LIVE_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

static DB_HANDLE_PRESENT: AtomicBool = AtomicBool::new(false);
static DB_PROBE_OK: AtomicBool = AtomicBool::new(false);
static DB_PROBED_AT_SECS: AtomicU64 = AtomicU64::new(0);
static STORAGE_PREFLIGHT_OK: AtomicBool = AtomicBool::new(false);
static STORAGE_WRITABLE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeHealthSnapshot {
    pub db_handle_present: bool,
    pub db_probe_ok: bool,
    pub db_probed_at: Option<u64>,
    pub storage_preflight: bool,
    pub storage_writable: bool,
}

pub fn snapshot() -> RuntimeHealthSnapshot {
    let probed_at = DB_PROBED_AT_SECS.load(Ordering::Acquire);
    RuntimeHealthSnapshot {
        db_handle_present: DB_HANDLE_PRESENT.load(Ordering::Acquire),
        db_probe_ok: DB_PROBE_OK.load(Ordering::Acquire),
        db_probed_at: (probed_at > 0).then_some(probed_at),
        storage_preflight: STORAGE_PREFLIGHT_OK.load(Ordering::Acquire),
        storage_writable: STORAGE_WRITABLE.load(Ordering::Acquire),
    }
}

/// Startup write check passed. Does not claim current writability.
pub fn mark_storage_preflight_ok() {
    STORAGE_PREFLIGHT_OK.store(true, Ordering::Release);
}

pub fn record_storage_writable(ok: bool) {
    STORAGE_WRITABLE.store(ok, Ordering::Release);
}

pub fn record_db_probe(handle_present: bool, probe_ok: bool) {
    DB_HANDLE_PRESENT.store(handle_present, Ordering::Release);
    DB_PROBE_OK.store(probe_ok, Ordering::Release);
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    DB_PROBED_AT_SECS.store(secs, Ordering::Release);
}

pub fn compute_routes_full(config_mode: bool, schema_ready: bool, db_probe_ok: bool) -> bool {
    !config_mode && db_probe_ok && schema_ready
}

pub fn is_business_ready(
    config_mode: bool,
    schema_ready: bool,
    db_probe_ok: bool,
    storage_writable: bool,
) -> bool {
    compute_routes_full(config_mode, schema_ready, db_probe_ok) && storage_writable
}

pub fn health_payload(
    snap: &RuntimeHealthSnapshot,
    config_mode: bool,
    schema_ready: bool,
    version: &str,
    commit_sha: Option<&str>,
    uptime_seconds: u64,
) -> Value {
    let db_connected = snap.db_probe_ok;
    let routes_full = compute_routes_full(config_mode, schema_ready, snap.db_probe_ok);
    json!({
        "status": "ok",
        "schema_version": 1,
        "version": version,
        "commit_sha": commit_sha,
        "db_connected": db_connected,
        "db_handle_present": snap.db_handle_present,
        "db_probed_at": snap.db_probed_at,
        "migrations_applied": schema_ready,
        "routes_full": routes_full,
        "storage_preflight": snap.storage_preflight,
        "storage_writable": snap.storage_writable,
        "uptime_seconds": uptime_seconds,
        "service": "myriad-backend",
        "mode": if config_mode || !routes_full {
            "configuration"
        } else {
            "full"
        },
        "database_connected": db_connected,
    })
}

pub async fn probe_database(db: &DatabaseConnection) -> bool {
    let result = tokio::time::timeout(
        LIVE_PROBE_TIMEOUT,
        db.execute_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT 1".to_owned(),
        )),
    )
    .await;
    let ok = matches!(result, Ok(Ok(_)));
    record_db_probe(true, ok);
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_connected_follows_probe_not_handle() {
        let snap = RuntimeHealthSnapshot {
            db_handle_present: true,
            db_probe_ok: false,
            db_probed_at: Some(1),
            storage_preflight: true,
            storage_writable: true,
        };
        let payload = health_payload(&snap, false, true, "v1", Some("abc"), 1);
        assert_eq!(payload["db_connected"], false);
        assert_eq!(payload["database_connected"], false);
        assert_eq!(payload["db_handle_present"], true);
        assert_eq!(payload["routes_full"], false);
        assert_eq!(payload["storage_preflight"], true);
        assert_eq!(payload["storage_writable"], true);
        assert_eq!(payload["mode"], "configuration");
        assert!(!is_business_ready(false, true, false, true));
    }

    #[test]
    fn ready_requires_full_routes_and_live_storage() {
        assert!(is_business_ready(false, true, true, true));
        assert!(!is_business_ready(true, true, true, true));
        assert!(!is_business_ready(false, false, true, true));
        assert!(!is_business_ready(false, true, true, false));
        let snap = RuntimeHealthSnapshot {
            db_handle_present: true,
            db_probe_ok: true,
            db_probed_at: Some(2),
            storage_preflight: true,
            storage_writable: true,
        };
        let payload = health_payload(&snap, false, true, "v1", Some("abc"), 9);
        assert_eq!(payload["db_connected"], true);
        assert_eq!(payload["routes_full"], true);
        assert_eq!(payload["mode"], "full");
    }
}
