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

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use once_cell::sync::Lazy;
use reqwest::Client;
use tracing::{error, info, warn};

const MAX_BODY: usize = 64 * 1024;
const GATEWAY_SECRET_MIN_LEN: usize = 32;
const HEADER_GATEWAY_SECRET: &str = "x-updater-gateway-secret";
const HEADER_UPDATE_TOKEN: &str = "x-update-token";
const MAX_FAILED_PER_MIN: u32 = 10;
const BLOCK_DURATION: Duration = Duration::from_secs(600);
const FAILURE_WINDOW: Duration = Duration::from_secs(60);

/// Per-source failed secret attempts: timestamps in the window + optional block-until.
type GatewayLimiterEntry = (Vec<Instant>, Option<Instant>);
/// Failed gateway-secret attempts by socket peer. Correct secrets never blocked.
static GATEWAY_LIMITER: Lazy<Mutex<HashMap<String, GatewayLimiterEntry>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

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

    let token =
        std::env::var("UPDATE_TOKEN").context("UPDATE_TOKEN is required by updater-gateway")?;
    if token.trim().len() < GATEWAY_SECRET_MIN_LEN {
        return Err(anyhow!(
            "UPDATE_TOKEN must be at least {GATEWAY_SECRET_MIN_LEN} characters"
        ));
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

    let app = build_router(Arc::new(state));

    let listener = tokio::net::TcpListener::bind(listen).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

fn build_router(state: Arc<GatewayState>) -> Router {
    // This is the gateway's authority boundary. A route must be added here and
    // to `validate_capability` before the updater token can ever be attached.
    Router::new()
        .route("/healthz", get(local_healthz))
        .route("/status", get(proxy))
        .route("/available", get(proxy))
        .route("/commits", get(proxy))
        .route("/builds", get(proxy))
        .route("/releases", get(proxy))
        .route("/compare", get(proxy))
        .route("/jobs", get(proxy))
        .route("/jobs/{id}", get(proxy))
        .route("/snapshots", get(proxy))
        .route("/snapshots/{id}", delete(proxy))
        .route("/self-update/last", get(proxy))
        .route("/diagnostics", get(proxy))
        .route("/process-logs", get(proxy))
        .route("/update", post(proxy))
        .route("/prefs", post(proxy))
        .route("/last-failed/dismiss", post(proxy))
        .route("/self-update/last/dismiss", post(proxy))
        .route("/rollback", post(proxy))
        .route("/rescue/continue", post(proxy))
        .route("/rescue/exit-maintenance", post(proxy))
        .route("/rescue/forget-current", post(proxy))
        .route("/admin/self-update", post(proxy))
        .fallback(unknown_capability)
        .with_state(state)
}

async fn unknown_capability() -> Response {
    rejection(
        StatusCode::NOT_FOUND,
        "updater capability is not exposed by this gateway",
    )
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

async fn local_healthz() -> impl IntoResponse {
    // Local liveness only — does not prove updater is up. Deep checks use
    // proxied `/status` (token injected) from admin routes. No gateway secret:
    // compose healthchecks hit localhost from the same container.
    (StatusCode::OK, axum::Json(serde_json::json!({"ok": true})))
}

async fn proxy(State(state): State<Arc<GatewayState>>, req: Request<Body>) -> Response {
    let (parts, body) = req.into_parts();
    let (method, uri, headers) = (&parts.method, &parts.uri, &parts.headers);
    let peer = parts
        .extensions
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|axum::extract::ConnectInfo(addr)| *addr);

    // Caller auth: shared secret between backend and gateway (admin-net peers).
    // Wrong secrets are rate-limited per socket peer; correct secrets always pass.
    // X-Forwarded-For is not a source key — this hop has no trusted proxy boundary.
    if let Err(resp) = authorize_gateway_caller(headers, &state.gateway_secret, peer) {
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

    if let Err(error) = validate_capability(method, uri, headers, &body) {
        return error.into_response();
    }

    let target = upstream_url(&state.upstream, uri);
    let mut builder = state
        .http
        .request(method_to_reqwest(method), &target)
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
        // Move the bounded, already-validated Bytes into the upstream request.
        builder = builder.body(body);
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

#[derive(Clone, Copy)]
enum BodySchema {
    None,
    Update,
    Prefs,
    Rollback,
}

struct Capability {
    query_fields: &'static [&'static str],
    required_query_fields: &'static [&'static str],
    body: BodySchema,
    actor_header: bool,
    idempotency_header: bool,
    confirm_risk_header: bool,
}

struct ValidationError {
    status: StatusCode,
    message: Cow<'static, str>,
}

impl IntoResponse for ValidationError {
    fn into_response(self) -> Response {
        (
            self.status,
            axum::Json(serde_json::json!({"message": self.message})),
        )
            .into_response()
    }
}

fn validation_error(status: StatusCode, message: impl Into<Cow<'static, str>>) -> ValidationError {
    ValidationError {
        status,
        message: message.into(),
    }
}

fn validate_capability(
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(), ValidationError> {
    let path = uri.path();
    let capability = match (method, path) {
        (&Method::GET, "/status")
        | (&Method::GET, "/jobs")
        | (&Method::GET, "/snapshots")
        | (&Method::GET, "/self-update/last") => read_capability(&[], &[]),
        (&Method::GET, "/diagnostics" | "/process-logs") => Capability {
            actor_header: false,
            ..read_capability(&[], &[])
        },
        (&Method::GET, "/available") => read_capability(&["channel", "mode"], &[]),
        (&Method::GET, "/commits") => read_capability(&["branch", "limit"], &[]),
        (&Method::GET, "/builds") => read_capability(&["limit"], &[]),
        (&Method::GET, "/releases") => read_capability(&["channel", "limit"], &[]),
        (&Method::GET, "/compare") => read_capability(&["from", "to"], &["to"]),
        (&Method::GET, p) if valid_dynamic_id(p, "/jobs/") => read_capability(&[], &[]),
        (&Method::DELETE, p) if valid_dynamic_id(p, "/snapshots/") => Capability {
            actor_header: true,
            ..read_capability(&[], &[])
        },
        (&Method::POST, "/update") => Capability {
            query_fields: &[],
            required_query_fields: &[],
            body: BodySchema::Update,
            actor_header: true,
            idempotency_header: true,
            confirm_risk_header: true,
        },
        (&Method::POST, "/prefs") => Capability {
            body: BodySchema::Prefs,
            ..write_capability(false)
        },
        (&Method::POST, "/last-failed/dismiss")
        | (&Method::POST, "/self-update/last/dismiss") => write_capability(false),
        (&Method::POST, "/rollback") => Capability {
            body: BodySchema::Rollback,
            ..write_capability(true)
        },
        (&Method::POST, "/rescue/continue")
        | (&Method::POST, "/rescue/exit-maintenance")
        | (&Method::POST, "/rescue/forget-current")
        | (&Method::POST, "/admin/self-update") => write_capability(true),
        // Router method/path filters should make this unreachable. Keeping the
        // second fence here prevents a future route edit from silently growing
        // token authority.
        _ => {
            return Err(validation_error(
                StatusCode::NOT_FOUND,
                "updater capability is not exposed by this gateway",
            ));
        }
    };

    validate_query(
        uri,
        capability.query_fields,
        capability.required_query_fields,
    )?;
    validate_headers(headers, &capability)?;
    validate_body(headers, body, capability.body)
}

fn read_capability(
    query_fields: &'static [&'static str],
    required_query_fields: &'static [&'static str],
) -> Capability {
    Capability {
        query_fields,
        required_query_fields,
        body: BodySchema::None,
        actor_header: false,
        idempotency_header: false,
        confirm_risk_header: false,
    }
}

