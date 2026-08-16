use axum::{
    Json,
    extract::Request,
    http::{HeaderMap, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use jsonwebtoken::{DecodingKey, Validation, decode};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::env;
use subtle::ConstantTimeEq;

use crate::middleware::auth::Claims;

type HmacSha256 = Hmac<Sha256>;

/// Wire format version for stateless browser CSRF tokens.
///
/// The version is present both as the token prefix and in the signed payload.
/// Keeping both copies makes version transitions explicit and prevents a token
/// from one format being interpreted as another format after a rolling deploy.
const CSRF_TOKEN_VERSION: u8 = 1;
const CSRF_TOKEN_VERSION_PREFIX: &str = "v1";
/// Keep the existing one-hour browser/server lifetime. The frontend refreshes
/// a little before this deadline, while the signed expiry remains authoritative.
pub(crate) const CSRF_TOKEN_TTL_SECS: i64 = 60 * 60;
const CSRF_CLOCK_SKEW_SECS: i64 = 60;
const CSRF_NONCE_BYTES: usize = 32;
/// Reject oversized values before decoding or running HMAC to keep this public
/// header endpoint bounded even when a client sends an arbitrary string.
const CSRF_TOKEN_MAX_LEN: usize = 1024;
const CSRF_KEY_CONTEXT: &[u8] = b"myriad-csrf-token-signing-key-v1\0";

#[derive(Debug, Clone, PartialEq, Eq)]
struct VerifiedSession {
    /// SHA-256 digest of the verified JWT signature segment. Keeping only a
    /// fixed-size digest in the CSRF payload avoids copying JWT material into
    /// another browser-visible token while retaining per-session binding.
    session_id: [u8; 32],
    /// Durable `users.token_version` epoch carried by the JWT (`tv`).
    epoch: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct CsrfTokenPayload {
    /// Signed payload version; must match [`CSRF_TOKEN_VERSION`].
    v: u8,
    /// Base64url SHA-256 digest of the verified JWT signature segment this
    /// token belongs to.
    sid: String,
    /// JWT session epoch (`Claims::tv`) at issuance.
    tv: i64,
    /// Unix seconds when the token was issued.
    iat: i64,
    /// Unix seconds when the token expires.
    exp: i64,
    /// Randomness prevents response caching from yielding one stable value;
    /// it is authenticated but otherwise has no server-side state.
    n: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CsrfTokenError {
    Malformed,
    Invalid,
    Expired,
}

/// Derive the CSRF MAC key from the existing JWT/session secret.
///
/// HMAC key derivation is domain-separated so a CSRF token cannot be used as a
/// JWT or another application HMAC even though all instances share `JWT_SECRET`.
fn csrf_signing_key(secret: &str) -> Option<[u8; 32]> {
    if secret.is_empty() {
        return None;
    }
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).ok()?;
    mac.update(CSRF_KEY_CONTEXT);
    let digest = mac.finalize().into_bytes();
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest);
    Some(key)
}

fn configured_csrf_key() -> Option<[u8; 32]> {
    let secret = env::var("JWT_SECRET").ok()?;
    csrf_signing_key(&secret)
}

fn unix_now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn sign_csrf_body(key: &[u8; 32], signed_body: &str) -> Option<[u8; 32]> {
    let mut mac = HmacSha256::new_from_slice(key).ok()?;
    mac.update(signed_body.as_bytes());
    let digest = mac.finalize().into_bytes();
    let mut signature = [0u8; 32];
    signature.copy_from_slice(&digest);
    Some(signature)
}

fn random_csrf_nonce() -> String {
    let mut nonce = [0u8; CSRF_NONCE_BYTES];
    rand::rng().fill_bytes(&mut nonce);
    URL_SAFE_NO_PAD.encode(nonce)
}

fn issue_csrf_token_with_key(
    key: &[u8; 32],
    session: &VerifiedSession,
    issued_at: i64,
) -> Option<String> {
    let payload = CsrfTokenPayload {
        v: CSRF_TOKEN_VERSION,
        sid: URL_SAFE_NO_PAD.encode(session.session_id),
        tv: session.epoch,
        iat: issued_at,
        exp: issued_at.checked_add(CSRF_TOKEN_TTL_SECS)?,
        n: random_csrf_nonce(),
    };
    let encoded_payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).ok()?);
    let signed_body = format!("{CSRF_TOKEN_VERSION_PREFIX}.{encoded_payload}");
    let signature = sign_csrf_body(key, &signed_body)?;
    Some(format!(
        "{signed_body}.{}",
        URL_SAFE_NO_PAD.encode(signature)
    ))
}

