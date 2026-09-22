//! Trusted one-shot handoff for the updater TCB.
//!
//! The running Guard starts this binary only from an image whose official
//! repository and immutable digest it has independently verified. The helper
//! has one operation: converge `docker-guard`, `updater`, and
//! `updater-gateway` on that exact image, or restore each service's previous
//! exact image from the Guard-verified recovery snapshot.

use std::io::{Read, Seek, SeekFrom};
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
// Fixed order: Guard, updater, gateway. Stored in the trusted helper's immutable
// container configuration so recovery survives Guard restarts.
pub const ENV_PREVIOUS_IMAGES: &str = "MYRIAD_SELF_UPDATE_PREVIOUS_IMAGES";
// v0.5.3 Guard checks this image label; remove after the refactor's first release.
pub const MIXED_RECOVERY_LABEL: &str = "io.myriad.updater.mixed-recovery";
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
pub const ENV_RECONCILE_ONLY: &str = "MYRIAD_SELF_UPDATE_RECONCILE_ONLY";
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
    /// Accepted by the worker, not yet handed to Guard. Restart resumes discovery.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub queued: bool,
    pub target_tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_image: Option<String>,
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
            queued: false,
            status: SelfUpdateOutcome::Pending,
            target_tag: cfg.target_tag.clone(),
            target_image: (!cfg.recovery_only).then(|| cfg.target_image.clone()),
            previous_tag: cfg.previous_tag.clone(),
            at: Utc::now().to_rfc3339(),
            error: None,
            recovery_attempt,
        }
    }

    pub(crate) fn succeeded_after_handoff(target_tag: String, previous_tag: String) -> Self {
        Self {
            queued: false,
            status: SelfUpdateOutcome::Succeeded,
            target_tag,
            target_image: None,
            previous_tag,
            at: Utc::now().to_rfc3339(),
            error: None,
            recovery_attempt: 0,
        }
    }

    fn recovery_pending(cfg: &HelperConfig, error: String, recovery_attempt: u8) -> Self {
        Self {
            queued: false,
            status: SelfUpdateOutcome::Pending,
            target_tag: cfg.target_tag.clone(),
            target_image: (!cfg.recovery_only).then(|| cfg.target_image.clone()),
            previous_tag: cfg.previous_tag.clone(),
            at: Utc::now().to_rfc3339(),
            error: Some(error),
            recovery_attempt,
        }
    }

    pub(crate) fn failed_before_handoff(
        target_tag: String,
        previous_tag: String,
        error: String,
    ) -> Self {
        Self {
            queued: false,
            status: SelfUpdateOutcome::Failed,
            target_tag,
            target_image: None,
            previous_tag,
            at: Utc::now().to_rfc3339(),
            error: Some(error),
            recovery_attempt: 0,
        }
    }

    pub(crate) fn pending_before_handoff(target_tag: String, previous_tag: String) -> Self {
        Self {
            queued: false,
            status: SelfUpdateOutcome::Pending,
            target_tag,
            target_image: None,
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
    previous_images: Option<[String; 3]>,
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
            previous_images: std::env::var(ENV_PREVIOUS_IMAGES)
                .ok()
                .map(|value| parse_previous_images(&value))
                .transpose()?,
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
        if cfg
            .previous_images
            .as_ref()
            .is_some_and(|images| images[0] != cfg.previous_image)
        {
            return Err(UpdaterError::Precondition(
                "previous Guard image differs from recovery snapshot".into(),
            ));
        }
        validate_exact_image(&cfg.target_image)?;
        crate::version::validate_image_tag(&cfg.previous_tag)
            .map_err(UpdaterError::InvalidInput)?;
        crate::version::validate_image_tag(&cfg.target_tag).map_err(UpdaterError::InvalidInput)?;
        for (name, value) in [
            (ENV_PROJECT, cfg.project.as_str()),
            (ENV_COMPOSE_NETWORK, cfg.compose_network.as_str()),
            (ENV_ADMIN_NETWORK, cfg.admin_network.as_str()),
            (ENV_GUARD_NETWORK, cfg.guard_network.as_str()),
        ] {
            validate_simple_name(name, value)?;
        }
        Ok(cfg)
    }
}

pub(crate) fn parse_previous_images(value: &str) -> Result<[String; 3]> {
    let images: [String; 3] = serde_json::from_str(value)
        .map_err(|_| UpdaterError::Precondition("invalid previous TCB images".into()))?;
    for image in &images {
        validate_exact_image(image)?;
    }
    Ok(images)
}

pub fn main_from_env() -> Result<()> {
    let cfg = HelperConfig::from_env()?;
    match std::env::var(ENV_RECONCILE_ONLY).as_deref() {
        Ok("1") => return reconcile_running_policy(&cfg),
        Ok("") | Err(std::env::VarError::NotPresent) => (),
        _ => {
            return Err(UpdaterError::Precondition(
                "invalid reconciliation mode".into(),
            ));
        }
    }
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
            // Guard owns the outcome, including recovery after this process exits.
            write_status(
                &cfg.status_file,
                &SelfUpdateLastStatus::recovery_pending(&cfg, detail, recovery_attempt),
            )?;
            Err(error)
        }
    }
}

