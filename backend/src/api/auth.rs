//! Core auth endpoints that are not provider-specific.
//!
//! GitHub/OIDC login is handled by `api::oauth` via `/api/auth/oauth/:slug/*`.

use axum::{
    extract::State,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    Json,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{json, Value};
use std::env;

// Re-export Claims so existing imports `super::auth::Claims` keep working
pub use crate::middleware::auth::Claims;

/// `GET /api/auth/me` — 返回当前登录用户信息
pub async fn get_current_user(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let token = headers
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
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "Unauthorized", "message": "Missing token"})),
            )
        })?;

    let jwt_secret = env::var("JWT_SECRET").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "JWT secret not configured"})),
        )
    })?;

    let token_data = jsonwebtoken::decode::<Claims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(jwt_secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    )
    .map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid token"})),
        )
    })?;

    let user_id: i32 = token_data.claims.sub.parse().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Invalid token data"})),
        )
    })?;

    use sea_orm::Value as SeaValue;

    let user_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT u.id, u.username, u.auth_provider, u.is_admin,
                      COALESCE(
                          NULLIF(
                              CASE
                                  WHEN u.avatar_url LIKE 'https://ui-avatars.com/%'
                                       OR u.avatar_url LIKE 'http://ui-avatars.com/%'
                                  THEN NULL
                                  ELSE u.avatar_url
                              END,
                              ''
                          ),
                          (
                              SELECT NULLIF(ui.avatar_url, '')
                              FROM user_identities ui
                              WHERE ui.user_id = u.id
                                AND ui.avatar_url IS NOT NULL
                                AND ui.avatar_url <> ''
                              ORDER BY ui.is_primary DESC, ui.last_login_at DESC NULLS LAST, ui.linked_at DESC
                              LIMIT 1
                          ),
                          NULLIF(u.avatar_url, '')
                      ) AS avatar_url,
                      u.github_id, u.linked_github_id, u.bio,
                      u.password_hash IS NOT NULL AS has_password
               FROM users u
               WHERE u.id = $1"#,
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "User not found"})),
            )
        })?;

    // identities 列表（用 user_identities 表）
    let identity_rows = db
        .query_all(Statement::from_sql_and_values(
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
    let avatar_url: String = user_row
        .try_get("", "avatar_url")
        .unwrap_or_else(|_| "https://github.com/ghost.png".to_string());
    let github_id: Option<i64> = user_row.try_get("", "github_id").ok();
    let linked_github_id: Option<i64> = user_row.try_get("", "linked_github_id").ok();
    let bio: Option<String> = user_row.try_get("", "bio").ok();
    let has_password: bool = user_row.try_get("", "has_password").unwrap_or(false);

    Ok(Json(json!({
        "id": id,
        "username": username,
        "display_name": username,
        "auth_provider": auth_provider,
        "is_admin": is_admin,
        "avatar_url": avatar_url,
        "github_id": github_id,
        "linked_github_id": linked_github_id.map(|id| id.to_string()),
        "bio": bio,
        "has_password": has_password,
        "identities": identities,
    })))
}

/// `POST /api/auth/logout`
pub async fn logout() -> impl IntoResponse {
    tracing::info!("🚪 User logout - clearing auth cookie");
    const COOKIE_VALUE: &str = "auth_token=deleted; Path=/; HttpOnly; SameSite=Strict; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT";
    let mut response =
        Json(json!({"success": true, "message": "Logged out successfully"})).into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, HeaderValue::from_static(COOKIE_VALUE));
    response
}
