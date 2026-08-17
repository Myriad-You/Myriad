//! Policy-enforcing proxy between the updater and the host Docker daemon.
//!
//! This is intentionally narrower than a generic socket proxy: mutations are limited to
//! containers in one Compose project, and container-create request bodies are validated before
//! reaching the daemon. The updater never receives the raw Unix socket.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
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

use crate::api::auth::constant_time_eq;
use crate::config::{SecretString, GUARD_SELF_UPDATE_TOKEN_MIN_LEN};
use crate::version::{DeployTag, DeployTagKind, MyriadVersion};

const MAX_REQUEST_BODY: usize = 1024 * 1024;
const MAX_INSPECT_BODY: usize = 2 * 1024 * 1024;
const TRUSTED_GUARD_REPOSITORY: &str = "docker.io/somekawahitomi/myriad-updater";
const TRUSTED_UPDATER_REPOSITORY: &str = TRUSTED_GUARD_REPOSITORY;
const SELF_UPDATE_HELPER_NAME: &str = "myriad-tcb-self-update";
const SELF_UPDATE_RECOVERY_NAME: &str = "myriad-tcb-self-update-recovery";
const SELF_UPDATE_EXHAUSTED_NAME: &str = "myriad-tcb-self-update-recovery-exhausted";
const DOCKER_API_TIMEOUT: Duration = Duration::from_secs(30);
const TRUSTED_PULL_TIMEOUT: Duration = Duration::from_secs(180);
const HELPER_LAUNCH_TIMEOUT: Duration = Duration::from_secs(30);
const HELPER_TOTAL_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const SELF_UPDATE_GATE: usize = 1usize << (usize::BITS - 1);
const SELF_UPDATE_TOKEN_HEADER: &str = "x-guard-self-update-token";

#[derive(Debug)]
struct HandoffCleanupUnconfirmed;

impl std::fmt::Display for HandoffCleanupUnconfirmed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("self-update helper cleanup could not be confirmed")
    }
}

impl std::error::Error for HandoffCleanupUnconfirmed {}

#[derive(Debug, Clone)]
struct HandoffAttempt {
    previous_image: String,
    target_image: String,
    previous_tag: String,
    target_tag: String,
    recovery_only: bool,
}

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
    pub state_dir: PathBuf,
    pub expected_guard_image: String,
    pub host_policy_path: String,
    pub self_update_token: SecretString,
    pub allow_unpinned_dev: bool,
    pub allowed_images: HashSet<String>,
    pub service_images: HashMap<String, String>,
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
        let state_dir = std::env::var("DOCKER_GUARD_STATE_DIR")
            .unwrap_or_else(|_| "/host/state".into())
            .into();
        let expected_guard_image = std::env::var("DOCKER_GUARD_EXPECTED_IMAGE")
            .context("DOCKER_GUARD_EXPECTED_IMAGE is required")?;
        let host_policy_path = std::env::var("DOCKER_GUARD_HOST_POLICY_PATH")
            .context("DOCKER_GUARD_HOST_POLICY_PATH is required")?;
        validate_host_policy_path(&host_policy_path)?;
        let self_update_token = std::env::var("DOCKER_GUARD_SELF_UPDATE_TOKEN")
            .context("DOCKER_GUARD_SELF_UPDATE_TOKEN is required")?;
        validate_self_update_token(&self_update_token)?;
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
        let service_images = [
            ("backend", "docker.io/somekawahitomi/myriad-backend"),
            (
                "backend-volume-init",
                "docker.io/somekawahitomi/myriad-backend",
            ),
            ("frontend", "docker.io/somekawahitomi/myriad-frontend"),
            ("proxy", "docker.io/somekawahitomi/myriad-proxy"),
            ("postgres", "postgres"),
        ]
        .into_iter()
        .map(|(service, repository)| (service.to_string(), repository.to_string()))
        .collect::<HashMap<_, _>>();
        let allowed_images = service_images.values().cloned().collect::<HashSet<_>>();

        Ok(Self {
            listen,
            socket_path,
            project,
            compose_network,
            admin_network,
            guard_network,
            compose_dir,
            state_dir,
            expected_guard_image,
            host_policy_path,
            self_update_token: SecretString::new(self_update_token),
            allow_unpinned_dev,
            allowed_images,
            service_images,
        })
    }
}

fn validate_self_update_token(token: &str) -> Result<()> {
    if token.len() < GUARD_SELF_UPDATE_TOKEN_MIN_LEN || token.len() > 256 {
        return Err(anyhow!(
            "DOCKER_GUARD_SELF_UPDATE_TOKEN must be 32..=256 characters"
        ));
    }
    if !token.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(anyhow!(
            "DOCKER_GUARD_SELF_UPDATE_TOKEN must contain only printable non-whitespace ASCII"
        ));
    }
    Ok(())
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
    mutation_gate: Arc<AtomicUsize>,
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
        mutation_gate: Arc::new(AtomicUsize::new(0)),
    };
    let recovery_exhausted =
        helper_container_exists(&state.config.socket_path, SELF_UPDATE_EXHAUSTED_NAME).await?;
    let residual_helper = if recovery_exhausted {
        None
    } else if helper_container_exists(&state.config.socket_path, SELF_UPDATE_RECOVERY_NAME).await? {
        Some((SELF_UPDATE_RECOVERY_NAME, true))
    } else if helper_container_exists(&state.config.socket_path, SELF_UPDATE_HELPER_NAME).await? {
        Some((SELF_UPDATE_HELPER_NAME, false))
    } else {
        None
    };
    if recovery_exhausted {
        state
            .mutation_gate
            .store(SELF_UPDATE_GATE, Ordering::SeqCst);
        error!(
            helper = SELF_UPDATE_EXHAUSTED_NAME,
            "previous-digest recovery retries are exhausted; host recovery is required"
        );
        fail_exhausted_pending_handoff(&state);
    } else if let Some((helper_name, recovery_only)) = residual_helper {
        state
            .mutation_gate
            .store(SELF_UPDATE_GATE, Ordering::SeqCst);
        warn!(
            helper = helper_name,
            "recovering mutation gate for an in-flight self-update helper"
        );
        let attempt = match inspect_handoff_attempt(&state.config.socket_path, helper_name).await {
            Ok(attempt) => Some(attempt),
            Err(error) => {
                warn!(%error, "could not recover trusted handoff intent from helper");
                None
            }
        };
        if attempt
            .as_ref()
            .is_some_and(|attempt| attempt.recovery_only != recovery_only)
        {
            warn!(
                helper = helper_name,
                "trusted helper name and mode disagree"
            );
        }
        let recovered_retries = if recovery_only {
            attempt
                .as_ref()
                .map(|attempt| recovery_attempt_from_status(&state, attempt))
                .unwrap_or(0)
        } else {
            0
        };
        let mut monitor_ready = true;
        let mut durable_exhausted = false;
        if recovery_only {
            let docker_host = format!("unix://{}", state.config.socket_path.display());
            let normal_absent =
                if helper_container_exists(&state.config.socket_path, SELF_UPDATE_HELPER_NAME)
                    .await
                    .unwrap_or(true)
                {
                    stop_helper(&docker_host, SELF_UPDATE_HELPER_NAME).await
                        && cleanup_helper(&docker_host, SELF_UPDATE_HELPER_NAME).await
                        && wait_for_helper_absence(
                            &state.config.socket_path,
                            SELF_UPDATE_HELPER_NAME,
                            Duration::from_secs(30),
                        )
                        .await
                } else {
                    true
                };
            let recovery_running = helper_container_running(&state.config.socket_path, helper_name)
                .await
                .unwrap_or(false);
            let recovery_exit = helper_container_exit_code(&state.config.socket_path, helper_name)
                .await
                .ok()
                .flatten();
            let recovery_succeeded = recovery_exit == Some(0);
            durable_exhausted = recovery_is_durably_exhausted(
                recovery_exit,
                recovered_retries,
                attempt
                    .as_ref()
                    .is_some_and(|attempt| failed_status_matches_attempt(&state, attempt)),
            );
            if durable_exhausted {
                let _ = mark_recovery_exhausted(&docker_host, helper_name).await;
                error!(
                    helper = helper_name,
                    "durable recovery failure is exhausted; retaining mutation gate"
                );
                monitor_ready = false;
            } else if !normal_absent
                || (!recovery_running
                    && !recovery_succeeded
                    && !restart_helper(&docker_host, helper_name).await)
            {
                error!(
                    helper = helper_name,
                    "could not resume staged previous-digest recovery; retaining mutation gate"
                );
                monitor_ready = false;
            }
        } else {
            let docker_host = format!("unix://{}", state.config.socket_path.display());
            let helper_running = helper_container_running(&state.config.socket_path, helper_name)
                .await
                .unwrap_or(false);
            let helper_completed =
                helper_container_exit_code(&state.config.socket_path, helper_name)
                    .await
                    .ok()
                    .flatten()
                    .is_some();
            if !helper_running
                && !helper_completed
                && !restart_helper(&docker_host, helper_name).await
            {
                error!(
                    helper = helper_name,
                    "could not resume staged trusted handoff; retaining mutation gate"
                );
                monitor_ready = false;
            }
        }
        if monitor_ready {
            monitor_handoff(
                state.clone(),
                helper_name.into(),
                attempt,
                recovery_only,
                recovered_retries,
            );
        } else if !durable_exhausted {
            if let Some(attempt) = attempt {
                resume_staged_recovery(state.clone(), attempt, recovered_retries);
            }
        }
    } else {
        let _ = finalize_or_fail_orphaned_pending_handoff(&state).await;
    }

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

