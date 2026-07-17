//! 组件注册 API

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::middleware::auth::Claims;
use crate::services::permission_service::TappPermission;

use super::common::{authorize_tapp_permission, parse_user_id, verify_tapp_ownership};
use super::runtime_grant::RuntimeGrantContext;
use crate::api::tapp_store::{storage_write_forbidden_error, TappStorageAccess};

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum ComponentType {
    Theme,
    Agent,
}

#[derive(Debug, Deserialize)]
pub struct RegisterComponentRequest {
    pub tapp_id: String,
    pub component_type: ComponentType,
    pub config: Value,
}

fn invalid_component(message: impl Into<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": message.into(),
            "code": "INVALID_COMPONENT_CONFIG"
        })),
    )
}

fn validate_component_config<'a>(
    component_type: &ComponentType,
    config: &'a Value,
) -> Result<&'a str, (StatusCode, Json<Value>)> {
    let object = config
        .as_object()
        .ok_or_else(|| invalid_component("Component config must be an object"))?;
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 64
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
        .ok_or_else(|| invalid_component("Component id is invalid"))?;
    object
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty() && name.len() <= 100)
        .ok_or_else(|| {
            invalid_component("Component name is required and must not exceed 100 bytes")
        })?;

    match component_type {
        ComponentType::Theme => {
            if object
                .keys()
                .any(|key| !matches!(key.as_str(), "id" | "name" | "surface" | "glow"))
            {
                return Err(invalid_component(
                    "Theme config only supports id, name, surface and glow",
                ));
            }
            if object.get("surface").is_some_and(|value| {
                !matches!(value.as_str(), Some("glass" | "solid" | "flat" | "outline"))
            }) {
                return Err(invalid_component("Theme surface is invalid"));
            }
            if object.get("glow").is_some_and(|value| {
                !matches!(value.as_str(), Some("identity" | "primary" | "none"))
            }) {
                return Err(invalid_component("Theme glow is invalid"));
            }
        }
        ComponentType::Agent => {
            if object
                .keys()
                .any(|key| !matches!(key.as_str(), "id" | "name" | "description" | "capabilities"))
            {
                return Err(invalid_component(
                    "Agent config only supports id, name, description and capabilities",
                ));
            }
            if object
                .get("description")
                .is_some_and(|value| value.as_str().is_none_or(|text| text.len() > 500))
            {
                return Err(invalid_component("Agent description is invalid"));
            }
            let capabilities = object
                .get("capabilities")
                .and_then(Value::as_array)
                .filter(|items| !items.is_empty() && items.len() <= 64)
                .ok_or_else(|| invalid_component("Agent capabilities must contain 1-64 items"))?;
            if capabilities.iter().any(|value| {
                value
                    .as_str()
                    .is_none_or(|capability| capability.is_empty() || capability.len() > 64)
            }) {
                return Err(invalid_component("Agent capability is invalid"));
            }
        }
    }
    Ok(id)
}

/// POST /api/tapp/components/register
pub async fn register_component(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Json(req): Json<RegisterComponentRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let (type_str, permission) = match &req.component_type {
        ComponentType::Theme => ("theme", TappPermission::ComponentTheme),
        ComponentType::Agent => ("agent", TappPermission::ComponentAgent),
    };
    runtime_grant.require_tapp_id(&req.tapp_id)?;
    runtime_grant.require(permission)?;
    authorize_tapp_permission(&db, &claims, &req.tapp_id, permission).await?;
    let access = TappStorageAccess::from_runtime_grant(&runtime_grant, &claims)
        .map_err(|status| (status, Json(json!({ "error": "Invalid runtime grant subject" }))))?;
    access
        .require_write()
        .map_err(|_| storage_write_forbidden_error())?;
    let owner_id = access.storage_namespace();

    tracing::info!(
        "[TAPP] register_component - User: {}, Tapp: {}, Type: {}",
        claims.username,
        req.tapp_id,
        type_str
    );

    use crate::models::entities::tapp_storage;
    use sea_orm::{ActiveModelTrait, ActiveValue::NotSet, Set};

    let component_id = validate_component_config(&req.component_type, &req.config)?;

    let now = chrono::Utc::now().fixed_offset();
    let storage_key = format!("_component:{}:{}", type_str, component_id);

    let component_data = json!({
        "id": component_id,
        "type": type_str,
        "tappId": req.tapp_id,
        "config": req.config,
        "registeredAt": now.to_rfc3339(),
        "enabled": true
    });

    let existing = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(owner_id))
        .filter(tapp_storage::Column::TappId.eq(&req.tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .one(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    if let Some(existing) = existing {
        let mut active: tapp_storage::ActiveModel = existing.into();
        active.value = Set(component_data.clone());
        active.updated_at = Set(now);
        active.update(&db).await.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to update component" })),
            )
        })?;
    } else {
        let storage = tapp_storage::ActiveModel {
            id: NotSet,
            tapp_id: Set(req.tapp_id.clone()),
            user_id: Set(owner_id),
            key: Set(storage_key),
            value: Set(component_data.clone()),
            created_at: Set(now),
            updated_at: Set(now),
        };
        storage.insert(&db).await.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to register component" })),
            )
        })?;
    }

    Ok(Json(json!({
        "success": true,
        "component": {
            "id": component_id,
            "type": type_str,
            "tappId": req.tapp_id,
            "registeredAt": now.to_rfc3339()
        }
    })))
}

