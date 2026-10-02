//! See-through 远程拆层：配置令牌，把当前立绘拆成 PSD 交给前端导入。

use axum::{
    Extension, Json,
    body::Body,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    ApiError, ApiResult, bad_request, internal_error,
    master::{MasterSlot, require_master_match, valid_generation_fingerprint},
    require_merope_enabled, require_owner,
};
use crate::{
    middleware::auth::Claims,
    services::{image_generation, see_through},
};

fn see_through_error(error: see_through::SeeThroughError) -> ApiError {
    use see_through::SeeThroughError;

    let (status, code, message) = match &error {
        SeeThroughError::NotConfigured => (
            StatusCode::PRECONDITION_REQUIRED,
            "see_through_token_required",
            "Configure a Hugging Face API token before using See-through",
        ),
        SeeThroughError::Busy => (
            StatusCode::CONFLICT,
            "see_through_busy",
            "A See-through decomposition is already running",
        ),
        SeeThroughError::InvalidInput(message) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": message, "code": "see_through_invalid_input" })),
            );
        }
        SeeThroughError::Authentication => (
            StatusCode::BAD_GATEWAY,
            "see_through_auth_failed",
            "Hugging Face rejected the configured API token",
        ),
        SeeThroughError::Quota | SeeThroughError::Rejected => (
            StatusCode::SERVICE_UNAVAILABLE,
            "see_through_quota_unavailable",
            "See-through ZeroGPU is unavailable; check the Hugging Face token and quota",
        ),
        SeeThroughError::Timeout => (
            StatusCode::GATEWAY_TIMEOUT,
            "see_through_timeout",
            "See-through inference timed out",
        ),
        SeeThroughError::Transport(_)
        | SeeThroughError::Upstream { .. }
        | SeeThroughError::InvalidOutput(_) => (
            StatusCode::BAD_GATEWAY,
            "see_through_upstream_failed",
            "See-through returned an invalid or unavailable result",
        ),
    };
    tracing::warn!(%error, code, "remote See-through request failed");
    (status, Json(json!({ "error": message, "code": code })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateSeeThroughTokenRequest {
    token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeeThroughDecomposeRequest {
    source_master_asset_id: String,
    #[serde(default)]
    source_generation_fingerprint: Option<String>,
    #[serde(default)]
    resolution: Option<u16>,
    #[serde(default)]
    seed: Option<u16>,
    #[serde(default)]
    split_arms_and_legs: Option<bool>,
}

pub async fn get_see_through_status(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let token_configured = {
        let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        see_through::configured_hf_token(&config).is_some()
    };
    Ok(Json(json!({
        "provider": see_through::SPACE_NAME,
        "tokenConfigured": token_configured,
        "defaultResolution": see_through::DecomposeOptions::default().resolution,
        "splitArmsAndLegs": true,
    })))
}

pub async fn update_see_through_token(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<UpdateSeeThroughTokenRequest>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let token = see_through::validate_hf_token(&payload.token).map_err(see_through_error)?;
    crate::services::config_service::ConfigService::new(db)
        .update_config("see_through_hf_token", json!(token.clone()))
        .await
        .map_err(internal_error)?;
    crate::GLOBAL_DYNAMIC_CONFIG
        .write()
        .await
        .see_through_hf_token = Some(token);
    Ok(Json(json!({
        "provider": see_through::SPACE_NAME,
        "tokenConfigured": true,
    })))
}

pub async fn decompose_with_see_through(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<SeeThroughDecomposeRequest>,
) -> ApiResult<Response> {
    decompose_master(&db, &claims, MasterSlot::WornBust, payload).await
}

/// Splits a master portrait into a PSD for the importer.
pub(super) async fn decompose_master(
    db: &DatabaseConnection,
    claims: &Claims,
    slot: MasterSlot<'_>,
    payload: SeeThroughDecomposeRequest,
) -> ApiResult<Response> {
    require_merope_enabled().await?;
    require_owner(claims, db).await?;
    let source_generation_fingerprint = payload
        .source_generation_fingerprint
        .as_deref()
        .map(str::to_ascii_lowercase);
    if source_generation_fingerprint
        .as_deref()
        .is_some_and(|value| !valid_generation_fingerprint(value))
    {
        return Err(bad_request(
            "Source generation fingerprint must be a SHA-256 hex digest",
        ));
    }
    let master = require_master_match(
        db,
        slot,
        &payload.source_master_asset_id,
        source_generation_fingerprint.as_deref(),
    )
    .await?;
    let token = {
        let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        see_through::configured_hf_token(&config)
    }
    .ok_or_else(|| see_through_error(see_through::SeeThroughError::NotConfigured))?;
    // Uploaded portraits live in the media store, not the image cache.
    let image = image_generation::load_local_reference(&master.asset_id)
        .await
        .map_err(|_| bad_request("The current master portrait is not available"))?;
    let defaults = see_through::DecomposeOptions::default();
    let options = see_through::DecomposeOptions {
        resolution: payload.resolution.unwrap_or(defaults.resolution),
        seed: payload.seed.unwrap_or(defaults.seed),
        split_arms_and_legs: payload
            .split_arms_and_legs
            .unwrap_or(defaults.split_arms_and_legs),
    }
    .validate()
    .map_err(see_through_error)?;
    let client = see_through::SeeThroughClient::new(token)
        .await
        .map_err(see_through_error)?;
    let output = client
        .decompose(image.bytes, &image.media_type, options)
        .await
        .map_err(see_through_error)?;

    // Inference can take minutes. Never hand a result back as current if the
    // master changed while the remote job was running.
    require_master_match(
        db,
        slot,
        &payload.source_master_asset_id,
        source_generation_fingerprint.as_deref(),
    )
    .await?;

    let event_header = HeaderValue::from_str(&output.event_id).map_err(internal_error)?;
    let filename = format!(
        "see-through-{}.psd",
        output.event_id.chars().take(12).collect::<String>()
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("image/vnd.adobe.photoshop"),
        )
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
                .map_err(internal_error)?,
        )
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, private"),
        )
        .header("x-see-through-event-id", event_header)
        .body(Body::from(output.psd))
        .map_err(internal_error)
}
