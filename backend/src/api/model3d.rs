//! Tripo 3D provider API.
//!
//! Task creation and uploads are admin-only at the router edge. Persisted,
//! content-addressed GLBs are public so guest home scenes can render them
//! without exposing the Tripo credential.

use axum::{
    body::Body,
    extract::{Multipart, Path, State},
    http::{header, HeaderValue, StatusCode},
    middleware::from_fn_with_state,
    response::Response,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    services::tripo::{
        apply_web_defaults, asset_path, is_configured, is_enabled, persist_task_models,
        poll_until_terminal, validate_upload_type, PersistedTripoAsset, TripoClient, TripoError,
        TripoOperation, TripoRuntimeConfig, TripoTask, DEFAULT_BASE_URL, DEFAULT_MODEL,
        PUBLIC_CAPABILITIES,
    },
    state::AppState,
};

type ApiResult<T> = Result<T, (StatusCode, Json<Value>)>;

pub fn create_routes(app_state: AppState) -> Router<AppState> {
    let admin = || from_fn_with_state(app_state.clone(), crate::middleware::auth::admin_middleware);
    Router::new()
        .route("/status", get(status).route_layer(admin()))
        .route(
            "/files",
            post(upload)
                .route_layer(axum::extract::DefaultBodyLimit::max(151 * 1024 * 1024))
                .route_layer(admin()),
        )
        .route("/tasks", post(create_task).route_layer(admin()))
        .route("/tasks/{task_id}", get(get_task).route_layer(admin()))
        .route(
            "/tasks/{task_id}/await",
            post(await_task).route_layer(admin()),
        )
        .route("/assets/{asset_id}", get(get_asset))
        .route("/assets/{asset_id}/metadata", get(get_asset_metadata))
}

#[derive(Debug, Serialize)]
pub struct TripoStatusResponse {
    pub enabled: bool,
    pub configured: bool,
    pub base_url: String,
    pub model: String,
    pub face_limit: u32,
    pub poll_interval_seconds: u64,
    pub task_timeout_seconds: u64,
    pub max_download_mb: usize,
    pub capabilities: &'static [&'static str],
}

#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub operation: TripoOperation,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Serialize)]
pub struct TaskCreatedResponse {
    pub task_id: String,
}

#[derive(Debug, Serialize)]
pub struct TaskResponse {
    #[serde(flatten)]
    pub task: TripoTask,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset: Option<PersistedTripoAsset>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<PersistedTripoAsset>,
}

pub async fn status(State(state): State<AppState>) -> Json<TripoStatusResponse> {
    let dynamic = state.dynamic_config.read().await;
    let enabled = is_enabled(&dynamic);
    let configured = is_configured(&dynamic);
    let base_url = env_string("TRIPO_BASE_URL")
        .unwrap_or_else(|| dynamic.tripo_base_url.clone())
        .trim_end_matches('/')
        .to_string();
    let model = env_string("TRIPO_MODEL").unwrap_or_else(|| dynamic.tripo_model.clone());
    let face_limit = env_i32("TRIPO_FACE_LIMIT")
        .unwrap_or(dynamic.tripo_face_limit)
        .clamp(50, 20_000) as u32;
    let poll_interval_seconds = env_i32("TRIPO_POLL_INTERVAL_SECONDS")
        .unwrap_or(dynamic.tripo_poll_interval_seconds)
        .clamp(2, 60) as u64;
    let task_timeout_seconds = env_i32("TRIPO_TASK_TIMEOUT_SECONDS")
        .unwrap_or(dynamic.tripo_task_timeout_seconds)
        .clamp(60, 3_600) as u64;
    let max_download_mb = env_i32("TRIPO_MAX_DOWNLOAD_MB")
        .unwrap_or(dynamic.tripo_max_download_mb)
        .clamp(1, 150) as usize;

    Json(TripoStatusResponse {
        enabled,
        configured,
        base_url: if base_url.is_empty() {
            DEFAULT_BASE_URL.to_string()
        } else {
            base_url
        },
        model: if model.trim().is_empty() {
            DEFAULT_MODEL.to_string()
        } else {
            model
        },
        face_limit,
        poll_interval_seconds,
        task_timeout_seconds,
        max_download_mb,
        capabilities: PUBLIC_CAPABILITIES,
    })
}

