//! 通用 OAuth handler（routing by `:slug`）
//!
//! 详见 docs/development/OAUTH.md
//!
//! 端点：
//! GET    /api/auth/oauth/providers              列出 enabled providers
//! GET    /api/auth/oauth/:slug/login            重定向到授权页
//! GET    /api/auth/oauth/:slug/callback         交换 code + 登录/创建用户
//! GET    /api/auth/oauth/:slug/link             绑定 (需 JWT + is_admin)
//! DELETE /api/auth/oauth/:slug/unlink/:id       解绑
//! GET    /api/auth/identities                   当前用户所有 identities
//! POST   /api/auth/identities/:id/primary       设为画像源（is_primary + 同步头像等）

use crate::error::HttpError;
use axum::{
    extract::{Path, Query},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, Value as SeaValue};
use serde::Deserialize;
use serde_json::{json, Value};
use std::env;

use crate::middleware::auth::{auth_cookie_value, encode_session_token, mint_session_claims};
use crate::oauth_url_builder::SiteConfig;
use crate::services::oauth::{
    registry::REGISTRY,
    state::{
        issue_state, oauth_tx_clear_cookie_value, oauth_tx_cookie_matches,
        oauth_tx_set_cookie_value, verify_state, ConsumeOutcome, ConsumeStateError, OAuthPurpose,
        StoredState, OAUTH_TX_COOKIE,
    },
    AuthFlowSecrets, NormalizedProfile,
};

// 工具函数

async fn build_redirect_uri(slug: &str) -> String {
    let base = SiteConfig::get_base_url().await;
    format!("{}/api/auth/oauth/{}/callback", base, slug)
}

fn err_500(msg: impl Into<String>) -> HttpError {
    let msg = msg.into();
    let mut body = json!({"error": msg.clone()});
    if let Some(code) = myriad_error::AppError::inferred_code(&msg) {
        body["code"] = json!(code);
    }
    HttpError::from((StatusCode::INTERNAL_SERVER_ERROR, Json(body)))
}

fn oauth_start_failed(error: impl std::fmt::Display) -> HttpError {
    tracing::error!("OAuth start failed: {error}");
    HttpError::from((
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "Failed to start authorization",
            "code": "oauth_authorize_failed",
        })),
    ))
}
fn err_400(msg: impl Into<String>) -> HttpError {
    HttpError::from((StatusCode::BAD_REQUEST, Json(json!({"error": msg.into()}))))
}
fn err_404(msg: impl Into<String>) -> HttpError {
    HttpError::from((StatusCode::NOT_FOUND, Json(json!({"error": msg.into()}))))
}

/// Attach anti-caching headers so OAuth redirects / callbacks are never stored by browsers or CDNs.
fn apply_no_store_headers(response: &mut Response) {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, private"),
    );
    response
        .headers_mut()
        .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
}

/// `Redirect` → `Response` with Cache-Control: no-store, private.
fn no_store_redirect(url: &str) -> Response {
    let mut response = Redirect::to(url).into_response();
    apply_no_store_headers(&mut response);
    response
}

/// Browser-friendly client error: always land on SiteConfig base_url `/login`.
/// `error_code` is a fixed token we control (never a free-form URL) — no open redirect.
fn oauth_client_error_redirect(frontend_base: &str, error_code: &str) -> Response {
    let url = format!(
        "{}/login?oauth_error={}",
        frontend_base.trim_end_matches('/'),
        urlencoding::encode(error_code),
    );
    no_store_redirect(&url)
}

/// Append a `Set-Cookie` header (must use `append` when multiple cookies are set).
fn append_set_cookie(response: &mut Response, cookie: &str) {
    if let Ok(value) = HeaderValue::from_str(cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    } else {
        tracing::error!("OAuth: invalid Set-Cookie header value");
    }
}

/// Login/link redirect with `oauth_tx` browser-binding cookie (MYR-003).
async fn redirect_with_oauth_tx(auth_url: &str, browser_tx: &str) -> Response {
    let is_production = SiteConfig::is_production().await;
    let mut response = no_store_redirect(auth_url);
    append_set_cookie(
        &mut response,
        &oauth_tx_set_cookie_value(browser_tx, is_production),
    );
    response
}

/// Clear `oauth_tx` on a response (success and fail-closed paths).
async fn with_oauth_tx_cleared(mut response: Response) -> Response {
    let is_production = SiteConfig::is_production().await;
    append_set_cookie(&mut response, &oauth_tx_clear_cookie_value(is_production));
    response
}

/// Fail closed when the callback browser lacks a matching `oauth_tx` cookie.
async fn reject_oauth_tx_mismatch(frontend_base: &str) -> Response {
    tracing::warn!(
        cookie = OAUTH_TX_COOKIE,
        "OAuth callback rejected: missing or mismatched browser transaction cookie"
    );
    with_oauth_tx_cleared(oauth_client_error_redirect(
        frontend_base,
        "browser_tx_mismatch",
    ))
    .await
}

