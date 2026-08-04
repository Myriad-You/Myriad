//! OAuth CSRF state — HMAC-signed tokens shared by all providers.
//!
//! Process-local memory alone breaks multi-instance / restart deployments:
//! the callback lands on a different process that never saw `insert_state`.
//! Signed state embeds purpose + slug + expiry; any process with the shared
//! secret can verify. Optional in-memory nonces provide anti-replay within a
//! process lifetime; after restart a still-valid signature is accepted
//! (provider authorization codes remain one-time).
//!
//! **Browser binding (MYR-003):** the payload nonce `n` is also issued as an
//! HttpOnly cookie (`oauth_tx`). Callbacks must present the matching cookie so
//! a completed OAuth URL cannot be swapped onto another browser session.
//!
//! Token format (URL-safe):
//! `base64url(payload_json) + '.' + base64url(hmac_sha256)`
//!
//! Payload fields:
//! `v` (version), `n` (nonce hex / browser_tx / OIDC nonce), `s` (slug),
//! `p` (login|link|platform), `uid?`, `plat?`, `exp` (unix seconds),
//! `cv?` (PKCE code_verifier, MYR-011)
//!
//! Secret: `OAUTH_STATE_SECRET` if set, else `JWT_SECRET`.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::{HashMap, VecDeque};
use std::env;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use tokio::sync::RwLock;

type HmacSha256 = Hmac<Sha256>;

/// state 有效期 10 分钟
pub const STATE_TTL: Duration = Duration::from_secs(600);
/// 防内存耗尽（used-nonce 表）
const MAX_USED_NONCES: usize = 10_000;

#[derive(Debug, Clone, PartialEq)]
pub enum OAuthPurpose {
    Login,
    /// LinkAccount 时携带"当前已登录用户 id"
    LinkAccount(i32),
    /// 数据平台授权（如 Discord 同步）：写入平台 token，不登录/不绑 identity
    PlatformData {
        user_id: i32,
        platform: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredState {
    pub provider_slug: String,
    pub purpose: OAuthPurpose,
}

/// Result of [`issue_state`]: signed token plus browser-binding / OIDC secrets.
///
/// Callers must set [`OAUTH_TX_COOKIE`] to `browser_tx` on the login/link
/// redirect response so the callback can prove same-browser continuity.
/// OIDC providers also use `browser_tx` as the OpenID `nonce` and
/// `code_verifier` for PKCE S256 (MYR-011).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedState {
    pub token: String,
    /// High-entropy transaction id (payload `n`); mirror into `oauth_tx` cookie.
    /// Also the OIDC `nonce` value for id_token binding.
    pub browser_tx: String,
    /// RFC 7636 PKCE code_verifier (payload `cv`); OIDC only.
    pub code_verifier: String,
}

/// Successful verify of a signed state token.
///
/// [`Fresh`](ConsumeOutcome::Fresh) is the normal one-shot consume.
/// [`Replay`](ConsumeOutcome::Replay) means the HMAC/exp were valid but the
/// nonce was already marked used in this process (browser double-load / retry).
/// Handlers must **not** re-exchange the authorization code on Replay; they may
/// soft-succeed when side effects from the first request are already durable.
///
/// `browser_tx` is the payload nonce and must match the request's `oauth_tx`
/// cookie before any code exchange or account side effects.
#[derive(Debug, Clone, PartialEq)]
pub enum ConsumeOutcome {
    Fresh {
        stored: StoredState,
        browser_tx: String,
    },
    Replay {
        stored: StoredState,
        browser_tx: String,
    },
}

impl ConsumeOutcome {
    pub fn stored(&self) -> &StoredState {
        match self {
            Self::Fresh { stored, .. } | Self::Replay { stored, .. } => stored,
        }
    }

    /// Browser transaction nonce from the signed payload (cookie binding value).
    pub fn browser_tx(&self) -> &str {
        match self {
            Self::Fresh { browser_tx, .. } | Self::Replay { browser_tx, .. } => browser_tx,
        }
    }

