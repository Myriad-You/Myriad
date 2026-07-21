//! Tapp installation lifecycle: store fetch, direct/file install and transactional updates.

use super::prepared_package::{PackageStageContext, PreparedTappPackage, PreparedTappResources};
use super::store_package::fetch_from_store;
use super::{
    api_error, canonical_installation_owner_id, cleanup_reinstall_orphans, current_user_role,
    filter_install_permissions, get_admin_user_id, installation_conflict_owner_ids,
    lock_tapp_lifecycle, log_install_failure, log_tapp_filesystem_access,
    reconcile_manifest_widgets, tapp_dir_for, tapp_filesystem_error_message,
    tapp_filesystem_error_status, validate_tapp_id, ApiResponse, TappDirStage, TappListItem,
    TappManifest, WidgetTemplateContents, MAX_TAPP_ARCHIVE_BYTES,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, DatabaseConnection, EntityTrait,
    QueryFilter, Set, TransactionTrait,
};
use serde::Deserialize;

use crate::middleware::auth::Claims;
use crate::models::entities::tapps;
use crate::services::permission_service::UserRole;

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

    // ===== direct 模式需要的字段 =====
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

    // ===== store 模式需要的字段 =====
    /// 商店源 URL 或 ID（store 模式必需）
    store_source: Option<String>,
    /// Tapp ID（store 模式必需）
    tapp_id: Option<String>,

    // ===== 通用字段 =====
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
    Extension(claims): Extension<Claims>,
    Json(req): Json<InstallTappRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<()>>)> {
    let user_id: i32 = claims
        .sub
        .parse()
        .map_err(|_| (StatusCode::UNAUTHORIZED, api_error("Invalid user")))?;
    let role = current_user_role(&claims).await;
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

    let package = match source.as_str() {
        "direct" => {
            let manifest = request_manifest.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    api_error("manifest is required for direct install"),
                )
            })?;
            let code = request_code.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    api_error("code is required for direct install"),
                )
            })?;
            // Prefer declared manifest paths: if pageStyles/widgetStyles are set,
            // request pageCss/widgetCss fill those channels (cssMode=separated apps).
            // Otherwise treat them as generated page.css / widget.css sidecars.
            // Mapping by declared fields is more robust than cssMode string alone.
            let (widget_styles, generated_widget_css) = if manifest.widget_styles.is_some() {
                (widget_css, None)
            } else {
                (None, widget_css)
            };
            let (page_styles, generated_page_css) = if manifest.page_styles.is_some() {
                (page_css, None)
            } else {
                (None, page_css)
            };
            PreparedTappPackage::from_resources(
                manifest,
                PreparedTappResources {
                    code,
                    styles,
                    page_template,
                    widget_templates,
                    widget_styles,
                    page_styles,
                    generated_widget_css,
                    generated_page_css,
                    i18n,
                    page_modules,
                    assets,
                    ..PreparedTappResources::default()
                },
            )
        }
        "store" => {
            let store_source = store_source.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    api_error("storeSource is required for store install"),
                )
            })?;
            let tapp_id = tapp_id.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    api_error("tappId is required for store install"),
                )
            })?;
            validate_tapp_id(&tapp_id)
                .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
            let mut package = fetch_from_store(&db, &store_source, &tapp_id).await?;
            package.apply_resource_overrides(i18n, page_modules, assets);
            package
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                api_error("Invalid source, must be 'direct' or 'store'"),
            ));
        }
    };
    install_prepared_package(
        &db,
        user_id,
        role,
        is_current_admin,
        package,
        permissions.unwrap_or_default(),
    )
    .await
}

