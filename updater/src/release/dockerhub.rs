//! Docker Hub tag lookup used as a fallback when GitHub commit metadata is unavailable.
//!
//! A deployable build exists only when the same immutable tag is present in both the
//! backend and frontend repositories:
//! - `dev-<sha>` commit builds
//! - formal release tags `vX.Y.Z`
//!
//! Branch-tip tags are deliberately ignored because they are mutable and therefore
//! unsuitable for update/rollback history.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::error::{Result, UpdaterError};
use crate::release::github::Freshness;
use crate::version::{DeployTag, DeployTagKind};

const DEFAULT_BASE_URL: &str = "https://hub.docker.com";

#[derive(Debug, Clone, PartialEq, Eq)]
struct DockerHubRepository {
    namespace: String,
    repository: String,
}

impl DockerHubRepository {
    fn parse(image: &str) -> Result<Self> {
        let image = image.trim();
        if image.is_empty() {
            return Err(UpdaterError::DockerHub("empty image repository".into()));
        }

        let without_digest = image.split('@').next().unwrap_or(image);
        let mut path = without_digest
            .strip_prefix("docker.io/")
            .or_else(|| without_digest.strip_prefix("index.docker.io/"))
            .or_else(|| without_digest.strip_prefix("registry-1.docker.io/"))
            .unwrap_or(without_digest);

        // Strip an accidental tag while preserving registry ports in unsupported hosts.
        if let Some((prefix, suffix)) = path.rsplit_once(':') {
            if !suffix.contains('/') {
                path = prefix;
            }
        }

        let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
        if parts.len() > 1
            && parts.first().is_some_and(|host| {
                host.contains('.') || host.contains(':') || *host == "localhost"
            })
        {
            return Err(UpdaterError::DockerHub(format!(
                "image repository {image} is not hosted on Docker Hub"
            )));
        }
        let (namespace, repository) = match parts.as_slice() {
            [repository] => ("library", *repository),
            [namespace, repository] => (*namespace, *repository),
            _ => {
                return Err(UpdaterError::DockerHub(format!(
                    "unsupported Docker Hub image repository: {image}"
                )));
            }
        };

        if namespace.is_empty() || repository.is_empty() {
            return Err(UpdaterError::DockerHub(format!(
                "invalid Docker Hub image repository: {image}"
            )));
        }

        Ok(Self {
            namespace: namespace.to_string(),
            repository: repository.to_string(),
        })
    }

    fn tags_url(&self, base_url: &str, page_size: usize) -> String {
        format!(
            "{}/v2/repositories/{}/{}/tags?page_size={}&ordering=last_updated",
            base_url.trim_end_matches('/'),
            self.namespace,
            self.repository,
            page_size
        )
    }

