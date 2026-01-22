use axum::{http::StatusCode, response::IntoResponse, Json};
use serde_json::json;
use std::sync::atomic::Ordering;

// ⚠️ 告警阈值配置 (P2优化)
const MEMORY_WARNING_MB: u64 = 500; // 内存使用超过500MB时警告
const MEMORY_CRITICAL_MB: u64 = 1000; // 内存使用超过1GB时严重告警
const TASKS_WARNING: usize = 50; // 任务数超过50时警告
const TASKS_CRITICAL: usize = 100; // 任务数超过100时严重告警

/// 获取系统指标（内存、CPU、连接等）
/// 用于监控和告警
pub async fn get_metrics() -> impl IntoResponse {
    // 1. 内存使用情况
    let memory_info = get_memory_info();

    // 2. 后台任务统计
    let task_stats = get_task_stats().await;

    // 3. CSRF Token 统计
    let csrf_stats = get_csrf_stats().await;

    // 4. 配置模式状态
    let config_mode = crate::CONFIG_MODE.load(Ordering::Relaxed);

    // 5. 数据库连接状态
    let db_connected = crate::DB_CONNECTION.read().await.is_some();

    // 6. 告警检查
    let alerts = check_alerts(&memory_info, &task_stats);

    (
        StatusCode::OK,
        Json(json!({
            "status": if alerts.is_empty() { "ok" } else { "warning" },
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "memory": memory_info,
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

/// 获取内存信息（跨平台）
fn get_memory_info() -> serde_json::Value {
    #[cfg(target_os = "linux")]
    {
        use std::fs;

        // Linux: 读取 /proc/self/status
        if let Ok(status) = fs::read_to_string("/proc/self/status") {
            let mut vm_rss = 0u64;
            let mut vm_size = 0u64;

            for line in status.lines() {
                if line.starts_with("VmRSS:") {
                    vm_rss = line
                        .split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                } else if line.starts_with("VmSize:") {
                    vm_size = line
                        .split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                }
            }

            json!({
                "rss_kb": vm_rss,
                "rss_mb": vm_rss / 1024,
                "virtual_kb": vm_size,
                "virtual_mb": vm_size / 1024,
            })
        } else {
            json!({
                "platform": "linux",
                "note": "Unable to read /proc/self/status"
            })
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Windows: 使用 GetProcessMemoryInfo (需要 winapi crate)
        // 简化版本：返回基础信息
        json!({
            "platform": "windows",
            "note": "Detailed memory metrics require additional dependencies"
        })
    }

    #[cfg(target_os = "macos")]
    {
        // macOS: 可以使用 mach API
        json!({
            "platform": "macos",
            "note": "Detailed memory metrics require additional dependencies"
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        json!({
            "platform": "unknown",
            "note": "Memory metrics not available"
        })
    }
}

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
    // 注意：这需要在 csrf.rs 中暴露 CSRF_TOKENS
    // 暂时返回占位符
    json!({
        "note": "CSRF token count requires exposing internal state"
    })
}
