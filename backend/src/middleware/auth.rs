use axum::{
    extract::Request,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use jsonwebtoken::{decode, DecodingKey, Validation};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::sync::{Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use uuid::Uuid;

/// JWT Claims structure (must match auth.rs)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,      // User ID
    pub username: String, // Username
    pub is_admin: bool,   // ✅ 安全修复 P0: Admin status
    pub exp: i64,         // Expiration time
    pub iat: i64,         // Issued at
}

/// Authentication middleware - verifies JWT token
/// Returns 401 if token is missing or invalid
pub async fn auth_middleware(req: Request, next: Next) -> Response {
    let headers = req.headers();

    match verify_jwt_token(headers) {
        Ok(claims) => {
            record_user_presence(&claims);
            // Token is valid, inject claims into request extensions
            let mut req = req;
            req.extensions_mut().insert(claims);
            next.run(req).await
        }
        Err(error_response) => *error_response,
    }
}

/// 每用户至少间隔 60s 才落一次库，避免高频请求放大写入。
const PRESENCE_WRITE_INTERVAL: Duration = Duration::from_secs(60);
/// 两次活跃间隔 ≤300s 视为持续在线，计入 online_seconds；更长间隔视为离线后重新上线。
const PRESENCE_SESSION_GAP_SECS: i64 = 300;

static PRESENCE_WRITE_TIMES: OnceLock<StdMutex<HashMap<i32, Instant>>> = OnceLock::new();

fn presence_write_due(user_id: i32) -> bool {
    let map = PRESENCE_WRITE_TIMES.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut guard = match map.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let now = Instant::now();
    match guard.get(&user_id) {
        Some(last) if now.duration_since(*last) < PRESENCE_WRITE_INTERVAL => false,
        _ => {
            guard.insert(user_id, now);
            true
        }
    }
}

/// 节流更新 users.last_seen_at / online_seconds（异步、尽力而为）。
/// 游客（负数 ID）不记录。
pub fn record_user_presence(claims: &Claims) {
    let Ok(user_id) = claims.sub.parse::<i32>() else {
        return;
    };
    if user_id <= 0 || !presence_write_due(user_id) {
        return;
    }
    tokio::spawn(async move {
        let db_guard = crate::DB_CONNECTION.read().await;
        let Some(db) = db_guard.as_ref() else {
            return;
        };
        let result = db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE users SET \
                     online_seconds = online_seconds + CASE \
                         WHEN last_seen_at IS NOT NULL AND NOW() - last_seen_at <= make_interval(secs => $2) \
                         THEN EXTRACT(EPOCH FROM (NOW() - last_seen_at))::BIGINT \
                         ELSE 0 END, \
                     last_seen_at = NOW() \
                 WHERE id = $1",
                [user_id.into(), PRESENCE_SESSION_GAP_SECS.into()],
            ))
            .await;
        if let Err(e) = result {
            tracing::debug!("Failed to record presence for user {}: {}", user_id, e);
        }
    });
}

/// Admin-only middleware - verifies JWT token and checks admin status
/// Returns 403 if user is not an admin
///
/// ✅ SECURITY: Checks both the signed claim and the current database role.
/// Used for dangerous operations like deleting all reports
pub async fn admin_middleware(req: Request, next: Next) -> Response {
    let headers = req.headers();

    match verify_jwt_token(headers) {
        Ok(claims) => {
            record_user_presence(&claims);
            if let Err((status, body)) = ensure_current_admin(&claims).await {
                tracing::warn!(
                    "⚠️  User {} (is_admin={}) attempted to access admin-only endpoint (Forbidden)",
                    claims.username,
                    claims.is_admin
                );
                return (status, body).into_response();
            }

            tracing::info!(
                "✅ Admin access granted to user: {} (is_admin=true)",
                claims.username
            );

            // ✅ 关键修复: 将 claims 注入到 request extensions 中
            // 这样后续的 Extension(claims) 提取器才能正常工作
            let mut req = req;
            req.extensions_mut().insert(claims);
            next.run(req).await
        }
        Err(error_response) => *error_response,
    }
}