    fn browser_url(&self, tag: &str) -> String {
        format!(
            "https://hub.docker.com/r/{}/{}/tags?name={}",
            self.namespace, self.repository, tag
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DockerBuild {
    pub tag: String,
    /// For commit builds: short sha fragment. For release builds: the version tag itself.
    pub short_sha: String,
    /// `commit` (`dev-<sha>`) or `release` (`vX.Y.Z`).
    pub kind: &'static str,
    pub pushed_at: Option<String>,
    pub backend_digest: Option<String>,
    pub frontend_digest: Option<String>,
    pub backend_url: String,
    pub frontend_url: String,
}

/// Single-component immutable tag from Docker Hub (proxy / updater fallback).
#[derive(Debug, Clone, Serialize)]
pub struct ComponentTag {
    pub tag: String,
    /// `commit` (`dev-<sha>`) or `release` (`vX.Y.Z`).
    pub kind: &'static str,
    pub pushed_at: Option<String>,
    pub digest: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TagPage {
    #[serde(default)]
    results: Vec<TagResult>,
}

#[derive(Debug, Clone, Deserialize)]
struct TagResult {
    name: String,
    #[serde(default)]
    digest: Option<String>,
    #[serde(default)]
    last_updated: Option<String>,
    #[serde(default)]
    tag_last_pushed: Option<String>,
    #[serde(default)]
    images: Vec<TagImage>,
}

impl TagResult {
    fn pushed_at(&self) -> Option<String> {
        self.tag_last_pushed
            .clone()
            .or_else(|| self.last_updated.clone())
    }

    fn preferred_digest(&self) -> Option<String> {
        self.preferred_digest_for_arch(host_docker_arch())
    }

    /// Prefer tag-level digest (usually the multi-arch index), else the linux
    /// image matching `arch` (Docker Hub arch names: amd64 / arm64 / …).
    fn preferred_digest_for_arch(&self, arch: &str) -> Option<String> {
        self.digest.clone().or_else(|| {
            self.images
                .iter()
                .find(|image| {
                    image.os.as_deref() == Some("linux")
                        && image.architecture.as_deref() == Some(arch)
                })
                .or_else(|| {
                    self.images
                        .iter()
                        .find(|image| image.os.as_deref() == Some("linux"))
                })
                .or_else(|| self.images.first())
                .and_then(|image| image.digest.clone())
        })
    }
}

/// Map rustc `std::env::consts::ARCH` to Docker Hub / OCI architecture names.
fn host_docker_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "arm" => "arm",
        other => other,
    }
}

#[derive(Debug, Clone, Deserialize)]
struct TagImage {
    #[serde(default)]
    architecture: Option<String>,
    #[serde(default)]
    os: Option<String>,
    #[serde(default)]
    digest: Option<String>,
}

pub struct DockerHubClient {
    client: Client,
    base_url: String,
}

impl DockerHubClient {
    pub fn new() -> Result<Self> {
        Self::with_base_url(DEFAULT_BASE_URL)
    }

    fn with_base_url(base_url: impl Into<String>) -> Result<Self> {
        let client = Client::builder()
            .user_agent("myriad-updater")
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| UpdaterError::DockerHub(format!("client build: {error}")))?;
        Ok(Self {
            client,
            base_url: base_url.into(),
        })
    }

    pub async fn list_common_builds(
        &self,
        backend_image: &str,
        frontend_image: &str,
        limit: u32,
    ) -> Result<Vec<DockerBuild>> {
        let backend = DockerHubRepository::parse(backend_image)?;
        let frontend = DockerHubRepository::parse(frontend_image)?;
        let page_size = ((limit.max(1) as usize) * 4).clamp(50, 100);

        let (backend_page, frontend_page) = tokio::join!(
            self.fetch_tags(&backend, page_size),
            self.fetch_tags(&frontend, page_size)
        );
        let backend_page = backend_page?;
        let frontend_page = frontend_page?;

        Ok(common_builds(
            &backend,
            backend_page.results,
            &frontend,
            frontend_page.results,
            limit.max(1) as usize,
        ))
    }

    /// List immutable tags (`dev-<sha>` / `vX.Y.Z`) for a single Docker Hub image, newest first.
    ///
    /// Used by proxy/self-update when GitHub release.json is unavailable and no explicit
    /// target tag was provided.
    pub async fn list_immutable_tags(&self, image: &str, limit: u32) -> Result<Vec<ComponentTag>> {
        let repo = DockerHubRepository::parse(image)?;
        let page_size = ((limit.max(1) as usize) * 4).clamp(50, 100);
        let page = self.fetch_tags(&repo, page_size).await?;
        Ok(immutable_component_tags(
            page.results,
            limit.max(1) as usize,
        ))
    }

    async fn fetch_tags(
        &self,
        repository: &DockerHubRepository,
        page_size: usize,
    ) -> Result<TagPage> {
        let url = repository.tags_url(&self.base_url, page_size);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|error| UpdaterError::DockerHub(format!("GET {url}: {error}")))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(UpdaterError::DockerHub(format!(
                "GET {url} failed: {status} {}",
                body.chars().take(200).collect::<String>()
            )));
        }
        response
            .json::<TagPage>()
            .await
            .map_err(|error| UpdaterError::DockerHub(format!("decode {url}: {error}")))
    }
}

fn immutable_component_tags(tags: Vec<TagResult>, limit: usize) -> Vec<ComponentTag> {
    let mut out: Vec<ComponentTag> = tags
        .into_iter()
        .filter_map(|tag| {
            let deploy_tag = DeployTag::parse(&tag.name).ok()?;
            let kind = match deploy_tag.kind() {
                DeployTagKind::Commit => "commit",
                DeployTagKind::Release => "release",
                DeployTagKind::Branch => return None,
            };
            Some(ComponentTag {
                tag: tag.name.clone(),
                kind,
                pushed_at: tag.pushed_at(),
                digest: tag.preferred_digest(),
            })
        })
        .collect();
    out.sort_by(|left, right| right.pushed_at.cmp(&left.pushed_at));
    out.truncate(limit);
    out
}

/// Prefer a tag matching the effective update mode for proxy/self Docker Hub fallback.
///
/// - Commit/dev: newest immutable tag overall (dev-* or v*), same tip policy as backend.
/// - Release: newest formal `vX.Y.Z` release tag; if none, fall through to newest immutable.
pub fn select_component_tip(tags: &[ComponentTag], prefer_release: bool) -> Option<&ComponentTag> {
    if prefer_release {
        if let Some(rel) = tags.iter().find(|t| t.kind == "release") {
            return Some(rel);
        }
    }
    tags.first()
}

fn common_builds(
    backend_repo: &DockerHubRepository,
    backend_tags: Vec<TagResult>,
    frontend_repo: &DockerHubRepository,
    frontend_tags: Vec<TagResult>,
    limit: usize,
) -> Vec<DockerBuild> {
    let frontend_by_tag: HashMap<String, TagResult> = frontend_tags
        .into_iter()
        .map(|tag| (tag.name.clone(), tag))
        .collect();

    let mut builds: Vec<DockerBuild> = backend_tags
        .into_iter()
        .filter_map(|backend| {
            let deploy_tag = DeployTag::parse(&backend.name).ok()?;
            let (kind, short_sha) = match deploy_tag.kind() {
                DeployTagKind::Commit => ("commit", deploy_tag.commit_sha()?.to_string()),
                DeployTagKind::Release => ("release", deploy_tag.as_str().to_string()),
                DeployTagKind::Branch => return None,
            };
            let frontend = frontend_by_tag.get(&backend.name)?;
            let pushed_at = match (backend.pushed_at(), frontend.pushed_at()) {
                (Some(left), Some(right)) => Some(left.min(right)),
                (left, right) => left.or(right),
            };
            Some(DockerBuild {
                short_sha,
                tag: backend.name.clone(),
                kind,
                pushed_at,
                backend_digest: backend.preferred_digest(),
                frontend_digest: frontend.preferred_digest(),
                backend_url: backend_repo.browser_url(&backend.name),
                frontend_url: frontend_repo.browser_url(&backend.name),
            })
        })
        .collect();

    builds.sort_by(|left, right| right.pushed_at.cmp(&left.pushed_at));
    builds.truncate(limit);
    builds
}

