use crate::error::HttpError;
use axum::Json;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};

// Global flag to trigger config reload
pub static CONFIG_RELOAD_REQUESTED: AtomicBool = AtomicBool::new(false);

pub fn is_config_reload_requested() -> bool {
    CONFIG_RELOAD_REQUESTED.load(Ordering::Relaxed)
}

pub fn reset_config_reload_flag() {
    CONFIG_RELOAD_REQUESTED.store(false, Ordering::Relaxed);
}

/// Reconnect only when the database target actually changed to a non-empty URL.
/// Empty next is not a reconnect; health recovery stays on its own path.
pub fn database_target_changed(previous: &str, next: &str) -> bool {
    previous != next && !next.is_empty()
}

/// Publish `AppConfig` + CORS from the current process env. Does not reconnect.
pub async fn publish_env_app_config() -> anyhow::Result<crate::config::AppConfig> {
    let new_config = crate::config::AppConfig::from_env()?;
    *crate::GLOBAL_CONFIG.write().await = new_config.clone();
    if let Ok(cors) = std::env::var("CORS_ORIGINS") {
        crate::middleware::cors_runtime::set_cors_origins_csv(&cors);
    }
    Ok(new_config)
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

/// GET /api/system/status — 公开探活。
pub async fn system_status() -> Json<Value> {
    use std::time::SystemTime;

    let uptime = crate::clock::now_system()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // 公开探活字段；管理员面板走 /api/admin/diagnostics
    Json(json!({
        "status": "running",
        "uptime_seconds": uptime,
        "version": env!("CARGO_PKG_VERSION"),
        "config_mode": crate::CONFIG_MODE.load(Ordering::Relaxed),
        // 如果需要更高安全性，可以移除 version 和 config_mode
    }))
}

#[cfg(test)]
mod tests {
    use super::database_target_changed;

    #[test]
    fn reconnects_only_when_database_target_changes() {
        assert!(!database_target_changed(
            "postgres://a/db",
            "postgres://a/db"
        ));
        assert!(database_target_changed(
            "postgres://a/db",
            "postgres://b/db"
        ));
        assert!(database_target_changed("", "postgres://a/db"));
        assert!(!database_target_changed("postgres://a/db", ""));
    }

    #[test]
    fn ordinary_saves_publish_without_reconnect_flag() {
        let save = include_str!("config/save.rs");
        assert!(save.contains("publish_env_app_config"));
        assert!(!save.contains("CONFIG_RELOAD_REQUESTED"));
        let backup = include_str!("config/backup.rs");
        assert!(backup.contains("publish_env_app_config"));
        assert!(!backup.contains("CONFIG_RELOAD_REQUESTED"));
        let domain = include_str!("site_domain.rs");
        assert!(domain.contains("publish_env_app_config"));
        assert!(!domain.contains("CONFIG_RELOAD_REQUESTED"));
        let router = include_str!("../router/mod.rs");
        assert!(router.contains("database_target_changed"));
        assert!(router.contains("skipping reconnect"));
    }
}
