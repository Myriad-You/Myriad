//! Server-side HTTP client for talking to the Myriad updater.
//!
//! Why it exists: the frontend used to call `/_updater/*` through the proxy with the user
//! pasting `UPDATE_TOKEN` into a form field. That works but leaks the token to the browser
//! and to anyone who can MITM the proxy.
//!
//! With this client (recommended production path):
//! - Backend talks to `updater-gateway` via `MYRIAD_UPDATER_URL` only.
//! - Backend holds `UPDATER_GATEWAY_SECRET` (not `UPDATE_TOKEN`) and sends
//! `X-Updater-Gateway-Secret` on every hop.
//! - Gateway injects `X-Update-Token` server-side toward updater.
//! - Admin-gated `/api/admin/updater/*` routes proxy requests through here.
//! - `UPDATE_TOKEN` never crosses the user→backend boundary and is not in the fat process.
//!
//! Optional legacy/dev: set `UPDATE_TOKEN` when talking **directly** to updater (no gateway).
//!
//! See docs/updater-spec.md §13 for the upstream API.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use myriad_error::{redact_secrets, AppError};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client, Method, StatusCode};
use serde::Serialize;

/// Cheap-to-clone handle. Wrap in `Arc` once at startup and pass to routes via `axum::extract::State`.
#[derive(Debug, Clone)]
pub struct UpdaterClient {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    base_url: String,
    /// Shared secret for updater-gateway (`X-Updater-Gateway-Secret`).
    gateway_secret: Option<String>,
    /// Optional direct-updater token (legacy/dev). Prefer gateway secret in production.
    token: Option<String>,
    http: Client,
}

#[derive(Debug)]
pub enum UpdaterClientError {
    NotConfigured,
    /// Upstream returned a non-2xx response. Body preserved verbatim so we can forward it to
    /// the admin UI for diagnostics.
    Upstream(StatusCode, String),
    Transport(String),
}

impl std::fmt::Display for UpdaterClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never echo UPDATE_TOKEN / JWT_SECRET / etc. into logs or JSON error bodies.
        match self {
            Self::NotConfigured => f.write_str("updater not configured (set MYRIAD_UPDATER_URL)"),
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
            Self::Upstream(status, body) => {
                AppError::from_status_u16(status.as_u16(), format!("updater upstream {status}"))
                    .with_message(body)
            }
            Self::Transport(e) => AppError::bad_gateway("updater transport error").with_message(e),
        }
    }
}

impl From<UpdaterClientError> for AppError {
    fn from(e: UpdaterClientError) -> Self {
        e.into_app_error()
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

        Some(Self {
            inner: Arc::new(Inner {
                base_url: base_url.trim_end_matches('/').to_string(),
                gateway_secret,
                token,
                http,
            }),
        })
    }

    /// True when the client can authenticate mutative calls: gateway secret (prod) or
    /// direct UPDATE_TOKEN (legacy).
    pub fn can_mutate(&self) -> bool {
        self.inner.gateway_secret.is_some() || self.inner.token.is_some()
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
    /// `actor` (e.g. `admin:1:alice`) is sent as `X-Update-Actor` for updater
    /// audit lines. Only set on the server-side hop after admin JWT auth;
    /// browsers never hold `UPDATE_TOKEN` so they cannot forge this via the
    /// normal backend path.
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
        if let Some(s) = &self.inner.gateway_secret {
            match HeaderValue::from_str(s) {
                Ok(v) => {
                    headers.insert("X-Updater-Gateway-Secret", v);
                }
                Err(_) => return Err(UpdaterClientError::NotConfigured),
            }
        } else if let Some(t) = &self.inner.token {
            // Legacy direct-to-updater only when no gateway secret is configured.
            match HeaderValue::from_str(t) {
                Ok(v) => {
                    headers.insert("X-Update-Token", v);
                }
                Err(_) => return Err(UpdaterClientError::NotConfigured),
            }
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
        serde_json::from_slice(&bytes).map_err(|e| {
            UpdaterClientError::Transport(redact_secrets(&format!("decode json: {e}")))
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
        assert!(
            json.contains("[REDACTED]") || json.contains("transport"),
            "{json}"
        );
    }

    #[test]
    fn into_app_error_upstream_preserves_status() {
        let e = UpdaterClientError::Upstream(StatusCode::CONFLICT, "busy".into()).into_app_error();
        assert_eq!(e.status_u16(), 409);
        assert_eq!(e.to_json()["message"], "busy");
    }
}