/// Result of comparing a commit/dev target to the currently running deploy.
///
/// Dev channel primarily uses **build publish time** (Docker Hub `pushed_at`).
/// Clear git ancestry is used when available; `unknown` ancestry alone must not
/// block upgrade / auto-install when the target build is newer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitUpgradeDirection {
    pub is_upgrade: bool,
    pub is_downgrade: bool,
    /// UI/cache relation string: ahead | behind | identical | diverged | unknown.
    pub relation: &'static str,
}

impl CommitUpgradeDirection {
    pub fn identical() -> Self {
        Self {
            is_upgrade: false,
            is_downgrade: false,
            relation: "identical",
        }
    }

    pub fn none_actionable() -> Self {
        Self {
            is_upgrade: false,
            is_downgrade: false,
            relation: "identical",
        }
    }
}

fn parse_push_time(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw.trim())
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|| {
            // Docker Hub sometimes omits subseconds / offset variants — try loose parse.
            chrono::NaiveDateTime::parse_from_str(raw.trim(), "%Y-%m-%dT%H:%M:%S%.fZ")
                .ok()
                .map(|n| n.and_utc())
                .or_else(|| {
                    chrono::NaiveDateTime::parse_from_str(raw.trim(), "%Y-%m-%dT%H:%M:%SZ")
                        .ok()
                        .map(|n| n.and_utc())
                })
        })
}

/// Normalize a deploy-tag / sha fragment for identity comparison.
///
/// Runtime health stamps commit builds as `dev-<full40>` while Docker Hub tags
/// use `dev-<short7>` (metadata-action `format=short`). Treat prefix-equal shas
/// as the same identity so tip discovery does not miss upgrades or thrash.
pub fn deploy_sha_fragment(tag_or_sha: &str) -> Option<String> {
    let raw = tag_or_sha.trim().to_ascii_lowercase();
    if raw.is_empty() {
        return None;
    }
    let sha = raw.strip_prefix("dev-").unwrap_or(raw.as_str()).trim();
    if sha.len() >= 7 && sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(sha.to_string())
    } else {
        None
    }
}

/// True when two tag/sha strings refer to the same commit image (short ↔ full).
pub fn same_commit_identity(a: &str, b: &str) -> bool {
    match (deploy_sha_fragment(a), deploy_sha_fragment(b)) {
        (Some(left), Some(right)) => {
            left == right || left.starts_with(&right) || right.starts_with(&left)
        }
        _ => a.trim().eq_ignore_ascii_case(b.trim()),
    }
}

/// True when the running deploy and a Docker Hub tip are the same artifact.
///
/// Handles:
/// - exact tag match
/// - short/long `dev-<sha>` (runtime vs Hub)
/// - formal `vX.Y.Z` vs `dev-<sha>` when `current_commit_sha` matches the tip sha
pub fn same_deploy_identity(
    target_tag: &str,
    current_tag: Option<&str>,
    current_commit_sha: Option<&str>,
    tip_commit_sha: Option<&str>,
) -> bool {
    let target_tag = target_tag.trim();
    if target_tag.is_empty() {
        return false;
    }
    if let Some(cur) = current_tag.map(str::trim).filter(|s| !s.is_empty()) {
        if cur.eq_ignore_ascii_case(target_tag) || same_commit_identity(cur, target_tag) {
            return true;
        }
    }
    // Cross-kind: running formal release stamped with commit_sha of tip (or vice versa).
    let tip_sha = tip_commit_sha
        .and_then(deploy_sha_fragment)
        .or_else(|| deploy_sha_fragment(target_tag));
    let cur_sha = current_commit_sha
        .and_then(deploy_sha_fragment)
        .or_else(|| current_tag.and_then(deploy_sha_fragment));
    match (tip_sha, cur_sha) {
        (Some(t), Some(c)) => t == c || t.starts_with(&c) || c.starts_with(&t),
        _ => false,
    }
}

/// Decide whether a commit/dev target is an upgrade relative to the running deploy.
///
/// Priority:
/// 1. Same deploy identity (tag / short↔full sha / known commit_sha) → identical.
/// 2. Both sides formal releases → **semver** (not wall-clock; survives Hub re-pushes).
/// 3. Clear git ancestry (ahead / behind / identical) when provided.
/// 4. Docker Hub push-time comparison (primary for private-repo / no-token cases).
/// 5. Different tag with missing times → treat as upgrade (do not block on unknown).
pub fn commit_upgrade_direction(
    target_tag: &str,
    current_tag: Option<&str>,
    target_pushed_at: Option<&str>,
    current_pushed_at: Option<&str>,
    ancestry: Option<&Freshness>,
) -> CommitUpgradeDirection {
    commit_upgrade_direction_ex(
        target_tag,
        current_tag,
        target_pushed_at,
        current_pushed_at,
        ancestry,
        None,
        None,
    )
}