/// 登录/绑定后把 provider 快照同步到 `users`，并重算画像源。
///
/// 重算是必须的：用户若选了某个 OAuth 身份作画像源，对方在 provider 改了头像，
/// 只有这次登录能把新地址带进来；不刷新 `avatar_resolved_url` 就会一直停在
/// 绑定当天那张脸。
async fn sync_user_oauth_profile_snapshot(
    db: &DatabaseConnection,
    user_id: i32,
    slug: &str,
    profile: &NormalizedProfile,
) {
    sync_user_oauth_columns(db, user_id, slug, profile).await;
    crate::services::avatar::refresh_avatar_snapshot(db, user_id).await;
}

async fn sync_user_oauth_columns(
    db: &DatabaseConnection,
    user_id: i32,
    slug: &str,
    profile: &NormalizedProfile,
) {
    let avatar_url = profile
        .avatar_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    if slug == "github" {
        let github_id = profile.provider_user_id.parse::<i64>().ok();
        if avatar_url.is_none() && github_id.is_none() {
            return;
        }

        if let Err(e) = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE users SET linked_github_id = COALESCE($1, linked_github_id), \
                 avatar_url = COALESCE($2, avatar_url), updated_at = NOW() WHERE id = $3",
                vec![
                    SeaValue::BigInt(github_id),
                    avatar_url
                        .map(|s| SeaValue::String(Some(s)))
                        .unwrap_or(SeaValue::String(None)),
                    SeaValue::Int(Some(user_id)),
                ],
            ))
            .await
        {
            tracing::warn!("Failed to sync GitHub OAuth snapshot to user: {}", e);
        }
        return;
    }

    let Some(avatar_url) = avatar_url else {
        return;
    };

    if let Err(e) = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE users SET avatar_url = $1, updated_at = NOW() WHERE id = $2",
            vec![
                SeaValue::String(Some(avatar_url)),
                SeaValue::Int(Some(user_id)),
            ],
        ))
        .await
    {
        tracing::warn!("Failed to sync OAuth avatar to user: {}", e);
    }
}

// GET /api/auth/oauth/providers

pub async fn list_providers() -> Json<Value> {
    let providers = REGISTRY.list().await;
    Json(json!({ "providers": providers }))
}

// GET /api/auth/oauth/:slug/login

pub async fn provider_login(Path(slug): Path<String>) -> Result<Response, HttpError> {
    let provider = REGISTRY
        .get(&slug)
        .await
        .ok_or_else(|| err_404(format!("OAuth provider '{slug}' not configured")))?;

    let issued = issue_state(StoredState {
        provider_slug: slug.clone(),
        purpose: OAuthPurpose::Login,
    })
    .await
    .map_err(oauth_start_failed)?;

    let secrets = AuthFlowSecrets {
        code_verifier: issued.code_verifier.clone(),
        oidc_nonce: issued.browser_tx.clone(),
    };
    let redirect_uri = build_redirect_uri(&slug).await;
    let auth_url = provider
        .build_auth_url(&issued.token, &redirect_uri, &secrets)
        .await
        .map_err(oauth_start_failed)?;

    // Bind signed state to this browser via oauth_tx (MYR-003).
    Ok(redirect_with_oauth_tx(&auth_url, &issued.browser_tx).await)
}

// GET /api/auth/oauth/:slug/link  (任何已登录用户)

pub async fn provider_link(
    Path(slug): Path<String>,
    crate::extract::Db(db): crate::extract::Db,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let claims = crate::middleware::auth::authenticate_request(&headers, &db)
        .await
        .map_err(|_| {
            HttpError::from((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Unauthorized"})),
            ))
        })?;

    let user_id: i32 = claims.sub.parse().map_err(|_| err_400("Invalid user id"))?;
    if user_id <= 0 {
        return Err(err_400("Guest sessions cannot link OAuth providers"));
    }

    let provider = REGISTRY
        .get(&slug)
        .await
        .ok_or_else(|| err_404(format!("OAuth provider '{slug}' not configured")))?;

    let issued = issue_state(StoredState {
        provider_slug: slug.clone(),
        purpose: OAuthPurpose::LinkAccount(user_id),
    })
    .await
    .map_err(oauth_start_failed)?;

    let secrets = AuthFlowSecrets {
        code_verifier: issued.code_verifier.clone(),
        oidc_nonce: issued.browser_tx.clone(),
    };
    let redirect_uri = build_redirect_uri(&slug).await;
    let auth_url = provider
        .build_auth_url(&issued.token, &redirect_uri, &secrets)
        .await
        .map_err(oauth_start_failed)?;

    Ok(redirect_with_oauth_tx(&auth_url, &issued.browser_tx).await)
}

// GET /api/auth/oauth/:slug/callback

/// OAuth 回调参数 — `code` 和 `state` 在成功路径必需，但 provider 报错时
/// （用户拒绝授权 / 配置错误等）会以 `?error=...&error_description=...` 形式回调，
/// 所以全部字段都 optional 来容错解析。
#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

