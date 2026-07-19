//! 报告 API

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::middleware::auth::Claims;
use crate::services::permission_service::TappPermission;

use super::common::{authorize_tapp_permission, parse_user_id, validate_platform_name};
use super::runtime_grant::RuntimeGrantContext;

/// GET /api/reports/list
pub async fn list_reports(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    tracing::info!("[TAPP] list_reports - User: {}", claims.username);

    use crate::models::entities::platform_reports;

    let user_id = claims.sub.parse::<i32>().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid user" })),
        )
    })?;

    let reports = platform_reports::Entity::find()
        .filter(platform_reports::Column::UserId.eq(user_id))
        .order_by_desc(platform_reports::Column::CreatedAt)
        .all(&db)
        .await
        .map_err(|e| {
            tracing::error!("[TAPP] Failed to fetch reports: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to fetch reports" })),
            )
        })?;

    let report_list: Vec<Value> = reports
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "platform": r.platform,
                "type": "platform",
                "createdAt": r.created_at.to_string(),
                "summary": r.report.get("summary").and_then(|v| v.as_str()).unwrap_or("")
            })
        })
        .collect();

    Ok(Json(json!({ "reports": report_list })))
}

fn platform_report_payload(report: &crate::models::entities::platform_reports::Model) -> Value {
    // `report.report` is the stored PlatformReport JSON (summary/insights/card_visuals/…).
    // Expose both nested `content` (legacy) and top-level card_visuals / cardVisuals so
    // host + Tapp clients can render home/catalog cards without field-mapping bugs.
    let card_visuals = report
        .report
        .get("card_visuals")
        .cloned()
        .filter(|v| !v.is_null())
        .unwrap_or_else(|| json!({}));
    let insights = report
        .report
        .get("insights")
        .cloned()
        .unwrap_or_else(|| json!([]));
    json!({
        "id": report.id,
        "platform": report.platform,
        "type": "platform",
        "summary": report.report.get("summary").and_then(Value::as_str).unwrap_or(""),
        "insights": insights,
        "content": report.report,
        "metadata": report.metadata,
        "card_visuals": card_visuals.clone(),
        "cardVisuals": card_visuals,
        "createdAt": report.created_at.to_string()
    })
}

/// GET /api/tapp/report-catalog
pub async fn list_runtime_reports(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require(TappPermission::ReportRead)?;
    let user_id = parse_user_id(&claims)?;
    use crate::models::entities::platform_reports;
    let reports = platform_reports::Entity::find()
        .filter(platform_reports::Column::UserId.eq(user_id))
        .order_by_desc(platform_reports::Column::CreatedAt)
        .all(&db)
        .await
        .map_err(|error| {
            tracing::error!(%error, "[TAPP] Failed to list runtime report catalog");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to fetch reports" })),
            )
        })?;
    Ok(Json(json!({
        "reports": reports.iter().map(platform_report_payload).collect::<Vec<_>>()
    })))
}

/// GET /api/tapp/report-catalog/{report_id}
pub async fn get_runtime_report(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(report_id): Path<i32>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require(TappPermission::ReportRead)?;
    let user_id = parse_user_id(&claims)?;
    use crate::models::entities::platform_reports;
    let report = platform_reports::Entity::find_by_id(report_id)
        .filter(platform_reports::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to fetch report" })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Report not found" })),
            )
        })?;
    Ok(Json(platform_report_payload(&report)))
}

/// GET /api/tapp/report-catalog/platform/{platform}
pub async fn get_runtime_platform_report(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(platform): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require(TappPermission::ReportRead)?;
    validate_platform_name(&platform)
        .map_err(|error| (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))))?;
    let user_id = parse_user_id(&claims)?;
    use crate::models::entities::platform_reports;
    let report = platform_reports::Entity::find()
        .filter(platform_reports::Column::UserId.eq(user_id))
        .filter(platform_reports::Column::Platform.eq(platform.to_lowercase()))
        .order_by_desc(platform_reports::Column::CreatedAt)
        .one(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to fetch report" })),
            )
        })?;
    Ok(Json(
        report
            .as_ref()
            .map(platform_report_payload)
            .unwrap_or(Value::Null),
    ))
}

// ============ Report CRUD ============

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

/// POST /api/tapp/reports
pub async fn create_report(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Json(req): Json<CreateReportRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require_tapp_id(&req.tapp_id)?;
    runtime_grant.require(TappPermission::ReportWrite)?;
    let user_id =
        authorize_tapp_permission(&db, &claims, &req.tapp_id, TappPermission::ReportWrite).await?;

    tracing::info!(
        "[TAPP] create_report - User: {}, Tapp: {}, Type: {}",
        claims.username,
        req.tapp_id,
        req.report_type
    );

    use crate::models::entities::tapp_storage;
    use sea_orm::{ActiveModelTrait, ActiveValue::NotSet, Set};

    let now = chrono::Utc::now().fixed_offset();
    let report_id = format!("report_{}_{}", req.report_type, uuid::Uuid::new_v4());
    let storage_key = format!("_report:{}", report_id);

    let report_data = json!({
        "id": report_id,
        "title": req.title,
        "type": req.report_type,
        "content": req.content,
        "metadata": req.metadata,
        "createdAt": now.to_rfc3339(),
        "updatedAt": now.to_rfc3339()
    });

    let storage = tapp_storage::ActiveModel {
        id: NotSet,
        tapp_id: Set(req.tapp_id.clone()),
        user_id: Set(user_id),
        key: Set(storage_key),
        value: Set(report_data.clone()),
        created_at: Set(now),
        updated_at: Set(now),
    };

    storage.insert(&db).await.map_err(|e| {
        tracing::error!("[TAPP] Failed to create report: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to create report" })),
        )
    })?;

    Ok(Json(json!({
        "success": true,
        "report": {
            "id": report_id,
            "title": req.title,
            "type": req.report_type,
            "createdAt": now.to_rfc3339()
        }
    })))
}

