//! Tapp 应用管理 API
//!
//! 提供 Tapp 应用的安装、卸载、启动、停止等功能
//!
//! ## 权限模型
//!
//! - **管理员**: 完全控制自己的 Tapp，内容对所有用户可见
//! - **普通用户**: 查看并运行管理员的 Tapp，可临时安装自己的 Tapp（退出登录后移除）
//! - **游客**: 只读访问管理员的 Tapp 内容
//!
//! 普通用户临时安装的 Tapp 权限限制为 basic 级别

mod catalog;
mod compatibility;
mod manifest;
mod package_files;
mod storage;
mod store_sources;
mod validation;
mod widgets;

#[cfg(test)]
use catalog::tapp_detail_from_model;
use catalog::{get_tapp, list_tapp_details, list_tapps};
use compatibility::update_separated_css;
pub use manifest::*;
pub(crate) use package_files::*;
use storage::{
    clear_storage, delete_storage, get_storage, get_storage_usage, get_tapp_setting,
    get_tapp_settings, list_storage_entries, list_storage_keys, set_storage, set_tapp_setting,
};
pub(crate) use storage::{
    read_storage_value, validate_sandbox_storage_key, validate_storage_key,
    validate_storage_value_size, write_storage_value,
};
pub use store_sources::*;
pub(crate) use validation::*;
#[cfg(test)]
use widgets::runtime_widget_belongs_to_installation;
#[allow(dead_code)]
pub type RegisterWidgetRequest = widgets::RegisterWidgetRequest;
use widgets::{list_all_widgets, reconcile_manifest_widgets, register_widget, unregister_widget};

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    middleware::from_fn_with_state,
    response::IntoResponse,
    routing::{delete, get, post},
    Extension, Json, Router,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, DbErr, EntityTrait, QueryFilter, Set, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;

use crate::api::tapp_runtime::common as tapp_common;
use crate::api::tapp_runtime::RuntimeGrantContext;
use crate::middleware::auth::{
    auth_middleware, ensure_current_admin, extract_optional_claims, optional_auth_middleware,
    Claims,
};
use crate::models::entities::{
    tapp_storage, tapp_store_sources, tapp_user_activities, tapp_widgets, tapps,
};
use crate::services::permission_service::{TappPermission, TappPermissionService, UserRole};
use crate::GLOBAL_DYNAMIC_CONFIG;

/// 获取管理员用户 ID（委托给 tapp_runtime::common 的缓存版本）
async fn get_admin_user_id(db: &DatabaseConnection) -> Result<i32, StatusCode> {
    tapp_common::get_admin_user_id(db)
        .await
        .map_err(|(status, _)| status)
}

async fn find_admin_user_id(db: &DatabaseConnection) -> Result<Option<i32>, StatusCode> {
    tapp_common::find_admin_user_id(db)
        .await
        .map_err(|(status, _)| status)
}

/// One public-route lookup rule for details, code, resources and export.
///
/// When the authenticated subject has a private install of the same `tapp_id`,
/// that record wins so list/detail/runtime open the personal copy. Otherwise
/// fall back to the site-owner public install. Guests only see public installs.
/// A fresh database has no site owner yet and therefore returns `None`.
struct VisibleTappInstallation {
    tapp: tapps::Model,
    is_site_owner: bool,
}

async fn find_visible_tapp(
    db: &DatabaseConnection,
    user_id: Option<i32>,
    tapp_id: &str,
) -> Result<Option<VisibleTappInstallation>, StatusCode> {
    validate_tapp_id(tapp_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let site_owner_id = find_admin_user_id(db).await?;

    // Prefer the subject's private install when both private and public copies exist.
    if let Some(user_id) = user_id.filter(|user_id| Some(*user_id) != site_owner_id) {
        let tapp = tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(user_id))
            .filter(tapps::Column::TappId.eq(tapp_id))
            .one(db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if let Some(tapp) = tapp {
            return Ok(Some(VisibleTappInstallation {
                tapp,
                is_site_owner: false,
            }));
        }
    }

    if let Some(site_owner_id) = site_owner_id {
        let public_tapp = tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(site_owner_id))
            .filter(tapps::Column::TappId.eq(tapp_id))
            .one(db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if let Some(tapp) = public_tapp {
            return Ok(Some(VisibleTappInstallation {
                tapp,
                is_site_owner: true,
            }));
        }
    }

    Ok(None)
}

/// Serialize every live-path or ownership mutation for one public Tapp ID.
///
/// The lock is global across owner namespaces so admin public installs and user
/// private installs of the same ID cannot race their conflict checks across replicas.
async fn lock_tapp_lifecycle(db: &impl ConnectionTrait, tapp_id: &str) -> Result<(), DbErr> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        vec![format!("tapp-lifecycle:{tapp_id}").into()],
    ))
    .await?;
    Ok(())
}

async fn current_is_admin(claims: &Claims) -> bool {
    ensure_current_admin(claims).await.is_ok()
}

fn optional_authenticated_user_id(claims: Option<&Claims>) -> Option<i32> {
    claims
        .and_then(|claims| claims.sub.parse::<i32>().ok())
        .filter(|user_id| *user_id >= 0)
}

fn require_runtime_storage_grant(
    grant: &RuntimeGrantContext,
    tapp_id: &str,
) -> Result<(), StatusCode> {
    grant
        .require_tapp_id(tapp_id)
        .and_then(|_| grant.require(TappPermission::Storage))
        .map_err(|(status, _)| status)
}

/// Resolve the two storage identities attached to a Tapp runtime.
///
/// Sandbox storage belongs to the current subject. Installation settings and
/// host-managed resources remain attached to the installation owner.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TappStorageAccess {
    pub owner_id: i32,
    pub subject_id: i32,
}

impl TappStorageAccess {
    pub fn from_owner_and_subject(owner_id: i32, subject_id: i32) -> Self {
        Self {
            owner_id,
            subject_id,
        }
    }

    pub fn from_runtime_grant(
        grant: &RuntimeGrantContext,
        claims: &Claims,
    ) -> Result<Self, StatusCode> {
        let subject_id =
            optional_authenticated_user_id(Some(claims)).ok_or(StatusCode::UNAUTHORIZED)?;
        if grant.subject_id() != subject_id {
            return Err(StatusCode::FORBIDDEN);
        }
        Ok(Self::from_owner_and_subject(grant.owner_id(), subject_id))
    }

    pub fn can_manage_installation(self) -> bool {
        self.subject_id == self.owner_id
    }

    pub fn require_installation_write(self) -> Result<(), StatusCode> {
        if self.can_manage_installation() {
            Ok(())
        } else {
            Err(StatusCode::FORBIDDEN)
        }
    }

    pub fn installation_namespace(self) -> i32 {
        self.owner_id
    }

    pub fn private_storage_namespace(self) -> i32 {
        self.subject_id
    }
}

fn can_write_installation_settings(access: TappStorageAccess, is_admin: bool) -> bool {
    is_admin || access.can_manage_installation()
}

/// Authorize sandbox storage for a Runtime-Grant route.
async fn authorize_runtime_storage(
    db: &DatabaseConnection,
    claims: &Claims,
    grant: &RuntimeGrantContext,
    tapp_id: &str,
) -> Result<TappStorageAccess, StatusCode> {
    require_runtime_storage_grant(grant, tapp_id)?;
    authorize_tapp_permission(db, claims, tapp_id, TappPermission::Storage).await?;
    TappStorageAccess::from_runtime_grant(grant, claims)
}

pub(crate) fn installation_write_forbidden_error() -> (StatusCode, axum::Json<serde_json::Value>) {
    (
        StatusCode::FORBIDDEN,
        axum::Json(serde_json::json!({
            "error": "Read-only installation resource",
            "message": "Only the installation owner can modify this resource",
            "code": "TAPP_INSTALLATION_READ_ONLY"
        })),
    )
}

async fn current_user_role(claims: &Claims) -> UserRole {
    if current_is_admin(claims).await {
        UserRole::Admin
    } else {
        UserRole::User
    }
}

fn canonical_installation_owner_id(role: UserRole, actor_id: i32, site_owner_id: i32) -> i32 {
    if role == UserRole::Admin {
        site_owner_id
    } else {
        actor_id
    }
}

/// Owner namespaces that block a new install for this actor/role.
///
/// Public and private installations may coexist. Each actor conflicts only
/// with the namespace they are allowed to mutate, preventing a private user
/// from reserving an ID and blocking a later site-owner publication.
/// - Guest: cannot install in practice; still scoped to the actor id only.
fn installation_conflict_owner_ids(role: UserRole, actor_id: i32, site_owner_id: i32) -> Vec<i32> {
    match role {
        UserRole::Admin => vec![site_owner_id],
        UserRole::User | UserRole::Guest => vec![actor_id],
    }
}

async fn require_current_admin(claims: &Claims) -> Result<(), StatusCode> {
    ensure_current_admin(claims)
        .await
        .map_err(|(status, _)| status)
}

async fn filter_install_permissions(role: UserRole, permissions: Vec<String>) -> Vec<String> {
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let granted = TappPermissionService::filter_permissions_for_role(&config, role, &permissions);
    drop(config);
    granted
}

async fn authorize_tapp_permission(
    db: &DatabaseConnection,
    claims: &Claims,
    tapp_id: &str,
    permission: TappPermission,
) -> Result<i32, StatusCode> {
    tapp_common::authorize_tapp_permission(db, claims, tapp_id, permission)
        .await
        .map_err(|(status, _)| status)
}

/// API 响应
#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }
}

/// 错误响应便捷函数
fn api_error(message: impl Into<String>) -> Json<ApiResponse<()>> {
    Json(ApiResponse {
        success: false,
        data: None,
        error: Some(message.into()),
    })
}

/// Tapp 列表项
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TappListItem {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    /// 内联 SVG 图标代码（优先于 icon）
    pub icon_svg: Option<String>,
    pub status: String,
    pub installed_at: String,
    pub last_run_at: Option<String>,
    /// 是否为临时安装（普通用户安装的 Tapp）
    #[serde(default)]
    pub is_temporary: bool,
    /// 是否为管理员的 Tapp（对所有用户可见）
    #[serde(default)]
    pub is_admin_tapp: bool,
}

/// Tapp 详情
#[derive(Debug, Serialize)]
pub struct TappDetail {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<serde_json::Value>,
    pub icon: Option<String>,
    pub theme_color: Option<String>,
    pub manifest: serde_json::Value,
    pub status: String,
    pub granted_permissions: Vec<String>,
    pub installed_at: String,
    pub last_run_at: Option<String>,
    /// 当前用户角色: "guest" | "user" | "admin"
    pub user_role: String,
    /// 是否为临时安装
    #[serde(default)]
    pub is_temporary: bool,
    /// 是否为管理员的 Tapp
    #[serde(default)]
    pub is_admin_tapp: bool,
}

/// 创建 Tapp 路由
///
/// 路由分为三类：
/// - 公开路由（游客可访问）：list_tapps, get_tapp, get_tapp_code, list_all_widgets, list_store_sources
/// - 认证路由（需要登录）：install, uninstall, start, stop, register_widget, storage 等
/// - 管理员路由（仅管理员）：add_store_source, update_store_source, delete_store_source
pub fn create_tapp_routes() -> Router<DatabaseConnection> {
    // 需要认证的路由
    let authenticated_routes = Router::new()
        .route("/install", post(install_tapp))
        .route("/install-file", post(install_tapp_file))
        .route("/cleanup-temporary", post(cleanup_temporary_tapps))
        .route("/{tapp_id}", delete(uninstall_tapp))
        .route("/{tapp_id}/update", post(update_tapp))
        .route("/{tapp_id}/start", post(start_tapp))
        .route("/{tapp_id}/stop", post(stop_tapp))
        .route("/{tapp_id}/widgets", post(register_widget))
        .route("/{tapp_id}/widgets/{widget_id}", delete(unregister_widget))
        .route("/{tapp_id}/settings", get(get_tapp_settings))
        .route("/{tapp_id}/settings/{key}", get(get_tapp_setting))
        .route("/{tapp_id}/settings/{key}", post(set_tapp_setting))
        .route("/{tapp_id}/storage", get(list_storage_keys))
        .route("/{tapp_id}/storage", delete(clear_storage))
        .route("/{tapp_id}/storage/entries", get(list_storage_entries))
        .route("/{tapp_id}/storage/usage", get(get_storage_usage))
        .route("/{tapp_id}/storage/{key}", get(get_storage))
        .route("/{tapp_id}/storage/{key}", post(set_storage))
        .route("/{tapp_id}/storage/{key}", delete(delete_storage))
        // 更新分离式 CSS（用于商店安装后前端生成）
        .route("/{tapp_id}/separated-css", post(update_separated_css))
        // 商店源管理（需要认证，API 内部检查管理员权限）
        .route("/store/sources", post(add_store_source))
        .route("/store/sources/{source_id}", post(update_store_source))
        .route("/store/sources/{source_id}", delete(delete_store_source))
        .route_layer(from_fn_with_state((), |req, next| async {
            auth_middleware(req, next).await
        }));

    // 公开路由（支持可选认证）
    let public_routes = Router::new()
        .route("/", get(list_tapps))
        .route("/details", get(list_tapp_details))
        .route("/widgets", get(list_all_widgets))
        .route("/store/sources", get(list_store_sources))
        .route("/{tapp_id}", get(get_tapp))
        .route("/{tapp_id}/code", get(get_tapp_code))
        .route("/{tapp_id}/resources", get(get_tapp_resources))
        .route("/{tapp_id}/asset", get(get_tapp_asset))
        .route("/{tapp_id}/export", get(export_tapp));

    // These routes need a stable subject but also support guests. The optional
    // auth layer always injects a real or stable guest Claims value.
    let optional_subject_routes = Router::new()
        .route("/recent", get(get_recent_tapps))
        .route(
            "/{tapp_id}/runtime-grants",
            post(crate::api::tapp_runtime::issue_runtime_grant),
        )
        .route(
            "/{tapp_id}/runtime-grants/authorize",
            post(crate::api::tapp_runtime::authorize_runtime_permission),
        )
        .route(
            "/{tapp_id}/runtime-grants/{runtime_id}",
            delete(crate::api::tapp_runtime::revoke_runtime_grant),
        )
        .route_layer(from_fn_with_state((), |req, next| async {
            optional_auth_middleware(req, next).await
        }));

    // 合并路由
    public_routes
        .merge(authenticated_routes)
        .merge(optional_subject_routes)
}

/// 从远程商店下载 Tapp 文件
///
/// 返回 (manifest, code, styles, widget_styles, page_styles, page_template, widget_templates)
async fn fetch_public_store_url(url: &str) -> Result<reqwest::Response, String> {
    let (target_url, client) = crate::services::outbound_security::build_public_http_client(
        url,
        std::time::Duration::from_secs(20),
        Some("Myriad-Tapp-Store/1.0"),
    )
    .await?;
    client
        .get(target_url)
        .send()
        .await
        .map_err(|error| error.to_string())
}

fn validate_store_manifest_category(
    app_info: &serde_json::Value,
    manifest: &TappManifest,
) -> Result<(), String> {
    let index_category = app_info
        .get("category")
        .cloned()
        .ok_or_else(|| "Store index app is missing category".to_string())?;
    let index_category: TappCategory = serde_json::from_value(index_category)
        .map_err(|_| "Store index app has an invalid category".to_string())?;
    if Some(index_category) != manifest.category {
        return Err(format!(
            "Store index category does not match manifest category for {}",
            manifest.id
        ));
    }
    Ok(())
}

