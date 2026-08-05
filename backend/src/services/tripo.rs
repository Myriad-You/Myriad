//! Tripo v3 client and persisted Web-GLB processing.
//!
//! Provider task URLs are short-lived. A successful model task therefore is
//! downloaded, validated and stored under DATA_DIR before it is exposed to the
//! Digital Life frontend.

use crate::config::DynamicConfig;
use futures::{stream, StreamExt, TryStreamExt};
use reqwest::{multipart, Client};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_BASE_URL: &str = "https://openapi.tripo3d.ai/v3";
pub const DEFAULT_MODEL: &str = "P1-20260311";
const GLB_MAGIC: &[u8; 4] = b"glTF";
const JSON_CHUNK_TYPE: u32 = 0x4e4f_534a;

#[derive(Debug, Clone)]
pub struct TripoRuntimeConfig {
    pub enabled: bool,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub face_limit: u32,
    pub poll_interval: Duration,
    pub task_timeout: Duration,
    pub max_download_bytes: usize,
}

impl TripoRuntimeConfig {
    pub fn resolve(config: &DynamicConfig) -> Result<Self, TripoError> {
        let enabled = env_bool("TRIPO_ENABLED").unwrap_or(config.tripo_enabled);
        if !enabled {
            return Err(TripoError::Disabled);
        }

        let api_key = config
            .tripo_api_key
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| std::env::var("TRIPO_API_KEY").ok())
            .filter(|value| !value.trim().is_empty())
            .ok_or(TripoError::NotConfigured)?;

        let base_url = env_string("TRIPO_BASE_URL")
            .unwrap_or_else(|| config.tripo_base_url.clone())
            .trim_end_matches('/')
            .to_string();
        validate_base_url(&base_url)?;

        let model = env_string("TRIPO_MODEL")
            .unwrap_or_else(|| config.tripo_model.clone())
            .trim()
            .to_string();
        if model.is_empty() {
            return Err(TripoError::InvalidConfig(
                "Tripo model cannot be empty".to_string(),
            ));
        }

        let face_limit = env_i32("TRIPO_FACE_LIMIT")
            .unwrap_or(config.tripo_face_limit)
            .clamp(50, 20_000) as u32;
        let poll_seconds = env_i32("TRIPO_POLL_INTERVAL_SECONDS")
            .unwrap_or(config.tripo_poll_interval_seconds)
            .clamp(2, 60) as u64;
        let timeout_seconds = env_i32("TRIPO_TASK_TIMEOUT_SECONDS")
            .unwrap_or(config.tripo_task_timeout_seconds)
            .clamp(60, 3_600) as u64;
        let max_download_mb = env_i32("TRIPO_MAX_DOWNLOAD_MB")
            .unwrap_or(config.tripo_max_download_mb)
            .clamp(1, 150) as usize;

        Ok(Self {
            enabled,
            api_key,
            base_url,
            model,
            face_limit,
            poll_interval: Duration::from_secs(poll_seconds),
            task_timeout: Duration::from_secs(timeout_seconds),
            max_download_bytes: max_download_mb * 1024 * 1024,
        })
    }
}

#[derive(Debug)]
pub enum TripoError {
    Disabled,
    NotConfigured,
    InvalidConfig(String),
    InvalidRequest(String),
    Upstream { status: u16, message: String },
    Transport(String),
    InvalidModel(String),
    Storage(String),
}

impl std::fmt::Display for TripoError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => write!(formatter, "Tripo 3D is disabled"),
            Self::NotConfigured => write!(formatter, "Tripo API key is not configured"),
            Self::InvalidConfig(message)
            | Self::InvalidRequest(message)
            | Self::Transport(message)
            | Self::InvalidModel(message)
            | Self::Storage(message) => formatter.write_str(message),
            Self::Upstream { status, message } => {
                write!(formatter, "Tripo API returned HTTP {status}: {message}")
            }
        }
    }
}

