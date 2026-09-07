use axum::{http::StatusCode, Json};
use serde_json::{json, Value};

pub mod admin_users;
pub mod agent;
pub mod ai_recommend;
pub mod analytics;
pub mod auth;
pub mod auth_local;
pub mod avatar_source; // 画像源选择（本人 + 管理员代改）
pub mod bangumi;
pub mod bilibili;
pub mod brew;
pub mod brewlia;
pub mod cache;
pub mod config;
pub mod diagnostics;
pub mod discord;
pub mod federation; // HTTP adapter (moved out of main)
pub mod game_presence; // public Enka / Xbox / PSN; no user cookies
pub mod github_stars; // GitHub repo summary for settings badges + Brew (platform egress)
pub mod home_stickers; // Free-layout AI stickers
pub mod mal;
pub mod merope_rig; // Site-wide Anime2.5D face for Agent 人设
pub mod metrics;
pub mod model3d; // Tripo-backed 3D generation + persisted Web GLBs
pub mod notification_preferences;
pub mod oauth; // generic OAuth (replaces hardcoded GitHub flow in auth.rs)
pub mod platforms;
pub mod profile;
pub mod profile_text_source; // 名称/简介文案来源（与画像源独立）
pub mod prompt;
pub mod proxy;
pub mod reports;
pub mod seo;
pub mod seo_geo;
pub mod seo_policy;
pub mod setup;
pub mod setup_bootstrap;
pub mod site_domain; // BASE_URL / FRONTEND_URL / CORS — not federation Move
pub mod speech; // TTS/ASR (Tencent / OpenAI / OpenRouter)
pub mod speech_conversation;
pub mod steam;
pub mod system;
pub mod tapp_playground; // Pro AI temporary Tapp workspace
pub mod tapp_runtime;
pub mod tapp_scheduler;
pub mod tapp_store;
pub mod tasks;
pub mod updater_admin;
pub mod widget_fonts; // Optional custom font for the game-presence widget
pub mod x;
pub mod youtube; // YouTube Data API v3; public channels, API key only

// Process / build identity: workspace crate `myriad-process-info` (agent + /health).
// Binary package version is injected at startup so fallback is myriad-backend's, not the helper crate's.
pub use myriad_process_info::{build_commit_sha, build_version, process_uptime_seconds};

/// Call once from binary main after logging is ready.
pub fn init_process_identity() {
    myriad_process_info::set_package_version_fallback(concat!("v", env!("CARGO_PKG_VERSION")));
    myriad_process_info::mark_startup();
}

/// `/health` endpoint consumed by the Myriad updater health probe.
///
/// Returns the schema described in docs/updater-spec.md §11.1:
///
/// ```json
/// {
/// "status": "ok",
/// "version": "v1.2.3",
/// "schema_version": 1,
/// "db_connected": true,
/// "migrations_applied": true,
/// "uptime_seconds": 123
/// }
/// ```
///
/// Older fields (`service`, `mode`, `database_connected`) are preserved for backwards
/// compatibility with existing dashboards.
pub async fn health() -> (StatusCode, Json<Value>) {
    use std::sync::atomic::Ordering;

    let config_mode = crate::CONFIG_MODE.load(Ordering::Relaxed);
    let schema_ready = crate::SCHEMA_READY.load(Ordering::Acquire);
    // Process DB handle (may exist while still on setup-only route table until restart).
    let db_connected = crate::services::tapp_registry::database().await.is_ok();
    // Route table is fixed at process start: config-mode router vs full router.
    // Do not equate CONFIG_MODE=false with "full APIs" without a cold start.
    let routes_full = !config_mode && db_connected && schema_ready;
    let migrations_applied = schema_ready;

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
            "routes_full": routes_full,
            // Reaching the server implies the startup storage write preflight passed.
            "storage_writable": true,
            "uptime_seconds": uptime,

            // backwards-compatible fields
            "service": "myriad-backend",
            "mode": if config_mode || !routes_full {
                "configuration"
            } else {
                "full"
            },
            "database_connected": db_connected,
        })),
    )
}
