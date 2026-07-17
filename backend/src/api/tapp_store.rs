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
    DatabaseConnection, DbErr, EntityTrait, FromQueryResult, QueryFilter, Set, Statement,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path as FsPath, PathBuf};
use tokio::fs;

use crate::api::tapp_runtime::common as tapp_common;
use crate::api::tapp_runtime::RuntimeGrantContext;
use crate::config::DynamicConfig;
use crate::middleware::auth::{
    auth_middleware, ensure_current_admin, extract_optional_claims, optional_auth_middleware,
    Claims,
};
use crate::models::entities::{
    tapp_storage, tapp_store_sources, tapp_user_activities, tapp_widgets, tapps,
};
use crate::services::data_paths::paths;
use crate::services::permission_service::{TappPermission, TappPermissionService, UserRole};
use crate::GLOBAL_DYNAMIC_CONFIG;

fn tapp_detail_from_model(
    tapp: tapps::Model,
    role: UserRole,
    is_temporary: bool,
    is_admin_tapp: bool,
    config: &DynamicConfig,
) -> TappDetail {
    let approved_permissions: Vec<String> =
        serde_json::from_value(tapp.approved_permissions.clone()).unwrap_or_default();
    let granted_permissions =
        TappPermissionService::filter_permissions_for_role(config, role, &approved_permissions);

    TappDetail {
        id: tapp.tapp_id,
        name: tapp.name,
        version: tapp.version,
        description: tapp.description,
        author: tapp.author,
        icon: tapp.icon,
        theme_color: tapp.theme_color,
        manifest: tapp.manifest,
        status: format!("{:?}", tapp.status).to_lowercase(),
        granted_permissions,
        installed_at: tapp.installed_at.to_rfc3339(),
        last_run_at: tapp.last_run_at.map(|dt| dt.to_rfc3339()),
        user_role: role.as_str().to_string(),
        is_temporary,
        is_admin_tapp,
    }
}

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

const MAX_TAPP_ID_LEN: usize = 128;
const MAX_RESOURCE_PATH_LEN: usize = 256;
const MAX_TAPP_ARCHIVE_BYTES: usize = 25 * 1024 * 1024;
const MAX_TAPP_ARCHIVE_FILES: usize = 512;
const MAX_TAPP_ARCHIVE_UNCOMPRESSED_BYTES: u64 = 100 * 1024 * 1024;
const MAX_TAPP_RESOURCE_BYTES: u64 = 25 * 1024 * 1024;
/// Declared package assets (textures, audio, wasm, levels). Binary allowed.
const MAX_TAPP_ASSETS: usize = 64;
const MAX_TAPP_ASSET_BYTES: u64 = 5 * 1024 * 1024;
const MAX_TAPP_ASSETS_TOTAL_BYTES: u64 = 20 * 1024 * 1024;
const MAX_TAPP_MANIFEST_BYTES: u64 = 256 * 1024;
const MAX_WIDGETS_PER_TAPP: usize = 64;
const MAX_DATA_EXCHANGE_DECLARATIONS: usize = 32;
const MAX_DATA_EXCHANGE_ID_LEN: usize = 128;
const MAX_DATA_EXCHANGE_SCHEMA_BYTES: usize = 64 * 1024;
const MAX_DATA_EXCHANGE_RESPONSE_BYTES: usize = 512 * 1024;
pub(crate) const MAX_AGENT_SCHEMA_RESOURCE_BYTES: usize = 64 * 1024;
const MAX_TAPP_I18N_FILES: usize = 32;
const MAX_TAPP_I18N_RESOURCE_BYTES: usize = 1024 * 1024;

fn valid_data_exchange_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_DATA_EXCHANGE_ID_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

