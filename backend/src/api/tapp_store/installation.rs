//! Tapp installation lifecycle: store fetch, direct/file install and transactional updates.
//!
//! Pure install decisions (source mode, direct CSS channels, approved-permission
//! selection, owner/conflict namespaces) live in
//! [`crate::services::tapp_install`] and [`crate::services::tapp_ownership`].
//! This module keeps Claims/DB/FS and role-config permission filtering.

use super::prepared_package::{PackageStageContext, PreparedTappPackage, PreparedTappResources};
use super::store_package::fetch_from_store;
use super::{
    api_http_error, api_response_err, canonical_installation_owner_id, cleanup_reinstall_orphans,
    current_user_role, ensure_tapp_install_allowed, filter_install_permissions, get_admin_user_id,
    installation_conflict_owner_ids, lock_tapp_lifecycle, log_install_failure,
    log_tapp_filesystem_access, reconcile_manifest_widgets, tapp_dir_for,
    tapp_filesystem_error_message, tapp_filesystem_error_status, validate_tapp_id, ApiResponse,
    TappDirStage, TappListItem, TappManifest, WidgetTemplateContents, MAX_TAPP_ARCHIVE_BYTES,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use chrono::Utc;
use once_cell::sync::Lazy;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, DatabaseConnection, EntityTrait,
    QueryFilter, Set, TransactionTrait,
};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::sync::{OwnedSemaphorePermit, RwLock, Semaphore};

use crate::config::DynamicConfig;
use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::models::entities::tapps;
use crate::services::permission_service::UserRole;
use crate::services::tapp_install::{
    archive_upload_too_large_message, archive_upload_would_exceed, build_new_install_persist,
    build_update_install_persist, classify_install_multipart_field, install_overloaded_message,
    install_overloaded_status, is_public_installation_namespace, map_direct_css_channels,
    parse_install_source, select_install_approved_permissions, select_update_approved_permissions,
    InstallMultipartField, InstallSource, INSTALL_ACQUIRE_TIMEOUT_SECS, MAX_CONCURRENT_INSTALLS,
};

/// Global install concurrency gate (MYR-025). Bounds simultaneous archive
/// buffers + extract work so handlers do not hold full zip clones unboundedly.
static INSTALL_SEMAPHORE: Lazy<Arc<Semaphore>> =
    Lazy::new(|| Arc::new(Semaphore::new(MAX_CONCURRENT_INSTALLS)));

/// Acquire an install slot, or fail 503 if the wait times out (overloaded).
async fn acquire_install_permit() -> Result<OwnedSemaphorePermit, HttpError> {
    match tokio::time::timeout(
        StdDuration::from_secs(INSTALL_ACQUIRE_TIMEOUT_SECS),
        INSTALL_SEMAPHORE.clone().acquire_owned(),
    )
    .await
    {
        Ok(Ok(permit)) => Ok(permit),
        Ok(Err(e)) => {
            tracing::error!("Tapp install semaphore closed: {:?}", e);
            Err(api_http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to schedule Tapp install",
            ))
        }
        Err(_) => {
            tracing::warn!(
                permits = MAX_CONCURRENT_INSTALLS,
                timeout_secs = INSTALL_ACQUIRE_TIMEOUT_SECS,
                "Tapp install concurrency limit reached; returning 503"
            );
            Err(api_http_error(
                StatusCode::from_u16(install_overloaded_status())
                    .unwrap_or(StatusCode::SERVICE_UNAVAILABLE),
                install_overloaded_message(),
            ))
        }
    }
}

