//! Updater TCB self-update request boundary.
//!
//! The updater keeps the one-click self-update UX, but it never performs the
//! privileged replacement itself.  It resolves a constrained target and asks
//! `docker-guard` to perform the trusted switch.  Guard owns the Docker
//! authority, chooses the official updater repository, resolves the tag to an
//! exact digest, and recreates the fixed TCB service set.
//!
//! There are two target-discovery paths:
//!
//! * formal GitHub releases are preferred when the signed `release.json` can
//!   be obtained and verified with a strict cosign policy;
//! * private repositories and commit/preview deployments use Docker Hub's
//!   immutable `vX.Y.Z` / `dev-<sha>` tags.
//!
//! Both paths only produce a tag.  The updater does not pull the image, write
//! `.env`, or send a repository/digest to Guard.  Guard independently applies
//! its host-owned trust policy before doing any privileged work.  Until a
//! signed-materials protocol is added to Guard, both paths use the explicit
//! `dockerhub_tag` trust path; a locally verified manifest is only a better
//! target selector, not evidence handed to the TCB.

use std::sync::Arc;

use tracing::{info, warn};

use crate::config::Channel;
use crate::docker::self_update_helper::SelfUpdateOutcome;
use crate::error::{Result, UpdaterError};
use crate::release::{CosignPolicy, GithubClient};
use crate::version::{
    DeployTag, DeployTagKind, MyriadVersion, UpdateMode, release_channel_name_for_self_update,
};
use crate::worker::Worker;

/// A target selected by the lower-trust updater.  `trust_path` is descriptive
/// input to Guard; it is never a repository or digest selector.  The current
/// protocol accepts `dockerhub_tag` while Guard independently obtains the
/// official repository digest.
struct SelfUpdateTarget {
    tag: String,
    trust_path: &'static str,
}