impl std::error::Error for TripoError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TripoTask {
    pub task_id: String,
    #[serde(rename = "type", default)]
    pub task_type: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub progress: u8,
    #[serde(default)]
    pub output: Value,
    #[serde(default)]
    pub credits_consumed: Option<u32>,
    #[serde(default)]
    pub error_code: Option<i64>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TripoEnvelope<T> {
    code: i64,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    suggestion: Option<String>,
    #[serde(default)]
    request_id: Option<String>,
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
struct TaskCreated {
    task_id: String,
}

#[derive(Debug, Deserialize)]
struct FileUploaded {
    file_token: String,
}

#[derive(Clone)]
pub struct TripoClient {
    config: TripoRuntimeConfig,
    client: Client,
}

impl TripoClient {
    pub async fn new(config: TripoRuntimeConfig) -> Result<Self, TripoError> {
        let proxy_config = crate::services::http_client::ProxyConfig::from_dynamic_config().await;
        let builder = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .user_agent("Myriad-Digital-Life/Tripo-v3");
        let client = crate::services::http_client::apply_proxy(builder, &proxy_config)
            .map_err(|error| TripoError::Transport(error.to_string()))?
            .build()
            .map_err(|error| TripoError::Transport(error.to_string()))?;
        Ok(Self { config, client })
    }

    pub fn config(&self) -> &TripoRuntimeConfig {
        &self.config
    }

    pub async fn upload(
        &self,
        file_name: String,
        content_type: String,
        bytes: Vec<u8>,
    ) -> Result<String, TripoError> {
        if bytes.is_empty() {
            return Err(TripoError::InvalidRequest("Upload is empty".to_string()));
        }
        let max_upload_bytes = if is_image_upload(&file_name, &content_type) {
            20 * 1024 * 1024
        } else {
            150 * 1024 * 1024
        };
        if bytes.len() > max_upload_bytes {
            return Err(TripoError::InvalidRequest(
                if max_upload_bytes == 20 * 1024 * 1024 {
                    "Image upload exceeds Tripo's 20 MB limit"
                } else {
                    "Model upload exceeds Tripo's 150 MB limit"
                }
                .to_string(),
            ));
        }
        let part = multipart::Part::bytes(bytes)
            .file_name(file_name)
            .mime_str(&content_type)
            .map_err(|error| TripoError::InvalidRequest(error.to_string()))?;
        let form = multipart::Form::new().part("file", part);
        let response = self
            .client
            .post(self.endpoint("files"))
            .bearer_auth(&self.config.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|error| TripoError::Transport(error.to_string()))?;
        let uploaded: FileUploaded = decode_response(response).await?;
        Ok(uploaded.file_token)
    }

    pub async fn create_task(
        &self,
        operation: TripoOperation,
        body: Value,
    ) -> Result<String, TripoError> {
        operation.validate_body(&body, &self.config)?;
        let response = self
            .client
            .post(self.endpoint(operation.path()))
            .bearer_auth(&self.config.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|error| TripoError::Transport(error.to_string()))?;
        let created: TaskCreated = decode_response(response).await?;
        validate_task_id(&created.task_id)?;
        Ok(created.task_id)
    }

    pub async fn query_task(&self, task_id: &str) -> Result<TripoTask, TripoError> {
        validate_task_id(task_id)?;
        let response = self
            .client
            .get(self.endpoint(&format!("tasks/{task_id}")))
            .bearer_auth(&self.config.api_key)
            .send()
            .await
            .map_err(|error| TripoError::Transport(error.to_string()))?;
        decode_response(response).await
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.config.base_url, path.trim_start_matches('/'))
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TripoOperation {
    ImageToModel,
    MultiviewToModel,
    RigCheck,
    Rig,
    Retarget,
}

impl TripoOperation {
    fn path(self) -> &'static str {
        match self {
            Self::ImageToModel => "generation/image-to-model",
            Self::MultiviewToModel => "generation/multiview-to-model",
            Self::RigCheck => "animations/rig-check",
            Self::Rig => "animations/rig",
            Self::Retarget => "animations/retarget",
        }
    }

    fn validate_body(self, body: &Value, config: &TripoRuntimeConfig) -> Result<(), TripoError> {
        let object = body.as_object().ok_or_else(|| {
            TripoError::InvalidRequest("Tripo request body must be an object".to_string())
        })?;
        match self {
            Self::ImageToModel => {
                require_nonempty_string(object.get("input"), "input")?;
            }
            Self::MultiviewToModel => validate_multiview_inputs(object.get("inputs"))?,
            Self::RigCheck | Self::Rig => {
                require_nonempty_string(object.get("input"), "input")?;
            }
            Self::Retarget => {
                let input = object
                    .get("input")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| {
                        TripoError::InvalidRequest("input must be a non-empty task id".to_string())
                    })?;
                validate_task_id(input)?;
                let one = object
                    .get("animation")
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty());
                let animations = object.get("animations").and_then(Value::as_array);
                let many = animations.is_some_and(|values| !values.is_empty());
                if one == many {
                    return Err(TripoError::InvalidRequest(
                        "Retarget requires exactly one of animation or animations".to_string(),
                    ));
                }
                if animations.is_some_and(|values| {
                    values.iter().any(|value| {
                        value
                            .as_str()
                            .is_none_or(|animation| animation.trim().is_empty())
                    })
                }) {
                    return Err(TripoError::InvalidRequest(
                        "animations must contain only non-empty preset identifiers".to_string(),
                    ));
                }
            }
        }
        if let Some(limit) = object.get("face_limit").and_then(Value::as_u64) {
            if !(50..=20_000).contains(&limit) {
                return Err(TripoError::InvalidRequest(
                    "face_limit must be between 50 and 20000".to_string(),
                ));
            }
        }
        // Defaults are injected by the API adapter; this check keeps callers
        // from accidentally using a high-poly model with the Web character path.
        if matches!(self, Self::ImageToModel | Self::MultiviewToModel) && config.face_limit > 20_000
        {
            return Err(TripoError::InvalidConfig(
                "Configured face limit exceeds Tripo P-series maximum".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlbMetrics {
    pub byte_length: usize,
    pub scene_count: usize,
    pub node_count: usize,
    pub mesh_count: usize,
    pub primitive_count: usize,
    pub material_count: usize,
    pub texture_count: usize,
    pub image_count: usize,
    pub skin_count: usize,
    pub joint_count: usize,
    pub animation_count: usize,
    pub accessor_count: usize,
    pub estimated_triangles: Option<u64>,
    pub web_budget: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedTripoAsset {
    pub asset_id: String,
    pub model_url: String,
    pub metadata_url: String,
    pub output_index: usize,
    pub metrics: GlbMetrics,
}

pub async fn persist_task_models(
    task: &TripoTask,
    max_download_bytes: usize,
) -> Result<Vec<PersistedTripoAsset>, TripoError> {
    if task.status != "success" {
        return Ok(Vec::new());
    }
    let model_urls = model_urls_from_output(&task.output);
    let mut assets = stream::iter(model_urls.into_iter().enumerate())
        .map(|(output_index, model_url)| async move {
            persist_model_url(task, &model_url, output_index, max_download_bytes).await
        })
        // Batch retarget URLs share the same short expiry window. Download a
        // small bounded group concurrently so later clips do not expire while
        // earlier GLBs are being validated and written.
        .buffer_unordered(4)
        .try_collect::<Vec<_>>()
        .await?;
    assets.sort_by_key(|asset| asset.output_index);
    Ok(assets)
}

async fn persist_model_url(
    task: &TripoTask,
    model_url: &str,
    output_index: usize,
    max_download_bytes: usize,
) -> Result<PersistedTripoAsset, TripoError> {
    let (safe_url, client) = crate::services::outbound_security::build_public_http_client(
        model_url,
        Duration::from_secs(120),
        Some("Myriad-Digital-Life/Model-Download"),
    )
    .await
    .map_err(TripoError::Transport)?;
    let response = client
        .get(safe_url)
        .send()
        .await
        .map_err(|error| TripoError::Transport(error.to_string()))?;
    if !response.status().is_success() {
        return Err(TripoError::Upstream {
            status: response.status().as_u16(),
            message: "Model download failed".to_string(),
        });
    }
    let bytes = crate::services::outbound_security::read_limited_body(response, max_download_bytes)
        .await
        .map_err(TripoError::Transport)?;
    let metrics = analyze_glb(&bytes)?;
    let asset_id = hex::encode(Sha256::digest(&bytes));
    let directory = model_storage_dir();
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|error| TripoError::Storage(error.to_string()))?;
    persist_once(&directory.join(format!("{asset_id}.glb")), &bytes).await?;
    let metadata = serde_json::to_vec_pretty(&json!({
        "asset_id": asset_id,
        "provider": "tripo",
        "provider_task_id": task.task_id,
        "task_type": task.task_type,
        "output_index": output_index,
        "credits_consumed": task.credits_consumed,
        "created_at": task.created_at,
        "completed_at": task.completed_at,
        "metrics": metrics,
    }))
    .map_err(|error| TripoError::Storage(error.to_string()))?;
    persist_once(&directory.join(format!("{asset_id}.json")), &metadata).await?;

    Ok(PersistedTripoAsset {
        model_url: format!("/api/digital-life/3d/assets/{asset_id}"),
        metadata_url: format!("/api/digital-life/3d/assets/{asset_id}/metadata"),
        asset_id,
        output_index,
        metrics,
    })
}

pub fn model_storage_dir() -> PathBuf {
    crate::services::data_paths::paths()
        .root
        .join("digital-life/models")
}

pub fn asset_path(asset_id: &str, metadata: bool) -> Result<PathBuf, TripoError> {
    validate_asset_id(asset_id)?;
    Ok(model_storage_dir().join(format!(
        "{asset_id}.{}",
        if metadata { "json" } else { "glb" }
    )))
}

pub fn analyze_glb(bytes: &[u8]) -> Result<GlbMetrics, TripoError> {
    if bytes.len() < 20 || &bytes[0..4] != GLB_MAGIC {
        return Err(TripoError::InvalidModel(
            "Downloaded file is not a GLB container".to_string(),
        ));
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let declared_length = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    if version != 2 {
        return Err(TripoError::InvalidModel(format!(
            "Unsupported GLB version {version}"
        )));
    }
    if declared_length != bytes.len() {
        return Err(TripoError::InvalidModel(format!(
            "GLB length mismatch: header={declared_length}, actual={}",
            bytes.len()
        )));
    }
    let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let chunk_type = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
    if chunk_type != JSON_CHUNK_TYPE || 20usize.saturating_add(json_length) > bytes.len() {
        return Err(TripoError::InvalidModel(
            "GLB JSON chunk is missing or truncated".to_string(),
        ));
    }
    let json_bytes = &bytes[20..20 + json_length];
    let json_end = json_bytes
        .iter()
        .rposition(|byte| !matches!(byte, b' ' | 0))
        .map(|index| index + 1)
        .unwrap_or(0);
    let document: Value = serde_json::from_slice(&json_bytes[..json_end])
        .map_err(|error| TripoError::InvalidModel(format!("Invalid GLB JSON: {error}")))?;

    let len = |key: &str| {
        document
            .get(key)
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
    };
    let primitive_count = document
        .get("meshes")
        .and_then(Value::as_array)
        .map(|meshes| {
            meshes
                .iter()
                .filter_map(|mesh| mesh.get("primitives").and_then(Value::as_array))
                .map(Vec::len)
                .sum()
        })
        .unwrap_or(0);
    let joint_count = document
        .get("skins")
        .and_then(Value::as_array)
        .map(|skins| {
            skins
                .iter()
                .filter_map(|skin| skin.get("joints").and_then(Value::as_array))
                .map(Vec::len)
                .sum()
        })
        .unwrap_or(0);
    let estimated_triangles = estimate_triangles(&document);
    let material_count = len("materials");
    let image_count = len("images");
    let skin_count = len("skins");
    let animation_count = len("animations");
    let mut warnings = Vec::new();
    if bytes.len() > 16 * 1024 * 1024 {
        warnings.push("Model exceeds the preferred 16 MB initial-download budget".to_string());
    }
    if primitive_count > 8 {
        warnings.push("More than 8 mesh primitives may increase draw calls".to_string());
    }
    if material_count > 4 {
        warnings.push("More than 4 materials may increase WebGL draw calls".to_string());
    }
    if image_count > 4 {
        warnings.push("More than 4 texture images may increase GPU memory".to_string());
    }
    if skin_count == 0 {
        warnings.push("Model has no skin; rigging is still required".to_string());
    }
    if animation_count == 0 {
        warnings.push("Model contains no animation clips".to_string());
    }
    if estimated_triangles.is_some_and(|triangles| triangles > 12_000) {
        warnings.push("Estimated triangles exceed the 12k Digital Life budget".to_string());
    }
    let web_budget = if warnings.iter().any(|warning| {
        warning.contains("16 MB") || warning.contains("12k") || warning.contains("draw calls")
    }) {
        "review"
    } else {
        "ready"
    }
    .to_string();

    Ok(GlbMetrics {
        byte_length: bytes.len(),
        scene_count: len("scenes"),
        node_count: len("nodes"),
        mesh_count: len("meshes"),
        primitive_count,
        material_count,
        texture_count: len("textures"),
        image_count,
        skin_count,
        joint_count,
        animation_count,
        accessor_count: len("accessors"),
        estimated_triangles,
        web_budget,
        warnings,
    })
}

fn estimate_triangles(document: &Value) -> Option<u64> {
    let accessors = document.get("accessors")?.as_array()?;
    let meshes = document.get("meshes")?.as_array()?;
    let mut total = 0u64;
    let mut found = false;
    for primitive in meshes
        .iter()
        .filter_map(|mesh| mesh.get("primitives")?.as_array())
        .flatten()
    {
        if primitive.get("mode").and_then(Value::as_u64).unwrap_or(4) != 4 {
            continue;
        }
        let accessor_index = primitive
            .get("indices")
            .and_then(Value::as_u64)
            .or_else(|| primitive.get("attributes")?.get("POSITION")?.as_u64())?
            as usize;
        let count = accessors.get(accessor_index)?.get("count")?.as_u64()?;
        total = total.saturating_add(count / 3);
        found = true;
    }
    found.then_some(total)
}

async fn decode_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, TripoError> {
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| TripoError::Transport(error.to_string()))?;
    let envelope: TripoEnvelope<T> =
        serde_json::from_slice(&bytes).map_err(|error| TripoError::Upstream {
            status: status.as_u16(),
            message: format!("Invalid JSON response: {error}"),
        })?;
    if !status.is_success() || envelope.code != 0 {
        let mut message = envelope
            .message
            .unwrap_or_else(|| "Tripo request failed".to_string());
        if let Some(suggestion) = envelope.suggestion {
            message.push_str(&format!(" ({suggestion})"));
        }
        if let Some(request_id) = envelope.request_id {
            message.push_str(&format!(" [request_id: {request_id}]"));
        }
        return Err(TripoError::Upstream {
            status: status.as_u16(),
            message,
        });
    }
    envelope.data.ok_or_else(|| TripoError::Upstream {
        status: status.as_u16(),
        message: "Tripo response did not include data".to_string(),
    })
}

async fn persist_once(path: &Path, bytes: &[u8]) -> Result<(), TripoError> {
    if tokio::fs::try_exists(path)
        .await
        .map_err(|error| TripoError::Storage(error.to_string()))?
    {
        return Ok(());
    }
    let temp = path.with_extension(format!(
        "{}.tmp-{}",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("bin"),
        uuid::Uuid::new_v4().simple()
    ));
    tokio::fs::write(&temp, bytes)
        .await
        .map_err(|error| TripoError::Storage(error.to_string()))?;
    if let Err(error) = tokio::fs::rename(&temp, path).await {
        if !tokio::fs::try_exists(path).await.unwrap_or(false) {
            let _ = tokio::fs::remove_file(&temp).await;
            return Err(TripoError::Storage(error.to_string()));
        }
        let _ = tokio::fs::remove_file(&temp).await;
    }
    Ok(())
}

fn model_urls_from_output(output: &Value) -> Vec<String> {
    let mut urls = Vec::new();
    if let Some(url) = output.get("model_url").and_then(Value::as_str) {
        if !url.trim().is_empty() {
            urls.push(url.to_string());
        }
    }
    if let Some(values) = output.get("model_urls").and_then(Value::as_array) {
        for value in values {
            let url = value.as_str().or_else(|| {
                value
                    .get("model_url")
                    .or_else(|| value.get("url"))
                    .and_then(Value::as_str)
            });
            if let Some(url) = url.filter(|url| !url.trim().is_empty()) {
                if !urls.iter().any(|existing| existing == url) {
                    urls.push(url.to_string());
                }
            }
        }
    }
    urls
}

fn validate_base_url(value: &str) -> Result<(), TripoError> {
    let url = url::Url::parse(value)
        .map_err(|error| TripoError::InvalidConfig(format!("Invalid Tripo base URL: {error}")))?;
    if url.scheme() != "https" && !cfg!(test) {
        return Err(TripoError::InvalidConfig(
            "Tripo base URL must use HTTPS".to_string(),
        ));
    }
    if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err(TripoError::InvalidConfig(
            "Tripo base URL must have a host and no credentials".to_string(),
        ));
    }
    Ok(())
}

fn validate_task_id(value: &str) -> Result<(), TripoError> {
    if value.starts_with("task_")
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        Ok(())
    } else {
        Err(TripoError::InvalidRequest(
            "Invalid Tripo task id".to_string(),
        ))
    }
}

fn validate_asset_id(value: &str) -> Result<(), TripoError> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(TripoError::InvalidRequest(
            "Invalid 3D asset id".to_string(),
        ))
    }
}

fn require_nonempty_string(value: Option<&Value>, field: &str) -> Result<(), TripoError> {
    if value
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        Ok(())
    } else {
        Err(TripoError::InvalidRequest(format!(
            "{field} must be a non-empty string"
        )))
    }
}

