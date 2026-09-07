use crate::error::HttpError;
use axum::Json;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

// Global flag to trigger config reload
pub static CONFIG_RELOAD_REQUESTED: AtomicBool = AtomicBool::new(false);

pub fn is_config_reload_requested() -> bool {
    CONFIG_RELOAD_REQUESTED.load(Ordering::Relaxed)
}

pub fn reset_config_reload_flag() {
    CONFIG_RELOAD_REQUESTED.store(false, Ordering::Relaxed);
}

/// POST /api/system/reload-config
/// Reload runtime configuration without restarting the server.
/// This does not rebuild the startup route table.
pub async fn reload_config() -> Result<Json<Value>, HttpError> {
    tracing::info!("🔄 Configuration reload requested via API");

    // Set config reload flag
    CONFIG_RELOAD_REQUESTED.store(true, Ordering::Relaxed);

    Ok(Json(json!({
        "success": true,
        "message": "Runtime configuration will be reloaded. Startup routes are not rebuilt; setup database changes require a restart.",
    })))
}

/// GET /api/system/status
/// Get system status and uptime
/// P2: This endpoint is public for monitoring purposes
/// Consider removing sensitive information if needed
pub async fn system_status() -> Json<Value> {
    use std::time::SystemTime;

    let uptime = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // 返回基本状态信息，适用于健康检查和监控
    // 注意：config_mode 信息保留用于前端判断系统状态
    Json(json!({
        "status": "running",
        "uptime_seconds": uptime,
        "version": env!("CARGO_PKG_VERSION"),
        "config_mode": crate::CONFIG_MODE.load(Ordering::Relaxed),
        // 如果需要更高安全性，可以移除 version 和 config_mode
    }))
}
