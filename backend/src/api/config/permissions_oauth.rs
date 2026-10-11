use myriad_error::AppError;
// 权限配置 API

use crate::middleware::auth::authenticate_optional_request;
use crate::services::permission_service::{TappPermissionService, UserRole};
use axum::Json;
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

/// 获取 Tapp 权限配置（公开端点）
/// 返回当前用户的权限等级和系统权限下放配置
pub async fn get_permissions(
    crate::extract::Db(db): crate::extract::Db,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let claims = match authenticate_optional_request(&headers, &db).await {
        Ok(claims) => claims,
        Err(response) => {
            return (
                response.status(),
                Json(AppError::fail_json("Invalid authentication state")),
            );
        }
    };
    let config_service = crate::services::config_service::ConfigService::new(db);
    let config = match config_service.load_config().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to load config: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": "Failed to load config",
                    "code": "config_save_failed"
                })),
            );
        }
    };

    // 获取当前用户角色
    let role = match &claims {
        Some(c) if c.is_admin => UserRole::Admin,
        Some(c) => {
            // 检查是否为游客（负数 ID）
            match c.subject_id() {
                Some(user_id) if user_id < 0 => UserRole::Guest,
                Some(user_id) if user_id > 0 => UserRole::User,
                _ => UserRole::Guest,
            }
        }
        None => UserRole::Guest,
    };

    // 获取用户可用的权限等级
    let allowed_levels: Vec<String> = TappPermissionService::get_allowed_levels(&config, role)
        .iter()
        .map(|l| format!("{:?}", l).to_lowercase())
        .collect();

    // 获取权限下放配置
    let perm_config = TappPermissionService::get_permission_config(&config);

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "role": role.as_str(),
            "allowed_levels": allowed_levels,
            "config": perm_config
        })),
    )
}

/// 更新 Tapp 权限下放配置（仅管理员）
#[derive(Debug, Deserialize)]
pub struct UpdatePermissionsPayload {
    /// Delegation flags by configuration key. Only keys in
    /// `permission_service::DELEGATIONS` are stored; a guest flag for a
    /// capability that needs a signed-in subject has no key and is ignored.
    #[serde(flatten)]
    pub delegations: std::collections::HashMap<String, serde_json::Value>,
    // AI 使用限额配置
    pub user_ai_daily_calls: Option<i32>,
    pub user_ai_daily_tokens: Option<i32>,
    pub user_ai_cooldown_seconds: Option<i32>,
    pub guest_ai_daily_calls: Option<i32>,
    pub guest_ai_daily_tokens: Option<i32>,
    pub guest_ai_cooldown_seconds: Option<i32>,
}

#[cfg(test)]
mod tapp_permission_payload_tests {
    use super::UpdatePermissionsPayload;

    #[test]
    fn permission_role_does_not_treat_subject_zero_as_user() {
        let src = include_str!("permissions_oauth.rs");
        let body = src
            .split("pub async fn get_permissions")
            .nth(1)
            .and_then(|rest| rest.split("#[cfg(test)]").next())
            .expect("get_permissions");
        assert!(body.contains("user_id > 0"));
        assert!(body.contains("UserRole::Guest"));
    }

    #[test]
    fn accepts_permission_delegation_fields() {
        // Extra keys (report:write / guest federation / guest phantasi comment) are
        // ignored: those grants are forced closed on save, not taken from the body.
        let payload: UpdatePermissionsPayload = serde_json::from_value(serde_json::json!({
            "user_perm_speech_tts": true,
            "user_perm_speech_asr": false,
            "user_perm_storage_write": true,
            "guest_perm_storage_write": false,
            "user_perm_federation_post": true,
            "user_perm_federation_channel": false,
            "user_perm_federation_room": true,
            "user_perm_phantasi_comment_write": true,
            "user_perm_report_write": true,
            "guest_perm_federation_post": true,
            "guest_perm_federation_channel": true,
            "guest_perm_federation_room": false,
            "guest_perm_phantasi_comment_write": true
        }))
        .unwrap();

        let updates = super::delegation_updates(&payload.delegations);
        for (key, value) in [
            ("user_perm_speech_tts", true),
            ("user_perm_speech_asr", false),
            ("user_perm_storage_write", true),
            ("guest_perm_storage_write", false),
            ("user_perm_federation_post", true),
            ("user_perm_federation_channel", false),
            ("user_perm_federation_room", true),
            ("user_perm_phantasi_comment_write", true),
        ] {
            assert_eq!(updates.get(key), Some(&serde_json::json!(value)), "{key}");
        }
        for ignored in [
            "user_perm_report_write",
            "guest_perm_federation_post",
            "guest_perm_federation_channel",
            "guest_perm_federation_room",
            "guest_perm_phantasi_comment_write",
        ] {
            assert!(
                !updates.contains_key(ignored),
                "{ignored} must not be stored"
            );
        }
    }

    #[test]
    fn empty_permission_payload_does_not_write_forced_false() {
        let src = include_str!("permissions_oauth.rs");
        let update = src
            .split("pub async fn update_permissions")
            .nth(1)
            .and_then(|rest| rest.split("if updates.is_empty()").next())
            .expect("update_permissions");
        assert!(!update.contains("json!(false)"));
        assert!(src.contains("No permission settings provided"));
    }
}

