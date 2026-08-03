// Discord 数据平台 API — 用户 OAuth token（identify / guilds / connections）
//
// 一键授权（与登录 OAuth 分离）：
// GET /api/platforms/discord/oauth/start     管理员发起，跳转 Discord
// GET /api/platforms/discord/oauth/callback  写回 platform tokens，回配置页
//
// 复用 OAuth 登录里配置的 Discord Application（client_id/secret），
// 但 redirect_uri 与 scope 独立，需在 Discord Developer Portal 额外登记 callback。

use crate::error::HttpError;
use axum::{
    extract::Query,
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
use crate::services::oauth::state::{
    consume_state, issue_state, oauth_tx_clear_cookie_value, oauth_tx_cookie_matches,
    oauth_tx_set_cookie_value, ConsumeStateError, OAuthPurpose, StoredState,
};

const DISCORD_AUTHORIZE_URL: &str = "https://discord.com/api/oauth2/authorize";
const DISCORD_TOKEN_URL: &str = "https://discord.com/api/oauth2/token";
/// 数据平台专用 scope（不含 openid，与登录 scope 分离）
const DISCORD_DATA_SCOPES: &str = "identify guilds connections";
const PLATFORM_STATE_SLUG: &str = "discord-platform";

/// Query for debug read endpoints. `access_token` must not be supplied (Steam/Bangumi style);
/// handlers use the server-stored platform token from OAuth.
#[derive(Debug, Deserialize, Default)]
pub struct DiscordTokenQuery {
    /// Forbidden in query — leaks into logs/Referer. Use Connect Discord OAuth instead.
    pub access_token: Option<String>,
}

/// Reject client-supplied Discord tokens in the query string.
pub(crate) fn reject_query_access_token(access_token: &Option<String>) -> Result<(), HttpError> {
    if access_token.as_ref().is_some_and(|k| !k.trim().is_empty()) {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "access_token_not_allowed",
                "message": "Do not pass Discord access tokens in the query string; connect Discord via OAuth so the server stores discord_access_token"
            })),
        )));
    }
    Ok(())
}

async fn server_discord_access_token() -> Result<String, HttpError> {
    let cfg = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
    cfg.discord_access_token
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "success": false,
                    "error": "discord_token_not_configured",
                    "message": "Discord platform token not configured. Use Connect Discord (OAuth) in settings."
                })),
            ))
        })
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

