//! Shared installation transaction, staging and activation.
use super::{
    access::*, package_files::*, prepared_package::*, widgets::reconcile_manifest_widgets,
};
use crate::config::DynamicConfig;
use crate::models::entities::tapps;
use crate::services::permission_service::UserRole;
use crate::services::tapp_catalog::TappListItem;
use crate::services::tapp_install::*;
use crate::services::tapp_ownership::{
    canonical_installation_owner_id, installation_conflict_owner_ids, lock_tapp_lifecycle,
};
use chrono::Utc;
use myriad_error::AppError;
use myriad_tapp_contract::manifest::TappManifest;
use once_cell::sync::Lazy;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, DatabaseConnection, EntityTrait,
    QueryFilter, Set, TransactionTrait,
};
use std::{sync::Arc, time::Duration as StdDuration};
use tokio::sync::{OwnedSemaphorePermit, RwLock, Semaphore};
/// Global install concurrency gate. Bounds simultaneous archive
/// buffers + extract work so handlers do not hold full zip clones unboundedly.
static INSTALL_SEMAPHORE: Lazy<Arc<Semaphore>> =
    Lazy::new(|| Arc::new(Semaphore::new(MAX_CONCURRENT_INSTALLS)));

/// Acquire an install slot, or fail 503 if the wait times out (overloaded).
pub(crate) async fn acquire_install_permit() -> Result<OwnedSemaphorePermit, AppError> {
    match tokio::time::timeout(
        StdDuration::from_secs(INSTALL_ACQUIRE_TIMEOUT_SECS),
        INSTALL_SEMAPHORE.clone().acquire_owned(),
    )
    .await
    {
        Ok(Ok(permit)) => Ok(permit),
        Ok(Err(e)) => {
            tracing::error!("Tapp install semaphore closed: {:?}", e);
            Err(AppError::from_status_u16(
                500,
                "Failed to schedule Tapp install",
            ))
        }
        Err(_) => {
            tracing::warn!(
                permits = MAX_CONCURRENT_INSTALLS,
                timeout_secs = INSTALL_ACQUIRE_TIMEOUT_SECS,
                "Tapp install concurrency limit reached; returning 503"
            );
            Err(AppError::from_status_u16(
                install_overloaded_status(),
                install_overloaded_message(),
            ))
        }
    }
}

/// Install a package an Agent generated through the same core as a direct
/// install: install gate and permit, store policy, full manifest validation,
/// conflict check, canonical installation owner and staged activation.
/// A second install path would skip all of that and write the live directory.
pub(crate) async fn install_generated(
    db: &DatabaseConnection,
    user_id: i32,
    manifest: TappManifest,
    modules: std::collections::HashMap<String, String>,
) -> Result<(), AppError> {
    ensure_tapp_install_allowed(db, user_id).await?;
    let is_current_admin = crate::services::agent::user_is_current_admin(db, user_id)
        .await
        .map_err(|error| AppError::internal(error))?;
    let role = if is_current_admin {
        UserRole::Admin
    } else {
        UserRole::User
    };
    let package = PreparedTappPackage::from_resources(
        manifest,
        PreparedTappResources {
            modules,
            ..Default::default()
        },
    );
    install_prepared_package(
        db,
        &crate::GLOBAL_DYNAMIC_CONFIG,
        user_id,
        role,
        is_current_admin,
        package,
        None,
        // Generated code may be steered by prompt injection; hosts that can
        // receive viewer data are left for the user to approve explicitly.
        None,
        false,
        None,
    )
    .await
    .map(|_| ())
}

fn declared_remote_media(manifest: &TappManifest) -> &[String] {
    manifest.remote_media.as_deref().unwrap_or(&[])
}

/// 409 body for an existing install. Carries the metadata the overwrite prompt
/// needs (both versions plus which declared permissions and remote media hosts
/// are new) — no secrets.
fn install_conflict_error(manifest: &TappManifest, existing: &tapps::Model) -> AppError {
    let previous: Vec<String> =
        serde_json::from_value(existing.approved_permissions.clone()).unwrap_or_default();
    let new_permissions: Vec<String> = manifest
        .permissions
        .iter()
        .filter(|permission| !previous.iter().any(|approved| approved == *permission))
        .cloned()
        .collect();
    let previous_hosts: Vec<String> =
        serde_json::from_value(existing.approved_remote_media.clone()).unwrap_or_default();
    let new_remote_media: Vec<String> = declared_remote_media(manifest)
        .iter()
        .filter(|host| !previous_hosts.iter().any(|approved| approved == *host))
        .cloned()
        .collect();
    myriad_error::AppError::conflict("Tapp already installed").with_details(serde_json::json!({
        "tappId": manifest.id.clone(),
        "name": manifest.name.clone(),
        "installedVersion": existing.version.clone(),
        "incomingVersion": manifest.version.clone(),
        "newPermissions": new_permissions,
        "newRemoteMedia": new_remote_media,
    }))
}

