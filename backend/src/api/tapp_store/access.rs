use super::validate_tapp_id;
use axum::http::StatusCode;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement,
};

use crate::api::tapp_runtime::common as tapp_common;
use crate::api::tapp_runtime::RuntimeGrantContext;
use crate::error::HttpError;
use crate::middleware::auth::{ensure_current_admin_on, Claims};
use crate::services::permission_service::{TappPermission, TappPermissionService, UserRole};
use crate::services::tapp_ownership::{self, TappAccessError};
use myriad_error::AppError;

/// 获取管理员用户 ID（委托给 tapp_runtime::common 的缓存版本）
pub(super) async fn get_admin_user_id(db: &DatabaseConnection) -> Result<i32, HttpError> {
    tapp_common::get_admin_user_id(db).await
}

/// Refuse new Tapp installs when an admin has locked the account.
pub(super) async fn ensure_tapp_install_allowed(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<(), HttpError> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT tapp_install_disabled FROM users WHERE id = $1",
            [user_id.into()],
        ))
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;
    let disabled = row
        .and_then(|row| row.try_get::<bool>("", "tapp_install_disabled").ok())
        .unwrap_or(false);
    if disabled {
        return Err(HttpError(AppError::forbidden(
            "Tapp installation is disabled for this account",
        )));
    }
    Ok(())
}

pub(super) async fn find_admin_user_id(db: &DatabaseConnection) -> Result<Option<i32>, HttpError> {
    tapp_common::find_admin_user_id(db).await
}

// Domain visibility + lifecycle: services::tapp_ownership (path-stable re-export).
pub(crate) use crate::services::tapp_ownership::VisibleTappInstallation;

/// HTTP adapter: validate tapp_id shape, then resolve the visible install.
pub(super) async fn find_visible_tapp(
    db: &DatabaseConnection,
    user_id: Option<i32>,
    tapp_id: &str,
) -> Result<Option<VisibleTappInstallation>, HttpError> {
    validate_tapp_id(tapp_id).map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    tapp_ownership::find_visible_tapp(db, user_id, tapp_id)
        .await
        .map_err(|err| match err {
            TappAccessError::Database => HttpError(AppError::internal("Database error")),
            TappAccessError::NoAdmin
            | TappAccessError::AccessDenied { .. }
            | TappAccessError::PermissionNotGranted { .. } => {
                HttpError(AppError::internal("Database error"))
            }
        })
}

/// Path-stable re-export of the domain lifecycle advisory lock.
pub(super) async fn lock_tapp_lifecycle(
    db: &impl ConnectionTrait,
    tapp_id: &str,
) -> Result<(), DbErr> {
    tapp_ownership::lock_tapp_lifecycle(db, tapp_id).await
}

pub(super) async fn current_is_admin(claims: &Claims, db: &DatabaseConnection) -> bool {
    ensure_current_admin_on(claims, db).await.is_ok()
}

/// HTTP adapter: parse Claims.sub via domain subject rules.
pub(super) fn optional_authenticated_user_id(claims: Option<&Claims>) -> Option<i32> {
    claims.and_then(|claims| tapp_ownership::parse_authenticated_subject_id(&claims.sub))
}

fn require_runtime_storage_grant(
    grant: &RuntimeGrantContext,
    tapp_id: &str,
    permission: TappPermission,
) -> Result<(), HttpError> {
    grant.require_tapp_id(tapp_id)?;
    grant.require(permission)?;
    Ok(())
}

// Domain identity type: services::tapp_storage (path-stable re-export).
pub(crate) use crate::services::tapp_storage::{
    can_write_installation_settings, TappStorageAccess, TappStorageAccessError,
};

/// Actor id for storage / grant binding: real users **and** signed guests
/// (negative `sub`). Differs from [`optional_authenticated_user_id`], which
/// drops guests for install-namespace lookups that only apply to durable users.
fn actor_subject_id(claims: &Claims) -> Option<i32> {
    claims.sub.parse::<i32>().ok()
}

