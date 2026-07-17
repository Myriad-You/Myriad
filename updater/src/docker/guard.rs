//! Policy-enforcing proxy between the updater and the host Docker daemon.
//!
//! This is intentionally narrower than a generic socket proxy: mutations are limited to
//! containers in one Compose project, and container-create request bodies are validated before
//! reaching the daemon. The updater never receives the raw Unix socket.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, State};
use axum::http::{header, Method, Request, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use bytes::Bytes;
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use serde_json::{json, Value};
use tokio::net::UnixStream;
use tokio::process::Command;
use tracing::{error, info, warn};

const MAX_REQUEST_BODY: usize = 1024 * 1024;
const MAX_INSPECT_BODY: usize = 2 * 1024 * 1024;
const SELF_UPDATE_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct GuardConfig {
    pub listen: SocketAddr,
    pub socket_path: PathBuf,
    pub project: String,
    pub compose_network: String,
    /// Admin plane (backend / updater / updater-gateway / proxy rescue). Not internal.
    pub admin_network: String,
    pub guard_network: String,
    pub compose_dir: PathBuf,
    pub env_file: PathBuf,
    pub update_token: String,
    pub allowed_images: HashSet<String>,
}

impl GuardConfig {
    pub fn load_from_env() -> Result<Self> {
        let listen = std::env::var("DOCKER_GUARD_LISTEN")
            .unwrap_or_else(|_| "0.0.0.0:2375".into())
            .parse()
            .context("invalid DOCKER_GUARD_LISTEN")?;
        let socket_path = std::env::var("DOCKER_GUARD_SOCKET")
            .unwrap_or_else(|_| "/var/run/docker.sock".into())
            .into();
        let project = std::env::var("COMPOSE_PROJECT_NAME").unwrap_or_else(|_| "myriad".into());
        validate_simple_name("COMPOSE_PROJECT_NAME", &project)?;
        let compose_network =
            std::env::var("MYRIAD_DOCKER_NETWORK").unwrap_or_else(|_| "myriad-net".into());
        validate_simple_name("MYRIAD_DOCKER_NETWORK", &compose_network)?;
        let admin_network =
            std::env::var("MYRIAD_ADMIN_NETWORK").unwrap_or_else(|_| "myriad-admin-net".into());
        validate_simple_name("MYRIAD_ADMIN_NETWORK", &admin_network)?;
        let guard_network = std::env::var("MYRIAD_DOCKER_GUARD_NETWORK")
            .unwrap_or_else(|_| "myriad-docker-guard-net".into());
        validate_simple_name("MYRIAD_DOCKER_GUARD_NETWORK", &guard_network)?;
        let compose_dir = std::env::var("DOCKER_GUARD_COMPOSE_DIR")
            .unwrap_or_else(|_| "/host/compose".into())
            .into();
        let env_file = std::env::var("DOCKER_GUARD_ENV_FILE")
            .unwrap_or_else(|_| "/host/compose/.env".into())
            .into();
        let update_token =
            std::env::var("UPDATE_TOKEN").context("UPDATE_TOKEN is required by docker guard")?;
        if update_token.trim().len() < 32 {
            return Err(anyhow!("UPDATE_TOKEN must be at least 32 characters"));
        }

        let configured = std::env::var("DOCKER_GUARD_ALLOWED_IMAGES").unwrap_or_else(|_| {
            [
                "docker.io/somekawahitomi/myriad-backend",
                "docker.io/somekawahitomi/myriad-frontend",
                "docker.io/somekawahitomi/myriad-updater",
                "postgres",
            ]
            .join(",")
        });
        let allowed_images = configured
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(normalize_repository)
            .collect::<HashSet<_>>();
        if allowed_images.is_empty() {
            return Err(anyhow!("DOCKER_GUARD_ALLOWED_IMAGES cannot be empty"));
        }

        Ok(Self {
            listen,
            socket_path,
            project,
            compose_network,
            admin_network,
            guard_network,
            compose_dir,
            env_file,
            update_token,
            allowed_images,
        })
    }
}

#[derive(Clone)]
struct GuardState {
    config: Arc<GuardConfig>,
    host_compose_root: Arc<PathBuf>,
    self_update_running: Arc<AtomicBool>,
}

pub async fn run(config: GuardConfig) -> Result<()> {
    let host_compose_root = match std::env::var("DOCKER_GUARD_HOST_COMPOSE_ROOT") {
        Ok(root) if !root.trim().is_empty() => PathBuf::from(root),
        _ => {
            let hostname = current_container_id()?;
            discover_host_compose_root(&config.socket_path, &hostname).await?
        }
    };
    let listen = config.listen;
    let state = GuardState {
        config: Arc::new(config),
        host_compose_root: Arc::new(host_compose_root.clone()),
        self_update_running: Arc::new(AtomicBool::new(false)),
    };

    info!(
        addr = %listen,
        project = %state.config.project,
        host_compose_root = %host_compose_root.display(),
        "docker guard listening"
    );
    let app = Router::new().fallback(any(handle)).with_state(state);
    let listener = tokio::net::TcpListener::bind(listen).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

async fn handle(
    State(state): State<GuardState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request<Body>,
) -> Response {
    if req.uri().path() == "/_myriad/self-update" {
        return handle_self_update(state, req).await;
    }

    let method = req.method().clone();
    let uri = req.uri().clone();
    let (parts, body) = req.into_parts();
    let body = match to_bytes(body, MAX_REQUEST_BODY).await {
        Ok(body) => body,
        Err(_) => return denial(StatusCode::PAYLOAD_TOO_LARGE, "request body exceeds 1 MiB"),
    };

    let decision = match classify_request(&state, &method, &uri, &body) {
        Ok(decision) => decision,
        Err(reason) => {
            warn!(%peer, %method, path = %uri.path(), %reason, "docker guard denied request");
            return denial(StatusCode::FORBIDDEN, &reason);
        }
    };
    match decision {
        Decision::Allow => {}
        Decision::ProjectContainer(container) => {
            match container_belongs_to_project(&state, &container).await {
                Ok(true) => {}
                Ok(false) => {
                    return denial(
                        StatusCode::FORBIDDEN,
                        "container is not a managed service in this Compose project",
                    )
                }
                Err(e) => {
                    warn!(container, err = %e, "docker guard could not authorize container");
                    return denial(StatusCode::FORBIDDEN, "container authorization failed");
                }
            }
        }
        Decision::ProjectNetworkMutation { network, container } => {
            if let Err(reason) =
                authorize_network_mutation(&state, &network, &container).await
            {
                warn!(
                    %peer,
                    %method,
                    path = %uri.path(),
                    network,
                    container,
                    %reason,
                    "docker guard denied network mutation"
                );
                return denial(StatusCode::FORBIDDEN, &reason);
            }
        }
    }

    let req = Request::from_parts(parts, Body::from(body));
    match forward(&state.config.socket_path, req).await {
        Ok(resp) => resp,
        Err(e) => {
            error!(err = %e, "docker guard upstream failure");
            denial(StatusCode::BAD_GATEWAY, "docker daemon unavailable")
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum Decision {
    Allow,
    ProjectContainer(String),
    ProjectNetworkMutation { network: String, container: String },
}

fn classify_request(
    state: &GuardState,
    method: &Method,
    uri: &Uri,
    body: &Bytes,
) -> std::result::Result<Decision, String> {
    let path = strip_api_version(uri.path());
    let segments = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    if matches!((method, path), (&Method::GET | &Method::HEAD, "/_ping"))
        || (*method == Method::GET && matches!(path, "/version" | "/info" | "/events"))
    {
        return Ok(Decision::Allow);
    }
    // Docker accepts slashes inside the image-name path parameter, and Bollard sends them
    // unescaped (`/images/docker.io/org/image:tag/tag`). Match the bounded prefix/suffix and
    // still enforce the source and target repository allowlists below.
    if *method == Method::POST {
        if let Some(source) = path
            .strip_prefix("/images/")
            .and_then(|value| value.strip_suffix("/tag"))
            .filter(|value| !value.is_empty())
        {
            validate_image_tag(state, source, uri)?;
            return Ok(Decision::Allow);
        }
    }

    match segments.as_slice() {
        ["containers", "json"] if *method == Method::GET => Ok(Decision::Allow),
        ["containers", "create"] if *method == Method::POST => {
            validate_container_create(state, body)?;
            Ok(Decision::Allow)
        }
        ["containers", id] if *method == Method::DELETE => {
            validate_identifier(id)?;
            Ok(Decision::ProjectContainer((*id).to_string()))
        }
        ["containers", id, "rename"] if *method == Method::POST => {
            validate_identifier(id)?;
            validate_container_rename(state, uri)?;
            Ok(Decision::ProjectContainer((*id).to_string()))
        }
        ["containers", id, action]
            if (*method == Method::GET
                && matches!(*action, "json" | "logs" | "stats" | "top" | "changes"))
                || (*method == Method::POST
                    && matches!(*action, "start" | "stop" | "restart" | "kill" | "wait")) =>
        {
            validate_identifier(id)?;
            Ok(Decision::ProjectContainer((*id).to_string()))
        }
        ["images", "create"] if *method == Method::POST => {
            validate_image_pull(state, uri)?;
            Ok(Decision::Allow)
        }
        ["images", ..] if *method == Method::GET => Ok(Decision::Allow),
        ["distribution", ..] if *method == Method::GET => Ok(Decision::Allow),
        ["networks", ..] if *method == Method::GET => Ok(Decision::Allow),
        ["networks", network, action]
            if *method == Method::POST && matches!(*action, "connect" | "disconnect") =>
        {
            validate_identifier(network)?;
            let value: Value = serde_json::from_slice(body)
                .map_err(|_| "network mutation body must be JSON".to_string())?;
            let container = value
                .get("Container")
                .and_then(Value::as_str)
                .ok_or_else(|| "network mutation is missing Container".to_string())?;
            validate_identifier(container)?;
            Ok(Decision::ProjectNetworkMutation {
                network: (*network).to_string(),
                container: container.to_string(),
            })
        }
        ["volumes", ..] if *method == Method::GET => Ok(Decision::Allow),
        ["system", ..] if *method == Method::GET => Ok(Decision::Allow),
        _ => Err(format!(
            "Docker API operation is not allowed: {method} {path}"
        )),
    }
}

fn validate_container_rename(state: &GuardState, uri: &Uri) -> std::result::Result<(), String> {
    let requested =
        query_param(uri, "name").ok_or_else(|| "container rename is missing name".to_string())?;
    let requested = requested.trim_start_matches('/');
    validate_identifier(requested)?;

    // Compose temporarily renames the old container to `<12 hex>_<original>` while replacing
    // it. Keep that one lifecycle operation, but do not expose arbitrary Docker renames.
    let base = requested
        .split_once('_')
        .filter(|(prefix, _)| prefix.len() == 12 && prefix.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|(_, base)| base)
        .unwrap_or(requested);
    let allowed = ["backend", "frontend", "postgres", "updater"]
        .into_iter()
        .any(|service| {
            base == format!("{}-{service}-1", state.config.project)
                || base == format!("{}_{service}_1", state.config.project)
                || base == format!("{}-{service}", state.config.project)
                || base == format!("myriad-{service}")
        });
    if !allowed {
        return Err("container rename target is outside the Compose lifecycle allowlist".into());
    }
    Ok(())
}

fn validate_container_create(state: &GuardState, body: &Bytes) -> std::result::Result<(), String> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| "containers/create body must be valid JSON".to_string())?;
    let labels = value
        .get("Labels")
        .and_then(Value::as_object)
        .ok_or_else(|| "managed container requires Compose labels".to_string())?;
    let project = labels
        .get("com.docker.compose.project")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if project != state.config.project {
        return Err("container project label is not allowed".into());
    }
    let service = labels
        .get("com.docker.compose.service")
        .and_then(Value::as_str)
        .ok_or_else(|| "container is missing Compose service label".to_string())?;
    if !matches!(service, "backend" | "frontend" | "postgres" | "updater") {
        return Err("Compose service is not managed by updater".into());
    }

    let image = value
        .get("Image")
        .and_then(Value::as_str)
        .ok_or_else(|| "container image is missing".to_string())?;
    if !state
        .config
        .allowed_images
        .contains(&normalize_repository(image))
    {
        return Err("container image repository is not allowlisted".into());
    }
    if nonempty(value.get("Entrypoint")) || nonempty(value.get("Cmd")) {
        return Err("command or entrypoint overrides are not allowed".into());
    }

    let host = value
        .get("HostConfig")
        .cloned()
        .unwrap_or_else(|| json!({}));
    reject_true(&host, "Privileged")?;
    reject_nonempty_fields(
        &host,
        &[
            "CapAdd",
            "Devices",
            "DeviceRequests",
            "VolumesFrom",
            "PortBindings",
            "Sysctls",
        ],
    )?;
    for field in [
        "PidMode",
        "IpcMode",
        "UTSMode",
        "UsernsMode",
        "CgroupnsMode",
    ] {
        if nonempty(host.get(field)) {
            return Err(format!("HostConfig.{field} is not allowed"));
        }
    }
    if let Some(mode) = host.get("NetworkMode").and_then(Value::as_str) {
        if !mode.is_empty()
            && !matches!(mode, "default" | "bridge" | "none")
            && mode != state.config.compose_network
            && mode != state.config.admin_network
            && mode != state.config.guard_network
        {
            return Err("host or foreign network mode is not allowed".into());
        }
        authorize_guard_network_attachment(service, mode, &state.config)?;
    }
    if let Some(options) = host.get("SecurityOpt").and_then(Value::as_array) {
        if options.iter().any(|v| {
            !v.as_str()
                .is_some_and(|s| s == "no-new-privileges" || s == "no-new-privileges:true")
        }) {
            return Err("only no-new-privileges SecurityOpt is allowed".into());
        }
    }

    for bind in host
        .get("Binds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let bind = bind
            .as_str()
            .ok_or_else(|| "HostConfig.Binds must contain strings".to_string())?;
        validate_bind(state, service, bind)?;
    }
    for mount in host
        .get("Mounts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        validate_mount(state, service, mount)?;
    }
    for mount in value
        .get("Mounts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        validate_mount(state, service, mount)?;
    }

    if let Some(endpoints) = value
        .pointer("/NetworkingConfig/EndpointsConfig")
        .and_then(Value::as_object)
    {
        for network in endpoints.keys() {
            if !is_allowlisted_network_name(network, &state.config) {
                return Err(format!("network {network} is outside the Myriad allowlist"));
            }
            authorize_guard_network_attachment(service, network, &state.config)?;
        }
    }
    Ok(())
}

fn is_allowlisted_network_name(name: &str, config: &GuardConfig) -> bool {
    name == config.compose_network
        || name == config.admin_network
        || name == config.guard_network
}

/// Only the Compose `updater` service may join the docker-guard network. Business services
/// may attach to the project Compose network (`myriad-net`) and the admin plane
/// (`myriad-admin-net`); only `updater` may dual-home onto guard-net.
fn authorize_guard_network_attachment(
    service: &str,
    network_name: &str,
    config: &GuardConfig,
) -> std::result::Result<(), String> {
    if network_name == config.guard_network && service != "updater" {
        return Err(
            "only the updater service may attach to the docker-guard network".into(),
        );
    }
    Ok(())
}

fn validate_bind(state: &GuardState, service: &str, bind: &str) -> std::result::Result<(), String> {
    let parts = bind.split(':').collect::<Vec<_>>();
    if !(2..=3).contains(&parts.len()) {
        return Err("invalid bind syntax".into());
    }
    if let Some(options) = parts.get(2) {
        validate_mount_options(options)?;
    }
    validate_mount_pair(
        state,
        service,
        parts[0],
        parts[1],
        Path::new(parts[0]).is_absolute(),
    )
}

fn validate_mount(
    state: &GuardState,
    service: &str,
    mount: &Value,
) -> std::result::Result<(), String> {
    let kind = mount
        .get("Type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !matches!(kind, "bind" | "volume") {
        return Err("only bind and volume mounts are allowed".into());
    }
    if kind == "bind" {
        if let Some(propagation) = mount
            .pointer("/BindOptions/Propagation")
            .and_then(Value::as_str)
        {
            if !matches!(propagation, "" | "private" | "rprivate") {
                return Err("bind mount propagation is not allowed".into());
            }
        }
    }
    if kind == "volume"
        && (nonempty(mount.pointer("/VolumeOptions/DriverConfig"))
            || nonempty(mount.pointer("/VolumeOptions/Subpath")))
    {
        return Err("volume driver configuration or subpath is not allowed".into());
    }
    let source = mount
        .get("Source")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let target = mount
        .get("Target")
        .or_else(|| mount.get("Destination"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    validate_mount_pair(state, service, source, target, kind == "bind")
}

fn validate_mount_pair(
    state: &GuardState,
    service: &str,
    source: &str,
    target: &str,
    host_bind: bool,
) -> std::result::Result<(), String> {
    let root = state.host_compose_root.as_path();
    let source_path = Path::new(source);
    let exact_host_pair = |relative: &str, expected_target: &str| {
        source_path == root.join(relative) && target == expected_target
    };
    match service {
        "frontend" => Err("frontend container may not add mounts".into()),
        "backend" => {
            if host_bind || source_path.is_absolute() {
                return Err("backend host bind mounts are forbidden".into());
            }
            let allowed_name = source == format!("{}_backend_cache", state.config.project)
                || source == format!("{}_backend_data", state.config.project);
            if !allowed_name || !matches!(target, "/app/cache" | "/app/data") {
                return Err("backend named volume is outside the allowlist".into());
            }
            Ok(())
        }
        "postgres" => {
            if !host_bind || !exact_host_pair("pgdata", "/var/lib/postgresql") {
                return Err("postgres may only bind the project pgdata directory".into());
            }
            validate_visible_host_directory(state, "pgdata")
        }
        "updater" => {
            if !host_bind || !exact_host_pair("", "/host/compose") {
                return Err("updater may only bind the deployment root at /host/compose".into());
            }
            validate_visible_host_directory(state, "")
        }
        _ => Err("service mount policy is undefined".into()),
    }
}

fn validate_mount_options(options: &str) -> std::result::Result<(), String> {
    for option in options.split(',').filter(|value| !value.is_empty()) {
        if matches!(
            option,
            "shared" | "rshared" | "slave" | "rslave" | "unbindable" | "runbindable"
        ) {
            return Err("bind mount propagation is not allowed".into());
        }
        if !matches!(
            option,
            "ro" | "rw" | "z" | "Z" | "consistent" | "cached" | "delegated" | "nocopy"
        ) {
            return Err(format!("mount option {option} is not allowed"));
        }
    }
    Ok(())
}

fn validate_visible_host_directory(
    state: &GuardState,
    relative: &str,
) -> std::result::Result<(), String> {
    let mut visible = state.config.compose_dir.clone();
    let root_meta = std::fs::symlink_metadata(&visible)
        .map_err(|_| "deployment root is unavailable to docker guard".to_string())?;
    if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
        return Err("deployment root must be a real directory, not a symlink".into());
    }

    for component in Path::new(relative).components() {
        let std::path::Component::Normal(component) = component else {
            return Err("host bind path contains an invalid component".into());
        };
        visible.push(component);
        let metadata = std::fs::symlink_metadata(&visible)
            .map_err(|_| "host bind source does not exist".to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("host bind source may not contain symbolic links".into());
        }
        if !metadata.is_dir() {
            return Err("host bind source must be a directory".into());
        }
    }
    Ok(())
}

fn validate_image_pull(state: &GuardState, uri: &Uri) -> std::result::Result<(), String> {
    let image = query_param(uri, "fromImage")
        .ok_or_else(|| "images/create requires fromImage".to_string())?;
    if state
        .config
        .allowed_images
        .contains(&normalize_repository(&image))
    {
        Ok(())
    } else {
        Err("image pull repository is not allowlisted".into())
    }
}

fn validate_image_tag(
    state: &GuardState,
    encoded_source: &str,
    uri: &Uri,
) -> std::result::Result<(), String> {
    let source = decode_path_segment(encoded_source);
    let repo = query_param(uri, "repo").ok_or_else(|| "image tag requires repo".to_string())?;
    if !state
        .config
        .allowed_images
        .contains(&normalize_repository(&source))
    {
        return Err("image tag source repository is not allowlisted".into());
    }
    if !state
        .config
        .allowed_images
        .contains(&normalize_repository(&repo))
    {
        return Err("image tag target repository is not allowlisted".into());
    }
    Ok(())
}

async fn container_belongs_to_project(state: &GuardState, id: &str) -> Result<bool> {
    let value = daemon_json(&state.config.socket_path, &format!("/containers/{id}/json")).await?;
    Ok(managed_project_service(&value, &state.config).is_some())
}

/// Returns the Compose service name when the container is a managed project member.
fn managed_project_service(inspect: &Value, config: &GuardConfig) -> Option<String> {
    let project = inspect
        .pointer("/Config/Labels/com.docker.compose.project")
        .and_then(Value::as_str)?;
    let service = inspect
        .pointer("/Config/Labels/com.docker.compose.service")
        .and_then(Value::as_str)?;
    if project == config.project
        && matches!(service, "backend" | "frontend" | "postgres" | "updater")
    {
        Some(service.to_string())
    } else {
        None
    }
}

fn allowlisted_network_name(inspect: &Value, config: &GuardConfig) -> Option<String> {
    let name = inspect.get("Name").and_then(Value::as_str)?;
    if is_allowlisted_network_name(name, config) {
        Some(name.to_string())
    } else {
        None
    }
}

/// Authorize `networks/{id}/connect|disconnect`: network must be allowlisted, container must
/// be a managed project service, and only `updater` may join/leave the docker-guard network.
async fn authorize_network_mutation(
    state: &GuardState,
    network: &str,
    container: &str,
) -> std::result::Result<(), String> {
    let container_inspect =
        daemon_json(&state.config.socket_path, &format!("/containers/{container}/json"))
            .await
            .map_err(|e| {
                warn!(container, err = %e, "docker guard could not authorize container");
                "container authorization failed".to_string()
            })?;
    let service = managed_project_service(&container_inspect, &state.config).ok_or_else(|| {
        "container is not a managed service in this Compose project".to_string()
    })?;

    let network_inspect = daemon_json(&state.config.socket_path, &format!("/networks/{network}"))
        .await
        .map_err(|e| {
            warn!(network, err = %e, "docker guard could not authorize network");
            "network authorization failed".to_string()
        })?;
    let network_name = allowlisted_network_name(&network_inspect, &state.config)
        .ok_or_else(|| "network is outside the Myriad allowlist".to_string())?;

    authorize_guard_network_attachment(&service, &network_name, &state.config)
}

const MAX_SELF_UPDATE_BODY: usize = 4 * 1024;

#[derive(Debug, Clone, serde::Deserialize)]
struct SelfUpdateRequestBody {
    previous_tag: String,
    target_tag: String,
}

async fn handle_self_update(state: GuardState, req: Request<Body>) -> Response {
    if req.method() != Method::POST {
        return denial(StatusCode::METHOD_NOT_ALLOWED, "POST required");
    }
    let provided = req
        .headers()
        .get("X-Update-Token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !constant_time_eq(provided.as_bytes(), state.config.update_token.as_bytes()) {
        return denial(StatusCode::UNAUTHORIZED, "invalid update token");
    }

    let body = match to_bytes(req.into_body(), MAX_SELF_UPDATE_BODY).await {
        Ok(body) => body,
        Err(_) => {
            return denial(
                StatusCode::PAYLOAD_TOO_LARGE,
                "self-update body exceeds 4 KiB",
            )
        }
    };
    let request: SelfUpdateRequestBody = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return denial(
                StatusCode::BAD_REQUEST,
                "JSON body required: {\"previous_tag\":\"...\",\"target_tag\":\"...\"}",
            )
        }
    };
    if let Err(reason) = validate_self_update_tags(&request.previous_tag, &request.target_tag) {
        return denial(StatusCode::BAD_REQUEST, &reason);
    }

    if state
        .self_update_running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return denial(StatusCode::CONFLICT, "self-update already scheduled");
    }

    let previous_tag = request.previous_tag;
    let target_tag = request.target_tag;
    let task_state = state.clone();
    let task_previous = previous_tag.clone();
    let task_target = target_tag.clone();
    tokio::spawn(async move {
        tokio::time::sleep(SELF_UPDATE_DELAY).await;
        let result =
            run_guarded_self_update(&task_state, &task_previous, &task_target).await;
        task_state
            .self_update_running
            .store(false, Ordering::SeqCst);
        match result {
            Ok(()) => info!(
                previous_tag = %task_previous,
                target_tag = %task_target,
                "guarded updater self-update completed"
            ),
            Err(e) => error!(
                err = %e,
                previous_tag = %task_previous,
                target_tag = %task_target,
                "guarded updater self-update failed (helper restores UPDATER_TAG on compose failure)"
            ),
        }
    });
    (
        StatusCode::ACCEPTED,
        axum::Json(json!({
            "scheduled": true,
            "executor": "docker-guard",
            "previous_tag": previous_tag,
            "target_tag": target_tag,
        })),
    )
        .into_response()
}

fn validate_self_update_tags(previous: &str, target: &str) -> std::result::Result<(), String> {
    use super::self_update_helper::is_safe_self_update_tag;
    if !is_safe_self_update_tag(previous) {
        return Err(format!("invalid previous_tag: {previous}"));
    }
    if !is_safe_self_update_tag(target) {
        return Err(format!("invalid target_tag: {target}"));
    }
    Ok(())
}

/// Recreate both TCB services so `docker-guard` tracks `UPDATER_TAG` alongside
/// `updater` and `updater-gateway` (same `UPDATER_TAG` image).
///
/// **Why the raw unix socket?** Container-create policy only allows Compose
/// services `backend|frontend|postgres|updater`. Recreating `docker-guard` or
/// `updater-gateway` through the policy proxy on `:2375` would be denied.
/// Self-update is a fixed argv TCB self-replace (not arbitrary Docker API), so
/// compose talks to `unix:///var/run/docker.sock` directly. All other updater
/// traffic still uses the policy proxy via `DOCKER_HOST=tcp://docker-guard:2375`.
///
/// **Why a one-shot helper?** Running `compose up docker-guard` from inside the
/// live guard container races with killing that container mid-compose. A short
/// helper (`myriad-tcb-self-update` on the target updater image) performs the
/// recreate, restores `UPDATER_TAG` on failure, writes durable status, and
/// exits. Compose dir is mounted **rw** only for this helper so it can rewrite
/// `.env` and `state/self-update-last.json` after the old guard may be gone.
async fn run_guarded_self_update(
    state: &GuardState,
    previous_tag: &str,
    target_tag: &str,
) -> Result<()> {
    // Tags already validated in the HTTP handler; re-check before docker run env.
    validate_self_update_tags(previous_tag, target_tag).map_err(anyhow::Error::msg)?;
    validate_simple_name("COMPOSE_PROJECT_NAME", &state.config.project)?;

    let helper_image = self_update_helper_image(&state.config.env_file)?;
    let host_root = state.host_compose_root.to_string_lossy().into_owned();
    let sock = state.config.socket_path.to_string_lossy().into_owned();
    let docker_host = format!("unix://{sock}");

    // Paths *inside* the helper container (host_root is bind-mounted at /host/compose).
    let helper_compose_dir = "/host/compose";
    let helper_env_file = "/host/compose/.env";
    let helper_status_file = "/host/compose/state/self-update-last.json";

    let mut command = Command::new("docker");
    command
        .env("DOCKER_HOST", &docker_host)
        .args([
            "run",
            "--rm",
            "--name",
            &format!("myriad-tcb-self-update-{}", std::process::id()),
            "-v",
            &format!("{sock}:/var/run/docker.sock"),
            // rw: helper may restore UPDATER_TAG and write self-update-last.json
            "-v",
            &format!("{host_root}:/host/compose:rw"),
            "-e",
            &format!(
                "{}={previous_tag}",
                super::self_update_helper::ENV_PREVIOUS_TAG
            ),
            "-e",
            &format!("{}={target_tag}", super::self_update_helper::ENV_TARGET_TAG),
            "-e",
            &format!(
                "{}={}",
                super::self_update_helper::ENV_PROJECT,
                state.config.project
            ),
            "-e",
            &format!(
                "{}={host_root}",
                super::self_update_helper::ENV_PROJECT_DIRECTORY
            ),
            "-e",
            &format!(
                "{}={helper_compose_dir}",
                super::self_update_helper::ENV_COMPOSE_DIR
            ),
            "-e",
            &format!(
                "{}={helper_env_file}",
                super::self_update_helper::ENV_ENV_FILE
            ),
            "-e",
            &format!(
                "{}={helper_status_file}",
                super::self_update_helper::ENV_STATUS_FILE
            ),
            "--network",
            "none",
            "--security-opt",
            "no-new-privileges:true",
            "--entrypoint",
            "/usr/local/bin/myriad-tcb-self-update",
            &helper_image,
        ]);

    let output = command
        .output()
        .await
        .context("spawn TCB self-update helper (direct docker.sock)")?;
    if !output.status.success() {
        return Err(anyhow!(
            "TCB self-update helper failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    info!(
        image = %helper_image,
        previous_tag,
        target_tag,
        "TCB self-update recreated docker-guard, updater, and updater-gateway via direct unix socket"
    );
    Ok(())
}

/// Image used by the self-update helper. Prefers the post-rewrite `UPDATER_TAG`
/// from the deployment `.env` so the helper binary matches the target release.
fn self_update_helper_image(env_file: &Path) -> Result<String> {
    let text = std::fs::read_to_string(env_file)
        .with_context(|| format!("read env file for self-update image: {}", env_file.display()))?;
    let mut image = "docker.io/somekawahitomi/myriad-updater".to_string();
    let mut tag = None::<String>;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("UPDATER_IMAGE=") {
            let v = rest.trim().trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                image = v.to_string();
            }
        } else if let Some(rest) = line.strip_prefix("UPDATER_TAG=") {
            let v = rest.trim().trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                tag = Some(v.to_string());
            }
        }
    }
    let tag = tag.ok_or_else(|| anyhow!("UPDATER_TAG missing from {}", env_file.display()))?;
    Ok(format!("{image}:{tag}"))
}

