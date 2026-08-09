//! Policy-enforcing proxy between the updater and the host Docker daemon.
//!
//! This is intentionally narrower than a generic socket proxy: mutations are limited to
//! containers in one Compose project, and container-create request bodies are validated before
//! reaching the daemon. The updater never receives the raw Unix socket.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
use tracing::{error, info, warn};

const MAX_REQUEST_BODY: usize = 1024 * 1024;
const MAX_INSPECT_BODY: usize = 2 * 1024 * 1024;
const TRUSTED_GUARD_REPOSITORY: &str = "docker.io/somekawahitomi/myriad-updater";

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
    pub expected_guard_image: String,
    pub host_policy_path: String,
    pub allow_unpinned_dev: bool,
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
        let expected_guard_image = std::env::var("DOCKER_GUARD_EXPECTED_IMAGE")
            .context("DOCKER_GUARD_EXPECTED_IMAGE is required")?;
        let host_policy_path = std::env::var("DOCKER_GUARD_HOST_POLICY_PATH")
            .context("DOCKER_GUARD_HOST_POLICY_PATH is required")?;
        validate_host_policy_path(&host_policy_path)?;
        let allow_unpinned_dev = std::env::var("DOCKER_GUARD_ALLOW_UNPINNED_DEV")
            .is_ok_and(|value| value.eq_ignore_ascii_case("true"));
        if allow_unpinned_dev && !cfg!(debug_assertions) {
            return Err(anyhow!(
                "DOCKER_GUARD_ALLOW_UNPINNED_DEV is forbidden in release builds"
            ));
        }
        validate_guard_image_ref(&expected_guard_image, allow_unpinned_dev)?;
        // This policy is compiled into the Guard TCB. It must never come from
        // updater-writable `.env` or runtime configuration.
        let allowed_images = [
            "docker.io/somekawahitomi/myriad-backend",
            "docker.io/somekawahitomi/myriad-frontend",
            "docker.io/somekawahitomi/myriad-proxy",
            "docker.io/somekawahitomi/myriad-updater",
            "postgres",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<HashSet<_>>();

        Ok(Self {
            listen,
            socket_path,
            project,
            compose_network,
            admin_network,
            guard_network,
            compose_dir,
            expected_guard_image,
            host_policy_path,
            allow_unpinned_dev,
            allowed_images,
        })
    }
}

fn validate_host_policy_path(path: &str) -> Result<()> {
    let looks_absolute = path.starts_with('/')
        || path
            .as_bytes()
            .get(1)
            .is_some_and(|separator| *separator == b':');
    if !looks_absolute
        || path.contains('\n')
        || path.contains('\r')
        || Path::new(path)
            .components()
            .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(anyhow!(
            "DOCKER_GUARD_HOST_POLICY_PATH must be an absolute path without traversal"
        ));
    }
    Ok(())
}

#[derive(Clone)]
struct GuardState {
    config: Arc<GuardConfig>,
    host_compose_root: Arc<PathBuf>,
}

