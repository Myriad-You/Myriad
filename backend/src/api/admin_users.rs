//! Admin 用户管理 API（设置页「用户管理」模块）。
//!
//! 所有路由要求管理员：router/base.rs 挂 `admin_middleware`，handler 内再复核一次
//! `ensure_current_admin_on`（与 auth_local.rs 既有做法一致，防止 wrapper 绕过）。
//!
//! - GET    /api/admin/users                              用户列表（含 OAuth identities、tapp 数、在线状态）
//! - GET    /api/admin/users/{id}                         用户详情（identities + 已安装 tapp）
//! - PATCH  /api/admin/users/{id}                         更新用户（is_admin/local_login_disabled/tapp_install_disabled）
//! - DELETE /api/admin/users/{id}                         删除用户（含关联数据清理）
//! - DELETE /api/admin/users/{id}/identities/{identity_id} 解绑某用户的 OAuth identity
//! - DELETE /api/admin/users/{id}/tapps/{tapp_id}         卸载某用户的已安装 Tapp
//!
//! Privilege model: durable `users.is_owner`.

use axum::{Json, extract::Path, http::StatusCode};
use chrono::{DateTime, Utc};
use myriad_error::AppError;
use sea_orm::Value as SeaValue;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, TransactionTrait,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::middleware::auth::{Claims, authenticate_request, notify_auth_cache_invalidation};

/// 距最近活跃 ≤300s 视为在线（与 presence 跟踪的会话间隔一致）。
const ONLINE_WINDOW_SECS: i64 = 300;

type ApiError = (StatusCode, Json<Value>);

fn db_error<E: std::fmt::Display>(context: &'static str) -> impl FnOnce(E) -> ApiError {
    move |error| {
        tracing::error!(%error, context, "admin users store failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AppError::public_json(format!("Failed to {context}"))),
        )
    }
}

fn http_to_api(err: crate::error::HttpError) -> ApiError {
    let status =
        StatusCode::from_u16(err.0.status_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(err.0.to_json()))
}

fn not_found() -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(AppError::public_json("User not found")),
    )
}

/// Non-owners may not change `is_admin` on anyone (promote / demote).
/// Owners return None.
pub(crate) fn non_owner_is_admin_change_error(actor_is_owner: bool) -> Option<&'static str> {
    if actor_is_owner {
        return None;
    }
    Some("Only the site owner can change admin roles")
}

/// Non-owners may not create users with `is_admin=true`.
pub(crate) fn non_owner_grant_admin_on_create_error(
    actor_is_owner: bool,
    want_is_admin: bool,
) -> Option<&'static str> {
    if !want_is_admin {
        return None;
    }
    non_owner_is_admin_change_error(actor_is_owner)
}

/// Non-owners may not delete admins or the owner.
/// Nobody may delete the site owner.
pub(crate) fn non_owner_delete_error(
    actor_is_owner: bool,
    target_is_owner: bool,
    target_is_admin: bool,
) -> Option<&'static str> {
    if target_is_owner {
        return Some("Cannot delete the site owner");
    }
    if actor_is_owner {
        return None;
    }
    if target_is_admin {
        return Some("Only the site owner can delete administrators");
    }
    None
}

/// The site owner cannot be barred from installing Tapps.
pub(crate) fn cannot_restrict_owner_install(
    target_is_owner: bool,
    disable: Option<bool>,
) -> Option<&'static str> {
    if target_is_owner && disable == Some(true) {
        return Some("Cannot disable Tapp install for the site owner");
    }
    None
}

/// The site owner cannot be demoted (is_admin=false).
pub(crate) fn cannot_demote_owner_error(
    target_is_owner: bool,
    new_is_admin: Option<bool>,
) -> Option<&'static str> {
    if target_is_owner && new_is_admin == Some(false) {
        return Some("Cannot demote the site owner");
    }
    None
}

/// Last-admin COUNT is only authoritative after this lock, in the same
/// transaction as the demote/delete. `ORDER BY id` keeps concurrent guards
/// from deadlocking on the admin row set.
pub(crate) const LOCK_ADMINS_SQL: &str =
    "SELECT id FROM users WHERE is_admin = true ORDER BY id FOR UPDATE";