    /// Consume the outcome, returning the verified [`StoredState`] regardless of Fresh/Replay.
    #[allow(dead_code)] // public helper for handlers/tests
    pub fn into_stored(self) -> StoredState {
        match self {
            Self::Fresh { stored, .. } | Self::Replay { stored, .. } => stored,
        }
    }

    pub fn is_replay(&self) -> bool {
        matches!(self, Self::Replay { .. })
    }
}

/// Cookie name for OAuth browser transaction binding (MYR-003).
///
/// Not `__Host-` prefixed: issuance must work on local HTTP (dev) as well as
/// production HTTPS; `__Host-` requires Secure always.
pub const OAUTH_TX_COOKIE: &str = "oauth_tx";

/// `Set-Cookie` value that stores the browser transaction id.
///
/// HttpOnly + SameSite=Lax + Path=/; Secure in production. Max-Age matches
/// [`STATE_TTL`] so a stale cookie cannot outlive state acceptance.
pub fn oauth_tx_set_cookie_value(browser_tx: &str, is_production: bool) -> String {
    format!(
        "{OAUTH_TX_COOKIE}={browser_tx}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
        STATE_TTL.as_secs(),
        if is_production { "; Secure" } else { "" }
    )
}

/// `Set-Cookie` value that clears the OAuth transaction cookie.
pub fn oauth_tx_clear_cookie_value(is_production: bool) -> String {
    format!(
        "{OAUTH_TX_COOKIE}=deleted; Path=/; HttpOnly; SameSite=Lax; Max-Age=0; \
         Expires=Thu, 01 Jan 1970 00:00:00 GMT{}",
        if is_production { "; Secure" } else { "" }
    )
}

/// Read a single cookie value from a raw `Cookie` header string.
pub fn cookie_value_from_header<'a>(cookie_header: &'a str, name: &str) -> Option<&'a str> {
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some((n, v)) = part.split_once('=') {
            if n.trim() == name {
                let v = v.trim();
                if !v.is_empty() && v != "deleted" {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// True when the request `Cookie` header carries `oauth_tx` equal to `expected`
/// (constant-time compare when lengths match).
pub fn oauth_tx_cookie_matches(cookie_header: Option<&str>, expected: &str) -> bool {
    if expected.is_empty() {
        return false;
    }
    let Some(header) = cookie_header else {
        return false;
    };
    let Some(got) = cookie_value_from_header(header, OAUTH_TX_COOKIE) else {
        return false;
    };
    let a = got.as_bytes();
    let b = expected.as_bytes();
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

/// Why `consume_state` failed — distinguishes never-seen vs TTL-elapsed vs
/// unrecoverable replay (when handlers surface `state_replay` after soft-recover fails).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumeStateError {
    /// Token missing/malformed/bad signature.
    Missing,
    /// Signature ok but past `exp`.
    Expired,
    /// Valid signature but nonce already used — used as a redirect/error code
    /// when soft-recover cannot confirm the first attempt succeeded.
    /// `consume_state` itself returns [`ConsumeOutcome::Replay`] with payload
    /// instead of this error when the token is fully parseable.
    Replay,
}

impl ConsumeStateError {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Expired => "expired",
            Self::Replay => "replay",
        }
    }
}

/// Compact signed payload (field names kept short for URL size).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StatePayload {
    v: u8,
    n: String,
    s: String,
    p: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    uid: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    plat: Option<String>,
    exp: i64,
    /// PKCE code_verifier (MYR-011). Optional for backward-compat with in-flight
    /// states issued before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cv: Option<String>,
}

/// Process-local used-nonce table with O(1) oldest eviction (MYR-016 pattern).
struct UsedNonceStore {
    /// nonce → drop-after Instant
    map: HashMap<String, Instant>,
    /// Insertion order for cap eviction (oldest first).
    order: VecDeque<String>,
}

impl UsedNonceStore {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn len(&self) -> usize {
        self.map.len()
    }

    fn contains(&self, nonce: &str) -> bool {
        self.map.contains_key(nonce)
    }