pub(crate) fn validate_inline_data_schema(schema: &serde_json::Value) -> Result<(), String> {
    let object = schema
        .as_object()
        .ok_or_else(|| "Data Exchange schema must be an inline JSON object".to_string())?;
    let encoded = serde_json::to_vec(schema)
        .map_err(|_| "Data Exchange schema cannot be serialized".to_string())?;
    if encoded.len() > MAX_DATA_EXCHANGE_SCHEMA_BYTES {
        return Err(format!(
            "Data Exchange schema is too large (max {MAX_DATA_EXCHANGE_SCHEMA_BYTES} bytes)"
        ));
    }

    fn reject_refs(value: &serde_json::Value, depth: usize) -> Result<(), String> {
        if depth > 32 {
            return Err("Data Exchange schema nesting is too deep".to_string());
        }
        match value {
            serde_json::Value::Object(map) => {
                if map.contains_key("$ref") {
                    return Err("Data Exchange schema does not support $ref".to_string());
                }
                for child in map.values() {
                    reject_refs(child, depth + 1)?;
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    reject_refs(child, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    reject_refs(schema, 0)?;
    if !object.contains_key("type")
        && !object.contains_key("properties")
        && !object.contains_key("enum")
        && !object.contains_key("const")
    {
        return Err(
            "Data Exchange schema must declare type, properties, enum, or const".to_string(),
        );
    }
    Ok(())
}

fn is_safe_path_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TAPP_ID_LEN
        && value != "."
        && value != ".."
        && !value.starts_with('.')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

fn validate_tapp_id(tapp_id: &str) -> Result<(), String> {
    if tapp_id.len() > MAX_TAPP_ID_LEN
        || !tapp_id
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric())
        || !is_safe_path_component(tapp_id)
    {
        return Err(
            "Invalid Tapp id: use 1-128 ASCII letters, numbers, dots, underscores, or hyphens"
                .to_string(),
        );
    }
    Ok(())
}

fn validate_resource_path(path: &str) -> Result<(), String> {
    if path.is_empty()
        || path.len() > MAX_RESOURCE_PATH_LEN
        || path.contains('\\')
        || FsPath::new(path).is_absolute()
    {
        return Err(format!("Invalid Tapp resource path: {path}"));
    }

    let mut saw_component = false;
    for component in FsPath::new(path).components() {
        match component {
            Component::Normal(value) => {
                let value = value
                    .to_str()
                    .ok_or_else(|| format!("Invalid Tapp resource path: {path}"))?;
                if !is_safe_path_component(value) {
                    return Err(format!("Invalid Tapp resource path: {path}"));
                }
                saw_component = true;
            }
            _ => return Err(format!("Invalid Tapp resource path: {path}")),
        }
    }

    if !saw_component {
        return Err(format!("Invalid Tapp resource path: {path}"));
    }
    Ok(())
}

fn is_valid_widget_size(size: &str) -> bool {
    matches!(
        size,
        "1x1" | "1x2" | "2x1" | "2x2" | "2x3" | "3x2" | "4x1" | "4x2" | "2x4" | "3x3" | "4x4"
    )
}

fn tapp_setting_value_is_valid(setting: &TappSettingDef, value: &serde_json::Value) -> bool {
    match setting.setting_type.as_str() {
        "toggle" => value.is_boolean(),
        "input" | "color" => value.is_string(),
        "select" => value.as_str().is_some_and(|value| {
            setting
                .options
                .as_ref()
                .is_some_and(|options| options.iter().any(|option| option.value == value))
        }),
        "number" => value.as_f64().is_some_and(|value| {
            value.is_finite()
                && setting.min.is_none_or(|min| value >= min)
                && setting.max.is_none_or(|max| value <= max)
        }),
        _ => false,
    }
}

fn validate_tapp_settings(settings: &[TappSettingDef], scope: &str) -> Result<(), String> {
    if settings.len() > 64 {
        return Err(format!("{scope} accepts at most 64 settings"));
    }
    let mut keys = std::collections::HashSet::new();
    for setting in settings {
        if validate_storage_key(&setting.key).is_err()
            || !keys.insert(setting.key.as_str())
            || setting.label.is_empty()
            || setting.label.len() > 255
            || !matches!(
                setting.setting_type.as_str(),
                "toggle" | "select" | "input" | "number" | "color"
            )
        {
            return Err(format!(
                "Invalid or duplicate {scope} setting: {}",
                setting.key
            ));
        }
        if setting.setting_type == "select"
            && setting
                .options
                .as_ref()
                .is_none_or(|options| options.is_empty() || options.len() > 100)
        {
            return Err(format!(
                "Select {scope} setting {} requires 1-100 options",
                setting.key
            ));
        }
        if let Some(options) = &setting.options {
            let mut values = std::collections::HashSet::new();
            if setting.setting_type != "select"
                || options.iter().any(|option| {
                    option.value.is_empty()
                        || option.value.len() > 255
                        || option.label.is_empty()
                        || option.label.len() > 255
                        || !values.insert(option.value.as_str())
                })
            {
                return Err(format!(
                    "Invalid options for {scope} setting: {}",
                    setting.key
                ));
            }
        }
        let has_numeric_constraints =
            setting.min.is_some() || setting.max.is_some() || setting.step.is_some();
        if (has_numeric_constraints && setting.setting_type != "number")
            || (setting.placeholder.is_some() && setting.setting_type != "input")
        {
            return Err(format!(
                "Incompatible fields for {scope} setting: {}",
                setting.key
            ));
        }
        if setting
            .min
            .zip(setting.max)
            .is_some_and(|(min, max)| min > max)
            || setting.step.is_some_and(|step| step <= 0.0)
        {
            return Err(format!(
                "Invalid numeric range for {scope} setting: {}",
                setting.key
            ));
        }
        if let Some(default) = &setting.default_value {
            if !tapp_setting_value_is_valid(setting, default) {
                return Err(format!(
                    "Invalid defaultValue for {scope} setting: {}",
                    setting.key
                ));
            }
        }
    }
    Ok(())
}

fn validate_widget_refresh_policy(
    policy: &TappWidgetRefreshPolicy,
    widget_id: &str,
) -> Result<(), String> {
    match policy.mode {
        TappWidgetRefreshMode::Event if policy.interval_seconds.is_some() => Err(format!(
            "Event-driven Widget {widget_id} cannot declare intervalSeconds"
        )),
        TappWidgetRefreshMode::Event => Ok(()),
        TappWidgetRefreshMode::Interval
            if !matches!(policy.interval_seconds, Some(15..=86_400)) =>
        {
            Err(format!(
                "Interval Widget {widget_id} requires intervalSeconds between 15 and 86400"
            ))
        }
        TappWidgetRefreshMode::Interval => Ok(()),
    }
}

fn parse_system_version(value: &str) -> Result<semver::Version, String> {
    let normalized = value.strip_prefix('v').unwrap_or(value);
    semver::Version::parse(normalized)
        .map_err(|_| format!("Invalid minSystemVersion: {value}; expected semantic version"))
}

fn validate_http_url(value: &str, field: &str) -> Result<(), String> {
    if value.len() > 2_048 {
        return Err(format!("Tapp {field} is too long"));
    }
    let parsed = reqwest::Url::parse(value).map_err(|_| format!("Invalid Tapp {field}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("Tapp {field} must be an HTTP(S) URL"));
    }
    Ok(())
}

fn validate_resource_extension(path: &str, extension: &str, field: &str) -> Result<(), String> {
    if !path.ends_with(extension) {
        return Err(format!("Tapp {field} must reference a {extension} file"));
    }
    Ok(())
}

fn validate_asset_path(path: &str) -> Result<(), String> {
    validate_resource_path(path)?;
    if !path.starts_with("assets/") || path == "assets" || path.ends_with('/') {
        return Err(format!(
            "Tapp asset path must be a file under assets/: {path}"
        ));
    }
    // Reject nested path escape already handled by validate_resource_path.
    // Disallow treating runtime entrypoints as assets.
    if path.ends_with(".js") || path.ends_with(".html") {
        return Err(format!(
            "Tapp asset path must not be a script or HTML entry: {path}"
        ));
    }
    Ok(())
}

fn guess_asset_mime_type(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else if lower.ends_with(".wav") {
        "audio/wav"
    } else if lower.ends_with(".mp3") {
        "audio/mpeg"
    } else if lower.ends_with(".ogg") {
        "audio/ogg"
    } else if lower.ends_with(".wasm") {
        "application/wasm"
    } else if lower.ends_with(".json") {
        "application/json"
    } else if lower.ends_with(".txt") || lower.ends_with(".csv") {
        "text/plain"
    } else if lower.ends_with(".bin") {
        "application/octet-stream"
    } else if lower.ends_with(".glb") {
        "model/gltf-binary"
    } else if lower.ends_with(".gltf") {
        "model/gltf+json"
    } else {
        "application/octet-stream"
    }
}

fn decode_asset_base64(value: &str) -> Result<Vec<u8>, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let trimmed = value.trim();
    // Allow optional data-URL prefix: data:<mime>;base64,<payload>
    let payload = trimmed
        .split_once("base64,")
        .map(|(_, data)| data)
        .unwrap_or(trimmed);
    STANDARD
        .decode(payload.trim())
        .map_err(|_| "Invalid asset base64 encoding".to_string())
}

pub(crate) fn validate_tapp_manifest(manifest: &TappManifest) -> Result<(), String> {
    validate_tapp_id(&manifest.id)?;
    if manifest.name.trim().is_empty() || manifest.name.len() > 255 {
        return Err("Tapp name must contain 1-255 characters".to_string());
    }
    semver::Version::parse(&manifest.version)
        .map_err(|_| "Tapp version must be valid semantic version".to_string())?;
    if manifest.category.is_none() {
        return Err("Tapp category is required".to_string());
    }
    if manifest
        .description
        .as_ref()
        .is_some_and(|description| description.len() > 2_000)
    {
        return Err("Tapp description must not exceed 2000 characters".to_string());
    }
    if manifest
        .icon
        .as_ref()
        .is_some_and(|icon| icon.len() > 2_048)
    {
        return Err("Tapp icon must not exceed 2048 characters".to_string());
    }
    if manifest
        .icon_svg
        .as_ref()
        .is_some_and(|icon| icon.len() > 65_536)
    {
        return Err("Tapp iconSvg must not exceed 64 KiB".to_string());
    }
    if manifest.theme_color.as_ref().is_some_and(|color| {
        color.len() != 7
            || !color.starts_with('#')
            || !color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        return Err("Tapp themeColor must use #RRGGBB format".to_string());
    }
    for (field, value) in [
        ("homepage", manifest.homepage.as_deref()),
        ("repository", manifest.repository.as_deref()),
    ] {
        if let Some(value) = value {
            validate_http_url(value, field)?;
        }
    }
    validate_resource_path(&manifest.main)?;
    validate_resource_extension(&manifest.main, ".js", "main")?;
    if let Some(required) = manifest.min_system_version.as_deref() {
        let required = parse_system_version(required)?;
        let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))
            .expect("backend package version must be valid semver");
        if current < required {
            return Err(format!(
                "Tapp requires Myriad {required} or newer; current version is {current}"
            ));
        }
    }
    if let Some(author) = &manifest.author {
        if author.name.trim().is_empty() || author.name.len() > 255 {
            return Err("Tapp author.name must contain 1-255 characters".to_string());
        }
        if author.email.as_ref().is_some_and(|email| {
            email.len() > 320 || email.chars().any(char::is_whitespace) || !email.contains('@')
        }) {
            return Err("Invalid Tapp author.email".to_string());
        }
        if let Some(url) = &author.url {
            validate_http_url(url, "author.url")?;
        }
    }
    if manifest.permissions.len() > 64 {
        return Err("Tapp permissions accepts at most 64 entries".to_string());
    }
    let mut permissions = std::collections::HashSet::new();
    for permission in &manifest.permissions {
        if TappPermission::from_str(permission).is_none()
            || !permissions.insert(permission.as_str())
        {
            return Err(format!(
                "Unknown or duplicate Tapp permission: {permission}"
            ));
        }
    }
    if manifest
        .css_mode
        .as_deref()
        .is_some_and(|mode| !matches!(mode, "unified" | "separated"))
    {
        return Err("Tapp cssMode must be unified or separated".to_string());
    }

    for (field, path, extension) in [
        ("styles", manifest.styles.as_deref(), ".css"),
        ("widgetStyles", manifest.widget_styles.as_deref(), ".css"),
        ("pageStyles", manifest.page_styles.as_deref(), ".css"),
        ("pageTemplate", manifest.page_template.as_deref(), ".html"),
    ] {
        let Some(path) = path else { continue };
        validate_resource_path(path)?;
        validate_resource_extension(path, extension, field)?;
    }

    if let Some(modules) = &manifest.page_modules {
        if modules.len() > 64 {
            return Err("Tapp pageModules accepts at most 64 entries".to_string());
        }
        let mut seen = std::collections::HashSet::new();
        for module in modules {
            if !is_safe_path_component(module)
                || !module.ends_with(".js")
                || !seen.insert(module.as_str())
            {
                return Err(format!(
                    "Invalid or duplicate page module filename: {module}; expected a .js file relative to page/"
                ));
            }
        }
    }

    if let Some(requirements) = &manifest.background_requirements {
        if requirements.len() > 16 {
            return Err("Tapp backgroundRequirements accepts at most 16 entries".to_string());
        }
        let mut seen = std::collections::HashSet::new();
        for requirement in requirements {
            if !matches!(
                requirement.as_str(),
                "media" | "sync" | "notification" | "scheduler" | "event-listener" | "realtime"
            ) || !seen.insert(requirement.as_str())
            {
                return Err(format!(
                    "Unknown or duplicate background requirement: {requirement}"
                ));
            }
        }
    }

    if let Some(settings) = &manifest.settings {
        validate_tapp_settings(settings, "Tapp")?;
    }

    if let Some(assets) = &manifest.assets {
        if assets.len() > MAX_TAPP_ASSETS {
            return Err(format!(
                "Tapp assets accepts at most {MAX_TAPP_ASSETS} entries"
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for path in assets {
            validate_asset_path(path)?;
            if !seen.insert(path.as_str()) {
                return Err(format!("Duplicate Tapp asset path: {path}"));
            }
        }
    }

    if let Some(widgets) = &manifest.widgets {
        if !widgets.is_empty()
            && !manifest
                .permissions
                .iter()
                .any(|permission| permission == "widget:register")
        {
            return Err("Tapp widgets require widget:register permission".to_string());
        }
        if widgets.len() > MAX_WIDGETS_PER_TAPP {
            return Err(format!("Too many Widgets (max {MAX_WIDGETS_PER_TAPP})"));
        }
        let mut widget_ids = std::collections::HashSet::new();
        for widget in widgets {
            if !is_safe_path_component(&widget.id) || !widget_ids.insert(widget.id.as_str()) {
                return Err(format!("Invalid or duplicate Widget ID: {}", widget.id));
            }
            if widget.name.is_empty() || widget.name.len() > 255 {
                return Err(format!("Invalid Widget name: {}", widget.id));
            }
            if widget.sizes.is_empty()
                || widget.sizes.len() > 10
                || widget.sizes.iter().any(|size| !is_valid_widget_size(size))
                || !widget.sizes.contains(&widget.default_size)
            {
                return Err(format!("Invalid Widget sizes: {}", widget.id));
            }
            validate_tapp_settings(&widget.settings, &format!("Widget {}", widget.id))?;
            if let Some(policy) = &widget.refresh_policy {
                validate_widget_refresh_policy(policy, &widget.id)?;
            }
            if let Some(templates) = &widget.templates {
                for (size, path) in templates {
                    if !is_valid_widget_size(size) || !widget.sizes.contains(size) {
                        return Err(format!(
                            "Widget template uses an undeclared size {size}: {}",
                            widget.id
                        ));
                    }
                    validate_resource_path(path)?;
                    validate_resource_extension(path, ".html", "Widget template")?;
                }
            }
        }
    }

    if let Some(apis) = &manifest.apis {
        if apis.len() > 64 {
            return Err("Tapp apis accepts at most 64 entries".to_string());
        }
        for (name, api) in apis {
            if !valid_agent_name(name) {
                return Err(format!("Invalid Tapp API name: {name}"));
            }
            if api.cache_ttl > 86_400 {
                return Err(format!(
                    "Tapp API {name} cacheTtl must not exceed 86400 seconds"
                ));
            }
            if serde_json::to_string(api).is_ok_and(|encoded| encoded.contains("{{secrets.")) {
                return Err(format!(
                    "Tapp API {name} cannot reference host secret templates"
                ));
            }
            match api.api_type.as_str() {
                "http" => {
                    if api.endpoint.is_none() {
                        return Err(format!("HTTP Tapp API {name} requires endpoint"));
                    }
                    if api.builtin.is_some() {
                        return Err(format!("HTTP Tapp API {name} cannot declare builtin"));
                    }
                    if !manifest
                        .permissions
                        .iter()
                        .any(|permission| permission == "network:fetch")
                    {
                        return Err(format!("HTTP Tapp API {name} requires network:fetch"));
                    }
                }
                "builtin" => {
                    let Some(builtin) = api.builtin.as_deref() else {
                        return Err(format!("Builtin Tapp API {name} requires builtin"));
                    };
                    if !matches!(builtin, "geo" | "ai:chat" | "ai:generate") {
                        return Err(format!("Unknown builtin Tapp API: {builtin}"));
                    }
                    if api.endpoint.is_some()
                        || api.headers.is_some()
                        || api.body.is_some()
                        || api.spoof.is_some()
                        || api.inject.is_some()
                    {
                        return Err(format!("Builtin Tapp API {name} contains HTTP-only fields"));
                    }
                    let required_permission = match builtin {
                        "ai:chat" => Some("ai:chat"),
                        "ai:generate" => Some("ai:generate"),
                        _ => None,
                    };
                    if required_permission.is_some_and(|required| {
                        !manifest
                            .permissions
                            .iter()
                            .any(|permission| permission == required)
                    }) {
                        return Err(format!(
                            "Builtin Tapp API {name} requires permission {}",
                            required_permission.expect("checked permission")
                        ));
                    }
                    let required_operation = match builtin {
                        "ai:chat" => Some(TappAiOperation::Chat),
                        "ai:generate" => Some(TappAiOperation::Generate),
                        _ => None,
                    };
                    if let Some(operation) = required_operation {
                        let ai = manifest.ai.as_ref().ok_or_else(|| {
                            format!(
                                "Builtin Tapp API {name} requires a protocolVersion 2 AI declaration"
                            )
                        })?;
                        if ai.protocol_version != 2 || !ai.operations.contains(&operation) {
                            return Err(format!(
                                "Builtin Tapp API {name} requires the matching AI operation"
                            ));
                        }
                        if !ai.output_formats.contains(&TappAiOutputFormat::Text) {
                            return Err(format!("Builtin Tapp API {name} requires AI text output"));
                        }
                    }
                }
                other => return Err(format!("Unknown Tapp API type: {other}")),
            }
            if api.method.len() > 16 || api.method.parse::<reqwest::Method>().is_err() {
                return Err(format!("Invalid HTTP method for Tapp API {name}"));
            }
            if let Some(inject) = &api.inject {
                if inject.len() > 32 {
                    return Err(format!("Tapp API {name} inject accepts at most 32 aliases"));
                }
                for (alias, template) in inject {
                    if !valid_agent_name(alias)
                        || ["user.", "geo.", "secrets.", "params."]
                            .iter()
                            .any(|prefix| alias.starts_with(prefix))
                    {
                        return Err(format!(
                            "Invalid or reserved inject alias for Tapp API {name}: {alias}"
                        ));
                    }
                    if template.is_empty() || template.len() > 2_048 {
                        return Err(format!(
                            "Invalid inject template for Tapp API {name}: {alias}"
                        ));
                    }
                }
            }
        }
    }

    if let Some(exchange) = &manifest.data_exchange {
        if exchange.exports.len() > MAX_DATA_EXCHANGE_DECLARATIONS
            || exchange.imports.len() > MAX_DATA_EXCHANGE_DECLARATIONS
        {
            return Err(format!(
                "Too many Data Exchange declarations (max {MAX_DATA_EXCHANGE_DECLARATIONS} per direction)"
            ));
        }

        let mut export_ids = std::collections::HashSet::new();
        for export in &exchange.exports {
            if !valid_data_exchange_id(&export.id) || !export_ids.insert(export.id.as_str()) {
                return Err(format!(
                    "Invalid or duplicate Data Exchange export id: {}",
                    export.id
                ));
            }
            if export.max_bytes == 0 || export.max_bytes > MAX_DATA_EXCHANGE_RESPONSE_BYTES {
                return Err(format!(
                    "Data Exchange export {} maxBytes must be between 1 and {MAX_DATA_EXCHANGE_RESPONSE_BYTES}",
                    export.id
                ));
            }
            if export
                .max_records
                .is_some_and(|limit| limit == 0 || limit > 10_000)
            {
                return Err(format!(
                    "Data Exchange export {} maxRecords must be between 1 and 10000",
                    export.id
                ));
            }
            if export
                .description
                .as_ref()
                .is_some_and(|description| description.len() > 500)
            {
                return Err(format!(
                    "Data Exchange export {} description is too long",
                    export.id
                ));
            }
            validate_inline_data_schema(&export.schema)?;
        }

        let mut imports = std::collections::HashSet::new();
        for import in &exchange.imports {
            validate_tapp_id(&import.tapp_id)?;
            if !valid_data_exchange_id(&import.export_id)
                || !imports.insert((import.tapp_id.as_str(), import.export_id.as_str()))
            {
                return Err(format!(
                    "Invalid or duplicate Data Exchange import: {} / {}",
                    import.tapp_id, import.export_id
                ));
            }
        }
    }

    if let Some(ai) = &manifest.ai {
        if ai.protocol_version != 2 {
            return Err("Tapp AI protocolVersion must be 2".to_string());
        }
        if ai.operations.is_empty() || ai.operations.len() > 4 {
            return Err("Tapp AI operations must contain 1-4 entries".to_string());
        }
        if ai.output_formats.is_empty() || ai.output_formats.len() > 3 {
            return Err("Tapp AI outputFormats must contain 1-3 entries".to_string());
        }

        let mut operations = std::collections::HashSet::new();
        for operation in &ai.operations {
            if !operations.insert(*operation) {
                return Err("Tapp AI operations contains duplicates".to_string());
            }
            let permission = operation.permission();
            if !manifest.permissions.iter().any(|value| value == permission) {
                return Err(format!(
                    "Tapp AI operation requires manifest permission {permission}"
                ));
            }
        }

        let mut context_sources = std::collections::HashSet::new();
        if ai.context_sources.len() > 4
            || ai
                .context_sources
                .iter()
                .any(|source| !context_sources.insert(*source))
        {
            return Err(
                "Tapp AI contextSources contains duplicates or too many entries".to_string(),
            );
        }
        if context_sources.contains(&TappAiContextSource::Platform)
            && !manifest
                .permissions
                .iter()
                .any(|value| value == "platform:read")
        {
            return Err("Tapp AI platform context requires platform:read".to_string());
        }
        if context_sources.contains(&TappAiContextSource::Report)
            && !manifest
                .permissions
                .iter()
                .any(|value| value == "report:read")
        {
            return Err("Tapp AI report context requires report:read".to_string());
        }

        let mut output_formats = std::collections::HashSet::new();
        if ai
            .output_formats
            .iter()
            .any(|format| !output_formats.insert(*format))
        {
            return Err("Tapp AI outputFormats contains duplicates".to_string());
        }
        if operations.contains(&TappAiOperation::Image)
            && !output_formats.contains(&TappAiOutputFormat::Image)
        {
            return Err("Tapp AI image operation requires image output format".to_string());
        }
    }

    if let Some(events) = &manifest.events {
        if events.publish.len() > 100 || events.subscribe.len() > 100 {
            return Err("Tapp events publish/subscribe accept at most 100 topics".to_string());
        }
        let publish_prefix = format!("tapp.{}.", manifest.id);
        let mut publish_topics = std::collections::HashSet::new();
        for topic in &events.publish {
            if !valid_event_topic(topic)
                || !topic.starts_with(&publish_prefix)
                || !publish_topics.insert(topic.as_str())
            {
                return Err(format!(
                    "Invalid or duplicate Tapp event publish topic: {topic}"
                ));
            }
        }
        let mut subscribe_topics = std::collections::HashSet::new();
        for topic in &events.subscribe {
            if !valid_event_topic(topic)
                || (!topic.starts_with("tapp.") && !topic.starts_with("system."))
                || !subscribe_topics.insert(topic.as_str())
            {
                return Err(format!(
                    "Invalid or duplicate Tapp event subscribe topic: {topic}"
                ));
            }
        }
        let declares =
            |permission: &str| manifest.permissions.iter().any(|value| value == permission);
        if !events.publish.is_empty() && !declares("event:publish") {
            return Err("Tapp event publish topics require event:publish".to_string());
        }
        if !events.subscribe.is_empty() && !declares("event:subscribe") {
            return Err("Tapp event subscribe topics require event:subscribe".to_string());
        }
    }

    if let Some(agent) = &manifest.agent {
        if agent.protocol_version != 2 {
            return Err("Tapp agent protocolVersion must be 2".to_string());
        }
        if agent.interactions.is_empty() || agent.interactions.len() > 32 {
            return Err("Tapp agent interactions must contain 1-32 entries".to_string());
        }
        let mut interaction_types = std::collections::HashSet::new();
        for interaction in &agent.interactions {
            if !valid_agent_name(&interaction.interaction_type)
                || !interaction_types.insert(interaction.interaction_type.as_str())
            {
                return Err(format!(
                    "Invalid or duplicate Agent interaction type: {}",
                    interaction.interaction_type
                ));
            }
            for schema in [
                interaction.input_schema.as_deref(),
                interaction.result_schema.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                validate_resource_path(schema)?;
                if !schema.ends_with(".json") {
                    return Err(format!("Agent schema must be a JSON resource: {schema}"));
                }
            }
        }
        if agent.intents.len() > 16 {
            return Err("Tapp agent intents accepts at most 16 entries".to_string());
        }
        let mut intents = std::collections::HashSet::new();
        for intent in &agent.intents {
            if !matches!(
                intent.as_str(),
                "ui.open" | "report.create" | "dataExchange.request"
            ) || !intents.insert(intent.as_str())
            {
                return Err(format!("Invalid or duplicate Agent intent: {intent}"));
            }
        }
    }
    Ok(())
}

fn validate_named_resource_keys<'a>(
    keys: impl IntoIterator<Item = &'a String>,
    kind: &str,
) -> Result<(), String> {
    for key in keys {
        if !is_safe_path_component(key) {
            return Err(format!("Invalid {kind}: {key}"));
        }
    }
    Ok(())
}

fn tapp_dir_for(user_id: i32, tapp_id: &str) -> Result<PathBuf, String> {
    validate_tapp_id(tapp_id)?;
    Ok(paths().tapp_user_dir(user_id).join(tapp_id))
}

struct TappDirStage {
    path: PathBuf,
}

impl TappDirStage {
    async fn create(final_path: &FsPath) -> Result<Self, std::io::Error> {
        let parent = final_path.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Tapp directory has no parent",
            )
        })?;
        fs::create_dir_all(parent).await?;
        let name = final_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("tapp");
        let path = parent.join(format!(".{name}.staging-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&path).await?;
        Ok(Self { path })
    }

    fn path(&self) -> &FsPath {
        &self.path
    }

    async fn activate(self, final_path: &FsPath) -> Result<ActivatedTappDir, std::io::Error> {
        let backup_path = if final_path.exists() {
            let parent = final_path.parent().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Tapp directory has no parent",
                )
            })?;
            let name = final_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("tapp");
            let backup = parent.join(format!(".{name}.backup-{}", uuid::Uuid::new_v4().simple()));
            fs::rename(final_path, &backup).await?;
            Some(backup)
        } else {
            None
        };

        if let Err(error) = fs::rename(&self.path, final_path).await {
            if let Some(backup) = &backup_path {
                let _ = fs::rename(backup, final_path).await;
            }
            return Err(error);
        }
        Ok(ActivatedTappDir {
            final_path: final_path.to_path_buf(),
            backup_path,
        })
    }
}

impl Drop for TappDirStage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

struct ActivatedTappDir {
    final_path: PathBuf,
    backup_path: Option<PathBuf>,
}

const TAPP_INSTALL_STATE_FILE: &str = ".myriad-install-state.json";

impl ActivatedTappDir {
    async fn commit(mut self) {
        if let Some(backup) = self.backup_path.take() {
            if let Err(error) = fs::remove_dir_all(&backup).await {
                tracing::warn!(path = %backup.display(), %error, "Failed to remove old Tapp backup");
            }
        }
    }

    async fn rollback(mut self) {
        let _ = fs::remove_dir_all(&self.final_path).await;
        if let Some(backup) = self.backup_path.take() {
            let _ = fs::rename(backup, &self.final_path).await;
        }
    }
}

fn directory_manifest_matches(directory: &FsPath, expected: &serde_json::Value) -> bool {
    let Ok(content) = std::fs::read_to_string(directory.join("manifest.json")) else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(&content).is_ok_and(|value| value == *expected)
}

fn write_install_generation(
    directory: &FsPath,
    updated_at: chrono::DateTime<chrono::FixedOffset>,
) -> Result<(), std::io::Error> {
    let value = serde_json::json!({ "updatedAtMicros": updated_at.timestamp_micros() });
    let encoded = serde_json::to_vec(&value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    std::fs::write(directory.join(TAPP_INSTALL_STATE_FILE), encoded)
}

fn directory_generation_matches(
    directory: &FsPath,
    expected_manifest: &serde_json::Value,
    expected_updated_at: chrono::DateTime<chrono::FixedOffset>,
) -> bool {
    let state_path = directory.join(TAPP_INSTALL_STATE_FILE);
    if state_path.exists() {
        return std::fs::read(state_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| {
                value
                    .get("updatedAtMicros")
                    .and_then(serde_json::Value::as_i64)
            })
            == Some(expected_updated_at.timestamp_micros());
    }
    // Compatibility for installations created before generation markers.
    directory_manifest_matches(directory, expected_manifest)
}

fn lifecycle_artifact_directories(final_path: &FsPath) -> Result<Vec<PathBuf>, std::io::Error> {
    let Some(parent) = final_path.parent() else {
        return Ok(Vec::new());
    };
    let Some(name) = final_path.file_name().and_then(|value| value.to_str()) else {
        return Ok(Vec::new());
    };
    let prefixes = [
        format!(".{name}.staging-"),
        format!(".{name}.backup-"),
        format!(".{name}.uninstall-"),
        format!(".{name}.recovery-discard-"),
    ];
    let mut artifacts = Vec::new();
    if !parent.is_dir() {
        return Ok(artifacts);
    }
    for entry in std::fs::read_dir(parent)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let filename = entry.file_name();
        let Some(filename) = filename.to_str() else {
            continue;
        };
        if prefixes.iter().any(|prefix| filename.starts_with(prefix)) {
            artifacts.push(entry.path());
        }
    }
    Ok(artifacts)
}

/// Reconcile one live resource directory with the database Manifest after an
/// interrupted install/update/uninstall lifecycle transaction.
fn recover_tapp_directory(
    final_path: &FsPath,
    expected_manifest: &serde_json::Value,
    expected_updated_at: chrono::DateTime<chrono::FixedOffset>,
) -> Result<bool, std::io::Error> {
    let mut artifacts = lifecycle_artifact_directories(final_path)?;
    // A backup/uninstall quarantine is the authoritative pre-transaction
    // generation. Consider staging only after those recovery sources.
    artifacts.sort_by_key(|path| {
        path.file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.contains(".staging-"))
    });
    if directory_generation_matches(final_path, expected_manifest, expected_updated_at) {
        for artifact in artifacts {
            std::fs::remove_dir_all(artifact)?;
        }
        return Ok(false);
    }

    let Some(recovery_source) = artifacts
        .iter()
        .find(|path| directory_generation_matches(path, expected_manifest, expected_updated_at))
        .cloned()
    else {
        return Ok(false);
    };

    let discard_path = final_path.with_file_name(format!(
        ".{}.recovery-discard-{}",
        final_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("tapp"),
        uuid::Uuid::new_v4().simple()
    ));
    let had_live_path = final_path.exists();
    if had_live_path {
        std::fs::rename(final_path, &discard_path)?;
    }
    if let Err(error) = std::fs::rename(&recovery_source, final_path) {
        if had_live_path {
            let _ = std::fs::rename(&discard_path, final_path);
        }
        return Err(error);
    }
    if had_live_path {
        let _ = std::fs::remove_dir_all(&discard_path);
    }
    for artifact in artifacts {
        if artifact != recovery_source {
            let _ = std::fs::remove_dir_all(artifact);
        }
    }
    Ok(true)
}

