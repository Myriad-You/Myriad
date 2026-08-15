//! Owner controls for inbound `/tapi` pause and IP-fingerprint blocks.

use super::credentials::authorize_credential_management;
use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::services::tapp_inbound_guard::{self, InboundGuardError, InboundGuardStatus};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use sea_orm::DatabaseConnection;
use serde::Deserialize;

use super::types::ApiResponse;

fn guard_http_error(error: InboundGuardError) -> HttpError {
    let status = match error {
        InboundGuardError::NotFound => StatusCode::NOT_FOUND,
        InboundGuardError::Database => StatusCode::INTERNAL_SERVER_ERROR,
    };
    HttpError(myriad_error::AppError::new(status, error.code()).with_message(error.message()))
}

pub(super) async fn get_inbound_guard(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<InboundGuardStatus>>, HttpError> {
    let tapp = authorize_credential_management(&db, &claims, &tapp_id).await?;
    let status = tapp_inbound_guard::guard_status(&db, tapp.user_id, &tapp.tapp_id)
        .await
        .map_err(guard_http_error)?;
    Ok(Json(ApiResponse::success(status)))
}

pub(super) async fn pause_inbound_guard(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    let tapp = authorize_credential_management(&db, &claims, &tapp_id).await?;
    tapp_inbound_guard::pause_inbound(&db, tapp.user_id, &tapp.tapp_id)
        .await
        .map_err(guard_http_error)?;
    Ok(Json(ApiResponse::success(())))
}

pub(super) async fn resume_inbound_guard(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    let tapp = authorize_credential_management(&db, &claims, &tapp_id).await?;
    tapp_inbound_guard::resume_inbound(&db, tapp.user_id, &tapp.tapp_id)
        .await
        .map_err(guard_http_error)?;
    Ok(Json(ApiResponse::success(())))
}

#[derive(Deserialize)]
pub(super) struct BlockFingerprintRequest {
    fingerprint: String,
}

pub(super) async fn block_inbound_fingerprint(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
    Json(request): Json<BlockFingerprintRequest>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    let tapp = authorize_credential_management(&db, &claims, &tapp_id).await?;
    tapp_inbound_guard::extend_block(&db, tapp.user_id, &tapp.tapp_id, &request.fingerprint)
        .await
        .map_err(guard_http_error)?;
    Ok(Json(ApiResponse::success(())))
}

pub(super) async fn unblock_inbound_fingerprint(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, fingerprint)): Path<(String, String)>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    let tapp = authorize_credential_management(&db, &claims, &tapp_id).await?;
    tapp_inbound_guard::unblock_fingerprint(&db, tapp.user_id, &tapp.tapp_id, &fingerprint)
        .await
        .map_err(guard_http_error)?;
    Ok(Json(ApiResponse::success(())))
}