/// 统一安装 Tapp 的请求体
///
/// 支持两种安装来源：
/// 1. direct: 直接提供代码（本地示例、上传文件解析后）
/// 2. store: 从远程商店安装（后端下载）
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct InstallTappRequest {
    /// 安装来源: "direct" | "store"
    source: String,

    // direct 模式需要的字段
    /// Tapp 清单（direct 模式必需）
    manifest: Option<TappManifest>,
    /// 主代码（direct 模式必需）
    code: Option<String>,
    /// CSS 样式（可选）
    styles: Option<String>,
    /// 页面 HTML 模板（可选）
    page_template: Option<String>,
    /// 小组件 HTML 模板（可选，Widget ID → 尺寸）
    widget_templates: Option<WidgetTemplateContents>,
    /// Widget 专用 Tailwind CSS（可选）
    widget_css: Option<String>,
    /// Page 专用 Tailwind CSS（可选）
    page_css: Option<String>,
    /// i18n 翻译数据（可选，lang_code → JSON 对象）
    i18n: Option<std::collections::HashMap<String, serde_json::Value>>,
    /// Page 模块文件（可选，filename → code）
    page_modules: Option<std::collections::HashMap<String, String>>,
    /// Package assets (optional, relative path → base64 or data-URL base64)
    assets: Option<std::collections::HashMap<String, String>>,

    // store 模式需要的字段
    /// 商店源 URL 或 ID（store 模式必需）
    store_source: Option<String>,
    /// Tapp ID（store 模式必需）
    tapp_id: Option<String>,

    // 通用字段
    /// 授权的权限列表（可选，默认全部授权）
    permissions: Option<Vec<String>>,
}

/// 安装 Tapp（统一接口）
///
/// 支持两种安装来源：
/// - direct: 直接提供代码
/// - store: 从远程商店下载

pub(super) async fn install_tapp(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<Arc<RwLock<DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<InstallTappRequest>,
) -> Result<impl IntoResponse, HttpError> {
    let user_id: i32 = claims
        .sub
        .parse()
        .map_err(|_| api_http_error(StatusCode::UNAUTHORIZED, "Invalid user"))?;
    ensure_tapp_install_allowed(&db, user_id).await?;
    let role = current_user_role(&claims, &db).await;
    let is_current_admin = role == UserRole::Admin;
    let InstallTappRequest {
        source,
        manifest: request_manifest,
        code: request_code,
        styles,
        page_template,
        widget_templates,
        widget_css,
        page_css,
        i18n,
        page_modules,
        assets,
        store_source,
        tapp_id,
        permissions,
    } = req;

    let (package, from_store) = match parse_install_source(&source)
        .map_err(|err| api_http_error(StatusCode::BAD_REQUEST, err.message()))?
    {
        InstallSource::Direct => {
            let manifest = request_manifest.ok_or_else(|| {
                api_http_error(
                    StatusCode::BAD_REQUEST,
                    "manifest is required for direct install",
                )
            })?;
            let code = request_code.ok_or_else(|| {
                api_http_error(
                    StatusCode::BAD_REQUEST,
                    "code is required for direct install",
                )
            })?;
            // Prefer declared manifest paths over cssMode string alone.
            let css = map_direct_css_channels(
                manifest.widget_styles.is_some(),
                manifest.page_styles.is_some(),
                widget_css,
                page_css,
            );
            (
                PreparedTappPackage::from_resources(
                    manifest,
                    PreparedTappResources {
                        code,
                        styles,
                        page_template,
                        widget_templates,
                        widget_styles: css.widget_styles,
                        page_styles: css.page_styles,
                        generated_widget_css: css.generated_widget_css,
                        generated_page_css: css.generated_page_css,
                        i18n,
                        page_modules,
                        assets,
                    },
                ),
                false,
            )
        }
        InstallSource::Store => {
            let store_source = store_source.ok_or_else(|| {
                api_http_error(
                    StatusCode::BAD_REQUEST,
                    "storeSource is required for store install",
                )
            })?;
            let tapp_id = tapp_id.ok_or_else(|| {
                api_http_error(
                    StatusCode::BAD_REQUEST,
                    "tappId is required for store install",
                )
            })?;
            validate_tapp_id(&tapp_id)
                .map_err(|error| api_http_error(StatusCode::BAD_REQUEST, error))?;
            let mut package = fetch_from_store(&db, &store_source, &tapp_id).await?;
            package.apply_resource_overrides(i18n, page_modules, assets);
            (package, true)
        }
    };
    let stats_app_id = package.manifest.id.clone();
    let stats_version = package.manifest.version.clone();
    let result = install_prepared_package(
        &db,
        &dynamic_config,
        user_id,
        role,
        is_current_admin,
        package,
        permissions.unwrap_or_default(),
    )
    .await?;
    if from_store {
        // Instance-day cap (1 install count / instance / app / day) — no shared secret.
        crate::services::store_stats_beacon::spawn_store_stats_hit(
            &stats_app_id,
            &stats_version,
            "install",
        );
    }
    Ok(result)
}

