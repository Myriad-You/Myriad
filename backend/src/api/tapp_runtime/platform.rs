//! 平台数据 API
//!
//! Item projection: [`crate::services::platform_items`].
//! Cache read/write/lock: [`crate::services::platform_cache`].

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::services::permission_service::TappPermission;
use crate::services::platform_cache::{
    append_filtered_items, build_tapp_written_item, PlatformCacheError,
};
use crate::services::platform_items::extract_platform_items;

use super::common::{authorize_tapp_permission, get_cached_platform_data, validate_platform_name};
use super::runtime_grant::RuntimeGrantContext;

#[derive(Debug, Deserialize)]
pub struct PlatformDataQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// GET /api/tapp/platform/{platform}/data
pub async fn get_platform_data(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(platform): Path<String>,
    Query(query): Query<PlatformDataQuery>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require(TappPermission::PlatformRead)?;
    tracing::debug!(
        "[TAPP] get_platform_data - User: {}, Platform: {}",
        claims.username,
        platform
    );

    let data = match get_cached_platform_data(&platform).await {
        Ok(d) => d,
        Err(error) => {
            let status = if error.starts_with("No cached ") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            return Err(HttpError::from((
                status,
                Json(json!({ "error": error, "platform": platform })),
            )));
        }
    };

    let items = extract_platform_items(&data, &platform);
    let total = items.len();
    let offset = query.offset.unwrap_or(0) as usize;
    let limit = (query.limit.unwrap_or(100) as usize).min(1000);
    let paged_items: Vec<_> = items.into_iter().skip(offset).take(limit).collect();

    Ok(Json(json!({
        "platform": platform,
        "items": paged_items,
        "total": total,
        "offset": offset,
        "limit": limit
    })))
}

/// GET /api/tapp/platform/{platform}/stats
pub async fn get_platform_stats(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(platform): Path<String>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require(TappPermission::PlatformRead)?;
    tracing::debug!(
        "[TAPP] get_platform_stats - User: {}, Platform: {}",
        claims.username,
        platform
    );

    let data = get_cached_platform_data(&platform)
        .await
        .unwrap_or(json!({ "items": [] }));
    let items = extract_platform_items(&data, &platform);
    let total = items.len();

    let mut type_distribution: HashMap<String, usize> = HashMap::new();
    for item in &items {
        if let Some(item_type) = item.get("type").and_then(|v| v.as_str()) {
            *type_distribution.entry(item_type.to_string()).or_default() += 1;
        }
    }

    Ok(Json(json!({
        "platform": platform,
        "total": total,
        "distribution": type_distribution,
        "recentActivity": []
    })))
}

