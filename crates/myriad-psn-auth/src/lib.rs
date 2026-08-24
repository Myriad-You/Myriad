//! PlayStation Network NPSSO → access token exchange.
//!
//! Shared by `services::fetcher` and `api::game_presence` so the fetcher does not
//! depend on the HTTP API layer. Outbound calls go through [`myriad_outbound`].
//!
//! # Security notes
//! - Redirects disabled; authorization `code` is read from the `Location` header.
//! - Query param extraction uses percent-decoding (Sony may encode `+`/`/` in codes).

use myriad_outbound::build_public_http_client;
use serde_json::Value;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use url::Url;

struct PsnTokenEntry {
    access_token: String,
    /// NPSSO hash so a different cookie does not reuse another account's token.
    npsso_fingerprint: u64,
    fetched_at: Instant,
}

static PSN_TOKEN_CACHE: OnceLock<Mutex<Option<PsnTokenEntry>>> = OnceLock::new();

/// Leave margin before Sony's ~1h mobile token expiry.
const PSN_TOKEN_TTL: Duration = Duration::from_secs(50 * 60);

/// Public client id used by community PSN tools / mobile app flow.
const PSN_CLIENT_ID: &str = "09515159-7237-4370-9b40-3806e67c0891";
const PSN_REDIRECT_URI: &str = "com.scee.psxandroid.scecompcall://redirect";
/// Precomputed Basic auth for the public client (id:secret) used by community tools.
const PSN_BASIC_AUTH: &str =
    "Basic MDk1MTUxNTktNzIzNy00MzcwLTliNDAtMzgwNmU2N2MwODkxOnVjUGprYTV0bnRCMktxc1A=";

fn psn_token_cache() -> &'static Mutex<Option<PsnTokenEntry>> {
    PSN_TOKEN_CACHE.get_or_init(|| Mutex::new(None))
}

fn npsso_fingerprint(npsso: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    npsso.trim().hash(&mut h);
    h.finish()
}

/// Obtain a PSN access token for the given NPSSO cookie value (cached ~50 minutes).
pub async fn get_psn_access_token(npsso: &str) -> Result<String, String> {
    let npsso = npsso.trim();
    if npsso.is_empty() {
        return Err("PSN NPSSO is empty".to_string());
    }
    let fp = npsso_fingerprint(npsso);

    if let Ok(guard) = psn_token_cache().lock() {
        if let Some(entry) = guard.as_ref() {
            if entry.npsso_fingerprint == fp && entry.fetched_at.elapsed() < PSN_TOKEN_TTL {
                return Ok(entry.access_token.clone());
            }
        }
    }

    let access_token = psn_exchange_npsso(npsso).await?;

    if let Ok(mut guard) = psn_token_cache().lock() {
        *guard = Some(PsnTokenEntry {
            access_token: access_token.clone(),
            npsso_fingerprint: fp,
            fetched_at: Instant::now(),
        });
    }

    Ok(access_token)
}

/// Exchange NPSSO cookie value for an access token (public client id flow).
async fn psn_exchange_npsso(npsso: &str) -> Result<String, String> {
    // Keep the historical query string byte-for-byte (including the space in scope).
    let auth_url = format!(
        "https://ca.account.sony.com/api/authz/v3/oauth/authorize?\
access_type=offline&client_id={PSN_CLIENT_ID}\
&redirect_uri={PSN_REDIRECT_URI}\
&response_type=code&scope=psn:mobile.v2.core psn:clientapp"
    );

    let (_, client) = build_public_http_client(
        &auth_url,
        Duration::from_secs(20),
        Some("Myriad/1.0 (game-presence)"),
    )
    .await
    .map_err(|e| e.to_string())?;

    let resp = client
        .get(&auth_url)
        .header("Cookie", format!("npsso={npsso}"))
        .send()
        .await
        .map_err(|e| format!("PSN authorize request failed: {e}"))?;

    // With redirects disabled, Location holds the code.
    let location = resp
        .headers()
        .get("location")
        .or_else(|| resp.headers().get("Location"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let code = extract_query_param(&location, "code").ok_or_else(|| {
        format!(
            "PSN NPSSO exchange failed (status {}). Cookie may be expired.",
            resp.status()
        )
    })?;

    let token_url = "https://ca.account.sony.com/api/authz/v3/oauth/token";
    let (_, token_client) = build_public_http_client(
        token_url,
        Duration::from_secs(20),
        Some("Myriad/1.0 (game-presence)"),
    )
    .await
    .map_err(|e| e.to_string())?;

    let form = [
        ("code", code.as_str()),
        ("redirect_uri", PSN_REDIRECT_URI),
        ("grant_type", "authorization_code"),
        ("token_format", "jwt"),
    ];

    let token_resp = token_client
        .post(token_url)
        .header("Authorization", PSN_BASIC_AUTH)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&form)
        .send()
        .await
        .map_err(|e| format!("PSN token request failed: {e}"))?;

    if !token_resp.status().is_success() {
        return Err(format!("PSN token exchange HTTP {}", token_resp.status()));
    }

    let token_json: Value = token_resp
        .json()
        .await
        .map_err(|e| format!("PSN token parse failed: {e}"))?;

    token_json
        .get("access_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "PSN token response missing access_token".to_string())
}

/// Parse a callback URL query parameter with percent-decoding.
///
/// Sony may percent-encode characters in `code`; hand-splitting on `&`/`=` would
/// leave raw `%XX` and break the token exchange.
pub fn extract_query_param(url: &str, key: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    parsed
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
        .filter(|v| !v.is_empty())
}

/// Clear the process token cache (tests / credential rotation).
pub fn clear_psn_token_cache() {
    if let Ok(mut guard) = psn_token_cache().lock() {
        *guard = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_query_param_decodes_percent() {
        let url = "com.scee.psxandroid.scecompcall://redirect?code=ab%2Bcd%2Fef&state=x";
        assert_eq!(
            extract_query_param(url, "code").as_deref(),
            Some("ab+cd/ef")
        );
        assert_eq!(extract_query_param(url, "state").as_deref(), Some("x"));
        assert!(extract_query_param(url, "missing").is_none());
    }

    #[test]
    fn extract_query_param_rejects_empty_and_bad_url() {
        assert!(extract_query_param("not a url", "code").is_none());
        assert!(extract_query_param("https://example.com/?code=", "code").is_none());
        assert!(extract_query_param("https://example.com/path-without-query", "code").is_none());
    }

    #[test]
    fn npsso_fingerprint_is_stable_and_sensitive() {
        assert_eq!(npsso_fingerprint("abc"), npsso_fingerprint(" abc "));
        assert_ne!(npsso_fingerprint("abc"), npsso_fingerprint("abd"));
    }

    #[test]
    fn empty_npsso_is_rejected_without_network() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = rt
            .block_on(get_psn_access_token("  "))
            .expect_err("empty npsso");
        assert!(err.contains("empty"), "{err}");
    }
}