async fn discover_host_compose_root(socket: &Path, container_id: &str) -> Result<PathBuf> {
    validate_identifier(container_id).map_err(anyhow::Error::msg)?;
    let value = daemon_json(socket, &format!("/containers/{container_id}/json")).await?;
    let mounts = value
        .get("Mounts")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("docker guard container has no mounts"))?;
    for mount in mounts {
        if mount.get("Destination").and_then(Value::as_str) == Some("/host/compose") {
            let source = mount
                .get("Source")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("/host/compose mount has no host source"))?;
            return Ok(PathBuf::from(source));
        }
    }
    Err(anyhow!(
        "docker guard requires the deployment directory mounted at /host/compose"
    ))
}

async fn daemon_json(socket: &Path, path: &str) -> Result<Value> {
    let req = Request::builder()
        .method(Method::GET)
        .uri(path)
        .header(header::HOST, "localhost")
        .body(Body::empty())?;
    let resp = forward(socket, req).await?;
    if !resp.status().is_success() {
        return Err(anyhow!("docker inspect returned {}", resp.status()));
    }
    let body = to_bytes(resp.into_body(), MAX_INSPECT_BODY).await?;
    Ok(serde_json::from_slice(&body)?)
}

async fn forward(socket: &Path, mut req: Request<Body>) -> Result<Response> {
    let path = req
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or(req.uri().path())
        .parse::<Uri>()?;
    *req.uri_mut() = path;
    req.headers_mut().remove(header::CONNECTION);

    let stream = UnixStream::connect(socket).await?;
    let io = TokioIo::new(stream);
    let (mut sender, connection) = http1::handshake(io).await?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            warn!(err = %e, "docker guard daemon connection ended");
        }
    });
    let resp = sender.send_request(req).await?;
    let (parts, body) = resp.into_parts();
    Ok(Response::from_parts(parts, Body::new(body)))
}

