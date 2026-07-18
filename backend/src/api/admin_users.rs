//! Admin 用户管理 API（设置页「用户管理」模块）。
//!
//! 所有路由要求管理员：main.rs 挂 `admin_middleware`，handler 内再复核一次
//! `ensure_current_admin`（与 auth_local.rs 既有做法一致，防止 wrapper 绕过）。
//!
//! - GET    /api/admin/users                              用户列表（含 OAuth identities、tapp 数、在线状态）
//! - GET    /api/admin/users/{id}                         用户详情（identities + 已安装 tapp）
//! - PATCH  /api/admin/users/{id}                         更新用户（display_name/email/is_admin/local_login_disabled）
//! - DELETE /api/admin/users/{id}/identities/{identity_id} 解绑某用户的 OAuth identity

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement};
use sea_orm::Value as SeaValue;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::middleware::auth::{verify_jwt_token, Claims};

/// 距最近活跃 ≤300s 视为在线（与 presence 跟踪的会话间隔一致）。
const ONLINE_WINDOW_SECS: i64 = 300;

type ApiError = (StatusCode, Json<Value>);

fn db_error(e: impl std::fmt::Debug) -> ApiError {
    tracing::error!("admin_users DB error: {:?}", e);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": "Database error"})),
    )
}

fn not_found() -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error": "User not found"})),
    )
}

async fn require_admin(headers: &axum::http::HeaderMap) -> Result<Claims, ApiError> {
    let claims = verify_jwt_token(headers).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Unauthorized"})),
        )
    })?;
    crate::middleware::auth::ensure_current_admin(&claims).await?;
    Ok(claims)
}

fn rfc3339(row: &QueryResult, col: &str) -> Option<String> {
    row.try_get::<Option<DateTime<Utc>>>("", col)
        .ok()
        .flatten()
        .map(|t| t.to_rfc3339())
}

fn user_row_to_json(row: &QueryResult, identities: &[Value]) -> Value {
    let last_seen = row
        .try_get::<Option<DateTime<Utc>>>("", "last_seen_at")
        .ok()
        .flatten();
    let online = last_seen
        .map(|t| Utc::now().signed_duration_since(t).num_seconds() <= ONLINE_WINDOW_SECS)
        .unwrap_or(false);
    json!({
        "id": row.try_get::<i32>("", "id").unwrap_or(0),
        "username": row.try_get::<String>("", "username").unwrap_or_default(),
        "display_name": row.try_get::<Option<String>>("", "display_name").unwrap_or(None),
        "email": row.try_get::<Option<String>>("", "email").unwrap_or(None),
        "avatar_url": row.try_get::<Option<String>>("", "avatar_url").unwrap_or(None),
        "is_admin": row.try_get::<bool>("", "is_admin").unwrap_or(false),
        "auth_provider": row.try_get::<String>("", "auth_provider").unwrap_or_default(),
        "local_login_disabled": row.try_get::<bool>("", "local_login_disabled").unwrap_or(false),
        "has_password": row.try_get::<bool>("", "has_password").unwrap_or(false),
        "created_at": rfc3339(row, "created_at"),
        "last_login_at": rfc3339(row, "last_login_at"),
        "last_seen_at": last_seen.map(|t| t.to_rfc3339()),
        "online": online,
        "online_seconds": row.try_get::<i64>("", "online_seconds").unwrap_or(0),
        "tapp_count": row.try_get::<i64>("", "tapp_count").unwrap_or(0),
        "identities": identities,
    })
}

fn identity_row_to_json(row: &QueryResult) -> Value {
    json!({
        "id": row.try_get::<i32>("", "id").unwrap_or(0),
        "provider": row.try_get::<String>("", "provider").unwrap_or_default(),
        "provider_username": row.try_get::<Option<String>>("", "provider_username").unwrap_or(None),
        "email": row.try_get::<Option<String>>("", "email").unwrap_or(None),
        "avatar_url": row.try_get::<Option<String>>("", "avatar_url").unwrap_or(None),
        "is_primary": row.try_get::<bool>("", "is_primary").unwrap_or(false),
        "linked_at": rfc3339(row, "linked_at"),
        "last_login_at": rfc3339(row, "last_login_at"),
    })
}

