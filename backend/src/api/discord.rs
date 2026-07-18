// Discord 数据平台 API — 用户 OAuth token（identify / guilds / connections）
//
// 一键授权（与登录 OAuth 分离）：
//   GET /api/platforms/discord/oauth/start     管理员发起，跳转 Discord
//   GET /api/platforms/discord/oauth/callback  写回 platform tokens，回配置页
//
// 复用 OAuth 登录里配置的 Discord Application（client_id/secret），
// 但 redirect_uri 与 scope 独立，需在 Discord Developer Portal 额外登记 callback。

use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::config::DynamicConfig;
use crate::middleware::auth::verify_current_admin_from_headers;
use crate::oauth_url_builder::SiteConfig;
use crate::services::config_service::ConfigService;
use crate::services::fetcher::PlatformFetcher;
use crate::services::oauth::state::{consume_state, insert_state, OAuthPurpose, StoredState};
use crate::GLOBAL_DYNAMIC_CONFIG;

const DISCORD_AUTHORIZE_URL: &str = "https://discord.com/api/oauth2/authorize";
const DISCORD_TOKEN_URL: &str = "https://discord.com/api/oauth2/token";
/// 数据平台专用 scope（不含 openid，与登录 scope 分离）
const DISCORD_DATA_SCOPES: &str = "identify guilds connections";
const PLATFORM_STATE_SLUG: &str = "discord-platform";

#[derive(Debug, Deserialize)]
pub struct DiscordTokenQuery {
    pub access_token: String,
}

