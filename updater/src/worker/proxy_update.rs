//! Explicit proxy update: resolve from its image repository, pull, replace and
//! check health. Failure restores the previous running image automatically.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::env_file::EnvFile;
use crate::error::{Result, UpdaterError};
use crate::state::atomic;
use crate::worker::Worker;

/// File under the deployment state root.
pub const PROXY_UPDATE_LAST_FILE: &str = "proxy-update-last.json";

/// How long to wait for proxy `/healthz` after recreate before rolling back.
const PROXY_HEALTH_DEADLINE: Duration = Duration::from_secs(25);
const PROXY_HEALTH_INTERVAL: Duration = Duration::from_secs(1);
const PROXY_HEALTH_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Compose service DNS + container_name (either may resolve depending on network aliases).
const PROXY_HEALTH_URLS: &[&str] = &["http://proxy:80/healthz", "http://myriad-proxy:80/healthz"];

#[derive(Debug, Clone, Serialize)]
pub struct ProxyUpdateReport {
    pub previous_proxy_tag: String,
    pub new_proxy_tag: String,
    // Compatibility with v0.5.3 clients; remove next release.
    pub image_ref: String,
    // Compatibility with v0.5.3 clients; remove next release.
    pub pulled_digest: String,
    pub scheduled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProxyUpdateOutcome {
    Pending,
    Succeeded,
    Failed,
}

/// Durable last proxy-update outcome (`state/proxy-update-last.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyUpdateLastStatus {
    pub status: ProxyUpdateOutcome,
    pub target_tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_image: Option<String>,
    pub previous_tag: String,
    /// RFC3339 UTC.
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// True when compose failed or health failed and we restored `PROXY_TAG`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rolled_back: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_image_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_image: Option<String>,
}

impl ProxyUpdateLastStatus {
    pub fn succeeded(previous_tag: &str, target_tag: &str) -> Self {
        Self {
            status: ProxyUpdateOutcome::Succeeded,
            target_tag: target_tag.to_string(),
            target_image: None,
            previous_tag: previous_tag.to_string(),
            at: Utc::now().to_rfc3339(),
            error: None,
            rolled_back: false,
            previous_image_id: None,
            previous_image: None,
        }
    }

    pub fn failed(
        previous_tag: &str,
        target_tag: &str,
        error: impl Into<String>,
        rolled_back: bool,
    ) -> Self {
        Self {
            status: ProxyUpdateOutcome::Failed,
            target_tag: target_tag.to_string(),
            target_image: None,
            previous_tag: previous_tag.to_string(),
            at: Utc::now().to_rfc3339(),
            error: Some(error.into()),
            rolled_back,
            previous_image_id: None,
            previous_image: None,
        }
    }
}

pub fn write_proxy_update_last(state_root: &Path, status: &ProxyUpdateLastStatus) -> Result<()> {
    let path = state_root.join(PROXY_UPDATE_LAST_FILE);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    atomic::write_atomic_json(&path, status)
}

pub fn read_proxy_update_last(state_root: &Path) -> Result<Option<ProxyUpdateLastStatus>> {
    crate::state::read_json(&state_root.join(PROXY_UPDATE_LAST_FILE))
}

/// Admission and recovery read. Unlike [`read_proxy_update_last`], a corrupt
/// record is not reported as "no request": that would let a new mutation start
/// while the previous proxy update may still be running.
pub(crate) fn read_proxy_update_last_outcome(
    state_root: &Path,
) -> Result<crate::state::Outcome<ProxyUpdateLastStatus>> {
    crate::state::read_outcome(&state_root.join(PROXY_UPDATE_LAST_FILE))
}

pub(crate) fn require_no_pending(state: &crate::state::StateDir) -> Result<()> {
    match read_proxy_update_last_outcome(state.root())? {
        crate::state::Outcome::Present(status)
            if status.status == ProxyUpdateOutcome::Pending =>
        {
            Err(UpdaterError::Conflict)
        }
        crate::state::Outcome::Unreadable(error) => Err(UpdaterError::State(format!(
            "proxy update outcome is unreadable; refusing a new mutation: {error}"
        ))),
        _ => Ok(()),
    }
}

