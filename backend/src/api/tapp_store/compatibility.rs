//! Compatibility endpoints retained for older Tapp clients.

use super::{
    canonical_installation_owner_id, copy_regular_tapp_directory, current_user_role,
    get_admin_user_id, installed_tapp_dir, lock_tapp_lifecycle, validate_installed_resources,
    validate_tapp_id, write_install_generation, write_tapp_resource, ApiResponse, TappDirStage,
    TappManifest, MAX_TAPP_RESOURCE_BYTES,
};
use crate::{middleware::auth::Claims, models::entities::tapps};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
    TransactionTrait,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UpdateSeparatedCssRequest {
    #[serde(default)]
    widget_css: Option<String>,
    #[serde(default)]
    page_css: Option<String>,
}

/// Replace generated unified-mode CSS for clients predating direct package updates.
pub(super) async fn update_separated_css(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
    Json(request): Json<UpdateSeparatedCssRequest>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    validate_tapp_id(&tapp_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    if request.widget_css.is_none() && request.page_css.is_none() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if [request.widget_css.as_deref(), request.page_css.as_deref()]
        .into_iter()
        .flatten()
        .any(|css| css.len() as u64 > MAX_TAPP_RESOURCE_BYTES)
    {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    let user_id = claims
        .sub
        .parse::<i32>()
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    let role = current_user_role(&claims).await;
    let site_owner_id = get_admin_user_id(&db).await?;
    let owner_id = canonical_installation_owner_id(role, user_id, site_owner_id);
    let txn = db
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    lock_tapp_lifecycle(&txn, &tapp_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let tapp = tapps::Entity::find()
        .filter(tapps::Column::UserId.eq(owner_id))
        .filter(tapps::Column::TappId.eq(&tapp_id))
        .one(&txn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let manifest: TappManifest = serde_json::from_value(tapp.manifest.clone())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if manifest.css_mode.as_deref() == Some("separated") {
        return Err(StatusCode::CONFLICT);
    }

    let final_tapp_dir = installed_tapp_dir(&tapp)?;
    let stage = TappDirStage::create(&final_tapp_dir)
        .await
        .map_err(|error| {
            super::log_tapp_filesystem_access(&final_tapp_dir, &error);
            super::tapp_filesystem_error_status(&error)
        })?;
    copy_regular_tapp_directory(&final_tapp_dir, stage.path())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(widget_css) = &request.widget_css {
        write_tapp_resource(stage.path(), "widget.css", widget_css)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    if let Some(page_css) = &request.page_css {
        write_tapp_resource(stage.path(), "page.css", page_css)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let now = Utc::now().fixed_offset();
    write_install_generation(stage.path(), now).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    validate_installed_resources(&manifest, stage.path()).map_err(|_| StatusCode::BAD_REQUEST)?;

    let activated = stage.activate(&final_tapp_dir).await.map_err(|error| {
        super::log_tapp_filesystem_access(&final_tapp_dir, &error);
        super::tapp_filesystem_error_status(&error)
    })?;
    let mut active: tapps::ActiveModel = tapp.into();
    active.updated_at = Set(now);
    if active.update(&txn).await.is_err() {
        txn.rollback().await.ok();
        activated.rollback().await;
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    if txn.commit().await.is_err() {
        activated.rollback().await;
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    activated.commit().await;
    Ok(Json(ApiResponse::success(())))
}