/// GET /api/tapp/platform/{platform}/distribution/{dimension}
pub async fn get_platform_distribution(
    State(_db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((platform, dimension)): Path<(String, String)>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require(TappPermission::PlatformRead)?;
    tracing::debug!(
        "[TAPP] get_platform_distribution - User: {}, Platform: {}, Dimension: {}",
        claims.username,
        platform,
        dimension
    );

    let data = get_cached_platform_data(&platform)
        .await
        .unwrap_or(json!({ "items": [] }));
    let items = extract_platform_items(&data, &platform);

    let mut distribution: HashMap<String, usize> = HashMap::new();
    for item in &items {
        if let Some(value) = item.get(&dimension).and_then(|v| v.as_str()) {
            *distribution.entry(value.to_string()).or_default() += 1;
        }
    }

    let distribution_data: Vec<Value> = distribution
        .into_iter()
        .map(|(label, value)| json!({ "label": label, "value": value }))
        .collect();

    Ok(Json(json!({
        "dimension": dimension,
        "data": distribution_data
    })))
}

// Platform Write API

#[derive(Debug, Deserialize)]
pub struct AddPlatformItemRequest {
    pub tapp_id: String,
    pub item: NewPlatformItem,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NewPlatformItem {
    pub platform: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub title: String,
    pub cover: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub metadata: Option<Value>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PlatformItemResult {
    pub success: bool,
    #[serde(rename = "itemId")]
    pub item_id: String,
    pub source: String,
}

fn cache_http_error(err: PlatformCacheError) -> (StatusCode, Json<Value>) {
    let status =
        StatusCode::from_u16(err.status_hint()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(json!({ "error": err.message() })))
}

fn new_item_document(item_id: &str, tapp_id: &str, item: &NewPlatformItem) -> Value {
    build_tapp_written_item(
        item_id,
        tapp_id,
        &item.item_type,
        &item.title,
        item.cover.clone(),
        item.description.clone(),
        item.url.clone(),
        item.metadata.clone(),
        item.created_at.clone(),
    )
}

/// POST /api/tapp/platform/items
pub async fn add_platform_item(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Json(req): Json<AddPlatformItemRequest>,
) -> Result<Json<PlatformItemResult>, HttpError> {
    runtime_grant.require_tapp_id(&req.tapp_id)?;
    runtime_grant.require(TappPermission::PlatformWrite)?;
    authorize_tapp_permission(
        &db,
        &claims,
        &req.tapp_id,
        TappPermission::PlatformWrite,
        &dynamic_config,
    )
    .await?;

    validate_platform_name(&req.item.platform)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;

    tracing::info!(
        "[TAPP] add_platform_item - User: {}, Tapp: {}, Platform: {}",
        claims.username,
        req.tapp_id,
        req.item.platform
    );

    let item_id = format!("tapp_{}", uuid::Uuid::new_v4());
    let new_item = new_item_document(&item_id, &req.tapp_id, &req.item);
    append_filtered_items(&req.item.platform, vec![new_item])
        .await
        .map_err(cache_http_error)?;

    Ok(Json(PlatformItemResult {
        success: true,
        item_id,
        source: format!("tapp:{}", req.tapp_id),
    }))
}

#[derive(Debug, Deserialize)]
pub struct AddPlatformItemsBatchRequest {
    pub tapp_id: String,
    pub items: Vec<NewPlatformItem>,
}

/// POST /api/tapp/platform/items/batch
pub async fn add_platform_items_batch(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Json(req): Json<AddPlatformItemsBatchRequest>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require_tapp_id(&req.tapp_id)?;
    runtime_grant.require(TappPermission::PlatformWrite)?;
    authorize_tapp_permission(
        &db,
        &claims,
        &req.tapp_id,
        TappPermission::PlatformWrite,
        &dynamic_config,
    )
    .await?;

    for item in &req.items {
        validate_platform_name(&item.platform)
            .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;
    }

    let mut grouped_items: HashMap<String, Vec<&NewPlatformItem>> = HashMap::new();
    for item in &req.items {
        grouped_items
            .entry(item.platform.to_lowercase())
            .or_default()
            .push(item);
    }

    let mut results = Vec::new();
    for (platform, items) in grouped_items {
        let mut docs = Vec::with_capacity(items.len());
        let mut ids = Vec::with_capacity(items.len());
        for item in items {
            let item_id = format!("tapp_{}", uuid::Uuid::new_v4());
            docs.push(new_item_document(&item_id, &req.tapp_id, item));
            ids.push(item_id);
        }
        match append_filtered_items(&platform, docs).await {
            Ok(()) => {
                for item_id in ids {
                    results.push(json!({
                        "success": true,
                        "itemId": item_id,
                        "source": format!("tapp:{}", req.tapp_id)
                    }));
                }
            }
            Err(PlatformCacheError::InvalidStructure) => {
                for _ in ids {
                    results
                        .push(json!({ "success": false, "error": "Invalid cache file structure" }));
                }
            }
            Err(error) => {
                let message = error.message();
                for _ in ids {
                    results.push(json!({ "success": false, "error": message }));
                }
            }
        }
    }

    let success_count = results
        .iter()
        .filter(|r| r.get("success").and_then(|v| v.as_bool()).unwrap_or(false))
        .count();

    Ok(Json(json!({
        "success": success_count == results.len(),
        "results": results,
        "totalProcessed": results.len(),
        "successCount": success_count
    })))
}
