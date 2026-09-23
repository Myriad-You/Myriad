//! Server-side HTTP client for talking to the Myriad updater.
//!
//! - Backend talks to `updater-gateway` via `MYRIAD_UPDATER_URL` only.
//! - Backend holds `UPDATER_GATEWAY_SECRET` (not `UPDATE_TOKEN`) and sends
//!   `X-Updater-Gateway-Secret` on authenticated hops (`ping` /healthz has none).
//! - Gateway injects `X-Update-Token` server-side toward updater.
//! - Admin-gated `/api/admin/updater/*` routes proxy requests through here.
//! - `UPDATE_TOKEN` never crosses the user→backend boundary and is not in the fat process.
//!
//! Optional legacy/dev: set `UPDATE_TOKEN` when talking **directly** to updater (no gateway).
//!
//! See docs/updater-spec.md §13 for the upstream API.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use myriad_error::{AppError, redact_secrets};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client, Method, StatusCode};
use serde::Serialize;

/// Cheap-to-clone handle. Held in `updater_admin::init` OnceLock, not axum `State`.
#[derive(Debug, Clone)]
pub struct UpdaterClient {
    inner: Arc<Inner>,
}

#[derive(Debug)]
enum MutateAuth {
    Gateway(HeaderValue),
    DirectToken(HeaderValue),
}

#[derive(Debug)]
struct Inner {
    base_url: String,
    auth: Option<MutateAuth>,
    /// Secret/token was set but is not a valid HTTP header value.
    credentials_invalid: bool,
    http: Client,
}

#[derive(Debug)]
pub enum UpdaterClientError {
    NotConfigured,
    InvalidCredentials,
    /// Upstream non-2xx. Stored body is redacted; HTTP adapter forwards status, not the body.
    Upstream(StatusCode, String),
    Transport(String),
}

impl std::fmt::Display for UpdaterClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never echo UPDATE_TOKEN / JWT_SECRET / etc. into logs or JSON error bodies.
        match self {
            Self::NotConfigured => f.write_str("updater not configured (set MYRIAD_UPDATER_URL)"),
            Self::InvalidCredentials => {
                f.write_str("updater credentials are not a valid HTTP header value")
            }
            Self::Upstream(s, body) => {
                write!(f, "upstream {s}: {}", redact_secrets(body))
            }
            Self::Transport(e) => {
                write!(f, "transport: {}", redact_secrets(e))
            }
        }
    }
}

impl std::error::Error for UpdaterClientError {}

impl UpdaterClientError {
    pub fn status(&self) -> StatusCode {
        match self {
            Self::NotConfigured => StatusCode::SERVICE_UNAVAILABLE,
            Self::InvalidCredentials => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Upstream(s, _) => *s,
            Self::Transport(_) => StatusCode::BAD_GATEWAY,
        }
    }

    /// Map into the shared [`AppError`] used by HTTP adapters.
    pub fn into_app_error(self) -> AppError {
        match self {
            Self::NotConfigured => {
                AppError::service_unavailable("updater not configured (set MYRIAD_UPDATER_URL)")
            }
            Self::InvalidCredentials => AppError::internal(
                "updater credentials are not a valid HTTP header value",
            ),
            Self::Upstream(status, body) => {
                tracing::error!(%status, body = %redact_secrets(&body), "updater upstream failed");
                let message = match extract_upstream_error(&body) {
                    Some(detail) => format!("updater upstream {status}: {detail}"),
                    None => format!("updater upstream {status}"),
                };
                AppError::from_status_u16(status.as_u16(), message)
            }
            Self::Transport(error) => {
                tracing::error!(%error, "updater transport failed");
                AppError::bad_gateway("updater transport error")
            }
        }
    }
}

impl From<UpdaterClientError> for AppError {
    fn from(e: UpdaterClientError) -> Self {
        e.into_app_error()
    }
}

/// Pull the `error` field out of an updater `{"error": "..."}` body so the
/// operator sees the real reason (e.g. a GitHub rate limit) instead of a bare
/// status line. Returns `None` when the body is not that shape.
fn extract_upstream_error(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let message = value.get("error")?.as_str()?.trim();
    if message.is_empty() {
        None
    } else {
        Some(message.to_string())
    }
}