async fn reload_global_config(
    db: &DatabaseConnection,
    dynamic_config: &std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>,
) {
    let svc = ConfigService::new(db.clone());
    match svc.load_config().await {
        Ok(cfg) => {
            *dynamic_config.write().await = cfg;
        }
        Err(e) => {
            tracing::warn!(
                "Failed to reload dynamic config after Discord OAuth: {}",
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
    response
        .headers_mut()
        .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
}

fn no_store_redirect(url: &str) -> Response {
    let mut response = Redirect::to(url).into_response();
    apply_no_store_headers(&mut response);
    response
}

/// Redirect back to settings → platforms with Discord focus.
/// Uses path `/config` (admin SPA) + section + platform query so FE
/// `useConfigNavigation` / ConfigForm can open Platforms and highlight Discord.
/// Never sends users to login OAuth callback (`/api/auth/oauth/...`).
fn config_redirect(frontend_base: &str, ok: bool, reason: &str) -> Response {
    let base = frontend_base.trim_end_matches('/');
    let url = if ok {
        format!(
            "{}/config?section=platforms&platform=discord&discord_oauth=ok",
            base
        )
    } else {
        format!(
            "{}/config?section=platforms&platform=discord&discord_oauth=error&reason={}",
            base,
            urlencoding::encode(reason)
        )
    };
    no_store_redirect(&url)
}

// 调试 / 状态

/// 获取 Discord 完整资料包（画像 + 服务器 + 连接）
///
/// Token 仅来自服务端 OAuth 落库的 `discord_access_token`；query 传 token 一律 400。
pub async fn get_discord_profile(
    Query(params): Query<DiscordTokenQuery>,
) -> Result<Json<ApiResponse<DiscordUserResponse>>, HttpError> {
    reject_query_access_token(&params.access_token)?;
    let access_token = server_discord_access_token().await?;

    let fetcher = PlatformFetcher::new().await;
    match fetcher.fetch_discord_profile_bundle(&access_token).await {
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

/// 仅验证服务端已存 token 并返回 /users/@me
///
/// Token 仅来自 `discord_access_token`；query 传 token 一律 400。
pub async fn get_discord_me(
    Query(params): Query<DiscordTokenQuery>,
) -> Result<Json<ApiResponse<Value>>, HttpError> {
    reject_query_access_token(&params.access_token)?;
    let access_token = server_discord_access_token().await?;

    let fetcher = PlatformFetcher::new().await;
    match fetcher.fetch_discord_me(&access_token).await {
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
pub async fn discord_status(
    axum::extract::State(dynamic_config): axum::extract::State<
        std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>,
    >,
) -> Json<Value> {
    let redirect_uri = platform_redirect_uri().await;
    let config = dynamic_config.read().await;
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
            "me": "GET /api/discord/me (uses server discord_access_token; query tokens rejected)",
            "profile": "GET /api/discord/profile (uses server discord_access_token; query tokens rejected)",
        },
        "security": {
            "query_access_token": "rejected",
            "token_source": "discord_access_token from OAuth Connect Discord",
        },
    }))
}

// 一键授权

/// 管理员发起 Discord 数据平台授权
pub async fn oauth_start(
    headers: HeaderMap,
    crate::extract::Db(db): crate::extract::Db,
    axum::extract::State(dynamic_config): axum::extract::State<
        std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>,
    >,
) -> Result<Response, HttpError> {
    let claims = verify_current_admin_from_headers(&headers, &db).await?;
    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Invalid user id"})),
        )
    })?;

    let config = dynamic_config.read().await;
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
    let issued = issue_state(StoredState {
        provider_slug: PLATFORM_STATE_SLUG.to_string(),
        purpose: OAuthPurpose::PlatformData {
            user_id,
            platform: "discord".to_string(),
        },
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))))?;

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
        .append_pair("state", &issued.token)
        // 确保用户能看到权限列表（含 connections / guilds）
        .append_pair("prompt", "consent");

    tracing::info!(
        "🔐 Discord platform OAuth start for user {} → redirect_uri={}",
        user_id,
        redirect_uri
    );

    // MYR-003: bind state to this browser via oauth_tx cookie.
    let is_production = SiteConfig::is_production().await;
    let mut response = no_store_redirect(url.as_str());
    if let Ok(value) =
        HeaderValue::from_str(&oauth_tx_set_cookie_value(&issued.browser_tx, is_production))
    {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    Ok(response)
}

