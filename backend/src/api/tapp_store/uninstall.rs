//! Tapp uninstall lifecycle: authorization, transactional cleanup and filesystem recovery.

use super::{
    ApiResponse, current_is_admin, find_admin_user_id, lock_tapp_lifecycle,
    remove_path_best_effort, require_current_admin, tapp_dir_for, validate_tapp_id,
};
use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter,
    Statement, TransactionTrait,
};
use serde::Deserialize;
use tokio::fs;

use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::models::entities::{tapp_storage, tapp_widgets, tapps};
use crate::services::tapp_lifecycle::{
    UninstallTarget, select_uninstall_target, uninstall_quarantine_dir_name,
};
use myriad_error::AppError;

/// 卸载 Tapp 查询参数
#[derive(Debug, Deserialize)]
pub(super) struct UninstallTappQuery {
    /// 是否保留应用数据（存储和设置），默认 false
    #[serde(default)]
    keep_data: bool,
}

/// 卸载 Tapp
///
/// 权限模型（private-first，与 list/detail 一致）：
/// 1. 调用者自己的安装（claims.user_id + tapp_id）优先；找到则直接卸载，无需 admin
/// 2. 否则若存在站点主公开安装，则 require_current_admin 后卸载公开行
/// 3. 否则 NOT_FOUND
///
/// 这样 private+public 双装时，非管理员可卸载私有副本且不误触公开安装的 403。
///
/// 查询参数：
/// - keep_data: bool - 是否保留应用数据，以便再次安装时恢复
pub(super) async fn uninstall_tapp(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
    Query(query): Query<UninstallTappQuery>,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    let user_id: i32 = claims
        .sub
        .parse()
        .map_err(|_| HttpError(AppError::unauthorized("Unauthorized")))?;
    validate_tapp_id(&tapp_id).map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let keep_data = query.keep_data;

    // 1. Prefer the caller's own install (private or site-owner public under their id).
    let own_tapp = tapps::Entity::find()
        .filter(tapps::Column::UserId.eq(user_id))
        .filter(tapps::Column::TappId.eq(&tapp_id))
        .one(&db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?;

    // 2. Site-owner public install only when own row is missing (other admins may
    // remove it; non-admins get 403 after target resolution).
    let public_tapp = if own_tapp.is_none() {
        if let Some(site_owner_id) = find_admin_user_id(&db).await? {
            if site_owner_id != user_id {
                tapps::Entity::find()
                    .filter(tapps::Column::UserId.eq(site_owner_id))
                    .filter(tapps::Column::TappId.eq(&tapp_id))
                    .one(&db)
                    .await
                    .map_err(|_| HttpError(AppError::internal("Database error")))?
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    match select_uninstall_target(own_tapp.is_some(), public_tapp.is_some()) {
        UninstallTarget::OwnInstall => {
            do_uninstall_tapp(&db, &own_tapp.expect("own install"), keep_data).await
        }
        UninstallTarget::PublicRequiresAdmin => {
            require_current_admin(&claims, &db).await?;
            do_uninstall_tapp(&db, &public_tapp.expect("public install"), keep_data).await
        }
        UninstallTarget::NotFound => Err(HttpError(AppError::not_found("Not found"))),
    }
}

/// Admin: uninstall one install row belonging to `user_id`.
pub(crate) async fn uninstall_tapp_for_user(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    keep_data: bool,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    validate_tapp_id(tapp_id).map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let tapp = tapps::Entity::find()
        .filter(tapps::Column::UserId.eq(user_id))
        .filter(tapps::Column::TappId.eq(tapp_id))
        .one(db)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))?
        .ok_or_else(|| HttpError(AppError::not_found("Tapp install not found")))?;
    do_uninstall_tapp(db, &tapp, keep_data).await
}

/// 执行卸载 Tapp 的具体操作
///
/// 参数：
/// - keep_data: 是否保留应用数据（存储和设置），以便再次安装时恢复
///
/// Filesystem quarantine is best-effort: a failed rename must not leave the
/// install row in place after DB cleanup has already been prepared. After a
/// successful commit the install row is gone even if leftover files remain.
async fn do_uninstall_tapp(
    db: &DatabaseConnection,
    tapp: &tapps::Model,
    keep_data: bool,
) -> Result<Json<ApiResponse<()>>, HttpError> {
    let user_id = tapp.user_id;
    let tapp_id = &tapp.tapp_id;
    let is_public_install = find_admin_user_id(db).await? == Some(user_id);

    let txn = db.begin().await.map_err(|error| {
        tracing::error!(tapp_id, user_id, %error, "Failed to begin uninstall transaction");
        HttpError(AppError::internal("Database error"))
    })?;
    lock_tapp_lifecycle(&txn, tapp_id).await.map_err(|error| {
        tracing::error!(tapp_id, user_id, %error, "Failed to acquire tapp lifecycle lock");
        HttpError(AppError::internal("Database error"))
    })?;
    let still_installed = tapps::Entity::find_by_id(tapp.id)
        .filter(tapps::Column::UserId.eq(user_id))
        .filter(tapps::Column::TappId.eq(tapp_id))
        .one(&txn)
        .await
        .map_err(|error| {
            tracing::error!(tapp_id, user_id, %error, "Failed to re-check tapp install under lock");
            HttpError(AppError::internal("Database error"))
        })?
        .is_some();
    if !still_installed {
        txn.rollback().await.ok();
        return Err(HttpError(AppError::not_found("Not found")));
    }

    crate::api::tapp_runtime::revoke_all_tapp_runtime_grants(db, user_id, tapp_id).await;

    // Prefer moving files out of the live path so a failed DB cleanup can restore
    // them. Rename failures (permissions, busy mount, EXDEV) must not abort
    // uninstall — DB cleanup still proceeds. Only a successful quarantine
    // rename gives post-commit cleanup exclusive ownership of a path.
    let tapp_dir = tapp_dir_for(user_id, tapp_id)
        .map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let quarantined_dir = if !tapp_dir.exists() {
        None
    } else if let Some(parent) = tapp_dir.parent() {
        let quarantine = parent.join(uninstall_quarantine_dir_name(
            tapp_id,
            &uuid::Uuid::new_v4().simple().to_string(),
        ));
        match fs::rename(&tapp_dir, &quarantine).await {
            Ok(()) => Some(quarantine),
            Err(error) => {
                tracing::error!(
                    tapp_id,
                    user_id,
                    from = %tapp_dir.display(),
                    to = %quarantine.display(),
                    kind = ?error.kind(),
                    %error,
                    "Failed to quarantine Tapp directory for uninstall; continuing with DB cleanup"
                );
                None
            }
        }
    } else {
        tracing::error!(
            tapp_id,
            path = %tapp_dir.display(),
            "Tapp directory has no parent; skipping quarantine rename"
        );
        None
    };

    let cleanup_result: Result<(), HttpError> = async {
        if is_public_install {
            txn.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"DELETE FROM tapp_widgets AS widget
                   WHERE widget.tapp_id = $1
                     AND (
                       widget.user_id = $2
                       OR widget.config->>'installationOwnerId' = $3
                       OR (
                         widget.config->>'source' = 'runtime'
                         AND NOT EXISTS (
                           SELECT 1 FROM tapps AS private_tapp
                           WHERE private_tapp.user_id = widget.user_id
                             AND private_tapp.tapp_id = widget.tapp_id
                             AND private_tapp.id <> $4
                         )
                       )
                     )"#,
                vec![
                    tapp_id.clone().into(),
                    user_id.into(),
                    user_id.to_string().into(),
                    tapp.id.into(),
                ],
            ))
            .await
            .map_err(|error| {
                tracing::error!(tapp_id, user_id, %error, "Failed to delete public-install widgets on uninstall");
                HttpError(AppError::internal("Database error"))
            })?;
        } else {
            tapp_widgets::Entity::delete_many()
                .filter(tapp_widgets::Column::UserId.eq(user_id))
                .filter(tapp_widgets::Column::TappId.eq(tapp_id))
                .exec(&txn)
                .await
                .map_err(|error| {
                    tracing::error!(tapp_id, user_id, %error, "Failed to delete private-install widgets on uninstall");
                    HttpError(AppError::internal("Database error"))
                })?;
        }

        if !keep_data {
            tapp_storage::Entity::delete_many()
                .filter(tapp_storage::Column::UserId.eq(user_id))
                .filter(tapp_storage::Column::TappId.eq(tapp_id))
                .exec(&txn)
                .await
                .map_err(|error| {
                    tracing::error!(tapp_id, user_id, %error, "Failed to delete tapp storage on uninstall");
                    HttpError(AppError::internal("Database error"))
                })?;
        }

        let (task_scope, task_values) = if is_public_install {
            (
                "tapp_id = $1 AND NOT EXISTS (
                    SELECT 1 FROM tapps AS remaining
                    WHERE remaining.user_id = tapp_scheduled_tasks.user_id
                      AND remaining.tapp_id = tapp_scheduled_tasks.tapp_id
                      AND remaining.id <> $2
                )",
                vec![tapp_id.clone().into(), tapp.id.into()],
            )
        } else {
            (
                "user_id = $1 AND tapp_id = $2",
                vec![user_id.into(), tapp_id.clone().into()],
            )
        };
        txn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "DELETE FROM tapp_task_executions WHERE scheduled_task_id IN (SELECT id FROM tapp_scheduled_tasks WHERE {task_scope})"
            ),
            task_values.clone(),
        ))
        .await
        .map_err(|error| {
            tracing::error!(tapp_id, user_id, %error, "Failed to delete task executions on uninstall");
            HttpError(AppError::internal("Database error"))
        })?;
        txn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!("DELETE FROM tapp_scheduled_tasks WHERE {task_scope}"),
            task_values,
        ))
        .await
        .map_err(|error| {
            tracing::error!(tapp_id, user_id, %error, "Failed to delete scheduled tasks on uninstall");
            HttpError(AppError::internal("Database error"))
        })?;
        tapps::Entity::delete_by_id(tapp.id)
            .exec(&txn)
            .await
            .map_err(|error| {
                tracing::error!(tapp_id, user_id, tapp_row_id = tapp.id, %error, "Failed to delete tapp install row on uninstall");
                HttpError(AppError::internal("Database error"))
            })?;
        txn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"DELETE FROM tapp_user_activities AS activity
               WHERE activity.tapp_id = $1
                 AND NOT EXISTS (
                   SELECT 1 FROM tapps AS installed
                   WHERE installed.user_id = activity.user_id
                     AND installed.tapp_id = activity.tapp_id
                 )"#,
            vec![tapp_id.clone().into()],
        ))
        .await
        .map_err(|error| {
            tracing::error!(tapp_id, user_id, %error, "Failed to prune orphan activities on uninstall");
            HttpError(AppError::internal("Database error"))
        })?;
        txn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"DELETE FROM tapp_runtime_registry AS registry
               WHERE registry.tapp_id = $1
                 AND NOT EXISTS (
                   SELECT 1 FROM tapps AS remaining
                   WHERE remaining.tapp_id = registry.tapp_id
                     AND (
                       remaining.user_id = registry.subject_id
                       OR remaining.user_id = registry.owner_id
                     )
                 )"#,
            vec![tapp_id.clone().into()],
        ))
        .await
        .map_err(|error| {
            tracing::error!(tapp_id, user_id, %error, "Failed to clean tapp_runtime_registry rows on uninstall");
            HttpError(AppError::internal("Database error"))
        })?;
        Ok(())
    }
    .await;

    if let Err(status) = cleanup_result {
        txn.rollback().await.ok();
        if let Some(quarantine) = quarantined_dir {
            if let Err(error) = fs::rename(&quarantine, &tapp_dir).await {
                tracing::error!(
                    tapp_id,
                    from = %quarantine.display(),
                    to = %tapp_dir.display(),
                    kind = ?error.kind(),
                    %error,
                    "Failed to restore Tapp directory after uninstall DB cleanup failure"
                );
            }
        }
        return Err(status);
    }
    if let Err(error) = txn.commit().await {
        tracing::error!(tapp_id, user_id, %error, "Failed to commit uninstall transaction");
        if let Some(quarantine) = quarantined_dir {
            if let Err(restore_error) = fs::rename(&quarantine, &tapp_dir).await {
                tracing::error!(
                    tapp_id,
                    from = %quarantine.display(),
                    to = %tapp_dir.display(),
                    kind = ?restore_error.kind(),
                    %restore_error,
                    "Failed to restore Tapp directory after uninstall commit failure"
                );
            }
        }
        return Err(HttpError(AppError::internal("Database error")));
    }

    // The lifecycle lock is released. Only our unique quarantine is still ours;
    // a reinstall may already own the shared live path and its staging directories.
    if let Some(cleanup_path) = quarantined_dir {
        if let Err(error) = remove_path_best_effort(&cleanup_path).await {
            tracing::warn!(
                tapp_id,
                user_id,
                path = %cleanup_path.display(),
                kind = ?error.kind(),
                %error,
                "Failed to remove uninstalled Tapp files"
            );
        }
    }
    crate::api::tapp_runtime::invalidate_tapp_apis_cache(tapp_id).await;

    Ok(Json(ApiResponse::success(())))
}

