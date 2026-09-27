//! Tapp installation lifecycle: store fetch, direct/file install and transactional updates.
//!
//! Source-mode parse, approved-permission selection, and persist snapshots live in
//! [`crate::services::tapp_install`]; owner/conflict namespaces live in
//! [`crate::services::tapp_ownership`]. This module decodes HTTP input and wraps
//! the results of [`crate::services::tapp_packages`] in the API response envelope.

use super::store_package::fetch_from_store;
use super::{
    ApiResponse, MAX_TAPP_GAME_ARCHIVE_BYTES, TappManifest, WidgetTemplateContents, api_http_error,
    current_user_role, ensure_tapp_install_allowed, validate_tapp_id,
};
use crate::config::DynamicConfig;
use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::services::permission_service::UserRole;
use crate::services::tapp_install::{
    InstallMultipartField, InstallSource, archive_upload_too_large_message,
    archive_upload_would_exceed, classify_install_multipart_field, parse_install_source,
};
use crate::services::tapp_packages::prepared_package::{
    PreparedTappPackage, PreparedTappResources, package_from_archive,
};
use crate::services::tapp_packages::{acquire_install_permit, install_prepared_package};
use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 统一安装 Tapp 的请求体
///
/// 支持两种安装来源：
/// 1. direct: 直接提供代码
/// 2. store: 从远程商店安装（后端下载）
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct InstallTappRequest {
    /// 安装来源: "direct" | "store"
    source: String,

    // direct 模式需要的字段
    /// Tapp 清单（direct 模式必需）
    manifest: Option<TappManifest>,
    /// 包内 `.js` 文件：相对路径 → 源码（direct 模式必需，须覆盖每个层入口）
    modules: Option<std::collections::HashMap<String, String>>,
    /// 作者共享样式（`core.styles`）
    core_styles: Option<String>,
    /// 作者 Page 样式（`page.styles`）
    page_styles: Option<String>,
    /// 作者 Widget 样式：widget id → 内容
    widget_styles: Option<std::collections::HashMap<String, String>>,
    /// 页面 HTML 模板（可选）
    page_template: Option<String>,
    /// 小组件 HTML 模板（可选，widget id → 尺寸 → HTML）
    widget_templates: Option<WidgetTemplateContents>,
    /// 宿主预编译的 Widget Tailwind CSS（可选）
    widget_css: Option<String>,
    /// 宿主预编译的 Page Tailwind CSS（可选）
    page_css: Option<String>,
    /// i18n 翻译数据（可选，lang_code → JSON 对象）
    i18n: Option<std::collections::HashMap<String, serde_json::Value>>,
    /// Package assets (optional, relative path → base64 or data-URL base64)
    assets: Option<std::collections::HashMap<String, String>>,

    // store 模式需要的字段
    /// 商店源 URL 或 ID（store 模式必需）
    store_source: Option<String>,
    /// Tapp ID（store 模式必需）
    tapp_id: Option<String>,

    // 通用字段
    /// 要批准的权限列表（可选；缺省则批准全部声明权限）
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
        .subject_id()
        .ok_or_else(|| api_http_error(StatusCode::UNAUTHORIZED, "Invalid user"))?;
    ensure_tapp_install_allowed(&db, user_id).await?;
    let role = current_user_role(&claims, &db).await?;
    let is_current_admin = role == UserRole::Admin;
    let install_permit = acquire_install_permit().await?;
    let InstallTappRequest {
        source,
        manifest: request_manifest,
        modules: request_modules,
        core_styles,
        page_styles,
        widget_styles,
        page_template,
        widget_templates,
        widget_css,
        page_css,
        i18n,
        assets,
        store_source,
        tapp_id,
        permissions,
    } = req;

    let (package, from_store) = match parse_install_source(&source).map_err(|err| {
        api_http_error(
            StatusCode::from_u16(err.status_hint()).unwrap_or(StatusCode::BAD_REQUEST),
            err.message(),
        )
    })? {
        InstallSource::Direct => {
            let manifest = request_manifest.ok_or_else(|| {
                api_http_error(
                    StatusCode::BAD_REQUEST,
                    "manifest is required for direct install",
                )
            })?;
            let modules = request_modules.ok_or_else(|| {
                api_http_error(
                    StatusCode::BAD_REQUEST,
                    "modules is required for direct install",
                )
            })?;
            (
                PreparedTappPackage::from_resources(
                    manifest,
                    PreparedTappResources {
                        modules,
                        core_styles,
                        page_styles,
                        widget_styles,
                        page_template,
                        widget_templates,
                        generated_widget_css: widget_css,
                        generated_page_css: page_css,
                        i18n,
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
            package.apply_resource_overrides(i18n, request_modules, assets);
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
        permissions,
        false,
        Some(install_permit),
    )
    .await?;
    if from_store {
        // Fire-and-forget store stats hit; payload has no secret.
        crate::services::store_stats_beacon::spawn_store_stats_hit(
            &stats_app_id,
            &stats_version,
            "install",
        );
    }
    Ok(Json(ApiResponse::success(result)))
}

/// Query options for `POST /api/tapps/install-file`.
#[derive(Debug, Default, Deserialize)]
pub(super) struct InstallOverwriteOptions {
    /// When true, an existing install of the same id is updated in place
    /// instead of rejected with 409. Defaults to false.
    #[serde(default)]
    overwrite: bool,
}

/// 安装 Tapp（上传 .tapp 文件）
///
/// 接收 multipart 文件上传，解压 ZIP 文件后安装
pub(super) async fn install_tapp_file(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<Arc<RwLock<DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    Query(options): Query<InstallOverwriteOptions>,
    mut multipart: axum::extract::Multipart,
) -> Result<impl IntoResponse, HttpError> {
    let user_id: i32 = claims
        .subject_id()
        .ok_or_else(|| api_http_error(StatusCode::UNAUTHORIZED, "Invalid user"))?;
    ensure_tapp_install_allowed(&db, user_id).await?;
    let role = current_user_role(&claims, &db).await?;
    let is_current_admin = role == UserRole::Admin;
    let install_permit = acquire_install_permit().await?;
    // 读取上传的文件
    let mut file_data: Option<Vec<u8>> = None;
    let mut permissions: Option<Vec<String>> = None;

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
                    if archive_upload_would_exceed(
                        bytes.len(),
                        chunk.len(),
                        MAX_TAPP_GAME_ARCHIVE_BYTES,
                    ) {
                        return Err(api_http_error(
                            StatusCode::PAYLOAD_TOO_LARGE,
                            archive_upload_too_large_message(MAX_TAPP_GAME_ARCHIVE_BYTES),
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
                    permissions = Some(parsed);
                }
            }
            InstallMultipartField::Ignore => {}
        }
    }

    let file_data =
        file_data.ok_or_else(|| api_http_error(StatusCode::BAD_REQUEST, "No file uploaded"))?;

    let package = package_from_archive(file_data)?;
    let result = install_prepared_package(
        &db,
        &dynamic_config,
        user_id,
        role,
        is_current_admin,
        package,
        permissions,
        options.overwrite,
        Some(install_permit),
    )
    .await?;
    Ok(Json(ApiResponse::success(result)))
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
    /// 包内 `.js` 文件：相对路径 → 源码（direct 模式必需，须覆盖每个层入口）
    modules: Option<std::collections::HashMap<String, String>>,
    /// 作者共享样式（`core.styles`）
    core_styles: Option<String>,
    /// 作者 Page 样式（`page.styles`）
    page_styles: Option<String>,
    /// 作者 Widget 样式：widget id → 内容
    widget_styles: Option<std::collections::HashMap<String, String>>,
    /// 页面 HTML 模板（可选）
    page_template: Option<String>,
    /// 小组件 HTML 模板（可选，widget id → 尺寸 → HTML）
    widget_templates: Option<WidgetTemplateContents>,
    /// 宿主预编译的 Widget Tailwind CSS（可选）
    widget_css: Option<String>,
    /// 宿主预编译的 Page Tailwind CSS（可选）
    page_css: Option<String>,
    /// i18n 翻译数据（可选，lang_code → JSON 对象）
    i18n: Option<std::collections::HashMap<String, serde_json::Value>>,
    /// Package assets (optional, relative path → base64 or data-URL base64)
    assets: Option<std::collections::HashMap<String, String>>,

    // store 模式需要的字段
    /// 商店源 URL 或 ID
    store_source: Option<String>,
    /// 要批准的权限列表（可选；缺省保留仍在声明中的原批准集）
    permissions: Option<Vec<String>>,
}

/// 更新 Tapp（从远程商店或内置代码获取最新版本）
///
/// 保留用户数据，仅更新代码和资源
pub(super) async fn update_tapp(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<Arc<RwLock<DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
    Json(req): Json<UpdateTappRequest>,
) -> Result<impl IntoResponse, HttpError> {
    let user_id: i32 = claims
        .subject_id()
        .ok_or_else(|| api_http_error(StatusCode::UNAUTHORIZED, "Invalid user"))?;
    ensure_tapp_install_allowed(&db, user_id).await?;
    let role = current_user_role(&claims, &db).await?;
    let _update_permit = acquire_install_permit().await?;
    let target =
        crate::services::tapp_packages::resolve_update_target(&db, user_id, role, &tapp_id).await?;

    let UpdateTappRequest {
        source,
        manifest: req_manifest,
        modules: req_modules,
        core_styles: req_core_styles,
        page_styles: req_page_styles,
        widget_styles: req_widget_styles,
        page_template: req_page_template,
        widget_templates: req_widget_templates,
        widget_css: req_widget_css,
        page_css: req_page_css,
        i18n: req_i18n,
        assets: req_assets,
        store_source,
        permissions,
    } = req;

    let (package, from_store) = match parse_install_source(&source).map_err(|err| {
        api_http_error(
            StatusCode::from_u16(err.status_hint()).unwrap_or(StatusCode::BAD_REQUEST),
            err.message(),
        )
    })? {
        InstallSource::Direct => {
            let manifest = req_manifest.ok_or_else(|| {
                api_http_error(
                    StatusCode::BAD_REQUEST,
                    "manifest is required for direct update",
                )
            })?;
            let modules = req_modules.ok_or_else(|| {
                api_http_error(
                    StatusCode::BAD_REQUEST,
                    "modules is required for direct update",
                )
            })?;
            (
                PreparedTappPackage::from_resources(
                    manifest,
                    PreparedTappResources {
                        modules,
                        core_styles: req_core_styles,
                        page_styles: req_page_styles,
                        widget_styles: req_widget_styles,
                        page_template: req_page_template,
                        widget_templates: req_widget_templates,
                        generated_widget_css: req_widget_css,
                        generated_page_css: req_page_css,
                        i18n: req_i18n,
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
    let stats_version = package.manifest.version.clone();
    let result = crate::services::tapp_packages::update_prepared_package(
        &db,
        &dynamic_config,
        user_id,
        role,
        target,
        tapp_id.clone(),
        package,
        permissions,
    )
    .await?;
    // List projection: services::tapp_catalog (preserves live status/last_run_at).
    if from_store {
        crate::services::store_stats_beacon::spawn_store_stats_hit(
            &tapp_id,
            &stats_version,
            "update",
        );
    }
    Ok(Json(ApiResponse::success(result)))
}

#[cfg(test)]
mod tests {

    #[test]
    fn install_permit_is_taken_before_archive_or_store_work() {
        let src = include_str!("installation.rs");
        let install = src
            .split("pub(super) async fn install_tapp(")
            .nth(1)
            .and_then(|rest| rest.split("pub(super) struct InstallOverwriteOptions").next())
            .expect("install_tapp");
        let permit = install.find("acquire_install_permit").expect("permit");
        assert!(permit < install.find("fetch_from_store").expect("store fetch"));
        assert!(permit < install.find("from_resources").expect("direct package"));

        let file = src
            .split("pub(super) async fn install_tapp_file(")
            .nth(1)
            .and_then(|rest| rest.split("pub(super) struct UpdateTappRequest").next())
            .expect("install_tapp_file");
        let file_permit = file.find("acquire_install_permit").expect("file permit");
        assert!(file_permit < file.find("next_field").expect("multipart"));
        assert!(file_permit < file.find("package_from_archive").expect("zip parse"));
    }
}
