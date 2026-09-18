//! Tapp 共享基础模块
//!
//! 提供：
//! - `platform_cache` 再导出
//! - 速率限制 HTTP 适配
//! - 安装解析 / 批准权限校验 HTTP 适配

use axum::{Json, http::StatusCode};
use sea_orm::DatabaseConnection;
use serde_json::json;

use crate::error::HttpError;
use crate::middleware::auth::{Claims, ensure_current_admin_on};
use crate::models::entities::tapps;
use crate::services::permission_service::{TappPermission, UserRole};
use crate::services::tapp_ownership::{self, TappAccessError};
use crate::services::tapp_rate_limit::{self, RateLimitError};

// 平台数据缓存
// Domain implementation: `services::platform_cache`.

pub use crate::services::platform_cache::{
    get_available_platforms, get_cached_platform_data, validate_platform_name,
};
// acquire_platform_lock / invalidate_cached_platform_data: import from services::platform_cache
// (write paths use append_filtered_items / write_filtered_document).

// 速率限制器
// Domain implementation: `services::tapp_rate_limit`. This module only adapts
// errors to [`HttpError`] for HTTP handlers.

pub use crate::services::tapp_rate_limit::get_rate_limit_config;

fn rate_limit_http_error(err: RateLimitError) -> HttpError {
    match err {
        RateLimitError::Unavailable => HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": err.message(),
                "code": err.code(),
            })),
        )),
        RateLimitError::Exceeded {
            retry_after,
            limit,
            remaining,
        } => HttpError::from((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": err.message(),
                "code": err.code(),
                "retryAfter": retry_after,
                "limit": limit,
                "remaining": remaining,
            })),
        )),
    }
}

/// 检查速率限制
pub async fn check_rate_limit(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    operation: &str,
) -> Result<(), HttpError> {
    tapp_rate_limit::check_rate_limit(db, user_id, tapp_id, operation)
        .await
        .map_err(rate_limit_http_error)
}

/// Coarse anonymous limiter keyed by a one-way client-address fingerprint.
/// The source address itself is never persisted in the runtime registry.
pub async fn check_anonymous_rate_limit(
    db: &sea_orm::DatabaseConnection,
    client_ip: Option<&str>,
    tapp_id: &str,
) -> Result<(), HttpError> {
    tapp_rate_limit::check_anonymous_rate_limit(db, client_ip, tapp_id)
        .await
        .map_err(rate_limit_http_error)
}

pub async fn check_route_verify_rate_limit(
    db: &sea_orm::DatabaseConnection,
    tapp_id: &str,
    credential_key: &str,
    revision: &str,
) -> Result<(), HttpError> {
    tapp_rate_limit::check_route_verify_rate_limit(db, tapp_id, credential_key, revision)
        .await
        .map_err(rate_limit_http_error)
}

pub async fn check_inbound_anonymous_rate_limit(
    db: &sea_orm::DatabaseConnection,
    client_ip: Option<&str>,
    tapp_id: &str,
) -> Result<(), HttpError> {
    tapp_rate_limit::check_inbound_anonymous_rate_limit(db, client_ip, tapp_id)
        .await
        .map_err(rate_limit_http_error)
}

/// 获取速率限制状态（只读，不记录）
pub async fn get_rate_limit_status_for(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    operation: &str,
) -> Result<(u32, u32, u64), HttpError> {
    tapp_rate_limit::get_rate_limit_status_for(db, user_id, tapp_id, operation)
        .await
        .map_err(rate_limit_http_error)
}

pub async fn get_rate_limiter_active_count(
    db: &sea_orm::DatabaseConnection,
) -> Result<usize, HttpError> {
    tapp_rate_limit::get_rate_limiter_active_count(db)
        .await
        .map_err(rate_limit_http_error)
}

// 安全验证
// Domain logic lives in `services::tapp_ownership`. This module only adapts
// errors to [`HttpError`] for HTTP handlers.

