//! Trusted TCB self-update handoff owned by Guard.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use tokio::process::Command;
use tracing::{error, info, warn};

use crate::api::auth::constant_time_eq;

use super::config::{
    canonicalize_trusted_digest_ref, digest_reference_matches, validate_guard_image_ref,
};
use super::forward::{daemon_json, forward};
use super::{
    DOCKER_API_TIMEOUT, GuardState, POLICY_CONTAINER_FILE, SELF_UPDATE_GATE,
    SELF_UPDATE_HELPER_NAME, SELF_UPDATE_RECOVERY_NAME, TRUSTED_UPDATER_REPOSITORY, denial,
    validate_identifier,
};

pub(crate) const SELF_UPDATE_TOKEN_HEADER: &str = "x-guard-self-update-token";
const TRUSTED_PULL_TIMEOUT: Duration = Duration::from_secs(180);
const HELPER_LAUNCH_TIMEOUT: Duration = Duration::from_secs(30);
const HELPER_TOTAL_TIMEOUT: Duration = Duration::from_secs(20 * 60);

#[derive(Debug)]
struct HandoffCleanupUnconfirmed;

impl std::fmt::Display for HandoffCleanupUnconfirmed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("self-update helper cleanup could not be confirmed")
    }
}

impl std::error::Error for HandoffCleanupUnconfirmed {}

#[derive(Debug, Clone)]
pub(crate) struct HandoffAttempt {
    pub(crate) previous_image: String,
    pub(crate) previous_images: Option<[String; 3]>,
    pub(crate) target_image: String,
    pub(crate) previous_tag: String,
    pub(crate) target_tag: String,
    pub(crate) recovery_only: bool,
}

pub(crate) async fn finalize_or_fail_orphaned_pending_handoff(state: &GuardState) -> bool {
    let path = state.config.state_dir.join("self-update-last.json");
    let Some(pending) = std::fs::read(&path).ok().and_then(|bytes| {
        serde_json::from_slice::<crate::docker::self_update_helper::SelfUpdateLastStatus>(&bytes)
            .ok()
    }) else {
        return false;
    };
    if pending.queued
        || !matches!(
            pending.status,
            crate::docker::self_update_helper::SelfUpdateOutcome::Pending
        )
    {
        return false;
    }
    let stack = super::startup::healthy_stack(&state.config).await;
    let target_active = stack.as_ref().is_ok_and(|stack| {
        stack.iter().all(|identity| {
            pending.target_image.as_ref().map_or_else(
                || {
                    pending.target_tag != pending.previous_tag
                        && identity.image == stack[0].image
                        && identity.version == pending.target_tag
                },
                |target| &identity.image == target,
            )
        })
    });
    let status = if target_active {
        crate::docker::self_update_helper::SelfUpdateLastStatus::succeeded_after_handoff(
            pending.target_tag,
            pending.previous_tag,
        )
    } else {
        crate::docker::self_update_helper::SelfUpdateLastStatus::failed_before_handoff(
            pending.target_tag,
            pending.previous_tag,
            "trusted handoff was interrupted; target TCB was not fully active".into(),
        )
    };
    crate::state::atomic::write_json_until_saved(&path, &status).await;
    true
}

const MAX_SELF_UPDATE_BODY: usize = 4 * 1024;

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SelfUpdateRequestBody {
    target_tag: String,
    trust_path: String,
}

