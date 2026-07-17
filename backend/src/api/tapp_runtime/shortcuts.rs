//! 快捷键注册 API

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::middleware::auth::Claims;
use crate::services::permission_service::TappPermission;

use super::common::authorize_tapp_permission;
use super::runtime_grant::RuntimeGrantContext;
use crate::api::tapp_store::{storage_write_forbidden_error, TappStorageAccess};

#[derive(Debug, Deserialize)]
pub struct RegisterShortcutRequest {
    pub tapp_id: String,
    pub shortcut_id: String,
    pub keys: String,
    pub description: String,
    pub action: String,
    pub scope: Option<String>,
}

/// POST /api/tapp/shortcuts/register
pub async fn register_shortcut(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Json(req): Json<RegisterShortcutRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require_tapp_id(&req.tapp_id)?;
    runtime_grant.require(TappPermission::ShortcutRegister)?;
    authorize_tapp_permission(&db, &claims, &req.tapp_id, TappPermission::ShortcutRegister).await?;
    let access = TappStorageAccess::from_runtime_grant(&runtime_grant, &claims)
        .map_err(|status| (status, Json(json!({ "error": "Invalid runtime grant subject" }))))?;
    access
        .require_write()
        .map_err(|_| storage_write_forbidden_error())?;
    let owner_id = access.storage_namespace();

    tracing::info!(
        "[TAPP] register_shortcut - User: {}, Tapp: {}, Keys: {}",
        claims.username,
        req.tapp_id,
        req.keys
    );

    use crate::models::entities::tapp_storage;
    use sea_orm::{ActiveModelTrait, ActiveValue::NotSet, Set};

    if !validate_shortcut_keys(&req.keys) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Invalid shortcut key format" })),
        ));
    }

    let now = chrono::Utc::now().fixed_offset();
    let storage_key = format!("_shortcut:{}", req.shortcut_id);

    let shortcut_data = json!({
        "id": req.shortcut_id,
        "tappId": req.tapp_id,
        "keys": req.keys,
        "description": req.description,
        "action": req.action,
        "scope": req.scope.unwrap_or_else(|| "tapp".to_string()),
        "registeredAt": now.to_rfc3339(),
        "enabled": true
    });

    // 检查快捷键冲突（同一安装 owner 命名空间内）
    let existing = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(owner_id))
        .filter(tapp_storage::Column::Key.starts_with("_shortcut:"))
        .all(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    for item in existing {
        if let Some(keys) = item.value.get("keys").and_then(|v| v.as_str()) {
            if keys == req.keys {
                if let Some(id) = item.value.get("id").and_then(|v| v.as_str()) {
                    if id != req.shortcut_id {
                        return Err((
                            StatusCode::CONFLICT,
                            Json(
                                json!({ "error": "Shortcut key conflict", "conflicting_shortcut": id }),
                            ),
                        ));
                    }
                }
            }
        }
    }

    // Upsert
    let existing_item = tapp_storage::Entity::find()
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

    if let Some(existing_item) = existing_item {
        let mut active: tapp_storage::ActiveModel = existing_item.into();
        active.value = Set(shortcut_data.clone());
        active.updated_at = Set(now);
        active.update(&db).await.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to update shortcut" })),
            )
        })?;
    } else {
        let storage = tapp_storage::ActiveModel {
            id: NotSet,
            tapp_id: Set(req.tapp_id.clone()),
            user_id: Set(owner_id),
            key: Set(storage_key),
            value: Set(shortcut_data.clone()),
            created_at: Set(now),
            updated_at: Set(now),
        };
        storage.insert(&db).await.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to register shortcut" })),
            )
        })?;
    }

    Ok(Json(json!({ "success": true, "shortcut": shortcut_data })))
}

/// DELETE /api/tapp/shortcuts/{tapp_id}/{shortcut_id}
pub async fn unregister_shortcut(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, shortcut_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(TappPermission::ShortcutRegister)?;
    authorize_tapp_permission(&db, &claims, &tapp_id, TappPermission::ShortcutRegister).await?;
    let access = TappStorageAccess::from_runtime_grant(&runtime_grant, &claims)
        .map_err(|status| (status, Json(json!({ "error": "Invalid runtime grant subject" }))))?;
    access
        .require_write()
        .map_err(|_| storage_write_forbidden_error())?;
    let owner_id = access.storage_namespace();
    tracing::info!(
        "[TAPP] unregister_shortcut - User: {}, Tapp: {}, ID: {}",
        claims.username,
        tapp_id,
        shortcut_id
    );

    use crate::models::entities::tapp_storage;

    let storage_key = format!("_shortcut:{}", shortcut_id);

    let result = tapp_storage::Entity::delete_many()
        .filter(tapp_storage::Column::UserId.eq(owner_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .exec(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to unregister shortcut" })),
            )
        })?;

    if result.rows_affected == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Shortcut not found" })),
        ));
    }

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
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require(TappPermission::ShortcutRegister)?;
    tracing::debug!("[TAPP] list_shortcuts - User: {}", claims.username);

    use crate::models::entities::tapp_storage;

    let access = TappStorageAccess::from_runtime_grant(&runtime_grant, &claims)
        .map_err(|status| (status, Json(json!({ "error": "Invalid runtime grant subject" }))))?;
    let owner_id = access.storage_namespace();

    let mut query = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(owner_id))
        .filter(tapp_storage::Column::Key.starts_with("_shortcut:"));

    if let Some(tapp_id) = params.get("tapp_id") {
        runtime_grant.require_tapp_id(tapp_id)?;
    }
    query = query.filter(tapp_storage::Column::TappId.eq(runtime_grant.tapp_id()));

    let items = query
        .order_by_asc(tapp_storage::Column::CreatedAt)
        .all(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    let shortcuts: Vec<Value> = items.into_iter().map(|item| item.value).collect();

    Ok(Json(json!({ "success": true, "shortcuts": shortcuts })))
}

fn validate_shortcut_keys(keys: &str) -> bool {
    if keys.is_empty() || keys.len() > 50 {
        return false;
    }

    let parts: Vec<&str> = keys.split('+').collect();
    if parts.is_empty() || parts.len() > 4 {
        return false;
    }

    let valid_modifiers = ["ctrl", "alt", "shift", "meta", "cmd"];
    let mut has_key = false;

    for (i, part) in parts.iter().enumerate() {
        let lower = part.to_lowercase();
        if i == parts.len() - 1 {
            if lower.len() == 1
                || lower.starts_with('f') && lower.len() <= 3
                || [
                    "enter",
                    "escape",
                    "space",
                    "tab",
                    "backspace",
                    "delete",
                    "up",
                    "down",
                    "left",
                    "right",
                    "home",
                    "end",
                    "pageup",
                    "pagedown",
                ]
                .contains(&lower.as_str())
            {
                has_key = true;
            }
        } else if !valid_modifiers.contains(&lower.as_str()) {
            return false;
        }
    }

    has_key
}