impl UpdaterClient {
    /// Construct from env. Returns `None` if the updater isn't wired up — backend can still
    /// boot in setups without it (development, fresh installs that haven't migrated yet).
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("MYRIAD_UPDATER_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(default_container_updater_url)?;
        let gateway_secret = std::env::var("UPDATER_GATEWAY_SECRET")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let token = std::env::var("UPDATE_TOKEN")
            .ok()
            .filter(|s| !s.trim().is_empty());

        // Production: gateway secret only. Optional UPDATE_TOKEN remains for legacy/dev
        // direct hops (host backend → published updater port without a gateway).
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .user_agent("myriad-backend/updater-client")
            .build()
            .ok()?;

        let (auth, credentials_invalid) = match parse_mutate_auth(gateway_secret, token) {
            Ok(auth) => (auth, false),
            Err(()) => {
                tracing::error!(
                    "UPDATER_GATEWAY_SECRET or UPDATE_TOKEN is not a valid HTTP header value"
                );
                (None, true)
            }
        };

        Some(Self {
            inner: Arc::new(Inner {
                base_url: base_url.trim_end_matches('/').to_string(),
                auth,
                credentials_invalid,
                http,
            }),
        })
    }

    /// True when mutative calls have a validated gateway secret or direct token.
    pub fn can_mutate(&self) -> bool {
        self.inner.auth.is_some()
    }

    pub fn credentials_invalid(&self) -> bool {
        self.inner.credentials_invalid
    }

    pub fn base_url(&self) -> &str {
        &self.inner.base_url
    }

    /// Forward a GET request. Auth headers attached when available.
    pub async fn get_json(&self, path: &str) -> Result<serde_json::Value, UpdaterClientError> {
        self.call(Method::GET, path, Option::<&()>::None, None, None)
            .await
    }

    /// Forward a POST request with a JSON body. `idempotency_key` becomes the
    /// `Idempotency-Key` header when supplied.
    ///
    /// `idempotency_key` becomes `Idempotency-Key`. Actor is not set here (`post_json_with_actor` / `delete_json`).
    pub async fn post_json<B: Serialize + ?Sized>(
        &self,
        path: &str,
        body: Option<&B>,
        idempotency_key: Option<&str>,
    ) -> Result<serde_json::Value, UpdaterClientError> {
        self.call(Method::POST, path, body, idempotency_key, None)
            .await
    }

    /// Like [`post_json`] but attaches `X-Update-Actor` when `actor` is present.
    pub async fn post_json_with_actor<B: Serialize + ?Sized>(
        &self,
        path: &str,
        body: Option<&B>,
        idempotency_key: Option<&str>,
        actor: Option<&str>,
    ) -> Result<serde_json::Value, UpdaterClientError> {
        self.call(Method::POST, path, body, idempotency_key, actor)
            .await
    }

    /// Forward a DELETE request. Same auth headers as other mutative calls.
    pub async fn delete_json(
        &self,
        path: &str,
        actor: Option<&str>,
    ) -> Result<serde_json::Value, UpdaterClientError> {
        self.call(Method::DELETE, path, Option::<&()>::None, None, actor)
            .await
    }

    async fn call<B: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
        idempotency_key: Option<&str>,
        actor: Option<&str>,
    ) -> Result<serde_json::Value, UpdaterClientError> {
        // Path must start with `/` to avoid base-URL slip.
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };

        let mut headers = HeaderMap::new();
        // Production hop: gateway secret. Do NOT send UPDATE_TOKEN toward the gateway
        // (gateway rejects client-supplied X-Update-Token and injects its own).
        match &self.inner.auth {
            Some(MutateAuth::Gateway(value)) => {
                headers.insert("X-Updater-Gateway-Secret", value.clone());
            }
            Some(MutateAuth::DirectToken(value)) => {
                headers.insert("X-Update-Token", value.clone());
            }
            None if self.inner.credentials_invalid => {
                return Err(UpdaterClientError::InvalidCredentials);
            }
            None => {}
        }
        if let Some(k) = idempotency_key {
            if let Ok(v) = HeaderValue::from_str(k) {
                headers.insert("Idempotency-Key", v);
            }
        }
        // Server-only actor note for audit; trusted only after UPDATE_TOKEN auth on updater.
        if let Some(a) = actor.map(str::trim).filter(|s| !s.is_empty()) {
            if let Ok(v) = HeaderValue::from_str(a) {
                headers.insert("X-Update-Actor", v);
            }
        }

        let url = format!("{}{}", self.inner.base_url, path);
        let mut req = self.inner.http.request(method, &url).headers(headers);
        if let Some(b) = body {
            req = req.json(b);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| UpdaterClientError::Transport(redact_secrets(&e.to_string())))?;
        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| UpdaterClientError::Transport(redact_secrets(&e.to_string())))?;

        if !status.is_success() {
            let detail = redact_secrets(&String::from_utf8_lossy(&bytes));
            return Err(UpdaterClientError::Upstream(status, detail));
        }

        if bytes.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        serde_json::from_slice(&bytes).map_err(|error| {
            tracing::error!(%error, "updater JSON decode failed");
            UpdaterClientError::Transport("decode json failed".to_string())
        })
    }

    /// Convenience for the `/healthz` probe used by backend startup logs.
    /// Gateway `/healthz` is intentionally unauthenticated for compose healthchecks.
    pub async fn ping(&self) -> Result<()> {
        let url = format!("{}/healthz", self.inner.base_url);
        let resp = self
            .inner
            .http
            .get(&url)
            .timeout(Duration::from_secs(3))
            .send()
            .await
            .context("ping updater")?;
        if !resp.status().is_success() {
            return Err(anyhow!("updater /healthz returned {}", resp.status()));
        }
        Ok(())
    }
}