    fn retain_live(&mut self, now: Instant) -> usize {
        let before = self.map.len();
        self.map.retain(|_, until| *until > now);
        // Drop order entries that no longer exist in the map.
        self.order.retain(|k| self.map.contains_key(k));
        before.saturating_sub(self.map.len())
    }

    /// Insert nonce with TTL. Returns `true` if newly inserted, `false` if already present.
    fn insert_if_absent(&mut self, nonce: String, until: Instant) -> bool {
        if self.map.contains_key(&nonce) {
            return false;
        }
        while self.map.len() >= MAX_USED_NONCES {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            } else {
                break;
            }
        }
        self.order.push_back(nonce.clone());
        self.map.insert(nonce, until);
        true
    }
}

/// Used nonces for optional anti-replay within this process.
/// Value = Instant when the entry may be dropped (exp + small grace).
static USED_NONCES: Lazy<Arc<RwLock<UsedNonceStore>>> = Lazy::new(|| {
    let store: Arc<RwLock<UsedNonceStore>> = Arc::new(RwLock::new(UsedNonceStore::new()));
    let store_clone = store.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            let mut nonces = store_clone.write().await;
            let removed = nonces.retain_live(Instant::now());
            if removed > 0 {
                tracing::debug!(
                    "🧹 OAuth used-nonce cleanup: removed {}, {} remain",
                    removed,
                    nonces.len()
                );
            }
        }
    });

    store
});

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn state_secret() -> Result<Vec<u8>, String> {
    env::var("OAUTH_STATE_SECRET")
        .or_else(|_| env::var("JWT_SECRET"))
        .map(|s| s.into_bytes())
        .map_err(|_| {
            "OAUTH_STATE_SECRET or JWT_SECRET must be set for OAuth state signing".to_string()
        })
}

