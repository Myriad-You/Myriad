//! Trusted one-shot handoff for the updater TCB.
//!
//! The running Guard starts this binary only from an image whose official
//! repository and immutable digest it has independently verified. The helper
//! has one operation: converge `docker-guard`, `updater`, and
//! `updater-gateway` on that exact image, or restore the previous exact image.

use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::env_file::EnvFile;
use crate::error::{Result, UpdaterError};
use crate::state::atomic;

pub const ENV_PREVIOUS_IMAGE: &str = "MYRIAD_SELF_UPDATE_PREVIOUS_IMAGE";
pub const ENV_TARGET_IMAGE: &str = "MYRIAD_SELF_UPDATE_TARGET_IMAGE";
pub const ENV_PREVIOUS_TAG: &str = "MYRIAD_SELF_UPDATE_PREVIOUS_TAG";
pub const ENV_TARGET_TAG: &str = "MYRIAD_SELF_UPDATE_TARGET_TAG";
pub const ENV_PROJECT: &str = "MYRIAD_SELF_UPDATE_PROJECT";
pub const ENV_PROJECT_DIRECTORY: &str = "MYRIAD_SELF_UPDATE_PROJECT_DIRECTORY";
pub const ENV_HOST_COMPOSE_ROOT: &str = "MYRIAD_SELF_UPDATE_HOST_COMPOSE_ROOT";
pub const ENV_COMPOSE_DIR: &str = "MYRIAD_SELF_UPDATE_COMPOSE_DIR";
pub const ENV_APP_ENV_FILE: &str = "MYRIAD_SELF_UPDATE_APP_ENV_FILE";
pub const ENV_GUARD_ENV_FILE: &str = "MYRIAD_SELF_UPDATE_GUARD_ENV_FILE";
pub const ENV_STATUS_FILE: &str = "MYRIAD_SELF_UPDATE_STATUS_FILE";
pub const ENV_COMPOSE_NETWORK: &str = "MYRIAD_SELF_UPDATE_COMPOSE_NETWORK";
pub const ENV_ADMIN_NETWORK: &str = "MYRIAD_SELF_UPDATE_ADMIN_NETWORK";
pub const ENV_GUARD_NETWORK: &str = "MYRIAD_SELF_UPDATE_GUARD_NETWORK";
pub const ENV_RECOVERY_ONLY: &str = "MYRIAD_SELF_UPDATE_RECOVERY_ONLY";