fn lifecycle_artifact_tapp_id(filename: &str) -> Option<&str> {
    let stem = filename.strip_prefix('.')?;
    let (prefix, nonce) = stem.rsplit_once('-')?;
    if nonce.len() != 32 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    [".staging", ".backup", ".uninstall", ".recovery-discard"]
        .into_iter()
        .find_map(|kind| prefix.strip_suffix(kind))
        .filter(|tapp_id| validate_tapp_id(tapp_id).is_ok())
}

fn looks_like_tapp_installation(directory: &FsPath) -> bool {
    ["manifest.json", TAPP_INSTALL_STATE_FILE]
        .into_iter()
        .any(|name| std::fs::symlink_metadata(directory.join(name)).is_ok())
}

/// Remove filesystem generations that cannot belong to any database row.
/// Artifacts for an installed key are deliberately retained when normal
/// recovery cannot identify the expected generation, avoiding destructive
/// guesses in the presence of partial/manual damage.
fn orphaned_tapp_directories(
    root: &FsPath,
    installed: &std::collections::HashSet<(i32, String)>,
) -> Result<Vec<(i32, String, PathBuf)>, std::io::Error> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut candidates = Vec::new();
    for owner_entry in std::fs::read_dir(root)? {
        let owner_entry = owner_entry?;
        let owner_type = owner_entry.file_type()?;
        if !owner_type.is_dir() || owner_type.is_symlink() {
            continue;
        }
        let Some(owner_name) = owner_entry.file_name().to_str().map(String::from) else {
            continue;
        };
        let Ok(owner_id) = owner_name.parse::<i32>() else {
            continue;
        };
        if owner_id < 0 || owner_id.to_string() != owner_name {
            continue;
        }
        for entry in std::fs::read_dir(owner_entry.path())? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }
            let Some(filename) = entry.file_name().to_str().map(String::from) else {
                continue;
            };
            let tapp_id = lifecycle_artifact_tapp_id(&filename).map(String::from);
            let live_tapp_id = if tapp_id.is_none()
                && validate_tapp_id(&filename).is_ok()
                && looks_like_tapp_installation(&entry.path())
            {
                Some(filename.clone())
            } else {
                None
            };
            let Some(tapp_id) = tapp_id.or(live_tapp_id) else {
                continue;
            };
            if installed.contains(&(owner_id, tapp_id.clone())) {
                continue;
            }
            candidates.push((owner_id, tapp_id, entry.path()));
        }
    }
    Ok(candidates)
}

async fn cleanup_orphaned_tapp_directories(
    db: &DatabaseConnection,
    installed: &std::collections::HashSet<(i32, String)>,
) -> Result<usize, DbErr> {
    let candidates = orphaned_tapp_directories(&paths().tapps, installed)
        .map_err(|error| DbErr::Custom(format!("Failed to inspect Tapp resources: {error}")))?;
    let mut removed = 0;
    for (owner_id, tapp_id, directory) in candidates {
        // Recovery also runs after a live database reconfiguration. Serialize
        // with install/update/uninstall and re-check under the lock so a newly
        // activated, not-yet-committed generation is never mistaken for an
        // orphan.
        let txn = db.begin().await?;
        lock_tapp_lifecycle(&txn, &tapp_id).await?;
        let exists = tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(owner_id))
            .filter(tapps::Column::TappId.eq(&tapp_id))
            .one(&txn)
            .await?
            .is_some();
        if !exists {
            match fs::symlink_metadata(&directory).await {
                Ok(metadata)
                    if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() =>
                {
                    fs::remove_dir_all(&directory).await.map_err(|error| {
                        DbErr::Custom(format!(
                            "Failed to remove orphaned Tapp directory {}: {error}",
                            directory.display()
                        ))
                    })?;
                    removed += 1;
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(DbErr::Custom(format!(
                        "Failed to inspect orphaned Tapp directory {}: {error}",
                        directory.display()
                    )))
                }
            }
        }
        txn.commit().await?;
    }
    Ok(removed)
}

/// Startup recovery for filesystem/DB transactions interrupted between the
/// atomic directory rename and the PostgreSQL commit.
pub(crate) async fn recover_tapp_filesystem_state(db: &DatabaseConnection) -> Result<usize, DbErr> {
    let installed = tapps::Entity::find().all(db).await?;
    let installed_keys = installed
        .iter()
        .map(|tapp| (tapp.user_id, tapp.tapp_id.clone()))
        .collect::<std::collections::HashSet<_>>();
    let mut recovered = 0;
    for tapp in installed {
        let Ok(final_path) = installed_tapp_dir(&tapp) else {
            tracing::error!(tapp_id = %tapp.tapp_id, "Invalid installed Tapp path during recovery");
            continue;
        };
        match recover_tapp_directory(&final_path, &tapp.manifest, tapp.updated_at) {
            Ok(true) => {
                recovered += 1;
                tracing::warn!(tapp_id = %tapp.tapp_id, owner_id = tapp.user_id, "Recovered interrupted Tapp filesystem transaction");
            }
            Ok(false) => {}
            Err(error) => tracing::error!(
                tapp_id = %tapp.tapp_id,
                owner_id = tapp.user_id,
                %error,
                "Failed to recover interrupted Tapp filesystem transaction"
            ),
        }
    }
    let removed = cleanup_orphaned_tapp_directories(db, &installed_keys).await?;
    if removed > 0 {
        tracing::warn!(removed, "Removed orphaned Tapp filesystem generations");
    }
    recovered += removed;
    Ok(recovered)
}

pub(crate) fn installed_tapp_dir(tapp: &tapps::Model) -> Result<PathBuf, StatusCode> {
    tapp_dir_for(tapp.user_id, &tapp.tapp_id).map_err(|_| StatusCode::BAD_REQUEST)
}

fn installed_code_path(tapp: &tapps::Model) -> Result<PathBuf, StatusCode> {
    // 新安装遵循 Manifest 的 main。旧安装可能曾把任意入口统一写为根目录
    // main.js/index.js，因此仅在 Manifest 路径不存在时回退持久化元数据。
    if let Some(main) = tapp.manifest.get("main").and_then(|value| value.as_str()) {
        if let Some(path) = regular_resource_path(&installed_tapp_dir(tapp)?, main) {
            return Ok(path);
        }
    }

    let stored_code_path = PathBuf::from(&tapp.code_path);
    let filename = stored_code_path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| matches!(*value, "main.js" | "index.js"))
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    regular_resource_path(&installed_tapp_dir(tapp)?, filename)
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)
}