fn validate_multiview_inputs(value: Option<&Value>) -> Result<(), TripoError> {
    let inputs = value
        .and_then(Value::as_array)
        .ok_or_else(|| TripoError::InvalidRequest("inputs must be an array".to_string()))?;

    // Tripo can reuse a completed image-to-multiview/edit-multiview task.
    if inputs.len() == 1 {
        if let Some(object) = inputs[0].as_object() {
            if object.len() == 1 && object.contains_key("task_id") {
                return require_nonempty_string(object.get("task_id"), "task_id");
            }
        }
    }

    // Legacy positional form is exactly [front, left, back, right], with empty
    // strings allowed for missing non-front views.
    if inputs.iter().all(Value::is_string) {
        if inputs.len() != 4 {
            return Err(TripoError::InvalidRequest(
                "Legacy multiview inputs must contain four positional slots".to_string(),
            ));
        }
        require_nonempty_string(inputs.first(), "front")?;
        let supplied = inputs
            .iter()
            .filter_map(Value::as_str)
            .filter(|source| !source.trim().is_empty())
            .count();
        if supplied < 2 {
            return Err(TripoError::InvalidRequest(
                "Multiview generation requires at least two images".to_string(),
            ));
        }
        return Ok(());
    }

    if !(2..=4).contains(&inputs.len()) {
        return Err(TripoError::InvalidRequest(
            "Multiview generation requires 2 to 4 views".to_string(),
        ));
    }
    let mut has_front = false;
    let mut directions = std::collections::HashSet::new();
    for input in inputs {
        let object = input.as_object().ok_or_else(|| {
            TripoError::InvalidRequest(
                "Use view-key inputs such as {\"front\": \"file_token\"}".to_string(),
            )
        })?;
        if object.len() != 1 {
            return Err(TripoError::InvalidRequest(
                "Each multiview input must contain exactly one direction".to_string(),
            ));
        }
        let (direction, source) = object.iter().next().unwrap();
        if !matches!(direction.as_str(), "front" | "left" | "back" | "right") {
            return Err(TripoError::InvalidRequest(format!(
                "Unsupported multiview direction: {direction}"
            )));
        }
        if !directions.insert(direction.as_str()) {
            return Err(TripoError::InvalidRequest(format!(
                "Duplicate multiview direction: {direction}"
            )));
        }
        validate_multiview_source(source, direction)?;
        has_front |= direction == "front";
    }
    if !has_front {
        return Err(TripoError::InvalidRequest(
            "Multiview generation requires a front view".to_string(),
        ));
    }
    Ok(())
}

