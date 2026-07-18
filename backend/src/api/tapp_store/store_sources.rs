//! Store source administration endpoints.

use super::{require_current_admin, ApiResponse};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, DatabaseConnection, EntityTrait,
    QueryFilter, Set,
};
use serde::{Deserialize, Serialize};

use crate::middleware::auth::Claims;
use crate::models::entities::tapp_store_sources;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreSourceResponse {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub url: String,
    pub enabled: bool,
    pub official: bool,
    pub icon: Option<String>,
}

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

pub(super) async fn list_store_sources(
    State(db): State<DatabaseConnection>,
) -> Result<Json<ApiResponse<Vec<StoreSourceResponse>>>, StatusCode> {
    let sources = tapp_store_sources::Entity::find()
        .all(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let items = sources
        .into_iter()
        .map(|source| StoreSourceResponse {
            id: source.id,
            name: source.name,
            description: source.description,
            url: source.url,
            enabled: source.enabled,
            official: source.official,
            icon: source.icon,
        })
        .collect();

    Ok(Json(ApiResponse::success(items)))
}

pub(super) async fn add_store_source(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<AddStoreSourceRequest>,
) -> Result<Json<ApiResponse<StoreSourceResponse>>, StatusCode> {
    require_current_admin(&claims).await?;

    let existing = tapp_store_sources::Entity::find()
        .filter(tapp_store_sources::Column::Url.eq(&request.url))
        .one(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if existing.is_some() {
        return Err(StatusCode::CONFLICT);
    }

    let now = Utc::now().fixed_offset();
    let source = tapp_store_sources::ActiveModel {
        id: NotSet,
        name: Set(request.name),
        description: Set(request.description),
        url: Set(request.url),
        enabled: Set(request.enabled.unwrap_or(true)),
        official: Set(false),
        icon: Set(request.icon),
        created_at: Set(now),
        updated_at: Set(now),
    };
    let result = source
        .insert(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ApiResponse::success(StoreSourceResponse {
        id: result.id,
        name: result.name,
        description: result.description,
        url: result.url,
        enabled: result.enabled,
        official: result.official,
        icon: result.icon,
    })))
}

pub(super) async fn update_store_source(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(source_id): Path<i32>,
    Json(request): Json<UpdateStoreSourceRequest>,
) -> Result<Json<ApiResponse<StoreSourceResponse>>, StatusCode> {
    require_current_admin(&claims).await?;

    let source = tapp_store_sources::Entity::find_by_id(source_id)
        .one(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    if source.official && request.url.is_some() {
        return Err(StatusCode::FORBIDDEN);
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
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ApiResponse::success(StoreSourceResponse {
        id: result.id,
        name: result.name,
        description: result.description,
        url: result.url,
        enabled: result.enabled,
        official: result.official,
        icon: result.icon,
    })))
}

pub(super) async fn delete_store_source(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(source_id): Path<i32>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    require_current_admin(&claims).await?;

    let source = tapp_store_sources::Entity::find_by_id(source_id)
        .one(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    if source.official {
        return Err(StatusCode::FORBIDDEN);
    }

    tapp_store_sources::Entity::delete_by_id(source_id)
        .exec(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ApiResponse::success(())))
}
