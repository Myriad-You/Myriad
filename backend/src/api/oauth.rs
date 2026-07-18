//! 通用 OAuth handler（routing by `:slug`）
//!
//! 详见 docs/oauth-refactor-plan.md §7.1
//!
//! 端点：
//!   GET    /api/auth/oauth/providers              列出 enabled providers
//!   GET    /api/auth/oauth/:slug/login            重定向到授权页
//!   GET    /api/auth/oauth/:slug/callback         交换 code + 登录/创建用户
//!   GET    /api/auth/oauth/:slug/link             绑定 (需 JWT + is_admin)
//!   DELETE /api/auth/oauth/:slug/unlink/:id       解绑
//!   GET    /api/auth/identities                   当前用户所有 identities

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{encode, EncodingKey, Header};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, Value as SeaValue};
use serde::Deserialize;
use serde_json::{json, Value};
use std::env;

use crate::middleware::auth::Claims;
use crate::oauth_url_builder::SiteConfig;
use crate::services::oauth::{
    registry::REGISTRY,
    state::{consume_state, issue_state, OAuthPurpose, StoredState},
    NormalizedProfile,
};

// ---------- 工具函数 ----------

async fn build_redirect_uri(slug: &str) -> String {
    let base = SiteConfig::get_base_url().await;
    format!("{}/api/auth/oauth/{}/callback", base, slug)
}

fn err_500(msg: impl Into<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": msg.into()})),
    )
}
fn err_400(msg: impl Into<String>) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({"error": msg.into()})))
}
fn err_404(msg: impl Into<String>) -> (StatusCode, Json<Value>) {
    (StatusCode::NOT_FOUND, Json(json!({"error": msg.into()})))
}

/// Attach anti-caching headers so OAuth redirects / callbacks are never stored by browsers or CDNs.
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

async fn sync_user_oauth_profile_snapshot(
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
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE users SET linked_github_id = COALESCE($1, linked_github_id), \
                 avatar_url = COALESCE($2, avatar_url), updated_at = NOW() WHERE id = $3",
                vec![
                    SeaValue::BigInt(github_id),
                    avatar_url
                        .map(|s| SeaValue::String(Some(Box::new(s))))
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
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE users SET avatar_url = $1, updated_at = NOW() WHERE id = $2",
            vec![
                SeaValue::String(Some(Box::new(avatar_url))),
                SeaValue::Int(Some(user_id)),
            ],
        ))
        .await
    {
        tracing::warn!("Failed to sync OAuth avatar to user: {}", e);
    }
}

// ---------- GET /api/auth/oauth/providers ----------

pub async fn list_providers() -> Json<Value> {
    let providers = REGISTRY.list().await;
    Json(json!({ "providers": providers }))
}

// ---------- GET /api/auth/oauth/:slug/login ----------

pub async fn provider_login(
    Path(slug): Path<String>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let provider = REGISTRY
        .get(&slug)
        .await
        .ok_or_else(|| err_404(format!("OAuth provider '{slug}' not configured")))?;

    let state = issue_state(StoredState {
        provider_slug: slug.clone(),
        purpose: OAuthPurpose::Login,
    })
    .await
    .map_err(err_500)?;

    let redirect_uri = build_redirect_uri(&slug).await;
    let auth_url = provider
        .build_auth_url(&state, &redirect_uri)
        .await
        .map_err(err_500)?;

    Ok(no_store_redirect(&auth_url))
}

// ---------- GET /api/auth/oauth/:slug/link  (任何已登录用户) ----------

pub async fn provider_link(
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<Value>)> {
    use crate::middleware::auth::verify_jwt_token;

    let claims = verify_jwt_token(&headers).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Unauthorized"})),
        )
    })?;

    let user_id: i32 = claims.sub.parse().map_err(|_| err_400("Invalid user id"))?;
    if user_id <= 0 {
        return Err(err_400("Guest sessions cannot link OAuth providers"));
    }

    let provider = REGISTRY
        .get(&slug)
        .await
        .ok_or_else(|| err_404(format!("OAuth provider '{slug}' not configured")))?;

    let state = issue_state(StoredState {
        provider_slug: slug.clone(),
        purpose: OAuthPurpose::LinkAccount(user_id),
    })
    .await
    .map_err(err_500)?;

    let redirect_uri = build_redirect_uri(&slug).await;
    let auth_url = provider
        .build_auth_url(&state, &redirect_uri)
        .await
        .map_err(err_500)?;

    Ok(no_store_redirect(&auth_url))
}

