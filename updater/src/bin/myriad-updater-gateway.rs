//! Thin reverse proxy that injects `X-Update-Token` for the backend hop.
//!
//! Production topology: backend holds **no** `UPDATE_TOKEN`. It calls this gateway
//! over `myriad-admin-net` with `X-Updater-Gateway-Secret`; the gateway injects the
//! update token and forwards to `updater` (same admin network). Attack surface is
//! intentionally tiny: no compose mounts, no Docker socket, no business logic.
//!
//! Trust note: leaking `UPDATER_GATEWAY_SECRET` lets an admin-net peer drive updates
//! via the gateway (token still never leaves gateway→updater). Prefer not placing
//! untrusted workloads on admin-net; protect the gateway secret like other secrets.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use reqwest::Client;
use tracing::{error, info, warn};

const MAX_BODY: usize = 16 * 1024 * 1024;
const GATEWAY_SECRET_MIN_LEN: usize = 32;
const HEADER_GATEWAY_SECRET: &str = "x-updater-gateway-secret";
const HEADER_UPDATE_TOKEN: &str = "x-update-token";

#[derive(Clone)]
struct GatewayState {
    upstream: String,
    token: String,
    gateway_secret: String,
    http: Client,
}

#[tokio::main]
async fn main() -> Result<()> {
    myriad_updater::log::init();

    let listen: SocketAddr = std::env::var("GATEWAY_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:1104".into())
        .parse()
        .context("invalid GATEWAY_LISTEN")?;

    let upstream = std::env::var("UPDATER_UPSTREAM")
        .unwrap_or_else(|_| "http://updater:1101".into())
        .trim_end_matches('/')
        .to_string();
    if upstream.is_empty() {
        return Err(anyhow!("UPDATER_UPSTREAM cannot be empty"));
    }

    let token = std::env::var("UPDATE_TOKEN").context("UPDATE_TOKEN is required by updater-gateway")?;
    if token.trim().len() < GATEWAY_SECRET_MIN_LEN {
        return Err(anyhow!("UPDATE_TOKEN must be at least {GATEWAY_SECRET_MIN_LEN} characters"));
    }

    let gateway_secret = std::env::var("UPDATER_GATEWAY_SECRET")
        .context("UPDATER_GATEWAY_SECRET is required by updater-gateway")?;
    if gateway_secret.trim().len() < GATEWAY_SECRET_MIN_LEN {
        return Err(anyhow!(
            "UPDATER_GATEWAY_SECRET must be at least {GATEWAY_SECRET_MIN_LEN} characters"
        ));
    }

    let http = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(120))
        .pool_idle_timeout(Duration::from_secs(30))
        .user_agent("myriad-updater-gateway")
        .build()
        .context("build HTTP client")?;

    let state = GatewayState {
        upstream: upstream.clone(),
        token,
        gateway_secret,
        http,
    };

    info!(%listen, %upstream, "updater-gateway listening");

    let app = Router::new()
        .route("/healthz", axum::routing::get(local_healthz))
        .fallback(any(proxy))
        .with_state(Arc::new(state));

    let listener = tokio::net::TcpListener::bind(listen).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn local_healthz() -> impl IntoResponse {
    // Local liveness only — does not prove updater is up. Deep checks use
    // proxied `/status` (token injected) from admin routes. No gateway secret:
    // compose healthchecks hit localhost from the same container.
    (StatusCode::OK, axum::Json(serde_json::json!({"ok": true})))
}

async fn proxy(State(state): State<Arc<GatewayState>>, req: Request<Body>) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let headers = req.headers().clone();
    let (parts, body) = req.into_parts();
    let _ = parts; // method/uri/headers already cloned

    // Caller auth: shared secret between backend and gateway (admin-net peers).
    if let Err(resp) = authorize_gateway_caller(&headers, &state.gateway_secret) {
        return resp;
    }

    // Never trust a client-supplied update token — reject if the caller tries to set one.
    if headers.get(HEADER_UPDATE_TOKEN).is_some() {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "message": "X-Update-Token must not be set by callers; gateway injects it"
            })),
        )
            .into_response();
    }

    let body = match to_bytes(body, MAX_BODY).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                axum::Json(serde_json::json!({"message": "request body too large"})),
            )
                .into_response();
        }
    };

    let target = upstream_url(&state.upstream, &uri);
    let mut builder = state
        .http
        .request(method_to_reqwest(&method), &target)
        .header("X-Update-Token", &state.token);

    // Forward a narrow header allowlist. Never forward gateway secret or update token.
    for name in [
        header::CONTENT_TYPE,
        header::ACCEPT,
        HeaderName::from_static("idempotency-key"),
        HeaderName::from_static("x-update-actor"),
        HeaderName::from_static("x-myriad-confirm-risk"),
    ] {
        if let Some(value) = headers.get(&name) {
            builder = builder.header(name.clone(), value.clone());
        }
    }

    if !body.is_empty() {
        builder = builder.body(body.to_vec());
    }

    match builder.send().await {
        Ok(upstream_resp) => forward_response(upstream_resp).await,
        Err(e) => {
            error!(err = %e, %target, "updater-gateway upstream failure");
            (
                StatusCode::BAD_GATEWAY,
                axum::Json(serde_json::json!({"message": "updater upstream unavailable"})),
            )
                .into_response()
        }
    }
}

