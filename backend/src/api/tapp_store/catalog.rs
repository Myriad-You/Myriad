//! Role-aware Tapp catalog and detail queries.
//!
//! Projection lives in [`crate::services::tapp_catalog`]. This module resolves
//! identity, loads install rows, wraps list/detail DTOs, and writes
//! `tapps.visibility` (`set_tapp_visibility`).

use super::{
    current_is_admin, find_admin_user_id, find_visible_tapp, optional_authenticated_user_id,
    require_current_admin, ApiResponse, TappDetail, TappListItem,
};
use axum::{
    extract::{Query, State},
    Extension, Json,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
};
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::DynamicConfig;
use crate::error::HttpError;
use crate::middleware::auth::{Claims, OptionalClaims};
use crate::models::entities::tapps;
use crate::services::tapp_catalog::{catalog_install_flags, tapp_list_item_from_model};
use crate::services::tapp_context::role_for_optional_subject;
use crate::services::tapp_ownership::{parse_tapp_visibility, public_install_visible_to_viewer};
use myriad_error::AppError;

// Path-stable for parent module / manifest_tests (`super::tapp_detail_from_model`).
pub(super) use crate::services::tapp_catalog::tapp_detail_from_model;

/// Catalog list scope:
/// - omit / `all`: personal installs first, then site-owner public (dedupe by id)
/// - `mine`: only the durable subject's personal installs
/// - `site`: only site-owner public installs (no personal overlay / no id collision drop)
#[derive(Debug, Default, Deserialize)]
pub(super) struct CatalogListQuery {
    #[serde(default)]
    scope: Option<String>,
}

fn parse_catalog_scope(raw: Option<&str>) -> &'static str {
    match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("mine") => "mine",
        Some("site") => "site",
        _ => "all",
    }
}

pub(super) async fn list_tapps(
    State(db): State<DatabaseConnection>,
    Extension(OptionalClaims(claims)): Extension<OptionalClaims>,
    Query(query): Query<CatalogListQuery>,
) -> Result<Json<ApiResponse<Vec<TappListItem>>>, HttpError> {
    let user_id = optional_authenticated_user_id(claims.as_ref());
    let is_admin = match claims.as_ref() {
        Some(claims) => current_is_admin(claims, &db).await,
        None => false,
    };
    let scope = parse_catalog_scope(query.scope.as_deref());
    let admin_id = find_admin_user_id(&db).await?;
    let mut items = Vec::new();
    let mut seen_tapp_ids = HashSet::new();

    let include_mine = scope == "all" || scope == "mine";
    let include_site = scope == "all" || scope == "site";

    if include_mine {
        if let Some(user_id) = user_id {
            if Some(user_id) != admin_id {
                let user_tapps = tapps::Entity::find()
                    .filter(tapps::Column::UserId.eq(user_id))
                    .all(&db)
                    .await
                    .map_err(|_| HttpError(AppError::internal("Database error")))?;
                for tapp in user_tapps {
                    if !seen_tapp_ids.insert(tapp.tapp_id.clone()) {
                        continue;
                    }
                    let (is_temporary, is_admin_tapp) = catalog_install_flags(false);
                    items.push(tapp_list_item_from_model(tapp, is_temporary, is_admin_tapp));
                }
            }
        }
    }

    if include_site {
        let admin_tapps = if let Some(admin_id) = admin_id {
            tapps::Entity::find()
                .filter(tapps::Column::UserId.eq(admin_id))
                .all(&db)
                .await
                .map_err(|_| HttpError(AppError::internal("Database error")))?
        } else {
            Vec::new()
        };
        for tapp in admin_tapps {
            if !public_install_visible_to_viewer(&tapp.visibility, is_admin) {
                continue;
            }
            // scope=all: skip ids already covered by personal installs
            // scope=site: only site rows were loaded — still dedupe within site list
            if !seen_tapp_ids.insert(tapp.tapp_id.clone()) {
                continue;
            }
            let (is_temporary, is_admin_tapp) = catalog_install_flags(true);
            items.push(tapp_list_item_from_model(tapp, is_temporary, is_admin_tapp));
        }
    }
    Ok(Json(ApiResponse::success(items)))
}