// ---------- GET /api/auth/oauth/:slug/callback ----------

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
    State(db): State<DatabaseConnection>,
) -> Result<Response, (StatusCode, Json<Value>)> {
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
        return Ok(no_store_redirect(&url));
    }

    let Some(code) = params.code.filter(|c| !c.trim().is_empty()) else {
        return Ok(oauth_client_error_redirect(&frontend_base, "missing_code"));
    };
    let Some(state_param) = params.state.filter(|s| !s.trim().is_empty()) else {
        return Ok(oauth_client_error_redirect(&frontend_base, "missing_state"));
    };

    // 1. 验证 state（区分 missing / expired，已在 store 层 warn-log）
    let stored = match consume_state(&state_param).await {
        Ok(s) => s,
        Err(err) => {
            return Ok(oauth_client_error_redirect(
                &frontend_base,
                &format!("state_{}", err.as_str()),
            ));
        }
    };

    if stored.provider_slug != slug {
        tracing::warn!(
            "OAuth state/slug mismatch: state was for '{}', got '{}'",
            stored.provider_slug,
            slug
        );
        return Ok(oauth_client_error_redirect(
            &frontend_base,
            "state_slug_mismatch",
        ));
    }

    // 2. 取 provider — browser-friendly redirect (not opaque JSON 500/404)
    let provider = match REGISTRY.get(&slug).await {
        Some(p) => p,
        None => {
            tracing::error!("OAuth provider '{}' missing at callback", slug);
            return Ok(oauth_client_error_redirect(
                &frontend_base,
                "provider_unavailable",
            ));
        }
    };

    // 3. exchange + fetch profile — redirect with stable codes instead of raw 500
    let redirect_uri = build_redirect_uri(&slug).await;
    let tokens = match provider.exchange_code(&code, &redirect_uri).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("OAuth token exchange failed for '{}': {}", slug, e);
            return Ok(oauth_client_error_redirect(
                &frontend_base,
                "token_exchange_failed",
            ));
        }
    };
    let profile = match provider.fetch_profile(&tokens).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("OAuth profile fetch failed for '{}': {}", slug, e);
            return Ok(oauth_client_error_redirect(
                &frontend_base,
                "profile_fetch_failed",
            ));
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
    match stored.purpose {
        OAuthPurpose::LinkAccount(link_user_id) => {
            handle_link(&db, &slug, link_user_id, &profile, &frontend_base).await
        }
        OAuthPurpose::Login => match handle_login(&db, &slug, &profile).await {
            Ok(resp) => Ok(resp),
            Err((status, body)) => {
                tracing::error!(
                    "OAuth login failed for provider '{}' (status={}): {:?}",
                    slug,
                    status,
                    body
                );
                // Prefer a stable browser redirect over opaque JSON 5xx for callbacks
                Ok(oauth_client_error_redirect(&frontend_base, "login_failed"))
            }
        },
        OAuthPurpose::PlatformData { platform, .. } => {
            tracing::warn!(
                "PlatformData OAuth state for '{}' hit login callback; redirecting",
                platform
            );
            let url = format!(
                "{}/config?discord_oauth=error&reason={}",
                frontend_base.trim_end_matches('/'),
                urlencoding::encode("wrong_callback")
            );
            Ok(no_store_redirect(&url))
        }
    }
}

// ---------- 内部：LinkAccount 流程 ----------