pub(crate) fn resource_path(tapp_dir: &FsPath, relative: &str) -> Option<PathBuf> {
    validate_resource_path(relative).ok()?;
    Some(tapp_dir.join(relative))
}

/// Resolve an installed resource only when every path component remains under
/// the canonical Tapp directory and the target is a regular file. This rejects
/// both final and intermediate symlinks inserted after installation.
fn regular_resource_path(tapp_dir: &FsPath, relative: &str) -> Option<PathBuf> {
    let joined = resource_path(tapp_dir, relative)?;
    let canonical_root = std::fs::canonicalize(tapp_dir).ok()?;
    let canonical_path = std::fs::canonicalize(joined).ok()?;
    if canonical_path != canonical_root.join(relative) {
        return None;
    }
    let metadata = std::fs::symlink_metadata(&canonical_path).ok()?;
    metadata.file_type().is_file().then_some(canonical_path)
}

fn regular_resource_directory(tapp_dir: &FsPath, relative: &str) -> Option<PathBuf> {
    let joined = resource_path(tapp_dir, relative)?;
    let canonical_root = std::fs::canonicalize(tapp_dir).ok()?;
    let canonical_path = std::fs::canonicalize(joined).ok()?;
    if canonical_path != canonical_root.join(relative) {
        return None;
    }
    let metadata = std::fs::symlink_metadata(&canonical_path).ok()?;
    metadata.file_type().is_dir().then_some(canonical_path)
}

async fn read_tapp_text_resource(
    tapp_dir: &FsPath,
    relative: &str,
) -> Result<String, std::io::Error> {
    let path = regular_resource_path(tapp_dir, relative).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Tapp resource is not a regular in-sandbox file",
        )
    })?;
    fs::read_to_string(path).await
}

async fn write_tapp_resource(
    tapp_dir: &FsPath,
    relative: &str,
    content: impl AsRef<[u8]>,
) -> Result<PathBuf, std::io::Error> {
    let path = resource_path(tapp_dir, relative).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid Tapp resource path",
        )
    })?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(&path, content).await?;
    Ok(path)
}

/// Write base64-encoded package assets for direct install/update.
async fn write_install_assets(
    tapp_dir: &FsPath,
    manifest: &TappManifest,
    assets: &std::collections::HashMap<String, String>,
) -> Result<(), String> {
    let declared: std::collections::HashSet<&str> = manifest
        .assets
        .as_ref()
        .map(|list| list.iter().map(String::as_str).collect())
        .unwrap_or_default();
    if declared.is_empty() && !assets.is_empty() {
        return Err("assets payload requires manifest.assets declarations".to_string());
    }
    let mut total: u64 = 0;
    for (relative, encoded) in assets {
        validate_asset_path(relative)?;
        if !declared.contains(relative.as_str()) {
            return Err(format!(
                "Asset path is not declared in manifest.assets: {relative}"
            ));
        }
        let bytes = decode_asset_base64(encoded)?;
        let size = bytes.len() as u64;
        if size > MAX_TAPP_ASSET_BYTES {
            return Err(format!(
                "Tapp asset exceeds {MAX_TAPP_ASSET_BYTES} bytes: {relative}"
            ));
        }
        total = total
            .checked_add(size)
            .ok_or_else(|| "Tapp assets total size overflow".to_string())?;
        if total > MAX_TAPP_ASSETS_TOTAL_BYTES {
            return Err(format!(
                "Tapp assets total size exceeds {MAX_TAPP_ASSETS_TOTAL_BYTES} bytes"
            ));
        }
        write_tapp_resource(tapp_dir, relative, &bytes)
            .await
            .map_err(|_| format!("Failed to save asset: {relative}"))?;
    }
    Ok(())
}

async fn copy_regular_tapp_directory(
    source: &FsPath,
    destination: &FsPath,
) -> Result<(), std::io::Error> {
    let mut pending = vec![(source.to_path_buf(), destination.to_path_buf())];
    while let Some((source_dir, destination_dir)) = pending.pop() {
        let mut entries = fs::read_dir(&source_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            if file_type.is_symlink() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "Tapp resource directory contains a symbolic link",
                ));
            }
            let destination_path = destination_dir.join(entry.file_name());
            if file_type.is_dir() {
                fs::create_dir(&destination_path).await?;
                pending.push((entry.path(), destination_path));
            } else if file_type.is_file() {
                fs::copy(entry.path(), destination_path).await?;
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Tapp resource directory contains a non-regular entry",
                ));
            }
        }
    }
    Ok(())
}

type WidgetTemplateContents =
    std::collections::HashMap<String, std::collections::HashMap<String, String>>;

fn widget_template_path<'a>(
    manifest: &'a TappManifest,
    widget_id: &str,
    size: &str,
) -> Option<&'a str> {
    manifest
        .widgets
        .as_ref()
        .into_iter()
        .flatten()
        .find(|widget| widget.id == widget_id)
        .and_then(|widget| widget.templates.as_ref())
        .and_then(|templates| templates.get(size))
        .map(String::as_str)
}

fn validate_widget_template_contents(
    manifest: &TappManifest,
    contents: &WidgetTemplateContents,
) -> Result<(), String> {
    for (widget_id, templates) in contents {
        let widget = manifest
            .widgets
            .as_ref()
            .and_then(|widgets| widgets.iter().find(|widget| widget.id == *widget_id))
            .ok_or_else(|| format!("Widget template references unknown Widget: {widget_id}"))?;
        for size in templates.keys() {
            if !widget.sizes.contains(size)
                || widget_template_path(manifest, widget_id, size).is_none()
            {
                return Err(format!(
                    "Widget template content has no matching Manifest path: {widget_id}/{size}"
                ));
            }
        }
    }
    Ok(())
}

fn validate_installed_resources(manifest: &TappManifest, tapp_dir: &FsPath) -> Result<(), String> {
    let mut resources = vec![manifest.main.as_str()];
    resources.extend(
        [
            manifest.styles.as_deref(),
            manifest.widget_styles.as_deref(),
            manifest.page_styles.as_deref(),
            manifest.page_template.as_deref(),
        ]
        .into_iter()
        .flatten(),
    );
    if let Some(widgets) = &manifest.widgets {
        for widget in widgets {
            if let Some(templates) = &widget.templates {
                resources.extend(templates.values().map(String::as_str));
            }
        }
    }
    for relative in resources {
        let path = regular_resource_path(tapp_dir, relative)
            .ok_or_else(|| format!("Declared Tapp resource is not a regular file: {relative}"))?;
        let bytes = std::fs::read(path)
            .map_err(|_| format!("Declared Tapp resource not found: {relative}"))?;
        std::str::from_utf8(&bytes)
            .map_err(|_| format!("Declared Tapp resource is not UTF-8 text: {relative}"))?;
    }

    if let Some(modules) = &manifest.page_modules {
        for module in modules {
            let relative = format!("page/{module}");
            let path = regular_resource_path(tapp_dir, &relative).ok_or_else(|| {
                format!("Declared Tapp resource is not a regular file: {relative}")
            })?;
            let bytes = std::fs::read(path)
                .map_err(|_| format!("Declared Tapp resource not found: {relative}"))?;
            std::str::from_utf8(&bytes)
                .map_err(|_| format!("Declared Tapp resource is not UTF-8 text: {relative}"))?;
        }
    }
    if let Some(agent) = &manifest.agent {
        for interaction in &agent.interactions {
            for relative in [
                interaction.input_schema.as_deref(),
                interaction.result_schema.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                let path = regular_resource_path(tapp_dir, relative).ok_or_else(|| {
                    format!("Declared Agent schema is not a regular file: {relative}")
                })?;
                let bytes = std::fs::read(path)
                    .map_err(|_| format!("Declared Agent schema not found: {relative}"))?;
                if bytes.len() > MAX_AGENT_SCHEMA_RESOURCE_BYTES {
                    return Err(format!(
                        "Agent schema exceeds {MAX_AGENT_SCHEMA_RESOURCE_BYTES} bytes: {relative}"
                    ));
                }
                let schema = serde_json::from_slice::<serde_json::Value>(&bytes)
                    .map_err(|_| format!("Agent schema is not valid JSON: {relative}"))?;
                validate_inline_data_schema(&schema)
                    .map_err(|error| format!("Invalid Agent schema {relative}: {error}"))?;
            }
        }
    }
    if let Some(assets) = &manifest.assets {
        let mut total: u64 = 0;
        for relative in assets {
            validate_asset_path(relative)?;
            let path = regular_resource_path(tapp_dir, relative)
                .ok_or_else(|| format!("Declared Tapp asset is not a regular file: {relative}"))?;
            let bytes = std::fs::read(&path)
                .map_err(|_| format!("Declared Tapp asset not found: {relative}"))?;
            let size = bytes.len() as u64;
            if size > MAX_TAPP_ASSET_BYTES {
                return Err(format!(
                    "Tapp asset exceeds {MAX_TAPP_ASSET_BYTES} bytes: {relative}"
                ));
            }
            total = total
                .checked_add(size)
                .ok_or_else(|| "Tapp assets total size overflow".to_string())?;
            if total > MAX_TAPP_ASSETS_TOTAL_BYTES {
                return Err(format!(
                    "Tapp assets total size exceeds {MAX_TAPP_ASSETS_TOTAL_BYTES} bytes"
                ));
            }
        }
    }
    validate_installed_i18n_resources(tapp_dir)?;
    Ok(())
}

fn validate_installed_i18n_resources(tapp_dir: &FsPath) -> Result<(), String> {
    let joined = tapp_dir.join("i18n");
    let metadata = match std::fs::symlink_metadata(&joined) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("Failed to inspect Tapp i18n directory".to_string()),
    };
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err("Tapp i18n must be an in-sandbox directory".to_string());
    }
    let directory = regular_resource_directory(tapp_dir, "i18n")
        .ok_or_else(|| "Tapp i18n must be an in-sandbox directory".to_string())?;
    let entries = std::fs::read_dir(directory)
        .map_err(|_| "Failed to read Tapp i18n directory".to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Failed to read Tapp i18n directory".to_string())?;
    if entries.len() > MAX_TAPP_I18N_FILES {
        return Err(format!(
            "Tapp i18n accepts at most {MAX_TAPP_I18N_FILES} locale files"
        ));
    }
    for entry in entries {
        let file_type = entry
            .file_type()
            .map_err(|_| "Failed to inspect Tapp i18n resource".to_string())?;
        let filename = entry
            .file_name()
            .into_string()
            .map_err(|_| "Tapp i18n filename must be UTF-8".to_string())?;
        let Some(locale) = filename.strip_suffix(".json") else {
            return Err(format!(
                "Tapp i18n resource must be a JSON file: {filename}"
            ));
        };
        if !file_type.is_file()
            || file_type.is_symlink()
            || !is_safe_path_component(&filename)
            || !is_safe_path_component(locale)
        {
            return Err(format!("Invalid Tapp i18n resource: {filename}"));
        }
        let relative = format!("i18n/{filename}");
        let path = regular_resource_path(tapp_dir, &relative)
            .ok_or_else(|| format!("Invalid Tapp i18n resource: {filename}"))?;
        let bytes = std::fs::read(path)
            .map_err(|_| format!("Failed to read Tapp i18n resource: {filename}"))?;
        if bytes.len() > MAX_TAPP_I18N_RESOURCE_BYTES {
            return Err(format!(
                "Tapp i18n resource exceeds {MAX_TAPP_I18N_RESOURCE_BYTES} bytes: {filename}"
            ));
        }
        let value = serde_json::from_slice::<serde_json::Value>(&bytes)
            .map_err(|_| format!("Tapp i18n resource is not valid JSON: {filename}"))?;
        if !value.is_object() {
            return Err(format!(
                "Tapp i18n locale must contain a JSON object: {filename}"
            ));
        }
    }
    Ok(())
}

fn archive_entry_path(tapp_dir: &FsPath, entry_name: &str) -> Result<PathBuf, String> {
    let relative = entry_name.trim_end_matches('/');
    validate_resource_path(relative)?;
    Ok(tapp_dir.join(relative))
}

fn validate_tapp_archive<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<(), String> {
    if archive.len() > MAX_TAPP_ARCHIVE_FILES {
        return Err(format!(
            "Tapp archive contains too many entries (max {MAX_TAPP_ARCHIVE_FILES})"
        ));
    }

    let mut total_size = 0_u64;
    let mut paths = std::collections::HashSet::new();
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|error| format!("Invalid Tapp archive entry: {error}"))?;
        let name = file.name().trim_end_matches('/');
        validate_resource_path(name)?;
        if !paths.insert(name.to_string()) {
            return Err(format!("Duplicate Tapp archive entry: {name}"));
        }
        if file.is_dir() {
            continue;
        }
        if file.size() > MAX_TAPP_RESOURCE_BYTES {
            return Err(format!(
                "Tapp archive entry is too large: {name} (max {MAX_TAPP_RESOURCE_BYTES} bytes)"
            ));
        }
        total_size = total_size
            .checked_add(file.size())
            .ok_or_else(|| "Tapp archive size overflow".to_string())?;
        if total_size > MAX_TAPP_ARCHIVE_UNCOMPRESSED_BYTES {
            return Err(format!(
                "Tapp archive expands beyond {MAX_TAPP_ARCHIVE_UNCOMPRESSED_BYTES} bytes"
            ));
        }
    }

    Ok(())
}

fn append_directory_to_zip<W: std::io::Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    root: &FsPath,
    directory: &FsPath,
    options: zip::write::SimpleFileOptions,
) -> Result<(), std::io::Error> {
    use std::io::{Read, Write};

    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        // Installed resources are regular files. Never follow a manually inserted
        // symlink while exporting, because it may point outside the Tapp directory.
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            append_directory_to_zip(zip, root, &path, options)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }

        let relative = path.strip_prefix(root).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Tapp export path escaped root",
            )
        })?;
        if relative == FsPath::new(TAPP_INSTALL_STATE_FILE) {
            continue;
        }
        let filename = relative.to_string_lossy().replace('\\', "/");
        let mut file = std::fs::File::open(&path)?;
        let mut content = Vec::new();
        file.read_to_end(&mut content)?;
        zip.start_file(filename, options)?;
        zip.write_all(&content)?;
    }

    Ok(())
}

