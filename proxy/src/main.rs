//! Myriad maintenance-aware reverse proxy.
//!
//! Behaviour:
//!  - Reads /state/maintenance.json. Missing file = inactive (rescue path).
//!    Unreadable or corrupt files fail closed (serve maintenance) instead of
//!    forwarding as if the site were idle.
//!  - When `active=true`, all non-allowlisted requests are served the embedded maintenance page.
//!  - Otherwise, forwards to backend/frontend over plain HTTP via internal docker network DNS.
//!  - Response bodies are **streamed** (no full-buffer collect) to keep memory/TTFB low.
//!  - `/healthz` (proxy itself) always returns 200.
//!  - `/_updater/*` can forward to the updater service when explicitly enabled for rescue.
//!
//! Missing state file: forward normally so the proxy stays a rescue path.
//! Unreadable/corrupt state: fail closed and serve the maintenance page.

use std::collections::HashSet;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::any;
use chrono::{DateTime, Utc};
use hyper_util::client::legacy::{Client, connect::HttpConnector};
use hyper_util::rt::{TokioExecutor, TokioIo};
use ipnet::IpNet;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::RwLock;
use tracing::{info, warn};

/// How long a successful maintenance.json read stays cached. Short enough that
/// updater phase transitions appear promptly; long enough to avoid a disk read
/// on every static-asset request.
const MAINT_CACHE_TTL: Duration = Duration::from_millis(250);

/// Document-level Permissions-Policy. Must match backend `security.rs` and
/// frontend Vite `DOCUMENT_PERMISSIONS_POLICY`: first-party geolocation (weather)
/// and microphone (listen/speak); camera stays off.
/// Applied on proxy responses when upstream omitted it (static frontend often does).
const PERMISSIONS_POLICY: &str = "geolocation=(self), microphone=(self), camera=()";

#[derive(Clone)]
struct AppState {
    state_path: PathBuf,
    backend_upstream: String,
    federation_upstream: Option<String>,
    persona_upstream: Option<String>,
    persona_isolated: Arc<std::sync::atomic::AtomicBool>,
    persona_requests: Arc<tokio::sync::Semaphore>,
    persona_streams: Arc<tokio::sync::Semaphore>,
    persona_controls: Arc<tokio::sync::Semaphore>,
    federation_isolated: Arc<std::sync::atomic::AtomicBool>,
    federation_requests: Arc<tokio::sync::Semaphore>,
    federation_websockets: Arc<tokio::sync::Semaphore>,
    frontend_upstream: String,
    updater_upstream: String,
    /// When false, `/_updater/*` returns 404. The intended path is through the backend
    /// (`/api/admin/updater/*`) which uses admin session auth + holds UPDATE_TOKEN.
    /// Set `PROXY_ALLOW_DIRECT_UPDATER=true` to open the direct channel for rescue scenarios.
    allow_direct_updater: bool,
    /// Addresses allowed to supply forwarding headers. Requests from every other peer
    /// have their forwarding headers discarded to prevent client-IP spoofing.
    trusted_upstreams: Vec<IpNet>,
    client: Client<HttpConnector, Body>,
    maint_cache: Arc<RwLock<MaintCache>>,
}

#[derive(Default)]
struct MaintCache {
    loaded_at: Option<Instant>,
    value: MaintenanceFile,
}

const MAINTENANCE_HTML: &str = include_str!("maintenance.html");

#[derive(Debug, Clone, Deserialize, Default)]
struct MaintenanceFile {
    #[serde(default)]
    active: bool,
    #[serde(default)]
    phase: Option<String>,
    #[serde(default)]
    from_version: Option<String>,
    #[serde(default)]
    to_version: Option<String>,
    #[serde(default)]
    updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    message_key: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let state_path = std::env::var("PROXY_STATE_FILE")
        .unwrap_or_else(|_| "/state/maintenance.json".into())
        .into();
    let backend_upstream =
        std::env::var("PROXY_BACKEND_UPSTREAM").unwrap_or_else(|_| "http://backend:1103".into());
    let federation_upstream = std::env::var("PROXY_FEDERATION_UPSTREAM")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let frontend_upstream =
        std::env::var("PROXY_FRONTEND_UPSTREAM").unwrap_or_else(|_| "http://frontend:1102".into());
    let updater_upstream =
        std::env::var("PROXY_UPDATER_UPSTREAM").unwrap_or_else(|_| "http://updater:1101".into());
    let allow_direct_updater = std::env::var("PROXY_ALLOW_DIRECT_UPDATER")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false);
    let trusted_upstreams =
        parse_trusted_upstreams(std::env::var("PROXY_TRUSTED_UPSTREAMS").ok().as_deref())?;
    let listen: SocketAddr = std::env::var("PROXY_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:80".into())
        .parse()?;

    let client = Client::builder(TokioExecutor::new())
        .pool_idle_timeout(Duration::from_secs(30))
        .build_http();

    info!(
        allow_direct_updater,
        trusted_upstream_count = trusted_upstreams.len(),
        "proxy startup: streaming forward; direct /_updater/* {}",
        if allow_direct_updater {
            "ENABLED"
        } else {
            "disabled"
        }
    );
    if allow_direct_updater {
        // Rescue path only — keep off for normal admin→backend→updater flow.
        warn!(
            "SECURITY: PROXY_ALLOW_DIRECT_UPDATER=true — /_updater/* is reachable via the proxy. \
             Leave this false except when rescuing a down backend; prefer /api/admin/updater/*."
        );
    }

    let state = AppState {
        state_path,
        backend_upstream,
        federation_upstream,
        persona_upstream: std::env::var("PROXY_PERSONA_UPSTREAM")
            .ok()
            .filter(|s| !s.trim().is_empty()),
        persona_isolated: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        persona_requests: Arc::new(tokio::sync::Semaphore::new(32)),
        persona_streams: Arc::new(tokio::sync::Semaphore::new(64)),
        persona_controls: Arc::new(tokio::sync::Semaphore::new(16)),
        // Unknown backend state must not send federation work into web.
        federation_isolated: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        federation_requests: Arc::new(tokio::sync::Semaphore::new(32)),
        federation_websockets: Arc::new(tokio::sync::Semaphore::new(64)),
        frontend_upstream,
        updater_upstream,
        allow_direct_updater,
        trusted_upstreams,
        client,
        maint_cache: Arc::new(RwLock::new(MaintCache::default())),
    };

    tokio::spawn(refresh_federation_routing(state.clone()));

    let app = Router::new()
        .route("/healthz", axum::routing::get(|| async { "ok" }))
        .route("/_proxy/status", axum::routing::get(proxy_status))
        .fallback(any(handle))
        .with_state(Arc::new(state));

    let listener = tokio::net::TcpListener::bind(listen).await?;
    info!(addr = %listen, "proxy listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

/// Wait for Ctrl+C or (on Unix) SIGTERM so Docker/K8s `stop` enters Axum graceful shutdown.
async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C signal");
        },
        _ = terminate => {
            info!("Received terminate signal");
        },
    }
}