pub(crate) fn last_admin_mutation_blocked(target_is_admin: bool, admin_count: i64) -> bool {
    target_is_admin && admin_count <= 1
}

pub(crate) async fn lock_and_count_admins<C: ConnectionTrait>(db: &C) -> Result<i64, ApiError> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            LOCK_ADMINS_SQL,
            vec![],
        ))
        .await
        .map_err(db_error("lock admins"))?;
    i64::try_from(rows.len()).map_err(db_error("count admins"))
}

/// Load `is_owner` for a user id (defaults false if missing).
async fn load_is_owner(db: &DatabaseConnection, user_id: i32) -> Result<bool, ApiError> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT is_owner FROM users WHERE id = $1",
            [user_id.into()],
        ))
        .await
        .map_err(db_error("check owner"))?;
    Ok(row
        .and_then(|r| r.try_get::<bool>("", "is_owner").ok())
        .unwrap_or(false))
}

/// Public helper for create-user path (auth_local).
pub async fn actor_is_owner(db: &DatabaseConnection, actor_id: i32) -> Result<bool, ApiError> {
    load_is_owner(db, actor_id).await
}

async fn require_admin(
    headers: &axum::http::HeaderMap,
    db: &DatabaseConnection,
) -> Result<Claims, ApiError> {
    let claims = authenticate_request(headers, db).await.map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(AppError::public_json("Unauthorized")),
        )
    })?;
    crate::middleware::auth::ensure_current_admin_on(&claims, db).await?;
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
        "avatar_url": crate::services::avatar::proxied_avatar_value(
            row.try_get::<Option<String>>("", "avatar_url").unwrap_or(None),
        ),
        "is_admin": row.try_get::<bool>("", "is_admin").unwrap_or(false),
        "is_owner": row.try_get::<bool>("", "is_owner").unwrap_or(false),
        "auth_provider": row.try_get::<String>("", "auth_provider").unwrap_or_default(),
        "local_login_disabled": row.try_get::<bool>("", "local_login_disabled").unwrap_or(false),
        "tapp_install_disabled": row.try_get::<bool>("", "tapp_install_disabled").unwrap_or(false),
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
        "avatar_url": crate::services::avatar::proxied_avatar_value(
            row.try_get::<Option<String>>("", "avatar_url").unwrap_or(None),
        ),
        "is_primary": row.try_get::<bool>("", "is_primary").unwrap_or(false),
        "linked_at": rfc3339(row, "linked_at"),
        "last_login_at": rfc3339(row, "last_login_at"),
    })
}

/// User list/detail SELECT with resolved face snapshot as `avatar_url`
/// (matches `/me` / public face). Identity rows keep identity-level URLs.
///
/// Only the list projection computes `tapp_count` in SQL; the detail endpoint
/// loads the full tapp list anyway and derives the count from it.
fn user_select_sql(with_tapp_count: bool) -> String {
    format!(
        "SELECT u.id, u.username, u.display_name, u.email, {avatar} AS avatar_url, \
        u.is_admin, u.is_owner, u.auth_provider, u.local_login_disabled, \
        u.tapp_install_disabled, \
        u.password_hash IS NOT NULL AS has_password, \
        u.created_at, u.last_login_at, u.last_seen_at, u.online_seconds{tapp_count} \
     FROM users u",
        avatar = crate::services::avatar::avatar_snapshot_expr("u"),
        tapp_count = if with_tapp_count {
            ", (SELECT COUNT(*) FROM tapps t WHERE t.user_id = u.id) AS tapp_count"
        } else {
            ""
        },
    )
}

