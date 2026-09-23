//! Manifest-declared host settings, install-level shared data,
//! installation-private owner/admin KV, and subject-private Tapp storage.
//!
//! Domain validators/IO live in `services::tapp_storage`; this module adapts
//! them to Axum status codes and owns HTTP handlers.

use super::{
    ApiResponse, TappSettingDef, TappStorageAccess, authorize_runtime_storage,
    authorize_runtime_storage_write, can_write_installation_settings, current_is_admin,
    optional_authenticated_user_id, tapp_setting_value_is_valid, validate_tapp_id,
};
use crate::api::tapp_runtime::{RuntimeGrantContext, common as tapp_common};
use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::models::entities::tapp_storage as tapp_storage_entity;
use crate::services::tapp_storage::{
    self as storage_svc, TAPP_STORAGE_QUOTA_BYTES, TappStorageError,
};
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
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

const SHARED_KEY_PREFIX: &str = "_shared.";
const PRIVATE_KEY_PREFIX: &str = "_private.";

fn prefixed_storage_key(prefix: &str, key: &str) -> Result<String, HttpError> {
    validate_sandbox_storage_key(key)
        .map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    Ok(format!("{prefix}{key}"))
}

async fn list_prefixed_keys(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    prefix: &str,
) -> Result<Vec<String>, HttpError> {
    Ok(tapp_storage_entity::Entity::find()
        .select_only()
        .column(tapp_storage_entity::Column::Key)
        .filter(tapp_storage_entity::Column::UserId.eq(user_id))
        .filter(tapp_storage_entity::Column::TappId.eq(tapp_id))
        .filter(tapp_storage_entity::Column::Key.starts_with(prefix))
        .into_tuple::<String>()
        .all(db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?
        .into_iter()
        .filter_map(|key| key.strip_prefix(prefix).map(str::to_string))
        .collect())
}

async fn list_prefixed_entries(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    prefix: &str,
) -> Result<BTreeMap<String, serde_json::Value>, HttpError> {
    Ok(tapp_storage_entity::Entity::find()
        .select_only()
        .column(tapp_storage_entity::Column::Key)
        .column(tapp_storage_entity::Column::Value)
        .filter(tapp_storage_entity::Column::UserId.eq(user_id))
        .filter(tapp_storage_entity::Column::TappId.eq(tapp_id))
        .filter(tapp_storage_entity::Column::Key.starts_with(prefix))
        .into_model::<StorageKeyValueRow>()
        .all(db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?
        .into_iter()
        .filter_map(|item| {
            item.key
                .strip_prefix(prefix)
                .map(|key| (key.to_string(), item.value))
        })
        .collect())
}

async fn delete_prefixed_key(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    storage_key: &str,
) -> Result<(), HttpError> {
    tapp_storage_entity::Entity::delete_many()
        .filter(tapp_storage_entity::Column::UserId.eq(user_id))
        .filter(tapp_storage_entity::Column::TappId.eq(tapp_id))
        .filter(tapp_storage_entity::Column::Key.eq(storage_key))
        .exec(db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;
    Ok(())
}

async fn clear_prefixed_keys(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    prefix: &str,
) -> Result<(), HttpError> {
    tapp_storage_entity::Entity::delete_many()
        .filter(tapp_storage_entity::Column::UserId.eq(user_id))
        .filter(tapp_storage_entity::Column::TappId.eq(tapp_id))
        .filter(tapp_storage_entity::Column::Key.starts_with(prefix))
        .exec(db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;
    Ok(())
}

/// Read path: real users and signed guests. Public installs resolve to the
/// site-owner namespace so visitors can read the owner's shared repository.
async fn authorize_tapp_shared(
    db: &DatabaseConnection,
    claims: &Claims,
    tapp_id: &str,
) -> Result<TappStorageAccess, HttpError> {
    validate_tapp_id(tapp_id).map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let subject_id = settings_subject_id(claims)?;
    let tapp = tapp_common::resolve_accessible_tapp(db, subject_id, tapp_id).await?;
    Ok(TappStorageAccess::from_owner_and_subject(
        tapp.user_id,
        subject_id,
    ))
}

/// Write path: durable authenticated users only (not guests).
async fn authorize_tapp_shared_write(
    db: &DatabaseConnection,
    claims: &Claims,
    tapp_id: &str,
) -> Result<TappStorageAccess, HttpError> {
    validate_tapp_id(tapp_id).map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let subject_id = optional_authenticated_user_id(Some(claims))
        .ok_or_else(|| HttpError(AppError::unauthorized("Unauthorized")))?;
    let tapp = tapp_common::resolve_accessible_tapp(db, subject_id, tapp_id).await?;
    Ok(TappStorageAccess::from_owner_and_subject(
        tapp.user_id,
        subject_id,
    ))
}

fn shared_storage_key(key: &str) -> Result<String, HttpError> {
    prefixed_storage_key(SHARED_KEY_PREFIX, key)
}

async fn require_shared_write(
    db: &DatabaseConnection,
    claims: &Claims,
    access: TappStorageAccess,
) -> Result<(), HttpError> {
    if can_write_installation_settings(access, current_is_admin(claims, db).await) {
        Ok(())
    } else {
        Err(HttpError(AppError::forbidden("Forbidden")))
    }
}

pub(super) async fn list_shared_keys(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<Vec<String>>>, HttpError> {
    let access = authorize_tapp_shared(&db, &claims, &tapp_id).await?;
    let keys = list_prefixed_keys(
        &db,
        access.installation_namespace(),
        &tapp_id,
        SHARED_KEY_PREFIX,
    )
    .await?;
    Ok(Json(ApiResponse::success(keys)))
}

pub(super) async fn list_shared_entries(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<BTreeMap<String, serde_json::Value>>>, HttpError> {
    let access = authorize_tapp_shared(&db, &claims, &tapp_id).await?;
    let values = list_prefixed_entries(
        &db,
        access.installation_namespace(),
        &tapp_id,
        SHARED_KEY_PREFIX,
    )
    .await?;
    Ok(Json(ApiResponse::success(values)))
}

pub(super) async fn get_shared_usage(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<TappStorageUsage>>, HttpError> {
    let access = authorize_tapp_shared(&db, &claims, &tapp_id).await?;
    let used = storage_bytes(&db, access.installation_namespace(), &tapp_id).await? as usize;
    Ok(Json(ApiResponse::success(TappStorageUsage {
        used,
        quota: TAPP_STORAGE_QUOTA_BYTES as usize,
    })))
}

pub(super) async fn get_shared(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, key)): Path<(String, String)>,
) -> Result<Json<ApiResponse<serde_json::Value>>, HttpError> {
    let storage_key = shared_storage_key(&key)?;
    let access = authorize_tapp_shared(&db, &claims, &tapp_id).await?;
    let value =
        read_storage_value(&db, access.installation_namespace(), &tapp_id, &storage_key).await?;
    Ok(Json(ApiResponse::success(value)))
}

pub(super) async fn set_shared(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, key)): Path<(String, String)>,
    Json(value): Json<serde_json::Value>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    let storage_key = shared_storage_key(&key)?;
    validate_storage_value_size(&value)?;
    let access = authorize_tapp_shared_write(&db, &claims, &tapp_id).await?;
    require_shared_write(&db, &claims, access).await?;
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

pub(super) async fn delete_shared(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, key)): Path<(String, String)>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    let storage_key = shared_storage_key(&key)?;
    let access = authorize_tapp_shared_write(&db, &claims, &tapp_id).await?;
    require_shared_write(&db, &claims, access).await?;
    delete_prefixed_key(&db, access.installation_namespace(), &tapp_id, &storage_key).await?;
    Ok(Json(ApiResponse::success(())))
}

pub(super) async fn clear_shared(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    let access = authorize_tapp_shared_write(&db, &claims, &tapp_id).await?;
    require_shared_write(&db, &claims, access).await?;
    clear_prefixed_keys(
        &db,
        access.installation_namespace(),
        &tapp_id,
        SHARED_KEY_PREFIX,
    )
    .await?;
    Ok(Json(ApiResponse::success(())))
}

/// Guest `sub` (negative / missing) never becomes a private-KV subject.
fn require_private_kv_subject(claims: &Claims) -> Result<i32, HttpError> {
    optional_authenticated_user_id(Some(claims))
        .ok_or_else(|| HttpError(AppError::unauthorized("Unauthorized")))
}

/// Owner or current admin only. Callers must already have an authenticated subject.
fn decide_private_kv_access(
    subject_id: i32,
    owner_id: i32,
    is_admin: bool,
) -> Result<TappStorageAccess, HttpError> {
    let access = TappStorageAccess::from_owner_and_subject(owner_id, subject_id);
    if can_write_installation_settings(access, is_admin) {
        Ok(access)
    } else {
        Err(HttpError(AppError::forbidden("Forbidden")))
    }
}

/// Read and write: durable authenticated owner or current admin only.
/// Guests and ordinary viewers never reach the storage lookup.
async fn authorize_tapp_private(
    db: &DatabaseConnection,
    claims: &Claims,
    tapp_id: &str,
) -> Result<TappStorageAccess, HttpError> {
    validate_tapp_id(tapp_id).map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let subject_id = require_private_kv_subject(claims)?;
    let tapp = tapp_common::resolve_accessible_tapp(db, subject_id, tapp_id).await?;
    decide_private_kv_access(subject_id, tapp.user_id, current_is_admin(claims, db).await)
}

fn private_storage_key(key: &str) -> Result<String, HttpError> {
    prefixed_storage_key(PRIVATE_KEY_PREFIX, key)
}

pub(super) async fn list_private_keys(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<Vec<String>>>, HttpError> {
    let access = authorize_tapp_private(&db, &claims, &tapp_id).await?;
    let keys = list_prefixed_keys(
        &db,
        access.installation_namespace(),
        &tapp_id,
        PRIVATE_KEY_PREFIX,
    )
    .await?;
    Ok(Json(ApiResponse::success(keys)))
}

pub(super) async fn list_private_entries(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<BTreeMap<String, serde_json::Value>>>, HttpError> {
    let access = authorize_tapp_private(&db, &claims, &tapp_id).await?;
    let values = list_prefixed_entries(
        &db,
        access.installation_namespace(),
        &tapp_id,
        PRIVATE_KEY_PREFIX,
    )
    .await?;
    Ok(Json(ApiResponse::success(values)))
}

pub(super) async fn get_private_usage(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<TappStorageUsage>>, HttpError> {
    let access = authorize_tapp_private(&db, &claims, &tapp_id).await?;
    let used = storage_bytes(&db, access.installation_namespace(), &tapp_id).await? as usize;
    Ok(Json(ApiResponse::success(TappStorageUsage {
        used,
        quota: TAPP_STORAGE_QUOTA_BYTES as usize,
    })))
}

pub(super) async fn get_private(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, key)): Path<(String, String)>,
) -> Result<Json<ApiResponse<serde_json::Value>>, HttpError> {
    let access = authorize_tapp_private(&db, &claims, &tapp_id).await?;
    let storage_key = private_storage_key(&key)?;
    let value =
        read_storage_value(&db, access.installation_namespace(), &tapp_id, &storage_key).await?;
    Ok(Json(ApiResponse::success(value)))
}

pub(super) async fn set_private(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, key)): Path<(String, String)>,
    Json(value): Json<serde_json::Value>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    let access = authorize_tapp_private(&db, &claims, &tapp_id).await?;
    let storage_key = private_storage_key(&key)?;
    validate_storage_value_size(&value)?;
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

pub(super) async fn delete_private(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, key)): Path<(String, String)>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    let access = authorize_tapp_private(&db, &claims, &tapp_id).await?;
    let storage_key = private_storage_key(&key)?;
    delete_prefixed_key(&db, access.installation_namespace(), &tapp_id, &storage_key).await?;
    Ok(Json(ApiResponse::success(())))
}

