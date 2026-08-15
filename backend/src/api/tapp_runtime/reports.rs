//! 报告 API
//!
//! Domain: [`crate::services::tapp_reports`] (platform catalog + custom CRUD).
//! This module owns grant/permission checks and Axum DTO mapping.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::services::permission_service::TappPermission;
use crate::services::tapp_reports::{self, ReportCatalogError, TappReportCrudError};

use super::common::{authorize_tapp_permission, parse_user_id};
use super::runtime_grant::RuntimeGrantContext;

fn catalog_http_error(err: ReportCatalogError) -> (StatusCode, Json<Value>) {
    let status =
        StatusCode::from_u16(err.status_hint()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    // Preserve prior bodies: list/fetch used plain "error" strings without codes.
    (
        status,
        Json(json!({
            "error": match &err {
                ReportCatalogError::Database => {
                    // get_runtime_report used "Failed to fetch report" (singular)
                    // for single-item paths; list used plural. Callers that need
                    // singular override below.
                    err.message()
                }
                other => other.message(),
            }
        })),
    )
}

/// GET /api/reports/list
pub async fn list_reports(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    tracing::info!("[TAPP] list_reports - User: {}", claims.username);

    let user_id = claims.sub.parse::<i32>().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    let reports = tapp_reports::list_user_platform_reports(&db, user_id)
        .await
        .map_err(catalog_http_error)?;
    let report_list: Vec<Value> = reports
        .iter()
        .map(tapp_reports::platform_report_list_item)
        .collect();

    Ok(Json(json!({ "reports": report_list })))
}

/// GET /api/tapp/report-catalog
pub async fn list_runtime_reports(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require(TappPermission::ReportRead)?;
    let user_id = parse_user_id(&claims)?;
    let reports = tapp_reports::list_user_platform_reports(&db, user_id)
        .await
        .map_err(catalog_http_error)?;
    Ok(Json(json!({
        "reports": reports.iter().map(tapp_reports::platform_report_payload).collect::<Vec<_>>()
    })))
}

/// GET /api/tapp/report-catalog/{report_id}
pub async fn get_runtime_report(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(report_id): Path<i32>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require(TappPermission::ReportRead)?;
    let user_id = parse_user_id(&claims)?;
    let report = tapp_reports::get_user_platform_report(&db, user_id, report_id)
        .await
        .map_err(|err| match err {
            ReportCatalogError::Database => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to fetch report" })),
            ),
            other => catalog_http_error(other),
        })?;
    Ok(Json(tapp_reports::platform_report_payload(&report)))
}

/// GET /api/tapp/report-catalog/platform/{platform}
pub async fn get_runtime_platform_report(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(platform): Path<String>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require(TappPermission::ReportRead)?;
    let user_id = parse_user_id(&claims)?;
    let report = tapp_reports::get_latest_user_platform_report(&db, user_id, &platform)
        .await
        .map_err(|err| match err {
            ReportCatalogError::Database => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to fetch report" })),
            ),
            ReportCatalogError::InvalidPlatform(msg) => {
                (StatusCode::BAD_REQUEST, Json(json!({ "error": msg })))
            }
            other => catalog_http_error(other),
        })?;
    Ok(Json(
        report
            .as_ref()
            .map(tapp_reports::platform_report_payload)
            .unwrap_or(Value::Null),
    ))
}

// Report CRUD

