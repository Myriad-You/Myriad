//! Manual proxy image upgrade (spec §12.3).
//!
//! Proxy is not part of the automatic business update path. Operators trigger this
//! when a release ships a newer `images.proxy` (or when `PROXY_TAG` lags). Flow:
//!   1. Resolve target: prefer GitHub `release.json` `images.proxy`; on GitHub
//!      unavailable / 404 / commit-channel tip, fall back to Docker Hub
//!      `PROXY_IMAGE:<tag>` (same pattern as backend/frontend preflight).
//!   2. Pull proxy image and verify digest when present in manifest
//!   3. Rewrite `.env` `PROXY_TAG`
//!   4. `docker compose up -d --no-deps proxy` via the policy docker-guard
//!
//! Brief downtime (<10s) is expected while the edge container recreates.

use std::sync::Arc;

use tracing::{info, warn};

use crate::env_file::EnvFile;
use crate::error::{Result, UpdaterError};
use crate::release::GithubClient;
use crate::version::{DeployTag, DeployTagKind, UpdateMode};
use crate::worker::Worker;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProxyUpdateReport {
    pub previous_proxy_tag: String,
    pub new_proxy_tag: String,
    pub image_ref: String,
    pub pulled_digest: String,
}

/// Resolved proxy target before pull/rewrite.
struct ProxyTarget {
    tag: String,
    image_ref: String,
    /// Digest pin from release.json when available.
    expected_digest: Option<String>,
    source: &'static str,
}

/// Upgrade the `proxy` service to the proxy image from the latest release for
/// the current channel (or to `explicit_tag` when provided).
pub async fn run(
    worker: Arc<Worker>,
    actor: Option<String>,
    explicit_tag: Option<String>,
) -> Result<ProxyUpdateReport> {
    let resolved = resolve_proxy_target(worker.as_ref(), explicit_tag).await?;
    info!(
        target = %resolved.tag,
        image = %resolved.image_ref,
        source = resolved.source,
        "proxy-update: resolved target"
    );

    // Skip no-op when PROXY_TAG already matches and digest is already local.
    let previous_tag = {
        let env = EnvFile::load(&worker.cli().env_file)?;
        env.get("PROXY_TAG").unwrap_or_default().to_string()
    };
    if previous_tag == resolved.tag {
        info!(tag = %resolved.tag, "proxy-update: PROXY_TAG already at target; still recreating container");
    }

    info!(
        target = %resolved.tag,
        image = %resolved.image_ref,
        "proxy-update: pulling proxy image"
    );
    let pulled_digest = worker
        .docker_pull_with_mirror(&resolved.image_ref)
        .await
        .map_err(|e| UpdaterError::Precondition(format_proxy_pull_error(&resolved.image_ref, &e)))?;
    if let Some(expected) = &resolved.expected_digest {
        if !pulled_digest.ends_with(expected) && pulled_digest != *expected {
            return Err(UpdaterError::Precondition(format!(
                "proxy digest mismatch: pulled {pulled_digest}, expected {expected}"
            )));
        }
    }

    info!(new_tag = %resolved.tag, "proxy-update: rewriting PROXY_TAG in .env");
    {
        let mut env = EnvFile::load(&worker.cli().env_file)?;
        env.set("PROXY_TAG", &resolved.tag)?;
        env.save()?;
    }

    let compose = crate::worker::update::build_compose_runner_pub(&worker).await?;
    let up = compose.up_detached(&["proxy"]).await?;
    if !up.ok() {
        // Restore previous tag so a failed recreate does not leave .env advanced.
        let mut env = EnvFile::load(&worker.cli().env_file)?;
        env.set("PROXY_TAG", &previous_tag)?;
        env.save()?;
        return Err(UpdaterError::Internal(anyhow::anyhow!(
            "compose up proxy failed: {}",
            up.error_summary()
        )));
    }

    let actor_suffix = actor
        .as_deref()
        .map(|a| format!(" actor={a}"))
        .unwrap_or_default();
    let audit = format!(
        "audit: proxy_update previous_tag={previous_tag} new_tag={} image={} digest={pulled_digest} source={}{actor_suffix}",
        resolved.tag, resolved.image_ref, resolved.source
    );
    worker.state().append_history(&audit)?;
    let _ = worker.state().append_audit(&audit);
    info!(%resolved.tag, "proxy-update: proxy recreated");

    Ok(ProxyUpdateReport {
        previous_proxy_tag: previous_tag,
        new_proxy_tag: resolved.tag,
        image_ref: resolved.image_ref,
        pulled_digest,
    })
}