/// Validate `X-Updater-Gateway-Secret` with a length-checked constant-time compare.
fn authorize_gateway_caller(
    headers: &HeaderMap,
    expected: &str,
) -> Result<(), Response> {
    let provided = headers
        .get(HEADER_GATEWAY_SECRET)
        .and_then(|v| v.to_str().ok());
    match provided {
        Some(got) if constant_time_eq(got.as_bytes(), expected.as_bytes()) => Ok(()),
        Some(_) => Err((
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({"message": "invalid gateway secret"})),
        )
            .into_response()),
        None => Err((
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({
                "message": "missing X-Updater-Gateway-Secret"
            })),
        )
            .into_response()),
    }
}

/// Constant-time equality for equal-length secrets. Different lengths return false
/// immediately (length is not secret for our fixed ≥32 random secrets).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn upstream_url(base: &str, uri: &Uri) -> String {
    let path = uri.path();
    let path = if path.is_empty() { "/" } else { path };
    match uri.query() {
        Some(q) => format!("{base}{path}?{q}"),
        None => format!("{base}{path}"),
    }
}

fn method_to_reqwest(method: &Method) -> reqwest::Method {
    match *method {
        Method::GET => reqwest::Method::GET,
        Method::POST => reqwest::Method::POST,
        Method::PUT => reqwest::Method::PUT,
        Method::PATCH => reqwest::Method::PATCH,
        Method::DELETE => reqwest::Method::DELETE,
        Method::HEAD => reqwest::Method::HEAD,
        Method::OPTIONS => reqwest::Method::OPTIONS,
        _ => reqwest::Method::GET,
    }
}

async fn forward_response(upstream: reqwest::Response) -> Response {
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut out_headers = HeaderMap::new();
    for (name, value) in upstream.headers().iter() {
        // Hop-by-hop / framing headers must not be blindly copied.
        let lower = name.as_str();
        if matches!(
            lower,
            "connection"
                | "transfer-encoding"
                | "keep-alive"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "te"
                | "trailers"
                | "upgrade"
                | "content-length"
        ) {
            continue;
        }
        if let Ok(v) = HeaderValue::from_bytes(value.as_bytes()) {
            out_headers.insert(name.clone(), v);
        }
    }

    match upstream.bytes().await {
        Ok(bytes) => {
            let mut response = Response::new(Body::from(bytes));
            *response.status_mut() = status;
            *response.headers_mut() = out_headers;
            response
        }
        Err(e) => {
            warn!(err = %e, "failed to read upstream body");
            (
                StatusCode::BAD_GATEWAY,
                axum::Json(serde_json::json!({"message": "failed to read updater response"})),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn upstream_url_joins_path_and_query() {
        let uri: Uri = "/status?full=1".parse().unwrap();
        assert_eq!(
            upstream_url("http://updater:1101", &uri),
            "http://updater:1101/status?full=1"
        );
        let root: Uri = "/".parse().unwrap();
        assert_eq!(
            upstream_url("http://updater:1101", &root),
            "http://updater:1101/"
        );
    }

    #[test]
    fn constant_time_eq_accepts_match() {
        assert!(constant_time_eq(b"same-secret-value-32chars-ok!!", b"same-secret-value-32chars-ok!!"));
    }

    #[test]
    fn constant_time_eq_rejects_mismatch_and_len() {
        assert!(!constant_time_eq(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"));
        assert!(!constant_time_eq(b"short", b"longer-than-short"));
        assert!(!constant_time_eq(b"", b"x"));
    }

    #[test]
    fn authorize_accepts_correct_secret() {
        let secret = "abcdefghijklmnopqrstuvwxyz012345"; // 32
        let mut headers = HeaderMap::new();
        headers.insert(
            HEADER_GATEWAY_SECRET,
            HeaderValue::from_static("abcdefghijklmnopqrstuvwxyz012345"),
        );
        assert!(authorize_gateway_caller(&headers, secret).is_ok());
    }

    #[test]
    fn authorize_rejects_missing_secret() {
        let headers = HeaderMap::new();
        let err = authorize_gateway_caller(&headers, "abcdefghijklmnopqrstuvwxyz012345").unwrap_err();
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn authorize_rejects_wrong_secret() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HEADER_GATEWAY_SECRET,
            HeaderValue::from_static("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"),
        );
        let err = authorize_gateway_caller(&headers, "abcdefghijklmnopqrstuvwxyz012345").unwrap_err();
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }
}