fn issue_csrf_token(session: &VerifiedSession, issued_at: i64) -> Option<String> {
    let key = configured_csrf_key()?;
    issue_csrf_token_with_key(&key, session, issued_at)
}

fn parse_csrf_token(token: &str) -> Result<(&str, &str, &str), CsrfTokenError> {
    if token.is_empty() || token.len() > CSRF_TOKEN_MAX_LEN {
        return Err(CsrfTokenError::Malformed);
    }
    let mut parts = token.split('.');
    let version = parts.next().ok_or(CsrfTokenError::Malformed)?;
    let payload = parts.next().ok_or(CsrfTokenError::Malformed)?;
    let signature = parts.next().ok_or(CsrfTokenError::Malformed)?;
    if parts.next().is_some()
        || version != CSRF_TOKEN_VERSION_PREFIX
        || payload.is_empty()
        || signature.is_empty()
    {
        return Err(CsrfTokenError::Malformed);
    }
    Ok((version, payload, signature))
}

/// Verify a signed token against the currently verified JWT session/epoch.
///
/// No process-local state participates in this check: any instance with the
/// shared `JWT_SECRET` can verify a token issued by another instance.
fn verify_csrf_token(
    token: &str,
    session: &VerifiedSession,
    now: i64,
) -> Result<(), CsrfTokenError> {
    let key = configured_csrf_key().ok_or(CsrfTokenError::Invalid)?;
    verify_csrf_token_with_key(&key, token, session, now)
}

fn verify_csrf_token_with_key(
    key: &[u8; 32],
    token: &str,
    session: &VerifiedSession,
    now: i64,
) -> Result<(), CsrfTokenError> {
    let (version, encoded_payload, encoded_signature) = parse_csrf_token(token)?;
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(encoded_payload)
        .map_err(|_| CsrfTokenError::Malformed)?;
    let supplied_signature = URL_SAFE_NO_PAD
        .decode(encoded_signature)
        .map_err(|_| CsrfTokenError::Malformed)?;
    if supplied_signature.len() != 32 {
        return Err(CsrfTokenError::Malformed);
    }

    let signed_body = format!("{version}.{encoded_payload}");
    let mut mac = HmacSha256::new_from_slice(key).map_err(|_| CsrfTokenError::Invalid)?;
    mac.update(signed_body.as_bytes());
    // `verify_slice` performs a constant-time MAC comparison. Do not replace
    // this with `==`; the CSRF token is a bearer secret in the browser.
    mac.verify_slice(&supplied_signature)
        .map_err(|_| CsrfTokenError::Invalid)?;

    let payload: CsrfTokenPayload =
        serde_json::from_slice(&payload_bytes).map_err(|_| CsrfTokenError::Malformed)?;
    if payload.v != CSRF_TOKEN_VERSION {
        return Err(CsrfTokenError::Malformed);
    }
    let nonce = URL_SAFE_NO_PAD
        .decode(&payload.n)
        .map_err(|_| CsrfTokenError::Malformed)?;
    if nonce.len() != CSRF_NONCE_BYTES {
        return Err(CsrfTokenError::Malformed);
    }

    let supplied_session = URL_SAFE_NO_PAD
        .decode(&payload.sid)
        .map_err(|_| CsrfTokenError::Malformed)?;
    if supplied_session.len() != 32 {
        return Err(CsrfTokenError::Malformed);
    }

    // The session binding is authenticated above. Use constant-time equality
    // for both the JWT signature digest and epoch before accepting it.
    let same_session = bool::from(supplied_session.as_slice().ct_eq(&session.session_id));
    let same_epoch = bool::from(payload.tv.to_be_bytes().ct_eq(&session.epoch.to_be_bytes()));
    if !same_session || !same_epoch {
        return Err(CsrfTokenError::Invalid);
    }

    if payload.exp <= payload.iat
        || payload
            .exp
            .checked_sub(payload.iat)
            .map(|lifetime| lifetime > CSRF_TOKEN_TTL_SECS)
            .unwrap_or(true)
        || payload.iat > now.saturating_add(CSRF_CLOCK_SKEW_SECS)
    {
        return Err(CsrfTokenError::Malformed);
    }
    if payload.exp <= now {
        return Err(CsrfTokenError::Expired);
    }

    Ok(())
}

