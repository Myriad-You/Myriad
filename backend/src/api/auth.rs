//! Core auth endpoints that are not provider-specific.
//!
//! GitHub/OIDC login is handled by `api::oauth` via `/api/auth/oauth/:slug/*`.

use crate::error::HttpError;
use axum::{
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::{json, Value};
use std::env;

// Re-export Claims so existing imports `super::auth::Claims` keep working
use crate::middleware::auth::clear_auth_cookie_value;
pub use crate::middleware::auth::Claims;

/// Guest body for the session probe. HTTP 200 — never 401 — so browsers do not
/// paint Network red for expected unauthenticated state.
pub fn unauthenticated_me_body() -> Value {
    json!({ "authenticated": false })
}

/// Extract JWT from `Authorization: Bearer` or `auth_token` cookie.
fn extract_auth_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .or_else(|| {
            headers
                .get(header::COOKIE)
                .and_then(|v| v.to_str().ok())
                .and_then(|cookies| {
                    cookies.split(';').find_map(|cookie| {
                        let (name, value) = cookie.trim().split_once('=')?;
                        if name == "auth_token" {
                            Some(value)
                        } else {
                            None
                        }
                    })
                })
        })
}

fn selected_auth_uses_cookie(headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_none()
        && headers
            .get(header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|cookies| {
                cookies.split(';').any(|cookie| {
                    cookie
                        .trim()
                        .split_once('=')
                        .is_some_and(|(name, _)| name == "auth_token")
                })
            })
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
    let Some(token) = extract_auth_token(&headers) else {
        return Ok(unauthenticated_me_response(false).await);
    };
    let clear_invalid_cookie = selected_auth_uses_cookie(&headers);

    let jwt_secret = env::var("JWT_SECRET").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to create session token", "code": "session_failed"})),
        )
    })?;

    let token_data = match jsonwebtoken::decode::<Claims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(jwt_secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    ) {
        Ok(data) => data,
        Err(_) => {
            // Expired/forged cookie — expected guest for this probe, not 401 noise.
            return Ok(unauthenticated_me_response(clear_invalid_cookie).await);
        }
    };

    let user_id: i32 = match token_data.claims.sub.parse() {
        Ok(id) => id,
        Err(_) => {
            return Ok(unauthenticated_me_response(clear_invalid_cookie).await);
        }
    };

    use sea_orm::Value as SeaValue;

    // 头像阶梯是 services::avatar 的共享片段（与 /api/tapp/context/user 同一份）
    // Include token_version so revoked sessions (logout / password change) probe
    // as unauthenticated without painting Network red (still HTTP 200).
    let user_sql = format!(
        r#"SELECT u.id, u.username, u.auth_provider, u.is_admin,
                  COALESCE(u.is_owner, false) AS is_owner,
                  COALESCE(u.token_version, 0) AS token_version,
                  {avatar} AS avatar_url,
                  u.github_id, u.linked_github_id, u.bio, u.display_name,
                  u.password_hash IS NOT NULL AS has_password,
                  u.last_login_at
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
        Err(_) => {
            return Err(HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error", "code": "database_error"})),
            )));
        }
    };

    let Some(user_row) = user_row else {
        // Token valid but user gone — still a session-probe miss, not auth gate.
        return Ok(unauthenticated_me_response(clear_invalid_cookie).await);
    };

    let db_tv: i64 = user_row
        .try_get::<i32>("", "token_version")
        .ok()
        .map(i64::from)
        .or_else(|| user_row.try_get::<i64>("", "token_version").ok())
        .unwrap_or(0);
    if !crate::middleware::auth::session_epoch_matches(token_data.claims.tv, Some(db_tv)) {
        // Revoked session — soft probe miss (not 401).
        return Ok(unauthenticated_me_response(clear_invalid_cookie).await);
    }

    // identities 列表（用 user_identities 表）
    let identity_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, provider, provider_username, is_primary, linked_at \
             FROM user_identities WHERE user_id = $1 ORDER BY linked_at ASC",
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

    let id: i32 = user_row.try_get("", "id").unwrap_or(0);
    let username: String = user_row.try_get("", "username").unwrap_or_default();
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
        "identities": identities,
    }))
    .into_response())
}

/// `POST /api/auth/logout`
///
/// Clears the browser cookie and, when a still-valid (signature) token is
/// present, bumps `users.token_version` so stolen copies of the same JWT fail
/// closed on protected routes until the next login.
pub async fn logout(
    crate::extract::Db(db): crate::extract::Db,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Crypto-only decode: even a soon-to-expire token should revoke the epoch.
    // Do **not** require session epoch match here — logout must succeed after
    // a prior password change already bumped tv.
    if let Ok(claims) = crate::middleware::auth::verify_jwt_token(&headers) {
        if let Ok(user_id) = claims.sub.parse::<i32>() {
            if user_id > 0 {
                match crate::middleware::auth::bump_token_version(&db, user_id).await {
                    Ok(Some(new_tv)) => {
                        tracing::info!(
                            user_id,
                            token_version = new_tv,
                            "🚪 User logout — session epoch bumped"
                        );
                    }
                    Ok(None) => {
                        tracing::debug!(user_id, "🚪 Logout for missing user — cookie clear only");
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
    } else {
        tracing::info!("🚪 User logout - clearing auth cookie (no/invalid token)");
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
    use axum::http::HeaderValue;

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
    fn extract_auth_token_from_bearer_and_cookie() {
        let mut headers = HeaderMap::new();
        assert!(extract_auth_token(&headers).is_none());

        headers.insert(
            "Authorization",
            HeaderValue::from_static("Bearer abc.def.ghi"),
        );
        assert_eq!(extract_auth_token(&headers), Some("abc.def.ghi"));

        let mut cookie_only = HeaderMap::new();
        cookie_only.insert(
            header::COOKIE,
            HeaderValue::from_static("other=1; auth_token=cookie.jwt.sig; x=y"),
        );
        assert_eq!(extract_auth_token(&cookie_only), Some("cookie.jwt.sig"));
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
