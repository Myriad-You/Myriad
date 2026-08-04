use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, KeyInit, Mac};
use jsonwebtoken::{decode, DecodingKey, Validation};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::sync::{Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use uuid::Uuid;

/// Browser / API session lifetime (days). Keep long-lived; revoke via `token_version`.
pub const JWT_TTL_DAYS: i64 = 30;
/// `auth_token` cookie Max-Age in seconds (matches JWT TTL).
pub const AUTH_COOKIE_MAX_AGE_SECS: i64 = JWT_TTL_DAYS * 24 * 60 * 60;

/// JWT Claims structure (must match auth issuance sites).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,      // User ID
    pub username: String, // Username
    pub is_admin: bool, // Admin status
    /// Durable site owner (`users.is_owner`). Defaults false for older tokens.
    #[serde(default)]
    pub is_owner: bool,
    pub exp: i64,         // Expiration time
    pub iat: i64,         // Issued at
    /// Session epoch (`users.token_version`). Defaults `0` for pre-MYR-005 tokens
    /// so existing sessions keep working until the first revoke bump.
    #[serde(default)]
    pub tv: i64,
}

/// Build claims for a durable user session (login / register / password reissue).
pub fn mint_session_claims(
    user_id: i32,
    username: impl Into<String>,
    is_admin: bool,
    is_owner: bool,
    token_version: i64,
) -> Claims {
    let now = chrono::Utc::now().timestamp();
    Claims {
        sub: user_id.to_string(),
        username: username.into(),
        is_admin,
        is_owner,
        exp: now + JWT_TTL_DAYS * 24 * 60 * 60,
        iat: now,
        tv: token_version,
    }
}

/// Encode claims with `JWT_SECRET`. Caller must set cookie / Authorization.
pub fn encode_session_token(claims: &Claims) -> Result<String, String> {
    let jwt_secret =
        env::var("JWT_SECRET").map_err(|_| "JWT_SECRET not configured".to_string())?;
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        claims,
        &jsonwebtoken::EncodingKey::from_secret(jwt_secret.as_bytes()),
    )
    .map_err(|e| format!("JWT encode failed: {e}"))
}

/// `auth_token=…` Set-Cookie value for a newly issued session.
pub fn auth_cookie_value(token: &str, is_production: bool) -> String {
    format!(
        "auth_token={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={AUTH_COOKIE_MAX_AGE_SECS}{}",
        if is_production { "; Secure" } else { "" }
    )
}