/// Owner lookup errors must fail closed; absence (`Ok(None)`) is not an error.
pub(crate) fn site_owner_id_for_prune<E>(lookup: Result<Option<i32>, E>) -> Result<Option<i32>, E> {
    lookup
}

/// Delete private Tapp installs owned by non-admin users who have been inactive
/// for `inactivity_days` (based on `COALESCE(last_login_at, last_seen_at, created_at)`).
///
/// Never touches site-owner / admin public installs. Used by the daily
/// background worker only (not the user-facing logout endpoint).
pub async fn prune_stale_private_tapps(
    db: &DatabaseConnection,
    inactivity_days: i64,
) -> Result<i32, String> {
    let days = inactivity_days.max(1);
    // Site owner id — same authority as SQL `$2`; lookup errors must fail closed.
    let site_owner_id = site_owner_id_for_prune(
        find_admin_user_id(db)
            .await
            .map_err(|error| error.0.to_string()),
    )?;

    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
            SELECT t.id
            FROM tapps t
            INNER JOIN users u ON u.id = t.user_id
            WHERE COALESCE(u.is_admin, false) = false
              AND COALESCE(u.is_owner, false) = false
              AND ($2::int IS NULL OR t.user_id <> $2)
              AND COALESCE(u.last_login_at, u.last_seen_at, u.created_at)
                  < NOW() - make_interval(days => $1::int)
            "#,
            [
                (days as i32).into(),
                match site_owner_id {
                    Some(id) => id.into(),
                    None => sea_orm::Value::Int(None),
                },
            ],
        ))
        .await
        .map_err(|e| e.to_string())?;

    let mut deleted = 0i32;
    for row in rows {
        let row_id: i32 = row.try_get("", "id").map_err(|e| e.to_string())?;
        let Some(model) = tapps::Entity::find_by_id(row_id)
            .one(db)
            .await
            .map_err(|e| e.to_string())?
        else {
            continue;
        };
        // Re-check: never uninstall site-owner rows
        if site_owner_id == Some(model.user_id) {
            continue;
        }
        match do_uninstall_tapp(db, &model, false).await {
            Ok(_) => deleted += 1,
            Err(HttpError(err)) => {
                tracing::warn!(
                    tapp_id = %model.tapp_id,
                    user_id = model.user_id,
                    error = %err,
                    "Failed to prune stale private Tapp install"
                );
            }
        }
    }
    Ok(deleted)
}