async fn handle_link(
    db: &DatabaseConnection,
    slug: &str,
    link_user_id: i32,
    profile: &NormalizedProfile,
    frontend_url: &str,
) -> Result<Response, (StatusCode, Json<Value>)> {
    // 验证发起绑定的用户仍然存在
    let user = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM users WHERE id = $1",
            vec![SeaValue::Int(Some(link_user_id))],
        ))
        .await
        .map_err(|e| err_500(format!("DB error: {e}")))?;

    if user.is_none() {
        let url = format!(
            "{}/?link=error&reason=user_not_found",
            frontend_url.trim_end_matches('/')
        );
        return Ok(no_store_redirect(&url));
    }

    // 检查 identity 是否已绑到别的 user
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT user_id FROM user_identities \
             WHERE provider = $1 AND provider_user_id = $2",
            vec![
                SeaValue::String(Some(Box::new(slug.to_string()))),
                SeaValue::String(Some(Box::new(profile.provider_user_id.clone()))),
            ],
        ))
        .await
        .map_err(|e| err_500(format!("DB error: {e}")))?;

    if let Some(row) = existing {
        let owner_id: i32 = row
            .try_get("", "user_id")
            .map_err(|e| err_500(format!("failed to read user_id: {e}")))?;
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

// ---------- 内部：Login 流程 ----------

async fn handle_login(
    db: &DatabaseConnection,
    slug: &str,
    profile: &NormalizedProfile,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let user_id = find_or_create_user(db, slug, profile).await?;

    // 查 is_admin
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT is_admin, username FROM users WHERE id = $1",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|e| err_500(format!("DB error: {e}")))?
        .ok_or_else(|| err_500("User vanished after create"))?;

    let is_admin: bool = row.try_get("", "is_admin").unwrap_or(false);
    let username: String = row.try_get("", "username").unwrap_or_default();

    // 生成 JWT
    let jwt_secret = env::var("JWT_SECRET").map_err(|_| err_500("JWT_SECRET not set"))?;
    let claims = Claims {
        sub: user_id.to_string(),
        username: username.clone(),
        is_admin,
        exp: (Utc::now() + Duration::days(30)).timestamp(),
        iat: Utc::now().timestamp(),
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_bytes()),
    )
    .map_err(|e| err_500(format!("JWT encode failed: {e}")))?;

    // Set-Cookie + HTML 重定向
    // 注意：使用相对路径，避免把管理员可控的 base_url 注入到 <meta refresh> / <script>
    let is_production = SiteConfig::is_production().await;
    let cookie_value = format!(
        "auth_token={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000{}",
        token,
        if is_production { "; Secure" } else { "" }
    );
    let html = "<!DOCTYPE html><html><head><meta charset=\"utf-8\">\
        <meta http-equiv=\"refresh\" content=\"0;url=/?auth=success\"><title>Login</title></head>\
        <body><p>Login successful, redirecting...</p>\
        <script>window.location.href=\"/?auth=success\";</script></body></html>";

    let mut response = axum::response::Html(html).into_response();
    let set_cookie = HeaderValue::from_str(&cookie_value)
        .map_err(|e| err_500(format!("invalid auth cookie header: {e}")))?;
    response
        .headers_mut()
        .insert(header::SET_COOKIE, set_cookie);
    apply_no_store_headers(&mut response);
    Ok(response)
}

// ---------- 内部：账户匹配策略 ----------