/// Authentication middleware - verifies JWT token + session epoch
/// Returns 401 if token is missing, invalid, or revoked
///
/// Requires `Router<AppState>` so `State<DatabaseConnection>` resolves via FromRef.
pub async fn auth_middleware(
    State(db): State<DatabaseConnection>,
    req: Request,
    next: Next,
) -> Response {
    let headers = req.headers();

    match authenticate_request(headers, &db).await {
        Ok(claims) => {
            record_user_presence(&claims, db);
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
/// Drop map entries older than this so long-lived processes do not retain every
/// user_id forever (MYR-037). Must be ≥ [`PRESENCE_WRITE_INTERVAL`].
const PRESENCE_MAP_TTL: Duration = Duration::from_secs(15 * 60);
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
    // Opportunistic TTL prune (cheap when map is small; caps growth on multi-user sites).
    if guard.len() > 64 {
        guard.retain(|_, last| now.duration_since(*last) < PRESENCE_MAP_TTL);
    }
    match guard.get(&user_id) {
        Some(last) if now.duration_since(*last) < PRESENCE_WRITE_INTERVAL => false,
        _ => {
            guard.insert(user_id, now);
            true
        }
    }
}

/// 节流更新 users.last_seen_at / online_seconds（异步、尽力而为）。
/// 游客（负数 ID）不记录。DB 由 middleware `State` 注入。
pub fn record_user_presence(claims: &Claims, db: DatabaseConnection) {
    let Ok(user_id) = claims.sub.parse::<i32>() else {
        return;
    };
    if user_id <= 0 || !presence_write_due(user_id) {
        return;
    }
    tokio::spawn(async move {
        let result = db
            .execute_raw(Statement::from_sql_and_values(
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
/// Checks both the signed claim and the current database role.
/// Used for dangerous operations like deleting all reports
pub async fn admin_middleware(
    State(db): State<DatabaseConnection>,
    req: Request,
    next: Next,
) -> Response {
    let headers = req.headers();

    match authenticate_request(headers, &db).await {
        Ok(claims) => {
            record_user_presence(&claims, db.clone());
            if let Err((status, body)) = ensure_current_admin_on(&claims, &db).await {
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

            // 关键修复: 将 claims 注入到 request extensions 中
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
/// Preferred: verify admin with an explicit DB handle (handlers / middleware State).
pub async fn ensure_current_admin_on(
    claims: &Claims,
    db: &DatabaseConnection,
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

    let row = db
        .query_one_raw(Statement::from_sql_and_values(
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

/// Verify JWT headers (incl. session epoch) and re-check the admin flag against the request DB.
pub async fn verify_current_admin_from_headers(
    headers: &HeaderMap,
    db: &DatabaseConnection,
) -> Result<Claims, (StatusCode, Json<serde_json::Value>)> {
    let claims = authenticate_request(headers, db).await.map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Unauthorized",
                "message": "Please login before using administrator functions."
            })),
        )
    })?;

    ensure_current_admin_on(&claims, db).await?;
    Ok(claims)
}

/// Load `users.token_version` for a durable user id.
///
/// Returns `None` when the user row is missing (deleted) or `user_id` is not a
/// positive durable id (guests use negative subjects and have no row).
pub async fn load_token_version(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Option<i64>, sea_orm::DbErr> {
    if user_id <= 0 {
        return Ok(None);
    }
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT token_version FROM users WHERE id = $1 LIMIT 1",
            [user_id.into()],
        ))
        .await?;
    Ok(row.and_then(|r| {
        r.try_get::<i32>("", "token_version")
            .ok()
            .map(i64::from)
            .or_else(|| r.try_get::<i64>("", "token_version").ok())
    }))
}

/// Pure session-epoch check used by auth middleware and unit tests.
///
/// - Missing user (`None`) → revoked (deleted account)
/// - Claim `tv` must equal stored version
pub fn session_epoch_matches(claim_tv: i64, db_version: Option<i64>) -> bool {
    match db_version {
        Some(v) => claim_tv == v,
        None => false,
    }
}

/// Fail closed when JWT session epoch does not match the user row.
///
/// Guests (`sub` ≤ 0) skip the DB check — they are not durable sessions.
pub async fn ensure_session_epoch(
    claims: &Claims,
    db: &DatabaseConnection,
) -> Result<(), Box<Response>> {
    let user_id: i32 = claims.sub.parse().map_err(|_| unauthorized_session_response())?;
    if user_id <= 0 {
        return Ok(());
    }

    let db_version = load_token_version(db, user_id).await.map_err(|e| {
        tracing::error!("Failed to load token_version for user {}: {}", user_id, e);
        Box::new(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Database error",
                    "message": "Session cannot be verified."
                })),
            )
                .into_response(),
        )
    })?;

    if session_epoch_matches(claims.tv, db_version) {
        Ok(())
    } else {
        tracing::debug!(
            user_id,
            claim_tv = claims.tv,
            db_version = ?db_version,
            "JWT session epoch mismatch — treating token as revoked"
        );
        Err(unauthorized_session_response())
    }
}

fn unauthorized_session_response() -> Box<Response> {
    Box::new(
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Invalid token",
                "message": "Session has been revoked. Please login again."
            })),
        )
            .into_response(),
    )
}

/// Cryptographic JWT verify **plus** server-side session epoch check.
///
/// Prefer this over [`verify_jwt_token`] for any path that must fail closed
/// after logout / password change / account deletion.
pub async fn authenticate_request(
    headers: &HeaderMap,
    db: &DatabaseConnection,
) -> Result<Claims, Box<Response>> {
    let claims = verify_jwt_token(headers)?;
    ensure_session_epoch(&claims, db).await?;
    Ok(claims)
}

/// Atomically bump `users.token_version` and return the new value.
///
/// Used on logout and password change so all previously issued JWTs fail
/// [`ensure_session_epoch`]. Returns `None` if the user row is gone.
pub async fn bump_token_version(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Option<i64>, sea_orm::DbErr> {
    if user_id <= 0 {
        return Ok(None);
    }
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE users SET token_version = token_version + 1, updated_at = NOW() \
             WHERE id = $1 RETURNING token_version",
            [user_id.into()],
        ))
        .await?;
    Ok(row.and_then(|r| {
        r.try_get::<i32>("", "token_version")
            .ok()
            .map(i64::from)
            .or_else(|| r.try_get::<i64>("", "token_version").ok())
    }))
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

