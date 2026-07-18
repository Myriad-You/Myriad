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

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::error::{Result, UpdaterError};
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
        self.digest.clone().or_else(|| {
            self.images
                .iter()
                .find(|image| {
                    image.os.as_deref() == Some("linux")
                        && image.architecture.as_deref() == Some("amd64")
                })
                .or_else(|| self.images.first())
                .and_then(|image| image.digest.clone())
        })
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
}