/// Extended direction check with optional resolved commit SHAs (runtime + tip).
pub fn commit_upgrade_direction_ex(
    target_tag: &str,
    current_tag: Option<&str>,
    target_pushed_at: Option<&str>,
    current_pushed_at: Option<&str>,
    ancestry: Option<&Freshness>,
    current_commit_sha: Option<&str>,
    tip_commit_sha: Option<&str>,
) -> CommitUpgradeDirection {
    let target_tag = target_tag.trim();
    if same_deploy_identity(
        target_tag,
        current_tag,
        current_commit_sha,
        tip_commit_sha.or_else(|| ancestry.and_then(|f| f.target_sha.as_deref())),
    ) {
        return CommitUpgradeDirection::identical();
    }
    // Ancestry may also prove identical commits under different tag kinds.
    if let Some(f) = ancestry {
        if matches!(f.relation, crate::release::CommitRelation::Identical) {
            return CommitUpgradeDirection::identical();
        }
        if let (Some(c), Some(t)) = (f.current_sha.as_deref(), f.target_sha.as_deref()) {
            if same_commit_identity(c, t) {
                return CommitUpgradeDirection::identical();
            }
        }
    }

    // Formal release → formal release: order by semver (re-push must not invert).
    if let (Some(cur_raw), Ok(tgt)) = (
        current_tag.map(str::trim).filter(|s| !s.is_empty()),
        DeployTag::parse(target_tag),
    ) {
        if let (Ok(cur), Some(tgt_rel)) = (DeployTag::parse(cur_raw), tgt.as_release()) {
            if let Some(cur_rel) = cur.as_release() {
                if cur_rel.as_str() == tgt_rel.as_str() {
                    return CommitUpgradeDirection::identical();
                }
                if cur_rel.older_than(&tgt_rel) {
                    return CommitUpgradeDirection {
                        is_upgrade: true,
                        is_downgrade: false,
                        relation: "ahead",
                    };
                }
                if tgt_rel.older_than(&cur_rel) {
                    return CommitUpgradeDirection {
                        is_upgrade: false,
                        is_downgrade: true,
                        relation: "behind",
                    };
                }
                // Non-orderable prerelease edge — fall through.
            }
        }
    }

    if let Some(f) = ancestry {
        match f.relation {
            crate::release::CommitRelation::Ahead => {
                return CommitUpgradeDirection {
                    is_upgrade: true,
                    is_downgrade: false,
                    relation: "ahead",
                };
            }
            crate::release::CommitRelation::Behind => {
                return CommitUpgradeDirection {
                    is_upgrade: false,
                    is_downgrade: true,
                    relation: "behind",
                };
            }
            crate::release::CommitRelation::Identical => {
                return CommitUpgradeDirection::identical();
            }
            crate::release::CommitRelation::Diverged => {
                // Keep diverged for risk UI; still report direction from ancestry helpers.
                return CommitUpgradeDirection {
                    is_upgrade: f.is_upgrade(),
                    is_downgrade: f.is_downgrade(),
                    relation: "diverged",
                };
            }
            crate::release::CommitRelation::Unknown => {
                // Fall through to push-time comparison.
            }
        }
    }

    match (
        target_pushed_at.and_then(parse_push_time),
        current_pushed_at.and_then(parse_push_time),
    ) {
        (Some(target_t), Some(current_t)) if target_t > current_t => CommitUpgradeDirection {
            is_upgrade: true,
            is_downgrade: false,
            // Time-based "newer build"; not commit-count ahead.
            relation: "ahead",
        },
        (Some(target_t), Some(current_t)) if target_t < current_t => CommitUpgradeDirection {
            is_upgrade: false,
            is_downgrade: true,
            relation: "behind",
        },
        (Some(_), Some(_)) => CommitUpgradeDirection::none_actionable(),
        // Missing one or both timestamps, different tag: tip is list-head (newest-first)
        // on Docker Hub discovery — treat as upgrade with relation=ahead so auto_install
        // and UI are not blocked by relation=unknown.
        _ if current_tag.is_some() => CommitUpgradeDirection {
            is_upgrade: true,
            is_downgrade: false,
            relation: "ahead",
        },
        // No current version recorded — first run / bootstrap.
        _ => CommitUpgradeDirection {
            is_upgrade: true,
            is_downgrade: false,
            relation: "ahead",
        },
    }
}

/// Look up `pushed_at` for a deploy tag in a Docker Hub common-build list.
///
/// Matches exact tag, bare/short sha, `dev-<sha>`, and short↔full sha prefixes.
pub fn pushed_at_for_tag<'a>(builds: &'a [DockerBuild], tag: &str) -> Option<&'a str> {
    let tag = tag.trim();
    builds
        .iter()
        .find(|b| {
            b.tag == tag
                || b.short_sha == tag
                || b.tag == format!("dev-{tag}")
                || same_commit_identity(&b.tag, tag)
                || same_commit_identity(&b.short_sha, tag)
        })
        .and_then(|b| b.pushed_at.as_deref())
}