pub(crate) async fn install_prepared_package(
    db: &DatabaseConnection,
    dynamic_config: &RwLock<DynamicConfig>,
    user_id: i32,
    role: UserRole,
    is_current_admin: bool,
    package: PreparedTappPackage,
    permissions: Option<Vec<String>>,
    remote_media: Option<Vec<String>>,
    overwrite: bool,
    install_permit: Option<OwnedSemaphorePermit>,
) -> Result<TappListItem, AppError> {
    let _install_permit = match install_permit {
        Some(permit) => permit,
        None => acquire_install_permit().await?,
    };
    super::ensure_permissions_allowed(&package.manifest.permissions).await?;
    package.validate_for_install(None)?;
    let manifest = package.manifest.clone();

    // 检查是否已安装
    let admin_id = get_admin_user_id(db).await?;
    // Every current administrator operates the one canonical public namespace;
    // the actor account is not used as a second public installation owner.
    let installation_owner_id = canonical_installation_owner_id(role, user_id, admin_id);
    let conflict_owner_ids = installation_conflict_owner_ids(role, user_id, admin_id);
    let existing_query = tapps::Entity::find()
        .filter(tapps::Column::TappId.eq(&manifest.id))
        .filter(tapps::Column::UserId.is_in(conflict_owner_ids.clone()));
    let existing = existing_query.one(db).await.map_err(|error| {
        log_install_failure(
            "conflict_recheck_pre",
            &manifest.id,
            user_id,
            installation_owner_id,
            None,
            &error,
        );
        AppError::from_status_u16(500, "Database error")
    })?;

    if let Some(existing) = existing.as_ref()
        && !overwrite
    {
        return Err(install_conflict_error(&manifest, existing));
    }

    // 所有资源先写入同文件系统的 staging 目录；校验通过后再原子切换。
    let final_tapp_dir = tapp_dir_for(installation_owner_id, &manifest.id)
        .map_err(|error| AppError::from_status_u16(400, error))?;
    let stage = TappDirStage::create(&final_tapp_dir)
        .await
        .map_err(|error| {
            log_tapp_filesystem_access(&final_tapp_dir, &error);
            log_install_failure(
                "TappDirStage::create",
                &manifest.id,
                user_id,
                installation_owner_id,
                Some(&final_tapp_dir),
                &error,
            );
            tapp_filesystem_error("Failed to create Tapp staging directory", &error)
        })?;
    let tapp_dir = stage.path();
    let now = Utc::now().fixed_offset();
    package
        .stage_into(
            tapp_dir,
            now,
            PackageStageContext {
                user_id,
                installation_owner_id,
            },
        )
        .await?;
    let txn = db.begin().await.map_err(|error| {
        log_install_failure(
            "txn.begin",
            &manifest.id,
            user_id,
            installation_owner_id,
            None,
            &error,
        );
        AppError::from_status_u16(500, "Database error")
    })?;
    lock_tapp_lifecycle(&txn, &manifest.id)
        .await
        .map_err(|error| {
            log_install_failure(
                "lock_tapp_lifecycle",
                &manifest.id,
                user_id,
                installation_owner_id,
                None,
                &error,
            );
            AppError::from_status_u16(500, "Database error")
        })?;
    let existing_tx = tapps::Entity::find()
        .filter(tapps::Column::TappId.eq(&manifest.id))
        .filter(tapps::Column::UserId.is_in(conflict_owner_ids))
        .one(&txn)
        .await
        .map_err(|error| {
            log_install_failure(
                "conflict_recheck",
                &manifest.id,
                user_id,
                installation_owner_id,
                None,
                &error,
            );
            AppError::from_status_u16(500, "Database error")
        })?;
    if let Some(existing_tx) = &existing_tx {
        if !overwrite {
            txn.rollback().await.ok();
            return Err(install_conflict_error(&manifest, existing_tx));
        }
    }

    // Approved = pure domain selection; granted = role-config filter (async).
    // Overwrite keeps the previous approvals and only adds accepted new ones;
    // a fresh install defaults to the full declared set when unspecified.
    // Remote media hosts are never approved by default (see tapp_install).
    let requested = permissions.as_deref().unwrap_or(&[]);
    let (approved, approved_remote_media) = match &existing_tx {
        Some(existing_tx) => {
            let previous: Vec<String> =
                serde_json::from_value(existing_tx.approved_permissions.clone())
                    .unwrap_or_default();
            let previous_hosts: Vec<String> =
                serde_json::from_value(existing_tx.approved_remote_media.clone())
                    .unwrap_or_default();
            (
                select_overwrite_approved_permissions(&manifest.permissions, requested, &previous),
                select_overwrite_approved_remote_media(
                    declared_remote_media(&manifest),
                    remote_media.as_deref().unwrap_or(&[]),
                    &previous_hosts,
                ),
            )
        }
        None => (
            select_install_approved_permissions(&manifest.permissions, requested),
            select_install_approved_remote_media(
                declared_remote_media(&manifest),
                remote_media.as_deref(),
            ),
        ),
    };
    if let Err(error) = filter_install_permissions(dynamic_config, role, approved.clone()).await {
        txn.rollback().await.ok();
        return Err(error);
    }

    // Clean leftover live/uninstall artifacts under the lifecycle lock.
    // Staging dirs are skipped: they may belong to a concurrent install.
    // Overwrite keeps the live install; `stage.activate` quarantines it.
    if existing_tx.is_none() {
        cleanup_reinstall_orphans(
            &final_tapp_dir,
            &manifest.id,
            installation_owner_id,
            user_id,
            Some(stage.path()),
        );
    }

    let activated = match stage.activate(&final_tapp_dir).await {
        Ok(activated) => activated,
        Err(error) => {
            txn.rollback().await.ok();
            log_tapp_filesystem_access(&final_tapp_dir, &error);
            log_install_failure(
                "stage.activate",
                &manifest.id,
                user_id,
                installation_owner_id,
                Some(&final_tapp_dir),
                &error,
            );
            return Err(tapp_filesystem_error(
                "Failed to activate staged Tapp",
                &error,
            ));
        }
    };

    // Overwrite of an existing install: update in place, preserving live status
    // and user data (storage is keyed by user+tapp and is never touched here).
    if let Some(existing_tx) = existing_tx {
        let persist = build_update_install_persist(
            &manifest,
            &approved,
            &approved_remote_media,
            &final_tapp_dir,
            now,
        )
        .map_err(|error| AppError::from_status_u16(500, error))?;
        let mut active: tapps::ActiveModel = existing_tx.clone().into();
        active.name = Set(persist.name);
        active.version = Set(persist.version);
        active.description = Set(persist.description);
        active.author = Set(persist.author);
        active.icon = Set(persist.icon);
        active.theme_color = Set(persist.theme_color);
        active.manifest = Set(persist.manifest);
        active.approved_permissions = Set(persist.approved_permissions);
        active.approved_remote_media = Set(persist.approved_remote_media);
        // A successful overwrite is explicit re-authorization.
        active.needs_reauthorization = Set(persist.needs_reauthorization);
        active.code_path = Set(persist.code_path);
        active.updated_at = Set(persist.updated_at);
        let result = match active.update(&txn).await {
            Ok(result) => result,
            Err(error) => {
                txn.rollback().await.ok();
                activated.rollback().await;
                log_install_failure(
                    "overwrite_update",
                    &manifest.id,
                    user_id,
                    installation_owner_id,
                    Some(&final_tapp_dir),
                    &error,
                );
                return Err(AppError::from_status_u16(500, "Database error"));
            }
        };
        if let Err(err) =
            reconcile_manifest_widgets(&txn, installation_owner_id, &manifest.id, &manifest).await
        {
            txn.rollback().await.ok();
            activated.rollback().await;
            log_install_failure(
                "reconcile_manifest_widgets",
                &manifest.id,
                user_id,
                installation_owner_id,
                Some(&final_tapp_dir),
                &format!("status={}", err.status_u16()),
            );
            return Err(err);
        }
        if let Err(error) = txn.commit().await {
            activated.rollback_after_commit_error().await;
            log_install_failure(
                "txn.commit",
                &manifest.id,
                user_id,
                installation_owner_id,
                Some(&final_tapp_dir),
                &error,
            );
            return Err(AppError::from_status_u16(500, "Database error"));
        }
        activated.commit().await;
        // Code or approved permissions changed; drop runtime grants.
        super::runtime::revoke_installation(db, installation_owner_id, &manifest.id).await;
        crate::services::tapp_declared_api::invalidate_tapp_apis_cache(&manifest.id).await;
        let is_site_owner = is_public_installation_namespace(installation_owner_id, admin_id);
        return Ok(crate::services::tapp_catalog::update_response_list_item(
            result,
            is_site_owner,
        ));
    }

    // Column projection (paths, Running default, permission JSON) is pure domain.
    let persist = build_new_install_persist(
        &manifest,
        installation_owner_id,
        &approved,
        &approved_remote_media,
        &final_tapp_dir,
        now,
    )
    .map_err(|error| AppError::from_status_u16(500, error))?;
    let tapp = tapps::ActiveModel {
        id: NotSet,
        tapp_id: Set(persist.tapp_id),
        user_id: Set(persist.user_id),
        name: Set(persist.name),
        version: Set(persist.version),
        description: Set(persist.description),
        author: Set(persist.author),
        icon: Set(persist.icon),
        theme_color: Set(persist.theme_color),
        manifest: Set(persist.manifest),
        // start_running is always true for new installs (public widgets render immediately).
        status: Set(if persist.start_running {
            tapps::TappStatus::Running
        } else {
            tapps::TappStatus::Installed
        }),
        approved_permissions: Set(persist.approved_permissions),
        approved_remote_media: Set(persist.approved_remote_media),
        needs_reauthorization: Set(persist.needs_reauthorization),
        file_path: Set(persist.file_path),
        code_path: Set(persist.code_path),
        installed_at: Set(persist.installed_at),
        last_run_at: Set(Some(persist.last_run_at)),
        updated_at: Set(persist.updated_at),
        error_message: Set(None),
        // New installs default visibility to everyone.
        visibility: Set(crate::services::tapp_ownership::TAPP_VISIBILITY_ALL.to_string()),
    };

    let result = match tapp.insert(&txn).await {
        Ok(result) => result,
        Err(error) => {
            txn.rollback().await.ok();
            activated.rollback().await;
            log_install_failure(
                "insert",
                &manifest.id,
                user_id,
                installation_owner_id,
                Some(&final_tapp_dir),
                &error,
            );
            return Err(AppError::from_status_u16(500, "Database error"));
        }
    };
    if let Err(err) =
        reconcile_manifest_widgets(&txn, installation_owner_id, &manifest.id, &manifest).await
    {
        txn.rollback().await.ok();
        activated.rollback().await;
        log_install_failure(
            "reconcile_manifest_widgets",
            &manifest.id,
            user_id,
            installation_owner_id,
            Some(&final_tapp_dir),
            &format!("status={}", err.status_u16()),
        );
        return Err(err);
    }
    if let Err(error) = txn.commit().await {
        // COMMIT errors are ambiguous: preserve the candidate generation so
        // startup recovery can follow the database's actual committed state.
        activated.rollback_after_commit_error().await;
        log_install_failure(
            "txn.commit",
            &manifest.id,
            user_id,
            installation_owner_id,
            Some(&final_tapp_dir),
            &error,
        );
        return Err(AppError::from_status_u16(500, "Database error"));
    }
    activated.commit().await;
    // Runtime grants and declared-API cache are keyed by tapp_id, not owner.
    super::runtime::revoke_installation(db, installation_owner_id, &manifest.id).await;
    crate::services::tapp_declared_api::invalidate_tapp_apis_cache(&manifest.id).await;

    // List projection: services::tapp_catalog (install contract forces status=installed).
    Ok(crate::services::tapp_catalog::install_response_list_item(
        result,
        is_current_admin,
    ))
}