/// Verify the signed admin claim against the current database state.
///
/// This prevents a demoted admin from keeping admin access until the old JWT
/// expires.
pub async fn ensure_current_admin(
    claims: &Claims,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if !claims.is_admin {
        return Err(admin_forbidden());
    }

    let user_id: i32 = claims.sub.parse().map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Unauthorized",
                "message": "Invalid user ID in authorization token."
            })),
        )
    })?;

    let db_guard = crate::DB_CONNECTION.read().await;
    let db = db_guard.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "Administrator status cannot be verified."
            })),
        )
    })?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT is_admin FROM users WHERE id = $1 LIMIT 1",
            [user_id.into()],
        ))
        .await
        .map_err(|e| {
            tracing::error!("Failed to verify current admin status: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Database error",
                    "message": "Administrator status cannot be verified."
                })),
            )
        })?;

    let is_admin = row
        .and_then(|r| r.try_get::<bool>("", "is_admin").ok())
        .unwrap_or(false);

    if is_admin {
        Ok(())
    } else {
        Err(admin_forbidden())
    }
}

/// Verify JWT headers and re-check the admin flag against the current database.
pub async fn verify_current_admin_from_headers(
    headers: &HeaderMap,
) -> Result<Claims, (StatusCode, Json<serde_json::Value>)> {
    let claims = verify_jwt_token(headers).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Unauthorized",
                "message": "Please login before using administrator functions."
            })),
        )
    })?;

    ensure_current_admin(&claims).await?;
    Ok(claims)
}

fn admin_forbidden() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": "Forbidden",
            "message": "Administrator access required. Only current admin users can perform this action."
        })),
    )
}

const GUEST_SESSION_COOKIE: &str = "myriad_guest_session";
const GUEST_SESSION_MAX_AGE: i64 = 30 * 24 * 60 * 60;

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                let (cookie_name, value) = cookie.trim().split_once('=')?;
                (cookie_name == name).then_some(value)
            })
        })
}

fn guest_signature(secret: &[u8], session_id: &str) -> Option<Vec<u8>> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).ok()?;
    mac.update(b"myriad-guest-session-v1\0");
    mac.update(session_id.as_bytes());
    Some(mac.finalize().into_bytes().to_vec())
}

fn sign_guest_session(secret: &[u8], session_id: &str) -> Option<String> {
    let signature = guest_signature(secret, session_id)?;
    Some(format!(
        "{session_id}.{}",
        URL_SAFE_NO_PAD.encode(signature)
    ))
}

fn verify_guest_session(secret: &[u8], token: &str) -> Option<String> {
    let (session_id, encoded_signature) = token.split_once('.')?;
    if session_id.len() != 32 || !session_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let supplied = URL_SAFE_NO_PAD.decode(encoded_signature).ok()?;
    let expected = guest_signature(secret, session_id)?;
    (supplied.len() == expected.len() && supplied.as_slice().ct_eq(expected.as_slice()).into())
        .then(|| session_id.to_ascii_lowercase())
}

fn guest_id(session_id: &str) -> i32 {
    let digest = Sha256::digest(session_id.as_bytes());
    let value = u32::from_be_bytes(digest[..4].try_into().expect("SHA-256 prefix"));
    -((value % i32::MAX as u32) as i32 + 1)
}