fn strip_api_version(path: &str) -> &str {
    let Some(rest) = path.strip_prefix("/v") else {
        return path;
    };
    let Some(slash) = rest.find('/') else {
        return path;
    };
    if rest[..slash]
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.')
    {
        &rest[slash..]
    } else {
        path
    }
}

fn normalize_repository(image: &str) -> String {
    let image = image.trim().split('@').next().unwrap_or(image.trim());
    let (prefix, last) = image.rsplit_once('/').unwrap_or(("", image));
    let name = last.split_once(':').map(|(name, _)| name).unwrap_or(last);
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}/{name}")
    }
}

fn query_param(uri: &Uri, name: &str) -> Option<String> {
    url::form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

fn decode_path_segment(value: &str) -> String {
    let encoded = format!("value={value}");
    url::form_urlencoded::parse(encoded.as_bytes())
        .next()
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default()
}

fn validate_identifier(value: &str) -> std::result::Result<(), String> {
    if value.is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err("invalid Docker object identifier".into());
    }
    Ok(())
}

fn validate_simple_name(label: &str, value: &str) -> Result<()> {
    validate_identifier(value).map_err(|e| anyhow!("{label}: {e}"))
}

fn reject_true(value: &Value, field: &str) -> std::result::Result<(), String> {
    if value.get(field).and_then(Value::as_bool) == Some(true) {
        Err(format!("HostConfig.{field}=true is not allowed"))
    } else {
        Ok(())
    }
}