/// Update an existing installation while preserving status and user data.
pub(crate) async fn update_prepared_package(
    db: &DatabaseConnection,
    dynamic_config: &RwLock<DynamicConfig>,
    user_id: i32,
    role: UserRole,
    target: UpdateTarget,
    tapp_id: String,
    package: PreparedTappPackage,
    permissions: Option<Vec<String>>,
    remote_media: Option<Vec<String>>,
) -> Result<TappListItem, AppError> {
    let UpdateTarget {
        existing: existing_tapp,
        owner_id: target_owner_id,
        is_site_owner,
    } = target;
    super::ensure_permissions_allowed(&package.manifest.permissions).await?;
    package.validate_for_install(Some(&tapp_id))?;
    let manifest = package.manifest.clone();

    let final_tapp_dir = tapp_dir_for(target_owner_id, &tapp_id)
        .map_err(|error| AppError::from_status_u16(400, error))?;
    let stage = TappDirStage::create(&final_tapp_dir)
        .await
        .map_err(|error| {
            log_tapp_filesystem_access(&final_tapp_dir, &error);
            log_install_failure(
                "TappDirStage::create",
                &tapp_id,
                user_id,
                target_owner_id,
                Some(&final_tapp_dir),
                &error,
            );
            tapp_filesystem_error("Failed to create Tapp update staging directory", &error)
        })?;
    let tapp_dir = stage.path();
    let now = Utc::now().fixed_offset();
    package
        .stage_into(
            tapp_dir,
            now,
            PackageStageContext {
                user_id,
                installation_owner_id: target_owner_id,
            },
        )
        .await?;
    let txn = db
        .begin()
        .await
        .map_err(|_| AppError::from_status_u16(500, "Failed to begin update transaction"))?;
    lock_tapp_lifecycle(&txn, &tapp_id)
        .await
        .map_err(|_| AppError::from_status_u16(500, "Failed to lock Tapp lifecycle"))?;
    let existing_tapp = tapps::Entity::find_by_id(existing_tapp.id)
        .filter(tapps::Column::UserId.eq(target_owner_id))
        .filter(tapps::Column::TappId.eq(&tapp_id))
        .one(&txn)
        .await
        .map_err(|_| AppError::from_status_u16(500, "Database error"))?
        .ok_or_else(|| AppError::from_status_u16(404, "Tapp not installed"))?;

    // Approved = pure domain selection; granted = role-config filter (async).
    let previous_approved: Vec<String> =
        serde_json::from_value(existing_tapp.approved_permissions.clone()).unwrap_or_default();
    let approved = select_update_approved_permissions(
        &manifest.permissions,
        permissions.as_deref(),
        &previous_approved,
    );
    let previous_hosts: Vec<String> =
        serde_json::from_value(existing_tapp.approved_remote_media.clone()).unwrap_or_default();
    let approved_remote_media = select_update_approved_remote_media(
        declared_remote_media(&manifest),
        remote_media.as_deref(),
        &previous_hosts,
    );
    filter_install_permissions(dynamic_config, role, approved.clone()).await?;

    let activated = match stage.activate(&final_tapp_dir).await {
        Ok(activated) => activated,
        Err(error) => {
            txn.rollback().await.ok();
            log_tapp_filesystem_access(&final_tapp_dir, &error);
            tracing::error!(
                step = "stage.activate",
                tapp_id = %tapp_id,
                user_id,
                owner_id = target_owner_id,
                path = %final_tapp_dir.display(),
                kind = ?error.kind(),
                %error,
                "Tapp update activate failed"
            );
            return Err(tapp_filesystem_error(
                "Failed to activate staged Tapp update",
                &error,
            ));
        }
    };
    // Column projection is pure domain; ActiveModel mapping stays here.
    let persist = build_update_install_persist(
        &manifest,
        &approved,
        &approved_remote_media,
        &final_tapp_dir,
        now,
    )
    .map_err(|error| AppError::from_status_u16(500, error))?;
    let mut active: tapps::ActiveModel = existing_tapp.clone().into();
    active.name = Set(persist.name);
    active.version = Set(persist.version);
    active.description = Set(persist.description);
    active.author = Set(persist.author);
    active.icon = Set(persist.icon);
    active.theme_color = Set(persist.theme_color);
    active.manifest = Set(persist.manifest);
    active.approved_permissions = Set(persist.approved_permissions);
    active.approved_remote_media = Set(persist.approved_remote_media);
    // Successful update is explicit re-authorization; persist always clears the flag.
    active.needs_reauthorization = Set(persist.needs_reauthorization);
    active.code_path = Set(persist.code_path);
    active.updated_at = Set(persist.updated_at);

    let result = match active.update(&txn).await {
        Ok(result) => result,
        Err(error) => {
            txn.rollback().await.ok();
            activated.rollback().await;
            tracing::error!(error = %error, "Tapp update database error");
            return Err(AppError::from_status_u16(500, "Database error"));
        }
    };
    if let Err(err) = reconcile_manifest_widgets(&txn, target_owner_id, &tapp_id, &manifest).await {
        txn.rollback().await.ok();
        activated.rollback().await;
        return Err(err);
    }
    if txn.commit().await.is_err() {
        activated.rollback_after_commit_error().await;
        return Err(AppError::from_status_u16(
            500,
            "Failed to commit Tapp update",
        ));
    }
    activated.commit().await;

    // Code or approved permissions may have changed; drop runtime grants.
    super::runtime::revoke_installation(db, target_owner_id, &tapp_id).await;

    // manifest 已更新，清除 API 解析缓存
    crate::services::tapp_declared_api::invalidate_tapp_apis_cache(&tapp_id).await;

    tracing::info!(
        "[TAPP] Updated Tapp {} from {} to {} for user {}",
        tapp_id,
        existing_tapp.version,
        result.version,
        user_id
    );

    Ok(crate::services::tapp_catalog::update_response_list_item(
        result,
        is_site_owner,
    ))
}

