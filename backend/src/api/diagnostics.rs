use crate::services::background_processor::{TaskStatus, BACKGROUND_PROCESSOR};
use axum::{http::StatusCode, Json};
use chrono::{DateTime, Duration, TimeZone, Utc};
use myriad_process_info::{MEMORY_CRITICAL_MB, MEMORY_WARNING_MB};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::time::Instant;

const STUCK_TASK_MINUTES: i64 = 30;
const RECENT_FAILURE_HOURS: i64 = 24;
const TASK_SNAPSHOT_LIMIT: usize = 100;
const RECENT_FAILURE_LIMIT: usize = 5;
const ERROR_DETAIL_LIMIT: usize = 500;

fn limited_detail(detail: impl AsRef<str>) -> String {
    detail.as_ref().chars().take(ERROR_DETAIL_LIMIT).collect()
}

/// Best-effort “when was this deployment’s database established”.
///
/// Preference order:
/// 1. Earliest `seaql_migrations.applied_at` (first schema apply ≈ first deploy)
/// 2. Postgres data-dir `PG_VERSION` mtime for the current database
///
/// Returns RFC3339 UTC when known.
async fn probe_database_established_at(
    db: &impl ConnectionTrait,
) -> Option<DateTime<Utc>> {
    // sea-orm: applied_at is typically a Unix epoch (bigint); some setups use timestamptz.
    if let Ok(Some(row)) = db
        .query_one_raw(Statement::from_string(
            db.get_database_backend(),
            r#"
            SELECT applied_at
            FROM seaql_migrations
            ORDER BY version ASC
            LIMIT 1
            "#
            .to_owned(),
        ))
        .await
    {
        // Try timestamptz / timestamp first.
        if let Ok(ts) = row.try_get::<DateTime<Utc>>("", "applied_at") {
            return Some(ts);
        }
        if let Ok(ts) = row.try_get::<chrono::NaiveDateTime>("", "applied_at") {
            return Some(DateTime::<Utc>::from_naive_utc_and_offset(ts, Utc));
        }
        // bigint / i64 / f64 epoch seconds (or ms)
        if let Ok(secs) = row.try_get::<i64>("", "applied_at") {
            if secs > 1_000_000_000_000 {
                return Utc.timestamp_millis_opt(secs).single();
            }
            if secs > 0 {
                return Utc.timestamp_opt(secs, 0).single();
            }
        }
        if let Ok(secs) = row.try_get::<f64>("", "applied_at") {
            if secs > 0.0 {
                return Utc.timestamp_opt(secs as i64, 0).single();
            }
        }
    }

    // Fallback: filesystem stamp of this database’s PG_VERSION (requires superuser
    // or appropriate grants; ignore failures quietly).
    if matches!(db.get_database_backend(), DatabaseBackend::Postgres) {
        if let Ok(Some(row)) = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                r#"
                SELECT (pg_catalog.pg_stat_file(
                    'base/' || d.oid::text || '/PG_VERSION',
                    true
                )).modification AS established_at
                FROM pg_catalog.pg_database d
                WHERE d.datname = current_database()
                "#
                .to_owned(),
            ))
            .await
        {
            if let Ok(ts) = row.try_get::<DateTime<Utc>>("", "established_at") {
                return Some(ts);
            }
            if let Ok(ts) = row.try_get::<chrono::NaiveDateTime>("", "established_at") {
                return Some(DateTime::<Utc>::from_naive_utc_and_offset(ts, Utc));
            }
        }
    }

    None
}