async fn install_prepared_package(
    db: &DatabaseConnection,
    user_id: i32,
    role: UserRole,
    is_current_admin: bool,
    package: PreparedTappPackage,
    permissions: Vec<String>,
) -> Result<Json<ApiResponse<TappListItem>>, (StatusCode, Json<ApiResponse<()>>)> {
    package.validate(None)?;
    let manifest = package.manifest.clone();

    // 检查是否已安装
    let admin_id = get_admin_user_id(db).await.map_err(|status| {
        (
            status,
            api_error("Failed to resolve administrator namespace"),
        )
    })?;
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
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error(format!("Database error: {error}")),
        )
    })?;

    if existing.is_some() {
        return Err((StatusCode::CONFLICT, api_error("Tapp already installed")));
    }

    // 所有资源先写入同文件系统的 staging 目录；校验通过后再原子切换。
    let final_tapp_dir = tapp_dir_for(installation_owner_id, &manifest.id)
        .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
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
            (
                tapp_filesystem_error_status(&error),
                api_error(tapp_filesystem_error_message(
                    "Failed to create Tapp staging directory",
                    &error,
                )),
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
        .await?;
    // 确定授权的权限
    let requested_permissions: Vec<String> = if permissions.is_empty() {
        manifest.permissions.clone()
    } else {
        manifest
            .permissions
            .iter()
            .filter(|p| permissions.contains(p))
            .cloned()
            .collect()
    };
    let approved = requested_permissions;
    let granted = filter_install_permissions(role, approved.clone()).await;

    let txn = db.begin().await.map_err(|error| {
        log_install_failure(
            "txn.begin",
            &manifest.id,
            user_id,
            installation_owner_id,
            None,
            &error,
        );
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error(format!("Failed to begin install transaction: {error}")),
        )
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
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error(format!("Failed to lock Tapp lifecycle: {error}")),
            )
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
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error(format!("Database error: {error}")),
            )
        })?
        .is_some()
    {
        txn.rollback().await.ok();
        return Err((StatusCode::CONFLICT, api_error("Tapp already installed")));
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
            return Err((
                tapp_filesystem_error_status(&error),
                api_error(tapp_filesystem_error_message(
                    "Failed to activate staged Tapp",
                    &error,
                )),
            ));
        }
    };
    let manifest_path = final_tapp_dir.join("manifest.json");
    let code_path = final_tapp_dir.join(&manifest.main);

    // 保存到数据库
    let tapp = tapps::ActiveModel {
        id: NotSet,
        tapp_id: Set(manifest.id.clone()),
        user_id: Set(installation_owner_id),
        name: Set(manifest.name.clone()),
        version: Set(manifest.version.clone()),
        description: Set(manifest.description.clone()),
        author: Set(manifest
            .author
            .as_ref()
            .map(|a| serde_json::to_value(a).unwrap())),
        icon: Set(manifest.icon.clone()),
        theme_color: Set(manifest.theme_color.clone()),
        manifest: Set(serde_json::to_value(&manifest).unwrap()),
        status: Set(tapps::TappStatus::Installed),
        granted_permissions: Set(serde_json::to_value(&granted).unwrap()),
        approved_permissions: Set(serde_json::to_value(&approved).unwrap()),
        file_path: Set(manifest_path.to_string_lossy().to_string()),
        code_path: Set(code_path.to_string_lossy().to_string()),
        installed_at: Set(now),
        last_run_at: Set(None),
        updated_at: Set(now),
        error_message: Set(None),
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
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error(format!("Database error: {error}")),
            ));
        }
    };
    if let Err(status) =
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
            &format!("status={status}"),
        );
        return Err((
            status,
            api_error(format!(
                "Failed to register manifest Widgets (status {status})"
            )),
        ));
    }
    if let Err(error) = txn.commit().await {
        activated.rollback().await;
        log_install_failure(
            "txn.commit",
            &manifest.id,
            user_id,
            installation_owner_id,
            Some(&final_tapp_dir),
            &error,
        );
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error(format!("Failed to commit Tapp installation: {error}")),
        ));
    }
    activated.commit().await;
    // A newly published installation can immediately shadow an existing
    // private copy with the same ID. No grant or declared-API cache produced
    // from the formerly visible installation may survive that ownership swap.
    crate::api::tapp_runtime::revoke_all_tapp_runtime_grants(&manifest.id).await;
    crate::api::tapp_runtime::invalidate_tapp_apis_cache(&manifest.id).await;

    // Only the deterministic site-owner namespace is public and persistent.
    let is_temporary = !is_current_admin;

    // 从 manifest 中提取 iconSvg
    let icon_svg = result
        .manifest
        .get("iconSvg")
        .and_then(|v| v.as_str())
        .map(String::from);
    let locales = super::types::manifest_locales(&result.manifest);

    Ok(Json(ApiResponse::success(TappListItem {
        id: result.tapp_id,
        name: result.name,
        version: result.version,
        description: result.description,
        icon: result.icon,
        icon_svg,
        locales,
        status: "installed".to_string(),
        installed_at: result.installed_at.to_rfc3339(),
        last_run_at: None,
        is_temporary,
        is_admin_tapp: is_current_admin,
    })))
}