async fn handle(
    State(state): State<Arc<AppState>>,
    ConnectInfo(client_addr): ConnectInfo<SocketAddr>,
    req: Request,
) -> Result<Response, Infallible> {
    let path = req.uri().path().to_string();
    if path == "/internal" || path.starts_with("/internal/") {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }

    // Updater API direct channel: off by default. The backend at /api/admin/updater/*
    // is the recommended path (admin session + server-held UPDATE_TOKEN). The direct path
    // is kept for rescue scenarios (backend itself down) — operators enable it via
    // PROXY_ALLOW_DIRECT_UPDATER=true. See docs/updater-spec.md §15.
    if path.starts_with("/_updater/") {
        if !state.allow_direct_updater {
            return Ok((
                StatusCode::NOT_FOUND,
                "direct updater channel disabled; use /api/admin/updater/*",
            )
                .into_response());
        }
        let upstream_path = updater_path_with_query(req.uri());
        return Ok(forward(
            &state,
            &state.updater_upstream,
            &upstream_path,
            req,
            client_addr,
        )
        .await
        .unwrap_or_else(bad_gateway));
    }

    let maint = read_maintenance_cached(&state).await;
    if maint.active {
        return Ok(maintenance_response(&maint));
    }

    // Route business traffic. Besides /api, the backend also serves the public
    // ActivityPub/MFP endpoints registered outside /api (WebFinger discovery,
    // NodeInfo, actor documents and inboxes) — remote instances resolve
    // @user@domain against these, so they must not fall through to the frontend.
    // SEO: crawler UAs on `/`, module indexes, /tapp/run/* and /journal/articles/*
    // get the backend HTML shell; browsers get SPA.
    let ua = req
        .headers()
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let query = req.uri().query().unwrap_or("");
    let persona = is_persona_path(&path);
    let upstream = if persona
        && state
            .persona_isolated
            .load(std::sync::atomic::Ordering::Acquire)
    {
        let Some(upstream) = state.persona_upstream.as_ref() else {
            return Ok(StatusCode::SERVICE_UNAVAILABLE.into_response());
        };
        upstream
    } else if is_federation_path(&path) {
        if state
            .federation_isolated
            .load(std::sync::atomic::Ordering::Acquire)
        {
            state
                .federation_upstream
                .as_ref()
                .unwrap_or(&state.backend_upstream)
        } else {
            &state.backend_upstream
        }
    } else if is_backend_path_for(&path, ua, query) {
        &state.backend_upstream
    } else {
        &state.frontend_upstream
    };
    if persona && is_websocket_upgrade(req.headers()) {
        return Ok(StatusCode::BAD_REQUEST.into_response());
    }
    if persona {
        let subscription = req.method() == hyper::Method::GET
            && (path.ends_with("/stream") || path == "/api/speech/convo/events");
        let control = path == "/api/agent/presence"
            || path.ends_with("/cancel")
            || matches!(
                path.as_str(),
                "/api/speech/convo/stop" | "/api/speech/convo/interrupt"
            );
        let budget = if subscription {
            &state.persona_streams
        } else if control {
            &state.persona_controls
        } else {
            &state.persona_requests
        };
        let Ok(permit) = budget.clone().try_acquire_owned() else {
            return Ok(StatusCode::SERVICE_UNAVAILABLE.into_response());
        };
        let response = tokio::time::timeout(
            Duration::from_secs(600),
            forward(
                &state,
                upstream,
                &path_with_query(req.uri()),
                req,
                client_addr,
            ),
        )
        .await;
        return Ok(match response {
            Ok(Ok(response)) => {
                let (parts, body) = response.into_parts();
                Response::from_parts(
                    parts,
                    Body::new(DomainBody {
                        body,
                        _permit: permit,
                        deadline: Box::pin(tokio::time::sleep(Duration::from_secs(3600))),
                    }),
                )
            }
            Ok(Err(error)) => bad_gateway(error),
            Err(_) => StatusCode::GATEWAY_TIMEOUT.into_response(),
        });
    }
    if is_websocket_upgrade(req.headers()) {
        let permit = if is_federation_path(&path) {
            match state.federation_websockets.clone().try_acquire_owned() {
                Ok(permit) => Some(permit),
                Err(_) => return Ok(StatusCode::SERVICE_UNAVAILABLE.into_response()),
            }
        } else {
            None
        };
        return Ok(forward_websocket(
            &state,
            upstream,
            &path_with_query(req.uri()),
            req,
            client_addr,
            permit,
        )
        .await
        .unwrap_or_else(bad_gateway));
    }
    if is_federation_path(&path) {
        let Ok(permit) = state.federation_requests.clone().try_acquire_owned() else {
            return Ok(StatusCode::SERVICE_UNAVAILABLE.into_response());
        };
        let response = tokio::time::timeout(
            Duration::from_secs(60),
            forward(
                &state,
                upstream,
                &path_with_query(req.uri()),
                req,
                client_addr,
            ),
        )
        .await;
        return Ok(match response {
            Ok(Ok(response)) => {
                let (parts, body) = response.into_parts();
                Response::from_parts(
                    parts,
                    Body::new(DomainBody {
                        body,
                        _permit: permit,
                        deadline: Box::pin(tokio::time::sleep(Duration::from_secs(180))),
                    }),
                )
            }
            Ok(Err(error)) => bad_gateway(error),
            Err(_) => StatusCode::GATEWAY_TIMEOUT.into_response(),
        });
    }
    Ok(forward(
        &state,
        upstream,
        &path_with_query(req.uri()),
        req,
        client_addr,
    )
    .await
    .unwrap_or_else(bad_gateway))
}

/// Keep the domain budget through streaming, including a stalled upstream body.
struct DomainBody {
    body: Body,
    _permit: tokio::sync::OwnedSemaphorePermit,
    deadline: std::pin::Pin<Box<tokio::time::Sleep>>,
}

impl hyper::body::Body for DomainBody {
    type Data = bytes::Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<hyper::body::Frame<Self::Data>, Self::Error>>> {
        use std::future::Future;
        if self.deadline.as_mut().poll(cx).is_ready() {
            return std::task::Poll::Ready(Some(Err(axum::Error::new(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "domain response deadline",
            )))));
        }
        std::pin::Pin::new(&mut self.body).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }
    fn size_hint(&self) -> hyper::body::SizeHint {
        self.body.size_hint()
    }
}

/// Routing follows an explicit backend capability, never a failed worker probe.
/// This preserves old-image rollback without silently moving failed worker work
/// into a current web process. Failed/malformed probes retain the last policy.
async fn refresh_federation_routing(state: AppState) {
    use http_body_util::{BodyExt, Limited};
    let mut interval = tokio::time::interval(Duration::from_secs(2));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        let probe = async {
            let request = hyper::Request::builder()
                .uri(format!("{}/health", state.backend_upstream))
                .body(Body::empty())
                .ok()?;
            let response = state.client.request(request).await.ok()?;
            if response.status() != StatusCode::OK {
                return None;
            }
            let bytes = Limited::new(response.into_body(), 64 * 1024)
                .collect()
                .await
                .ok()?
                .to_bytes();
            let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
            Some((
                backend_federation_isolated(&value),
                backend_isolated(&value, "persona_http_isolated"),
            ))
        };
        if let Ok(Some((federation, persona))) =
            tokio::time::timeout(Duration::from_secs(2), probe).await
        {
            if let Some(isolated) = federation {
                state
                    .federation_isolated
                    .store(isolated, std::sync::atomic::Ordering::Release);
            }
            if let Some(isolated) = persona {
                state
                    .persona_isolated
                    .store(isolated, std::sync::atomic::Ordering::Release);
            }
        }
    }
}

fn backend_federation_isolated(value: &serde_json::Value) -> Option<bool> {
    backend_isolated(value, "federation_http_isolated")
}

fn backend_isolated(value: &serde_json::Value, field: &str) -> Option<bool> {
    if value.get("service")?.as_str()? != "myriad-backend" || value.get("mode")?.as_str()? != "full"
    {
        return None;
    }
    match value.get(field) {
        Some(value) => value.as_bool(),
        None => Some(false), // Proven legacy backend, before domain extraction.
    }
}

fn is_persona_path(path: &str) -> bool {
    [
        "/api/agent",
        "/api/speech",
        "/api/merope/rig",
        "/api/tapp/agent/v2/interactions",
    ]
    .iter()
    .any(|prefix| {
        path == *prefix
            || path
                .strip_prefix(prefix)
                .is_some_and(|tail| tail.starts_with('/'))
    })
}

/// Includes AP object dereference paths as well as inbox and authenticated APIs.
/// Index/SEO paths such as /reports and /library remain on their existing owners.
fn is_federation_path(path: &str) -> bool {
    matches!(
        path,
        "/.well-known/webfinger" | "/.well-known/nodeinfo" | "/nodeinfo/2.1" | "/inbox"
    ) || [
        "/api/federation/",
        "/api/admin/federation/",
        "/api/tapp/federation/",
        "/users/",
        "/activities/",
        "/notes/",
        "/reports/",
        "/tapps/",
        "/library/",
        "/phantasi/articles/",
    ]
    .iter()
    .any(|prefix| path.len() > prefix.len() && path.starts_with(prefix))
}

/// RFC 6455 handshake detection: `Connection: upgrade` + `Upgrade: websocket`.
fn is_websocket_upgrade(headers: &HeaderMap) -> bool {
    let connection_has_upgrade = parse_connection_tokens(headers).contains("upgrade");
    let upgrade_is_websocket = headers
        .get(header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);
    connection_has_upgrade && upgrade_is_websocket
}

/// Forward a WebSocket handshake and, on 101, bridge the two upgraded
/// connections byte-for-byte. The plain `forward` path strips hop-by-hop
/// headers and never resolves upgrades, so it can only break handshakes.
async fn forward_websocket(
    state: &AppState,
    upstream_base: &str,
    path_q: &str,
    req: Request,
    client_addr: SocketAddr,
    permit: Option<tokio::sync::OwnedSemaphorePermit>,
) -> anyhow::Result<Response> {
    let (mut parts, _body) = req.into_parts();
    let client_on_upgrade = parts
        .extensions
        .remove::<hyper::upgrade::OnUpgrade>()
        .ok_or_else(|| anyhow::anyhow!("client connection does not support upgrade"))?;

    let url = format!("{}{}", upstream_base, path_q);
    let mut builder = hyper::Request::builder()
        .method(parts.method.clone())
        .uri(&url);
    let request_headers = build_forward_request_headers(
        &parts.headers,
        client_addr.ip(),
        &state.trusted_upstreams,
        HeaderTransfer::WebSocket,
    );
    for (k, v) in request_headers.iter() {
        builder = builder.header(k, v);
    }
    let upstream_req = builder.body(Body::empty())?;
    let upstream_resp =
        tokio::time::timeout(Duration::from_secs(15), state.client.request(upstream_req)).await??;

    if upstream_resp.status() != StatusCode::SWITCHING_PROTOCOLS {
        // Upstream refused the upgrade (auth failure, bad ticket, …) — relay its answer.
        let (resp_parts, body) = upstream_resp.into_parts();
        let mut out = Response::builder().status(resp_parts.status);
        let response_headers =
            filter_forward_response_headers(&resp_parts.headers, HeaderTransfer::Http);
        for (k, v) in response_headers.iter() {
            out = out.header(k, v);
        }
        return Ok(out.body(Body::new(body))?);
    }

    // Relay the 101 after the shared response policy preserves only the
    // upgrade fields plus end-to-end WebSocket metadata. The client upgrade
    // only resolves after this response is written out.
    let mut out = Response::builder().status(StatusCode::SWITCHING_PROTOCOLS);
    let response_headers =
        filter_forward_response_headers(upstream_resp.headers(), HeaderTransfer::WebSocket);
    for (k, v) in response_headers.iter() {
        out = out.header(k, v);
    }
    let response = out.body(Body::empty())?;

    tokio::spawn(async move {
        let _permit = permit;
        let upstream_io = match hyper::upgrade::on(upstream_resp).await {
            Ok(io) => io,
            Err(err) => {
                warn!(error = %err, "websocket upstream upgrade failed");
                return;
            }
        };
        let client_io = match client_on_upgrade.await {
            Ok(io) => io,
            Err(err) => {
                warn!(error = %err, "websocket client upgrade failed");
                return;
            }
        };
        let mut upstream_io = TokioIo::new(upstream_io);
        let mut client_io = TokioIo::new(client_io);
        // Bind the copy result so `_permit` outlives the bridge future
        // (2024 drops tail temps before locals; 2021 dropped them last).
        let copied = tokio::io::copy_bidirectional(&mut client_io, &mut upstream_io).await;
        if let Err(err) = copied {
            // Normal on abrupt disconnects; keep at debug level.
            tracing::debug!(error = %err, "websocket bridge closed with error");
        }
    });

    Ok(response)
}