fn fail_exhausted_pending_handoff(state: &GuardState) {
    let path = state.config.state_dir.join("self-update-last.json");
    let Some(pending) = std::fs::read(&path).ok().and_then(|bytes| {
        serde_json::from_slice::<super::self_update_helper::SelfUpdateLastStatus>(&bytes).ok()
    }) else {
        return;
    };
    if !matches!(
        pending.status,
        super::self_update_helper::SelfUpdateOutcome::Pending
    ) {
        return;
    }
    let failed = super::self_update_helper::SelfUpdateLastStatus::failed_before_handoff(
        pending.target_tag,
        pending.previous_tag,
        "previous-digest recovery retries are exhausted; host recovery is required".into(),
    );
    if let Err(error) = super::self_update_helper::write_status(&path, &failed) {
        warn!(%error, "could not persist exhausted self-update outcome");
    }
}

async fn finalize_or_fail_orphaned_pending_handoff(state: &GuardState) -> bool {
    let path = state.config.state_dir.join("self-update-last.json");
    let Some(pending) = std::fs::read(&path).ok().and_then(|bytes| {
        serde_json::from_slice::<super::self_update_helper::SelfUpdateLastStatus>(&bytes).ok()
    }) else {
        return false;
    };
    if !matches!(
        pending.status,
        super::self_update_helper::SelfUpdateOutcome::Pending
    ) {
        return false;
    }
    let updater_consistent = verify_managed_tcb_container(
        &state.config.socket_path,
        "myriad-updater",
        "updater",
        &state.config.project,
        &state.config.expected_guard_image,
    )
    .await
    .is_ok();
    let gateway_consistent = verify_managed_tcb_container(
        &state.config.socket_path,
        "myriad-updater-gateway",
        "updater-gateway",
        &state.config.project,
        &state.config.expected_guard_image,
    )
    .await
    .is_ok();
    let running_tag = running_updater_tag(&state.config.socket_path).await.ok();
    let tcb_consistent = updater_consistent && gateway_consistent;
    let status = if tcb_consistent && running_tag.as_deref() == Some(pending.target_tag.as_str()) {
        super::self_update_helper::SelfUpdateLastStatus::succeeded_after_handoff(
            pending.target_tag,
            pending.previous_tag,
        )
    } else {
        if !tcb_consistent {
            state
                .mutation_gate
                .store(SELF_UPDATE_GATE, Ordering::SeqCst);
            error!("orphaned handoff left an inconsistent TCB; retaining mutation gate");
        }
        super::self_update_helper::SelfUpdateLastStatus::failed_before_handoff(
            pending.target_tag,
            pending.previous_tag,
            "trusted handoff was interrupted; target TCB was not fully active".into(),
        )
    };
    if let Err(error) = super::self_update_helper::write_status(&path, &status) {
        warn!(%error, "could not persist interrupted self-update outcome");
    }
    tcb_consistent
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

const MAX_SELF_UPDATE_BODY: usize = 4 * 1024;

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SelfUpdateRequestBody {
    target_tag: String,
    trust_path: String,
}

async fn handle_self_update(state: GuardState, req: Request<Body>) -> Response {
    if req.method() != Method::POST {
        return denial(StatusCode::METHOD_NOT_ALLOWED, "POST required");
    }
    let authorized = req
        .headers()
        .get(SELF_UPDATE_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|provided| {
            constant_time_eq(
                provided.as_bytes(),
                state.config.self_update_token.expose().as_bytes(),
            )
        });
    if !authorized {
        return denial(
            StatusCode::UNAUTHORIZED,
            "valid host-policy self-update capability required",
        );
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
        Ok(request) => request,
        Err(_) => {
            return denial(
                StatusCode::BAD_REQUEST,
                "JSON body requires only target_tag and trust_path",
            )
        }
    };
    if request.trust_path != "dockerhub_tag" {
        return denial(
            StatusCode::BAD_REQUEST,
            "unsupported self-update trust path",
        );
    }
    if let Err(reason) = validate_self_update_tag(&request.target_tag) {
        return denial(StatusCode::BAD_REQUEST, &reason);
    }
    if let Err(reason) = ensure_no_business_update(&state) {
        return denial(StatusCode::CONFLICT, &reason);
    }
    if state
        .mutation_gate
        .compare_exchange(0, SELF_UPDATE_GATE, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return denial(StatusCode::CONFLICT, "self-update already scheduled");
    }

    let target_tag = request.target_tag;
    let task_state = state.clone();
    let task_target = target_tag.clone();
    tokio::spawn(async move {
        let mut reset = SelfUpdateGateReset::new(task_state.mutation_gate.clone());
        match prepare_trusted_self_update(&task_state, &task_target).await {
            Ok((helper_id, attempt)) => {
                info!(
                    %helper_id,
                    exact_image = %attempt.target_image,
                    previous_tag = %attempt.previous_tag,
                    target_tag = %attempt.target_tag,
                    "trusted self-update handoff launched"
                );
                monitor_handoff(task_state.clone(), helper_id, Some(attempt), false, 0);
                reset.disarm();
            }
            Err(error) => {
                let retain_gate = error.downcast_ref::<HandoffCleanupUnconfirmed>().is_some();
                warn!(%error, target_tag = %task_target, "trusted self-update rejected");
                let previous_tag = running_updater_tag(&task_state.config.socket_path)
                    .await
                    .unwrap_or_else(|_| "unknown".into());
                let helper_exists = if retain_gate {
                    helper_container_exists(&task_state.config.socket_path, SELF_UPDATE_HELPER_NAME)
                        .await
                        .ok()
                } else {
                    Some(false)
                };
                if retain_gate && helper_exists != Some(false) {
                    let attempt = inspect_handoff_attempt(
                        &task_state.config.socket_path,
                        SELF_UPDATE_HELPER_NAME,
                    )
                    .await
                    .ok();
                    monitor_handoff(
                        task_state.clone(),
                        SELF_UPDATE_HELPER_NAME.into(),
                        attempt.clone(),
                        attempt
                            .as_ref()
                            .is_some_and(|attempt| attempt.recovery_only),
                        0,
                    );
                    reset.disarm();
                    error!("helper cleanup is unconfirmed; retaining mutation gate");
                } else {
                    let status =
                        super::self_update_helper::SelfUpdateLastStatus::failed_before_handoff(
                            task_target,
                            previous_tag,
                            error.to_string(),
                        );
                    if let Err(status_error) = super::self_update_helper::write_status(
                        &task_state.config.state_dir.join("self-update-last.json"),
                        &status,
                    ) {
                        warn!(%status_error, "could not persist rejected self-update outcome");
                    }
                }
            }
        }
    });

    (
        StatusCode::ACCEPTED,
        axum::Json(json!({
            "scheduled": true,
            "executor": "docker-guard",
            "target_tag": target_tag,
            "services": ["docker-guard", "updater", "updater-gateway"],
        })),
    )
        .into_response()
}

struct SelfUpdateGateReset {
    gate: Arc<AtomicUsize>,
    armed: bool,
}

impl SelfUpdateGateReset {
    fn new(gate: Arc<AtomicUsize>) -> Self {
        Self { gate, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for SelfUpdateGateReset {
    fn drop(&mut self) {
        if self.armed {
            self.gate.store(0, Ordering::SeqCst);
        }
    }
}

#[derive(Clone, Debug)]
struct GenericMutationLease {
    _inner: Arc<GenericMutationLeaseInner>,
}

#[derive(Debug)]
struct GenericMutationLeaseInner {
    gate: Arc<AtomicUsize>,
}

impl GenericMutationLease {
    fn acquire(gate: Arc<AtomicUsize>) -> std::result::Result<Self, String> {
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

fn ensure_no_business_update(state: &GuardState) -> std::result::Result<(), String> {
    let current_job = state.config.state_dir.join("job.current");
    let metadata = match std::fs::symlink_metadata(&current_job) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("cannot verify updater job state".into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("updater job state must be a regular file".into());
    }
    let job = std::fs::read_to_string(current_job)
        .map_err(|_| "cannot verify updater job state".to_string())?;
    if !job.trim().is_empty() {
        return Err("a business update or rollback is still in progress".into());
    }
    Ok(())
}

fn monitor_handoff(
    state: GuardState,
    helper_id: String,
    attempt: Option<HandoffAttempt>,
    recovery_only: bool,
    recovery_retries: u8,
) {
    tokio::spawn(async move {
        let monitor_started = chrono::Utc::now();
        let docker_host = format!("unix://{}", state.config.socket_path.display());
        let mut wait = Command::new("docker");
        wait.env("DOCKER_HOST", &docker_host)
            .args(["container", "wait", &helper_id]);
        let result = tokio::time::timeout(HELPER_TOTAL_TIMEOUT, wait.output()).await;
        let failure = match result {
            Ok(Ok(output)) if output.status.success() => {
                let exit_code = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<i32>()
                    .unwrap_or(-1);
                if exit_code != 0 {
                    warn!(%helper_id, exit_code, "trusted TCB handoff process failed");
                    Some(format!(
                        "trusted self-update helper exited with code {exit_code}"
                    ))
                } else {
                    info!(%helper_id, "trusted TCB handoff process exited");
                    None
                }
            }
            Ok(Ok(output)) => {
                let detail = String::from_utf8_lossy(&output.stderr).into_owned();
                warn!(
                    %helper_id,
                    error = %detail,
                    "could not observe trusted TCB handoff outcome"
                );
                Some(format!("could not observe trusted TCB handoff: {detail}"))
            }
            Ok(Err(error)) => {
                warn!(%helper_id, %error, "could not wait for trusted TCB handoff");
                Some(format!("could not wait for trusted TCB handoff: {error}"))
            }
            Err(_) => {
                warn!(%helper_id, "trusted TCB handoff exceeded its total deadline");
                Some("trusted TCB handoff exceeded its total deadline".into())
            }
        };
        if let (Some(failure), Some(attempt)) = (failure.as_deref(), attempt.as_ref()) {
            if !recovery_only {
                warn!(%failure, "starting fixed previous-digest recovery handoff");
                match launch_trusted_handoff(
                    &state,
                    &attempt.previous_image,
                    &attempt.target_image,
                    &attempt.previous_tag,
                    &attempt.target_tag,
                    true,
                )
                .await
                {
                    Ok(recovery_id) => {
                        let old_stopped = stop_helper(&docker_host, &helper_id).await;
                        let old_removed = old_stopped
                            && cleanup_helper(&docker_host, &helper_id).await
                            && wait_for_helper_absence(
                                &state.config.socket_path,
                                &helper_id,
                                Duration::from_secs(30),
                            )
                            .await;
                        if !old_removed {
                            error!(
                                %helper_id,
                                %recovery_id,
                                "old helper cleanup is unconfirmed; recovery remains staged"
                            );
                            resume_staged_recovery(state, attempt.clone(), 0);
                            return;
                        }
                        if !restart_helper(&docker_host, &recovery_id).await {
                            error!(
                                %recovery_id,
                                "staged previous-digest recovery could not be started"
                            );
                            resume_staged_recovery(state, attempt.clone(), 0);
                            return;
                        }
                        monitor_handoff(state, recovery_id, Some(attempt.clone()), true, 0);
                        return;
                    }
                    Err(error) => {
                        error!(%error, "could not launch fixed previous-digest recovery handoff");
                        resume_staged_recovery(state, attempt.clone(), 0);
                        return;
                    }
                }
            }
        }
        if failure.is_some() && recovery_only && recovery_retries < 2 {
            let next_attempt = recovery_retries + 1;
            let retry_persisted = attempt
                .as_ref()
                .is_some_and(|attempt| persist_recovery_attempt(&state, attempt, next_attempt));
            if retry_persisted
                && stop_helper(&docker_host, &helper_id).await
                && restart_helper(&docker_host, &helper_id).await
            {
                warn!(
                    %helper_id,
                    attempt = next_attempt + 1,
                    "retrying fixed previous-digest recovery handoff"
                );
                monitor_handoff(state, helper_id, attempt, true, next_attempt);
                return;
            }
            if let Some(attempt) = attempt.clone() {
                warn!(
                    %helper_id,
                    "recovery retry control failed; entering background recovery loop"
                );
                resume_staged_recovery(state, attempt, next_attempt);
                return;
            }
        }
        if let Some(failure) = failure {
            if attempt.is_none() {
                warn!(
                    %helper_id,
                    "handoff intent is temporarily unavailable; entering reconciliation loop"
                );
                resume_unidentified_handoff(state, helper_id);
                return;
            }
            let failure_tags = attempt
                .as_ref()
                .map(|attempt| (attempt.target_tag.clone(), attempt.previous_tag.clone()))
                .or_else(|| {
                    std::fs::read(state.config.state_dir.join("self-update-last.json"))
                        .ok()
                        .and_then(|bytes| {
                            serde_json::from_slice::<
                                super::self_update_helper::SelfUpdateLastStatus,
                            >(&bytes)
                            .ok()
                        })
                        .filter(|status| {
                            matches!(
                                status.status,
                                super::self_update_helper::SelfUpdateOutcome::Pending
                            )
                        })
                        .map(|status| (status.target_tag, status.previous_tag))
                });
            if let Some((target_tag, previous_tag)) = failure_tags {
                record_helper_failure_if_missing(
                    &state,
                    &target_tag,
                    &previous_tag,
                    failure,
                    monitor_started,
                );
            }
            if recovery_only
                && recovery_retries >= 2
                && !mark_recovery_exhausted(&docker_host, &helper_id).await
            {
                warn!(
                    %helper_id,
                    "could not persist exhausted recovery identity; durable failure remains"
                );
            }
            error!(
                %helper_id,
                "trusted handoff did not converge; retaining mutation gate"
            );
            return;
        }
        let current_clean = cleanup_helper(&docker_host, &helper_id).await;
        let normal_clean =
            !recovery_only || cleanup_helper(&docker_host, SELF_UPDATE_HELPER_NAME).await;
        if !current_clean || !normal_clean {
            warn!("trusted helper cleanup is pending before gate recovery");
        }
        release_gate_when_helpers_absent(state.clone()).await;
        if let Some(attempt) = attempt {
            let status = if recovery_only {
                super::self_update_helper::SelfUpdateLastStatus::failed_before_handoff(
                    attempt.target_tag,
                    attempt.previous_tag,
                    "trusted handoff terminated; previous TCB restored".into(),
                )
            } else {
                super::self_update_helper::SelfUpdateLastStatus::succeeded_after_handoff(
                    attempt.target_tag,
                    attempt.previous_tag,
                )
            };
            if let Err(error) = super::self_update_helper::write_status(
                &state.config.state_dir.join("self-update-last.json"),
                &status,
            ) {
                warn!(%error, "could not persist final trusted handoff outcome");
            }
        }
    });
}

fn resume_unidentified_handoff(state: GuardState, helper_id: String) {
    tokio::spawn(async move {
        loop {
            match inspect_handoff_attempt(&state.config.socket_path, &helper_id).await {
                Ok(attempt) => {
                    let recovery_only = attempt.recovery_only;
                    let recovery_retries = if recovery_only {
                        recovery_attempt_from_status(&state, &attempt)
                    } else {
                        0
                    };
                    monitor_handoff(
                        state,
                        helper_id,
                        Some(attempt),
                        recovery_only,
                        recovery_retries,
                    );
                    return;
                }
                Err(error) => {
                    match helper_container_exists(&state.config.socket_path, &helper_id).await {
                        Ok(false) => {
                            warn!(%helper_id, %error, "unidentified helper disappeared");
                            if finalize_or_fail_orphaned_pending_handoff(&state).await {
                                release_gate_when_helpers_absent(state).await;
                            } else {
                                error!(
                                    %helper_id,
                                    "orphaned handoff could not be proven consistent; retaining mutation gate"
                                );
                            }
                            return;
                        }
                        Ok(true) => {
                            warn!(%helper_id, %error, "waiting to recover trusted handoff intent");
                        }
                        Err(exists_error) => {
                            warn!(
                                %helper_id,
                                %error,
                                %exists_error,
                                "helper identity remains unknown"
                            );
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

fn resume_staged_recovery(state: GuardState, attempt: HandoffAttempt, recovery_retries: u8) {
    tokio::spawn(async move {
        let docker_host = format!("unix://{}", state.config.socket_path.display());
        loop {
            let recovery_exists =
                helper_container_exists(&state.config.socket_path, SELF_UPDATE_RECOVERY_NAME)
                    .await
                    .unwrap_or(true);
            if !recovery_exists
                && launch_trusted_handoff(
                    &state,
                    &attempt.previous_image,
                    &attempt.target_image,
                    &attempt.previous_tag,
                    &attempt.target_tag,
                    true,
                )
                .await
                .is_err()
            {
                warn!("could not yet stage previous-digest recovery; retrying");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }

            let normal_exists =
                helper_container_exists(&state.config.socket_path, SELF_UPDATE_HELPER_NAME)
                    .await
                    .unwrap_or(true);
            let normal_absent = !normal_exists
                || (stop_helper(&docker_host, SELF_UPDATE_HELPER_NAME).await
                    && cleanup_helper(&docker_host, SELF_UPDATE_HELPER_NAME).await
                    && wait_for_helper_absence(
                        &state.config.socket_path,
                        SELF_UPDATE_HELPER_NAME,
                        Duration::from_secs(30),
                    )
                    .await);
            if recovery_retries > 0 && !persist_recovery_attempt(&state, &attempt, recovery_retries)
            {
                warn!("could not persist recovery retry budget; retrying without execution");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
            let recovery_running =
                helper_container_running(&state.config.socket_path, SELF_UPDATE_RECOVERY_NAME)
                    .await
                    .unwrap_or(false);
            let recovery_succeeded =
                helper_container_exit_code(&state.config.socket_path, SELF_UPDATE_RECOVERY_NAME)
                    .await
                    .ok()
                    .flatten()
                    == Some(0);
            if normal_absent
                && (recovery_running
                    || recovery_succeeded
                    || restart_helper(&docker_host, SELF_UPDATE_RECOVERY_NAME).await)
            {
                monitor_handoff(
                    state,
                    SELF_UPDATE_RECOVERY_NAME.into(),
                    Some(attempt),
                    true,
                    recovery_retries,
                );
                return;
            }
            warn!("staged previous-digest recovery is not ready; retrying");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

fn record_helper_failure_if_missing(
    state: &GuardState,
    target_tag: &str,
    previous_tag: &str,
    error: String,
    not_before: chrono::DateTime<chrono::Utc>,
) {
    let path = state.config.state_dir.join("self-update-last.json");
    let already_recorded = std::fs::read(&path)
        .ok()
        .and_then(|bytes| {
            serde_json::from_slice::<super::self_update_helper::SelfUpdateLastStatus>(&bytes).ok()
        })
        .is_some_and(|status| {
            status.target_tag == target_tag
                && matches!(
                    status.status,
                    super::self_update_helper::SelfUpdateOutcome::Failed
                )
                && chrono::DateTime::parse_from_rfc3339(&status.at)
                    .map(|at| at.with_timezone(&chrono::Utc) >= not_before)
                    .unwrap_or(false)
        });
    if already_recorded {
        return;
    }
    let status = super::self_update_helper::SelfUpdateLastStatus::failed_before_handoff(
        target_tag.to_owned(),
        previous_tag.to_owned(),
        error,
    );
    if let Err(status_error) = super::self_update_helper::write_status(&path, &status) {
        warn!(%status_error, "could not persist helper failure outcome");
    }
}

fn failed_status_matches_attempt(state: &GuardState, attempt: &HandoffAttempt) -> bool {
    std::fs::read(state.config.state_dir.join("self-update-last.json"))
        .ok()
        .and_then(|bytes| {
            serde_json::from_slice::<super::self_update_helper::SelfUpdateLastStatus>(&bytes).ok()
        })
        .is_some_and(|status| {
            matches!(
                status.status,
                super::self_update_helper::SelfUpdateOutcome::Failed
            ) && status.target_tag == attempt.target_tag
                && status.previous_tag == attempt.previous_tag
        })
}

fn recovery_attempt_from_status(state: &GuardState, attempt: &HandoffAttempt) -> u8 {
    std::fs::read(state.config.state_dir.join("self-update-last.json"))
        .ok()
        .and_then(|bytes| {
            serde_json::from_slice::<super::self_update_helper::SelfUpdateLastStatus>(&bytes).ok()
        })
        .filter(|status| {
            status.target_tag == attempt.target_tag && status.previous_tag == attempt.previous_tag
        })
        .map(|status| status.recovery_attempt.min(2))
        .unwrap_or(0)
}

fn recovery_is_durably_exhausted(
    completed_exit: Option<i64>,
    recovery_retries: u8,
    matching_failed_status: bool,
) -> bool {
    completed_exit.is_some_and(|code| code != 0)
        && (recovery_retries >= 2 || matching_failed_status)
}

fn persist_recovery_attempt(
    state: &GuardState,
    attempt: &HandoffAttempt,
    recovery_attempt: u8,
) -> bool {
    let path = state.config.state_dir.join("self-update-last.json");
    let mut status = std::fs::read(&path)
        .ok()
        .and_then(|bytes| {
            serde_json::from_slice::<super::self_update_helper::SelfUpdateLastStatus>(&bytes).ok()
        })
        .filter(|status| {
            status.target_tag == attempt.target_tag && status.previous_tag == attempt.previous_tag
        })
        .unwrap_or_else(|| {
            super::self_update_helper::SelfUpdateLastStatus::pending_before_handoff(
                attempt.target_tag.clone(),
                attempt.previous_tag.clone(),
            )
        });
    status.status = super::self_update_helper::SelfUpdateOutcome::Pending;
    status.recovery_attempt = recovery_attempt.min(2);
    super::self_update_helper::write_status(&path, &status).is_ok()
}

async fn release_gate_when_helpers_absent(state: GuardState) {
    let docker_host = format!("unix://{}", state.config.socket_path.display());
    let mut consecutive_absent = 0u8;
    loop {
        let mut all_absent = true;
        for helper in [SELF_UPDATE_HELPER_NAME, SELF_UPDATE_RECOVERY_NAME] {
            match helper_container_exists(&state.config.socket_path, helper).await {
                Ok(false) => {}
                Ok(true) => {
                    all_absent = false;
                    if !cleanup_helper(&docker_host, helper).await {
                        warn!(%helper, "trusted helper cleanup retry did not complete");
                    }
                }
                Err(error) => {
                    all_absent = false;
                    warn!(%helper, %error, "cannot yet recover self-update mutation gate");
                }
            }
        }
        if all_absent {
            consecutive_absent += 1;
            if consecutive_absent >= 5 {
                state.mutation_gate.store(0, Ordering::SeqCst);
                info!("self-update helpers are stably absent; mutation gate recovered");
                return;
            }
        } else {
            consecutive_absent = 0;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn stop_helper(docker_host: &str, helper_id: &str) -> bool {
    let mut stop = Command::new("docker");
    stop.env("DOCKER_HOST", docker_host)
        .args(["container", "stop", "--time", "10", helper_id]);
    stop.kill_on_drop(true);
    match tokio::time::timeout(HELPER_LAUNCH_TIMEOUT, stop.output()).await {
        Ok(Ok(output)) if output.status.success() => true,
        Ok(Ok(output)) => {
            let error = String::from_utf8_lossy(&output.stderr);
            error.contains("is not running")
        }
        _ => false,
    }
}

async fn mark_recovery_exhausted(docker_host: &str, helper_id: &str) -> bool {
    let mut rename = Command::new("docker");
    rename.env("DOCKER_HOST", docker_host).args([
        "container",
        "rename",
        helper_id,
        SELF_UPDATE_EXHAUSTED_NAME,
    ]);
    rename.kill_on_drop(true);
    matches!(
        tokio::time::timeout(HELPER_LAUNCH_TIMEOUT, rename.output()).await,
        Ok(Ok(output)) if output.status.success()
    )
}

async fn restart_helper(docker_host: &str, helper_id: &str) -> bool {
    let mut start = Command::new("docker");
    start
        .env("DOCKER_HOST", docker_host)
        .args(["container", "start", helper_id]);
    start.kill_on_drop(true);
    matches!(
        tokio::time::timeout(HELPER_LAUNCH_TIMEOUT, start.output()).await,
        Ok(Ok(output)) if output.status.success()
    )
}

async fn cleanup_helper(docker_host: &str, helper_id: &str) -> bool {
    let mut remove = Command::new("docker");
    remove
        .env("DOCKER_HOST", docker_host)
        .args(["container", "rm", "--force", helper_id]);
    remove.kill_on_drop(true);
    match tokio::time::timeout(HELPER_LAUNCH_TIMEOUT, remove.output()).await {
        Ok(Ok(output)) if output.status.success() => true,
        Ok(Ok(output)) => {
            let error = String::from_utf8_lossy(&output.stderr);
            error.contains("No such container") || error.contains("No such object")
        }
        _ => false,
    }
}

async fn helper_container_exists(socket: &Path, helper_id: &str) -> Result<bool> {
    validate_identifier(helper_id).map_err(anyhow::Error::msg)?;
    tokio::time::timeout(DOCKER_API_TIMEOUT, async {
        let req = Request::builder()
            .method(Method::GET)
            .uri(format!("/containers/{helper_id}/json"))
            .header(header::HOST, "localhost")
            .body(Body::empty())?;
        let response = forward(socket, req).await?;
        match response.status() {
            status if status.is_success() => Ok(true),
            StatusCode::NOT_FOUND => Ok(false),
            status => Err(anyhow!("helper inspect returned {status}")),
        }
    })
    .await
    .context("helper inspection timed out")?
}

async fn helper_container_running(socket: &Path, helper_id: &str) -> Result<bool> {
    validate_identifier(helper_id).map_err(anyhow::Error::msg)?;
    let inspect = daemon_json(socket, &format!("/containers/{helper_id}/json")).await?;
    inspect
        .pointer("/State/Running")
        .and_then(Value::as_bool)
        .ok_or_else(|| anyhow!("trusted helper has no running state"))
}

async fn helper_container_exit_code(socket: &Path, helper_id: &str) -> Result<Option<i64>> {
    validate_identifier(helper_id).map_err(anyhow::Error::msg)?;
    let inspect = daemon_json(socket, &format!("/containers/{helper_id}/json")).await?;
    helper_exit_code_from_inspect(&inspect)
}

fn helper_exit_code_from_inspect(inspect: &Value) -> Result<Option<i64>> {
    // A newly created, not-yet-started container reports Running=false and
    // ExitCode=0. Only an actual `exited` state is a completed handoff; staged
    // `created` recovery containers must still be started after Guard restarts.
    if inspect.pointer("/State/Status").and_then(Value::as_str) != Some("exited") {
        return Ok(None);
    }
    inspect
        .pointer("/State/ExitCode")
        .and_then(Value::as_i64)
        .map(Some)
        .ok_or_else(|| anyhow!("trusted helper has no exit code"))
}

async fn wait_for_helper_absence(socket: &Path, helper_id: &str, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut consecutive_absent = 0u8;
    loop {
        match helper_container_exists(socket, helper_id).await {
            Ok(false) => {
                consecutive_absent += 1;
                if consecutive_absent >= 5 {
                    return true;
                }
            }
            Ok(true) | Err(_) => consecutive_absent = 0,
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn inspect_handoff_attempt(socket: &Path, helper_id: &str) -> Result<HandoffAttempt> {
    validate_identifier(helper_id).map_err(anyhow::Error::msg)?;
    let inspect = daemon_json(socket, &format!("/containers/{helper_id}/json")).await?;
    handoff_attempt_from_inspect(&inspect)
}

fn handoff_attempt_from_inspect(inspect: &Value) -> Result<HandoffAttempt> {
    if inspect.get("Path").and_then(Value::as_str) != Some("/usr/local/bin/myriad-tcb-self-update")
        || inspect
            .pointer("/HostConfig/NetworkMode")
            .and_then(Value::as_str)
            != Some("none")
        || inspect
            .pointer("/HostConfig/ReadonlyRootfs")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Err(anyhow!("container is not the fixed trusted handoff helper"));
    }
    let env = inspect
        .pointer("/Config/Env")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("trusted handoff helper has no fixed environment"))?;
    let required = |name: &str| -> Result<String> {
        env.iter()
            .filter_map(Value::as_str)
            .find_map(|entry| entry.strip_prefix(&format!("{name}=")))
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("trusted handoff helper is missing {name}"))
    };
    let recovery_only = match env.iter().filter_map(Value::as_str).find_map(|entry| {
        entry.strip_prefix(&format!(
            "{}=",
            super::self_update_helper::ENV_RECOVERY_ONLY
        ))
    }) {
        None => false,
        Some("1") => true,
        Some(_) => return Err(anyhow!("trusted handoff helper has invalid recovery mode")),
    };
    let attempt = HandoffAttempt {
        previous_image: required(super::self_update_helper::ENV_PREVIOUS_IMAGE)?,
        target_image: required(super::self_update_helper::ENV_TARGET_IMAGE)?,
        previous_tag: required(super::self_update_helper::ENV_PREVIOUS_TAG)?,
        target_tag: required(super::self_update_helper::ENV_TARGET_TAG)?,
        recovery_only,
    };
    validate_guard_image_ref(&attempt.previous_image, false)?;
    validate_guard_image_ref(&attempt.target_image, false)?;
    validate_self_update_tag(&attempt.previous_tag).map_err(anyhow::Error::msg)?;
    validate_self_update_tag(&attempt.target_tag).map_err(anyhow::Error::msg)?;
    let configured_image = inspect
        .pointer("/Config/Image")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("trusted handoff helper has no configured image"))?;
    if !digest_reference_matches(configured_image, &attempt.target_image) {
        return Err(anyhow!(
            "trusted handoff helper image does not match its target digest"
        ));
    }
    Ok(attempt)
}

async fn prepare_trusted_self_update(
    state: &GuardState,
    requested_tag: &str,
) -> Result<(String, HandoffAttempt)> {
    let previous_image = state.config.expected_guard_image.clone();
    verify_managed_tcb_container(
        &state.config.socket_path,
        "myriad-updater",
        "updater",
        &state.config.project,
        &previous_image,
    )
    .await?;
    verify_managed_tcb_container(
        &state.config.socket_path,
        "myriad-updater-gateway",
        "updater-gateway",
        &state.config.project,
        &previous_image,
    )
    .await?;
    let previous_tag = running_updater_tag(&state.config.socket_path).await?;
    prevent_release_downgrade(&previous_tag, requested_tag)?;

    // Record intent before the pull so a long Hub fetch is not an invisible
    // "confirming result" gap, and a post-pull rejection can replace it.
    let pending = super::self_update_helper::SelfUpdateLastStatus::pending_before_handoff(
        requested_tag.to_owned(),
        previous_tag.clone(),
    );
    super::self_update_helper::write_status(
        &state.config.state_dir.join("self-update-last.json"),
        &pending,
    )
    .context("persist trusted handoff intent")?;

    // Guard has no egress. The host daemon pulls only the compiled-in official
    // repository; Guard then converts the result to repo@sha256 before handoff.
    let (exact_image, target_created_at) =
        pull_trusted_tag_and_resolve(&state.config.socket_path, requested_tag).await?;
    let current_created_at =
        managed_container_image_created_at(&state.config.socket_path, "myriad-updater").await?;
    if target_created_at < current_created_at {
        return Err(anyhow!("TCB image creation-time downgrade is forbidden"));
    }
    let attempt = HandoffAttempt {
        previous_image: previous_image.clone(),
        target_image: exact_image.clone(),
        previous_tag,
        target_tag: requested_tag.to_owned(),
        recovery_only: false,
    };
    let helper_id = launch_trusted_handoff(
        state,
        &attempt.previous_image,
        &attempt.target_image,
        &attempt.previous_tag,
        &attempt.target_tag,
        false,
    )
    .await?;
    Ok((helper_id, attempt))
}

fn validate_self_update_tag(tag: &str) -> std::result::Result<(), String> {
    let parsed = DeployTag::parse(tag).map_err(|error| error.to_string())?;
    if matches!(parsed.kind(), DeployTagKind::Branch) {
        return Err("mutable branch tags are forbidden for TCB self-update".into());
    }
    Ok(())
}

fn prevent_release_downgrade(previous: &str, target: &str) -> Result<()> {
    if let (Ok(previous), Ok(target)) =
        (MyriadVersion::parse(previous), MyriadVersion::parse(target))
    {
        if target.older_than(&previous) {
            return Err(anyhow!("TCB release downgrade is forbidden"));
        }
    }
    Ok(())
}

async fn running_updater_tag(socket: &Path) -> Result<String> {
    let inspect = daemon_json(socket, "/containers/myriad-updater/json").await?;
    inspect
        .pointer("/Config/Env")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find_map(|entry| entry.strip_prefix("MYRIAD_VERSION="))
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("running updater has no immutable MYRIAD_VERSION identity"))
        .and_then(|tag| {
            validate_self_update_tag(&tag).map_err(anyhow::Error::msg)?;
            Ok(tag)
        })
}

async fn verify_managed_tcb_container(
    socket: &Path,
    container: &str,
    service: &str,
    project: &str,
    expected_image: &str,
) -> Result<()> {
    let inspect = daemon_json(socket, &format!("/containers/{container}/json")).await?;
    if inspect
        .pointer("/Config/Labels/com.docker.compose.project")
        .and_then(Value::as_str)
        != Some(project)
        || inspect
            .pointer("/Config/Labels/com.docker.compose.service")
            .and_then(Value::as_str)
            != Some(service)
    {
        return Err(anyhow!("{container} is outside the fixed Compose identity"));
    }
    if inspect.pointer("/State/Running").and_then(Value::as_bool) != Some(true)
        || inspect
            .pointer("/State/Health/Status")
            .and_then(Value::as_str)
            != Some("healthy")
    {
        return Err(anyhow!("{container} is not running and healthy"));
    }
    let image_id = inspect
        .get("Image")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("{container} has no immutable image id"))?;
    let image = daemon_json(socket, &format!("/images/{image_id}/json")).await?;
    let matches = image
        .get("RepoDigests")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|actual| digest_reference_matches(actual, expected_image));
    if !matches {
        return Err(anyhow!(
            "{container} does not match the currently host-pinned Guard digest"
        ));
    }
    Ok(())
}

async fn pull_trusted_tag_and_resolve(
    socket: &Path,
    target_tag: &str,
) -> Result<(String, chrono::DateTime<chrono::FixedOffset>)> {
    let docker_host = format!("unix://{}", socket.display());
    let tagged_image = format!("{TRUSTED_UPDATER_REPOSITORY}:{target_tag}");
    let mut pull = Command::new("docker");
    pull.env("DOCKER_HOST", &docker_host)
        .args(["image", "pull", "--quiet", &tagged_image]);
    pull.kill_on_drop(true);
    let output = tokio::time::timeout(TRUSTED_PULL_TIMEOUT, pull.output())
        .await
        .context("trusted updater pull timed out")?
        .context("spawn fixed trusted-repository pull")?;
    if !output.status.success() {
        return Err(anyhow!(
            "pull trusted updater digest failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let mut inspect = Command::new("docker");
    inspect.env("DOCKER_HOST", &docker_host).args([
        "image",
        "inspect",
        "--format",
        "{{.Id}}",
        &tagged_image,
    ]);
    inspect.kill_on_drop(true);
    let output = tokio::time::timeout(DOCKER_API_TIMEOUT, inspect.output())
        .await
        .context("inspect pulled updater digest timed out")?
        .context("inspect pulled updater digest")?;
    if !output.status.success() {
        return Err(anyhow!("inspect trusted updater digest failed"));
    }
    let image_id = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !image_id.starts_with("sha256:") {
        return Err(anyhow!("pulled updater has no immutable image id"));
    }
    let image = daemon_json(socket, &format!("/images/{image_id}/json")).await?;
    let exact_image = image
        .get("RepoDigests")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find(|actual| {
            normalize_repository(actual).trim_start_matches("docker.io/")
                == TRUSTED_UPDATER_REPOSITORY.trim_start_matches("docker.io/")
        })
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("pulled image has no trusted updater repository digest"))?;
    validate_guard_image_ref(&exact_image, false)?;
    let created = image
        .get("Created")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("pulled updater image has no creation timestamp"))?;
    let created = chrono::DateTime::parse_from_rfc3339(created)
        .context("pulled updater image has invalid creation timestamp")?;
    Ok((exact_image, created))
}

async fn managed_container_image_created_at(
    socket: &Path,
    container: &str,
) -> Result<chrono::DateTime<chrono::FixedOffset>> {
    let inspect = daemon_json(socket, &format!("/containers/{container}/json")).await?;
    let image_id = inspect
        .get("Image")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("{container} has no immutable image id"))?;
    let image = daemon_json(socket, &format!("/images/{image_id}/json")).await?;
    let created = image
        .get("Created")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("{container} image has no creation timestamp"))?;
    chrono::DateTime::parse_from_rfc3339(created)
        .with_context(|| format!("{container} image has invalid creation timestamp"))
}

async fn launch_trusted_handoff(
    state: &GuardState,
    previous_image: &str,
    target_image: &str,
    previous_tag: &str,
    target_tag: &str,
    recovery_only: bool,
) -> Result<String> {
    let helper_name = if recovery_only {
        SELF_UPDATE_RECOVERY_NAME
    } else {
        SELF_UPDATE_HELPER_NAME
    };
    let socket = state.config.socket_path.to_string_lossy().into_owned();
    let docker_host = format!("unix://{socket}");
    let host_root = state.host_compose_root.to_string_lossy().into_owned();
    // `host_compose_root` is a Docker-daemon host source and generally does
    // not exist at the same path inside Guard (especially on Docker Desktop).
    // Validate through the dedicated container-visible read-only/read-write
    // mounts, then pass the host source only to Docker's --mount API.
    let env_path = state.config.compose_dir.join(".env");
    let state_path = &state.config.state_dir;
    if !env_path.is_file() || !state_path.is_dir() {
        return Err(anyhow!("fixed updater .env/state paths are unavailable"));
    }
    let (policy_parent, policy_name) = split_host_policy_path(&state.config.host_policy_path)?;
    let policy_container_path = format!("/host/policy/{policy_name}");

    let mut command = Command::new("docker");
    command.env("DOCKER_HOST", &docker_host);
    if recovery_only {
        command.arg("create");
    } else {
        command.args(["run", "--detach"]);
    }
    command.args([
        "--pull",
        "never",
        "--name",
        helper_name,
        "--network",
        "none",
        "--read-only",
        "--security-opt",
        "no-new-privileges:true",
        "--tmpfs",
        "/tmp",
        "--tmpfs",
        "/run",
        "--entrypoint",
        "/usr/local/bin/myriad-tcb-self-update",
        "--mount",
        &format!("type=bind,source={socket},target=/var/run/docker.sock"),
        // Compose definitions stay read-only. Atomic EnvFile writes use a
        // second, short-lived mount of the same host root; only this verified,
        // fixed-entrypoint helper receives the writable view.
        "--mount",
        &format!("type=bind,source={host_root},target=/host/compose,readonly"),
        "--mount",
        &format!("type=bind,source={host_root},target=/host/write"),
        "--mount",
        &format!("type=bind,source={policy_parent},target=/host/policy"),
    ]);
    for (name, value) in [
        (
            super::self_update_helper::ENV_PREVIOUS_IMAGE,
            previous_image,
        ),
        (super::self_update_helper::ENV_TARGET_IMAGE, target_image),
        (super::self_update_helper::ENV_PREVIOUS_TAG, previous_tag),
        (super::self_update_helper::ENV_TARGET_TAG, target_tag),
        (
            super::self_update_helper::ENV_PROJECT,
            &state.config.project,
        ),
        (
            super::self_update_helper::ENV_PROJECT_DIRECTORY,
            "/host/compose",
        ),
        (super::self_update_helper::ENV_HOST_COMPOSE_ROOT, &host_root),
        (super::self_update_helper::ENV_COMPOSE_DIR, "/host/compose"),
        (
            super::self_update_helper::ENV_APP_ENV_FILE,
            "/host/write/.env",
        ),
        (
            super::self_update_helper::ENV_GUARD_ENV_FILE,
            &policy_container_path,
        ),
        (
            super::self_update_helper::ENV_STATUS_FILE,
            "/host/write/state/self-update-last.json",
        ),
        (
            super::self_update_helper::ENV_POLICY_HOST_PATH,
            &state.config.host_policy_path,
        ),
        (
            super::self_update_helper::ENV_COMPOSE_NETWORK,
            &state.config.compose_network,
        ),
        (
            super::self_update_helper::ENV_ADMIN_NETWORK,
            &state.config.admin_network,
        ),
        (
            super::self_update_helper::ENV_GUARD_NETWORK,
            &state.config.guard_network,
        ),
    ] {
        command.arg("-e").arg(format!("{name}={value}"));
    }
    if recovery_only {
        command.arg("-e").arg(format!(
            "{}=1",
            super::self_update_helper::ENV_RECOVERY_ONLY
        ));
    }
    command.arg(target_image);
    command.kill_on_drop(true);
    let output = match tokio::time::timeout(HELPER_LAUNCH_TIMEOUT, command.output()).await {
        Ok(output) => output.context("launch trusted TCB handoff")?,
        Err(_) => {
            let _ = cleanup_helper(&docker_host, helper_name).await;
            return Err(anyhow::Error::new(HandoffCleanupUnconfirmed));
        }
    };
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        if detail.contains("already in use") || detail.contains("Conflict") {
            return Err(anyhow::Error::new(HandoffCleanupUnconfirmed));
        }
        return Err(anyhow!("launch trusted TCB handoff failed: {}", detail));
    }
    let helper_id = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    validate_identifier(&helper_id).map_err(anyhow::Error::msg)?;
    info!(%helper_id, %target_image, "trusted TCB handoff scheduled");
    Ok(helper_id)
}

fn split_host_policy_path(path: &str) -> Result<(String, String)> {
    let split = path
        .rfind(['/', '\\'])
        .ok_or_else(|| anyhow!("Guard policy path has no parent directory"))?;
    let parent = &path[..split];
    let name = &path[split + 1..];
    if parent.is_empty()
        || name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(anyhow!("Guard policy path is not a safe fixed file path"));
    }
    Ok((parent.to_owned(), name.to_owned()))
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
            validate_container_create_name(uri)?;
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
            validate_image_pull(state, uri, body)?;
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

fn validate_container_create_name(uri: &Uri) -> std::result::Result<(), String> {
    let Some(name) = query_param(uri, "name") else {
        return Ok(());
    };
    let name = name.trim_start_matches('/');
    validate_identifier(name)?;
    if matches!(
        name,
        SELF_UPDATE_HELPER_NAME
            | SELF_UPDATE_RECOVERY_NAME
            | SELF_UPDATE_EXHAUSTED_NAME
            | "myriad-docker-guard"
            | "myriad-updater"
            | "myriad-updater-gateway"
    ) {
        return Err("container name is reserved for the trusted updater control plane".into());
    }
    Ok(())
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
    let expected_repository =
        state.config.service_images.get(service).ok_or_else(|| {
            "Compose service is not managed by the generic updater API".to_string()
        })?;

    let image = value
        .get("Image")
        .and_then(Value::as_str)
        .ok_or_else(|| "container image is missing".to_string())?;
    if normalize_repository(image) != *expected_repository {
        return Err("container image does not match the fixed service repository".into());
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

fn validate_image_pull(
    state: &GuardState,
    uri: &Uri,
    body: &Bytes,
) -> std::result::Result<(), String> {
    if !body.is_empty() {
        return Err("image pull request body must be empty".into());
    }
    let mut has_tag = false;
    for (name, value) in url::form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes()) {
        match name.as_ref() {
            "fromImage" => {}
            "tag" => {
                has_tag = true;
                validate_pull_tag(&value)?;
            }
            // Compose/Bollard may select an architecture, but import/build
            // selectors such as fromSrc/repo are never part of a registry pull.
            "platform" if !value.trim().is_empty() => {}
            _ => return Err(format!("image pull query parameter {name} is forbidden")),
        }
    }
    if !has_tag {
        return Err("images/create requires an explicit tag".into());
    }
    let image = query_param(uri, "fromImage")
        .ok_or_else(|| "images/create requires fromImage".to_string())?;
    let repository = normalize_repository(&image);
    if repository == TRUSTED_UPDATER_REPOSITORY {
        return Err("TCB image pulls require the dedicated self-update endpoint".into());
    }
    if state.config.allowed_images.contains(&repository) {
        Ok(())
    } else {
        Err("image pull repository is not allowlisted".into())
    }
}

fn validate_pull_tag(tag: &str) -> std::result::Result<(), String> {
    let tag = tag.trim();
    if tag.is_empty()
        || tag.len() > 128
        || tag.eq_ignore_ascii_case("latest")
        || !tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err("image pull tag has unsafe syntax".into());
    }
    Ok(())
}

fn validate_image_tag(
    state: &GuardState,
    encoded_source: &str,
    uri: &Uri,
) -> std::result::Result<(), String> {
    let source = decode_path_segment(encoded_source);
    let repo = query_param(uri, "repo").ok_or_else(|| "image tag requires repo".to_string())?;
    let source_repository = normalize_repository(&source);
    if source_repository == TRUSTED_UPDATER_REPOSITORY {
        return Err("TCB image tagging is forbidden on the generic updater API".into());
    }
    if !state.config.allowed_images.contains(&source_repository) {
        return Err("image tag source repository is not allowlisted".into());
    }
    let target_repository = normalize_repository(&repo);
    if target_repository == TRUSTED_UPDATER_REPOSITORY {
        return Err("TCB image tagging is forbidden on the generic updater API".into());
    }
    if !state.config.allowed_images.contains(&target_repository) {
        return Err("image tag target repository is not allowlisted".into());
    }
    if source_repository != target_repository {
        return Err("cross-repository image tagging is forbidden".into());
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
                state_dir: "/host/state".into(),
                expected_guard_image: format!(
                    "{TRUSTED_GUARD_REPOSITORY}@sha256:{}",
                    "a".repeat(64)
                ),
                host_policy_path: "/etc/myriad/docker-guard.env".into(),
                self_update_token: SecretString::new("g7N2pQ8xV4mK6rT9wY3zA5bC1dF0hJ8l"),
                allow_unpinned_dev: false,
                allowed_images: [
                    "docker.io/example/backend".into(),
                    "docker.io/example/frontend".into(),
                    "docker.io/example/proxy".into(),
                    "postgres".into(),
                ]
                .into_iter()
                .collect(),
                service_images: [
                    ("backend".into(), "docker.io/example/backend".into()),
                    (
                        "backend-volume-init".into(),
                        "docker.io/example/backend".into(),
                    ),
                    ("frontend".into(), "docker.io/example/frontend".into()),
                    ("proxy".into(), "docker.io/example/proxy".into()),
                    ("postgres".into(), "postgres".into()),
                ]
                .into_iter()
                .collect(),
            }),
            host_compose_root: Arc::new(PathBuf::from("/srv/myriad")),
            mutation_gate: Arc::new(AtomicUsize::new(0)),
        }
    }

    #[test]
    fn compromised_updater_cannot_inject_mutable_or_foreign_image_identity() {
        for tag in ["latest", "preview", "../v9.9.9", "v1.2.3;id"] {
            assert!(validate_self_update_tag(tag).is_err(), "accepted {tag}");
        }
        assert!(validate_self_update_tag("v1.2.3").is_ok());
        assert!(validate_self_update_tag("dev-0123456").is_ok());
    }

    #[test]
    fn self_update_request_rejects_repo_digest_and_command_injection_fields() {
        let injected = br#"{
            "target_tag":"v1.2.3",
            "trust_path":"dockerhub_tag",
            "repo":"docker.io/attacker/root",
            "digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "command":["sh","-c","id"]
        }"#;
        assert!(serde_json::from_slice::<SelfUpdateRequestBody>(injected).is_err());
    }

    #[tokio::test]
    async fn self_update_requires_the_host_policy_capability_before_parsing_body() {
        let missing = Request::builder()
            .method(Method::POST)
            .uri("/_myriad/self-update")
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(
            handle_self_update(state(), missing).await.status(),
            StatusCode::UNAUTHORIZED
        );

        let wrong = Request::builder()
            .method(Method::POST)
            .uri("/_myriad/self-update")
            .header(SELF_UPDATE_TOKEN_HEADER, "x".repeat(40))
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(
            handle_self_update(state(), wrong).await.status(),
            StatusCode::UNAUTHORIZED
        );

        let valid = Request::builder()
            .method(Method::POST)
            .uri("/_myriad/self-update")
            .header(SELF_UPDATE_TOKEN_HEADER, "g7N2pQ8xV4mK6rT9wY3zA5bC1dF0hJ8l")
            .body(Body::from("{}"))
            .unwrap();
        assert_eq!(
            handle_self_update(state(), valid).await.status(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn mutation_gate_is_exclusive_and_clone_safe() {
        let gate = Arc::new(AtomicUsize::new(0));
        let lease = GenericMutationLease::acquire(gate.clone()).unwrap();
        let response_lease = lease.clone();
        drop(lease);
        assert_eq!(gate.load(Ordering::SeqCst), 1);
        assert!(gate
            .compare_exchange(0, SELF_UPDATE_GATE, Ordering::SeqCst, Ordering::SeqCst)
            .is_err());
        drop(response_lease);
        assert_eq!(gate.load(Ordering::SeqCst), 0);
        gate.store(SELF_UPDATE_GATE, Ordering::SeqCst);
        assert!(GenericMutationLease::acquire(gate).is_err());
    }

    #[test]
    fn self_update_rejects_nonempty_or_unsafe_job_state() {
        let state_root = tempfile::tempdir().unwrap();
        let mut state = state();
        Arc::make_mut(&mut state.config).state_dir = state_root.path().to_path_buf();
        assert!(ensure_no_business_update(&state).is_ok());
        std::fs::write(state_root.path().join("job.current"), "job-123").unwrap();
        assert!(ensure_no_business_update(&state).is_err());
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

    #[test]
    fn host_policy_path_split_supports_linux_and_windows() {
        assert_eq!(
            split_host_policy_path("/etc/myriad/docker-guard.env").unwrap(),
            ("/etc/myriad".into(), "docker-guard.env".into())
        );
        assert_eq!(
            split_host_policy_path(r"C:\ProgramData\Myriad\docker-guard.env").unwrap(),
            (r"C:\ProgramData\Myriad".into(), "docker-guard.env".into())
        );
        assert!(split_host_policy_path("docker-guard.env").is_err());
    }

    #[test]
    fn release_self_update_rejects_semver_downgrade() {
        assert!(prevent_release_downgrade("v1.2.3", "v1.2.2").is_err());
        assert!(prevent_release_downgrade("v1.2.3", "v1.2.3").is_ok());
        assert!(prevent_release_downgrade("v1.2.3", "v1.3.0").is_ok());
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
    fn generic_api_cannot_recreate_tcb_services() {
        for service in ["updater", "updater-gateway", "docker-guard"] {
            let body = create(service, "docker.io/example/updater:v1", json!({}));
            assert!(validate_container_create(&state(), &body)
                .unwrap_err()
                .contains("generic updater API"));
        }
    }

    #[test]
    fn generic_create_cannot_reserve_control_plane_container_names() {
        let body = create("backend", "docker.io/example/backend:v1", json!({}));
        for name in [
            "myriad-tcb-self-update",
            "myriad-tcb-self-update-recovery",
            "myriad-tcb-self-update-recovery-exhausted",
            "myriad-docker-guard",
            "myriad-updater",
            "myriad-updater-gateway",
        ] {
            let uri: Uri = format!("/v1.51/containers/create?name={name}")
                .parse()
                .unwrap();
            assert!(classify_request(&state(), &Method::POST, &uri, &body).is_err());
        }
    }

    #[test]
    fn persisted_helper_identity_recovers_only_guard_created_handoff_intent() {
        let previous = format!(
            "docker.io/somekawahitomi/myriad-updater@sha256:{}",
            "a".repeat(64)
        );
        let target = format!(
            "docker.io/somekawahitomi/myriad-updater@sha256:{}",
            "b".repeat(64)
        );
        let inspect = json!({
            "Path": "/usr/local/bin/myriad-tcb-self-update",
            "HostConfig": {"NetworkMode": "none", "ReadonlyRootfs": true},
            "Config": {
                "Image": target.clone(),
                "Env": [
                    format!("{}={previous}", super::super::self_update_helper::ENV_PREVIOUS_IMAGE),
                    format!("{}={target}", super::super::self_update_helper::ENV_TARGET_IMAGE),
                    format!("{}=v1.2.2", super::super::self_update_helper::ENV_PREVIOUS_TAG),
                    format!("{}=v1.2.3", super::super::self_update_helper::ENV_TARGET_TAG),
                ]
            }
        });
        let attempt = handoff_attempt_from_inspect(&inspect).unwrap();
        assert_eq!(attempt.previous_tag, "v1.2.2");
        assert_eq!(attempt.target_tag, "v1.2.3");
        assert!(!attempt.recovery_only);

        let mut recovery = inspect.clone();
        recovery["Config"]["Env"]
            .as_array_mut()
            .unwrap()
            .push(json!(format!(
                "{}=1",
                super::super::self_update_helper::ENV_RECOVERY_ONLY
            )));
        assert!(
            handoff_attempt_from_inspect(&recovery)
                .unwrap()
                .recovery_only
        );

        let mut forged = inspect;
        forged["Config"]["Image"] = json!(previous);
        assert!(handoff_attempt_from_inspect(&forged).is_err());
    }

    #[tokio::test]
    async fn orphaned_pending_handoff_becomes_a_fresh_failure() {
        let root = tempfile::tempdir().unwrap();
        let mut state = state();
        Arc::make_mut(&mut state.config).state_dir = root.path().to_path_buf();
        let path = root.path().join("self-update-last.json");
        let pending =
            super::super::self_update_helper::SelfUpdateLastStatus::pending_before_handoff(
                "v1.2.3".into(),
                "v1.2.2".into(),
            );
        super::super::self_update_helper::write_status(&path, &pending).unwrap();

        assert!(!finalize_or_fail_orphaned_pending_handoff(&state).await);

        let status: super::super::self_update_helper::SelfUpdateLastStatus =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert!(matches!(
            status.status,
            super::super::self_update_helper::SelfUpdateOutcome::Failed
        ));
        assert!(status
            .error
            .unwrap()
            .contains("target TCB was not fully active"));
        assert_eq!(state.mutation_gate.load(Ordering::SeqCst), SELF_UPDATE_GATE);
    }

    #[test]
    fn recovery_retry_budget_survives_guard_restart() {
        let root = tempfile::tempdir().unwrap();
        let mut state = state();
        Arc::make_mut(&mut state.config).state_dir = root.path().to_path_buf();
        let attempt = HandoffAttempt {
            previous_image: format!(
                "docker.io/somekawahitomi/myriad-updater@sha256:{}",
                "a".repeat(64)
            ),
            target_image: format!(
                "docker.io/somekawahitomi/myriad-updater@sha256:{}",
                "b".repeat(64)
            ),
            previous_tag: "v1.2.2".into(),
            target_tag: "v1.2.3".into(),
            recovery_only: true,
        };

        assert!(persist_recovery_attempt(&state, &attempt, 1));
        assert_eq!(recovery_attempt_from_status(&state, &attempt), 1);

        let status: super::super::self_update_helper::SelfUpdateLastStatus =
            serde_json::from_slice(
                &std::fs::read(root.path().join("self-update-last.json")).unwrap(),
            )
            .unwrap();
        assert!(matches!(
            status.status,
            super::super::self_update_helper::SelfUpdateOutcome::Pending
        ));
        assert_eq!(status.recovery_attempt, 1);
    }

    #[test]
    fn third_recovery_failure_is_exhausted_even_before_final_status_write() {
        assert!(recovery_is_durably_exhausted(Some(1), 2, false));
        assert!(recovery_is_durably_exhausted(Some(1), 0, true));
        assert!(!recovery_is_durably_exhausted(Some(1), 1, false));
        assert!(!recovery_is_durably_exhausted(Some(0), 2, true));
        assert!(!recovery_is_durably_exhausted(None, 2, true));
    }

    #[test]
    fn staged_recovery_is_not_mistaken_for_successful_exit() {
        let created = json!({
            "State": {"Status": "created", "Running": false, "ExitCode": 0}
        });
        let running = json!({
            "State": {"Status": "running", "Running": true, "ExitCode": 0}
        });
        let succeeded = json!({
            "State": {"Status": "exited", "Running": false, "ExitCode": 0}
        });
        let failed = json!({
            "State": {"Status": "exited", "Running": false, "ExitCode": 1}
        });

        assert_eq!(helper_exit_code_from_inspect(&created).unwrap(), None);
        assert_eq!(helper_exit_code_from_inspect(&running).unwrap(), None);
        assert_eq!(helper_exit_code_from_inspect(&succeeded).unwrap(), Some(0));
        assert_eq!(helper_exit_code_from_inspect(&failed).unwrap(), Some(1));
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
    fn service_repository_mapping_rejects_cross_service_images() {
        let body = create("frontend", "docker.io/example/backend:v1", json!({}));
        assert!(validate_container_create(&state(), &body)
            .unwrap_err()
            .contains("fixed service repository"));
    }

    #[test]
    fn bind_mount_propagation_is_denied() {
        let visible = tempfile::tempdir().unwrap();
        std::fs::create_dir(visible.path().join("pgdata")).unwrap();
        let state = state_with_visible_root(visible.path().to_path_buf());
        let string_bind = create(
            "postgres",
            "postgres:18-alpine",
            json!({"Binds": ["/srv/myriad/pgdata:/var/lib/postgresql:rw,rshared"]}),
        );
        assert!(validate_container_create(&state, &string_bind)
            .unwrap_err()
            .contains("propagation"));

        let structured_mount = create(
            "postgres",
            "postgres:18-alpine",
            json!({"Mounts": [{
                "Type": "bind",
                "Source": "/srv/myriad/pgdata",
                "Target": "/var/lib/postgresql",
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
        assert!(validate_image_pull(&state(), &allowed, &Bytes::new()).is_ok());
        let denied = Uri::from_static("/v1.51/images/create?fromImage=evil%2Fpayload&tag=latest");
        assert!(validate_image_pull(&state(), &denied, &Bytes::new()).is_err());

        let imported = Uri::from_static(
            "/v1.51/images/create?fromImage=docker.io%2Fexample%2Fbackend&tag=v1&fromSrc=https%3A%2F%2Fevil.invalid%2Fimage.tar",
        );
        assert!(validate_image_pull(&state(), &imported, &Bytes::new()).is_err());
        assert!(
            validate_image_pull(&state(), &allowed, &Bytes::from_static(b"tar payload")).is_err()
        );
    }

    #[test]
    fn image_tag_requires_allowlisted_source_and_target() {
        let allowed = Uri::from_static("/v1.51/images/docker.io%2Fexample%2Fbackend:v1/tag?repo=docker.io%2Fexample%2Fbackend&tag=myriad-rollback");
        assert!(classify_request(&state(), &Method::POST, &allowed, &Bytes::new()).is_ok());

        let denied_source = Uri::from_static("/v1.51/images/evil%2Fpayload:v1/tag?repo=docker.io%2Fexample%2Fbackend&tag=myriad-rollback");
        assert!(classify_request(&state(), &Method::POST, &denied_source, &Bytes::new()).is_err());

        let cross_repository = Uri::from_static("/v1.51/images/docker.io%2Fexample%2Fbackend:v1/tag?repo=docker.io%2Fexample%2Ffrontend&tag=v1");
        assert!(
            classify_request(&state(), &Method::POST, &cross_repository, &Bytes::new()).is_err()
        );

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
    fn generic_api_cannot_create_any_guard_network_client() {
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
        assert!(validate_container_create(&s, &updater).is_err());
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
    fn generic_api_rejects_guard_network_mode_for_every_service() {
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
        assert!(validate_container_create(&s, &updater).is_err());
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