pub async fn run(config: GuardConfig) -> Result<()> {
    let hostname = current_container_id()?;
    verify_running_guard_image(
        &config.socket_path,
        &hostname,
        &config.expected_guard_image,
        config.allow_unpinned_dev,
    )
    .await?;
    let host_compose_root = match std::env::var("DOCKER_GUARD_HOST_COMPOSE_ROOT") {
        Ok(root) if !root.trim().is_empty() => PathBuf::from(root),
        _ => discover_host_compose_root(&config.socket_path, &hostname).await?,
    };
    let listen = config.listen;
    let state = GuardState {
        config: Arc::new(config),
        host_compose_root: Arc::new(host_compose_root.clone()),
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

fn validate_guard_image_ref(image: &str, allow_unpinned_dev: bool) -> Result<()> {
    if allow_unpinned_dev && image.starts_with("myriad-updater-dev:") {
        return Ok(());
    }
    let prefix = format!("{TRUSTED_GUARD_REPOSITORY}@sha256:");
    let Some(digest) = image.strip_prefix(&prefix) else {
        return Err(anyhow!(
            "DOCKER_GUARD_EXPECTED_IMAGE must be {TRUSTED_GUARD_REPOSITORY}@sha256:<64 hex>"
        ));
    };
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(anyhow!(
            "docker-guard image digest must contain exactly 64 hex characters"
        ));
    }
    Ok(())
}

async fn verify_running_guard_image(
    socket: &Path,
    container_id: &str,
    expected: &str,
    allow_unpinned_dev: bool,
) -> Result<()> {
    validate_guard_image_ref(expected, allow_unpinned_dev)?;
    validate_identifier(container_id).map_err(anyhow::Error::msg)?;
    let inspect = daemon_json(socket, &format!("/containers/{container_id}/json")).await?;
    let configured = inspect
        .pointer("/Config/Image")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("docker guard container inspect has no Config.Image"))?;
    if configured != expected {
        return Err(anyhow!(
            "running docker-guard image identity mismatch: configured {configured}, expected {expected}"
        ));
    }
    if allow_unpinned_dev && expected.starts_with("myriad-updater-dev:") {
        return Ok(());
    }

    let image_id = inspect
        .get("Image")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("docker guard container inspect has no immutable image id"))?;
    let Some(image_digest) = image_id.strip_prefix("sha256:") else {
        return Err(anyhow!(
            "docker guard container image id is not sha256-pinned"
        ));
    };
    if image_digest.len() != 64 || !image_digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(anyhow!("docker guard container image id is malformed"));
    }
    let image_inspect = daemon_json(socket, &format!("/images/{image_id}/json")).await?;
    let repo_digests = image_inspect
        .get("RepoDigests")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("docker guard image inspect has no RepoDigests"))?;
    if !repo_digests
        .iter()
        .filter_map(Value::as_str)
        .any(|actual| digest_reference_matches(actual, expected))
    {
        return Err(anyhow!(
            "running docker-guard content digest does not match the host-pinned identity"
        ));
    }
    Ok(())
}

fn digest_reference_matches(actual: &str, expected: &str) -> bool {
    let Some((actual_repo, actual_digest)) = actual.rsplit_once("@sha256:") else {
        return false;
    };
    let Some((expected_repo, expected_digest)) = expected.rsplit_once("@sha256:") else {
        return false;
    };
    actual_repo.trim_start_matches("docker.io/") == expected_repo.trim_start_matches("docker.io/")
        && actual_digest.eq_ignore_ascii_case(expected_digest)
}

