//! Role-aware Tapp catalog and detail queries.

use super::{
    current_is_admin, find_admin_user_id, find_visible_tapp, optional_authenticated_user_id,
    ApiResponse, TappDetail, TappListItem,
};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use std::collections::HashSet;

use crate::config::DynamicConfig;
use crate::middleware::auth::extract_optional_claims;
use crate::models::entities::tapps;
use crate::services::permission_service::{TappPermissionService, UserRole};
use crate::GLOBAL_DYNAMIC_CONFIG;

pub(super) fn tapp_detail_from_model(
    tapp: tapps::Model,
    role: UserRole,
    is_temporary: bool,
    is_admin_tapp: bool,
    config: &DynamicConfig,
) -> TappDetail {
    let approved_permissions: Vec<String> =
        serde_json::from_value(tapp.approved_permissions.clone()).unwrap_or_default();
    let granted_permissions =
        TappPermissionService::filter_permissions_for_role(config, role, &approved_permissions);
    TappDetail {
        id: tapp.tapp_id,
        name: tapp.name,
        version: tapp.version,
        description: tapp.description,
        author: tapp.author,
        icon: tapp.icon,
        theme_color: tapp.theme_color,
        manifest: tapp.manifest,
        status: format!("{:?}", tapp.status).to_lowercase(),
        granted_permissions,
        installed_at: tapp.installed_at.to_rfc3339(),
        last_run_at: tapp.last_run_at.map(|date| date.to_rfc3339()),
        user_role: role.as_str().to_string(),
        is_temporary,
        is_admin_tapp,
    }
}

pub(super) async fn list_tapps(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<Vec<TappListItem>>>, StatusCode> {
    let claims = extract_optional_claims(&headers);
    let user_id = optional_authenticated_user_id(claims.as_ref());
    let admin_id = find_admin_user_id(&db).await?;
    let mut items = Vec::new();
    let mut seen_tapp_ids = HashSet::new();

    if let Some(user_id) = user_id {
        if Some(user_id) != admin_id {
            let user_tapps = tapps::Entity::find()
                .filter(tapps::Column::UserId.eq(user_id))
                .all(&db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            for tapp in user_tapps {
                seen_tapp_ids.insert(tapp.tapp_id.clone());
                let icon_svg = tapp
                    .manifest
                    .get("iconSvg")
                    .and_then(serde_json::Value::as_str)
                    .map(String::from);
                items.push(TappListItem {
                    id: tapp.tapp_id,
                    name: tapp.name,
                    version: tapp.version,
                    description: tapp.description,
                    icon: tapp.icon,
                    icon_svg,
                    status: format!("{:?}", tapp.status).to_lowercase(),
                    installed_at: tapp.installed_at.to_rfc3339(),
                    last_run_at: tapp.last_run_at.map(|date| date.to_rfc3339()),
                    is_temporary: true,
                    is_admin_tapp: false,
                });
            }
        }
    }

    let admin_tapps = if let Some(admin_id) = admin_id {
        tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(admin_id))
            .all(&db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        Vec::new()
    };
    for tapp in admin_tapps {
        if !seen_tapp_ids.insert(tapp.tapp_id.clone()) {
            continue;
        }
        let icon_svg = tapp
            .manifest
            .get("iconSvg")
            .and_then(serde_json::Value::as_str)
            .map(String::from);
        items.push(TappListItem {
            id: tapp.tapp_id,
            name: tapp.name,
            version: tapp.version,
            description: tapp.description,
            icon: tapp.icon,
            icon_svg,
            status: format!("{:?}", tapp.status).to_lowercase(),
            installed_at: tapp.installed_at.to_rfc3339(),
            last_run_at: tapp.last_run_at.map(|date| date.to_rfc3339()),
            is_temporary: false,
            is_admin_tapp: true,
        });
    }
    Ok(Json(ApiResponse::success(items)))
}

pub(super) async fn list_tapp_details(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<Vec<TappDetail>>>, StatusCode> {
    let claims = extract_optional_claims(&headers);
    let user_id = optional_authenticated_user_id(claims.as_ref());
    let role = match claims.as_ref() {
        Some(claims) if current_is_admin(claims).await => UserRole::Admin,
        _ if user_id.is_some() => UserRole::User,
        _ => UserRole::Guest,
    };
    let admin_id = find_admin_user_id(&db).await?;
    let admin_tapps = if let Some(admin_id) = admin_id {
        tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(admin_id))
            .all(&db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        Vec::new()
    };
    let user_tapps = if let Some(user_id) = user_id {
        if Some(user_id) != admin_id {
            tapps::Entity::find()
                .filter(tapps::Column::UserId.eq(user_id))
                .all(&db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let mut seen = HashSet::new();
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let mut details = Vec::with_capacity(admin_tapps.len() + user_tapps.len());
    for tapp in user_tapps {
        seen.insert(tapp.tapp_id.clone());
        details.push(tapp_detail_from_model(tapp, role, true, false, &config));
    }
    for tapp in admin_tapps {
        if seen.insert(tapp.tapp_id.clone()) {
            details.push(tapp_detail_from_model(tapp, role, false, true, &config));
        }
    }
    Ok(Json(ApiResponse::success(details)))
}

pub(super) async fn get_tapp(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    axum::extract::Path(tapp_id): axum::extract::Path<String>,
) -> Result<Json<ApiResponse<TappDetail>>, StatusCode> {
    let claims = extract_optional_claims(&headers);
    let user_id = optional_authenticated_user_id(claims.as_ref());
    let is_admin = match claims.as_ref() {
        Some(claims) => current_is_admin(claims).await,
        None => false,
    };
    let visible = find_visible_tapp(&db, user_id, &tapp_id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;
    let role = if is_admin {
        UserRole::Admin
    } else if user_id.is_some_and(|user_id| user_id >= 0) {
        UserRole::User
    } else {
        UserRole::Guest
    };
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let detail = tapp_detail_from_model(
        visible.tapp,
        role,
        !visible.is_site_owner,
        visible.is_site_owner,
        &config,
    );
    Ok(Json(ApiResponse::success(detail)))
}