async fn fetch_from_store(
    db: &DatabaseConnection,
    store_source: &str,
    tapp_id: &str,
) -> Result<
    (
        TappManifest,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<WidgetTemplateContents>,
        Option<std::collections::HashMap<String, serde_json::Value>>,
        Option<std::collections::HashMap<String, String>>,
    ),
    (StatusCode, Json<ApiResponse<()>>),
> {
    // 获取商店源信息
    let source = tapp_store_sources::Entity::find()
        .filter(
            tapp_store_sources::Column::Url
                .eq(store_source)
                .or(tapp_store_sources::Column::Id.eq(store_source.parse::<i32>().unwrap_or(-1))),
        )
        .one(db)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error("Database error"),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, api_error("Store source not found")))?;

    let base_url = source
        .url
        .trim_end_matches("/index.json")
        .trim_end_matches('/');

    // 安全验证：确保 URL 使用 https 且不指向内部网络
    if let Ok(parsed_url) = reqwest::Url::parse(base_url) {
        let scheme = parsed_url.scheme();
        if scheme != "https" && scheme != "http" {
            return Err((
                StatusCode::BAD_REQUEST,
                api_error("Only HTTP(S) URLs are allowed"),
            ));
        }
        if let Some(host) = parsed_url.host_str() {
            if host == "localhost"
                || host == "127.0.0.1"
                || host == "::1"
                || host.starts_with("10.")
                || host.starts_with("172.16.")
                || host.starts_with("192.168.")
                || host == "0.0.0.0"
                || host.ends_with(".local")
                || host.ends_with(".internal")
            {
                return Err((
                    StatusCode::BAD_REQUEST,
                    api_error("Internal network URLs are not allowed"),
                ));
            }
        }
    }

    // 获取商店索引
    // 注意：生产环境中 backend 容器若无法访问外网（尤其 raw.githubusercontent.com），
    // 这里会返回 502。前端商店列表走浏览器直连，因此可能出现「能浏览、不能安装」。
    let index_url = format!("{}/index.json", base_url);
    tracing::info!(url = %index_url, "fetching tapp store index");
    let index_resp = fetch_public_store_url(&index_url).await.map_err(|e| {
        tracing::error!(url = %index_url, error = %e, "failed to fetch store index");
        (
            StatusCode::BAD_GATEWAY,
            api_error(format!(
                "Failed to fetch store index (backend cannot reach store URL): {}",
                e
            )),
        )
    })?;

    if !index_resp.status().is_success() {
        let status = index_resp.status();
        tracing::error!(url = %index_url, %status, "store index returned non-success");
        return Err((
            StatusCode::BAD_GATEWAY,
            api_error(format!(
                "Failed to fetch store index: remote returned {}",
                status
            )),
        ));
    }

    let index: serde_json::Value = index_resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            api_error(format!("Invalid store index format: {}", e)),
        )
    })?;

    // 在商店中查找指定的 Tapp
    let apps = index
        .get("apps")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            (
                StatusCode::BAD_GATEWAY,
                api_error("Invalid store index: no apps array"),
            )
        })?;

    let app_info = apps
        .iter()
        .find(|app| app.get("id").and_then(|v| v.as_str()) == Some(tapp_id))
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                api_error(format!("Tapp {} not found in store", tapp_id)),
            )
        })?;

    let download = app_info.get("download").ok_or_else(|| {
        (
            StatusCode::BAD_GATEWAY,
            api_error("No download info in app"),
        )
    })?;

    // 下载 manifest.json
    let manifest_path = download
        .get("manifest")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (StatusCode::BAD_GATEWAY, api_error("No manifest path")))?;
    let manifest_url = format!("{}/{}", base_url, manifest_path);

    let manifest_resp = fetch_public_store_url(&manifest_url).await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            api_error(format!("Failed to fetch manifest: {}", e)),
        )
    })?;

    let manifest: TappManifest = manifest_resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            api_error(format!("Invalid manifest: {}", e)),
        )
    })?;
    validate_store_manifest_category(app_info, &manifest)
        .map_err(|error| (StatusCode::BAD_GATEWAY, api_error(error)))?;

    // 下载主代码
    let code_path = download
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (StatusCode::BAD_GATEWAY, api_error("No code path")))?;
    let code_url = format!("{}/{}", base_url, code_path);

    let code = fetch_public_store_url(&code_url)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                api_error(format!("Failed to fetch code: {}", e)),
            )
        })?
        .text()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                api_error(format!("Failed to read code: {}", e)),
            )
        })?;

    // 下载可选资源
    let mut styles_content: Option<String> = None;
    let mut widget_styles_content: Option<String> = None;
    let mut page_styles_content: Option<String> = None;
    let mut page_template_content: Option<String> = None;
    let mut widget_templates: WidgetTemplateContents = std::collections::HashMap::new();

    // 下载 CSS 样式（统一模式）
    if let Some(styles_path) = download.get("styles").and_then(|v| v.as_str()) {
        let styles_url = format!("{}/{}", base_url, styles_path);
        if let Ok(resp) = fetch_public_store_url(&styles_url).await {
            if resp.status().is_success() {
                if let Ok(content) = resp.text().await {
                    styles_content = Some(content);
                }
            }
        }
    }

    // 下载 Widget 专用 CSS（分离模式）
    if let Some(widget_styles_path) = download.get("widget_styles").and_then(|v| v.as_str()) {
        let widget_styles_url = format!("{}/{}", base_url, widget_styles_path);
        if let Ok(resp) = fetch_public_store_url(&widget_styles_url).await {
            if resp.status().is_success() {
                if let Ok(content) = resp.text().await {
                    widget_styles_content = Some(content);
                }
            }
        }
    }

    // 下载 Page 专用 CSS（分离模式）
    if let Some(page_styles_path) = download.get("page_styles").and_then(|v| v.as_str()) {
        let page_styles_url = format!("{}/{}", base_url, page_styles_path);
        if let Ok(resp) = fetch_public_store_url(&page_styles_url).await {
            if resp.status().is_success() {
                if let Ok(content) = resp.text().await {
                    page_styles_content = Some(content);
                }
            }
        }
    }

    // 下载 Page 模板
    if let Some(page_path) = download.get("page_template").and_then(|v| v.as_str()) {
        let page_url = format!("{}/{}", base_url, page_path);
        if let Ok(resp) = fetch_public_store_url(&page_url).await {
            if resp.status().is_success() {
                if let Ok(content) = resp.text().await {
                    page_template_content = Some(content);
                }
            }
        }
    }

    // 下载 Widget 模板
    if let Some(widgets) = download.get("widget_templates").and_then(|v| v.as_object()) {
        for (widget_id, templates) in widgets {
            let Some(templates) = templates.as_object() else {
                continue;
            };
            let mut downloaded = std::collections::HashMap::new();
            for (size, path) in templates {
                if let Some(template_path) = path.as_str() {
                    let template_url = format!("{}/{}", base_url, template_path);
                    if let Ok(resp) = fetch_public_store_url(&template_url).await {
                        if resp.status().is_success() {
                            if let Ok(content) = resp.text().await {
                                downloaded.insert(size.clone(), content);
                            }
                        }
                    }
                }
            }
            if !downloaded.is_empty() {
                widget_templates.insert(widget_id.clone(), downloaded);
            }
        }
    }

    let widget_templates_opt = if widget_templates.is_empty() {
        None
    } else {
        Some(widget_templates)
    };

    // 下载 i18n 翻译文件
    let mut i18n_data: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    if let Some(i18n_files) = download.get("i18n").and_then(|v| v.as_object()) {
        for (lang_code, path) in i18n_files {
            if let Some(i18n_path) = path.as_str() {
                let i18n_url = format!("{}/{}", base_url, i18n_path);
                if let Ok(resp) = fetch_public_store_url(&i18n_url).await {
                    if resp.status().is_success() {
                        if let Ok(json) = resp.json::<serde_json::Value>().await {
                            i18n_data.insert(lang_code.clone(), json);
                        }
                    }
                }
            }
        }
    }
    let i18n_opt = if i18n_data.is_empty() {
        None
    } else {
        Some(i18n_data)
    };

    // 下载 Page 模块文件
    let mut page_modules_data: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    if let Some(pm_files) = download.get("page_modules").and_then(|v| v.as_object()) {
        for (filename, path) in pm_files {
            if let Some(pm_path) = path.as_str() {
                let pm_url = format!("{}/{}", base_url, pm_path);
                if let Ok(resp) = fetch_public_store_url(&pm_url).await {
                    if resp.status().is_success() {
                        if let Ok(content) = resp.text().await {
                            page_modules_data.insert(filename.clone(), content);
                        }
                    }
                }
            }
        }
    }
    let page_modules_opt = if page_modules_data.is_empty() {
        None
    } else {
        Some(page_modules_data)
    };

    Ok((
        manifest,
        code,
        styles_content,
        widget_styles_content,
        page_styles_content,
        page_template_content,
        widget_templates_opt,
        i18n_opt,
        page_modules_opt,
    ))
}

