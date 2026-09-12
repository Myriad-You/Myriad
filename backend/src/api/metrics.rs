use axum::{http::StatusCode, response::IntoResponse, Json};
use myriad_process_info::{MEMORY_CRITICAL_MB, MEMORY_WARNING_MB};
use serde_json::json;
use std::sync::atomic::Ordering;

const TASKS_WARNING: usize = 50; // 任务数超过50时警告
const TASKS_CRITICAL: usize = 100; // 任务数超过100时严重告警

/// 获取系统指标（内存、后台任务、CSRF、config_mode）。无 CPU；`db_connected` 恒为 true。
/// 用于监控和告警
///
/// Mounted only under full-mode `AppState` routes. Presence of this handler
/// implies the process has a wired DB (request State / process registry).
pub async fn get_metrics(_db: crate::extract::Db) -> impl IntoResponse {
    // 1. 内存使用情况
    let memory_info = process_memory_info();

    // 2. 后台任务统计
    let task_stats = get_task_stats().await;

    // 3. CSRF Token 统计
    let csrf_stats = get_csrf_stats().await;

    // 4. 配置模式状态
    let config_mode = crate::CONFIG_MODE.load(Ordering::Relaxed);

    // 5. 数据库连接状态 — extract::Db succeeded ⇒ connected for this router.
    let db_connected = true;

    // 6. 告警检查
    let alerts = check_alerts(&memory_info, &task_stats);

    (
        StatusCode::OK,
        Json(json!({
            "status": if alerts.is_empty() { "ok" } else { "warning" },
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "memory": memory_info,
            "memory_profile": crate::services::memory_profile::metrics_snapshot(),
            "memory_alerts": {
                "warning_mb": MEMORY_WARNING_MB,
                "critical_mb": MEMORY_CRITICAL_MB,
            },
            "tasks": task_stats,
            "csrf": csrf_stats,
            "system": {
                "config_mode": config_mode,
                "db_connected": db_connected,
            },
            "alerts": alerts,
        })),
    )
}

/// 检查告警条件
fn check_alerts(
    memory_info: &serde_json::Value,
    task_stats: &serde_json::Value,
) -> Vec<serde_json::Value> {
    let mut alerts = Vec::new();

    // 检查内存告警
    if let Some(rss_mb) = memory_info.get("rss_mb").and_then(|v| v.as_u64()) {
        if rss_mb >= MEMORY_CRITICAL_MB {
            alerts.push(json!({
                "level": "critical",
                "type": "memory",
                "message": format!("Memory usage critical: {}MB (threshold: {}MB)", rss_mb, MEMORY_CRITICAL_MB),
                "value": rss_mb,
                "threshold": MEMORY_CRITICAL_MB,
            }));
        } else if rss_mb >= MEMORY_WARNING_MB {
            alerts.push(json!({
                "level": "warning",
                "type": "memory",
                "message": format!("Memory usage high: {}MB (threshold: {}MB)", rss_mb, MEMORY_WARNING_MB),
                "value": rss_mb,
                "threshold": MEMORY_WARNING_MB,
            }));
        }
    }

    // 检查任务告警
    if let Some(total) = task_stats.get("total").and_then(|v| v.as_u64()) {
        let total = total as usize;
        if total >= TASKS_CRITICAL {
            alerts.push(json!({
                "level": "critical",
                "type": "tasks",
                "message": format!("Task count critical: {} (threshold: {})", total, TASKS_CRITICAL),
                "value": total,
                "threshold": TASKS_CRITICAL,
            }));
        } else if total >= TASKS_WARNING {
            alerts.push(json!({
                "level": "warning",
                "type": "tasks",
                "message": format!("Task count high: {} (threshold: {})", total, TASKS_WARNING),
                "value": total,
                "threshold": TASKS_WARNING,
            }));
        }
    }

    alerts
}

/// Process memory info (cross-platform, best-effort).
/// Re-export for HTTP `/api/metrics` and diagnostics. Agent `system.metrics`
/// imports `myriad-process-info` directly.
pub use myriad_process_info::process_memory_info;

/// 获取后台任务统计
async fn get_task_stats() -> serde_json::Value {
    let processor = &crate::services::background_processor::BACKGROUND_PROCESSOR;
    let (total, pending, processing, completed, failed) = processor.get_task_stats().await;

    json!({
        "total": total,
        "pending": pending,
        "processing": processing,
        "completed": completed,
        "failed": failed,
    })
}

/// 获取 CSRF Token 统计
async fn get_csrf_stats() -> serde_json::Value {
    json!({
        "mode": "stateless",
        "stored_tokens": 0
    })
}