fn validate_multiview_source(source: &Value, field: &str) -> Result<(), TripoError> {
    if source.is_string() {
        return require_nonempty_string(Some(source), field);
    }
    let object = source.as_object().ok_or_else(|| {
        TripoError::InvalidRequest(format!(
            "{field} must be a URL, file token, or supported source object"
        ))
    })?;
    if object.len() != 1 {
        return Err(TripoError::InvalidRequest(format!(
            "{field} source object must contain exactly one source type"
        )));
    }
    if let Some(value) = object.get("url").or_else(|| object.get("file_token")) {
        return require_nonempty_string(Some(value), field);
    }
    if let Some(storage) = object.get("object").and_then(Value::as_object) {
        require_nonempty_string(storage.get("bucket"), "bucket")?;
        require_nonempty_string(storage.get("key"), "key")?;
        return Ok(());
    }
    Err(TripoError::InvalidRequest(format!(
        "Unsupported {field} source object"
    )))
}

fn is_image_upload(file_name: &str, content_type: &str) -> bool {
    let extension = file_name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    content_type.starts_with("image/") || matches!(extension.as_str(), "jpg" | "jpeg" | "png")
}

fn env_string(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn env_i32(key: &str) -> Option<i32> {
    env_string(key)?.parse().ok()
}

fn env_bool(key: &str) -> Option<bool> {
    env_string(key).map(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::Json,
        http::{header::AUTHORIZATION, header::CONTENT_TYPE, HeaderMap},
        routing::{get, post},
        Router,
    };

    fn test_runtime_config() -> TripoRuntimeConfig {
        TripoRuntimeConfig {
            enabled: true,
            api_key: "test".to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
            face_limit: 5_000,
            poll_interval: Duration::from_secs(2),
            task_timeout: Duration::from_secs(900),
            max_download_bytes: 64 * 1024 * 1024,
        }
    }

    async fn upload_fixture(headers: HeaderMap) -> Json<Value> {
        assert_eq!(
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer test")
        );
        assert!(headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("multipart/form-data; boundary=")));
        Json(json!({
            "code": 0,
            "data": { "file_token": "file_front" }
        }))
    }

    async fn create_fixture(headers: HeaderMap, Json(body): Json<Value>) -> Json<Value> {
        assert_eq!(
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer test")
        );
        assert_eq!(body["input"], "file_front");
        assert_eq!(body["model"], DEFAULT_MODEL);
        Json(json!({
            "code": 0,
            "data": { "task_id": "task_contract" }
        }))
    }

    async fn task_fixture(headers: HeaderMap) -> Json<Value> {
        assert_eq!(
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer test")
        );
        Json(json!({
            "code": 0,
            "data": {
                "task_id": "task_contract",
                "type": "image_to_model",
                "status": "success",
                "progress": 100,
                "output": { "model_url": "https://cdn.example/model.glb" },
                "credits_consumed": 12
            }
        }))
    }

    #[tokio::test]
    async fn calls_v3_upload_generation_and_task_contract() {
        let app = Router::new()
            .route("/v3/files", post(upload_fixture))
            .route("/v3/generation/image-to-model", post(create_fixture))
            .route("/v3/tasks/task_contract", get(task_fixture));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let mut config = test_runtime_config();
        config.base_url = format!("http://{address}/v3");
        let client = TripoClient {
            config,
            client: Client::builder().build().unwrap(),
        };

        let token = client
            .upload(
                "front.png".to_string(),
                "image/png".to_string(),
                b"fixture".to_vec(),
            )
            .await
            .unwrap();
        assert_eq!(token, "file_front");

        let task_id = client
            .create_task(
                TripoOperation::ImageToModel,
                json!({ "input": token, "model": DEFAULT_MODEL }),
            )
            .await
            .unwrap();
        assert_eq!(task_id, "task_contract");

        let task = client.query_task(&task_id).await.unwrap();
        assert_eq!(task.status, "success");
        assert_eq!(task.credits_consumed, Some(12));
        assert_eq!(
            model_urls_from_output(&task.output),
            vec!["https://cdn.example/model.glb"]
        );

        server.abort();
    }

    fn minimal_glb(document: Value) -> Vec<u8> {
        let mut json = serde_json::to_vec(&document).unwrap();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let total = 12 + 8 + json.len();
        let mut bytes = Vec::with_capacity(total);
        bytes.extend_from_slice(GLB_MAGIC);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&(total as u32).to_le_bytes());
        bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&JSON_CHUNK_TYPE.to_le_bytes());
        bytes.extend_from_slice(&json);
        bytes
    }

    #[test]
    fn analyzes_web_character_glb() {
        let bytes = minimal_glb(json!({
            "scenes": [{}],
            "nodes": [{}, {}],
            "meshes": [{"primitives": [{"indices": 0}]}],
            "accessors": [{"count": 15000}],
            "materials": [{}],
            "textures": [{}],
            "images": [{}],
            "skins": [{"joints": [0, 1]}],
            "animations": [{}]
        }));
        let metrics = analyze_glb(&bytes).unwrap();
        assert_eq!(metrics.estimated_triangles, Some(5_000));
        assert_eq!(metrics.joint_count, 2);
        assert_eq!(metrics.animation_count, 1);
        assert_eq!(metrics.web_budget, "ready");
    }

    #[test]
    fn rejects_non_glb_download() {
        let error = analyze_glb(b"<html>expired</html>").unwrap_err();
        assert!(error.to_string().contains("not a GLB"));
    }

    #[test]
    fn multiview_requires_front_and_unique_direction_objects() {
        let missing_front = json!([{"left": "file_a"}, {"back": "file_b"}]);
        assert!(validate_multiview_inputs(Some(&missing_front)).is_err());
        let valid = json!([{"front": "file_a"}, {"back": "file_b"}]);
        validate_multiview_inputs(Some(&valid)).unwrap();
        let duplicate = json!([{"front": "file_a"}, {"front": "file_b"}]);
        assert!(validate_multiview_inputs(Some(&duplicate)).is_err());
    }

    #[test]
    fn multiview_accepts_all_official_input_forms() {
        let nested = json!([
            {"front": {"file_token": "file_a"}},
            {"back": {"url": "https://example.com/back.png"}},
            {"right": {"object": {"bucket": "assets", "key": "right.png"}}}
        ]);
        validate_multiview_inputs(Some(&nested)).unwrap();

        let positional = json!(["file_front", "", "file_back", ""]);
        validate_multiview_inputs(Some(&positional)).unwrap();

        let task = json!([{"task_id": "task_multiview_source"}]);
        validate_multiview_inputs(Some(&task)).unwrap();
    }

    #[test]
    fn rejects_path_like_task_and_asset_ids() {
        assert!(validate_task_id("../../secret").is_err());
        assert!(validate_asset_id(&"a".repeat(64)).is_ok());
        assert!(validate_asset_id("../asset").is_err());
    }

    #[test]
    fn retarget_requires_a_task_and_nonempty_presets() {
        let config = test_runtime_config();
        assert!(TripoOperation::Retarget
            .validate_body(
                &json!({"input": "file_model", "animation": "preset:biped:walk"}),
                &config,
            )
            .is_err());
        assert!(TripoOperation::Retarget
            .validate_body(
                &json!({"input": "task_rigged", "animations": ["preset:biped:idle", ""]}),
                &config,
            )
            .is_err());
        TripoOperation::Retarget
            .validate_body(
                &json!({"input": "task_rigged", "animations": ["preset:biped:idle", "preset:biped:walk"]}),
                &config,
            )
            .unwrap();
    }

    #[test]
    fn collects_every_batch_model_url_in_order_without_duplicates() {
        let output = json!({
            "model_url": "https://cdn.example/primary.glb",
            "model_urls": [
                "https://cdn.example/primary.glb",
                "https://cdn.example/idle.glb",
                {"model_url": "https://cdn.example/walk.glb"},
                {"url": "https://cdn.example/run.glb"}
            ]
        });
        assert_eq!(
            model_urls_from_output(&output),
            vec![
                "https://cdn.example/primary.glb",
                "https://cdn.example/idle.glb",
                "https://cdn.example/walk.glb",
                "https://cdn.example/run.glb"
            ]
        );
    }
}
