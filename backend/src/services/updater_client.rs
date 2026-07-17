//! Server-side HTTP client for talking to the Myriad updater.
//!
//! Why it exists: the frontend used to call `/_updater/*` through the proxy with the user
//! pasting `UPDATE_TOKEN` into a form field. That works but leaks the token to the browser
//! and to anyone who can MITM the proxy.
//!
//! With this client (recommended production path):
//!   - Backend talks to `updater-gateway` via `MYRIAD_UPDATER_URL` only.
//!   - Gateway injects `X-Update-Token` server-side; backend process need not hold
//!     `UPDATE_TOKEN` (optional for legacy/dev direct-to-updater setups).
//!   - Admin-gated `/api/admin/updater/*` routes proxy requests through here.
//!   - Token never crosses the user→backend boundary.
//!
//! See docs/updater-spec.md §13 for the upstream API.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
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
            Self::NotConfigured => {
                f.write_str("updater not configured (set MYRIAD_UPDATER_URL)")
            }
            Self::Upstream(s, body) => {
                write!(
                    f,
                    "upstream {s}: {}",
                    crate::util::redact::redact_secrets(body)
                )
            }
            Self::Transport(e) => {
                write!(f, "transport: {}", crate::util::redact::redact_secrets(e))
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
}

impl UpdaterClient {
    /// Construct from env. Returns `None` if the updater isn't wired up — backend can still
    /// boot in setups without it (development, fresh installs that haven't migrated yet).
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("MYRIAD_UPDATER_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(default_container_updater_url)?;
        let token = std::env::var("UPDATE_TOKEN")
            .ok()
            .filter(|s| !s.trim().is_empty());

        // URL-only is the recommended production shape: updater-gateway injects
        // X-Update-Token. Optional UPDATE_TOKEN remains for legacy/dev direct hops
        // (host backend → published updater port without a gateway).
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .user_agent("myriad-backend/updater-client")
            .build()
            .ok()?;

        Some(Self {
            inner: Arc::new(Inner {
                base_url: base_url.trim_end_matches('/').to_string(),
                token,
                http,
            }),
        })
    }

    pub fn has_token(&self) -> bool {
        self.inner.token.is_some()
    }

    pub fn base_url(&self) -> &str {
        &self.inner.base_url
    }

    /// Forward a GET request. Token is attached if available.
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
        if let Some(t) = &self.inner.token {
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
            .map_err(|e| {
                UpdaterClientError::Transport(crate::util::redact::redact_secrets(&e.to_string()))
            })?;
        let status = resp.status();
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| {
                UpdaterClientError::Transport(crate::util::redact::redact_secrets(&e.to_string()))
            })?;

        if !status.is_success() {
            let detail = crate::util::redact::redact_secrets(
                &String::from_utf8_lossy(&bytes),
            );
            return Err(UpdaterClientError::Upstream(status, detail));
        }

        if bytes.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        serde_json::from_slice(&bytes).map_err(|e| {
            UpdaterClientError::Transport(crate::util::redact::redact_secrets(&format!(
                "decode json: {e}"
            )))
        })
    }

    /// Convenience for the `/healthz` probe used by backend startup logs.
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