pub async fn provider_callback(
    Path(slug): Path<String>,
    Query(params): Query<CallbackQuery>,
    headers: HeaderMap,
    crate::extract::Db(db): crate::extract::Db,
) -> Result<Response, HttpError> {
    let frontend_base = SiteConfig::get_base_url().await;

    // provider 报错路径：直接重定向到登录页，带 error 信息
    if let Some(err) = params.error.as_ref() {
        let desc = params.error_description.unwrap_or_default();
        tracing::warn!(
            "🚨 OAuth provider '{}' returned error: {} ({})",
            slug,
            err,
            desc
        );
        let url = format!(
            "{}/login?oauth_error={}&desc={}",
            frontend_base.trim_end_matches('/'),
            urlencoding::encode(err),
            urlencoding::encode(&desc),
        );
        // Drop any leftover oauth_tx from a partial flow.
        return Ok(with_oauth_tx_cleared(no_store_redirect(&url)).await);
    }

    let Some(code) = params.code.filter(|c| !c.trim().is_empty()) else {
        return Ok(with_oauth_tx_cleared(oauth_client_error_redirect(
            &frontend_base,
            "missing_code",
        ))
        .await);
    };
    let Some(state_param) = params.state.filter(|s| !s.trim().is_empty()) else {
        return Ok(with_oauth_tx_cleared(oauth_client_error_redirect(
            &frontend_base,
            "missing_state",
        ))
        .await);
    };

    // 1. Verify signed state WITHOUT burning the nonce yet.
    // Cookie binding (MYR-003) must succeed first so a session-swap attempt
    // cannot one-shot invalidate a legitimate browser's pending state.
    let verified = match verify_state(&state_param).await {
        Ok(v) => v,
        Err(err) => {
            return Ok(with_oauth_tx_cleared(oauth_client_error_redirect(
                &frontend_base,
                &format!("state_{}", err.as_str()),
            ))
            .await);
        }
    };

    // MYR-003: require oauth_tx cookie == payload nonce (fail closed) BEFORE mark_used.
    let cookie_header = headers.get(header::COOKIE).and_then(|v| v.to_str().ok());
    if !oauth_tx_cookie_matches(cookie_header, verified.browser_tx()) {
        return Ok(reject_oauth_tx_mismatch(&frontend_base).await);
    }

    if verified.stored().provider_slug != slug {
        tracing::warn!(
            "OAuth state/slug mismatch: state was for '{}', got '{}'",
            verified.stored().provider_slug,
            slug
        );
        return Ok(with_oauth_tx_cleared(oauth_client_error_redirect(
            &frontend_base,
            "state_slug_mismatch",
        ))
        .await);
    }

    // Capture PKCE / OIDC secrets before mark_used consumes VerifiedState.
    let secrets = AuthFlowSecrets {
        code_verifier: verified.code_verifier().to_string(),
        oidc_nonce: verified.oidc_nonce().to_string(),
    };

    // Cookie matched → burn nonce (Fresh or Replay).
    let outcome = verified.mark_used().await;

    // Browser double-load / retry: soft-recover without token exchange.
    // Replay is only reached after the same oauth_tx cookie check above.
    let stored = match outcome {
        ConsumeOutcome::Replay { stored, .. } => {
            let resp = handle_callback_replay(&db, &slug, &stored, &frontend_base).await?;
            return Ok(with_oauth_tx_cleared(resp).await);
        }
        ConsumeOutcome::Fresh { stored, .. } => stored,
    };

    // 2. 取 provider — browser-friendly redirect (not opaque JSON 500/404)
    let provider = match REGISTRY.get(&slug).await {
        Some(p) => p,
        None => {
            tracing::error!("OAuth provider '{}' missing at callback", slug);
            return Ok(with_oauth_tx_cleared(oauth_client_error_redirect(
                &frontend_base,
                "provider_unavailable",
            ))
            .await);
        }
    };

    // 3. exchange + fetch profile — redirect with stable codes instead of raw 500
    //    Pass AuthFlowSecrets so OIDC can send code_verifier and verify nonce.
    let redirect_uri = build_redirect_uri(&slug).await;
    let tokens = match provider.exchange_code(&code, &redirect_uri, &secrets).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("OAuth token exchange failed for '{}': {}", slug, e);
            return Ok(with_oauth_tx_cleared(oauth_client_error_redirect(
                &frontend_base,
                "token_exchange_failed",
            ))
            .await);
        }
    };
    let profile = match provider.fetch_profile(&tokens).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("OAuth profile fetch failed for '{}': {}", slug, e);
            return Ok(with_oauth_tx_cleared(oauth_client_error_redirect(
                &frontend_base,
                "profile_fetch_failed",
            ))
            .await);
        }
    };

    tracing::info!(
        "🔐 OAuth callback for {} — provider_user_id={}, username={}",
        slug,
        profile.provider_user_id,
        profile.username
    );

    // 4. 分流：LinkAccount vs Login vs 数据平台授权
    // 数据平台授权走独立 callback（如 /api/platforms/discord/oauth/callback），
    // 若误入登录 callback 则友好重定向提示。
    // Always clear oauth_tx — map all Err paths to browser redirects so cookie is cleared.
    let response: Response = match stored.purpose {
        OAuthPurpose::LinkAccount(link_user_id) => {
            match handle_link(&db, &slug, link_user_id, &profile, &frontend_base).await {
                Ok(resp) => resp,
                Err(err) => {
                    // handle_link usually returns Ok(redirect) for business errors;
                    // map rare Err (DB/internal) to a browser redirect so oauth_tx clears.
                    tracing::error!(
                        "OAuth link failed for provider '{}' (status={}): {:?}",
                        slug,
                        err.0.status_u16(),
                        err.0
                    );
                    oauth_client_error_redirect(&frontend_base, "link_failed")
                }
            }
        }
        OAuthPurpose::Login => match handle_login(&db, &slug, &profile).await {
            Ok(resp) => resp,
            Err(err) => {
                let status = err.0.status_u16();
                let label = err.0.error_label().to_string();
                tracing::error!(
                    "OAuth login failed for provider '{}' (status={}): {:?}",
                    slug,
                    status,
                    err.0
                );
                // Prefer stable browser redirects over opaque JSON for callbacks.
                let code = if status == StatusCode::CONFLICT.as_u16()
                    || label == "email_already_registered"
                {
                    "email_already_registered"
                } else {
                    "login_failed"
                };
                oauth_client_error_redirect(&frontend_base, code)
            }
        },
        OAuthPurpose::PlatformData { platform, .. } => {
            tracing::warn!(
                "PlatformData OAuth state for '{}' hit login callback; redirecting",
                platform
            );
            // Platform OAuth used wrong redirect_uri (login callback). Send admin
            // to platforms settings with Discord focus — never leave them on /login.
            let platform = platform.to_ascii_lowercase();
            let url = format!(
                "{}/config?section=platforms&platform={}&discord_oauth=error&reason={}",
                frontend_base.trim_end_matches('/'),
                urlencoding::encode(&platform),
                urlencoding::encode("wrong_callback")
            );
            no_store_redirect(&url)
        }
    };

    Ok(with_oauth_tx_cleared(response).await)
}

