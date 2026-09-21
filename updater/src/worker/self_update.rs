//! Self-update admission and discovery. Guard alone replaces the stack.
use crate::docker::self_update_helper::{
    SelfUpdateLastStatus, SelfUpdateOutcome, read_status, write_status,
};
use crate::error::{Result, UpdaterError};
use crate::worker::Worker;
use std::sync::Arc;
use tracing::{info, warn};

pub fn schedule(worker: Arc<Worker>, actor: Option<String>) -> Result<SelfUpdateReport> {
    let mut pending =
        SelfUpdateLastStatus::pending_before_handoff(String::new(), crate::self_version().into());
    pending.queued = true;
    write_status(
        &worker.state().root().join("self-update-last.json"),
        &pending,
    )?;
    let report = SelfUpdateReport {
        helper_container_id: "docker-guard".into(),
        new_updater_tag: String::new(),
        previous_updater_tag: pending.previous_tag.clone(),
        scheduled: true,
    };
    spawn_request(worker, actor);
    Ok(report)
}

pub(crate) fn resume_pending(worker: Arc<Worker>) {
    match read_status(worker.state().root()) {
        Ok(Some(status)) if status.queued && status.status == SelfUpdateOutcome::Pending => {
            spawn_request(worker, None)
        }
        Err(error) => warn!(%error, "cannot read self-update request"),
        _ => {}
    }
}

fn spawn_request(worker: Arc<Worker>, actor: Option<String>) {
    let owner = worker.clone();
    let task = tokio::spawn(async move {
        if let Err(error) = request_handoff(&worker, actor).await {
            // A lost HTTP response does not undo Guard's persisted acceptance.
            if let Ok(Some(pending)) = read_status(worker.state().root())
                && pending.queued
            {
                let failed = SelfUpdateLastStatus::failed_before_handoff(
                    pending.target_tag,
                    pending.previous_tag,
                    error.to_string(),
                );
                crate::state::atomic::write_json_until_saved(
                    &worker.state().root().join("self-update-last.json"),
                    &failed,
                )
                .await;
            }
        }
    });
    *owner.component_task.lock().unwrap() = Some(task);
}