pub(super) async fn clear_private(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    let access = authorize_tapp_private(&db, &claims, &tapp_id).await?;
    clear_prefixed_keys(
        &db,
        access.installation_namespace(),
        &tapp_id,
        PRIVATE_KEY_PREFIX,
    )
    .await?;
    Ok(Json(ApiResponse::success(())))
}

pub(super) async fn list_storage_keys(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<Vec<String>>>, HttpError> {
    let access = authorize_runtime_storage(&claims, &runtime_grant, &tapp_id)?;
    let keys =
        storage_svc::sandbox_storage_keys(&db, access.private_storage_namespace(), &tapp_id)
            .await
            .map_err(|error| HttpError::from(storage_status(error)))?;
    Ok(Json(ApiResponse::success(keys)))
}

pub(super) async fn list_storage_entries(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<BTreeMap<String, serde_json::Value>>>, HttpError> {
    let access = authorize_runtime_storage(&claims, &runtime_grant, &tapp_id)?;
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
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<TappStorageUsage>>, HttpError> {
    let access = authorize_runtime_storage(&claims, &runtime_grant, &tapp_id)?;
    let used = storage_bytes(&db, access.private_storage_namespace(), &tapp_id).await? as usize;
    Ok(Json(ApiResponse::success(TappStorageUsage {
        used,
        quota: TAPP_STORAGE_QUOTA_BYTES as usize,
    })))
}

pub(super) async fn get_storage(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, key)): Path<(String, String)>,
) -> Result<Json<ApiResponse<serde_json::Value>>, HttpError> {
    validate_sandbox_storage_key(&key)
        .map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let access = authorize_runtime_storage(&claims, &runtime_grant, &tapp_id)?;
    let value = read_storage_value(&db, access.private_storage_namespace(), &tapp_id, &key).await?;
    Ok(Json(ApiResponse::success(value)))
}

pub(super) async fn set_storage(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, key)): Path<(String, String)>,
    Json(value): Json<serde_json::Value>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    validate_sandbox_storage_key(&key)
        .map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    validate_storage_value_size(&value)?;
    let access = authorize_runtime_storage_write(&claims, &runtime_grant, &tapp_id)?;
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
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, key)): Path<(String, String)>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    validate_sandbox_storage_key(&key)
        .map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let access = authorize_runtime_storage_write(&claims, &runtime_grant, &tapp_id)?;
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
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    let access = authorize_runtime_storage_write(&claims, &runtime_grant, &tapp_id)?;
    storage_svc::clear_sandbox_storage(&db, access.private_storage_namespace(), &tapp_id)
        .await
        .map_err(|error| HttpError::from(storage_status(error)))?;
    Ok(Json(ApiResponse::success(())))
}