/// Known crawler / link-preview user-agents that should receive backend SEO HTML
/// for `/tapp/run/*` instead of the SPA shell.
fn is_seo_crawler_ua(ua: &str) -> bool {
    let ua = ua.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "googlebot",
        "bingbot",
        "slurp",
        "duckduckbot",
        "baiduspider",
        "yandexbot",
        "facebookexternalhit",
        "facebot",
        "twitterbot",
        "linkedinbot",
        "embedly",
        "quora link preview",
        "pinterest",
        "applebot",
        "semrushbot",
        "ahrefsbot",
        "mj12bot",
        "dotbot",
        "petalbot",
        "bytespider",
        "discordbot",
        "telegrambot",
        "whatsapp",
        "slackbot",
        "redditbot",
        "skypeuripreview",
        "rogerbot",
        "screaming frog",
        "ia_archiver",
        "chatgpt-user",
        "gptbot",
        "claudebot",
        "anthropic-ai",
        "storebot-google",
        "google-inspectiontool",
        "google-site-verification",
        "preview",
        "qq-url-preview",
        "dingtalkbot",
    ];
    MARKERS.iter().any(|m| ua.contains(m))
        // Generic bot/spider/crawl (exclude common false positives is hard; keep short)
        || ua.contains("bot/")
        || ua.contains("spider")
        || ua.contains("crawler")
}

/// In-app browsers that also fetch link previews with the same UA (WeChat / Weibo /
/// WeCom). Must NOT be treated as ordinary browsers, but humans in these WebViews
/// still need the SPA — backend shells inject a `?_spa=1` bounce for them.
/// Keep in sync with frontend `seoShell.mjs` and backend `is_inapp_share_ua`.
fn is_inapp_share_ua(ua: &str) -> bool {
    let ua = ua.to_ascii_lowercase();
    ua.contains("micromessenger")
        || ua.contains("windowswechat")
        || ua.contains("wxwork")
        || ua.contains("weibo")
}

fn query_has_spa_bypass(query: &str) -> bool {
    query.split('&').any(|pair| pair == "_spa=1")
}

fn wants_seo_html_shell(user_agent: &str) -> bool {
    is_seo_crawler_ua(user_agent) || is_inapp_share_ua(user_agent)
}

/// Paths served by the backend. Everything else goes to the frontend SPA.
///
/// Keep in sync with:
/// - `backend` public federation routes in `main.rs` (non-`/api` ActivityPub + media)
/// - frontend `isBackendDevProxyPath` in `scripts/vite/backendDevProxy.mjs`
/// - docs/deployment/PORTS.md
///
/// Missing an entry silently serves the SPA HTML for that URL (broken media, broken
/// WebFinger, etc.). Prefer whole-site outer reverse proxies so this list only needs
/// to live in Myriad proxy.
///
/// `user_agent` is used only for crawler HTML shells (crawlers → backend, humans → SPA).
fn is_seo_document_shell_path(path: &str) -> bool {
    matches!(
        path,
        "/" | "/tapp"
            | "/journal"
            | "/journal/feeds"
            | "/journal/notes"
            | "/journal/friends"
            | "/library"
            | "/reports"
    ) || path.starts_with("/tapp/run/")
        || path.starts_with("/journal/articles/")
        || path.starts_with("/journal/topics/")
}

#[cfg(test)]
fn is_backend_path(path: &str, user_agent: &str) -> bool {
    is_backend_path_for(path, user_agent, "")
}

fn is_backend_path_for(path: &str, user_agent: &str, query: &str) -> bool {
    if path.starts_with("/api/")
        || path == "/health"
        || path == "/ready"
        // Public SEO sitemap + robots (backend api::seo)
        || path == "/sitemap.xml"
        || path == "/robots.txt"
        || path == "/llms.txt"
        || path == "/journal/notes.xml"
    {
        return true;
    }
    // Crawler / in-app share HTML shells (ordinary browsers stay on SPA).
    // `?_spa=1` is the bounce target so WeChat/Weibo WebViews can load the SPA
    // after the first-byte OG document.
    if is_seo_document_shell_path(path)
        && wants_seo_html_shell(user_agent)
        && !query_has_spa_bypass(query)
    {
        return true;
    }
    // Federation (ActivityPub/MFP) public endpoints, see backend main.rs.
    path == "/.well-known/webfinger"
        || path == "/.well-known/nodeinfo"
        || path == "/nodeinfo/2.1"
        || path == "/inbox"
        // Actor, outbox, followers, following, per-user inbox, avatar
        || path.starts_with("/users/")
        // Site media is served by the web process, including historical federation
        // attachment URLs. Must not hit SPA or the federation worker.
        || path.starts_with("/media/federation/")
        || path.starts_with("/media/assets/")
}

fn bad_gateway(err: anyhow::Error) -> Response {
    warn!(error = %err, "upstream proxy error");
    (StatusCode::BAD_GATEWAY, format!("bad gateway: {err}")).into_response()
}

fn path_with_query(uri: &Uri) -> String {
    match uri.path_and_query() {
        Some(pq) => pq.to_string(),
        None => uri.path().to_string(),
    }
}

fn updater_path_with_query(uri: &Uri) -> String {
    let full = path_with_query(uri);
    match full.strip_prefix("/_updater/") {
        Some(rest) => format!("/{rest}"),
        None => full,
    }
}

async fn forward(
    state: &AppState,
    upstream_base: &str,
    path_q: &str,
    req: Request,
    client_addr: SocketAddr,
) -> anyhow::Result<Response> {
    let (parts, body) = req.into_parts();
    let url = format!("{}{}", upstream_base, path_q);
    let mut builder = hyper::Request::builder().method(parts.method).uri(&url);
    let request_headers = build_forward_request_headers(
        &parts.headers,
        client_addr.ip(),
        &state.trusted_upstreams,
        HeaderTransfer::Http,
    );
    for (k, v) in request_headers.iter() {
        builder = builder.header(k, v);
    }
    let upstream_req = builder.body(body)?;
    let resp = state.client.request(upstream_req).await?;
    let (parts, body) = resp.into_parts();
    // Stream the upstream body through — do not buffer into memory.
    let mut out = Response::builder().status(parts.status);
    let response_headers = filter_forward_response_headers(&parts.headers, HeaderTransfer::Http);
    for (k, v) in response_headers.iter() {
        out = out.header(k, v);
    }
    let mut response = out.body(Body::new(body))?;
    ensure_permissions_policy(response.headers_mut());
    Ok(response)
}

/// Ensure SPA/document responses expose geolocation for weather when upstream
/// (e.g. `serve` static frontend) did not set Permissions-Policy. Never overrides
/// an existing header (backend already sets the same policy).
fn ensure_permissions_policy(headers: &mut HeaderMap) {
    if headers.contains_key("permissions-policy") {
        return;
    }
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static(PERMISSIONS_POLICY),
    );
}

/// Headers nominated by `Connection` are hop-by-hop even when they are not
/// part of the fixed HTTP/1.1 list.  The policy is shared by HTTP and WS in
/// both directions so a dynamic token cannot leak through one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeaderTransfer {
    Http,
    WebSocket,
}

#[derive(Debug, Clone, Default)]
struct HeaderPolicy {
    connection_tokens: HashSet<String>,
    trusted_peer: bool,
}

impl HeaderPolicy {
    fn for_request(headers: &HeaderMap, peer_ip: IpAddr, trusted_upstreams: &[IpNet]) -> Self {
        Self {
            connection_tokens: parse_connection_tokens(headers),
            trusted_peer: is_trusted_hop(peer_ip, trusted_upstreams),
        }
    }

    fn for_response(headers: &HeaderMap) -> Self {
        Self {
            connection_tokens: parse_connection_tokens(headers),
            trusted_peer: false,
        }
    }

    fn should_strip(&self, name: &str, transfer: HeaderTransfer) -> bool {
        // HeaderName::as_str is already lowercase; Connection tokens are stored lower.
        if self.connection_tokens.contains(name) {
            return true;
        }
        if transfer == HeaderTransfer::WebSocket && matches!(name, "connection" | "upgrade") {
            // The caller adds the canonical Connection/Upgrade fields below.
            return false;
        }
        is_hop_by_hop(name)
    }