fn run_recovery(cfg: &HelperConfig) -> Result<()> {
    let images = cfg
        .previous_images
        .clone()
        .unwrap_or_else(|| std::array::from_fn(|_| cfg.previous_image.clone()));
    install_images(cfg, &images, &cfg.previous_tag)
}

fn run_handoff(cfg: &HelperConfig) -> Result<()> {
    let images = std::array::from_fn(|_| cfg.target_image.clone());
    install_images(cfg, &images, &cfg.target_tag)
}

fn install_images(cfg: &HelperConfig, images: &[String; 3], tag: &str) -> Result<()> {
    let files = prepare_install(cfg, images, tag)?;
    crate::deployment::manage_compose_files(&cfg.compose_dir, cfg.app_env_file.parent().unwrap())?;
    install_prepared(cfg, &files, images, tag)
}

fn prepare_install(cfg: &HelperConfig, images: &[String; 3], tag: &str) -> Result<Vec<PathBuf>> {
    let files = find_compose_files(&cfg.compose_dir)?;
    let config = run_compose(cfg, &files, images, tag, &["config", "--format", "json"])?;
    validate_stack_compose_model(&config, images, cfg)?;
    Ok(files)
}

fn install_prepared(
    cfg: &HelperConfig,
    files: &[PathBuf],
    images: &[String; 3],
    tag: &str,
) -> Result<()> {
    write_stack_policy_files(cfg, images, Some(tag), false)?;
    run_compose(
        cfg,
        files,
        images,
        tag,
        &[
            "up",
            "-d",
            "--no-deps",
            "--force-recreate",
            "--pull",
            "never",
            SERVICES[0],
            SERVICES[1],
            SERVICES[2],
        ],
    )?;
    wait_for_running_services(images, SERVICE_HEALTH_TIMEOUT)?;
    Ok(())
}

/// A host already replaced the stack. Verify again in the only process with
/// a writable deployment mount, then update metadata without recreating anything.
fn reconcile_running_policy(cfg: &HelperConfig) -> Result<()> {
    use crate::docker::guard::startup::{runtime_identity, validate_image_id};
    if cfg.recovery_only
        || cfg.previous_image != cfg.target_image
        || cfg.previous_tag != cfg.target_tag
    {
        return Err(UpdaterError::Precondition(
            "reconciliation cannot perform an upgrade or recovery".into(),
        ));
    }
    // Serialize retries after Guard restarts, including partial two-file writes.
    let lock_path = cfg
        .app_env_file
        .parent()
        .unwrap()
        .join("state/startup-reconcile.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)?;
    fs2::FileExt::try_lock_exclusive(&lock)?;
    let mut identities = Vec::new();
    for service in SERVICES {
        let container = docker_inspect_json("container", &format!("myriad-{service}"))?;
        let id = container
            .get("Image")
            .and_then(Value::as_str)
            .unwrap_or_default();
        validate_image_id(id).map_err(|e| UpdaterError::Precondition(e.to_string()))?;
        let image = docker_inspect_json("image", id)?;
        identities.push(
            runtime_identity(&container, &image, &cfg.project, service, false)
                .map_err(|e| UpdaterError::Precondition(e.to_string()))?,
        );
    }
    persist_reconciled_policy(cfg, &identities)?;
    crate::deployment::manage_compose_files(&cfg.compose_dir, cfg.app_env_file.parent().unwrap())
}

fn docker_inspect_json(kind: &str, name: &str) -> Result<Value> {
    let mut command = Command::new("docker");
    command.args([kind, "inspect", name]);
    let output = command_output_with_timeout(
        &mut command,
        DOCKER_INSPECT_TIMEOUT,
        "inspect startup runtime",
    )?;
    if !output.status.success() {
        return Err(UpdaterError::Precondition(
            "cannot inspect startup runtime".into(),
        ));
    }
    let values: Vec<Value> = serde_json::from_slice(&output.stdout)
        .map_err(|e| UpdaterError::Precondition(format!("invalid runtime inspection: {e}")))?;
    values
        .into_iter()
        .next()
        .ok_or_else(|| UpdaterError::Precondition("empty runtime inspection".into()))
}

fn persist_reconciled_policy(
    cfg: &HelperConfig,
    identities: &[crate::docker::guard::startup::RuntimeIdentity],
) -> Result<()> {
    let Some(actual) = identities.first() else {
        return Err(UpdaterError::Precondition(
            "missing running stack identity".into(),
        ));
    };
    let expected = cfg
        .previous_images
        .clone()
        .unwrap_or_else(|| std::array::from_fn(|_| cfg.target_image.clone()));
    if identities.len() != SERVICES.len()
        || identities
            .iter()
            .zip(&expected)
            .any(|(identity, image)| &identity.image != image)
        || identities.iter().any(|left| {
            identities
                .iter()
                .any(|right| left.image == right.image && left != right)
        })
        || actual.image != cfg.target_image
        || actual.version != cfg.target_tag
    {
        return Err(UpdaterError::Precondition(
            "running stack changed or has inconsistent identities".into(),
        ));
    }
    // Parse both before writing either; retries converge if a later write fails.
    EnvFile::load(&cfg.app_env_file)?;
    EnvFile::load(&cfg.guard_env_file)?;
    // UPDATER_TAG is the operator's deployment intent. Recording the observed
    // image must not erase a pending manual TAG change (nor claim it was applied).
    write_stack_policy_files(cfg, &expected, None, true)
}

#[cfg(test)]
fn update_policy_files(cfg: &HelperConfig, exact_image: &str, tag: &str) -> Result<()> {
    write_stack_policy_files(
        cfg,
        &std::array::from_fn(|_| exact_image.to_owned()),
        Some(tag),
        false,
    )
}

fn write_stack_policy_files(
    cfg: &HelperConfig,
    images: &[String; 3],
    deployment_tag: Option<&str>,
    preserve_inode: bool,
) -> Result<()> {
    let mut app = EnvFile::load(&cfg.app_env_file)?;
    if let Some(tag) = deployment_tag {
        app.set("UPDATER_TAG", tag)?;
    }
    app.set("UPDATER_IMAGE_REF", &images[1])?;
    if images[2] != images[1] || app.get("UPDATER_GATEWAY_IMAGE_REF").is_some() {
        app.set(
            "UPDATER_GATEWAY_IMAGE_REF",
            if images[2] == images[1] {
                ""
            } else {
                &images[2]
            },
        )?;
    }
    app.set("DOCKER_GUARD_IMAGE", &images[0])?;

    let mut guard = EnvFile::load(&cfg.guard_env_file)?;
    guard.set("DOCKER_GUARD_IMAGE", &images[0])?;
    guard.set("MYRIAD_GUARD_ENV_FILE", "guard-policy/docker-guard.env")?;
    if preserve_inode {
        app.save_preserving_inode()?;
    } else {
        app.save()?;
    }
    guard.save()
}

pub(crate) fn read_status(state_root: &Path) -> Result<Option<SelfUpdateLastStatus>> {
    crate::state::read_json(&state_root.join("self-update-last.json"))
}

/// Admission and recovery read. Unlike [`read_status`], a corrupt record is not
/// reported as "no request": that would let a new mutation start while Guard is
/// still running the previous handoff.
pub(crate) fn read_status_outcome(
    state_root: &Path,
) -> Result<crate::state::Outcome<SelfUpdateLastStatus>> {
    crate::state::read_outcome(&state_root.join("self-update-last.json"))
}

pub(crate) fn write_status(path: &Path, status: &SelfUpdateLastStatus) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    atomic::write_atomic_json(path, status)
}