/// Soft-recover a second callback hit with the same (already-consumed) state.
///
/// **Must only run after** `oauth_tx` cookie matched the state nonce — never skip
/// that check for soft-success (MYR-003 / session-swap defense).
/// We never re-exchange the authorization code (provider codes are one-time).
///
/// Tradeoff for Login: without `provider_user_id` on pure replay we cannot prove
/// a session was issued. Prefer soft-success (`/?auth=success`) so a browser
/// double-load after a real login does not toast an error; false positives are
/// rare (first request would have to fail after mark_used but before session cookie).
async fn handle_callback_replay(
    db: &DatabaseConnection,
    slug: &str,
    stored: &StoredState,
    frontend_base: &str,
) -> Result<Response, HttpError> {
    match &stored.purpose {
        OAuthPurpose::LinkAccount(link_user_id) => {
            handle_link_replay(db, slug, *link_user_id, frontend_base).await
        }
        OAuthPurpose::Login => {
            tracing::info!(
                provider = %slug,
                "OAuth Login state replay — soft-success redirect (no re-exchange)"
            );
            let url = format!("{}/?auth=success", frontend_base.trim_end_matches('/'));
            Ok(no_store_redirect(&url))
        }
        OAuthPurpose::PlatformData { platform, .. } => {
            // Platform data uses a separate callback; soft-recover only if that
            // path already stored tokens is handled there. Login callback replay
            // of a platform state is always wrong_callback.
            tracing::warn!(
                "PlatformData OAuth state replay for '{}' hit login callback",
                platform
            );
            let platform = platform.to_ascii_lowercase();
            let url = format!(
                "{}/config?section=platforms&platform={}&discord_oauth=error&reason={}",
                frontend_base.trim_end_matches('/'),
                urlencoding::encode(&platform),
                urlencoding::encode("wrong_callback")
            );
            Ok(no_store_redirect(&url))
        }
    }
}

/// LinkAccount replay: if this user already has an identity for `slug`, treat as
/// success (first request bound). If not, surface `state_replay` (first attempt
/// may have failed after consume). Without `provider_user_id` we cannot detect
/// "bound to another user" on pure replay — that path only appears on Fresh.
async fn handle_link_replay(
    db: &DatabaseConnection,
    slug: &str,
    link_user_id: i32,
    frontend_url: &str,
) -> Result<Response, HttpError> {
    let existing = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT provider_username FROM user_identities \
             WHERE user_id = $1 AND provider = $2 \
             ORDER BY linked_at DESC LIMIT 1",
            vec![
                SeaValue::Int(Some(link_user_id)),
                SeaValue::String(Some(slug.to_string())),
            ],
        ))
        .await
        .map_err(|e| {
            tracing::error!("OAuth DB error: {e}");
            err_500("Database error")
        })?;

    if let Some(row) = existing {
        let username: Option<String> = row.try_get("", "provider_username").ok().flatten();
        tracing::info!(
            provider = %slug,
            user_id = link_user_id,
            "OAuth LinkAccount state replay — identity already bound, soft-success"
        );
        let url = match username.filter(|s| !s.is_empty()) {
            Some(u) => format!(
                "{}/?link=success&provider={}&username={}",
                frontend_url.trim_end_matches('/'),
                slug,
                urlencoding::encode(&u)
            ),
            None => format!(
                "{}/?link=success&provider={}",
                frontend_url.trim_end_matches('/'),
                slug
            ),
        };
        return Ok(no_store_redirect(&url));
    }

    // Also check whether this provider identity is bound to a *different* user
    // for any row of this provider — we lack provider_user_id on pure replay,
    // so we can only fail closed if this user has no binding yet.
    tracing::warn!(
        provider = %slug,
        user_id = link_user_id,
        "OAuth LinkAccount state replay — no identity for user; first attempt may have failed"
    );
    Ok(oauth_client_error_redirect(
        frontend_url,
        &format!("state_{}", ConsumeStateError::Replay.as_str()),
    ))
}