/// Optional authentication middleware - allows guest access
///
/// 用于支持权限下放的 API：
/// - 如果有有效 token，验证并注入 Claims
/// - 如果没有 token 或 token 无效，注入游客 Claims
///
/// 游客 ID 策略：
/// - 使用浏览器持有的 HttpOnly 签名 session，而不是共享出口 IP
/// - 同一浏览器 session 获得稳定的负数 ID
/// - 负数 ID 与正数用户 ID 区分，便于管理
///
/// 安全说明：
/// - 游客 Claims 的 is_admin 为 false
/// - API 端点需要自行检查权限（通过 TappPermissionService）
pub async fn optional_auth_middleware(req: Request, next: Next) -> Response {
    let headers = req.headers();
    let mut set_guest_cookie = None;
    let claims = match verify_jwt_token(headers) {
        Ok(claims) => {
            record_user_presence(&claims);
            claims
        }
        Err(_) => {
            let secret = match env::var("JWT_SECRET") {
                Ok(secret) if !secret.is_empty() => secret,
                _ => {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error": "Guest session signing is unavailable"})),
                    )
                        .into_response()
                }
            };
            let session_id = cookie_value(headers, GUEST_SESSION_COOKIE)
                .and_then(|token| verify_guest_session(secret.as_bytes(), token))
                .unwrap_or_else(|| {
                    let session_id = Uuid::new_v4().simple().to_string();
                    set_guest_cookie = sign_guest_session(secret.as_bytes(), &session_id);
                    session_id
                });
            let guest_id = guest_id(&session_id);
            tracing::debug!(guest_id, "Guest access through signed browser session");

            Claims {
                sub: guest_id.to_string(),
                username: format!("guest:{}", &session_id[..8]),
                is_admin: false,
                exp: chrono::Utc::now().timestamp() + GUEST_SESSION_MAX_AGE,
                iat: chrono::Utc::now().timestamp(),
            }
        }
    };

    let mut req = req;
    req.extensions_mut().insert(claims);
    let mut response = next.run(req).await;
    if let Some(token) = set_guest_cookie {
        let is_production = crate::oauth_url_builder::SiteConfig::is_production().await;
        let cookie = format!(
            "{GUEST_SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={GUEST_SESSION_MAX_AGE}{}",
            if is_production { "; Secure" } else { "" }
        );
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
    response
}

/// Verify JWT token from Authorization header or Cookie
/// Verify JWT for handlers that need claims outside the middleware pipeline.
pub fn verify_jwt_token(headers: &HeaderMap) -> Result<Claims, Box<Response>> {
    // Extract token from Authorization header or Cookie (优先 Header)
    let token = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .or_else(|| {
            // 回退到 HttpOnly Cookie
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
            tracing::debug!("Missing or invalid Authorization header/cookie");
            Box::new(
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "error": "Unauthorized",
                        "message": "Missing or invalid authorization token. Please login first."
                    })),
                )
                    .into_response(),
            )
        })?;

    // Get JWT secret
    let jwt_secret = env::var("JWT_SECRET").map_err(|_| {
        tracing::error!("JWT_SECRET not configured");
        Box::new(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Server configuration error",
                    "message": "Authentication system not properly configured"
                })),
            )
                .into_response(),
        )
    })?;

    // Decode and verify token
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|e| {
        tracing::debug!("Invalid JWT token: {:?}", e);
        Box::new(
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "Invalid token",
                    "message": "Token is invalid or expired. Please login again."
                })),
            )
                .into_response(),
        )
    })?;

    Ok(token_data.claims)
}

/// Optional authentication - extracts claims if token is present, but doesn't fail if missing
/// Useful for endpoints that behave differently for authenticated users but are also public
pub fn extract_optional_claims(headers: &HeaderMap) -> Option<Claims> {
    // 尝试从 Authorization header 或 Cookie 获取 token
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
        })?;

    let jwt_secret = env::var("JWT_SECRET").ok()?;
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .ok()
    .map(|data| data.claims)
}

#[cfg(test)]
mod tests {
    use super::{guest_id, sign_guest_session, verify_guest_session};

    #[test]
    fn signed_guest_session_is_stable_and_tamper_evident() {
        let secret = b"test-secret-at-least-thirty-two-bytes-long";
        let session_id = "0123456789abcdef0123456789abcdef";
        let token = sign_guest_session(secret, session_id).expect("session signs");
        assert_eq!(
            verify_guest_session(secret, &token).as_deref(),
            Some(session_id)
        );
        assert!(verify_guest_session(secret, &format!("{token}x")).is_none());
        assert!(verify_guest_session(b"different-secret", &token).is_none());
    }

    #[test]
    fn guest_ids_are_negative_and_browser_session_scoped() {
        let first = guest_id("0123456789abcdef0123456789abcdef");
        assert!(first < 0);
        assert_eq!(first, guest_id("0123456789abcdef0123456789abcdef"));
        assert_ne!(first, guest_id("fedcba9876543210fedcba9876543210"));
    }
}