pub async fn upload(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> ApiResult<Json<Value>> {
    let client = client_from_state(&state).await?;
    let field = multipart
        .next_field()
        .await
        .map_err(|error| bad_request(error.to_string()))?
        .ok_or_else(|| bad_request("Multipart body must contain a file"))?;
    let file_name = field.file_name().unwrap_or("upload.bin").to_string();
    let content_type = field
        .content_type()
        .unwrap_or("application/octet-stream")
        .to_string();
    validate_upload_type(&file_name, &content_type).map_err(map_tripo_error)?;
    let bytes = field
        .bytes()
        .await
        .map_err(|error| bad_request(error.to_string()))?
        .to_vec();
    if multipart
        .next_field()
        .await
        .map_err(|error| bad_request(error.to_string()))?
        .is_some()
    {
        return Err(bad_request("Upload exactly one file per request"));
    }
    let file_token = client
        .upload(file_name, content_type, bytes)
        .await
        .map_err(map_tripo_error)?;
    Ok(Json(json!({ "file_token": file_token })))
}

pub async fn create_task(
    State(state): State<AppState>,
    Json(request): Json<CreateTaskRequest>,
) -> ApiResult<Json<TaskCreatedResponse>> {
    let client = client_from_state(&state).await?;
    let payload = apply_web_defaults(request.operation, request.payload, client.config())
        .map_err(map_tripo_error)?;
    let task_id = client
        .create_task(request.operation, payload)
        .await
        .map_err(map_tripo_error)?;
    Ok(Json(TaskCreatedResponse { task_id }))
}

pub async fn get_task(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> ApiResult<Json<TaskResponse>> {
    let client = client_from_state(&state).await?;
    let task = client.query_task(&task_id).await.map_err(map_tripo_error)?;
    // Tripo result URLs expire after about five minutes. Persist immediately
    // on the first successful status read; content addressing makes retries safe.
    let assets = persist_task_models(&task, client.config().max_download_bytes)
        .await
        .map_err(map_tripo_error)?;
    let asset = assets.first().cloned();
    Ok(Json(TaskResponse {
        task,
        asset,
        assets,
    }))
}

pub async fn await_task(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> ApiResult<Json<TaskResponse>> {
    let client = client_from_state(&state).await?;
    let task = poll_until_terminal(&client, &task_id)
        .await
        .map_err(map_tripo_error)?;
    match task.status.as_str() {
        "success" => {
            let assets = persist_task_models(&task, client.config().max_download_bytes)
                .await
                .map_err(map_tripo_error)?;
            let asset = assets.first().cloned();
            Ok(Json(TaskResponse {
                task,
                asset,
                assets,
            }))
        }
        _ => Ok(Json(TaskResponse {
            task,
            asset: None,
            assets: Vec::new(),
        })),
    }
}

pub async fn get_asset(Path(asset_id): Path<String>) -> ApiResult<Response> {
    serve_asset(&asset_id, false).await
}

pub async fn get_asset_metadata(Path(asset_id): Path<String>) -> ApiResult<Response> {
    serve_asset(&asset_id, true).await
}

async fn serve_asset(asset_id: &str, metadata: bool) -> ApiResult<Response> {
    let path = asset_path(asset_id, metadata).map_err(map_tripo_error)?;
    let bytes = tokio::fs::read(path).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "3D asset not found" })),
            )
        } else {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Could not read 3D asset" })),
            )
        }
    })?;
    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(if metadata {
            "application/json; charset=utf-8"
        } else {
            "model/gltf-binary"
        }),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    if let Ok(etag) = HeaderValue::from_str(&format!("\"{asset_id}\"")) {
        response.headers_mut().insert(header::ETAG, etag);
    }
    Ok(response)
}

async fn client_from_state(state: &AppState) -> ApiResult<TripoClient> {
    let dynamic = state.dynamic_config.read().await;
    let config = TripoRuntimeConfig::resolve(&dynamic).map_err(map_tripo_error)?;
    drop(dynamic);
    TripoClient::new(config).await.map_err(map_tripo_error)
}

fn map_tripo_error(error: TripoError) -> (StatusCode, Json<Value>) {
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
    (status, Json(json!({ "error": error.to_string() })))
}

fn bad_request(message: impl Into<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": message.into() })),
    )
}

fn env_string(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn env_i32(key: &str) -> Option<i32> {
    env_string(key)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime_config() -> TripoRuntimeConfig {
        TripoRuntimeConfig {
            enabled: true,
            api_key: "test".to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
            face_limit: 5_000,
            poll_interval: std::time::Duration::from_secs(2),
            task_timeout: std::time::Duration::from_secs(900),
            max_download_bytes: 64 * 1024 * 1024,
        }
    }

    #[test]
    fn generation_defaults_are_web_budgeted() {
        let payload = apply_web_defaults(
            TripoOperation::MultiviewToModel,
            json!({"inputs": [{"front": "file_a"}, {"back": "file_b"}]}),
            &runtime_config(),
        )
        .unwrap();
        assert_eq!(payload["model"], DEFAULT_MODEL);
        assert_eq!(payload["face_limit"], 5_000);
        assert_eq!(payload["pbr"], false);
        assert!(payload.get("compress").is_none());
        assert!(payload.get("texture_quality").is_none());
    }

    #[test]
    fn explicit_generation_options_are_not_overwritten() {
        let payload = apply_web_defaults(
            TripoOperation::ImageToModel,
            json!({"input": "file_a", "face_limit": 8_000, "pbr": true}),
            &runtime_config(),
        )
        .unwrap();
        assert_eq!(payload["face_limit"], 8_000);
        assert_eq!(payload["pbr"], true);
    }

    #[test]
    fn image_file_token_alias_is_normalized_and_explicit_compress_is_preserved() {
        let payload = apply_web_defaults(
            TripoOperation::ImageToModel,
            json!({"file_token": "file_a", "compress": "geometry"}),
            &runtime_config(),
        )
        .unwrap();
        assert_eq!(payload["input"], "file_a");
        assert!(payload.get("file_token").is_none());
        assert_eq!(payload["compress"], "geometry");

        assert!(apply_web_defaults(
            TripoOperation::ImageToModel,
            json!({"input": "file_a", "file_token": "file_b"}),
            &runtime_config(),
        )
        .is_err());
    }
}
