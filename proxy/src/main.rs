//! Myriad maintenance-aware reverse proxy.
//!
//! Behaviour:
//!  - Reads /state/maintenance.json (best-effort; missing/corrupt = inactive).
//!  - When `active=true`, all non-allowlisted requests are served the embedded maintenance page.
//!  - Otherwise, forwards to backend/frontend over plain HTTP via internal docker network DNS.
//!  - Response bodies are **streamed** (no full-buffer collect) to keep memory/TTFB low.
//!  - `/healthz` (proxy itself) always returns 200.
//!  - `/_updater/*` can forward to the updater service when explicitly enabled for rescue.
//!
//! Fail-open: if the state file disappears, requests are forwarded normally. The proxy
//! is the user's only rescue path, so it MUST NOT trap traffic by accident.

use std::collections::HashSet;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::any;
use axum::Router;
use chrono::{DateTime, Utc};
use hyper_util::client::legacy::{connect::HttpConnector, Client};
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
/// frontend `serve.json`: allow first-party geolocation (weather), keep mic/cam off.
/// Applied on proxy responses when upstream omitted it (static frontend often does).
const PERMISSIONS_POLICY: &str = "geolocation=(self), microphone=(), camera=()";

#[derive(Clone)]
struct AppState {
    state_path: PathBuf,
    backend_upstream: String,
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
    // `started_at` is part of the on-disk schema but not surfaced in maintenance HTML.
    // Keep it so deserialization stays forward-compatible.
    #[serde(default)]
    #[allow(dead_code)]
    started_at: Option<DateTime<Utc>>,
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
        frontend_upstream,
        updater_upstream,
        allow_direct_updater,
        trusted_upstreams,
        client,
        maint_cache: Arc::new(RwLock::new(MaintCache::default())),
    };

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
    // SEO: crawler UAs on /tapp/run/* get the backend HTML shell; browsers get SPA.
    let ua = req
        .headers()
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let upstream = if is_backend_path(&path, ua) {
        &state.backend_upstream
    } else {
        &state.frontend_upstream
    };
    if is_websocket_upgrade(req.headers()) {
        return Ok(forward_websocket(
            &state,
            upstream,
            &path_with_query(req.uri()),
            req,
            client_addr,
        )
        .await
        .unwrap_or_else(bad_gateway));
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
    let upstream_resp = state.client.request(upstream_req).await?;

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
        if let Err(err) = tokio::io::copy_bidirectional(&mut client_io, &mut upstream_io).await {
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
        "preview",
    ];
    MARKERS.iter().any(|m| ua.contains(m))
        // Generic bot/spider/crawl (exclude common false positives is hard; keep short)
        || ua.contains("bot/")
        || ua.contains("spider")
        || ua.contains("crawler")
}

/// Paths served by the backend. Everything else goes to the frontend SPA.
///
/// Keep in sync with:
/// - `backend` public federation routes in `main.rs` (non-`/api` ActivityPub + media)
/// - frontend `isBackendDevProxyPath` in `astro.config.mjs`
/// - docs/deployment/PORTS.md
///
/// Missing an entry silently serves the SPA HTML for that URL (broken media, broken
/// WebFinger, etc.). Prefer whole-site outer reverse proxies so this list only needs
/// to live in Myriad proxy.
///
/// `user_agent` is used only for `/tapp/run/*` SEO shell routing (crawlers → backend).
fn is_backend_path(path: &str, user_agent: &str) -> bool {
    if path.starts_with("/api/")
        || path == "/health"
        // Public SEO sitemap + robots (backend api::seo)
        || path == "/sitemap.xml"
        || path == "/robots.txt"
        || path == "/llms.txt"
    {
        return true;
    }
    // Crawler HTML shells (humans stay on SPA)
    if path.starts_with("/tapp/run/") && is_seo_crawler_ua(user_agent) {
        return true;
    }
    // Site-owner original Brew articles only (backend 404s non-own content)
    if path.starts_with("/brew/item/") && is_seo_crawler_ua(user_agent) {
        return true;
    }
    // Federation (ActivityPub/MFP) public endpoints, see backend main.rs.
    path == "/.well-known/webfinger"
        || path == "/.well-known/nodeinfo"
        || path == "/nodeinfo/2.1"
        || path == "/inbox"
        // Actor, outbox, followers, following, per-user inbox, avatar
        || path.starts_with("/users/")
        // Note attachment media (Image/Video) under data/federation_media.
        // Absolute URLs: /media/federation/{userId}/{filename} — must not hit SPA.
        || path.starts_with("/media/federation/")
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
        let normalized = name.to_ascii_lowercase();
        if self.connection_tokens.contains(&normalized) {
            return true;
        }
        if transfer == HeaderTransfer::WebSocket
            && matches!(normalized.as_str(), "connection" | "upgrade")
        {
            // The caller adds the canonical Connection/Upgrade fields below.
            return false;
        }
        is_hop_by_hop(&normalized)
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
            let normalized = name.as_str().to_ascii_lowercase();
            if normalized == "host"
                || is_proxy_managed_forwarded_header(&normalized)
                || self.should_strip(&normalized, transfer)
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
            let normalized = name.as_str().to_ascii_lowercase();
            if transfer == HeaderTransfer::WebSocket
                && matches!(normalized.as_str(), "connection" | "upgrade")
            {
                continue;
            }
            if self.should_strip(&normalized, transfer) {
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
        name.to_ascii_lowercase().as_str(),
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
        name.to_ascii_lowercase().as_str(),
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
        if let Some(loaded_at) = cache.loaded_at {
            if loaded_at.elapsed() < MAINT_CACHE_TTL {
                return cache.value.clone();
            }
        }
    }

    let value = read_maintenance_from_disk(&state.state_path).await;
    let mut cache = state.maint_cache.write().await;
    // Another task may have refreshed while we waited for the write lock; still fine to overwrite.
    cache.loaded_at = Some(Instant::now());
    cache.value = value.clone();
    value
}

async fn read_maintenance_from_disk(path: &PathBuf) -> MaintenanceFile {
    match tokio::fs::read(path).await {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => MaintenanceFile::default(),
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
        // Public SEO sitemap + robots
        assert!(is_backend_path("/sitemap.xml", browser));
        assert!(is_backend_path("/robots.txt", browser));
        assert!(is_backend_path("/llms.txt", browser));
        assert!(is_backend_path("/api/seo/sitemap.xml", browser));
        // Tapp / Brew item SEO shells: crawlers only
        assert!(is_backend_path("/tapp/run/com.example.app", googlebot));
        assert!(is_backend_path(
            "/tapp/run/com.example.app",
            "facebookexternalhit/1.1"
        ));
        assert!(!is_backend_path("/tapp/run/com.example.app", browser));
        assert!(is_backend_path("/brew/item/42", googlebot));
        assert!(is_backend_path("/brew/item/42", "Twitterbot/1.0"));
        assert!(!is_backend_path("/brew/item/42", browser));
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
}