/// GET /api/admin/diagnostics
///
/// Runs bounded, side-effect-free or self-cleaning checks used by the Advanced
/// Settings diagnostics panel. The storage probe creates a unique file and
/// removes it immediately, matching the startup storage preflight.
pub async fn runtime_diagnostics(
    crate::extract::Db(db): crate::extract::Db,
) -> (StatusCode, Json<Value>) {
    let generated_at = Utc::now();
    let config_mode = crate::CONFIG_MODE.load(Ordering::Relaxed);
    let server_location_task =
        tokio::spawn(crate::services::server_location::inspect_server_location());

    let database_started = Instant::now();
    let database_result = db
        .query_one_raw(Statement::from_string(
            db.get_database_backend(),
            "SELECT 1 AS diagnostic_probe".to_owned(),
        ))
        .await
        .map(|_| ());
    let database_latency_ms = database_started.elapsed().as_millis() as u64;

    // Only meaningful when DB is reachable; used as deploy-time proxy.
    let database_established_at = if database_result.is_ok() {
        probe_database_established_at(&db).await
    } else {
        None
    };

    let storage_started = Instant::now();
    let storage_result =
        tokio::task::spawn_blocking(crate::services::data_paths::verify_runtime_storage_writable)
            .await;
    let storage_latency_ms = storage_started.elapsed().as_millis() as u64;
    let storage_error = match storage_result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(limited_detail(error.to_string())),
        Err(error) => Some(limited_detail(format!(
            "Storage diagnostic task failed: {error}"
        ))),
    };

    let database_error = database_result
        .err()
        .map(|error| limited_detail(error.to_string()));
    let migrations_ok = database_error.is_none() && !config_mode;

    let memory = crate::api::metrics::process_memory_info();
    let rss_mb = memory.get("rss_mb").and_then(Value::as_u64);
    let memory_status = match rss_mb {
        Some(value) if value >= MEMORY_CRITICAL_MB => "error",
        Some(value) if value >= MEMORY_WARNING_MB => "warning",
        _ => "ok",
    };
    let server_location = server_location_task.await.unwrap_or_else(|error| {
        tracing::warn!(%error, "Server location diagnostic task failed");
        crate::services::server_location::unavailable_assessment(false)
    });

    let (total, pending, processing, completed, failed) =
        BACKGROUND_PROCESSOR.get_task_stats().await;
    let recent_tasks = BACKGROUND_PROCESSOR
        .list_recent_tasks(TASK_SNAPSHOT_LIMIT)
        .await;
    let stuck_before = generated_at - Duration::minutes(STUCK_TASK_MINUTES);
    let recent_failure_after = generated_at - Duration::hours(RECENT_FAILURE_HOURS);

    let active_tasks = recent_tasks
        .iter()
        .filter(|task| matches!(task.status, TaskStatus::Pending | TaskStatus::Processing))
        .map(|task| {
            let stuck = task.updated_at < stuck_before;
            json!({
                "id": task.id,
                "platform": task.platform,
                "status": format!("{:?}", task.status).to_lowercase(),
                "progress": task.progress,
                "created_at": task.created_at.to_rfc3339(),
                "updated_at": task.updated_at.to_rfc3339(),
                "stuck": stuck,
            })
        })
        .collect::<Vec<_>>();

    let recent_failures = recent_tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Failed && task.updated_at >= recent_failure_after)
        .take(RECENT_FAILURE_LIMIT)
        .map(|task| {
            json!({
                "id": task.id,
                "platform": task.platform,
                "error": task.error.as_deref().map(limited_detail),
                "updated_at": task.updated_at.to_rfc3339(),
            })
        })
        .collect::<Vec<_>>();

    let has_stuck_task = active_tasks
        .iter()
        .any(|task| task.get("stuck").and_then(Value::as_bool) == Some(true));
    let has_critical_check = database_error.is_some() || storage_error.is_some() || !migrations_ok;
    // 服务器出口位置仅作信息展示，不参与 overall（单源/冲突等不算「需要关注」）
    let overall_status = if has_critical_check || memory_status == "error" {
        "critical"
    } else if has_stuck_task || !recent_failures.is_empty() || memory_status == "warning" {
        "warning"
    } else {
        "healthy"
    };

    let checks = vec![
        json!({
            "id": "database",
            "status": if database_error.is_none() { "ok" } else { "error" },
            "latency_ms": database_latency_ms,
            "detail": database_error,
        }),
        json!({
            "id": "storage",
            "status": if storage_error.is_none() { "ok" } else { "error" },
            "latency_ms": storage_latency_ms,
            "detail": storage_error,
        }),
        json!({
            "id": "migrations",
            "status": if migrations_ok { "ok" } else { "error" },
            "detail": if config_mode {
                Some("Backend is still in configuration mode")
            } else if database_error.is_some() {
                Some("Migration state cannot be verified while the database is unavailable")
            } else {
                None
            },
        }),
        json!({
            "id": "memory",
            "status": memory_status,
            // Surface RSS + profile so Advanced Settings can show process memory
            // without a second /api/metrics call.
            "detail": rss_mb.map(|mb| {
                format!(
                    "rss_mb={mb} profile={} (warn≥{MEMORY_WARNING_MB}, critical≥{MEMORY_CRITICAL_MB})",
                    crate::services::memory_profile::active_profile().as_str()
                )
            }),
            "rss_mb": rss_mb,
            "rss_kb": memory.get("rss_kb").cloned().unwrap_or(Value::Null),
            "warning_mb": MEMORY_WARNING_MB,
            "critical_mb": MEMORY_CRITICAL_MB,
            "memory_profile": crate::services::memory_profile::metrics_snapshot(),
        }),
        json!({
            "id": "location",
            "status": server_location.status,
            "detail": Value::Null,
        }),
    ];

    // Process target OS/arch (Docker guest or bare metal — not host under
    // cross-arch emulation).
    let platform = myriad_process_info::process_platform_info();

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "generated_at": generated_at.to_rfc3339(),
            "overall_status": overall_status,
            "runtime": {
                "version": crate::api::build_version(),
                "commit_sha": crate::api::build_commit_sha(),
                "uptime_seconds": crate::api::process_uptime_seconds(),
                "config_mode": config_mode,
                // First schema apply / PG data dir stamp — proxy for “deployed since”.
                "database_established_at": database_established_at
                    .map(|t| t.to_rfc3339())
                    .map(Value::String)
                    .unwrap_or(Value::Null),
                "os": platform.get("os").cloned().unwrap_or(Value::Null),
                "arch": platform.get("arch").cloned().unwrap_or(Value::Null),
                "family": platform.get("family").cloned().unwrap_or(Value::Null),
                "pointer_width": platform
                    .get("pointer_width")
                    .cloned()
                    .unwrap_or(Value::Null),
            },
            "checks": checks,
            "memory": memory,
            "server_location": server_location,
            "tasks": {
                "counts": {
                    "total": total,
                    "pending": pending,
                    "processing": processing,
                    "completed": completed,
                    "failed": failed,
                },
                "active": active_tasks,
                "recent_failures": recent_failures,
                "stuck_after_minutes": STUCK_TASK_MINUTES,
                "recent_failure_hours": RECENT_FAILURE_HOURS,
            },
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_diagnostic_error_details() {
        let detail = "x".repeat(ERROR_DETAIL_LIMIT + 25);
        assert_eq!(limited_detail(detail).chars().count(), ERROR_DETAIL_LIMIT);
    }
}
