//! 数据转换处理 API
//!
//! Pure pipeline evaluation: [`crate::services::tapp_data_transform`].
//! Storage IO/validation: [`crate::services::tapp_storage`].
//! Platform cache IO: [`crate::services::platform_cache`].
//! This module owns grant/permission checks and Axum DTOs.

use axum::{extract::State, http::StatusCode, Extension, Json};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::middleware::auth::Claims;
use crate::services::permission_service::TappPermission;
use crate::services::platform_cache::{
    acquire_platform_lock, get_cached_platform_data, validate_platform_name, write_filtered_document,
};
use crate::services::tapp_data_transform::{self, DataTransformError, ProcessStep};
use crate::error::HttpError;
use crate::services::tapp_storage::{
    self, read_storage_value, validate_sandbox_storage_key, validate_storage_value_size,
    write_storage_value, TappStorageError,
};

use super::common::{authorize_tapp_permissions, parse_user_id, verify_tapp_ownership};
use super::runtime_grant::RuntimeGrantContext;

#[derive(Debug, Deserialize)]
pub struct DataTransformRequest {
    pub tapp_id: String,
    pub input: DataInput,
    pub pipeline: Vec<ProcessStep>,
    pub output: Option<DataOutput>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "source")]
pub enum DataInput {
    #[serde(rename = "platform")]
    Platform { platform: String },
    #[serde(rename = "storage")]
    Storage { key: String },
    #[serde(rename = "inline")]
    Inline { data: Value },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "target")]
pub enum DataOutput {
    #[serde(rename = "platform")]
    Platform { platform: String },
    #[serde(rename = "storage")]
    Storage { key: String },
}

fn transform_http_error(err: DataTransformError) -> (StatusCode, Json<Value>) {
    let status =
        StatusCode::from_u16(err.status_hint()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(json!({ "error": err.message() })))
}

fn storage_http_error(err: TappStorageError) -> (StatusCode, Json<Value>) {
    // Preserve historical transform error strings for storage I/O.
    match err {
        TappStorageError::InvalidKey(reason) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": reason })),
        ),
        TappStorageError::TooLarge => (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({ "error": "Storage value too large" })),
        ),
        TappStorageError::Database => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to read storage" })),
        ),
    }
}

fn storage_write_http_error(err: TappStorageError) -> (StatusCode, Json<Value>) {
    match err {
        TappStorageError::TooLarge => (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({ "error": "Storage value too large" })),
        ),
        TappStorageError::InvalidKey(reason) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": reason })),
        ),
        TappStorageError::Database => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to save storage" })),
        ),
    }
}

/// Resolve subject namespace for sandbox storage: grant subject must match JWT.
fn storage_subject_id(
    claims: &Claims,
    runtime_grant: &RuntimeGrantContext,
) -> Result<i32, HttpError> {
    let subject_id = parse_user_id(claims)?;
    if runtime_grant.subject_id() != subject_id {
        return Err(HttpError::from((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Invalid runtime grant subject" })),
        )));
    }
    Ok(subject_id)
}

/// POST /api/tapp/data/transform
pub async fn data_transform(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Json(req): Json<DataTransformRequest>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&req.tapp_id)?;
    let mut required_permissions = Vec::with_capacity(2);
    match &req.input {
        DataInput::Platform { .. } => required_permissions.push(TappPermission::PlatformRead),
        DataInput::Storage { key } => {
            validate_sandbox_storage_key(key)
                .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))))?;
            required_permissions.push(TappPermission::StorageRead);
        }
        DataInput::Inline { .. } => {}
    }
    match &req.output {
        Some(DataOutput::Platform { .. }) => {
            required_permissions.push(TappPermission::PlatformWrite)
        }
        Some(DataOutput::Storage { key }) => {
            validate_sandbox_storage_key(key)
                .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))))?;
            required_permissions.push(TappPermission::StorageWrite);
        }
        None => {}
    }
    for permission in &required_permissions {
        runtime_grant.require(*permission)?;
    }

    if required_permissions.is_empty() {
        let user_id = parse_user_id(&claims)?;
        verify_tapp_ownership(&db, user_id, &req.tapp_id).await?;
    } else {
        authorize_tapp_permissions(&db, &claims, &req.tapp_id, &required_permissions, &dynamic_config).await?;
    }

    // Storage I/O always follows the current runtime subject, including when a
    // public installation is owned by the site administrator.
    let storage_subject_id = storage_subject_id(&claims, &runtime_grant)?;

    tracing::debug!(
        "[TAPP] data_transform - User: {}, Tapp: {}, Steps: {}",
        claims.username,
        req.tapp_id,
        req.pipeline.len()
    );

    // 1. 获取输入数据
    let mut items: Vec<Value> = match req.input {
        DataInput::Platform { platform } => {
            let data = get_cached_platform_data(&platform).await.map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": error })),
                )
            })?;
            data.get("items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
        }
        DataInput::Storage { key } => {
            let value = read_storage_value(&db, storage_subject_id, &req.tapp_id, &key)
                .await
                .map_err(storage_http_error)?;
            value.as_array().cloned().unwrap_or_default()
        }
        DataInput::Inline { data } => tapp_data_transform::items_from_value(data),
    };

    // 2. 执行处理管道（pure domain）
    items =
        tapp_data_transform::apply_pipeline(items, req.pipeline).map_err(transform_http_error)?;

    // 3. 输出结果
    if let Some(output) = req.output {
        match output {
            DataOutput::Platform { platform } => {
                validate_platform_name(&platform)
                    .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;
                let _platform_guard = acquire_platform_lock(&platform)
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;
                // Replace the filtered document items array (historical transform semantics).
                let data = json!({ "items": items });
                write_filtered_document(&platform, &data)
                    .await
                    .map_err(|error| {
                        (
                            StatusCode::from_u16(error.status_hint())
                                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                            Json(json!({ "error": error.message() })),
                        )
                    })?;
            }
            DataOutput::Storage { key } => {
                let storage_value = json!(items);
                validate_storage_value_size(&storage_value).map_err(storage_write_http_error)?;
                write_storage_value(&db, storage_subject_id, &req.tapp_id, &key, storage_value)
                    .await
                    .map_err(storage_write_http_error)?;
            }
        }
    }

    Ok(Json(json!({
        "success": true,
        "count": items.len(),
        "data": items
    })))
}

// Keep services module linked for quota constant visibility in docs/tests.
const _: i64 = tapp_storage::TAPP_STORAGE_QUOTA_BYTES;