/// Tapp 清单
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TappCategory {
    #[serde(rename = "ai")]
    Ai,
    #[serde(
        rename = "data",
        alias = "data-extension",
        alias = "platform",
        alias = "visualization"
    )]
    Data,
    #[serde(rename = "developer", alias = "development", alias = "dev")]
    Developer,
    #[serde(rename = "game", alias = "games")]
    Game,
    #[serde(rename = "media", alias = "entertainment", alias = "music")]
    Media,
    #[serde(rename = "productivity")]
    Productivity,
    #[serde(rename = "social", alias = "communication")]
    Social,
    #[serde(
        rename = "utility",
        alias = "demo",
        alias = "page",
        alias = "test",
        alias = "tool",
        alias = "tools",
        alias = "utilities",
        alias = "widget"
    )]
    Utility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<TappAuthor>,
    pub main: String,
    pub styles: Option<String>,
    pub widget_styles: Option<String>,
    pub page_styles: Option<String>,
    pub page_template: Option<String>,
    pub css_mode: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    pub icon: Option<String>,
    /// 内联 SVG 图标代码（优先于 icon）
    pub icon_svg: Option<String>,
    pub theme_color: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    /// Minimum compatible Myriad release. Installation and update fail closed
    /// when the running backend package version is older.
    pub min_system_version: Option<String>,
    pub widgets: Option<Vec<TappWidgetDef>>,
    #[serde(default)]
    pub has_page: bool,
    /// 声明式后台需求，用于在没有可见 Page / Widget 时拉起 headless core。
    /// 必须显式保留，否则 manifest 经后端反序列化再写盘时会静默丢字段。
    #[serde(default)]
    pub background_requirements: Option<Vec<String>>,
    pub settings: Option<Vec<TappSettingDef>>,
    /// 应用用途分类。旧安装可继续读取缺失字段；新安装和更新必须显式提供。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<TappCategory>,
    /// Page 模块加载顺序（文件名数组）
    /// 当使用 page/ 文件夹模块化开发时，指定加载顺序
    #[serde(default)]
    pub page_modules: Option<Vec<String>>,
    /// Tapp API 声明
    /// 允许 Tapp 声明可调用的外部 API，后端自动注入上下文和密钥
    #[serde(default)]
    pub apis: Option<std::collections::HashMap<String, TappApiDef>>,
    /// 显式声明的跨 Tapp 数据导入/导出契约。声明本身不授予访问权；
    /// 每次调用仍必须经过宿主的一次性授权流程。
    #[serde(default)]
    pub data_exchange: Option<TappDataExchangeManifest>,
    /// Server-governed AI Task declaration. Provider/model parameters are
    /// deliberately absent: the host resolves those from its own policy.
    #[serde(default)]
    pub ai: Option<TappAiManifest>,
    /// Declared Event Broker topics. Subscription state is derived from this
    /// manifest and the online runtime registry, never persisted separately.
    #[serde(default)]
    pub events: Option<TappEventsManifest>,
    /// Stateful Agent Interaction declaration.
    #[serde(default)]
    pub agent: Option<TappAgentManifest>,
    /// Package-static binary/text assets under `assets/…`, loaded via the
    /// host assets API (not Tapp.storage). Paths are relative to the install root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum TappAiOperation {
    Generate,
    Analyze,
    Chat,
    Image,
}

impl TappAiOperation {
    pub fn permission(self) -> &'static str {
        match self {
            Self::Generate => "ai:generate",
            Self::Analyze => "ai:analyze",
            Self::Chat => "ai:chat",
            Self::Image => "ai:image",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TappAiModelTier {
    Standard,
    Pro,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum TappAiContextSource {
    Platform,
    Report,
    Profile,
    Custom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum TappAiOutputFormat {
    Text,
    Json,
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappAiManifest {
    pub protocol_version: u8,
    pub operations: Vec<TappAiOperation>,
    pub model_tier: TappAiModelTier,
    #[serde(default)]
    pub context_sources: Vec<TappAiContextSource>,
    pub output_formats: Vec<TappAiOutputFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappEventsManifest {
    #[serde(default)]
    pub publish: Vec<String>,
    #[serde(default)]
    pub subscribe: Vec<String>,
}

fn valid_event_topic(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappAgentManifest {
    pub protocol_version: u8,
    pub interactions: Vec<TappAgentInteractionDef>,
    #[serde(default)]
    pub intents: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappAgentInteractionDef {
    #[serde(rename = "type")]
    pub interaction_type: String,
    #[serde(default)]
    pub input_schema: Option<String>,
    #[serde(default)]
    pub result_schema: Option<String>,
}

fn valid_agent_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappDataExchangeManifest {
    #[serde(default)]
    pub exports: Vec<TappDataExport>,
    #[serde(default)]
    pub imports: Vec<TappDataImport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappDataExport {
    pub id: String,
    /// 受支持的内联 JSON Schema 子集；不允许远程或文件 `$ref`。
    pub schema: serde_json::Value,
    pub max_bytes: usize,
    #[serde(default)]
    pub max_records: Option<usize>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappDataImport {
    pub tapp_id: String,
    pub export_id: String,
}

/// Tapp API 访问级别
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum TappApiAccess {
    /// 公开 API：调用者范围含游客；HTTP 仍需 `network:fetch`
    Public,
    /// 受保护 API：调用者需登录（默认）；HTTP 仍需 `network:fetch`
    #[default]
    Protected,
}

/// Tapp API 定义
///
/// 支持两种类型：
/// 1. HTTP API：调用外部 HTTP 服务
/// 2. 内置 API：调用后端内置功能（如 geo、ai 等）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappApiDef {
    /// 访问级别: public（所有人）或 protected（需要权限）
    #[serde(default)]
    pub access: TappApiAccess,

    /// API 类型: http | builtin
    /// - http: 调用外部 HTTP API
    /// - builtin: 调用后端内置功能
    #[serde(rename = "type", default = "default_api_type")]
    pub api_type: String,

    /// HTTP API 的端点 URL（支持模板变量和查询串）
    pub endpoint: Option<String>,

    /// HTTP 方法 (GET, POST, etc.)，默认 GET
    #[serde(default = "default_http_method")]
    pub method: String,

    /// 请求头（支持模板变量）
    pub headers: Option<std::collections::HashMap<String, String>>,

    /// 请求体模板（支持模板变量）
    pub body: Option<serde_json::Value>,

    /// 内置 API 名称（当 type = builtin 时使用）
    /// 可选值: geo, ai:chat, ai:generate 等
    pub builtin: Option<String>,

    /// 上下文注入配置
    /// 后端自动注入的变量，前端无需提供
    /// 可用变量:
    /// - {{geo.lat}}, {{geo.lon}}, {{geo.city}} - 地理位置
    /// - {{user.id}}, {{user.username}} - 用户信息
    /// - {{secrets.KEY_NAME}} - 后端配置的密钥
    pub inject: Option<std::collections::HashMap<String, String>>,

    /// 响应缓存时间（秒），0 表示不缓存
    #[serde(default)]
    pub cache_ttl: u32,

    /// 区域伪装配置
    /// 用于绕过地区限制，自动添加伪装请求头
    /// 可选值:
    /// - "china": 伪装为中国大陆 IP（适用于网易云、B站等）
    /// - "japan": 伪装为日本 IP
    /// - "us": 伪装为美国 IP
    /// - 自定义区域代码
    pub spoof: Option<String>,

    /// API 描述
    pub description: Option<String>,
}

fn default_api_type() -> String {
    "http".to_string()
}

fn default_http_method() -> String {
    "GET".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TappAuthor {
    pub name: String,
    pub email: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappWidgetDef {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub default_size: String,
    pub sizes: Vec<String>,
    pub category: Option<TappWidgetCategory>,
    pub templates: Option<std::collections::HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<TappSettingDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_policy: Option<TappWidgetRefreshPolicy>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TappWidgetCategory {
    #[serde(rename = "stats")]
    Stats,
    #[serde(rename = "activity")]
    Activity,
    #[serde(rename = "visualization")]
    Visualization,
    #[serde(rename = "utility", alias = "tool")]
    Utility,
    #[serde(rename = "custom")]
    Custom,
}

impl TappWidgetCategory {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stats => "stats",
            Self::Activity => "activity",
            Self::Visualization => "visualization",
            Self::Utility => "utility",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TappWidgetRefreshMode {
    Event,
    Interval,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappWidgetRefreshPolicy {
    pub mode: TappWidgetRefreshMode,
    #[serde(default)]
    pub interval_seconds: Option<u32>,
    #[serde(default = "default_true")]
    pub refresh_on_visible: bool,
}

fn default_true() -> bool {
    true
}

/// Tapp 设置项定义
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappSettingDef {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub setting_type: String, // toggle | select | input | number | color
    pub description: Option<String>,
    pub default_value: Option<serde_json::Value>,
    pub options: Option<Vec<TappSettingOption>>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub placeholder: Option<String>,
}

/// Tapp 设置选项（用于 select 类型）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TappSettingOption {
    pub value: String,
    pub label: String,
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

/// 获取 Tapp 列表
///
/// 权限模型：
/// - 游客：只能看到管理员的 Tapp 列表（只读）
/// - 普通用户：看到管理员的 Tapp + 自己临时安装的 Tapp
/// - 管理员：看到自己的 Tapp（可管理）
async fn list_tapps(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<Vec<TappListItem>>>, StatusCode> {
    // 可选认证：游客也可以访问
    let claims = extract_optional_claims(&headers);
    let user_id = optional_authenticated_user_id(claims.as_ref());
    // 获取管理员用户 ID
    let admin_id = find_admin_user_id(&db).await?;

    let mut items: Vec<TappListItem> = Vec::new();
    let mut seen_tapp_ids = std::collections::HashSet::new();

    // Prefer the subject's private install when both private and public copies exist.
    if let Some(uid) = user_id {
        if Some(uid) != admin_id {
            let user_tapps = tapps::Entity::find()
                .filter(tapps::Column::UserId.eq(uid))
                .all(&db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            for t in user_tapps {
                seen_tapp_ids.insert(t.tapp_id.clone());
                let icon_svg = t
                    .manifest
                    .get("iconSvg")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                items.push(TappListItem {
                    id: t.tapp_id,
                    name: t.name,
                    version: t.version,
                    description: t.description,
                    icon: t.icon,
                    icon_svg,
                    status: format!("{:?}", t.status).to_lowercase(),
                    installed_at: t.installed_at.to_rfc3339(),
                    last_run_at: t.last_run_at.map(|dt| dt.to_rfc3339()),
                    // Every installation outside the one site-owner namespace
                    // follows the per-user temporary lifecycle, even when the
                    // current account also has an administrator role.
                    is_temporary: true,
                    is_admin_tapp: false,
                });
            }
        }
    }

    // Site-owner public installs are visible to everyone; skip IDs already covered
    // by the subject's private copy.
    let admin_tapps = if let Some(admin_id) = admin_id {
        tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(admin_id))
            .all(&db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        Vec::new()
    };

    for t in admin_tapps {
        if !seen_tapp_ids.insert(t.tapp_id.clone()) {
            continue;
        }
        let icon_svg = t
            .manifest
            .get("iconSvg")
            .and_then(|v| v.as_str())
            .map(String::from);
        items.push(TappListItem {
            id: t.tapp_id,
            name: t.name,
            version: t.version,
            description: t.description,
            icon: t.icon,
            icon_svg,
            status: format!("{:?}", t.status).to_lowercase(),
            installed_at: t.installed_at.to_rfc3339(),
            last_run_at: t.last_run_at.map(|dt| dt.to_rfc3339()),
            is_temporary: false,
            is_admin_tapp: true,
        });
    }

    Ok(Json(ApiResponse::success(items)))
}

/// 批量获取当前会话可见的全部 Tapp 详情。
///
/// 与 `GET /api/tapps/{tapp_id}` 保持相同的可见性和权限过滤规则，但固定只执行
/// 管理员 Tapp 与当前用户 Tapp 两次查询，避免前端列表同步产生 N+1 请求。
async fn list_tapp_details(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<Vec<TappDetail>>>, StatusCode> {
    let claims = extract_optional_claims(&headers);
    let parsed_user_id = optional_authenticated_user_id(claims.as_ref());
    let role = match claims.as_ref() {
        Some(claims) if current_is_admin(claims).await => UserRole::Admin,
        _ if parsed_user_id.is_some() => UserRole::User,
        _ => UserRole::Guest,
    };
    let admin_id = find_admin_user_id(&db).await?;

    let admin_tapps = if let Some(admin_id) = admin_id {
        tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(admin_id))
            .all(&db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        Vec::new()
    };

    let user_tapps = if let Some(user_id) = parsed_user_id {
        if Some(user_id) != admin_id {
            tapps::Entity::find()
                .filter(tapps::Column::UserId.eq(user_id))
                .all(&db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    // Prefer private install when subject has both; guests only see public installs.
    let mut seen = std::collections::HashSet::new();
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let mut details = Vec::with_capacity(admin_tapps.len() + user_tapps.len());
    for tapp in user_tapps {
        seen.insert(tapp.tapp_id.clone());
        details.push(tapp_detail_from_model(tapp, role, true, false, &config));
    }
    for tapp in admin_tapps {
        if seen.insert(tapp.tapp_id.clone()) {
            details.push(tapp_detail_from_model(tapp, role, false, true, &config));
        }
    }

    Ok(Json(ApiResponse::success(details)))
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
    let existing = existing_query.one(&db).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error("Database error"),
        )
    })?;

    if existing.is_some() {
        return Err((StatusCode::CONFLICT, api_error("Tapp already installed")));
    }

    // 所有资源先写入同文件系统的 staging 目录；校验通过后再原子切换。
    let final_tapp_dir = tapp_dir_for(installation_owner_id, &manifest.id)
        .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
    let stage = TappDirStage::create(&final_tapp_dir).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error("Failed to create staging directory"),
        )
    })?;
    let tapp_dir = stage.path();

    // 保存到 Manifest 声明的入口；安装/导出往返后路径保持一致。
    write_tapp_resource(tapp_dir, &manifest.main, &code)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error("Failed to save code"),
            )
        })?;

    // 保存可选资源
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

    // 🎯 保存分离式 CSS（从商店下载的）
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

    // Direct unified-mode installs may include frontend-compiled Tailwind CSS.
    // It belongs to this staged generation and must never overwrite resources
    // declared by a separated-mode/store package.
    if req.source == "direct" && manifest.css_mode.as_deref() != Some("separated") {
        if let Some(widget_css) = &req.widget_css {
            write_tapp_resource(tapp_dir, "widget.css", widget_css)
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        api_error("Failed to save generated widget CSS"),
                    )
                })?;
        }
        if let Some(page_css) = &req.page_css {
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
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        api_error("Failed to save i18n resource"),
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
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        api_error("Failed to save page module"),
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

    let txn = db.begin().await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error("Failed to begin install transaction"),
        )
    })?;
    lock_tapp_lifecycle(&txn, &manifest.id).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error("Failed to lock Tapp lifecycle"),
        )
    })?;
    let conflict_query = tapps::Entity::find()
        .filter(tapps::Column::TappId.eq(&manifest.id))
        .filter(tapps::Column::UserId.is_in(conflict_owner_ids));
    if conflict_query
        .one(&txn)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error("Database error"),
            )
        })?
        .is_some()
    {
        txn.rollback().await.ok();
        return Err((StatusCode::CONFLICT, api_error("Tapp already installed")));
    }

    let activated = match stage.activate(&final_tapp_dir).await {
        Ok(activated) => activated,
        Err(_) => {
            txn.rollback().await.ok();
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error("Failed to activate staged Tapp"),
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
        return Err((status, api_error("Failed to register manifest Widgets")));
    }
    if txn.commit().await.is_err() {
        activated.rollback().await;
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error("Failed to commit Tapp installation"),
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
    let existing = existing_query.one(&db).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error("Database error"),
        )
    })?;