/// GET /api/admin/users
pub async fn list_users(
    crate::extract::Db(db): crate::extract::Db,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_admin(&headers, &db).await?;

    let user_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "{} ORDER BY u.is_owner DESC, u.is_admin DESC, u.created_at ASC",
                user_select_sql(true)
            ),
            vec![],
        ))
        .await
        .map_err(db_error("list users"))?;

    let identity_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, user_id, provider, provider_username, email, avatar_url, \
                    is_primary, linked_at, last_login_at \
             FROM user_identities WHERE LOWER(provider) NOT IN ('qq', 'telegram', 'discord_dm', 'feishu') ORDER BY is_primary DESC, linked_at ASC",
            vec![],
        ))
        .await
        .map_err(db_error("list user identities"))?;

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
    crate::extract::Db(db): crate::extract::Db,
    Path(user_id): Path<i32>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_admin(&headers, &db).await?;

    let user_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!("{} WHERE u.id = $1", user_select_sql(false)),
            [user_id.into()],
        ))
        .await
        .map_err(db_error("find user"))?
        .ok_or_else(not_found)?;

    let identity_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, user_id, provider, provider_username, email, avatar_url, \
                    is_primary, linked_at, last_login_at \
             FROM user_identities WHERE user_id = $1 AND LOWER(provider) NOT IN ('qq', 'telegram', 'discord_dm', 'feishu') \
             ORDER BY is_primary DESC, linked_at ASC",
            [user_id.into()],
        ))
        .await
        .map_err(db_error("list user identities"))?;
    let identities: Vec<Value> = identity_rows.iter().map(identity_row_to_json).collect();

    let tapp_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT tapp_id, name, version, status, icon, installed_at, last_run_at \
             FROM tapps WHERE user_id = $1 ORDER BY installed_at DESC",
            [user_id.into()],
        ))
        .await
        .map_err(db_error("list user apps"))?;
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
    user["tapp_count"] = json!(tapps.len());
    user["tapps"] = json!(tapps);
    Ok(Json(json!({ "user": user })))
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub is_admin: Option<bool>,
    pub local_login_disabled: Option<bool>,
    pub tapp_install_disabled: Option<bool>,
}

/// Notice for promote: JWT claim stays false until re-login.
pub const PROMOTE_RELOGIN_NOTICE: &str =
    "Admin role granted. The promoted user must sign out and sign in again for admin API access.";

/// Notice for demote: admin middleware double-checks DB so access ends immediately.
pub const DEMOTE_IMMEDIATE_NOTICE: &str =
    "Admin access revoked. It takes effect immediately for admin API checks.";