/// Replace the approved `remoteMedia` hosts of an existing install without a
/// package change. Approved = declared ∩ `hosts`; the CSP follows the next detail
/// load. Same owner resolution and install gate as an update.
pub(crate) async fn set_approved_remote_media(
    db: &DatabaseConnection,
    user_id: i32,
    role: UserRole,
    tapp_id: &str,
    hosts: &[String],
) -> Result<Vec<String>, AppError> {
    ensure_tapp_install_allowed(db, user_id).await?;
    let target = resolve_update_target(db, user_id, role, tapp_id).await?;
    let txn = db
        .begin()
        .await
        .map_err(|_| AppError::internal("Database error"))?;
    lock_tapp_lifecycle(&txn, tapp_id)
        .await
        .map_err(|_| AppError::internal("Database error"))?;
    let existing = tapps::Entity::find_by_id(target.existing.id)
        .filter(tapps::Column::UserId.eq(target.owner_id))
        .filter(tapps::Column::TappId.eq(tapp_id))
        .one(&txn)
        .await
        .map_err(|_| AppError::internal("Database error"))?
        .ok_or_else(|| AppError::not_found("Tapp not installed"))?;
    let declared: Vec<String> = existing
        .manifest
        .get("remoteMedia")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    let approved = select_install_approved_remote_media(&declared, Some(hosts));
    let mut active: tapps::ActiveModel = existing.into();
    active.approved_remote_media =
        Set(serde_json::to_value(&approved).unwrap_or_else(|_| serde_json::json!([])));
    active.updated_at = Set(Utc::now().fixed_offset());
    active
        .update(&txn)
        .await
        .map_err(|_| AppError::internal("Database error"))?;
    txn.commit()
        .await
        .map_err(|_| AppError::internal("Database error"))?;
    Ok(approved)
}