pub fn schedule(
    worker: Arc<Worker>,
    actor: Option<String>,
    target: Option<String>,
) -> Result<ProxyUpdateReport> {
    let env = EnvFile::load(&worker.cli().env_file)?;
    let previous = env.get("PROXY_TAG").unwrap_or_default();
    let mut pending =
        ProxyUpdateLastStatus::succeeded(previous, target.as_deref().unwrap_or_default());
    pending.status = ProxyUpdateOutcome::Pending;
    write_proxy_update_last(worker.state().root(), &pending)?;
    let report = ProxyUpdateReport {
        previous_proxy_tag: pending.previous_tag.clone(),
        new_proxy_tag: pending.target_tag.clone(),
        image_ref: String::new(),
        pulled_digest: String::new(),
        scheduled: true,
    };
    spawn_update(worker, actor, pending);
    Ok(report)
}

pub fn resume_pending(worker: Arc<Worker>) {
    match read_proxy_update_last_outcome(worker.state().root()) {
        Ok(crate::state::Outcome::Present(pending))
            if pending.status == ProxyUpdateOutcome::Pending =>
        {
            spawn_update(worker, None, pending)
        }
        Ok(crate::state::Outcome::Unreadable(error)) => warn!(
            %error,
            "proxy update outcome is unreadable; not resuming and not treating it as absent"
        ),
        Err(error) => warn!(%error, "cannot read proxy update state"),
        _ => {}
    }
}

fn spawn_update(worker: Arc<Worker>, actor: Option<String>, pending: ProxyUpdateLastStatus) {
    let owner = worker.clone();
    let task = tokio::spawn(async move {
        let outcome = match run(worker.clone(), actor, pending.clone()).await {
            Ok(outcome) => outcome,
            Err(error) => {
                warn!(%error, "proxy update failed before replacement");
                let last = read_proxy_update_last(worker.state().root())
                    .ok()
                    .flatten()
                    .unwrap_or(pending);
                ProxyUpdateLastStatus::failed(
                    &last.previous_tag,
                    &last.target_tag,
                    error.to_string(),
                    false,
                )
            }
        };
        crate::state::atomic::write_json_until_saved(
            &worker.state().root().join(PROXY_UPDATE_LAST_FILE),
            &outcome,
        )
        .await;
    });
    *owner.component_task.lock().unwrap() = Some(task);
}

/// Resolved proxy target before pull/rewrite.
struct ProxyTarget {
    tag: String,
    image_ref: String,
}

