//! Policy-enforcing proxy between the updater and the host Docker daemon.
//!
//! This is intentionally narrower than a generic socket proxy: mutations are limited to
//! containers in one Compose project, and container-create request bodies are validated before
//! reaching the daemon. The updater never receives the raw Unix socket.

mod classify;
mod config;
mod forward;
mod self_update;
pub(crate) mod startup;
mod validate;

#[cfg(test)]
mod tests;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Result, anyhow};
use axum::Router;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tracing::{error, info, warn};

pub use config::GuardConfig;

use config::{digest_reference_matches, ensure_host_policy_file, validate_guard_image_ref};
use forward::{daemon_json, discover_host_compose_root, handle};
use self_update::{
    cleanup_helper, helper_container_exists, helper_container_exit_code, helper_container_running,
    inspect_handoff_attempt, monitor_handoff, recovery_attempt_from_status, restart_helper,
    resume_staged_recovery, stop_helper, wait_for_helper_absence,
};

pub(crate) const TRUSTED_GUARD_REPOSITORY: &str = "docker.io/somekawahitomi/myriad-updater";
pub(crate) const TRUSTED_UPDATER_REPOSITORY: &str = TRUSTED_GUARD_REPOSITORY;
pub(crate) const STARTUP_RECONCILE_NAME: &str = "myriad-tcb-reconcile";
pub(crate) const SELF_UPDATE_HELPER_NAME: &str = "myriad-tcb-self-update";
pub(crate) const SELF_UPDATE_RECOVERY_NAME: &str = "myriad-tcb-self-update-recovery";
pub(crate) const SELF_UPDATE_EXHAUSTED_NAME: &str = "myriad-tcb-self-update-recovery-exhausted";
pub(crate) const DOCKER_API_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const MAX_CONTAINER_LOG_BODY: usize = 4 * 1024 * 1024;
pub(crate) const MAX_CONTAINER_LOG_TAIL: u64 = 10_000;
pub(crate) const MAX_CONCURRENT_LOG_READS: usize = 3;
pub(crate) const SELF_UPDATE_GATE: usize = 1usize << (usize::BITS - 1);
pub(crate) const POLICY_CONTAINER_FILE: &str = "/guard-policy/docker-guard.env";

#[derive(Clone)]
pub(crate) struct GuardState {
    config: Arc<GuardConfig>,
    host_compose_root: Arc<PathBuf>,
    mutation_gate: Arc<AtomicUsize>,
    log_read_gate: Arc<Semaphore>,
}

pub async fn run(mut config: GuardConfig) -> Result<()> {
    let hostname = current_container_id()?;
    if config.allow_unpinned_dev {
        verify_running_guard_image(
            &config.socket_path,
            &hostname,
            &config.expected_guard_image,
            true,
        )
        .await?;
    } else {
        // Host-managed Compose changes supersede stale pins, but only the
        // daemon's actual official image identity is accepted (never .env's tag).
        let actual = startup::inspect_identity(&config, &hostname, "docker-guard", false).await?;
        config.expected_guard_image = actual.image;
    }
    let host_compose_root = match std::env::var("DOCKER_GUARD_HOST_COMPOSE_ROOT") {
        Ok(root) if !root.trim().is_empty() => PathBuf::from(root),
        _ => discover_host_compose_root(&config.socket_path, &hostname).await?,
    };
    ensure_host_policy_file(&config, Path::new(POLICY_CONTAINER_FILE))?;
    let listen = config.listen;
    let state = GuardState {
        config: Arc::new(config),
        host_compose_root: Arc::new(host_compose_root.clone()),
        mutation_gate: Arc::new(AtomicUsize::new(0)),
        log_read_gate: Arc::new(Semaphore::new(MAX_CONCURRENT_LOG_READS)),
    };
    // v0.5.3 retained exhausted recovery under a third name. Treat it as a
    // stopped recovery, not a permanent lock. Remove this name next release.
    let residual_helper =
        if helper_container_exists(&state.config.socket_path, SELF_UPDATE_RECOVERY_NAME).await? {
            Some((SELF_UPDATE_RECOVERY_NAME, true))
        } else if helper_container_exists(&state.config.socket_path, SELF_UPDATE_HELPER_NAME)
            .await?
        {
            Some((SELF_UPDATE_HELPER_NAME, false))
        } else if helper_container_exists(&state.config.socket_path, SELF_UPDATE_EXHAUSTED_NAME)
            .await?
        {
            Some((SELF_UPDATE_EXHAUSTED_NAME, true))
        } else {
            None
        };
    if let Some((helper_name, recovery_only)) = residual_helper {
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
        let recovered_retries = if helper_name == SELF_UPDATE_EXHAUSTED_NAME {
            2
        } else if recovery_only {
            attempt
                .as_ref()
                .map(|attempt| recovery_attempt_from_status(&state, attempt))
                .unwrap_or(0)
        } else {
            0
        };
        let mut monitor_ready = true;
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
            if !normal_absent
                || (!recovery_running
                    && recovery_exit.is_none()
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
        } else if let Some(attempt) = attempt {
            resume_staged_recovery(state.clone(), attempt, recovered_retries);
        }
    } else {
        if state.config.allow_unpinned_dev {
            let _ = self_update::finalize_or_fail_orphaned_pending_handoff(&state).await;
        } else {
            startup::schedule_reconciliation(state.clone());
        }
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

pub(crate) fn strip_api_version(path: &str) -> &str {
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

pub(crate) fn validate_identifier(value: &str) -> std::result::Result<(), String> {
    if value.is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err("invalid Docker object identifier".into());
    }
    Ok(())
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

pub(crate) fn denial(status: StatusCode, message: &str) -> Response {
    (status, axum::Json(json!({"message": message}))).into_response()
}