const USER_SELECT: &str = "SELECT u.id, u.username, u.display_name, u.email, u.avatar_url, \
        u.is_admin, u.auth_provider, u.local_login_disabled, \
        u.password_hash IS NOT NULL AS has_password, \
        u.created_at, u.last_login_at, u.last_seen_at, u.online_seconds, \
        (SELECT COUNT(*) FROM tapps t WHERE t.user_id = u.id) AS tapp_count \
     FROM users u";

/// GET /api/admin/users
pub async fn list_users(
    State(db): State<DatabaseConnection>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_admin(&headers).await?;

    let user_rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &format!("{USER_SELECT} ORDER BY u.is_admin DESC, u.created_at ASC"),
            vec![],
        ))
        .await
        .map_err(db_error)?;

    let identity_rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, user_id, provider, provider_username, email, avatar_url, \
                    is_primary, linked_at, last_login_at \
             FROM user_identities ORDER BY is_primary DESC, linked_at ASC",
            vec![],
        ))
        .await
        .map_err(db_error)?;

    let mut identities_by_user: HashMap<i32, Vec<Value>> = HashMap::new();
    for row in &identity_rows {
        let user_id = row.try_get::<i32>("", "user_id").unwrap_or(0);
        identities_by_user
            .entry(user_id)
            .or_default()
            .push(identity_row_to_json(row));
    }

    let users: Vec<Value> = user_rows
        .iter()
        .map(|row| {
            let id = row.try_get::<i32>("", "id").unwrap_or(0);
            let identities = identities_by_user.remove(&id).unwrap_or_default();
            user_row_to_json(row, &identities)
        })
        .collect();

    Ok(Json(json!({ "users": users })))
}

/// GET /api/admin/users/{id}
pub async fn get_user(
    State(db): State<DatabaseConnection>,
    Path(user_id): Path<i32>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_admin(&headers).await?;

    let user_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &format!("{USER_SELECT} WHERE u.id = $1"),
            [user_id.into()],
        ))
        .await
        .map_err(db_error)?
        .ok_or_else(not_found)?;

    let identity_rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, user_id, provider, provider_username, email, avatar_url, \
                    is_primary, linked_at, last_login_at \
             FROM user_identities WHERE user_id = $1 \
             ORDER BY is_primary DESC, linked_at ASC",
            [user_id.into()],
        ))
        .await
        .map_err(db_error)?;
    let identities: Vec<Value> = identity_rows.iter().map(identity_row_to_json).collect();

    let tapp_rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT tapp_id, name, version, status, icon, installed_at, last_run_at \
             FROM tapps WHERE user_id = $1 ORDER BY installed_at DESC",
            [user_id.into()],
        ))
        .await
        .map_err(db_error)?;
    let tapps: Vec<Value> = tapp_rows
        .iter()
        .map(|row| {
            json!({
                "tapp_id": row.try_get::<String>("", "tapp_id").unwrap_or_default(),
                "name": row.try_get::<String>("", "name").unwrap_or_default(),
                "version": row.try_get::<String>("", "version").unwrap_or_default(),
                "status": row.try_get::<String>("", "status").unwrap_or_default(),
                "icon": row.try_get::<Option<String>>("", "icon").unwrap_or(None),
                "installed_at": rfc3339(row, "installed_at"),
                "last_run_at": rfc3339(row, "last_run_at"),
            })
        })
        .collect();

    let mut user = user_row_to_json(&user_row, &identities);
    user["tapps"] = json!(tapps);
    Ok(Json(json!({ "user": user })))
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub is_admin: Option<bool>,
    pub local_login_disabled: Option<bool>,
}

