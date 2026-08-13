//! Manifest-declared host settings and subject-private Tapp storage.
//!
//! Domain validators/IO live in `services::tapp_storage`; this module adapts
//! them to Axum status codes and owns HTTP handlers.

use super::{
    authorize_runtime_storage, authorize_runtime_storage_write, can_write_installation_settings,
    current_is_admin,
    optional_authenticated_user_id, tapp_setting_value_is_valid, validate_tapp_id, ApiResponse,
    TappSettingDef, TappStorageAccess,
};
use crate::api::tapp_runtime::{common as tapp_common, RuntimeGrantContext};
use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::models::entities::tapp_storage as tapp_storage_entity;
use crate::services::tapp_storage::{
    self as storage_svc, TappStorageError, TAPP_STORAGE_QUOTA_BYTES,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use myriad_error::AppError;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, FromQueryResult, QueryFilter, QuerySelect,
};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

pub(crate) use storage_svc::{validate_sandbox_storage_key, validate_storage_key};

#[derive(FromQueryResult)]
struct StorageKeyValueRow {
    key: String,
    value: serde_json::Value,
}

fn storage_status(err: TappStorageError) -> StatusCode {
    match err {
        TappStorageError::InvalidKey(_) => StatusCode::BAD_REQUEST,
        TappStorageError::Database => StatusCode::INTERNAL_SERVER_ERROR,
        TappStorageError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
    }
}

pub(crate) fn validate_storage_value_size(value: &serde_json::Value) -> Result<(), HttpError> {
    storage_svc::validate_storage_value_size(value).map_err(|e| HttpError::from(storage_status(e)))
}

async fn storage_bytes(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
) -> Result<i64, HttpError> {
    storage_svc::storage_bytes(db, user_id, tapp_id)
        .await
        .map_err(|e| HttpError::from(storage_status(e)))
}

pub(crate) async fn read_storage_value(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    key: &str,
) -> Result<serde_json::Value, HttpError> {
    storage_svc::read_storage_value(db, user_id, tapp_id, key)
        .await
        .map_err(|e| HttpError::from(storage_status(e)))
}

pub(crate) async fn write_storage_value(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    key: &str,
    value: serde_json::Value,
) -> Result<(), HttpError> {
    storage_svc::write_storage_value(db, user_id, tapp_id, key, value)
        .await
        .map_err(|e| HttpError::from(storage_status(e)))
}

/// Subject for settings read: real users **and** signed guests (negative ids).
/// Public installs resolve to the site-owner namespace so guests can read host
/// settings without a login 401.
fn settings_subject_id(claims: &Claims) -> Result<i32, HttpError> {
    claims
        .sub
        .parse::<i32>()
        .map_err(|_| HttpError(AppError::unauthorized("Unauthorized")))
}

async fn authorize_tapp_settings(
    db: &DatabaseConnection,
    claims: &Claims,
    tapp_id: &str,
) -> Result<(TappStorageAccess, Vec<TappSettingDef>), HttpError> {
    validate_tapp_id(tapp_id).map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let subject_id = settings_subject_id(claims)?;
    let tapp = tapp_common::resolve_accessible_tapp(db, subject_id, tapp_id).await?;
    let settings = tapp
        .manifest
        .get("settings")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|setting| {
            serde_json::from_value(setting)
                .map_err(|_| HttpError(AppError::internal("Database error")))
        })
        .collect::<Result<Vec<TappSettingDef>, HttpError>>()?;
    Ok((
        TappStorageAccess::from_owner_and_subject(tapp.user_id, subject_id),
        settings,
    ))
}