/// Dev-channel tip: newest common build by `pushed_at` among **both** `dev-*`
/// commit builds and formal `vX.Y.Z` releases.
///
/// `builds` must already be newest-first (as produced by [`common_builds`] /
/// [`DockerHubClient::list_common_builds`]). No kind filter — time wins.
///
/// When `current_*` is provided, skip tips that are the same deploy identity as
/// the running image so a formal `vX.Y.Z` and its `dev-<same-sha>` sibling do not
/// hide a real subsequent build further down the list.
pub fn select_dev_channel_tip(builds: &[DockerBuild]) -> Option<&DockerBuild> {
    select_dev_channel_tip_for(builds, None, None)
}

/// Like [`select_dev_channel_tip`] but skips artifacts already running and
/// tips that are clearly older than the running deploy (by push time, or by
/// semver when both sides are formal releases). That way a same-commit
/// `dev-*` sibling of the current `v*` release does not expose the previous
/// build as a fake "available" tip.
pub fn select_dev_channel_tip_for<'a>(
    builds: &'a [DockerBuild],
    current_tag: Option<&str>,
    current_commit_sha: Option<&str>,
) -> Option<&'a DockerBuild> {
    if current_tag.is_none() && current_commit_sha.is_none() {
        return builds.first();
    }
    let current_pushed = current_tag.and_then(|t| pushed_at_for_tag(builds, t));
    let current_time = current_pushed.and_then(parse_push_time);
    let current_release =
        current_tag.and_then(|t| DeployTag::parse(t).ok().and_then(|d| d.as_release()));

    builds.iter().find(|b| {
        if same_deploy_identity(
            &b.tag,
            current_tag,
            current_commit_sha,
            Some(b.short_sha.as_str()),
        ) {
            return false;
        }
        // Semver-newer formal release is always a candidate (Hub re-push safe).
        if let (Some(cur_rel), Ok(tip_tag)) = (&current_release, DeployTag::parse(&b.tag)) {
            if let Some(tip_rel) = tip_tag.as_release() {
                if cur_rel.older_than(&tip_rel) {
                    return true;
                }
                if tip_rel.older_than(cur_rel) {
                    return false;
                }
            }
        }
        // Drop tips older than current by wall-clock (previous builds).
        !matches!(
            (
                b.pushed_at.as_deref().and_then(parse_push_time),
                current_time,
            ),
            (Some(tip_t), Some(cur_t)) if tip_t < cur_t
        )
    })
}