#[derive(Debug, Deserialize)]
pub struct CreateReportRequest {
    pub tapp_id: String,
    pub title: String,
    pub report_type: String,
    pub content: Value,
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateReportRequest {
    pub title: Option<String>,
    pub content: Option<Value>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct ListReportsQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

fn crud_http_error(err: TappReportCrudError) -> (StatusCode, Json<Value>) {
    let status =
        StatusCode::from_u16(err.status_hint()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(json!({ "error": err.message() })))
}

/// POST /api/tapp/reports
pub async fn create_report(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Json(req): Json<CreateReportRequest>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&req.tapp_id)?;
    runtime_grant.require(TappPermission::ReportWrite)?;
    let user_id = authorize_tapp_permission(
        &db,
        &claims,
        &req.tapp_id,
        TappPermission::ReportWrite,
        &dynamic_config,
    )
    .await?;

    tracing::info!(
        "[TAPP] create_report - User: {}, Tapp: {}, Type: {}",
        claims.username,
        req.tapp_id,
        req.report_type
    );

    let created = tapp_reports::create_tapp_report(
        &db,
        user_id,
        &req.tapp_id,
        &req.title,
        &req.report_type,
        req.content,
        req.metadata,
    )
    .await
    .map_err(crud_http_error)?;

    Ok(Json(json!({
        "success": true,
        "report": {
            "id": created.id,
            "title": created.title,
            "type": created.report_type,
            "createdAt": created.created_at
        }
    })))
}

/// GET /api/tapp/reports/tapp/{tapp_id}
pub async fn list_tapp_reports(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
    Query(query): Query<ListReportsQuery>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(TappPermission::ReportRead)?;
    let user_id = authorize_tapp_permission(
        &db,
        &claims,
        &tapp_id,
        TappPermission::ReportRead,
        &dynamic_config,
    )
    .await?;
    tracing::debug!(
        "[TAPP] list_tapp_reports - User: {}, Tapp: {}",
        claims.username,
        tapp_id
    );

    let (limit, offset) = tapp_reports::clamp_report_list_pagination(query.limit, query.offset);
    let reports = tapp_reports::list_tapp_reports(&db, user_id, &tapp_id, limit, offset)
        .await
        .map_err(crud_http_error)?;

    Ok(Json(json!({
        "success": true,
        "reports": reports,
        "pagination": { "limit": limit, "offset": offset }
    })))
}

/// GET /api/tapp/reports/{tapp_id}/{report_id}
pub async fn get_tapp_report(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, report_id)): Path<(String, String)>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(TappPermission::ReportRead)?;
    let user_id = authorize_tapp_permission(
        &db,
        &claims,
        &tapp_id,
        TappPermission::ReportRead,
        &dynamic_config,
    )
    .await?;
    tracing::debug!(
        "[TAPP] get_tapp_report - User: {}, Report: {}",
        claims.username,
        report_id
    );

    let report = tapp_reports::get_tapp_report(&db, user_id, &tapp_id, &report_id)
        .await
        .map_err(crud_http_error)?;
    Ok(Json(json!({ "success": true, "report": report })))
}

/// PUT /api/tapp/reports/{tapp_id}/{report_id}
pub async fn update_tapp_report(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, report_id)): Path<(String, String)>,
    Json(req): Json<UpdateReportRequest>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(TappPermission::ReportWrite)?;
    let user_id = authorize_tapp_permission(
        &db,
        &claims,
        &tapp_id,
        TappPermission::ReportWrite,
        &dynamic_config,
    )
    .await?;
    tracing::info!(
        "[TAPP] update_tapp_report - User: {}, Report: {}",
        claims.username,
        report_id
    );

    let report_data = tapp_reports::update_tapp_report(
        &db,
        user_id,
        &tapp_id,
        &report_id,
        req.title,
        req.content,
        req.metadata,
    )
    .await
    .map_err(crud_http_error)?;
    Ok(Json(json!({ "success": true, "report": report_data })))
}

/// DELETE /api/tapp/reports/{tapp_id}/{report_id}
pub async fn delete_tapp_report(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, report_id)): Path<(String, String)>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(TappPermission::ReportWrite)?;
    let user_id = authorize_tapp_permission(
        &db,
        &claims,
        &tapp_id,
        TappPermission::ReportWrite,
        &dynamic_config,
    )
    .await?;
    tracing::info!(
        "[TAPP] delete_tapp_report - User: {}, Report: {}",
        claims.username,
        report_id
    );

    tapp_reports::delete_tapp_report(&db, user_id, &tapp_id, &report_id)
        .await
        .map_err(crud_http_error)?;
    Ok(Json(json!({ "success": true, "deleted": report_id })))
}