const TRUSTED_UPDATER_REPOSITORY: &str = "docker.io/somekawahitomi/myriad-updater";
const SERVICES: [&str; 3] = ["docker-guard", "updater", "updater-gateway"];
const COMPOSE_CONFIG_TIMEOUT: Duration = Duration::from_secs(30);
const COMPOSE_UP_TIMEOUT: Duration = Duration::from_secs(120);
const SERVICE_HEALTH_TIMEOUT: Duration = Duration::from_secs(120);
const DOCKER_INSPECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelfUpdateOutcome {
    Pending,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfUpdateLastStatus {
    pub status: SelfUpdateOutcome,
    pub target_tag: String,
    pub previous_tag: String,
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub recovery_attempt: u8,
}

fn is_zero(value: &u8) -> bool {
    *value == 0
}

impl SelfUpdateLastStatus {
    fn pending(cfg: &HelperConfig, recovery_attempt: u8) -> Self {
        Self {
            status: SelfUpdateOutcome::Pending,
            target_tag: cfg.target_tag.clone(),
            previous_tag: cfg.previous_tag.clone(),
            at: Utc::now().to_rfc3339(),
            error: None,
            recovery_attempt,
        }
    }

    pub(super) fn succeeded_after_handoff(target_tag: String, previous_tag: String) -> Self {
        Self {
            status: SelfUpdateOutcome::Succeeded,
            target_tag,
            previous_tag,
            at: Utc::now().to_rfc3339(),
            error: None,
            recovery_attempt: 0,
        }
    }

    fn recovery_pending(cfg: &HelperConfig, error: String, recovery_attempt: u8) -> Self {
        Self {
            status: SelfUpdateOutcome::Pending,
            target_tag: cfg.target_tag.clone(),
            previous_tag: cfg.previous_tag.clone(),
            at: Utc::now().to_rfc3339(),
            error: Some(error),
            recovery_attempt,
        }
    }

    pub(super) fn failed_before_handoff(
        target_tag: String,
        previous_tag: String,
        error: String,
    ) -> Self {
        Self {
            status: SelfUpdateOutcome::Failed,
            target_tag,
            previous_tag,
            at: Utc::now().to_rfc3339(),
            error: Some(error),
            recovery_attempt: 0,
        }
    }

    pub(super) fn pending_before_handoff(target_tag: String, previous_tag: String) -> Self {
        Self {
            status: SelfUpdateOutcome::Pending,
            target_tag,
            previous_tag,
            at: Utc::now().to_rfc3339(),
            error: None,
            recovery_attempt: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct HelperConfig {
    previous_image: String,
    target_image: String,
    previous_tag: String,
    target_tag: String,
    project: String,
    project_directory: PathBuf,
    host_compose_root: String,
    compose_dir: PathBuf,
    app_env_file: PathBuf,
    guard_env_file: PathBuf,
    status_file: PathBuf,
    compose_network: String,
    admin_network: String,
    guard_network: String,
    recovery_only: bool,
}

impl HelperConfig {
    fn from_env() -> Result<Self> {
        let cfg = Self {
            previous_image: required_env(ENV_PREVIOUS_IMAGE)?,
            target_image: required_env(ENV_TARGET_IMAGE)?,
            previous_tag: required_env(ENV_PREVIOUS_TAG)?,
            target_tag: required_env(ENV_TARGET_TAG)?,
            project: required_env(ENV_PROJECT)?,
            project_directory: required_env(ENV_PROJECT_DIRECTORY)?.into(),
            host_compose_root: required_env(ENV_HOST_COMPOSE_ROOT)?,
            compose_dir: required_env(ENV_COMPOSE_DIR)?.into(),
            app_env_file: required_env(ENV_APP_ENV_FILE)?.into(),
            guard_env_file: required_env(ENV_GUARD_ENV_FILE)?.into(),
            status_file: required_env(ENV_STATUS_FILE)?.into(),
            compose_network: required_env(ENV_COMPOSE_NETWORK)?,
            admin_network: required_env(ENV_ADMIN_NETWORK)?,
            guard_network: required_env(ENV_GUARD_NETWORK)?,
            recovery_only: match std::env::var(ENV_RECOVERY_ONLY).as_deref() {
                Ok("1") => true,
                Ok("") | Err(std::env::VarError::NotPresent) => false,
                _ => {
                    return Err(UpdaterError::Precondition(
                        "invalid trusted self-update recovery mode".into(),
                    ));
                }
            },
        };
        validate_exact_image(&cfg.previous_image)?;
        validate_exact_image(&cfg.target_image)?;
        crate::version::DeployTag::parse(&cfg.previous_tag)?;
        crate::version::DeployTag::parse(&cfg.target_tag)?;
        for (name, value) in [
            (ENV_PROJECT, cfg.project.as_str()),
            (ENV_COMPOSE_NETWORK, cfg.compose_network.as_str()),
            (ENV_ADMIN_NETWORK, cfg.admin_network.as_str()),
            (ENV_GUARD_NETWORK, cfg.guard_network.as_str()),
        ] {
            validate_simple_name(name, value)?;
        }
        if cfg.guard_env_file != Path::new("/guard-policy/docker-guard.env") {
            return Err(UpdaterError::Precondition(
                "Guard policy file must be /guard-policy/docker-guard.env".into(),
            ));
        }
        Ok(cfg)
    }
}

pub fn main_from_env() -> Result<()> {
    let cfg = HelperConfig::from_env()?;
    // Let Guard finish the HTTP 202 response before Compose replaces it.
    std::thread::sleep(std::time::Duration::from_secs(2));
    ensure_status_writable(&cfg.status_file)?;
    let recovery_attempt = std::fs::read(&cfg.status_file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<SelfUpdateLastStatus>(&bytes).ok())
        .filter(|status| {
            status.target_tag == cfg.target_tag && status.previous_tag == cfg.previous_tag
        })
        .map(|status| status.recovery_attempt)
        .unwrap_or(0);
    write_status(
        &cfg.status_file,
        &SelfUpdateLastStatus::pending(&cfg, recovery_attempt),
    )?;

    if cfg.recovery_only {
        return match run_recovery(&cfg) {
            Ok(()) => Ok(()),
            Err(error) => {
                let detail = format!("trusted handoff recovery failed: {error}");
                if let Err(status_error) = write_status(
                    &cfg.status_file,
                    &SelfUpdateLastStatus::recovery_pending(&cfg, detail.clone(), recovery_attempt),
                ) {
                    return Err(UpdaterError::Precondition(format!(
                        "{detail}; persist recovery-pending outcome: {status_error}"
                    )));
                }
                Err(UpdaterError::Precondition(detail))
            }
        };
    }

    match run_handoff(&cfg) {
        Ok(()) => Ok(()),
        Err(error) => {
            let detail = error.to_string();
            // Terminal failure — not recovery_pending. The updater HTTP waiter
            // and admin UI only leave "confirming result" on succeeded/failed.
            // A pending error left them spinning after a pull-but-no-switch.
            if let Err(status_error) = write_status(
                &cfg.status_file,
                &SelfUpdateLastStatus::failed_before_handoff(
                    cfg.target_tag.clone(),
                    cfg.previous_tag.clone(),
                    detail.clone(),
                ),
            ) {
                return Err(UpdaterError::Precondition(format!(
                    "{detail}; persist failed outcome: {status_error}"
                )));
            }
            Err(error)
        }
    }
}

fn run_recovery(cfg: &HelperConfig) -> Result<()> {
    wait_for_compose_quiescence(Duration::from_secs(120))?;
    rollback_previous(cfg)
}

fn run_handoff(cfg: &HelperConfig) -> Result<()> {
    let app_before = std::fs::read(&cfg.app_env_file)?;
    let guard_before = std::fs::read(&cfg.guard_env_file)?;

    if let Err(error) = install(cfg, &cfg.target_image, &cfg.target_tag) {
        let restore_files = restore_files(cfg, &app_before, &guard_before);
        let quiescence = wait_for_compose_quiescence(Duration::from_secs(120));
        let rollback = match &quiescence {
            Ok(()) => rollback_previous(cfg),
            Err(error) => Err(UpdaterError::Precondition(format!(
                "target Docker operations did not quiesce before rollback: {error}"
            ))),
        };
        let detail = format!(
            "target switch failed: {error}; restore files: {}; quiescence: {}; rollback: {}",
            display_result(restore_files),
            display_result(quiescence),
            display_result(rollback)
        );
        return Err(UpdaterError::Precondition(detail));
    }
    Ok(())
}

fn wait_for_compose_quiescence(timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let mut previous = None;
    let mut stable_samples = 0u8;
    loop {
        let mut sample = Vec::new();
        let mut sample_complete = true;
        for container in [
            "myriad-docker-guard",
            "myriad-updater",
            "myriad-updater-gateway",
        ] {
            let mut inspect = Command::new("docker");
            inspect.args([
                "container",
                "inspect",
                "--format",
                "{{.Id}}|{{.State.Status}}|{{.Image}}",
                container,
            ]);
            match command_output_with_timeout(
                &mut inspect,
                DOCKER_INSPECT_TIMEOUT,
                "inspect TCB quiescence",
            ) {
                Ok(output) if output.status.success() => {
                    sample.extend_from_slice(container.as_bytes());
                    sample.push(b'=');
                    sample.extend_from_slice(&output.stdout);
                }
                Ok(output) if is_missing_container_error(&output.stderr) => {
                    sample.extend_from_slice(container.as_bytes());
                    sample.extend_from_slice(b"=<absent>\n");
                }
                Ok(_) | Err(_) => {
                    sample_complete = false;
                    break;
                }
            }
        }
        if sample_complete {
            if previous.as_deref() == Some(sample.as_slice()) {
                stable_samples += 1;
                if stable_samples >= 5 {
                    return Ok(());
                }
            } else {
                previous = Some(sample);
                stable_samples = 0;
            }
        } else {
            stable_samples = 0;
            previous = None;
        }
        if Instant::now() >= deadline {
            return Err(UpdaterError::Precondition(
                "TCB container identities did not stabilize".into(),
            ));
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn is_missing_container_error(stderr: &[u8]) -> bool {
    let error = String::from_utf8_lossy(stderr);
    error.contains("No such container") || error.contains("No such object")
}

fn ensure_status_writable(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| UpdaterError::Precondition("self-update status has no parent".into()))?;
    std::fs::create_dir_all(parent)?;
    let mut probe = tempfile::NamedTempFile::new_in(parent)?;
    probe.write_all(b"self-update-status-probe")?;
    probe.as_file().sync_all()?;
    Ok(())
}

fn install(cfg: &HelperConfig, exact_image: &str, tag: &str) -> Result<()> {
    update_policy_files(cfg, exact_image, tag)?;
    let files = find_compose_files(&cfg.compose_dir)?;
    let config = run_compose(
        cfg,
        &files,
        exact_image,
        tag,
        &["config", "--format", "json"],
    )?;
    validate_compose_model(&config, exact_image, cfg)?;
    run_compose(
        cfg,
        &files,
        exact_image,
        tag,
        &[
            "up",
            "-d",
            "--no-deps",
            "--force-recreate",
            SERVICES[0],
            SERVICES[1],
            SERVICES[2],
        ],
    )?;
    wait_for_running_services(exact_image, SERVICE_HEALTH_TIMEOUT)?;
    Ok(())
}

fn rollback_previous(cfg: &HelperConfig) -> Result<()> {
    for attempt in 1..=2 {
        match install(cfg, &cfg.previous_image, &cfg.previous_tag) {
            Ok(()) => return Ok(()),
            Err(error) if attempt == 2 => return Err(error),
            Err(_) => std::thread::sleep(Duration::from_secs(2)),
        }
    }
    Err(UpdaterError::Precondition(
        "rollback did not reach the previous TCB invariant".into(),
    ))
}

fn update_policy_files(cfg: &HelperConfig, exact_image: &str, tag: &str) -> Result<()> {
    let mut app = EnvFile::load(&cfg.app_env_file)?;
    app.set("UPDATER_TAG", tag)?;
    app.set("UPDATER_IMAGE_REF", exact_image)?;
    app.save()?;

    let mut guard = EnvFile::load(&cfg.guard_env_file)?;
    guard.set("DOCKER_GUARD_IMAGE", exact_image)?;
    guard.set("MYRIAD_GUARD_ENV_FILE", "guard-policy/docker-guard.env")?;
    guard.save()
}

fn restore_files(cfg: &HelperConfig, app: &[u8], guard: &[u8]) -> Result<()> {
    atomic::write_atomic_bytes(&cfg.app_env_file, app)?;
    atomic::write_atomic_bytes(&cfg.guard_env_file, guard)
}

pub(super) fn write_status(path: &Path, status: &SelfUpdateLastStatus) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    atomic::write_atomic_json(path, status)
}

fn run_compose(
    cfg: &HelperConfig,
    files: &[PathBuf],
    exact_image: &str,
    tag: &str,
    tail: &[&str],
) -> Result<Vec<u8>> {
    let mut command = Command::new("docker");
    command.args([
        "compose",
        "-p",
        &cfg.project,
        "--project-directory",
        cfg.project_directory
            .to_str()
            .ok_or_else(|| UpdaterError::InvalidInput("project path is not UTF-8".into()))?,
    ]);
    for file in files {
        command.arg("-f").arg(file);
    }
    command
        .arg("--env-file")
        .arg(&cfg.app_env_file)
        .arg("--env-file")
        .arg(&cfg.guard_env_file)
        .args(tail)
        // These values are authoritative and override every mutable .env key.
        .env("UPDATER_IMAGE_REF", exact_image)
        .env("UPDATER_TAG", tag)
        .env("DOCKER_GUARD_IMAGE", exact_image)
        .env("MYRIAD_GUARD_ENV_FILE", "guard-policy/docker-guard.env")
        .env("MYRIAD_COMPOSE_HOST_ROOT", &cfg.host_compose_root)
        .env("COMPOSE_PROJECT_NAME", &cfg.project)
        .env("MYRIAD_DOCKER_NETWORK", &cfg.compose_network)
        .env("MYRIAD_ADMIN_NETWORK", &cfg.admin_network)
        .env("MYRIAD_DOCKER_GUARD_NETWORK", &cfg.guard_network)
        .env("GUARD_COMPOSE_PROJECT_NAME", &cfg.project)
        .env("GUARD_MYRIAD_DOCKER_NETWORK", &cfg.compose_network)
        .env("GUARD_MYRIAD_ADMIN_NETWORK", &cfg.admin_network)
        .env("GUARD_MYRIAD_DOCKER_GUARD_NETWORK", &cfg.guard_network);

    let timeout = if tail.first() == Some(&"config") {
        COMPOSE_CONFIG_TIMEOUT
    } else {
        COMPOSE_UP_TIMEOUT
    };
    let output = command_output_with_timeout(&mut command, timeout, "fixed compose operation")?;
    if !output.status.success() {
        return Err(UpdaterError::Docker(format!(
            "fixed compose operation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(output.stdout)
}

fn command_output_with_timeout(
    command: &mut Command,
    timeout: Duration,
    operation: &str,
) -> Result<Output> {
    let mut stdout = tempfile::tempfile().map_err(|error| {
        UpdaterError::Io(std::io::Error::new(
            error.kind(),
            format!("create {operation} stdout: {error}"),
        ))
    })?;
    let mut stderr = tempfile::tempfile().map_err(|error| {
        UpdaterError::Io(std::io::Error::new(
            error.kind(),
            format!("create {operation} stderr: {error}"),
        ))
    })?;
    command
        .stdout(Stdio::from(stdout.try_clone()?))
        .stderr(Stdio::from(stderr.try_clone()?));
    // The helper runs in a Linux container. Put Docker CLI and any Compose
    // plugin descendants in their own process group so a timeout cannot leave
    // a target deployment racing the rollback path.
    unsafe {
        command.pre_exec(|| {
            nix::unistd::setsid()
                .map(|_| ())
                .map_err(std::io::Error::other)
        });
    }
    let mut child = command
        .spawn()
        .map_err(|error| UpdaterError::Docker(format!("spawn {operation}: {error}")))?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| UpdaterError::Docker(format!("wait for {operation}: {error}")))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            let process_group = nix::unistd::Pid::from_raw(child.id() as i32);
            let _ = nix::sys::signal::killpg(process_group, nix::sys::signal::Signal::SIGTERM);
            let grace_deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < grace_deadline {
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if child.try_wait().ok().flatten().is_none() {
                let _ = nix::sys::signal::killpg(process_group, nix::sys::signal::Signal::SIGKILL);
            }
            let _ = child.wait();
            return Err(UpdaterError::Docker(format!(
                "{operation} exceeded {} seconds",
                timeout.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    stdout.seek(SeekFrom::Start(0))?;
    stderr.seek(SeekFrom::Start(0))?;
    let mut stdout_bytes = Vec::new();
    let mut stderr_bytes = Vec::new();
    stdout.read_to_end(&mut stdout_bytes)?;
    stderr.read_to_end(&mut stderr_bytes)?;
    Ok(Output {
        status,
        stdout: stdout_bytes,
        stderr: stderr_bytes,
    })
}

fn validate_compose_model(bytes: &[u8], exact_image: &str, cfg: &HelperConfig) -> Result<()> {
    let model: Value = serde_json::from_slice(bytes).map_err(|error| {
        UpdaterError::Precondition(format!("compose config is not valid JSON: {error}"))
    })?;
    let services = model
        .get("services")
        .and_then(Value::as_object)
        .ok_or_else(|| UpdaterError::Precondition("compose config has no services".into()))?;
    for service in SERVICES {
        let value = services.get(service).ok_or_else(|| {
            UpdaterError::Precondition(format!("compose config is missing {service}"))
        })?;
        let actual_image = value.get("image").and_then(Value::as_str).unwrap_or("");
        if !digest_matches(actual_image, exact_image) {
            return Err(UpdaterError::Precondition(format!(
                "{service} image is not the Guard-verified digest"
            )));
        }
        if value.get("privileged").and_then(Value::as_bool) == Some(true) {
            return Err(UpdaterError::Precondition(format!(
                "{service} may not be privileged"
            )));
        }
        for forbidden in ["ports", "cap_add", "devices", "pid", "ipc"] {
            if value.get(forbidden).is_some_and(nonempty_json) {
                return Err(UpdaterError::Precondition(format!(
                    "{service} field {forbidden} is outside the fixed TCB contract"
                )));
            }
        }
        if service != "docker-guard" && value.get("command").is_some_and(nonempty_json) {
            return Err(UpdaterError::Precondition(format!(
                "{service} field command is outside the fixed TCB contract"
            )));
        }
        let security_opt = value
            .get("security_opt")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                UpdaterError::Precondition(format!("{service} must set no-new-privileges"))
            })?;
        if security_opt.len() != 1
            || !security_opt[0].as_str().is_some_and(|option| {
                matches!(option, "no-new-privileges:true" | "no-new-privileges")
            })
        {
            return Err(UpdaterError::Precondition(format!(
                "{service} security options are outside the fixed TCB contract"
            )));
        }
    }

    let guard = &services["docker-guard"];
    require_read_only(guard, true, "docker-guard")?;
    require_guard_bootstrap(guard)?;
    require_healthcheck(guard, "http://localhost:2375/_ping", "docker-guard")?;
    require_guard_mount_targets(guard)?;
    require_networks(guard, &[&cfg.guard_network], "docker-guard")?;
    let updater = &services["updater"];
    require_read_only(updater, false, "updater")?;
    if updater.get("entrypoint").is_some_and(nonempty_json) {
        return Err(UpdaterError::Precondition(
            "updater entrypoint override is forbidden".into(),
        ));
    }
    require_healthcheck(updater, "http://localhost:1101/healthz", "updater")?;
    require_updater_mount_targets(updater)?;
    require_networks(
        updater,
        &[&cfg.admin_network, &cfg.guard_network],
        "updater",
    )?;
    let gateway = &services["updater-gateway"];
    require_read_only(gateway, true, "updater-gateway")?;
    require_entrypoint(
        gateway,
        &[
            "/usr/bin/tini",
            "--",
            "/usr/local/bin/myriad-updater-gateway",
        ],
        "updater-gateway",
    )?;
    require_healthcheck(gateway, "http://localhost:1104/healthz", "updater-gateway")?;
    require_mount_targets(gateway, &[], "updater-gateway")?;
    require_networks(gateway, &[&cfg.admin_network], "updater-gateway")?;
    Ok(())
}

fn nonempty_json(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
        Value::Number(_) => true,
    }
}

fn require_read_only(service: &Value, expected: bool, name: &str) -> Result<()> {
    let actual = service
        .get("read_only")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if actual != expected {
        return Err(UpdaterError::Precondition(format!(
            "{name} read_only does not match the fixed TCB contract"
        )));
    }
    Ok(())
}

fn service_string_list<'a>(service: &'a Value, key: &str) -> Vec<&'a str> {
    service
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn require_guard_bootstrap(service: &Value) -> Result<()> {
    let entrypoint = service_string_list(service, "entrypoint");
    if entrypoint != ["/bin/sh", "-c"] {
        return Err(UpdaterError::Precondition(
            "docker-guard entrypoint is outside the fixed TCB contract".into(),
        ));
    }
    let script = match service.get("command") {
        Some(Value::Array(items)) if items.len() == 1 => items[0].as_str().unwrap_or(""),
        Some(Value::String(text)) => text.as_str(),
        _ => {
            return Err(UpdaterError::Precondition(
                "docker-guard command must be the policy bootstrap script".into(),
            ))
        }
    };
    if !script.contains("exec /usr/bin/tini -- /usr/local/bin/myriad-docker-guard")
        || !script.contains("/guard-policy/docker-guard.env")
        || !script.contains("umask 077")
        || script.contains("docker.sock")
        || script.contains("privileged")
        || script.contains("cat >")
        || script.contains("<<")
    {
        return Err(UpdaterError::Precondition(
            "docker-guard command is outside the fixed TCB contract".into(),
        ));
    }
    Ok(())
}

fn require_guard_mount_targets(service: &Value) -> Result<()> {
    require_mount_targets(
        service,
        &[
            "/var/run/docker.sock",
            "/host/compose",
            "/host/state",
            "/guard-policy",
        ],
        "docker-guard",
    )
}

fn require_entrypoint(service: &Value, expected: &[&str], name: &str) -> Result<()> {
    let actual = service
        .get("entrypoint")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    if actual != expected {
        return Err(UpdaterError::Precondition(format!(
            "{name} entrypoint is outside the fixed TCB contract"
        )));
    }
    Ok(())
}

fn require_healthcheck(service: &Value, url: &str, name: &str) -> Result<()> {
    let actual = service
        .pointer("/healthcheck/test")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    if actual != ["CMD", "curl", "-fsS", url] {
        return Err(UpdaterError::Precondition(format!(
            "{name} healthcheck is outside the fixed TCB contract"
        )));
    }
    Ok(())
}

fn persistent_mount_targets(service: &Value) -> Vec<&str> {
    service
        .get("volumes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|mount| mount.get("type").and_then(Value::as_str) != Some("tmpfs"))
        .filter_map(|mount| mount.get("target").and_then(Value::as_str))
        .collect()
}

fn require_mount_targets(service: &Value, expected: &[&str], name: &str) -> Result<()> {
    let mut actual = persistent_mount_targets(service);
    let mut expected = expected.to_vec();
    actual.sort_unstable();
    expected.sort_unstable();
    if actual != expected {
        return Err(UpdaterError::Precondition(format!(
            "{name} mount targets are outside the fixed TCB contract"
        )));
    }
    Ok(())
}

fn require_updater_mount_targets(service: &Value) -> Result<()> {
    let mut actual = persistent_mount_targets(service);
    actual.sort_unstable();
    let mut bundled = vec![
        "/host/compose",
        "/host/compose/.env",
        "/host/compose/pgdata",
        "/host/compose/state",
        "/run/secrets",
    ];
    bundled.sort_unstable();
    let mut external = bundled.clone();
    external.retain(|target| *target != "/host/compose/pgdata");
    if actual == bundled || actual == external {
        return Ok(());
    }
    Err(UpdaterError::Precondition(
        "updater mount targets are outside the fixed bundled/external contract".into(),
    ))
}

fn wait_for_running_services(exact_image: &str, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let error = match verify_running_services(exact_image) {
            Ok(()) => return Ok(()),
            Err(error) => error,
        };
        if Instant::now() >= deadline {
            return Err(error);
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn verify_running_services(exact_image: &str) -> Result<()> {
    for container in [
        "myriad-docker-guard",
        "myriad-updater",
        "myriad-updater-gateway",
    ] {
        let mut inspect = Command::new("docker");
        inspect.args(["container", "inspect", container]);
        let output = command_output_with_timeout(
            &mut inspect,
            DOCKER_INSPECT_TIMEOUT,
            &format!("inspect {container}"),
        )?;
        if !output.status.success() {
            return Err(UpdaterError::Docker(format!(
                "inspect recreated {container} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let inspected: Vec<Value> = serde_json::from_slice(&output.stdout).map_err(|error| {
            UpdaterError::Precondition(format!("invalid inspect for {container}: {error}"))
        })?;
        let inspected = inspected
            .first()
            .ok_or_else(|| UpdaterError::Precondition(format!("empty inspect for {container}")))?;
        if inspected.pointer("/State/Running").and_then(Value::as_bool) != Some(true)
            || inspected.pointer("/State/Status").and_then(Value::as_str) != Some("running")
            || inspected
                .pointer("/State/Health/Status")
                .and_then(Value::as_str)
                != Some("healthy")
        {
            return Err(UpdaterError::Precondition(format!(
                "{container} is not running and healthy"
            )));
        }
        let configured = inspected
            .pointer("/Config/Image")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let image_id = inspected
            .get("Image")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if configured != exact_image || !image_id.starts_with("sha256:") {
            return Err(UpdaterError::Precondition(format!(
                "{container} did not start from the Guard-verified digest"
            )));
        }
        let mut image_inspect = Command::new("docker");
        image_inspect.args([
            "image",
            "inspect",
            "--format",
            "{{json .RepoDigests}}",
            image_id,
        ]);
        let output = command_output_with_timeout(
            &mut image_inspect,
            DOCKER_INSPECT_TIMEOUT,
            &format!("inspect image for {container}"),
        )?;
        let digests: Vec<String> = serde_json::from_slice(&output.stdout).map_err(|error| {
            UpdaterError::Precondition(format!("invalid RepoDigests for {container}: {error}"))
        })?;
        if !digests
            .iter()
            .any(|actual| digest_matches(actual, exact_image))
        {
            return Err(UpdaterError::Precondition(format!(
                "{container} content digest does not match the verified target"
            )));
        }
    }
    Ok(())
}

fn digest_matches(actual: &str, expected: &str) -> bool {
    let Some((actual_repo, actual_digest)) = actual.rsplit_once("@sha256:") else {
        return false;
    };
    let Some((expected_repo, expected_digest)) = expected.rsplit_once("@sha256:") else {
        return false;
    };
    normalize_image_repository(actual_repo) == normalize_image_repository(expected_repo)
        && actual_digest.eq_ignore_ascii_case(expected_digest)
}

/// Compose / Docker inspect often drop `docker.io/` and may keep `name:tag@sha256`.
fn normalize_image_repository(repo: &str) -> String {
    let repo = repo
        .trim()
        .trim_start_matches("docker.io/")
        .trim_start_matches("index.docker.io/")
        .trim_start_matches("registry-1.docker.io/");
    if let Some((name, maybe_tag)) = repo.rsplit_once(':') {
        if !maybe_tag.contains('/') {
            return name.to_string();
        }
    }
    repo.to_string()
}

fn require_networks(service: &Value, expected: &[&str], name: &str) -> Result<()> {
    let mut actual: Vec<String> =
        if let Some(map) = service.get("networks").and_then(Value::as_object) {
            map.keys().cloned().collect()
        } else if let Some(list) = service.get("networks").and_then(Value::as_array) {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        } else {
            return Err(UpdaterError::Precondition(format!(
                "{name} has no network map"
            )));
        };
    let mut expected: Vec<String> = expected.iter().map(|item| (*item).to_string()).collect();
    actual.sort();
    expected.sort();
    if actual != expected {
        return Err(UpdaterError::Precondition(format!(
            "{name} network topology is outside the fixed allowlist"
        )));
    }
    Ok(())
}

fn find_compose_files(root: &Path) -> Result<Vec<PathBuf>> {
    let files = [
        "compose.yaml",
        "compose.yml",
        "docker-compose.yaml",
        "docker-compose.yml",
    ]
    .into_iter()
    .map(|name| root.join(name))
    .filter(|path| path.is_file())
    .collect::<Vec<_>>();
    if files.is_empty() {
        return Err(UpdaterError::Precondition(format!(
            "no Compose file found in {}",
            root.display()
        )));
    }
    Ok(files)
}

fn validate_exact_image(image: &str) -> Result<()> {
    let prefix = format!("{TRUSTED_UPDATER_REPOSITORY}@sha256:");
    let digest = image.strip_prefix(&prefix).ok_or_else(|| {
        UpdaterError::Precondition(format!("TCB image must use {prefix}<64 hex>"))
    })?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(UpdaterError::Precondition(
            "TCB image digest must contain exactly 64 hex characters".into(),
        ));
    }
    Ok(())
}

fn validate_simple_name(name: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(UpdaterError::InvalidInput(format!("invalid {name}")));
    }
    Ok(())
}

fn required_env(name: &str) -> Result<String> {
    std::env::var(name)
        .map_err(|_| UpdaterError::Precondition(format!("missing required helper env {name}")))
}

fn display_result(result: Result<()>) -> String {
    match result {
        Ok(()) => "ok".into(),
        Err(error) => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exact_image() -> String {
        format!("{TRUSTED_UPDATER_REPOSITORY}@sha256:{}", "a".repeat(64))
    }

    fn config() -> HelperConfig {
        HelperConfig {
            previous_image: exact_image(),
            target_image: exact_image(),
            previous_tag: "v1.2.2".into(),
            target_tag: "v1.2.3".into(),
            project: "myriad".into(),
            project_directory: "/host/compose".into(),
            host_compose_root: "/srv/myriad".into(),
            compose_dir: "/host/compose".into(),
            app_env_file: "/host/write/.env".into(),
            guard_env_file: "/guard-policy/docker-guard.env".into(),
            status_file: "/host/write/state/self-update-last.json".into(),
            compose_network: "myriad-net".into(),
            admin_network: "myriad-admin-net".into(),
            guard_network: "myriad-docker-guard-net".into(),
            recovery_only: false,
        }
    }

    fn service(
        image: &str,
        networks: &[&str],
        volumes: &[&str],
        entrypoint: Option<&[&str]>,
        read_only: bool,
        health_url: &str,
    ) -> Value {
        let mut value = serde_json::json!({
            "image": image,
            "networks": networks.iter().map(|name| ((*name).to_owned(), serde_json::json!(null))).collect::<serde_json::Map<_, _>>(),
            "volumes": volumes.iter().map(|target| serde_json::json!({"target": target})).collect::<Vec<_>>(),
            "read_only": read_only,
            "security_opt": ["no-new-privileges:true"],
            "healthcheck": {"test": ["CMD", "curl", "-fsS", health_url]}
        });
        if let Some(entrypoint) = entrypoint {
            value["entrypoint"] = serde_json::json!(entrypoint);
        }
        value
    }

    fn compose_model(image: &str) -> Value {
        let mut model = serde_json::json!({
            "services": {
                "docker-guard": service(
                    image,
                    &["myriad-docker-guard-net"],
                    &["/var/run/docker.sock", "/host/compose", "/host/state", "/guard-policy"],
                    Some(&["/bin/sh", "-c"]),
                    true,
                    "http://localhost:2375/_ping",
                ),
                "updater": service(
                    image,
                    &["myriad-admin-net", "myriad-docker-guard-net"],
                    &[
                        "/host/compose",
                        "/host/compose/.env",
                        "/host/compose/pgdata",
                        "/host/compose/state",
                        "/run/secrets",
                    ],
                    None,
                    false,
                    "http://localhost:1101/healthz",
                ),
                "updater-gateway": service(
                    image,
                    &["myriad-admin-net"],
                    &[],
                    Some(&["/usr/bin/tini", "--", "/usr/local/bin/myriad-updater-gateway"]),
                    true,
                    "http://localhost:1104/healthz",
                )
            }
        });
        model["services"]["docker-guard"]["command"] = serde_json::json!([
            "set -eu\nif [ ! -f /guard-policy/docker-guard.env ]; then umask 077; fi\nexec /usr/bin/tini -- /usr/local/bin/myriad-docker-guard\n"
        ]);
        model
    }

    #[test]
    fn exact_image_rejects_tags_and_foreign_repositories() {
        assert!(validate_exact_image(
            "docker.io/somekawahitomi/myriad-updater@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        )
        .is_ok());
        assert!(validate_exact_image("docker.io/somekawahitomi/myriad-updater:v1.2.3").is_err());
        assert!(validate_exact_image(
            "docker.io/attacker/myriad-updater@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        )
        .is_err());
    }

    #[test]
    fn fixed_compose_model_accepts_only_guard_owned_image_and_services() {
        let image = exact_image();
        let model = compose_model(&image);
        let bytes = serde_json::to_vec(&model).unwrap();
        assert!(validate_compose_model(&bytes, &image, &config()).is_ok());

        let mut injected = model;
        injected["services"]["updater"]["command"] = serde_json::json!(["sh", "-c", "id"]);
        let bytes = serde_json::to_vec(&injected).unwrap();
        assert!(validate_compose_model(&bytes, &image, &config()).is_err());
    }

    #[test]
    fn fixed_compose_model_allows_external_db_without_pgdata_mount() {
        let image = exact_image();
        let mut model = compose_model(&image);
        model["services"]["updater"]["volumes"]
            .as_array_mut()
            .unwrap()
            .retain(|mount| mount["target"] != "/host/compose/pgdata");
        let bytes = serde_json::to_vec(&model).unwrap();
        assert!(validate_compose_model(&bytes, &image, &config()).is_ok());
    }

    #[test]
    fn digest_comparison_allows_only_docker_hub_canonicalization() {
        let expected = exact_image();
        let short = expected.trim_start_matches("docker.io/");
        assert!(digest_matches(short, &expected));
        let tagged = format!(
            "somekawahitomi/myriad-updater:v1.2.3@sha256:{}",
            "a".repeat(64)
        );
        assert!(digest_matches(&tagged, &expected));
        assert!(!digest_matches(
            &expected.replace("somekawahitomi", "attacker"),
            &expected
        ));
    }

    #[test]
    fn compose_model_accepts_canonicalized_digest_and_tmpfs_mounts() {
        let image = exact_image();
        let mut model = compose_model(&image);
        let short = image.trim_start_matches("docker.io/");
        for service in ["docker-guard", "updater", "updater-gateway"] {
            model["services"][service]["image"] = serde_json::json!(short);
        }
        model["services"]["docker-guard"]["volumes"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"type": "tmpfs", "target": "/tmp"}));
        model["services"]["updater-gateway"]["volumes"] = serde_json::json!([
            {"type": "tmpfs", "target": "/tmp"},
            {"type": "tmpfs", "target": "/run"}
        ]);
        model["services"]["updater"]["networks"] =
            serde_json::json!(["myriad-admin-net", "myriad-docker-guard-net"]);
        let bytes = serde_json::to_vec(&model).unwrap();
        assert!(validate_compose_model(&bytes, &image, &config()).is_ok());
    }

    #[test]
    fn guard_bootstrap_rejects_quoted_policy_stub() {
        let stub = serde_json::json!({
            "entrypoint": ["/bin/sh", "-c"],
            "command": [
                "set -eu\nif [ ! -f /guard-policy/docker-guard.env ]; then\n  umask 077\n  cat > /guard-policy/docker-guard.env <<'POLICY'\nDOCKER_GUARD_IMAGE=bad\nPOLICY\nfi\nexec /usr/bin/tini -- /usr/local/bin/myriad-docker-guard\n"
            ]
        });
        assert!(require_guard_bootstrap(&stub).is_err());

        let umask_only = serde_json::json!({
            "entrypoint": ["/bin/sh", "-c"],
            "command": [
                "set -eu\nif [ ! -f /guard-policy/docker-guard.env ]; then umask 077; fi\nexec /usr/bin/tini -- /usr/local/bin/myriad-docker-guard\n"
            ]
        });
        assert!(require_guard_bootstrap(&umask_only).is_ok());
    }

    #[test]
    fn official_compose_guard_command_does_not_write_policy_stub() {
        let compose = include_str!("../../../docker-compose.yml");
        let example = include_str!(
            "../../../docs/deployment/examples/docker-compose.external-db.example.yml"
        );
        for text in [compose, example] {
            assert!(!text.contains("<<'POLICY'"));
            assert!(!text.contains("cat > /guard-policy/docker-guard.env"));
            assert!(text.contains("if [ ! -f /guard-policy/docker-guard.env ]; then umask 077; fi"));
            assert!(text.contains(
                "MYRIAD_SETUP_SECRET: ${MYRIAD_SETUP_SECRET:?Set MYRIAD_SETUP_SECRET in .env}"
            ));
        }
    }

    #[test]
    fn missing_container_is_a_stable_quiescence_state() {
        assert!(is_missing_container_error(
            b"Error: No such container: myriad-updater"
        ));
        assert!(is_missing_container_error(
            b"Error response from daemon: No such object: myriad-updater"
        ));
        assert!(!is_missing_container_error(
            b"Cannot connect to the Docker daemon"
        ));
    }
}
