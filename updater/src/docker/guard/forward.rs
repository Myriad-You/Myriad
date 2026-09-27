//! Request handling and daemon forwarding after a policy [`Decision`].

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, anyhow};
use axum::body::{Body, to_bytes};
use axum::extract::{ConnectInfo, State};
use axum::http::{Method, Request, StatusCode, Uri, header};
use axum::response::Response;
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use serde_json::Value;
use tokio::net::UnixStream;
use tracing::{error, warn};

use super::classify::{Decision, classify_request};
use super::self_update::handle_self_update;
use super::validate::{
    allowlisted_network_name, authorize_guard_network_attachment, managed_project_service,
    managed_project_service_for_logs, validate_endpoint_settings,
};
use super::{
    DOCKER_API_TIMEOUT, GuardState, MAX_CONTAINER_LOG_BODY, SELF_UPDATE_GATE, denial,
    strip_api_version, validate_identifier,
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
    let container_logs_request = matches!(&decision, Decision::ProjectContainerLogs(_));
    match decision {
        Decision::Allow => {}
        Decision::ProjectContainer(container) => {
            match container_belongs_to_project(&state, &container).await {
                Ok(None) => {
                    // Authoritative absence is not an authorization failure.
                    // Return now; never forward a mutation after a missing inspect.
                    return denial(StatusCode::NOT_FOUND, "container does not exist");
                }
                Ok(Some(true)) => {}
                Ok(Some(false)) => {
                    return denial(
                        StatusCode::FORBIDDEN,
                        "container is not a managed service in this Compose project",
                    );
                }
                Err(e) => {
                    warn!(container, err = %e, "docker guard could not authorize container");
                    return denial(StatusCode::FORBIDDEN, "container authorization failed");
                }
            }
        }
        Decision::ProjectContainerLogs(container) => {
            match container_logs_belong_to_project(&state, &container).await {
                Ok(None) => {
                    // Authoritative absence is not an authorization failure.
                    // Return now; never forward a mutation after a missing inspect.
                    return denial(StatusCode::NOT_FOUND, "container does not exist");
                }
                Ok(Some(true)) => {}
                Ok(Some(false)) => {
                    return denial(
                        StatusCode::FORBIDDEN,
                        "container logs are not from a managed service in this Compose project",
                    );
                }
                Err(e) => {
                    warn!(container, err = %e, "docker guard could not authorize container logs");
                    return denial(StatusCode::FORBIDDEN, "container log authorization failed");
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
    let _log_read_permit = if container_logs_request {
        match state.log_read_gate.clone().try_acquire_owned() {
            Ok(permit) => Some(permit),
            Err(_) => {
                return denial(
                    StatusCode::TOO_MANY_REQUESTS,
                    "too many concurrent Docker log reads",
                );
            }
        }
    } else {
        None
    };

    let req = Request::from_parts(parts, Body::from(body));
    match forward(&state.config.socket_path, req).await {
        Ok(resp) if container_logs_request => bound_container_log_response(resp).await,
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

async fn bound_container_log_response(resp: Response) -> Response {
    let (mut parts, body) = resp.into_parts();
    // The body is re-framed from a buffer, so the daemon's framing headers no longer
    // describe it. Docker streams logs chunked; kept, hyper drops the response and
    // the client sees the connection close before any byte.
    parts.headers.remove(header::TRANSFER_ENCODING);
    parts.headers.remove(header::CONTENT_LENGTH);
    match tokio::time::timeout(
        DOCKER_API_TIMEOUT,
        to_bytes(body, MAX_CONTAINER_LOG_BODY),
    )
    .await
    {
        Ok(Ok(body)) => Response::from_parts(parts, Body::from(body)),
        Ok(Err(_)) => denial(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Docker log response exceeds 4 MiB",
        ),
        Err(_) => denial(
            StatusCode::REQUEST_TIMEOUT,
            "Docker log response timed out",
        ),
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

async fn container_belongs_to_project(state: &GuardState, id: &str) -> Result<Option<bool>> {
    let value =
        daemon_json_if_present(&state.config.socket_path, &format!("/containers/{id}/json"))
            .await?;
    Ok(value.map(|value| managed_project_service(&value, &state.config).is_some()))
}

async fn container_logs_belong_to_project(state: &GuardState, id: &str) -> Result<Option<bool>> {
    let value =
        daemon_json_if_present(&state.config.socket_path, &format!("/containers/{id}/json"))
            .await?;
    Ok(value.map(|value| managed_project_service_for_logs(&value, &state.config).is_some()))
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
    daemon_json_if_present(socket, path)
        .await?
        .ok_or_else(|| anyhow!("docker inspect returned 404 Not Found"))
}

async fn daemon_json_if_present(socket: &Path, path: &str) -> Result<Option<Value>> {
    tokio::time::timeout(DOCKER_API_TIMEOUT, async {
        let req = Request::builder()
            .method(Method::GET)
            .uri(path)
            .header(header::HOST, "localhost")
            .body(Body::empty())?;
        let resp = forward(socket, req).await?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            return Err(anyhow!("docker inspect returned {}", resp.status()));
        }
        let body = to_bytes(resp.into_body(), MAX_INSPECT_BODY).await?;
        Ok(Some(serde_json::from_slice(&body)?))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn chunked_daemon_log_response_reaches_the_client() {
        use std::future::IntoFuture;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        // Real daemons answer logs with `Transfer-Encoding: chunked`.
        let app = axum::Router::new().fallback(|| async {
            let mut response = Response::new(Body::from("docker log frame"));
            response.headers_mut().insert(
                header::TRANSFER_ENCODING,
                header::HeaderValue::from_static("chunked"),
            );
            bound_container_log_response(response).await
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(axum::serve(listener, app).into_future());
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream
            .write_all(b"GET /containers/id/logs HTTP/1.1\r\nHost: guard\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut raw = Vec::new();
        tokio::time::timeout(std::time::Duration::from_secs(5), stream.read_to_end(&mut raw))
            .await
            .unwrap()
            .unwrap();
        let raw = String::from_utf8_lossy(&raw);
        assert!(raw.starts_with("HTTP/1.1 200 OK"), "{raw}");
        assert!(raw.contains("content-length: 16"), "{raw}");
        assert!(raw.ends_with("docker log frame"), "{raw}");
    }

    #[tokio::test]
    async fn container_log_response_is_bounded() {
        let response = Response::new(Body::from(vec![0; MAX_CONTAINER_LOG_BODY + 1]));
        let response = bound_container_log_response(response).await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