fn random_nonce_hex() -> String {
    use rand::Rng;
    let mut buf = [0u8; 16];
    rand::rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

/// RFC 7636 code_verifier: 43 chars of URL-safe base64 (32 random bytes).
fn random_code_verifier() -> String {
    use rand::Rng;
    let mut buf = [0u8; 32];
    rand::rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

/// First 8 chars of state for safe logging (full token is a CSRF secret).
fn state_prefix(state: &str) -> &str {
    match state.char_indices().nth(8) {
        Some((i, _)) => &state[..i],
        None => state,
    }
}

fn purpose_to_payload_parts(purpose: &OAuthPurpose) -> (String, Option<i32>, Option<String>) {
    match purpose {
        OAuthPurpose::Login => ("login".to_string(), None, None),
        OAuthPurpose::LinkAccount(uid) => ("link".to_string(), Some(*uid), None),
        OAuthPurpose::PlatformData { user_id, platform } => (
            "platform".to_string(),
            Some(*user_id),
            Some(platform.clone()),
        ),
    }
}

fn purpose_from_payload(p: &str, uid: Option<i32>, plat: Option<String>) -> Option<OAuthPurpose> {
    match p {
        "login" => Some(OAuthPurpose::Login),
        "link" => uid.map(OAuthPurpose::LinkAccount),
        "platform" => match (uid, plat) {
            (Some(user_id), Some(platform)) => {
                Some(OAuthPurpose::PlatformData { user_id, platform })
            }
            _ => None,
        },
        _ => None,
    }
}

fn sign_payload_b64(payload_b64: &str, secret: &[u8]) -> Result<String, String> {
    let mut mac = HmacSha256::new_from_slice(secret).map_err(|e| format!("HMAC key error: {e}"))?;
    mac.update(payload_b64.as_bytes());
    let sig = mac.finalize().into_bytes();
    Ok(URL_SAFE_NO_PAD.encode(sig))
}

fn verify_signature(payload_b64: &str, sig_b64: &str, secret: &[u8]) -> bool {
    let Ok(expected_b64) = sign_payload_b64(payload_b64, secret) else {
        return false;
    };
    // Compare decoded bytes when possible; fall back to constant-time string compare.
    match (
        URL_SAFE_NO_PAD.decode(sig_b64),
        URL_SAFE_NO_PAD.decode(&expected_b64),
    ) {
        (Ok(a), Ok(b)) if a.len() == b.len() => a.ct_eq(&b).into(),
        _ => {
            let a = sig_b64.as_bytes();
            let b = expected_b64.as_bytes();
            a.len() == b.len() && bool::from(a.ct_eq(b))
        }
    }
}

fn stored_to_payload(
    stored: &StoredState,
    nonce: String,
    code_verifier: String,
    exp: i64,
) -> StatePayload {
    let (p, uid, plat) = purpose_to_payload_parts(&stored.purpose);
    StatePayload {
        v: 1,
        n: nonce,
        s: stored.provider_slug.clone(),
        p,
        uid,
        plat,
        exp,
        cv: Some(code_verifier),
    }
}

fn payload_to_stored(payload: StatePayload) -> Result<StoredState, ConsumeStateError> {
    if payload.v != 1 {
        return Err(ConsumeStateError::Missing);
    }
    let purpose = purpose_from_payload(&payload.p, payload.uid, payload.plat)
        .ok_or(ConsumeStateError::Missing)?;
    Ok(StoredState {
        provider_slug: payload.s,
        purpose,
    })
}

/// Issue a signed OAuth state token for any provider / purpose.
///
/// Returns the token (for the IdP `state` query), `browser_tx` (for the
/// `oauth_tx` cookie / OIDC nonce), and `code_verifier` (PKCE). Anti-replay
/// marks the nonce used on consume; empty memory after restart still accepts a
/// valid signature.
pub async fn issue_state(stored: StoredState) -> Result<IssuedState, String> {
    let secret = state_secret()?;
    let nonce = random_nonce_hex();
    let code_verifier = random_code_verifier();
    let exp = unix_now() + STATE_TTL.as_secs() as i64;
    let payload = stored_to_payload(&stored, nonce.clone(), code_verifier.clone(), exp);
    let json = serde_json::to_vec(&payload).map_err(|e| format!("state serialize: {e}"))?;
    let payload_b64 = URL_SAFE_NO_PAD.encode(&json);
    let sig_b64 = sign_payload_b64(&payload_b64, &secret)?;
    let token = format!("{payload_b64}.{sig_b64}");

    tracing::debug!(
        state_prefix = %state_prefix(&token),
        provider_slug = %stored.provider_slug,
        purpose = %payload.p,
        exp,
        "OAuth signed state issued"
    );

    Ok(IssuedState {
        token,
        browser_tx: nonce,
        code_verifier,
    })
}

/// Verified OAuth state **without** burning the anti-replay nonce.
///
/// Handlers should: (1) [`verify_state`], (2) match `oauth_tx` cookie to
/// [`browser_tx`](Self::browser_tx), (3) only then [`mark_used`](Self::mark_used).
/// That order avoids consuming a one-shot nonce when the browser binding fails.
#[derive(Debug, Clone)]
pub struct VerifiedState {
    stored: StoredState,
    browser_tx: String,
    code_verifier: String,
    /// Grace TTL for the used-nonce table entry once marked.
    remaining_ttl: Duration,
    /// Snapshot of "nonce already in used map" at verify time (hint only).
    already_used: bool,
}

impl VerifiedState {
    pub fn stored(&self) -> &StoredState {
        &self.stored
    }

    pub fn browser_tx(&self) -> &str {
        &self.browser_tx
    }

    /// PKCE code_verifier from signed state (empty if pre-MYR-011 state).
    pub fn code_verifier(&self) -> &str {
        &self.code_verifier
    }

    /// OIDC `nonce` expected in id_token — same high-entropy value as browser_tx.
    pub fn oidc_nonce(&self) -> &str {
        &self.browser_tx
    }

    /// True if the nonce was already marked used when this state was verified.
    pub fn already_used(&self) -> bool {
        self.already_used
    }

    /// Mark the nonce one-shot used and return [`ConsumeOutcome`].
    ///
    /// Call **only after** the browser `oauth_tx` cookie has matched.
    /// Concurrent double-mark races still yield [`ConsumeOutcome::Replay`].
    pub async fn mark_used(self) -> ConsumeOutcome {
        let Self {
            stored,
            browser_tx,
            remaining_ttl,
            ..
        } = self;

        let mut nonces = USED_NONCES.write().await;
        if nonces.contains(&browser_tx) {
            tracing::warn!(
                reason = "replay",
                store_size = nonces.len(),
                provider_slug = %stored.provider_slug,
                "OAuth state mark_used: nonce already used (soft-recover possible)"
            );
            return ConsumeOutcome::Replay {
                stored,
                browser_tx,
            };
        }
        // O(1) oldest eviction when at cap (process-local anti-replay table).
        let _inserted =
            nonces.insert_if_absent(browser_tx.clone(), Instant::now() + remaining_ttl);

        ConsumeOutcome::Fresh {
            stored,
            browser_tx,
        }
    }
}

/// Verify HMAC / expiry / payload **without** marking the nonce used.
///
/// Prefer this over [`consume_state`] when a browser cookie must bind first
/// (MYR-003): cookie mismatch must not burn a fresh state token.
pub async fn verify_state(token: &str) -> Result<VerifiedState, ConsumeStateError> {
    let prefix = state_prefix(token);

    let secret = match state_secret() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "OAuth state verify: secret unavailable");
            return Err(ConsumeStateError::Missing);
        }
    };

    let (payload_b64, sig_b64) = match token.split_once('.') {
        Some((p, s)) if !p.is_empty() && !s.is_empty() && !s.contains('.') => (p, s),
        _ => {
            tracing::warn!(
                reason = "malformed",
                state_prefix = %prefix,
                "OAuth state verify failed"
            );
            return Err(ConsumeStateError::Missing);
        }
    };

    if !verify_signature(payload_b64, sig_b64, &secret) {
        tracing::warn!(
            reason = "bad_signature",
            state_prefix = %prefix,
            "OAuth state verify failed"
        );
        return Err(ConsumeStateError::Missing);
    }

    let json = match URL_SAFE_NO_PAD.decode(payload_b64) {
        Ok(b) => b,
        Err(_) => {
            tracing::warn!(
                reason = "bad_payload_b64",
                state_prefix = %prefix,
                "OAuth state verify failed"
            );
            return Err(ConsumeStateError::Missing);
        }
    };

    let payload: StatePayload = match serde_json::from_slice(&json) {
        Ok(p) => p,
        Err(_) => {
            tracing::warn!(
                reason = "bad_payload_json",
                state_prefix = %prefix,
                "OAuth state verify failed"
            );
            return Err(ConsumeStateError::Missing);
        }
    };

    if payload.exp < unix_now() {
        tracing::warn!(
            reason = "expired",
            state_prefix = %prefix,
            exp = payload.exp,
            "OAuth state verify failed"
        );
        return Err(ConsumeStateError::Expired);
    }

    let browser_tx = payload.n.clone();
    let code_verifier = payload.cv.clone().unwrap_or_default();
    let remaining_ttl =
        Duration::from_secs((payload.exp - unix_now()).max(0) as u64) + Duration::from_secs(60); // grace so cleanup does not race TTL edge

    let stored = payload_to_stored(payload)?;

    let already_used = {
        let nonces = USED_NONCES.read().await;
        nonces.contains(&browser_tx)
    };

    Ok(VerifiedState {
        stored,
        browser_tx,
        code_verifier,
        remaining_ttl,
        already_used,
    })
}

