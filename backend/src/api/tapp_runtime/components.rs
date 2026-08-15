//! 组件注册 API
//!
//! Domain registry: [`crate::services::tapp_components`]. This module owns
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
use crate::services::tapp_components::{
    self, ComponentRegistryError, ComponentType,
};

use super::common::{authorize_tapp_permission, parse_user_id, verify_tapp_ownership};
use super::runtime_grant::RuntimeGrantContext;
use crate::api::tapp_store::{installation_write_forbidden_error, TappStorageAccess};
use crate::error::HttpError;

#[derive(Debug, Deserialize)]
pub struct RegisterComponentRequest {
    pub tapp_id: String,
    pub component_type: ComponentType,
    pub config: Value,
}

fn component_http_error(err: ComponentRegistryError) -> (StatusCode, Json<Value>) {
    let status =
        StatusCode::from_u16(err.status_hint()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    match &err {
        ComponentRegistryError::InvalidConfig { .. } => (
            status,
            Json(json!({
                "error": err.message(),
                "code": err.code(),
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

/// POST /api/tapp/components/register
pub async fn register_component(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Json(req): Json<RegisterComponentRequest>,
) -> Result<Json<Value>, HttpError> {
    let (type_str, permission) = match &req.component_type {
        ComponentType::Theme => ("theme", TappPermission::ComponentTheme),
        ComponentType::Agent => ("agent", TappPermission::ComponentAgent),
    };
    runtime_grant.require_tapp_id(&req.tapp_id)?;
    runtime_grant.require(permission)?;
    authorize_tapp_permission(&db, &claims, &req.tapp_id, permission, &dynamic_config).await?;
    let owner_id = installation_owner(&claims, &runtime_grant)?;

    tracing::info!(
        "[TAPP] register_component - User: {}, Tapp: {}, Type: {}",
        claims.username,
        req.tapp_id,
        type_str
    );

    let registered = tapp_components::register_component(
        &db,
        owner_id,
        &req.tapp_id,
        req.component_type,
        req.config,
    )
    .await
    .map_err(component_http_error)?;

    Ok(Json(json!({
        "success": true,
        "component": {
            "id": registered.id,
            "type": registered.component_type,
            "tappId": registered.tapp_id,
            "registeredAt": registered.registered_at
        }
    })))
}

/// DELETE /api/tapp/components/{tapp_id}/{component_type}/{component_id}
pub async fn unregister_component(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, component_type, component_id)): Path<(String, String, String)>,
) -> Result<Json<Value>, HttpError> {
    let permission = match component_type.as_str() {
        "theme" => TappPermission::ComponentTheme,
        "agent" => TappPermission::ComponentAgent,
        _ => {
            return Err(HttpError::from(component_http_error(
                ComponentRegistryError::InvalidType,
            )));
        }
    };
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(permission)?;
    authorize_tapp_permission(&db, &claims, &tapp_id, permission, &dynamic_config).await?;
    let owner_id = installation_owner(&claims, &runtime_grant)?;
    tracing::info!(
        "[TAPP] unregister_component - User: {}, Tapp: {}, Type: {}, ID: {}",
        claims.username,
        tapp_id,
        component_type,
        component_id
    );

    tapp_components::unregister_component(
        &db,
        owner_id,
        &tapp_id,
        &component_type,
        &component_id,
    )
    .await
    .map_err(component_http_error)?;

    Ok(Json(json!({
        "success": true,
        "unregistered": { "id": component_id, "type": component_type, "tappId": tapp_id }
    })))
}

/// GET /api/tapp/components/{tapp_id}
pub async fn list_components(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    let user_id = parse_user_id(&claims)?;
    verify_tapp_ownership(&db, user_id, &tapp_id).await?;
    let access = TappStorageAccess::from_runtime_grant(&runtime_grant, &claims)?;
    let owner_id = access.installation_namespace();
    tracing::debug!(
        "[TAPP] list_components - User: {}, Tapp: {}",
        claims.username,
        tapp_id
    );

    let type_filter = params.get("type").map(String::as_str);
    if type_filter.is_some_and(|value| !matches!(value, "theme" | "agent")) {
        return Err(HttpError::from(component_http_error(
            ComponentRegistryError::InvalidType,
        )));
    }
    match type_filter {
        Some("theme") => runtime_grant.require(TappPermission::ComponentTheme)?,
        Some("agent") => runtime_grant.require(TappPermission::ComponentAgent)?,
        None if !runtime_grant.has(TappPermission::ComponentTheme)
            && !runtime_grant.has(TappPermission::ComponentAgent) =>
        {
            runtime_grant.require(TappPermission::ComponentTheme)?
        }
        _ => {}
    }

    let components = tapp_components::list_components_for_tapp(
        &db,
        owner_id,
        &tapp_id,
        type_filter,
    )
    .await
    .map_err(component_http_error)?;

    let components: Vec<Value> = components
        .into_iter()
        .filter(|value| match value.get("type").and_then(Value::as_str) {
            Some("theme") => runtime_grant.has(TappPermission::ComponentTheme),
            Some("agent") => runtime_grant.has(TappPermission::ComponentAgent),
            _ => false,
        })
        .collect();

    Ok(Json(json!({ "success": true, "components": components })))
}

/// GET /api/tapp/components/all/{component_type}
pub async fn list_all_components_by_type(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(component_type): Path<String>,
) -> Result<Json<Value>, HttpError> {
    match component_type.as_str() {
        "theme" => runtime_grant.require(TappPermission::ComponentTheme)?,
        "agent" => runtime_grant.require(TappPermission::ComponentAgent)?,
        _ => {
            return Err(HttpError::from(component_http_error(
                ComponentRegistryError::InvalidType,
            )))
        }
    }
    tracing::debug!(
        "[TAPP] list_all_components_by_type - User: {}, Type: {}",
        claims.username,
        component_type
    );

    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    let components =
        tapp_components::list_components_by_type_for_subject(&db, user_id, &component_type)
            .await
            .map_err(component_http_error)?;

    Ok(Json(
        json!({ "success": true, "type": component_type, "components": components }),
    ))
}