#[derive(Debug, Serialize)]
pub struct DiscordUserResponse {
    pub user: Value,
    pub guilds: Vec<Value>,
    pub connections: Vec<Value>,
    pub guild_count: usize,
    pub connection_count: usize,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct OAuthCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

fn random_state() -> String {
    use rand::Rng;
    let mut buf = [0u8; 32];
    rand::rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

async fn platform_redirect_uri() -> String {
    let base = SiteConfig::get_base_url().await;
    format!(
        "{}/api/platforms/discord/oauth/callback",
        base.trim_end_matches('/')
    )
}

/// 从已配置的 OAuth providers 中解析 Discord Application 凭证
fn resolve_discord_oauth_app(config: &DynamicConfig) -> Result<(String, String), String> {
    for p in &config.oauth_providers {
        if !p.enabled {
            continue;
        }
        let is_discord = p.slug.eq_ignore_ascii_case("discord")
            || p.display_name.eq_ignore_ascii_case("discord")
            || p.discovery_url
                .as_deref()
                .map(|u| u.contains("discord.com"))
                .unwrap_or(false);
        if is_discord && !p.client_id.trim().is_empty() && !p.client_secret.trim().is_empty() {
            return Ok((
                p.client_id.trim().to_string(),
                p.client_secret.trim().to_string(),
            ));
        }
    }
    Err(
        "请先在「OAuth 登录」中添加并启用 Discord 应用（client_id / client_secret）。数据授权会复用同一 Application。"
            .to_string(),
    )
}

async fn reload_global_config(db: &DatabaseConnection) {
    let svc = ConfigService::new(db.clone());
    match svc.load_config().await {
        Ok(cfg) => {
            *GLOBAL_DYNAMIC_CONFIG.write().await = cfg;
        }
        Err(e) => {
            tracing::warn!(
                "Failed to reload GLOBAL_DYNAMIC_CONFIG after Discord OAuth: {}",
                e
            );
        }
    }
}

/// Attach anti-caching headers so OAuth redirects / callbacks are never stored.
fn apply_no_store_headers(response: &mut Response) {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, private"),
    );
    response.headers_mut().insert(
        header::PRAGMA,
        HeaderValue::from_static("no-cache"),
    );
}

fn no_store_redirect(url: &str) -> Response {
    let mut response = Redirect::to(url).into_response();
    apply_no_store_headers(&mut response);
    response
}

/// Redirect back to config platforms section. `reason` is a fixed token we control.
fn config_redirect(frontend_base: &str, ok: bool, reason: &str) -> Response {
    let url = if ok {
        format!(
            "{}/config?section=platforms&discord_oauth=ok",
            frontend_base.trim_end_matches('/')
        )
    } else {
        format!(
            "{}/config?section=platforms&discord_oauth=error&reason={}",
            frontend_base.trim_end_matches('/'),
            urlencoding::encode(reason)
        )
    };
    no_store_redirect(&url)
}

// ---------- 调试 / 状态 ----------

/// 获取 Discord 完整资料包（画像 + 服务器 + 连接）
pub async fn get_discord_profile(
    Query(params): Query<DiscordTokenQuery>,
) -> Result<Json<ApiResponse<DiscordUserResponse>>, StatusCode> {
    let access_token = params.access_token.trim();
    if access_token.is_empty() {
        return Ok(Json(ApiResponse {
            success: false,
            data: None,
            message: "access_token 为必填".to_string(),
        }));
    }

    let fetcher = PlatformFetcher::new().await;
    match fetcher.fetch_discord_profile_bundle(access_token).await {
        Ok(bundle) => {
            let user = bundle.get("user").cloned().unwrap_or(Value::Null);
            let guilds = bundle
                .get("guilds")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let connections = bundle
                .get("connections")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let display = user
                .get("global_name")
                .and_then(|v| v.as_str())
                .or_else(|| user.get("username").and_then(|v| v.as_str()))
                .unwrap_or("unknown")
                .to_string();

            Ok(Json(ApiResponse {
                success: true,
                data: Some(DiscordUserResponse {
                    guild_count: guilds.len(),
                    connection_count: connections.len(),
                    user,
                    guilds,
                    connections,
                }),
                message: format!("✓ Discord user {} verified", display),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to fetch Discord profile: {}", e);
            Ok(Json(ApiResponse {
                success: false,
                data: None,
                message: format!("获取 Discord 资料失败: {}", e),
            }))
        }
    }
}

/// 仅验证 token 并返回 /users/@me
pub async fn get_discord_me(
    Query(params): Query<DiscordTokenQuery>,
) -> Result<Json<ApiResponse<Value>>, StatusCode> {
    let access_token = params.access_token.trim();
    if access_token.is_empty() {
        return Ok(Json(ApiResponse {
            success: false,
            data: None,
            message: "access_token 为必填".to_string(),
        }));
    }

    let fetcher = PlatformFetcher::new().await;
    match fetcher.fetch_discord_me(access_token).await {
        Ok(user) => {
            let display = user
                .get("global_name")
                .and_then(|v| v.as_str())
                .or_else(|| user.get("username").and_then(|v| v.as_str()))
                .unwrap_or("unknown")
                .to_string();
            Ok(Json(ApiResponse {
                success: true,
                data: Some(user),
                message: format!("✓ Discord user {} verified", display),
            }))
        }
        Err(e) => Ok(Json(ApiResponse {
            success: false,
            data: None,
            message: format!("验证 Discord token 失败: {}", e),
        })),
    }
}

/// 说明端点：数据平台所需 scope、一键授权 URL、需登记的 redirect_uri
pub async fn discord_status() -> Json<Value> {
    let redirect_uri = platform_redirect_uri().await;
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let app_configured = resolve_discord_oauth_app(&config).is_ok();
    let has_token = config
        .discord_access_token
        .as_ref()
        .is_some_and(|s| !s.is_empty());

    Json(json!({
        "platform": "discord",
        "api_base": "https://discord.com/api/v10",
        "required_scopes": ["identify", "guilds", "connections"],
        "oauth": {
            "start": "GET /api/platforms/discord/oauth/start",
            "callback": "GET /api/platforms/discord/oauth/callback",
            "redirect_uri": redirect_uri,
            "app_configured": app_configured,
            "has_platform_token": has_token,
            "hint": "Add redirect_uri to Discord Developer Portal → OAuth2 → Redirects (in addition to login callback)."
        },
        "endpoints": {
            "me": "GET /api/discord/me?access_token=...",
            "profile": "GET /api/discord/profile?access_token=...",
        },
    }))
}

// ---------- 一键授权 ----------

/// 管理员发起 Discord 数据平台授权
pub async fn oauth_start(headers: HeaderMap) -> Result<Response, (StatusCode, Json<Value>)> {
    let claims = verify_current_admin_from_headers(&headers).await?;
    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid user id"})),
        )
    })?;

    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let (client_id, _client_secret) = resolve_discord_oauth_app(&config).map_err(|msg| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "discord_app_not_configured",
                "message": msg,
            })),
        )
    })?;
    drop(config);

    let redirect_uri = platform_redirect_uri().await;
    let state = random_state();

    insert_state(
        state.clone(),
        StoredState {
            provider_slug: PLATFORM_STATE_SLUG.to_string(),
            purpose: OAuthPurpose::PlatformData {
                user_id,
                platform: "discord".to_string(),
            },
            created_at: std::time::Instant::now(),
        },
    )
    .await;

    let mut url = url::Url::parse(DISCORD_AUTHORIZE_URL).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("invalid authorize url: {e}")})),
        )
    })?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &client_id)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("scope", DISCORD_DATA_SCOPES)
        .append_pair("state", &state)
        // 确保用户能看到权限列表（含 connections / guilds）
        .append_pair("prompt", "consent");

    tracing::info!(
        "🔐 Discord platform OAuth start for user {} → redirect_uri={}",
        user_id,
        redirect_uri
    );

    Ok(no_store_redirect(url.as_str()))
}