/// GET /api/tapp/reports/tapp/{tapp_id}
pub async fn list_tapp_reports(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
    Query(query): Query<ListReportsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(TappPermission::ReportRead)?;
    let user_id =
        authorize_tapp_permission(&db, &claims, &tapp_id, TappPermission::ReportRead).await?;
    tracing::debug!(
        "[TAPP] list_tapp_reports - User: {}, Tapp: {}",
        claims.username,
        tapp_id
    );

    use crate::models::entities::tapp_storage;

    let limit = query.limit.unwrap_or(50).min(100) as u64;
    let offset = query.offset.unwrap_or(0) as u64;

    let items = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.starts_with("_report:"))
        .order_by_desc(tapp_storage::Column::CreatedAt)
        .offset(offset)
        .limit(limit)
        .all(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?;

    let reports: Vec<Value> = items
        .into_iter()
        .filter_map(|item| {
            let data = item.value;
            Some(json!({
                "id": data.get("id")?,
                "title": data.get("title")?,
                "type": data.get("type")?,
                "createdAt": data.get("createdAt")?,
                "updatedAt": data.get("updatedAt")?
            }))
        })
        .collect();

    Ok(Json(json!({
        "success": true,
        "reports": reports,
        "pagination": { "limit": limit, "offset": offset }
    })))
}

/// GET /api/tapp/reports/{tapp_id}/{report_id}
pub async fn get_tapp_report(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, report_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(TappPermission::ReportRead)?;
    let user_id =
        authorize_tapp_permission(&db, &claims, &tapp_id, TappPermission::ReportRead).await?;
    tracing::debug!(
        "[TAPP] get_tapp_report - User: {}, Report: {}",
        claims.username,
        report_id
    );

    use crate::models::entities::tapp_storage;

    let storage_key = format!("_report:{}", report_id);

    let item = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .one(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Report not found" })),
            )
        })?;

    Ok(Json(json!({ "success": true, "report": item.value })))
}

/// PUT /api/tapp/reports/{tapp_id}/{report_id}
pub async fn update_tapp_report(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, report_id)): Path<(String, String)>,
    Json(req): Json<UpdateReportRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(TappPermission::ReportWrite)?;
    let user_id =
        authorize_tapp_permission(&db, &claims, &tapp_id, TappPermission::ReportWrite).await?;
    tracing::info!(
        "[TAPP] update_tapp_report - User: {}, Report: {}",
        claims.username,
        report_id
    );

    use crate::models::entities::tapp_storage;
    use sea_orm::{ActiveModelTrait, Set};

    let storage_key = format!("_report:{}", report_id);

    let item = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .one(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error" })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Report not found" })),
            )
        })?;

    let now = chrono::Utc::now().fixed_offset();
    let mut report_data = item.value.clone();

    if let Some(title) = req.title {
        report_data["title"] = json!(title);
    }
    if let Some(content) = req.content {
        report_data["content"] = content;
    }
    if let Some(metadata) = req.metadata {
        report_data["metadata"] = metadata;
    }
    report_data["updatedAt"] = json!(now.to_rfc3339());

    let mut active: tapp_storage::ActiveModel = item.into();
    active.value = Set(report_data.clone());
    active.updated_at = Set(now);

    active.update(&db).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to update report" })),
        )
    })?;

    Ok(Json(json!({ "success": true, "report": report_data })))
}

/// DELETE /api/tapp/reports/{tapp_id}/{report_id}
pub async fn delete_tapp_report(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, report_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    runtime_grant.require_tapp_id(&tapp_id)?;
    runtime_grant.require(TappPermission::ReportWrite)?;
    let user_id =
        authorize_tapp_permission(&db, &claims, &tapp_id, TappPermission::ReportWrite).await?;
    tracing::info!(
        "[TAPP] delete_tapp_report - User: {}, Report: {}",
        claims.username,
        report_id
    );

    use crate::models::entities::tapp_storage;

    let storage_key = format!("_report:{}", report_id);

    let result = tapp_storage::Entity::delete_many()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .exec(&db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to delete report" })),
            )
        })?;

    if result.rows_affected == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Report not found" })),
        ));
    }

    Ok(Json(json!({ "success": true, "deleted": report_id })))
}
