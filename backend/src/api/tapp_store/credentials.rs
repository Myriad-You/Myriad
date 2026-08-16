//! Authenticated management surface for installation-scoped Tapp credentials.
//!
//! This API is deliberately write-only: status responses contain binding
//! metadata but never the encrypted or plaintext credential value.

use super::{
    can_write_installation_settings, current_is_admin, optional_authenticated_user_id,
    validate_tapp_id, ApiResponse, TappStorageAccess,
};
use crate::api::tapp_runtime::common as tapp_common;
use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::models::entities::tapps;
use crate::services::tapp_credentials::{self, TappCredentialError, TappCredentialStatus};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use myriad_error::AppError;
use sea_orm::{DatabaseConnection, TransactionTrait};
use serde::Deserialize;

// Do not derive `Debug`: the request contains the plaintext secret and must
// never become printable through extractor/error instrumentation.
#[derive(Deserialize)]
pub(super) struct PutCredentialRequest {
    value: String,
}

fn credential_http_error(error: TappCredentialError) -> HttpError {
    let status = match error {
        TappCredentialError::InvalidDefinition(_) | TappCredentialError::InvalidValue => {
            StatusCode::BAD_REQUEST
        }
        TappCredentialError::Missing => StatusCode::NOT_FOUND,
        TappCredentialError::ReauthorizationRequired => StatusCode::CONFLICT,
        TappCredentialError::Encryption | TappCredentialError::Database => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    HttpError(AppError::new(status, error.code()).with_message(error.message()))
}

pub(super) async fn authorize_credential_management(
    db: &DatabaseConnection,
    claims: &Claims,
    tapp_id: &str,
) -> Result<tapps::Model, HttpError> {
    validate_tapp_id(tapp_id).map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let subject_id = optional_authenticated_user_id(Some(claims))
        .ok_or_else(|| HttpError(AppError::unauthorized("Unauthorized")))?;
    let tapp = tapp_common::resolve_accessible_tapp(db, subject_id, tapp_id).await?;
    let access = TappStorageAccess::from_owner_and_subject(tapp.user_id, subject_id);
    if !can_write_installation_settings(access, current_is_admin(claims, db).await) {
        return Err(HttpError(AppError::forbidden("Forbidden")));
    }
    Ok(tapp)
}

pub(super) async fn list_tapp_credential_statuses(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<Vec<TappCredentialStatus>>>, HttpError> {
    let tapp = authorize_credential_management(&db, &claims, &tapp_id).await?;
    let statuses = tapp_credentials::credential_statuses(&db, &tapp)
        .await
        .map_err(credential_http_error)?;
    Ok(Json(ApiResponse::success(statuses)))
}

pub(super) async fn put_tapp_credential(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, key)): Path<(String, String)>,
    Json(request): Json<PutCredentialRequest>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    let tapp = authorize_credential_management(&db, &claims, &tapp_id).await?;
    tapp_credentials::credential_definition(&tapp.manifest, &key).map_err(credential_http_error)?;
    let fingerprint = tapp_credentials::credential_binding_fingerprint(&tapp.manifest, &key)
        .map_err(credential_http_error)?;
    let txn = db
        .begin()
        .await
        .map_err(|_| credential_http_error(TappCredentialError::Database))?;
    tapp_credentials::put_credential(
        &txn,
        tapp.user_id,
        &tapp.tapp_id,
        &key,
        &request.value,
        &fingerprint,
    )
    .await
    .map_err(credential_http_error)?;
    txn.commit()
        .await
        .map_err(|_| credential_http_error(TappCredentialError::Database))?;
    tracing::info!(
        tapp_id = %tapp.tapp_id,
        owner_id = tapp.user_id,
        credential_key = %key,
        "Tapp credential configured"
    );
    Ok(Json(ApiResponse::success(())))
}

pub(super) async fn delete_tapp_credential(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, key)): Path<(String, String)>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    let tapp = authorize_credential_management(&db, &claims, &tapp_id).await?;
    tapp_credentials::credential_definition(&tapp.manifest, &key).map_err(credential_http_error)?;
    tapp_credentials::delete_credential(&db, tapp.user_id, &tapp.tapp_id, &key)
        .await
        .map_err(credential_http_error)?;
    tracing::info!(
        tapp_id = %tapp.tapp_id,
        owner_id = tapp.user_id,
        credential_key = %key,
        "Tapp credential removed"
    );
    Ok(Json(ApiResponse::success(())))
}