/// 安装 Tapp（上传 .tapp 文件）
///
/// 接收 multipart 文件上传，解压 ZIP 文件后安装
pub(super) async fn install_tapp_file(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    mut multipart: axum::extract::Multipart,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<()>>)> {
    let user_id: i32 = claims
        .sub
        .parse()
        .map_err(|_| (StatusCode::UNAUTHORIZED, api_error("Invalid user")))?;
    let role = current_user_role(&claims).await;
    let is_current_admin = role == UserRole::Admin;
    // 读取上传的文件
    let mut file_data: Option<Vec<u8>> = None;
    let mut permissions: Vec<String> = Vec::new();

    while let Some(mut field) = multipart.next_field().await.map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            api_error("Failed to read multipart"),
        )
    })? {
        let name = field.name().unwrap_or("").to_string();

        if name == "file" {
            let mut bytes = Vec::new();
            while let Some(chunk) = field
                .chunk()
                .await
                .map_err(|_| (StatusCode::BAD_REQUEST, api_error("Failed to read file")))?
            {
                if bytes.len().saturating_add(chunk.len()) > MAX_TAPP_ARCHIVE_BYTES {
                    return Err((
                        StatusCode::PAYLOAD_TOO_LARGE,
                        api_error(format!(".tapp file exceeds {MAX_TAPP_ARCHIVE_BYTES} bytes")),
                    ));
                }
                bytes.extend_from_slice(&chunk);
            }
            file_data = Some(bytes);
        } else if name == "permissions" {
            let text = field.text().await.map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    api_error("Failed to read permissions"),
                )
            })?;
            if let Ok(parsed) = serde_json::from_str::<Vec<String>>(&text) {
                permissions = parsed;
            }
        }
    }

    let file_data =
        file_data.ok_or_else(|| (StatusCode::BAD_REQUEST, api_error("No file uploaded")))?;

    let package = PreparedTappPackage::from_archive(file_data)?;
    install_prepared_package(&db, user_id, role, is_current_admin, package, permissions).await
}

