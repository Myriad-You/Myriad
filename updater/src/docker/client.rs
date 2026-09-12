//! Thin wrapper over bollard for the few operations we need: pull, manifest inspect,
//! HTTP probe inside the container network.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bollard::auth::DockerCredentials;
use bollard::query_parameters::CreateImageOptions;
use bollard::Docker;
use futures::StreamExt;
use tracing::{debug, warn};

use crate::error::{Result, UpdaterError};

pub struct DockerClient {
    inner: Docker,
}

impl DockerClient {
    pub async fn connect() -> Result<Self> {
        // Honor DOCKER_HOST so production can use the guarded TCP endpoint. The local-only
        // constructor always selects /var/run/docker.sock and silently bypasses this setting.
        let docker = Docker::connect_with_defaults()
            .map_err(|e| UpdaterError::Docker(format!("connect: {e}")))?;
        // Cheap liveness check.
        docker
            .ping()
            .await
            .map_err(|e| UpdaterError::Docker(format!("ping: {e}")))?;
        Ok(Self { inner: docker })
    }

    pub fn raw(&self) -> &Docker {
        &self.inner
    }

    /// Pull an image, streaming progress lines. Returns the resolved digest of the pulled image
    /// (read from `inspect` post-pull).
    pub async fn pull(&self, image_ref: &str, creds: Option<DockerCredentials>) -> Result<String> {
        let (image, tag) = parse_image_ref(image_ref);
        let opts = CreateImageOptions {
            from_image: Some(image.clone()),
            tag: Some(tag.clone()),
            ..Default::default()
        };
        let mut stream = self.inner.create_image(Some(opts), None, creds);
        while let Some(item) = stream.next().await {
            match item {
                Ok(info) => {
                    if let Some(status) = info.status {
                        debug!(image = %image_ref, %status, "pull progress");
                    }
                    if let Some(err) = info.error_detail {
                        let msg = err.message.unwrap_or_default();
                        return Err(UpdaterError::Docker(format!("pull {image_ref}: {msg}")));
                    }
                }
                Err(e) => return Err(UpdaterError::Docker(format!("pull stream: {e}"))),
            }
        }
        // Resolve digest via inspect.
        let inspect = self
            .inner
            .inspect_image(image_ref)
            .await
            .map_err(|e| UpdaterError::Docker(format!("inspect {image_ref}: {e}")))?;
        let digest =
            matching_repo_digest(&image, inspect.repo_digests.as_deref().unwrap_or_default())?;
        Ok(digest)
    }

    /// Probe an HTTP endpoint directly from the updater on the compose network.
    ///
    /// Deliberately do not fall back to Docker exec or a one-shot probe container. Keeping
    /// health checks on the network path removes the updater's need for Docker exec and avoids
    /// granting generic container-create permission merely for diagnostics.
    pub async fn http_probe(&self, target: &str, timeout: Duration) -> Result<(u16, String)> {
        direct_http_probe(target, timeout).await
    }

    /// Image reference the container was created with (e.g. `repo:dev-abc1234`).
    pub async fn container_image_ref(&self, name: &str) -> Result<String> {
        let info = self
            .inner
            .inspect_container(name, None)
            .await
            .map_err(|e| UpdaterError::Docker(format!("inspect {name}: {e}")))?;
        Ok(info.config.and_then(|c| c.image).unwrap_or_default())
    }

    /// Docker healthcheck status if defined: "healthy" | "unhealthy" | "starting" | …
    pub async fn container_health_status(&self, name: &str) -> Option<String> {
        let info = self.inner.inspect_container(name, None).await.ok()?;
        info.state?
            .health?
            .status
            .map(|s| s.to_string().to_ascii_lowercase())
    }

    /// Inspect a container by name and return whether it is running.
    pub async fn is_running(&self, name: &str) -> Result<bool> {
        let info = self
            .inner
            .inspect_container(name, None)
            .await
            .map_err(|e| UpdaterError::Docker(format!("inspect {name}: {e}")))?;
        Ok(info.state.as_ref().and_then(|s| s.running).unwrap_or(false))
    }