/// Logout / manual cleanup endpoint.
///
/// Behavior is driven by site config (`tapp_private_install_cleanup`):
/// - `logout`: wipe the caller's private installs immediately
/// - `inactivity` (default): no-op here — global prune runs only in the daily
///   background worker so logout cannot delete other users' installs or stall
pub(super) async fn cleanup_temporary_tapps(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<ApiResponse<i32>>, HttpError> {
    let user_id: i32 = claims
        .sub
        .parse()
        .map_err(|_| HttpError(AppError::unauthorized("Unauthorized")))?;

    let mode = {
        let cfg = dynamic_config.read().await;
        cfg.tapp_private_install_cleanup.trim().to_ascii_lowercase()
    };

    if mode == "logout" {
        // Wipe this subject's private installs now.
        // Admins operate the public namespace and never have private temps here.
        if current_is_admin(&claims, &db).await {
            return Ok(Json(ApiResponse::success(0)));
        }
        let user_tapps = tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(user_id))
            .all(&db)
            .await
            .map_err(|_| HttpError(AppError::internal("Database error")))?;
        let mut deleted = 0i32;
        for tapp in &user_tapps {
            match do_uninstall_tapp(&db, tapp, false).await {
                Ok(_) => deleted += 1,
                Err(HttpError(err)) => {
                    tracing::warn!(
                        tapp_id = %tapp.tapp_id,
                        user_id = tapp.user_id,
                        error = %err,
                        "Failed to uninstall private Tapp on logout cleanup"
                    );
                }
            }
        }
        return Ok(Json(ApiResponse::success(deleted)));
    }

    // inactivity (default): rely on the daily worker; do not prune globally
    // from a user-facing logout call.
    Ok(Json(ApiResponse::success(0)))
}

