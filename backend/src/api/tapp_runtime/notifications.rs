//! Tapp 通知 API。
//!
//! Domain enqueue lives in [`crate::services::tapp_notification`]. This module
//! owns grant/permission checks and Axum DTO mapping. Foreground and background
//! Tapp notifications enter the shared `NotificationManager`; clients consume
//! the same events for toast / island / system surfaces.

use axum::{extract::State, http::StatusCode, Extension, Json};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::middleware::auth::Claims;
use crate::services::permission_service::TappPermission;
use crate::services::tapp_notification::{self, TappNotificationError};
use crate::error::HttpError;

use super::common::authorize_tapp_permission;
use super::runtime_grant::RuntimeGrantContext;

#[derive(Debug, Deserialize)]
pub struct TappNotificationRequest {
    pub tapp_id: String,
    pub title: Option<String>,
    #[serde(default)]
    pub message: String,
    #[serde(default = "default_notification_type")]
    pub notification_type: String,
}

fn default_notification_type() -> String {
    "info".to_string()
}

fn notification_http_error(err: TappNotificationError) -> (StatusCode, Json<Value>) {
    let status =
        StatusCode::from_u16(err.status_hint()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        Json(json!({
            "error": err.message(),
            // Keep prior body shape (message only) while adding a stable code
            // for clients that opt in.
            "code": err.code(),
        })),
    )
}

/// POST /api/tapp/notifications
pub async fn create_tapp_notification(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Json(request): Json<TappNotificationRequest>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&request.tapp_id)?;
    runtime_grant.require(TappPermission::UiNotification)?;
    let user_id = authorize_tapp_permission(
        &db,
        &claims,
        &request.tapp_id,
        TappPermission::UiNotification,
        &dynamic_config).await?;

    let notification_id = tapp_notification::create_tapp_notification(
        user_id,
        &request.tapp_id,
        request.title.as_deref(),
        &request.message,
        &request.notification_type,
    )
    .await
    .map_err(notification_http_error)?;

    Ok(Json(json!({
        "success": true,
        "notification_id": notification_id
    })))
}
