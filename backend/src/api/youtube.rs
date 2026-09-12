//! YouTube Data API v3 routes (server-side API key only).
//!
//! No OAuth / `mine=true` / private playlists. Admin-authenticated; key comes
//! from dynamic config (never from the client query string).
//!
//! **Admin diagnostic / key validation** — the public site and reports pipeline
//! use configured `youtube_*` keys + platform fetchers, not these routes.
//! Front-end does not call `/api/youtube/*` yet; keep for curl / config smoke
//! tests (`GET /api/youtube/channel?channel_id=UC…`). Not scheduled for removal.

use crate::error::HttpError;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::DynamicConfig;
use crate::middleware::auth::{ensure_current_admin_on, Claims};
use crate::services::fetcher::PlatformFetcher;
use sea_orm::DatabaseConnection;

#[derive(Debug, Deserialize)]
pub struct YouTubeChannelQuery {
    /// UC… id, `@handle`, or bare handle
    pub channel_id: String,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub message: String,
}

fn youtube_api_key(config: &DynamicConfig) -> Result<String, HttpError> {
    config
        .youtube_api_key
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("YOUTUBE_API_KEY")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "success": false,
                    "error": "YouTube API key not configured",
                    "code": "youtube_api_key_required",
                    "message": "Set youtube_api_key in site config or YOUTUBE_API_KEY"
                })),
            ))
        })
}

/// GET /api/youtube/channel?channel_id=…
///
/// Admin only. Resolves a public channel (snippet + statistics + contentDetails)
/// using the **server** YouTube API key.
pub async fn get_youtube_channel(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<Arc<RwLock<DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    Query(params): Query<YouTubeChannelQuery>,
) -> Result<Json<ApiResponse<serde_json::Value>>, HttpError> {
    ensure_current_admin_on(&claims, &db)
        .await
        .map_err(|(status, body)| HttpError::from((status, body)))?;

    let channel_id = params.channel_id.trim();
    if channel_id.is_empty() {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::fail_json("channel_id is required")),
        )));
    }

    let api_key = {
        let config = dynamic_config.read().await;
        youtube_api_key(&config)?
    };

    let fetcher = PlatformFetcher::new().await;
    match fetcher.fetch_youtube_channel(&api_key, channel_id).await {
        Ok(channel) => Ok(Json(ApiResponse {
            success: true,
            data: Some(channel),
            message: "ok".to_string(),
        })),
        Err(e) => {
            tracing::warn!(error = %e, "YouTube channel fetch failed");
            Err(HttpError::from((
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "success": false,
                    "error": "YouTube upstream failed",
                    "code": "youtube_upstream_failed"
                })),
            )))
        }
    }
}

/// GET /api/youtube/bundle?channel_id=…
///
/// Admin only. Full public bundle via server API key.
pub async fn get_youtube_bundle(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<Arc<RwLock<DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    Query(params): Query<YouTubeChannelQuery>,
) -> Result<Json<ApiResponse<serde_json::Value>>, HttpError> {
    ensure_current_admin_on(&claims, &db)
        .await
        .map_err(|(status, body)| HttpError::from((status, body)))?;

    let channel_id = params.channel_id.trim();
    if channel_id.is_empty() {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::fail_json("channel_id is required")),
        )));
    }

    let api_key = {
        let config = dynamic_config.read().await;
        youtube_api_key(&config)?
    };

    let fetcher = PlatformFetcher::new().await;
    match fetcher
        .fetch_youtube_channel_bundle(&api_key, channel_id)
        .await
    {
        Ok(bundle) => {
            let n = bundle
                .get("videos")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Ok(Json(ApiResponse {
                success: true,
                data: Some(bundle),
                message: format!("ok, {n} sample videos"),
            }))
        }
        Err(e) => {
            tracing::warn!(error = %e, "YouTube bundle fetch failed");
            Err(HttpError::from((
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "success": false,
                    "error": "YouTube upstream failed",
                    "code": "youtube_upstream_failed"
                })),
            )))
        }
    }
}
use myriad_error::AppError;