// 内部：LinkAccount 流程

async fn handle_link(
    db: &DatabaseConnection,
    slug: &str,
    link_user_id: i32,
    profile: &NormalizedProfile,
    frontend_url: &str,
) -> Result<Response, HttpError> {
    // 验证发起绑定的用户仍然存在
    let user = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM users WHERE id = $1",
            vec![SeaValue::Int(Some(link_user_id))],
        ))
        .await
        .map_err(|e| {
            tracing::error!("OAuth DB error: {e}");
            err_500("Database error")
        })?;

    if user.is_none() {
        let url = format!(
            "{}/?link=error&reason=user_not_found",
            frontend_url.trim_end_matches('/')
        );
        return Ok(no_store_redirect(&url));
    }

    // 检查 identity 是否已绑到别的 user
    let existing = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT user_id FROM user_identities \
             WHERE provider = $1 AND provider_user_id = $2",
            vec![
                SeaValue::String(Some(slug.to_string())),
                SeaValue::String(Some(profile.provider_user_id.clone())),
            ],
        ))
        .await
        .map_err(|e| {
            tracing::error!("OAuth DB error: {e}");
            err_500("Database error")
        })?;

    if let Some(row) = existing {
        let owner_id: i32 = row.try_get("", "user_id").map_err(|e| {
            tracing::error!(error = %e, "OAuth: failed to read user_id");
            err_500("Database error")
        })?;
        if owner_id != link_user_id {
            let url = format!(
                "{}/?link=error&reason=already_linked",
                frontend_url.trim_end_matches('/')
            );
            return Ok(no_store_redirect(&url));
        }
    }

    upsert_identity(db, slug, link_user_id, profile).await?;
    sync_user_oauth_profile_snapshot(db, link_user_id, slug, profile).await;

    let url = format!(
        "{}/?link=success&provider={}&username={}",
        frontend_url.trim_end_matches('/'),
        slug,
        urlencoding::encode(&profile.username)
    );
    Ok(no_store_redirect(&url))
}

// 内部：Login 流程

async fn handle_login(
    db: &DatabaseConnection,
    slug: &str,
    profile: &NormalizedProfile,
) -> Result<Response, HttpError> {
    let user_id = find_or_create_user(db, slug, profile).await?;

    // 查 is_admin + session epoch for JWT mint
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT is_admin, username, COALESCE(is_owner, false) AS is_owner, \
                    COALESCE(token_version, 0) AS token_version \
             FROM users WHERE id = $1",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|e| {
            tracing::error!("OAuth DB error: {e}");
            err_500("Database error")
        })?
        .ok_or_else(|| err_500("User vanished after create"))?;

    let is_admin: bool = row.try_get("", "is_admin").unwrap_or(false);
    let is_owner: bool = row.try_get("", "is_owner").unwrap_or(false);
    let username: String = row.try_get("", "username").unwrap_or_default();
    let token_version: i64 = row
        .try_get::<i32>("", "token_version")
        .ok()
        .map(i64::from)
        .or_else(|| row.try_get::<i64>("", "token_version").ok())
        .unwrap_or(0);

    let claims = mint_session_claims(user_id, &username, is_admin, is_owner, token_version);
    let token = encode_session_token(&claims).map_err(|e| {
        tracing::error!(error = %e, "OAuth: JWT encode failed");
        err_500("Internal error")
    })?;

    // Set-Cookie + HTML 重定向
    // 注意：使用相对路径，避免把管理员可控的 base_url 注入到 <meta refresh> / <script>
    let is_production = SiteConfig::is_production().await;
    let cookie_value = auth_cookie_value(&token, is_production);
    let html = "<!DOCTYPE html><html><head><meta charset=\"utf-8\">\
        <meta http-equiv=\"refresh\" content=\"0;url=/?auth=success\"><title>Login</title></head>\
        <body><p>Login successful, redirecting...</p>\
        <script>window.location.href=\"/?auth=success\";</script></body></html>";

    let mut response = axum::response::Html(html).into_response();
    let set_cookie = HeaderValue::from_str(&cookie_value).map_err(|e| {
        tracing::error!(error = %e, "OAuth: invalid auth cookie header");
        err_500("Internal error")
    })?;
    response
        .headers_mut()
        .insert(header::SET_COOKIE, set_cookie);
    apply_no_store_headers(&mut response);
    Ok(response)
}

// 内部：账户匹配策略