/// Upgrade the `proxy` service to the proxy image from the latest release for
/// the current channel (or to `explicit_tag` when provided).
async fn run(
    worker: Arc<Worker>,
    actor: Option<String>,
    mut pending: ProxyUpdateLastStatus,
) -> Result<ProxyUpdateLastStatus> {
    let resolved = if let Some(image_ref) = &pending.target_image {
        let target_id = worker.docker().image_id(image_ref).await?;
        if worker
            .docker()
            .container_ready("myriad-proxy", &target_id)
            .await
            .unwrap_or(false)
        {
            return Ok(ProxyUpdateLastStatus::succeeded(
                &pending.previous_tag,
                &pending.target_tag,
            ));
        }
        ProxyTarget {
            tag: pending.target_tag.clone(),
            image_ref: image_ref.clone(),
        }
    } else {
        let target = (!pending.target_tag.is_empty()).then(|| pending.target_tag.clone());
        resolve_proxy_target(worker.as_ref(), target).await?
    };
    info!(
        target = %resolved.tag,
        image = %resolved.image_ref,
        "proxy-update: resolved target"
    );

    let previous_tag = pending.previous_tag.clone();
    let previous_id = match pending.previous_image_id.clone() {
        Some(id) => id,
        None => worker
            .docker()
            .raw()
            .inspect_container("myriad-proxy", None)
            .await
            .map_err(|e| UpdaterError::Docker(format!("inspect previous proxy: {e}")))?
            .image
            .ok_or_else(|| UpdaterError::Docker("previous proxy image is missing".into()))?,
    };
    let repo = worker.proxy_image_repo()?;
    let previous_digest = match &pending.previous_image {
        Some(image) => image.clone(),
        None => worker
            .docker()
            .raw()
            .inspect_image(&previous_id)
            .await
            .map_err(|e| UpdaterError::Docker(format!("inspect previous proxy image: {e}")))?
            .repo_digests
            .unwrap_or_default()
            .into_iter()
            .find_map(|reference| {
                let (source, digest) = reference.rsplit_once('@')?;
                (source.trim_start_matches("docker.io/") == repo.trim_start_matches("docker.io/"))
                    .then(|| digest.to_owned())
            })
            .ok_or_else(|| {
                UpdaterError::Docker("previous proxy has no repository digest".into())
            })?,
    };
    pending.target_tag = resolved.tag.clone();
    pending.previous_image_id = Some(previous_id.clone());
    // Pulling a mutable tag can remove the old image's RepoDigests. Retain its
    // deployment reference before the pull so restart recovery uses the same image.
    pending.previous_image = Some(previous_digest.clone());
    write_proxy_update_last(worker.state().root(), &pending)?;

    info!(
        target = %resolved.tag,
        image = %resolved.image_ref,
        "proxy-update: pulling proxy image"
    );
    let pulled_digest = match &pending.target_image {
        Some(image) => image.clone(),
        None => {
            let digest = worker.pull_image(&resolved.image_ref).await.map_err(|e| {
                UpdaterError::Precondition(format_proxy_pull_error(&resolved.image_ref, &e))
            })?;
            let image = format!("{repo}@{}", digest.rsplit('@').next().unwrap_or(&digest));
            pending.target_image = Some(image.clone());
            write_proxy_update_last(worker.state().root(), &pending)?;
            image
        }
    };
    // Resolve all local prerequisites before mutating deployment intent.
    let compose = crate::worker::update::build_compose_runner_pub(&worker).await?;
    let target_id = worker.docker().image_id(&pulled_digest).await?;
    let apply = async {
        restore_proxy_tag(&resolved.tag, worker.as_ref())?;
        recreate_proxy(&compose, &resolved.tag, &pulled_digest).await?;
        wait_proxy_healthy(worker.as_ref(), &target_id).await
    }
    .await;
    if let Err(error) = apply {
        let recovery = async {
            restore_proxy_tag(&previous_tag, worker.as_ref())?;
            recreate_proxy(&compose, &previous_tag, &previous_digest).await?;
            wait_proxy_healthy(worker.as_ref(), &previous_id).await
        }
        .await;
        let rolled_back = recovery.is_ok();
        let detail = match recovery {
            Ok(()) => format!("proxy update failed: {error}; previous proxy restored and healthy"),
            Err(recovery) => format!("proxy update failed: {error}; recovery failed: {recovery}"),
        };
        return Ok(ProxyUpdateLastStatus::failed(
            &previous_tag,
            &resolved.tag,
            &detail,
            rolled_back,
        ));
    }

    let actor_suffix = actor
        .as_deref()
        .map(|a| format!(" actor={a}"))
        .unwrap_or_default();
    let audit = format!(
        "audit: proxy_update previous_tag={previous_tag} new_tag={} image={} digest={pulled_digest} source=dockerhub{actor_suffix}",
        resolved.tag, resolved.image_ref
    );
    let _ = worker.state().append_history(&audit);
    let _ = worker.state().append_audit(&audit);
    info!(%resolved.tag, "proxy-update: proxy recreated and healthy");

    Ok(ProxyUpdateLastStatus::succeeded(
        &previous_tag,
        &resolved.tag,
    ))
}

fn restore_proxy_tag(previous_tag: &str, worker: &Worker) -> Result<()> {
    if previous_tag.trim().is_empty() {
        return Err(UpdaterError::Precondition(
            "cannot restore PROXY_TAG: previous tag is empty".into(),
        ));
    }
    let mut env = EnvFile::load(&worker.cli().env_file)?;
    env.set("PROXY_TAG", previous_tag)?;
    env.save()?;
    Ok(())
}

