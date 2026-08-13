// 权限配置 API

use crate::middleware::auth::authenticate_optional_request;
use crate::services::permission_service::{TappPermissionService, UserRole};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

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
                Json(json!({"success": false, "error": "Invalid authentication state"})),
            );
        }
    };
    let config_service = crate::services::config_service::ConfigService::new(db);
    let config = match config_service.load_config().await {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": format!("Failed to load config: {}", e)
                })),
            );
        }
    };

    // 获取当前用户角色
    let role = match &claims {
        Some(c) if c.is_admin => UserRole::Admin,
        Some(c) => {
            // 检查是否为游客（负数 ID）
            if let Ok(user_id) = c.sub.parse::<i32>() {
                if user_id < 0 {
                    UserRole::Guest
                } else {
                    UserRole::User
                }
            } else {
                UserRole::Guest
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
    // 普通用户可下放的 elevated 权限（13 项）
    pub user_perm_ai_generate: Option<bool>,
    pub user_perm_ai_analyze: Option<bool>,
    pub user_perm_ai_chat: Option<bool>,
    pub user_perm_ai_image: Option<bool>,
    #[allow(dead_code)]
    pub user_perm_report_write: Option<bool>, // 忽略：强制 false
    pub user_perm_network_fetch: Option<bool>,
    pub user_perm_component_theme: Option<bool>,
    pub user_perm_shortcut_register: Option<bool>,
    pub user_perm_event_publish: Option<bool>,
    pub user_perm_scheduler_register: Option<bool>,
    pub user_perm_speech_tts: Option<bool>,
    pub user_perm_speech_asr: Option<bool>,
    pub user_perm_storage_write: Option<bool>,
    // 游客 elevated 配置；认证绑定字段仅为兼容旧请求，实际强制关闭
    pub guest_perm_ai_generate: Option<bool>,
    pub guest_perm_ai_analyze: Option<bool>,
    pub guest_perm_ai_chat: Option<bool>,
    pub guest_perm_ai_image: Option<bool>,
    #[allow(dead_code)]
    pub guest_perm_report_write: Option<bool>, // 忽略：强制 false
    pub guest_perm_network_fetch: Option<bool>,
    #[allow(dead_code)] // accepted for compatibility; update endpoint forces false
    pub guest_perm_component_theme: Option<bool>,
    #[allow(dead_code)] // accepted for compatibility; update endpoint forces false
    pub guest_perm_shortcut_register: Option<bool>,
    pub guest_perm_event_publish: Option<bool>,
    #[allow(dead_code)] // accepted for compatibility; update endpoint forces false
    pub guest_perm_scheduler_register: Option<bool>,
    #[allow(dead_code)] // accepted for compatibility; update endpoint forces false
    pub guest_perm_speech_tts: Option<bool>,
    #[allow(dead_code)] // accepted for compatibility; update endpoint forces false
    pub guest_perm_speech_asr: Option<bool>,
    pub guest_perm_storage_write: Option<bool>,
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
    fn accepts_permission_delegation_fields() {
        let payload: UpdatePermissionsPayload = serde_json::from_value(serde_json::json!({
            "user_perm_speech_tts": true,
            "user_perm_speech_asr": false,
            "guest_perm_speech_tts": false,
            "guest_perm_speech_asr": true,
            "user_perm_storage_write": true,
            "guest_perm_storage_write": false
        }))
        .unwrap();

        assert_eq!(payload.user_perm_speech_tts, Some(true));
        assert_eq!(payload.user_perm_speech_asr, Some(false));
        assert_eq!(payload.guest_perm_speech_tts, Some(false));
        assert_eq!(payload.guest_perm_speech_asr, Some(true));
        assert_eq!(payload.user_perm_storage_write, Some(true));
        assert_eq!(payload.guest_perm_storage_write, Some(false));
    }
}

pub async fn update_permissions(
    axum::extract::State(app): axum::extract::State<crate::state::AppState>,
    Json(payload): Json<UpdatePermissionsPayload>,
) -> (StatusCode, Json<Value>) {
    let Some(db) = app.db() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "Database not connected" })),
        );
    };
    let dynamic_config = app.dynamic_config.clone();

    let config_service = crate::services::config_service::ConfigService::new(db);
    let mut updates = std::collections::HashMap::new();

    // 普通用户权限（13 项 elevated）
    if let Some(v) = payload.user_perm_ai_generate {
        updates.insert("user_perm_ai_generate".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_ai_analyze {
        updates.insert("user_perm_ai_analyze".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_ai_chat {
        updates.insert("user_perm_ai_chat".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_ai_image {
        updates.insert("user_perm_ai_image".to_string(), json!(v));
    }
    // report:write 不再下放：强制写入 false
    updates.insert("user_perm_report_write".to_string(), json!(false));
    if let Some(v) = payload.user_perm_network_fetch {
        updates.insert("user_perm_network_fetch".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_component_theme {
        updates.insert("user_perm_component_theme".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_shortcut_register {
        updates.insert("user_perm_shortcut_register".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_event_publish {
        updates.insert("user_perm_event_publish".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_scheduler_register {
        updates.insert("user_perm_scheduler_register".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_speech_tts {
        updates.insert("user_perm_speech_tts".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_speech_asr {
        updates.insert("user_perm_speech_asr".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_storage_write {
        updates.insert("user_perm_storage_write".to_string(), json!(v));
    }

    // 游客权限。需要持久登录主体的能力保留兼容字段，但强制关闭。
    if let Some(v) = payload.guest_perm_ai_generate {
        updates.insert("guest_perm_ai_generate".to_string(), json!(v));
    }
    if let Some(v) = payload.guest_perm_ai_analyze {
        updates.insert("guest_perm_ai_analyze".to_string(), json!(v));
    }
    if let Some(v) = payload.guest_perm_ai_chat {
        updates.insert("guest_perm_ai_chat".to_string(), json!(v));
    }
    if let Some(v) = payload.guest_perm_ai_image {
        updates.insert("guest_perm_ai_image".to_string(), json!(v));
    }
    // report:write 不再下放：强制写入 false
    updates.insert("guest_perm_report_write".to_string(), json!(false));
    if let Some(v) = payload.guest_perm_network_fetch {
        updates.insert("guest_perm_network_fetch".to_string(), json!(v));
    }
    updates.insert("guest_perm_component_theme".to_string(), json!(false));
    updates.insert("guest_perm_shortcut_register".to_string(), json!(false));
    if let Some(v) = payload.guest_perm_event_publish {
        updates.insert("guest_perm_event_publish".to_string(), json!(v));
    }
    updates.insert("guest_perm_scheduler_register".to_string(), json!(false));
    updates.insert("guest_perm_speech_tts".to_string(), json!(false));
    updates.insert("guest_perm_speech_asr".to_string(), json!(false));
    if let Some(v) = payload.guest_perm_storage_write {
        updates.insert("guest_perm_storage_write".to_string(), json!(v));
    }

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
                "message": "No permission settings provided"
            })),
        );
    }

    if let Err(e) = config_service.update_configs(updates).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": format!("Failed to update permissions: {}", e)
            })),
        );
    }

    // 刷新全局配置缓存
    match config_service.load_config().await {
        Ok(new_config) => {
            *dynamic_config.write().await = new_config;
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
            "message": "Permission settings updated successfully"
        })),
    )
}

// PR #6: OAuth Providers + 本地注册开关 — 专用端点
// 详见 docs/development/OAUTH.md
//
// GitHub 可以作为 kind="github" 的 provider entry 配置；旧的
// github_client_id/github_client_secret 字段保留为兼容镜像。
// 这里集中处理 provider 列表 + 注册开关。

/// GET /api/config/oauth-providers
///
/// 返回 OIDC providers 列表 + 本地注册开关。
/// `client_secret` 字段在响应中被掩码（仅在数据库已设置时返回 `***`），
/// 前端不应展示明文；保存时若收到 `***` 表示用户没改，沿用旧值。
pub async fn get_oauth_providers(
    axum::extract::State(dynamic_config): axum::extract::State<
        std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>,
    >,
) -> (StatusCode, Json<Value>) {
    let config = dynamic_config.read().await;

    let mut providers: Vec<Value> = config
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

    // 自动迁移：若 legacy github_client_id 有值但 entries 里没有 slug="github"，
    // 合成一条只读 entry 展示给前端。客户端首次保存时会写到 oauth_providers。
    let has_github_entry = config.oauth_providers.iter().any(|p| p.slug == "github");
    if !has_github_entry {
        if let (Some(cid), Some(_csec)) = (
            config.github_client_id.as_ref().filter(|s| !s.is_empty()),
            config
                .github_client_secret
                .as_ref()
                .filter(|s| !s.is_empty()),
        ) {
            providers.insert(
                0,
                json!({
                    "slug": "github",
                    "kind": "github",
                    "display_name": "GitHub",
                    "enabled": true,
                    "client_id": cid,
                    "client_secret": "***",
                    "scopes": Vec::<String>::new(),
                    "discovery_url": null,
                    "icon_url": null,
                }),
            );
        }
    }

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
    /// Optional so older clients still work; omitted fields leave existing config unchanged.
    #[serde(default)]
    pub tapp_private_install_cleanup: Option<String>,
    #[serde(default)]
    pub tapp_private_install_inactivity_days: Option<i32>,
}

/// PUT /api/config/oauth-providers
///
/// 全量覆盖 providers 列表 + 注册开关。
/// 校验：
/// 1. slug 必填、URL-safe、不能重复
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
            Json(json!({ "error": "Database not connected" })),
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
                    Json(json!({"error": "Provider slug is required"})),
                );
            }
            // slug 必须 URL-safe（路由参数）：字母数字 + 连字符/下划线，2-32 字符
            if slug.len() > 32
                || !slug
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": format!("invalid slug '{}': only ASCII letters, digits, '-' and '_' allowed (max 32 chars)", slug)
                    })),
                );
            }
            p.slug = slug.to_string();
            if !seen.insert(p.slug.clone()) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": format!("duplicate provider slug: {}", p.slug)})),
                );
            }
            if p.enabled && p.client_id.trim().is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": format!("provider '{}' requires client_id (disable it if not ready)", p.slug)
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
                                "error": format!("OIDC provider '{}' requires discovery_url", p.slug)
                            })),
                        );
                    }
                }
                other => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": format!("unsupported provider kind '{}'", other),
                            "message": "kind must be 'github' or 'oidc'"
                        })),
                    );
                }
            }
            // secret 回填：前端送 "***" 表示沿用
            if p.client_secret == "***" || p.client_secret.is_empty() {
                if let Some(existing) = current.oauth_providers.iter().find(|e| e.slug == p.slug) {
                    p.client_secret = existing.client_secret.clone();
                } else if p.kind == "github" && p.slug == "github" {
                    // 从 legacy 字段拿一次作为初值
                    if let Some(legacy) = current
                        .github_client_secret
                        .as_ref()
                        .filter(|s| !s.is_empty())
                    {
                        p.client_secret = legacy.clone();
                    } else {
                        p.client_secret.clear();
                    }
                } else {
                    p.client_secret.clear();
                }
            }

            // 启用的 provider 必须有 secret（兜底检查，回填后仍为空才报错）
            if p.enabled && p.client_secret.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": format!("provider '{}' requires client_secret (disable it if not ready)", p.slug)
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
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("Failed to serialize providers: {}", e)})),
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
            updates.insert(
                "tapp_private_install_cleanup".to_string(),
                json!(mode),
            );
        }
    }
    if let Some(days) = payload.tapp_private_install_inactivity_days {
        updates.insert(
            "tapp_private_install_inactivity_days".to_string(),
            json!(days.clamp(1, 365)),
        );
    }

    // 兼容镜像：若 entries 里有 slug="github"，同时写到 legacy 平铺字段；
    // 反之则清空它们，让 registry 不会同时拿到两份冲突的凭证。
    if let Some(gh) = payload
        .providers
        .iter()
        .find(|p| p.slug == "github" && p.kind == "github")
    {
        updates.insert("github_client_id".to_string(), json!(gh.client_id));
        updates.insert("github_client_secret".to_string(), json!(gh.client_secret));
    } else {
        updates.insert("github_client_id".to_string(), json!(""));
        updates.insert("github_client_secret".to_string(), json!(""));
    }

    if let Err(e) = config_service.update_configs(updates).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("Failed to save: {}", e)})),
        );
    }

    // 刷新全局配置缓存
    match config_service.load_config().await {
        Ok(new_config) => {
            *dynamic_config.write().await = new_config;
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