fn write_capability(actor_header: bool) -> Capability {
    Capability {
        query_fields: &[],
        required_query_fields: &[],
        body: BodySchema::None,
        actor_header,
        idempotency_header: false,
        confirm_risk_header: false,
    }
}

fn valid_dynamic_id(path: &str, prefix: &str) -> bool {
    path.strip_prefix(prefix).is_some_and(|id| {
        !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    })
}

fn validate_query(uri: &Uri, allowed: &[&str], required: &[&str]) -> Result<(), ValidationError> {
    let Some(raw) = uri.query() else {
        if required.is_empty() {
            return Ok(());
        }
        return Err(validation_error(
            StatusCode::BAD_REQUEST,
            "required updater query field is missing",
        ));
    };
    if raw.len() > 2_048 || !valid_percent_encoding(raw) {
        return Err(validation_error(
            StatusCode::BAD_REQUEST,
            "invalid updater query encoding",
        ));
    }

    let allowed: HashSet<&str> = allowed.iter().copied().collect();
    let mut seen = HashSet::new();
    for (key, value) in url::form_urlencoded::parse(raw.as_bytes()) {
        let key = key.as_ref();
        if !allowed.contains(key) || !seen.insert(key.to_string()) {
            return Err(validation_error(
                StatusCode::BAD_REQUEST,
                "unknown or duplicate updater query field",
            ));
        }
        validate_query_value(key, value.as_ref())?;
    }
    if required.iter().any(|field| !seen.contains(*field)) {
        return Err(validation_error(
            StatusCode::BAD_REQUEST,
            "required updater query field is missing",
        ));
    }
    Ok(())
}