/// Extract raw JWT string preferring `auth_token` cookie over Bearer.
fn extract_raw_jwt(headers: &HeaderMap) -> Option<String> {
    // Prefer cookie when present so store key matches browser session even if
    // a stale Authorization header is also sent.
    if let Some(cookie_header) = headers.get(header::COOKIE) {
        if let Ok(cookies) = cookie_header.to_str() {
            for cookie in cookies.split(';') {
                if let Some((name, value)) = cookie.trim().split_once('=') {
                    if name == AUTH_TOKEN_COOKIE {
                        let value = value.trim();
                        if !value.is_empty() && value != "deleted" {
                            return Some(value.to_string());
                        }
                    }
                }
            }
        }
    }

    if let Some(auth_header) = headers.get("Authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                let token = token.trim();
                if !token.is_empty() {
                    return Some(token.to_string());
                }
            }
        }
    }

    None
}

/// Extract the stable session identifier retained for compatibility with the
/// old unit-level helper. The stateless token itself stores only a SHA-256
/// digest of this verified signature segment.
fn extract_session_id(headers: &HeaderMap) -> Option<String> {
    let token = extract_raw_jwt(headers)?;
    session_id_from_verified_jwt(&token)
}

/// Verify JWT and return the session binding plus durable epoch carried by its
/// signed claims. Unverified JWT shape is never accepted as CSRF authority.
fn extract_session_context(headers: &HeaderMap) -> Option<VerifiedSession> {
    let token = extract_raw_jwt(headers)?;
    verified_session_from_jwt(&token)
}

fn session_id_from_verified_jwt(token: &str) -> Option<String> {
    let sig = jwt_signature_segment(token)?;
    verified_session_from_jwt(token).map(|_| sig)
}

fn verified_session_from_jwt(token: &str) -> Option<VerifiedSession> {
    let sig = jwt_signature_segment(token)?;
    let jwt_secret = env::var("JWT_SECRET")
        .ok()
        .filter(|secret| !secret.is_empty())?;
    let claims = decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .ok()?;
    let digest = Sha256::digest(sig.as_bytes());
    let mut session_id = [0u8; 32];
    session_id.copy_from_slice(&digest);
    Some(VerifiedSession {
        session_id,
        epoch: claims.claims.tv,
    })
}

/// True for methods that can mutate server state (CSRF surface).
pub(crate) fn is_state_changing_method(method: &Method) -> bool {
    matches!(
        method,
        &Method::POST | &Method::PUT | &Method::PATCH | &Method::DELETE
    )
}

/// Cookie name used for browser JWT sessions (`auth_local` / OAuth).
pub(crate) const AUTH_TOKEN_COOKIE: &str = "auth_token";

/// True when the request carries an `auth_token` cookie (browser session).
///
/// Browsers auto-attach cookies on cross-site navigations/forms; that is the
/// classic CSRF risk. Pure `Authorization: Bearer` clients do not auto-send
/// cookies and are not the same attack surface.
pub(crate) fn has_auth_token_cookie(headers: &HeaderMap) -> bool {
    let Some(cookie_header) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    for cookie in cookie_header.split(';') {
        if let Some((name, value)) = cookie.trim().split_once('=') {
            if name == AUTH_TOKEN_COOKIE {
                let value = value.trim();
                // Require JWT-shaped value so an empty/deleted cookie does not
                // force CSRF on guests clearing session.
                return jwt_signature_segment(value).is_some();
            }
        }
    }
    false
}