async fn resolve_proxy_target(
    worker: &Worker,
    explicit_tag: Option<String>,
) -> Result<ProxyTarget> {
    // 1) Prefer GitHub release.json when the target is a formal release (or channel tip).
    match try_proxy_from_github(worker, explicit_tag.as_deref()).await {
        Ok(Some(t)) => return Ok(t),
        Ok(None) => {
            warn!("proxy-update: GitHub release.json unavailable; falling back to Docker Hub");
        }
        Err(e) => return Err(e),
    }

    // 2) Docker Hub / env image repo + tag (commit tips and private-repo releases).
    resolve_proxy_via_dockerhub(worker, explicit_tag).await
}

/// Attempt to resolve proxy from GitHub.
///
/// - `Ok(Some)` — manifest with proxy image
/// - `Ok(None)` — GitHub unavailable / no matching release; caller may fall back
/// - `Err` — hard failure (cosign, invalid manifest with present asset, missing proxy image
///   when release.json *was* successfully fetched)
async fn try_proxy_from_github(
    worker: &Worker,
    explicit_tag: Option<&str>,
) -> Result<Option<ProxyTarget>> {
    // Commit-mode immutable tips never have release.json; skip GitHub noise.
    if let Some(tag) = explicit_tag {
        if let Ok(dt) = DeployTag::parse(tag) {
            if dt.kind() == DeployTagKind::Commit {
                info!(
                    tag = %tag,
                    "proxy-update: commit tag; skipping GitHub, using Docker Hub path"
                );
                return Ok(None);
            }
        }
    }

    let gh = match worker.github_client() {
        Ok(gh) => gh,
        Err(e) => {
            warn!(err = %e, "proxy-update: cannot build GitHub client");
            return Ok(None);
        }
    };

    let (tag, manifest) = if let Some(tag) = explicit_tag {
        match gh.fetch_manifest(tag).await {
            Ok(m) => (tag.to_string(), m),
            Err(e) if GithubClient::is_release_json_unavailable(&e) => {
                warn!(err = %e, tag = %tag, "proxy-update: GitHub release.json unavailable");
                return Ok(None);
            }
            Err(e) => return Err(e),
        }
    } else {
        let cfg = worker.config();
        let ch_name =
            crate::version::release_channel_name_for_self_update(&worker.effective_channel());
        let ch: crate::config::Channel = ch_name.parse().unwrap_or(cfg.channel);
        info!(channel = %ch, "proxy-update: looking up latest GitHub release for channel");
        let rel = match gh.latest_for_channel(ch).await {
            Ok(Some(r)) => r,
            Ok(None) => {
                warn!(channel = %ch, "proxy-update: channel has no GitHub releases");
                return Ok(None);
            }
            Err(e) if GithubClient::is_release_json_unavailable(&e) => {
                warn!(err = %e, "proxy-update: GitHub list releases failed");
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        match gh.fetch_manifest(&rel.tag_name).await {
            Ok(m) => (rel.tag_name, m),
            Err(e) if GithubClient::is_release_json_unavailable(&e) => {
                warn!(
                    err = %e,
                    tag = %rel.tag_name,
                    "proxy-update: GitHub release.json unavailable"
                );
                return Ok(None);
            }
            Err(e) => return Err(e),
        }
    };

    let proxy = manifest.image("proxy").ok_or_else(|| {
        UpdaterError::Precondition(format!(
            "release {tag} has no `proxy` image in the manifest"
        ))
    })?;
    Ok(Some(ProxyTarget {
        tag: manifest.version.as_str().to_string(),
        image_ref: proxy.r#ref.clone(),
        expected_digest: Some(proxy.digest.clone()),
        source: "github",
    }))
}

async fn resolve_proxy_via_dockerhub(
    worker: &Worker,
    explicit_tag: Option<String>,
) -> Result<ProxyTarget> {
    let repo = worker.proxy_image_repo()?;
    let tag = if let Some(tag) = explicit_tag {
        validate_immutable_component_tag(&tag)?;
        tag
    } else {
        // List proxy tags first. App `latest_available` may not exist on proxy repo.
        let prefer_release = worker.effective_mode() == UpdateMode::Release;
        let tags = worker
            .dockerhub_client()?
            .list_immutable_tags(&repo, 25)
            .await?;
        if let Some(from_state) = tip_tag_from_state(worker)? {
            if tags.iter().any(|t| t.tag == from_state) {
                info!(
                    tag = %from_state,
                    "proxy-update: state tip exists on proxy repo; using it"
                );
                from_state
            } else {
                let tip = crate::release::select_component_tip(&tags, prefer_release)
                    .ok_or_else(|| {
                        UpdaterError::Precondition(format!(
                            "Docker Hub has no immutable tags for {repo} (need dev-<sha> or vX.Y.Z)"
                        ))
                    })?;
                info!(
                    state_tip = %from_state,
                    tag = %tip.tag,
                    kind = tip.kind,
                    "proxy-update: state tip missing on proxy repo; selected component tip"
                );
                tip.tag.clone()
            }
        } else {
            let tip =
                crate::release::select_component_tip(&tags, prefer_release).ok_or_else(|| {
                    UpdaterError::Precondition(format!(
                        "Docker Hub has no immutable tags for {repo} (need dev-<sha> or vX.Y.Z)"
                    ))
                })?;
            info!(
                tag = %tip.tag,
                kind = tip.kind,
                "proxy-update: selected tip from Docker Hub proxy tags"
            );
            tip.tag.clone()
        }
    };

    if tag.ends_with(":latest") || tag == "latest" {
        return Err(UpdaterError::Precondition(
            "proxy image tag must be immutable (dev-<sha> or vX.Y.Z), got latest".into(),
        ));
    }

    Ok(ProxyTarget {
        image_ref: format!("{repo}:{tag}"),
        tag,
        expected_digest: None,
        source: "dockerhub",
    })
}

fn tip_tag_from_state(worker: &Worker) -> Result<Option<String>> {
    let st = worker.state().read_updater()?;
    Ok(st
        .latest_available
        .as_ref()
        .map(|la| la.version.as_str().to_string())
        .filter(|t| DeployTag::parse(t).is_ok_and(|d| d.kind() != DeployTagKind::Branch)))
}

fn validate_immutable_component_tag(tag: &str) -> Result<()> {
    let tag = tag.trim();
    if tag.is_empty() {
        return Err(UpdaterError::InvalidInput("empty proxy target tag".into()));
    }
    match DeployTag::parse(tag) {
        Ok(d) if d.kind() == DeployTagKind::Branch => Err(UpdaterError::InvalidInput(format!(
            "proxy target {tag} is a mutable branch tip; use dev-<sha> or vX.Y.Z"
        ))),
        Ok(_) => Ok(()),
        Err(e) => Err(UpdaterError::InvalidInput(format!(
            "invalid proxy target tag {tag}: {e}"
        ))),
    }
}

/// Operator-facing pull failure message. Allowlist denials are far more common
/// than a missing Docker Hub tag after a release, so surface docker-guard config
/// first when the error text indicates that.
fn format_proxy_pull_error(image_ref: &str, err: &UpdaterError) -> String {
    let detail = err.to_string();
    if is_docker_guard_allowlist_denial(&detail) {
        format!(
            "pull proxy {image_ref}: {detail} — docker-guard DOCKER_GUARD_ALLOWED_IMAGES must include the proxy repository \
(e.g. docker.io/somekawahitomi/myriad-proxy or your PROXY_IMAGE). After changing compose env, recreate docker-guard: \
`docker compose up -d --force-recreate docker-guard`. If the allowlist already includes proxy, confirm the tag is published on the registry / Docker Hub."
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
    fn rejects_branch_tip_tags() {
        assert!(validate_immutable_component_tag("preview").is_err());
        assert!(validate_immutable_component_tag("main").is_err());
    }

    #[test]
    fn accepts_commit_and_release_tags() {
        assert!(validate_immutable_component_tag("dev-abc1234").is_ok());
        assert!(validate_immutable_component_tag("v0.3.6").is_ok());
    }

    #[test]
    fn allowlist_denial_mentions_docker_guard_config() {
        let err = UpdaterError::Docker(
            "pull stream: Docker responded with status code 403: image pull repository is not allowlisted"
                .into(),
        );
        let msg = format_proxy_pull_error(
            "docker.io/somekawahitomi/myriad-proxy:v0.3.8",
            &err,
        );
        assert!(
            msg.contains("DOCKER_GUARD_ALLOWED_IMAGES"),
            "expected allowlist hint, got: {msg}"
        );
        assert!(
            msg.contains("force-recreate docker-guard"),
            "expected recreate hint, got: {msg}"
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
            !msg.contains("DOCKER_GUARD_ALLOWED_IMAGES"),
            "non-allowlist errors should not mention allowlist: {msg}"
        );
    }
}