fn validate_query_value(key: &str, value: &str) -> Result<(), ValidationError> {
    if value.len() > 256 || value.chars().any(char::is_control) {
        return Err(validation_error(
            StatusCode::BAD_REQUEST,
            "invalid updater query value",
        ));
    }
    match key {
        "channel" if !matches!(value, "stable" | "preview") => Err(validation_error(
            StatusCode::BAD_REQUEST,
            "channel must be stable or preview",
        )),
        "mode" if !matches!(value, "release" | "commit") => Err(validation_error(
            StatusCode::BAD_REQUEST,
            "mode must be release or commit",
        )),
        "limit" => match value.parse::<u32>() {
            Ok(1..=100) => Ok(()),
            _ => Err(validation_error(
                StatusCode::BAD_REQUEST,
                "limit must be an integer from 1 to 100",
            )),
        },
        "to" if value.trim().is_empty() => Err(validation_error(
            StatusCode::BAD_REQUEST,
            "compare target cannot be empty",
        )),
        _ => Ok(()),
    }
}

fn valid_percent_encoding(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
}

fn validate_headers(headers: &HeaderMap, capability: &Capability) -> Result<(), ValidationError> {
    const TRANSPORT_HEADERS: &[&str] = &[
        "host",
        "accept",
        "accept-encoding",
        "content-type",
        "content-length",
        "transfer-encoding",
        "connection",
        "user-agent",
        "x-forwarded-for",
    ];

    for name in headers.keys() {
        let name = name.as_str();
        let allowed = TRANSPORT_HEADERS.contains(&name)
            || name == HEADER_GATEWAY_SECRET
            || (name == "x-update-actor" && capability.actor_header)
            || (name == "idempotency-key" && capability.idempotency_header)
            || (name == "x-myriad-confirm-risk" && capability.confirm_risk_header);
        if !allowed || name == HEADER_UPDATE_TOKEN {
            return Err(validation_error(
                StatusCode::BAD_REQUEST,
                "request header is not allowed for this updater capability",
            ));
        }
    }
    Ok(())
}

fn validate_body(
    headers: &HeaderMap,
    body: &[u8],
    schema: BodySchema,
) -> Result<(), ValidationError> {
    if matches!(schema, BodySchema::None) {
        if body.is_empty() {
            return Ok(());
        }
        return Err(validation_error(
            StatusCode::BAD_REQUEST,
            "this updater capability does not accept a request body",
        ));
    }

    let is_json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
        });
    if !is_json {
        return Err(validation_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "updater capability requires application/json",
        ));
    }

    let value: serde_json::Value = serde_json::from_slice(body).map_err(|_| {
        validation_error(
            StatusCode::BAD_REQUEST,
            "updater request body must be valid JSON",
        )
    })?;
    let object = value.as_object().ok_or_else(|| {
        validation_error(
            StatusCode::BAD_REQUEST,
            "updater request body must be a JSON object",
        )
    })?;

    match schema {
        BodySchema::None => unreachable!(),
        BodySchema::Update => validate_update_body(object),
        BodySchema::Prefs => validate_prefs_body(object),
        BodySchema::Rollback => validate_rollback_body(object),
    }
}

