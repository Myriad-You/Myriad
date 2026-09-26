use axum::Json;
use axum::extract::Request;
#[cfg(test)]
use axum::http::header;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::env;
use subtle::ConstantTimeEq;

use crate::middleware::auth::{CredentialSource, SessionCredential};

type HmacSha256 = Hmac<Sha256>;

/// Wire format version for stateless browser CSRF tokens.
///
/// The version is present both as the token prefix and in the signed payload.
/// Keeping both copies makes version transitions explicit and prevents a token
/// from one format being interpreted as another format after a rolling deploy.
const CSRF_TOKEN_VERSION: u8 = 1;
const CSRF_TOKEN_VERSION_PREFIX: &str = "v1";
/// One-hour browser/server lifetime. The frontend refreshes a little before
/// this deadline; the signed expiry remains authoritative.
pub(crate) const CSRF_TOKEN_TTL_SECS: i64 = 60 * 60;
const CSRF_CLOCK_SKEW_SECS: i64 = 60;
const CSRF_NONCE_BYTES: usize = 32;
/// Reject tokens longer than 1024 bytes before decode/HMAC.
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
    /// Authenticated 32-byte nonce so two issuances at the same `iat` still differ.
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
    let secret = crate::middleware::auth::session_secret()?;
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

/// Session id used by the CSRF unit tests: the verified JWT signature segment.
///
/// 生产路径走 `extract_session_context`；这一对只服务本文件的 #[cfg(test)]，
/// 它们锁的是「未经验签的 JWT 形状永远不能当 CSRF 凭据」这条不变量。
#[cfg(test)]
fn extract_session_id(headers: &HeaderMap) -> Option<String> {
    let credential = SessionCredential::from_headers(headers)?;
    session_id_from_verified_jwt(credential.token)
}

#[cfg(test)]
fn session_id_from_verified_jwt(token: &str) -> Option<String> {
    let sig = jwt_signature_segment(token)?;
    verified_session_from_jwt(token).map(|_| sig)
}

/// Verify the credential the auth layer selects for this request and return
/// its session binding plus the durable epoch carried by its signed claims.
/// Unverified JWT shape is never accepted as CSRF authority.
///
/// The CSRF layer runs before route-level authentication, so it cannot read
/// the authenticated claims; it applies the same [`SessionCredential`]
/// selection instead, which keeps both layers on one credential.
fn extract_session_context(headers: &HeaderMap) -> Option<VerifiedSession> {
    let credential = SessionCredential::from_headers(headers)?;
    verified_session_from_jwt(credential.token)
}

fn verified_session_from_jwt(token: &str) -> Option<VerifiedSession> {
    let sig = jwt_signature_segment(token)?;
    let claims = crate::middleware::auth::verify_signed(token).ok()?;
    let digest = Sha256::digest(sig.as_bytes());
    let mut session_id = [0u8; 32];
    session_id.copy_from_slice(&digest);
    Some(VerifiedSession {
        session_id,
        epoch: claims.tv,
    })
}

/// True for methods that can mutate server state (CSRF surface).
pub(crate) fn is_state_changing_method(method: &Method) -> bool {
    matches!(
        method,
        &Method::POST | &Method::PUT | &Method::PATCH | &Method::DELETE
    )
}

