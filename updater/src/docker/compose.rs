//! Compose runner: shells out to `docker compose -p <project> -f <files> ...`.
//!
//! We don't reach for the compose-as-library crate because it's not first-party. Shelling out
//! is what users would do; it keeps behavior obvious and avoids API drift.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tracing::{debug, info};

use crate::error::{Result, UpdaterError};
use crate::probe::compose::ComposeBinary;

pub(crate) const GUARD_ENV_KEYS: [&str; 7] = [
    "DOCKER_GUARD_IMAGE",
    "DOCKER_GUARD_LOG",
    "GUARD_COMPOSE_PROJECT_NAME",
    "GUARD_MYRIAD_DOCKER_NETWORK",
    "GUARD_MYRIAD_ADMIN_NETWORK",
    "GUARD_MYRIAD_DOCKER_GUARD_NETWORK",
    "MYRIAD_GUARD_ENV_FILE",
];
pub(crate) const GUARDED_DOCKER_HOST: &str = "tcp://docker-guard:2375";

pub(crate) fn harden_docker_command(command: &mut Command) {
    let guarded_host = if cfg!(debug_assertions) {
        std::env::var("UPDATER_DEBUG_GUARDED_DOCKER_HOST")
            .unwrap_or_else(|_| GUARDED_DOCKER_HOST.to_string())
    } else {
        GUARDED_DOCKER_HOST.to_string()
    };
    for key in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
    ] {
        command.env_remove(key);
    }
    command
        .env_remove("DOCKER_CONTEXT")
        .env_remove("DOCKER_CONFIG")
        .env("DOCKER_HOST", guarded_host);
}

pub(crate) fn guard_env_file_path() -> PathBuf {
    if cfg!(debug_assertions) {
        std::env::var("UPDATER_GUARD_ENV_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| "/run/secrets/docker-guard.env".into())
    } else {
        PathBuf::from("/run/secrets/docker-guard.env")
    }
}

pub(crate) fn validate_guard_policy_file(path: &std::path::Path) -> Result<()> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        UpdaterError::Precondition(format!(
            "cannot read host-owned Guard policy {}: {error}",
            path.display()
        ))
    })?;
    let mut values = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(UpdaterError::Precondition(
                "Guard policy contains a malformed line".into(),
            ));
        };
        if values.insert(key, value).is_some() {
            return Err(UpdaterError::Precondition(format!(
                "Guard policy contains duplicate key {key}"
            )));
        }
    }
    for key in [
        "DOCKER_GUARD_IMAGE",
        "GUARD_COMPOSE_PROJECT_NAME",
        "GUARD_MYRIAD_DOCKER_NETWORK",
        "GUARD_MYRIAD_ADMIN_NETWORK",
        "GUARD_MYRIAD_DOCKER_GUARD_NETWORK",
        "MYRIAD_GUARD_ENV_FILE",
    ] {
        if values.get(key).is_none_or(|value| value.trim().is_empty()) {
            return Err(UpdaterError::Precondition(format!(
                "host-owned Guard policy is missing {key}"
            )));
        }
    }
    let configured_path = values["MYRIAD_GUARD_ENV_FILE"];
    if configured_path.trim() != "guard-policy/docker-guard.env" {
        return Err(UpdaterError::Precondition(format!(
            "MYRIAD_GUARD_ENV_FILE must be guard-policy/docker-guard.env (got {configured_path})"
        )));
    }
    let image = values["DOCKER_GUARD_IMAGE"];
    if cfg!(debug_assertions) && image.starts_with("myriad-updater-dev:") {
        return Ok(());
    }
    let prefix = "docker.io/somekawahitomi/myriad-updater@sha256:";
    let digest = image.strip_prefix(prefix).ok_or_else(|| {
        UpdaterError::Precondition("Guard policy image is outside the trusted repository".into())
    })?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(UpdaterError::Precondition(
            "Guard policy image must use an exact sha256 digest".into(),
        ));
    }
    Ok(())
}

