//! Admin switch for the full audio relay (`/api/proxy/music/*/audio/`).
//!
//! The player stops building relay URLs when the switch is off; this guard
//! stops stale clients and direct callers from spending server bandwidth.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use once_cell::sync::Lazy;
use sea_orm::DatabaseConnection;
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::services::config_service::ConfigService;

/// Range requests hit the relay several times per track; a short TTL keeps
/// that to one query while a saved change still lands within seconds.
const SWITCH_TTL: Duration = Duration::from_secs(10);

static SWITCH_CACHE: Lazy<Mutex<Option<(Instant, bool)>>> = Lazy::new(|| Mutex::new(None));

pub(crate) async fn music_proxy_enabled(db: Option<DatabaseConnection>) -> bool {
    let mut cache = SWITCH_CACHE.lock().await;
    if let Some((at, enabled)) = *cache
        && at.elapsed() < SWITCH_TTL
    {
        return enabled;
    }
    // No DB or a failed read keeps the default (on) and is not cached.
    let Some(db) = db else { return true };
    let Ok(enabled) = ConfigService::load_music_proxy_enabled_on(&db).await else {
        return true;
    };
    *cache = Some((Instant::now(), enabled));
    enabled
}

pub(crate) fn music_proxy_disabled_response() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": "music_proxy_disabled",
            "message": "Audio relay is turned off; use the play-url endpoint",
        })),
    )
        .into_response()
}