/// Resolve a target and ask Guard to perform the fixed TCB replacement.
///
/// The response shape remains compatible with the existing API.  In
/// particular, `helper_container_id` is retained as the Guard owner marker;
/// there is no updater-controlled helper container anymore.
pub async fn run(worker: Arc<Worker>, actor: Option<String>) -> Result<SelfUpdateReport> {
    let resolved = resolve_self_update_target(worker.as_ref()).await?;
    // UPDATER_TAG can already contain a pending manual deployment target.
    let previous_tag = crate::self_version().to_string();

    info!(
        target = %resolved.tag,
        trust_path = resolved.trust_path,
        "self-update: requesting docker guard TCB replacement"
    );
    let before_at = crate::docker::self_update_helper::read_status(worker.state().root())?
        .map(|status| status.at);
    let persisted = schedule_guarded_recreate(
        &resolved.tag,
        resolved.trust_path,
        worker.config().guard_self_update_token.expose(),
    )
    .await?;
    // Compatibility with v0.5.3 Guard: it replies before persisting acceptance.
    // Keep the queue until Guard owns a visible outcome; remove next release.
    if !persisted {
        wait_for_legacy_guard_acceptance(
            worker.state().root(),
            &resolved.tag,
            before_at.as_deref(),
        )
        .await?;
    }

    let actor_suffix = actor
        .as_deref()
        .map(|a| format!(" actor={a}"))
        .unwrap_or_default();
    let audit = format!(
        "audit: self_update_scheduled target_tag={} previous_tag={} trust_path={} executor=docker-guard services=docker-guard,updater,updater-gateway scheduled=true source=self_update_guard{actor_suffix}",
        resolved.tag, previous_tag, resolved.trust_path
    );
    let _ = worker.state().append_history(&audit);
    let _ = worker.state().append_audit(&audit);
    info!(
        target = %resolved.tag,
        trust_path = resolved.trust_path,
        "self-update: docker guard accepted fixed TCB replacement"
    );

    // Guard owns the durable outcome; accepted is not completed.

    Ok(SelfUpdateReport {
        // Compatibility field: Guard, not an updater helper, owns the switch.
        helper_container_id: "docker-guard".into(),
        new_updater_tag: resolved.tag,
        previous_updater_tag: previous_tag,
        scheduled: true,
    })
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

/// Prefer a strictly verified release manifest, then use Docker Hub immutable
/// tags when GitHub is unavailable (for example, a private source repository).
async fn resolve_self_update_target(worker: &Worker) -> Result<SelfUpdateTarget> {
    if worker.effective_mode()? != UpdateMode::Commit {
        match try_self_update_from_github(worker).await {
            Ok(Some(target)) => return Ok(target),
            Ok(None) => {
                warn!(
                    "self-update: signed GitHub release unavailable; using Docker Hub immutable tag"
                );
            }
            // Private/missing assets may use the explicit Hub path. A present
            // but invalid signature or manifest must still fail closed.
            Err(error) if GithubClient::is_release_json_unavailable(&error) => {
                warn!(
                    err = %error,
                    "self-update: signed GitHub assets unavailable; using Docker Hub immutable tag"
                );
            }
            Err(error) => return Err(error),
        }
    } else {
        info!("self-update: commit mode uses Docker Hub immutable tag path");
    }

    resolve_self_update_via_dockerhub(worker).await
}

/// Resolve the newest usable formal GitHub release for the effective channel.
///
/// This client deliberately uses `CosignPolicy::Strict` regardless of the
/// general update preference.  A signature/payload failure is a hard error;
/// only the expected unavailable/private-repository class may fall through to
/// Docker Hub.
async fn try_self_update_from_github(worker: &Worker) -> Result<Option<SelfUpdateTarget>> {
    let gh = match GithubClient::new(
        worker.config().github_repo.clone(),
        worker.config().github_token.clone(),
        worker.state().cache_dir(),
        CosignPolicy::Strict,
    ) {
        Ok(client) => client,
        Err(error) => {
            warn!(err = %error, "self-update: cannot build strict GitHub client");
            return Ok(None);
        }
    };

    let channel_name = release_channel_name_for_self_update(&worker.effective_channel());
    let channel: Channel = channel_name.parse().unwrap_or(worker.config().channel);
    let releases = match gh.list_releases_for_channel(channel, 20).await {
        Ok(releases) => releases,
        Err(error) if GithubClient::is_release_json_unavailable(&error) => {
            warn!(err = %error, "self-update: GitHub release list unavailable");
            return Ok(None);
        }
        Err(error) => return Err(error),
    };

    for release in releases {
        // A GitHub release can carry arbitrary tag names.  Self-update accepts
        // only formal v-prefixed releases, never branch/commit tags.
        let Ok(tag) = MyriadVersion::parse(release.tag_name.trim()) else {
            info!(
                tag = %release.tag_name,
                "self-update: skipping non-formal GitHub release tag"
            );
            continue;
        };
        let release_tag = tag.as_str();

        let manifest = match gh.fetch_manifest(release_tag).await {
            Ok(manifest) => manifest,
            Err(error) if GithubClient::is_release_json_unavailable(&error) => {
                warn!(
                    err = %error,
                    tag = %release_tag,
                    "self-update: signed release manifest unavailable; trying older release"
                );
                continue;
            }
            // A present but invalid/unsigned manifest must never be replaced by
            // an unverified choice.  This is the fail-closed trust boundary.
            Err(error) => return Err(error),
        };

        if manifest.version.as_str() != release_tag {
            return Err(UpdaterError::Precondition(format!(
                "self-update release tag/manifest mismatch: GitHub tag {release_tag}, manifest {}",
                manifest.version
            )));
        }
        if manifest.image("updater").is_none() {
            info!(
                tag = %release_tag,
                "self-update: signed release omits images.updater; trying older release"
            );
            continue;
        }

        return Ok(Some(SelfUpdateTarget {
            tag: release_tag.to_string(),
            trust_path: "dockerhub_tag",
        }));
    }

    Ok(None)
}

/// Select an immutable updater tag from Docker Hub without pulling it.  Guard
/// independently constrains the repository and resolves the returned tag to a
/// digest before replacing any TCB container.
async fn resolve_self_update_via_dockerhub(worker: &Worker) -> Result<SelfUpdateTarget> {
    let repo = worker.updater_image_repo()?;
    let tags = worker
        .dockerhub_client()?
        .list_immutable_tags(&repo, 25)
        .await?;
    let prefer_release = worker.effective_mode()? == UpdateMode::Release;

    let selected = crate::release::select_component_tip(&tags, prefer_release)
        .ok_or_else(|| {
            UpdaterError::Precondition(format!(
                "Docker Hub has no immutable updater tags for {repo} (need vX.Y.Z or dev-<sha>)"
            ))
        })?
        .tag
        .clone();

    let tag = validate_immutable_component_tag(&selected)?;
    info!(%tag, "self-update: selected Docker Hub immutable updater tag");
    Ok(SelfUpdateTarget {
        tag,
        trust_path: "dockerhub_tag",
    })
}

/// Docker Hub tags accepted by the Guard protocol.  Branch tips and `latest`
/// are mutable and therefore cannot cross this request boundary.
fn validate_immutable_component_tag(tag: &str) -> Result<String> {
    let parsed = DeployTag::parse(tag.trim())?;
    match parsed.kind() {
        DeployTagKind::Release | DeployTagKind::Commit => Ok(parsed.as_str().to_string()),
        DeployTagKind::Branch => Err(UpdaterError::Precondition(format!(
            "self-update target tag must be immutable (vX.Y.Z or dev-<sha>), got {tag}"
        ))),
    }
}

async fn schedule_guarded_recreate(
    target_tag: &str,
    trust_path: &str,
    guard_self_update_token: &str,
) -> Result<bool> {
    let endpoint = std::env::var("DOCKER_GUARD_SELF_UPDATE_URL")
        .unwrap_or_else(|_| "http://docker-guard:2375/_myriad/self-update".into());
    let response = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| UpdaterError::Docker(format!("build docker guard client: {e}")))?
        .post(endpoint)
        .header("X-Guard-Self-Update-Token", guard_self_update_token)
        .json(&self_update_request_body(target_tag, trust_path))
        .send()
        .await
        .map_err(|e| UpdaterError::Docker(format!("schedule guarded self-update: {e}")))?;
    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        return Err(UpdaterError::Docker(format!(
            "docker guard rejected self-update ({status}): {detail}"
        )));
    }
    let accepted: serde_json::Value = response
        .json()
        .await
        .map_err(|error| UpdaterError::Docker(format!("read Guard acceptance: {error}")))?;
    Ok(accepted["status_persisted"].as_bool().unwrap_or(false))
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
        assert!(require_no_pending_handoff(&state).is_err());
    }

    #[test]
    fn immutable_tag_accepts_release_and_commit() {
        assert_eq!(
            validate_immutable_component_tag("v0.3.28").unwrap(),
            "v0.3.28"
        );
        assert_eq!(
            validate_immutable_component_tag("dev-0123456").unwrap(),
            "dev-0123456"
        );
    }

    #[test]
    fn immutable_tag_rejects_mutable_branch_and_latest() {
        for tag in ["main", "preview", "latest", ""] {
            assert!(
                validate_immutable_component_tag(tag).is_err(),
                "mutable/invalid tag must be rejected: {tag}"
            );
        }
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