/// True when the credential the auth layer uses for this request is a
/// JWT-shaped `auth_token` cookie (browser session).
///
/// Browsers auto-attach cookies on cross-site navigations/forms; that is the
/// classic CSRF risk. A request whose selected credential is
/// `Authorization: Bearer` acts as that token, which a cross-site page cannot
/// set, even when a cookie rides along. An empty or deleted cookie does not
/// force CSRF on guests clearing their session.
pub(crate) fn selects_cookie_session(headers: &HeaderMap) -> bool {
    SessionCredential::from_headers(headers).is_some_and(|credential| {
        credential.source == CredentialSource::Cookie
            && jwt_signature_segment(credential.token).is_some()
    })
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
/// 3. The credential auth selects is the `auth_token` **cookie** (browser
///    session; see [`SessionCredential`] for the priority)
///
/// Agent routes are **not** path-exempt: cookie sessions must present
/// `X-CSRF-Token`. Requests authenticated by `Authorization: Bearer` skip
/// CSRF (a cross-site page cannot set that header).
pub(crate) fn csrf_check_needed(path: &str, method: &Method, headers: &HeaderMap) -> bool {
    if !is_state_changing_method(method) {
        return false;
    }
    if is_csrf_exempt(path) {
        return false;
    }
    selects_cookie_session(headers)
}

/// CSRF 防护中间件
/// Cookie 会话的状态变更必须带有效 CSRF Token
///
/// 安全策略：
/// - Cookie JWT 用户：状态变更必须提供有效的 CSRF Token（含 `/api/agent/*`）
/// - Bearer 会话 / 游客：跳过 CSRF（跨站页面设不了 Authorization）
/// - 登录、健康检查、公开 proxy 等路径硬豁免
pub async fn csrf_middleware(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let headers = req.headers();

    if !csrf_check_needed(&path, &method, headers) {
        return next.run(req).await;
    }

    // Cookie session selected — bind to the same verified JWT signature and
    // epoch the auth layer will authenticate.
    let Some(session) = extract_session_context(headers) else {
        // JWT-shaped `auth_token` cookie present but JWT verify failed — fail closed.
        tracing::warn!("🚨 CSRF check failed: auth cookie present but session id missing");
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "CSRF token not found",
                "code": "csrf_failed",
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
                "code": "csrf_failed",
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
                    "code": "csrf_failed",
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
                    "code": "csrf_failed",
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
        || path.starts_with("/api/auth/logout") // Logout is hard-exempt.
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
                "code": "csrf_failed",
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
    use crate::middleware::auth::{AUTH_TOKEN_COOKIE, Claims};
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
                unsafe { env::set_var("JWT_SECRET", "csrf-unit-test-jwt-secret-key-32b") };
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
            subject: crate::middleware::auth::AuthSubject::from_test_sub(sub),
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
        // Test helper returns the JWT signature segment; CSRF `sid` is its SHA-256 digest.
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

        // 这些路径不在豁免列表；Cookie 会话仍需 CSRF。
        assert!(!is_csrf_exempt("/api/auth/oauth/github/callback"));
        assert!(!is_csrf_exempt("/api/tapp/ai/v2/tasks"));
        assert!(!is_csrf_exempt("/api/tapps/install"));
        assert!(!is_csrf_exempt("/api/tapps/my-app/start"));
        assert!(!is_csrf_exempt("/api/tapps/my-app/credentials/wegame"));
        assert!(!is_csrf_exempt("/api/tapps/my-app/private/token"));

        assert!(!is_csrf_exempt("/api/config"));
        assert!(!is_csrf_exempt("/api/auth/change-password"));

        // Agent is NOT path-exempt (cookie sessions need CSRF).
        assert!(!is_csrf_exempt("/api/agent/process"));
        assert!(!is_csrf_exempt("/api/agent/process/stream"));
        assert!(!is_csrf_exempt("/api/agent/confirm/stream"));
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
    fn selects_cookie_session_detects_jwt_shaped_cookie() {
        let jwt = "aaa.bbb.signaturecookie";
        assert!(selects_cookie_session(&cookie_headers(jwt)));
        assert!(!selects_cookie_session(&bearer_headers(jwt)));
        assert!(!selects_cookie_session(&HeaderMap::new()));
        // Deleted / empty cookie must not force CSRF.
        let mut empty = HeaderMap::new();
        empty.insert(
            header::COOKIE,
            format!("{AUTH_TOKEN_COOKIE}=deleted").parse().unwrap(),
        );
        assert!(!selects_cookie_session(&empty));
    }

    #[test]
    fn csrf_check_needed_cookie_session_on_tapp_private_writes() {
        let jwt = "hdr.pay.sig-private";
        let cookie = cookie_headers(jwt);
        let bearer = bearer_headers(jwt);
        let empty = HeaderMap::new();
        let item = "/api/tapps/com.example.app/private/token";
        let collection = "/api/tapps/com.example.app/private";
        let shared = "/api/tapps/com.example.app/shared/posts";

        assert!(csrf_check_needed(item, &Method::POST, &cookie));
        assert!(csrf_check_needed(item, &Method::DELETE, &cookie));
        assert!(csrf_check_needed(collection, &Method::DELETE, &cookie));
        assert!(csrf_check_needed(shared, &Method::POST, &cookie));

        assert!(!csrf_check_needed(item, &Method::GET, &cookie));
        assert!(!csrf_check_needed(collection, &Method::GET, &cookie));
        assert!(!csrf_check_needed(item, &Method::POST, &bearer));
        assert!(!csrf_check_needed(item, &Method::POST, &empty));
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

    /// CSRF binds to exactly the session the auth layer authenticates, for
    /// every way a request can present credentials, and demands a token
    /// exactly when that session came from the cookie.
    #[test]
    fn csrf_binds_to_the_credential_auth_selects() {
        let cookie_jwt = mint_test_jwt("10", "cookie-user");
        std::thread::sleep(std::time::Duration::from_millis(5));
        let bearer_jwt = mint_test_jwt("11", "bearer-user");
        let jwt_for = |sub: &str| match sub {
            "10" => cookie_jwt.clone(),
            "11" => bearer_jwt.clone(),
            other => panic!("unexpected subject {other}"),
        };

        let cookie = format!("{AUTH_TOKEN_COOKIE}={cookie_jwt}");
        let bearer = format!("Bearer {bearer_jwt}");
        let cases: Vec<(&str, Vec<(header::HeaderName, String)>)> = vec![
            ("cookie only", vec![(header::COOKIE, cookie.clone())]),
            ("bearer only", vec![(header::AUTHORIZATION, bearer.clone())]),
            (
                "bearer and cookie",
                vec![
                    (header::COOKIE, cookie.clone()),
                    (header::AUTHORIZATION, bearer.clone()),
                ],
            ),
            (
                "non-bearer scheme and cookie",
                vec![
                    (header::COOKIE, cookie.clone()),
                    (header::AUTHORIZATION, "Basic dXNlcjpwYXNz".into()),
                ],
            ),
            (
                "tombstone first",
                vec![(
                    header::COOKIE,
                    format!("{AUTH_TOKEN_COOKIE}=deleted; {cookie}"),
                )],
            ),
            (
                "split cookie headers",
                vec![
                    (header::COOKIE, "other=1".into()),
                    (header::COOKIE, cookie.clone()),
                ],
            ),
            (
                "empty bearer and cookie",
                vec![
                    (header::COOKIE, cookie.clone()),
                    (header::AUTHORIZATION, "Bearer ".into()),
                ],
            ),
        ];

        for (name, pairs) in cases {
            let mut headers = HeaderMap::new();
            for (header_name, value) in pairs {
                headers.append(header_name, value.parse().unwrap());
            }
            let auth = crate::middleware::auth::verify_request_signature(&headers);
            let csrf = extract_session_context(&headers);
            match auth {
                Ok(claims) => {
                    let expected = verified_session_from_jwt(&jwt_for(&claims.sub));
                    assert_eq!(csrf, expected, "{name}: CSRF bound a different session");
                    let auth_used_cookie = claims.sub == "10";
                    assert_eq!(
                        csrf_check_needed("/api/config", &Method::POST, &headers),
                        auth_used_cookie,
                        "{name}: CSRF requirement must follow the selected credential"
                    );
                }
                Err(_) => {
                    assert!(csrf.is_none(), "{name}: CSRF accepted what auth rejects");
                    assert!(
                        !csrf_check_needed("/api/config", &Method::POST, &headers),
                        "{name}: a rejected Bearer is not a cookie session"
                    );
                }
            }
        }
    }

    #[test]
    fn extract_session_id_prefers_bearer_over_cookie() {
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
            jwt_signature_segment(&bearer_jwt).as_deref()
        );
        assert!(!csrf_check_needed("/api/config", &Method::POST, &h));
    }

    /// A request authenticated by Bearer acts as that token even with a
    /// session cookie riding along, so the cookie does not demand a CSRF token.
    #[tokio::test]
    async fn bearer_selected_post_with_cookie_needs_no_csrf_token() {
        use axum::Router;
        use axum::body::Body;
        use axum::middleware::from_fn;
        use axum::routing::post;
        use tower::ServiceExt;

        async fn ok() -> &'static str {
            "ok"
        }

        let app = Router::new()
            .route("/api/config", post(ok))
            .layer(from_fn(csrf_middleware));
        let cookie_jwt = mint_test_jwt("30", "cookie-user");
        let bearer_jwt = mint_test_jwt("31", "bearer-user");
        let request = |bearer: bool| {
            let mut builder = Request::builder()
                .method("POST")
                .uri("/api/config")
                .header(header::COOKIE, format!("{AUTH_TOKEN_COOKIE}={cookie_jwt}"));
            if bearer {
                builder = builder.header(header::AUTHORIZATION, format!("Bearer {bearer_jwt}"));
            }
            builder.body(Body::empty()).unwrap()
        };

        let with_bearer = app.clone().oneshot(request(true)).await.expect("response");
        assert_eq!(with_bearer.status(), StatusCode::OK);
        let cookie_only = app.oneshot(request(false)).await.expect("response");
        assert_eq!(cookie_only.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn cookie_private_post_without_csrf_header_is_forbidden() {
        use axum::Router;
        use axum::body::Body;
        use axum::middleware::from_fn;
        use axum::routing::get;
        use tower::ServiceExt;

        async fn ok() -> &'static str {
            "ok"
        }

        let app = Router::new()
            .route("/api/tapps/{tapp_id}/private/{key}", get(ok).post(ok))
            .layer(from_fn(csrf_middleware));
        let jwt = mint_test_jwt("21", "private-writer");

        let denied = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/tapps/com.example.app/private/token")
                    .header(header::COOKIE, format!("{AUTH_TOKEN_COOKIE}={jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        let denied_body = to_bytes(denied.into_body(), 2048).await.expect("body");
        let denied_json: serde_json::Value = serde_json::from_slice(&denied_body).expect("json");
        assert_eq!(denied_json["error"], "CSRF token missing");

        let allowed_get = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/tapps/com.example.app/private/token")
                    .header(header::COOKIE, format!("{AUTH_TOKEN_COOKIE}={jwt}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");
        assert_eq!(allowed_get.status(), StatusCode::OK);

        let session = verified_session_from_jwt(&jwt).expect("session");
        let token = issue_csrf_token(&session, unix_now()).expect("csrf");
        let allowed_post = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/tapps/com.example.app/private/token")
                    .header(header::COOKIE, format!("{AUTH_TOKEN_COOKIE}={jwt}"))
                    .header("X-CSRF-Token", token)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");
        assert_eq!(allowed_post.status(), StatusCode::OK);
    }
}
