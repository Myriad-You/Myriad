//! OAuth CSRF state — HMAC-signed tokens shared by all providers.
//!
//! Process-local memory alone breaks multi-instance / restart deployments:
//! the callback lands on a different process that never saw `insert_state`.
//! Signed state embeds purpose + slug + expiry; any process with the shared
//! secret can verify. Optional in-memory nonces provide anti-replay within a
//! process lifetime; after restart a still-valid signature is accepted
//! (provider authorization codes remain one-time).
//!
//! Token format (URL-safe):
//!   `base64url(payload_json) + '.' + base64url(hmac_sha256)`
//!
//! Payload fields:
//!   `v` (version), `n` (nonce hex), `s` (slug), `p` (login|link|platform),
//!   `uid?`, `plat?`, `exp` (unix seconds)
//!
//! Secret: `OAUTH_STATE_SECRET` if set, else `JWT_SECRET`.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;
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

/// Why `consume_state` failed — distinguishes never-seen vs TTL-elapsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumeStateError {
    /// Token missing/malformed/bad signature, or nonce already consumed (replay).
    Missing,
    /// Signature ok but past `exp`.
    Expired,
}

impl ConsumeStateError {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Expired => "expired",
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
}

/// Used nonces for optional anti-replay within this process.
/// Value = Instant when the entry may be dropped (exp + small grace).
static USED_NONCES: Lazy<Arc<RwLock<HashMap<String, Instant>>>> = Lazy::new(|| {
    let store: Arc<RwLock<HashMap<String, Instant>>> = Arc::new(RwLock::new(HashMap::new()));
    let store_clone = store.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            let mut nonces = store_clone.write().await;
            let now = Instant::now();
            let before = nonces.len();
            nonces.retain(|_, until| *until > now);
            let removed = before - nonces.len();
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
            (Some(user_id), Some(platform)) => Some(OAuthPurpose::PlatformData {
                user_id,
                platform,
            }),
            _ => None,
        },
        _ => None,
    }
}