async fn find_or_create_user(
    db: &DatabaseConnection,
    slug: &str,
    profile: &NormalizedProfile,
) -> Result<i32, HttpError> {
    // 1. identity 命中 → 直接登录
    if let Some(row) = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT user_id FROM user_identities \
             WHERE provider = $1 AND provider_user_id = $2",
            vec![
                SeaValue::String(Some(slug.to_string())),
                SeaValue::String(Some(profile.provider_user_id.clone())),
            ],
        ))
        .await
        .map_err(|e| {
            tracing::error!("OAuth DB error: {e}");
            err_500("Database error")
        })?
    {
        let uid: i32 = row.try_get("", "user_id").map_err(|e| {
            tracing::error!(error = %e, "OAuth: failed to read user_id");
            err_500("Database error")
        })?;
        // 更新 identity 的 last_login_at + 档案字段
        let _ = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE user_identities SET \
                    provider_username = $1, email = $2, avatar_url = $3, profile_url = $4, \
                    raw_profile = $5, last_login_at = NOW() \
                 WHERE provider = $6 AND provider_user_id = $7",
                vec![
                    SeaValue::String(Some(profile.username.clone())),
                    profile
                        .email
                        .clone()
                        .map(|s| SeaValue::String(Some(s)))
                        .unwrap_or(SeaValue::String(None)),
                    profile
                        .avatar_url
                        .clone()
                        .map(|s| SeaValue::String(Some(s)))
                        .unwrap_or(SeaValue::String(None)),
                    profile
                        .profile_url
                        .clone()
                        .map(|s| SeaValue::String(Some(s)))
                        .unwrap_or(SeaValue::String(None)),
                    SeaValue::Json(Some(Box::new(profile.raw.clone()))),
                    SeaValue::String(Some(slug.to_string())),
                    SeaValue::String(Some(profile.provider_user_id.clone())),
                ],
            ))
            .await;
        sync_user_oauth_profile_snapshot(db, uid, slug, profile).await;
        // 更新 users 的 last_login_at
        let _ = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE users SET last_login_at = NOW() WHERE id = $1",
                vec![SeaValue::Int(Some(uid))],
            ))
            .await;
        return Ok(uid);
    }

    // 2. MYR-012 — no silent cross-issuer auto-link by email.
    //
    // Login only when (provider, provider_user_id) is already linked (step 1).
    // If the provider email is already on another account, refuse account creation
    // and require the user to sign in with their original method, then use the
    // authenticated link flow. Do not merge solely because email_verified is true.
    if let Some(email) = profile
        .email
        .as_ref()
        .map(|e| e.trim())
        .filter(|e| !e.is_empty())
    {
        if let Some(_row) = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT id FROM users WHERE LOWER(email) = LOWER($1) LIMIT 1",
                vec![SeaValue::String(Some(email.to_string()))],
            ))
            .await
            .map_err(|e| {
                tracing::error!("OAuth DB error: {e}");
                err_500("Database error")
            })?
        {
            tracing::warn!(
                provider = %slug,
                "OAuth login refused: email already registered on another account \
                 (no silent cross-issuer merge; use explicit link while authenticated)"
            );
            return Err(HttpError::from((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "email_already_registered",
                    "message": "An account with this email already exists. \
                Sign in with your original method, then link this provider from account settings."
                })),
            )));
        }
    }

    // 3. Create new user + identity (email free, or provider sent none).
    let unique_username = ensure_unique_username(db, &profile.username).await?;
    let provider_label = if slug == "github" { "github" } else { "oidc" };

    let insert = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO users (username, display_name, email, avatar_url, \
                                 is_admin, auth_provider, github_id, github_profile_url, \
                                 last_login_at, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, false, $5, $6, $7, NOW(), NOW(), NOW()) \
             RETURNING id",
            vec![
                SeaValue::String(Some(unique_username.clone())),
                SeaValue::String(Some(profile.username.clone())),
                profile
                    .email
                    .clone()
                    .map(|s| SeaValue::String(Some(s)))
                    .unwrap_or(SeaValue::String(None)),
                profile
                    .avatar_url
                    .clone()
                    .map(|s| SeaValue::String(Some(s)))
                    .unwrap_or(SeaValue::String(None)),
                SeaValue::String(Some(provider_label.to_string())),
                // 兼容层：GitHub 时写 github_id 镜像
                if slug == "github" {
                    profile
                        .provider_user_id
                        .parse::<i64>()
                        .ok()
                        .map(|n| SeaValue::BigInt(Some(n)))
                        .unwrap_or(SeaValue::BigInt(None))
                } else {
                    SeaValue::BigInt(None)
                },
                if slug == "github" {
                    profile
                        .profile_url
                        .clone()
                        .map(|s| SeaValue::String(Some(s)))
                        .unwrap_or(SeaValue::String(None))
                } else {
                    SeaValue::String(None)
                },
            ],
        ))
        .await
        .map_err(|e| {
            // Unique / constraint races on email or username → clear conflict, not 500.
            let msg = e.to_string();
            if msg.contains("unique") || msg.contains("duplicate") || msg.contains("Unique") {
                tracing::warn!(error = %e, "OAuth: INSERT user conflict");
                return HttpError::from((
                    StatusCode::CONFLICT,
                    Json(json!({
                        "error": "email_already_registered",
                        "message": "An account with this email already exists. \
Sign in with your original method, then link this provider from account settings."
                    })),
                ));
            }
            tracing::error!(error = %e, "OAuth: INSERT user failed");
            err_500("Database error")
        })?
        .ok_or_else(|| err_500("INSERT user returned no row"))?;

    let new_id: i32 = insert
        .try_get("", "id")
        .map_err(|_| err_500("Failed to read new user id"))?;

    upsert_identity(db, slug, new_id, profile).await?;
    Ok(new_id)
}