/// Discord 数据平台 OAuth 回调：交换 token → 写入配置 → 回配置页
pub async fn oauth_callback(
    State(db): State<DatabaseConnection>,
    Query(params): Query<OAuthCallbackQuery>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let frontend_base = SiteConfig::get_base_url().await;

    if let Some(err) = params.error.as_ref() {
        let desc = params.error_description.unwrap_or_default();
        tracing::warn!("Discord platform OAuth error: {} ({})", err, desc);
        // Provider error codes are URL-encoded by config_redirect; still fixed path under base_url.
        return Ok(config_redirect(&frontend_base, false, err));
    }

    let code = match params
        .code
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(c) => c.to_string(),
        None => {
            return Ok(config_redirect(&frontend_base, false, "missing_code"));
        }
    };
    let state_param = match params
        .state
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(s) => s.to_string(),
        None => {
            return Ok(config_redirect(&frontend_base, false, "missing_state"));
        }
    };

    let stored = match consume_state(&state_param).await {
        Ok(s) => s,
        Err(err) => {
            return Ok(config_redirect(
                &frontend_base,
                false,
                &format!("state_{}", err.as_str()),
            ));
        }
    };

    if stored.provider_slug != PLATFORM_STATE_SLUG {
        return Ok(config_redirect(&frontend_base, false, "state_mismatch"));
    }
    let OAuthPurpose::PlatformData {
        user_id: _admin_id,
        platform,
    } = stored.purpose
    else {
        return Ok(config_redirect(&frontend_base, false, "wrong_purpose"));
    };
    if platform != "discord" {
        return Ok(config_redirect(&frontend_base, false, "wrong_platform"));
    }

    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let (client_id, client_secret) = match resolve_discord_oauth_app(&config) {
        Ok(v) => v,
        Err(msg) => {
            tracing::error!("Discord app missing on callback: {}", msg);
            return Ok(config_redirect(&frontend_base, false, "app_not_configured"));
        }
    };
    drop(config);

    let redirect_uri = platform_redirect_uri().await;
    let token_json =
        match exchange_discord_code(&code, &redirect_uri, &client_id, &client_secret).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("Discord token exchange failed: {}", e);
                return Ok(config_redirect(&frontend_base, false, "token_exchange"));
            }
        };

    let access_token = token_json
        .get("access_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let Some(access_token) = access_token else {
        return Ok(config_redirect(&frontend_base, false, "no_access_token"));
    };

    let refresh_token = token_json
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let expires_in = token_json
        .get("expires_in")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let expires_at = if expires_in > 0 {
        Some(chrono::Utc::now().timestamp() + expires_in)
    } else {
        None
    };

    // 拉一次 @me 校验 token，并写入 user_id
    let fetcher = PlatformFetcher::new().await;
    let user_id_discord = match fetcher.fetch_discord_me(&access_token).await {
        Ok(user) => user
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        Err(e) => {
            tracing::warn!("Discord @me after OAuth failed (still saving token): {}", e);
            None
        }
    };

    let mut updates: HashMap<String, Value> = HashMap::new();
    updates.insert("discord_access_token".to_string(), json!(access_token));
    if let Some(rt) = refresh_token {
        updates.insert("discord_refresh_token".to_string(), json!(rt));
    }
    if let Some(exp) = expires_at {
        updates.insert(
            "discord_token_expires_at".to_string(),
            json!(exp.to_string()),
        );
    }
    if let Some(uid) = user_id_discord {
        updates.insert("discord_user_id".to_string(), json!(uid));
    }
    // 授权成功默认启用平台
    updates.insert("discord_enabled".to_string(), json!(true));

    let svc = ConfigService::new(db.clone());
    if let Err(e) = svc.update_configs(updates).await {
        tracing::error!("Failed to persist Discord platform tokens: {}", e);
        return Ok(config_redirect(&frontend_base, false, "save_failed"));
    }

    reload_global_config(&db).await;

    tracing::info!("✓ Discord platform OAuth tokens saved and platform enabled");
    Ok(config_redirect(&frontend_base, true, "ok"))
}

async fn exchange_discord_code(
    code: &str,
    redirect_uri: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<Value, String> {
    let http = crate::services::http_client::get_global_client().await;
    let resp = http
        .post(DISCORD_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ])
        .send()
        .await
        .map_err(|e| format!("token request failed: {e}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "token endpoint {} — {}",
            status,
            body.chars().take(300).collect::<String>()
        ));
    }
    serde_json::from_str(&body).map_err(|e| format!("token JSON parse: {e}"))
}