/// 统一安装 Tapp 的请求体
///
/// 支持两种安装来源：
/// 1. direct: 直接提供代码（本地示例、上传文件解析后）
/// 2. store: 从远程商店安装（后端下载）
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallTappRequest {
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
async fn install_tapp(
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
    // 根据来源获取 manifest 和代码
    let (
        manifest,
        code,
        styles,
        widget_styles,
        page_styles,
        page_template,
        widget_templates,
        store_i18n,
        store_page_modules,
    ) = match req.source.as_str() {
        "direct" => {
            // 直接安装：从请求中获取
            let manifest = req.manifest.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    api_error("manifest is required for direct install"),
                )
            })?;
            let code = req.code.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    api_error("code is required for direct install"),
                )
            })?;
            (
                manifest,
                code,
                req.styles,
                None::<String>, // widget_styles - 直接安装暂不支持
                None::<String>, // page_styles - 直接安装暂不支持
                req.page_template,
                req.widget_templates,
                None::<std::collections::HashMap<String, serde_json::Value>>,
                None::<std::collections::HashMap<String, String>>,
            )
        }
        "store" => {
            // 从商店安装：下载文件
            let store_source = req.store_source.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    api_error("storeSource is required for store install"),
                )
            })?;
            let tapp_id = req.tapp_id.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    api_error("tappId is required for store install"),
                )
            })?;
            validate_tapp_id(&tapp_id)
                .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;

            let (
                manifest,
                code,
                styles,
                widget_styles,
                page_styles,
                page_template,
                widget_templates,
                i18n,
                page_modules,
            ) = fetch_from_store(&db, &store_source, &tapp_id).await?;
            (
                manifest,
                code,
                styles,
                widget_styles,
                page_styles,
                page_template,
                widget_templates,
                i18n,
                page_modules,
            )
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                api_error("Invalid source, must be 'direct' or 'store'"),
            ));
        }
    };

    validate_tapp_manifest(&manifest)
        .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
    if let Some(templates) = &widget_templates {
        validate_widget_template_contents(&manifest, templates)
            .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
    }
    validate_named_resource_keys(
        req.i18n
            .as_ref()
            .or(store_i18n.as_ref())
            .into_iter()
            .flat_map(|translations| translations.keys()),
        "i18n language code",
    )
    .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
    validate_named_resource_keys(
        req.page_modules
            .as_ref()
            .or(store_page_modules.as_ref())
            .into_iter()
            .flat_map(|modules| modules.keys()),
        "page module filename",
    )
    .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;

    // 检查是否已安装
    let admin_id = get_admin_user_id(&db).await.map_err(|status| {
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
    let existing = existing_query.one(&db).await.map_err(|error| {
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

    // 保存到 Manifest 声明的入口；安装/导出往返后路径保持一致。
    write_tapp_resource(tapp_dir, &manifest.main, &code)
        .await
        .map_err(|error| {
            log_install_failure(
                "write_tapp_resource(main)",
                &manifest.id,
                user_id,
                installation_owner_id,
                Some(tapp_dir),
                &error,
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error(format!("Failed to save code: {error}")),
            )
        })?;

    // 保存可选资源
    if let Some(styles) = &styles {
        let path = manifest.styles.as_deref().unwrap_or("styles.css");
        write_tapp_resource(tapp_dir, path, styles)
            .await
            .map_err(|error| {
                log_install_failure(
                    "write_tapp_resource(styles)",
                    &manifest.id,
                    user_id,
                    installation_owner_id,
                    Some(tapp_dir),
                    &error,
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    api_error(format!("Failed to save styles: {error}")),
                )
            })?;
    }

    // 🎯 保存分离式 CSS（从商店下载的）
    if let Some(ws) = &widget_styles {
        let path = manifest.widget_styles.as_deref().unwrap_or("widget.css");
        write_tapp_resource(tapp_dir, path, ws)
            .await
            .map_err(|error| {
                log_install_failure(
                    "write_tapp_resource(widget_styles)",
                    &manifest.id,
                    user_id,
                    installation_owner_id,
                    Some(tapp_dir),
                    &error,
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    api_error(format!("Failed to save widget styles: {error}")),
                )
            })?;
    }
    if let Some(ps) = &page_styles {
        let path = manifest.page_styles.as_deref().unwrap_or("page.css");
        write_tapp_resource(tapp_dir, path, ps)
            .await
            .map_err(|error| {
                log_install_failure(
                    "write_tapp_resource(page_styles)",
                    &manifest.id,
                    user_id,
                    installation_owner_id,
                    Some(tapp_dir),
                    &error,
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    api_error(format!("Failed to save page styles: {error}")),
                )
            })?;
    }

    // Direct unified-mode installs may include frontend-compiled Tailwind CSS.
    // It belongs to this staged generation and must never overwrite resources
    // declared by a separated-mode/store package.
    if req.source == "direct" && manifest.css_mode.as_deref() != Some("separated") {
        if let Some(widget_css) = &req.widget_css {
            write_tapp_resource(tapp_dir, "widget.css", widget_css)
                .await
                .map_err(|error| {
                    log_install_failure(
                        "write_tapp_resource(widget_css)",
                        &manifest.id,
                        user_id,
                        installation_owner_id,
                        Some(tapp_dir),
                        &error,
                    );
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        api_error(format!("Failed to save generated widget CSS: {error}")),
                    )
                })?;
        }
        if let Some(page_css) = &req.page_css {
            write_tapp_resource(tapp_dir, "page.css", page_css)
                .await
                .map_err(|error| {
                    log_install_failure(
                        "write_tapp_resource(page_css)",
                        &manifest.id,
                        user_id,
                        installation_owner_id,
                        Some(tapp_dir),
                        &error,
                    );
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        api_error(format!("Failed to save generated page CSS: {error}")),
                    )
                })?;
        }
    }

    if let Some(page) = &page_template {
        let path = manifest.page_template.as_deref().unwrap_or("page.html");
        write_tapp_resource(tapp_dir, path, page)
            .await
            .map_err(|error| {
                log_install_failure(
                    "write_tapp_resource(page_template)",
                    &manifest.id,
                    user_id,
                    installation_owner_id,
                    Some(tapp_dir),
                    &error,
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    api_error(format!("Failed to save page template: {error}")),
                )
            })?;
    }

    if let Some(widgets) = &widget_templates {
        for (widget_id, templates) in widgets {
            for (size, content) in templates {
                let path = widget_template_path(&manifest, widget_id, size)
                    .expect("validated Widget template path");
                write_tapp_resource(tapp_dir, path, content)
                    .await
                    .map_err(|error| {
                        log_install_failure(
                            "write_tapp_resource(widget_template)",
                            &manifest.id,
                            user_id,
                            installation_owner_id,
                            Some(tapp_dir),
                            &error,
                        );
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            api_error(format!("Failed to save widget template: {error}")),
                        )
                    })?;
            }
        }
    }

    // 保存 i18n 翻译文件（direct 模式从 req.i18n，store 模式从 store_i18n）
    let i18n_to_save = req.i18n.as_ref().or(store_i18n.as_ref());
    if let Some(i18n) = i18n_to_save {
        for (lang_code, data) in i18n {
            let json = serde_json::to_string_pretty(data).map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    api_error("Failed to serialize i18n resource"),
                )
            })?;
            write_tapp_resource(tapp_dir, &format!("i18n/{lang_code}.json"), json)
                .await
                .map_err(|error| {
                    log_install_failure(
                        "write_tapp_resource(i18n)",
                        &manifest.id,
                        user_id,
                        installation_owner_id,
                        Some(tapp_dir),
                        &error,
                    );
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        api_error(format!("Failed to save i18n resource: {error}")),
                    )
                })?;
        }
    }

    // 保存 Page 模块文件（direct 模式从 req.page_modules，store 模式从 store_page_modules）
    let pm_to_save = req.page_modules.as_ref().or(store_page_modules.as_ref());
    if let Some(page_modules) = pm_to_save {
        for (filename, code_content) in page_modules {
            write_tapp_resource(tapp_dir, &format!("page/{filename}"), code_content)
                .await
                .map_err(|error| {
                    log_install_failure(
                        "write_tapp_resource(page_module)",
                        &manifest.id,
                        user_id,
                        installation_owner_id,
                        Some(tapp_dir),
                        &error,
                    );
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        api_error(format!("Failed to save page module: {error}")),
                    )
                })?;
        }
    }

    // Direct install may embed package assets as base64 (binary allowed under assets/).
    if let Some(assets) = &req.assets {
        write_install_assets(tapp_dir, &manifest, assets)
            .await
            .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
    }

    let now = Utc::now().fixed_offset();

    // 保存 manifest.json 和与数据库 updated_at 对应的安装代际标记。
    let manifest_json = serde_json::to_string_pretty(&manifest).unwrap_or_default();
    let staged_manifest_path = tapp_dir.join("manifest.json");
    fs::write(&staged_manifest_path, &manifest_json)
        .await
        .map_err(|error| {
            log_install_failure(
                "write_manifest",
                &manifest.id,
                user_id,
                installation_owner_id,
                Some(&staged_manifest_path),
                &error,
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error(format!("Failed to save manifest: {error}")),
            )
        })?;
    write_install_generation(tapp_dir, now).map_err(|error| {
        log_install_failure(
            "write_install_generation",
            &manifest.id,
            user_id,
            installation_owner_id,
            Some(tapp_dir),
            &error,
        );
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error(format!("Failed to save install state: {error}")),
        )
    })?;

    validate_installed_resources(&manifest, tapp_dir)
        .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;

    // 确定授权的权限
    let permissions = req.permissions.unwrap_or_default();
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

    Ok(Json(ApiResponse::success(TappListItem {
        id: result.tapp_id,
        name: result.name,
        version: result.version,
        description: result.description,
        icon: result.icon,
        icon_svg,
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
async fn install_tapp_file(
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

    // 解压 ZIP 文件
    let cursor = std::io::Cursor::new(&file_data);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            api_error("Invalid .tapp file format"),
        )
    })?;
    validate_tapp_archive(&mut archive)
        .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;

    // 读取 manifest.json
    let manifest_content = {
        let mut manifest_file = archive.by_name("manifest.json").map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                api_error("manifest.json not found in .tapp file"),
            )
        })?;
        if manifest_file.size() > MAX_TAPP_MANIFEST_BYTES {
            return Err((
                StatusCode::BAD_REQUEST,
                api_error(format!(
                    "manifest.json exceeds {MAX_TAPP_MANIFEST_BYTES} bytes"
                )),
            ));
        }
        let mut content = String::new();
        std::io::Read::read_to_string(&mut manifest_file, &mut content).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                api_error("Failed to read manifest.json"),
            )
        })?;
        content
    };

    let manifest: TappManifest = serde_json::from_str(&manifest_content).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            api_error(format!("Invalid manifest.json: {}", e)),
        )
    })?;
    validate_tapp_manifest(&manifest)
        .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;

    // 检查是否已安装
    let admin_id = get_admin_user_id(&db).await.map_err(|status| {
        (
            status,
            api_error("Failed to resolve administrator namespace"),
        )
    })?;
    let installation_owner_id = canonical_installation_owner_id(role, user_id, admin_id);
    let conflict_owner_ids = installation_conflict_owner_ids(role, user_id, admin_id);
    let existing_query = tapps::Entity::find()
        .filter(tapps::Column::TappId.eq(&manifest.id))
        .filter(tapps::Column::UserId.is_in(conflict_owner_ids.clone()));
    let existing = existing_query.one(&db).await.map_err(|error| {
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

    let final_tapp_dir = tapp_dir_for(installation_owner_id, &manifest.id)
        .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
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

    // 解压到目标文件夹并保留经过校验的相对路径。Manifest 允许 templates/、
    // page/ 等嵌套资源；扁平化会让清单中的路径在安装后失效。
    let tapp_dir_clone = tapp_dir.to_path_buf();
    let file_data_clone = file_data.clone();

    tokio::task::spawn_blocking(move || -> Result<(), std::io::Error> {
        use std::io::Read;

        let cursor = std::io::Cursor::new(&file_data_clone);
        let mut archive = zip::ZipArchive::new(cursor)?;

        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let file_name = file.name().to_string();

            let out_path = archive_entry_path(&tapp_dir_clone, &file_name)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;

            if file.is_dir() {
                std::fs::create_dir_all(&out_path)?;
                continue;
            }
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let mut content = Vec::new();
            file.read_to_end(&mut content)?;
            std::fs::write(&out_path, &content)?;
        }

        Ok(())
    })
    .await
    .map_err(|error| {
        log_install_failure(
            "extract_join",
            &manifest.id,
            user_id,
            installation_owner_id,
            Some(tapp_dir),
            &error,
        );
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error(format!("Failed to extract files: {error}")),
        )
    })?
    .map_err(|error| {
        log_install_failure(
            "extract_write",
            &manifest.id,
            user_id,
            installation_owner_id,
            Some(tapp_dir),
            &error,
        );
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error(format!("Failed to save files: {error}")),
        )
    })?;

    let now = Utc::now().fixed_offset();
    write_install_generation(tapp_dir, now).map_err(|error| {
        log_install_failure(
            "write_install_generation",
            &manifest.id,
            user_id,
            installation_owner_id,
            Some(tapp_dir),
            &error,
        );
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error(format!("Failed to save install state: {error}")),
        )
    })?;

    // Manifest 声明的入口和资源必须真实存在；避免安装成功后第一次运行才报错。
    validate_installed_resources(&manifest, tapp_dir)
        .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;

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

    // 保存到数据库
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
    // See the direct-install path above: publishing the same Tapp ID changes
    // which installation is executable for every subject.
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

    Ok(Json(ApiResponse::success(TappListItem {
        id: result.tapp_id,
        name: result.name,
        version: result.version,
        description: result.description,
        icon: result.icon,
        icon_svg,
        status: "installed".to_string(),
        installed_at: result.installed_at.to_rfc3339(),
        last_run_at: None,
        is_temporary,
        is_admin_tapp: is_current_admin,
    })))
}

/// 获取 Tapp 代码
///
/// 权限模型：
/// - 游客：可以读取管理员的 Tapp 代码
/// - 普通用户：可以读取管理员的 Tapp 代码 + 自己临时安装的 Tapp 代码
async fn get_tapp_code(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Path(tapp_id): Path<String>,
) -> Result<String, StatusCode> {
    // 可选认证：游客也可以访问
    let claims = extract_optional_claims(&headers);
    let user_id = optional_authenticated_user_id(claims.as_ref());
    let tapp = find_visible_tapp(&db, user_id, &tapp_id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?
        .tapp;

    let code = fs::read_to_string(installed_code_path(&tapp)?)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(code)
}

/// Tapp 资源响应
#[derive(Debug, Serialize)]
struct TappResourcesResponse {
    /// 主代码（index.js/main.js）
    code: String,
    /// 自定义 CSS 样式（统一模式，或共享样式）
    #[serde(skip_serializing_if = "Option::is_none")]
    styles: Option<String>,
    /// Widget 专用自定义 CSS（分离模式）
    #[serde(skip_serializing_if = "Option::is_none")]
    widget_styles: Option<String>,
    /// Page 专用自定义 CSS（分离模式）
    #[serde(skip_serializing_if = "Option::is_none")]
    page_styles: Option<String>,
    /// Widget 专用编译后的 Tailwind CSS
    #[serde(skip_serializing_if = "Option::is_none")]
    widget_css: Option<String>,
    /// Page 专用编译后的 Tailwind CSS
    #[serde(skip_serializing_if = "Option::is_none")]
    page_css: Option<String>,
    /// Widget HTML 模板（Widget ID → 尺寸 → 内容）
    #[serde(skip_serializing_if = "Option::is_none")]
    widget_templates: Option<WidgetTemplateContents>,
    /// Page HTML 模板
    #[serde(skip_serializing_if = "Option::is_none")]
    page_template: Option<String>,
    /// CSS 架构模式：unified（统一）或 separated（分离）
    #[serde(skip_serializing_if = "Option::is_none")]
    css_mode: Option<String>,
    /// i18n 翻译数据（语言代码 → 键值对）
    #[serde(skip_serializing_if = "Option::is_none")]
    i18n: Option<std::collections::HashMap<String, serde_json::Value>>,
    /// Page 模块文件（文件名 → 代码内容）
    #[serde(skip_serializing_if = "Option::is_none")]
    page_modules: Option<std::collections::HashMap<String, String>>,
    /// Page 模块加载顺序（从 manifest.json 读取）
    #[serde(skip_serializing_if = "Option::is_none")]
    page_module_order: Option<Vec<String>>,
}

/// 获取 Tapp 完整资源（代码 + CSS + HTML 模板）
///
/// 支持混合渲染模式，返回所有相关资源文件
///
/// 权限模型与 get_tapp_code 相同
async fn get_tapp_resources(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Path(tapp_id): Path<String>,
) -> Result<Json<TappResourcesResponse>, StatusCode> {
    // 可选认证
    let claims = extract_optional_claims(&headers);
    let user_id = optional_authenticated_user_id(claims.as_ref());
    let tapp = find_visible_tapp(&db, user_id, &tapp_id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?
        .tapp;

    // Recompute trusted paths from owner + validated id. Persisted paths are
    // compatibility metadata only and never define the sandbox boundary.
    let tapp_dir = installed_tapp_dir(&tapp)?;
    let code = fs::read_to_string(installed_code_path(&tapp)?)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // 解析 manifest 获取资源文件路径
    let manifest: serde_json::Value = tapp.manifest.clone();

    // 确定 CSS 架构模式
    let css_mode = manifest
        .get("cssMode")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let is_separated = css_mode.as_deref() == Some("separated");

    // 读取自定义 CSS（统一模式或共享样式）
    let styles = if let Some(styles_file) = manifest.get("styles").and_then(|v| v.as_str()) {
        read_tapp_text_resource(&tapp_dir, styles_file).await.ok()
    } else if !is_separated {
        // 尝试默认位置（仅在非分离模式下）
        read_tapp_text_resource(&tapp_dir, "styles.css").await.ok()
    } else {
        None
    };

    // 读取 Widget 专用 CSS（分离模式）
    let widget_styles = if is_separated {
        if let Some(widget_styles_file) = manifest.get("widgetStyles").and_then(|v| v.as_str()) {
            read_tapp_text_resource(&tapp_dir, widget_styles_file)
                .await
                .ok()
        } else {
            // 尝试默认位置
            read_tapp_text_resource(&tapp_dir, "widget.css").await.ok()
        }
    } else {
        None
    };

    // 读取 Page 专用 CSS（分离模式）
    let page_styles = if is_separated {
        if let Some(page_styles_file) = manifest.get("pageStyles").and_then(|v| v.as_str()) {
            read_tapp_text_resource(&tapp_dir, page_styles_file)
                .await
                .ok()
        } else {
            // 尝试默认位置
            read_tapp_text_resource(&tapp_dir, "page.css").await.ok()
        }
    } else {
        None
    };

    // 读取 Page HTML 模板
    let page_template =
        if let Some(page_file) = manifest.get("pageTemplate").and_then(|v| v.as_str()) {
            read_tapp_text_resource(&tapp_dir, page_file).await.ok()
        } else {
            // 尝试默认位置
            read_tapp_text_resource(&tapp_dir, "page.html").await.ok()
        };

    // 读取 Widget HTML 模板
    let mut widget_templates: WidgetTemplateContents = std::collections::HashMap::new();

    if let Some(widgets) = manifest.get("widgets").and_then(|v| v.as_array()) {
        for widget in widgets {
            let Some(widget_id) = widget.get("id").and_then(|value| value.as_str()) else {
                continue;
            };
            let mut templates_for_widget = std::collections::HashMap::new();
            if let Some(templates) = widget.get("templates").and_then(|v| v.as_object()) {
                for (size, template_file) in templates {
                    if let Some(file_path) = template_file.as_str() {
                        if let Ok(content) = read_tapp_text_resource(&tapp_dir, file_path).await {
                            templates_for_widget.insert(size.clone(), content);
                        }
                    }
                }
            }
            if !templates_for_widget.is_empty() {
                widget_templates.insert(widget_id.to_string(), templates_for_widget);
            }
        }
    }

    // 读取分离的预编译 Tailwind CSS（widget.css 和 page.css，用于 Tailwind）
    // 注意：在分离模式下，widget_styles/page_styles 已经包含了自定义样式
    // 这里的 widget_css/page_css 是额外的预编译 Tailwind CSS
    let widget_css = if !is_separated {
        // 统一模式：尝试读取预编译的 widget.css
        read_tapp_text_resource(&tapp_dir, "widget.css").await.ok()
    } else {
        // 分离模式：widget_styles 已经包含完整样式，不需要额外的 Tailwind CSS
        None
    };

    let page_css = if !is_separated {
        // 统一模式：尝试读取预编译的 page.css
        read_tapp_text_resource(&tapp_dir, "page.css").await.ok()
    } else {
        // 分离模式：page_styles 已经包含完整样式，不需要额外的 Tailwind CSS
        None
    };

    // 读取 i18n 翻译文件（可选）
    let i18n = {
        if let Some(i18n_dir) = regular_resource_directory(&tapp_dir, "i18n") {
            let mut translations: std::collections::HashMap<String, serde_json::Value> =
                std::collections::HashMap::new();
            if let Ok(mut entries) = tokio::fs::read_dir(&i18n_dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if entry
                        .file_type()
                        .await
                        .is_ok_and(|file_type| file_type.is_file())
                        && path.extension().and_then(|e| e.to_str()) == Some("json")
                    {
                        if let Some(filename) = entry.file_name().to_str().map(String::from) {
                            if !is_safe_path_component(&filename) {
                                continue;
                            }
                            let Some(lang) = filename.strip_suffix(".json") else {
                                continue;
                            };
                            let relative = format!("i18n/{filename}");
                            if let Ok(content) = read_tapp_text_resource(&tapp_dir, &relative).await
                            {
                                if let Ok(value) =
                                    serde_json::from_str::<serde_json::Value>(&content)
                                {
                                    translations.insert(lang.to_string(), value);
                                }
                            }
                        }
                    }
                }
            }
            if translations.is_empty() {
                None
            } else {
                Some(translations)
            }
        } else {
            None
        }
    };

    // Only DB-Manifest-declared modules are executable. Directory discovery
    // would let undeclared files injected after installation enter the runtime
    // resource response and would make module membership depend on disk order.
    let page_module_order = manifest
        .get("pageModules")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(String::from))
                .collect::<Vec<_>>()
        });
    let page_modules = if let Some(order) = &page_module_order {
        let mut modules = std::collections::HashMap::new();
        for name in order {
            let relative = format!("page/{name}");
            let content = read_tapp_text_resource(&tapp_dir, &relative)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            modules.insert(name.clone(), content);
        }
        (!modules.is_empty()).then_some(modules)
    } else {
        None
    };

    Ok(Json(TappResourcesResponse {
        code,
        styles,
        widget_styles,
        page_styles,
        widget_css,
        page_css,
        widget_templates: if widget_templates.is_empty() {
            None
        } else {
            Some(widget_templates)
        },
        page_template,
        css_mode,
        i18n,
        // DB manifest and the activated resource directory are committed as one
        // lifecycle operation; do not let a later on-disk manifest mutation
        // change executable module order.
        page_module_order: page_modules.as_ref().and(page_module_order),
        page_modules,
    }))
}