/// 更新 Tapp 的请求体
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UpdateTappRequest {
    /// 更新来源: "store" | "direct"
    source: String,
    // ===== direct 模式需要的字段 =====
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

    // ===== store 模式需要的字段 =====
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
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
    Json(req): Json<UpdateTappRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<()>>)> {
    let user_id: i32 = claims
        .sub
        .parse()
        .map_err(|_| (StatusCode::UNAUTHORIZED, api_error("Invalid user")))?;
    let role = current_user_role(&claims).await;
    validate_tapp_id(&tapp_id).map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
    let admin_id = get_admin_user_id(&db).await.map_err(|status| {
        (
            status,
            api_error("Failed to resolve administrator namespace"),
        )
    })?;
    let target_owner_id = canonical_installation_owner_id(role, user_id, admin_id);
    let is_site_owner = target_owner_id == admin_id;

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
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error("Database error"),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, api_error("Tapp not installed")))?;

    let package = match source.as_str() {
        "direct" => {
            let manifest = req_manifest.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    api_error("manifest is required for direct update"),
                )
            })?;
            let code = req_code.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    api_error("code is required for direct update"),
                )
            })?;
            // Prefer declared pageStyles/widgetStyles over cssMode string alone.
            let (widget_styles, generated_widget_css) = if manifest.widget_styles.is_some() {
                (req_widget_css, None)
            } else {
                (None, req_widget_css)
            };
            let (page_styles, generated_page_css) = if manifest.page_styles.is_some() {
                (req_page_css, None)
            } else {
                (None, req_page_css)
            };
            PreparedTappPackage::from_resources(
                manifest,
                PreparedTappResources {
                    code,
                    styles: req_styles,
                    page_template: req_page_template,
                    widget_templates: req_widget_templates,
                    widget_styles,
                    page_styles,
                    generated_widget_css,
                    generated_page_css,
                    i18n: req_i18n,
                    page_modules: req_page_modules,
                    assets: req_assets,
                    ..PreparedTappResources::default()
                },
            )
        }
        "store" => {
            let store_source = store_source.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    api_error("storeSource is required for store update"),
                )
            })?;
            let mut package = fetch_from_store(&db, &store_source, &tapp_id).await?;
            package.apply_resource_overrides(None, None, req_assets);
            package
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                api_error("Invalid source, must be 'direct' or 'store'"),
            ));
        }
    };
    package.validate(Some(&tapp_id))?;
    let manifest = package.manifest.clone();

    let final_tapp_dir = tapp_dir_for(target_owner_id, &tapp_id)
        .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
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
            (
                tapp_filesystem_error_status(&error),
                api_error(tapp_filesystem_error_message(
                    "Failed to create Tapp update staging directory",
                    &error,
                )),
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
        .await?;
    let txn = db.begin().await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error("Failed to begin update transaction"),
        )
    })?;
    lock_tapp_lifecycle(&txn, &tapp_id).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error("Failed to lock Tapp lifecycle"),
        )
    })?;
    let existing_tapp = tapps::Entity::find_by_id(existing_tapp.id)
        .filter(tapps::Column::UserId.eq(target_owner_id))
        .filter(tapps::Column::TappId.eq(&tapp_id))
        .one(&txn)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error("Database error"),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, api_error("Tapp not installed")))?;

    // 确定授权的权限（保留原有权限或使用新权限）
    let requested_permissions: Vec<String> = if let Some(perms) = permissions {
        if perms.is_empty() {
            manifest.permissions.clone()
        } else {
            manifest
                .permissions
                .iter()
                .filter(|p| perms.contains(p))
                .cloned()
                .collect()
        }
    } else {
        // 保留原有已授权的权限，同时过滤掉新版本不再需要的权限
        let original_perms: Vec<String> =
            serde_json::from_value(existing_tapp.approved_permissions.clone()).unwrap_or_default();
        manifest
            .permissions
            .iter()
            .filter(|p| original_perms.contains(p))
            .cloned()
            .collect()
    };
    let approved = requested_permissions;
    let granted = filter_install_permissions(role, approved.clone()).await;

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
            return Err((
                tapp_filesystem_error_status(&error),
                api_error(tapp_filesystem_error_message(
                    "Failed to activate staged Tapp update",
                    &error,
                )),
            ));
        }
    };
    let code_path = final_tapp_dir.join(&manifest.main);

    // 更新数据库记录
    let mut active: tapps::ActiveModel = existing_tapp.clone().into();
    active.name = Set(manifest.name.clone());
    active.version = Set(manifest.version.clone());
    active.description = Set(manifest.description.clone());
    active.author = Set(manifest
        .author
        .as_ref()
        .map(|a| serde_json::to_value(a).unwrap()));
    active.icon = Set(manifest.icon.clone());
    active.theme_color = Set(manifest.theme_color.clone());
    active.manifest = Set(serde_json::to_value(&manifest).unwrap());
    active.granted_permissions = Set(serde_json::to_value(&granted).unwrap());
    active.approved_permissions = Set(serde_json::to_value(&approved).unwrap());
    active.code_path = Set(code_path.to_string_lossy().to_string());
    active.updated_at = Set(now);

    let result = match active.update(&txn).await {
        Ok(result) => result,
        Err(error) => {
            txn.rollback().await.ok();
            activated.rollback().await;
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error(format!("Database error: {error}")),
            ));
        }
    };
    if let Err(status) = reconcile_manifest_widgets(
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
        return Err((status, api_error("Failed to reconcile manifest Widgets")));
    }
    if txn.commit().await.is_err() {
        activated.rollback().await;
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error("Failed to commit Tapp update"),
        ));
    }
    activated.commit().await;

    // Code or permissions may have changed; existing grants must not survive the update.
    crate::api::tapp_runtime::revoke_all_tapp_runtime_grants(&tapp_id).await;

    // manifest 已更新，清除 API 解析缓存
    crate::api::tapp_runtime::invalidate_tapp_apis_cache(&tapp_id).await;

    // Only the deterministic site-owner namespace is public and persistent.
    let is_temporary = !is_site_owner;

    tracing::info!(
        "[TAPP] Updated Tapp {} from {} to {} for user {}",
        tapp_id,
        existing_tapp.version,
        result.version,
        user_id
    );

    // 从 manifest 中提取 iconSvg
    let icon_svg = result
        .manifest
        .get("iconSvg")
        .and_then(|v| v.as_str())
        .map(String::from);
    let locales = super::types::manifest_locales(&result.manifest);

    Ok(Json(ApiResponse::success(TappListItem {
        id: result.tapp_id,
        name: result.name,
        version: result.version,
        description: result.description,
        icon: result.icon,
        icon_svg,
        locales,
        status: format!("{:?}", result.status).to_lowercase(),
        installed_at: result.installed_at.to_rfc3339(),
        last_run_at: result.last_run_at.map(|dt| dt.to_rfc3339()),
        is_temporary,
        is_admin_tapp: is_site_owner,
    })))
}