async fn request_handoff(worker: &Worker, actor: Option<String>) -> Result<()> {
    let mut pending = read_status(worker.state().root())?
        .ok_or_else(|| UpdaterError::State("self-update request is missing".into()))?;
    if pending.target_tag.is_empty() {
        let repo = worker.updater_image_repo()?;
        pending.target_tag = worker.component_target(&repo, None).await?;
        write_status(
            &worker.state().root().join("self-update-last.json"),
            &pending,
        )?;
    }
    let target = &pending.target_tag;
    let before_at = Some(pending.at.as_str());
    let endpoint = std::env::var("DOCKER_GUARD_SELF_UPDATE_URL")
        .unwrap_or_else(|_| "http://docker-guard:2375/_myriad/self-update".into());
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| UpdaterError::Docker(format!("build docker guard client: {e}")))?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(120);
    let mut response_lost = false;
    let persisted = loop {
        if read_status(worker.state().root())?.is_some_and(|s| !s.queued) {
            return Ok(());
        }
        let response = client
            .post(&endpoint)
            .header(
                "X-Guard-Self-Update-Token",
                worker.config().guard_self_update_token.expose(),
            )
            .json(&self_update_request_body(target, "dockerhub_tag"))
            .send()
            .await;
        match response {
            Ok(response) if response.status().is_success() => {
                // A successful response with a truncated body is still acceptance.
                let body: serde_json::Value = response.json().await.unwrap_or_default();
                break body["status_persisted"].as_bool().unwrap_or(false);
            }
            Ok(response) if response.status() == reqwest::StatusCode::CONFLICT => {
                // v0.5.3 writes its record after preparation. If our response was lost,
                // a busy Guard may already be executing this request. Remove next release.
                if response_lost {
                    break false;
                }
                if tokio::time::Instant::now() >= deadline {
                    return Err(UpdaterError::Conflict);
                }
            }
            Ok(response) => {
                let status = response.status();
                let detail = response.text().await.unwrap_or_default();
                return Err(UpdaterError::Docker(format!(
                    "docker guard rejected self-update ({status}): {detail}"
                )));
            }
            Err(error) => {
                response_lost |= !error.is_connect();
                if tokio::time::Instant::now() >= deadline {
                    if response_lost {
                        break false;
                    }
                    return Err(UpdaterError::Docker(format!(
                        "schedule guarded self-update: {error}"
                    )));
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    };
    // v0.5.3 accepts before writing status. Remove after the refactor's first release.
    if !persisted {
        wait_for_legacy_guard_acceptance(worker.state().root(), target, before_at).await?;
    }
    let audit = format!(
        "audit: self_update_scheduled target_tag={} actor={} executor=docker-guard",
        target,
        actor.as_deref().unwrap_or("")
    );
    let _ = worker.state().append_audit(&audit);
    info!(target = %target, "Guard accepted self-update");
    Ok(())
}

pub(crate) fn require_no_pending_handoff(state: &crate::state::StateDir) -> Result<()> {
    let Some(status) = crate::docker::self_update_helper::read_status(state.root())? else {
        return Ok(());
    };
    if status.status == SelfUpdateOutcome::Pending {
        return Err(UpdaterError::Conflict);
    }
    Ok(())
}

// Compatibility with v0.5.3; remove in the release after this refactor ships.
async fn wait_for_legacy_guard_acceptance(
    root: &std::path::Path,
    target: &str,
    before_at: Option<&str>,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(90 * 60);
    loop {
        if let Some(status) = crate::docker::self_update_helper::read_status(root)?
            && status.target_tag == target
            && Some(status.at.as_str()) != before_at
        {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(UpdaterError::Precondition(
                "Guard has not recorded self-update acceptance".into(),
            ));
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

fn self_update_request_body(target_tag: &str, trust_path: &str) -> serde_json::Value {
    serde_json::json!({
        "target_tag": target_tag,
        "trust_path": trust_path,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SelfUpdateReport {
    // Compatibility with v0.5.3 clients; remove next release.
    pub helper_container_id: String,
    pub new_updater_tag: String,
    pub previous_updater_tag: String,
    pub scheduled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_worker(root: &std::path::Path) -> Arc<Worker> {
        Arc::new(Worker::new(
            Arc::new(crate::state::StateDir::open(root).unwrap()),
            Arc::new(crate::docker::DockerClient::connect().await.unwrap()),
            crate::config::Config {
                update_token: crate::config::SecretString::new("test"),
                guard_self_update_token: crate::config::SecretString::new("test"),
                channel: crate::config::Channel::Stable,
                github_repo: "unused/repo".into(),
                github_token: None,
                check_interval_secs: 0,
                cosign_verify: "strict".into(),
            },
            crate::worker::WorkerCli {
                state_dir: root.into(),
                compose_dir: root.into(),
                env_file: root.join("missing.env"),
                pgdata: root.join("pgdata"),
                listen: "127.0.0.1:0".into(),
                db_mode: crate::config::DbMode::Bundled,
            },
        ))
    }

    #[tokio::test]
    #[ignore = "requires Docker daemon ping; does not change Docker state"]
    async fn acceptance_precedes_discovery_and_discovery_failure_has_an_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let worker = test_worker(dir.path()).await;
        let report = schedule(worker.clone(), None).unwrap();
        assert!(report.scheduled);
        let accepted = read_status(dir.path()).unwrap().unwrap();
        assert_eq!(accepted.status, SelfUpdateOutcome::Pending);
        assert!(accepted.queued);
        let task = worker.component_task.lock().unwrap().take().unwrap();
        task.await.unwrap();
        let failed = read_status(dir.path()).unwrap().unwrap();
        assert_eq!(failed.status, SelfUpdateOutcome::Failed);
        assert!(!failed.queued);
        assert!(worker.require_no_component_update().is_ok());
    }

    #[tokio::test]
    #[ignore = "requires Docker daemon ping; does not change Docker state"]
    async fn restart_resumes_unhanded_request_without_another_click() {
        let dir = tempfile::tempdir().unwrap();
        let worker = test_worker(dir.path()).await;
        let mut pending =
            SelfUpdateLastStatus::pending_before_handoff(String::new(), "v0.5.3".into());
        pending.queued = true;
        write_status(&dir.path().join("self-update-last.json"), &pending).unwrap();
        resume_pending(worker.clone());
        let task = worker.component_task.lock().unwrap().take().unwrap();
        task.await.unwrap();
        assert_eq!(
            read_status(dir.path()).unwrap().unwrap().status,
            SelfUpdateOutcome::Failed
        );
    }

    #[tokio::test]
    #[ignore = "requires Docker daemon ping; does not change Docker state"]
    async fn unreadable_history_does_not_release_a_running_component() {
        let dir = tempfile::tempdir().unwrap();
        let worker = test_worker(dir.path()).await;
        let path = dir.path().join("self-update-last.json");
        std::fs::write(path, b"broken old outcome").unwrap();
        *worker.component_task.lock().unwrap() = Some(tokio::spawn(std::future::pending()));
        assert!(matches!(
            worker.require_no_component_update(),
            Err(UpdaterError::Conflict)
        ));
        let task = worker.component_task.lock().unwrap().take().unwrap();
        task.abort();
        let _ = task.await;
        assert!(worker.require_no_component_update().is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn v053_guard_acceptance_waits_for_a_fresh_record_not_completion() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_owned();
        let path = root.join("self-update-last.json");
        let old = serde_json::json!({"status":"succeeded", "target_tag":"v0.5.4", "previous_tag":"v0.5.3", "at":"old"});
        std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
        let task = tokio::spawn(async move {
            wait_for_legacy_guard_acceptance(&root, "v0.5.4", Some("old")).await
        });
        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        let mut accepted = old;
        accepted["status"] = serde_json::json!("pending");
        accepted["at"] = serde_json::json!("new");
        std::fs::write(&path, serde_json::to_vec(&accepted).unwrap()).unwrap();
        tokio::time::advance(std::time::Duration::from_secs(1)).await;
        task.await.unwrap().unwrap();
        let state = crate::state::StateDir::open(dir.path()).unwrap();
        assert!(matches!(
            require_no_pending_handoff(&state),
            Err(UpdaterError::Conflict)
        ));
    }

    #[test]
    fn accepted_handoff_excludes_mutations_until_durable_terminal_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let state = crate::state::StateDir::open(&dir.path().join("state")).unwrap();
        assert!(require_no_pending_handoff(&state).is_ok());
        let path = state.root().join("self-update-last.json");
        let mut status = serde_json::json!({"status":"pending", "target_tag":"v0.5.2", "previous_tag":"v0.5.0", "at":"2026-09-21T00:00:00Z"});
        std::fs::write(&path, serde_json::to_vec(&status).unwrap()).unwrap();
        assert!(matches!(
            require_no_pending_handoff(&state),
            Err(UpdaterError::Conflict)
        ));
        status["status"] = serde_json::json!("succeeded");
        std::fs::write(&path, serde_json::to_vec(&status).unwrap()).unwrap();
        assert!(require_no_pending_handoff(&state).is_ok());
        std::fs::write(path, b"broken").unwrap();
        assert!(require_no_pending_handoff(&state).is_ok());
    }

    #[test]
    fn guard_request_contains_only_tag_and_trust_path() {
        let body = self_update_request_body("v0.3.28", "dockerhub_tag");
        assert_eq!(body["target_tag"], "v0.3.28");
        assert_eq!(body["trust_path"], "dockerhub_tag");
        assert!(body.get("repo").is_none());
        assert!(body.get("digest").is_none());
        assert!(body.get("previous_tag").is_none());
    }

    #[test]
    fn operator_does_not_disable_self_update() {
        let body = self_update_request_body("v0.3.28", "dockerhub_tag");
        assert_eq!(body["trust_path"], "dockerhub_tag");
    }
}