/// Poll proxy `/healthz` (and container running) until deadline.
async fn wait_proxy_healthy(worker: &Worker, image_id: &str) -> Result<()> {
    let start = std::time::Instant::now();
    let mut last = String::from("no probe yet");
    while start.elapsed() < PROXY_HEALTH_DEADLINE {
        tokio::time::sleep(PROXY_HEALTH_INTERVAL).await;

        let running = match worker
            .docker()
            .raw()
            .inspect_container("myriad-proxy", None)
            .await
        {
            Ok(container) => {
                container.image.as_deref() == Some(image_id)
                    && container.state.as_ref().and_then(|state| state.running) == Some(true)
            }
            Err(error) => {
                last = format!("inspect proxy: {error}");
                continue;
            }
        };
        match probe_proxy_healthz(worker).await {
            Ok(()) if running => {
                info!(
                    elapsed_ms = start.elapsed().as_millis() as u64,
                    "proxy-update: healthz ok"
                );
                return Ok(());
            }
            Ok(()) => last = "proxy image/health does not match the expected deployment".into(),
            Err(e) => {
                last = if running {
                    format!("running but healthz failed: {e}")
                } else {
                    format!("not running / healthz failed: {e}")
                };
            }
        }
    }
    Err(UpdaterError::Precondition(format!(
        "proxy health probe exceeded {}s; last={last}",
        PROXY_HEALTH_DEADLINE.as_secs()
    )))
}

async fn recreate_proxy(
    compose: &crate::docker::ComposeRunner,
    tag: &str,
    digest: &str,
) -> Result<()> {
    let digest = digest.rsplit('@').next().unwrap_or(digest);
    let pinned_tag = format!("{tag}@{digest}");
    let output = compose
        .run_with_env(
            &[
                "up",
                "-d",
                "--no-deps",
                "--force-recreate",
                "--pull",
                "never",
                "proxy",
            ],
            Duration::from_secs(120),
            &[("PROXY_TAG", pinned_tag.as_str())],
        )
        .await?;
    if !output.ok() {
        return Err(UpdaterError::Docker(output.error_summary()));
    }
    Ok(())
}

async fn probe_proxy_healthz(worker: &Worker) -> Result<()> {
    let mut last_err = String::new();
    for url in PROXY_HEALTH_URLS {
        match worker
            .docker()
            .http_probe(url, PROXY_HEALTH_PROBE_TIMEOUT)
            .await
        {
            Ok((200, _)) => return Ok(()),
            Ok((code, body)) => {
                last_err = format!(
                    "{url} → HTTP {code}: {}",
                    body.chars().take(60).collect::<String>()
                );
            }
            Err(e) => {
                last_err = format!("{url}: {e}");
            }
        }
    }
    Err(UpdaterError::Docker(last_err))
}

async fn resolve_proxy_target(
    worker: &Worker,
    explicit_tag: Option<String>,
) -> Result<ProxyTarget> {
    let repo = worker.proxy_image_repo()?;
    let tag = worker.component_target(&repo, explicit_tag).await?;
    Ok(ProxyTarget {
        image_ref: format!("{repo}:{tag}"),
        tag,
    })
}

/// Operator-facing pull failure message. The Guard image policy is compiled into
/// the independently verified TCB, so an allowlist denial cannot be repaired by
/// editing updater-controlled runtime configuration.
fn format_proxy_pull_error(image_ref: &str, err: &UpdaterError) -> String {
    let detail = err.to_string();
    if is_docker_guard_allowlist_denial(&detail) {
        format!(
            "pull proxy {image_ref}: {detail} — the independently verified docker-guard policy does not trust this repository. \
Use the supported docker.io/somekawahitomi/myriad-proxy repository, or install a separately reviewed Guard build whose compiled policy explicitly trusts your repository."
        )
    } else {
        format!(
            "pull proxy {image_ref}: {detail} (is the tag published on the registry / Docker Hub?)"
        )
    }
}

