//! 名称/简介文案来源选择 API（与画像源独立）。
//!
//! - GET /api/users/me/profile-text-sources
//! - PUT /api/users/me/profile-text-source
//! - GET /api/admin/users/{id}/profile-text-sources
//! - PUT /api/admin/users/{id}/profile-text-source
//!
//! 切换后前端应广播 `profile-display-changed`（见 avatarSourceApi /
//! profileTextSourceApi），首页信息条会强制刷新 name/bio。

use axum::{extract::Path, http::StatusCode, Json};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::middleware::auth::{authenticate_request, Claims};
use crate::services::profile_text::{
    current_profile_text_source, list_profile_text_sources, set_profile_text_source,
    ProfileTextSourceKind,
};

type ApiError = (StatusCode, Json<Value>);

#[derive(Debug, Deserialize)]
pub struct SetProfileTextSourceRequest {
    /// auto | account | identity | platform
    pub kind: String,
    #[serde(default, rename = "ref")]
    pub source_ref: Option<String>,
}

fn unauthorized() -> ApiError {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": "Unauthorized"})),
    )
}

fn bad_request(message: impl Into<String>) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"success": false, "message": message.into()})),
    )
}

fn server_error(context: &str, error: impl std::fmt::Display) -> ApiError {
    tracing::error!("profile_text_source {context}: {error}");
    let message = if context == "set" {
        "Failed to save profile text source"
    } else {
        "Failed to load profile text sources"
    };
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"success": false, "message": message})),
    )
}

fn apply_error(error: String) -> ApiError {
    if error.starts_with("Failed to ") {
        server_error("set", error)
    } else {
        bad_request(error)
    }
}

async fn current_user_id(
    headers: &axum::http::HeaderMap,
    db: &DatabaseConnection,
) -> Result<i32, ApiError> {
    let claims = authenticate_request(headers, db)
        .await
        .map_err(|_| unauthorized())?;
    claims.sub.parse::<i32>().map_err(|_| unauthorized())
}

fn parse_kind(raw: &str) -> Result<ProfileTextSourceKind, ApiError> {
    match raw.trim() {
        "auto" => Ok(ProfileTextSourceKind::Auto),
        "account" => Ok(ProfileTextSourceKind::Account),
        "identity" => Ok(ProfileTextSourceKind::Identity),
        "platform" => Ok(ProfileTextSourceKind::Platform),
        other => Err(bad_request(format!(
            "Unknown profile text source kind: {other}"
        ))),
    }
}

async fn sources_payload(db: &DatabaseConnection, user_id: i32) -> Result<Json<Value>, ApiError> {
    let sources = list_profile_text_sources(db, user_id)
        .await
        .map_err(|e| server_error("list", e))?;
    let (kind, source_ref) = current_profile_text_source(db, user_id)
        .await
        .map_err(|e| server_error("current", e))?;

    let (kind, source_ref) = if let Some(current_row) = sources.iter().find(|s| s.is_current) {
        let r = if current_row.source_ref.is_empty() {
            None
        } else {
            Some(current_row.source_ref.clone())
        };
        (current_row.kind, r)
    } else {
        (kind, source_ref)
    };

    Ok(Json(json!({
        "success": true,
        "current": { "kind": kind.as_str(), "ref": source_ref },
        "sources": sources.iter().map(|s| s.to_json()).collect::<Vec<_>>(),
    })))
}

async fn apply_source(
    db: &DatabaseConnection,
    user_id: i32,
    payload: SetProfileTextSourceRequest,
) -> Result<Json<Value>, ApiError> {
    let kind = parse_kind(&payload.kind)?;
    let resolved = set_profile_text_source(db, user_id, kind, payload.source_ref.as_deref())
        .await
        .map_err(apply_error)?;

    Ok(Json(json!({
        "success": true,
        "kind": kind.as_str(),
        "ref": payload.source_ref,
        "name": resolved.name,
        "bio": resolved.bio,
        "platform": resolved.platform,
    })))
}

/// GET /api/users/me/profile-text-sources
pub async fn list_my_profile_text_sources(
    crate::extract::Db(db): crate::extract::Db,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let user_id = current_user_id(&headers, &db).await?;
    sources_payload(&db, user_id).await
}

/// PUT /api/users/me/profile-text-source
pub async fn set_my_profile_text_source(
    crate::extract::Db(db): crate::extract::Db,
    headers: axum::http::HeaderMap,
    Json(payload): Json<SetProfileTextSourceRequest>,
) -> Result<Json<Value>, ApiError> {
    let user_id = current_user_id(&headers, &db).await?;
    apply_source(&db, user_id, payload).await
}

/// GET /api/admin/users/{id}/profile-text-sources
pub async fn list_user_profile_text_sources(
    crate::extract::Db(db): crate::extract::Db,
    Path(user_id): Path<i32>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_admin(&headers, &db).await?;
    sources_payload(&db, user_id).await
}

/// PUT /api/admin/users/{id}/profile-text-source
pub async fn set_user_profile_text_source(
    crate::extract::Db(db): crate::extract::Db,
    Path(user_id): Path<i32>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<SetProfileTextSourceRequest>,
) -> Result<Json<Value>, ApiError> {
    let admin = require_admin(&headers, &db).await?;
    tracing::info!(
        actor = %admin.sub,
        target_user = user_id,
        kind = %payload.kind,
        source_ref = ?payload.source_ref,
        "Admin changed another user's profile text source"
    );
    apply_source(&db, user_id, payload).await
}

async fn require_admin(
    headers: &axum::http::HeaderMap,
    db: &DatabaseConnection,
) -> Result<Claims, ApiError> {
    let claims = authenticate_request(headers, db)
        .await
        .map_err(|_| unauthorized())?;
    crate::middleware::auth::ensure_current_admin_on(&claims, db).await?;
    Ok(claims)
}