/// 写 / 更新一条 identity（idempotent），并把它标 primary（首条）
async fn upsert_identity(
    db: &DatabaseConnection,
    slug: &str,
    user_id: i32,
    profile: &NormalizedProfile,
) -> Result<(), HttpError> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO user_identities ( \
            user_id, provider, provider_user_id, provider_username, email, email_verified, \
            avatar_url, profile_url, raw_profile, is_primary, linked_at, last_login_at \
         ) VALUES ( \
            $1, $2, $3, $4, $5, $6, $7, $8, $9, \
            (NOT EXISTS (SELECT 1 FROM user_identities WHERE user_id = $1)), \
            NOW(), NOW() \
         ) \
         ON CONFLICT (provider, provider_user_id) DO UPDATE SET \
            provider_username = EXCLUDED.provider_username, \
            email = EXCLUDED.email, \
            email_verified = EXCLUDED.email_verified, \
            avatar_url = EXCLUDED.avatar_url, \
            profile_url = EXCLUDED.profile_url, \
            raw_profile = EXCLUDED.raw_profile, \
            last_login_at = NOW() \
         WHERE user_identities.user_id = EXCLUDED.user_id",
        vec![
            SeaValue::Int(Some(user_id)),
            SeaValue::String(Some(slug.to_string())),
            SeaValue::String(Some(profile.provider_user_id.clone())),
            SeaValue::String(Some(profile.username.clone())),
            profile
                .email
                .clone()
                .map(|s| SeaValue::String(Some(s)))
                .unwrap_or(SeaValue::String(None)),
            SeaValue::Bool(Some(profile.email_verified)),
            profile
                .avatar_url
                .clone()
                .map(|s| SeaValue::String(Some(s)))
                .unwrap_or(SeaValue::String(None)),
            profile
                .profile_url
                .clone()
                .map(|s| SeaValue::String(Some(s)))
                .unwrap_or(SeaValue::String(None)),
            SeaValue::Json(Some(Box::new(profile.raw.clone()))),
        ],
    ))
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "OAuth: upsert identity failed");
        err_500("Database error")
    })?;
    Ok(())
}

/// 防止 username 冲突：若已存在，追加 `_<n>` 后缀
///
/// MYR-036: one range scan for `base` / `base_*` instead of up to 100 point probes.
async fn ensure_unique_username(db: &DatabaseConnection, base: &str) -> Result<String, HttpError> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT username FROM users \
             WHERE LOWER(username) = LOWER($1) \
                OR LOWER(username) LIKE LOWER($1) || '\\_%' ESCAPE '\\' \
             LIMIT 200",
            vec![SeaValue::String(Some(base.to_string()))],
        ))
        .await
        .map_err(|e| {
            tracing::error!("OAuth DB error: {e}");
            err_500("Database error")
        })?;

    let mut taken = std::collections::HashSet::with_capacity(rows.len());
    for row in rows {
        if let Ok(name) = row.try_get::<String>("", "username") {
            taken.insert(name.to_ascii_lowercase());
        }
    }

    let base_lower = base.to_ascii_lowercase();
    if !taken.contains(&base_lower) {
        return Ok(base.to_string());
    }
    for n in 2..=100u32 {
        let candidate = format!("{base}_{n}");
        if !taken.contains(&candidate.to_ascii_lowercase()) {
            return Ok(candidate);
        }
    }
    Err(err_500(
        "Failed to generate unique username after 100 tries",
    ))
}

// DELETE /api/auth/oauth/:slug/unlink/:identity_id

pub async fn provider_unlink(
    Path((slug, identity_id)): Path<(String, i32)>,
    crate::extract::Db(db): crate::extract::Db,
    headers: HeaderMap,
) -> Result<Json<Value>, HttpError> {
    let claims = crate::middleware::auth::authenticate_request(&headers, &db)
        .await
        .map_err(|_| {
            HttpError::from((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Unauthorized"})),
            ))
        })?;
    let user_id: i32 = claims.sub.parse().map_err(|_| err_400("Invalid user id"))?;

    // 确认 identity 属于当前用户
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM user_identities WHERE id = $1 AND user_id = $2 AND provider = $3",
            vec![
                SeaValue::Int(Some(identity_id)),
                SeaValue::Int(Some(user_id)),
                SeaValue::String(Some(slug.clone())),
            ],
        ))
        .await
        .map_err(|e| {
            tracing::error!("OAuth DB error: {e}");
            err_500("Database error")
        })?;

    if row.is_none() {
        return Err(err_404("identity not found or not yours"));
    }

    // 防失联：若此 identity 是唯一登录方式（没密码 + 只有这一条 identity），拒绝
    let summary = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT \
                (SELECT password_hash IS NOT NULL FROM users WHERE id = $1) AS has_password, \
                (SELECT COUNT(*) FROM user_identities WHERE user_id = $1) AS identity_count",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|e| {
            tracing::error!("OAuth DB error: {e}");
            err_500("Database error")
        })?
        .ok_or_else(|| err_500("user not found"))?;

    let has_password: bool = summary.try_get("", "has_password").unwrap_or(false);
    let identity_count: i64 = summary.try_get("", "identity_count").unwrap_or(0);

    if !has_password && identity_count <= 1 {
        return Err(HttpError::from((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Cannot unlink last identity",
                "message": "Please set a local password first, or link another provider."
            })),
        )));
    }

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM user_identities WHERE id = $1",
        vec![SeaValue::Int(Some(identity_id))],
    ))
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "OAuth: DELETE identity failed");
        err_500("Database error")
    })?;

    // 兼容层：解绑 GitHub 时清掉 users.linked_github_id（若仍持有该 id）
    if slug == "github" {
        let _ = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE users SET linked_github_id = NULL WHERE id = $1",
                vec![SeaValue::Int(Some(user_id))],
            ))
            .await;
    }

    Ok(Json(json!({"success": true})))
}