#[cfg(test)]
mod tests {
    use super::site_owner_id_for_prune;

    #[test]
    fn prune_fails_closed_when_owner_lookup_errors() {
        let error: Result<Option<i32>, &str> = Err("database");
        assert!(site_owner_id_for_prune(error).is_err());
    }

    #[test]
    fn prune_allows_missing_owner_before_setup() {
        assert_eq!(site_owner_id_for_prune::<&str>(Ok(None)).unwrap(), None);
        assert_eq!(
            site_owner_id_for_prune::<&str>(Ok(Some(1))).unwrap(),
            Some(1)
        );
    }

    #[test]
    fn uninstall_does_not_sweep_other_installs_by_tapp_id() {
        let src = include_str!("uninstall.rs")
            .split("mod tests")
            .next()
            .expect("uninstall impl");
        assert!(
            src.contains("remaining.user_id = tapp_scheduled_tasks.user_id"),
            "public uninstall must leave other installs' scheduled tasks"
        );
        assert!(
            src.contains("remaining.user_id = registry.subject_id"),
            "registry cleanup must keep rows for remaining installs"
        );
        assert!(
            !src.contains("DELETE FROM tapp_runtime_registry WHERE tapp_id = $1"),
            "unscoped tapp_id registry delete would wipe other owners"
        );
    }
}
