use axum::{Json, http::StatusCode};
use serde_json::{Value, json};

pub mod admin_users;
pub mod agent;
pub mod ai_recommend;
pub mod analytics;
pub mod auth;
pub mod auth_local;
pub mod avatar_source; // 画像源选择（本人 + 管理员代改）
pub mod bangumi;
pub mod bilibili;
pub mod cache;
pub mod config;
pub mod diagnostics;
pub mod discord;
pub mod federation; // HTTP surface; domain lives in crate::federation
pub mod game_presence; // public Enka / Xbox / PSN; no user cookies
pub mod github_stars; // GitHub repo summary for settings badges + Phantasi (platform egress)
pub mod home_stickers; // Free-layout AI stickers
pub mod local_music; // Local music library (admin catalog + guest player)
pub mod mal;
pub mod media;
pub mod media_edit;
pub mod media_public;
pub mod merope_rig; // Site-wide Anime2.5D face for Agent 人设
pub mod metrics;
pub mod model3d; // Tripo-backed 3D generation + persisted Web GLBs
pub mod notification_preferences;
pub mod oauth; // generic OAuth via /api/auth/oauth/:slug/*
pub mod phantasi;
pub mod phantasiai;
pub mod platforms;
pub mod process_logs;
pub mod profile;
pub mod profile_text_source; // 名称/简介文案来源（与画像源独立）
pub mod prompt;
pub mod proxy;
pub mod reports;
pub mod seo;
pub mod seo_geo;
pub mod seo_policy;
pub mod seo_review;
pub mod setup;
pub mod setup_bootstrap;
pub mod site_domain; // BASE_URL / FRONTEND_URL / CORS — not federation Move
pub mod speech; // TTS/ASR (Tencent / OpenAI / OpenRouter / Gemini / MiniMax)
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

fn health_json_from_snapshot() -> Value {
    use std::sync::atomic::Ordering;

    let config_mode = crate::CONFIG_MODE.load(Ordering::Relaxed);
    let schema_ready = crate::SCHEMA_READY.load(Ordering::Acquire);
    crate::db::health::health_payload(
        &crate::db::health::snapshot(),
        config_mode,
        schema_ready,
        &build_version(),
        build_commit_sha(),
        process_uptime_seconds(),
    )
}

/// `/health` is liveness: the process is up. Always HTTP 200.
///
/// `db_connected` is the last successful `SELECT 1`, not “a handle exists”.
/// Spec §11.1 fields plus `commit_sha`, `routes_full`, `storage_preflight`,
/// `db_handle_present`, `db_probed_at`, and older `service` / `mode` /
/// `database_connected`.
pub async fn health() -> (StatusCode, Json<Value>) {
    let mut payload = health_json_from_snapshot();
    // Persona is still hosted by web. Report a driver failure separately so it
    // cannot turn a recoverable persona failure into a web health restart loop.
    payload["persona_background"] = json!(crate::persona::background_status());
    payload["persona_http_isolated"] = json!(
        crate::runtime_role::PERSONA_HTTP_ISOLATED.load(std::sync::atomic::Ordering::Acquire)
    );
    payload["federation_http_isolated"] = Value::Bool(
        crate::runtime_role::FEDERATION_HTTP_ISOLATED.load(std::sync::atomic::Ordering::Acquire),
    );
    (StatusCode::OK, Json(payload))
}

/// `/ready` is business readiness: live DB probe (2s), schema, full routes, storage.
/// Returns 503 when the process should not take traffic.
pub async fn ready() -> (StatusCode, Json<Value>) {
    use std::sync::atomic::Ordering;

    match crate::services::process_db::database() {
        Ok(db) => {
            let _ = crate::db::health::probe_database(&db).await;
        }
        Err(_) => {
            crate::db::health::record_db_probe(false, false);
        }
    }
    match tokio::task::spawn_blocking(crate::services::data_paths::verify_runtime_storage_writable)
        .await
    {
        Ok(Ok(())) => crate::db::health::record_storage_writable(true),
        _ => crate::db::health::record_storage_writable(false),
    }

    let config_mode = crate::CONFIG_MODE.load(Ordering::Relaxed);
    let schema_ready = crate::SCHEMA_READY.load(Ordering::Acquire);
    let snap = crate::db::health::snapshot();
    let mut payload = crate::db::health::health_payload(
        &snap,
        config_mode,
        schema_ready,
        &build_version(),
        build_commit_sha(),
        process_uptime_seconds(),
    );
    let ready = crate::db::health::is_business_ready(
        config_mode,
        schema_ready,
        snap.db_probe_ok,
        snap.storage_writable,
    );
    payload["ready"] = json!(ready);
    payload["persona_background"] = json!(crate::persona::background_status());
    let status = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(payload))
}
