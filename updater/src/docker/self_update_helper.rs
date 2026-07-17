//! One-shot TCB self-update helper (runs inside a short-lived container).
//!
//! Responsibilities:
//! 1. `docker compose up -d --no-deps docker-guard updater updater-gateway` via fixed argv
//! 2. On failure: restore `UPDATER_TAG` in the deployment `.env` to `previous_tag`
//! 3. Always write durable status to `state/self-update-last.json`
//!
//! Tags arrive via env vars (set by docker-guard after charset validation). The
//! helper re-validates before any filesystem write so shell metacharacters never
//! reach compose argv or `.env` content.

use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::env_file::EnvFile;
use crate::error::{Result, UpdaterError};
use crate::state::atomic;

/// Env vars set by docker-guard when spawning this helper.
pub const ENV_PREVIOUS_TAG: &str = "MYRIAD_SELF_UPDATE_PREVIOUS_TAG";
pub const ENV_TARGET_TAG: &str = "MYRIAD_SELF_UPDATE_TARGET_TAG";
pub const ENV_PROJECT: &str = "MYRIAD_SELF_UPDATE_PROJECT";
pub const ENV_PROJECT_DIRECTORY: &str = "MYRIAD_SELF_UPDATE_PROJECT_DIRECTORY";
pub const ENV_COMPOSE_DIR: &str = "MYRIAD_SELF_UPDATE_COMPOSE_DIR";
pub const ENV_ENV_FILE: &str = "MYRIAD_SELF_UPDATE_ENV_FILE";
pub const ENV_STATUS_FILE: &str = "MYRIAD_SELF_UPDATE_STATUS_FILE";

/// Max length for a tag / simple name we accept from the self-update path.
const MAX_TAG_LEN: usize = 128;