fn jwt_signature_segment(token: &str) -> Option<String> {
    let mut segments = token.split('.');
    let header = segments.next()?;
    let payload = segments.next()?;
    let signature = segments.next()?;
    if header.is_empty() || payload.is_empty() || signature.is_empty() || segments.next().is_some()
    {
        return None;
    }
    Some(signature.to_string())
}

/// Whether CSRF validation must run for this request.
///
/// Policy (defense in depth for Cookie JWT):
/// 1. Only state-changing methods
/// 2. Path not on the hard exempt list
/// 3. Request has an `auth_token` **cookie** (browser session)
///
/// Agent routes are **not** path-exempt: cookie sessions must present
/// `X-CSRF-Token`. Bearer-only callers skip CSRF (no auto cookie attach).
pub(crate) fn csrf_check_needed(path: &str, method: &Method, headers: &HeaderMap) -> bool {
    if !is_state_changing_method(method) {
        return false;
    }
    if is_csrf_exempt(path) {
        return false;
    }
    has_auth_token_cookie(headers)
}

/// CSRF 防护中间件
/// Cookie 会话的状态变更必须带有效 CSRF Token
///
/// 安全策略：
/// - Cookie JWT 用户：状态变更必须提供有效的 CSRF Token（含 `/api/agent/*`）
/// - 纯 Bearer / 游客：跳过 CSRF（无 cookie 自动附带）
/// - 登录、健康检查、公开 proxy 等路径硬豁免
pub async fn csrf_middleware(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let headers = req.headers();

    if !csrf_check_needed(&path, &method, headers) {
        return next.run(req).await;
    }

    // Cookie session present — bind to the same verified JWT signature and
    // epoch used by the browser session (cookie preferred when present for
    // session continuity).
    let Some(session) = extract_session_context(headers) else {
        // Cookie parse edge case: has_auth_token_cookie true but signature
        // extract failed — fail closed.
        tracing::warn!("🚨 CSRF check failed: auth cookie present but session id missing");
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "CSRF token not found",
                "message": "No CSRF token found for this session. Please refresh the page."
            })),
        )
            .into_response();
    };

    // 从请求头获取 CSRF Token
    let client_token = headers
        .get("X-CSRF-Token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if client_token.is_empty() {
        tracing::warn!(
            "🚨 CSRF check failed: Missing X-CSRF-Token header on {}",
            path
        );
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "CSRF token missing",
                "message": "X-CSRF-Token header is required for state-changing operations"
            })),
        )
            .into_response();
    }

    // Verify the signed, expiring token without consulting process-local or
    // database state. Every instance can validate the same token with the
    // shared JWT secret.
    match verify_csrf_token(client_token, &session, unix_now()) {
        Ok(()) => {
            tracing::debug!("✅ CSRF check passed for {}", path);
            next.run(req).await
        }
        Err(CsrfTokenError::Expired) => {
            tracing::warn!("🚨 CSRF check failed: Token expired");
            (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "CSRF token expired",
                    "message": "Please refresh the page and try again"
                })),
            )
                .into_response()
        }
        Err(_) => {
            tracing::warn!("🚨 CSRF check failed: Token invalid");
            (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "CSRF token invalid",
                    "message": "Invalid CSRF token. Please refresh the page and try again."
                })),
            )
                .into_response()
        }
    }
}

/// 检查路径是否不需要 CSRF 保护（硬豁免，与认证方式无关）。
///
/// **Agent is not exempt** — cookie sessions must send `X-CSRF-Token`.
/// Frontend `agent/sseTransport` already attaches CSRF on POST.
pub(crate) fn is_csrf_exempt(path: &str) -> bool {
    // 公开接口、登录接口、健康检查等不需要 CSRF 保护
    path.starts_with("/api/auth/login")
        || path.starts_with("/api/auth/logout") // 退出登录不需要 CSRF（已经在退出了）
        || path.starts_with("/api/setup/")
        || path.starts_with("/health")
        || path.starts_with("/api/proxy/") // 图片代理等公开接口
        || path.starts_with("/api/ai/") // AI 推荐等公开接口
        // 仅公开写入埋点；export/import/summary 需会话 + CSRF（admin）
        || path.starts_with("/api/analytics/collect")
        || path.starts_with("/api/analytics/pageview")
        // HMAC-authenticated inbound routes ignore cookies; other programs
        // must not need a browser CSRF token.
        || path.starts_with("/tapi/")
    // 注意: /api/agent/、/api/tapps/、/api/tapp/ 不在豁免列表
    // Cookie 会话必须带 CSRF；纯 Bearer / 游客由 csrf_check_needed 跳过
}

