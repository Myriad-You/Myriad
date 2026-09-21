//! Thin wrapper over bollard for the few operations we need: pull, manifest inspect,
//! HTTP probe inside the container network.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bollard::Docker;
use bollard::auth::DockerCredentials;
use bollard::query_parameters::CreateImageOptions;
use futures::StreamExt;
use tracing::{debug, warn};

use crate::error::{Result, UpdaterError};

pub(crate) const APP_WORKERS: [&str; 2] = ["federation-worker", "persona-worker"];

pub struct DockerClient {
    inner: Docker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InspectRunState {
    Absent,
    Stopped,
    Running,
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

    pub async fn image_id(&self, image: &str) -> Result<String> {
        self.inner
            .inspect_image(image)
            .await
            .map_err(|error| UpdaterError::Docker(format!("inspect image {image}: {error}")))?
            .id
            .filter(|id| !id.is_empty())
            .ok_or_else(|| UpdaterError::Docker(format!("image {image} has no content identity")))
    }

    /// The container's actual image and local healthcheck are execution evidence.
    /// Config.Image (a mutable tag) and public-page text are not.
    pub async fn container_ready(&self, name: &str, image_id: &str) -> Result<bool> {
        let info = self
            .inner
            .inspect_container(name, None)
            .await
            .map_err(|error| UpdaterError::Docker(format!("inspect {name}: {error}")))?;
        let state = info.state.as_ref();
        Ok(info.image.as_deref() == Some(image_id)
            && state.and_then(|s| s.running) == Some(true)
            && state
                .and_then(|s| s.health.as_ref())
                .and_then(|h| h.status.as_ref())
                .is_some_and(|s| s.to_string() == "healthy"))
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
        let digest = inspect
            .repo_digests
            .as_ref()
            .and_then(|v| v.first())
            .and_then(|s| s.split_once('@').map(|(_, d)| d.to_string()))
            .or(inspect.id)
            .ok_or_else(|| {
                UpdaterError::Docker(format!("could not determine digest for {image_ref}"))
            })?;
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

    /// Inspect a container by name and return whether it is running.
    ///
    /// A 404 is "not running". Any other inspect error is an error — it is not
    /// evidence that the container has stopped.
    pub async fn is_running(&self, name: &str) -> Result<bool> {
        match self.inspect_run_state(name).await? {
            InspectRunState::Running => Ok(true),
            InspectRunState::Absent | InspectRunState::Stopped => Ok(false),
        }
    }

    async fn inspect_run_state(&self, name: &str) -> Result<InspectRunState> {
        match self.inner.inspect_container(name, None).await {
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(InspectRunState::Absent),
            Err(e) => Err(UpdaterError::Docker(format!("inspect {name}: {e}"))),
            Ok(info) => match info.state.as_ref().and_then(|s| s.running) {
                Some(true) => Ok(InspectRunState::Running),
                Some(false) => Ok(InspectRunState::Stopped),
                None => Err(UpdaterError::Docker(format!(
                    "inspect {name}: running state unavailable"
                ))),
            },
        }
    }

    /// Stop a container by name; escalate to kill if still running.
    /// Used before pgdata restore so bind mounts are fully released.
    ///
    /// Missing containers (inspect 404) are already stopped. Inspect, stop, or
    /// kill failures are errors — never treated as "already stopped".
    pub async fn force_stop_container(&self, name: &str) -> Result<()> {
        use bollard::query_parameters::{KillContainerOptionsBuilder, StopContainerOptionsBuilder};
        match self.inspect_run_state(name).await? {
            InspectRunState::Absent | InspectRunState::Stopped => return Ok(()),
            InspectRunState::Running => {}
        }
        let stop_opts = StopContainerOptionsBuilder::default()
            .t(if name == "myriad-persona-worker" {
                45
            } else {
                15
            })
            .build();
        if let Err(e) = self.inner.stop_container(name, Some(stop_opts)).await {
            warn!(%name, err = %e, "docker stop failed; trying kill");
        }
        for _ in 0..10 {
            match self.inspect_run_state(name).await? {
                InspectRunState::Absent | InspectRunState::Stopped => return Ok(()),
                InspectRunState::Running => {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        }
        let kill_opts = KillContainerOptionsBuilder::default()
            .signal("SIGKILL")
            .build();
        self.inner
            .kill_container(name, Some(kill_opts))
            .await
            .map_err(|e| UpdaterError::Docker(format!("kill {name}: {e}")))?;
        match self.inspect_run_state(name).await? {
            InspectRunState::Absent | InspectRunState::Stopped => Ok(()),
            InspectRunState::Running => Err(UpdaterError::Precondition(format!(
                "{name} is still running after kill; database restore forbidden"
            ))),
        }
    }

    /// Prove every known postgres container name is stopped before PGDATA restore.
    /// A failure to inspect or stop any name aborts restore — 404 on a name is OK.
    pub async fn stop_postgres_for_restore(&self) -> Result<()> {
        for name in ["myriad-postgres", "postgres"] {
            self.force_stop_container(name).await?;
        }
        Ok(())
    }

    /// Prove every known database writer is stopped before snapshot restore.
    pub async fn stop_app_writers_for_restore(&self) -> Result<()> {
        for name in [
            "myriad-federation-worker",
            "myriad-persona-worker",
            "myriad-backend",
            "backend",
        ] {
            self.force_stop_container(name).await?;
        }
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

    async fn worker_names(&self) -> Result<Vec<String>> {
        // Guard denies missing inspect requests; inventory distinguishes absence.
        let options = bollard::query_parameters::ListContainersOptionsBuilder::default()
            .all(true)
            .build();
        let containers = self
            .inner
            .list_containers(Some(options))
            .await
            .map_err(|error| UpdaterError::Docker(format!("list workers: {error}")))?;
        Ok(containers
            .into_iter()
            .flat_map(|container| container.names.unwrap_or_default())
            .map(|name| name.trim_start_matches('/').to_owned())
            .collect())
    }

    /// Also stops a writer removed from the host Compose file.
    pub async fn stop_worker(&self, role: &str) -> Result<()> {
        let name = format!("myriad-{role}");
        if !self.worker_names().await?.contains(&name) {
            return Ok(());
        }
        // force_stop_container already verifies the terminal state after stop/kill.
        self.force_stop_container(&name).await
    }

    pub async fn worker_support(&self, image: &str) -> Result<[bool; 2]> {
        let info =
            self.inner.inspect_image(image).await.map_err(|error| {
                UpdaterError::Docker(format!("inspect worker capability: {error}"))
            })?;
        let labels = info
            .config
            .and_then(|config| config.labels)
            .unwrap_or_default();
        Ok(APP_WORKERS.map(|role| {
            labels
                .get(&format!("io.myriad.runtime.{role}"))
                .is_some_and(|value| value == "1")
        }))
    }

    pub async fn workers_healthy(&self) -> Result<bool> {
        let backend = self
            .inner
            .inspect_container("myriad-backend", None)
            .await
            .map_err(|error| UpdaterError::Docker(format!("inspect backend role: {error}")))?;
        let image = backend
            .image
            .ok_or_else(|| UpdaterError::Docker("backend image identity missing".into()))?;
        let web = backend
            .config
            .as_ref()
            .and_then(|config| config.env.as_ref())
            .is_some_and(|env| env.iter().any(|value| value == "MYRIAD_PROCESS_ROLE=web"));
        let support = if web {
            self.worker_support(&image).await?
        } else {
            [false, false]
        };
        let names = self.worker_names().await?;
        for (role, supported) in APP_WORKERS.into_iter().zip(support) {
            let split = web && supported;
            let name = format!("myriad-{role}");
            if !names.contains(&name) {
                if split {
                    return Ok(false);
                }
                continue;
            }
            let worker = self
                .inner
                .inspect_container(&name, None)
                .await
                .map_err(|error| UpdaterError::Docker(format!("inspect {role}: {error}")))?;
            let state = worker.state;
            if !split {
                if state.and_then(|state| state.running) == Some(true) {
                    return Ok(false);
                }
                continue;
            }
            if worker.image.as_deref() != Some(image.as_str()) {
                return Ok(false);
            }
            let Some(state) = state else {
                return Ok(false);
            };
            let healthy = if state.running == Some(true) {
                state
                    .health
                    .as_ref()
                    .and_then(|health| health.status.as_ref())
                    .is_some_and(|status| status.to_string().eq_ignore_ascii_case("healthy"))
            } else {
                // Federation can exit normally when its application gate is closed.
                role == "federation-worker" && state.exit_code == Some(0)
            };
            if !healthy {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// True if the named image ref exists locally (inspect succeeds).
    pub async fn image_exists_local(&self, image_ref: &str) -> bool {
        self.inner.inspect_image(image_ref).await.is_ok()
    }
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

#[cfg(test)]
mod worker_presence_tests {
    use super::*;
    #[tokio::test]
    async fn readiness_uses_actual_image_and_local_health_not_config_tag() {
        use std::future::IntoFuture;
        let current = std::sync::Arc::new(std::sync::Mutex::new(serde_json::json!({
            "Image": "sha256:selected",
            "Config": {"Image": "repo:v0.5.30"},
            "State": {"Running": true, "Health": {"Status": "healthy"}}
        })));
        let response = current.clone();
        let app = axum::Router::new().fallback(move || {
            let value = response.lock().unwrap().clone();
            async move { axum::Json(value) }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(axum::serve(listener, app).into_future());
        let client = DockerClient {
            inner: Docker::connect_with_http(&address, 2, bollard::API_DEFAULT_VERSION).unwrap(),
        };
        assert!(
            client
                .container_ready("frontend", "sha256:selected")
                .await
                .unwrap()
        );
        assert!(
            !client
                .container_ready("frontend", "sha256:other")
                .await
                .unwrap()
        );
        current.lock().unwrap()["State"]["Health"]["Status"] = serde_json::json!("starting");
        assert!(
            !client
                .container_ready("frontend", "sha256:selected")
                .await
                .unwrap()
        );
        current.lock().unwrap()["State"] = serde_json::json!({"Running": true});
        assert!(
            !client
                .container_ready("frontend", "sha256:selected")
                .await
                .unwrap()
        );
        server.abort();
    }
    use axum::{
        Json, Router,
        extract::State,
        http::{StatusCode, Uri},
        response::IntoResponse,
    };
    use std::future::IntoFuture;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    #[tokio::test]
    async fn absence_is_safe_but_guard_inspection_failure_blocks_restore() {
        let present = Arc::new(AtomicBool::new(false));
        async fn fake_guard(State(present): State<Arc<AtomicBool>>, uri: Uri) -> impl IntoResponse {
            if uri.path().ends_with("/containers/json") {
                let containers = if present.load(Ordering::Acquire) {
                    serde_json::json!([{"Id":"worker", "Names":["/myriad-federation-worker"]}])
                } else {
                    serde_json::json!([])
                };
                (StatusCode::OK, Json(containers))
            } else {
                (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"message":"container authorization failed"})),
                )
            }
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(
            axum::serve(
                listener,
                Router::new()
                    .fallback(fake_guard)
                    .with_state(present.clone()),
            )
            .into_future(),
        );
        let client = DockerClient {
            inner: Docker::connect_with_http(&address, 2, bollard::API_DEFAULT_VERSION).unwrap(),
        };
        assert!(client.stop_worker("federation-worker").await.is_ok());
        present.store(true, Ordering::Release);
        assert!(client.stop_worker("federation-worker").await.is_err());
        server.abort();
    }
    #[tokio::test]
    async fn worker_health_requires_the_running_backend_image_and_capability() {
        use std::sync::Mutex;
        #[derive(Clone)]
        struct Case {
            capable: bool,
            present: bool,
            matching: bool,
            healthy: bool,
            web: bool,
            running: bool,
            exit_code: i64,
        }
        let state = Arc::new(Mutex::new(Case {
            capable: true,
            present: true,
            matching: true,
            healthy: true,
            web: true,
            running: true,
            exit_code: 0,
        }));
        async fn fake_guard(
            State(state): State<Arc<Mutex<Case>>>,
            uri: Uri,
        ) -> Json<serde_json::Value> {
            let case = state.lock().unwrap().clone();
            let path = uri.path();
            let value = if path.ends_with("/containers/json") {
                if case.present {
                    serde_json::json!([{"Id":"worker","Names":["/myriad-federation-worker"]}])
                } else {
                    serde_json::json!([])
                }
            } else if path.ends_with("/containers/myriad-backend/json") {
                serde_json::json!({"Image":"sha256:backend", "Config":{"Env": if case.web {vec!["MYRIAD_PROCESS_ROLE=web"]} else {vec![]}}})
            } else if path.ends_with("/containers/myriad-federation-worker/json") {
                serde_json::json!({"Image": if case.matching {"sha256:backend"} else {"sha256:previous"},
                    "State":{"Running":case.running,"ExitCode":case.exit_code,"Health":{"Status": if case.healthy {"healthy"} else {"unhealthy"}}}})
            } else {
                serde_json::json!({"Config":{"Labels":{"io.myriad.runtime.federation-worker": if case.capable {"1"} else {""}}}})
            };
            Json(value)
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(
            axum::serve(
                listener,
                Router::new().fallback(fake_guard).with_state(state.clone()),
            )
            .into_future(),
        );
        let client = DockerClient {
            inner: Docker::connect_with_http(&address, 2, bollard::API_DEFAULT_VERSION).unwrap(),
        };
        assert!(client.workers_healthy().await.unwrap());
        state.lock().unwrap().running = false;
        assert!(client.workers_healthy().await.unwrap());
        state.lock().unwrap().exit_code = 1;
        assert!(!client.workers_healthy().await.unwrap());
        state.lock().unwrap().running = true;
        state.lock().unwrap().exit_code = 0;
        state.lock().unwrap().matching = false;
        assert!(!client.workers_healthy().await.unwrap());
        state.lock().unwrap().matching = true;
        state.lock().unwrap().healthy = false;
        assert!(!client.workers_healthy().await.unwrap());
        state.lock().unwrap().present = false;
        assert!(!client.workers_healthy().await.unwrap());
        state.lock().unwrap().capable = false;
        assert!(client.workers_healthy().await.unwrap());
        state.lock().unwrap().present = true;
        assert!(!client.workers_healthy().await.unwrap());
        state.lock().unwrap().web = false;
        state.lock().unwrap().capable = true;
        state.lock().unwrap().present = false;
        assert!(client.workers_healthy().await.unwrap());
        server.abort();
    }
    #[tokio::test]
    async fn persona_absence_is_safe_but_guard_inspection_failure_blocks_restore() {
        let present = Arc::new(AtomicBool::new(false));
        async fn fake_guard(State(present): State<Arc<AtomicBool>>, uri: Uri) -> impl IntoResponse {
            if uri.path().ends_with("/containers/json") {
                let containers = if present.load(Ordering::Acquire) {
                    serde_json::json!([{"Id":"worker", "Names":["/myriad-persona-worker"]}])
                } else {
                    serde_json::json!([])
                };
                (StatusCode::OK, Json(containers))
            } else {
                (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({"message":"container authorization failed"})),
                )
            }
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(
            axum::serve(
                listener,
                Router::new()
                    .fallback(fake_guard)
                    .with_state(present.clone()),
            )
            .into_future(),
        );
        let client = DockerClient {
            inner: Docker::connect_with_http(&address, 2, bollard::API_DEFAULT_VERSION).unwrap(),
        };
        assert!(client.stop_worker("persona-worker").await.is_ok());
        present.store(true, Ordering::Release);
        assert!(client.stop_worker("persona-worker").await.is_err());
        server.abort();
    }
    #[tokio::test]
    async fn persona_worker_health_requires_the_running_backend_image_and_capability() {
        use std::sync::Mutex;
        #[derive(Clone)]
        struct Case {
            capable: bool,
            present: bool,
            matching: bool,
            healthy: bool,
            web: bool,
        }
        let state = Arc::new(Mutex::new(Case {
            capable: true,
            present: true,
            matching: true,
            healthy: true,
            web: true,
        }));
        async fn fake_guard(
            State(state): State<Arc<Mutex<Case>>>,
            uri: Uri,
        ) -> Json<serde_json::Value> {
            let case = state.lock().unwrap().clone();
            let path = uri.path();
            let value = if path.ends_with("/containers/json") {
                if case.present {
                    serde_json::json!([{"Id":"worker","Names":["/myriad-persona-worker"]}])
                } else {
                    serde_json::json!([])
                }
            } else if path.ends_with("/containers/myriad-backend/json") {
                serde_json::json!({"Image":"sha256:backend", "Config":{"Env": if case.web {vec!["MYRIAD_PROCESS_ROLE=web"]} else {vec![]}}})
            } else if path.ends_with("/containers/myriad-persona-worker/json") {
                serde_json::json!({"Image": if case.matching {"sha256:backend"} else {"sha256:previous"},
                    "State":{"Running":true,"Health":{"Status": if case.healthy {"healthy"} else {"unhealthy"}}}})
            } else {
                serde_json::json!({"Config":{"Labels":{"io.myriad.runtime.persona-worker": if case.capable {"1"} else {""}}}})
            };
            Json(value)
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(
            axum::serve(
                listener,
                Router::new().fallback(fake_guard).with_state(state.clone()),
            )
            .into_future(),
        );
        let client = DockerClient {
            inner: Docker::connect_with_http(&address, 2, bollard::API_DEFAULT_VERSION).unwrap(),
        };
        assert!(client.workers_healthy().await.unwrap());
        state.lock().unwrap().matching = false;
        assert!(!client.workers_healthy().await.unwrap());
        state.lock().unwrap().matching = true;
        state.lock().unwrap().healthy = false;
        assert!(!client.workers_healthy().await.unwrap());
        state.lock().unwrap().present = false;
        assert!(!client.workers_healthy().await.unwrap());
        state.lock().unwrap().capable = false;
        assert!(client.workers_healthy().await.unwrap());
        state.lock().unwrap().present = true;
        assert!(!client.workers_healthy().await.unwrap());
        state.lock().unwrap().web = false;
        state.lock().unwrap().capable = true;
        state.lock().unwrap().present = false;
        assert!(client.workers_healthy().await.unwrap());
        server.abort();
    }

    #[tokio::test]
    async fn force_stop_treats_inspect_404_as_already_stopped() {
        async fn fake_guard(_uri: Uri) -> impl IntoResponse {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"message": "no such container"})),
            )
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let server =
            tokio::spawn(axum::serve(listener, Router::new().fallback(fake_guard)).into_future());
        let client = DockerClient {
            inner: Docker::connect_with_http(&address, 2, bollard::API_DEFAULT_VERSION).unwrap(),
        };
        client
            .force_stop_container("missing")
            .await
            .expect("404 is already stopped");
        client
            .stop_postgres_for_restore()
            .await
            .expect("both postgres names 404 is stopped");
        server.abort();
    }

    #[tokio::test]
    async fn force_stop_does_not_treat_inspect_failure_as_stopped() {
        async fn fake_guard(_uri: Uri) -> impl IntoResponse {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"message": "daemon unavailable"})),
            )
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let server =
            tokio::spawn(axum::serve(listener, Router::new().fallback(fake_guard)).into_future());
        let client = DockerClient {
            inner: Docker::connect_with_http(&address, 2, bollard::API_DEFAULT_VERSION).unwrap(),
        };
        let err = client
            .force_stop_container("myriad-postgres")
            .await
            .expect_err("inspect 500 is not already-stopped");
        assert!(
            err.to_string().contains("inspect"),
            "expected inspect error, got {err}"
        );
        let err = client
            .stop_postgres_for_restore()
            .await
            .expect_err("postgres restore stop must fail closed");
        assert!(
            err.to_string().contains("inspect"),
            "expected inspect error, got {err}"
        );
        server.abort();
    }

    #[tokio::test]
    async fn force_stop_does_not_treat_forbidden_inspect_as_stopped() {
        async fn fake_guard() -> impl IntoResponse {
            (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"message": "container authorization failed"})),
            )
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let server =
            tokio::spawn(axum::serve(listener, Router::new().fallback(fake_guard)).into_future());
        let client = DockerClient {
            inner: Docker::connect_with_http(&address, 2, bollard::API_DEFAULT_VERSION).unwrap(),
        };
        assert!(
            client
                .force_stop_container("myriad-postgres")
                .await
                .is_err(),
            "403 inspect must not continue as already-stopped"
        );
        server.abort();
    }
}
