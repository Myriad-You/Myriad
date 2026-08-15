//! Authenticated store stats report (browser fallback → backend → edge).
//! Instance-day cap: 1 count / instance / app / event / UTC day (no shared secret).

use super::{api_http_error, ApiResponse};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serde::Deserialize;

use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::models::entities::tapps;
use crate::services::store_stats_beacon;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreStatsReportRequest {
    pub app_id: String,
    pub version: String,
    /// `install` | `update`
    pub event: String,
}

/// POST /api/tapps/store/stats-report
pub(super) async fn report_store_stats(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<StoreStatsReportRequest>,
) -> Result<impl IntoResponse, HttpError> {
    let user_id: i32 = claims
        .sub
        .parse()
        .map_err(|_| api_http_error(StatusCode::UNAUTHORIZED, "Invalid user"))?;

    let app_id = req.app_id.trim();
    let version = req.version.trim();
    let event = req.event.trim();
    if app_id.is_empty() || version.is_empty() {
        return Err(api_http_error(
            StatusCode::BAD_REQUEST,
            "appId and version are required",
        ));
    }
    if !is_plausible_app_id(app_id) {
        return Err(api_http_error(StatusCode::BAD_REQUEST, "invalid appId"));
    }
    if version.len() > 64 {
        return Err(api_http_error(StatusCode::BAD_REQUEST, "invalid version"));
    }
    if event != "install" && event != "update" {
        return Err(api_http_error(
            StatusCode::BAD_REQUEST,
            "event must be install or update",
        ));
    }

    // Must be installed by this user on this instance.
    let installed = tapps::Entity::find()
        .filter(tapps::Column::TappId.eq(app_id))
        .filter(tapps::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .map_err(|_| api_http_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error"))?;

    if installed.is_none() {
        return Err(api_http_error(
            StatusCode::FORBIDDEN,
            "app is not installed for this user",
        ));
    }

    // Instance-day key (edge also recomputes from instance_hash).
    let key = store_stats_beacon::instance_day_idempotency_key(app_id, event);
    store_stats_beacon::spawn_store_stats_hit_with_key(app_id, version, event, Some(key));

    Ok(Json(ApiResponse::success(serde_json::json!({
        "queued": true
    }))))
}

fn is_plausible_app_id(id: &str) -> bool {
    if id.len() < 3 || id.len() > 128 {
        return false;
    }
    let mut parts = 0;
    for part in id.split('.') {
        if part.is_empty() || part.len() > 63 {
            return false;
        }
        if !part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return false;
        }
        parts += 1;
    }
    parts >= 2
}