/// PATCH /api/admin/users/{id}
pub async fn update_user(
    crate::extract::Db(db): crate::extract::Db,
    Path(user_id): Path<i32>,
    headers: axum::http::HeaderMap,
    Json(req): Json<UpdateUserRequest>,
) -> Result<Json<Value>, ApiError> {
    let claims = require_admin(&headers, &db).await?;
    let self_id =
        crate::services::tapp_ownership::positive_user_id(&claims.sub).ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                Json(AppError::public_json("A durable user account is required")),
            )
        })?;
    let actor_is_owner = load_is_owner(&db, self_id).await?;

    let target = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, username, is_admin, is_owner FROM users WHERE id = $1",
            [user_id.into()],
        ))
        .await
        .map_err(db_error("find user"))?
        .ok_or_else(not_found)?;
    let target_is_admin = target.try_get::<bool>("", "is_admin").unwrap_or(false);
    let target_is_owner = target.try_get::<bool>("", "is_owner").unwrap_or(false);
    let target_username = target.try_get::<String>("", "username").unwrap_or_default();

    // is_admin 变更保护：仅站点 owner 可改任何用户的 is_admin
    if req.is_admin.is_some() {
        if let Some(msg) = non_owner_is_admin_change_error(actor_is_owner) {
            return Err((StatusCode::FORBIDDEN, Json(AppError::public_json(msg))));
        }
        if let Some(msg) = cannot_demote_owner_error(target_is_owner, req.is_admin) {
            return Err((StatusCode::BAD_REQUEST, Json(AppError::public_json(msg))));
        }
        if req.is_admin == Some(false) && target_is_admin && user_id == self_id {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json("Cannot revoke your own admin role")),
            ));
        }
    }

    let demoting_admin = req.is_admin == Some(false) && target_is_admin;
    let login_methods_txn = if req.local_login_disabled.is_some() || demoting_admin {
        let txn = db.begin().await.map_err(db_error("begin user update"))?;
        if demoting_admin {
            let admin_count = lock_and_count_admins(&txn).await?;
            if last_admin_mutation_blocked(true, admin_count) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(AppError::public_json(
                        "Cannot demote the last administrator",
                    )),
                ));
            }
        }
        if req.local_login_disabled.is_some() {
            crate::api::oauth::lock_login_methods(&txn, user_id)
                .await
                .map_err(db_error("lock login methods"))?;
        }
        if req.local_login_disabled == Some(true) {
            let identity_count = txn
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    format!(
                        "SELECT COUNT(*) AS n FROM user_identities WHERE user_id = $1 AND {}",
                        crate::services::channel_pairing::SQL_NOT_PAIRING_PROVIDER
                    ),
                    [user_id.into()],
                ))
                .await
                .map_err(db_error("count identities"))?
                .and_then(|r| r.try_get::<i64>("", "n").ok())
                .unwrap_or(0);
            if crate::api::oauth::disable_local_login_blocks(identity_count) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(AppError::public_json(
                        "Cannot disable local login: user has no linked OAuth identity",
                    )),
                ));
            }
        }
        Some(txn)
    } else {
        None
    };

    if let Some(msg) = cannot_restrict_owner_install(target_is_owner, req.tapp_install_disabled) {
        return Err((StatusCode::BAD_REQUEST, Json(AppError::public_json(msg))));
    }

    let mut sets: Vec<String> = Vec::new();
    let mut params: Vec<SeaValue> = Vec::new();
    let push = |sets: &mut Vec<String>, params: &mut Vec<SeaValue>, col: &str, v: SeaValue| {
        params.push(v);
        sets.push(format!("{col} = ${}", params.len()));
    };
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
    if let Some(disabled) = req.tapp_install_disabled {
        push(
            &mut sets,
            &mut params,
            "tapp_install_disabled",
            SeaValue::Bool(Some(disabled)),
        );
    }

    if sets.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("No fields to update")),
        ));
    }

    params.push(SeaValue::Int(Some(user_id)));
    let sql = format!(
        "UPDATE users SET {}, updated_at = NOW() WHERE id = ${}",
        sets.join(", "),
        params.len()
    );
    let update = Statement::from_sql_and_values(DatabaseBackend::Postgres, &sql, params);
    if let Some(txn) = login_methods_txn {
        txn.execute_raw(update)
            .await
            .map_err(db_error("update user"))?;
        txn.commit()
            .await
            .map_err(db_error("commit login-method update"))?;
    } else {
        db.execute_raw(update)
            .await
            .map_err(db_error("update user"))?;
    }

    // Role changes are authorization facts, not merely profile fields.  Drop
    // this process's snapshot and fan out a PostgreSQL invalidation so a
    // demoted session cannot wait for the normal five-second cache expiry.
    if req.is_admin.is_some() {
        if let Err(error) = notify_auth_cache_invalidation(&db, user_id).await {
            tracing::warn!(
                user_id,
                error = %error,
                "auth cache invalidation NOTIFY failed after admin role update"
            );
        }
    }

    tracing::info!(
        "✅ Admin {} updated user {} (id={}): {:?}",
        claims.username,
        target_username,
        user_id,
        req
    );

    // 返回更新后的完整行；promote 附带 re-login 提示，demote 附带立即生效提示
    let Json(mut body) = get_user(crate::extract::Db(db), Path(user_id), headers).await?;
    if req.is_admin == Some(true) && !target_is_admin {
        body["notice"] = json!(PROMOTE_RELOGIN_NOTICE);
        body["message"] = json!(PROMOTE_RELOGIN_NOTICE);
    } else if req.is_admin == Some(false) && target_is_admin {
        body["notice"] = json!(DEMOTE_IMMEDIATE_NOTICE);
        body["message"] = json!(DEMOTE_IMMEDIATE_NOTICE);
    }
    Ok(Json(body))
}

/// DELETE /api/admin/users/{id}/tapps/{tapp_id}
pub async fn uninstall_user_tapp(
    crate::extract::Db(db): crate::extract::Db,
    Path((user_id, tapp_id)): Path<(i32, String)>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_admin(&headers, &db).await?;
    let _ = crate::api::tapp_store::uninstall_tapp_for_user(&db, user_id, &tapp_id, false)
        .await
        .map_err(http_to_api)?;
    get_user(crate::extract::Db(db), Path(user_id), headers).await
}