/// Write path: durable authenticated users only (not guests).
async fn authorize_tapp_settings_write(
    db: &DatabaseConnection,
    claims: &Claims,
    tapp_id: &str,
) -> Result<(TappStorageAccess, Vec<TappSettingDef>), HttpError> {
    validate_tapp_id(tapp_id).map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let subject_id = optional_authenticated_user_id(Some(claims))
        .ok_or_else(|| HttpError(AppError::unauthorized("Unauthorized")))?;
    let tapp = tapp_common::resolve_accessible_tapp(db, subject_id, tapp_id).await?;
    let settings = tapp
        .manifest
        .get("settings")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|setting| {
            serde_json::from_value(setting)
                .map_err(|_| HttpError(AppError::internal("Database error")))
        })
        .collect::<Result<Vec<TappSettingDef>, HttpError>>()?;
    Ok((
        TappStorageAccess::from_owner_and_subject(tapp.user_id, subject_id),
        settings,
    ))
}

async fn authorize_tapp_setting(
    db: &DatabaseConnection,
    claims: &Claims,
    tapp_id: &str,
    key: &str,
) -> Result<(TappStorageAccess, String, TappSettingDef), HttpError> {
    validate_storage_key(key).map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let (access, settings) = authorize_tapp_settings(db, claims, tapp_id).await?;
    let setting = settings
        .into_iter()
        .find(|setting| setting.key == key)
        .ok_or_else(|| HttpError(AppError::not_found("Not found")))?;
    Ok((access, format!("_settings.{key}"), setting))
}

async fn authorize_tapp_setting_write(
    db: &DatabaseConnection,
    claims: &Claims,
    tapp_id: &str,
    key: &str,
) -> Result<(TappStorageAccess, String, TappSettingDef), HttpError> {
    validate_storage_key(key).map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let (access, settings) = authorize_tapp_settings_write(db, claims, tapp_id).await?;
    let setting = settings
        .into_iter()
        .find(|setting| setting.key == key)
        .ok_or_else(|| HttpError(AppError::not_found("Not found")))?;
    Ok((access, format!("_settings.{key}"), setting))
}

pub(super) async fn get_tapp_settings(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<BTreeMap<String, serde_json::Value>>>, HttpError> {
    let (access, settings) = authorize_tapp_settings(&db, &claims, &tapp_id).await?;
    let declared_keys: HashSet<String> = settings.into_iter().map(|setting| setting.key).collect();
    let values = tapp_storage_entity::Entity::find()
        .select_only()
        .column(tapp_storage_entity::Column::Key)
        .column(tapp_storage_entity::Column::Value)
        .filter(tapp_storage_entity::Column::UserId.eq(access.installation_namespace()))
        .filter(tapp_storage_entity::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage_entity::Column::Key.starts_with("_settings."))
        .into_model::<StorageKeyValueRow>()
        .all(&db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?
        .into_iter()
        .filter_map(|item| {
            let key = item.key.strip_prefix("_settings.")?.to_string();
            declared_keys.contains(&key).then_some((key, item.value))
        })
        .collect();
    Ok(Json(ApiResponse::success(values)))
}

pub(super) async fn get_tapp_setting(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, key)): Path<(String, String)>,
) -> Result<Json<ApiResponse<serde_json::Value>>, HttpError> {
    let (access, storage_key, _) = authorize_tapp_setting(&db, &claims, &tapp_id, &key).await?;
    let value =
        read_storage_value(&db, access.installation_namespace(), &tapp_id, &storage_key).await?;
    Ok(Json(ApiResponse::success(value)))
}

pub(super) async fn set_tapp_setting(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, key)): Path<(String, String)>,
    Json(value): Json<serde_json::Value>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    validate_storage_value_size(&value)?;
    let (access, storage_key, setting) =
        authorize_tapp_setting_write(&db, &claims, &tapp_id, &key).await?;
    if !can_write_installation_settings(access, current_is_admin(&claims, &db).await) {
        return Err(HttpError(AppError::forbidden("Forbidden")));
    }
    if !tapp_setting_value_is_valid(&setting, &value) {
        return Err(HttpError(AppError::bad_request("Bad request")));
    }
    write_storage_value(
        &db,
        access.installation_namespace(),
        &tapp_id,
        &storage_key,
        value,
    )
    .await?;
    Ok(Json(ApiResponse::success(())))
}