fn deny_unknown_fields(
    object: &serde_json::Map<String, serde_json::Value>,
    allowed: &[&str],
) -> Result<(), ValidationError> {
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        Err(validation_error(
            StatusCode::BAD_REQUEST,
            "updater request contains an unknown field",
        ))
    } else {
        Ok(())
    }
}

fn validate_update_body(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), ValidationError> {
    const STRINGS: &[&str] = &["target_version", "target_commit", "mode"];
    const BOOLEANS: &[&str] = &[
        "allow_downgrade",
        "allow_risk",
        "allow_diverged",
        "allow_unknown",
        "allow_irreversible",
        "allow_compose_override",
        "allow_skip_versions",
        "confirm_risk",
    ];
    let mut allowed = STRINGS.to_vec();
    allowed.extend_from_slice(BOOLEANS);
    deny_unknown_fields(object, &allowed)?;
    for key in STRINGS {
        if let Some(value) = object.get(*key) {
            validate_short_string(value, key)?;
        }
    }
    if let Some(mode) = object.get("mode").and_then(|value| value.as_str())
        && !matches!(mode, "release" | "commit")
    {
        return Err(validation_error(
            StatusCode::BAD_REQUEST,
            "mode must be release or commit",
        ));
    }
    for key in BOOLEANS {
        if object.get(*key).is_some_and(|value| !value.is_boolean()) {
            return Err(validation_error(
                StatusCode::BAD_REQUEST,
                "updater boolean field has the wrong type",
            ));
        }
    }
    Ok(())
}

fn validate_prefs_body(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), ValidationError> {
    const ALLOWED: &[&str] = &[
        "channel",
        "mode",
        "check_interval_secs",
        "auto_install",
        "snapshot_limit_enabled",
        "snapshot_limit",
    ];
    deny_unknown_fields(object, ALLOWED)?;
    if object.is_empty() {
        return Err(validation_error(
            StatusCode::BAD_REQUEST,
            "prefs request must change at least one field",
        ));
    }
    if let Some(value) = object.get("channel") {
        let channel = value
            .as_str()
            .ok_or_else(|| validation_error(StatusCode::BAD_REQUEST, "channel must be a string"))?;
        if !matches!(channel, "stable" | "preview") {
            return Err(validation_error(
                StatusCode::BAD_REQUEST,
                "channel must be stable or preview",
            ));
        }
    }
    if let Some(value) = object.get("mode") {
        let mode = value
            .as_str()
            .ok_or_else(|| validation_error(StatusCode::BAD_REQUEST, "mode must be a string"))?;
        if !matches!(mode, "release" | "commit") {
            return Err(validation_error(
                StatusCode::BAD_REQUEST,
                "mode must be release or commit",
            ));
        }
    }
    if let Some(value) = object.get("check_interval_secs") {
        let valid =
            value.is_null() || matches!(value.as_u64(), Some(0 | 3_600 | 21_600 | 43_200 | 86_400));
        if !valid {
            return Err(validation_error(
                StatusCode::BAD_REQUEST,
                "check_interval_secs is not an allowed interval",
            ));
        }
    }
    for key in ["auto_install", "snapshot_limit_enabled"] {
        if object.get(key).is_some_and(|value| !value.is_boolean()) {
            return Err(validation_error(
                StatusCode::BAD_REQUEST,
                "updater preference boolean has the wrong type",
            ));
        }
    }
    if object
        .get("snapshot_limit")
        .is_some_and(|value| !matches!(value.as_u64(), Some(1..=20)))
    {
        return Err(validation_error(
            StatusCode::BAD_REQUEST,
            "snapshot_limit must be an integer from 1 to 20",
        ));
    }
    Ok(())
}

fn validate_rollback_body(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), ValidationError> {
    deny_unknown_fields(object, &["snapshot_id"])?;
    let id = object
        .get("snapshot_id")
        .and_then(|value| value.as_str())
        .ok_or_else(|| validation_error(StatusCode::BAD_REQUEST, "snapshot_id is required"))?;
    if valid_id(id) {
        Ok(())
    } else {
        Err(validation_error(
            StatusCode::BAD_REQUEST,
            "snapshot_id has an invalid shape",
        ))
    }
}

