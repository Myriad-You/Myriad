//! One-shot, consent-gated data exchange between installed Tapps.
//!
//! Domain storage / authorize / consume lives in
//! [`crate::services::tapp_data_exchange`]. This module owns Axum handlers,
//! ownership resolution (stable access-denied mapping), and HTTP DTO mapping.

use crate::error::HttpError;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::services::tapp_data_exchange::{
    self, DataExchangeError, DataExchangeRuntime, ExchangeParty, PrepareExchangeInput,
};

use super::{RuntimeGrantContext, common::resolve_accessible_tapp};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareDataExchangeRequest {
    target_tapp_id: String,
    export_id: String,
    #[serde(default)]
    params: Value,
    purpose: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedDataExchangeResponse {
    request_id: String,
    requester_tapp_id: String,
    requester_name: String,
    provider_tapp_id: String,
    provider_owner_id: i32,
    provider_name: String,
    export_id: String,
    export_description: Option<String>,
    params: Value,
    purpose: String,
    max_bytes: usize,
    max_records: Option<usize>,
    expires_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataAccessGrantResponse {
    version: u8,
    grant_id: String,
    token: String,
    request_id: String,
    provider_tapp_id: String,
    provider_owner_id: i32,
    export_id: String,
    params: Value,
    purpose: String,
    request_hash: String,
    max_bytes: usize,
    max_records: Option<usize>,
    expires_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsumeDataExchangeRequest {
    grant_token: String,
    response: Value,
}

type ApiError = HttpError;

fn exchange_http_error(err: DataExchangeError) -> ApiError {
    let status =
        StatusCode::from_u16(err.status_hint()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    HttpError::from((
        status,
        Json(json!({
            "error": err.message(),
            "code": err.code(),
        })),
    ))
}

fn runtime_from_grant(grant: &RuntimeGrantContext) -> DataExchangeRuntime {
    DataExchangeRuntime {
        runtime_id: grant.runtime_id().to_string(),
        tapp_id: grant.tapp_id().to_string(),
        owner_id: grant.owner_id(),
        subject_id: grant.subject_id(),
    }
}

fn rfc3339(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp(timestamp, 0)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339()
}

/// POST /api/tapp/data-exchange/requests
pub async fn prepare_data_exchange(
    State(db): State<DatabaseConnection>,
    grant: RuntimeGrantContext,
    Json(request): Json<PrepareDataExchangeRequest>,
) -> Result<Json<PreparedDataExchangeResponse>, ApiError> {
    let runtime = runtime_from_grant(&grant);
    // Ownership HTTP mapping stays in the API adapter (stable access-denied codes).
    let requester = resolve_accessible_tapp(&db, grant.subject_id(), grant.tapp_id()).await?;
    let provider =
        resolve_accessible_tapp(&db, grant.subject_id(), &request.target_tapp_id).await?;

    let prepared = tapp_data_exchange::prepare_exchange(
        &db,
        &runtime,
        &ExchangeParty {
            tapp_id: requester.tapp_id.clone(),
            owner_id: requester.user_id,
            name: requester.name.clone(),
            manifest: requester.manifest,
        },
        &ExchangeParty {
            tapp_id: provider.tapp_id.clone(),
            owner_id: provider.user_id,
            name: provider.name.clone(),
            manifest: provider.manifest,
        },
        PrepareExchangeInput {
            target_tapp_id: request.target_tapp_id,
            export_id: request.export_id,
            params: request.params,
            purpose: request.purpose,
        },
    )
    .await
    .map_err(exchange_http_error)?;

    Ok(Json(PreparedDataExchangeResponse {
        request_id: prepared.request_id,
        requester_tapp_id: prepared.requester_tapp_id,
        requester_name: prepared.requester_name,
        provider_tapp_id: prepared.provider_tapp_id,
        provider_owner_id: prepared.provider_owner_id,
        provider_name: prepared.provider_name,
        export_id: prepared.export_id,
        export_description: prepared.export_description,
        params: prepared.params,
        purpose: prepared.purpose,
        max_bytes: prepared.max_bytes,
        max_records: prepared.max_records,
        expires_at: rfc3339(prepared.expires_at),
    }))
}

/// POST /api/tapp/data-exchange/requests/{request_id}/authorize
pub async fn authorize_data_exchange(
    State(db): State<DatabaseConnection>,
    grant: RuntimeGrantContext,
    Path(request_id): Path<String>,
) -> Result<Json<DataAccessGrantResponse>, ApiError> {
    let authorized =
        tapp_data_exchange::authorize_exchange(&db, &runtime_from_grant(&grant), &request_id)
            .await
            .map_err(exchange_http_error)?;

    Ok(Json(DataAccessGrantResponse {
        version: authorized.version,
        grant_id: authorized.grant_id,
        token: authorized.token,
        request_id: authorized.request_id,
        provider_tapp_id: authorized.provider_tapp_id,
        provider_owner_id: authorized.provider_owner_id,
        export_id: authorized.export_id,
        params: authorized.params,
        purpose: authorized.purpose,
        request_hash: authorized.request_hash,
        max_bytes: authorized.max_bytes,
        max_records: authorized.max_records,
        expires_at: rfc3339(authorized.expires_at),
    }))
}

/// DELETE /api/tapp/data-exchange/requests/{request_id}
pub async fn cancel_data_exchange(
    State(db): State<DatabaseConnection>,
    grant: RuntimeGrantContext,
    Path(request_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let cancelled =
        tapp_data_exchange::cancel_exchange(&db, &runtime_from_grant(&grant), &request_id)
            .await
            .map_err(exchange_http_error)?;
    Ok(Json(json!({ "success": true, "cancelled": cancelled })))
}

/// POST /api/tapp/data-exchange/consume
pub async fn consume_data_exchange(
    State(db): State<DatabaseConnection>,
    provider_grant: RuntimeGrantContext,
    Json(request): Json<ConsumeDataExchangeRequest>,
) -> Result<Json<Value>, ApiError> {
    let consumed = tapp_data_exchange::consume_exchange(
        &db,
        &runtime_from_grant(&provider_grant),
        &request.grant_token,
        request.response,
    )
    .await
    .map_err(exchange_http_error)?;

    Ok(Json(json!({
        "success": true,
        "data": consumed.data,
        "grantId": consumed.grant_id,
        "requestHash": consumed.request_hash
    })))
}

// Teardown helpers used by runtime_grant revoke paths — re-export services.
pub(super) async fn cancel_runtime_data_exchanges(
    subject_id: i32,
    tapp_id: &str,
    runtime_id: &str,
) {
    tapp_data_exchange::cancel_runtime_data_exchanges(subject_id, tapp_id, runtime_id).await;
}

pub(super) async fn cancel_tapp_data_exchanges(subject_id: i32, tapp_id: &str) {
    tapp_data_exchange::cancel_tapp_data_exchanges(subject_id, tapp_id).await;
}

pub(super) async fn cancel_all_tapp_data_exchanges(owner_id: i32, tapp_id: &str) {
    tapp_data_exchange::cancel_all_tapp_data_exchanges(owner_id, tapp_id).await;
}