/// DELETE /api/admin/users/{id}/identities/{identity_id}
pub async fn unlink_identity(
    crate::extract::Db(db): crate::extract::Db,
    Path((user_id, identity_id)): Path<(i32, i32)>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let claims = require_admin(&headers, &db).await?;

    let txn = db
        .begin()
        .await
        .map_err(db_error("begin identity unlink"))?;
    crate::api::oauth::lock_login_methods(&txn, user_id)
        .await
        .map_err(db_error("lock login methods"))?;

    let info = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT u.password_hash IS NOT NULL AS has_password, u.local_login_disabled, \
                    (SELECT COUNT(*) FROM user_identities i WHERE i.user_id = u.id AND LOWER(i.provider) NOT IN ('qq', 'telegram', 'discord_dm', 'feishu')) AS identity_count, \
                    EXISTS(SELECT 1 FROM user_identities i WHERE i.id = $2 AND i.user_id = u.id AND LOWER(i.provider) NOT IN ('qq', 'telegram', 'discord_dm', 'feishu')) AS identity_belongs \
             FROM users u WHERE u.id = $1",
            [user_id.into(), identity_id.into()],
        ))
        .await
        .map_err(db_error("load identity"))?
        .ok_or_else(not_found)?;

    if !info
        .try_get::<bool>("", "identity_belongs")
        .unwrap_or(false)
    {
        return Err((
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Identity not found for this user")),
        ));
    }
    let has_password = info.try_get::<bool>("", "has_password").unwrap_or(false);
    let local_login_disabled = info
        .try_get::<bool>("", "local_login_disabled")
        .unwrap_or(false);
    let identity_count = info.try_get::<i64>("", "identity_count").unwrap_or(0);
    // 防锁死：这是最后一个 OAuth 绑定，且本地登录不可用（无密码或已禁用）时禁止解绑
    if crate::api::oauth::unlink_blocks_last_signin(
        has_password,
        local_login_disabled,
        identity_count,
    ) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(
                "Cannot unlink the user's only sign-in method",
            )),
        ));
    }

    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM user_identities WHERE id = $1 AND user_id = $2",
        [identity_id.into(), user_id.into()],
    ))
    .await
    .map_err(db_error("unlink identity"))?;
    txn.commit()
        .await
        .map_err(db_error("commit identity unlink"))?;

    tracing::info!(
        "✅ Admin {} unlinked identity {} from user {}",
        claims.username,
        identity_id,
        user_id
    );

    get_user(crate::extract::Db(db), Path(user_id), headers).await
}