fn sign_payload_b64(payload_b64: &str, secret: &[u8]) -> Result<String, String> {
    let mut mac =
        HmacSha256::new_from_slice(secret).map_err(|e| format!("HMAC key error: {e}"))?;
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

fn stored_to_payload(stored: &StoredState, nonce: String, exp: i64) -> StatePayload {
    let (p, uid, plat) = purpose_to_payload_parts(&stored.purpose);
    StatePayload {
        v: 1,
        n: nonce,
        s: stored.provider_slug.clone(),
        p,
        uid,
        plat,
        exp,
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
/// Optionally registers the nonce for process-local anti-replay bookkeeping
/// (consume marks it used; empty memory after restart still accepts a valid sig).
pub async fn issue_state(stored: StoredState) -> Result<String, String> {
    let secret = state_secret()?;
    let nonce = random_nonce_hex();
    let exp = unix_now() + STATE_TTL.as_secs() as i64;
    let payload = stored_to_payload(&stored, nonce.clone(), exp);
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

    Ok(token)
}

/// Verify and one-shot consume a signed state token.
///
/// 1. Verify HMAC (constant-time)
/// 2. Check `exp` → [`ConsumeStateError::Expired`]
/// 3. If nonce already used in this process → [`ConsumeStateError::Missing`]
/// 4. Mark nonce used; if memory was empty (restart), valid signature still OK
pub async fn consume_state(token: &str) -> Result<StoredState, ConsumeStateError> {
    let prefix = state_prefix(token);

    let secret = match state_secret() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "OAuth state consume: secret unavailable");
            return Err(ConsumeStateError::Missing);
        }
    };

    let (payload_b64, sig_b64) = match token.split_once('.') {
        Some((p, s)) if !p.is_empty() && !s.is_empty() && !s.contains('.') => (p, s),
        _ => {
            tracing::warn!(
                reason = "malformed",
                state_prefix = %prefix,
                "OAuth state consume failed"
            );
            return Err(ConsumeStateError::Missing);
        }
    };

    if !verify_signature(payload_b64, sig_b64, &secret) {
        tracing::warn!(
            reason = "bad_signature",
            state_prefix = %prefix,
            "OAuth state consume failed"
        );
        return Err(ConsumeStateError::Missing);
    }

    let json = match URL_SAFE_NO_PAD.decode(payload_b64) {
        Ok(b) => b,
        Err(_) => {
            tracing::warn!(
                reason = "bad_payload_b64",
                state_prefix = %prefix,
                "OAuth state consume failed"
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
                "OAuth state consume failed"
            );
            return Err(ConsumeStateError::Missing);
        }
    };

    if payload.exp < unix_now() {
        tracing::warn!(
            reason = "expired",
            state_prefix = %prefix,
            exp = payload.exp,
            "OAuth state consume failed"
        );
        return Err(ConsumeStateError::Expired);
    }

    let nonce = payload.n.clone();
    let remaining_ttl = Duration::from_secs((payload.exp - unix_now()).max(0) as u64)
        + Duration::from_secs(60); // grace so cleanup does not race TTL edge

    // Anti-replay: mark nonce used. If already present → replay.
    {
        let mut nonces = USED_NONCES.write().await;
        if nonces.contains_key(&nonce) {
            tracing::warn!(
                reason = "replay",
                state_prefix = %prefix,
                store_size = nonces.len(),
                "OAuth state consume failed"
            );
            return Err(ConsumeStateError::Missing);
        }
        if nonces.len() >= MAX_USED_NONCES {
            if let Some(oldest) = nonces
                .iter()
                .min_by_key(|(_, until)| *until)
                .map(|(k, _)| k.clone())
            {
                nonces.remove(&oldest);
            }
        }
        nonces.insert(nonce, Instant::now() + remaining_ttl);
    }

    let stored = payload_to_stored(payload)?;
    Ok(stored)
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
                env::set_var("OAUTH_STATE_SECRET", "test-oauth-state-secret-for-unit-tests");
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
        let token = issue_state(sample_login()).await.expect("issue");
        assert!(token.contains('.'));
        let stored = consume_state(&token).await.expect("consume");
        assert_eq!(stored.provider_slug, "github");
        assert_eq!(stored.purpose, OAuthPurpose::Login);
    }

    #[tokio::test]
    async fn consume_bad_signature_returns_missing() {
        ensure_test_secret();
        let token = issue_state(sample_login()).await.expect("issue");
        let (payload_b64, sig_b64) = token.split_once('.').unwrap();
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
        let token = issue_state(sample_login()).await.expect("issue");
        let (_payload_b64, sig_b64) = token.split_once('.').unwrap();
        let fake = StatePayload {
            v: 1,
            n: "00".repeat(16),
            s: "evil".to_string(),
            p: "login".to_string(),
            uid: None,
            plat: None,
            exp: unix_now() + 600,
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
        let token = issue_state(sample_link(42)).await.expect("issue");
        let stored = consume_state(&token).await.expect("consume");
        assert_eq!(stored.provider_slug, "google");
        assert_eq!(stored.purpose, OAuthPurpose::LinkAccount(42));
    }

    #[tokio::test]
    async fn platform_purpose_roundtrip() {
        ensure_test_secret();
        let token = issue_state(sample_platform()).await.expect("issue");
        let stored = consume_state(&token).await.expect("consume");
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
    async fn replay_returns_missing() {
        ensure_test_secret();
        let token = issue_state(sample_login()).await.expect("issue");
        let _ = consume_state(&token).await.expect("first");
        let err = consume_state(&token).await.expect_err("replay");
        assert_eq!(err, ConsumeStateError::Missing);
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
            let token = issue_state(stored).await.expect("issue");
            let got = consume_state(&token).await.expect("consume");
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
}