// GET /api/auth/identities

pub async fn list_my_identities(
    crate::extract::Db(db): crate::extract::Db,
    headers: HeaderMap,
) -> Result<Json<Value>, HttpError> {
    let claims = crate::middleware::auth::authenticate_request(&headers, &db)
        .await
        .map_err(|_| {
            HttpError::from((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Unauthorized"})),
            ))
        })?;
    let user_id: i32 = claims.sub.parse().map_err(|_| err_400("Invalid user id"))?;

    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, provider, provider_username, email, avatar_url, profile_url, \
                    is_primary, linked_at, last_login_at \
             FROM user_identities WHERE user_id = $1 ORDER BY linked_at ASC",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|e| {
            tracing::error!("OAuth DB error: {e}");
            err_500("Database error")
        })?;

    let identities: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.try_get::<i32>("", "id").unwrap_or(0),
                "provider": r.try_get::<String>("", "provider").unwrap_or_default(),
                "provider_username": r.try_get::<Option<String>>("", "provider_username").unwrap_or(None),
                "email": r.try_get::<Option<String>>("", "email").unwrap_or(None),
                // 画像源选择器直接把它当 <img src>：不代理则 hdslb 等防盗链 CDN 裂图
                "avatar_url": crate::services::avatar::proxied_avatar_value(
                    r.try_get::<Option<String>>("", "avatar_url").unwrap_or(None),
                ),
                "profile_url": r.try_get::<Option<String>>("", "profile_url").unwrap_or(None),
                "is_primary": r.try_get::<bool>("", "is_primary").unwrap_or(false),
                "linked_at": r.try_get::<chrono::DateTime<chrono::Utc>>("", "linked_at").ok().map(|t| t.to_rfc3339()),
                "last_login_at": r.try_get::<chrono::DateTime<chrono::Utc>>("", "last_login_at").ok().map(|t| t.to_rfc3339()),
            })
        })
        .collect();

    Ok(Json(json!({ "identities": identities })))
}

// POST /api/auth/identities/{identity_id}/primary
//
// 兼容别名：等价于 PUT /api/users/me/avatar-source {kind:"identity", ref:<id>}。
// 画像源的唯一写入处是 services::avatar::set_avatar_source（它一并维护
// is_primary 与 avatar_resolved_url 快照），这里只做 GitHub 账号联结的补写。
//
// 已移除的旧副作用：**不再覆盖 users.display_name**。改画像源是选头像，
// 顺手改掉展示名属于两件事绑一起，用户切个头像却发现名字变了。
// 同样不再覆盖 users.avatar_url —— 那是"账号"这一来源本身，覆盖后就切不回来了。

pub async fn set_primary_identity(
    Path(identity_id): Path<i32>,
    crate::extract::Db(db): crate::extract::Db,
    headers: HeaderMap,
) -> Result<Json<Value>, HttpError> {
    let claims = crate::middleware::auth::authenticate_request(&headers, &db)
        .await
        .map_err(|_| {
            HttpError::from((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Unauthorized"})),
            ))
        })?;
    let user_id: i32 = claims.sub.parse().map_err(|_| err_400("Invalid user id"))?;

    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT provider, provider_username, provider_user_id \
             FROM user_identities WHERE id = $1 AND user_id = $2",
            vec![
                SeaValue::Int(Some(identity_id)),
                SeaValue::Int(Some(user_id)),
            ],
        ))
        .await
        .map_err(|e| {
            tracing::error!("OAuth DB error: {e}");
            err_500("Database error")
        })?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Identity not found"})),
            ))
        })?;

    let provider: String = row.try_get("", "provider").unwrap_or_default();
    let provider_username: Option<String> = row.try_get("", "provider_username").unwrap_or(None);
    let provider_user_id: String = row.try_get("", "provider_user_id").unwrap_or_default();

    // GitHub 账号联结（仅在为空时补写）+ avatar_source + is_primary 同事务。
    let linked_github_id = if provider == "github" {
        provider_user_id.parse::<i64>().ok()
    } else {
        None
    };

    let avatar_url = crate::services::avatar::set_primary_identity_source(
        &db,
        user_id,
        identity_id,
        linked_github_id,
    )
    .await
    .map_err(|message| {
        HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({"success": false, "message": message})),
        ))
    })?;

    Ok(Json(json!({
        "success": true,
        "identity_id": identity_id,
        "provider": provider,
        "provider_username": provider_username,
        "avatar_url": avatar_url,
    })))
}
