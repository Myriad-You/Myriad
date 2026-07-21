use axum::{http::StatusCode, Json};
use serde_json::{json, Value};

pub mod agent; // 🤖 AI Agent 自然语言任务编排 API
pub mod ai_recommend; // ✅ AI图标推荐 API
pub mod analysis;
pub mod auth;
pub mod auth_local;
pub mod bangumi;
pub mod bilibili;
pub mod brew; // ✅ Brew 阅读 RSS/Atom/JSON Feed 订阅 API
pub mod brewlia; // ✅ Brewlia AI增强阅读 API
pub mod cache; // ✅ 缓存管理 API
pub mod config;
pub mod discord; // ✅ Discord 数据平台 API
pub mod game_presence; // ✅ 游戏平台公开状态（Enka / Xbox / PSN，无用户 Cookie）
pub mod mal; // ✅ MyAnimeList 数据平台 API
pub mod metrics; // ✅ 系统监控指标 API (P2优化)
pub mod notification_preferences;
pub mod oauth; // 🔐 通用 OAuth handler (PR #2 — 取代 auth.rs 里的硬编码 GitHub 流)
pub mod platforms;
pub mod profile;
pub mod prompt;
pub mod proxy;
pub mod reports; // ✅ 双层报告系统API
pub mod setup;
pub mod speech; // 🎙️ 腾讯云语音服务 API (TTS/ASR)
pub mod steam;
pub mod system;
pub mod tapp_playground; // 🧪 Pro AI 驱动的临时 Tapp 开发环境
pub mod tapp_runtime; // ✅ Tapp 运行时 API（平台数据、AI、上下文、事件…）
pub mod tapp_scheduler; // ✅ Tapp 定时任务调度 API
pub mod tapp_store; // ✅ Tapp 应用商店/管理 API（安装、卸载、配置…）
pub mod tasks; // ✅ 后台任务管理 API
pub mod admin_users;
pub mod updater_admin;
pub mod x; // ✅ X (Twitter) 平台 API // 🚀 Updater admin proxy

// Process start time for uptime reporting in /health.
static STARTED_AT: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

pub fn mark_startup() {
    let _ = STARTED_AT.set(std::time::Instant::now());
}

/// Process uptime in seconds since `mark_startup` (0 if not marked).
pub fn process_uptime_seconds() -> u64 {
    STARTED_AT.get().map(|t| t.elapsed().as_secs()).unwrap_or(0)
}

pub fn build_version() -> &'static str {
    option_env!("MYRIAD_VERSION").unwrap_or(concat!("v", env!("CARGO_PKG_VERSION")))
}

pub fn build_commit_sha() -> Option<&'static str> {
    option_env!("MYRIAD_COMMIT_SHA")
        .map(str::trim)
        .filter(|sha| sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// `/health` endpoint consumed by the Myriad updater health probe.
///
/// Returns the schema described in docs/updater-spec.md §11.1:
///
/// ```json
/// {
///   "status": "ok",
///   "version": "v1.2.3",
///   "schema_version": 1,
///   "db_connected": true,
///   "migrations_applied": true,
///   "uptime_seconds": 123
/// }
/// ```
///
/// Older fields (`service`, `mode`, `database_connected`) are preserved for backwards
/// compatibility with existing dashboards.
pub async fn health() -> (StatusCode, Json<Value>) {
    use std::sync::atomic::Ordering;

    let config_mode = crate::CONFIG_MODE.load(Ordering::Relaxed);
    let db_connected = !config_mode;
    // Once the backend has left config mode, migrations have been verified by the startup
    // schema check (see backend/src/db/schema_check.rs). So in full mode this is true.
    let migrations_applied = db_connected;

    // Build-time version injected via `MYRIAD_VERSION` env var (set by Dockerfile build-arg).
    // Falls back to crate version so local `cargo run` still works.
    let version = build_version();
    let commit_sha = build_commit_sha();

    let uptime = process_uptime_seconds();

    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "schema_version": 1,
            "version": version,
            "commit_sha": commit_sha,
            "db_connected": db_connected,
            "migrations_applied": migrations_applied,
            // Reaching the server implies the startup storage write preflight passed.
            "storage_writable": true,
            "uptime_seconds": uptime,

            // backwards-compatible fields
            "service": "myriad-backend",
            "mode": if config_mode { "configuration" } else { "full" },
            "database_connected": db_connected,
        })),
    )
}