/// 在删除 users 行之前，清理无 FK / 非 CASCADE 的用户关联数据。
///
/// - `user_identities`：有 `ON DELETE CASCADE`，随 users 删除即可。
/// - 其余带 `user_id` 的表多为逻辑关联（无 FK），需显式删除以免残留。
/// - `phantasi_sources` 删除会 CASCADE 到 `phantasi_items` 及其下游。
/// - `federation_room_members.local_user_id` 可空：置 NULL，保留房间成员记录。
/// - 磁盘上的 Tapp 安装目录等文件资源不在此清理（与卸载路径不同）；DB 行删除后
/// 对应目录成为孤立文件，可后续由运维/GC 处理。
async fn cleanup_user_related_data(
    txn: &impl ConnectionTrait,
    user_id: i32,
) -> Result<(), ApiError> {
    // 顺序：先子表/依赖，再用户拥有的顶层资源。
    const CLEANUP_SQL: &[&str] = &[
        // Agent
        "DELETE FROM agent_messages WHERE session_id IN \
         (SELECT id FROM agent_sessions WHERE user_id = $1)",
        "DELETE FROM agent_sessions WHERE user_id = $1",
        "DELETE FROM agent_tasks WHERE user_id = $1",
        "DELETE FROM agent_notifications WHERE user_id = $1",
        "DELETE FROM agent_task_presets WHERE user_id = $1",
        // Platform data
        "DELETE FROM activity_events WHERE user_id = $1",
        "DELETE FROM metadata_history WHERE user_id = $1",
        "DELETE FROM platform_metadata WHERE user_id = $1",
        "DELETE FROM platform_reports WHERE user_id = $1",
        // Tapp stack
        "DELETE FROM tapp_task_executions WHERE scheduled_task_id IN \
         (SELECT id FROM tapp_scheduled_tasks WHERE user_id = $1) \
         OR user_id = $1",
        "DELETE FROM tapp_scheduled_tasks WHERE user_id = $1",
        "DELETE FROM tapp_user_activities WHERE user_id = $1",
        "DELETE FROM tapp_quota_usage WHERE user_id = $1",
        "DELETE FROM tapp_storage WHERE user_id = $1",
        "DELETE FROM tapp_widgets WHERE user_id = $1",
        "DELETE FROM tapps WHERE user_id = $1",
        "DELETE FROM tapp_runtime_registry WHERE subject_id = $1 OR owner_id = $1",
        "DELETE FROM tapp_ai_cost_ledger WHERE subject_id = $1 OR owner_id = $1",
        // Phantasi（sources → items CASCADE）
        "DELETE FROM phantasi_comments WHERE user_id = $1",
        "DELETE FROM phantasi_user_states WHERE user_id = $1",
        "DELETE FROM phantasi_categories WHERE user_id = $1",
        "DELETE FROM phantasi_sources WHERE user_id = $1",
        "DELETE FROM rsshub_instances WHERE user_id = $1",
        // Federation
        "DELETE FROM federation_timeline WHERE user_id = $1",
        "DELETE FROM federation_published_content WHERE user_id = $1",
        "DELETE FROM federation_channel_messages WHERE channel_id IN \
         (SELECT channel_id FROM federation_channels WHERE user_id = $1)",
        "DELETE FROM federation_channels WHERE user_id = $1",
        "DELETE FROM federation_activities WHERE user_id = $1",
        "DELETE FROM federation_follows WHERE user_id = $1",
        "DELETE FROM federation_keys WHERE user_id = $1",
        "UPDATE federation_room_members SET local_user_id = NULL WHERE local_user_id = $1",
    ];

    for sql in CLEANUP_SQL {
        txn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            *sql,
            [user_id.into()],
        ))
        .await
        .map_err(db_error("cleanup user data"))?;
    }

    Ok(())
}

/// DELETE /api/admin/users/{id}
///
/// 安全规则：
/// - 不能删除自己（JWT sub == target id）→ 400
/// - 不能删除站点 owner → 400
/// - 非 owner 不得删除管理员 → 403
/// - 不能删除最后一位管理员 → 400
pub async fn delete_user(
    crate::extract::Db(db): crate::extract::Db,
    Path(user_id): Path<i32>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let claims = require_admin(&headers, &db).await?;
    let self_id =
        crate::services::tapp_ownership::positive_user_id(&claims.sub).ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                Json(AppError::public_json("A durable user account is required")),
            )
        })?;
    let actor_is_owner = load_is_owner(&db, self_id).await?;

    if user_id == self_id {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Cannot delete your own account")),
        ));
    }

    let target = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, username, is_admin, is_owner FROM users WHERE id = $1",
            [user_id.into()],
        ))
        .await
        .map_err(db_error("find user"))?
        .ok_or_else(not_found)?;
    let target_is_admin = target.try_get::<bool>("", "is_admin").unwrap_or(false);
    let target_is_owner = target.try_get::<bool>("", "is_owner").unwrap_or(false);
    let target_username = target.try_get::<String>("", "username").unwrap_or_default();

    if let Some(msg) = non_owner_delete_error(actor_is_owner, target_is_owner, target_is_admin) {
        let status = if target_is_owner {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::FORBIDDEN
        };
        return Err((status, Json(AppError::public_json(msg))));
    }

    let txn = db.begin().await.map_err(db_error("begin user delete"))?;
    if target_is_admin {
        let admin_count = lock_and_count_admins(&txn).await?;
        if last_admin_mutation_blocked(true, admin_count) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json(
                    "Cannot delete the last administrator",
                )),
            ));
        }
    }
    cleanup_user_related_data(&txn, user_id).await?;
    // user_identities CASCADE；其余已在 cleanup 中处理
    let result = txn
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM users WHERE id = $1",
            [user_id.into()],
        ))
        .await
        .map_err(db_error("delete user"))?;
    if result.rows_affected() == 0 {
        txn.rollback()
            .await
            .map_err(db_error("rollback user delete"))?;
        return Err(not_found());
    }

    txn.commit().await.map_err(db_error("commit user delete"))?;
    crate::services::agent::consciousness::clear_user_attention(user_id);

    // Invalidate locally after the destructive commit, then fan out a best-
    // effort PostgreSQL notification.  If the notification is missed, the
    // bounded cache TTL remains the correctness limit for peer instances.
    if let Err(error) = notify_auth_cache_invalidation(&db, user_id).await {
        tracing::warn!(
            user_id,
            error = %error,
            "auth cache invalidation NOTIFY failed after user deletion"
        );
    }

    tracing::info!(
        "✅ Admin {} deleted user {} (id={})",
        claims.username,
        target_username,
        user_id
    );

    Ok(Json(json!({
        "success": true,
        "deleted_user_id": user_id,
        "username": target_username,
    })))
}