fn validate_short_string(value: &serde_json::Value, field: &str) -> Result<(), ValidationError> {
    let Some(value) = value.as_str() else {
        return Err(validation_error(
            StatusCode::BAD_REQUEST,
            "updater string field has the wrong type",
        ));
    };
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(validation_error(
            StatusCode::BAD_REQUEST,
            format!("{field} has an invalid shape"),
        ));
    }
    Ok(())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

fn rejection(status: StatusCode, message: &str) -> Response {
    (status, axum::Json(serde_json::json!({"message": message}))).into_response()
}

/// Validate `X-Updater-Gateway-Secret` with a length-checked constant-time compare.
///
/// `Err` 携带的是一个完整的 `Response`（较大）。这条路径每个请求最多走一次、
/// 且失败即返回，装箱换来的间接寻址不值得。
#[allow(clippy::result_large_err)]
fn authorize_gateway_caller(
    headers: &HeaderMap,
    expected: &str,
    peer: Option<SocketAddr>,
) -> Result<(), Response> {
    let source = gateway_source_key(peer);
    let provided = headers
        .get(HEADER_GATEWAY_SECRET)
        .and_then(|v| v.to_str().ok());
    match provided {
        Some(got) if constant_time_eq(got.as_bytes(), expected.as_bytes()) => {
            gateway_clear_failures(&source);
            Ok(())
        }
        Some(_) => Err(gateway_reject_invalid_secret(&source, "invalid gateway secret")),
        None => Err(gateway_reject_invalid_secret(
            &source,
            "missing X-Updater-Gateway-Secret",
        )),
    }
}

fn gateway_reject_invalid_secret(source: &str, unauthorized_message: &str) -> Response {
    if gateway_is_blocked(source) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            axum::Json(serde_json::json!({
                "message": "too many invalid gateway secret attempts; try again later"
            })),
        )
            .into_response();
    }
    let blocked = gateway_record_failure(source);
    if blocked {
        (
            StatusCode::TOO_MANY_REQUESTS,
            axum::Json(serde_json::json!({
                "message": "too many invalid gateway secret attempts; try again later"
            })),
        )
            .into_response()
    } else {
        (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "message": unauthorized_message })),
        )
            .into_response()
    }
}

fn gateway_source_key(peer: Option<SocketAddr>) -> String {
    match peer {
        Some(addr) => format!("ip:{}", addr.ip()),
        None => "ip:unknown".into(),
    }
}

const MAX_GATEWAY_LIMITER_KEYS: usize = 1024;

fn gateway_record_failure(key: &str) -> bool {
    let mut map = GATEWAY_LIMITER.lock().unwrap();
    let now = Instant::now();
    gateway_gc_limiter(&mut map, now);
    let entry = map.entry(key.to_string()).or_default();
    entry.0.retain(|t| now.duration_since(*t) < FAILURE_WINDOW);
    entry.0.push(now);
    if entry.0.len() as u32 > MAX_FAILED_PER_MIN {
        entry.1 = Some(now + BLOCK_DURATION);
        true
    } else {
        false
    }
}

fn gateway_gc_limiter(map: &mut HashMap<String, GatewayLimiterEntry>, now: Instant) {
    map.retain(|_, (times, until)| {
        if until.is_some_and(|block_until| now < block_until) {
            return true;
        }
        times.iter().any(|t| now.duration_since(*t) < FAILURE_WINDOW)
    });
    if map.len() > MAX_GATEWAY_LIMITER_KEYS {
        let overflow = map.len() - MAX_GATEWAY_LIMITER_KEYS;
        let drop_keys: Vec<String> = map
            .iter()
            .filter(|(_, (_, until))| until.is_none_or(|block_until| now >= block_until))
            .take(overflow)
            .map(|(k, _)| k.clone())
            .collect();
        for key in drop_keys {
            map.remove(&key);
        }
    }
}

fn gateway_is_blocked(key: &str) -> bool {
    let mut map = GATEWAY_LIMITER.lock().unwrap();
    let now = Instant::now();
    if let Some(entry) = map.get_mut(key)
        && let Some(until) = entry.1
    {
        if now < until {
            return true;
        }
        entry.1 = None;
        entry.0.clear();
    }
    false
}

