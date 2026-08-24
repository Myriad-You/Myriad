//! TAPP-facing Tripo 3D surface.
//!
//! Admin Merope routes stay admin-only. These handlers require a Runtime
//! Grant with `3d:generate` (except public asset reads, which the host does
//! locally). The Tripo API key never appears in responses or error bodies.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::HttpError;
use crate::services::permission_service::TappPermission;
use crate::services::tripo::{
    apply_web_defaults, is_configured, is_enabled, persist_task_models, poll_until_terminal,
    validate_upload_type, PersistedTripoAsset, TripoClient, TripoError, TripoOperation, TripoTask,
    PUBLIC_CAPABILITIES, TAPP_UPLOAD_MAX_BYTES,
};
use crate::state::AppState;

use super::RuntimeGrantContext;

type ApiError = HttpError;

#[derive(Debug, Deserialize)]
pub struct UploadRequest {
    #[serde(alias = "fileName")]
    pub file_name: String,
    #[serde(alias = "contentType")]
    pub content_type: String,
    pub base64: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub operation: TripoOperation,
    #[serde(default)]
    pub payload: Value,
}

fn api_error(status: StatusCode, code: &str, message: impl Into<String>) -> ApiError {
    HttpError::from((
        status,
        Json(json!({
            "error": message.into(),
            "code": code,
        })),
    ))
}

fn map_tripo_error(error: TripoError) -> ApiError {
    let status = match error {
        TripoError::Disabled | TripoError::NotConfigured | TripoError::InvalidConfig(_) => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        TripoError::InvalidRequest(_) | TripoError::InvalidModel(_) => StatusCode::BAD_REQUEST,
        TripoError::Timeout => StatusCode::GATEWAY_TIMEOUT,
        TripoError::Upstream { status, .. } if status == 401 || status == 403 => {
            StatusCode::BAD_GATEWAY
        }
        TripoError::Upstream { status, .. } if status == 429 => StatusCode::TOO_MANY_REQUESTS,
        TripoError::Upstream { .. } | TripoError::Transport(_) => StatusCode::BAD_GATEWAY,
        TripoError::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    api_error(status, "TRIPO_ERROR", error.to_string())
}

async fn client_from_state(state: &AppState) -> Result<TripoClient, ApiError> {
    let dynamic = state.dynamic_config.read().await;
    let config =
        crate::services::tripo::TripoRuntimeConfig::resolve(&dynamic).map_err(map_tripo_error)?;
    drop(dynamic);
    TripoClient::new(config).await.map_err(map_tripo_error)
}

fn task_response(task: TripoTask, assets: Vec<PersistedTripoAsset>) -> Value {
    json!({
        "task": task,
        "asset": assets.first(),
        "assets": assets,
    })
}

/// GET /api/tapp/3d/status
pub async fn status(
    State(state): State<AppState>,
    runtime_grant: RuntimeGrantContext,
) -> Result<Json<Value>, ApiError> {
    runtime_grant.require(TappPermission::ThreeDGenerate)?;
    let dynamic = state.dynamic_config.read().await;
    Ok(Json(json!({
        "enabled": is_enabled(&dynamic),
        "configured": is_configured(&dynamic),
        "capabilities": PUBLIC_CAPABILITIES,
    })))
}

/// POST /api/tapp/3d/files
pub async fn upload(
    State(state): State<AppState>,
    runtime_grant: RuntimeGrantContext,
    Json(request): Json<UploadRequest>,
) -> Result<Json<Value>, ApiError> {
    runtime_grant.require(TappPermission::ThreeDGenerate)?;
    let bytes = STANDARD.decode(request.base64.trim()).map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_UPLOAD",
            "base64 payload is invalid",
        )
    })?;
    if bytes.len() > TAPP_UPLOAD_MAX_BYTES {
        return Err(api_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "UPLOAD_TOO_LARGE",
            "Upload exceeds the 16 MiB TAPP limit",
        ));
    }
    validate_upload_type(&request.file_name, &request.content_type).map_err(map_tripo_error)?;
    let client = client_from_state(&state).await?;
    let file_token = client
        .upload(request.file_name, request.content_type, bytes)
        .await
        .map_err(map_tripo_error)?;
    Ok(Json(json!({ "file_token": file_token })))
}

/// POST /api/tapp/3d/tasks
pub async fn create_task(
    State(state): State<AppState>,
    runtime_grant: RuntimeGrantContext,
    Json(request): Json<CreateTaskRequest>,
) -> Result<Json<Value>, ApiError> {
    runtime_grant.require(TappPermission::ThreeDGenerate)?;
    let client = client_from_state(&state).await?;
    let payload = apply_web_defaults(request.operation, request.payload, client.config())
        .map_err(map_tripo_error)?;
    let task_id = client
        .create_task(request.operation, payload)
        .await
        .map_err(map_tripo_error)?;
    Ok(Json(json!({ "task_id": task_id })))
}

/// GET /api/tapp/3d/tasks/{task_id}
pub async fn get_task(
    State(state): State<AppState>,
    runtime_grant: RuntimeGrantContext,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    runtime_grant.require(TappPermission::ThreeDGenerate)?;
    let client = client_from_state(&state).await?;
    let task = client.query_task(&task_id).await.map_err(map_tripo_error)?;
    let assets = persist_task_models(&task, client.config().max_download_bytes)
        .await
        .map_err(map_tripo_error)?;
    Ok(Json(task_response(task, assets)))
}

/// POST /api/tapp/3d/tasks/{task_id}/await
pub async fn await_task(
    State(state): State<AppState>,
    runtime_grant: RuntimeGrantContext,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    runtime_grant.require(TappPermission::ThreeDGenerate)?;
    let client = client_from_state(&state).await?;
    let task = poll_until_terminal(&client, &task_id)
        .await
        .map_err(map_tripo_error)?;
    let assets = if task.status == "success" {
        persist_task_models(&task, client.config().max_download_bytes)
            .await
            .map_err(map_tripo_error)?
    } else {
        Vec::new()
    };
    Ok(Json(task_response(task, assets)))
}
