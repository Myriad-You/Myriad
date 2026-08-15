//! Store source administration endpoints.
//!
//! Domain projection and official-source policy live in
//! [`crate::services::tapp_store_sources`]. This module keeps Claims/DB and
//! maps policy errors to HTTP status codes.

use super::{require_current_admin, ApiResponse};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, DatabaseConnection, EntityTrait, QueryOrder, Set,
};
use serde::Deserialize;

use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::models::entities::tapp_store_sources;
use crate::services::tapp_store_sources::{
    default_store_source_enabled, may_change_store_source_url, may_delete_store_source,
    store_source_urls_conflict, validate_store_source_url, StoreSourcePolicyError, StoreSourceView,
};
use myriad_error::AppError;

/// Path-stable public response DTO (serde camelCase via domain).
pub type StoreSourceResponse = StoreSourceView;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddStoreSourceRequest {
    pub name: String,
    pub description: Option<String>,
    pub url: String,
    pub enabled: Option<bool>,
    pub icon: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStoreSourceRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub enabled: Option<bool>,
    pub icon: Option<String>,
}

fn policy_status(err: StoreSourcePolicyError) -> StatusCode {
    StatusCode::from_u16(err.status_hint()).unwrap_or(StatusCode::BAD_REQUEST)
}

fn model_to_view(source: tapp_store_sources::Model) -> StoreSourceView {
    StoreSourceView::new(
        source.id,
        source.name,
        source.description,
        source.url,
        source.enabled,
        source.official,
        source.icon,
    )
}

pub(super) async fn list_store_sources(
    State(db): State<DatabaseConnection>,
) -> Result<Json<ApiResponse<Vec<StoreSourceResponse>>>, HttpError> {
    let sources = tapp_store_sources::Entity::find()
        .order_by_asc(tapp_store_sources::Column::Id)
        .all(&db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;

    let items = sources.into_iter().map(model_to_view).collect();
    Ok(Json(ApiResponse::success(items)))
}

pub(super) async fn add_store_source(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<AddStoreSourceRequest>,
) -> Result<Json<ApiResponse<StoreSourceResponse>>, HttpError> {
    require_current_admin(&claims, &db).await?;
    validate_store_source_url(&request.url).map_err(policy_status)?;

    let existing = tapp_store_sources::Entity::find()
        .all(&db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;
    if existing
        .iter()
        .any(|source| store_source_urls_conflict(&source.url, &request.url))
    {
        return Err(HttpError(AppError::conflict("Conflict")));
    }

    let now = Utc::now().fixed_offset();
    let source = tapp_store_sources::ActiveModel {
        id: NotSet,
        name: Set(request.name),
        description: Set(request.description),
        url: Set(request.url),
        enabled: Set(default_store_source_enabled(request.enabled)),
        official: Set(false),
        icon: Set(request.icon),
        created_at: Set(now),
        updated_at: Set(now),
    };
    let result = source
        .insert(&db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;

    Ok(Json(ApiResponse::success(model_to_view(result))))
}

pub(super) async fn update_store_source(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(source_id): Path<i32>,
    Json(request): Json<UpdateStoreSourceRequest>,
) -> Result<Json<ApiResponse<StoreSourceResponse>>, HttpError> {
    require_current_admin(&claims, &db).await?;

    let source = tapp_store_sources::Entity::find_by_id(source_id)
        .one(&db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?
        .ok_or_else(|| HttpError(AppError::not_found("Not found")))?;
    may_change_store_source_url(source.official, request.url.as_deref()).map_err(policy_status)?;
    if let Some(url) = request.url.as_deref() {
        validate_store_source_url(url).map_err(policy_status)?;
        let others = tapp_store_sources::Entity::find()
            .all(&db)
            .await
            .map_err(|_| HttpError(AppError::internal("Database error")))?;
        if others
            .iter()
            .any(|other| other.id != source_id && store_source_urls_conflict(&other.url, url))
        {
            return Err(HttpError(AppError::conflict("Conflict")));
        }
    }

    let now = Utc::now().fixed_offset();
    let mut active: tapp_store_sources::ActiveModel = source.into();
    if let Some(name) = request.name {
        active.name = Set(name);
    }
    if let Some(description) = request.description {
        active.description = Set(Some(description));
    }
    if let Some(url) = request.url {
        active.url = Set(url);
    }
    if let Some(enabled) = request.enabled {
        active.enabled = Set(enabled);
    }
    if let Some(icon) = request.icon {
        active.icon = Set(Some(icon));
    }
    active.updated_at = Set(now);

    let result = active
        .update(&db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;

    Ok(Json(ApiResponse::success(model_to_view(result))))
}

pub(super) async fn delete_store_source(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(source_id): Path<i32>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    require_current_admin(&claims, &db).await?;

    let source = tapp_store_sources::Entity::find_by_id(source_id)
        .one(&db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?
        .ok_or_else(|| HttpError(AppError::not_found("Not found")))?;
    may_delete_store_source(source.official).map_err(policy_status)?;

    tapp_store_sources::Entity::delete_by_id(source_id)
        .exec(&db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;

    Ok(Json(ApiResponse::success(())))
}