#[cfg(test)]
mod private_kv_gate_tests {
    use super::{
        PRIVATE_KEY_PREFIX, decide_private_kv_access, prefixed_storage_key,
        require_private_kv_subject,
    };
    use crate::middleware::auth::Claims;

    fn claims(sub: &str) -> Claims {
        Claims {
            sub: sub.to_string(),
            username: "tester".to_string(),
            is_admin: false,
            is_owner: false,
            exp: 0,
            iat: 0,
            tv: 0,
            subject: crate::middleware::auth::AuthSubject::from_test_sub(sub),
        }
    }

    #[test]
    fn guests_and_missing_subjects_are_unauthorized_before_lookup() {
        assert_eq!(
            require_private_kv_subject(&claims("-12"))
                .unwrap_err()
                .0
                .status_u16(),
            401
        );
        assert_eq!(
            require_private_kv_subject(&claims("not-a-number"))
                .unwrap_err()
                .0
                .status_u16(),
            401
        );
        assert_eq!(require_private_kv_subject(&claims("7")).unwrap(), 7);
    }

    #[test]
    fn viewers_are_forbidden_owner_and_admin_are_allowed() {
        let viewer = decide_private_kv_access(42, 1, false).unwrap_err();
        assert_eq!(viewer.0.status_u16(), 403);

        let owner = decide_private_kv_access(1, 1, false).unwrap();
        assert_eq!(owner.installation_namespace(), 1);
        assert_eq!(owner.private_storage_namespace(), 1);

        let admin = decide_private_kv_access(42, 1, true).unwrap();
        assert_eq!(
            admin.installation_namespace(),
            1,
            "admin writes the install owner namespace, not their own subject id"
        );
        assert_eq!(admin.private_storage_namespace(), 42);
    }

    #[test]
    fn private_user_keys_cannot_reuse_host_prefixes_or_route_names() {
        assert_eq!(
            prefixed_storage_key(PRIVATE_KEY_PREFIX, "token").unwrap(),
            "_private.token"
        );
        assert!(prefixed_storage_key(PRIVATE_KEY_PREFIX, "_private.token").is_err());
        assert!(prefixed_storage_key(PRIVATE_KEY_PREFIX, "entries").is_err());
        assert!(prefixed_storage_key(PRIVATE_KEY_PREFIX, "usage").is_err());
    }
}
