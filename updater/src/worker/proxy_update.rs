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
//!   5. Probe `/healthz` on the compose network; on failure restore `PROXY_TAG`
//!      and recreate the previous proxy image
//!
//! Brief downtime (<10s) is expected while the edge container recreates.
//! Durable outcome: `state/proxy-update-last.json` (mirrors self-update-last).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::env_file::EnvFile;
use crate::error::{Result, UpdaterError};
use crate::release::GithubClient;
use crate::state::atomic;
use crate::version::{DeployTag, DeployTagKind, UpdateMode};
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
}

impl ProxyUpdateLastStatus {
    pub fn succeeded(previous_tag: &str, target_tag: &str) -> Self {
        Self {
            status: ProxyUpdateOutcome::Succeeded,
            target_tag: target_tag.to_string(),
            previous_tag: previous_tag.to_string(),
            at: Utc::now().to_rfc3339(),
            error: None,
            rolled_back: false,
            previous_image_id: None,
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
            previous_tag: previous_tag.to_string(),
            at: Utc::now().to_rfc3339(),
            error: Some(error.into()),
            rolled_back,
            previous_image_id: None,
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

pub(crate) fn require_no_pending(state: &crate::state::StateDir) -> Result<()> {
    if read_proxy_update_last(state.root())?
        .is_some_and(|s| s.status == ProxyUpdateOutcome::Pending)
    {
        return Err(UpdaterError::Conflict);
    }
    Ok(())
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
    match read_proxy_update_last(worker.state().root()) {
        Ok(Some(pending)) if pending.status == ProxyUpdateOutcome::Pending => {
            spawn_update(worker, None, pending)
        }
        Err(error) => warn!(%error, "cannot read proxy update state"),
        _ => {}
    }
}

fn spawn_update(worker: Arc<Worker>, actor: Option<String>, pending: ProxyUpdateLastStatus) {
    tokio::spawn(async move {
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
        while let Err(error) = write_proxy_update_last(worker.state().root(), &outcome) {
            warn!(%error, "retrying proxy outcome write");
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
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
async fn run(
    worker: Arc<Worker>,
    actor: Option<String>,
    mut pending: ProxyUpdateLastStatus,
) -> Result<ProxyUpdateLastStatus> {
    let target = (!pending.target_tag.is_empty()).then(|| pending.target_tag.clone());
    let resolved = resolve_proxy_target(worker.as_ref(), target).await?;
    info!(
        target = %resolved.tag,
        image = %resolved.image_ref,
        source = resolved.source,
        "proxy-update: resolved target"
    );

    let previous_tag = pending.previous_tag.clone();
    let previous_id = match pending.previous_image_id.clone() {
        Some(id) => id,
        None => {
            worker
                .docker()
                .image_id(&format!("{}:{previous_tag}", worker.proxy_image_repo()?))
                .await?
        }
    };
    pending.target_tag = resolved.tag.clone();
    pending.previous_image_id = Some(previous_id.clone());
    write_proxy_update_last(worker.state().root(), &pending)?;

    info!(
        target = %resolved.tag,
        image = %resolved.image_ref,
        "proxy-update: pulling proxy image"
    );
    let pulled_digest = worker.pull_image(&resolved.image_ref).await.map_err(|e| {
        UpdaterError::Precondition(format_proxy_pull_error(&resolved.image_ref, &e))
    })?;
    if let Some(expected) = &resolved.expected_digest
        && !pulled_digest.ends_with(expected)
        && pulled_digest != *expected
    {
        return Err(UpdaterError::Precondition(format!(
            "proxy digest mismatch: pulled {pulled_digest}, expected {expected}"
        )));
    }

    // Resolve all local prerequisites before mutating deployment intent.
    let compose = crate::worker::update::build_compose_runner_pub(&worker).await?;
    let target_id = worker.docker().image_id(&resolved.image_ref).await?;
    let apply = async {
        restore_proxy_tag(&resolved.tag, worker.as_ref())?;
        recreate_proxy(&compose).await?;
        wait_proxy_healthy(worker.as_ref(), &target_id).await
    }
    .await;
    if let Err(error) = apply {
        let recovery = async {
            restore_proxy_tag(&previous_tag, worker.as_ref())?;
            recreate_proxy(&compose).await?;
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
        "audit: proxy_update previous_tag={previous_tag} new_tag={} image={} digest={pulled_digest} source={}{actor_suffix}",
        resolved.tag, resolved.image_ref, resolved.source
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

async fn recreate_proxy(compose: &crate::docker::ComposeRunner) -> Result<()> {
    let output = compose.up_detached_recreate(&["proxy"]).await?;
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
    // 1) Prefer GitHub release.json when the target is a formal release (or channel tip).
    match try_proxy_from_github(worker, explicit_tag.as_deref()).await {
        Ok(Some(t)) => return Ok(t),
        Ok(None) => {
            warn!("proxy-update: GitHub release.json unavailable; falling back to Docker Hub");
        }
        Err(e) => return Err(e),
    }

    // Docker Hub remains the available source when GitHub discovery is unavailable.
    resolve_proxy_via_dockerhub(worker, explicit_tag).await
}

/// Attempt to resolve proxy from GitHub.
///
/// - `Ok(Some)` — manifest with proxy image
/// - `Ok(None)` — GitHub unavailable / no matching release / release omits
///   `images.proxy` (independent cadence); caller may fall back to Docker Hub
/// - `Err` — hard failure (cosign, invalid manifest with present asset)
async fn try_proxy_from_github(
    worker: &Worker,
    explicit_tag: Option<&str>,
) -> Result<Option<ProxyTarget>> {
    // Commit-mode immutable tips never have release.json; skip GitHub noise.
    if let Some(tag) = explicit_tag
        && let Ok(dt) = DeployTag::parse(tag)
        && dt.kind() == DeployTagKind::Commit
    {
        info!(
            tag = %tag,
            "proxy-update: commit tag; skipping GitHub, using Docker Hub path"
        );
        return Ok(None);
    }

    let gh = match worker.github_client() {
        Ok(gh) => gh,
        Err(e) => {
            warn!(err = %e, "proxy-update: cannot build GitHub client");
            return Ok(None);
        }
    };

    if let Some(tag) = explicit_tag {
        let manifest = match gh.fetch_manifest(tag).await {
            Ok(m) => m,
            Err(e) if GithubClient::is_release_json_unavailable(&e) => {
                warn!(err = %e, tag = %tag, "proxy-update: GitHub release.json unavailable");
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        let Some(proxy) = manifest.image("proxy") else {
            warn!(
                tag = %tag,
                "proxy-update: release omits images.proxy; falling back to Docker Hub"
            );
            return Ok(None);
        };
        return Ok(Some(ProxyTarget {
            tag: manifest.version.as_str().to_string(),
            image_ref: proxy.r#ref.clone(),
            expected_digest: Some(proxy.digest.clone()),
            source: "github",
        }));
    }

    let cfg = worker.config();
    let ch_name = crate::version::release_channel_name_for_self_update(&worker.effective_channel());
    let ch: crate::config::Channel = ch_name.parse().unwrap_or(cfg.channel);
    // Walk recent channel releases — app tags often omit proxy when unchanged.
    info!(
        channel = %ch,
        "proxy-update: looking up GitHub releases for channel (may skip releases without images.proxy)"
    );
    let releases = match gh.list_releases_for_channel(ch, 20).await {
        Ok(r) if !r.is_empty() => r,
        Ok(_) => {
            warn!(channel = %ch, "proxy-update: channel has no GitHub releases");
            return Ok(None);
        }
        Err(e) if GithubClient::is_release_json_unavailable(&e) => {
            warn!(err = %e, "proxy-update: GitHub list releases failed");
            return Ok(None);
        }
        Err(e) => return Err(e),
    };

    for rel in releases {
        let manifest = match gh.fetch_manifest(&rel.tag_name).await {
            Ok(m) => m,
            Err(e) if GithubClient::is_release_json_unavailable(&e) => {
                warn!(
                    err = %e,
                    tag = %rel.tag_name,
                    "proxy-update: GitHub release.json unavailable; trying older release"
                );
                continue;
            }
            Err(e) => return Err(e),
        };
        let Some(proxy) = manifest.image("proxy") else {
            info!(
                tag = %rel.tag_name,
                "proxy-update: release omits images.proxy; trying older release"
            );
            continue;
        };
        return Ok(Some(ProxyTarget {
            tag: manifest.version.as_str().to_string(),
            image_ref: proxy.r#ref.clone(),
            expected_digest: Some(proxy.digest.clone()),
            source: "github",
        }));
    }

    warn!(
        channel = %ch,
        "proxy-update: no recent GitHub release lists images.proxy"
    );
    Ok(None)
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
        let prefer_release = worker.effective_mode()? == UpdateMode::Release;
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
                let tip = crate::release::select_component_tip(&tags, prefer_release).ok_or_else(
                    || {
                        UpdaterError::Precondition(format!(
                            "Docker Hub has no immutable tags for {repo} (need dev-<sha> or vX.Y.Z)"
                        ))
                    },
                )?;
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
        assert!(require_no_pending(&state).is_err());
    }

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