/// DELETE /api/tapp/components/{tapp_id}/{component_type}/{component_id}
pub async fn unregister_component(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, component_type, component_id)): Path<(String, String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let permission = match component_type.as_str() {
        "theme" => TappPermission::ComponentTheme,
        "agent" => TappPermission::ComponentAgent,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Invalid component type" })),
            ));
        }
    };
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(permission)?;
    authorize_tapp_permission(&db, &claims, &tapp_id, permission).await?;
    let access = TappStorageAccess::from_runtime_grant(&runtime_grant, &claims)
        .map_err(|status| (status, Json(json!({ "error": "Invalid runtime grant subject" }))))?;
    access
        .require_write()
        .map_err(|_| storage_write_forbidden_error())?;
    let owner_id = access.storage_namespace();
    tracing::info!(
        "[TAPP] unregister_component - User: {}, Tapp: {}, Type: {}, ID: {}",
        claims.username,
        tapp_id,
        component_type,
        component_id
    );

    use crate::models::entities::tapp_storage;

    let storage_key = format!("_component:{}:{}", component_type, component_id);

    let result = tapp_storage::Entity::delete_many()
        .filter(tapp_storage::Column::UserId.eq(owner_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .exec(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to unregister component" })),
            )
        })?;

    if result.rows_affected == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Component not found" })),
        ));
    }

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
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    let user_id = parse_user_id(&claims)?;
    verify_tapp_ownership(&db, user_id, &tapp_id).await?;
    let access = TappStorageAccess::from_runtime_grant(&runtime_grant, &claims)
        .map_err(|status| (status, Json(json!({ "error": "Invalid runtime grant subject" }))))?;
    let owner_id = access.storage_namespace();
    tracing::debug!(
        "[TAPP] list_components - User: {}, Tapp: {}",
        claims.username,
        tapp_id
    );

    use crate::models::entities::tapp_storage;

    let type_filter = params.get("type");
    if type_filter.is_some_and(|value| !matches!(value.as_str(), "theme" | "agent")) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Invalid component type" })),
        ));
    }
    match type_filter.map(String::as_str) {
        Some("theme") => runtime_grant.require(TappPermission::ComponentTheme)?,
        Some("agent") => runtime_grant.require(TappPermission::ComponentAgent)?,
        None if !runtime_grant.has(TappPermission::ComponentTheme)
            && !runtime_grant.has(TappPermission::ComponentAgent) =>
        {
            runtime_grant.require(TappPermission::ComponentTheme)?
        }
        _ => {}
    }
    let key_prefix = if let Some(t) = type_filter {
        format!("_component:{}:", t)
    } else {
        "_component:".to_string()
    };

    let items = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(owner_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.starts_with(&key_prefix))
        .order_by_asc(tapp_storage::Column::CreatedAt)
        .all(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    let components: Vec<Value> = items
        .into_iter()
        .map(|item| item.value)
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
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match component_type.as_str() {
        "theme" => runtime_grant.require(TappPermission::ComponentTheme)?,
        "agent" => runtime_grant.require(TappPermission::ComponentAgent)?,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Invalid component type" })),
            ))
        }
    }
    tracing::debug!(
        "[TAPP] list_all_components_by_type - User: {}, Type: {}",
        claims.username,
        component_type
    );

    use crate::models::entities::tapp_storage;

    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    let key_prefix = format!("_component:{}:", component_type);

    let items = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::Key.starts_with(&key_prefix))
        .order_by_asc(tapp_storage::Column::CreatedAt)
        .all(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    let components: Vec<Value> = items.into_iter().map(|item| item.value).collect();

    Ok(Json(
        json!({ "success": true, "type": component_type, "components": components }),
    ))
}

#[cfg(test)]
mod tests {
    use super::{validate_component_config, ComponentType};
    use serde_json::json;

    #[test]
    fn theme_config_accepts_only_effective_fields() {
        assert!(validate_component_config(
            &ComponentType::Theme,
            &json!({
                "id": "glass.primary",
                "name": "Glass Primary",
                "surface": "glass",
                "glow": "primary"
            })
        )
        .is_ok());
        assert!(validate_component_config(
            &ComponentType::Theme,
            &json!({ "id": "legacy", "name": "Legacy", "styles": "*{}" })
        )
        .is_err());
    }

    #[test]
    fn agent_config_requires_declared_capabilities() {
        assert!(validate_component_config(
            &ComponentType::Agent,
            &json!({ "id": "helper", "name": "Helper", "capabilities": ["chat"] })
        )
        .is_ok());
        assert!(validate_component_config(
            &ComponentType::Agent,
            &json!({ "id": "helper", "name": "Helper", "capabilities": [] })
        )
        .is_err());
    }
}