fn run_compose(
    cfg: &HelperConfig,
    files: &[PathBuf],
    images: &[String; 3],
    tag: &str,
    tail: &[&str],
) -> Result<Vec<u8>> {
    // Pins in .env are observations, not deployment selectors. Only this
    // authenticated, fixed-service handoff may select per-service exact images,
    // and it does so through process-only env (MYRIAD_TCB_*_IMAGE). Never pass a
    // `-f` override: Compose records every `-f` in the container label
    // com.docker.compose.project.config_files, so a persisted selector would pin
    // images on later host-side rebuilds (1Panel) instead of honoring UPDATER_TAG.
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
        // Select the Guard-verified exact digests for this transaction via
        // process-only env; the base compose falls back to UPDATER_TAG when they
        // are unset, so a later host `compose up` selects by TAG again.
        .env("MYRIAD_TCB_GUARD_IMAGE", &images[0])
        .env("MYRIAD_TCB_UPDATER_IMAGE", &images[1])
        .env("MYRIAD_TCB_GATEWAY_IMAGE", &images[2])
        .env("UPDATER_IMAGE_REF", &images[1])
        .env("UPDATER_GATEWAY_IMAGE_REF", &images[2])
        .env("UPDATER_TAG", tag)
        .env("DOCKER_GUARD_IMAGE", &images[0])
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

#[cfg(test)]
fn validate_compose_model(bytes: &[u8], exact_image: &str, cfg: &HelperConfig) -> Result<()> {
    validate_stack_compose_model(bytes, &std::array::from_fn(|_| exact_image.to_owned()), cfg)
}

fn validate_stack_compose_model(
    bytes: &[u8],
    images: &[String; 3],
    _cfg: &HelperConfig,
) -> Result<()> {
    let model: Value = serde_json::from_slice(bytes).map_err(|error| {
        UpdaterError::Precondition(format!("compose config is not valid JSON: {error}"))
    })?;
    let services = model
        .get("services")
        .and_then(Value::as_object)
        .ok_or_else(|| UpdaterError::Precondition("compose config has no services".into()))?;
    for (service, exact_image) in SERVICES.into_iter().zip(images) {
        let value = services.get(service).ok_or_else(|| {
            UpdaterError::Precondition(format!("compose config is missing {service}"))
        })?;
        let actual_image = value.get("image").and_then(Value::as_str).unwrap_or("");
        if !same_local_image(actual_image, exact_image)? {
            return Err(UpdaterError::Precondition(format!(
                "{service} image does not resolve to the selected image"
            )));
        }
    }
    Ok(())
}

fn wait_for_running_services(images: &[String; 3], timeout: Duration) -> Result<()> {
    let selected = images
        .iter()
        .map(|image| docker_inspect_json("image", image))
        .collect::<Result<Vec<_>>>()?;
    let deadline = Instant::now() + timeout;
    loop {
        let error = match verify_running_services(&selected) {
            Ok(()) => return Ok(()),
            Err(error) => error,
        };
        if Instant::now() >= deadline {
            return Err(error);
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn verify_running_services(images: &[Value]) -> Result<()> {
    for (container, exact_image) in [
        "myriad-docker-guard",
        "myriad-updater",
        "myriad-updater-gateway",
    ]
    .into_iter()
    .zip(images)
    {
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
            || inspected
                .pointer("/State/Health/Status")
                .and_then(Value::as_str)
                != Some("healthy")
        {
            return Err(UpdaterError::Precondition(format!(
                "{container} is not running and healthy"
            )));
        }
        if !running_selected_image(inspected, exact_image) {
            return Err(UpdaterError::Precondition(format!(
                "{container} is not running the selected image"
            )));
        }
    }
    Ok(())
}

fn running_selected_image(container: &Value, image: &Value) -> bool {
    match (
        container.get("Image").and_then(Value::as_str),
        image.get("Id").and_then(Value::as_str),
    ) {
        (Some(actual), Some(expected)) => !actual.is_empty() && actual == expected,
        _ => false,
    }
}

/// A version tag and a digest reference may identify the same local image.
/// Compare Docker's content identity, not the spelling chosen by an old Compose file.
fn same_local_image(actual: &str, expected: &str) -> Result<bool> {
    if digest_matches(actual, expected) {
        return Ok(true);
    }
    let actual = docker_inspect_json("image", actual)?;
    let expected = docker_inspect_json("image", expected)?;
    Ok(
        matches!((actual.get("Id").and_then(Value::as_str), expected.get("Id").and_then(Value::as_str)),
        (Some(a), Some(b)) if !a.is_empty() && a == b),
    )
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
    if let Some((name, maybe_tag)) = repo.rsplit_once(':')
        && !maybe_tag.contains('/')
    {
        return name.to_string();
    }
    repo.to_string()
}

fn find_compose_files(root: &Path) -> Result<Vec<PathBuf>> {
    let files =
        crate::probe::compose::collect_compose_files(root).map_err(UpdaterError::Precondition)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn exact_image() -> String {
        format!("{TRUSTED_UPDATER_REPOSITORY}@sha256:{}", "a".repeat(64))
    }

    #[test]
    #[ignore = "requires Docker Compose CLI; no daemon or network needed"]
    fn v053_published_compose_accepts_upgrade_and_rollback_without_host_edits() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = config();
        cfg.compose_dir = dir.path().to_owned();
        cfg.project_directory = dir.path().to_owned();
        cfg.host_compose_root = dir.path().to_string_lossy().into_owned();
        cfg.app_env_file = dir.path().join(".env");
        cfg.guard_env_file = dir.path().join("guard.env");
        let compose = include_str!("../../testdata/v0.5.3/docker-compose.yml");
        let file = dir.path().join("docker-compose.yml");
        std::fs::write(&file, compose).unwrap();
        let env = "MYRIAD_TAG=v0.5.3\nUPDATER_TAG=v0.5.3\nPROXY_TAG=v0.5.3\nMYRIAD_SETUP_SECRET=test\nGUARD_SELF_UPDATE_TOKEN=test\nPERSONA_DB_PASSWORD=test\nFEDERATION_DB_PASSWORD=test\nPOSTGRES_PASSWORD=test\nUPDATE_TOKEN=test\nREGISTRY_MIRROR=\n";
        std::fs::write(&cfg.app_env_file, env).unwrap();
        std::fs::write(&cfg.guard_env_file, "").unwrap();
        for tag in ["v0.5.4", "v0.5.3"] {
            let images = std::array::from_fn(|_| exact_image());
            let bytes = run_compose(
                &cfg,
                &find_compose_files(dir.path()).unwrap(),
                &images,
                tag,
                &["config", "--format", "json"],
            )
            .unwrap();
            validate_stack_compose_model(&bytes, &images, &cfg).unwrap();
        }
        let pinned = format!("preview@sha256:{}", "d".repeat(64));
        let output = Command::new("docker")
            .args(["compose", "-f"])
            .arg(&file)
            .arg("--env-file")
            .arg(&cfg.app_env_file)
            .args(["config", "--format", "json"])
            .env("PROXY_TAG", &pinned)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let model: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(
            model["services"]["proxy"]["image"]
                .as_str()
                .unwrap()
                .ends_with(&pinned)
        );
        assert_eq!(std::fs::read_to_string(file).unwrap(), compose);
        assert_eq!(std::fs::read_to_string(&cfg.app_env_file).unwrap(), env);
    }

    #[test]
    fn running_identity_is_content_not_reference_spelling() {
        let selected = serde_json::json!({"Id":"sha256:selected"});
        let mut container = serde_json::json!({"Image":"sha256:selected", "Config":{"Image":"official/updater:v0.5.2"}});
        assert!(running_selected_image(&container, &selected));
        container["Image"] = serde_json::json!("sha256:old");
        assert!(!running_selected_image(&container, &selected));
        assert!(!running_selected_image(
            &serde_json::json!({}),
            &serde_json::json!({})
        ));
    }

    #[test]
    #[ignore = "requires Docker daemon and local debian:trixie-slim; read-only"]
    fn legacy_compose_tag_is_accepted_when_it_resolves_to_selected_content() {
        let reference = "debian:trixie-slim";
        let image = docker_inspect_json("image", reference).unwrap();
        let id = image["Id"].as_str().unwrap().to_string();
        let model = compose_model(reference);
        validate_stack_compose_model(
            &serde_json::to_vec(&model).unwrap(),
            &std::array::from_fn(|_| id.clone()),
            &config(),
        )
        .unwrap();
        assert!(!same_local_image(reference, "rust:1.98.1-slim-trixie").unwrap());
    }

    fn config() -> HelperConfig {
        HelperConfig {
            previous_images: None,
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
    fn mixed_stack_reconciliation_preserves_actual_images_and_pending_deployment_tag() {
        use crate::docker::guard::startup::RuntimeIdentity;
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = config();
        cfg.app_env_file = dir.path().join(".env");
        cfg.guard_env_file = dir.path().join("guard.env");
        std::fs::write(&cfg.app_env_file, "MYRIAD_TAG=v0.4.14\nPROXY_TAG=v0.3.32\nUPDATER_TAG=v9.9.9\nUPDATER_IMAGE_REF=stale\nDOCKER_GUARD_IMAGE=stale\n").unwrap();
        std::fs::write(&cfg.guard_env_file, "DOCKER_GUARD_IMAGE=stale\n").unwrap();
        let images: [String; 3] = std::array::from_fn(|i| {
            format!(
                "{TRUSTED_UPDATER_REPOSITORY}@sha256:{}",
                ["a", "b", "c"][i].repeat(64)
            )
        });
        cfg.previous_images = Some(images.clone());
        let identities: Vec<_> = images
            .iter()
            .enumerate()
            .map(|(i, image)| RuntimeIdentity {
                image: image.clone(),
                image_id: format!("sha256:{}", ["d", "e", "f"][i].repeat(64)),
                version: ["v1.2.3", "v0.4.6", "v0.4.8"][i].into(),
            })
            .collect();
        let mut live = std::fs::File::open(&cfg.app_env_file).unwrap();
        persist_reconciled_policy(&cfg, &identities).unwrap();
        let mut text = String::new();
        live.read_to_string(&mut text).unwrap();
        assert!(text.contains("UPDATER_TAG=v9.9.9"));
        let app = EnvFile::load(&cfg.app_env_file).unwrap();
        assert_eq!(app.get("MYRIAD_TAG"), Some("v0.4.14"));
        assert_eq!(app.get("PROXY_TAG"), Some("v0.3.32"));
        for (key, image) in [
            "DOCKER_GUARD_IMAGE",
            "UPDATER_IMAGE_REF",
            "UPDATER_GATEWAY_IMAGE_REF",
        ]
        .into_iter()
        .zip(&images)
        {
            assert_eq!(app.get(key), Some(image.as_str()));
        }
        assert_eq!(
            EnvFile::load(&cfg.guard_env_file)
                .unwrap()
                .get("DOCKER_GUARD_IMAGE"),
            Some(images[0].as_str())
        );

        let before = std::fs::read(&cfg.app_env_file).unwrap();
        let mut changed = identities.clone();
        changed[1].image = exact_image();
        assert!(persist_reconciled_policy(&cfg, &changed).is_err());
        assert_eq!(std::fs::read(&cfg.app_env_file).unwrap(), before);
    }

    #[test]
    #[ignore = "requires Docker Compose CLI; no Docker daemon or network needed"]
    fn compose_handoff_selects_exact_images_without_shadowing_manual_tag() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = config();
        cfg.compose_dir = dir.path().to_owned();
        cfg.project_directory = dir.path().to_owned();
        cfg.host_compose_root = dir.path().to_string_lossy().into_owned();
        cfg.app_env_file = dir.path().join(".env");
        cfg.guard_env_file = dir.path().join("guard.env");
        std::fs::create_dir_all(dir.path().join("state")).unwrap();
        cfg.status_file = dir.path().join("state").join("self-update-last.json");
        std::fs::write(&cfg.app_env_file, "UPDATER_TAG=v0.4.14\nUPDATER_IMAGE_REF=stale-updater\nUPDATER_GATEWAY_IMAGE_REF=stale-gateway\nDOCKER_GUARD_IMAGE=stale-guard\n").unwrap();
        std::fs::write(&cfg.guard_env_file, "DOCKER_GUARD_IMAGE=stale-host-pin\n").unwrap();
        let compose = dir.path().join("compose.json");
        let base = serde_json::json!({"services": {
            "docker-guard": {
                "image": "${MYRIAD_TCB_GUARD_IMAGE:-docker.io/somekawahitomi/myriad-updater:${UPDATER_TAG}}",
                "environment": {"DOCKER_GUARD_EXPECTED_IMAGE": "${MYRIAD_TCB_GUARD_IMAGE:-docker.io/somekawahitomi/myriad-updater:${UPDATER_TAG}}"}
            },
            "updater": {"image": "${MYRIAD_TCB_UPDATER_IMAGE:-docker.io/somekawahitomi/myriad-updater:${UPDATER_TAG}}"},
            "updater-gateway": {"image": "${MYRIAD_TCB_GATEWAY_IMAGE:-docker.io/somekawahitomi/myriad-updater:${UPDATER_TAG}}"},
            "proxy": {"image": "docker.io/somekawahitomi/myriad-proxy:v0.3.32"}
        }});
        std::fs::write(&compose, serde_json::to_vec(&base).unwrap()).unwrap();
        let files = vec![compose.clone()];
        let previous: [String; 3] = std::array::from_fn(|i| {
            format!(
                "{TRUSTED_UPDATER_REPOSITORY}@sha256:{}",
                ["a", "b", "c"][i].repeat(64)
            )
        });
        let target = std::array::from_fn(|_| {
            format!("{TRUSTED_UPDATER_REPOSITORY}@sha256:{}", "d".repeat(64))
        });
        for images in [&target, &previous] {
            let bytes = run_compose(
                &cfg,
                &files,
                images,
                "v0.4.14",
                &["config", "--format", "json"],
            )
            .unwrap();
            let model: Value = serde_json::from_slice(&bytes).unwrap();
            for (service, image) in SERVICES.into_iter().zip(images) {
                assert_eq!(model["services"][service]["image"], *image);
            }
            assert_eq!(
                model["services"]["docker-guard"]["environment"]["DOCKER_GUARD_EXPECTED_IMAGE"],
                images[0]
            );
            assert_eq!(
                model["services"]["proxy"]["image"],
                base["services"]["proxy"]["image"]
            );
        }
        // A later ordinary host invocation (e.g. a 1Panel rebuild) does not set
        // the handoff selectors, so the TCB images resolve by TAG again even
        // though the stale observed *_REF pins still sit in both env files.
        let output = Command::new("docker")
            .args(["compose", "-p", "myriad-tag-test", "-f"])
            .arg(&compose)
            .arg("--env-file")
            .arg(&cfg.app_env_file)
            .arg("--env-file")
            .arg(&cfg.guard_env_file)
            .args(["config", "--format", "json"])
            .env_remove("UPDATER_TAG")
            .env_remove("MYRIAD_TCB_GUARD_IMAGE")
            .env_remove("MYRIAD_TCB_UPDATER_IMAGE")
            .env_remove("MYRIAD_TCB_GATEWAY_IMAGE")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let model: Value = serde_json::from_slice(&output.stdout).unwrap();
        for service in SERVICES {
            assert_eq!(
                model["services"][service]["image"],
                "docker.io/somekawahitomi/myriad-updater:v0.4.14"
            );
        }
        // No handoff override file may remain under the deployment root.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 4);
        assert!(!dir.path().join("state/self-update-override.json").exists());
    }

    #[test]
    fn transaction_image_selection_rejects_untrusted_refs() {
        let mut images: [String; 3] = std::array::from_fn(|_| exact_image());
        assert!(
            images
                .iter()
                .all(|image| validate_exact_image(image).is_ok())
        );
        for bad in [
            "evil.example/updater:v1.2.3",
            "docker.io/somekawahitomi/myriad-updater:v1.2.3",
        ] {
            images[2] = bad.into();
            assert!(validate_exact_image(&images[2]).is_err());
        }
    }

    #[test]
    fn per_service_rollback_model_keeps_images_distinct_and_checks_each_digest() {
        let cfg = config();
        let images: [String; 3] = std::array::from_fn(|i| {
            format!(
                "{TRUSTED_UPDATER_REPOSITORY}@sha256:{}",
                ["a", "b", "c"][i].repeat(64)
            )
        });
        let mut model = compose_model(&exact_image());
        for (service, image) in SERVICES.into_iter().zip(&images) {
            model["services"][service]["image"] = json!(image);
        }
        assert!(
            validate_stack_compose_model(&serde_json::to_vec(&model).unwrap(), &images, &cfg)
                .is_ok()
        );
        for service in SERVICES {
            let mut wrong = model.clone();
            wrong["services"][service]["image"] = json!(format!(
                "{TRUSTED_UPDATER_REPOSITORY}@sha256:{}",
                "f".repeat(64)
            ));
            assert!(
                validate_stack_compose_model(&serde_json::to_vec(&wrong).unwrap(), &images, &cfg)
                    .is_err()
            );
        }
        assert!(parse_previous_images(&serde_json::to_string(&images).unwrap()).is_ok());
        for bad in [
            json!([images[0]]),
            json!([images[0], images[1], "evil.example/updater@sha256:bad"]),
        ] {
            assert!(parse_previous_images(&bad.to_string()).is_err());
        }
    }

    #[test]
    fn uniform_upgrade_clears_old_gateway_override() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = config();
        cfg.app_env_file = dir.path().join(".env");
        cfg.guard_env_file = dir.path().join("guard.env");
        std::fs::write(&cfg.app_env_file, "UPDATER_GATEWAY_IMAGE_REF=old\n").unwrap();
        std::fs::write(&cfg.guard_env_file, "").unwrap();
        update_policy_files(&cfg, &exact_image(), "v1.2.3").unwrap();
        assert_eq!(
            EnvFile::load(&cfg.app_env_file)
                .unwrap()
                .get("UPDATER_GATEWAY_IMAGE_REF"),
            Some("")
        );
    }

    #[test]
    fn reconciliation_preserves_files_when_stack_is_mixed_or_changed() {
        use crate::docker::guard::startup::RuntimeIdentity;
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = config();
        cfg.app_env_file = dir.path().join(".env");
        cfg.guard_env_file = dir.path().join("guard.env");
        let original = "MYRIAD_TAG=v0.4.14\nPROXY_TAG=v0.3.32\nUPDATER_TAG=v0.4.6\nUPDATER_IMAGE_REF=old\nDOCKER_GUARD_IMAGE=old\n";
        std::fs::write(&cfg.app_env_file, original).unwrap();
        std::fs::write(&cfg.guard_env_file, original).unwrap();
        let identity = RuntimeIdentity {
            image: exact_image(),
            image_id: format!("sha256:{}", "b".repeat(64)),
            version: cfg.target_tag.clone(),
        };
        for field in ["image", "version", "image_id"] {
            let mut identities = vec![identity.clone(); 3];
            match field {
                "image" => {
                    identities[1].image =
                        format!("{TRUSTED_UPDATER_REPOSITORY}@sha256:{}", "c".repeat(64))
                }
                "version" => identities[1].version = "v0.4.6".into(),
                _ => identities[1].image_id = format!("sha256:{}", "d".repeat(64)),
            }
            assert!(persist_reconciled_policy(&cfg, &identities).is_err());
            assert_eq!(
                std::fs::read_to_string(&cfg.app_env_file).unwrap(),
                original
            );
            assert_eq!(
                std::fs::read_to_string(&cfg.guard_env_file).unwrap(),
                original
            );
        }
        let mut changed = identity.clone();
        changed.version = "v0.4.6".into();
        assert!(persist_reconciled_policy(&cfg, &vec![changed; 3]).is_err());
        assert!(persist_reconciled_policy(&cfg, std::slice::from_ref(&identity)).is_err());
        let mut live_updater_view = std::fs::File::open(&cfg.app_env_file).unwrap();
        persist_reconciled_policy(&cfg, &vec![identity; 3]).unwrap();
        let mut live_text = String::new();
        live_updater_view.read_to_string(&mut live_text).unwrap();
        assert!(
            live_text.contains(&format!("UPDATER_IMAGE_REF={}", exact_image())),
            "live file bind must observe the synchronized digest"
        );
        let app = EnvFile::load(&cfg.app_env_file).unwrap();
        assert_eq!(app.get("MYRIAD_TAG"), Some("v0.4.14"));
        assert_eq!(app.get("PROXY_TAG"), Some("v0.3.32"));
        assert_eq!(app.get("UPDATER_TAG"), Some("v0.4.6"));
        assert_eq!(app.get("UPDATER_IMAGE_REF"), Some(exact_image().as_str()));
        assert_eq!(app.get("DOCKER_GUARD_IMAGE"), Some(exact_image().as_str()));
    }

    #[test]
    fn reconciliation_validates_both_files_before_any_write() {
        use crate::docker::guard::startup::RuntimeIdentity;
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = config();
        cfg.app_env_file = dir.path().join(".env");
        cfg.guard_env_file = dir.path().join("guard.env");
        let original = "UPDATER_TAG=v0.4.6\n";
        std::fs::write(&cfg.app_env_file, original).unwrap();
        std::fs::write(
            &cfg.guard_env_file,
            "DOCKER_GUARD_IMAGE=old\nDOCKER_GUARD_IMAGE=duplicate\n",
        )
        .unwrap();
        let identity = RuntimeIdentity {
            image: exact_image(),
            image_id: "sha256:unused".into(),
            version: cfg.target_tag.clone(),
        };
        assert!(persist_reconciled_policy(&cfg, &vec![identity; 3]).is_err());
        assert_eq!(
            std::fs::read_to_string(&cfg.app_env_file).unwrap(),
            original
        );
    }

    #[test]
    fn policy_sync_updates_both_pins_and_preserves_unrelated_settings() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = config();
        cfg.app_env_file = dir.path().join(".env");
        cfg.guard_env_file = dir.path().join("guard.env");
        std::fs::write(&cfg.app_env_file,
            "# host settings\nUPDATER_TAG=v0.4.6\nUPDATER_IMAGE_REF=old\nDOCKER_GUARD_IMAGE=old\nUPDATE_TOKEN=keep-app-token\n").unwrap();
        std::fs::write(
            &cfg.guard_env_file,
            "DOCKER_GUARD_IMAGE=old\nGUARD_SELF_UPDATE_TOKEN=keep-host-token\n",
        )
        .unwrap();
        update_policy_files(&cfg, &exact_image(), "v0.4.13").unwrap();
        let app = EnvFile::load(&cfg.app_env_file).unwrap();
        let guard = EnvFile::load(&cfg.guard_env_file).unwrap();
        assert_eq!(app.get("UPDATER_TAG"), Some("v0.4.13"));
        assert_eq!(app.get("UPDATER_IMAGE_REF"), Some(exact_image().as_str()));
        assert_eq!(app.get("DOCKER_GUARD_IMAGE"), Some(exact_image().as_str()));
        assert_eq!(
            guard.get("DOCKER_GUARD_IMAGE"),
            Some(exact_image().as_str())
        );
        assert_eq!(app.get("UPDATE_TOKEN"), Some("keep-app-token"));
        assert_eq!(
            guard.get("GUARD_SELF_UPDATE_TOKEN"),
            Some("keep-host-token")
        );
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
    fn host_compose_customization_does_not_block_selected_images() {
        let image = exact_image();
        let model = compose_model(&image);
        let bytes = serde_json::to_vec(&model).unwrap();
        assert!(validate_compose_model(&bytes, &image, &config()).is_ok());

        let mut custom = model;
        custom["services"]["docker-guard"]["command"] =
            json!(["sh", "-c", "exec /usr/local/bin/myriad-docker-guard"]);
        custom["services"]["updater"]["healthcheck"] =
            json!({"test":["CMD-SHELL", "curl http://localhost:1101/healthz"]});
        custom["services"]["updater"]["volumes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"target":"/etc/ssl/certs/custom.pem"}));
        custom["services"]["updater"]["security_opt"] =
            json!(["no-new-privileges:true", "apparmor:custom"]);
        assert!(
            validate_compose_model(&serde_json::to_vec(&custom).unwrap(), &image, &config())
                .is_ok()
        );
        custom["services"]
            .as_object_mut()
            .unwrap()
            .remove("updater-gateway");
        assert!(
            validate_compose_model(&serde_json::to_vec(&custom).unwrap(), &image, &config())
                .is_err()
        );
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
    fn fixed_compose_model_allows_writable_root_without_env_file_bind() {
        let image = exact_image();
        let mut model = compose_model(&image);
        model["services"]["updater"]["volumes"]
            .as_array_mut()
            .unwrap()
            .retain(|mount| mount["target"] != "/host/compose/.env");
        let bytes = serde_json::to_vec(&model).unwrap();
        assert!(validate_compose_model(&bytes, &image, &config()).is_ok());
    }

    #[test]
    fn fixed_compose_model_accepts_identity_host_root_mounts() {
        let image = exact_image();
        let mut model = compose_model(&image);
        for mount in model["services"]["updater"]["volumes"]
            .as_array_mut()
            .unwrap()
        {
            if let Some(target) = mount["target"].as_str()
                && let Some(rest) = target.strip_prefix("/host/compose")
            {
                mount["target"] = serde_json::json!(format!("/srv/myriad{rest}"));
            }
        }
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
    fn official_compose_guard_command_does_not_write_policy_stub() {
        let compose = include_str!("../../../docker-compose.yml");
        let example = include_str!(
            "../../../docs/deployment/examples/docker-compose.external-db.example.yml"
        );
        for text in [compose, example] {
            assert!(!text.contains("<<'POLICY'"));
            assert!(!text.contains("cat > /guard-policy/docker-guard.env"));
            assert!(
                text.contains("if [ ! -f /guard-policy/docker-guard.env ]; then umask 077; fi")
            );
            assert!(text.contains(
                "MYRIAD_SETUP_SECRET: ${MYRIAD_SETUP_SECRET:?Set MYRIAD_SETUP_SECRET in .env}"
            ));
        }
    }

    #[test]
    fn official_compose_selects_tcb_images_by_process_env_with_tag_fallback() {
        let compose = include_str!("../../../docker-compose.yml");
        let example = include_str!(
            "../../../docs/deployment/examples/docker-compose.external-db.example.yml"
        );
        for text in [compose, example] {
            for selector in [
                "${MYRIAD_TCB_GUARD_IMAGE:-",
                "${MYRIAD_TCB_UPDATER_IMAGE:-",
                "${MYRIAD_TCB_GATEWAY_IMAGE:-",
            ] {
                assert!(text.contains(selector), "missing {selector}");
            }
            // The observed *_REF / DOCKER_GUARD_IMAGE records must never become
            // Compose image selectors, or a host-side rebuild (1Panel) would pin
            // instead of honoring UPDATER_TAG.
            assert!(!text.contains("image: ${UPDATER_IMAGE_REF"));
            assert!(!text.contains("image: ${UPDATER_GATEWAY_IMAGE_REF"));
            assert!(!text.contains("image: ${DOCKER_GUARD_IMAGE"));
        }
    }
}