pub(crate) struct UpdateTarget {
    existing: tapps::Model,
    owner_id: i32,
    is_site_owner: bool,
}

/// Resolve the update target before downloading a store package.
pub(crate) async fn resolve_update_target(
    db: &DatabaseConnection,
    user_id: i32,
    role: UserRole,
    tapp_id: &str,
) -> Result<UpdateTarget, AppError> {
    crate::services::tapp_validation::validate_tapp_id(tapp_id).map_err(AppError::bad_request)?;
    let admin_id = get_admin_user_id(db).await?;
    let owner_id = canonical_installation_owner_id(role, user_id, admin_id);
    let existing = tapps::Entity::find()
        .filter(tapps::Column::UserId.eq(owner_id))
        .filter(tapps::Column::TappId.eq(tapp_id))
        .one(db)
        .await
        .map_err(|_| AppError::internal("Database error"))?
        .ok_or_else(|| AppError::not_found("Tapp not installed"))?;
    Ok(UpdateTarget {
        existing,
        owner_id,
        is_site_owner: is_public_installation_namespace(owner_id, admin_id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest_with_permissions(permissions: &[&str]) -> TappManifest {
        serde_json::from_value(json!({
            "id": "com.example.overwrite",
            "name": "Overwrite",
            "version": "2.0.0",
            "core": { "entry": "main.js" },
            "category": "utility",
            "permissions": permissions,
        }))
        .unwrap()
    }

    fn existing_model(version: &str, approved: serde_json::Value) -> tapps::Model {
        let now = Utc::now().fixed_offset();
        tapps::Model {
            id: 1,
            tapp_id: "com.example.overwrite".to_string(),
            user_id: 7,
            name: "Overwrite".to_string(),
            version: version.to_string(),
            description: None,
            author: None,
            icon: None,
            theme_color: None,
            manifest: json!({}),
            status: tapps::TappStatus::Installed,
            approved_permissions: approved,
            file_path: "manifest.json".to_string(),
            code_path: "main.js".to_string(),
            installed_at: now,
            last_run_at: None,
            updated_at: now,
            error_message: None,
            visibility: "all".to_string(),
            needs_reauthorization: false,
            approved_remote_media: json!(["a.example.com"]),
        }
    }

    #[test]
    fn conflict_error_reports_versions_and_only_new_permissions() {
        let mut manifest =
            manifest_with_permissions(&["storage:read", "ai:generate", "media:remote"]);
        manifest.remote_media = Some(vec!["a.example.com".into(), "b.example.com".into()]);
        let existing = existing_model("1.0.0", json!(["storage:read", "legacy"]));
        let body = install_conflict_error(&manifest, &existing).to_json();
        assert_eq!(body["error"], "Tapp already installed");
        assert_eq!(body["code"], "tapp_already_installed");
        assert_eq!(body["details"]["tappId"], "com.example.overwrite");
        assert_eq!(body["details"]["installedVersion"], "1.0.0");
        assert_eq!(body["details"]["incomingVersion"], "2.0.0");
        assert_eq!(
            body["details"]["newPermissions"],
            json!(["ai:generate", "media:remote"])
        );
        assert_eq!(body["details"]["newRemoteMedia"], json!(["b.example.com"]));
    }
}

#[cfg(test)]
mod database_tests {
    use super::*;
    use sea_orm::ConnectionTrait;
    use serde_json::json;

    fn package(id: &str, version: &str) -> PreparedTappPackage {
        PreparedTappPackage::from_resources(
            serde_json::from_value(json!({
                "id": id, "name": "Install contract", "version": version,
                "core": {"entry": "main.js"}, "category": "utility",
                "permissions": ["storage:read"],
            }))
            .unwrap(),
            PreparedTappResources {
                modules: std::collections::HashMap::from([(
                    "main.js".into(),
                    format!("// {version}\nmodule.exports = {{}};"),
                )]),
                ..Default::default()
            },
        )
    }

    #[tokio::test]
    #[ignore = "requires a disposable TAPP_TEST_DATABASE_URL and DATA_DIR"]
    async fn install_conflict_overwrite_update_and_failed_activation_preserve_contracts() {
        let url = std::env::var("TAPP_TEST_DATABASE_URL").expect("disposable database");
        std::env::var("DATA_DIR").expect("explicit disposable filesystem root");
        let isolated = crate::db::IsolatedSchema::migrated(&url, "install_contract").await;
        let db = &isolated.db;
        db.execute_unprepared("INSERT INTO users (id, username, is_admin, is_owner) VALUES (910001, 'install-owner', true, true)").await.unwrap();
        crate::services::principal::invalidate_site_owner_cache();
        let id = format!("com.example.install{}", uuid::Uuid::new_v4().simple());
        let config = RwLock::new(DynamicConfig::default());
        let first = install_prepared_package(
            db,
            &config,
            910001,
            UserRole::Admin,
            true,
            package(&id, "1.0.0"),
            None,
            None,
            false,
            None,
        )
        .await
        .unwrap();
        assert_eq!(first.status, "installed");
        assert!(first.is_admin_tapp && !first.is_temporary);
        let conflict = install_prepared_package(
            db,
            &config,
            910001,
            UserRole::Admin,
            true,
            package(&id, "2.0.0"),
            None,
            None,
            false,
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(conflict.status_u16(), 409);
        assert_eq!(conflict.to_json()["details"]["installedVersion"], "1.0.0");
        let replaced = install_prepared_package(
            db,
            &config,
            910001,
            UserRole::Admin,
            true,
            package(&id, "2.0.0"),
            None,
            None,
            true,
            None,
        )
        .await
        .unwrap();
        assert_eq!(replaced.status, "running");
        let target = resolve_update_target(db, 910001, UserRole::Admin, &id)
            .await
            .unwrap();
        let updated = update_prepared_package(
            db,
            &config,
            910001,
            UserRole::Admin,
            target,
            id.clone(),
            package(&id, "3.0.0"),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(updated.version, "3.0.0");
        assert_eq!(updated.installed_at, first.installed_at);
        let live = tapp_dir_for(910001, &id).unwrap();
        let code = tokio::fs::read(live.join("main.js")).await.unwrap();
        // The first activation rename moves the old live generation aside.
        fail_next_activation_rename(&live);
        let failed = install_prepared_package(
            db,
            &config,
            910001,
            UserRole::Admin,
            true,
            package(&id, "4.0.0"),
            None,
            None,
            true,
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(failed.code(), Some("tapp_save_failed"));
        assert_eq!(tokio::fs::read(live.join("main.js")).await.unwrap(), code);
        let row = tapps::Entity::find()
            .filter(tapps::Column::TappId.eq(&id))
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.version, "3.0.0");
        assert_eq!(row.approved_permissions, json!(["storage:read"]));
        tokio::fs::remove_dir_all(live).await.unwrap();
        crate::services::principal::invalidate_site_owner_cache();
        isolated.drop().await;
    }

    fn media_package(id: &str, version: &str, hosts: &[&str]) -> PreparedTappPackage {
        PreparedTappPackage::from_resources(
            serde_json::from_value(json!({
                "id": id, "name": "Remote media", "version": version,
                "core": {"entry": "main.js"}, "category": "utility",
                "permissions": ["storage:read", "media:remote"],
                "remoteMedia": hosts,
            }))
            .unwrap(),
            PreparedTappResources {
                modules: std::collections::HashMap::from([(
                    "main.js".into(),
                    format!("// {version}\nmodule.exports = {{}};"),
                )]),
                ..Default::default()
            },
        )
    }

    async fn approved_hosts(db: &DatabaseConnection, id: &str) -> serde_json::Value {
        tapps::Entity::find()
            .filter(tapps::Column::TappId.eq(id))
            .one(db)
            .await
            .unwrap()
            .unwrap()
            .approved_remote_media
    }

    #[tokio::test]
    #[ignore = "requires a disposable TAPP_TEST_DATABASE_URL and DATA_DIR"]
    async fn remote_media_hosts_need_explicit_approval_on_every_path() {
        let url = std::env::var("TAPP_TEST_DATABASE_URL").expect("disposable database");
        std::env::var("DATA_DIR").expect("explicit disposable filesystem root");
        let isolated = crate::db::IsolatedSchema::migrated(&url, "remote_media").await;
        let db = &isolated.db;
        db.execute_unprepared("INSERT INTO users (id, username, is_admin, is_owner) VALUES (910002, 'media-owner', true, true)").await.unwrap();
        crate::services::principal::invalidate_site_owner_cache();
        let id = format!("com.example.media{}", uuid::Uuid::new_v4().simple());
        let config = RwLock::new(DynamicConfig::default());
        let install = |version: &'static str,
                       hosts: &'static [&'static str],
                       remote_media: Option<Vec<String>>,
                       overwrite: bool| {
            let id = id.clone();
            let config = &config;
            async move {
                install_prepared_package(
                    db,
                    config,
                    910002,
                    UserRole::Admin,
                    true,
                    media_package(&id, version, hosts),
                    None,
                    remote_media,
                    overwrite,
                    None,
                )
                .await
            }
        };

        // Fresh install without a host list approves the permission but no host.
        install("1.0.0", &["a.example.com", "b.example.com"], None, false)
            .await
            .unwrap();
        assert_eq!(approved_hosts(db, &id).await, json!([]));

        // The approval endpoint keeps only declared hosts.
        let approved = set_approved_remote_media(
            db,
            910002,
            UserRole::Admin,
            &id,
            &["a.example.com".into(), "evil.example.net".into()],
        )
        .await
        .unwrap();
        assert_eq!(approved, vec!["a.example.com".to_string()]);

        // Overwrite: 409 names only the unapproved host; accepting it adds it.
        let conflict = install("2.0.0", &["a.example.com", "b.example.com"], None, false)
            .await
            .unwrap_err();
        assert_eq!(
            conflict.to_json()["details"]["newRemoteMedia"],
            json!(["b.example.com"])
        );
        install(
            "2.0.0",
            &["a.example.com", "b.example.com"],
            Some(vec!["b.example.com".into()]),
            true,
        )
        .await
        .unwrap();
        assert_eq!(
            approved_hosts(db, &id).await,
            json!(["a.example.com", "b.example.com"])
        );

        // Update without a list: dropped hosts leave, new hosts wait for approval.
        let target = resolve_update_target(db, 910002, UserRole::Admin, &id)
            .await
            .unwrap();
        update_prepared_package(
            db,
            &config,
            910002,
            UserRole::Admin,
            target,
            id.clone(),
            media_package(&id, "3.0.0", &["b.example.com", "c.example.com"]),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(approved_hosts(db, &id).await, json!(["b.example.com"]));

        // The detail view grants only declared ∩ approved, guests included.
        let row = tapps::Entity::find()
            .filter(tapps::Column::TappId.eq(&id))
            .one(db)
            .await
            .unwrap()
            .unwrap();
        let detail = crate::services::tapp_catalog::tapp_detail_from_model(
            row,
            UserRole::Guest,
            false,
            true,
            &DynamicConfig::default(),
        );
        assert_eq!(detail.granted_remote_media, vec!["b.example.com"]);

        tokio::fs::remove_dir_all(tapp_dir_for(910002, &id).unwrap())
            .await
            .unwrap();
        crate::services::principal::invalidate_site_owner_cache();
        isolated.drop().await;
    }
}