/// HTTP adapter: resolve [`TappStorageAccess`] from a Runtime Grant + Claims.
///
/// Subject may be a signed guest session id (negative); private storage is
/// namespaced under that id. Grant subject must match Claims.sub.
pub(crate) fn storage_access_from_runtime_grant(
    grant: &RuntimeGrantContext,
    claims: &Claims,
) -> Result<TappStorageAccess, HttpError> {
    TappStorageAccess::from_grant_and_subject(
        grant.owner_id(),
        grant.subject_id(),
        actor_subject_id(claims),
    )
    .map_err(|err| match err {
        TappStorageAccessError::Unauthenticated => {
            HttpError(AppError::unauthorized("Unauthorized"))
        }
        TappStorageAccessError::SubjectMismatch | TappStorageAccessError::InstallationReadOnly => {
            HttpError(AppError::forbidden("Forbidden"))
        }
    })
}

/// Back-compat alias used across runtime/store modules.
impl TappStorageAccess {
    pub fn from_runtime_grant(
        grant: &RuntimeGrantContext,
        claims: &Claims,
    ) -> Result<Self, HttpError> {
        storage_access_from_runtime_grant(grant, claims)
    }
}

/// Authorize sandbox storage for a Runtime-Grant route.
pub(super) async fn authorize_runtime_storage(
    db: &DatabaseConnection,
    claims: &Claims,
    grant: &RuntimeGrantContext,
    tapp_id: &str,
    dynamic_config: &std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>,
) -> Result<TappStorageAccess, HttpError> {
    require_runtime_storage_grant(grant, tapp_id, TappPermission::StorageRead)?;
    authorize_tapp_permission(db, claims, tapp_id, TappPermission::StorageRead, dynamic_config).await?;
    storage_access_from_runtime_grant(grant, claims)
}

pub(crate) async fn authorize_runtime_storage_write(
    db: &DatabaseConnection,
    claims: &Claims,
    grant: &RuntimeGrantContext,
    tapp_id: &str,
    dynamic_config: &std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>,
) -> Result<TappStorageAccess, HttpError> {
    require_runtime_storage_grant(grant, tapp_id, TappPermission::StorageWrite)?;
    authorize_tapp_permission(db, claims, tapp_id, TappPermission::StorageWrite, dynamic_config).await?;
    storage_access_from_runtime_grant(grant, claims)
}

pub(crate) fn installation_write_forbidden_error() -> HttpError {
    HttpError::from((
        StatusCode::FORBIDDEN,
        axum::Json(serde_json::json!({
            "error": "Read-only installation resource",
            "message": TappStorageAccessError::InstallationReadOnly.message(),
            "code": TappStorageAccessError::InstallationReadOnly.code()
        })),
    ))
}

pub(super) async fn current_user_role(claims: &Claims, db: &DatabaseConnection) -> UserRole {
    if current_is_admin(claims, db).await {
        UserRole::Admin
    } else {
        UserRole::User
    }
}

// Domain install-namespace helpers: services::tapp_ownership (path-stable re-export).
pub(crate) use crate::services::tapp_ownership::{
    canonical_installation_owner_id, installation_conflict_owner_ids,
};

pub(super) async fn require_current_admin(
    claims: &Claims,
    db: &DatabaseConnection,
) -> Result<(), HttpError> {
    ensure_current_admin_on(claims, db)
        .await
        .map_err(|(status, body)| HttpError::from((status, body)))
}

pub(super) async fn filter_install_permissions(
    dynamic_config: &tokio::sync::RwLock<crate::config::DynamicConfig>,
    role: UserRole,
    permissions: Vec<String>,
) -> Result<Vec<String>, HttpError> {
    let config = dynamic_config.read().await;
    let granted = TappPermissionService::filter_permissions_for_role(&config, role, &permissions)
        .map_err(|error| {
        HttpError::from((
            StatusCode::CONFLICT,
            axum::Json(serde_json::json!({
                "error": error.message(),
                "code": error.code(),
            })),
        ))
    })?;
    drop(config);
    Ok(granted)
}

pub(super) async fn authorize_tapp_permission(
    db: &DatabaseConnection,
    claims: &Claims,
    tapp_id: &str,
    permission: TappPermission,
    dynamic_config: &std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>,
) -> Result<i32, HttpError> {
    tapp_common::authorize_tapp_permission(db, claims, tapp_id, permission, dynamic_config).await
}