    /// Stop a container by name; escalate to kill if still running.
    /// Used before pgdata restore so bind mounts are fully released.
    pub async fn force_stop_container(&self, name: &str) -> Result<()> {
        use bollard::query_parameters::{KillContainerOptionsBuilder, StopContainerOptionsBuilder};
        if !self.is_running(name).await.unwrap_or(false) {
            return Ok(());
        }
        let stop_opts = StopContainerOptionsBuilder::default().t(15).build();
        if let Err(e) = self.inner.stop_container(name, Some(stop_opts)).await {
            warn!(%name, err = %e, "docker stop failed; trying kill");
        }
        // Brief wait for graceful stop.
        for _ in 0..10 {
            if !self.is_running(name).await.unwrap_or(false) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        let kill_opts = KillContainerOptionsBuilder::default()
            .signal("SIGKILL")
            .build();
        self.inner
            .kill_container(name, Some(kill_opts))
            .await
            .map_err(|e| UpdaterError::Docker(format!("kill {name}: {e}")))?;
        Ok(())
    }

    pub async fn ping(&self) -> Result<()> {
        self.inner
            .ping()
            .await
            .map(|_| ())
            .map_err(|e| UpdaterError::Docker(format!("ping: {e}")))?;
        Ok(())
    }

    /// Map a path visible inside the updater container back to the bind source path expected by
    /// the host daemon. Docker Compose needs this as `--project-directory`; otherwise relative
    /// binds would be resolved to `/host/compose/...`, which exists only inside this container.
    pub async fn resolve_host_bind_source(&self, container_path: &Path) -> Result<PathBuf> {
        let container_id = current_container_id()?;
        let info = self
            .inner
            .inspect_container(&container_id, None)
            .await
            .map_err(|e| UpdaterError::Docker(format!("inspect current updater container: {e}")))?;

        let mut best: Option<(usize, PathBuf)> = None;
        for mount in info.mounts.unwrap_or_default() {
            let (Some(source), Some(destination)) = (mount.source, mount.destination) else {
                continue;
            };
            let destination = PathBuf::from(destination);
            if !container_path.starts_with(&destination) {
                continue;
            }
            let relative = container_path
                .strip_prefix(&destination)
                .unwrap_or_else(|_| Path::new(""));
            let score = destination.as_os_str().len();
            if best
                .as_ref()
                .is_none_or(|(best_score, _)| score > *best_score)
            {
                best = Some((score, PathBuf::from(source).join(relative)));
            }
        }

        best.map(|(_, source)| source).ok_or_else(|| {
            UpdaterError::Precondition(format!(
                "could not map container path {} to a host bind source",
                container_path.display()
            ))
        })
    }

    /// Create an additional tag for an already-local image (does not pull).
    /// Used to pin last-known-good backend/frontend images so casual
    /// `docker image prune` does not remove the only rollback target.
    pub async fn tag_image(
        &self,
        source_ref: &str,
        target_repo: &str,
        target_tag: &str,
    ) -> Result<()> {
        use bollard::query_parameters::TagImageOptionsBuilder;
        let opts = TagImageOptionsBuilder::default()
            .repo(target_repo)
            .tag(target_tag)
            .build();
        self.inner
            .tag_image(source_ref, Some(opts))
            .await
            .map_err(|e| {
                UpdaterError::Docker(format!(
                    "tag {source_ref} → {target_repo}:{target_tag}: {e}"
                ))
            })?;
        Ok(())
    }

    /// Compare Docker's running config image ID with the local image addressed
    /// by the preflight registry digest. Tag names are not identity evidence.
    pub async fn require_container_digest(&self, container: &str, pinned_ref: &str) -> Result<()> {
        let expected = self.inner.inspect_image(pinned_ref).await.map_err(|_| {
            UpdaterError::Precondition(format!("cannot inspect verified image for {container}"))
        })?;
        let running = self
            .inner
            .inspect_container(container, None)
            .await
            .map_err(|_| {
                UpdaterError::Precondition(format!("cannot inspect running {container}"))
            })?;
        if expected.id.is_none() || expected.id != running.image {
            return Err(UpdaterError::Precondition(format!(
                "running {container} differs from the preflight image digest"
            )));
        }
        Ok(())
    }

    /// True if the named image ref exists locally (inspect succeeds).
    pub async fn image_exists_local(&self, image_ref: &str) -> bool {
        self.inner.inspect_image(image_ref).await.is_ok()
    }
}

fn canonical_repository(repo: &str) -> String {
    let repo = repo
        .strip_prefix("docker.io/")
        .or_else(|| repo.strip_prefix("index.docker.io/"))
        .or_else(|| repo.strip_prefix("registry-1.docker.io/"))
        .unwrap_or(repo);
    if repo.contains('/') {
        repo.into()
    } else {
        format!("library/{repo}")
    }
}

fn matching_repo_digest(repository: &str, digests: &[String]) -> Result<String> {
    let want = canonical_repository(repository);
    let matching: std::collections::BTreeSet<_> = digests
        .iter()
        .filter_map(|entry| {
            let (repo, digest) = entry.split_once('@')?;
            (canonical_repository(repo) == want).then_some(digest)
        })
        .collect();
    // A Docker config ID is not a registry manifest digest. Ambiguous aliases
    // also cannot establish which artifact a tag pull resolved to.
    if matching.len() != 1 {
        return Err(UpdaterError::Precondition(
            "pull did not resolve one unambiguous repository digest".into(),
        ));
    }
    let digest = matching.into_iter().next().unwrap();
    crate::release::dev_signature::require_digest(digest)?;
    Ok(digest.into())
}

/// Split "registry/image:tag" into (image_without_tag, tag).
/// Falls back to tag = "latest" if absent; release preflight rejects latest for managed components.
fn parse_image_ref(s: &str) -> (String, String) {
    // We must avoid splitting on ":" inside the registry port (e.g. "host:5000/img:tag").
    // Strategy: split off everything after the last '/' first.
    let (prefix, last) = match s.rsplit_once('/') {
        Some((a, b)) => (Some(a), b),
        None => (None, s),
    };
    let (img_name, tag) = match last.split_once(':') {
        Some((n, t)) => (n.to_string(), t.to_string()),
        None => (last.to_string(), "latest".to_string()),
    };
    let full_image = match prefix {
        Some(p) => format!("{p}/{img_name}"),
        None => img_name,
    };
    (full_image, tag)
}

fn current_container_id() -> Result<String> {
    let hostname = match std::env::var("HOSTNAME") {
        Ok(value) => value,
        Err(_) => std::fs::read_to_string("/etc/hostname")?,
    };
    let hostname = hostname.trim();
    if hostname.is_empty() {
        return Err(UpdaterError::Precondition(
            "cannot determine current updater container id".into(),
        ));
    }
    Ok(hostname.to_string())
}

/// Direct HTTP probe from the updater process (same Docker network as backend/frontend).
async fn direct_http_probe(target: &str, timeout: Duration) -> Result<(u16, String)> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(timeout.min(Duration::from_secs(5)))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| UpdaterError::Docker(format!("probe client: {e}")))?;
    let resp = client
        .get(target)
        .send()
        .await
        .map_err(|e| UpdaterError::Docker(format!("probe GET {target}: {e}")))?;
    let code = resp.status().as_u16();
    let mut body = resp
        .text()
        .await
        .map_err(|e| UpdaterError::Docker(format!("probe body: {e}")))?;
    if body.len() > 8192 {
        body.truncate(8192);
    }
    Ok((code, body))
}

