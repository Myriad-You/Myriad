//! Core auth endpoints that are not provider-specific.
//!
//! GitHub/OIDC login is handled by `api::oauth` via `/api/auth/oauth/:slug/*`.

use crate::error::HttpError;
use axum::{
    Json,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use myriad_error::AppError;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::{Value, json};

// Re-export `Claims`.
use crate::middleware::auth::{
    CredentialSource, SessionCredential, authenticate_optional_request, clear_auth_cookie_value,
};

/// Guest body for the session probe. HTTP 200 — never 401 — so browsers do not
/// paint Network red for expected unauthenticated state.
pub fn unauthenticated_me_body() -> Value {
    json!({ "authenticated": false })
}

async fn unauthenticated_me_response(clear_cookie: bool) -> Response {
    let mut response = Json(unauthenticated_me_body()).into_response();
    if clear_cookie {
        let is_production = crate::oauth_url_builder::SiteConfig::is_production().await;
        if let Ok(value) = HeaderValue::from_str(&clear_auth_cookie_value(is_production)) {
            response.headers_mut().insert(header::SET_COOKIE, value);
        }
    }
    response
}

/// `GET /api/auth/me` — session probe + current user profile.
///
/// **Contract (durable guest UX):** missing/invalid token or unknown user →
/// **HTTP 200** `{ "authenticated": false }`. Real server faults stay 5xx.
/// This is intentionally a probe, not a hard auth gate — protected mutating
/// routes keep their own 401/403 checks.
pub async fn get_current_user(
    crate::extract::Db(db): crate::extract::Db,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    // Same credential selection, verification, session epoch and auth cache
    // as every protected route; only the failure shape differs.
    let clear_invalid_cookie = matches!(
        SessionCredential::from_headers(&headers),
        Some(credential) if credential.source == CredentialSource::Cookie
    );
    let claims = match authenticate_optional_request(&headers, &db).await {
        Ok(Some(claims)) => claims,
        Ok(None) => return Ok(unauthenticated_me_response(false).await),
        Err(response) if response.status() == StatusCode::UNAUTHORIZED => {
            // Expired/forged/revoked session — expected guest for this probe, not 401 noise.
            return Ok(unauthenticated_me_response(clear_invalid_cookie).await);
        }
        Err(response) => return Ok(*response),
    };

    // Signed guest sessions are not accounts.
    let Some(user_id) = claims.durable_user_id() else {
        return Ok(unauthenticated_me_response(clear_invalid_cookie).await);
    };

    use sea_orm::Value as SeaValue;

    // 头像阶梯是 services::avatar 的共享片段（与 /api/tapp/context/user 同一份）
    let user_sql = format!(
        r#"SELECT u.id, u.username, u.auth_provider, u.is_admin,
                  COALESCE(u.is_owner, false) AS is_owner,
                  {avatar} AS avatar_url,
                  u.github_id, u.linked_github_id, u.bio, u.display_name,
                  u.password_hash IS NOT NULL AS has_password,
                  u.last_login_at,
                  u.locale
           FROM users u
           WHERE u.id = $1"#,
        avatar = crate::services::avatar::avatar_snapshot_expr("u"),
    );

    let user_row = match db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            user_sql,
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
    {
        Ok(row) => row,
        Err(error) => {
            tracing::error!(%error, "failed to load current user");
            return Err(HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::public_json("Failed to load current user")),
            )));
        }
    };

    let Some(user_row) = user_row else {
        // Token valid but user gone — still a session-probe miss, not auth gate.
        return Ok(unauthenticated_me_response(clear_invalid_cookie).await);
    };

    // identities 列表（用 user_identities 表）
    let identity_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "SELECT id, provider, provider_username, is_primary, linked_at \
                 FROM user_identities WHERE user_id = $1 \
                    AND {} \
                 ORDER BY linked_at ASC",
                crate::services::channel_pairing::SQL_NOT_PAIRING_PROVIDER
            ),
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .unwrap_or_default();

    let identities: Vec<Value> = identity_rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.try_get::<i32>("", "id").unwrap_or(0),
                "provider": r.try_get::<String>("", "provider").unwrap_or_default(),
                "provider_username": r.try_get::<Option<String>>("", "provider_username").unwrap_or(None),
                "is_primary": r.try_get::<bool>("", "is_primary").unwrap_or(false),
                "linked_at": r.try_get::<chrono::DateTime<chrono::Utc>>("", "linked_at").ok().map(|t| t.to_rfc3339()),
            })
        })
        .collect();

    let id: i32 = match user_row.try_get::<i32>("", "id") {
        Ok(id) if id > 0 => id,
        Ok(_) | Err(_) => {
            tracing::error!("current user row is missing a positive id");
            return Err(HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::public_json("Failed to load current user")),
            )));
        }
    };
    let username: String = match user_row.try_get::<String>("", "username") {
        Ok(username) if !username.is_empty() => username,
        Ok(_) | Err(_) => {
            tracing::error!("current user row is missing a username");
            return Err(HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::public_json("Failed to load current user")),
            )));
        }
    };
    let auth_provider: String = user_row
        .try_get("", "auth_provider")
        .unwrap_or_else(|_| "local".to_string());
    let is_admin: bool = user_row.try_get("", "is_admin").unwrap_or(false);
    let is_owner: bool = user_row.try_get("", "is_owner").unwrap_or(false);
    // 无头像返回 null（不再编造 ghost.png）：前端 <Avatar> 负责生成本地兜底，
    // 后端编造会让"没有头像"和"头像就是这张"无法区分。
    let avatar_url = crate::services::avatar::proxied_avatar_value(
        user_row
            .try_get::<Option<String>>("", "avatar_url")
            .ok()
            .flatten(),
    );
    let github_id: Option<i64> = user_row.try_get("", "github_id").ok();
    let linked_github_id: Option<i64> = user_row.try_get("", "linked_github_id").ok();
    let bio: Option<String> = user_row.try_get("", "bio").ok();
    let display_name: Option<String> = user_row.try_get("", "display_name").ok();
    let has_password: bool = user_row.try_get("", "has_password").unwrap_or(false);
    let last_login_at: Option<String> = user_row
        .try_get::<Option<chrono::DateTime<chrono::Utc>>>("", "last_login_at")
        .ok()
        .flatten()
        .map(|t| t.to_rfc3339());
    let locale = user_row
        .try_get::<Option<String>>("", "locale")
        .ok()
        .flatten()
        .as_deref()
        .and_then(crate::api::reports::locale::parse_stored_ui_locale);

    Ok(Json(json!({
        "authenticated": true,
        "id": id,
        "username": username,
        "display_name": display_name.filter(|s| !s.trim().is_empty()).unwrap_or(username.clone()),
        "auth_provider": auth_provider,
        "is_admin": is_admin,
        "is_owner": is_owner,
        "avatar_url": avatar_url,
        "github_id": github_id,
        "linked_github_id": linked_github_id.map(|id| id.to_string()),
        "bio": bio,
        "has_password": has_password,
        "last_login_at": last_login_at,
        "locale": locale,
        "identities": identities,
    }))
    .into_response())
}