/// Stable negative subject id for a browser guest session.
///
/// ## Capacity / collision bound
/// Postgres `tapp_runtime_registry.subject_id` (and related columns) are
/// `INTEGER`, so the id must fit in signed 32-bit. We use the full negative
/// `i32` range `i32::MIN ..= -1` (~2³¹ values). Birthday bound for ~50%
/// collision probability among *distinct concurrent guest sessions* is on the
/// order of √(π · 2³¹ / 2) ≈ **~58k** sessions — residual collisions only
/// alias guest storage / rate-limit namespaces, never escalate to a real user
/// (real users are non-negative).
///
/// ## Hash material
/// Full SHA-256 is XOR-folded into 31 bits (previous scheme used only 4 raw
/// prefix bytes). Cookie session tokens themselves are unchanged (HMAC of the
/// 32-hex session id); only the derived numeric subject remaps. Existing
/// guest storage rows under the old mapping become orphaned after upgrade —
/// acceptable for ephemeral guest sandbox data (no migration).
///
/// A true 63-bit negative `i64` would need `BIGINT` subject columns site-wide;
/// until then this is the strongest scheme that still fits the DB type.
fn guest_id(session_id: &str) -> i32 {
    let digest = Sha256::digest(session_id.as_bytes());
    let mut acc = 0u64;
    for chunk in digest.chunks_exact(8) {
        acc ^= u64::from_be_bytes(chunk.try_into().expect("SHA-256 8-byte chunk"));
    }
    // 31 payload bits → always map into i32::MIN ..= -1 (never 0 / positive).
    let bits31 = (acc & 0x7FFF_FFFF) as u32;
    if bits31 == 0 {
        i32::MIN
    } else {
        -(bits31 as i32)
    }
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
pub async fn optional_auth_middleware(
    State(db): State<DatabaseConnection>,
    req: Request,
    next: Next,
) -> Response {
    let headers = req.headers();
    let mut set_guest_cookie = None;
    // Full auth (crypto + epoch). Revoked / missing sessions fall through to guest.
    let claims = match authenticate_request(headers, &db).await {
        Ok(claims) => {
            record_user_presence(&claims, db);
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
                is_owner: false,
                exp: chrono::Utc::now().timestamp() + GUEST_SESSION_MAX_AGE,
                iat: chrono::Utc::now().timestamp(),
                tv: 0,
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
    use super::{
        guest_id, mint_session_claims, session_epoch_matches, sign_guest_session,
        verify_guest_session, AUTH_COOKIE_MAX_AGE_SECS, JWT_TTL_DAYS,
    };

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
        // Never collide with real user ids (non-negative).
        assert!(first <= -1);
        // Distinct sessions should almost always differ under full-hash fold.
        let many: std::collections::HashSet<i32> = (0u32..256)
            .map(|i| guest_id(&format!("{i:032x}")))
            .collect();
        assert_eq!(many.len(), 256, "unexpected guest_id collision in 256 samples");
    }

    #[test]
    fn session_epoch_matches_requires_equal_version() {
        assert!(session_epoch_matches(0, Some(0)));
        assert!(session_epoch_matches(3, Some(3)));
        assert!(!session_epoch_matches(0, Some(1)));
        assert!(!session_epoch_matches(2, Some(1)));
        // Deleted / missing user → fail closed
        assert!(!session_epoch_matches(0, None));
        assert!(!session_epoch_matches(5, None));
    }

    #[test]
    fn mint_session_claims_embeds_token_version_and_30d_ttl() {
        let claims = mint_session_claims(7, "alice", false, true, 4);
        assert_eq!(claims.sub, "7");
        assert_eq!(claims.username, "alice");
        assert!(!claims.is_admin);
        assert!(claims.is_owner);
        assert_eq!(claims.tv, 4);
        let span = claims.exp - claims.iat;
        assert_eq!(span, JWT_TTL_DAYS * 24 * 60 * 60);
        assert_eq!(AUTH_COOKIE_MAX_AGE_SECS, span);
    }

    #[test]
    fn claims_tv_defaults_when_absent_in_json() {
        // Pre-MYR-005 tokens omit `tv`; serde default keeps them at epoch 0.
        let json = r#"{"sub":"1","username":"u","is_admin":false,"exp":1,"iat":0}"#;
        let claims: super::Claims = serde_json::from_str(json).expect("deserialize");
        assert_eq!(claims.tv, 0);
        assert!(!claims.is_owner);
    }
}