/// 生成并返回 CSRF Token 的接口
/// GET /api/csrf-token
///
/// **Contract (durable guest UX):** no session → **HTTP 200**
/// `{ "csrf_token": null }` (not 401). Guests do not need CSRF tokens;
/// middleware already skips CSRF checks when there is no session.
pub async fn get_csrf_token(headers: HeaderMap) -> impl IntoResponse {
    let session = match extract_session_context(&headers) {
        Some(session) => session,
        None => {
            tracing::debug!("[csrf] no session — returning null token (guest probe)");
            return (
                StatusCode::OK,
                Json(json!({
                    "csrf_token": null
                })),
            )
                .into_response();
        }
    };

    // A fresh signed token is safe to issue on every probe: validation is
    // entirely stateless, so no process-local authority or eviction task is
    // required. The response keeps the existing `expires_in` contract.
    let Some(token) = issue_csrf_token(&session, unix_now()) else {
        tracing::error!("CSRF token signing unavailable");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "CSRF token unavailable",
                "message": "CSRF token signing is not configured"
            })),
        )
            .into_response();
    };

    (
        StatusCode::OK,
        Json(json!({
            "csrf_token": token,
            "expires_in": CSRF_TOKEN_TTL_SECS
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use jsonwebtoken::{EncodingKey, Header, encode};
    use std::sync::Once;

    static INIT_JWT: Once = Once::new();

    fn ensure_jwt_secret() {
        INIT_JWT.call_once(|| {
            if env::var("JWT_SECRET")
                .map(|secret| secret.is_empty())
                .unwrap_or(true)
            {
                // SAFETY: tests single-process; set once before concurrent use.
                env::set_var("JWT_SECRET", "csrf-unit-test-jwt-secret-key-32b");
            }
        });
    }

    fn mint_test_jwt(sub: &str, username: &str) -> String {
        mint_test_jwt_with_epoch(sub, username, 0)
    }

    fn mint_test_jwt_with_epoch(sub: &str, username: &str, epoch: i64) -> String {
        ensure_jwt_secret();
        let secret = env::var("JWT_SECRET").expect("JWT_SECRET");
        let now = chrono::Utc::now().timestamp();
        let claims = Claims {
            sub: sub.to_string(),
            username: username.to_string(),
            is_admin: false,
            is_owner: false,
            exp: now + 3600,
            iat: now,
            tv: epoch,
        };
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .expect("encode jwt")
    }

    #[test]
    fn csrf_token_is_versioned_and_randomized_without_process_state() {
        ensure_jwt_secret();
        let session = verified_session_from_jwt(&mint_test_jwt("1", "alice")).expect("session");
        let now = unix_now();
        let token1 = issue_csrf_token(&session, now).expect("token 1");
        let token2 = issue_csrf_token(&session, now).expect("token 2");

        assert!(token1.starts_with("v1."));
        assert!(token1.len() < CSRF_TOKEN_MAX_LEN);
        assert_ne!(token1, token2);
        assert!(verify_csrf_token(&token1, &session, now).is_ok());
        assert!(verify_csrf_token(&token2, &session, now).is_ok());
    }

    #[test]
    fn csrf_token_tamper_is_rejected() {
        ensure_jwt_secret();
        let session = verified_session_from_jwt(&mint_test_jwt("2", "bob")).expect("session");
        let token = issue_csrf_token(&session, unix_now()).expect("token");
        let mut tampered = token.clone();
        let signature_start = tampered.rfind('.').expect("signature separator") + 1;
        let replacement = if tampered.as_bytes()[signature_start] == b'A' {
            'B'
        } else {
            'A'
        };
        tampered.replace_range(
            signature_start..signature_start + 1,
            &replacement.to_string(),
        );

        assert_eq!(
            verify_csrf_token(&tampered, &session, unix_now()),
            Err(CsrfTokenError::Invalid)
        );
    }

    #[test]
    fn csrf_token_is_bound_to_session_and_epoch() {
        ensure_jwt_secret();
        let session_a = verified_session_from_jwt(&mint_test_jwt("3", "carol")).expect("session");
        let session_b = verified_session_from_jwt(&mint_test_jwt("4", "dave")).expect("session");
        let now = unix_now();
        let token = issue_csrf_token(&session_a, now).expect("token");

        assert_eq!(
            verify_csrf_token(&token, &session_b, now),
            Err(CsrfTokenError::Invalid)
        );
        let epoch_mismatch = VerifiedSession {
            session_id: session_a.session_id,
            epoch: session_a.epoch + 1,
        };
        assert_eq!(
            verify_csrf_token(&token, &epoch_mismatch, now),
            Err(CsrfTokenError::Invalid)
        );
    }

    #[test]
    fn csrf_token_expiry_is_checked_from_signed_unix_time() {
        ensure_jwt_secret();
        let session = verified_session_from_jwt(&mint_test_jwt("5", "erin")).expect("session");
        let now = unix_now();
        let token = issue_csrf_token(&session, now - CSRF_TOKEN_TTL_SECS - 1).expect("token");

        assert_eq!(
            verify_csrf_token(&token, &session, now),
            Err(CsrfTokenError::Expired)
        );
    }

    #[test]
    fn csrf_token_rejects_malformed_and_version_mismatch() {
        ensure_jwt_secret();
        let session = verified_session_from_jwt(&mint_test_jwt("6", "frank")).expect("session");
        let now = unix_now();

        for malformed in [
            "",
            "v1",
            "v1..sig",
            "v2.payload.signature",
            "v1.payload.sig.extra",
        ] {
            assert_eq!(
                verify_csrf_token(malformed, &session, now),
                Err(CsrfTokenError::Malformed),
                "malformed token should fail closed: {malformed:?}"
            );
        }

        let key = configured_csrf_key().expect("key");
        let payload = CsrfTokenPayload {
            v: CSRF_TOKEN_VERSION + 1,
            sid: URL_SAFE_NO_PAD.encode(session.session_id),
            tv: session.epoch,
            iat: now,
            exp: now + CSRF_TOKEN_TTL_SECS,
            n: random_csrf_nonce(),
        };
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).expect("payload"));
        let body = format!("{CSRF_TOKEN_VERSION_PREFIX}.{encoded}");
        let signature = sign_csrf_body(&key, &body).expect("signature");
        let mismatched = format!("{body}.{}", URL_SAFE_NO_PAD.encode(signature));
        assert_eq!(
            verify_csrf_token(&mismatched, &session, now),
            Err(CsrfTokenError::Malformed)
        );
    }

    #[test]
    fn csrf_token_verifies_after_fresh_verifier_state() {
        ensure_jwt_secret();
        let session = verified_session_from_jwt(&mint_test_jwt("7", "grace")).expect("session");
        let token = issue_csrf_token(&session, unix_now()).expect("token");

        // There is deliberately no store to carry over. Re-derive the key as
        // a fresh instance/restart would, then verify the prior token.
        let secret = env::var("JWT_SECRET").expect("JWT_SECRET");
        let fresh_key = csrf_signing_key(&secret).expect("fresh key");
        assert_eq!(fresh_key, configured_csrf_key().expect("configured key"));
        assert!(verify_csrf_token_with_key(&fresh_key, &token, &session, unix_now()).is_ok());
    }

    #[tokio::test]
    async fn csrf_token_guest_returns_200_with_null_token() {
        let headers = HeaderMap::new();
        let response = get_csrf_token(headers).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), 1024).await.expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert!(value.get("csrf_token").is_some());
        assert!(value["csrf_token"].is_null());
        assert!(value.get("error").is_none());
    }

    #[test]
    fn session_id_requires_verified_jwt_not_shape_alone() {
        ensure_jwt_secret();
        // Unverified three-part garbage must not become a session context.
        let mut forged = HeaderMap::new();
        forged.insert(
            "Authorization",
            "Bearer same.header.signature-one".parse().unwrap(),
        );
        assert!(extract_session_id(&forged).is_none());

        let jwt_a = mint_test_jwt("1", "alice");
        // Distinct iat is not guaranteed if called same second; mint with delay via different sub.
        std::thread::sleep(std::time::Duration::from_millis(5));
        let jwt_b = mint_test_jwt("2", "bob");

        let mut first = HeaderMap::new();
        first.insert("Authorization", format!("Bearer {jwt_a}").parse().unwrap());
        let mut second = HeaderMap::new();
        second.insert("Authorization", format!("Bearer {jwt_b}").parse().unwrap());

        let id_a = extract_session_id(&first).expect("verified jwt a");
        let id_b = extract_session_id(&second).expect("verified jwt b");
        assert_ne!(id_a, id_b);
        // Compatibility helper returns the exact signature segment; signed
        // CSRF payloads retain only its fixed-size digest.
        assert_eq!(id_a, jwt_signature_segment(&jwt_a).unwrap());
        assert_eq!(id_b, jwt_signature_segment(&jwt_b).unwrap());
    }

    #[tokio::test]
    async fn get_csrf_token_issues_for_verified_jwt_cookie() {
        let jwt = mint_test_jwt("42", "csrf-user");
        let headers = cookie_headers(&jwt);
        let response = get_csrf_token(headers).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 2048).await.expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        let token = value["csrf_token"].as_str().expect("csrf_token string");
        assert!(token.starts_with("v1."));
        assert_eq!(value["expires_in"].as_i64(), Some(CSRF_TOKEN_TTL_SECS));

        let session = verified_session_from_jwt(&jwt).expect("session");
        assert!(verify_csrf_token(token, &session, unix_now()).is_ok());

        // Stateless issuance does not depend on the first instance's memory.
        let headers2 = cookie_headers(&jwt);
        let response2 = get_csrf_token(headers2).await.into_response();
        let body2 = to_bytes(response2.into_body(), 2048).await.expect("body");
        let value2: serde_json::Value = serde_json::from_slice(&body2).expect("json");
        let token2 = value2["csrf_token"].as_str().expect("csrf_token string");
        assert!(verify_csrf_token(token2, &session, unix_now()).is_ok());
    }

    #[tokio::test]
    async fn get_csrf_token_rejects_forged_jwt_shape() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("{AUTH_TOKEN_COOKIE}=aaa.bbb.forged-sig")
                .parse()
                .unwrap(),
        );
        let response = get_csrf_token(headers).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024).await.expect("body");
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        // Forged cookie is not a session — guest probe contract.
        assert!(value["csrf_token"].is_null());
    }

    #[test]
    fn test_is_csrf_exempt() {
        assert!(is_csrf_exempt("/api/auth/login"));
        assert!(is_csrf_exempt("/api/setup/init-database"));
        assert!(is_csrf_exempt("/health"));
        assert!(is_csrf_exempt("/api/proxy/image"));
        assert!(is_csrf_exempt("/api/analytics/collect"));
        assert!(is_csrf_exempt("/api/analytics/pageview"));
        assert!(is_csrf_exempt("/tapi/com.example.app/sponsors"));
        assert!(!is_csrf_exempt("/api/analytics/summary"));
        assert!(!is_csrf_exempt("/api/analytics/export"));
        assert!(!is_csrf_exempt("/api/analytics/import"));

        // Tapp API 不在豁免列表（通过 session 检查决定是否需要 CSRF）
        assert!(!is_csrf_exempt("/api/auth/oauth/github/callback"));
        assert!(!is_csrf_exempt("/api/tapp/ai/v2/tasks"));
        assert!(!is_csrf_exempt("/api/tapps/install"));
        assert!(!is_csrf_exempt("/api/tapps/my-app/start"));
        assert!(!is_csrf_exempt("/api/tapps/my-app/credentials/wegame"));

        assert!(!is_csrf_exempt("/api/config"));
        assert!(!is_csrf_exempt("/api/auth/change-password"));

        // Agent is NOT path-exempt (cookie sessions need CSRF).
        assert!(!is_csrf_exempt("/api/agent/process"));
        assert!(!is_csrf_exempt("/api/agent/process/stream"));
        assert!(!is_csrf_exempt("/api/agent/confirm"));
        assert!(!is_csrf_exempt("/api/agent/presets"));
    }

    fn cookie_headers(jwt: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            format!("{AUTH_TOKEN_COOKIE}={jwt}").parse().unwrap(),
        );
        h
    }

    fn bearer_headers(jwt: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("Authorization", format!("Bearer {jwt}").parse().unwrap());
        h
    }

    #[test]
    fn has_auth_token_cookie_detects_jwt_shaped_cookie() {
        let jwt = "aaa.bbb.signaturecookie";
        assert!(has_auth_token_cookie(&cookie_headers(jwt)));
        assert!(!has_auth_token_cookie(&bearer_headers(jwt)));
        assert!(!has_auth_token_cookie(&HeaderMap::new()));
        // Deleted / empty cookie must not force CSRF.
        let mut empty = HeaderMap::new();
        empty.insert(
            header::COOKIE,
            format!("{AUTH_TOKEN_COOKIE}=deleted").parse().unwrap(),
        );
        assert!(!has_auth_token_cookie(&empty));
    }

    #[test]
    fn csrf_check_needed_cookie_session_on_agent_post() {
        let jwt = "hdr.pay.sig-agent";
        let headers = cookie_headers(jwt);
        assert!(csrf_check_needed(
            "/api/agent/process",
            &Method::POST,
            &headers
        ));
        assert!(csrf_check_needed(
            "/api/agent/confirm/stream",
            &Method::POST,
            &headers
        ));
        // GET never needs CSRF
        assert!(!csrf_check_needed(
            "/api/agent/sessions",
            &Method::GET,
            &headers
        ));
    }

    #[test]
    fn csrf_check_needed_bearer_only_skips_even_on_agent() {
        let jwt = "hdr.pay.sig-bearer";
        let headers = bearer_headers(jwt);
        assert!(!csrf_check_needed(
            "/api/agent/process",
            &Method::POST,
            &headers
        ));
        assert!(!csrf_check_needed("/api/config", &Method::PUT, &headers));
    }

    #[test]
    fn csrf_check_needed_guest_and_exempt_paths() {
        let empty = HeaderMap::new();
        assert!(!csrf_check_needed(
            "/api/agent/process",
            &Method::POST,
            &empty
        ));
        let jwt = "hdr.pay.sig";
        let cookie = cookie_headers(jwt);
        // Hard exempt still wins even with cookie
        assert!(!csrf_check_needed(
            "/api/auth/login",
            &Method::POST,
            &cookie
        ));
        assert!(!csrf_check_needed(
            "/api/proxy/image",
            &Method::POST,
            &cookie
        ));
        assert!(!csrf_check_needed(
            "/tapi/com.example.app/sponsors",
            &Method::POST,
            &cookie
        ));
    }

    #[test]
    fn extract_session_id_prefers_cookie_over_bearer() {
        let cookie_jwt = mint_test_jwt("10", "cookie-user");
        std::thread::sleep(std::time::Duration::from_millis(5));
        let bearer_jwt = mint_test_jwt("11", "bearer-user");
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            format!("{AUTH_TOKEN_COOKIE}={cookie_jwt}").parse().unwrap(),
        );
        h.insert(
            "Authorization",
            format!("Bearer {bearer_jwt}").parse().unwrap(),
        );
        assert_eq!(
            extract_session_id(&h).as_deref(),
            jwt_signature_segment(&cookie_jwt).as_deref()
        );
        assert_ne!(
            extract_session_id(&h).as_deref(),
            jwt_signature_segment(&bearer_jwt).as_deref()
        );
    }
}