pub(super) async fn list_storage_keys(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<Vec<String>>>, HttpError> {
    let access =
        authorize_runtime_storage(&db, &claims, &runtime_grant, &tapp_id, &dynamic_config).await?;
    let keys =
        storage_svc::sandbox_storage_entries(&db, access.private_storage_namespace(), &tapp_id)
            .await
            .map_err(|error| HttpError::from(storage_status(error)))?
            .into_iter()
            .map(|item| item.key)
            .collect();
    Ok(Json(ApiResponse::success(keys)))
}

pub(super) async fn list_storage_entries(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<BTreeMap<String, serde_json::Value>>>, HttpError> {
    let access =
        authorize_runtime_storage(&db, &claims, &runtime_grant, &tapp_id, &dynamic_config).await?;
    let entries =
        storage_svc::sandbox_storage_entries(&db, access.private_storage_namespace(), &tapp_id)
            .await
            .map_err(|error| HttpError::from(storage_status(error)))?
            .into_iter()
            .map(|item| (item.key, item.value))
            .collect();
    Ok(Json(ApiResponse::success(entries)))
}

#[derive(Debug, Serialize)]
pub(super) struct TappStorageUsage {
    used: usize,
    quota: usize,
}

pub(super) async fn get_storage_usage(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<TappStorageUsage>>, HttpError> {
    let access =
        authorize_runtime_storage(&db, &claims, &runtime_grant, &tapp_id, &dynamic_config).await?;
    let used = storage_bytes(&db, access.private_storage_namespace(), &tapp_id).await? as usize;
    Ok(Json(ApiResponse::success(TappStorageUsage {
        used,
        quota: TAPP_STORAGE_QUOTA_BYTES as usize,
    })))
}

pub(super) async fn get_storage(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, key)): Path<(String, String)>,
) -> Result<Json<ApiResponse<serde_json::Value>>, HttpError> {
    validate_sandbox_storage_key(&key)
        .map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let access =
        authorize_runtime_storage(&db, &claims, &runtime_grant, &tapp_id, &dynamic_config).await?;
    let value = read_storage_value(&db, access.private_storage_namespace(), &tapp_id, &key).await?;
    Ok(Json(ApiResponse::success(value)))
}

pub(super) async fn set_storage(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, key)): Path<(String, String)>,
    Json(value): Json<serde_json::Value>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    validate_sandbox_storage_key(&key)
        .map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    validate_storage_value_size(&value)?;
    let access =
        authorize_runtime_storage_write(&db, &claims, &runtime_grant, &tapp_id, &dynamic_config).await?;
    write_storage_value(
        &db,
        access.private_storage_namespace(),
        &tapp_id,
        &key,
        value,
    )
    .await?;
    Ok(Json(ApiResponse::success(())))
}

pub(super) async fn delete_storage(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, key)): Path<(String, String)>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    validate_sandbox_storage_key(&key)
        .map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let access =
        authorize_runtime_storage_write(&db, &claims, &runtime_grant, &tapp_id, &dynamic_config).await?;
    tapp_storage_entity::Entity::delete_many()
        .filter(tapp_storage_entity::Column::UserId.eq(access.private_storage_namespace()))
        .filter(tapp_storage_entity::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage_entity::Column::Key.eq(&key))
        .exec(&db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;
    Ok(Json(ApiResponse::success(())))
}

pub(super) async fn clear_storage(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    let access =
        authorize_runtime_storage_write(&db, &claims, &runtime_grant, &tapp_id, &dynamic_config).await?;
    storage_svc::clear_sandbox_storage(&db, access.private_storage_namespace(), &tapp_id)
        .await
        .map_err(|error| HttpError::from(storage_status(error)))?;
    Ok(Json(ApiResponse::success(())))
}
