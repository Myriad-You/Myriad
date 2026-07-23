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

mod access;
mod catalog;
mod installation;
mod lifecycle;
mod package_api;
mod package_files;
mod prepared_package;
mod storage;
mod store_package;
mod store_sources;
mod types;
mod uninstall;
mod validation;
mod widgets;

use access::{
    authorize_runtime_storage, authorize_tapp_permission, can_write_installation_settings,
    canonical_installation_owner_id, current_is_admin, current_user_role,
    filter_install_permissions, find_admin_user_id, find_visible_tapp, get_admin_user_id,
    installation_conflict_owner_ids, lock_tapp_lifecycle, optional_authenticated_user_id,
    require_current_admin,
};
pub(crate) use access::{installation_write_forbidden_error, TappStorageAccess};
#[cfg(test)]
use catalog::tapp_detail_from_model;
use catalog::{get_tapp, list_tapp_details, list_tapps};
use installation::{install_tapp, install_tapp_file, update_tapp};
use lifecycle::{get_recent_tapps, start_tapp, stop_tapp};
pub use myriad_tapp_contract::manifest::*;
use package_api::{export_tapp, get_tapp_asset, get_tapp_code, get_tapp_resources};
pub(crate) use package_files::*;
use storage::{
    clear_storage, delete_storage, get_storage, get_storage_usage, get_tapp_setting,
    get_tapp_settings, list_storage_entries, list_storage_keys, set_storage, set_tapp_setting,
};
pub(crate) use storage::{
    read_storage_value, validate_sandbox_storage_key, validate_storage_key,
    validate_storage_value_size, write_storage_value,
};
#[cfg(test)]
use store_package::validate_store_manifest_category;
pub use store_sources::*;
use types::api_error;
pub use types::{ApiResponse, TappDetail, TappListItem};
#[cfg(test)]
use uninstall::uninstall_post_commit_cleanup_path;
use uninstall::{cleanup_temporary_tapps, uninstall_tapp};
pub(crate) use validation::*;
#[cfg(test)]
use widgets::runtime_widget_belongs_to_installation;
#[allow(dead_code)]
pub type RegisterWidgetRequest = widgets::RegisterWidgetRequest;
use widgets::{list_all_widgets, reconcile_manifest_widgets, register_widget, unregister_widget};

use axum::{
    middleware::from_fn_with_state,
    routing::{delete, get, post},
    Router,
};
use sea_orm::DatabaseConnection;

use crate::middleware::auth::{auth_middleware, optional_auth_middleware};
use crate::services::permission_service::TappPermission;

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

#[cfg(test)]
mod manifest_tests;