fn reject_nonempty_fields(value: &Value, fields: &[&str]) -> std::result::Result<(), String> {
    for field in fields {
        if nonempty(value.get(*field)) {
            return Err(format!("HostConfig.{field} is not allowed"));
        }
    }
    Ok(())
}

fn nonempty(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(false)) => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(v)) => !v.is_empty(),
        Some(Value::Object(v)) => !v.is_empty(),
        Some(Value::Number(v)) => v.as_i64().unwrap_or_default() != 0,
        Some(Value::Bool(true)) => true,
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left, right) in a.iter().zip(b.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

fn current_container_id() -> Result<String> {
    let hostname = match std::env::var("HOSTNAME") {
        Ok(value) => value,
        Err(_) => std::fs::read_to_string("/etc/hostname")?,
    };
    let hostname = hostname.trim();
    if hostname.is_empty() {
        return Err(anyhow!("cannot determine docker guard container id"));
    }
    Ok(hostname.to_string())
}

fn denial(status: StatusCode, message: &str) -> Response {
    (status, axum::Json(json!({"message": message}))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> GuardState {
        state_with_visible_root(PathBuf::from("/host/compose"))
    }

    fn state_with_visible_root(visible_root: PathBuf) -> GuardState {
        GuardState {
            config: Arc::new(GuardConfig {
                listen: "127.0.0.1:2375".parse().unwrap(),
                socket_path: "/var/run/docker.sock".into(),
                project: "myriad".into(),
                compose_network: "myriad-net".into(),
                admin_network: "myriad-admin-net".into(),
                guard_network: "myriad-docker-guard-net".into(),
                compose_dir: visible_root,
                env_file: "/host/compose/.env".into(),
                update_token: "9xQ3vN8mP2rT5wY7zA1bC4dF6hJ8kL0n".into(),
                allowed_images: [
                    "docker.io/example/backend".into(),
                    "docker.io/example/frontend".into(),
                    "docker.io/example/updater".into(),
                    "postgres".into(),
                ]
                .into_iter()
                .collect(),
            }),
            host_compose_root: Arc::new(PathBuf::from("/srv/myriad")),
            self_update_running: Arc::new(AtomicBool::new(false)),
        }
    }

    fn create(service: &str, image: &str, host: Value) -> Bytes {
        Bytes::from(
            serde_json::to_vec(&json!({
                "Image": image,
                "Labels": {
                    "com.docker.compose.project": "myriad",
                    "com.docker.compose.service": service,
                },
                "HostConfig": host,
            }))
            .unwrap(),
        )
    }

    #[test]
    fn backend_create_allows_only_named_project_volumes() {
        let body = create(
            "backend",
            "docker.io/example/backend:v1",
            json!({"Binds": ["myriad_backend_cache:/app/cache:rw"]}),
        );
        assert!(validate_container_create(&state(), &body).is_ok());
    }

    #[test]
    fn backend_host_bind_is_denied() {
        let body = create(
            "backend",
            "docker.io/example/backend:v1",
            json!({"Binds": ["/:/host:rw"]}),
        );
        assert!(validate_container_create(&state(), &body)
            .unwrap_err()
            .contains("host bind"));
    }

    #[test]
    fn privileged_create_is_denied() {
        let body = create(
            "frontend",
            "docker.io/example/frontend:v1",
            json!({"Privileged": true}),
        );
        assert!(validate_container_create(&state(), &body)
            .unwrap_err()
            .contains("Privileged"));
    }

    #[test]
    fn updater_accepts_only_the_deployment_root_bind() {
        let visible = tempfile::tempdir().unwrap();
        let state = state_with_visible_root(visible.path().to_path_buf());
        let good = create(
            "updater",
            "docker.io/example/updater:v1",
            json!({"Binds": ["/srv/myriad:/host/compose:rw"]}),
        );
        assert!(validate_container_create(&state, &good).is_ok());

        let legacy_state = create(
            "updater",
            "docker.io/example/updater:v1",
            json!({"Binds": ["/srv/myriad/state:/state:rw"]}),
        );
        assert!(validate_container_create(&state, &legacy_state).is_err());

        let socket = create(
            "updater",
            "docker.io/example/updater:v1",
            json!({"Binds": ["/var/run/docker.sock:/var/run/docker.sock"]}),
        );
        assert!(validate_container_create(&state, &socket).is_err());
    }

    #[test]
    fn postgres_symlink_bind_is_denied() {
        let visible = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("/etc", visible.path().join("pgdata")).unwrap();
        let state = state_with_visible_root(visible.path().to_path_buf());
        let body = create(
            "postgres",
            "postgres:18-alpine",
            json!({"Binds": ["/srv/myriad/pgdata:/var/lib/postgresql:rw"]}),
        );
        assert!(validate_container_create(&state, &body)
            .unwrap_err()
            .contains("symbolic links"));
    }

    #[test]
    fn updater_state_symlink_cannot_be_mounted_separately() {
        let visible = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("/", visible.path().join("state")).unwrap();
        let state = state_with_visible_root(visible.path().to_path_buf());
        let body = create(
            "updater",
            "docker.io/example/updater:v1",
            json!({"Binds": ["/srv/myriad/state:/state:rw"]}),
        );
        assert!(validate_container_create(&state, &body).is_err());
    }

    #[test]
    fn bind_mount_propagation_is_denied() {
        let visible = tempfile::tempdir().unwrap();
        let state = state_with_visible_root(visible.path().to_path_buf());
        let string_bind = create(
            "updater",
            "docker.io/example/updater:v1",
            json!({"Binds": ["/srv/myriad:/host/compose:rw,rshared"]}),
        );
        assert!(validate_container_create(&state, &string_bind)
            .unwrap_err()
            .contains("propagation"));

        let structured_mount = create(
            "updater",
            "docker.io/example/updater:v1",
            json!({"Mounts": [{
                "Type": "bind",
                "Source": "/srv/myriad",
                "Target": "/host/compose",
                "BindOptions": {"Propagation": "rshared"}
            }]}),
        );
        assert!(validate_container_create(&state, &structured_mount)
            .unwrap_err()
            .contains("propagation"));
    }

    #[test]
    fn exec_and_unknown_mutations_are_denied() {
        let request = Uri::from_static("/v1.51/containers/myriad-backend/exec");
        assert!(classify_request(&state(), &Method::POST, &request, &Bytes::new()).is_err());
    }

    #[test]
    fn compose_recreate_rename_is_narrowly_allowed() {
        for name in [
            "myriad-backend",
            "myriad-backend-1",
            "myriad_backend_1",
            "0fea459923c4_myriad-backend-1",
        ] {
            let uri: Uri = format!("/v1.51/containers/abc123/rename?name={name}")
                .parse()
                .unwrap();
            assert!(validate_container_rename(&state(), &uri).is_ok(), "{name}");
        }

        for name in ["docker-guard", "other-backend-1", "abc_myriad-backend-1"] {
            let uri: Uri = format!("/v1.51/containers/abc123/rename?name={name}")
                .parse()
                .unwrap();
            assert!(validate_container_rename(&state(), &uri).is_err(), "{name}");
        }
    }

    #[test]
    fn image_pull_is_repository_allowlisted() {
        let allowed =
            Uri::from_static("/v1.51/images/create?fromImage=docker.io%2Fexample%2Fbackend&tag=v1");
        assert!(validate_image_pull(&state(), &allowed).is_ok());
        let denied = Uri::from_static("/v1.51/images/create?fromImage=evil%2Fpayload&tag=latest");
        assert!(validate_image_pull(&state(), &denied).is_err());
    }

    #[test]
    fn image_tag_requires_allowlisted_source_and_target() {
        let allowed = Uri::from_static("/v1.51/images/docker.io%2Fexample%2Fbackend:v1/tag?repo=docker.io%2Fexample%2Fbackend&tag=myriad-rollback");
        assert!(classify_request(&state(), &Method::POST, &allowed, &Bytes::new()).is_ok());

        let denied_source = Uri::from_static("/v1.51/images/evil%2Fpayload:v1/tag?repo=docker.io%2Fexample%2Fbackend&tag=myriad-rollback");
        assert!(classify_request(&state(), &Method::POST, &denied_source, &Bytes::new()).is_err());

        let unescaped_slashes = Uri::from_static("/v1.51/images/docker.io/example/backend:v1/tag?repo=docker.io%2Fexample%2Fbackend&tag=myriad-rollback");
        assert!(
            classify_request(&state(), &Method::POST, &unescaped_slashes, &Bytes::new()).is_ok()
        );
    }

    #[test]
    fn api_version_prefix_is_normalized() {
        assert_eq!(
            strip_api_version("/v1.51/containers/json"),
            "/containers/json"
        );
        assert_eq!(strip_api_version("/containers/json"), "/containers/json");
    }

    #[test]
    fn self_update_tags_reject_shell_metacharacters() {
        assert!(validate_self_update_tags("v0.1.0", "v0.2.0").is_ok());
        assert!(validate_self_update_tags("v0.1.0", "v1;rm -rf /").is_err());
        assert!(validate_self_update_tags("$(reboot)", "v0.2.0").is_err());
        assert!(validate_self_update_tags("v0.1.0", "").is_err());
    }

    fn create_with_networking(
        service: &str,
        image: &str,
        host: Value,
        endpoints: Value,
    ) -> Bytes {
        Bytes::from(
            serde_json::to_vec(&json!({
                "Image": image,
                "Labels": {
                    "com.docker.compose.project": "myriad",
                    "com.docker.compose.service": service,
                },
                "HostConfig": host,
                "NetworkingConfig": {
                    "EndpointsConfig": endpoints,
                },
            }))
            .unwrap(),
        )
    }

    #[test]
    fn only_updater_may_create_with_guard_network_endpoint() {
        let s = state();
        let backend = create_with_networking(
            "backend",
            "docker.io/example/backend:v1",
            json!({}),
            json!({"myriad-docker-guard-net": {}}),
        );
        assert!(
            validate_container_create(&s, &backend)
                .unwrap_err()
                .contains("only the updater service may attach to the docker-guard network"),
            "backend must not join guard-net at create time"
        );

        let postgres = create_with_networking(
            "postgres",
            "postgres:18-alpine",
            json!({}),
            json!({"myriad-docker-guard-net": {}}),
        );
        assert!(validate_container_create(&s, &postgres).is_err());

        let updater = create_with_networking(
            "updater",
            "docker.io/example/updater:v1",
            json!({}),
            json!({
                "myriad-admin-net": {},
                "myriad-docker-guard-net": {},
            }),
        );
        assert!(
            validate_container_create(&s, &updater).is_ok(),
            "updater dual-homing (admin + guard) at create must remain allowed"
        );
    }

    #[test]
    fn admin_network_is_allowlisted_but_guard_stays_updater_only() {
        let s = state();
        let backend = create_with_networking(
            "backend",
            "docker.io/example/backend:v1",
            json!({}),
            json!({
                "myriad-net": {},
                "myriad-admin-net": {},
            }),
        );
        assert!(
            validate_container_create(&s, &backend).is_ok(),
            "backend may dual-home business + admin nets"
        );

        let backend_guard = create_with_networking(
            "backend",
            "docker.io/example/backend:v1",
            json!({}),
            json!({
                "myriad-net": {},
                "myriad-admin-net": {},
                "myriad-docker-guard-net": {},
            }),
        );
        assert!(
            validate_container_create(&s, &backend_guard)
                .unwrap_err()
                .contains("only the updater service may attach to the docker-guard network"),
            "backend must still be denied on guard-net"
        );

        let mode_admin = create(
            "backend",
            "docker.io/example/backend:v1",
            json!({"NetworkMode": "myriad-admin-net"}),
        );
        assert!(validate_container_create(&s, &mode_admin).is_ok());

        let foreign = create_with_networking(
            "backend",
            "docker.io/example/backend:v1",
            json!({}),
            json!({"bridge": {}}),
        );
        assert!(
            validate_container_create(&s, &foreign)
                .unwrap_err()
                .contains("outside the Myriad allowlist")
        );
    }

    #[test]
    fn only_updater_may_use_guard_network_mode() {
        let s = state();
        let backend = create(
            "backend",
            "docker.io/example/backend:v1",
            json!({"NetworkMode": "myriad-docker-guard-net"}),
        );
        assert!(
            validate_container_create(&s, &backend)
                .unwrap_err()
                .contains("only the updater service may attach to the docker-guard network")
        );

        let updater = create(
            "updater",
            "docker.io/example/updater:v1",
            json!({"NetworkMode": "myriad-docker-guard-net"}),
        );
        assert!(validate_container_create(&s, &updater).is_ok());
    }

    #[test]
    fn managed_services_may_still_attach_compose_network() {
        let s = state();
        for service_image in [
            ("backend", "docker.io/example/backend:v1"),
            ("frontend", "docker.io/example/frontend:v1"),
            ("updater", "docker.io/example/updater:v1"),
        ] {
            let body = create_with_networking(
                service_image.0,
                service_image.1,
                json!({}),
                json!({"myriad-net": {}}),
            );
            assert!(
                validate_container_create(&s, &body).is_ok(),
                "{} must still attach to compose network",
                service_image.0
            );

            let mode_body = create(
                service_image.0,
                service_image.1,
                json!({"NetworkMode": "myriad-net"}),
            );
            assert!(
                validate_container_create(&s, &mode_body).is_ok(),
                "{} NetworkMode=compose network must remain allowed",
                service_image.0
            );
        }

        assert!(authorize_guard_network_attachment(
            "backend",
            "myriad-net",
            &s.config
        )
        .is_ok());
        assert!(authorize_guard_network_attachment(
            "postgres",
            "myriad-net",
            &s.config
        )
        .is_ok());
    }

    #[test]
    fn guard_network_attachment_policy_is_updater_only() {
        let s = state();
        assert!(
            authorize_guard_network_attachment("backend", "myriad-docker-guard-net", &s.config)
                .is_err()
        );
        assert!(
            authorize_guard_network_attachment("frontend", "myriad-docker-guard-net", &s.config)
                .is_err()
        );
        assert!(
            authorize_guard_network_attachment("postgres", "myriad-docker-guard-net", &s.config)
                .is_err()
        );
        assert!(
            authorize_guard_network_attachment("updater", "myriad-docker-guard-net", &s.config)
                .is_ok()
        );
    }

    #[test]
    fn network_connect_classify_requires_container_field() {
        let uri = Uri::from_static("/v1.51/networks/myriad-docker-guard-net/connect");
        let missing = classify_request(&state(), &Method::POST, &uri, &Bytes::from_static(b"{}"));
        assert!(missing.unwrap_err().contains("Container"));

        let body = Bytes::from(serde_json::to_vec(&json!({"Container": "myriad-backend-1"})).unwrap());
        let decision = classify_request(&state(), &Method::POST, &uri, &body).unwrap();
        assert_eq!(
            decision,
            Decision::ProjectNetworkMutation {
                network: "myriad-docker-guard-net".into(),
                container: "myriad-backend-1".into(),
            }
        );
    }

    #[test]
    fn managed_project_service_and_network_helpers() {
        let s = state();
        let backend = json!({
            "Config": {
                "Labels": {
                    "com.docker.compose.project": "myriad",
                    "com.docker.compose.service": "backend",
                }
            }
        });
        assert_eq!(
            managed_project_service(&backend, &s.config).as_deref(),
            Some("backend")
        );

        let foreign = json!({
            "Config": {
                "Labels": {
                    "com.docker.compose.project": "other",
                    "com.docker.compose.service": "backend",
                }
            }
        });
        assert!(managed_project_service(&foreign, &s.config).is_none());

        let guard_net = json!({"Name": "myriad-docker-guard-net"});
        assert_eq!(
            allowlisted_network_name(&guard_net, &s.config).as_deref(),
            Some("myriad-docker-guard-net")
        );
        let other_net = json!({"Name": "bridge"});
        assert!(allowlisted_network_name(&other_net, &s.config).is_none());

        // Connect path: non-updater + guard-net is denied once labels/name are resolved.
        let service = managed_project_service(&backend, &s.config).unwrap();
        let network = allowlisted_network_name(&guard_net, &s.config).unwrap();
        assert!(
            authorize_guard_network_attachment(&service, &network, &s.config)
                .unwrap_err()
                .contains("only the updater service may attach")
        );

        let updater = json!({
            "Config": {
                "Labels": {
                    "com.docker.compose.project": "myriad",
                    "com.docker.compose.service": "updater",
                }
            }
        });
        let service = managed_project_service(&updater, &s.config).unwrap();
        assert!(authorize_guard_network_attachment(&service, &network, &s.config).is_ok());

        // Compose business network stays open to managed services.
        let compose_net = json!({"Name": "myriad-net"});
        let network = allowlisted_network_name(&compose_net, &s.config).unwrap();
        let service = managed_project_service(&backend, &s.config).unwrap();
        assert!(authorize_guard_network_attachment(&service, &network, &s.config).is_ok());
    }
}