pub struct ComposeRunner {
    binary: ComposeBinary,
    project: String,
    files: Vec<PathBuf>,
    /// Path to .env. Passed as --env-file so compose sees the same set as we do.
    env_file: PathBuf,
    /// Host-owned Guard TCB policy, mounted read-only into the updater. Passed
    /// after `.env` to preserve host-owned network policy. Normal image selection
    /// follows TAG; the generic Docker API still cannot recreate TCB services.
    guard_env_file: PathBuf,
    /// Working directory for compose, so relative bind mounts resolve correctly.
    workdir: PathBuf,
    /// Host-side project directory used to resolve relative bind sources for the daemon.
    project_directory: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ComposeOutput {
    pub status: i32,
    pub stdout_tail: String,
    pub stderr_tail: String,
}

impl ComposeOutput {
    pub fn ok(&self) -> bool {
        self.status == 0
    }
    pub fn error_summary(&self) -> String {
        format!(
            "exit {}: stdout={:?} stderr={:?}",
            self.status, self.stdout_tail, self.stderr_tail
        )
    }
}

impl ComposeRunner {
    pub fn new(
        binary: ComposeBinary,
        project: impl Into<String>,
        files: Vec<PathBuf>,
        env_file: PathBuf,
        guard_env_file: PathBuf,
        workdir: PathBuf,
        project_directory: PathBuf,
    ) -> Self {
        Self {
            binary,
            project: project.into(),
            files,
            env_file,
            guard_env_file,
            workdir,
            project_directory,
        }
    }

    fn base_cmd(&self) -> Command {
        let mut c = self.binary.command(&self.project, &self.files);
        // Compose gives the calling process environment precedence over every
        // --env-file. Strip all Guard-TCB interpolation keys so a compromised
        // updater cannot override the host-owned policy file when spawning the
        // Compose subprocess.
        for key in GUARD_ENV_KEYS {
            c.env_remove(key);
        }
        harden_docker_command(&mut c);
        c.arg("--project-directory")
            .arg(&self.project_directory)
            .arg("--env-file")
            .arg(&self.env_file)
            .arg("--env-file")
            .arg(&self.guard_env_file);
        c.current_dir(&self.workdir);
        c.stdout(Stdio::piped());
        c.stderr(Stdio::piped());
        c
    }

    /// Run `compose <args...>` with a timeout. Captures stdout/stderr (last 32 KiB each).
    pub async fn run(&self, args: &[&str], timeout: Duration) -> Result<ComposeOutput> {
        let mut cmd = self.base_cmd();
        cmd.args(args);
        debug!(?args, project = %self.project, "compose exec");

        let mut child = cmd
            .spawn()
            .map_err(|e| UpdaterError::Docker(format!("spawn compose {args:?}: {e}")))?;

        let mut stdout_buf = Vec::<u8>::with_capacity(8192);
        let mut stderr_buf = Vec::<u8>::with_capacity(8192);
        let mut stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();

        let copy_stdout = async {
            let _ = stdout.read_to_end(&mut stdout_buf).await;
        };
        let copy_stderr = async {
            let _ = stderr.read_to_end(&mut stderr_buf).await;
        };
        let wait_status = async { child.wait().await };

        let status = tokio::select! {
            res = async {
                tokio::join!(copy_stdout, copy_stderr);
                wait_status.await
            } => res,
            _ = tokio::time::sleep(timeout) => {
                let _ = child.start_kill();
                return Err(UpdaterError::Docker(format!(
                    "compose {args:?} timed out after {}s", timeout.as_secs()
                )));
            }
        };

        let status =
            status.map_err(|e| UpdaterError::Docker(format!("await compose {args:?}: {e}")))?;
        Ok(ComposeOutput {
            status: status.code().unwrap_or(-1),
            stdout_tail: tail_string(&stdout_buf, 32 * 1024),
            stderr_tail: tail_string(&stderr_buf, 32 * 1024),
        })
    }

    /// Optional on older host compose files. New topology uses the same backend
    /// image with a fixed dedicated executable; role support is an image capability.
    pub async fn federation_worker_image(&self) -> Result<Option<String>> {
        let config = self.config_json().await?;
        let Some(worker) = config.pointer("/services/federation-worker") else {
            return Ok(None);
        };
        let image = worker
            .get("image")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| UpdaterError::Precondition("federation worker image missing".into()))?;
        if config
            .pointer("/services/backend/image")
            .and_then(serde_json::Value::as_str)
            != Some(image)
        {
            return Err(UpdaterError::Precondition(
                "federation worker must use the backend image".into(),
            ));
        }
        Ok(Some(image.into()))
    }