async fn find_or_create_user(
    db: &DatabaseConnection,
    slug: &str,
    profile: &NormalizedProfile,
) -> Result<i32, (StatusCode, Json<Value>)> {
    // 1. identity 命中 → 直接登录
    if let Some(row) = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT user_id FROM user_identities \
             WHERE provider = $1 AND provider_user_id = $2",
            vec![
                SeaValue::String(Some(Box::new(slug.to_string()))),
                SeaValue::String(Some(Box::new(profile.provider_user_id.clone()))),
            ],
        ))
        .await
        .map_err(|e| err_500(format!("DB error: {e}")))?
    {
        let uid: i32 = row
            .try_get("", "user_id")
            .map_err(|e| err_500(format!("failed to read user_id: {e}")))?;
        // 更新 identity 的 last_login_at + 档案字段
        let _ = db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE user_identities SET \
                    provider_username = $1, email = $2, avatar_url = $3, profile_url = $4, \
                    raw_profile = $5, last_login_at = NOW() \
                 WHERE provider = $6 AND provider_user_id = $7",
                vec![
                    SeaValue::String(Some(Box::new(profile.username.clone()))),
                    profile
                        .email
                        .clone()
                        .map(|s| SeaValue::String(Some(Box::new(s))))
                        .unwrap_or(SeaValue::String(None)),
                    profile
                        .avatar_url
                        .clone()
                        .map(|s| SeaValue::String(Some(Box::new(s))))
                        .unwrap_or(SeaValue::String(None)),
                    profile
                        .profile_url
                        .clone()
                        .map(|s| SeaValue::String(Some(Box::new(s))))
                        .unwrap_or(SeaValue::String(None)),
                    SeaValue::Json(Some(Box::new(profile.raw.clone()))),
                    SeaValue::String(Some(Box::new(slug.to_string()))),
                    SeaValue::String(Some(Box::new(profile.provider_user_id.clone()))),
                ],
            ))
            .await;
        sync_user_oauth_profile_snapshot(db, uid, slug, profile).await;
        // 更新 users 的 last_login_at
        let _ = db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE users SET last_login_at = NOW() WHERE id = $1",
                vec![SeaValue::Int(Some(uid))],
            ))
            .await;
        return Ok(uid);
    }

    // 2. email 自动合并（仅当 verified）
    if profile.email_verified {
        if let Some(email) = profile.email.as_ref() {
            if let Some(row) = db
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT id FROM users WHERE LOWER(email) = LOWER($1) LIMIT 1",
                    vec![SeaValue::String(Some(Box::new(email.clone())))],
                ))
                .await
                .map_err(|e| err_500(format!("DB error: {e}")))?
            {
                let uid: i32 = row
                    .try_get("", "id")
                    .map_err(|e| err_500(format!("failed to read id: {e}")))?;
                upsert_identity(db, slug, uid, profile).await?;
                sync_user_oauth_profile_snapshot(db, uid, slug, profile).await;
                return Ok(uid);
            }
        }
    }

    // 3. 创建新 user + identity
    let unique_username = ensure_unique_username(db, &profile.username).await?;
    let provider_label = if slug == "github" { "github" } else { "oidc" };

    let insert = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO users (username, display_name, email, avatar_url, \
                                 is_admin, auth_provider, github_id, github_profile_url, \
                                 last_login_at, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, false, $5, $6, $7, NOW(), NOW(), NOW()) \
             RETURNING id",
            vec![
                SeaValue::String(Some(Box::new(unique_username.clone()))),
                SeaValue::String(Some(Box::new(profile.username.clone()))),
                profile
                    .email
                    .clone()
                    .map(|s| SeaValue::String(Some(Box::new(s))))
                    .unwrap_or(SeaValue::String(None)),
                profile
                    .avatar_url
                    .clone()
                    .map(|s| SeaValue::String(Some(Box::new(s))))
                    .unwrap_or(SeaValue::String(None)),
                SeaValue::String(Some(Box::new(provider_label.to_string()))),
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
                        .map(|s| SeaValue::String(Some(Box::new(s))))
                        .unwrap_or(SeaValue::String(None))
                } else {
                    SeaValue::String(None)
                },
            ],
        ))
        .await
        .map_err(|e| err_500(format!("INSERT user failed: {e}")))?
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
) -> Result<(), (StatusCode, Json<Value>)> {
    db.execute(Statement::from_sql_and_values(
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
            SeaValue::String(Some(Box::new(slug.to_string()))),
            SeaValue::String(Some(Box::new(profile.provider_user_id.clone()))),
            SeaValue::String(Some(Box::new(profile.username.clone()))),
            profile
                .email
                .clone()
                .map(|s| SeaValue::String(Some(Box::new(s))))
                .unwrap_or(SeaValue::String(None)),
            SeaValue::Bool(Some(profile.email_verified)),
            profile
                .avatar_url
                .clone()
                .map(|s| SeaValue::String(Some(Box::new(s))))
                .unwrap_or(SeaValue::String(None)),
            profile
                .profile_url
                .clone()
                .map(|s| SeaValue::String(Some(Box::new(s))))
                .unwrap_or(SeaValue::String(None)),
            SeaValue::Json(Some(Box::new(profile.raw.clone()))),
        ],
    ))
    .await
    .map_err(|e| err_500(format!("upsert identity failed: {e}")))?;
    Ok(())
}

