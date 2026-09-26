use super::validate_tapp_id;
use axum::http::StatusCode;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr};

use crate::api::tapp_runtime::RuntimeGrantContext;
use crate::api::tapp_runtime::common as tapp_common;
use crate::error::HttpError;
use crate::middleware::auth::{Claims, current_admin_status, ensure_current_admin_on};
use crate::services::permission_service::{TappPermission, UserRole};
use crate::services::tapp_ownership::{self, TappAccessError};
use myriad_error::AppError;

/// 获取管理员用户 ID（委托给 tapp_runtime::common 的缓存版本）
pub(super) async fn get_admin_user_id(db: &DatabaseConnection) -> Result<i32, HttpError> {
    tapp_common::get_admin_user_id(db).await
}

pub(super) use crate::services::tapp_packages::ensure_tapp_install_allowed;

pub(super) async fn find_admin_user_id(db: &DatabaseConnection) -> Result<Option<i32>, HttpError> {
    tapp_common::find_admin_user_id(db).await
}

// Domain visibility row: services::tapp_ownership (path-stable re-export). Lifecycle lock is `lock_tapp_lifecycle`.
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
            TappAccessError::Database => HttpError(AppError::internal("Failed to find Tapp")),
            TappAccessError::NoAdmin
            | TappAccessError::AccessDenied { .. }
            | TappAccessError::PermissionNotGranted { .. } => {
                HttpError(AppError::internal("Failed to find Tapp"))
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

/// Live admin probe; "not an admin" is `Ok(false)`, a failed read is an error.
pub(super) async fn current_is_admin(
    claims: &Claims,
    db: &DatabaseConnection,
) -> Result<bool, HttpError> {
    current_admin_status(claims, db)
        .await
        .map_err(HttpError::from)
}

/// HTTP adapter: parse Claims.sub via domain subject rules.
pub(super) fn optional_authenticated_user_id(claims: Option<&Claims>) -> Option<i32> {
    claims.and_then(|claims| claims.subject_id().filter(|id| *id >= 0))
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
    TappStorageAccess, TappStorageAccessError, can_write_installation_settings,
};

/// Actor id for storage / grant binding: real users **and** signed guests
/// (negative `sub`). Differs from [`optional_authenticated_user_id`], which
/// drops guests for install-namespace lookups that only apply to durable users.
fn actor_subject_id(claims: &Claims) -> Option<i32> {
    claims.subject_id().filter(|id| *id != 0)
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
    .map_err(|err| {
        HttpError(
            AppError::from_status_u16(err.status_hint(), err.message())
                .with_message(err.message())
                .with_code(err.code()),
        )
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
pub(super) fn authorize_runtime_storage(
    claims: &Claims,
    grant: &RuntimeGrantContext,
    tapp_id: &str,
) -> Result<TappStorageAccess, HttpError> {
    require_runtime_storage_grant(grant, tapp_id, TappPermission::StorageRead)?;
    storage_access_from_runtime_grant(grant, claims)
}

pub(crate) fn authorize_runtime_storage_write(
    claims: &Claims,
    grant: &RuntimeGrantContext,
    tapp_id: &str,
) -> Result<TappStorageAccess, HttpError> {
    require_runtime_storage_grant(grant, tapp_id, TappPermission::StorageWrite)?;
    storage_access_from_runtime_grant(grant, claims)
}

pub(crate) fn installation_write_forbidden_error() -> HttpError {
    let err = TappStorageAccessError::InstallationReadOnly;
    HttpError::from((
        StatusCode::from_u16(err.status_hint()).unwrap_or(StatusCode::FORBIDDEN),
        axum::Json(serde_json::json!({
            "error": "Read-only installation resource",
            "message": err.message(),
            "code": err.code()
        })),
    ))
}

pub(super) async fn current_user_role(
    claims: &Claims,
    db: &DatabaseConnection,
) -> Result<UserRole, HttpError> {
    Ok(if current_is_admin(claims, db).await? {
        UserRole::Admin
    } else {
        UserRole::User
    })
}

// Domain install-namespace helpers: services::tapp_ownership (path-stable re-export).
#[cfg(test)]
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

#[cfg(test)]
mod tests {
    #[test]
    fn storage_subject_rejects_zero() {
        let src = include_str!("access.rs");
        let actor = src
            .split("fn actor_subject_id")
            .nth(1)
            .expect("actor_subject_id");
        assert!(actor.contains("id != 0"));
    }
}
