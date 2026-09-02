//! 画像源（头像来源）选择 API。
//!
//! 头像有四类来源且都保留（账号 / OAuth 身份 / 站长平台画像 / 生成兜底），
//! 由本人显式选定一个，写进 `users.avatar_source_*` 并解析成
//! `users.avatar_resolved_url` 快照 —— 之后 `/api/auth/me`、
//! `/api/tapp/context/user`、`/api/profile/user-info`、`/api/admin/users`
//! 读到的都是同一张脸。所有解析规则在 [`crate::services::avatar`]。
//!
//! - GET /api/users/me/avatar-sources        本人可选来源 + 当前选择
//! - PUT /api/users/me/avatar-source         本人切换
//! - GET /api/admin/users/{id}/avatar-sources 管理员查看他人可选来源
//! - PUT /api/admin/users/{id}/avatar-source  管理员替他人切换（只能选对方已有的来源）

use axum::{extract::Path, http::StatusCode, Json};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::middleware::auth::{authenticate_request, Claims};
use crate::services::avatar::{
    current_avatar_source, list_avatar_sources, set_avatar_source, AvatarSourceKind,
};

type ApiError = (StatusCode, Json<Value>);

#[derive(Debug, Deserialize)]
pub struct SetAvatarSourceRequest {
    /// auto | account | identity | platform
    pub kind: String,
    /// identity id 或平台名；auto/account 可省略
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
    tracing::error!("avatar_source {context}: {error}");
    let message = if context == "set" {
        "Failed to save avatar source"
    } else {
        "Failed to load avatar sources"
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

/// 未知 kind 一律拒绝，而不是静默当 auto —— 前端写错字段名时要立刻可见。
fn parse_kind(raw: &str) -> Result<AvatarSourceKind, ApiError> {
    match raw.trim() {
        "auto" => Ok(AvatarSourceKind::Auto),
        "account" => Ok(AvatarSourceKind::Account),
        "identity" => Ok(AvatarSourceKind::Identity),
        "platform" => Ok(AvatarSourceKind::Platform),
        other => Err(bad_request(format!("Unknown avatar source kind: {other}"))),
    }
}

async fn sources_payload(db: &DatabaseConnection, user_id: i32) -> Result<Json<Value>, ApiError> {
    let sources = list_avatar_sources(db, user_id)
        .await
        .map_err(|e| server_error("list", e))?;
    let (kind, source_ref) = current_avatar_source(db, user_id)
        .await
        .map_err(|e| server_error("current", e))?;

    // 同站合并后列表行可能是 platform，而库里仍是 identity：回显 current 时
    // 与 `is_current` 那一行对齐，避免选择器高亮与 current 字段各说各话。
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
    payload: SetAvatarSourceRequest,
) -> Result<Json<Value>, ApiError> {
    let kind = parse_kind(&payload.kind)?;
    let avatar_url = set_avatar_source(db, user_id, kind, payload.source_ref.as_deref())
        .await
        .map_err(apply_error)?;

    Ok(Json(json!({
        "success": true,
        "kind": kind.as_str(),
        "ref": payload.source_ref,
        "avatar_url": avatar_url,
    })))
}

/// GET /api/users/me/avatar-sources
pub async fn list_my_avatar_sources(
    crate::extract::Db(db): crate::extract::Db,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let user_id = current_user_id(&headers, &db).await?;
    sources_payload(&db, user_id).await
}

/// PUT /api/users/me/avatar-source
pub async fn set_my_avatar_source(
    crate::extract::Db(db): crate::extract::Db,
    headers: axum::http::HeaderMap,
    Json(payload): Json<SetAvatarSourceRequest>,
) -> Result<Json<Value>, ApiError> {
    let user_id = current_user_id(&headers, &db).await?;
    apply_source(&db, user_id, payload).await
}

/// GET /api/admin/users/{id}/avatar-sources
pub async fn list_user_avatar_sources(
    crate::extract::Db(db): crate::extract::Db,
    Path(user_id): Path<i32>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_admin(&headers, &db).await?;
    sources_payload(&db, user_id).await
}

/// PUT /api/admin/users/{id}/avatar-source
///
/// 管理员改的是别人的脸，留一条审计日志；`set_avatar_source` 保证只能落在对方
/// **已有**的来源上，管理员无法塞任意 URL。
pub async fn set_user_avatar_source(
    crate::extract::Db(db): crate::extract::Db,
    Path(user_id): Path<i32>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<SetAvatarSourceRequest>,
) -> Result<Json<Value>, ApiError> {
    let admin = require_admin(&headers, &db).await?;
    tracing::info!(
        actor = %admin.sub,
        target_user = user_id,
        kind = %payload.kind,
        source_ref = ?payload.source_ref,
        "Admin changed another user's avatar source"
    );
    apply_source(&db, user_id, payload).await
}

/// 路由层已挂 `admin_middleware`；这里复核一次，与 admin_users.rs 的做法一致
/// （防止 wrapper 注册顺序变动时静默失去保护）。
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