/// 下放开关只从下放表取键：表外的键（含对游客永不开放的能力）一律不写。
fn delegation_updates(
    body: &std::collections::HashMap<String, Value>,
) -> std::collections::HashMap<String, Value> {
    let mut updates = std::collections::HashMap::new();
    for row in crate::services::permission_service::DELEGATIONS {
        for key in std::iter::once(row.user_key).chain(row.guest_key) {
            if let Some(Value::Bool(value)) = body.get(key) {
                updates.insert(key.to_string(), json!(value));
            }
        }
    }
    updates
}

pub async fn update_permissions(
    axum::extract::State(app): axum::extract::State<crate::state::AppState>,
    Json(payload): Json<UpdatePermissionsPayload>,
) -> (StatusCode, Json<Value>) {
    let Some(db) = app.db() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json("Database not connected")),
        );
    };
    let dynamic_config = app.dynamic_config.clone();

    let config_service = crate::services::config_service::ConfigService::new(db);
    let mut updates = std::collections::HashMap::new();

    updates.extend(delegation_updates(&payload.delegations));

    // AI 使用限额配置
    if let Some(v) = payload.user_ai_daily_calls {
        updates.insert("user_ai_daily_calls".to_string(), json!(v));
    }
    if let Some(v) = payload.user_ai_daily_tokens {
        updates.insert("user_ai_daily_tokens".to_string(), json!(v));
    }
    if let Some(v) = payload.user_ai_cooldown_seconds {
        updates.insert("user_ai_cooldown_seconds".to_string(), json!(v));
    }
    if let Some(v) = payload.guest_ai_daily_calls {
        updates.insert("guest_ai_daily_calls".to_string(), json!(v));
    }
    if let Some(v) = payload.guest_ai_daily_tokens {
        updates.insert("guest_ai_daily_tokens".to_string(), json!(v));
    }
    if let Some(v) = payload.guest_ai_cooldown_seconds {
        updates.insert("guest_ai_cooldown_seconds".to_string(), json!(v));
    }

    if updates.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "message": "No permission settings provided",
                "code": "no_permission_settings",
            })),
        );
    }

    if let Err(e) = config_service.update_configs(updates).await {
        tracing::error!("Failed to update permissions: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "error": "Failed to update permissions",
                "code": "config_save_failed",
                "message": "Failed to update permissions"
            })),
        );
    }

    // 刷新全局配置缓存
    match config_service.load_config().await {
        Ok(new_config) => {
            crate::services::config_service::install_dynamic_config(&dynamic_config, new_config)
                .await;
            tracing::info!("✅ Global dynamic config refreshed after permission update");
        }
        Err(e) => {
            tracing::warn!("⚠️ Failed to refresh global config: {}", e);
        }
    }

    tracing::info!("✅ Tapp permission delegation settings updated by admin");

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": "ok"
        })),
    )
}

// OAuth Providers + 本地注册开关 — 专用端点
// 详见 docs/development/OAUTH.md
//
// GitHub 走 kind="github" 的 provider entry，和 OIDC 一起放在 oauth_providers。
// 这里集中处理 provider 列表 + 注册开关。