#[cfg(test)]
mod tests {
    use super::{
        LOCK_ADMINS_SQL, cannot_demote_owner_error, cannot_restrict_owner_install,
        last_admin_mutation_blocked, non_owner_delete_error, non_owner_grant_admin_on_create_error,
        non_owner_is_admin_change_error,
    };

    #[test]
    fn admin_actor_id_is_not_decoded_to_zero() {
        let src = include_str!("admin_users.rs");
        let production = src.split("#[cfg(test)]").next().expect("production");
        assert!(production.contains("positive_user_id"));
        assert!(!production.contains("claims.sub.parse().unwrap_or(0)"));
    }

    /// 与 handler 中安全规则保持一致的纯函数，便于无 DB 单测。
    fn reject_self_delete(actor_id: i32, target_id: i32) -> bool {
        actor_id == target_id
    }

    fn reject_last_admin_delete(target_is_admin: bool, admin_count: i64) -> bool {
        last_admin_mutation_blocked(target_is_admin, admin_count)
    }

    fn reject_last_admin_demote(
        target_is_admin: bool,
        new_is_admin: bool,
        admin_count: i64,
    ) -> bool {
        !new_is_admin && last_admin_mutation_blocked(target_is_admin, admin_count)
    }

    #[test]
    fn cannot_delete_self() {
        assert!(reject_self_delete(3, 3));
        assert!(!reject_self_delete(3, 4));
    }

    #[test]
    fn cannot_delete_last_admin() {
        assert!(reject_last_admin_delete(true, 1));
        assert!(reject_last_admin_delete(true, 0));
        assert!(!reject_last_admin_delete(true, 2));
        assert!(!reject_last_admin_delete(false, 1));
    }

    #[test]
    fn non_owner_cannot_change_any_is_admin() {
        assert!(non_owner_is_admin_change_error(false).is_some());
        assert!(non_owner_is_admin_change_error(true).is_none());
    }

    #[test]
    fn non_owner_cannot_promote_or_create_admin() {
        assert!(non_owner_grant_admin_on_create_error(false, true).is_some());
        assert!(non_owner_grant_admin_on_create_error(false, false).is_none());
        assert!(non_owner_grant_admin_on_create_error(true, true).is_none());
    }

    #[test]
    fn owner_can_change_admin_roles() {
        assert!(non_owner_is_admin_change_error(true).is_none());
        assert!(!reject_last_admin_demote(true, false, 2));
        assert!(reject_last_admin_demote(true, false, 1));
    }

    #[test]
    fn cannot_restrict_owner_tapp_install() {
        assert!(cannot_restrict_owner_install(true, Some(true)).is_some());
        assert!(cannot_restrict_owner_install(true, Some(false)).is_none());
        assert!(cannot_restrict_owner_install(false, Some(true)).is_none());
        assert!(cannot_restrict_owner_install(true, None).is_none());
    }

    #[test]
    fn cannot_demote_owner() {
        assert!(cannot_demote_owner_error(true, Some(false)).is_some());
        assert!(cannot_demote_owner_error(true, Some(true)).is_none());
        assert!(cannot_demote_owner_error(false, Some(false)).is_none());
        assert!(cannot_demote_owner_error(true, None).is_none());
    }

    #[test]
    fn cannot_delete_owner() {
        assert!(non_owner_delete_error(true, true, true).is_some());
        assert!(non_owner_delete_error(false, true, true).is_some());
        assert!(non_owner_delete_error(false, true, false).is_some());
    }