/// `PUT /api/auth/me/locale` — durable users only; guests stay on localStorage.
///
/// Route sits behind `auth_middleware`; the subject comes from its claims.
pub async fn set_current_user_locale(
    crate::extract::Db(db): crate::extract::Db,
    crate::extract::AuthedClaims(claims): crate::extract::AuthedClaims,
    Json(payload): Json<Value>,
) -> Result<impl IntoResponse, HttpError> {
    let Some(user_id) = claims.subject_id().filter(|id| *id >= 0) else {
        return Err(HttpError::from((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Forbidden",
                "code": "GUEST_LOCALE_READONLY",
            })),
        )));
    };

    let raw = payload
        .get("locale")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let Some(locale) = crate::api::reports::locale::parse_stored_ui_locale(raw) else {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Bad request",
                "code": "locale_invalid",
                "message": "locale must be a host UI tag",
            })),
        )));
    };

    let updated = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE users SET locale = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2",
            vec![locale.to_string().into(), user_id.into()],
        ))
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to save user locale");
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::public_json("Failed to update user")),
            ))
        })?;

    if updated.rows_affected() == 0 {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Not found")),
        )));
    }

    Ok(Json(json!({ "ok": true, "locale": locale })))
}

/// `POST /api/auth/logout`
///
/// Clears the browser cookie and, when a valid token matches the current
/// session epoch, bumps `users.token_version` so copies of the same JWT fail
/// closed on protected routes until the next login.
pub async fn logout(
    crate::extract::Db(db): crate::extract::Db,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Always clear the cookie; only the current session may revoke itself.
    // The UPDATE checks tv atomically so a stale logout cannot kill a new login.
    match authenticate_optional_request(&headers, &db).await {
        Ok(Some(claims)) => {
            if let Some(user_id) = claims.durable_user_id() {
                match crate::middleware::auth::bump_token_version(&db, user_id, claims.tv).await {
                    Ok(Some(new_tv)) => {
                        tracing::info!(
                            user_id,
                            token_version = new_tv,
                            "🚪 User logout — session epoch bumped"
                        );
                    }
                    Ok(None) => {
                        tracing::debug!(user_id, "🚪 Stale or missing session — cookie clear only");
                    }
                    Err(e) => {
                        // Cookie still cleared; epoch bump is best-effort so
                        // logout never 500s on a transient DB blip.
                        tracing::warn!(
                            user_id,
                            error = %e,
                            "🚪 Logout: failed to bump token_version (cookie still cleared)"
                        );
                    }
                }
            }
        }
        Ok(None) => {
            tracing::info!("🚪 User logout - clearing auth cookie (no token)");
        }
        Err(response) if response.status() == StatusCode::UNAUTHORIZED => {
            tracing::info!("🚪 User logout - clearing auth cookie (invalid or revoked token)");
        }
        Err(response) => {
            // Cookie still cleared, same as a failed epoch bump.
            tracing::warn!(
                status = %response.status(),
                "🚪 Logout: session could not be verified (cookie still cleared)"
            );
        }
    }

    let is_production = crate::oauth_url_builder::SiteConfig::is_production().await;
    let cookie_value = clear_auth_cookie_value(is_production);
    let mut response =
        Json(json!({"success": true, "message": "Logged out successfully"})).into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie_value) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