fn is_docker_guard_allowlist_denial(detail: &str) -> bool {
    let lower = detail.to_ascii_lowercase();
    lower.contains("not allowlisted")
        || (lower.contains("status code 403") && lower.contains("allowlist"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v053_outcomes_remain_readable_without_new_recovery_fields() {
        let dir = tempfile::tempdir().unwrap();
        let state = crate::state::StateDir::open(dir.path()).unwrap();
        let path = dir.path().join(PROXY_UPDATE_LAST_FILE);
        for status in ["succeeded", "failed"] {
            let old = serde_json::json!({"status":status,"target_tag":"v0.5.3","previous_tag":"v0.5.2","at":"2026-09-21T02:20:54Z"});
            std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
            let loaded = read_proxy_update_last(dir.path()).unwrap().unwrap();
            assert_eq!(loaded.previous_image_id, None);
            assert!(require_no_pending(&state).is_ok());
        }
        std::fs::write(&path, b"{broken").unwrap();
        assert!(
            matches!(require_no_pending(&state), Err(UpdaterError::State(_))),
            "a corrupt outcome must fail closed, not look like no request"
        );
    }

    #[test]
    fn allowlist_denial_points_to_independent_guard_policy() {
        let err = UpdaterError::Docker(
            "pull stream: Docker responded with status code 403: image pull repository is not allowlisted"
                .into(),
        );
        let msg = format_proxy_pull_error("docker.io/somekawahitomi/myriad-proxy:v0.3.8", &err);
        assert!(
            msg.contains("independently verified docker-guard policy"),
            "expected independent-policy hint, got: {msg}"
        );
        assert!(
            !msg.contains("DOCKER_GUARD_ALLOWED_IMAGES"),
            "runtime allowlist must not be offered as a repair: {msg}"
        );
        assert!(
            msg.contains("myriad-proxy"),
            "expected proxy repo example, got: {msg}"
        );
        // Published-tag hint stays secondary, not the primary blame.
        assert!(
            !msg.contains("(is the tag published on the registry / Docker Hub?)"),
            "allowlist path should not lead with tag-missing framing: {msg}"
        );
    }

    #[test]
    fn other_pull_errors_keep_registry_hint() {
        let err = UpdaterError::Docker("pull stream: timeout waiting for registry".into());
        let msg = format_proxy_pull_error("docker.io/example/myriad-proxy:v0.3.8", &err);
        assert!(
            msg.contains("is the tag published on the registry / Docker Hub?"),
            "expected registry hint, got: {msg}"
        );
        assert!(
            !msg.contains("independently verified docker-guard policy"),
            "non-allowlist errors should not mention allowlist: {msg}"
        );
    }

    #[test]
    fn accepted_proxy_update_retains_recovery_identity_and_excludes_other_mutations() {
        let dir = tempfile::tempdir().unwrap();
        let state = crate::state::StateDir::open(dir.path()).unwrap();
        let mut pending = ProxyUpdateLastStatus::succeeded("v1.0.0", "v1.1.0");
        pending.status = ProxyUpdateOutcome::Pending;
        pending.previous_image_id = Some("old-content".into());
        write_proxy_update_last(dir.path(), &pending).unwrap();
        assert_eq!(
            read_proxy_update_last(dir.path()).unwrap().unwrap(),
            pending
        );
        assert!(matches!(
            require_no_pending(&state),
            Err(UpdaterError::Conflict)
        ));
        write_proxy_update_last(
            dir.path(),
            &ProxyUpdateLastStatus::failed("v1.0.0", "v1.1.0", "pull failed", false),
        )
        .unwrap();
        assert!(require_no_pending(&state).is_ok());
    }

    #[test]
    fn last_status_roundtrip_json() {
        let dir = tempfile::tempdir().unwrap();
        let failed = ProxyUpdateLastStatus::failed("v0.1.0", "v0.2.0", "health boom", true);
        write_proxy_update_last(dir.path(), &failed).unwrap();
        let loaded = read_proxy_update_last(dir.path())
            .unwrap()
            .expect("load last");
        assert_eq!(loaded.status, ProxyUpdateOutcome::Failed);
        assert_eq!(loaded.previous_tag, "v0.1.0");
        assert_eq!(loaded.target_tag, "v0.2.0");
        assert!(loaded.rolled_back);
        assert_eq!(loaded.error.as_deref(), Some("health boom"));

        let ok = ProxyUpdateLastStatus::succeeded("v0.1.0", "v0.2.0");
        write_proxy_update_last(dir.path(), &ok).unwrap();
        let loaded = read_proxy_update_last(dir.path())
            .unwrap()
            .expect("load ok");
        assert_eq!(loaded.status, ProxyUpdateOutcome::Succeeded);
        assert!(!loaded.rolled_back);
    }
}