async fn handle(
    State(state): State<GuardState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request<Body>,
) -> Response {
    if req.uri().path() == "/_myriad/self-update" {
        // A component that holds updater credentials is not allowed to select
        // or execute the Guard TCB. Guard upgrades are a host-operator action
        // using a digest-pinned image from independently signed release data.
        return denial(
            StatusCode::FORBIDDEN,
            "docker-guard is a separate TCB; host-verified upgrade required",
        );
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
        Decision::ProjectNetworkMutation {
            network,
            container,
            endpoint,
        } => {
            if let Err(reason) =
                authorize_network_mutation(&state, &network, &container, endpoint.as_ref()).await
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
    ProjectNetworkMutation {
        network: String,
        container: String,
        endpoint: Option<Value>,
    },
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
                endpoint: value.get("EndpointConfig").cloned(),
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
    let allowed = ["backend", "frontend", "postgres", "proxy", "updater"]
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
    if !matches!(
        service,
        "backend" | "backend-volume-init" | "frontend" | "postgres" | "proxy" | "updater"
    ) {
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
    let narrow_volume_init = is_narrow_backend_volume_init(&value, &host, service);
    if service == "backend-volume-init" && !narrow_volume_init {
        return Err("backend-volume-init is allowed only in the narrow root init mode".into());
    }
    if requests_root_user(&value) && !narrow_volume_init {
        return Err("explicit root user is allowed only for the backend volume initializer".into());
    }
    reject_true(&host, "Privileged")?;
    reject_true(&host, "PublishAllPorts")?;
    // PortBindings are forbidden for internal services. Proxy is the edge entry and must
    // publish host ports (80/443); validate those bindings separately below.
    let mut host_forbidden = vec![
        "CapAdd",
        "Devices",
        "DeviceRequests",
        "VolumesFrom",
        "Sysctls",
        "ContainerIDFile",
        "Runtime",
        "VolumeDriver",
        "CgroupParent",
        "Isolation",
        "Annotations",
        "Cgroup",
        "StorageOpt",
        "Links",
    ];
    if service != "proxy" {
        host_forbidden.push("PortBindings");
    }
    reject_nonempty_fields(&host, &host_forbidden)?;
    if let Some(log_config) = host.get("LogConfig").filter(|value| nonempty(Some(value))) {
        let log_type = log_config.get("Type").and_then(Value::as_str);
        let config_is_safe = log_config
            .get("Config")
            .and_then(Value::as_object)
            .is_none_or(|config| {
                config
                    .keys()
                    .all(|key| matches!(key.as_str(), "max-size" | "max-file"))
            });
        if log_type != Some("json-file") || !config_is_safe {
            return Err("HostConfig.LogConfig is outside the json-file allowlist".into());
        }
    }
    if service == "proxy" {
        authorize_proxy_port_bindings(host.get("PortBindings"))?;
    }
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
        if !mode.is_empty() && !matches!(mode, "default" | "bridge" | "none") {
            if mode != state.config.compose_network
                && mode != state.config.admin_network
                && mode != state.config.guard_network
            {
                return Err("host or foreign network mode is not allowed".into());
            }
            authorize_guard_network_attachment(service, mode, &state.config)?;
        }
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
        for (network, endpoint) in endpoints {
            if !is_allowlisted_network_name(network, &state.config) {
                return Err(format!("network {network} is outside the Myriad allowlist"));
            }
            authorize_guard_network_attachment(service, network, &state.config)?;
            validate_endpoint_settings(service, endpoint, &state.config)?;
        }
    }
    Ok(())
}

fn validate_endpoint_settings(
    service: &str,
    endpoint: &Value,
    config: &GuardConfig,
) -> std::result::Result<(), String> {
    let Some(object) = endpoint.as_object() else {
        return Err("network endpoint settings must be an object".into());
    };
    for (key, value) in object {
        if key != "Aliases" && nonempty(Some(value)) {
            return Err(format!("network endpoint field {key} is not allowed"));
        }
    }
    if let Some(aliases) = object.get("Aliases").and_then(Value::as_array) {
        let service_alias = service;
        let fixed_name = format!("{}-{service}", config.project);
        let generated_name = format!("{}-{service}-1", config.project);
        let legacy_generated_name = format!("{}_{service}_1", config.project);
        if aliases.iter().any(|alias| {
            !alias.as_str().is_some_and(|alias| {
                alias == service_alias
                    || alias == fixed_name
                    || alias == generated_name
                    || alias == legacy_generated_name
            })
        }) {
            return Err(
                "network aliases are outside the Compose service identity allowlist".into(),
            );
        }
    }
    Ok(())
}

fn requests_root_user(value: &Value) -> bool {
    value
        .get("User")
        .and_then(Value::as_str)
        .and_then(|user| user.split(':').next())
        .is_some_and(|user| matches!(user, "0" | "root"))
}

fn has_exact_env(value: &Value, expected: &str) -> bool {
    let key = expected
        .split_once('=')
        .map(|(key, _)| key)
        .unwrap_or(expected);
    let prefix = format!("{key}=");
    let mut matches = value
        .get("Env")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|entry| entry.starts_with(&prefix));
    matches.next() == Some(expected) && matches.next().is_none()
}

fn has_mount_target(value: &Value, host: &Value, expected: &str) -> bool {
    let bind_has_target = host
        .get("Binds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|bind| bind.split(':').nth(1) == Some(expected));
    let structured_has_target = [host.get("Mounts"), value.get("Mounts")]
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
        .flatten()
        .any(|mount| {
            mount
                .get("Target")
                .or_else(|| mount.get("Destination"))
                .and_then(Value::as_str)
                == Some(expected)
        });
    bind_has_target || structured_has_target
}

/// The updater may run one disposable `backend-volume-init` container as root
/// solely to repair the two backend named volumes. Keep this exception narrower
/// than normal Compose service creation: dedicated service label, exact uid,
/// exact mode flag, one-off label, no network, no-new-privileges, and both
/// already-allowlisted volume targets are required.
fn is_narrow_backend_volume_init(value: &Value, host: &Value, service: &str) -> bool {
    service == "backend-volume-init"
        && value.get("User").and_then(Value::as_str) == Some("0:0")
        && has_exact_env(value, "MYRIAD_VOLUME_INIT_ONLY=true")
        && value
            .pointer("/Labels/com.docker.compose.oneoff")
            .and_then(Value::as_str)
            == Some("True")
        && host.get("AutoRemove").and_then(Value::as_bool) != Some(true)
        && host
            .pointer("/RestartPolicy/Name")
            .and_then(Value::as_str)
            .is_none_or(|name| name.is_empty() || name == "no")
        && host
            .get("SecurityOpt")
            .and_then(Value::as_array)
            .is_some_and(|options| {
                options.iter().any(|option| {
                    option.as_str().is_some_and(|option| {
                        matches!(option, "no-new-privileges" | "no-new-privileges:true")
                    })
                })
            })
        && host.get("NetworkMode").and_then(Value::as_str) == Some("none")
        && value
            .pointer("/NetworkingConfig/EndpointsConfig")
            .and_then(Value::as_object)
            .is_none_or(serde_json::Map::is_empty)
        && has_mount_target(value, host, "/app/cache")
        && has_mount_target(value, host, "/app/data")
}

fn is_allowlisted_network_name(name: &str, config: &GuardConfig) -> bool {
    // Keep in sync with `crate::docker::network_allowlist::NetworkAllowlist::contains`
    // (same three env-backed names). Preflight rejects updates before stop/snapshot when
    // compose would attach managed services outside this set.
    let name = name.trim_start_matches('/');
    name == config.compose_network || name == config.admin_network || name == config.guard_network
}

/// Exact service-to-network topology. In particular, a compromised updater may
/// not attach itself to the business network to reach Postgres/frontend peers.
fn authorize_guard_network_attachment(
    service: &str,
    network_name: &str,
    config: &GuardConfig,
) -> std::result::Result<(), String> {
    let allowed = match service {
        "postgres" | "frontend" => network_name == config.compose_network,
        "backend" | "proxy" => {
            network_name == config.compose_network || network_name == config.admin_network
        }
        "updater" => network_name == config.admin_network || network_name == config.guard_network,
        "backend-volume-init" => false,
        _ => false,
    };
    if !allowed {
        if network_name == config.guard_network && service != "updater" {
            return Err("only the updater service may attach to the docker-guard network".into());
        }
        return Err(format!(
            "service {service} may not attach to network {network_name}"
        ));
    }
    Ok(())
}

fn validate_bind(state: &GuardState, service: &str, bind: &str) -> std::result::Result<(), String> {
    let parts = bind.split(':').collect::<Vec<_>>();
    if !(2..=3).contains(&parts.len()) {
        return Err("invalid bind syntax".into());
    }
    let options = parts.get(2).copied().unwrap_or("");
    if !options.is_empty() {
        validate_mount_options(options)?;
    }
    validate_mount_pair(
        state,
        service,
        parts[0],
        parts[1],
        Path::new(parts[0]).is_absolute(),
        options.split(',').any(|option| option == "ro"),
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
    let read_only = mount
        .get("ReadOnly")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    validate_mount_pair(state, service, source, target, kind == "bind", read_only)
}

fn validate_mount_pair(
    state: &GuardState,
    service: &str,
    source: &str,
    target: &str,
    host_bind: bool,
    read_only: bool,
) -> std::result::Result<(), String> {
    let root = state.host_compose_root.as_path();
    let source_path = Path::new(source);
    let exact_host_pair = |relative: &str, expected_target: &str| {
        source_path == root.join(relative) && target == expected_target
    };
    match service {
        "frontend" => Err("frontend container may not add mounts".into()),
        "backend" | "backend-volume-init" => {
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
            if !host_bind {
                return Err("updater mounts must be fixed host bind paths".into());
            }
            if exact_host_pair("", "/host/compose") {
                if !read_only {
                    return Err("updater deployment root must be mounted read-only".into());
                }
                return validate_visible_host_directory(state, "");
            }
            if exact_host_pair(".env", "/host/compose/.env") {
                return validate_visible_host_path(state, ".env", false);
            }
            if exact_host_pair("state", "/host/compose/state") {
                return validate_visible_host_directory(state, "state");
            }
            if exact_host_pair("pgdata", "/host/compose/pgdata") {
                return validate_visible_host_directory(state, "pgdata");
            }
            if source == state.config.host_policy_path && target == "/run/secrets/docker-guard.env"
            {
                if !read_only {
                    return Err("host Guard policy must be mounted read-only".into());
                }
                return Ok(());
            }
            if cfg!(debug_assertions)
                && exact_host_pair("docker-guard.env", "/run/secrets/docker-guard.env")
            {
                if !read_only {
                    return Err("development Guard policy must be mounted read-only".into());
                }
                return validate_visible_host_path(state, "docker-guard.env", false);
            }
            Err("updater host bind is outside the fixed deployment allowlist".into())
        }
        "proxy" => {
            // Proxy only needs the maintenance state file (read-only).
            if !host_bind || !exact_host_pair("state", "/state") {
                return Err("proxy may only bind the project state directory at /state".into());
            }
            validate_visible_host_directory(state, "state")
        }
        _ => Err("service mount policy is undefined".into()),
    }
}

/// Proxy may only publish container ports 80 and/or 443 to the host.
fn authorize_proxy_port_bindings(bindings: Option<&Value>) -> std::result::Result<(), String> {
    let Some(obj) = bindings.and_then(Value::as_object) else {
        return Ok(());
    };
    if obj.is_empty() {
        return Ok(());
    }
    for key in obj.keys() {
        // Docker API keys look like "80/tcp" or "443/tcp".
        let port = key.split('/').next().unwrap_or(key);
        if port != "80" && port != "443" {
            return Err(format!(
                "proxy may only publish ports 80/443, got HostConfig.PortBindings key {key}"
            ));
        }
    }
    Ok(())
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

fn validate_visible_host_path(
    state: &GuardState,
    relative: &str,
    directory: bool,
) -> std::result::Result<(), String> {
    let path = state.config.compose_dir.join(relative);
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|_| "host bind source does not exist".to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("host bind source may not contain symbolic links".into());
    }
    if metadata.is_dir() != directory {
        return Err(if directory {
            "host bind source must be a directory".into()
        } else {
            "host bind source must be a regular file".into()
        });
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
        && matches!(
            service,
            "backend" | "backend-volume-init" | "frontend" | "postgres" | "proxy" | "updater"
        )
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

/// Authorize `networks/{id}/connect|disconnect` against the exact production
/// service-to-network topology.
async fn authorize_network_mutation(
    state: &GuardState,
    network: &str,
    container: &str,
    endpoint: Option<&Value>,
) -> std::result::Result<(), String> {
    let container_inspect = daemon_json(
        &state.config.socket_path,
        &format!("/containers/{container}/json"),
    )
    .await
    .map_err(|e| {
        warn!(container, err = %e, "docker guard could not authorize container");
        "container authorization failed".to_string()
    })?;
    let service = managed_project_service(&container_inspect, &state.config)
        .ok_or_else(|| "container is not a managed service in this Compose project".to_string())?;

    let network_inspect = daemon_json(&state.config.socket_path, &format!("/networks/{network}"))
        .await
        .map_err(|e| {
            warn!(network, err = %e, "docker guard could not authorize network");
            "network authorization failed".to_string()
        })?;
    let network_name = allowlisted_network_name(&network_inspect, &state.config)
        .ok_or_else(|| "network is outside the Myriad allowlist".to_string())?;

    authorize_guard_network_attachment(&service, &network_name, &state.config)?;
    if let Some(endpoint) = endpoint {
        validate_endpoint_settings(&service, endpoint, &state.config)?;
    }
    Ok(())
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
                expected_guard_image: format!(
                    "{TRUSTED_GUARD_REPOSITORY}@sha256:{}",
                    "a".repeat(64)
                ),
                host_policy_path: "/etc/myriad/docker-guard.env".into(),
                allow_unpinned_dev: false,
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
        }
    }

    #[tokio::test]
    async fn compromised_updater_cannot_schedule_privileged_self_update() {
        let request = Request::builder()
            .method(Method::POST)
            .uri("/_myriad/self-update")
            .header("X-Update-Token", "attacker-knows-the-former-shared-token")
            .body(Body::from(
                r#"{"previous_tag":"v0.3.28","target_tag":"v9.9.9"}"#,
            ))
            .unwrap();
        let response = handle(
            State(state()),
            ConnectInfo("172.30.0.9:50000".parse().unwrap()),
            request,
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn guard_identity_requires_trusted_repository_and_exact_digest() {
        let valid = format!(
            "{TRUSTED_GUARD_REPOSITORY}@sha256:{}",
            "0123456789abcdef".repeat(4)
        );
        assert!(validate_guard_image_ref(&valid, false).is_ok());
        assert!(validate_guard_image_ref("evil.example/guard@sha256:aaaaaaaa", false).is_err());
        assert!(
            validate_guard_image_ref("docker.io/somekawahitomi/myriad-updater:latest", false)
                .is_err()
        );
        assert!(validate_guard_image_ref(
            &format!("{TRUSTED_GUARD_REPOSITORY}@sha256:{}", "g".repeat(64)),
            false
        )
        .is_err());
        assert!(validate_guard_image_ref("myriad-updater-dev:v0.0.0-dev", true).is_ok());
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

    fn backend_volume_init_create(user: &str, init_env: &str, host: Value) -> Bytes {
        Bytes::from(
            serde_json::to_vec(&json!({
                "Image": "docker.io/example/backend:v1",
                "User": user,
                "Env": [
                    "DATABASE_URL=postgres://example",
                    init_env,
                ],
                "Labels": {
                    "com.docker.compose.project": "myriad",
                    "com.docker.compose.service": "backend-volume-init",
                    "com.docker.compose.oneoff": "True",
                },
                "HostConfig": host,
            }))
            .unwrap(),
        )
    }

    #[test]
    fn backend_volume_init_allows_only_narrow_root_one_off() {
        let allowed = backend_volume_init_create(
            "0:0",
            "MYRIAD_VOLUME_INIT_ONLY=true",
            json!({
                "AutoRemove": false,
                "Binds": [
                    "myriad_backend_cache:/app/cache:rw",
                    "myriad_backend_data:/app/data:rw"
                ],
                "NetworkMode": "none",
                "SecurityOpt": ["no-new-privileges:true"]
            }),
        );
        assert!(validate_container_create(&state(), &allowed).is_ok());

        let missing_flag = backend_volume_init_create(
            "0:0",
            "MYRIAD_VOLUME_INIT_ONLY=false",
            json!({
                "AutoRemove": true,
                "Binds": [
                    "myriad_backend_cache:/app/cache:rw",
                    "myriad_backend_data:/app/data:rw"
                ],
                "SecurityOpt": ["no-new-privileges:true"]
            }),
        );
        assert!(validate_container_create(&state(), &missing_flag)
            .unwrap_err()
            .contains("narrow root init mode"));

        let auto_remove_payload = backend_volume_init_create(
            "0:0",
            "MYRIAD_VOLUME_INIT_ONLY=true",
            json!({
                "AutoRemove": true,
                "Binds": [
                    "myriad_backend_cache:/app/cache:rw",
                    "myriad_backend_data:/app/data:rw"
                ],
                "SecurityOpt": ["no-new-privileges:true"]
            }),
        );
        assert!(validate_container_create(&state(), &auto_remove_payload)
            .unwrap_err()
            .contains("narrow root init mode"));

        let missing_data_volume = backend_volume_init_create(
            "0:0",
            "MYRIAD_VOLUME_INIT_ONLY=true",
            json!({
                "AutoRemove": false,
                "Binds": ["myriad_backend_cache:/app/cache:rw"],
                "SecurityOpt": ["no-new-privileges:true"]
            }),
        );
        assert!(validate_container_create(&state(), &missing_data_volume)
            .unwrap_err()
            .contains("narrow root init mode"));

        let arbitrary_root_backend = backend_volume_init_create(
            "root:root",
            "MYRIAD_VOLUME_INIT_ONLY=true",
            json!({
                "AutoRemove": false,
                "Binds": [
                    "myriad_backend_cache:/app/cache:rw",
                    "myriad_backend_data:/app/data:rw"
                ],
                "SecurityOpt": ["no-new-privileges:true"]
            }),
        );
        assert!(validate_container_create(&state(), &arbitrary_root_backend)
            .unwrap_err()
            .contains("narrow root init mode"));

        let networked_initializer = backend_volume_init_create(
            "0:0",
            "MYRIAD_VOLUME_INIT_ONLY=true",
            json!({
                "AutoRemove": false,
                "Binds": [
                    "myriad_backend_cache:/app/cache:rw",
                    "myriad_backend_data:/app/data:rw"
                ],
                "NetworkMode": "myriad-net",
                "SecurityOpt": ["no-new-privileges:true"]
            }),
        );
        assert!(validate_container_create(&state(), &networked_initializer)
            .unwrap_err()
            .contains("narrow root init mode"));

        let non_root_initializer = backend_volume_init_create(
            "1000:1000",
            "MYRIAD_VOLUME_INIT_ONLY=true",
            json!({
                "AutoRemove": false,
                "Binds": [
                    "myriad_backend_cache:/app/cache:rw",
                    "myriad_backend_data:/app/data:rw"
                ],
                "NetworkMode": "none",
                "SecurityOpt": ["no-new-privileges:true"]
            }),
        );
        assert!(validate_container_create(&state(), &non_root_initializer)
            .unwrap_err()
            .contains("narrow root init mode"));

        let regular_root_backend = Bytes::from(
            serde_json::to_vec(&json!({
                "Image": "docker.io/example/backend:v1",
                "User": "0:0",
                "Env": ["MYRIAD_VOLUME_INIT_ONLY=true"],
                "Labels": {
                    "com.docker.compose.project": "myriad",
                    "com.docker.compose.service": "backend",
                    "com.docker.compose.oneoff": "True"
                },
                "HostConfig": {
                    "AutoRemove": false,
                    "Binds": [
                        "myriad_backend_cache:/app/cache:rw",
                        "myriad_backend_data:/app/data:rw"
                    ],
                    "NetworkMode": "none",
                    "SecurityOpt": ["no-new-privileges:true"]
                }
            }))
            .unwrap(),
        );
        assert!(validate_container_create(&state(), &regular_root_backend)
            .unwrap_err()
            .contains("explicit root user"));
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
    fn host_runtime_and_daemon_file_write_overrides_are_denied() {
        for host in [
            json!({"Runtime": "custom-root-runtime"}),
            json!({"ContainerIDFile": "/etc/cron.d/escape"}),
            json!({"CgroupParent": "/system.slice"}),
            json!({"Annotations": {"run.oci.handler": "host-runtime"}}),
            json!({"LogConfig": {"Type": "syslog", "Config": {}}}),
        ] {
            let body = create("frontend", "docker.io/example/frontend:v1", host);
            assert!(validate_container_create(&state(), &body).is_err());
        }
    }

    #[test]
    fn updater_accepts_only_read_only_root_and_fixed_writable_overlays() {
        let visible = tempfile::tempdir().unwrap();
        std::fs::write(visible.path().join(".env"), "MYRIAD_TAG=v1\n").unwrap();
        std::fs::create_dir(visible.path().join("state")).unwrap();
        std::fs::create_dir(visible.path().join("pgdata")).unwrap();
        let state = state_with_visible_root(visible.path().to_path_buf());
        let good = create(
            "updater",
            "docker.io/example/updater:v1",
            json!({"Binds": [
                "/srv/myriad:/host/compose:ro",
                "/srv/myriad/.env:/host/compose/.env:rw",
                "/srv/myriad/state:/host/compose/state:rw",
                "/srv/myriad/pgdata:/host/compose/pgdata:rw",
                "/etc/myriad/docker-guard.env:/run/secrets/docker-guard.env:ro"
            ]}),
        );
        assert!(validate_container_create(&state, &good).is_ok());

        let writable_root = create(
            "updater",
            "docker.io/example/updater:v1",
            json!({"Binds": ["/srv/myriad:/host/compose:rw"]}),
        );
        assert!(validate_container_create(&state, &writable_root)
            .unwrap_err()
            .contains("read-only"));

        let writable_policy = create(
            "updater",
            "docker.io/example/updater:v1",
            json!({"Binds": [
                "/etc/myriad/docker-guard.env:/run/secrets/docker-guard.env:rw"
            ]}),
        );
        assert!(validate_container_create(&state, &writable_policy)
            .unwrap_err()
            .contains("read-only"));

        let socket = create(
            "updater",
            "docker.io/example/updater:v1",
            json!({"Binds": ["/var/run/docker.sock:/var/run/docker.sock"]}),
        );
        assert!(validate_container_create(&state, &socket).is_err());
    }

    #[test]
    fn repo_digest_match_accepts_docker_hub_canonicalization_only() {
        let expected = format!(
            "docker.io/somekawahitomi/myriad-updater@sha256:{}",
            "a".repeat(64)
        );
        let canonical = format!("somekawahitomi/myriad-updater@sha256:{}", "a".repeat(64));
        assert!(digest_reference_matches(&canonical, &expected));
        assert!(!digest_reference_matches(
            &format!("attacker/myriad-updater@sha256:{}", "a".repeat(64)),
            &expected
        ));
        assert!(!digest_reference_matches(
            &format!("somekawahitomi/myriad-updater@sha256:{}", "b".repeat(64)),
            &expected
        ));
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
    fn initializer_logs_remain_project_scoped_read_only_access() {
        let request =
            Uri::from_static("/v1.51/containers/init-container-id/logs?stdout=1&stderr=1");
        assert_eq!(
            classify_request(&state(), &Method::GET, &request, &Bytes::new()).unwrap(),
            Decision::ProjectContainer("init-container-id".into())
        );
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

        let restore_version_ref = Uri::from_static("/v1.51/images/docker.io%2Fexample%2Fbackend:myriad-rollback/tag?repo=docker.io%2Fexample%2Fbackend&tag=v1");
        assert!(
            classify_request(&state(), &Method::POST, &restore_version_ref, &Bytes::new()).is_ok()
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

    fn create_with_networking(service: &str, image: &str, host: Value, endpoints: Value) -> Bytes {
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
        assert!(validate_container_create(&s, &foreign)
            .unwrap_err()
            .contains("outside the Myriad allowlist"));
    }

    #[test]
    fn only_updater_may_use_guard_network_mode() {
        let s = state();
        let backend = create(
            "backend",
            "docker.io/example/backend:v1",
            json!({"NetworkMode": "myriad-docker-guard-net"}),
        );
        assert!(validate_container_create(&s, &backend)
            .unwrap_err()
            .contains("only the updater service may attach to the docker-guard network"));

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

        assert!(authorize_guard_network_attachment("backend", "myriad-net", &s.config).is_ok());
        assert!(authorize_guard_network_attachment("postgres", "myriad-net", &s.config).is_ok());

        let updater = create_with_networking(
            "updater",
            "docker.io/example/updater:v1",
            json!({}),
            json!({"myriad-net": {}}),
        );
        assert!(validate_container_create(&s, &updater).is_err());
        assert!(authorize_guard_network_attachment("updater", "myriad-net", &s.config).is_err());
    }

    #[test]
    fn endpoint_identity_overrides_are_denied() {
        let s = state();
        assert!(validate_endpoint_settings(
            "backend",
            &json!({"Aliases": ["backend", "myriad-backend"]}),
            &s.config,
        )
        .is_ok());
        for endpoint in [
            json!({"Aliases": ["postgres"]}),
            json!({"IPAMConfig": {"IPv4Address": "172.28.0.2"}}),
            json!({"MacAddress": "02:42:ac:1c:00:02"}),
            json!({"DriverOpts": {"com.example.host": "true"}}),
            json!({"DNSNames": ["postgres"]}),
        ] {
            assert!(validate_endpoint_settings("backend", &endpoint, &s.config).is_err());
        }
    }

    #[test]
    fn guard_network_attachment_policy_is_updater_only() {
        let s = state();
        assert!(authorize_guard_network_attachment(
            "backend",
            "myriad-docker-guard-net",
            &s.config
        )
        .is_err());
        assert!(authorize_guard_network_attachment(
            "frontend",
            "myriad-docker-guard-net",
            &s.config
        )
        .is_err());
        assert!(authorize_guard_network_attachment(
            "postgres",
            "myriad-docker-guard-net",
            &s.config
        )
        .is_err());
        assert!(authorize_guard_network_attachment(
            "updater",
            "myriad-docker-guard-net",
            &s.config
        )
        .is_ok());
    }

    #[test]
    fn network_connect_classify_requires_container_field() {
        let uri = Uri::from_static("/v1.51/networks/myriad-docker-guard-net/connect");
        let missing = classify_request(&state(), &Method::POST, &uri, &Bytes::from_static(b"{}"));
        assert!(missing.unwrap_err().contains("Container"));

        let body =
            Bytes::from(serde_json::to_vec(&json!({"Container": "myriad-backend-1"})).unwrap());
        let decision = classify_request(&state(), &Method::POST, &uri, &body).unwrap();
        assert_eq!(
            decision,
            Decision::ProjectNetworkMutation {
                network: "myriad-docker-guard-net".into(),
                container: "myriad-backend-1".into(),
                endpoint: None,
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