#[cfg(test)]
mod auth_me_probe_tests {
    use super::*;

    /// `/me` resolves the session through the shared auth boundary, never
    /// its own credential parse, JWT decode or epoch compare.
    #[test]
    fn current_user_probe_uses_the_shared_auth_boundary() {
        let src = include_str!("auth.rs");
        let probe = src
            .split("pub async fn get_current_user(")
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .expect("get_current_user");
        assert!(probe.contains("authenticate_optional_request(&headers, &db)"));
        assert!(probe.contains("claims.durable_user_id()"));
        for forbidden in [
            "jsonwebtoken",
            "session_epoch_matches",
            "token_version",
            ".sub",
        ] {
            assert!(!probe.contains(forbidden), "/me must not use {forbidden}");
        }
    }

    #[test]
    fn current_user_probe_does_not_decode_id_to_zero() {
        let src = include_str!("auth.rs");
        let body = src
            .split("let id: i32 = match user_row.try_get")
            .nth(1)
            .and_then(|rest| rest.split("let username:").next())
            .expect("current user id decode");
        assert!(body.contains("id > 0"));
        assert!(!body.contains("unwrap_or(0)"));
    }

    #[test]
    fn unauthenticated_me_body_contract() {
        let body = unauthenticated_me_body();
        assert_eq!(body["authenticated"], false);
        assert!(body.get("id").is_none());
        assert!(body.get("error").is_none());
    }

    #[tokio::test]
    async fn unauthenticated_me_can_clear_an_invalid_httponly_cookie() {
        let response = unauthenticated_me_response(true).await;
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .expect("invalid browser session must be cleared");
        assert!(cookie.starts_with("auth_token=deleted;"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("Max-Age=0"));
    }

    #[test]
    fn logout_clear_cookie_matches_issuance_samesite_lax_and_secure_flag() {
        let dev = clear_auth_cookie_value(false);
        assert!(dev.contains("auth_token=deleted"));
        assert!(dev.contains("Path=/"));
        assert!(dev.contains("HttpOnly"));
        assert!(dev.contains("SameSite=Lax"));
        assert!(dev.contains("Max-Age=0"));
        assert!(!dev.contains("Secure"));
        assert!(!dev.contains("SameSite=Strict"));

        let prod = clear_auth_cookie_value(true);
        assert!(prod.contains("SameSite=Lax"));
        assert!(prod.contains("; Secure"));
        assert!(!prod.contains("SameSite=Strict"));
    }
}
