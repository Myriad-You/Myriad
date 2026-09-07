//! Trusted TCB self-update handoff owned by Guard.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use axum::body::{to_bytes, Body};
use axum::http::{header, Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use tokio::process::Command;
use tracing::{error, info, warn};

use crate::api::auth::constant_time_eq;
use crate::version::{DeployTag, DeployTagKind, MyriadVersion};

use super::config::{
    canonicalize_trusted_digest_ref, digest_reference_matches, validate_guard_image_ref,
};
use super::forward::{daemon_json, forward};
use super::{
    denial, validate_identifier, GuardState, DOCKER_API_TIMEOUT, POLICY_CONTAINER_FILE,
    SELF_UPDATE_EXHAUSTED_NAME, SELF_UPDATE_GATE, SELF_UPDATE_HELPER_NAME,
    SELF_UPDATE_RECOVERY_NAME, TRUSTED_UPDATER_REPOSITORY,
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
    pub(crate) target_image: String,
    pub(crate) previous_tag: String,
    pub(crate) target_tag: String,
    pub(crate) recovery_only: bool,
}

pub(crate) fn fail_exhausted_pending_handoff(state: &GuardState) {
    let path = state.config.state_dir.join("self-update-last.json");
    let Some(pending) = std::fs::read(&path).ok().and_then(|bytes| {
        serde_json::from_slice::<crate::docker::self_update_helper::SelfUpdateLastStatus>(&bytes).ok()
    }) else {
        return;
    };
    if !matches!(
        pending.status,
        crate::docker::self_update_helper::SelfUpdateOutcome::Pending
    ) {
        return;
    }
    let failed = crate::docker::self_update_helper::SelfUpdateLastStatus::failed_before_handoff(
        pending.target_tag,
        pending.previous_tag,
        "previous-digest recovery retries are exhausted; host recovery is required".into(),
    );
    if let Err(error) = crate::docker::self_update_helper::write_status(&path, &failed) {
        warn!(%error, "could not persist exhausted self-update outcome");
    }
}

pub(crate) async fn finalize_or_fail_orphaned_pending_handoff(state: &GuardState) -> bool {
    let path = state.config.state_dir.join("self-update-last.json");
    let Some(pending) = std::fs::read(&path).ok().and_then(|bytes| {
        serde_json::from_slice::<crate::docker::self_update_helper::SelfUpdateLastStatus>(&bytes).ok()
    }) else {
        return false;
    };
    if !matches!(
        pending.status,
        crate::docker::self_update_helper::SelfUpdateOutcome::Pending
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
        crate::docker::self_update_helper::SelfUpdateLastStatus::succeeded_after_handoff(
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
        crate::docker::self_update_helper::SelfUpdateLastStatus::failed_before_handoff(
            pending.target_tag,
            pending.previous_tag,
            "trusted handoff was interrupted; target TCB was not fully active".into(),
        )
    };
    if let Err(error) = crate::docker::self_update_helper::write_status(&path, &status) {
        warn!(%error, "could not persist interrupted self-update outcome");
    }
    tcb_consistent
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
                        crate::docker::self_update_helper::SelfUpdateLastStatus::failed_before_handoff(
                            task_target,
                            previous_tag,
                            error.to_string(),
                        );
                    if let Err(status_error) = crate::docker::self_update_helper::write_status(
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
                                crate::docker::self_update_helper::SelfUpdateLastStatus,
                            >(&bytes)
                            .ok()
                        })
                        .filter(|status| {
                            matches!(
                                status.status,
                                crate::docker::self_update_helper::SelfUpdateOutcome::Pending
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
                crate::docker::self_update_helper::SelfUpdateLastStatus::failed_before_handoff(
                    attempt.target_tag,
                    attempt.previous_tag,
                    "trusted handoff terminated; previous TCB restored".into(),
                )
            } else {
                crate::docker::self_update_helper::SelfUpdateLastStatus::succeeded_after_handoff(
                    attempt.target_tag,
                    attempt.previous_tag,
                )
            };
            if let Err(error) = crate::docker::self_update_helper::write_status(
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

pub(crate) fn resume_staged_recovery(state: GuardState, attempt: HandoffAttempt, recovery_retries: u8) {
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
            serde_json::from_slice::<crate::docker::self_update_helper::SelfUpdateLastStatus>(&bytes).ok()
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
    if let Err(status_error) = crate::docker::self_update_helper::write_status(&path, &status) {
        warn!(%status_error, "could not persist helper failure outcome");
    }
}

pub(crate) fn failed_status_matches_attempt(state: &GuardState, attempt: &HandoffAttempt) -> bool {
    std::fs::read(state.config.state_dir.join("self-update-last.json"))
        .ok()
        .and_then(|bytes| {
            serde_json::from_slice::<crate::docker::self_update_helper::SelfUpdateLastStatus>(&bytes).ok()
        })
        .is_some_and(|status| {
            matches!(
                status.status,
                crate::docker::self_update_helper::SelfUpdateOutcome::Failed
            ) && status.target_tag == attempt.target_tag
                && status.previous_tag == attempt.previous_tag
        })
}

pub(crate) fn recovery_attempt_from_status(state: &GuardState, attempt: &HandoffAttempt) -> u8 {
    std::fs::read(state.config.state_dir.join("self-update-last.json"))
        .ok()
        .and_then(|bytes| {
            serde_json::from_slice::<crate::docker::self_update_helper::SelfUpdateLastStatus>(&bytes).ok()
        })
        .filter(|status| {
            status.target_tag == attempt.target_tag && status.previous_tag == attempt.previous_tag
        })
        .map(|status| status.recovery_attempt.min(2))
        .unwrap_or(0)
}

pub(crate) fn recovery_is_durably_exhausted(
    completed_exit: Option<i64>,
    recovery_retries: u8,
    matching_failed_status: bool,
) -> bool {
    completed_exit.is_some_and(|code| code != 0)
        && (recovery_retries >= 2 || matching_failed_status)
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
            serde_json::from_slice::<crate::docker::self_update_helper::SelfUpdateLastStatus>(&bytes).ok()
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

pub(crate) async fn mark_recovery_exhausted(docker_host: &str, helper_id: &str) -> bool {
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

pub(crate) async fn helper_container_exit_code(socket: &Path, helper_id: &str) -> Result<Option<i64>> {
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

pub(crate) async fn wait_for_helper_absence(socket: &Path, helper_id: &str, timeout: Duration) -> bool {
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

pub(crate) async fn inspect_handoff_attempt(socket: &Path, helper_id: &str) -> Result<HandoffAttempt> {
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
        target_image: required(crate::docker::self_update_helper::ENV_TARGET_IMAGE)?,
        previous_tag: required(crate::docker::self_update_helper::ENV_PREVIOUS_TAG)?,
        target_tag: required(crate::docker::self_update_helper::ENV_TARGET_TAG)?,
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
    let pending = crate::docker::self_update_helper::SelfUpdateLastStatus::pending_before_handoff(
        requested_tag.to_owned(),
        previous_tag.clone(),
    );
    crate::docker::self_update_helper::write_status(
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

pub(crate) fn validate_self_update_tag(tag: &str) -> std::result::Result<(), String> {
    let parsed = DeployTag::parse(tag).map_err(|error| error.to_string())?;
    if matches!(parsed.kind(), DeployTagKind::Branch) {
        return Err("mutable branch tags are forbidden for TCB self-update".into());
    }
    Ok(())
}

pub(crate) fn prevent_release_downgrade(previous: &str, target: &str) -> Result<()> {
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
    // Image-baked ENV (Dockerfile), not Compose ${UPDATER_TAG}. Overlaying the
    // tag made digest-pinned TCB advertise a version it was not running.
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
        .find_map(|actual| canonicalize_trusted_digest_ref(actual).ok())
        .ok_or_else(|| anyhow!("pulled image has no trusted updater repository digest"))?;
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
        &format!("type=bind,source={host_root},target=/host/compose,readonly"),
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
        (crate::docker::self_update_helper::ENV_TARGET_IMAGE, target_image),
        (crate::docker::self_update_helper::ENV_PREVIOUS_TAG, previous_tag),
        (crate::docker::self_update_helper::ENV_TARGET_TAG, target_tag),
        (
            crate::docker::self_update_helper::ENV_PROJECT,
            &state.config.project,
        ),
        (
            crate::docker::self_update_helper::ENV_PROJECT_DIRECTORY,
            "/host/compose",
        ),
        (crate::docker::self_update_helper::ENV_HOST_COMPOSE_ROOT, &host_root),
        (crate::docker::self_update_helper::ENV_COMPOSE_DIR, "/host/compose"),
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