fn gateway_clear_failures(key: &str) {
    let mut map = GATEWAY_LIMITER.lock().unwrap();
    map.remove(key);
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
    reqwest::Method::from_bytes(method.as_str().as_bytes())
        .expect("Axum accepted a standard HTTP method")
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
    use tower::ServiceExt;

    const TEST_SECRET: &str = "abcdefghijklmnopqrstuvwxyz012345";

    fn test_router() -> Router {
        build_router(Arc::new(GatewayState {
            upstream: "http://127.0.0.1:9".into(),
            token: TEST_SECRET.into(),
            gateway_secret: TEST_SECRET.into(),
            http: Client::builder()
                .connect_timeout(Duration::from_millis(50))
                .build()
                .unwrap(),
        }))
    }

    fn authorized_request(method: Method, uri: &str, body: Body) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(HEADER_GATEWAY_SECRET, TEST_SECRET)
            .body(body)
            .unwrap()
    }

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
        assert!(constant_time_eq(
            b"same-secret-value-32chars-ok!!",
            b"same-secret-value-32chars-ok!!"
        ));
    }

    #[test]
    fn constant_time_eq_rejects_mismatch_and_len() {
        assert!(!constant_time_eq(
            b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        ));
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
        let peer = "203.0.113.10:1".parse().ok();
        assert!(authorize_gateway_caller(&headers, secret, peer).is_ok());
    }

    #[test]
    fn authorize_rejects_missing_secret() {
        let headers = HeaderMap::new();
        let peer = "203.0.113.11:1".parse().ok();
        let err = authorize_gateway_caller(&headers, "abcdefghijklmnopqrstuvwxyz012345", peer)
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn authorize_rejects_wrong_secret() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HEADER_GATEWAY_SECRET,
            HeaderValue::from_static("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"),
        );
        let peer = "203.0.113.12:1".parse().ok();
        let err = authorize_gateway_caller(&headers, "abcdefghijklmnopqrstuvwxyz012345", peer)
            .unwrap_err();
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn gateway_source_key_is_socket_peer_not_xff() {
        let peer: SocketAddr = "10.0.0.8:5555".parse().unwrap();
        assert_eq!(gateway_source_key(Some(peer)), "ip:10.0.0.8");
        assert_eq!(gateway_source_key(None), "ip:unknown");
        assert_ne!(
            gateway_source_key(Some("10.0.0.8:1".parse().unwrap())),
            gateway_source_key(Some("10.0.0.9:1".parse().unwrap()))
        );
    }

    #[test]
    fn forged_xff_cannot_split_or_share_limiter_buckets() {
        let secret = "abcdefghijklmnopqrstuvwxyz012345";
        let peer_a: SocketAddr = "198.51.100.1:1".parse().unwrap();
        let peer_b: SocketAddr = "198.51.100.2:1".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            HEADER_GATEWAY_SECRET,
            HeaderValue::from_static("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"),
        );
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.9"));

        let mut blocked = false;
        for _ in 0..=MAX_FAILED_PER_MIN {
            let err = authorize_gateway_caller(&headers, secret, Some(peer_a)).unwrap_err();
            if err.status() == StatusCode::TOO_MANY_REQUESTS {
                blocked = true;
            }
        }
        assert!(blocked, "peer A must be throttled after repeated failures");

        // Same forged XFF, different socket peer — must not inherit the block.
        let err = authorize_gateway_caller(&headers, secret, Some(peer_b)).unwrap_err();
        assert_eq!(
            err.status(),
            StatusCode::UNAUTHORIZED,
            "XFF must not be the limiter key"
        );
    }

    #[test]
    fn correct_secret_passes_even_when_peer_is_blocked() {
        let secret = "abcdefghijklmnopqrstuvwxyz012345";
        let peer: SocketAddr = "198.51.100.40:1".parse().unwrap();
        let mut bad = HeaderMap::new();
        bad.insert(
            HEADER_GATEWAY_SECRET,
            HeaderValue::from_static("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"),
        );
        let mut blocked = false;
        for _ in 0..=MAX_FAILED_PER_MIN {
            let err = authorize_gateway_caller(&bad, secret, Some(peer)).unwrap_err();
            if err.status() == StatusCode::TOO_MANY_REQUESTS {
                blocked = true;
            }
        }
        assert!(blocked);
        let mut good = HeaderMap::new();
        good.insert(
            HEADER_GATEWAY_SECRET,
            HeaderValue::from_static("abcdefghijklmnopqrstuvwxyz012345"),
        );
        assert!(
            authorize_gateway_caller(&good, secret, Some(peer)).is_ok(),
            "a correct secret must not inherit a block from prior failures"
        );
        let err = authorize_gateway_caller(&bad, secret, Some(peer)).unwrap_err();
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn router_rejects_unknown_path_and_method_without_proxying() {
        let unknown = test_router()
            .oneshot(
                Request::builder()
                    .uri("/arbitrary/admin/action")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

        let wrong_method = test_router()
            .oneshot(authorized_request(Method::PUT, "/status", Body::empty()))
            .await
            .unwrap();
        assert_eq!(wrong_method.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn router_rejects_encoded_path_traversal() {
        for path in [
            "/jobs/%2e%2e",
            "/jobs/%2E%2E%2Fstatus",
            "/snapshots/%2e%2e%2fsecret",
        ] {
            let method = if path.starts_with("/snapshots/") {
                Method::DELETE
            } else {
                Method::GET
            };
            let response = test_router()
                .oneshot(authorized_request(method, path, Body::empty()))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "path={path}");
        }
    }

    #[tokio::test]
    async fn router_rejects_unknown_query_header_and_json_field() {
        let query = test_router()
            .oneshot(authorized_request(
                Method::GET,
                "/status?full=1",
                Body::empty(),
            ))
            .await
            .unwrap();
        assert_eq!(query.status(), StatusCode::BAD_REQUEST);

        let mut header = authorized_request(Method::GET, "/status", Body::empty());
        header
            .headers_mut()
            .insert("x-unexpected", HeaderValue::from_static("1"));
        let header = test_router().oneshot(header).await.unwrap();
        assert_eq!(header.status(), StatusCode::BAD_REQUEST);

        let mut body = authorized_request(
            Method::POST,
            "/update",
            Body::from(r#"{"command":"shell"}"#),
        );
        body.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        let body = test_router().oneshot(body).await.unwrap();
        assert_eq!(body.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn capability_validation_rejects_wrong_body_shapes() {
        let mut headers = HeaderMap::new();
        headers.insert(HEADER_GATEWAY_SECRET, HeaderValue::from_static(TEST_SECRET));
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );

        let uri: Uri = "/rollback".parse().unwrap();
        assert!(
            validate_capability(
                &Method::POST,
                &uri,
                &headers,
                br#"{"snapshot_id":"../outside"}"#,
            )
            .is_err()
        );

        let uri: Uri = "/prefs".parse().unwrap();
        assert!(
            validate_capability(&Method::POST, &uri, &headers, br#"{"snapshot_limit":21}"#,)
                .is_err()
        );

        let uri: Uri = "/last-failed/dismiss".parse().unwrap();
        assert!(validate_capability(&Method::POST, &uri, &headers, b"").is_ok());
        assert!(validate_capability(&Method::POST, &uri, &headers, br#"{}"#).is_err());
        let uri: Uri = "/self-update/last/dismiss".parse().unwrap();
        assert!(validate_capability(&Method::POST, &uri, &headers, b"").is_ok());
        assert!(validate_capability(&Method::POST, &uri, &headers, br#"{}"#).is_err());
    }

    #[test]
    fn process_logs_is_a_get_only_parameterless_capability() {
        let mut headers = HeaderMap::new();
        headers.insert(HEADER_GATEWAY_SECRET, HeaderValue::from_static(TEST_SECRET));
        let uri: Uri = "/process-logs".parse().unwrap();
        assert!(validate_capability(&Method::GET, &uri, &headers, &[]).is_ok());
        assert!(validate_capability(&Method::POST, &uri, &headers, &[]).is_err());

        let uri: Uri = "/process-logs?all=true".parse().unwrap();
        assert!(validate_capability(&Method::GET, &uri, &headers, &[]).is_err());
    }
}