fn tapp_access_http_error(err: TappAccessError) -> HttpError {
    HttpError::from(match err {
        TappAccessError::Database => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": err.message(),
                "code": "tapp_access_check_failed",
            })),
        ),
        TappAccessError::NoAdmin => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": err.error_code(),
                "code": "no_admin",
            })),
        ),
        TappAccessError::AccessDenied { .. } => (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": err.error_code(),
                "message": err.message(),
                "code": "access_denied",
            })),
        ),
        TappAccessError::PermissionNotGranted { .. } => (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": err.error_code(),
                "message": err.message(),
                "code": "TAPP_PERMISSION_NOT_GRANTED"
            })),
        ),
    })
}

/// 获取可选管理员用户 ID（带缓存）。
///
/// 全新数据库在 setup 创建站点 owner 前合法地没有管理员；公开读取路径应把它视为空集合，
/// 需要 owner 的控制面路径再通过 `get_admin_user_id` 提升为错误。
pub async fn find_admin_user_id(db: &DatabaseConnection) -> Result<Option<i32>, HttpError> {
    tapp_ownership::find_admin_user_id(db)
        .await
        .map_err(tapp_access_http_error)
}

/// 获取管理员用户 ID；仅用于确实要求站点 owner 已完成 setup 的路径。
pub async fn get_admin_user_id(db: &DatabaseConnection) -> Result<i32, HttpError> {
    tapp_ownership::get_admin_user_id(db)
        .await
        .map_err(tapp_access_http_error)
}

/// Resolve the exact installation record used to execute a Tapp for this subject.
///
/// When the subject has a private install of the same `tapp_id`, that record wins over the
/// site-owner public install. This helper only returns the install row; grants are rebound later from approved ∩ role.
pub async fn resolve_accessible_tapp(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
) -> Result<tapps::Model, HttpError> {
    tapp_ownership::resolve_accessible_tapp(db, user_id, tapp_id)
        .await
        .map_err(tapp_access_http_error)
}

/// 验证当前可访问安装的批准权限包含指定项。
///
/// 角色级授予只能说明调用者角色可以使用该能力；这里再检查安装的批准权限，
/// 防止客户端伪造 tapp_id 绕过 `approved_permissions`。
pub async fn verify_tapp_approved_permissions(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    permissions: &[TappPermission],
) -> Result<(), HttpError> {
    tapp_ownership::verify_tapp_approved_permissions(db, user_id, tapp_id, permissions)
        .await
        .map_err(tapp_access_http_error)
}

/// Priority for install selection: 0 = subject's private, 1 = site admin public, 2 = other.
/// Used by `resolve_accessible_tapp` (and declared-API paths that call it).
pub use tapp_ownership::tapp_owner_priority;

/// 从 Claims 解析 user_id
pub fn parse_user_id(claims: &Claims) -> Result<i32, HttpError> {
    claims.sub.parse().map_err(|_| {
        HttpError::from((
            StatusCode::UNAUTHORIZED,
            Json(AppError::public_json("Invalid user ID")),
        ))
    })
}

/// Resolve the current role used by Tapp capability filtering.
pub async fn current_tapp_user_role(db: &DatabaseConnection, claims: &Claims) -> UserRole {
    if claims.is_admin && ensure_current_admin_on(claims, db).await.is_ok() {
        UserRole::Admin
    } else if let Ok(user_id) = claims.sub.parse::<i32>() {
        if user_id < 0 {
            UserRole::Guest
        } else {
            UserRole::User
        }
    } else {
        UserRole::Guest
    }
}

// Prompt 安全验证
// Implementation lives in workspace crate `myriad-prompt-security` so services
// can share the same heuristics without depending on this API module.

/// 验证提示词安全性（后端层）
pub use myriad_prompt_security::validate_prompt_security;

#[cfg(test)]
mod tests {
    use super::tapp_owner_priority;

    #[test]
    fn private_install_precedes_same_id_admin_tapp() {
        // Subject's private install wins over site-owner public install.
        assert_eq!(tapp_owner_priority(42, 42, 1), 0);
        assert_eq!(tapp_owner_priority(1, 42, 1), 1);
        assert_eq!(tapp_owner_priority(99, 42, 1), 2);
        // Guests never match a private owner_id; public admin still ranks above unrelated.
        assert_eq!(tapp_owner_priority(1, -1, 1), 1);
        assert_eq!(tapp_owner_priority(99, -1, 1), 2);
    }
}
use myriad_error::AppError;