async fn install_prepared_package(
    db: &DatabaseConnection,
    dynamic_config: &RwLock<DynamicConfig>,
    user_id: i32,
    role: UserRole,
    is_current_admin: bool,
    package: PreparedTappPackage,
    permissions: Vec<String>,
) -> Result<Json<ApiResponse<TappListItem>>, HttpError> {
    // Bound concurrent installs early so overload fails 503 without staging work.
    let _install_permit = acquire_install_permit().await?;
    package.validate_for_http(None).map_err(api_response_err)?;
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
        api_http_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
    })?;

    if existing.is_some() {
        return Err(api_http_error(
            StatusCode::CONFLICT,
            "Tapp already installed",
        ));
    }

    // 所有资源先写入同文件系统的 staging 目录；校验通过后再原子切换。
    let final_tapp_dir = tapp_dir_for(installation_owner_id, &manifest.id)
        .map_err(|error| api_http_error(StatusCode::BAD_REQUEST, error))?;
    // DB has no conflict row, but uninstall can leave a live dir or lifecycle
    // artifacts that make activate rename fail with a bare 500.
    cleanup_reinstall_orphans(
        &final_tapp_dir,
        &manifest.id,
        installation_owner_id,
        user_id,
        None,
    );
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
            api_http_error(
                tapp_filesystem_error_status(&error),
                tapp_filesystem_error_message("Failed to create Tapp staging directory", &error),
            )
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
        .await
        .map_err(api_response_err)?;
    // Approved = pure domain selection; granted = role-config filter (async).
    let approved = select_install_approved_permissions(&manifest.permissions, &permissions);
    let granted = filter_install_permissions(dynamic_config, role, approved.clone()).await?;

    let txn = db.begin().await.map_err(|error| {
        log_install_failure(
            "txn.begin",
            &manifest.id,
            user_id,
            installation_owner_id,
            None,
            &error,
        );
        api_http_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
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
            api_http_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
        })?;
    let conflict_query = tapps::Entity::find()
        .filter(tapps::Column::TappId.eq(&manifest.id))
        .filter(tapps::Column::UserId.is_in(conflict_owner_ids));
    if conflict_query
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
            api_http_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error")
        })?
        .is_some()
    {
        txn.rollback().await.ok();
        return Err(api_http_error(
            StatusCode::CONFLICT,
            "Tapp already installed",
        ));
    }

    // Re-clean under the lifecycle lock so a leftover live path cannot race
    // activate after the unlocked pre-stage cleanup.
    cleanup_reinstall_orphans(
        &final_tapp_dir,
        &manifest.id,
        installation_owner_id,
        user_id,
        Some(stage.path()),
    );

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
            return Err(api_http_error(
                tapp_filesystem_error_status(&error),
                tapp_filesystem_error_message("Failed to activate staged Tapp", &error),
            ));
        }
    };

    // Column projection (paths, Running default, permission JSON) is pure domain.
    let persist = build_new_install_persist(
        &manifest,
        installation_owner_id,
        &granted,
        &approved,
        &final_tapp_dir,
        now,
    )
    .map_err(|error| api_http_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
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
        granted_permissions: Set(persist.granted_permissions),
        approved_permissions: Set(persist.approved_permissions),
        file_path: Set(persist.file_path),
        code_path: Set(persist.code_path),
        installed_at: Set(persist.installed_at),
        last_run_at: Set(Some(persist.last_run_at)),
        updated_at: Set(persist.updated_at),
        error_message: Set(None),
        // New installs default to everyone; admin can tighten on the detail page.
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
            return Err(api_http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
            ));
        }
    };
    if let Err(err) =
        reconcile_manifest_widgets(&txn, installation_owner_id, &manifest.id, &manifest, None).await
    {
        txn.rollback().await.ok();
        activated.rollback().await;
        log_install_failure(
            "reconcile_manifest_widgets",
            &manifest.id,
            user_id,
            installation_owner_id,
            Some(&final_tapp_dir),
            &format!("status={}", err.0.status_u16()),
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
        return Err(api_http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database error",
        ));
    }
    activated.commit().await;
    // A newly published installation can immediately shadow an existing
    // private copy with the same ID. No grant or declared-API cache produced
    // from the formerly visible installation may survive that ownership swap.
    crate::api::tapp_runtime::revoke_all_tapp_runtime_grants(db, &manifest.id).await;
    crate::api::tapp_runtime::invalidate_tapp_apis_cache(&manifest.id).await;

    // Only the deterministic site-owner namespace is public and persistent.
    // List projection: services::tapp_catalog (install contract forces status=installed).
    Ok(Json(ApiResponse::success(
        crate::services::tapp_catalog::install_response_list_item(result, is_current_admin),
    )))
}