    /// Copy end-to-end request headers and add only proxy-owned forwarding
    /// headers. For WS handshakes, Connection/Upgrade are canonicalized so an
    /// inbound `Connection: Foo, Upgrade` cannot nominate `Foo` upstream.
    fn request_headers(
        &self,
        headers: &HeaderMap,
        transfer: HeaderTransfer,
        client_ip: IpAddr,
    ) -> HeaderMap {
        let mut out = HeaderMap::new();
        for (name, value) in headers.iter() {
            let normalized = name.as_str();
            if normalized == "host"
                || is_proxy_managed_forwarded_header(normalized)
                || self.should_strip(normalized, transfer)
            {
                continue;
            }
            out.append(name.clone(), value.clone());
        }

        if transfer == HeaderTransfer::WebSocket {
            out.insert(header::CONNECTION, HeaderValue::from_static("Upgrade"));
            out.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
        }

        let client_ip = HeaderValue::from_str(&client_ip.to_string())
            .expect("an IpAddr always serializes to a valid header value");
        out.insert(
            HeaderName::from_static("x-forwarded-for"),
            client_ip.clone(),
        );
        out.insert(HeaderName::from_static("x-real-ip"), client_ip);
        out
    }

    /// Strip fixed and Connection-nominated response headers. A successful WS
    /// handshake keeps only the two fields needed by the client upgrade; all
    /// other hop-by-hop fields remain removed.
    fn response_headers(&self, headers: &HeaderMap, transfer: HeaderTransfer) -> HeaderMap {
        let mut out = HeaderMap::new();
        for (name, value) in headers.iter() {
            let normalized = name.as_str();
            if transfer == HeaderTransfer::WebSocket
                && matches!(normalized, "connection" | "upgrade")
            {
                continue;
            }
            if self.should_strip(normalized, transfer) {
                continue;
            }
            out.append(name.clone(), value.clone());
        }

        if transfer == HeaderTransfer::WebSocket {
            if self.connection_tokens.contains("upgrade") {
                out.insert(header::CONNECTION, HeaderValue::from_static("Upgrade"));
            }
            if headers
                .get(header::UPGRADE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("websocket"))
            {
                out.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
            }
        }
        out
    }
}

fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "proxy-connection"
            | "te"
            | "trailer"
            | "trailers"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn is_proxy_managed_forwarded_header(name: &str) -> bool {
    matches!(
        name,
        "x-forwarded-for"
            | "x-real-ip"
            | "x-forwarded-host"
            | "x-forwarded-proto"
            | "cf-connecting-ip"
            | "true-client-ip"
    )
}

/// Parse every Connection field (including repeated fields) into normalized
/// header names. Invalid tokens are ignored; all actual HeaderName values are
/// already validated by hyper/axum.
fn parse_connection_tokens(headers: &HeaderMap) -> HashSet<String> {
    let mut tokens = HashSet::new();
    for value in headers.get_all(header::CONNECTION).iter() {
        let Ok(value) = value.to_str() else {
            continue;
        };
        for token in value.split(',') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            if HeaderName::from_bytes(token.as_bytes()).is_ok() {
                tokens.insert(token.to_ascii_lowercase());
            }
        }
    }
    tokens
}

fn parse_trusted_upstreams(value: Option<&str>) -> anyhow::Result<Vec<IpNet>> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .parse::<IpNet>()
                .or_else(|_| value.parse::<IpAddr>().map(IpNet::from))
                .map_err(|_| anyhow::anyhow!("invalid PROXY_TRUSTED_UPSTREAMS entry: {value}"))
        })
        .collect()
}

fn is_in_allowlist(ip: IpAddr, trusted_upstreams: &[IpNet]) -> bool {
    trusted_upstreams
        .iter()
        .any(|network| network.contains(&ip))
}

/// A forwarding header is trusted only when the direct TCP peer is explicitly
/// present in `PROXY_TRUSTED_UPSTREAMS`. An empty allowlist therefore trusts no
/// peer; local development can opt in by listing its loopback/Docker peer
/// explicitly rather than inheriting a production-wide private-network trust.
fn is_trusted_hop(ip: IpAddr, trusted_upstreams: &[IpNet]) -> bool {
    is_in_allowlist(ip, trusted_upstreams)
}

/// Parse X-Forwarded-For, skipping unparseable tokens instead of dropping the whole chain.
fn parse_forwarded_for_ips(headers: &HeaderMap) -> Vec<IpAddr> {
    headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| {
            value
                .split(',')
                .filter_map(|entry| entry.trim().parse::<IpAddr>().ok())
        })
        .collect()
}

fn parse_single_ip_header(headers: &HeaderMap, name: &str) -> Option<IpAddr> {
    headers
        .get_all(name)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find_map(|value| value.trim().parse::<IpAddr>().ok())
}

fn resolve_client_ip(headers: &HeaderMap, peer_ip: IpAddr, trusted_upstreams: &[IpNet]) -> IpAddr {
    // Public (or otherwise untrusted) direct peers cannot inject forwarding headers.
    if !is_trusted_hop(peer_ip, trusted_upstreams) {
        return peer_ip;
    }

    let forwarded_ips = parse_forwarded_for_ips(headers);

    // Walk from the trusted edge towards the client, stripping trusted hops.
    // Client-supplied addresses on the left of the real client are not accepted
    // as long as intermediate proxies append correctly.
    if let Some(client_ip) = forwarded_ips
        .iter()
        .rev()
        .copied()
        .find(|ip| !is_trusted_hop(*ip, trusted_upstreams))
    {
        return client_ip;
    }

    // CDN / edge provider headers (only consulted when peer is already trusted).
    if let Some(cdn_ip) = parse_single_ip_header(headers, "cf-connecting-ip")
        .or_else(|| parse_single_ip_header(headers, "true-client-ip"))
    {
        return cdn_ip;
    }

    if let Some(real_ip) = parse_single_ip_header(headers, "x-real-ip") {
        return real_ip;
    }

    // Entire XFF chain was trusted proxies (or empty); prefer leftmost original if any.
    forwarded_ips.first().copied().unwrap_or(peer_ip)
}

/// Build the sanitized request map used by both ordinary HTTP and WS upgrade
/// forwarding. The direct peer trust decision is made once and controls every
/// forwarding field, not just client IP extraction.
fn build_forward_request_headers(
    headers: &HeaderMap,
    peer_ip: IpAddr,
    trusted_upstreams: &[IpNet],
    transfer: HeaderTransfer,
) -> HeaderMap {
    let policy = HeaderPolicy::for_request(headers, peer_ip, trusted_upstreams);
    let client_ip = resolve_client_ip(headers, peer_ip, trusted_upstreams);
    let mut out = policy.request_headers(headers, transfer, client_ip);

    let proto = if policy.trusted_peer {
        forwarded_proto(headers).unwrap_or_else(|| HeaderValue::from_static("http"))
    } else {
        // The proxy listener is plain HTTP. Never let an untrusted XFP claim
        // that this connection was HTTPS (or another scheme).
        HeaderValue::from_static("http")
    };
    out.insert(HeaderName::from_static("x-forwarded-proto"), proto);

    // XFH is consumed only from a trusted direct peer. Otherwise Host is the
    // request authority observed on this connection and is copied as the
    // canonical public host for backend ActivityPub signature verification.
    if let Some(host) = forwarded_host_for_peer(headers, policy.trusted_peer) {
        out.insert(HeaderName::from_static("x-forwarded-host"), host);
    }
    if let Some(host) = upstream_request_host_for_peer(headers, policy.trusted_peer) {
        out.insert(header::HOST, host);
    }
    out
}

fn filter_forward_response_headers(headers: &HeaderMap, transfer: HeaderTransfer) -> HeaderMap {
    HeaderPolicy::for_response(headers).response_headers(headers, transfer)
}

// Raw accessors are intentionally only used after HeaderPolicy has
// established a trusted direct peer. Untrusted requests use the peer and Host
// fallbacks in `build_forward_request_headers` instead.
fn forwarded_proto(headers: &HeaderMap) -> Option<HeaderValue> {
    headers.get("x-forwarded-proto").cloned()
}

fn forwarded_host(headers: &HeaderMap) -> Option<HeaderValue> {
    headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .cloned()
}

fn forwarded_host_for_peer(headers: &HeaderMap, trusted_peer: bool) -> Option<HeaderValue> {
    if trusted_peer {
        forwarded_host(headers)
    } else {
        headers.get(header::HOST).cloned()
    }
}

/// Public `Host` to send on the upstream request.
///
/// Prefer the inbound `Host` (what the client or outer TLS proxy presented).
/// Fall back to `X-Forwarded-Host` when Host was stripped by an outer hop.
/// ActivityPub HTTP Signatures always cover `host`; remotes sign the public
/// name, so this must never become the internal upstream authority.
fn upstream_request_host(headers: &HeaderMap) -> Option<HeaderValue> {
    headers
        .get(header::HOST)
        .cloned()
        .or_else(|| headers.get("x-forwarded-host").cloned())
}

fn upstream_request_host_for_peer(headers: &HeaderMap, trusted_peer: bool) -> Option<HeaderValue> {
    if trusted_peer {
        upstream_request_host(headers)
    } else {
        headers.get(header::HOST).cloned()
    }
}

async fn read_maintenance_cached(state: &AppState) -> MaintenanceFile {
    {
        let cache = state.maint_cache.read().await;
        if let Some(loaded_at) = cache.loaded_at
            && loaded_at.elapsed() < MAINT_CACHE_TTL
        {
            return cache.value.clone();
        }
    }

    let mut cache = state.maint_cache.write().await;
    if let Some(loaded_at) = cache.loaded_at
        && loaded_at.elapsed() < MAINT_CACHE_TTL
    {
        return cache.value.clone();
    }
    let value = match read_maintenance_from_disk(&state.state_path).await {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(
                error = %error,
                path = %state.state_path.display(),
                "maintenance state unreadable"
            );
            if cache.value.active {
                cache.value.clone()
            } else {
                MaintenanceFile {
                    active: true,
                    ..MaintenanceFile::default()
                }
            }
        }
    };
    cache.loaded_at = Some(Instant::now());
    cache.value = value.clone();
    value
}