#[cfg(test)]
mod tests {
    #[test]
    fn pull_digest_belongs_to_requested_repository_and_is_unambiguous() {
        let a = format!("sha256:{}", "a".repeat(64));
        let b = format!("sha256:{}", "b".repeat(64));
        let values = vec![format!("other/image@{b}"), format!("org/backend@{a}")];
        assert_eq!(
            super::matching_repo_digest("docker.io/org/backend", &values).unwrap(),
            a
        );
        assert!(super::matching_repo_digest("org/frontend", &values).is_err());
        assert!(super::matching_repo_digest("org/backend", &[]).is_err());
        assert!(super::matching_repo_digest(
            "org/backend",
            &[format!("org/backend@{a}"), format!("org/backend@{b}")]
        )
        .is_err());
    }

    use super::*;

    #[test]
    fn parse_image_basic() {
        assert_eq!(
            parse_image_ref("docker.io/foo/bar:v1.2.3"),
            ("docker.io/foo/bar".to_string(), "v1.2.3".to_string())
        );
    }

    #[test]
    fn parse_image_with_registry_port() {
        assert_eq!(
            parse_image_ref("host:5000/foo/bar:v1"),
            ("host:5000/foo/bar".to_string(), "v1".to_string())
        );
    }

    #[test]
    fn parse_image_no_tag() {
        assert_eq!(
            parse_image_ref("alpine"),
            ("alpine".to_string(), "latest".to_string())
        );
    }
}
