//! Tapp 运行状态与速率限制 API

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use sea_orm::DatabaseConnection;
use serde_json::{json, Value};

use crate::error::HttpError;
use crate::middleware::auth::{ensure_current_admin_on, Claims};
use crate::services::tapp_scheduler::{
    active_frontend_subject_count, scheduler_counters, scheduler_mailbox_depth,
};

use super::common::{
    get_rate_limit_config, get_rate_limit_status_for, get_rate_limiter_active_count,
};
use super::runtime_grant;

/// GET /api/tapp/metrics
pub async fn get_tapp_metrics(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    ensure_current_admin_on(&claims, &db).await?;

    let active_limits = get_rate_limiter_active_count(&db).await?;

    let cached_platforms = crate::services::platform_cache::platform_cache_entry_count().await;
    let active_scheduler_subjects = active_frontend_subject_count(&db).await.map_err(|error| {
        tracing::error!(%error, "[TAPP] Failed to collect scheduler metrics");
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Failed to collect scheduler metrics" })),
        )
    })?;
    let scheduler_mailbox = scheduler_mailbox_depth(&db).await.map_err(|error| {
        tracing::error!(%error, "[TAPP] Failed to collect scheduler mailbox metrics");
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Failed to collect scheduler metrics" })),
        )
    })?;
    let active_runtime_grants = runtime_grant::active_runtime_grant_count(&db)
        .await
        .map_err(|error| {
            tracing::error!(%error, "[TAPP] Failed to collect runtime grant metrics");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "Failed to collect runtime metrics" })),
            )
        })?;

    Ok(Json(json!({
        "success": true,
        "rateLimiter": { "activeLimits": active_limits },
        "cache": { "platforms": cached_platforms },
        "runtime": { "activeGrants": active_runtime_grants },
        "scheduler": {
            "activeSubjects": active_scheduler_subjects,
            "mailboxDepth": scheduler_mailbox,
            "processCounters": scheduler_counters()
        }
    })))
}

/// GET /api/tapp/rate-limit/{tapp_id}
pub async fn get_rate_limit_status(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    let operations = ["ai.task", "platform.write", "storage.set"];
    let mut limits = Vec::new();

    for op in operations {
        let (limit, _window_secs) = get_rate_limit_config(op);
        let (used, remaining, reset_in) =
            get_rate_limit_status_for(&db, user_id, &tapp_id, op).await?;

        limits.push(json!({
            "operation": op,
            "limit": limit,
            "used": used,
            "remaining": remaining,
            "resetIn": reset_in
        }));
    }

    Ok(Json(json!({
        "success": true,
        "tappId": tapp_id,
        "limits": limits
    })))
}