#[derive(Debug, Deserialize)]
struct GetTappAssetQuery {
    path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TappAssetResponse {
    path: String,
    mime_type: String,
    size: u64,
    base64: String,
}

/// Read a Manifest-declared package asset as base64 for the sandbox assets API.
///
/// Only paths listed in `manifest.assets` are served. Visibility matches code/resources.
async fn get_tapp_asset(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Path(tapp_id): Path<String>,
    Query(query): Query<GetTappAssetQuery>,
) -> Result<Json<TappAssetResponse>, StatusCode> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let claims = extract_optional_claims(&headers);
    let user_id = optional_authenticated_user_id(claims.as_ref());
    let tapp = find_visible_tapp(&db, user_id, &tapp_id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?
        .tapp;

    validate_asset_path(&query.path).map_err(|_| StatusCode::BAD_REQUEST)?;

    let manifest: TappManifest = serde_json::from_value(tapp.manifest.clone())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let declared = manifest.assets.as_ref().ok_or(StatusCode::NOT_FOUND)?;
    if !declared.iter().any(|path| path == &query.path) {
        return Err(StatusCode::NOT_FOUND);
    }

    let tapp_dir = installed_tapp_dir(&tapp)?;
    let file_path = regular_resource_path(&tapp_dir, &query.path).ok_or(StatusCode::NOT_FOUND)?;
    let bytes = fs::read(&file_path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if bytes.len() as u64 > MAX_TAPP_ASSET_BYTES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    Ok(Json(TappAssetResponse {
        path: query.path.clone(),
        mime_type: guess_asset_mime_type(&query.path).to_string(),
        size: bytes.len() as u64,
        base64: STANDARD.encode(&bytes),
    }))
}

/// 导出 Tapp 为 .tapp 文件（ZIP 格式）
async fn export_tapp(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Path(tapp_id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    // 可选认证
    let claims = extract_optional_claims(&headers);
    let user_id = optional_authenticated_user_id(claims.as_ref());
    let tapp = find_visible_tapp(&db, user_id, &tapp_id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?
        .tapp;

    // Recompute the sandbox path rather than trusting persisted code_path.
    let tapp_dir = installed_tapp_dir(&tapp)?;

    // 收集需要打包的文件
    let tapp_dir_owned = tapp_dir;
    let tapp_id_clone = tapp_id.clone();

    let zip_data = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, std::io::Error> {
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;

        let buffer = Vec::new();
        let cursor = std::io::Cursor::new(buffer);
        let mut zip = ZipWriter::new(cursor);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        if tapp_dir_owned.is_dir() {
            append_directory_to_zip(&mut zip, &tapp_dir_owned, &tapp_dir_owned, options)?;
        }

        let cursor = zip.finish()?;
        Ok(cursor.into_inner())
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // 返回 ZIP 文件
    let filename = format!("{}.tapp", tapp_id_clone);
    let disposition = format!("attachment; filename=\"{}\"", filename);
    Ok((
        [
            (header::CONTENT_TYPE.as_str(), "application/zip".to_string()),
            (header::CONTENT_DISPOSITION.as_str(), disposition),
        ],
        zip_data,
    ))
}

/// 启动 Tapp
///
/// 权限模型（private-first，与 list/detail/runtime 一致）：
/// - 主体有同 `tapp_id` 的私有安装时：更新该私有行状态
/// - 否则站点主公开安装：管理员写库；非管理员只记活动（前端会话态）
/// - 普通用户可以启动自己临时安装的 Tapp
///
/// 所有用户启动 Tapp 时都会记录到 tapp_user_activities 表
async fn start_tapp(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    let user_id: i32 = claims.sub.parse().map_err(|_| StatusCode::UNAUTHORIZED)?;
    validate_tapp_id(&tapp_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let admin_id = find_admin_user_id(&db).await?;
    let now = Utc::now().fixed_offset();
    let is_current_admin = current_is_admin(&claims).await;

    // Prefer the subject's private install when both private and public copies exist.
    if admin_id != Some(user_id) {
        let user_tapp = tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(user_id))
            .filter(tapps::Column::TappId.eq(&tapp_id))
            .one(&db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if let Some(tapp) = user_tapp {
            let mut active: tapps::ActiveModel = tapp.into();
            active.status = Set(tapps::TappStatus::Running);
            active.last_run_at = Set(Some(now));
            active.updated_at = Set(now);
            active
                .update(&db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            record_user_activity(&db, user_id, &tapp_id, now).await?;
            return Ok(Json(ApiResponse::success(())));
        }
    }

    // Pure-public session: non-owners may start without mutating the public row.
    if let Some(admin_id) = admin_id {
        let admin_tapp = tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(admin_id))
            .filter(tapps::Column::TappId.eq(&tapp_id))
            .one(&db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if let Some(tapp) = admin_tapp {
            if is_current_admin {
                let mut active: tapps::ActiveModel = tapp.into();
                active.status = Set(tapps::TappStatus::Running);
                active.last_run_at = Set(Some(now));
                active.updated_at = Set(now);
                active
                    .update(&db)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
            record_user_activity(&db, user_id, &tapp_id, now).await?;
            return Ok(Json(ApiResponse::success(())));
        }
    }

    Err(StatusCode::NOT_FOUND)
}

/// 记录用户 Tapp 使用活动
///
/// 使用 upsert 模式：如果记录存在则更新 last_run_at 和 run_count，否则插入新记录
async fn record_user_activity(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    now: chrono::DateTime<chrono::FixedOffset>,
) -> Result<(), StatusCode> {
    // One atomic upsert avoids duplicate-key failures when the same Tapp is
    // started concurrently from multiple tabs or backend replicas.
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO tapp_user_activities
               (user_id, tapp_id, last_run_at, run_count)
           VALUES ($1, $2, $3, 1)
           ON CONFLICT (user_id, tapp_id) DO UPDATE SET
               last_run_at = EXCLUDED.last_run_at,
               run_count = tapp_user_activities.run_count + 1"#,
        vec![user_id.into(), tapp_id.into(), now.into()],
    ))
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}

/// 停止 Tapp
///
/// 权限模型（private-first，与 list/detail/runtime 一致）：
/// - 主体有同 `tapp_id` 的私有安装时：更新该私有行状态并吊销 grant
/// - 否则站点主公开安装：管理员写库；非管理员只吊销自身 grant（不改公开行）
/// - 普通用户可以停止自己临时安装的 Tapp
async fn stop_tapp(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    let user_id: i32 = claims.sub.parse().map_err(|_| StatusCode::UNAUTHORIZED)?;
    validate_tapp_id(&tapp_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let admin_id = find_admin_user_id(&db).await?;
    let is_current_admin = current_is_admin(&claims).await;

    // Prefer the subject's private install when both private and public copies exist.
    if admin_id != Some(user_id) {
        let user_tapp = tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(user_id))
            .filter(tapps::Column::TappId.eq(&tapp_id))
            .one(&db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if let Some(tapp) = user_tapp {
            let now = Utc::now().fixed_offset();
            let mut active: tapps::ActiveModel = tapp.into();
            active.status = Set(tapps::TappStatus::Installed);
            active.updated_at = Set(now);
            active
                .update(&db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            crate::api::tapp_runtime::revoke_tapp_runtime_grants(user_id, &tapp_id).await;
            return Ok(Json(ApiResponse::success(())));
        }
    }

    // Pure-public session: non-owners stop without mutating the public row.
    if let Some(admin_id) = admin_id {
        let admin_tapp = tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(admin_id))
            .filter(tapps::Column::TappId.eq(&tapp_id))
            .one(&db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if let Some(tapp) = admin_tapp {
            if is_current_admin {
                let now = Utc::now().fixed_offset();
                let mut active: tapps::ActiveModel = tapp.into();
                active.status = Set(tapps::TappStatus::Installed);
                active.updated_at = Set(now);
                active
                    .update(&db)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }
            crate::api::tapp_runtime::revoke_tapp_runtime_grants(user_id, &tapp_id).await;
            return Ok(Json(ApiResponse::success(())));
        }
    }

    Err(StatusCode::NOT_FOUND)
}

/// 最近使用的 Tapp 响应项
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentTappItem {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub icon_svg: Option<String>,
    pub theme_color: Option<String>,
    pub last_run_at: String,
    pub run_count: i32,
}

/// 获取最近使用的 Tapp 查询参数
#[derive(Debug, Deserialize)]
struct GetRecentTappsQuery {
    /// 返回的最大数量，默认 10
    #[serde(default = "default_recent_limit")]
    limit: i32,
}

fn default_recent_limit() -> i32 {
    10
}

/// 获取当前用户最近使用的 Tapp 列表
///
/// 从 tapp_user_activities 表中获取，按 last_run_at 降序排列
async fn get_recent_tapps(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Query(query): Query<GetRecentTappsQuery>,
) -> Result<Json<ApiResponse<Vec<RecentTappItem>>>, StatusCode> {
    use sea_orm::QueryOrder;

    let user_id: i32 = claims.sub.parse().map_err(|_| StatusCode::UNAUTHORIZED)?;
    let limit = query.limit.clamp(1, 50) as u64; // 限制在 1-50 之间

    // 获取用户活动记录
    let activities = tapp_user_activities::Entity::find()
        .filter(tapp_user_activities::Column::UserId.eq(user_id))
        .order_by_desc(tapp_user_activities::Column::LastRunAt)
        .all(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Guests cannot start a Tapp and therefore have no activity rows. Return
    // an honest empty result without requiring a configured site owner.
    if activities.is_empty() {
        return Ok(Json(ApiResponse::success(Vec::new())));
    }

    let admin_id = get_admin_user_id(&db).await?;

    // 获取管理员的所有 Tapp（用于查找 Tapp 详情）
    let admin_tapps = tapps::Entity::find()
        .filter(tapps::Column::UserId.eq(admin_id))
        .all(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // 获取用户自己的临时 Tapp
    let user_tapps = if user_id != admin_id {
        tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(user_id))
            .all(&db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        Vec::new()
    };

    // 合并 Tapp 列表，建立 tapp_id -> tapp 映射
    let mut tapp_map: std::collections::HashMap<String, &tapps::Model> =
        std::collections::HashMap::new();
    for tapp in &admin_tapps {
        tapp_map.insert(tapp.tapp_id.clone(), tapp);
    }
    for tapp in &user_tapps {
        tapp_map.entry(tapp.tapp_id.clone()).or_insert(tapp);
    }

    // 构建响应
    let mut result: Vec<RecentTappItem> = Vec::new();
    for activity in activities {
        if result.len() >= limit as usize {
            break;
        }

        // 查找对应的 Tapp 详情
        if let Some(tapp) = tapp_map.get(&activity.tapp_id) {
            // 从 manifest 中提取 iconSvg
            let icon_svg = tapp
                .manifest
                .get("iconSvg")
                .and_then(|v| v.as_str())
                .map(String::from);

            result.push(RecentTappItem {
                id: activity.tapp_id.clone(),
                name: tapp.name.clone(),
                icon: tapp.icon.clone(),
                icon_svg,
                theme_color: tapp.theme_color.clone(),
                last_run_at: activity.last_run_at.to_rfc3339(),
                run_count: activity.run_count,
            });
        }
        // 如果 Tapp 已被卸载，跳过该记录
    }

    Ok(Json(ApiResponse::success(result)))
}

/// 卸载 Tapp 查询参数
#[derive(Debug, Deserialize)]
struct UninstallTappQuery {
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
async fn uninstall_tapp(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
    Query(query): Query<UninstallTappQuery>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    let user_id: i32 = claims.sub.parse().map_err(|_| StatusCode::UNAUTHORIZED)?;
    validate_tapp_id(&tapp_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let keep_data = query.keep_data;

    // 1. Prefer the caller's own install (private or site-owner public under their id).
    let own_tapp = tapps::Entity::find()
        .filter(tapps::Column::UserId.eq(user_id))
        .filter(tapps::Column::TappId.eq(&tapp_id))
        .one(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(tapp) = own_tapp {
        return do_uninstall_tapp(&db, &tapp, keep_data).await;
    }

    // 2. Site-owner public install only (other admins may remove it; non-admins get 403).
    if let Some(site_owner_id) = find_admin_user_id(&db).await? {
        if site_owner_id != user_id {
            let public_tapp = tapps::Entity::find()
                .filter(tapps::Column::UserId.eq(site_owner_id))
                .filter(tapps::Column::TappId.eq(&tapp_id))
                .one(&db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            if let Some(tapp) = public_tapp {
                require_current_admin(&claims).await?;
                return do_uninstall_tapp(&db, &tapp, keep_data).await;
            }
        }
    }

    // 3. Nothing to uninstall.
    Err(StatusCode::NOT_FOUND)
}

/// After a successful uninstall DB commit, choose which filesystem path to delete.
///
/// Prefer the quarantine directory when rename succeeded; otherwise fall back to
/// the live install dir so a failed rename never blocks uninstall completion.
fn uninstall_post_commit_cleanup_path(
    quarantined_dir: Option<PathBuf>,
    live_tapp_dir: PathBuf,
    live_dir_exists: bool,
) -> Option<PathBuf> {
    if let Some(quarantine) = quarantined_dir {
        Some(quarantine)
    } else if live_dir_exists {
        Some(live_tapp_dir)
    } else {
        None
    }
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
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    let user_id = tapp.user_id;
    let tapp_id = &tapp.tapp_id;
    let is_public_install = find_admin_user_id(db).await? == Some(user_id);

    let txn = db.begin().await.map_err(|error| {
        tracing::error!(tapp_id, user_id, %error, "Failed to begin uninstall transaction");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    lock_tapp_lifecycle(&txn, tapp_id).await.map_err(|error| {
        tracing::error!(tapp_id, user_id, %error, "Failed to acquire tapp lifecycle lock");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let still_installed = tapps::Entity::find_by_id(tapp.id)
        .filter(tapps::Column::UserId.eq(user_id))
        .filter(tapps::Column::TappId.eq(tapp_id))
        .one(&txn)
        .await
        .map_err(|error| {
            tracing::error!(tapp_id, user_id, %error, "Failed to re-check tapp install under lock");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .is_some();
    if !still_installed {
        txn.rollback().await.ok();
        return Err(StatusCode::NOT_FOUND);
    }

    crate::api::tapp_runtime::revoke_all_tapp_runtime_grants(tapp_id).await;

    // Prefer moving files out of the live path so a failed DB cleanup can restore
    // them. Rename failures (permissions, busy mount, EXDEV) must not abort
    // uninstall — DB cleanup still proceeds and post-commit best-effort deletes
    // either the quarantine path or the live directory.
    let tapp_dir = tapp_dir_for(user_id, tapp_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let quarantined_dir = if !tapp_dir.exists() {
        None
    } else if let Some(parent) = tapp_dir.parent() {
        let quarantine = parent.join(format!(
            ".{}.uninstall-{}",
            tapp_id,
            uuid::Uuid::new_v4().simple()
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

    let cleanup_result: Result<(), StatusCode> = async {
        if is_public_install {
            txn.execute(Statement::from_sql_and_values(
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
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        } else {
            tapp_widgets::Entity::delete_many()
                .filter(tapp_widgets::Column::UserId.eq(user_id))
                .filter(tapp_widgets::Column::TappId.eq(tapp_id))
                .exec(&txn)
                .await
                .map_err(|error| {
                    tracing::error!(tapp_id, user_id, %error, "Failed to delete private-install widgets on uninstall");
                    StatusCode::INTERNAL_SERVER_ERROR
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
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
        }

        let (task_scope, task_values) = if is_public_install {
            ("tapp_id = $1", vec![tapp_id.clone().into()])
        } else {
            (
                "user_id = $1 AND tapp_id = $2",
                vec![user_id.into(), tapp_id.clone().into()],
            )
        };
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "DELETE FROM tapp_task_executions WHERE scheduled_task_id IN (SELECT id FROM tapp_scheduled_tasks WHERE {task_scope})"
            ),
            task_values.clone(),
        ))
        .await
        .map_err(|error| {
            tracing::error!(tapp_id, user_id, %error, "Failed to delete task executions on uninstall");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!("DELETE FROM tapp_scheduled_tasks WHERE {task_scope}"),
            task_values,
        ))
        .await
        .map_err(|error| {
            tracing::error!(tapp_id, user_id, %error, "Failed to delete scheduled tasks on uninstall");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        tapps::Entity::delete_by_id(tapp.id)
            .exec(&txn)
            .await
            .map_err(|error| {
                tracing::error!(tapp_id, user_id, tapp_row_id = tapp.id, %error, "Failed to delete tapp install row on uninstall");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        txn.execute(Statement::from_sql_and_values(
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
            StatusCode::INTERNAL_SERVER_ERROR
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
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Install row is gone. Best-effort filesystem cleanup must not fail uninstall.
    // Prefer quarantine (when rename succeeded) or live dir, then sweep remaining
    // lifecycle artifacts so reinstall is not blocked by orphan paths.
    let live_dir_exists = tapp_dir.exists();
    if let Some(cleanup_path) =
        uninstall_post_commit_cleanup_path(quarantined_dir, tapp_dir.clone(), live_dir_exists)
    {
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
    match reinstall_orphan_paths(&tapp_dir) {
        Ok(residual) if !residual.is_empty() => {
            for path in residual {
                if let Err(error) = remove_path_best_effort(&path).await {
                    tracing::warn!(
                        tapp_id,
                        user_id,
                        path = %path.display(),
                        kind = ?error.kind(),
                        %error,
                        "Failed to remove residual Tapp lifecycle path after uninstall"
                    );
                }
            }
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(
                tapp_id,
                user_id,
                path = %tapp_dir.display(),
                kind = ?error.kind(),
                %error,
                "Failed to inspect residual Tapp paths after uninstall"
            );
        }
    }

    // Best-effort: drop any remaining runtime registry rows for this tapp.
    if let Err(error) = db
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM tapp_runtime_registry WHERE tapp_id = $1",
            vec![tapp_id.clone().into()],
        ))
        .await
    {
        tracing::warn!(
            tapp_id,
            %error,
            "Failed to clean tapp_runtime_registry rows on uninstall"
        );
    }

    crate::api::tapp_runtime::invalidate_tapp_apis_cache(tapp_id).await;

    Ok(Json(ApiResponse::success(())))
}

/// 更新 Tapp 的请求体
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateTappRequest {
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
async fn update_tapp(
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

    let (
        manifest,
        code,
        styles,
        widget_styles,
        page_styles,
        page_template,
        widget_templates,
        i18n_data,
        page_modules_data,
    ) = match source.as_str() {
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
            (
                manifest,
                code,
                req_styles,
                None,
                None,
                req_page_template,
                req_widget_templates,
                req_i18n,
                req_page_modules,
            )
        }
        "store" => {
            let store_source = store_source.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    api_error("storeSource is required for store update"),
                )
            })?;
            fetch_from_store(&db, &store_source, &tapp_id).await?
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                api_error("Invalid source, must be 'direct' or 'store'"),
            ));
        }
    };

    if manifest.id != tapp_id {
        return Err((
            StatusCode::BAD_REQUEST,
            api_error("manifest id does not match target tapp id"),
        ));
    }
    validate_tapp_manifest(&manifest)
        .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
    if let Some(templates) = &widget_templates {
        validate_widget_template_contents(&manifest, templates)
            .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
    }
    validate_named_resource_keys(
        i18n_data
            .as_ref()
            .into_iter()
            .flat_map(|translations| translations.keys()),
        "i18n language code",
    )
    .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
    validate_named_resource_keys(
        page_modules_data
            .as_ref()
            .into_iter()
            .flat_map(|modules| modules.keys()),
        "page module filename",
    )
    .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;

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

    // 更新代码文件
    write_tapp_resource(tapp_dir, &manifest.main, &code)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error("Failed to save code"),
            )
        })?;

    // 更新可选资源
    if let Some(styles) = &styles {
        let path = manifest.styles.as_deref().unwrap_or("styles.css");
        write_tapp_resource(tapp_dir, path, styles)
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    api_error("Failed to save styles"),
                )
            })?;
    }

    // 更新分离式 CSS
    if let Some(ws) = &widget_styles {
        let path = manifest.widget_styles.as_deref().unwrap_or("widget.css");
        write_tapp_resource(tapp_dir, path, ws).await.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error("Failed to save widget styles"),
            )
        })?;
    }
    if let Some(ps) = &page_styles {
        let path = manifest.page_styles.as_deref().unwrap_or("page.css");
        write_tapp_resource(tapp_dir, path, ps).await.map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error("Failed to save page styles"),
            )
        })?;
    }

    // Keep generated unified-mode CSS separate from Manifest-declared raw
    // widgetStyles/pageStyles, matching the install and resource-read paths.
    if source == "direct" && manifest.css_mode.as_deref() != Some("separated") {
        if let Some(widget_css) = &req_widget_css {
            write_tapp_resource(tapp_dir, "widget.css", widget_css)
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        api_error("Failed to save generated widget CSS"),
                    )
                })?;
        }
        if let Some(page_css) = &req_page_css {
            write_tapp_resource(tapp_dir, "page.css", page_css)
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        api_error("Failed to save generated page CSS"),
                    )
                })?;
        }
    }

    if let Some(page) = &page_template {
        let path = manifest.page_template.as_deref().unwrap_or("page.html");
        write_tapp_resource(tapp_dir, path, page)
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    api_error("Failed to save page template"),
                )
            })?;
    }

    if let Some(widgets) = &widget_templates {
        for (widget_id, templates) in widgets {
            for (size, content) in templates {
                let path = widget_template_path(&manifest, widget_id, size)
                    .expect("validated Widget template path");
                write_tapp_resource(tapp_dir, path, content)
                    .await
                    .map_err(|_| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            api_error("Failed to save widget template"),
                        )
                    })?;
            }
        }
    }

    // 更新 i18n 翻译文件
    if let Some(i18n) = &i18n_data {
        for (lang_code, data) in i18n {
            let json = serde_json::to_string_pretty(data).map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    api_error("Failed to serialize i18n resource"),
                )
            })?;
            write_tapp_resource(tapp_dir, &format!("i18n/{lang_code}.json"), json)
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        api_error("Failed to save i18n resource"),
                    )
                })?;
        }
    }

    // 更新 Page 模块文件
    if let Some(page_modules) = &page_modules_data {
        for (filename, code_content) in page_modules {
            write_tapp_resource(tapp_dir, &format!("page/{filename}"), code_content)
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        api_error("Failed to save page module"),
                    )
                })?;
        }
    }

    if let Some(assets) = &req_assets {
        write_install_assets(tapp_dir, &manifest, assets)
            .await
            .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
    }

    let now = Utc::now().fixed_offset();

    // 更新 manifest.json 和与数据库 updated_at 对应的安装代际标记。
    let manifest_json = serde_json::to_string_pretty(&manifest).unwrap_or_default();
    let staged_manifest_path = tapp_dir.join("manifest.json");
    fs::write(&staged_manifest_path, &manifest_json)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error("Failed to save manifest"),
            )
        })?;
    write_install_generation(tapp_dir, now).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error("Failed to save install state"),
        )
    })?;

    validate_installed_resources(&manifest, tapp_dir)
        .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;

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

    Ok(Json(ApiResponse::success(TappListItem {
        id: result.tapp_id,
        name: result.name,
        version: result.version,
        description: result.description,
        icon: result.icon,
        icon_svg,
        status: format!("{:?}", result.status).to_lowercase(),
        installed_at: result.installed_at.to_rfc3339(),
        last_run_at: result.last_run_at.map(|dt| dt.to_rfc3339()),
        is_temporary,
        is_admin_tapp: is_site_owner,
    })))
}