/// Verify and one-shot consume a signed state token.
///
/// Convenience wrapper: [`verify_state`] then immediately [`VerifiedState::mark_used`].
/// Prefer verify → browser cookie match → mark_used in HTTP callbacks so a
/// mismatched `oauth_tx` does not burn the nonce.
///
/// After process restart the used-nonce map is empty; a still-valid signature is
/// accepted as Fresh (provider authorization codes remain one-time).
pub async fn consume_state(token: &str) -> Result<ConsumeOutcome, ConsumeStateError> {
    let verified = verify_state(token).await?;
    Ok(verified.mark_used().await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;

    static INIT_SECRET: Once = Once::new();

    fn ensure_test_secret() {
        INIT_SECRET.call_once(|| {
            if env::var("OAUTH_STATE_SECRET").is_err() && env::var("JWT_SECRET").is_err() {
                // SAFETY: tests run single-process; set once before concurrent tests use state.
                env::set_var(
                    "OAUTH_STATE_SECRET",
                    "test-oauth-state-secret-for-unit-tests",
                );
            }
        });
    }

    fn sample_login() -> StoredState {
        StoredState {
            provider_slug: "github".to_string(),
            purpose: OAuthPurpose::Login,
        }
    }

    fn sample_link(uid: i32) -> StoredState {
        StoredState {
            provider_slug: "google".to_string(),
            purpose: OAuthPurpose::LinkAccount(uid),
        }
    }

    fn sample_platform() -> StoredState {
        StoredState {
            provider_slug: "discord-platform".to_string(),
            purpose: OAuthPurpose::PlatformData {
                user_id: 7,
                platform: "discord".to_string(),
            },
        }
    }

    #[tokio::test]
    async fn issue_and_consume_valid_login() {
        ensure_test_secret();
        let issued = issue_state(sample_login()).await.expect("issue");
        assert!(issued.token.contains('.'));
        assert_eq!(issued.browser_tx.len(), 32); // 16 bytes hex
        assert!(
            issued.code_verifier.len() >= 43,
            "PKCE verifier should be >= 43 chars"
        );
        let outcome = consume_state(&issued.token).await.expect("consume");
        assert!(!outcome.is_replay());
        assert_eq!(outcome.browser_tx(), issued.browser_tx);
        let stored = outcome.into_stored();
        assert_eq!(stored.provider_slug, "github");
        assert_eq!(stored.purpose, OAuthPurpose::Login);
    }

    #[tokio::test]
    async fn issue_embeds_pkce_verifier_roundtrip() {
        ensure_test_secret();
        let issued = issue_state(sample_login()).await.expect("issue");
        let verified = verify_state(&issued.token).await.expect("verify");
        assert_eq!(verified.code_verifier(), issued.code_verifier);
        assert_eq!(verified.oidc_nonce(), issued.browser_tx);
        assert!(!verified.code_verifier().is_empty());
    }

    #[tokio::test]
    async fn verify_state_does_not_burn_nonce_until_mark_used() {
        ensure_test_secret();
        let issued = issue_state(sample_login()).await.expect("issue");

        let peeked = verify_state(&issued.token).await.expect("verify");
        assert_eq!(peeked.browser_tx(), issued.browser_tx);
        assert!(!peeked.already_used());

        // Second verify still Fresh-eligible (nonce not burned).
        let peeked2 = verify_state(&issued.token).await.expect("verify again");
        assert!(!peeked2.already_used());

        let first = peeked.mark_used().await;
        assert!(!first.is_replay());

        // After mark_used, verify reports already_used and mark yields Replay.
        let peeked3 = verify_state(&issued.token).await.expect("verify after mark");
        assert!(peeked3.already_used());
        assert!(peeked3.mark_used().await.is_replay());
    }

    #[tokio::test]
    async fn consume_bad_signature_returns_missing() {
        ensure_test_secret();
        let issued = issue_state(sample_login()).await.expect("issue");
        let (payload_b64, sig_b64) = issued.token.split_once('.').unwrap();
        // Flip last char of signature
        let mut sig: Vec<u8> = sig_b64.as_bytes().to_vec();
        let last = sig.last_mut().unwrap();
        *last = if *last == b'A' { b'B' } else { b'A' };
        let bad = format!("{payload_b64}.{}", String::from_utf8_lossy(&sig));
        let err = consume_state(&bad).await.expect_err("bad sig");
        assert_eq!(err, ConsumeStateError::Missing);
        assert_eq!(err.as_str(), "missing");
    }

    #[tokio::test]
    async fn consume_tampered_payload_returns_missing() {
        ensure_test_secret();
        let issued = issue_state(sample_login()).await.expect("issue");
        let (_payload_b64, sig_b64) = issued.token.split_once('.').unwrap();
        let fake = StatePayload {
            v: 1,
            n: "00".repeat(16),
            s: "evil".to_string(),
            p: "login".to_string(),
            uid: None,
            plat: None,
            exp: unix_now() + 600,
            cv: None,
        };
        let fake_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&fake).unwrap());
        let bad = format!("{fake_b64}.{sig_b64}");
        let err = consume_state(&bad).await.expect_err("tampered");
        assert_eq!(err, ConsumeStateError::Missing);
    }

    #[tokio::test]
    async fn consume_expired_returns_expired() {
        ensure_test_secret();
        let secret = state_secret().unwrap();
        let payload = StatePayload {
            v: 1,
            n: random_nonce_hex(),
            s: "github".to_string(),
            p: "login".to_string(),
            uid: None,
            plat: None,
            exp: unix_now() - 10,
            cv: None,
        };
        let json = serde_json::to_vec(&payload).unwrap();
        let payload_b64 = URL_SAFE_NO_PAD.encode(&json);
        let sig_b64 = sign_payload_b64(&payload_b64, &secret).unwrap();
        let token = format!("{payload_b64}.{sig_b64}");

        let err = consume_state(&token).await.expect_err("expired");
        assert_eq!(err, ConsumeStateError::Expired);
        assert_eq!(err.as_str(), "expired");
    }

    #[tokio::test]
    async fn link_purpose_roundtrip() {
        ensure_test_secret();
        let issued = issue_state(sample_link(42)).await.expect("issue");
        let stored = consume_state(&issued.token)
            .await
            .expect("consume")
            .into_stored();
        assert_eq!(stored.provider_slug, "google");
        assert_eq!(stored.purpose, OAuthPurpose::LinkAccount(42));
    }

    #[tokio::test]
    async fn platform_purpose_roundtrip() {
        ensure_test_secret();
        let issued = issue_state(sample_platform()).await.expect("issue");
        let stored = consume_state(&issued.token)
            .await
            .expect("consume")
            .into_stored();
        assert_eq!(stored.provider_slug, "discord-platform");
        assert_eq!(
            stored.purpose,
            OAuthPurpose::PlatformData {
                user_id: 7,
                platform: "discord".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn replay_returns_replay_with_recoverable_payload() {
        ensure_test_secret();
        let issued = issue_state(sample_link(99)).await.expect("issue");
        let first = consume_state(&issued.token).await.expect("first");
        assert!(matches!(first, ConsumeOutcome::Fresh { .. }));
        assert_eq!(first.stored().purpose, OAuthPurpose::LinkAccount(99));
        assert_eq!(first.browser_tx(), issued.browser_tx);

        let second = consume_state(&issued.token)
            .await
            .expect("replay is Ok(Replay)");
        assert!(second.is_replay());
        assert_eq!(second.stored().provider_slug, "google");
        assert_eq!(second.stored().purpose, OAuthPurpose::LinkAccount(99));
        assert_eq!(second.browser_tx(), issued.browser_tx);
        // Still one-shot for Fresh: third consume remains Replay, never Fresh again.
        let third = consume_state(&issued.token).await.expect("still replay");
        assert!(matches!(third, ConsumeOutcome::Replay { .. }));
    }

    #[tokio::test]
    async fn replay_login_payload_recoverable() {
        ensure_test_secret();
        let issued = issue_state(sample_login()).await.expect("issue");
        let _ = consume_state(&issued.token).await.expect("first");
        match consume_state(&issued.token).await.expect("replay") {
            ConsumeOutcome::Replay { stored, browser_tx } => {
                assert_eq!(stored.provider_slug, "github");
                assert_eq!(stored.purpose, OAuthPurpose::Login);
                assert_eq!(browser_tx, issued.browser_tx);
            }
            ConsumeOutcome::Fresh { .. } => panic!("expected Replay, got Fresh"),
        }
    }

    #[test]
    fn consume_state_error_as_str_includes_replay() {
        assert_eq!(ConsumeStateError::Missing.as_str(), "missing");
        assert_eq!(ConsumeStateError::Expired.as_str(), "expired");
        assert_eq!(ConsumeStateError::Replay.as_str(), "replay");
    }

    #[tokio::test]
    async fn malformed_token_returns_missing() {
        ensure_test_secret();
        assert_eq!(
            consume_state("not-a-token").await.unwrap_err(),
            ConsumeStateError::Missing
        );
        assert_eq!(
            consume_state("").await.unwrap_err(),
            ConsumeStateError::Missing
        );
        assert_eq!(
            consume_state("onlyonepart").await.unwrap_err(),
            ConsumeStateError::Missing
        );
    }

    #[tokio::test]
    async fn slug_fields_preserved_for_oidc_slugs() {
        ensure_test_secret();
        for slug in ["discord", "microsoft", "gitlab", "keycloak", "auth0"] {
            let stored = StoredState {
                provider_slug: slug.to_string(),
                purpose: OAuthPurpose::Login,
            };
            let issued = issue_state(stored).await.expect("issue");
            let got = consume_state(&issued.token)
                .await
                .expect("consume")
                .into_stored();
            assert_eq!(got.provider_slug, slug);
        }
    }

    #[test]
    fn state_prefix_truncates_to_eight() {
        assert_eq!(state_prefix("abcdefghij"), "abcdefgh");
        assert_eq!(state_prefix("abc"), "abc");
        assert_eq!(state_prefix(""), "");
    }

    #[test]
    fn purpose_payload_helpers() {
        let (p, uid, plat) = purpose_to_payload_parts(&OAuthPurpose::Login);
        assert_eq!(p, "login");
        assert!(uid.is_none());
        assert!(plat.is_none());

        let (p, uid, plat) = purpose_to_payload_parts(&OAuthPurpose::LinkAccount(9));
        assert_eq!((p.as_str(), uid, plat), ("link", Some(9), None));

        let purpose = purpose_from_payload("platform", Some(1), Some("discord".into())).unwrap();
        assert_eq!(
            purpose,
            OAuthPurpose::PlatformData {
                user_id: 1,
                platform: "discord".into()
            }
        );
        assert!(purpose_from_payload("link", None, None).is_none());
    }

    #[test]
    fn oauth_tx_cookie_binding_helpers() {
        let set = oauth_tx_set_cookie_value("abc123", true);
        assert!(set.contains("oauth_tx=abc123"));
        assert!(set.contains("HttpOnly"));
        assert!(set.contains("SameSite=Lax"));
        assert!(set.contains("Secure"));
        assert!(set.contains(&format!("Max-Age={}", STATE_TTL.as_secs())));

        let set_dev = oauth_tx_set_cookie_value("abc123", false);
        assert!(!set_dev.contains("Secure"));

        let clear = oauth_tx_clear_cookie_value(true);
        assert!(clear.contains("Max-Age=0"));
        assert!(clear.contains("Secure"));

        let header = "auth_token=jwt; oauth_tx=deadbeef; other=1";
        assert_eq!(
            cookie_value_from_header(header, OAUTH_TX_COOKIE),
            Some("deadbeef")
        );
        assert!(oauth_tx_cookie_matches(Some(header), "deadbeef"));
        assert!(!oauth_tx_cookie_matches(Some(header), "other"));
        assert!(!oauth_tx_cookie_matches(None, "deadbeef"));
        assert!(!oauth_tx_cookie_matches(Some("oauth_tx=deleted"), "deadbeef"));
        assert!(!oauth_tx_cookie_matches(Some(""), "deadbeef"));
    }
}