/// True when target and current are different deploy kinds (release vs commit/branch).
/// Cross-kind upgrade direction uses push time as primary (no git-ancestry preference).
pub fn is_cross_kind_deploy(target_tag: &str, current_tag: Option<&str>) -> bool {
    let Ok(target) = DeployTag::parse(target_tag.trim()) else {
        return false;
    };
    let Some(current_raw) = current_tag.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    let Ok(current) = DeployTag::parse(current_raw) else {
        return false;
    };
    match (target.kind(), current.kind()) {
        (DeployTagKind::Release, DeployTagKind::Release) => false,
        (DeployTagKind::Release, _) | (_, DeployTagKind::Release) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(name: &str, pushed_at: &str, digest: &str) -> TagResult {
        TagResult {
            name: name.to_string(),
            digest: Some(digest.to_string()),
            last_updated: Some(pushed_at.to_string()),
            tag_last_pushed: None,
            images: Vec::new(),
        }
    }

    #[test]
    fn preferred_digest_prefers_host_arch_when_tag_digest_missing() {
        let tag = TagResult {
            name: "v0.3.0".into(),
            digest: None,
            last_updated: None,
            tag_last_pushed: None,
            images: vec![
                TagImage {
                    architecture: Some("amd64".into()),
                    os: Some("linux".into()),
                    digest: Some("sha256:amd64".into()),
                },
                TagImage {
                    architecture: Some("arm64".into()),
                    os: Some("linux".into()),
                    digest: Some("sha256:arm64".into()),
                },
            ],
        };
        assert_eq!(
            tag.preferred_digest_for_arch("arm64").as_deref(),
            Some("sha256:arm64")
        );
        assert_eq!(
            tag.preferred_digest_for_arch("amd64").as_deref(),
            Some("sha256:amd64")
        );
        // Tag-level digest wins over per-arch images (manifest list).
        let with_index = TagResult {
            digest: Some("sha256:index".into()),
            ..tag
        };
        assert_eq!(
            with_index.preferred_digest_for_arch("arm64").as_deref(),
            Some("sha256:index")
        );
    }

    #[test]
    fn parses_docker_hub_image_repositories() {
        let explicit = DockerHubRepository::parse("docker.io/example/myriad-backend").unwrap();
        assert_eq!(explicit.namespace, "example");
        assert_eq!(explicit.repository, "myriad-backend");

        let implicit = DockerHubRepository::parse("example/myriad-frontend:preview").unwrap();
        assert_eq!(implicit.namespace, "example");
        assert_eq!(implicit.repository, "myriad-frontend");

        assert!(DockerHubRepository::parse("ghcr.io/example/myriad-backend").is_err());
        assert!(DockerHubRepository::parse("ghcr.io/myriad-backend").is_err());
    }

    #[test]
    fn keeps_only_common_immutable_dev_and_release_builds_newest_first() {
        let backend = DockerHubRepository::parse("example/backend").unwrap();
        let frontend = DockerHubRepository::parse("example/frontend").unwrap();
        let builds = common_builds(
            &backend,
            vec![
                tag("preview", "2026-07-15T10:00:00Z", "sha256:branch"),
                tag("dev-aaaaaaa", "2026-07-15T09:00:00Z", "sha256:ba"),
                tag("dev-bbbbbbb", "2026-07-15T11:00:00Z", "sha256:bb"),
                tag("v0.2.6", "2026-07-16T08:00:00Z", "sha256:r1"),
                tag("v0.2.5", "2026-07-14T08:00:00Z", "sha256:r0"),
                tag("stable", "2026-07-16T09:00:00Z", "sha256:mutable"),
            ],
            &frontend,
            vec![
                tag("dev-aaaaaaa", "2026-07-15T09:30:00Z", "sha256:fa"),
                tag("dev-bbbbbbb", "2026-07-15T10:30:00Z", "sha256:fb"),
                tag("dev-ccccccc", "2026-07-15T12:00:00Z", "sha256:fc"),
                tag("v0.2.6", "2026-07-16T07:30:00Z", "sha256:fr1"),
                // v0.2.5 missing on frontend → excluded
            ],
            10,
        );

        assert_eq!(builds.len(), 3);
        assert_eq!(builds[0].tag, "v0.2.6");
        assert_eq!(builds[0].kind, "release");
        assert_eq!(builds[0].pushed_at.as_deref(), Some("2026-07-16T07:30:00Z"));
        assert_eq!(builds[1].tag, "dev-bbbbbbb");
        assert_eq!(builds[1].kind, "commit");
        assert_eq!(builds[1].pushed_at.as_deref(), Some("2026-07-15T10:30:00Z"));
        assert_eq!(builds[2].tag, "dev-aaaaaaa");
        assert_eq!(builds[2].kind, "commit");
    }

    #[test]
    fn commit_upgrade_uses_push_time_when_ancestry_unknown() {
        let dir = commit_upgrade_direction(
            "dev-bbbbbbb",
            Some("dev-aaaaaaa"),
            Some("2026-07-16T12:00:00Z"),
            Some("2026-07-15T12:00:00Z"),
            None,
        );
        assert!(dir.is_upgrade);
        assert!(!dir.is_downgrade);
        assert_eq!(dir.relation, "ahead");

        let older = commit_upgrade_direction(
            "dev-aaaaaaa",
            Some("dev-bbbbbbb"),
            Some("2026-07-15T12:00:00Z"),
            Some("2026-07-16T12:00:00Z"),
            None,
        );
        assert!(!older.is_upgrade);
        assert!(older.is_downgrade);
        assert_eq!(older.relation, "behind");
    }

    #[test]
    fn commit_upgrade_same_tag_is_identical() {
        let dir = commit_upgrade_direction(
            "dev-aaaaaaa",
            Some("dev-aaaaaaa"),
            Some("2026-07-16T12:00:00Z"),
            Some("2026-07-15T12:00:00Z"),
            None,
        );
        assert!(!dir.is_upgrade);
        assert!(!dir.is_downgrade);
        assert_eq!(dir.relation, "identical");
    }

    #[test]
    fn commit_upgrade_missing_times_with_different_tag_is_upgrade() {
        let dir = commit_upgrade_direction("dev-bbbbbbb", Some("dev-aaaaaaa"), None, None, None);
        assert!(dir.is_upgrade);
        assert!(!dir.is_downgrade);
        // Prefer ahead (list-head / tip) over unknown so auto_install is not gated.
        assert_eq!(dir.relation, "ahead");
    }

    #[test]
    fn commit_upgrade_target_time_only_is_upgrade_ahead() {
        // Current tag not found on Hub (no pushed_at); tip has a push time.
        let dir = commit_upgrade_direction(
            "dev-bbbbbbb",
            Some("dev-aaaaaaa"),
            Some("2026-07-16T12:00:00Z"),
            None,
            None,
        );
        assert!(dir.is_upgrade);
        assert!(!dir.is_downgrade);
        assert_eq!(dir.relation, "ahead");
    }

    #[test]
    fn commit_upgrade_clear_ancestry_wins_over_times() {
        let freshness = Freshness {
            relation: crate::release::CommitRelation::Behind,
            ahead_by: 0,
            behind_by: 3,
            current_sha: None,
            target_sha: None,
            current_ref: None,
            target_ref: "preview".into(),
        };
        // Times would say upgrade, but clear ancestry says behind.
        let dir = commit_upgrade_direction(
            "dev-bbbbbbb",
            Some("dev-aaaaaaa"),
            Some("2026-07-16T12:00:00Z"),
            Some("2026-07-15T12:00:00Z"),
            Some(&freshness),
        );
        assert!(!dir.is_upgrade);
        assert!(dir.is_downgrade);
        assert_eq!(dir.relation, "behind");
    }

    #[test]
    fn newer_release_beats_older_commit_by_push_time() {
        let dir = commit_upgrade_direction(
            "v0.3.0",
            Some("dev-aaaaaaa"),
            Some("2026-07-17T12:00:00Z"),
            Some("2026-07-16T12:00:00Z"),
            None,
        );
        assert!(dir.is_upgrade);
        assert!(!dir.is_downgrade);
        assert_eq!(dir.relation, "ahead");
        assert!(is_cross_kind_deploy("v0.3.0", Some("dev-aaaaaaa")));
    }

    #[test]
    fn newer_commit_beats_older_release_by_push_time() {
        let dir = commit_upgrade_direction(
            "dev-bbbbbbb",
            Some("v0.2.8"),
            Some("2026-07-18T08:00:00Z"),
            Some("2026-07-17T08:00:00Z"),
            None,
        );
        assert!(dir.is_upgrade);
        assert!(!dir.is_downgrade);
        assert_eq!(dir.relation, "ahead");
        assert!(is_cross_kind_deploy("dev-bbbbbbb", Some("v0.2.8")));
    }

    #[test]
    fn older_release_vs_newer_commit_is_downgrade() {
        let dir = commit_upgrade_direction(
            "v0.2.8",
            Some("dev-bbbbbbb"),
            Some("2026-07-17T08:00:00Z"),
            Some("2026-07-18T08:00:00Z"),
            None,
        );
        assert!(!dir.is_upgrade);
        assert!(dir.is_downgrade);
        assert_eq!(dir.relation, "behind");
    }

    #[test]
    fn same_tag_is_not_upgrade() {
        let release = commit_upgrade_direction(
            "v0.3.0",
            Some("v0.3.0"),
            Some("2026-07-18T12:00:00Z"),
            Some("2026-07-17T12:00:00Z"),
            None,
        );
        assert!(!release.is_upgrade);
        assert!(!release.is_downgrade);
        assert_eq!(release.relation, "identical");

        let commit = commit_upgrade_direction(
            "dev-aaaaaaa",
            Some("dev-aaaaaaa"),
            Some("2026-07-18T12:00:00Z"),
            Some("2026-07-17T12:00:00Z"),
            None,
        );
        assert!(!commit.is_upgrade);
        assert!(!commit.is_downgrade);
        assert_eq!(commit.relation, "identical");
    }

    #[test]
    fn immutable_component_tags_skip_branch_tips_newest_first() {
        let tags = immutable_component_tags(
            vec![
                tag("preview", "2026-07-16T12:00:00Z", "sha256:branch"),
                tag("dev-aaaaaaa", "2026-07-15T09:00:00Z", "sha256:dev"),
                tag("v0.3.6", "2026-07-16T08:00:00Z", "sha256:rel"),
                tag("latest", "2026-07-17T08:00:00Z", "sha256:latest"),
            ],
            10,
        );
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].tag, "v0.3.6");
        assert_eq!(tags[0].kind, "release");
        assert_eq!(tags[1].tag, "dev-aaaaaaa");
        assert_eq!(tags[1].kind, "commit");
    }

    #[test]
    fn select_component_tip_prefers_release_when_requested() {
        let tags = immutable_component_tags(
            vec![
                tag("dev-bbbbbbb", "2026-07-17T12:00:00Z", "sha256:dev"),
                tag("v0.3.6", "2026-07-16T08:00:00Z", "sha256:rel"),
            ],
            10,
        );
        assert_eq!(
            select_component_tip(&tags, true).map(|t| t.tag.as_str()),
            Some("v0.3.6")
        );
        assert_eq!(
            select_component_tip(&tags, false).map(|t| t.tag.as_str()),
            Some("dev-bbbbbbb")
        );
    }

    #[test]
    fn select_dev_channel_tip_is_newest_without_kind_filter() {
        let backend = DockerHubRepository::parse("example/backend").unwrap();
        let frontend = DockerHubRepository::parse("example/frontend").unwrap();
        // Newer formal release must win over older commit (no prefer-dev policy).
        let builds = common_builds(
            &backend,
            vec![
                tag("dev-bbbbbbb", "2026-07-15T11:00:00Z", "sha256:bb"),
                tag("v0.3.0", "2026-07-16T08:00:00Z", "sha256:r1"),
                tag("dev-aaaaaaa", "2026-07-14T09:00:00Z", "sha256:ba"),
            ],
            &frontend,
            vec![
                tag("dev-bbbbbbb", "2026-07-15T10:30:00Z", "sha256:fb"),
                tag("v0.3.0", "2026-07-16T07:30:00Z", "sha256:fr1"),
                tag("dev-aaaaaaa", "2026-07-14T09:30:00Z", "sha256:fa"),
            ],
            10,
        );
        let tip = select_dev_channel_tip(&builds).expect("tip");
        assert_eq!(tip.tag, "v0.3.0");
        assert_eq!(tip.kind, "release");

        // Newer commit beats older release.
        let builds2 = common_builds(
            &backend,
            vec![
                tag("v0.2.8", "2026-07-15T08:00:00Z", "sha256:r0"),
                tag("dev-ccccccc", "2026-07-16T12:00:00Z", "sha256:bc"),
            ],
            &frontend,
            vec![
                tag("v0.2.8", "2026-07-15T07:30:00Z", "sha256:fr0"),
                tag("dev-ccccccc", "2026-07-16T11:30:00Z", "sha256:fc"),
            ],
            10,
        );
        let tip2 = select_dev_channel_tip(&builds2).expect("tip");
        assert_eq!(tip2.tag, "dev-ccccccc");
        assert_eq!(tip2.kind, "commit");
    }

    #[test]
    fn short_and_full_dev_sha_are_same_identity() {
        assert!(same_commit_identity(
            "dev-f6e2c4d",
            "dev-f6e2c4d95a43d56ebc25722b785f44c73ef427a2"
        ));
        assert!(same_deploy_identity(
            "dev-f6e2c4d",
            Some("dev-f6e2c4d95a43d56ebc25722b785f44c73ef427a2"),
            None,
            None,
        ));
        // Formal release + matching commit_sha vs dev tip of same commit.
        assert!(same_deploy_identity(
            "dev-f6e2c4d",
            Some("v0.3.21"),
            Some("f6e2c4d95a43d56ebc25722b785f44c73ef427a2"),
            Some("f6e2c4d"),
        ));
    }

    #[test]
    fn short_vs_full_dev_sha_is_identical_not_upgrade() {
        let dir = commit_upgrade_direction(
            "dev-f6e2c4d",
            Some("dev-f6e2c4d95a43d56ebc25722b785f44c73ef427a2"),
            Some("2026-07-31T14:13:00Z"),
            Some("2026-07-31T14:12:00Z"),
            None,
        );
        assert!(!dir.is_upgrade);
        assert!(!dir.is_downgrade);
        assert_eq!(dir.relation, "identical");
    }

    #[test]
    fn release_to_release_uses_semver_not_push_time() {
        // Older formal tag re-pushed later must not invert semver order.
        let dir = commit_upgrade_direction(
            "v0.3.17",
            Some("v0.3.21"),
            Some("2026-08-01T12:00:00Z"), // re-pushed "newer"
            Some("2026-07-31T12:00:00Z"),
            None,
        );
        assert!(!dir.is_upgrade);
        assert!(dir.is_downgrade);
        assert_eq!(dir.relation, "behind");

        let up = commit_upgrade_direction(
            "v0.3.21",
            Some("v0.3.20"),
            Some("2026-07-31T10:00:00Z"),
            Some("2026-07-31T14:00:00Z"), // current "newer" by clock, older by semver
            None,
        );
        assert!(up.is_upgrade);
        assert!(!up.is_downgrade);
        assert_eq!(up.relation, "ahead");
    }

    #[test]
    fn select_tip_skips_running_identity_and_older_builds() {
        let backend = DockerHubRepository::parse("example/backend").unwrap();
        let frontend = DockerHubRepository::parse("example/frontend").unwrap();
        // All sha fragments must be hex (DeployTag / docker-publish short sha).
        let builds = common_builds(
            &backend,
            vec![
                tag("dev-f6e2c4d", "2026-07-31T14:13:00Z", "sha256:d1"),
                tag("v0.3.21", "2026-07-31T14:12:00Z", "sha256:r1"),
                tag("dev-ba5c400", "2026-07-31T10:00:00Z", "sha256:d0"),
                tag("dev-abcdef1", "2026-08-01T09:00:00Z", "sha256:dn"),
            ],
            &frontend,
            vec![
                tag("dev-f6e2c4d", "2026-07-31T14:13:00Z", "sha256:fd1"),
                tag("v0.3.21", "2026-07-31T14:12:00Z", "sha256:fr1"),
                tag("dev-ba5c400", "2026-07-31T10:00:00Z", "sha256:fd0"),
                tag("dev-abcdef1", "2026-08-01T09:00:00Z", "sha256:fdn"),
            ],
            10,
        );
        // Running v0.3.21 @ f6e2c4d → skip same-commit siblings + older ba5c400;
        // surface the later commit tip.
        let tip = select_dev_channel_tip_for(
            &builds,
            Some("v0.3.21"),
            Some("f6e2c4d95a43d56ebc25722b785f44c73ef427a2"),
        )
        .expect("real upgrade tip");
        assert_eq!(tip.tag, "dev-abcdef1");

        // Already on newest → no tip (only older remains after identity skip).
        let none = select_dev_channel_tip_for(
            &builds,
            Some("dev-abcdef1"),
            Some("abcdef1aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        );
        assert!(none.is_none());

        // Running an older commit → tip is newest different identity.
        let tip_up =
            select_dev_channel_tip_for(&builds, Some("dev-ba5c400"), None).expect("upgrade tip");
        assert_eq!(tip_up.tag, "dev-abcdef1");
    }

    #[test]
    fn pushed_at_matches_full_runtime_sha_to_short_hub_tag() {
        let backend = DockerHubRepository::parse("example/backend").unwrap();
        let frontend = DockerHubRepository::parse("example/frontend").unwrap();
        let builds = common_builds(
            &backend,
            vec![tag("dev-f6e2c4d", "2026-07-31T14:13:00Z", "sha256:d1")],
            &frontend,
            vec![tag("dev-f6e2c4d", "2026-07-31T14:13:00Z", "sha256:fd1")],
            10,
        );
        assert_eq!(
            pushed_at_for_tag(&builds, "dev-f6e2c4d95a43d56ebc25722b785f44c73ef427a2"),
            Some("2026-07-31T14:13:00Z")
        );
    }
}