/// 安装 Tapp（上传 .tapp 文件）
///
/// 接收 multipart 文件上传，解压 ZIP 文件后安装
pub(super) async fn install_tapp_file(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<Arc<RwLock<DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    mut multipart: axum::extract::Multipart,
) -> Result<impl IntoResponse, HttpError> {
    let user_id: i32 = claims
        .sub
        .parse()
        .map_err(|_| api_http_error(StatusCode::UNAUTHORIZED, "Invalid user"))?;
    ensure_tapp_install_allowed(&db, user_id).await?;
    let role = current_user_role(&claims, &db).await;
    let is_current_admin = role == UserRole::Admin;
    // 读取上传的文件
    let mut file_data: Option<Vec<u8>> = None;
    let mut permissions: Vec<String> = Vec::new();

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| api_http_error(StatusCode::BAD_REQUEST, "Failed to read multipart"))?
    {
        let name = field.name().unwrap_or("").to_string();
        match classify_install_multipart_field(&name) {
            InstallMultipartField::File => {
                let mut bytes = Vec::new();
                while let Some(chunk) = field
                    .chunk()
                    .await
                    .map_err(|_| api_http_error(StatusCode::BAD_REQUEST, "Failed to read file"))?
                {
                    if archive_upload_would_exceed(bytes.len(), chunk.len(), MAX_TAPP_ARCHIVE_BYTES)
                    {
                        return Err(api_http_error(
                            StatusCode::PAYLOAD_TOO_LARGE,
                            archive_upload_too_large_message(MAX_TAPP_ARCHIVE_BYTES),
                        ));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                file_data = Some(bytes);
            }
            InstallMultipartField::Permissions => {
                let text = field.text().await.map_err(|_| {
                    api_http_error(StatusCode::BAD_REQUEST, "Failed to read permissions")
                })?;
                if let Ok(parsed) = serde_json::from_str::<Vec<String>>(&text) {
                    permissions = parsed;
                }
            }
            InstallMultipartField::Ignore => {}
        }
    }

    let file_data =
        file_data.ok_or_else(|| api_http_error(StatusCode::BAD_REQUEST, "No file uploaded"))?;

    let package = PreparedTappPackage::from_archive(file_data).map_err(api_response_err)?;
    install_prepared_package(
        &db,
        &dynamic_config,
        user_id,
        role,
        is_current_admin,
        package,
        permissions,
    )
    .await
}

/// 更新 Tapp 的请求体
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UpdateTappRequest {
    /// 更新来源: "store" | "direct"
    source: String,
    // direct 模式需要的字段
    /// Tapp 清单（direct 模式必需）
    manifest: Option<TappManifest>,
    /// 主代码（direct 模式必需）
    code: Option<String>,
    /// CSS 样式（可选）
    styles: Option<String>,
    /// 页面 HTML 模板（可选）
    page_template: Option<String>,
    /// 小组件 HTML 模板（可选，Widget ID → 尺寸）
    widget_templates: Option<WidgetTemplateContents>,
    /// Widget 专用 Tailwind CSS（可选）
    widget_css: Option<String>,
    /// Page 专用 Tailwind CSS（可选）
    page_css: Option<String>,
    /// i18n 翻译数据（可选，lang_code → JSON 对象）
    i18n: Option<std::collections::HashMap<String, serde_json::Value>>,
    /// Page 模块文件（可选，filename → code）
    page_modules: Option<std::collections::HashMap<String, String>>,
    /// Package assets (optional, relative path → base64 or data-URL base64)
    assets: Option<std::collections::HashMap<String, String>>,

    // store 模式需要的字段
    /// 商店源 URL 或 ID
    store_source: Option<String>,
    /// 授权的权限列表（可选，保留原有权限）
    permissions: Option<Vec<String>>,
}

/// 更新 Tapp（从远程商店或内置代码获取最新版本）
///
/// 保留用户数据，仅更新代码和资源
///
/// 权限模型：
/// - 管理员可以更新自己的 Tapp
/// - 普通用户可以更新自己临时安装的 Tapp
pub(super) async fn update_tapp(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<Arc<RwLock<DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
    Json(req): Json<UpdateTappRequest>,
) -> Result<impl IntoResponse, HttpError> {
    let user_id: i32 = claims
        .sub
        .parse()
        .map_err(|_| api_http_error(StatusCode::UNAUTHORIZED, "Invalid user"))?;
    ensure_tapp_install_allowed(&db, user_id).await?;
    let role = current_user_role(&claims, &db).await;
    validate_tapp_id(&tapp_id).map_err(|error| api_http_error(StatusCode::BAD_REQUEST, error))?;
    let admin_id = get_admin_user_id(&db).await?;
    let target_owner_id = canonical_installation_owner_id(role, user_id, admin_id);
    let is_site_owner = is_public_installation_namespace(target_owner_id, admin_id);

    let UpdateTappRequest {
        source,
        manifest: req_manifest,
        code: req_code,
        styles: req_styles,
        page_template: req_page_template,
        widget_templates: req_widget_templates,
        widget_css: req_widget_css,
        page_css: req_page_css,
        i18n: req_i18n,
        page_modules: req_page_modules,
        assets: req_assets,
        store_source,
        permissions,
    } = req;

    // Administrators update the canonical public installation; ordinary users
    // update only their own temporary installation.
    let existing_tapp = tapps::Entity::find()
        .filter(tapps::Column::UserId.eq(target_owner_id))
        .filter(tapps::Column::TappId.eq(&tapp_id))
        .one(&db)
        .await
        .map_err(|_| api_http_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error"))?
        .ok_or_else(|| api_http_error(StatusCode::NOT_FOUND, "Tapp not installed"))?;

    let (package, from_store) = match parse_install_source(&source)
        .map_err(|err| api_http_error(StatusCode::BAD_REQUEST, err.message()))?
    {
        InstallSource::Direct => {
            let manifest = req_manifest.ok_or_else(|| {
                api_http_error(
                    StatusCode::BAD_REQUEST,
                    "manifest is required for direct update",
                )
            })?;
            let code = req_code.ok_or_else(|| {
                api_http_error(
                    StatusCode::BAD_REQUEST,
                    "code is required for direct update",
                )
            })?;
            let css = map_direct_css_channels(
                manifest.widget_styles.is_some(),
                manifest.page_styles.is_some(),
                req_widget_css,
                req_page_css,
            );
            (
                PreparedTappPackage::from_resources(
                    manifest,
                    PreparedTappResources {
                        code,
                        styles: req_styles,
                        page_template: req_page_template,
                        widget_templates: req_widget_templates,
                        widget_styles: css.widget_styles,
                        page_styles: css.page_styles,
                        generated_widget_css: css.generated_widget_css,
                        generated_page_css: css.generated_page_css,
                        i18n: req_i18n,
                        page_modules: req_page_modules,
                        assets: req_assets,
                    },
                ),
                false,
            )
        }
        InstallSource::Store => {
            let store_source = store_source.ok_or_else(|| {
                api_http_error(
                    StatusCode::BAD_REQUEST,
                    "storeSource is required for store update",
                )
            })?;
            let mut package = fetch_from_store(&db, &store_source, &tapp_id).await?;
            package.apply_resource_overrides(None, None, req_assets);
            (package, true)
        }
    };
    package
        .validate_for_http(Some(&tapp_id))
        .map_err(api_response_err)?;
    let manifest = package.manifest.clone();
    let stats_version = manifest.version.clone();

    let final_tapp_dir = tapp_dir_for(target_owner_id, &tapp_id)
        .map_err(|error| api_http_error(StatusCode::BAD_REQUEST, error))?;
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
            api_http_error(
                tapp_filesystem_error_status(&error),
                tapp_filesystem_error_message(
                    "Failed to create Tapp update staging directory",
                    &error,
                ),
            )
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
        .await
        .map_err(api_response_err)?;
    let txn = db.begin().await.map_err(|_| {
        api_http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to begin update transaction",
        )
    })?;
    lock_tapp_lifecycle(&txn, &tapp_id).await.map_err(|_| {
        api_http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to lock Tapp lifecycle",
        )
    })?;
    let existing_tapp = tapps::Entity::find_by_id(existing_tapp.id)
        .filter(tapps::Column::UserId.eq(target_owner_id))
        .filter(tapps::Column::TappId.eq(&tapp_id))
        .one(&txn)
        .await
        .map_err(|_| api_http_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error"))?
        .ok_or_else(|| api_http_error(StatusCode::NOT_FOUND, "Tapp not installed"))?;

    // Approved = pure domain selection; granted = role-config filter (async).
    let previous_approved: Vec<String> =
        serde_json::from_value(existing_tapp.approved_permissions.clone()).unwrap_or_default();
    let approved = select_update_approved_permissions(
        &manifest.permissions,
        permissions.as_deref(),
        &previous_approved,
    );
    let granted = filter_install_permissions(&dynamic_config, role, approved.clone()).await?;

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
            return Err(api_http_error(
                tapp_filesystem_error_status(&error),
                tapp_filesystem_error_message("Failed to activate staged Tapp update", &error),
            ));
        }
    };
    // Column projection is pure domain; ActiveModel mapping stays here.
    let persist =
        build_update_install_persist(&manifest, &granted, &approved, &final_tapp_dir, now)
            .map_err(|error| api_http_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let mut active: tapps::ActiveModel = existing_tapp.clone().into();
    active.name = Set(persist.name);
    active.version = Set(persist.version);
    active.description = Set(persist.description);
    active.author = Set(persist.author);
    active.icon = Set(persist.icon);
    active.theme_color = Set(persist.theme_color);
    active.manifest = Set(persist.manifest);
    active.granted_permissions = Set(persist.granted_permissions);
    active.approved_permissions = Set(persist.approved_permissions);
    active.code_path = Set(persist.code_path);
    active.updated_at = Set(persist.updated_at);

    let result = match active.update(&txn).await {
        Ok(result) => result,
        Err(error) => {
            txn.rollback().await.ok();
            activated.rollback().await;
            tracing::error!(error = %error, "Tapp update database error");
            return Err(api_http_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
            ));
        }
    };
    if let Err(err) = reconcile_manifest_widgets(
        &txn,
        target_owner_id,
        &tapp_id,
        &manifest,
        Some(&existing_tapp.manifest),
    )
    .await
    {
        txn.rollback().await.ok();
        activated.rollback().await;
        return Err(err);
    }
    if txn.commit().await.is_err() {
        activated.rollback_after_commit_error().await;
        return Err(api_http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to commit Tapp update",
        ));
    }
    activated.commit().await;

    // Code or permissions may have changed; existing grants must not survive the update.
    crate::api::tapp_runtime::revoke_all_tapp_runtime_grants(&db, &tapp_id).await;

    // manifest 已更新，清除 API 解析缓存
    crate::api::tapp_runtime::invalidate_tapp_apis_cache(&tapp_id).await;

    tracing::info!(
        "[TAPP] Updated Tapp {} from {} to {} for user {}",
        tapp_id,
        existing_tapp.version,
        result.version,
        user_id
    );

    // Only the deterministic site-owner namespace is public and persistent.
    // List projection: services::tapp_catalog (preserves live status/last_run_at).
    if from_store {
        crate::services::store_stats_beacon::spawn_store_stats_hit(
            &tapp_id,
            &stats_version,
            "update",
        );
    }
    Ok(Json(ApiResponse::success(
        crate::services::tapp_catalog::update_response_list_item(result, is_site_owner),
    )))
}