    pub async fn persona_worker_image(&self) -> Result<Option<String>> {
        let config = self.config_json().await?;
        let Some(worker) = config.pointer("/services/persona-worker") else {
            return Ok(None);
        };
        let image = worker
            .get("image")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| UpdaterError::Precondition("persona worker image missing".into()))?;
        if config
            .pointer("/services/backend/image")
            .and_then(serde_json::Value::as_str)
            != Some(image)
        {
            return Err(UpdaterError::Precondition(
                "persona worker must use the backend image".into(),
            ));
        }
        Ok(Some(image.into()))
    }

    async fn application_services<'a>(
        &self,
        requested: &[&'a str],
        starting: bool,
    ) -> Result<Vec<&'a str>> {
        let mut services = requested.to_vec();
        if requested.contains(&"backend")
            && !requested.contains(&"federation-worker")
            && let Some(image) = self.federation_worker_image().await?
        {
            let supported = !starting
                || super::DockerClient::connect()
                    .await?
                    .supports_federation_worker(&image)
                    .await?;
            if supported {
                services.insert(0, "federation-worker");
            }
        }
        if requested.contains(&"backend")
            && !requested.contains(&"persona-worker")
            && let Some(image) = self.persona_worker_image().await?
        {
            let supported = !starting
                || super::DockerClient::connect()
                    .await?
                    .supports_persona_worker(&image)
                    .await?;
            if supported {
                services.insert(0, "persona-worker");
            }
        }
        Ok(services)
    }

    pub async fn stop(&self, services: &[&str], timeout_secs: u32) -> Result<ComposeOutput> {
        if services.contains(&"backend") {
            super::DockerClient::connect()
                .await?
                .stop_federation_worker()
                .await?;
            super::DockerClient::connect()
                .await?
                .stop_persona_worker()
                .await?;
        }
        let services = self.application_services(services, false).await?;
        let timeout_str = timeout_secs.to_string();
        let mut args: Vec<&str> = vec!["stop", "-t", &timeout_str];
        args.extend_from_slice(&services);
        self.run(&args, Duration::from_secs((timeout_secs as u64) + 60))
            .await
    }

    pub async fn start(&self, services: &[&str]) -> Result<ComposeOutput> {
        let services = self.application_services(services, true).await?;
        let mut args: Vec<&str> = vec!["start"];
        args.extend_from_slice(&services);
        self.run(&args, Duration::from_secs(120)).await
    }

    pub async fn up_detached(&self, services: &[&str]) -> Result<ComposeOutput> {
        self.up_detached_opts(services, false).await
    }

    /// Like [`up_detached`] but with `--force-recreate` so tag swaps / rollbacks do not
    /// reuse stopped containers that still match a stale create config.
    pub async fn up_detached_recreate(&self, services: &[&str]) -> Result<ComposeOutput> {
        self.up_detached_opts(services, true).await
    }

    async fn up_detached_opts(
        &self,
        services: &[&str],
        force_recreate: bool,
    ) -> Result<ComposeOutput> {
        let services = self.application_services(services, true).await?;
        let mut args: Vec<&str> = vec!["up", "-d", "--no-deps", "--pull", "never"];
        if force_recreate {
            args.push("--force-recreate");
        }
        args.extend_from_slice(&services);
        self.run(&args, Duration::from_secs(600)).await
    }

    /// Repair backend named-volume ownership with the target backend image.
    /// Recreate the compose service in place so the completed latch tracks the
    /// current tag; a parallel `compose run` would leave the named container
    /// pinning the previous image. The regular backend stays uid 1000.
    pub async fn init_backend_volumes(&self) -> Result<ComposeOutput> {
        let started = self
            .run(
                &[
                    "up",
                    "-d",
                    "--no-deps",
                    "--force-recreate",
                    "--pull",
                    "never",
                    "backend-volume-init",
                ],
                Duration::from_secs(600),
            )
            .await?;
        if !started.ok() {
            return Ok(started);
        }

        let container_id = self.volume_init_service_container_id().await?;
        info!(%container_id, "backend volume initializer recreated");
        let waited = self
            .run_docker(&["wait", &container_id], Duration::from_secs(600))
            .await?;
        // Capture the init entrypoint's uid-1000 write-probe diagnostics.
        // Failure to read logs is diagnostic only; `docker wait` is authoritative.
        let logs = self
            .run_docker(
                &["logs", "--tail", "10000", &container_id],
                Duration::from_secs(120),
            )
            .await;
        if !waited.ok() {
            return Ok(waited);
        }
        let init_exit = waited
            .stdout_tail
            .lines()
            .rev()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .and_then(|line| line.parse::<i32>().ok())
            .ok_or_else(|| {
                UpdaterError::Docker(format!(
                    "backend volume initializer returned invalid wait output: {:?}",
                    waited.stdout_tail
                ))
            })?;
        let (init_stdout, init_stderr) = match logs {
            Ok(output) if output.ok() => (output.stdout_tail, output.stderr_tail),
            Ok(output) => (
                String::new(),
                format!(
                    "unable to read backend volume initializer logs: {}",
                    output.error_summary()
                ),
            ),
            Err(error) => (
                String::new(),
                format!("unable to read backend volume initializer logs: {error}"),
            ),
        };
        Ok(ComposeOutput {
            status: init_exit,
            stdout_tail: init_stdout,
            stderr_tail: init_stderr,
        })
    }

    async fn volume_init_service_container_id(&self) -> Result<String> {
        let project_filter = format!("label=com.docker.compose.project={}", self.project);
        let listed = self
            .run_docker(
                &[
                    "ps",
                    "-aq",
                    "--filter",
                    &project_filter,
                    "--filter",
                    "label=com.docker.compose.service=backend-volume-init",
                    "--filter",
                    "label=com.docker.compose.oneoff=False",
                ],
                Duration::from_secs(30),
            )
            .await?;
        if !listed.ok() {
            return Err(UpdaterError::Docker(format!(
                "list backend-volume-init container failed: {}",
                listed.error_summary()
            )));
        }
        first_container_id(&listed.stdout_tail).ok_or_else(|| {
            UpdaterError::Docker(
                "backend volume initializer did not create a compose service container".into(),
            )
        })
    }

    async fn run_docker(&self, args: &[&str], timeout: Duration) -> Result<ComposeOutput> {
        let mut command = Command::new("docker");
        harden_docker_command(&mut command);
        command
            .args(args)
            .current_dir(&self.workdir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        debug!(?args, "docker exec");
        let output = tokio::time::timeout(timeout, command.output())
            .await
            .map_err(|_| {
                UpdaterError::Docker(format!(
                    "docker {args:?} timed out after {}s",
                    timeout.as_secs()
                ))
            })?
            .map_err(|error| UpdaterError::Docker(format!("spawn docker {args:?}: {error}")))?;
        Ok(ComposeOutput {
            status: output.status.code().unwrap_or(-1),
            stdout_tail: tail_string(&output.stdout, 32 * 1024),
            stderr_tail: tail_string(&output.stderr, 32 * 1024),
        })
    }

    pub async fn pull(&self, services: &[&str]) -> Result<ComposeOutput> {
        let mut args: Vec<&str> = vec!["pull"];
        args.extend_from_slice(services);
        self.run(&args, Duration::from_secs(1800)).await
    }

    /// Resolved compose project config as JSON (full stdout; not truncated).
    ///
    /// Used by preflight network allowlist checks — must not truncate lest we
    /// parse incomplete JSON on larger stacks.
    pub async fn config_json(&self) -> Result<serde_json::Value> {
        let mut cmd = self.base_cmd();
        cmd.args(["config", "--format", "json"]);
        debug!(project = %self.project, "compose config --format json");

        let mut child = cmd
            .spawn()
            .map_err(|e| UpdaterError::Docker(format!("spawn compose config: {e}")))?;

        let mut stdout_buf = Vec::<u8>::with_capacity(64 * 1024);
        let mut stderr_buf = Vec::<u8>::with_capacity(8192);
        let mut stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();

        let copy_stdout = async {
            let _ = stdout.read_to_end(&mut stdout_buf).await;
        };
        let copy_stderr = async {
            let _ = stderr.read_to_end(&mut stderr_buf).await;
        };
        let wait_status = async { child.wait().await };

        let status = tokio::select! {
            res = async {
                tokio::join!(copy_stdout, copy_stderr);
                wait_status.await
            } => res,
            _ = tokio::time::sleep(Duration::from_secs(60)) => {
                let _ = child.start_kill();
                return Err(UpdaterError::Docker(
                    "compose config timed out after 60s".into(),
                ));
            }
        };

        let status =
            status.map_err(|e| UpdaterError::Docker(format!("await compose config: {e}")))?;
        if !status.success() {
            return Err(UpdaterError::Docker(format!(
                "compose config failed: exit {:?}: {}",
                status.code(),
                String::from_utf8_lossy(&stderr_buf).trim()
            )));
        }
        serde_json::from_slice(&stdout_buf).map_err(|e| {
            UpdaterError::Docker(format!(
                "compose config JSON parse failed ({e}); stdout_len={}",
                stdout_buf.len()
            ))
        })
    }

    pub fn project(&self) -> &str {
        &self.project
    }
}

fn first_container_id(ps_output: &str) -> Option<String> {
    ps_output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn tail_string(buf: &[u8], limit: usize) -> String {
    if buf.len() <= limit {
        return String::from_utf8_lossy(buf).into_owned();
    }
    let start = buf.len() - limit;
    let slice = &buf[start..];
    let s = String::from_utf8_lossy(slice).into_owned();
    format!("…(truncated {} bytes)\n{}", start, s)
}

#[cfg(test)]
mod first_container_id_tests {
    use super::first_container_id;

    #[test]
    fn skips_blank_lines() {
        assert_eq!(
            first_container_id("\n  \n8a0903d4f94f\nabc123deadbe\n").as_deref(),
            Some("8a0903d4f94f")
        );
    }

    #[test]
    fn empty_ps_output_is_none() {
        assert_eq!(first_container_id(""), None);
        assert_eq!(first_container_id("\n  \n"), None);
    }
}