/// Clear oauth_tx on a Discord platform OAuth redirect response.
async fn discord_oauth_tx_cleared(mut response: Response) -> Response {
    let is_production = SiteConfig::is_production().await;
    if let Ok(value) = HeaderValue::from_str(&oauth_tx_clear_cookie_value(is_production)) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

/// Discord 数据平台 OAuth 回调：交换 token → 写入配置 → 回配置页
pub async fn oauth_callback(
    crate::extract::Db(db): crate::extract::Db,
    axum::extract::State(dynamic_config): axum::extract::State<
        std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>,
    >,
    headers: HeaderMap,
    Query(params): Query<OAuthCallbackQuery>,
) -> Result<Response, HttpError> {
    let frontend_base = SiteConfig::get_base_url().await;

    if let Some(err) = params.error.as_ref() {
        let desc = params.error_description.unwrap_or_default();
        tracing::warn!("Discord platform OAuth error: {} ({})", err, desc);
        // Provider error codes are URL-encoded by config_redirect; still fixed path under base_url.
        return Ok(discord_oauth_tx_cleared(config_redirect(&frontend_base, false, err)).await);
    }

    let code = match params
        .code
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(c) => c.to_string(),
        None => {
            return Ok(
                discord_oauth_tx_cleared(config_redirect(&frontend_base, false, "missing_code"))
                    .await,
            );
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
            return Ok(
                discord_oauth_tx_cleared(config_redirect(&frontend_base, false, "missing_state"))
                    .await,
            );
        }
    };

    let outcome = match consume_state(&state_param).await {
        Ok(o) => o,
        Err(err) => {
            return Ok(discord_oauth_tx_cleared(config_redirect(
                &frontend_base,
                false,
                &format!("state_{}", err.as_str()),
            ))
            .await);
        }
    };

    // MYR-003: require oauth_tx cookie match (fail closed).
    let cookie_header = headers.get(header::COOKIE).and_then(|v| v.to_str().ok());
    if !oauth_tx_cookie_matches(cookie_header, outcome.browser_tx()) {
        tracing::warn!("Discord platform OAuth: missing/mismatched oauth_tx cookie");
        return Ok(discord_oauth_tx_cleared(config_redirect(
            &frontend_base,
            false,
            "browser_tx_mismatch",
        ))
        .await);
    }

    if outcome.stored().provider_slug != PLATFORM_STATE_SLUG {
        return Ok(discord_oauth_tx_cleared(config_redirect(
            &frontend_base,
            false,
            "state_mismatch",
        ))
        .await);
    }
    let purpose = outcome.stored().purpose.clone();
    let OAuthPurpose::PlatformData {
        user_id: _admin_id,
        platform,
    } = purpose
    else {
        return Ok(
            discord_oauth_tx_cleared(config_redirect(&frontend_base, false, "wrong_purpose")).await,
        );
    };
    if platform != "discord" {
        return Ok(
            discord_oauth_tx_cleared(config_redirect(&frontend_base, false, "wrong_platform")).await,
        );
    }

    // Browser double-load / retry: never re-exchange the one-time code.
    // Soft-success if platform tokens were already persisted by the first request.
    if outcome.is_replay() {
        let config = dynamic_config.read().await;
        let has_token = config
            .discord_access_token
            .as_ref()
            .is_some_and(|s| !s.is_empty());
        drop(config);
        if has_token {
            tracing::info!(
                "Discord platform OAuth state replay — tokens already stored, soft-success"
            );
            return Ok(discord_oauth_tx_cleared(config_redirect(&frontend_base, true, "ok")).await);
        }
        tracing::warn!(
            "Discord platform OAuth state replay — no tokens stored; first attempt may have failed"
        );
        return Ok(discord_oauth_tx_cleared(config_redirect(
            &frontend_base,
            false,
            &format!("state_{}", ConsumeStateError::Replay.as_str()),
        ))
        .await);
    }

    let config = dynamic_config.read().await;
    let (client_id, client_secret) = match resolve_discord_oauth_app(&config) {
        Ok(v) => v,
        Err(msg) => {
            tracing::error!("Discord app missing on callback: {}", msg);
            return Ok(discord_oauth_tx_cleared(config_redirect(
                &frontend_base,
                false,
                "app_not_configured",
            ))
            .await);
        }
    };
    drop(config);

    let redirect_uri = platform_redirect_uri().await;
    let token_json =
        match exchange_discord_code(&code, &redirect_uri, &client_id, &client_secret).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("Discord token exchange failed: {}", e);
                return Ok(discord_oauth_tx_cleared(config_redirect(
                    &frontend_base,
                    false,
                    "token_exchange",
                ))
                .await);
            }
        };

    let access_token = token_json
        .get("access_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let Some(access_token) = access_token else {
        return Ok(
            discord_oauth_tx_cleared(config_redirect(&frontend_base, false, "no_access_token"))
                .await,
        );
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
        return Ok(
            discord_oauth_tx_cleared(config_redirect(&frontend_base, false, "save_failed")).await,
        );
    }

    // Same Arc as AppState.dynamic_config (from_shared); write via State handle.
    reload_global_config(&db, &dynamic_config).await;

    tracing::info!("✓ Discord platform OAuth tokens saved and platform enabled");
    Ok(discord_oauth_tx_cleared(config_redirect(&frontend_base, true, "ok")).await)
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

#[cfg(test)]
mod discord_secret_gate_tests {
    use super::*;
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn reject_query_access_token_blocks_nonempty() {
        let err = reject_query_access_token(&Some("ODM.secret".into())).unwrap_err();
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .expect("body");
        let v: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(v["error"], "access_token_not_allowed");
        assert!(
            v.get("message")
                .and_then(|m| m.as_str())
                .is_some_and(|m| m.to_ascii_lowercase().contains("query")),
            "message should mention query restriction: {v}"
        );
    }

    #[test]
    fn reject_query_access_token_allows_absent_or_blank() {
        assert!(reject_query_access_token(&None).is_ok());
        assert!(reject_query_access_token(&Some(String::new())).is_ok());
        assert!(reject_query_access_token(&Some(" \t ".into())).is_ok());
    }
}