    if existing.is_some() {
        return Err((StatusCode::CONFLICT, api_error("Tapp already installed")));
    }

    let final_tapp_dir = tapp_dir_for(installation_owner_id, &manifest.id)
        .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
    let stage = TappDirStage::create(&final_tapp_dir).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error("Failed to create staging directory"),
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
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error("Failed to extract files"),
        )
    })?
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error("Failed to save files"),
        )
    })?;

    let now = Utc::now().fixed_offset();
    write_install_generation(tapp_dir, now).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error("Failed to save install state"),
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
    let txn = db.begin().await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error("Failed to begin install transaction"),
        )
    })?;
    lock_tapp_lifecycle(&txn, &manifest.id).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error("Failed to lock Tapp lifecycle"),
        )
    })?;
    let conflict_query = tapps::Entity::find()
        .filter(tapps::Column::TappId.eq(&manifest.id))
        .filter(tapps::Column::UserId.is_in(conflict_owner_ids));
    if conflict_query
        .one(&txn)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error("Database error"),
            )
        })?
        .is_some()
    {
        txn.rollback().await.ok();
        return Err((StatusCode::CONFLICT, api_error("Tapp already installed")));
    }

    let activated = match stage.activate(&final_tapp_dir).await {
        Ok(activated) => activated,
        Err(_) => {
            txn.rollback().await.ok();
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error("Failed to activate staged Tapp"),
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
        return Err((status, api_error("Failed to register manifest Widgets")));
    }
    if txn.commit().await.is_err() {
        activated.rollback().await;
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error("Failed to commit Tapp installation"),
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

/// 获取 Tapp 详情
///
/// 权限模型：
/// - 游客：只能访问管理员的 Tapp（只读）
/// - 普通用户：可以访问管理员的 Tapp + 自己临时安装的 Tapp
async fn get_tapp(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<TappDetail>>, StatusCode> {
    // 可选认证：游客也可以访问
    let claims = extract_optional_claims(&headers);
    let user_id = optional_authenticated_user_id(claims.as_ref());
    let is_admin = match claims.as_ref() {
        Some(claims) => current_is_admin(claims).await,
        None => false,
    };
    let visible = find_visible_tapp(&db, user_id, &tapp_id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;

    let role = if is_admin {
        UserRole::Admin
    } else if user_id.is_some_and(|user_id| user_id >= 0) {
        UserRole::User
    } else {
        UserRole::Guest
    };
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let detail = tapp_detail_from_model(
        visible.tapp,
        role,
        !visible.is_site_owner,
        visible.is_site_owner,
        &config,
    );
    Ok(Json(ApiResponse::success(detail)))
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

/// 执行卸载 Tapp 的具体操作
///
/// 参数：
/// - keep_data: 是否保留应用数据（存储和设置），以便再次安装时恢复
async fn do_uninstall_tapp(
    db: &DatabaseConnection,
    tapp: &tapps::Model,
    keep_data: bool,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    let user_id = tapp.user_id;
    let tapp_id = &tapp.tapp_id;
    let is_public_install = find_admin_user_id(db).await? == Some(user_id);

    let txn = db
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    lock_tapp_lifecycle(&txn, tapp_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let still_installed = tapps::Entity::find_by_id(tapp.id)
        .filter(tapps::Column::UserId.eq(user_id))
        .filter(tapps::Column::TappId.eq(tapp_id))
        .one(&txn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .is_some();
    if !still_installed {
        txn.rollback().await.ok();
        return Err(StatusCode::NOT_FOUND);
    }

    crate::api::tapp_runtime::revoke_all_tapp_runtime_grants(tapp_id).await;

    // First move files out of the live path. The rename is atomic and can be
    // restored if any database cleanup fails.
    let tapp_dir = tapp_dir_for(user_id, tapp_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let quarantined_dir = if tapp_dir.exists() {
        let parent = tapp_dir.parent().ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
        let quarantine = parent.join(format!(
            ".{}.uninstall-{}",
            tapp_id,
            uuid::Uuid::new_v4().simple()
        ));
        if fs::rename(&tapp_dir, &quarantine).await.is_err() {
            txn.rollback().await.ok();
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
        Some(quarantine)
    } else {
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
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        } else {
            tapp_widgets::Entity::delete_many()
                .filter(tapp_widgets::Column::UserId.eq(user_id))
                .filter(tapp_widgets::Column::TappId.eq(tapp_id))
                .exec(&txn)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }

        if !keep_data {
            tapp_storage::Entity::delete_many()
                .filter(tapp_storage::Column::UserId.eq(user_id))
                .filter(tapp_storage::Column::TappId.eq(tapp_id))
                .exec(&txn)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        txn.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!("DELETE FROM tapp_scheduled_tasks WHERE {task_scope}"),
            task_values,
        ))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        tapps::Entity::delete_by_id(tapp.id)
            .exec(&txn)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(())
    }
    .await;

    if let Err(status) = cleanup_result {
        txn.rollback().await.ok();
        if let Some(quarantine) = quarantined_dir {
            let _ = fs::rename(quarantine, &tapp_dir).await;
        }
        return Err(status);
    }
    if txn.commit().await.is_err() {
        if let Some(quarantine) = quarantined_dir {
            let _ = fs::rename(quarantine, &tapp_dir).await;
        }
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    if let Some(quarantine) = quarantined_dir {
        if let Err(error) = fs::remove_dir_all(&quarantine).await {
            tracing::warn!(path = %quarantine.display(), %error, "Failed to remove uninstalled Tapp files");
        }
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
    let stage = TappDirStage::create(&final_tapp_dir).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            api_error("Failed to create staging directory"),
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
        Err(_) => {
            txn.rollback().await.ok();
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error("Failed to activate staged Tapp update"),
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

/// 列出用户所有已注册的小组件（跨所有 Tapp）
///
/// 权限模型：
/// - 游客：只返回公共安装的 Manifest Widget
/// - 已登录用户：再返回当前主体、当前可见安装下的动态 Widget
fn widget_source(config: &serde_json::Value) -> Option<&str> {
    config.get("source").and_then(serde_json::Value::as_str)
}

fn widget_installation_owner(config: &serde_json::Value) -> Option<i32> {
    config
        .get("installationOwnerId")
        .and_then(serde_json::Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

fn runtime_widget_belongs_to_installation(
    widget: &tapp_widgets::Model,
    subject_id: i32,
    installation_owner_id: i32,
) -> bool {
    widget_source(&widget.config) == Some("runtime")
        && (widget_installation_owner(&widget.config) == Some(installation_owner_id)
            // Compatibility for runtime Widgets created before owner binding:
            // they are safe only when subject and installation owner coincide.
            || (widget_installation_owner(&widget.config).is_none()
                && subject_id == installation_owner_id))
}

fn tapp_widget_response(widget: &tapp_widgets::Model, is_admin_widget: bool) -> serde_json::Value {
    serde_json::json!({
        "id": widget.widget_id,
        "tappId": widget.tapp_id,
        "config": {
            "id": widget.widget_id.strip_prefix(&format!("tapp.{}.", widget.tapp_id)).unwrap_or(&widget.widget_id),
            "name": widget.name,
            "description": widget.description,
            "icon": widget.icon,
            "defaultSize": widget.default_size,
            "sizes": widget.sizes,
            "category": widget.category,
            "settings": widget.config.get("settings").cloned().unwrap_or_else(|| serde_json::json!([])),
            "refreshPolicy": widget.config.get("refreshPolicy").cloned().unwrap_or(serde_json::Value::Null),
        },
        "instanceCount": 0,
        "registeredAt": widget.registered_at.to_rfc3339(),
        "isAdminWidget": is_admin_widget,
    })
}

async fn list_all_widgets(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<Vec<serde_json::Value>>>, StatusCode> {
    // 可选认证：游客也可以访问
    let claims = extract_optional_claims(&headers);
    let user_id = optional_authenticated_user_id(claims.as_ref());
    let admin_id = find_admin_user_id(&db).await?;
    let admin_tapp_ids: std::collections::HashSet<String> = if let Some(admin_id) = admin_id {
        tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(admin_id))
            .all(&db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .into_iter()
            .map(|tapp| tapp.tapp_id)
            .collect()
    } else {
        std::collections::HashSet::new()
    };
    let user_tapp_ids: std::collections::HashSet<String> = if let Some(uid) = user_id {
        if Some(uid) != admin_id {
            tapps::Entity::find()
                .filter(tapps::Column::UserId.eq(uid))
                .all(&db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .into_iter()
                .map(|tapp| tapp.tapp_id)
                .collect()
        } else {
            std::collections::HashSet::new()
        }
    } else {
        std::collections::HashSet::new()
    };

    let mut items: Vec<serde_json::Value> = Vec::new();
    let mut public_manifest_widget_ids = std::collections::HashSet::new();
    // 1. Public owner Manifest Widgets are shared. Runtime Widgets owned by
    // the site-owner subject remain private to that subject.
    let admin_widgets = if let Some(admin_id) = admin_id {
        tapp_widgets::Entity::find()
            .filter(tapp_widgets::Column::UserId.eq(admin_id))
            .all(&db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        Vec::new()
    };

    for widget in admin_widgets {
        if widget_source(&widget.config) == Some("runtime") {
            if user_id == admin_id
                && admin_id.is_some_and(|owner_id| {
                    runtime_widget_belongs_to_installation(&widget, owner_id, owner_id)
                })
            {
                items.push(tapp_widget_response(&widget, false));
            }
            continue;
        }
        if user_tapp_ids.contains(&widget.tapp_id) {
            continue;
        }
        public_manifest_widget_ids.insert(widget.widget_id.clone());
        items.push(tapp_widget_response(&widget, true));
    }

    // 2. Subject-owned Widgets are visible only when they belong to the
    // installation that currently wins resolution for this subject.
    if let Some(uid) = user_id {
        if Some(uid) != admin_id {
            let user_widgets = tapp_widgets::Entity::find()
                .filter(tapp_widgets::Column::UserId.eq(uid))
                .all(&db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            for widget in user_widgets {
                let visible = if user_tapp_ids.contains(&widget.tapp_id) {
                    widget_source(&widget.config) != Some("runtime")
                        || runtime_widget_belongs_to_installation(&widget, uid, uid)
                } else if admin_tapp_ids.contains(&widget.tapp_id) {
                    admin_id.is_some_and(|owner_id| {
                        runtime_widget_belongs_to_installation(&widget, uid, owner_id)
                            && !public_manifest_widget_ids.contains(&widget.widget_id)
                    })
                } else {
                    false
                };
                if visible {
                    items.push(tapp_widget_response(&widget, false));
                }
            }
        }
    }

    Ok(Json(ApiResponse::success(items)))
}

/// 注册小组件请求
#[derive(Debug, Deserialize)]
pub struct RegisterWidgetRequest {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub default_size: String,
    pub sizes: Vec<String>,
    pub category: Option<TappWidgetCategory>,
    #[serde(default)]
    pub settings: Vec<TappSettingDef>,
    #[serde(default)]
    pub refresh_policy: Option<TappWidgetRefreshPolicy>,
}

async fn reconcile_manifest_widgets(
    db: &impl ConnectionTrait,
    user_id: i32,
    tapp_id: &str,
    manifest: &TappManifest,
    previous_manifest: Option<&serde_json::Value>,
) -> Result<(), StatusCode> {
    let desired_widgets = manifest.widgets.as_deref().unwrap_or_default();
    let desired_ids: std::collections::HashSet<String> = desired_widgets
        .iter()
        .map(|widget| format!("tapp.{tapp_id}.{}", widget.id))
        .collect();
    let legacy_manifest_ids: std::collections::HashSet<String> = previous_manifest
        .and_then(|value| value.get("widgets"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|widget| widget.get("id").and_then(serde_json::Value::as_str))
        .map(|id| format!("tapp.{tapp_id}.{id}"))
        .collect();

    let existing = tapp_widgets::Entity::find()
        .filter(tapp_widgets::Column::UserId.eq(user_id))
        .filter(tapp_widgets::Column::TappId.eq(tapp_id))
        .all(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    for widget in existing {
        let is_manifest = widget
            .config
            .get("source")
            .and_then(serde_json::Value::as_str)
            == Some("manifest")
            || legacy_manifest_ids.contains(&widget.widget_id);
        if is_manifest && !desired_ids.contains(&widget.widget_id) {
            tapp_widgets::Entity::delete_by_id(widget.id)
                .exec(db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
    }

    for widget in desired_widgets {
        let widget_id = format!("tapp.{tapp_id}.{}", widget.id);
        // A runtime Widget belongs to one concrete installation even when its
        // subject differs from the installation owner (the public-install
        // case). Once that installation declares the same ID in its manifest,
        // remove only runtime rows bound to this owner; private same-ID rows
        // belonging to another installation must survive.
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"DELETE FROM tapp_widgets
               WHERE widget_id = $1
                 AND user_id <> $2
                 AND config->>'source' = 'runtime'
                 AND config->>'installationOwnerId' = $3"#,
            vec![
                widget_id.clone().into(),
                user_id.into(),
                user_id.to_string().into(),
            ],
        ))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let runtime_config = serde_json::json!({
            "settings": &widget.settings,
            "refreshPolicy": &widget.refresh_policy,
            "source": "manifest",
            "installationOwnerId": user_id,
        });
        let existing = tapp_widgets::Entity::find()
            .filter(tapp_widgets::Column::UserId.eq(user_id))
            .filter(tapp_widgets::Column::WidgetId.eq(&widget_id))
            .one(db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if let Some(existing) = existing {
            let mut active: tapp_widgets::ActiveModel = existing.into();
            active.name = Set(widget.name.clone());
            active.description = Set(widget.description.clone());
            active.icon = Set(widget.icon.clone());
            active.default_size = Set(widget.default_size.clone());
            active.sizes = Set(serde_json::to_value(&widget.sizes).unwrap_or_default());
            active.category = Set(widget
                .category
                .map(|category| category.as_str().to_string()));
            active.config = Set(runtime_config);
            active
                .update(db)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        } else {
            tapp_widgets::ActiveModel {
                id: NotSet,
                widget_id: Set(widget_id),
                tapp_id: Set(tapp_id.to_string()),
                user_id: Set(user_id),
                name: Set(widget.name.clone()),
                description: Set(widget.description.clone()),
                icon: Set(widget.icon.clone()),
                default_size: Set(widget.default_size.clone()),
                sizes: Set(serde_json::to_value(&widget.sizes).unwrap_or_default()),
                category: Set(widget
                    .category
                    .map(|category| category.as_str().to_string())),
                config: Set(runtime_config),
                registered_at: Set(Utc::now().fixed_offset()),
            }
            .insert(db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
    }
    Ok(())
}

/// 注册小组件
async fn register_widget(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
    Json(req): Json<RegisterWidgetRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, StatusCode> {
    // Dynamic Widget registration changes the site Dashboard capability
    // surface. A manifest permission or stale Runtime Grant must never make
    // this operation available to a non-admin subject.
    require_current_admin(&claims).await?;
    runtime_grant
        .require_tapp_id(&tapp_id)
        .and_then(|_| runtime_grant.require(TappPermission::WidgetRegister))
        .map_err(|(status, _)| status)?;
    let user_id =
        authorize_tapp_permission(&db, &claims, &tapp_id, TappPermission::WidgetRegister).await?;
    let installation_owner_id = runtime_grant.owner_id();
    if !is_safe_path_component(&req.id)
        || req.name.is_empty()
        || req.name.len() > 255
        || req.sizes.is_empty()
        || req.sizes.len() > 10
        || req.sizes.iter().any(|size| !is_valid_widget_size(size))
        || !req.sizes.contains(&req.default_size)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    validate_tapp_settings(&req.settings, &format!("Widget {}", req.id))
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    if let Some(policy) = &req.refresh_policy {
        validate_widget_refresh_policy(policy, &req.id).map_err(|_| StatusCode::BAD_REQUEST)?;
    }
    let widget_id = format!("tapp.{}.{}", tapp_id, req.id);
    let site_owner_id = find_admin_user_id(&db).await?;
    let txn = db
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // Serialize the visibility check, per-installation count and insert with
    // install/update/uninstall across all backend replicas.
    lock_tapp_lifecycle(&txn, &tapp_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let private_install_exists = if Some(user_id) != site_owner_id {
        tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(user_id))
            .filter(tapps::Column::TappId.eq(&tapp_id))
            .one(&txn)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .is_some()
    } else {
        false
    };
    let public_install_exists = if let Some(site_owner_id) = site_owner_id {
        tapps::Entity::find()
            .filter(tapps::Column::UserId.eq(site_owner_id))
            .filter(tapps::Column::TappId.eq(&tapp_id))
            .one(&txn)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .is_some()
    } else {
        false
    };
    let visible_owner_id = if private_install_exists {
        user_id
    } else if public_install_exists {
        site_owner_id.ok_or(StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        user_id
    };
    if visible_owner_id != installation_owner_id {
        txn.rollback().await.ok();
        return Err(StatusCode::FORBIDDEN);
    }

    let installed_tapp = tapps::Entity::find()
        .filter(tapps::Column::UserId.eq(installation_owner_id))
        .filter(tapps::Column::TappId.eq(&tapp_id))
        .one(&txn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::FORBIDDEN)?;
    if installed_tapp
        .manifest
        .get("widgets")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|widgets| {
            widgets.iter().any(|widget| {
                widget.get("id").and_then(serde_json::Value::as_str) == Some(req.id.as_str())
            })
        })
    {
        return Err(StatusCode::CONFLICT);
    }
    let runtime_config = serde_json::json!({
        "settings": &req.settings,
        "refreshPolicy": &req.refresh_policy,
        "source": "runtime",
        "installationOwnerId": installation_owner_id,
    });
    let now = Utc::now().fixed_offset();

    // 检查是否已存在
    let existing = tapp_widgets::Entity::find()
        .filter(tapp_widgets::Column::UserId.eq(user_id))
        .filter(tapp_widgets::Column::WidgetId.eq(&widget_id))
        .one(&txn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(item) = existing {
        if item
            .config
            .get("source")
            .and_then(serde_json::Value::as_str)
            == Some("manifest")
        {
            return Err(StatusCode::CONFLICT);
        }
        if !runtime_widget_belongs_to_installation(&item, user_id, installation_owner_id) {
            return Err(StatusCode::CONFLICT);
        }
        // 更新现有
        let mut active: tapp_widgets::ActiveModel = item.into();
        active.name = Set(req.name.clone());
        active.description = Set(req.description.clone());
        active.icon = Set(req.icon.clone());
        active.default_size = Set(req.default_size.clone());
        active.sizes = Set(serde_json::to_value(&req.sizes).unwrap());
        active.category = Set(req.category.map(|category| category.as_str().to_string()));
        active.config = Set(runtime_config.clone());
        active
            .update(&txn)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    } else {
        let manifest_widget_count = installed_tapp
            .manifest
            .get("widgets")
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len);
        let subject_widgets = tapp_widgets::Entity::find()
            .filter(tapp_widgets::Column::UserId.eq(user_id))
            .filter(tapp_widgets::Column::TappId.eq(&tapp_id))
            .all(&txn)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let runtime_widget_count = subject_widgets
            .iter()
            .filter(|widget| {
                runtime_widget_belongs_to_installation(widget, user_id, installation_owner_id)
            })
            .count();
        if manifest_widget_count + runtime_widget_count >= MAX_WIDGETS_PER_TAPP {
            return Err(StatusCode::BAD_REQUEST);
        }
        // 创建新的
        let widget = tapp_widgets::ActiveModel {
            id: NotSet,
            widget_id: Set(widget_id.clone()),
            tapp_id: Set(tapp_id.clone()),
            user_id: Set(user_id),
            name: Set(req.name.clone()),
            description: Set(req.description.clone()),
            icon: Set(req.icon.clone()),
            default_size: Set(req.default_size.clone()),
            sizes: Set(serde_json::to_value(&req.sizes).unwrap()),
            category: Set(req.category.map(|category| category.as_str().to_string())),
            config: Set(runtime_config),
            registered_at: Set(now),
        };
        widget
            .insert(&txn)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    txn.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ApiResponse::success(serde_json::json!({
        "id": widget_id,
        "tappId": tapp_id,
        "name": req.name,
    }))))
}

/// 注销小组件
async fn unregister_widget(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, widget_id)): Path<(String, String)>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    // Keep unregister on the same administrator-only control boundary as
    // registration; otherwise a normal user could remove dynamic metadata.
    require_current_admin(&claims).await?;
    runtime_grant
        .require_tapp_id(&tapp_id)
        .and_then(|_| runtime_grant.require(TappPermission::WidgetRegister))
        .map_err(|(status, _)| status)?;
    let user_id =
        authorize_tapp_permission(&db, &claims, &tapp_id, TappPermission::WidgetRegister).await?;
    let installation_owner_id = runtime_grant.owner_id();
    let full_widget_id = if widget_id.starts_with("tapp.") {
        let expected_prefix = format!("tapp.{}.", tapp_id);
        if !widget_id.starts_with(&expected_prefix) {
            return Err(StatusCode::BAD_REQUEST);
        }
        widget_id
    } else {
        format!("tapp.{}.{}", tapp_id, widget_id)
    };

    let widget = tapp_widgets::Entity::find()
        .filter(tapp_widgets::Column::UserId.eq(user_id))
        .filter(tapp_widgets::Column::WidgetId.eq(&full_widget_id))
        .one(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(widget) = &widget {
        if widget_source(&widget.config) == Some("manifest") {
            return Err(StatusCode::CONFLICT);
        }
        if !runtime_widget_belongs_to_installation(widget, user_id, installation_owner_id) {
            return Err(StatusCode::NOT_FOUND);
        }
    }
    tapp_widgets::Entity::delete_many()
        .filter(tapp_widgets::Column::UserId.eq(user_id))
        .filter(tapp_widgets::Column::WidgetId.eq(&full_widget_id))
        .exec(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ApiResponse::success(())))
}

/// 验证存储 key 格式（防止路径遍历攻击）
///
/// 规则：
/// - 只允许字母、数字、下划线、连字符、点、冒号
/// - 不允许连续的点（..）
/// - 不允许以点开头或结尾
/// - 长度限制 1-256 字符
pub(crate) fn validate_storage_key(key: &str) -> Result<(), &'static str> {
    if key.is_empty() {
        return Err("Key cannot be empty");
    }
    if key.len() > 256 {
        return Err("Key too long (max 256 characters)");
    }
    if key.starts_with('.') || key.ends_with('.') {
        return Err("Key cannot start or end with a dot");
    }
    if key.contains("..") {
        return Err("Key cannot contain consecutive dots");
    }
    // 只允许安全字符
    let valid = key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'));
    if !valid {
        return Err(
            "Key contains invalid characters (only alphanumeric, underscore, hyphen, dot, colon allowed)",
        );
    }
    Ok(())
}

const HOST_STORAGE_KEY_PREFIXES: [&str; 4] =
    ["_settings.", "_component:", "_shortcut:", "_report:"];

fn is_host_storage_key(key: &str) -> bool {
    key == "_settings"
        || HOST_STORAGE_KEY_PREFIXES
            .iter()
            .any(|prefix| key.starts_with(prefix))
}

pub(crate) fn validate_sandbox_storage_key(key: &str) -> Result<(), &'static str> {
    validate_storage_key(key)?;
    if is_host_storage_key(key) {
        return Err("Key prefix is reserved for host-managed Tapp data");
    }
    Ok(())
}

pub(crate) fn validate_storage_value_size(value: &serde_json::Value) -> Result<(), StatusCode> {
    let size = serde_json::to_vec(value)
        .map_err(|_| StatusCode::BAD_REQUEST)?
        .len();
    if size > 1024 * 1024 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    Ok(())
}

const TAPP_STORAGE_QUOTA_BYTES: i64 = 5 * 1024 * 1024;

#[derive(FromQueryResult)]
struct StorageBytesRow {
    bytes: i64,
}

async fn storage_bytes(
    db: &impl ConnectionTrait,
    user_id: i32,
    tapp_id: &str,
) -> Result<i64, StatusCode> {
    StorageBytesRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"SELECT COALESCE(SUM(octet_length(key) + octet_length(value::text)), 0)::BIGINT AS bytes
           FROM tapp_storage WHERE user_id = $1 AND tapp_id = $2"#,
        vec![user_id.into(), tapp_id.into()],
    ))
    .one(db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    .map(|row| row.map_or(0, |row| row.bytes))
}

async fn authorize_tapp_settings(
    db: &DatabaseConnection,
    claims: &Claims,
    tapp_id: &str,
) -> Result<(TappStorageAccess, Vec<TappSettingDef>), StatusCode> {
    validate_tapp_id(tapp_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let subject_id =
        optional_authenticated_user_id(Some(claims)).ok_or(StatusCode::UNAUTHORIZED)?;
    let tapp = tapp_common::resolve_accessible_tapp(db, subject_id, tapp_id)
        .await
        .map_err(|(status, _)| status)?;
    let settings = tapp
        .manifest
        .get("settings")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|setting| {
            serde_json::from_value(setting).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        })
        .collect::<Result<Vec<TappSettingDef>, StatusCode>>()?;
    let access = TappStorageAccess::from_owner_and_subject(tapp.user_id, subject_id);
    Ok((access, settings))
}

async fn authorize_tapp_setting(
    db: &DatabaseConnection,
    claims: &Claims,
    tapp_id: &str,
    key: &str,
) -> Result<(TappStorageAccess, String, TappSettingDef), StatusCode> {
    validate_storage_key(key).map_err(|_| StatusCode::BAD_REQUEST)?;
    let (access, settings) = authorize_tapp_settings(db, claims, tapp_id).await?;
    let setting = settings
        .into_iter()
        .find(|setting| setting.key == key)
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok((access, format!("_settings.{key}"), setting))
}

pub(crate) async fn read_storage_value(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    key: &str,
) -> Result<serde_json::Value, StatusCode> {
    let item = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(tapp_id))
        .filter(tapp_storage::Column::Key.eq(key))
        .one(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(item.map_or(serde_json::Value::Null, |item| item.value))
}

pub(crate) async fn write_storage_value(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    key: &str,
    value: serde_json::Value,
) -> Result<(), StatusCode> {
    let txn = db
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    txn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        vec![format!("tapp-storage:{user_id}:{tapp_id}").into()],
    ))
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    #[derive(FromQueryResult)]
    struct ProjectedBytesRow {
        bytes: i64,
    }
    let projected = ProjectedBytesRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
SELECT (
    COALESCE(SUM(octet_length(key) + octet_length(value::text))
        FILTER (WHERE key <> $3), 0)
    + octet_length($3)
    + octet_length($4::jsonb::text)
)::BIGINT AS bytes
FROM tapp_storage
WHERE user_id = $1 AND tapp_id = $2
"#,
        vec![
            user_id.into(),
            tapp_id.into(),
            key.into(),
            value.clone().into(),
        ],
    ))
    .one(&txn)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_or(i64::MAX, |row| row.bytes);
    if projected > TAPP_STORAGE_QUOTA_BYTES {
        txn.rollback().await.ok();
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    txn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
INSERT INTO tapp_storage (tapp_id, user_id, key, value, created_at, updated_at)
VALUES ($1, $2, $3, $4, NOW(), NOW())
ON CONFLICT (user_id, tapp_id, key) DO UPDATE SET
    value = EXCLUDED.value,
    updated_at = NOW()
"#,
        vec![tapp_id.into(), user_id.into(), key.into(), value.into()],
    ))
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    txn.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}

/// Host settings editor: authenticated viewers may read keys declared by the
/// accessible Tapp manifest; only the installation owner may persist values.
/// These settings are deliberately separate from subject-private sandbox storage.
async fn get_tapp_settings(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<std::collections::BTreeMap<String, serde_json::Value>>>, StatusCode> {
    let (access, settings) = authorize_tapp_settings(&db, &claims, &tapp_id).await?;
    let declared_keys: std::collections::HashSet<String> =
        settings.into_iter().map(|setting| setting.key).collect();
    let values = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(access.installation_namespace()))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.starts_with("_settings."))
        .all(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_iter()
        .filter_map(|item| {
            let key = item.key.strip_prefix("_settings.")?.to_string();
            declared_keys.contains(&key).then_some((key, item.value))
        })
        .collect();
    Ok(Json(ApiResponse::success(values)))
}

async fn get_tapp_setting(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, key)): Path<(String, String)>,
) -> Result<Json<ApiResponse<serde_json::Value>>, StatusCode> {
    let (access, storage_key, _) = authorize_tapp_setting(&db, &claims, &tapp_id, &key).await?;
    let value =
        read_storage_value(&db, access.installation_namespace(), &tapp_id, &storage_key).await?;
    Ok(Json(ApiResponse::success(value)))
}

async fn set_tapp_setting(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, key)): Path<(String, String)>,
    Json(value): Json<serde_json::Value>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    validate_storage_value_size(&value)?;
    let (access, storage_key, setting) =
        authorize_tapp_setting(&db, &claims, &tapp_id, &key).await?;
    if !can_write_installation_settings(access, current_is_admin(&claims).await) {
        return Err(StatusCode::FORBIDDEN);
    }
    if !tapp_setting_value_is_valid(&setting, &value) {
        return Err(StatusCode::BAD_REQUEST);
    }
    write_storage_value(
        &db,
        access.installation_namespace(),
        &tapp_id,
        &storage_key,
        value,
    )
    .await?;
    Ok(Json(ApiResponse::success(())))
}

/// 列出存储键
async fn list_storage_keys(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<Vec<String>>>, StatusCode> {
    let access = authorize_runtime_storage(&db, &claims, &runtime_grant, &tapp_id).await?;
    let subject_id = access.private_storage_namespace();

    let items = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(subject_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .all(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let keys: Vec<String> = items
        .into_iter()
        .filter(|item| !is_host_storage_key(&item.key))
        .map(|item| item.key)
        .collect();

    Ok(Json(ApiResponse::success(keys)))
}

/// 一次查询返回全部存储项，避免 SDK 的 keys + N 次 get 请求。
async fn list_storage_entries(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<std::collections::BTreeMap<String, serde_json::Value>>>, StatusCode> {
    let access = authorize_runtime_storage(&db, &claims, &runtime_grant, &tapp_id).await?;
    let subject_id = access.private_storage_namespace();

    let entries = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(subject_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .all(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_iter()
        .filter(|item| !is_host_storage_key(&item.key))
        .map(|item| (item.key, item.value))
        .collect();

    Ok(Json(ApiResponse::success(entries)))
}

#[derive(Debug, Serialize)]
struct TappStorageUsage {
    used: usize,
    quota: usize,
}

/// Return storage usage with one database query instead of one request per key.
async fn get_storage_usage(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<TappStorageUsage>>, StatusCode> {
    let access = authorize_runtime_storage(&db, &claims, &runtime_grant, &tapp_id).await?;
    let used = storage_bytes(&db, access.private_storage_namespace(), &tapp_id).await? as usize;

    Ok(Json(ApiResponse::success(TappStorageUsage {
        used,
        quota: TAPP_STORAGE_QUOTA_BYTES as usize,
    })))
}

/// 获取存储值
async fn get_storage(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, key)): Path<(String, String)>,
) -> Result<Json<ApiResponse<serde_json::Value>>, StatusCode> {
    // 🔒 安全校验：验证 key 格式
    if let Err(_e) = validate_sandbox_storage_key(&key) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let access = authorize_runtime_storage(&db, &claims, &runtime_grant, &tapp_id).await?;
    let value = read_storage_value(&db, access.private_storage_namespace(), &tapp_id, &key).await?;
    Ok(Json(ApiResponse::success(value)))
}

/// 设置存储值
async fn set_storage(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, key)): Path<(String, String)>,
    Json(value): Json<serde_json::Value>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    // 🔒 安全校验：验证 key 格式
    if let Err(_e) = validate_sandbox_storage_key(&key) {
        return Err(StatusCode::BAD_REQUEST);
    }

    // 🔒 安全校验：限制值大小（1MB）
    validate_storage_value_size(&value)?;

    let access = authorize_runtime_storage(&db, &claims, &runtime_grant, &tapp_id).await?;
    write_storage_value(
        &db,
        access.private_storage_namespace(),
        &tapp_id,
        &key,
        value,
    )
    .await?;

    Ok(Json(ApiResponse::success(())))
}

/// 删除存储值
async fn delete_storage(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, key)): Path<(String, String)>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    // 🔒 安全校验：验证 key 格式
    if let Err(_e) = validate_sandbox_storage_key(&key) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let access = authorize_runtime_storage(&db, &claims, &runtime_grant, &tapp_id).await?;
    let subject_id = access.private_storage_namespace();

    tapp_storage::Entity::delete_many()
        .filter(tapp_storage::Column::UserId.eq(subject_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.eq(&key))
        .exec(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ApiResponse::success(())))
}

/// 清除所有存储
async fn clear_storage(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    let access = authorize_runtime_storage(&db, &claims, &runtime_grant, &tapp_id).await?;
    let subject_id = access.private_storage_namespace();
    let clearable_ids: Vec<i32> = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(subject_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .all(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_iter()
        .filter(|item| !is_host_storage_key(&item.key))
        .map(|item| item.id)
        .collect();

    if !clearable_ids.is_empty() {
        tapp_storage::Entity::delete_many()
            .filter(tapp_storage::Column::Id.is_in(clearable_ids))
            .exec(&db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    Ok(Json(ApiResponse::success(())))
}

// ==================== 商店源管理 API ====================

/// 商店源响应
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreSourceResponse {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub url: String,
    pub enabled: bool,
    pub official: bool,
    pub icon: Option<String>,
}

/// 添加商店源请求
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddStoreSourceRequest {
    pub name: String,
    pub description: Option<String>,
    pub url: String,
    pub enabled: Option<bool>,
    pub icon: Option<String>,
}

/// 更新商店源请求
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStoreSourceRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
    pub enabled: Option<bool>,
    pub icon: Option<String>,
}

/// 获取商店源列表（公开 API）
async fn list_store_sources(
    State(db): State<DatabaseConnection>,
) -> Result<Json<ApiResponse<Vec<StoreSourceResponse>>>, StatusCode> {
    let sources = tapp_store_sources::Entity::find()
        .all(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let items: Vec<StoreSourceResponse> = sources
        .into_iter()
        .map(|s| StoreSourceResponse {
            id: s.id,
            name: s.name,
            description: s.description,
            url: s.url,
            enabled: s.enabled,
            official: s.official,
            icon: s.icon,
        })
        .collect();

    Ok(Json(ApiResponse::success(items)))
}

/// 添加商店源（仅管理员）
async fn add_store_source(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<AddStoreSourceRequest>,
) -> Result<Json<ApiResponse<StoreSourceResponse>>, StatusCode> {
    // 检查管理员权限
    require_current_admin(&claims).await?;

    // 检查 URL 是否已存在
    let existing = tapp_store_sources::Entity::find()
        .filter(tapp_store_sources::Column::Url.eq(&req.url))
        .one(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if existing.is_some() {
        return Err(StatusCode::CONFLICT);
    }

    let now = Utc::now().fixed_offset();
    let source = tapp_store_sources::ActiveModel {
        id: NotSet,
        name: Set(req.name),
        description: Set(req.description),
        url: Set(req.url),
        enabled: Set(req.enabled.unwrap_or(true)),
        official: Set(false), // 用户添加的源不是官方的
        icon: Set(req.icon),
        created_at: Set(now),
        updated_at: Set(now),
    };

    let result = source
        .insert(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ApiResponse::success(StoreSourceResponse {
        id: result.id,
        name: result.name,
        description: result.description,
        url: result.url,
        enabled: result.enabled,
        official: result.official,
        icon: result.icon,
    })))
}

/// 更新商店源（仅管理员）
async fn update_store_source(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(source_id): Path<i32>,
    Json(req): Json<UpdateStoreSourceRequest>,
) -> Result<Json<ApiResponse<StoreSourceResponse>>, StatusCode> {
    // 检查管理员权限
    require_current_admin(&claims).await?;

    // 获取现有源
    let source = tapp_store_sources::Entity::find_by_id(source_id)
        .one(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // 官方源不能修改 URL
    if source.official && req.url.is_some() {
        return Err(StatusCode::FORBIDDEN);
    }

    let now = Utc::now().fixed_offset();
    let mut active: tapp_store_sources::ActiveModel = source.into();

    if let Some(name) = req.name {
        active.name = Set(name);
    }
    if let Some(description) = req.description {
        active.description = Set(Some(description));
    }
    if let Some(url) = req.url {
        active.url = Set(url);
    }
    if let Some(enabled) = req.enabled {
        active.enabled = Set(enabled);
    }
    if let Some(icon) = req.icon {
        active.icon = Set(Some(icon));
    }
    active.updated_at = Set(now);

    let result = active
        .update(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ApiResponse::success(StoreSourceResponse {
        id: result.id,
        name: result.name,
        description: result.description,
        url: result.url,
        enabled: result.enabled,
        official: result.official,
        icon: result.icon,
    })))
}

/// 删除商店源（仅管理员）
async fn delete_store_source(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(source_id): Path<i32>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    // 检查管理员权限
    require_current_admin(&claims).await?;

    // 获取源信息
    let source = tapp_store_sources::Entity::find_by_id(source_id)
        .one(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // 不能删除官方源
    if source.official {
        return Err(StatusCode::FORBIDDEN);
    }

    tapp_store_sources::Entity::delete_by_id(source_id)
        .exec(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(ApiResponse::success(())))
}

/// 更新分离式 CSS 的请求体
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateSeparatedCssRequest {
    /// Widget 专用编译后的 Tailwind CSS
    #[serde(default)]
    widget_css: Option<String>,
    /// Page 专用编译后的 Tailwind CSS
    #[serde(default)]
    page_css: Option<String>,
}

/// Compatibility endpoint for replacing generated unified-mode CSS.
///
/// Current clients include generated CSS in direct install/update requests.
/// Older clients may still call this route, so it uses the same staged
/// filesystem generation and database transaction instead of modifying live
/// resources in place.
async fn update_separated_css(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
    Json(req): Json<UpdateSeparatedCssRequest>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    validate_tapp_id(&tapp_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    if req.widget_css.is_none() && req.page_css.is_none() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if [req.widget_css.as_deref(), req.page_css.as_deref()]
        .into_iter()
        .flatten()
        .any(|css| css.len() as u64 > MAX_TAPP_RESOURCE_BYTES)
    {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let user_id: i32 = claims.sub.parse().map_err(|_| StatusCode::UNAUTHORIZED)?;
    let role = current_user_role(&claims).await;
    let site_owner_id = get_admin_user_id(&db).await?;
    let owner_id = canonical_installation_owner_id(role, user_id, site_owner_id);

    let txn = db
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    lock_tapp_lifecycle(&txn, &tapp_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let tapp = tapps::Entity::find()
        .filter(tapps::Column::UserId.eq(owner_id))
        .filter(tapps::Column::TappId.eq(&tapp_id))
        .one(&txn)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let manifest: TappManifest = serde_json::from_value(tapp.manifest.clone())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if manifest.css_mode.as_deref() == Some("separated") {
        return Err(StatusCode::CONFLICT);
    }

    let final_tapp_dir = installed_tapp_dir(&tapp)?;
    let stage = TappDirStage::create(&final_tapp_dir)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    copy_regular_tapp_directory(&final_tapp_dir, stage.path())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(widget_css) = &req.widget_css {
        write_tapp_resource(stage.path(), "widget.css", widget_css)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    if let Some(page_css) = &req.page_css {
        write_tapp_resource(stage.path(), "page.css", page_css)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let now = Utc::now().fixed_offset();
    write_install_generation(stage.path(), now).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    validate_installed_resources(&manifest, stage.path()).map_err(|_| StatusCode::BAD_REQUEST)?;

    let activated = stage
        .activate(&final_tapp_dir)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut active: tapps::ActiveModel = tapp.into();
    active.updated_at = Set(now);
    if active.update(&txn).await.is_err() {
        txn.rollback().await.ok();
        activated.rollback().await;
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    if txn.commit().await.is_err() {
        activated.rollback().await;
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    activated.commit().await;

    Ok(Json(ApiResponse::success(())))
}

#[cfg(test)]
mod manifest_tests {
    use super::{
        append_directory_to_zip, archive_entry_path, canonical_installation_owner_id,
        copy_regular_tapp_directory, installation_conflict_owner_ids, orphaned_tapp_directories,
        recover_tapp_directory, tapp_dir_for, tapp_setting_value_is_valid, validate_asset_path,
        validate_installed_resources, validate_resource_path, validate_store_manifest_category,
        validate_tapp_archive, validate_tapp_id, validate_tapp_manifest,
        validate_widget_template_contents, widget_template_path, write_install_generation,
        RegisterWidgetRequest, TappCategory, TappDirStage, TappManifest, TappSettingDef,
        TappStorageAccess, TappWidgetCategory, TappWidgetDef, WidgetTemplateContents,
    };
    use crate::models::entities::{tapp_widgets, tapps};
    use crate::services::permission_service::UserRole;
    use serde_json::json;

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