/// Image-tag / deploy-tag charset: alphanumeric plus `.` `_` `-`.
/// Rejects shell metacharacters, path separators, whitespace, etc.
pub fn is_safe_self_update_tag(tag: &str) -> bool {
    if tag.is_empty() || tag.len() > MAX_TAG_LEN {
        return false;
    }
    // Leading `-` can look like a flag if ever interpolated into argv incorrectly.
    if tag.starts_with('-') {
        return false;
    }
    tag.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Compose project / network style name (same charset as tags, no leading dash required ban is fine).
pub fn is_safe_simple_name(name: &str) -> bool {
    is_safe_self_update_tag(name)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelfUpdateOutcome {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfUpdateLastStatus {
    pub status: SelfUpdateOutcome,
    pub target_tag: String,
    pub previous_tag: String,
    /// RFC3339 UTC timestamp.
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SelfUpdateLastStatus {
    pub fn succeeded(previous_tag: &str, target_tag: &str) -> Self {
        Self {
            status: SelfUpdateOutcome::Succeeded,
            target_tag: target_tag.to_string(),
            previous_tag: previous_tag.to_string(),
            at: Utc::now().to_rfc3339(),
            error: None,
        }
    }

    pub fn failed(previous_tag: &str, target_tag: &str, error: impl Into<String>) -> Self {
        Self {
            status: SelfUpdateOutcome::Failed,
            target_tag: target_tag.to_string(),
            previous_tag: previous_tag.to_string(),
            at: Utc::now().to_rfc3339(),
            error: Some(error.into()),
        }
    }
}

/// Rewrite `UPDATER_TAG` in `env_path` to `previous_tag` using [`EnvFile`].
pub fn restore_updater_tag(env_path: &Path, previous_tag: &str) -> Result<()> {
    if !is_safe_self_update_tag(previous_tag) {
        return Err(UpdaterError::InvalidInput(format!(
            "refusing to restore unsafe UPDATER_TAG: {previous_tag}"
        )));
    }
    let mut env = EnvFile::load(env_path)?;
    env.set("UPDATER_TAG", previous_tag)?;
    env.save()?;
    Ok(())
}

/// Persist durable self-update outcome under the deployment volume.
pub fn write_self_update_status(path: &Path, status: &SelfUpdateLastStatus) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    atomic::write_atomic_json(path, status)
}

#[derive(Debug, Clone)]
pub struct HelperConfig {
    pub previous_tag: String,
    pub target_tag: String,
    pub project: String,
    /// Host-side compose root (passed to `docker compose --project-directory`).
    pub project_directory: PathBuf,
    /// Path inside the helper container for the compose tree (usually `/host/compose`).
    pub compose_dir: PathBuf,
    pub env_file: PathBuf,
    pub status_file: PathBuf,
}

impl HelperConfig {
    /// Load from the env vars docker-guard sets on the helper container.
    pub fn from_env() -> Result<Self> {
        let previous_tag = require_env(ENV_PREVIOUS_TAG)?;
        let target_tag = require_env(ENV_TARGET_TAG)?;
        let project = require_env(ENV_PROJECT)?;
        let project_directory = PathBuf::from(require_env(ENV_PROJECT_DIRECTORY)?);
        let compose_dir = PathBuf::from(
            std::env::var(ENV_COMPOSE_DIR).unwrap_or_else(|_| "/host/compose".into()),
        );
        let env_file = PathBuf::from(
            std::env::var(ENV_ENV_FILE).unwrap_or_else(|_| "/host/compose/.env".into()),
        );
        let status_file = PathBuf::from(
            std::env::var(ENV_STATUS_FILE)
                .unwrap_or_else(|_| "/host/compose/state/self-update-last.json".into()),
        );

        validate_config_tags(&previous_tag, &target_tag, &project)?;

        Ok(Self {
            previous_tag,
            target_tag,
            project,
            project_directory,
            compose_dir,
            env_file,
            status_file,
        })
    }
}

fn require_env(key: &str) -> Result<String> {
    std::env::var(key).map_err(|_| {
        UpdaterError::Precondition(format!("missing required env var {key} for TCB self-update helper"))
    })
}

fn validate_config_tags(previous: &str, target: &str, project: &str) -> Result<()> {
    if !is_safe_self_update_tag(previous) {
        return Err(UpdaterError::InvalidInput(format!(
            "invalid previous_tag for self-update: {previous}"
        )));
    }
    if !is_safe_self_update_tag(target) {
        return Err(UpdaterError::InvalidInput(format!(
            "invalid target_tag for self-update: {target}"
        )));
    }
    if !is_safe_simple_name(project) {
        return Err(UpdaterError::InvalidInput(format!(
            "invalid compose project for self-update: {project}"
        )));
    }
    Ok(())
}

/// Discover compose files under `root` (same order as docker-guard).
pub fn find_compose_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for name in [
        "compose.yaml",
        "compose.yml",
        "docker-compose.yaml",
        "docker-compose.yml",
    ] {
        let path = root.join(name);
        if path.exists() {
            files.push(path);
        }
    }
    if files.is_empty() {
        return Err(UpdaterError::Precondition(format!(
            "no Compose file found in {}",
            root.display()
        )));
    }
    Ok(files)
}

/// Run compose recreate; on failure restore `UPDATER_TAG` and always write status.
pub fn run_helper(cfg: &HelperConfig) -> Result<()> {
    let compose_files = find_compose_files(&cfg.compose_dir)?;

    let mut command = Command::new("docker");
    command.args([
        "compose",
        "-p",
        &cfg.project,
        "--project-directory",
        cfg.project_directory
            .to_str()
            .ok_or_else(|| UpdaterError::InvalidInput("project_directory is not valid UTF-8".into()))?,
    ]);
    for file in &compose_files {
        command.arg("-f").arg(file);
    }
    let env_path = cfg
        .env_file
        .to_str()
        .ok_or_else(|| UpdaterError::InvalidInput("env_file is not valid UTF-8".into()))?;
    command
        .arg("--env-file")
        .arg(env_path)
        .args([
            "up",
            "-d",
            "--no-deps",
            "docker-guard",
            "updater",
            "updater-gateway",
        ]);

    let output = command.output().map_err(|e| {
        UpdaterError::Docker(format!("spawn docker compose for TCB self-update: {e}"))
    })?;

    if output.status.success() {
        let status = SelfUpdateLastStatus::succeeded(&cfg.previous_tag, &cfg.target_tag);
        if let Err(e) = write_self_update_status(&cfg.status_file, &status) {
            // Compose already succeeded; still surface status write failure.
            return Err(UpdaterError::State(format!(
                "self-update compose succeeded but failed to write status file {}: {e}",
                cfg.status_file.display()
            )));
        }
        return Ok(());
    }

    let compose_err = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let compose_err = if compose_err.is_empty() {
        format!("docker compose exited with status {}", output.status)
    } else {
        compose_err
    };

    let mut error_parts = vec![format!("compose failed: {compose_err}")];

    match restore_updater_tag(&cfg.env_file, &cfg.previous_tag) {
        Ok(()) => error_parts.push(format!(
            "restored UPDATER_TAG to {}",
            cfg.previous_tag
        )),
        Err(e) => error_parts.push(format!(
            "FAILED to restore UPDATER_TAG to {}: {e}",
            cfg.previous_tag
        )),
    }

    let combined = error_parts.join("; ");
    let status = SelfUpdateLastStatus::failed(&cfg.previous_tag, &cfg.target_tag, &combined);
    let _ = write_self_update_status(&cfg.status_file, &status);

    Err(UpdaterError::Docker(combined))
}

/// Entry used by the `myriad-tcb-self-update` binary.
pub fn main_from_env() -> Result<()> {
    let cfg = HelperConfig::from_env()?;
    run_helper(&cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_validation_accepts_normal_tags() {
        for tag in [
            "v0.2.3",
            "v0.2.3-beta.1",
            "v0.0.0-dev",
            "dev-abc1234",
            "main",
            "preview",
            "0.1.0",
        ] {
            assert!(is_safe_self_update_tag(tag), "{tag}");
        }
    }

    #[test]
    fn tag_validation_rejects_shell_metacharacters() {
        for tag in [
            "; rm -rf /",
            "v1;id",
            "v1$(reboot)",
            "v1`id`",
            "v1|cat",
            "v1&true",
            "v1\nmalicious",
            "v1/../etc",
            "../escape",
            "v1 tag",
            "",
            "-evil",
            "v1'quote",
            "v1\"quote",
            "v1$FOO",
        ] {
            assert!(!is_safe_self_update_tag(tag), "should reject: {tag:?}");
        }
    }

    #[test]
    fn restore_updater_tag_rewrites_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let env_path = dir.path().join(".env");
        std::fs::write(
            &env_path,
            "MYRIAD_TAG=v0.1.0\nUPDATER_TAG=v0.9.0\nPROXY_TAG=v0.1.0\n",
        )
        .unwrap();

        restore_updater_tag(&env_path, "v0.8.0").unwrap();

        let loaded = EnvFile::load(&env_path).unwrap();
        assert_eq!(loaded.get("UPDATER_TAG"), Some("v0.8.0"));
        assert_eq!(loaded.get("MYRIAD_TAG"), Some("v0.1.0"));
        assert_eq!(loaded.get("PROXY_TAG"), Some("v0.1.0"));
    }

    #[test]
    fn restore_updater_tag_rejects_unsafe_previous() {
        let dir = tempfile::tempdir().unwrap();
        let env_path = dir.path().join(".env");
        std::fs::write(&env_path, "UPDATER_TAG=v0.9.0\n").unwrap();
        assert!(restore_updater_tag(&env_path, "v0;rm -rf /").is_err());
        let loaded = EnvFile::load(&env_path).unwrap();
        assert_eq!(loaded.get("UPDATER_TAG"), Some("v0.9.0"));
    }

    #[test]
    fn write_status_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state").join("self-update-last.json");
        let status = SelfUpdateLastStatus::failed("v0.1.0", "v0.2.0", "compose boom");
        write_self_update_status(&path, &status).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: SelfUpdateLastStatus = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.status, SelfUpdateOutcome::Failed);
        assert_eq!(parsed.previous_tag, "v0.1.0");
        assert_eq!(parsed.target_tag, "v0.2.0");
        assert_eq!(parsed.error.as_deref(), Some("compose boom"));
    }
}