/// 防止 username 冲突：若已存在，追加 `_<n>` 后缀
async fn ensure_unique_username(
    db: &DatabaseConnection,
    base: &str,
) -> Result<String, (StatusCode, Json<Value>)> {
    let mut candidate = base.to_string();
    for n in 0..100u32 {
        let hit = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT 1 FROM users WHERE LOWER(username) = LOWER($1) LIMIT 1",
                vec![SeaValue::String(Some(Box::new(candidate.clone())))],
            ))
            .await
            .map_err(|e| err_500(format!("DB error: {e}")))?;
        if hit.is_none() {
            return Ok(candidate);
        }
        candidate = format!("{}_{}", base, n + 2);
    }
    Err(err_500(
        "Failed to generate unique username after 100 tries",
    ))
}

// ---------- DELETE /api/auth/oauth/:slug/unlink/:identity_id ----------

pub async fn provider_unlink(
    Path((slug, identity_id)): Path<(String, i32)>,
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    use crate::middleware::auth::verify_jwt_token;
    let claims = verify_jwt_token(&headers).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Unauthorized"})),
        )
    })?;
    let user_id: i32 = claims.sub.parse().map_err(|_| err_400("Invalid user id"))?;

    // 确认 identity 属于当前用户
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM user_identities WHERE id = $1 AND user_id = $2 AND provider = $3",
            vec![
                SeaValue::Int(Some(identity_id)),
                SeaValue::Int(Some(user_id)),
                SeaValue::String(Some(Box::new(slug.clone()))),
            ],
        ))
        .await
        .map_err(|e| err_500(format!("DB error: {e}")))?;

    if row.is_none() {
        return Err(err_404("identity not found or not yours"));
    }

    // 防失联：若此 identity 是唯一登录方式（没密码 + 只有这一条 identity），拒绝
    let summary = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT \
                (SELECT password_hash IS NOT NULL FROM users WHERE id = $1) AS has_password, \
                (SELECT COUNT(*) FROM user_identities WHERE user_id = $1) AS identity_count",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|e| err_500(format!("DB error: {e}")))?
        .ok_or_else(|| err_500("user not found"))?;

    let has_password: bool = summary.try_get("", "has_password").unwrap_or(false);
    let identity_count: i64 = summary.try_get("", "identity_count").unwrap_or(0);

    if !has_password && identity_count <= 1 {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Cannot unlink last identity",
                "message": "Please set a local password first, or link another provider."
            })),
        ));
    }

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM user_identities WHERE id = $1",
        vec![SeaValue::Int(Some(identity_id))],
    ))
    .await
    .map_err(|e| err_500(format!("DELETE failed: {e}")))?;

    // 兼容层：解绑 GitHub 时清掉 users.linked_github_id（若仍持有该 id）
    if slug == "github" {
        let _ = db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE users SET linked_github_id = NULL WHERE id = $1",
                vec![SeaValue::Int(Some(user_id))],
            ))
            .await;
    }

    Ok(Json(json!({"success": true})))
}

// ---------- GET /api/auth/identities ----------

pub async fn list_my_identities(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    use crate::middleware::auth::verify_jwt_token;
    let claims = verify_jwt_token(&headers).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Unauthorized"})),
        )
    })?;
    let user_id: i32 = claims.sub.parse().map_err(|_| err_400("Invalid user id"))?;

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, provider, provider_username, email, avatar_url, profile_url, \
                    is_primary, linked_at, last_login_at \
             FROM user_identities WHERE user_id = $1 ORDER BY linked_at ASC",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|e| err_500(format!("DB error: {e}")))?;

    let identities: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.try_get::<i32>("", "id").unwrap_or(0),
                "provider": r.try_get::<String>("", "provider").unwrap_or_default(),
                "provider_username": r.try_get::<Option<String>>("", "provider_username").unwrap_or(None),
                "email": r.try_get::<Option<String>>("", "email").unwrap_or(None),
                "avatar_url": r.try_get::<Option<String>>("", "avatar_url").unwrap_or(None),
                "profile_url": r.try_get::<Option<String>>("", "profile_url").unwrap_or(None),
                "is_primary": r.try_get::<bool>("", "is_primary").unwrap_or(false),
                "linked_at": r.try_get::<chrono::DateTime<chrono::Utc>>("", "linked_at").ok().map(|t| t.to_rfc3339()),
                "last_login_at": r.try_get::<chrono::DateTime<chrono::Utc>>("", "last_login_at").ok().map(|t| t.to_rfc3339()),
            })
        })
        .collect();

    Ok(Json(json!({ "identities": identities })))
}