/// GET /api/config/oauth-providers
///
/// 返回 oauth_providers（含 GitHub）+ 本地注册开关 + tapp_private_install_*。
/// `client_secret` 字段在响应中被掩码（仅在数据库已设置时返回 `***`），
/// 前端不应展示明文；保存时若收到 `***` 表示用户没改，沿用旧值。
pub async fn get_oauth_providers(
    axum::extract::State(dynamic_config): axum::extract::State<
        std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>,
    >,
) -> (StatusCode, Json<Value>) {
    let config = dynamic_config.read().await;

    let providers: Vec<Value> = config
        .oauth_providers
        .iter()
        .map(|p| {
            json!({
                "slug": p.slug,
                "kind": p.kind,
                "display_name": p.display_name,
                "enabled": p.enabled,
                "client_id": p.client_id,
                "client_secret": if p.client_secret.is_empty() { "" } else { "***" },
                "scopes": p.scopes,
                "discovery_url": p.discovery_url,
                "icon_url": p.icon_url,
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(json!({
            "providers": providers,
            "allow_local_registration": config.allow_local_registration,
            "tapp_private_install_cleanup": config.tapp_private_install_cleanup,
            "tapp_private_install_inactivity_days": config.tapp_private_install_inactivity_days,
        })),
    )
}

#[derive(Debug, Deserialize)]
pub struct UpdateOAuthProvidersPayload {
    pub providers: Vec<crate::config::OAuthProviderEntry>,
    pub allow_local_registration: bool,
    /// 省略则保持现有配置。
    #[serde(default)]
    pub tapp_private_install_cleanup: Option<String>,
    #[serde(default)]
    pub tapp_private_install_inactivity_days: Option<i32>,
}

/// PUT /api/config/oauth-providers
///
/// 全量覆盖 providers 列表 + 注册开关。
/// 校验：
/// 1. slug 必填、URL-safe、不能重复、不能占用 qq/telegram 配对保留名
/// 2. kind="oidc" 时 discovery_url 必填
/// 3. client_secret 若为掩码 `***`，沿用现有 secret
///
/// 保存后触发 [`ProviderRegistry::reload`]。
pub async fn update_oauth_providers(
    axum::extract::State(app): axum::extract::State<crate::state::AppState>,
    Json(mut payload): Json<UpdateOAuthProvidersPayload>,
) -> (StatusCode, Json<Value>) {
    let Some(db) = app.db() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(AppError::public_json("Database not connected")),
        );
    };
    let dynamic_config = app.dynamic_config.clone();

    // 校验 + secret 回填
    let mut seen = std::collections::HashSet::new();
    {
        let current = dynamic_config.read().await;
        for p in payload.providers.iter_mut() {
            let slug = p.slug.trim();
            if slug.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Provider slug is required",
                        "code": "oauth_slug_required",
                    })),
                );
            }
            // slug 必须 URL-safe（路由参数）：字母数字 + 连字符/下划线，非空且 ≤32 字符
            if slug.len() > 32
                || !slug
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Invalid provider slug",
                        "code": "oauth_slug_invalid",
                    })),
                );
            }
            p.slug = slug.to_string();
            if crate::services::channel_pairing::is_pairing_provider(&p.slug) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Provider slug is reserved for channel pairing",
                        "code": "oauth_slug_reserved",
                    })),
                );
            }
            if !seen.insert(p.slug.clone()) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Duplicate provider slug",
                        "code": "oauth_slug_duplicate",
                    })),
                );
            }
            if p.enabled && p.client_id.trim().is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Provider client_id is required",
                        "code": "oauth_client_id_required",
                    })),
                );
            }
            match p.kind.as_str() {
                "github" => { /* no extra requirements */ }
                "oidc" => {
                    if p.discovery_url.as_deref().unwrap_or("").trim().is_empty() {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({
                                "error": "OIDC discovery URL is required",
                                "code": "oauth_discovery_required",
                            })),
                        );
                    }
                }
                _other => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": "Unsupported provider kind",
                            "code": "oauth_kind_unsupported",
                        })),
                    );
                }
            }
            // secret 回填：前端送 "***" 表示沿用 oauth_providers 里已有的值
            if p.client_secret == "***" || p.client_secret.is_empty() {
                if let Some(existing) = current.oauth_providers.iter().find(|e| e.slug == p.slug) {
                    p.client_secret = existing.client_secret.clone();
                } else {
                    p.client_secret.clear();
                }
            }

            // 启用的 provider 必须有 secret（兜底检查，回填后仍为空才报错）
            if p.enabled && p.client_secret.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Provider client_secret is required",
                        "code": "oauth_client_secret_required",
                    })),
                );
            }
        }
    }

    let config_service = crate::services::config_service::ConfigService::new(db);
    let mut updates = std::collections::HashMap::new();
    let providers_json = match serde_json::to_value(&payload.providers) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Failed to serialize OAuth providers: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error": "Failed to serialize providers", "code": "config_save_failed"}),
                ),
            );
        }
    };
    updates.insert("oauth_providers".to_string(), providers_json);
    updates.insert(
        "allow_local_registration".to_string(),
        json!(payload.allow_local_registration),
    );

    // Optional fields: only write when the client actually sent them so older
    // clients (or partial PUTs) do not silently reset cleanup policy.
    if let Some(raw) = payload.tapp_private_install_cleanup.as_deref() {
        let mode = raw.trim().to_ascii_lowercase();
        if mode == "logout" || mode == "inactivity" {
            updates.insert("tapp_private_install_cleanup".to_string(), json!(mode));
        }
    }
    if let Some(days) = payload.tapp_private_install_inactivity_days {
        updates.insert(
            "tapp_private_install_inactivity_days".to_string(),
            json!(days.clamp(1, 365)),
        );
    }

    if let Err(e) = config_service.update_configs(updates).await {
        tracing::error!("Failed to save OAuth providers: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to save", "code": "config_save_failed"})),
        );
    }

    // 刷新全局配置缓存
    match config_service.load_config().await {
        Ok(new_config) => {
            crate::services::config_service::install_dynamic_config(&dynamic_config, new_config)
                .await;
            tracing::info!("✅ Global dynamic config refreshed (oauth providers)");
        }
        Err(e) => {
            tracing::warn!("⚠️ Failed to refresh global config: {}", e);
        }
    }

    // 热重载 OAuth 注册中心
    crate::services::oauth::registry::REGISTRY.reload().await;

    // Report effective cleanup settings (post-update cache, or defaults).
    let (cleanup_mode, inactivity_days) = {
        let cfg = dynamic_config.read().await;
        (
            cfg.tapp_private_install_cleanup.clone(),
            cfg.tapp_private_install_inactivity_days,
        )
    };

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "providers_count": payload.providers.len(),
            "allow_local_registration": payload.allow_local_registration,
            "tapp_private_install_cleanup": cleanup_mode,
            "tapp_private_install_inactivity_days": inactivity_days,
        })),
    )
}