fn sensitive_header(raw: &str) -> Result<HeaderValue, ()> {
    let mut value = HeaderValue::from_str(raw).map_err(|_| ())?;
    value.set_sensitive(true);
    Ok(value)
}

fn parse_mutate_auth(
    gateway_secret: Option<String>,
    token: Option<String>,
) -> Result<Option<MutateAuth>, ()> {
    if let Some(secret) = gateway_secret {
        return Ok(Some(MutateAuth::Gateway(sensitive_header(&secret)?)));
    }
    if let Some(token) = token {
        return Ok(Some(MutateAuth::DirectToken(sensitive_header(&token)?)));
    }
    Ok(None)
}

fn default_container_updater_url() -> Option<String> {
    let production = std::env::var("ENVIRONMENT")
        .map(|v| v.eq_ignore_ascii_case("production"))
        .unwrap_or(false);
    let in_container = std::path::Path::new("/.dockerenv").exists()
        || std::env::var("container").is_ok()
        || std::env::var("KUBERNETES_SERVICE_HOST").is_ok();

    if production || in_container {
        // Preferred production hop: thin gateway injects the token on admin-net.
        Some("http://updater-gateway:1104".to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn into_app_error_not_configured_is_503() {
        let e = UpdaterClientError::NotConfigured.into_app_error();
        assert_eq!(e.status_u16(), 503);
        assert!(e.error_label().contains("not configured"));
    }

    #[test]
    fn into_app_error_transport_is_502_and_redacts() {
        let e = UpdaterClientError::Transport(
            "connect failed Authorization: Bearer supersecrettoken99".into(),
        )
        .into_app_error();
        assert_eq!(e.status_u16(), 502);
        let json = e.to_json().to_string();
        assert!(!json.contains("supersecrettoken99"), "leaked: {json}");
        assert!(json.contains("transport"), "{json}");
    }

    #[test]
    fn parse_mutate_auth_rejects_invalid_header_and_does_not_claim_mutate() {
        assert!(parse_mutate_auth(None, None).unwrap().is_none());
        assert!(parse_mutate_auth(Some("ok-secret-without-ctl".into()), None)
            .unwrap()
            .is_some());
        assert!(parse_mutate_auth(Some("bad\nsecret".into()), None).is_err());
        assert!(
            parse_mutate_auth(Some("bad\nsecret".into()), Some("fallback-token".into()))
                .is_err(),
            "invalid gateway secret must not fall back to UPDATE_TOKEN"
        );
    }

    #[test]
    fn into_app_error_invalid_credentials_is_not_not_configured() {
        let e = UpdaterClientError::InvalidCredentials.into_app_error();
        assert_eq!(e.status_u16(), 500);
        assert!(!e.error_label().contains("not configured"));
    }

    #[test]
    fn into_app_error_upstream_preserves_status() {
        let e = UpdaterClientError::Upstream(StatusCode::CONFLICT, "busy".into()).into_app_error();
        assert_eq!(e.status_u16(), 409);
        let json = e.to_json();
        assert_eq!(json["error"], "updater upstream 409 Conflict");
        assert!(json.get("message").is_none());
    }

    #[test]
    fn into_app_error_upstream_surfaces_the_updater_reason() {
        let body = r#"{"error":"github: GET releases failed: 403 Forbidden (rate limit)"}"#;
        let e = UpdaterClientError::Upstream(StatusCode::INTERNAL_SERVER_ERROR, body.into())
            .into_app_error();
        assert_eq!(e.status_u16(), 500);
        let json = e.to_json();
        assert_eq!(
            json["error"],
            "updater upstream 500 Internal Server Error: github: GET releases failed: \
             403 Forbidden (rate limit)"
        );
    }
}