/// PATCH /api/admin/users/{id}
pub async fn update_user(
    State(db): State<DatabaseConnection>,
    Path(user_id): Path<i32>,
    headers: axum::http::HeaderMap,
    Json(req): Json<UpdateUserRequest>,
) -> Result<Json<Value>, ApiError> {
    let claims = require_admin(&headers).await?;
    let self_id: i32 = claims.sub.parse().unwrap_or(0);

    let target = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, username, is_admin FROM users WHERE id = $1",
            [user_id.into()],
        ))
        .await
        .map_err(db_error)?
        .ok_or_else(not_found)?;
    let target_is_admin = target.try_get::<bool>("", "is_admin").unwrap_or(false);
    let target_username = target.try_get::<String>("", "username").unwrap_or_default();

    // 管理员降权保护：不能降级自己；不能降级最后一位管理员
    if req.is_admin == Some(false) && target_is_admin {
        if user_id == self_id {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Cannot revoke your own admin role"})),
            ));
        }
        let admin_count = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT COUNT(*) AS n FROM users WHERE is_admin = true",
                vec![],
            ))
            .await
            .map_err(db_error)?
            .and_then(|r| r.try_get::<i64>("", "n").ok())
            .unwrap_or(0);
        if admin_count <= 1 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Cannot demote the last administrator"})),
            ));
        }
    }

    let mut sets: Vec<String> = Vec::new();
    let mut params: Vec<SeaValue> = Vec::new();
    let push = |sets: &mut Vec<String>, params: &mut Vec<SeaValue>, col: &str, v: SeaValue| {
        params.push(v);
        sets.push(format!("{col} = ${}", params.len()));
    };
    if let Some(display_name) = &req.display_name {
        let trimmed = display_name.trim();
        let value = if trimmed.is_empty() {
            SeaValue::String(None)
        } else {
            SeaValue::String(Some(Box::new(trimmed.to_string())))
        };
        push(&mut sets, &mut params, "display_name", value);
    }
    if let Some(email) = &req.email {
        let trimmed = email.trim();
        if !trimmed.is_empty() && !trimmed.contains('@') {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid email address"})),
            ));
        }
        let value = if trimmed.is_empty() {
            SeaValue::String(None)
        } else {
            SeaValue::String(Some(Box::new(trimmed.to_string())))
        };
        push(&mut sets, &mut params, "email", value);
    }
    if let Some(is_admin) = req.is_admin {
        push(
            &mut sets,
            &mut params,
            "is_admin",
            SeaValue::Bool(Some(is_admin)),
        );
    }
    if let Some(disabled) = req.local_login_disabled {
        push(
            &mut sets,
            &mut params,
            "local_login_disabled",
            SeaValue::Bool(Some(disabled)),
        );
    }

    if sets.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "No fields to update"})),
        ));
    }

    params.push(SeaValue::Int(Some(user_id)));
    let sql = format!(
        "UPDATE users SET {}, updated_at = NOW() WHERE id = ${}",
        sets.join(", "),
        params.len()
    );
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        &sql,
        params,
    ))
    .await
    .map_err(db_error)?;

    tracing::info!(
        "✅ Admin {} updated user {} (id={}): {:?}",
        claims.username,
        target_username,
        user_id,
        req
    );

    // 返回更新后的完整行，前端直接原位替换
    get_user(State(db), Path(user_id), headers).await
}

/// DELETE /api/admin/users/{id}/identities/{identity_id}
pub async fn unlink_identity(
    State(db): State<DatabaseConnection>,
    Path((user_id, identity_id)): Path<(i32, i32)>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let claims = require_admin(&headers).await?;

    let info = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT u.password_hash IS NOT NULL AS has_password, \
                    (SELECT COUNT(*) FROM user_identities i WHERE i.user_id = u.id) AS identity_count, \
                    EXISTS(SELECT 1 FROM user_identities i WHERE i.id = $2 AND i.user_id = u.id) AS identity_belongs \
             FROM users u WHERE u.id = $1",
            [user_id.into(), identity_id.into()],
        ))
        .await
        .map_err(db_error)?
        .ok_or_else(not_found)?;

    if !info
        .try_get::<bool>("", "identity_belongs")
        .unwrap_or(false)
    {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Identity not found for this user"})),
        ));
    }
    let has_password = info.try_get::<bool>("", "has_password").unwrap_or(false);
    let identity_count = info.try_get::<i64>("", "identity_count").unwrap_or(0);
    // 防锁死：无密码且这是唯一登录方式时禁止解绑
    if !has_password && identity_count <= 1 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Cannot unlink the user's only sign-in method"})),
        ));
    }

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM user_identities WHERE id = $1 AND user_id = $2",
        [identity_id.into(), user_id.into()],
    ))
    .await
    .map_err(db_error)?;

    tracing::info!(
        "✅ Admin {} unlinked identity {} from user {}",
        claims.username,
        identity_id,
        user_id
    );

    get_user(State(db), Path(user_id), headers).await
}
