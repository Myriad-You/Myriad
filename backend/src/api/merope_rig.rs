//! Site-wide Anime2.5D face for Agent 人设.
//!
//! Owner writes the compiled package. Guests read the same public atlas and
//! manifest. Arm fragments (`rigid-*-arm-fragment`) are required.
//! Independently articulated limbs are outside this API contract.

use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    http::StatusCode,
    middleware::from_fn_with_state,
    routing::{get, patch, post},
};
use myriad_error::AppError;
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use crate::{
    middleware::auth::Claims,
    services::{image_generation, site_owner::site_owner_user_id},
    state::AppState,
};

mod avatar;
mod decompose;
mod expressions;
mod full_body;
mod import;
mod master;
mod package;
mod portrait;
mod portrait_upload;
mod pose;
mod skeleton;

pub(crate) use package::{cleanup_verified_packages, wardrobe_outfit_face};

type ApiError = (StatusCode, Json<Value>);
type ApiResult<T> = Result<T, ApiError>;

pub fn create_routes(app_state: AppState) -> Router<AppState> {
    let owner = Router::new()
        .route("/", get(package::get_site_rig))
        .route("/portrait", post(portrait::generate_portrait))
        .route("/skeleton", get(skeleton::get_skeleton))
        .route(
            "/full-body/{outfit_id}/skeleton",
            get(skeleton::get_full_body_skeleton),
        )
        .route("/full-body/{outfit_id}", get(full_body::get_full_body))
        .route(
            "/full-body/{outfit_id}/portrait",
            post(full_body::generate_full_body_portrait),
        )
        .route(
            "/full-body/{outfit_id}/portrait/upload",
            post(full_body::upload_full_body_portrait)
                .layer(DefaultBodyLimit::max(12 * 1024 * 1024)),
        )
        .route(
            "/full-body/{outfit_id}/see-through/decompose",
            post(full_body::decompose_full_body),
        )
        .route(
            "/full-body/{outfit_id}/import",
            post(full_body::import_full_body_rig).layer(DefaultBodyLimit::max(24 * 1024 * 1024)),
        )
        .route(
            "/full-body/{outfit_id}/import/preview",
            post(full_body::preview_full_body_rig)
                .layer(DefaultBodyLimit::max(36 * 1024 * 1024)),
        )
        .route("/avatar", post(avatar::generate_sticker_avatar))
        .route("/expressions", get(expressions::list_expressions))
        .route(
            "/expressions/{kind}",
            post(expressions::generate_expression),
        )
        .route(
            "/portrait/upload",
            post(portrait_upload::upload_portrait).layer(DefaultBodyLimit::max(12 * 1024 * 1024)),
        )
        .route(
            "/see-through/status",
            get(decompose::get_see_through_status),
        )
        .route(
            "/see-through/token",
            patch(decompose::update_see_through_token),
        )
        .route(
            "/see-through/space",
            patch(decompose::update_see_through_space),
        )
        .route(
            "/see-through/decompose",
            post(decompose::decompose_with_see_through),
        )
        .route(
            "/pose-corrections",
            patch(pose::save_pose_corrections).layer(DefaultBodyLimit::max(64 * 1024)),
        )
        .route(
            "/import",
            post(import::import_site_rig).layer(DefaultBodyLimit::max(24 * 1024 * 1024)),
        )
        .route(
            "/import/preview",
            post(import::preview_site_rig).layer(DefaultBodyLimit::max(36 * 1024 * 1024)),
        )
        .route_layer(from_fn_with_state(
            app_state.clone(),
            crate::middleware::auth::auth_middleware,
        ));

    Router::new()
        .route("/active", get(package::get_active_rig))
        .route("/assets/{asset_id}", get(package::get_atlas))
        .merge(owner)
}

fn bad_request(message: &str) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(AppError::public_json(message)),
    )
}

fn portrait_generation_config_error(error: image_generation::ImageGenerationError) -> ApiError {
    let code = image_generation::image_generation_failure_code(&error);
    tracing::error!(%error, code, "site portrait generation rejected before provider call");
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": error.to_string(), "code": code })),
    )
}

fn portrait_generation_provider_error(error: image_generation::ImageGenerationError) -> ApiError {
    let code = image_generation::image_generation_failure_code(&error);
    tracing::error!(%error, code, "site portrait generation failed");
    (
        if code == "image_provider_rejected" {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::BAD_GATEWAY
        },
        Json(json!({ "error": error.to_string(), "code": code })),
    )
}

fn not_found(message: &str) -> ApiError {
    (StatusCode::NOT_FOUND, Json(AppError::public_json(message)))
}

fn internal_error(error: impl std::fmt::Display) -> ApiError {
    tracing::error!(%error, "merope rig failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "Internal server error",
            "code": "merope_rig_failed"
        })),
    )
}

async fn require_merope_enabled() -> ApiResult<()> {
    let enabled = crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .merope_enabled_resolved();
    if enabled {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Agent persona is disabled",
                "code": "merope_disabled"
            })),
        ))
    }
}

async fn require_owner(claims: &Claims, db: &DatabaseConnection) -> ApiResult<i32> {
    let owner = site_owner_user_id(db).await.map_err(internal_error)?;
    let user_id = claims.durable_user_id().ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(AppError::public_json("Invalid user")),
        )
    })?;
    if user_id != owner {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Only the site owner can change the face",
                "code": "site_owner_required"
            })),
        ));
    }
    Ok(user_id)
}
