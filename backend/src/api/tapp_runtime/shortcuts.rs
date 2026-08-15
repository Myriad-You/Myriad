//! 快捷键注册 API
//!
//! Domain registry: [`crate::services::tapp_shortcuts`]. This module owns
//! grant/permission checks, installation-write gating, and Axum DTOs.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::middleware::auth::Claims;
use crate::services::permission_service::TappPermission;
use crate::services::tapp_shortcuts::{self, ShortcutRegistryError};

use super::common::authorize_tapp_permission;
use super::runtime_grant::RuntimeGrantContext;
use crate::api::tapp_store::{installation_write_forbidden_error, TappStorageAccess};
use crate::error::HttpError;

#[derive(Debug, Deserialize)]
pub struct RegisterShortcutRequest {
    pub tapp_id: String,
    pub shortcut_id: String,
    pub keys: String,
    pub description: String,
    pub action: String,
    pub scope: Option<String>,
}

fn shortcut_http_error(err: ShortcutRegistryError) -> (StatusCode, Json<Value>) {
    let status =
        StatusCode::from_u16(err.status_hint()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    match &err {
        ShortcutRegistryError::Conflict {
            conflicting_shortcut,
        } => (
            status,
            Json(json!({
                "error": "Shortcut key conflict",
                "conflicting_shortcut": conflicting_shortcut,
            })),
        ),
        _ => (status, Json(json!({ "error": err.message() }))),
    }
}

fn installation_owner(
    claims: &Claims,
    runtime_grant: &RuntimeGrantContext,
) -> Result<i32, HttpError> {
    let access = TappStorageAccess::from_runtime_grant(runtime_grant, claims)?;
    access
        .require_installation_write()
        .map_err(|_| installation_write_forbidden_error())?;
    Ok(access.installation_namespace())
}

/// POST /api/tapp/shortcuts/register
pub async fn register_shortcut(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Json(req): Json<RegisterShortcutRequest>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&req.tapp_id)?;
    runtime_grant.require(TappPermission::ShortcutRegister)?;
    authorize_tapp_permission(&db, &claims, &req.tapp_id, TappPermission::ShortcutRegister, &dynamic_config).await?;
    let owner_id = installation_owner(&claims, &runtime_grant)?;

    tracing::info!(
        "[TAPP] register_shortcut - User: {}, Tapp: {}, Keys: {}",
        claims.username,
        req.tapp_id,
        req.keys
    );

    let shortcut_data = tapp_shortcuts::register_shortcut(
        &db,
        owner_id,
        &req.tapp_id,
        &req.shortcut_id,
        &req.keys,
        &req.description,
        &req.action,
        req.scope,
    )
    .await
    .map_err(shortcut_http_error)?;

    Ok(Json(json!({ "success": true, "shortcut": shortcut_data })))
}

/// DELETE /api/tapp/shortcuts/{tapp_id}/{shortcut_id}
pub async fn unregister_shortcut(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, shortcut_id)): Path<(String, String)>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(TappPermission::ShortcutRegister)?;
    authorize_tapp_permission(&db, &claims, &tapp_id, TappPermission::ShortcutRegister, &dynamic_config).await?;
    let owner_id = installation_owner(&claims, &runtime_grant)?;
    tracing::info!(
        "[TAPP] unregister_shortcut - User: {}, Tapp: {}, ID: {}",
        claims.username,
        tapp_id,
        shortcut_id
    );

    tapp_shortcuts::unregister_shortcut(&db, owner_id, &tapp_id, &shortcut_id)
        .await
        .map_err(shortcut_http_error)?;

    Ok(Json(
        json!({ "success": true, "unregistered": shortcut_id }),
    ))
}

/// GET /api/tapp/shortcuts
pub async fn list_shortcuts(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require(TappPermission::ShortcutRegister)?;
    tracing::debug!("[TAPP] list_shortcuts - User: {}", claims.username);

    let access = TappStorageAccess::from_runtime_grant(&runtime_grant, &claims)?;
    let owner_id = access.installation_namespace();

    if let Some(tapp_id) = params.get("tapp_id") {
        runtime_grant.require_tapp_id(tapp_id)?;
    }
    let tapp_id = runtime_grant.tapp_id();

    let shortcuts = tapp_shortcuts::list_shortcuts(&db, owner_id, tapp_id)
        .await
        .map_err(shortcut_http_error)?;

    Ok(Json(json!({ "success": true, "shortcuts": shortcuts })))
}
