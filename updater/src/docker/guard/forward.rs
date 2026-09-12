//! Request handling and daemon forwarding after a policy [`Decision`].

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, State};
use axum::http::{header, Method, Request, StatusCode, Uri};
use axum::response::Response;
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use serde_json::Value;
use tokio::net::UnixStream;
use tracing::{error, warn};

use super::classify::{classify_request, Decision};
use super::self_update::handle_self_update;
use super::validate::{
    allowlisted_network_name, authorize_guard_network_attachment, managed_project_service,
    validate_endpoint_settings,
};
use super::{
    denial, strip_api_version, validate_identifier, GuardState, DOCKER_API_TIMEOUT,
    SELF_UPDATE_GATE,
};

const MAX_REQUEST_BODY: usize = 1024 * 1024;
const MAX_INSPECT_BODY: usize = 2 * 1024 * 1024;

pub(crate) async fn handle(
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
    let body = match tokio::time::timeout(DOCKER_API_TIMEOUT, to_bytes(body, MAX_REQUEST_BODY))
        .await
    {
        Ok(Ok(body)) => body,
        Ok(Err(_)) => return denial(StatusCode::PAYLOAD_TOO_LARGE, "request body exceeds 1 MiB"),
        Err(_) => return denial(StatusCode::REQUEST_TIMEOUT, "request body read timed out"),
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

    let mutation_lease = if requires_mutation_lease(&method, &uri) {
        match GenericMutationLease::acquire(state.mutation_gate.clone()) {
            Ok(lease) => Some(lease),
            Err(reason) => return denial(StatusCode::CONFLICT, &reason),
        }
    } else {
        None
    };

    let req = Request::from_parts(parts, Body::from(body));
    match forward(&state.config.socket_path, req).await {
        Ok(mut resp) => {
            if let Some(lease) = mutation_lease {
                resp.extensions_mut().insert(lease);
            }
            resp
        }
        Err(e) => {
            error!(err = %e, "docker guard upstream failure");
            denial(StatusCode::BAD_GATEWAY, "docker daemon unavailable")
        }
    }
}

fn requires_mutation_lease(method: &Method, uri: &Uri) -> bool {
    if !matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) {
        return false;
    }
    let path = strip_api_version(uri.path());
    // Docker models container wait as POST, but it is observation-only and may
    // legitimately stream until a container exits.
    !path.ends_with("/wait")
}

#[derive(Clone, Debug)]
pub(crate) struct GenericMutationLease {
    _inner: Arc<GenericMutationLeaseInner>,
}

#[derive(Debug)]
struct GenericMutationLeaseInner {
    gate: Arc<AtomicUsize>,
}

impl GenericMutationLease {
    pub(crate) fn acquire(gate: Arc<AtomicUsize>) -> std::result::Result<Self, String> {
        loop {
            let current = gate.load(Ordering::SeqCst);
            if current & SELF_UPDATE_GATE != 0 {
                return Err("Docker mutations are paused during trusted self-update".into());
            }
            if current == SELF_UPDATE_GATE - 1 {
                return Err("too many concurrent Docker mutations".into());
            }
            if gate
                .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return Ok(Self {
                    _inner: Arc::new(GenericMutationLeaseInner { gate }),
                });
            }
        }
    }
}

impl Drop for GenericMutationLeaseInner {
    fn drop(&mut self) {
        self.gate.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn container_belongs_to_project(state: &GuardState, id: &str) -> Result<bool> {
    let value = daemon_json(&state.config.socket_path, &format!("/containers/{id}/json")).await?;
    Ok(managed_project_service(&value, &state.config).is_some())
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

pub(crate) async fn discover_host_compose_root(
    socket: &Path,
    container_id: &str,
) -> Result<PathBuf> {
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

pub(crate) async fn daemon_json(socket: &Path, path: &str) -> Result<Value> {
    tokio::time::timeout(DOCKER_API_TIMEOUT, async {
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
    })
    .await
    .context("Docker API inspection timed out")?
}

pub(crate) async fn forward(socket: &Path, mut req: Request<Body>) -> Result<Response> {
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
