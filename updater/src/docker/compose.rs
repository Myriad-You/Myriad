//! Compose runner: shells out to `docker compose -p <project> -f <files> ...`.
//!
//! We don't reach for the compose-as-library crate because it's not first-party. Shelling out
//! is what users would do; it keeps behavior obvious and avoids API drift.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tracing::debug;

use crate::error::{Result, UpdaterError};
use crate::probe::compose::ComposeBinary;

pub struct ComposeRunner {
    binary: ComposeBinary,
    project: String,
    files: Vec<PathBuf>,
    /// Path to .env. Passed as --env-file so compose sees the same set as we do.
    env_file: PathBuf,
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
        workdir: PathBuf,
        project_directory: PathBuf,
    ) -> Self {
        Self {
            binary,
            project: project.into(),
            files,
            env_file,
            workdir,
            project_directory,
        }
    }

    fn base_cmd(&self) -> Command {
        let mut c = self.binary.command(&self.project, &self.files);
        c.arg("--project-directory")
            .arg(&self.project_directory)
            .arg("--env-file")
            .arg(&self.env_file);
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

    pub async fn stop(&self, services: &[&str], timeout_secs: u32) -> Result<ComposeOutput> {
        let timeout_str = timeout_secs.to_string();
        let mut args: Vec<&str> = vec!["stop", "-t", &timeout_str];
        args.extend_from_slice(services);
        self.run(&args, Duration::from_secs((timeout_secs as u64) + 60))
            .await
    }

    pub async fn start(&self, services: &[&str]) -> Result<ComposeOutput> {
        let mut args: Vec<&str> = vec!["start"];
        args.extend_from_slice(services);
        self.run(&args, Duration::from_secs(120)).await
    }

    pub async fn up_detached(&self, services: &[&str]) -> Result<ComposeOutput> {
        let mut args: Vec<&str> = vec!["up", "-d", "--no-deps"];
        args.extend_from_slice(services);
        self.run(&args, Duration::from_secs(600)).await
    }

    /// Repair backend named-volume ownership with the target backend image's
    /// narrowly scoped init mode. The regular backend container remains uid
    /// 1000; only this disposable container runs as root.
    pub async fn init_backend_volumes(&self) -> Result<ComposeOutput> {
        let container_name = format!(
            "{}-backend-volume-init-{}",
            self.project,
            uuid::Uuid::new_v4().simple()
        );
        let started = self
            .run(
                &[
                    "run",
                    "--detach",
                    "--no-deps",
                    "--no-TTY",
                    "--name",
                    &container_name,
                    "--user",
                    "0:0",
                    "-e",
                    "MYRIAD_VOLUME_INIT_ONLY=true",
                    "backend",
                ],
                Duration::from_secs(600),
            )
            .await?;
        if !started.ok() {
            return Ok(started);
        }

        let container_id = started
            .stdout_tail
            .lines()
            .rev()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .ok_or_else(|| {
                UpdaterError::Docker(
                    "backend volume initializer did not return a container id".into(),
                )
            })?;
        let waited = self
            .run_docker(&["wait", container_id], Duration::from_secs(600))
            .await;
        let removed = self
            .run_docker(&["rm", "--force", container_id], Duration::from_secs(120))
            .await;

        let waited = waited?;
        let removed = removed?;
        if !removed.ok() {
            return Ok(removed);
        }
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
        Ok(ComposeOutput {
            status: init_exit,
            stdout_tail: waited.stdout_tail,
            stderr_tail: waited.stderr_tail,
        })
    }

    async fn run_docker(&self, args: &[&str], timeout: Duration) -> Result<ComposeOutput> {
        let mut command = Command::new("docker");
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

    pub async fn ps_json(&self) -> Result<String> {
        let out = self
            .run(&["ps", "--format", "json"], Duration::from_secs(30))
            .await?;
        if !out.ok() {
            return Err(UpdaterError::Docker(format!(
                "compose ps: {}",
                out.error_summary()
            )));
        }
        Ok(out.stdout_tail)
    }
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