async fn read_maintenance_from_disk(path: &PathBuf) -> std::io::Result<MaintenanceFile> {
    match tokio::fs::read(path).await {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(MaintenanceFile::default())
        }
        Err(error) => Err(error),
    }
}

/// Ordered update phases for simple step / percent progress on the maintenance page.
const UPDATE_PHASE_ORDER: &[&str] = &[
    "checking",
    "ready",
    "preflight",
    "maintenance_on",
    "stopping",
    "snapshotting",
    "swap_tag",
    "starting_new",
    "health_probing",
    "swapping_proxy",
    "finalize",
    "cleanup",
];

const ROLLBACK_PHASE_ORDER: &[&str] = &[
    "rollback_in_progress",
    "stop_new",
    "restore_snapshot",
    "swap_tag_back",
    "start_old",
    "health_probing",
    "finalize",
];

fn phase_progress(phase: &str) -> (usize, usize, u32) {
    let order = if phase.contains("rollback")
        || matches!(
            phase,
            "stop_new" | "restore_snapshot" | "swap_tag_back" | "start_old"
        ) {
        ROLLBACK_PHASE_ORDER
    } else {
        UPDATE_PHASE_ORDER
    };
    let total = order.len().max(1);
    let idx = order
        .iter()
        .position(|p| *p == phase)
        .map(|i| i + 1)
        .unwrap_or(0);
    let pct = if idx == 0 {
        0
    } else {
        ((idx * 100) / total) as u32
    };
    (idx, total, pct)
}

fn maintenance_response(m: &MaintenanceFile) -> Response {
    let now = Utc::now();
    let stale = m
        .updated_at
        .map(|t| now.signed_duration_since(t).num_seconds() > 600)
        .unwrap_or(false);
    let very_stale = m
        .updated_at
        .map(|t| now.signed_duration_since(t).num_seconds() > 1800)
        .unwrap_or(false);
    let phase = m.phase.as_deref().unwrap_or("unknown");
    let (step_idx, step_total, step_pct) = phase_progress(phase);
    let body = MAINTENANCE_HTML
        .replace("{{PHASE}}", phase)
        .replace(
            "{{MESSAGE_KEY}}",
            m.message_key.as_deref().unwrap_or("updater.phase.unknown"),
        )
        .replace("{{FROM}}", m.from_version.as_deref().unwrap_or("-"))
        .replace("{{TO}}", m.to_version.as_deref().unwrap_or("-"))
        .replace(
            "{{UPDATED_AT}}",
            &m.updated_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
        )
        .replace("{{STEP_INDEX}}", &step_idx.to_string())
        .replace("{{STEP_TOTAL}}", &step_total.to_string())
        .replace("{{STEP_PCT}}", &step_pct.to_string())
        .replace(
            "{{STALE_CLASS}}",
            if very_stale {
                "very-stale"
            } else if stale {
                "stale"
            } else {
                ""
            },
        );
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert("Retry-After", HeaderValue::from_static("15"));
    ensure_permissions_policy(&mut headers);
    (StatusCode::SERVICE_UNAVAILABLE, headers, Html(body)).into_response()
}