    #[test]
    fn non_owner_cannot_delete_admin() {
        assert!(non_owner_delete_error(false, false, true).is_some());
        // 删除普通用户允许
        assert!(non_owner_delete_error(false, false, false).is_none());
    }

    #[test]
    fn owner_can_delete_other_admin_when_not_last() {
        assert!(non_owner_delete_error(true, false, true).is_none());
        assert!(!reject_last_admin_delete(true, 2));
        assert!(reject_last_admin_delete(true, 1));
    }

    #[test]
    fn is_owner_gates_are_boolean_not_id() {
        assert!(non_owner_is_admin_change_error(false).is_some());
        assert!(non_owner_is_admin_change_error(true).is_none());
    }

    #[test]
    fn last_admin_guard_locks_admins_in_id_order() {
        assert!(LOCK_ADMINS_SQL.contains("ORDER BY id FOR UPDATE"));
        let src = include_str!("admin_users.rs");
        let update = src
            .split("pub async fn update_user")
            .nth(1)
            .and_then(|rest| rest.split("pub async fn uninstall_user_tapp").next())
            .expect("update_user");
        assert!(update.contains("lock_and_count_admins"));
        assert!(update.contains("begin user update"));
        assert!(
            !update.contains("SELECT COUNT(*) AS n FROM users WHERE is_admin = true"),
            "demote must not COUNT admins outside the write transaction"
        );
        let delete = src
            .split("pub async fn delete_user")
            .nth(1)
            .and_then(|rest| rest.split("#[cfg(test)]").next())
            .expect("delete_user");
        assert!(delete.contains("lock_and_count_admins"));
        let lock_at = delete.find("lock_and_count_admins").expect("lock");
        let delete_sql_at = delete.find("DELETE FROM users WHERE id").expect("delete");
        assert!(
            lock_at < delete_sql_at,
            "last-admin lock must precede DELETE in the same transaction"
        );
    }

    #[tokio::test]
    async fn concurrent_last_admin_demotes_leave_one_admin() {
        use sea_orm::{
            ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection,
            Statement, TransactionTrait,
        };
        let Ok(url) = std::env::var("AUTH_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("PHANTASI_TEST_DATABASE_URL"))
        else {
            return;
        };
        let admin = Database::connect(&url).await.unwrap();
        let scope = format!("last_admin_{}", uuid::Uuid::new_v4().simple());
        admin
            .execute_unprepared(&format!("CREATE SCHEMA {scope}"))
            .await
            .unwrap();
        let connect = || {
            let mut options = ConnectOptions::new(url.clone());
            options
                .max_connections(1)
                .min_connections(1)
                .sqlx_logging(false)
                .set_schema_search_path(scope.clone());
            Database::connect(options)
        };
        let setup = connect().await.unwrap();
        setup
            .execute_unprepared(
                "CREATE TABLE users (id INTEGER PRIMARY KEY, is_admin BOOLEAN NOT NULL)",
            )
            .await
            .unwrap();
        setup
            .execute_unprepared("INSERT INTO users (id, is_admin) VALUES (1, true), (2, true)")
            .await
            .unwrap();
        let a = connect().await.unwrap();
        let b = connect().await.unwrap();
        let demote = |db: DatabaseConnection, target: i32| async move {
            let txn = db.begin().await.unwrap();
            let count = super::lock_and_count_admins(&txn).await.unwrap();
            if super::last_admin_mutation_blocked(true, count) {
                txn.rollback().await.ok();
                return false;
            }
            txn.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE users SET is_admin = false WHERE id = $1",
                [target.into()],
            ))
            .await
            .unwrap();
            txn.commit().await.unwrap();
            true
        };
        let (left, right) = tokio::join!(demote(a, 1), demote(b, 2));
        let remaining = setup
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT COUNT(*) FILTER (WHERE is_admin) AS n FROM users".to_string(),
            ))
            .await
            .unwrap()
            .and_then(|row| row.try_get::<i64>("", "n").ok())
            .unwrap_or(0);
        admin
            .execute_unprepared(&format!("DROP SCHEMA {scope} CASCADE"))
            .await
            .ok();
        assert!(
            left ^ right,
            "exactly one concurrent last-admin demote may commit"
        );
        assert_eq!(remaining, 1, "the last administrator must remain");
    }
}