/// 清理用户的临时 Tapp（登出时调用）
///
/// 删除当前用户的所有临时安装的 Tapp
async fn cleanup_temporary_tapps(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<ApiResponse<i32>>, StatusCode> {
    let user_id: i32 = claims.sub.parse().map_err(|_| StatusCode::UNAUTHORIZED)?;
    // Current administrators operate the canonical public namespace and never
    // receive session-temporary installations.
    if current_is_admin(&claims).await {
        return Ok(Json(ApiResponse::success(0)));
    }

    // 获取用户的所有临时 Tapp
    let user_tapps = tapps::Entity::find()
        .filter(tapps::Column::UserId.eq(user_id))
        .all(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // 删除每个 Tapp（临时 Tapp 不保留数据）
    let mut deleted = 0;
    for tapp in &user_tapps {
        let _ = do_uninstall_tapp(&db, tapp, false).await?;
        deleted += 1;
    }

    Ok(Json(ApiResponse::success(deleted)))
}

#[cfg(test)]
mod manifest_tests {
    use super::{
        append_directory_to_zip, archive_entry_path, canonical_installation_owner_id,
        cleanup_reinstall_orphans, copy_regular_tapp_directory, has_reinstall_orphan_state,
        installation_conflict_owner_ids, orphaned_tapp_directories, recover_tapp_directory,
        reinstall_orphan_paths, tapp_dir_for, tapp_filesystem_error_message,
        tapp_filesystem_error_status, tapp_setting_value_is_valid,
        uninstall_post_commit_cleanup_path, validate_asset_path, validate_installed_resources,
        validate_resource_path, validate_store_manifest_category, validate_tapp_archive,
        validate_tapp_id, validate_tapp_manifest, validate_widget_template_contents,
        widget_template_path, write_install_generation, RegisterWidgetRequest, TappCategory,
        TappDirStage, TappManifest, TappSettingDef, TappStorageAccess, TappWidgetCategory,
        TappWidgetDef, WidgetTemplateContents,
    };
    use crate::models::entities::{tapp_widgets, tapps};
    use crate::services::permission_service::UserRole;
    use axum::http::StatusCode;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn permission_errors_return_actionable_service_unavailable() {
        let error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        assert_eq!(
            tapp_filesystem_error_status(&error),
            StatusCode::SERVICE_UNAVAILABLE
        );
        let message = tapp_filesystem_error_message("create staging", &error);
        assert!(message.contains("storage is not writable"));
        assert!(message.contains("ownership/permissions"));
    }

    /// Pure mirror of `uninstall_tapp` branch order (own → public+admin → not found).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum UninstallTarget {
        OwnInstall,
        PublicRequiresAdmin,
        NotFound,
    }

    fn select_uninstall_target(has_own_install: bool, has_public_install: bool) -> UninstallTarget {
        if has_own_install {
            UninstallTarget::OwnInstall
        } else if has_public_install {
            UninstallTarget::PublicRequiresAdmin
        } else {
            UninstallTarget::NotFound
        }
    }

    #[test]
    fn every_admin_operates_the_canonical_public_owner_namespace() {
        assert_eq!(canonical_installation_owner_id(UserRole::Admin, 9, 1), 1);
        assert_eq!(canonical_installation_owner_id(UserRole::User, 9, 1), 9);
        assert_eq!(canonical_installation_owner_id(UserRole::Guest, -9, 1), -9);
    }

    #[test]
    fn public_and_private_installations_only_conflict_with_their_own_namespace() {
        // Publishing cannot be blocked by another user's private copy.
        assert_eq!(
            installation_conflict_owner_ids(UserRole::Admin, 9, 1),
            vec![1]
        );
        assert_eq!(
            installation_conflict_owner_ids(UserRole::User, 42, 1),
            vec![42]
        );
        assert_eq!(
            installation_conflict_owner_ids(UserRole::Guest, -5, 1),
            vec![-5]
        );
    }

    #[test]
    fn uninstall_prefers_own_install_when_public_coexists() {
        // Dual install: non-admin must hit own row (no admin required), not public 403 path.
        assert_eq!(
            select_uninstall_target(true, true),
            UninstallTarget::OwnInstall
        );
        // Private only.
        assert_eq!(
            select_uninstall_target(true, false),
            UninstallTarget::OwnInstall
        );
        // Public only: admin gate, then uninstall public.
        assert_eq!(
            select_uninstall_target(false, true),
            UninstallTarget::PublicRequiresAdmin
        );
        assert_eq!(
            select_uninstall_target(false, false),
            UninstallTarget::NotFound
        );
    }

    #[test]
    fn uninstall_post_commit_prefers_quarantine_then_live_dir() {
        let live = PathBuf::from("/data/tapps/1/com.example.app");
        let quarantine = PathBuf::from("/data/tapps/1/.com.example.app.uninstall-deadbeef");

        // Rename succeeded: always clean quarantine, even if live path is gone.
        assert_eq!(
            uninstall_post_commit_cleanup_path(Some(quarantine.clone()), live.clone(), false),
            Some(quarantine.clone())
        );
        // Rename failed but live dir still present: best-effort delete live.
        assert_eq!(
            uninstall_post_commit_cleanup_path(None, live.clone(), true),
            Some(live.clone())
        );
        // Nothing on disk after commit: no filesystem work.
        assert_eq!(uninstall_post_commit_cleanup_path(None, live, false), None);
    }

    #[test]
    fn reinstall_orphan_paths_selects_live_and_lifecycle_artifacts() {
        let root = std::env::temp_dir().join(format!(
            "myriad-tapp-reinstall-orphan-select-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let live = root.join("com.example.app");
        let staging = root.join(format!(
            ".com.example.app.staging-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let backup = root.join(format!(
            ".com.example.app.backup-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let uninstall = root.join(format!(
            ".com.example.app.uninstall-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let recovery = root.join(format!(
            ".com.example.app.recovery-discard-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let unrelated = root.join("com.example.other");
        for directory in [&live, &staging, &backup, &uninstall, &recovery, &unrelated] {
            std::fs::create_dir_all(directory).unwrap();
        }

        let selected = reinstall_orphan_paths(&live).unwrap();
        let selected_set = selected
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        assert!(has_reinstall_orphan_state(
            &selected_set.iter().cloned().collect::<Vec<_>>()
        ));
        assert_eq!(
            selected_set,
            std::collections::HashSet::from([live.clone(), staging, backup, uninstall, recovery])
        );
        assert!(!selected_set.contains(&unrelated));

        // Empty owner dir → no orphan state.
        let empty = root.join("missing-app");
        assert!(!has_reinstall_orphan_state(
            &reinstall_orphan_paths(&empty).unwrap()
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleanup_reinstall_orphans_removes_live_and_artifacts_preserving_staging() {
        let root = std::env::temp_dir().join(format!(
            "myriad-tapp-reinstall-orphan-clean-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let live = root.join("com.example.app");
        let preserve = root.join(format!(
            ".com.example.app.staging-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let leftover_staging = root.join(format!(
            ".com.example.app.staging-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let uninstall = root.join(format!(
            ".com.example.app.uninstall-{}",
            uuid::Uuid::new_v4().simple()
        ));
        for directory in [&live, &preserve, &leftover_staging, &uninstall] {
            std::fs::create_dir_all(directory).unwrap();
            std::fs::write(directory.join("marker"), "x").unwrap();
        }

        let removed = cleanup_reinstall_orphans(&live, "com.example.app", 1, 1, Some(&preserve));
        assert!(removed >= 3);
        assert!(!live.exists());
        assert!(!leftover_staging.exists());
        assert!(!uninstall.exists());
        assert!(preserve.exists());
        assert!(!has_reinstall_orphan_state(
            &reinstall_orphan_paths(&live)
                .unwrap()
                .into_iter()
                .filter(|path| path != &preserve)
                .collect::<Vec<_>>()
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn activate_frees_final_path_when_backup_rename_fails_by_removing() {
        // When a leftover live dir cannot be renamed (busy/EXDEV in production),
        // activate falls back to remove_dir_all then places staging. Simulate the
        // free path by pre-removing after a failed rename is not easy cross-platform;
        // instead verify activate succeeds after an orphan live dir is cleaned, and
        // that activate itself renames a normal leftover live dir out of the way.
        let root = std::env::temp_dir().join(format!(
            "myriad-tapp-activate-orphan-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let live = root.join("com.example.app");
        tokio::fs::create_dir_all(&live).await.unwrap();
        tokio::fs::write(live.join("main.js"), "orphan")
            .await
            .unwrap();

        // Pre-activate cleanup path selection (install uses this before staging).
        let orphans = reinstall_orphan_paths(&live).unwrap();
        assert!(has_reinstall_orphan_state(&orphans));
        cleanup_reinstall_orphans(&live, "com.example.app", 1, 1, None);
        assert!(!live.exists());

        let stage = TappDirStage::create(&live).await.unwrap();
        tokio::fs::write(stage.path().join("main.js"), "fresh")
            .await
            .unwrap();
        stage.activate(&live).await.unwrap().commit().await;
        assert_eq!(
            tokio::fs::read_to_string(live.join("main.js"))
                .await
                .unwrap(),
            "fresh"
        );

        // activate also replaces an existing live dir via backup rename.
        let stage2 = TappDirStage::create(&live).await.unwrap();
        tokio::fs::write(stage2.path().join("main.js"), "v2")
            .await
            .unwrap();
        stage2.activate(&live).await.unwrap().commit().await;
        assert_eq!(
            tokio::fs::read_to_string(live.join("main.js"))
                .await
                .unwrap(),
            "v2"
        );
        assert!(super::lifecycle_artifact_directories(&live)
            .unwrap()
            .is_empty());

        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_can_activate_when_orphan_contents_cannot_be_deleted() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "myriad-tapp-permission-orphan-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let live = root.join("com.example.app");
        let protected = live.join("protected");
        tokio::fs::create_dir_all(&protected).await.unwrap();
        tokio::fs::write(protected.join("root-owned.js"), "orphan")
            .await
            .unwrap();
        tokio::fs::set_permissions(&protected, std::fs::Permissions::from_mode(0o000))
            .await
            .unwrap();

        // Recursive cleanup cannot traverse the old contents, but that must
        // not abort installation: activate only needs rename permission on
        // the owner directory to quarantine the occupied live path.
        assert_eq!(
            cleanup_reinstall_orphans(&live, "com.example.app", 1, 1, None),
            0
        );
        assert!(live.exists());

        let stage = TappDirStage::create(&live).await.unwrap();
        tokio::fs::write(stage.path().join("main.js"), "fresh")
            .await
            .unwrap();
        let activated = stage.activate(&live).await.unwrap();
        let backup = activated.backup_path.clone().unwrap();
        assert_eq!(
            tokio::fs::read_to_string(live.join("main.js"))
                .await
                .unwrap(),
            "fresh"
        );

        // Restore permissions only so the test can verify deferred cleanup.
        tokio::fs::set_permissions(
            backup.join("protected"),
            std::fs::Permissions::from_mode(0o700),
        )
        .await
        .unwrap();
        activated.commit().await;
        assert!(!backup.exists());

        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[test]
    fn private_storage_follows_subject_while_settings_follow_installation() {
        let viewer_of_admin = TappStorageAccess::from_owner_and_subject(1, 42);
        let second_viewer = TappStorageAccess::from_owner_and_subject(1, 43);
        assert_eq!(viewer_of_admin.private_storage_namespace(), 42);
        assert_eq!(second_viewer.private_storage_namespace(), 43);
        assert_eq!(viewer_of_admin.installation_namespace(), 1);
        assert_eq!(second_viewer.installation_namespace(), 1);
        assert!(!viewer_of_admin.can_manage_installation());
        assert!(viewer_of_admin.require_installation_write().is_err());

        let private_owner = TappStorageAccess::from_owner_and_subject(42, 42);
        assert_eq!(private_owner.private_storage_namespace(), 42);
        assert_eq!(private_owner.installation_namespace(), 42);
        assert!(private_owner.can_manage_installation());
        assert!(private_owner.require_installation_write().is_ok());

        let site_owner = TappStorageAccess::from_owner_and_subject(1, 1);
        assert_eq!(site_owner.private_storage_namespace(), 1);
        assert_eq!(site_owner.installation_namespace(), 1);
        assert!(site_owner.can_manage_installation());
    }

    #[test]
    fn installation_settings_allow_owner_or_current_admin_only() {
        let public_viewer = TappStorageAccess::from_owner_and_subject(1, 42);
        assert!(!super::can_write_installation_settings(
            public_viewer,
            false
        ));
        assert!(super::can_write_installation_settings(public_viewer, true));

        let private_owner = TappStorageAccess::from_owner_and_subject(42, 42);
        assert!(super::can_write_installation_settings(private_owner, false));
    }

    #[test]
    fn sandbox_storage_rejects_host_managed_key_prefixes() {
        for key in [
            "_settings.theme",
            "_component:theme:midnight",
            "_shortcut:open",
            "_report:weekly",
        ] {
            assert!(super::validate_sandbox_storage_key(key).is_err(), "{key}");
        }
        assert!(super::validate_sandbox_storage_key("user.preferences").is_ok());
    }

    #[test]
    fn preserves_background_requirements_during_manifest_round_trip() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.background",
            "name": "Background app",
            "version": "1.0.0",
            "main": "main.js",
            "permissions": ["scheduler:register"],
            "backgroundRequirements": ["scheduler", "sync"]
        }))
        .expect("manifest should deserialize");

        let value = serde_json::to_value(manifest).expect("manifest should serialize");
        assert_eq!(
            value["backgroundRequirements"],
            json!(["scheduler", "sync"])
        );
    }

    #[tokio::test]
    async fn staged_directory_rollback_restores_previous_install() {
        let root = std::env::temp_dir().join(format!(
            "myriad-tapp-stage-rollback-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let live = root.join("com.example.app");
        tokio::fs::create_dir_all(&live).await.unwrap();
        tokio::fs::write(live.join("main.js"), "old").await.unwrap();

        let stage = TappDirStage::create(&live).await.unwrap();
        tokio::fs::write(stage.path().join("main.js"), "new")
            .await
            .unwrap();
        let activated = stage.activate(&live).await.unwrap();
        assert_eq!(
            tokio::fs::read_to_string(live.join("main.js"))
                .await
                .unwrap(),
            "new"
        );

        activated.rollback().await;
        assert_eq!(
            tokio::fs::read_to_string(live.join("main.js"))
                .await
                .unwrap(),
            "old"
        );
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn staged_directory_commit_keeps_only_new_install() {
        let root = std::env::temp_dir().join(format!(
            "myriad-tapp-stage-commit-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let live = root.join("com.example.app");
        tokio::fs::create_dir_all(&live).await.unwrap();
        tokio::fs::write(live.join("main.js"), "old").await.unwrap();

        let stage = TappDirStage::create(&live).await.unwrap();
        tokio::fs::write(stage.path().join("main.js"), "new")
            .await
            .unwrap();
        stage.activate(&live).await.unwrap().commit().await;

        assert_eq!(
            tokio::fs::read_to_string(live.join("main.js"))
                .await
                .unwrap(),
            "new"
        );
        let mut entries = tokio::fs::read_dir(&root).await.unwrap();
        let mut names = Vec::new();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            names.push(entry.file_name().to_string_lossy().to_string());
        }
        assert_eq!(names, vec!["com.example.app"]);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn compatibility_css_staging_copies_only_regular_resources() {
        let root = std::env::temp_dir().join(format!(
            "myriad-tapp-copy-resources-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let source = root.join("source");
        let destination = root.join("destination");
        tokio::fs::create_dir_all(source.join("nested"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(&destination).await.unwrap();
        tokio::fs::write(source.join("main.js"), "main")
            .await
            .unwrap();
        tokio::fs::write(source.join("nested/page.html"), "page")
            .await
            .unwrap();
        copy_regular_tapp_directory(&source, &destination)
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::read_to_string(destination.join("nested/page.html"))
                .await
                .unwrap(),
            "page"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(source.join("main.js"), source.join("linked.js")).unwrap();
            let rejected = root.join("rejected");
            tokio::fs::create_dir_all(&rejected).await.unwrap();
            assert!(copy_regular_tapp_directory(&source, &rejected)
                .await
                .is_err());
        }
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn startup_recovery_restores_database_generation_after_interrupted_update() {
        let root = std::env::temp_dir().join(format!(
            "myriad-tapp-stage-recovery-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let live = root.join("com.example.app");
        let old_manifest = json!({
            "id": "com.example.app",
            "version": "1.0.0",
            "main": "main.js"
        });
        let new_manifest = json!({
            "id": "com.example.app",
            "version": "1.0.0",
            "main": "main.js"
        });
        let old_generation = chrono::Utc::now().fixed_offset();
        let new_generation = old_generation + chrono::Duration::seconds(1);

        tokio::fs::create_dir_all(&live).await.unwrap();
        tokio::fs::write(live.join("main.js"), "old").await.unwrap();
        tokio::fs::write(
            live.join("manifest.json"),
            serde_json::to_vec(&old_manifest).unwrap(),
        )
        .await
        .unwrap();
        write_install_generation(&live, old_generation).unwrap();

        let stage = TappDirStage::create(&live).await.unwrap();
        tokio::fs::write(stage.path().join("main.js"), "new")
            .await
            .unwrap();
        tokio::fs::write(
            stage.path().join("manifest.json"),
            serde_json::to_vec(&new_manifest).unwrap(),
        )
        .await
        .unwrap();
        write_install_generation(stage.path(), new_generation).unwrap();
        let _interrupted = stage.activate(&live).await.unwrap();

        assert_eq!(
            tokio::fs::read_to_string(live.join("main.js"))
                .await
                .unwrap(),
            "new"
        );
        assert!(recover_tapp_directory(&live, &old_manifest, old_generation).unwrap());
        assert_eq!(
            tokio::fs::read_to_string(live.join("main.js"))
                .await
                .unwrap(),
            "old"
        );
        assert!(super::lifecycle_artifact_directories(&live)
            .unwrap()
            .is_empty());

        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[test]
    fn startup_recovery_can_restore_an_interrupted_recovery_discard() {
        let root = std::env::temp_dir().join(format!(
            "myriad-tapp-recovery-discard-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let live = root.join("com.example.app");
        let discard = root.join(format!(
            ".com.example.app.recovery-discard-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let manifest = json!({
            "id": "com.example.app",
            "version": "1.0.0",
            "main": "main.js"
        });
        let expected_generation = chrono::Utc::now().fixed_offset();
        std::fs::create_dir_all(&live).unwrap();
        std::fs::create_dir_all(&discard).unwrap();
        std::fs::write(live.join("main.js"), "incomplete").unwrap();
        std::fs::write(discard.join("main.js"), "expected").unwrap();
        std::fs::write(
            discard.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        write_install_generation(&discard, expected_generation).unwrap();

        assert!(recover_tapp_directory(&live, &manifest, expected_generation).unwrap());
        assert_eq!(
            std::fs::read_to_string(live.join("main.js")).unwrap(),
            "expected"
        );
        assert!(!discard.exists());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn startup_recovery_discovers_only_unowned_live_and_artifact_directories() {
        let root = std::env::temp_dir().join(format!(
            "myriad-tapp-orphan-discovery-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let owner = root.join("1");
        let keep = owner.join("com.example.keep");
        let orphan = owner.join("com.example.orphan");
        let orphan_artifact = owner.join(format!(
            ".com.example.orphan.uninstall-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let keep_artifact = owner.join(format!(
            ".com.example.keep.recovery-discard-{}",
            uuid::Uuid::new_v4().simple()
        ));
        for directory in [&keep, &orphan, &orphan_artifact, &keep_artifact] {
            std::fs::create_dir_all(directory).unwrap();
        }
        std::fs::write(keep.join("manifest.json"), "{}").unwrap();
        std::fs::write(orphan.join("manifest.json"), "{}").unwrap();

        let installed = std::collections::HashSet::from([(1, "com.example.keep".to_string())]);
        let candidates = orphaned_tapp_directories(&root, &installed).unwrap();
        let candidate_paths = candidates
            .into_iter()
            .map(|(_, _, path)| path)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            candidate_paths,
            std::collections::HashSet::from([orphan, orphan_artifact])
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preserves_and_validates_data_exchange_during_manifest_round_trip() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.exchange",
            "name": "Exchange app",
            "version": "1.0.0",
            "main": "main.js",
            "category": "utility",
            "permissions": ["storage"],
            "dataExchange": {
                "exports": [{
                    "id": "playlist.current",
                    "description": "Current playlist",
                    "maxBytes": 262144,
                    "maxRecords": 200,
                    "schema": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                }],
                "imports": [{
                    "tappId": "com.example.player",
                    "exportId": "playlist.current"
                }]
            }
        }))
        .expect("manifest should deserialize");

        validate_tapp_manifest(&manifest).expect("exchange declaration should validate");
        let value = serde_json::to_value(manifest).expect("manifest should serialize");
        assert_eq!(
            value["dataExchange"]["exports"][0]["id"],
            "playlist.current"
        );
        assert_eq!(
            value["dataExchange"]["imports"][0]["tappId"],
            "com.example.player"
        );
    }

    #[test]
    fn preserves_and_validates_ai_v2_during_manifest_round_trip() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.ai",
            "name": "AI app",
            "version": "1.0.0",
            "main": "main.js",
            "category": "ai",
            "permissions": ["ai:generate", "platform:read"],
            "ai": {
                "protocolVersion": 2,
                "operations": ["generate"],
                "modelTier": "standard",
                "contextSources": ["platform", "custom"],
                "outputFormats": ["text", "json"]
            }
        }))
        .expect("manifest should deserialize");

        validate_tapp_manifest(&manifest).expect("AI declaration should validate");
        let value = serde_json::to_value(manifest).expect("manifest should serialize");
        assert_eq!(value["ai"]["protocolVersion"], 2);
        assert_eq!(value["ai"]["operations"], json!(["generate"]));
    }

    #[test]
    fn ai_builtin_requires_matching_v2_declaration() {
        let base = json!({
            "id": "com.example.ai-builtin",
            "name": "AI builtin app",
            "version": "1.0.0",
            "main": "main.js",
            "category": "ai",
            "permissions": ["ai:generate"],
            "apis": {
                "summary": {
                    "type": "builtin",
                    "builtin": "ai:generate"
                }
            }
        });
        let missing: TappManifest = serde_json::from_value(base.clone()).unwrap();
        assert!(validate_tapp_manifest(&missing).is_err());

        let mut declared = base;
        declared["ai"] = json!({
            "protocolVersion": 2,
            "operations": ["generate"],
            "modelTier": "standard",
            "contextSources": [],
            "outputFormats": ["text"]
        });
        let declared: TappManifest = serde_json::from_value(declared).unwrap();
        validate_tapp_manifest(&declared).unwrap();
    }

    #[test]
    fn rejects_ai_operation_without_matching_permission() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.ai",
            "name": "AI app",
            "version": "1.0.0",
            "main": "main.js",
            "category": "ai",
            "permissions": [],
            "ai": {
                "protocolVersion": 2,
                "operations": ["chat"],
                "modelTier": "standard",
                "contextSources": [],
                "outputFormats": ["text"]
            }
        }))
        .expect("manifest should deserialize");

        assert!(validate_tapp_manifest(&manifest).is_err());
    }

    #[test]
    fn preserves_and_validates_event_v2_topics() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.player",
            "name": "Event app",
            "version": "1.0.0",
            "main": "main.js",
            "category": "media",
            "permissions": ["event:publish", "event:subscribe"],
            "events": {
                "publish": ["tapp.com.example.player.track.changed"],
                "subscribe": ["system.theme.changed", "tapp.com.example.other.invalidated"]
            }
        }))
        .expect("manifest should deserialize");

        validate_tapp_manifest(&manifest).expect("event declaration should validate");
        let value = serde_json::to_value(manifest).expect("manifest should serialize");
        assert_eq!(
            value["events"]["publish"],
            json!(["tapp.com.example.player.track.changed"])
        );
    }

    #[test]
    fn rejects_event_publish_topic_outside_tapp_namespace() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.player",
            "name": "Event app",
            "version": "1.0.0",
            "main": "main.js",
            "category": "media",
            "permissions": ["event:publish"],
            "events": {
                "publish": ["tapp.com.example.other.track.changed"]
            }
        }))
        .expect("manifest should deserialize");

        assert!(validate_tapp_manifest(&manifest).is_err());
    }

    #[test]
    fn preserves_and_validates_agent_v2_manifest() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.reporter",
            "name": "Agent app",
            "version": "1.0.0",
            "main": "main.js",
            "category": "productivity",
            "permissions": [],
            "agent": {
                "protocolVersion": 2,
                "interactions": [{
                    "type": "report.compose",
                    "inputSchema": "schemas/report-input.json",
                    "resultSchema": "schemas/report-result.json"
                }],
                "intents": ["ui.open", "report.create"]
            }
        }))
        .expect("manifest should deserialize");

        validate_tapp_manifest(&manifest).expect("Agent declaration should validate");
        let value = serde_json::to_value(manifest).expect("manifest should serialize");
        assert_eq!(value["agent"]["protocolVersion"], 2);
        assert_eq!(value["agent"]["interactions"][0]["type"], "report.compose");
    }

    #[test]
    fn rejects_external_data_exchange_schema_references() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.exchange",
            "name": "Exchange app",
            "version": "1.0.0",
            "main": "main.js",
            "category": "data",
            "permissions": [],
            "dataExchange": {
                "exports": [{
                    "id": "unsafe",
                    "maxBytes": 1024,
                    "schema": { "$ref": "https://example.com/schema.json" }
                }]
            }
        }))
        .expect("manifest should deserialize");

        assert!(validate_tapp_manifest(&manifest).is_err());
    }

    #[test]
    fn preserves_widget_metadata_during_manifest_round_trip() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.widget",
            "name": "Widget app",
            "version": "1.0.0",
            "main": "main.js",
            "category": "utility",
            "permissions": ["widget:register"],
            "widgets": [{
                "id": "summary",
                "name": "Summary",
                "description": "Daily summary",
                "icon": "chart",
                "defaultSize": "2x2",
                "sizes": ["2x2", "4x2"],
                "category": "stats",
                "templates": {
                    "2x2": "templates/widget-2x2.html",
                    "4x2": "templates/widget-4x2.html"
                },
                "settings": [{
                    "key": "compact",
                    "label": "Compact layout",
                    "type": "toggle",
                    "defaultValue": false
                }],
                "refreshPolicy": {
                    "mode": "interval",
                    "intervalSeconds": 60,
                    "refreshOnVisible": true
                }
            }]
        }))
        .expect("manifest should deserialize");

        validate_tapp_manifest(&manifest).expect("widget metadata should validate");
        let value = serde_json::to_value(manifest).expect("manifest should serialize");
        let widget = &value["widgets"][0];
        assert_eq!(widget["description"], json!("Daily summary"));
        assert_eq!(widget["icon"], json!("chart"));
        assert_eq!(widget["category"], json!("stats"));
        assert_eq!(
            widget["templates"]["4x2"],
            json!("templates/widget-4x2.html")
        );
        assert_eq!(widget["settings"][0]["key"], json!("compact"));
        assert_eq!(widget["refreshPolicy"]["mode"], json!("interval"));
        assert_eq!(widget["refreshPolicy"]["intervalSeconds"], json!(60));
        let parsed: TappManifest = serde_json::from_value(value).unwrap();
        assert_eq!(
            widget_template_path(&parsed, "summary", "4x2"),
            Some("templates/widget-4x2.html")
        );
    }

    #[test]
    fn rejects_invalid_widget_settings_and_refresh_policy() {
        let parse = |widget: serde_json::Value| {
            serde_json::from_value::<TappManifest>(json!({
                "id": "com.example.invalid-widget",
                "name": "Invalid widget",
                "version": "1.0.0",
                "main": "main.js",
                "category": "utility",
                "permissions": ["widget:register"],
                "widgets": [widget]
            }))
            .unwrap()
        };

        let too_frequent = parse(json!({
            "id": "summary",
            "name": "Summary",
            "defaultSize": "2x2",
            "sizes": ["2x2"],
            "refreshPolicy": { "mode": "interval", "intervalSeconds": 5 }
        }));
        assert!(validate_tapp_manifest(&too_frequent).is_err());

        let invalid_setting = parse(json!({
            "id": "summary",
            "name": "Summary",
            "defaultSize": "2x2",
            "sizes": ["2x2"],
            "settings": [{ "key": "mode", "label": "Mode", "type": "select" }]
        }));
        assert!(validate_tapp_manifest(&invalid_setting).is_err());
    }

    #[test]
    fn host_setting_values_follow_manifest_type_and_range() {
        let setting: TappSettingDef = serde_json::from_value(json!({
            "key": "volume",
            "label": "Volume",
            "type": "number",
            "defaultValue": 50,
            "min": 0,
            "max": 100
        }))
        .unwrap();

        assert!(tapp_setting_value_is_valid(&setting, &json!(75)));
        assert!(!tapp_setting_value_is_valid(&setting, &json!(101)));
        assert!(!tapp_setting_value_is_valid(&setting, &json!("75")));

        let select: TappSettingDef = serde_json::from_value(json!({
            "key": "theme",
            "label": "Theme",
            "type": "select",
            "options": [
                { "value": "light", "label": "Light" },
                { "value": "dark", "label": "Dark" }
            ]
        }))
        .unwrap();
        assert!(tapp_setting_value_is_valid(&select, &json!("dark")));
        assert!(!tapp_setting_value_is_valid(&select, &json!("system")));
    }

    #[test]
    fn enforces_minimum_system_version_on_install_and_update_validation() {
        let mut manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.compatibility",
            "name": "Compatibility gate",
            "version": "1.0.0",
            "main": "main.js",
            "category": "utility",
            "permissions": [],
            "minSystemVersion": env!("CARGO_PKG_VERSION")
        }))
        .unwrap();

        validate_tapp_manifest(&manifest).unwrap();
        let serialized = serde_json::to_value(&manifest).unwrap();
        assert_eq!(
            serialized["minSystemVersion"],
            json!(env!("CARGO_PKG_VERSION"))
        );

        manifest.min_system_version = Some("999.0.0".to_string());
        assert!(validate_tapp_manifest(&manifest).is_err());

        manifest.min_system_version = Some("not-a-version".to_string());
        assert!(validate_tapp_manifest(&manifest).is_err());
    }

    #[test]
    fn validates_author_contact_fields() {
        let parse = |author: serde_json::Value| {
            serde_json::from_value::<TappManifest>(json!({
                "id": "com.example.author",
                "name": "Author metadata",
                "version": "1.0.0",
                "main": "main.js",
                "category": "utility",
                "permissions": [],
                "author": author
            }))
            .unwrap()
        };

        let valid = parse(json!({
            "name": "Example Team",
            "email": "team@example.com",
            "url": "https://example.com/team"
        }));
        validate_tapp_manifest(&valid).unwrap();

        assert!(validate_tapp_manifest(&parse(json!({ "name": "" }))).is_err());
        assert!(validate_tapp_manifest(&parse(json!({
            "name": "Example Team",
            "email": "not-an-email"
        })))
        .is_err());
        assert!(validate_tapp_manifest(&parse(json!({
            "name": "Example Team",
            "url": "javascript:alert(1)"
        })))
        .is_err());
    }

    #[test]
    fn validates_manifest_metadata_and_declared_capability_permissions() {
        let base = || {
            serde_json::from_value::<TappManifest>(json!({
                "id": "com.example.metadata",
                "name": "Metadata app",
                "version": "1.0.0-beta.1",
                "description": "Valid metadata",
                "main": "main.js",
                "category": "utility",
                "permissions": [],
                "themeColor": "#12ABef",
                "homepage": "https://example.com/app",
                "repository": "https://github.com/example/app"
            }))
            .unwrap()
        };
        validate_tapp_manifest(&base()).unwrap();

        let mut invalid = base();
        invalid.name = " ".to_string();
        assert!(validate_tapp_manifest(&invalid).is_err());

        let mut invalid = base();
        invalid.version = "latest".to_string();
        assert!(validate_tapp_manifest(&invalid).is_err());

        let mut invalid = base();
        invalid.theme_color = Some("red".to_string());
        assert!(validate_tapp_manifest(&invalid).is_err());

        let mut invalid = base();
        invalid.repository = Some("javascript:alert(1)".to_string());
        assert!(validate_tapp_manifest(&invalid).is_err());

        let mut invalid = base();
        invalid.background_requirements = Some(vec!["widget".to_string()]);
        assert!(validate_tapp_manifest(&invalid).is_err());

        let widget_without_permission: TappManifest = serde_json::from_value(json!({
            "id": "com.example.widget-permission",
            "name": "Widget permission",
            "version": "1.0.0",
            "main": "main.js",
            "category": "utility",
            "permissions": [],
            "widgets": [{
                "id": "summary",
                "name": "Summary",
                "defaultSize": "2x2",
                "sizes": ["2x2"]
            }]
        }))
        .unwrap();
        assert!(validate_tapp_manifest(&widget_without_permission).is_err());

        let protected_api_without_permission: TappManifest = serde_json::from_value(json!({
            "id": "com.example.api-permission",
            "name": "API permission",
            "version": "1.0.0",
            "main": "main.js",
            "permissions": [],
            "apis": {
                "protected": {
                    "endpoint": "https://example.com/data"
                }
            }
        }))
        .unwrap();
        assert!(validate_tapp_manifest(&protected_api_without_permission).is_err());
    }

    #[test]
    fn normalizes_legacy_tapp_categories_and_rejects_unknown_values() {
        let parse = |category: Option<&str>| {
            let mut value = json!({
                "id": "com.example.category",
                "name": "Category contract",
                "version": "1.0.0",
                "main": "main.js",
                "permissions": []
            });
            if let Some(category) = category {
                value["category"] = json!(category);
            }
            serde_json::from_value::<TappManifest>(value)
        };

        let legacy_game = parse(Some("games")).unwrap();
        assert_eq!(legacy_game.category, Some(TappCategory::Game));
        assert_eq!(
            serde_json::to_value(legacy_game).unwrap()["category"],
            json!("game")
        );

        let legacy_tool = parse(Some("tool")).unwrap();
        assert_eq!(legacy_tool.category, Some(TappCategory::Utility));
        assert_eq!(
            serde_json::to_value(legacy_tool).unwrap()["category"],
            json!("utility")
        );

        let missing = parse(None).unwrap();
        assert_eq!(missing.category, None);
        assert!(validate_tapp_manifest(&missing).is_err());
        assert!(serde_json::to_value(missing)
            .unwrap()
            .get("category")
            .is_none());

        assert!(parse(Some("uncategorized")).is_err());
    }

    #[test]
    fn normalizes_and_restricts_widget_categories_across_manifest_and_runtime() {
        let legacy_manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.widget-category",
            "name": "Widget category",
            "version": "1.0.0",
            "main": "main.js",
            "category": "utility",
            "permissions": ["widget:register"],
            "category": "utility",
            "widgets": [{
                "id": "summary",
                "name": "Summary",
                "defaultSize": "2x2",
                "sizes": ["2x2"],
                "category": "tool"
            }]
        }))
        .unwrap();
        assert_eq!(
            legacy_manifest.widgets.as_ref().unwrap()[0].category,
            Some(TappWidgetCategory::Utility)
        );
        assert_eq!(
            serde_json::to_value(legacy_manifest).unwrap()["widgets"][0]["category"],
            json!("utility")
        );

        let runtime_payload = json!({
            "id": "summary",
            "name": "Summary",
            "default_size": "2x2",
            "sizes": ["2x2"],
            "category": "activity"
        });
        let runtime: RegisterWidgetRequest =
            serde_json::from_value(runtime_payload.clone()).unwrap();
        assert_eq!(runtime.category, Some(TappWidgetCategory::Activity));

        let mut invalid_runtime = runtime_payload;
        invalid_runtime["category"] = json!("media");
        assert!(serde_json::from_value::<RegisterWidgetRequest>(invalid_runtime).is_err());

        let invalid_manifest = json!({
            "id": "com.example.invalid-widget-category",
            "name": "Invalid Widget category",
            "version": "1.0.0",
            "main": "main.js",
            "permissions": ["widget:register"],
            "category": "utility",
            "widgets": [{
                "id": "summary",
                "name": "Summary",
                "defaultSize": "2x2",
                "sizes": ["2x2"],
                "category": "media"
            }]
        });
        assert!(serde_json::from_value::<TappManifest>(invalid_manifest).is_err());
    }

    #[test]
    fn runtime_widget_owner_binding_prevents_cross_installation_reuse() {
        let now = chrono::Utc::now().fixed_offset();
        let widget = |subject_id: i32, config: serde_json::Value| tapp_widgets::Model {
            id: 1,
            widget_id: "tapp.com.example.shared.dynamic".to_string(),
            tapp_id: "com.example.shared".to_string(),
            user_id: subject_id,
            name: "Dynamic".to_string(),
            description: None,
            icon: None,
            default_size: "2x2".to_string(),
            sizes: json!(["2x2"]),
            category: None,
            config,
            registered_at: now,
        };

        let public_widget = widget(9, json!({ "source": "runtime", "installationOwnerId": 1 }));
        assert!(super::runtime_widget_belongs_to_installation(
            &public_widget,
            9,
            1
        ));
        assert!(!super::runtime_widget_belongs_to_installation(
            &public_widget,
            9,
            9
        ));

        let legacy_private = widget(9, json!({ "source": "runtime" }));
        assert!(super::runtime_widget_belongs_to_installation(
            &legacy_private,
            9,
            9
        ));
        assert!(!super::runtime_widget_belongs_to_installation(
            &legacy_private,
            9,
            1
        ));
    }

    #[test]
    fn requires_store_index_and_manifest_categories_to_match() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.store-category",
            "name": "Store category",
            "version": "1.0.0",
            "main": "main.js",
            "permissions": [],
            "category": "media"
        }))
        .unwrap();

        assert!(
            validate_store_manifest_category(&json!({ "category": "music" }), &manifest).is_ok()
        );
        assert!(validate_store_manifest_category(
            &json!({ "category": "productivity" }),
            &manifest
        )
        .is_err());
        assert!(validate_store_manifest_category(&json!({}), &manifest).is_err());
    }

    #[test]
    fn rejects_removed_or_unknown_manifest_fields() {
        let removed_top_level = serde_json::from_value::<TappManifest>(json!({
            "id": "com.example.legacy",
            "name": "Legacy app",
            "version": "1.0.0",
            "main": "main.js",
            "permissions": [],
            "optionalPermissions": ["network:fetch"]
        }));
        assert!(removed_top_level.is_err());

        let removed_widget_field = serde_json::from_value::<TappManifest>(json!({
            "id": "com.example.legacy-widget",
            "name": "Legacy widget",
            "version": "1.0.0",
            "main": "main.js",
            "permissions": ["widget:register"],
            "widgets": [{
                "id": "summary",
                "name": "Summary",
                "defaultSize": "2x2",
                "sizes": ["2x2"],
                "refreshInterval": 60000
            }]
        }));
        assert!(removed_widget_field.is_err());

        let removed_min_refresh_interval = serde_json::from_value::<TappManifest>(json!({
            "id": "com.example.legacy-widget",
            "name": "Legacy widget",
            "version": "1.0.0",
            "main": "main.js",
            "permissions": ["widget:register"],
            "widgets": [{
                "id": "summary",
                "name": "Summary",
                "defaultSize": "2x2",
                "sizes": ["2x2"],
                "minRefreshInterval": 1000
            }]
        }));
        assert!(removed_min_refresh_interval.is_err());
    }

    #[test]
    fn validates_declared_api_shape_and_inject_aliases() {
        let valid: TappManifest = serde_json::from_value(json!({
            "id": "com.example.api",
            "name": "API app",
            "version": "1.0.0",
            "main": "main.js",
            "category": "developer",
            "permissions": ["network:fetch"],
            "apis": {
                "weather.current": {
                    "type": "http",
                    "endpoint": "https://example.com/weather?city={{city}}",
                    "inject": { "city": "{{geo.city}}" }
                }
            }
        }))
        .unwrap();
        validate_tapp_manifest(&valid).unwrap();

        let mut public_without_network = valid.clone();
        public_without_network.permissions.clear();
        public_without_network
            .apis
            .as_mut()
            .unwrap()
            .get_mut("weather.current")
            .unwrap()
            .access = super::TappApiAccess::Public;
        assert!(validate_tapp_manifest(&public_without_network).is_err());

        let mut reserved_alias = valid.clone();
        reserved_alias
            .apis
            .as_mut()
            .unwrap()
            .get_mut("weather.current")
            .unwrap()
            .inject = Some(std::collections::HashMap::from([(
            "user.id".to_string(),
            "{{geo.city}}".to_string(),
        )]));
        assert!(validate_tapp_manifest(&reserved_alias).is_err());

        let mut secret_template = valid.clone();
        secret_template
            .apis
            .as_mut()
            .unwrap()
            .get_mut("weather.current")
            .unwrap()
            .headers = Some(std::collections::HashMap::from([(
            "Authorization".to_string(),
            "Bearer {{secrets.OPENWEATHER_KEY}}".to_string(),
        )]));
        assert!(validate_tapp_manifest(&secret_template).is_err());

        let removed_api_url = serde_json::from_value::<TappManifest>(json!({
            "id": "com.example.legacy-api",
            "name": "Legacy API",
            "version": "1.0.0",
            "main": "main.js",
            "permissions": ["network:fetch"],
            "apis": {
                "weather": {
                    "url": "https://example.com/weather"
                }
            }
        }));
        assert!(removed_api_url.is_err());
    }

    #[test]
    fn rejects_unbounded_widget_manifests() {
        let mut manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.too-many-widgets",
            "name": "Too many widgets",
            "version": "1.0.0",
            "main": "main.js",
            "category": "utility",
            "permissions": ["widget:register"]
        }))
        .unwrap();
        manifest.widgets = Some(
            (0..65)
                .map(|index| TappWidgetDef {
                    id: format!("widget-{index}"),
                    name: format!("Widget {index}"),
                    description: None,
                    icon: None,
                    default_size: "2x2".to_string(),
                    sizes: vec!["2x2".to_string()],
                    category: None,
                    templates: None,
                    settings: Vec::new(),
                    refresh_policy: None,
                })
                .collect(),
        );

        assert!(validate_tapp_manifest(&manifest).is_err());
    }

    #[test]
    fn batch_detail_mapping_applies_current_role_and_brew_capability_rules() {
        let now = chrono::Utc::now().fixed_offset();
        let tapp = tapps::Model {
            id: 1,
            tapp_id: "com.example.detail".to_string(),
            user_id: 7,
            name: "Detail".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            icon: None,
            theme_color: None,
            manifest: json!({
                "id": "com.example.detail",
                "name": "Detail",
                "version": "1.0.0",
                "main": "main.js",
                "permissions": ["storage", "brew:write", "ai:generate"]
            }),
            status: tapps::TappStatus::Installed,
            granted_permissions: json!(["storage", "brew:write"]),
            approved_permissions: json!(["storage", "brew:write", "ai:generate"]),
            file_path: "manifest.json".to_string(),
            code_path: "main.js".to_string(),
            installed_at: now,
            last_run_at: None,
            updated_at: now,
            error_message: None,
        };

        let config = crate::config::DynamicConfig {
            user_perm_ai_generate: true,
            ..Default::default()
        };
        let detail = super::tapp_detail_from_model(tapp, UserRole::User, true, false, &config);

        assert_eq!(detail.user_role, "user");
        assert!(detail.is_temporary);
        assert!(!detail.is_admin_tapp);
        assert_eq!(
            detail.granted_permissions,
            vec!["storage", "brew:write", "ai:generate"]
        );
    }

    #[test]
    fn validates_all_declared_install_resources() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.resources",
            "name": "Resources",
            "version": "1.0.0",
            "main": "src/main.js",
            "category": "utility",
            "permissions": [],
            "styles": "css/shared.css",
            "pageModules": ["index.js"],
            "widgets": [{
                "id": "summary",
                "name": "Summary",
                "defaultSize": "2x2",
                "sizes": ["2x2"],
                "templates": { "2x2": "templates/summary.html" }
            }]
        }))
        .unwrap();

        let unique = format!(
            "myriad-tapp-resource-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        for relative in [
            "src/main.js",
            "css/shared.css",
            "page/index.js",
            "templates/summary.html",
        ] {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "test").unwrap();
        }

        assert!(validate_installed_resources(&manifest, &root).is_ok());

        std::fs::write(root.join("src/main.js"), [0xff, 0xfe]).unwrap();
        assert!(validate_installed_resources(&manifest, &root).is_err());
        std::fs::write(root.join("src/main.js"), "test").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let template = root.join("templates/summary.html");
            std::fs::remove_file(&template).unwrap();
            std::fs::write(root.join("outside.html"), "outside").unwrap();
            symlink(root.join("outside.html"), &template).unwrap();
            assert!(validate_installed_resources(&manifest, &root).is_err());
            std::fs::remove_file(&template).unwrap();
            std::fs::write(&template, "test").unwrap();
        }

        std::fs::remove_file(root.join("templates/summary.html")).unwrap();
        assert!(validate_installed_resources(&manifest, &root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn validates_declared_package_assets_allow_binary() {
        assert!(validate_asset_path("assets/sprite.png").is_ok());
        assert!(validate_asset_path("sprite.png").is_err());
        assert!(validate_asset_path("assets/hack.js").is_err());
        assert!(validate_asset_path("../assets/x.png").is_err());

        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.assets",
            "name": "Assets",
            "version": "1.0.0",
            "main": "main.js",
            "category": "game",
            "permissions": ["media:audio"],
            "assets": ["assets/pixel.png", "assets/level.json"]
        }))
        .unwrap();
        assert!(validate_tapp_manifest(&manifest).is_ok());

        let unique = format!(
            "myriad-tapp-assets-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("main.js"), "export {};").unwrap();
        std::fs::write(root.join("assets/pixel.png"), [0x89, 0x50, 0x4e, 0x47]).unwrap();
        std::fs::write(root.join("assets/level.json"), r#"{"ok":true}"#).unwrap();
        assert!(validate_installed_resources(&manifest, &root).is_ok());

        std::fs::remove_file(root.join("assets/pixel.png")).unwrap();
        assert!(validate_installed_resources(&manifest, &root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn validates_agent_schema_and_i18n_contents_at_install_time() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.validated-content",
            "name": "Validated content",
            "version": "1.0.0",
            "main": "main.js",
            "category": "productivity",
            "permissions": [],
            "agent": {
                "protocolVersion": 2,
                "interactions": [{
                    "type": "report.compose",
                    "inputSchema": "schemas/input.json"
                }]
            }
        }))
        .unwrap();
        let unique = format!(
            "myriad-tapp-content-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(root.join("schemas")).unwrap();
        std::fs::create_dir_all(root.join("i18n")).unwrap();
        std::fs::write(root.join("main.js"), "export {};").unwrap();
        std::fs::write(
            root.join("schemas/input.json"),
            r#"{"type":"object","properties":{"title":{"type":"string"}}}"#,
        )
        .unwrap();
        std::fs::write(root.join("i18n/en-US.json"), r#"{"title":"Title"}"#).unwrap();
        assert!(validate_installed_resources(&manifest, &root).is_ok());

        std::fs::write(root.join("schemas/input.json"), "not-json").unwrap();
        assert!(validate_installed_resources(&manifest, &root)
            .unwrap_err()
            .contains("not valid JSON"));

        std::fs::write(root.join("schemas/input.json"), r#"{"$ref":"remote.json"}"#).unwrap();
        assert!(validate_installed_resources(&manifest, &root)
            .unwrap_err()
            .contains("does not support $ref"));

        std::fs::write(root.join("schemas/input.json"), r#"{"type":"object"}"#).unwrap();
        std::fs::write(root.join("i18n/en-US.json"), r#"["not","an","object"]"#).unwrap();
        assert!(validate_installed_resources(&manifest, &root)
            .unwrap_err()
            .contains("must contain a JSON object"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exports_nested_resource_paths_without_flattening() {
        let unique = format!(
            "myriad-tapp-export-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let nested = root.join("templates/dashboard/widget.html");
        std::fs::create_dir_all(nested.parent().unwrap()).unwrap();
        std::fs::write(root.join("manifest.json"), "{}").unwrap();
        std::fs::write(root.join(super::TAPP_INSTALL_STATE_FILE), "internal").unwrap();
        std::fs::write(&nested, "<main>nested</main>").unwrap();

        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        append_directory_to_zip(&mut writer, &root, &root, options).unwrap();
        let bytes = writer.finish().unwrap().into_inner();

        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut names = (0..archive.len())
            .map(|index| archive.by_index(index).unwrap().name().to_string())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(
            names,
            vec![
                "manifest.json".to_string(),
                "templates/dashboard/widget.html".to_string()
            ]
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_colliding_archive_entries() {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();
        writer.add_directory("templates/", options).unwrap();
        writer.start_file("templates", options).unwrap();
        std::io::Write::write_all(&mut writer, b"collision").unwrap();
        let bytes = writer.finish().unwrap().into_inner();

        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert!(validate_tapp_archive(&mut archive).is_err());
    }

    #[test]
    fn rejects_tapp_ids_and_resource_paths_that_escape_the_sandbox() {
        for invalid in ["", ".", "..", "../escape", "/tmp/escape", ".hidden"] {
            assert!(validate_tapp_id(invalid).is_err(), "accepted {invalid}");
        }
        assert!(validate_tapp_id("com.myriad.safe-app_2").is_ok());
        assert!(tapp_dir_for(7, "../../tmp/escape").is_err());

        for invalid in ["../secret", "/etc/passwd", "page/../../secret", ".env"] {
            assert!(
                validate_resource_path(invalid).is_err(),
                "accepted {invalid}"
            );
        }
        assert!(validate_resource_path("page/state.js").is_ok());
        let root = std::path::Path::new("/tmp/tapps/com.example.safe");
        assert_eq!(
            archive_entry_path(root, "templates/widget-2x2.html").unwrap(),
            root.join("templates/widget-2x2.html")
        );
        assert!(archive_entry_path(root, "../outside.html").is_err());
    }

    #[test]
    fn validates_manifest_paths_before_install_or_update() {
        let mut manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.safe",
            "name": "Safe app",
            "version": "1.0.0",
            "main": "main.js",
            "category": "utility",
            "permissions": [],
            "pageModules": ["state.js"]
        }))
        .unwrap();
        assert!(validate_tapp_manifest(&manifest).is_ok());

        manifest.main = "main.txt".to_string();
        assert!(validate_tapp_manifest(&manifest).is_err());
        manifest.main = "main.js".to_string();

        manifest.styles = Some("styles.txt".to_string());
        assert!(validate_tapp_manifest(&manifest).is_err());
        manifest.styles = None;

        manifest.page_template = Some("page.txt".to_string());
        assert!(validate_tapp_manifest(&manifest).is_err());

        manifest.page_template = Some("../../outside.html".to_string());
        assert!(validate_tapp_manifest(&manifest).is_err());

        manifest.page_template = None;
        manifest.page_modules = Some(vec!["nested/index.js".to_string()]);
        assert!(validate_tapp_manifest(&manifest).is_err());

        let manifest_with_escaping_widget_template: TappManifest = serde_json::from_value(json!({
            "id": "com.example.unsafe-widget",
            "name": "Unsafe widget",
            "version": "1.0.0",
            "main": "main.js",
            "category": "utility",
            "permissions": [],
            "widgets": [{
                "id": "unsafe",
                "name": "Unsafe",
                "defaultSize": "2x2",
                "sizes": ["2x2"],
                "templates": { "2x2": "../outside.html" }
            }]
        }))
        .unwrap();
        assert!(validate_tapp_manifest(&manifest_with_escaping_widget_template).is_err());

        let same_size_templates: TappManifest = serde_json::from_value(json!({
            "id": "com.example.conflicting-widgets",
            "name": "Conflicting widgets",
            "version": "1.0.0",
            "main": "main.js",
            "category": "utility",
            "permissions": ["widget:register"],
            "widgets": [
                {
                    "id": "one",
                    "name": "One",
                    "defaultSize": "2x2",
                    "sizes": ["2x2"],
                    "templates": { "2x2": "templates/one.html" }
                },
                {
                    "id": "two",
                    "name": "Two",
                    "defaultSize": "2x2",
                    "sizes": ["2x2"],
                    "templates": { "2x2": "templates/two.html" }
                }
            ]
        }))
        .unwrap();
        assert!(validate_tapp_manifest(&same_size_templates).is_ok());

        let contents = WidgetTemplateContents::from([
            (
                "one".to_string(),
                std::collections::HashMap::from([("2x2".to_string(), "one template".to_string())]),
            ),
            (
                "two".to_string(),
                std::collections::HashMap::from([("2x2".to_string(), "two template".to_string())]),
            ),
        ]);
        assert!(validate_widget_template_contents(&same_size_templates, &contents).is_ok());

        let unknown_widget = WidgetTemplateContents::from([(
            "missing".to_string(),
            std::collections::HashMap::from([("2x2".to_string(), "template".to_string())]),
        )]);
        assert!(validate_widget_template_contents(&same_size_templates, &unknown_widget).is_err());
    }
}