async fn proxy_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let m = read_maintenance_cached(&state).await;
    Json(json!({
        "schema_version": 1,
        "maintenance": {
            "active": m.active,
            "phase": m.phase,
            "from": m.from_version,
            "to": m.to_version,
            "updated_at": m.updated_at,
            "message_key": m.message_key,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintenance_file_ignores_writer_only_started_at() {
        let with_started: MaintenanceFile = serde_json::from_str(
            r#"{"active":true,"phase":"migrating","from_version":"0.5.3","to_version":"0.5.4",
                "started_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:01:00Z",
                "message_key":"update"}"#,
        )
        .unwrap();
        let without_started: MaintenanceFile = serde_json::from_str(
            r#"{"active":true,"phase":"migrating","from_version":"0.5.3","to_version":"0.5.4",
                "updated_at":"2026-01-01T00:01:00Z","message_key":"update"}"#,
        )
        .unwrap();
        for m in [with_started, without_started] {
            assert!(m.active);
            assert_eq!(m.phase.as_deref(), Some("migrating"));
            assert_eq!(m.from_version.as_deref(), Some("0.5.3"));
            assert_eq!(m.to_version.as_deref(), Some("0.5.4"));
            assert_eq!(
                m.updated_at,
                Some("2026-01-01T00:01:00Z".parse::<DateTime<Utc>>().unwrap())
            );
            assert_eq!(m.message_key.as_deref(), Some("update"));
        }
    }

    #[test]
    fn federation_routing_covers_objects_without_stealing_spa_indexes() {
        for path in [
            "/.well-known/webfinger",
            "/inbox",
            "/users/alice/inbox",
            "/api/federation/rooms",
            "/api/admin/federation/domain-move",
            "/api/tapp/federation/feed",
            "/activities/id",
            "/notes/id",
            "/reports/id",
            "/tapps/id",
            "/library/id",
            "/phantasi/articles/id",
        ] {
            assert!(is_federation_path(path), "missing {path}");
        }
        for path in ["/media/federation/file", "/media/assets/id/a.png"] {
            assert!(
                !is_federation_path(path),
                "site media must not go to the federation worker: {path}"
            );
        }
        for path in [
            "/",
            "/reports",
            "/reports/",
            "/library",
            "/library/",
            "/phantasi",
            "/phantasi/item/id",
            "/api/tapps",
            "/api/agent/process",
            "/api/profile/user-info",
        ] {
            assert!(!is_federation_path(path), "stole {path}");
        }
    }

    #[test]
    fn legacy_routing_requires_a_recognized_full_backend() {
        let mut value = json!({"service":"myriad-backend", "mode":"full"});
        assert_eq!(backend_federation_isolated(&value), Some(false));
        value["federation_http_isolated"] = json!(true);
        assert_eq!(backend_federation_isolated(&value), Some(true));
        value["federation_http_isolated"] = json!("false");
        assert_eq!(backend_federation_isolated(&value), None);
        value["mode"] = json!("configuration");
        value["federation_http_isolated"] = json!(false);
        assert_eq!(backend_federation_isolated(&value), None);
        assert_eq!(backend_federation_isolated(&json!({"status":"ok"})), None);
    }

    #[tokio::test]
    async fn saturated_federation_streams_do_not_block_homepage() {
        let web_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let web_address = web_listener.local_addr().unwrap();
        let web = tokio::spawn(async move {
            axum::serve(
                web_listener,
                Router::new().fallback(|| async { "homepage" }),
            )
            .await
            .unwrap();
        });
        // A stalled response with headers is enough to reproduce streams that
        // outlive a header-only concurrency permit.
        let fed_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let fed_address = fed_listener.local_addr().unwrap();
        let federation = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = fed_listener.accept().await.unwrap();
            let mut buf = [0; 4096];
            assert!(stream.read(&mut buf).await.unwrap() > 0);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n")
                .await
                .unwrap();
            std::future::pending::<()>().await;
        });
        let budget = Arc::new(tokio::sync::Semaphore::new(1));
        let state = Arc::new(AppState {
            state_path: PathBuf::from("/__myriad_proxy_test_no_maintenance"),
            backend_upstream: format!("http://{web_address}"),
            frontend_upstream: format!("http://{web_address}"),
            updater_upstream: format!("http://{web_address}"),
            persona_upstream: None,
            persona_isolated: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            persona_requests: Arc::new(tokio::sync::Semaphore::new(32)),
            persona_streams: Arc::new(tokio::sync::Semaphore::new(64)),
            persona_controls: Arc::new(tokio::sync::Semaphore::new(16)),
            federation_upstream: Some(format!("http://{fed_address}")),
            federation_isolated: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            federation_requests: budget.clone(),
            federation_websockets: Arc::new(tokio::sync::Semaphore::new(1)),
            allow_direct_updater: false,
            trusted_upstreams: vec![],
            client: Client::builder(TokioExecutor::new()).build_http(),
            maint_cache: Arc::new(RwLock::new(MaintCache::default())),
        });
        let request = |path| Request::builder().uri(path).body(Body::empty()).unwrap();
        let peer = "127.0.0.1:4444".parse::<SocketAddr>().unwrap();
        let response = handle(State(state.clone()), ConnectInfo(peer), request("/inbox"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(budget.available_permits(), 0);
        let rejected = handle(State(state.clone()), ConnectInfo(peer), request("/inbox"))
            .await
            .unwrap();
        assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
        let home = tokio::time::timeout(
            Duration::from_secs(2),
            handle(State(state), ConnectInfo(peer), request("/")),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(home.status(), StatusCode::OK);
        assert_eq!(
            axum::body::to_bytes(home.into_body(), 1024).await.unwrap(),
            "homepage"
        );
        drop(response);
        assert_eq!(budget.available_permits(), 1);
        web.abort();
        federation.abort();
    }

    #[tokio::test]
    async fn saturated_persona_streams_do_not_block_homepage() {
        let web_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let web_address = web_listener.local_addr().unwrap();
        let web = tokio::spawn(async move {
            axum::serve(
                web_listener,
                Router::new().fallback(|| async { "homepage" }),
            )
            .await
            .unwrap();
        });
        // A stalled response with headers is enough to reproduce streams that
        // outlive a header-only concurrency permit.
        let fed_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let fed_address = fed_listener.local_addr().unwrap();
        let federation = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut stream, _) = fed_listener.accept().await.unwrap();
            let mut buf = [0; 4096];
            assert!(stream.read(&mut buf).await.unwrap() > 0);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n")
                .await
                .unwrap();
            std::future::pending::<()>().await;
        });
        let budget = Arc::new(tokio::sync::Semaphore::new(1));
        let state = Arc::new(AppState {
            state_path: PathBuf::from("/__myriad_proxy_test_no_maintenance"),
            backend_upstream: format!("http://{web_address}"),
            frontend_upstream: format!("http://{web_address}"),
            updater_upstream: format!("http://{web_address}"),
            persona_upstream: Some(format!("http://{fed_address}")),
            persona_isolated: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            persona_requests: Arc::new(tokio::sync::Semaphore::new(32)),
            persona_streams: budget.clone(),
            persona_controls: Arc::new(tokio::sync::Semaphore::new(16)),
            federation_upstream: Some(format!("http://{fed_address}")),
            federation_isolated: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            federation_requests: Arc::new(tokio::sync::Semaphore::new(32)),
            federation_websockets: Arc::new(tokio::sync::Semaphore::new(1)),
            allow_direct_updater: false,
            trusted_upstreams: vec![],
            client: Client::builder(TokioExecutor::new()).build_http(),
            maint_cache: Arc::new(RwLock::new(MaintCache::default())),
        });
        let request = |path| Request::builder().uri(path).body(Body::empty()).unwrap();
        let peer = "127.0.0.1:4444".parse::<SocketAddr>().unwrap();
        let response = handle(
            State(state.clone()),
            ConnectInfo(peer),
            request("/api/agent/notifications/stream"),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(budget.available_permits(), 0);
        let rejected = handle(
            State(state.clone()),
            ConnectInfo(peer),
            request("/api/agent/notifications/stream"),
        )
        .await
        .unwrap();
        assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
        let home = tokio::time::timeout(
            Duration::from_secs(2),
            handle(State(state), ConnectInfo(peer), request("/")),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(home.status(), StatusCode::OK);
        assert_eq!(
            axum::body::to_bytes(home.into_body(), 1024).await.unwrap(),
            "homepage"
        );
        drop(response);
        assert_eq!(budget.available_permits(), 1);
        web.abort();
        federation.abort();
    }

    #[tokio::test]
    async fn federation_websocket_upgrade_keeps_its_budget_until_disconnect() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = upstream.local_addr().unwrap();
        let worker = tokio::spawn(async move {
            let (mut socket, _) = upstream.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.unwrap());
            }
            assert!(
                String::from_utf8_lossy(&request)
                    .to_ascii_lowercase()
                    .contains("upgrade: websocket")
            );
            socket.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n\r\n").await.unwrap();
            let mut frame = [0; 8];
            socket.read_exact(&mut frame).await.unwrap();
            socket.write_all(&frame).await.unwrap();
            let mut byte = [0];
            assert_eq!(socket.read(&mut byte).await.unwrap(), 0);
        });
        let budget = Arc::new(tokio::sync::Semaphore::new(1));
        let state = Arc::new(AppState {
            state_path: PathBuf::from("/__myriad_proxy_test_no_maintenance"),
            backend_upstream: "http://127.0.0.1:1".into(),
            frontend_upstream: "http://127.0.0.1:1".into(),
            updater_upstream: "http://127.0.0.1:1".into(),
            persona_upstream: None,
            persona_isolated: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            persona_requests: Arc::new(tokio::sync::Semaphore::new(32)),
            persona_streams: Arc::new(tokio::sync::Semaphore::new(64)),
            persona_controls: Arc::new(tokio::sync::Semaphore::new(16)),
            federation_upstream: Some(format!("http://{address}")),
            federation_isolated: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            federation_requests: Arc::new(tokio::sync::Semaphore::new(1)),
            federation_websockets: budget.clone(),
            allow_direct_updater: false,
            trusted_upstreams: vec![],
            client: Client::builder(TokioExecutor::new()).build_http(),
            maint_cache: Arc::new(RwLock::new(MaintCache::default())),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_address = listener.local_addr().unwrap();
        let proxy = tokio::spawn(async move {
            let app = Router::new().fallback(any(handle)).with_state(state);
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        let test = async {
            let mut client = tokio::net::TcpStream::connect(proxy_address).await.unwrap();
            client.write_all(b"GET /api/federation/channels/example/ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").await.unwrap();
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                headers.push(client.read_u8().await.unwrap());
            }
            assert!(headers.starts_with(b"HTTP/1.1 101"));
            assert_eq!(budget.available_permits(), 0);
            let frame = [0x81, 0x82, 1, 2, 3, 4, b'h' ^ 1, b'i' ^ 2];
            client.write_all(&frame).await.unwrap();
            let mut echoed = [0; 8];
            client.read_exact(&mut echoed).await.unwrap();
            assert_eq!(echoed, frame);
            drop(client);
            worker.await.unwrap();
            while budget.available_permits() == 0 {
                tokio::task::yield_now().await;
            }
        };
        tokio::time::timeout(Duration::from_secs(5), test)
            .await
            .unwrap();
        proxy.abort();
    }

    #[test]
    fn path_with_query_preserves_query() {
        let uri: Uri = "/api/setup/status?mode=config".parse().unwrap();

        assert_eq!(path_with_query(&uri), "/api/setup/status?mode=config");
    }

    #[test]
    fn backend_paths_include_federation_endpoints() {
        let browser = "Mozilla/5.0 (Macintosh) Chrome/120.0.0.0";
        let googlebot = "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)";

        // REST / health
        assert!(is_backend_path("/api/federation/channels", browser));
        assert!(is_backend_path("/api/federation/media", browser));
        assert!(is_backend_path(
            "/api/federation/avatar-cache/x.webp",
            browser
        ));
        assert!(is_backend_path(
            "/api/federation/channels/ch_x/transfers",
            browser
        ));
        assert!(is_backend_path(
            "/api/federation/rooms/rm_x/transfers",
            browser
        ));
        assert!(is_backend_path(
            "/api/federation/transfers/tr_x/chunks",
            browser
        ));
        assert!(is_backend_path(
            "/api/federation/transfers/tr_x/content",
            browser
        ));
        assert!(is_backend_path("/health", browser));
        assert!(is_backend_path("/ready", browser));
        // Public SEO sitemap + robots
        assert!(is_backend_path("/sitemap.xml", browser));
        assert!(is_backend_path("/robots.txt", browser));
        assert!(is_backend_path("/llms.txt", browser));
        assert!(is_backend_path("/journal/notes.xml", browser));
        assert!(!is_backend_path("/phantasi/notes.xml", browser));
        assert!(is_backend_path("/api/phantasi/notes.xml", browser));
        assert!(is_backend_path("/api/seo/sitemap.xml", browser));
        // Homepage / module indexes / Tapp / Phantasi SEO shells: crawlers only
        assert!(is_backend_path("/", googlebot));
        assert!(is_backend_path("/tapp", googlebot));
        assert!(is_backend_path("/journal", "facebookexternalhit/1.1"));
        assert!(is_backend_path("/journal/notes", googlebot));
        assert!(!is_backend_path("/phantasi", "facebookexternalhit/1.1"));
        assert!(is_backend_path("/library", googlebot));
        assert!(is_backend_path("/reports", googlebot));
        assert!(!is_backend_path("/", browser));
        assert!(!is_backend_path("/tapp", browser));
        assert!(!is_backend_path("/library", browser));
        assert!(!is_backend_path("/tapp/store", googlebot));
        assert!(!is_backend_path("/tapp/playground", googlebot));
        assert!(is_backend_path("/tapp/run/com.example.app", googlebot));
        assert!(is_backend_path(
            "/tapp/run/com.example.app",
            "facebookexternalhit/1.1"
        ));
        assert!(!is_backend_path("/tapp/run/com.example.app", browser));
        assert!(is_backend_path("/journal/articles/42", googlebot));
        assert!(is_backend_path("/journal/feeds", googlebot));
        assert!(!is_backend_path("/journal/feeds/9", googlebot));
        assert!(is_backend_path("/journal/topics/ai", googlebot));
        assert!(!is_backend_path("/journal/feeds/9", browser));
        assert!(!is_backend_path("/journal/workbench/feeds", googlebot));
        assert!(!is_backend_path("/phantasi/item/42", googlebot));
        assert!(!is_backend_path("/phantasi/item/42", "Twitterbot/1.0"));
        assert!(!is_backend_path("/phantasi/item/42", browser));
        // WeChat / QQ share: first document is the SEO shell; `?_spa=1` is SPA.
        assert!(is_backend_path(
            "/",
            "Mozilla/5.0 MicroMessenger/8.0.42 NetType/WIFI"
        ));
        assert!(!is_backend_path_for(
            "/",
            "Mozilla/5.0 MicroMessenger/8.0.42 NetType/WIFI",
            "_spa=1"
        ));
        assert!(!is_backend_path("/phantasi/item/42", "QQ-URL-Preview/1.0"));
        assert!(is_backend_path("/", "Mozilla/5.0 Weibo"));
        // GSC HTML-tag verification crawler (not Googlebot)
        assert!(is_backend_path(
            "/",
            "Mozilla/5.0 (compatible; Google-Site-Verification/1.0)"
        ));
        assert!(!is_backend_path("/", browser));
        // Discovery
        assert!(is_backend_path("/.well-known/webfinger", browser));
        assert!(is_backend_path("/.well-known/nodeinfo", browser));
        assert!(is_backend_path("/nodeinfo/2.1", browser));
        // Inbox + actor graph
        assert!(is_backend_path("/inbox", browser));
        assert!(is_backend_path("/users/misakimei", browser));
        assert!(is_backend_path("/users/misakimei/inbox", browser));
        assert!(is_backend_path("/users/misakimei/outbox", browser));
        assert!(is_backend_path("/users/misakimei/followers", browser));
        assert!(is_backend_path("/users/misakimei/following", browser));
        assert!(is_backend_path("/users/misakimei/avatar", browser));
        // Note attachment media
        assert!(is_backend_path(
            "/media/federation/1/abc-def_01.jpg",
            browser
        ));
        assert!(is_backend_path("/media/federation/42/uuid.mp4", browser));
        assert!(is_backend_path(
            "/media/assets/3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708/a.png",
            browser
        ));

        // Must stay on SPA / ACME / non-backend
        assert!(!is_backend_path("/", browser));
        assert!(!is_backend_path("/users", browser));
        assert!(!is_backend_path("/settings", browser));
        assert!(!is_backend_path("/tapp/com.myriad.aro", browser));
        assert!(!is_backend_path(
            "/.well-known/acme-challenge/token",
            browser
        ));
        assert!(!is_backend_path("/media/federation", browser));
        assert!(!is_backend_path("/media/other/x.jpg", browser));
    }

    #[test]
    fn updater_path_with_query_strips_prefix_and_preserves_query() {
        let uri: Uri = "/_updater/status?detail=1".parse().unwrap();

        assert_eq!(updater_path_with_query(&uri), "/status?detail=1");
    }

    #[test]
    fn permissions_policy_injected_when_missing() {
        let mut headers = HeaderMap::new();
        ensure_permissions_policy(&mut headers);
        assert_eq!(
            headers
                .get("permissions-policy")
                .and_then(|v| v.to_str().ok()),
            Some(PERMISSIONS_POLICY)
        );
    }

    #[test]
    fn permissions_policy_not_overridden_when_present() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("geolocation=()"),
        );
        ensure_permissions_policy(&mut headers);
        assert_eq!(
            headers
                .get("permissions-policy")
                .and_then(|v| v.to_str().ok()),
            Some("geolocation=()")
        );
    }

    #[test]
    fn updater_path_with_query_handles_nested_paths() {
        let uri: Uri = "/_updater/rescue/exit-maintenance?force=true"
            .parse()
            .unwrap();

        assert_eq!(
            updater_path_with_query(&uri),
            "/rescue/exit-maintenance?force=true"
        );
    }

    #[test]
    fn phase_progress_advances_through_update_path() {
        let (idx, total, pct) = phase_progress("preflight");
        assert!(idx > 0);
        assert!(total >= idx);
        assert!(pct > 0 && pct <= 100);

        let (pre_idx, _, _) = phase_progress("preflight");
        let (stop_idx, _, _) = phase_progress("stopping");
        assert!(stop_idx > pre_idx);

        let (cleanup_idx, cleanup_total, cleanup_pct) = phase_progress("cleanup");
        assert_eq!(cleanup_idx, cleanup_total);
        assert_eq!(cleanup_pct, 100);

        let (unknown_idx, _, unknown_pct) = phase_progress("not_a_phase");
        assert_eq!(unknown_idx, 0);
        assert_eq!(unknown_pct, 0);
    }

    #[test]
    fn connection_token_strips_nominated_request_header() {
        let mut headers = HeaderMap::new();
        headers.insert("Connection", HeaderValue::from_static("Foo"));
        headers.insert("Foo", HeaderValue::from_static("secret"));
        headers.insert("x-end-to-end", HeaderValue::from_static("kept"));

        let forwarded = build_forward_request_headers(
            &headers,
            "192.0.2.10".parse().unwrap(),
            &[],
            HeaderTransfer::Http,
        );

        assert!(forwarded.get("connection").is_none());
        assert!(forwarded.get("foo").is_none());
        assert_eq!(
            forwarded.get("x-end-to-end").and_then(|v| v.to_str().ok()),
            Some("kept")
        );
    }

    #[test]
    fn repeated_mixed_case_connection_tokens_are_stripped_for_http_and_ws() {
        let mut headers = HeaderMap::new();
        headers.append("cOnNeCtIoN", HeaderValue::from_static("fOo, Upgrade"));
        headers.append("Connection", HeaderValue::from_static("BAR"));
        headers.insert("Foo", HeaderValue::from_static("foo-secret"));
        headers.insert("bar", HeaderValue::from_static("bar-secret"));
        headers.insert("Upgrade", HeaderValue::from_static("websocket"));

        let tokens = parse_connection_tokens(&headers);
        assert!(tokens.contains("foo"));
        assert!(tokens.contains("upgrade"));
        assert!(tokens.contains("bar"));

        let http = build_forward_request_headers(
            &headers,
            "192.0.2.10".parse().unwrap(),
            &[],
            HeaderTransfer::Http,
        );
        assert!(http.get("connection").is_none());
        assert!(http.get("foo").is_none());
        assert!(http.get("bar").is_none());
        assert!(http.get("upgrade").is_none());

        let ws = build_forward_request_headers(
            &headers,
            "192.0.2.10".parse().unwrap(),
            &[],
            HeaderTransfer::WebSocket,
        );
        assert_eq!(
            ws.get(header::CONNECTION).and_then(|v| v.to_str().ok()),
            Some("Upgrade")
        );
        assert_eq!(
            ws.get(header::UPGRADE).and_then(|v| v.to_str().ok()),
            Some("websocket")
        );
        assert!(ws.get("foo").is_none());
        assert!(ws.get("bar").is_none());
    }

    #[test]
    fn websocket_detection_reads_repeated_connection_fields() {
        let mut headers = HeaderMap::new();
        headers.append("Connection", HeaderValue::from_static("keep-alive"));
        headers.append("connection", HeaderValue::from_static("UpGrAdE"));
        headers.insert("Upgrade", HeaderValue::from_static("WebSocket"));

        assert!(is_websocket_upgrade(&headers));
    }

    #[test]
    fn connection_tokens_strip_nominated_response_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("Connection", HeaderValue::from_static("Foo, keep-alive"));
        headers.insert("Foo", HeaderValue::from_static("secret"));
        headers.insert("Keep-Alive", HeaderValue::from_static("timeout=5"));
        headers.insert("x-end-to-end", HeaderValue::from_static("kept"));

        let filtered = filter_forward_response_headers(&headers, HeaderTransfer::Http);
        assert!(filtered.get("connection").is_none());
        assert!(filtered.get("foo").is_none());
        assert!(filtered.get("keep-alive").is_none());
        assert_eq!(
            filtered.get("x-end-to-end").and_then(|v| v.to_str().ok()),
            Some("kept")
        );
    }

    #[test]
    fn websocket_response_keeps_only_upgrade_fields_from_hop_by_hop_set() {
        let mut headers = HeaderMap::new();
        headers.insert("Connection", HeaderValue::from_static("Foo, Upgrade"));
        headers.insert("Foo", HeaderValue::from_static("secret"));
        headers.insert("Upgrade", HeaderValue::from_static("websocket"));
        headers.insert("Keep-Alive", HeaderValue::from_static("timeout=5"));
        headers.insert(
            "Sec-WebSocket-Accept",
            HeaderValue::from_static("accept-token"),
        );

        let filtered = filter_forward_response_headers(&headers, HeaderTransfer::WebSocket);
        assert_eq!(
            filtered
                .get(header::CONNECTION)
                .and_then(|v| v.to_str().ok()),
            Some("Upgrade")
        );
        assert_eq!(
            filtered.get(header::UPGRADE).and_then(|v| v.to_str().ok()),
            Some("websocket")
        );
        assert!(filtered.get("foo").is_none());
        assert!(filtered.get("keep-alive").is_none());
        assert_eq!(
            filtered
                .get("sec-websocket-accept")
                .and_then(|v| v.to_str().ok()),
            Some("accept-token")
        );
    }

    #[test]
    fn untrusted_peer_rebuilds_all_forwarding_headers_from_connection() {
        let mut headers = HeaderMap::new();
        headers.insert("X-Forwarded-For", HeaderValue::from_static("198.51.100.99"));
        headers.insert("X-Real-IP", HeaderValue::from_static("198.51.100.98"));
        headers.insert("X-Forwarded-Proto", HeaderValue::from_static("https"));
        headers.insert(
            "X-Forwarded-Host",
            HeaderValue::from_static("attacker.example"),
        );
        headers.insert(header::HOST, HeaderValue::from_static("public.example"));

        let forwarded = build_forward_request_headers(
            &headers,
            "192.0.2.10".parse().unwrap(),
            &[],
            HeaderTransfer::Http,
        );
        assert_eq!(
            forwarded
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok()),
            Some("192.0.2.10")
        );
        assert_eq!(
            forwarded.get("x-real-ip").and_then(|v| v.to_str().ok()),
            Some("192.0.2.10")
        );
        assert_eq!(
            forwarded
                .get("x-forwarded-proto")
                .and_then(|v| v.to_str().ok()),
            Some("http")
        );
        assert_eq!(
            forwarded
                .get("x-forwarded-host")
                .and_then(|v| v.to_str().ok()),
            Some("public.example")
        );
        assert_eq!(
            forwarded.get(header::HOST).and_then(|v| v.to_str().ok()),
            Some("public.example")
        );
    }

    #[test]
    fn trusted_proxy_can_supply_forwarded_values_without_changing_activitypub_host() {
        let trusted = parse_trusted_upstreams(Some("10.0.0.5")).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("X-Forwarded-For", HeaderValue::from_static("198.51.100.9"));
        headers.insert("X-Forwarded-Proto", HeaderValue::from_static("https"));
        headers.insert("X-Forwarded-Host", HeaderValue::from_static("edge.example"));
        headers.insert(header::HOST, HeaderValue::from_static("public.example"));

        let forwarded = build_forward_request_headers(
            &headers,
            "10.0.0.5".parse().unwrap(),
            &trusted,
            HeaderTransfer::Http,
        );
        assert_eq!(
            forwarded
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok()),
            Some("198.51.100.9")
        );
        assert_eq!(
            forwarded
                .get("x-forwarded-proto")
                .and_then(|v| v.to_str().ok()),
            Some("https")
        );
        assert_eq!(
            forwarded
                .get("x-forwarded-host")
                .and_then(|v| v.to_str().ok()),
            Some("edge.example")
        );
        // ActivityPub HTTP Signatures cover the inbound public Host. Keep it
        // ahead of X-Forwarded-Host when both are present.
        assert_eq!(
            forwarded.get(header::HOST).and_then(|v| v.to_str().ok()),
            Some("public.example")
        );
    }

    #[test]
    fn direct_client_ignores_forwarded_headers() {
        let trusted = parse_trusted_upstreams(Some("10.0.0.0/8")).unwrap();
        let headers = HeaderMap::new();

        assert_eq!(
            resolve_client_ip(&headers, "192.0.2.10".parse().unwrap(), &trusted),
            "192.0.2.10".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn untrusted_client_cannot_spoof_forwarded_headers() {
        let trusted = parse_trusted_upstreams(Some("10.0.0.0/8")).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.9"));

        assert_eq!(
            resolve_client_ip(&headers, "192.0.2.10".parse().unwrap(), &trusted),
            "192.0.2.10".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn trusted_upstream_forwards_real_client_ip() {
        let trusted = parse_trusted_upstreams(Some("10.0.0.0/8")).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.9"));

        assert_eq!(
            resolve_client_ip(&headers, "10.0.0.5".parse().unwrap(), &trusted),
            "203.0.113.9".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn trusted_proxy_chain_is_removed_from_the_right() {
        let trusted = parse_trusted_upstreams(Some("10.0.0.0/8, 2001:db8:1::/48")).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("198.51.100.8, 2001:db8:1::20, 10.1.2.3"),
        );

        assert_eq!(
            resolve_client_ip(&headers, "10.0.0.5".parse().unwrap(), &trusted),
            "198.51.100.8".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn trusted_proxy_combines_repeated_xff_fields_in_wire_order() {
        let trusted = parse_trusted_upstreams(Some("10.0.0.5")).unwrap();
        let mut headers = HeaderMap::new();
        headers.append("x-forwarded-for", HeaderValue::from_static("198.51.100.8"));
        headers.append("X-Forwarded-For", HeaderValue::from_static("10.0.0.5"));

        assert_eq!(
            resolve_client_ip(&headers, "10.0.0.5".parse().unwrap(), &trusted),
            "198.51.100.8".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn trusted_upstream_can_use_x_real_ip() {
        let trusted = parse_trusted_upstreams(Some("10.0.0.5")).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("203.0.113.9"));

        assert_eq!(
            resolve_client_ip(&headers, "10.0.0.5".parse().unwrap(), &trusted),
            "203.0.113.9".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn malformed_xff_skips_bad_tokens_without_dropping_chain() {
        let trusted = parse_trusted_upstreams(Some("10.0.0.5")).unwrap();
        let mut headers = HeaderMap::new();
        // "spoofed" is not a valid IP; robust parse keeps the valid hop.
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("spoofed, 198.51.100.8"),
        );
        headers.insert("x-real-ip", HeaderValue::from_static("203.0.113.1"));

        assert_eq!(
            resolve_client_ip(&headers, "10.0.0.5".parse().unwrap(), &trusted),
            "198.51.100.8".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn empty_allowlist_does_not_trust_private_peer_xff() {
        // A Docker bridge peer is not trusted unless explicitly listed.
        let trusted = parse_trusted_upstreams(None).unwrap();
        assert!(trusted.is_empty());
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.50"));

        assert_eq!(
            resolve_client_ip(&headers, "172.17.0.1".parse().unwrap(), &trusted),
            "172.17.0.1".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn private_peer_can_be_explicitly_allowlisted_for_development() {
        let trusted = parse_trusted_upstreams(Some("172.17.0.1/32")).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.50"));

        assert_eq!(
            resolve_client_ip(&headers, "172.17.0.1".parse().unwrap(), &trusted),
            "203.0.113.50".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn empty_allowlist_public_peer_cannot_spoof_xff() {
        let trusted = parse_trusted_upstreams(Some("")).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("198.51.100.99"));

        assert_eq!(
            resolve_client_ip(&headers, "192.0.2.10".parse().unwrap(), &trusted),
            "192.0.2.10".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn empty_allowlist_does_not_strip_private_hops_from_right() {
        let trusted = Vec::new();
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.9, 10.0.0.5, 172.17.0.1"),
        );

        assert_eq!(
            resolve_client_ip(&headers, "172.17.0.1".parse().unwrap(), &trusted),
            "172.17.0.1".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn explicit_allowlist_ignores_private_peer_not_listed() {
        // Non-empty list is explicit-only: private peer outside the list is not trusted.
        let trusted = parse_trusted_upstreams(Some("192.0.2.10/32")).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.9"));

        assert_eq!(
            resolve_client_ip(&headers, "10.0.0.5".parse().unwrap(), &trusted),
            "10.0.0.5".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn trusted_peer_accepts_cf_connecting_ip() {
        let trusted = parse_trusted_upstreams(Some("10.0.0.0/8")).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            "cf-connecting-ip",
            HeaderValue::from_static("198.51.100.42"),
        );

        assert_eq!(
            resolve_client_ip(&headers, "10.0.0.5".parse().unwrap(), &trusted),
            "198.51.100.42".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn trusted_peer_accepts_true_client_ip() {
        let trusted = parse_trusted_upstreams(Some("127.0.0.1")).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("true-client-ip", HeaderValue::from_static("198.51.100.77"));

        assert_eq!(
            resolve_client_ip(&headers, "127.0.0.1".parse().unwrap(), &trusted),
            "198.51.100.77".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn untrusted_public_peer_cannot_spoof_cdn_headers() {
        let trusted = Vec::new();
        let mut headers = HeaderMap::new();
        headers.insert(
            "cf-connecting-ip",
            HeaderValue::from_static("198.51.100.42"),
        );

        assert_eq!(
            resolve_client_ip(&headers, "192.0.2.10".parse().unwrap(), &trusted),
            "192.0.2.10".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn invalid_trusted_upstream_fails_configuration() {
        assert!(parse_trusted_upstreams(Some("not-an-address")).is_err());
    }

    #[test]
    fn forwarded_host_falls_back_to_host() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::HOST,
            HeaderValue::from_static("example.myriad.local"),
        );

        assert_eq!(
            forwarded_host(&headers).unwrap(),
            HeaderValue::from_static("example.myriad.local")
        );
    }

    #[test]
    fn upstream_request_host_prefers_host_over_forwarded_host() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("public.example"));
        headers.insert(
            "x-forwarded-host",
            HeaderValue::from_static("forwarded.example"),
        );

        assert_eq!(
            upstream_request_host(&headers).unwrap(),
            HeaderValue::from_static("public.example")
        );
    }

    #[test]
    fn upstream_request_host_falls_back_to_x_forwarded_host() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-host",
            HeaderValue::from_static("public.example"),
        );

        assert_eq!(
            upstream_request_host(&headers).unwrap(),
            HeaderValue::from_static("public.example")
        );
    }

    #[test]
    fn upstream_request_host_none_when_missing() {
        let headers = HeaderMap::new();
        assert!(upstream_request_host(&headers).is_none());
    }
    #[test]
    fn persona_routes_and_legacy_capability_are_explicit() {
        for path in [
            "/api/agent",
            "/api/agent/process/stream",
            "/api/speech/convo/chat/completions",
            "/api/merope/rig/assets/id",
            "/api/tapp/agent/v2/interactions/id/result",
        ] {
            assert!(is_persona_path(path), "missing {path}");
        }
        for path in [
            "/api/agentish",
            "/api/tapp/agent/v2/interactions-other",
            "/api/tapp/events/stream",
            "/api/federation/delivery",
            "/",
        ] {
            assert!(!is_persona_path(path), "stole {path}");
        }
        let mut value = json!({"service":"myriad-backend","mode":"full"});
        assert_eq!(
            backend_isolated(&value, "persona_http_isolated"),
            Some(false)
        );
        value["persona_http_isolated"] = json!(true);
        assert_eq!(
            backend_isolated(&value, "persona_http_isolated"),
            Some(true)
        );
        value["persona_http_isolated"] = json!("false");
        assert_eq!(backend_isolated(&value, "persona_http_isolated"), None);
    }

    #[tokio::test]
    async fn missing_maintenance_file_is_inactive() {
        let path = std::env::temp_dir().join(format!(
            "myriad-proxy-maint-missing-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let got = super::read_maintenance_from_disk(&path).await.unwrap();
        assert!(!got.active);
    }

    #[tokio::test]
    async fn corrupt_maintenance_file_is_not_treated_as_inactive() {
        let path = std::env::temp_dir().join(format!(
            "myriad-proxy-maint-corrupt-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        tokio::fs::write(&path, b"{not-json").await.unwrap();
        let err = super::read_maintenance_from_disk(&path)
            .await
            .expect_err("corrupt file must not look missing");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        let _ = tokio::fs::remove_file(&path).await;
    }
}