pub(super) async fn list_tapp_details(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<Arc<RwLock<DynamicConfig>>>,
    Extension(OptionalClaims(claims)): Extension<OptionalClaims>,
    Query(query): Query<CatalogListQuery>,
) -> Result<Json<ApiResponse<Vec<TappDetail>>>, HttpError> {
    let user_id = optional_authenticated_user_id(claims.as_ref());
    let is_admin = match claims.as_ref() {
        Some(claims) => current_is_admin(claims, &db).await,
        None => false,
    };
    let scope = parse_catalog_scope(query.scope.as_deref());
    let role = role_for_optional_subject(user_id, is_admin);
    let admin_id = find_admin_user_id(&db).await?;

    let include_mine = scope == "all" || scope == "mine";
    let include_site = scope == "all" || scope == "site";

    let admin_tapps = if include_site {
        if let Some(admin_id) = admin_id {
            tapps::Entity::find()
                .filter(tapps::Column::UserId.eq(admin_id))
                .all(&db)
                .await
                .map_err(|_| HttpError(AppError::internal("Database error")))?
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    let user_tapps = if include_mine {
        if let Some(user_id) = user_id {
            if Some(user_id) != admin_id {
                tapps::Entity::find()
                    .filter(tapps::Column::UserId.eq(user_id))
                    .all(&db)
                    .await
                    .map_err(|_| HttpError(AppError::internal("Database error")))?
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let mut seen = HashSet::new();
    let config = dynamic_config.read().await;
    let mut details = Vec::with_capacity(admin_tapps.len() + user_tapps.len());
    for tapp in user_tapps {
        if !seen.insert(tapp.tapp_id.clone()) {
            continue;
        }
        let (is_temporary, is_admin_tapp) = catalog_install_flags(false);
        details.push(tapp_detail_from_model(
            tapp,
            role,
            is_temporary,
            is_admin_tapp,
            &config,
        ));
    }
    for tapp in admin_tapps {
        if !public_install_visible_to_viewer(&tapp.visibility, is_admin) {
            continue;
        }
        // scope=site loads only site rows, so personal installs never enter `seen`
        // and public apps remain visible even when the viewer also installed them.
        if !seen.insert(tapp.tapp_id.clone()) {
            continue;
        }
        let (is_temporary, is_admin_tapp) = catalog_install_flags(true);
        details.push(tapp_detail_from_model(
            tapp,
            role,
            is_temporary,
            is_admin_tapp,
            &config,
        ));
    }
    Ok(Json(ApiResponse::success(details)))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SetTappVisibilityRequest {
    visibility: String,
}

/// Update public-install visibility (`all` | `admin`). Admin-only; site-owner installs only.
pub(super) async fn set_tapp_visibility(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    axum::extract::Path(tapp_id): axum::extract::Path<String>,
    Json(req): Json<SetTappVisibilityRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, HttpError> {
    require_current_admin(&claims, &db).await?;
    let visibility = parse_tapp_visibility(&req.visibility).ok_or_else(|| {
        HttpError(AppError::bad_request(
            "Invalid visibility, must be 'all' or 'admin'",
        ))
    })?;
    let admin_id = find_admin_user_id(&db)
        .await?
        .ok_or_else(|| HttpError(AppError::internal("No admin user found")))?;

    let existing = tapps::Entity::find()
        .filter(tapps::Column::UserId.eq(admin_id))
        .filter(tapps::Column::TappId.eq(&tapp_id))
        .one(&db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?
        .ok_or_else(|| HttpError(AppError::not_found("Not found")))?;

    let mut active: tapps::ActiveModel = existing.into();
    active.visibility = Set(visibility.to_string());
    active.updated_at = Set(Utc::now().fixed_offset());
    let updated = active
        .update(&db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;

    Ok(Json(ApiResponse::success(serde_json::json!({
        "id": updated.tapp_id,
        "visibility": visibility,
    }))))
}

pub(super) async fn get_tapp(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<Arc<RwLock<DynamicConfig>>>,
    Extension(OptionalClaims(claims)): Extension<OptionalClaims>,
    axum::extract::Path(tapp_id): axum::extract::Path<String>,
) -> Result<Json<ApiResponse<TappDetail>>, HttpError> {
    let user_id = optional_authenticated_user_id(claims.as_ref());
    let is_admin = match claims.as_ref() {
        Some(claims) => current_is_admin(claims, &db).await,
        None => false,
    };
    let visible = find_visible_tapp(&db, user_id, &tapp_id)
        .await?
        .ok_or_else(|| HttpError(AppError::not_found("Not found")))?;
    let role = role_for_optional_subject(user_id, is_admin);
    let (is_temporary, is_admin_tapp) = catalog_install_flags(visible.is_site_owner);
    let config = dynamic_config.read().await;
    let detail = tapp_detail_from_model(visible.tapp, role, is_temporary, is_admin_tapp, &config);
    Ok(Json(ApiResponse::success(detail)))
}