pub(crate) async fn handle_self_update(state: GuardState, req: Request<Body>) -> Response {
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
            );
        }
    };
    let request: SelfUpdateRequestBody = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(_) => {
            return denial(
                StatusCode::BAD_REQUEST,
                "JSON body requires only target_tag and trust_path",
            );
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
    // Persist acceptance before replying: callers use this record for exclusion.
    let previous_tag = crate::docker::self_update_helper::read_status(&state.config.state_dir)
        .ok()
        .flatten()
        .map(|status| status.previous_tag)
        .unwrap_or_default();
    let pending = crate::docker::self_update_helper::SelfUpdateLastStatus::pending_before_handoff(
        target_tag.clone(),
        previous_tag,
    );
    if let Err(error) = crate::docker::self_update_helper::write_status(
        &state.config.state_dir.join("self-update-last.json"),
        &pending,
    ) {
        state.mutation_gate.store(0, Ordering::SeqCst);
        return denial(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("persist self-update acceptance: {error}"),
        );
    }
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
                let previous_tag =
                    crate::docker::self_update_helper::read_status(&task_state.config.state_dir)
                        .ok()
                        .flatten()
                        .map(|status| status.previous_tag)
                        .unwrap_or_default();
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
                        crate::docker::self_update_helper::SelfUpdateLastStatus::failed_before_handoff(
                            task_target,
                            previous_tag,
                            error.to_string(),
                        );
                    crate::state::atomic::write_json_until_saved(
                        &task_state.config.state_dir.join("self-update-last.json"),
                        &status,
                    )
                    .await;
                }
            }
        }
    });

    (
        StatusCode::ACCEPTED,
        axum::Json(json!({
            "scheduled": true,
            "status_persisted": true,
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

pub(crate) fn ensure_no_business_update(state: &GuardState) -> std::result::Result<(), String> {
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

pub(crate) fn monitor_handoff(
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
        // The helper may reject before changing anything, or may already have
        // rolled back successfully. Do not replace a healthy original mixed
        // deployment again merely because the requested upgrade failed.
        let restored = if failure.is_some()
            && helper_container_exit_code(&state.config.socket_path, &helper_id)
                .await
                .ok()
                .flatten()
                .is_some()
        {
            if let Some(attempt) = &attempt {
                previous_stack_is_healthy(&state, attempt).await
            } else {
                false
            }
        } else {
            false
        };
        let failure = if restored { None } else { failure };
        let recovery_only = recovery_only || restored;
        if let (Some(failure), Some(attempt)) = (failure.as_deref(), attempt.as_ref())
            && !recovery_only
        {
            warn!(%failure, "starting fixed previous-digest recovery handoff");
            match launch_trusted_handoff(
                &state,
                &attempt.previous_image,
                &attempt.target_image,
                &attempt.previous_tag,
                &attempt.target_tag,
                true,
                attempt.previous_images.as_ref(),
            )
            .await
            {
                Ok(recovery_id) => {
                    let old_stopped = stop_helper(&docker_host, &helper_id).await;
                    if !old_stopped {
                        error!(
                            %helper_id,
                            %recovery_id,
                            "old helper has not stopped; recovery remains staged"
                        );
                        resume_staged_recovery(state, attempt.clone(), 0);
                        return;
                    }
                    let _ = cleanup_helper(&docker_host, &helper_id).await;
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
            let attempt = attempt.unwrap();
            // A terminal result is only published once no executor can keep mutating.
            if helper_container_exit_code(&state.config.socket_path, &helper_id)
                .await
                .ok()
                .flatten()
                .is_none()
                && !stop_helper(&docker_host, &helper_id).await
            {
                resume_staged_recovery(state, attempt, recovery_retries);
                return;
            }
            record_helper_failure_if_missing(
                &state,
                &attempt.target_tag,
                &attempt.previous_tag,
                failure,
                monitor_started,
            )
            .await;
            let _ = cleanup_helper(&docker_host, &helper_id).await;
            state.mutation_gate.store(0, Ordering::SeqCst);
            error!(%helper_id, "recovery failed; stopped execution and released the update slot");
            return;
        }
        if let Some(attempt) = attempt {
            let status = if recovery_only {
                let detail =
                    crate::docker::self_update_helper::read_status(&state.config.state_dir)
                        .ok()
                        .flatten()
                        .and_then(|status| status.error)
                        .unwrap_or_else(|| {
                            "target switch failed; previous components restored".into()
                        });
                crate::docker::self_update_helper::SelfUpdateLastStatus::failed_before_handoff(
                    attempt.target_tag,
                    attempt.previous_tag,
                    detail,
                )
            } else {
                crate::docker::self_update_helper::SelfUpdateLastStatus::succeeded_after_handoff(
                    attempt.target_tag,
                    attempt.previous_tag,
                )
            };
            crate::state::atomic::write_json_until_saved(
                &state.config.state_dir.join("self-update-last.json"),
                &status,
            )
            .await;
        }
        let _ = cleanup_helper(&docker_host, &helper_id).await;
        state.mutation_gate.store(0, Ordering::SeqCst);
    });
}

pub(crate) async fn previous_stack_is_healthy(
    state: &GuardState,
    attempt: &HandoffAttempt,
) -> bool {
    let images = attempt
        .previous_images
        .clone()
        .unwrap_or_else(|| std::array::from_fn(|_| attempt.previous_image.clone()));
    for (i, service) in ["docker-guard", "updater", "updater-gateway"]
        .iter()
        .enumerate()
    {
        let Ok(actual) = super::startup::inspect_identity(
            &state.config,
            &format!("myriad-{service}"),
            service,
            true,
        )
        .await
        else {
            return false;
        };
        if actual.image != images[i] || (i == 1 && actual.version != attempt.previous_tag) {
            return false;
        }
    }
    true
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
                                state.mutation_gate.store(0, Ordering::SeqCst);
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

pub(crate) fn resume_staged_recovery(
    state: GuardState,
    attempt: HandoffAttempt,
    recovery_retries: u8,
) {
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
                    attempt.previous_images.as_ref(),
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
            let normal_stopped =
                !normal_exists || stop_helper(&docker_host, SELF_UPDATE_HELPER_NAME).await;
            if normal_exists && normal_stopped {
                let _ = cleanup_helper(&docker_host, SELF_UPDATE_HELPER_NAME).await;
            }
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
            if normal_stopped
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

async fn record_helper_failure_if_missing(
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
            serde_json::from_slice::<crate::docker::self_update_helper::SelfUpdateLastStatus>(
                &bytes,
            )
            .ok()
        })
        .is_some_and(|status| {
            status.target_tag == target_tag
                && matches!(
                    status.status,
                    crate::docker::self_update_helper::SelfUpdateOutcome::Failed
                )
                && chrono::DateTime::parse_from_rfc3339(&status.at)
                    .map(|at| at.with_timezone(&chrono::Utc) >= not_before)
                    .unwrap_or(false)
        });
    if already_recorded {
        return;
    }
    let status = crate::docker::self_update_helper::SelfUpdateLastStatus::failed_before_handoff(
        target_tag.to_owned(),
        previous_tag.to_owned(),
        error,
    );
    crate::state::atomic::write_json_until_saved(&path, &status).await;
}

pub(crate) fn recovery_attempt_from_status(state: &GuardState, attempt: &HandoffAttempt) -> u8 {
    std::fs::read(state.config.state_dir.join("self-update-last.json"))
        .ok()
        .and_then(|bytes| {
            serde_json::from_slice::<crate::docker::self_update_helper::SelfUpdateLastStatus>(
                &bytes,
            )
            .ok()
        })
        .filter(|status| {
            status.target_tag == attempt.target_tag && status.previous_tag == attempt.previous_tag
        })
        .map(|status| status.recovery_attempt.min(2))
        .unwrap_or(0)
}

pub(crate) fn persist_recovery_attempt(
    state: &GuardState,
    attempt: &HandoffAttempt,
    recovery_attempt: u8,
) -> bool {
    let path = state.config.state_dir.join("self-update-last.json");
    let mut status = std::fs::read(&path)
        .ok()
        .and_then(|bytes| {
            serde_json::from_slice::<crate::docker::self_update_helper::SelfUpdateLastStatus>(
                &bytes,
            )
            .ok()
        })
        .filter(|status| {
            status.target_tag == attempt.target_tag && status.previous_tag == attempt.previous_tag
        })
        .unwrap_or_else(|| {
            crate::docker::self_update_helper::SelfUpdateLastStatus::pending_before_handoff(
                attempt.target_tag.clone(),
                attempt.previous_tag.clone(),
            )
        });
    status.status = crate::docker::self_update_helper::SelfUpdateOutcome::Pending;
    status.recovery_attempt = recovery_attempt.min(2);
    crate::docker::self_update_helper::write_status(&path, &status).is_ok()
}

pub(crate) async fn stop_helper(docker_host: &str, helper_id: &str) -> bool {
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

pub(crate) async fn restart_helper(docker_host: &str, helper_id: &str) -> bool {
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

pub(crate) async fn cleanup_helper(docker_host: &str, helper_id: &str) -> bool {
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

pub(crate) async fn helper_container_exists(socket: &Path, helper_id: &str) -> Result<bool> {
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

pub(crate) async fn helper_container_running(socket: &Path, helper_id: &str) -> Result<bool> {
    validate_identifier(helper_id).map_err(anyhow::Error::msg)?;
    let inspect = daemon_json(socket, &format!("/containers/{helper_id}/json")).await?;
    inspect
        .pointer("/State/Running")
        .and_then(Value::as_bool)
        .ok_or_else(|| anyhow!("trusted helper has no running state"))
}

pub(crate) async fn helper_container_exit_code(
    socket: &Path,
    helper_id: &str,
) -> Result<Option<i64>> {
    validate_identifier(helper_id).map_err(anyhow::Error::msg)?;
    let inspect = daemon_json(socket, &format!("/containers/{helper_id}/json")).await?;
    helper_exit_code_from_inspect(&inspect)
}

pub(crate) fn helper_exit_code_from_inspect(inspect: &Value) -> Result<Option<i64>> {
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

pub(crate) async fn inspect_handoff_attempt(
    socket: &Path,
    helper_id: &str,
) -> Result<HandoffAttempt> {
    validate_identifier(helper_id).map_err(anyhow::Error::msg)?;
    let inspect = daemon_json(socket, &format!("/containers/{helper_id}/json")).await?;
    handoff_attempt_from_inspect(&inspect)
}

pub(crate) fn handoff_attempt_from_inspect(inspect: &Value) -> Result<HandoffAttempt> {
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
            crate::docker::self_update_helper::ENV_RECOVERY_ONLY
        ))
    }) {
        None => false,
        Some("1") => true,
        Some(_) => return Err(anyhow!("trusted handoff helper has invalid recovery mode")),
    };
    let attempt = HandoffAttempt {
        previous_image: required(crate::docker::self_update_helper::ENV_PREVIOUS_IMAGE)?,
        previous_images: env
            .iter()
            .filter_map(Value::as_str)
            .find_map(|entry| {
                entry.strip_prefix(&format!(
                    "{}=",
                    crate::docker::self_update_helper::ENV_PREVIOUS_IMAGES
                ))
            })
            .map(crate::docker::self_update_helper::parse_previous_images)
            .transpose()?,
        target_image: required(crate::docker::self_update_helper::ENV_TARGET_IMAGE)?,
        previous_tag: required(crate::docker::self_update_helper::ENV_PREVIOUS_TAG)?,
        target_tag: required(crate::docker::self_update_helper::ENV_TARGET_TAG)?,
        recovery_only,
    };
    validate_guard_image_ref(&attempt.previous_image, false)?;
    if attempt
        .previous_images
        .as_ref()
        .is_some_and(|images| images[0] != attempt.previous_image)
    {
        return Err(anyhow!(
            "previous Guard image differs from recovery snapshot"
        ));
    }
    validate_guard_image_ref(&attempt.target_image, false)?;
    validate_self_update_tag(&attempt.previous_tag).map_err(anyhow::Error::msg)?;
    validate_self_update_tag(&attempt.target_tag).map_err(anyhow::Error::msg)?;
    let configured_image = inspect
        .pointer("/Config/Image")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("trusted handoff helper has no configured image"))?;
    if !digest_reference_matches(configured_image, &attempt.target_image)
        && !(attempt.recovery_only
            && digest_reference_matches(configured_image, &attempt.previous_image))
    {
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
    let stack = super::startup::inspect_stack(&state.config, false).await?;
    let previous_image = stack[0].image.clone();
    let previous_tag = stack[1].version.clone();
    let previous_images = recovery_images(&stack);
    // A tag selects the target; the resolved digest fixes this operation's bytes.
    let exact_image =
        pull_trusted_tag_and_resolve(&state.config.socket_path, requested_tag).await?;
    let mut pending =
        crate::docker::self_update_helper::SelfUpdateLastStatus::pending_before_handoff(
            requested_tag.to_owned(),
            previous_tag.clone(),
        );
    pending.target_image = Some(exact_image.clone());
    crate::docker::self_update_helper::write_status(
        &state.config.state_dir.join("self-update-last.json"),
        &pending,
    )?;
    let attempt = HandoffAttempt {
        previous_image: previous_image.clone(),
        previous_images,
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
        attempt.previous_images.as_ref(),
    )
    .await?;
    Ok((helper_id, attempt))
}

pub(crate) fn recovery_images(stack: &[super::startup::RuntimeIdentity; 3]) -> Option<[String; 3]> {
    let images = std::array::from_fn(|i| stack[i].image.clone());
    if images.iter().all(|image| image == &images[0]) {
        None
    } else {
        Some(images)
    }
}

pub(crate) fn validate_self_update_tag(tag: &str) -> std::result::Result<(), String> {
    crate::version::validate_image_tag(tag)
}

async fn pull_trusted_tag_and_resolve(socket: &Path, target_tag: &str) -> Result<String> {
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
        .find_map(|actual| canonicalize_trusted_digest_ref(actual).ok())
        .ok_or_else(|| anyhow!("pulled image has no trusted updater repository digest"))?;
    Ok(exact_image)
}

async fn launch_trusted_handoff(
    state: &GuardState,
    previous_image: &str,
    target_image: &str,
    previous_tag: &str,
    target_tag: &str,
    recovery_only: bool,
    previous_images: Option<&[String; 3]>,
) -> Result<String> {
    launch_trusted_helper(
        state,
        previous_image,
        target_image,
        previous_tag,
        target_tag,
        recovery_only,
        false,
        previous_images,
    )
    .await
}

pub(crate) async fn wait_for_stopped_helper(state: &GuardState, name: &str) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    loop {
        if !helper_container_running(&state.config.socket_path, name).await? {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            let host = format!("unix://{}", state.config.socket_path.display());
            if stop_helper(&host, name).await
                && !helper_container_running(&state.config.socket_path, name).await?
            {
                return Ok(());
            }
            return Err(anyhow!("could not stop previous startup/recovery helper"));
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

pub(crate) async fn reconcile_runtime_policy(
    state: &GuardState,
    stack: &[super::startup::RuntimeIdentity; 3],
) -> Result<()> {
    let identity = &stack[0];
    let images = std::array::from_fn(|i| stack[i].image.clone());
    let name = super::STARTUP_RECONCILE_NAME;
    let host = format!("unix://{}", state.config.socket_path.display());
    if helper_container_exists(&state.config.socket_path, name).await? {
        // An interrupted reconciliation only writes verified identity; never
        // remove it while it is still executing.
        wait_for_stopped_helper(state, name).await?;
        if !cleanup_helper(&host, name).await {
            return Err(anyhow!("could not remove completed reconciliation helper"));
        }
    }
    launch_trusted_helper(
        state,
        &identity.image,
        &identity.image,
        &identity.version,
        &identity.version,
        false,
        true,
        Some(&images),
    )
    .await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    loop {
        if let Some(code) = helper_container_exit_code(&state.config.socket_path, name).await? {
            let _ = cleanup_helper(&host, name).await;
            if code != 0 {
                return Err(anyhow!("startup reconciliation helper exited with {code}"));
            }
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            let _ = stop_helper(&host, name).await;
            return Err(anyhow!("startup reconciliation helper timed out"));
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn launch_trusted_helper(
    state: &GuardState,
    previous_image: &str,
    target_image: &str,
    previous_tag: &str,
    target_tag: &str,
    recovery_only: bool,
    reconcile_only: bool,
    previous_images: Option<&[String; 3]>,
) -> Result<String> {
    let helper_name = if reconcile_only {
        super::STARTUP_RECONCILE_NAME
    } else if recovery_only {
        SELF_UPDATE_RECOVERY_NAME
    } else {
        SELF_UPDATE_HELPER_NAME
    };
    if helper_container_exists(&state.config.socket_path, helper_name).await? {
        if helper_container_exit_code(&state.config.socket_path, helper_name)
            .await?
            .is_none()
        {
            return Err(anyhow::Error::new(HandoffCleanupUnconfirmed));
        }
        let host = format!("unix://{}", state.config.socket_path.display());
        if !cleanup_helper(&host, helper_name).await {
            return Err(anyhow::Error::new(HandoffCleanupUnconfirmed));
        }
    }
    let socket = state.config.socket_path.to_string_lossy().into_owned();
    let docker_host = format!("unix://{socket}");
    let host_root = state.host_compose_root.to_string_lossy().into_owned();
    // Mount the deployment root at the *host* path inside the helper so the
    // Compose file it passes to `-f` is recorded in the container label
    // com.docker.compose.project.config_files as a host-valid path. An
    // in-container-only path (e.g. /host/compose) makes external tools such as
    // 1Panel look for a compose file that does not exist on the host.
    // `host_compose_root` is a Docker-daemon host source; the helper receives it
    // as an identity bind target, and the host source is passed to --mount.
    let env_path = state.config.compose_dir.join(".env");
    let state_path = &state.config.state_dir;
    if !env_path.is_file() || !state_path.is_dir() {
        return Err(anyhow!("fixed updater .env/state paths are unavailable"));
    }
    let policy_host_dir = state
        .host_compose_root
        .join("guard-policy")
        .to_string_lossy()
        .into_owned();

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
        &format!("type=bind,source={host_root},target={host_root},readonly"),
        "--mount",
        &format!("type=bind,source={host_root},target=/host/write"),
        "--mount",
        &format!("type=bind,source={policy_host_dir},target=/guard-policy"),
    ]);
    for (name, value) in [
        (
            crate::docker::self_update_helper::ENV_PREVIOUS_IMAGE,
            previous_image,
        ),
        (
            crate::docker::self_update_helper::ENV_TARGET_IMAGE,
            // v0.5.3 identifies a helper by TARGET_IMAGE == Config.Image.
            // Recovery runs the previous binary; keep that identity consistent.
            // Remove this compatibility constraint after the refactor's first release.
            if recovery_only {
                previous_image
            } else {
                target_image
            },
        ),
        (
            crate::docker::self_update_helper::ENV_PREVIOUS_TAG,
            previous_tag,
        ),
        (
            crate::docker::self_update_helper::ENV_TARGET_TAG,
            target_tag,
        ),
        (
            crate::docker::self_update_helper::ENV_PROJECT,
            &state.config.project,
        ),
        (
            crate::docker::self_update_helper::ENV_PROJECT_DIRECTORY,
            &host_root,
        ),
        (
            crate::docker::self_update_helper::ENV_HOST_COMPOSE_ROOT,
            &host_root,
        ),
        (
            crate::docker::self_update_helper::ENV_COMPOSE_DIR,
            &host_root,
        ),
        (
            crate::docker::self_update_helper::ENV_APP_ENV_FILE,
            "/host/write/.env",
        ),
        (
            crate::docker::self_update_helper::ENV_GUARD_ENV_FILE,
            POLICY_CONTAINER_FILE,
        ),
        (
            crate::docker::self_update_helper::ENV_STATUS_FILE,
            "/host/write/state/self-update-last.json",
        ),
        (
            crate::docker::self_update_helper::ENV_COMPOSE_NETWORK,
            &state.config.compose_network,
        ),
        (
            crate::docker::self_update_helper::ENV_ADMIN_NETWORK,
            &state.config.admin_network,
        ),
        (
            crate::docker::self_update_helper::ENV_GUARD_NETWORK,
            &state.config.guard_network,
        ),
    ] {
        command.arg("-e").arg(format!("{name}={value}"));
    }
    if recovery_only {
        command.arg("-e").arg(format!(
            "{}=1",
            crate::docker::self_update_helper::ENV_RECOVERY_ONLY
        ));
    }
    if reconcile_only {
        command.arg("-e").arg(format!(
            "{}=1",
            crate::docker::self_update_helper::ENV_RECONCILE_ONLY
        ));
    }
    if let Some(images) = previous_images {
        command.arg("-e").arg(format!(
            "{}={}",
            crate::docker::self_update_helper::ENV_PREVIOUS_IMAGES,
            serde_json::to_string(images)?
        ));
    }
    // Recovery must work even when the target helper cannot run.
    command.arg(if recovery_only {
        previous_image
    } else {
        target_image
    });
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
