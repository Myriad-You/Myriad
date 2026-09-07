//! Guard process configuration and host policy file pin/heal.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use tracing::{info, warn};

use crate::config::{SecretString, GUARD_SELF_UPDATE_TOKEN_MIN_LEN};

use super::{validate_identifier, POLICY_CONTAINER_FILE, TRUSTED_GUARD_REPOSITORY};

const COMPOSE_RELATIVE_POLICY_PATH: &str = "guard-policy/docker-guard.env";

#[derive(Debug, Clone)]
pub struct GuardConfig {
    pub listen: SocketAddr,
    pub socket_path: PathBuf,
    pub project: String,
    pub compose_network: String,
    /// Admin plane (backend / updater / updater-gateway / proxy rescue). Not internal.
    pub admin_network: String,
    pub guard_network: String,
    pub compose_dir: PathBuf,
    pub state_dir: PathBuf,
    pub expected_guard_image: String,
    pub self_update_token: SecretString,
    pub allow_unpinned_dev: bool,
    pub allowed_images: HashSet<String>,
    pub service_images: HashMap<String, String>,
}

impl GuardConfig {
    pub fn load_from_env() -> Result<Self> {
        let listen = std::env::var("DOCKER_GUARD_LISTEN")
            .unwrap_or_else(|_| "0.0.0.0:2375".into())
            .parse()
            .context("invalid DOCKER_GUARD_LISTEN")?;
        let socket_path = std::env::var("DOCKER_GUARD_SOCKET")
            .unwrap_or_else(|_| "/var/run/docker.sock".into())
            .into();
        let project = std::env::var("COMPOSE_PROJECT_NAME").unwrap_or_else(|_| "myriad".into());
        validate_simple_name("COMPOSE_PROJECT_NAME", &project)?;
        let compose_network =
            std::env::var("MYRIAD_DOCKER_NETWORK").unwrap_or_else(|_| "myriad-net".into());
        validate_simple_name("MYRIAD_DOCKER_NETWORK", &compose_network)?;
        let admin_network =
            std::env::var("MYRIAD_ADMIN_NETWORK").unwrap_or_else(|_| "myriad-admin-net".into());
        validate_simple_name("MYRIAD_ADMIN_NETWORK", &admin_network)?;
        let guard_network = std::env::var("MYRIAD_DOCKER_GUARD_NETWORK")
            .unwrap_or_else(|_| "myriad-docker-guard-net".into());
        validate_simple_name("MYRIAD_DOCKER_GUARD_NETWORK", &guard_network)?;
        let compose_dir = std::env::var("DOCKER_GUARD_COMPOSE_DIR")
            .unwrap_or_else(|_| "/host/compose".into())
            .into();
        let state_dir = std::env::var("DOCKER_GUARD_STATE_DIR")
            .unwrap_or_else(|_| "/host/state".into())
            .into();
        let expected_guard_image = std::env::var("DOCKER_GUARD_EXPECTED_IMAGE")
            .context("DOCKER_GUARD_EXPECTED_IMAGE is required")?;
        let host_policy_path = std::env::var("DOCKER_GUARD_HOST_POLICY_PATH")
            .context("DOCKER_GUARD_HOST_POLICY_PATH is required")?;
        validate_host_policy_path(&host_policy_path)?;
        let self_update_token = std::env::var("DOCKER_GUARD_SELF_UPDATE_TOKEN")
            .context("DOCKER_GUARD_SELF_UPDATE_TOKEN is required")?;
        validate_self_update_token(&self_update_token)?;
        let allow_unpinned_dev = std::env::var("DOCKER_GUARD_ALLOW_UNPINNED_DEV")
            .is_ok_and(|value| value.eq_ignore_ascii_case("true"));
        if allow_unpinned_dev && !cfg!(debug_assertions) {
            return Err(anyhow!(
                "DOCKER_GUARD_ALLOW_UNPINNED_DEV is forbidden in release builds"
            ));
        }
        validate_guard_image_ref(&expected_guard_image, allow_unpinned_dev)?;
        // This policy is compiled into the Guard TCB. It must never come from
        // updater-writable `.env` or runtime configuration.
        let service_images = [
            ("backend", "docker.io/somekawahitomi/myriad-backend"),
            (
                "backend-volume-init",
                "docker.io/somekawahitomi/myriad-backend",
            ),
            ("frontend", "docker.io/somekawahitomi/myriad-frontend"),
            ("proxy", "docker.io/somekawahitomi/myriad-proxy"),
            ("postgres", "postgres"),
        ]
        .into_iter()
        .map(|(service, repository)| (service.to_string(), repository.to_string()))
        .collect::<HashMap<_, _>>();
        let allowed_images = service_images.values().cloned().collect::<HashSet<_>>();

        Ok(Self {
            listen,
            socket_path,
            project,
            compose_network,
            admin_network,
            guard_network,
            compose_dir,
            state_dir,
            expected_guard_image,
            self_update_token: SecretString::new(self_update_token),
            allow_unpinned_dev,
            allowed_images,
            service_images,
        })
    }
}

fn validate_self_update_token(token: &str) -> Result<()> {
    if token.len() < GUARD_SELF_UPDATE_TOKEN_MIN_LEN || token.len() > 256 {
        return Err(anyhow!(
            "DOCKER_GUARD_SELF_UPDATE_TOKEN must be 32..=256 characters"
        ));
    }
    if !token.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(anyhow!(
            "DOCKER_GUARD_SELF_UPDATE_TOKEN must contain only printable non-whitespace ASCII"
        ));
    }
    Ok(())
}

pub(crate) fn validate_host_policy_path(path: &str) -> Result<()> {
    if path == POLICY_CONTAINER_FILE {
        return Ok(());
    }
    Err(anyhow!(
        "DOCKER_GUARD_HOST_POLICY_PATH must be {POLICY_CONTAINER_FILE}"
    ))
}

pub(crate) fn ensure_host_policy_file(config: &GuardConfig, write_path: &Path) -> Result<()> {
    let Some(parent) = write_path.parent() else {
        return Ok(());
    };
    if !parent.exists() {
        info!(
            path = %write_path.display(),
            "Guard policy parent is not mounted; skipping automatic policy creation"
        );
        return Ok(());
    }
    if write_path.exists() {
        if host_policy_file_is_pinned(write_path) {
            heal_host_policy_compose_path(write_path)?;
            return Ok(());
        }
        warn!(
            path = %write_path.display(),
            "replacing invalid host Guard policy"
        );
    }
    let body = format!(
        "DOCKER_GUARD_IMAGE={}\n\
         GUARD_SELF_UPDATE_TOKEN={}\n\
         GUARD_COMPOSE_PROJECT_NAME={}\n\
         GUARD_MYRIAD_DOCKER_NETWORK={}\n\
         GUARD_MYRIAD_ADMIN_NETWORK={}\n\
         GUARD_MYRIAD_DOCKER_GUARD_NETWORK={}\n\
         MYRIAD_GUARD_ENV_FILE={}\n",
        config.expected_guard_image,
        config.self_update_token.expose(),
        config.project,
        config.compose_network,
        config.admin_network,
        config.guard_network,
        COMPOSE_RELATIVE_POLICY_PATH
    );
    let tmp = parent.join(".docker-guard.env.tmp");
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)
            .with_context(|| format!("cannot create {}", tmp.display()))?;
        file.write_all(body.as_bytes())
            .with_context(|| format!("cannot write {}", tmp.display()))?;
        file.sync_all()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(&tmp, write_path)
        .with_context(|| format!("cannot install {}", write_path.display()))?;
    info!(path = %write_path.display(), "wrote host Guard policy");
    Ok(())
}

fn host_policy_file_is_pinned(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    for line in text.lines() {
        let line = line.trim();
        let Some(image) = line.strip_prefix("DOCKER_GUARD_IMAGE=") else {
            continue;
        };
        return validate_guard_image_ref(image, false).is_ok();
    }
    false
}

/// Digest-pinned policy files are otherwise left untouched so a newer Guard
/// cannot rewrite TCB identity. The compose-relative path is not identity:
/// older deploys wrote `/etc/myriad/docker-guard.env` and that value now
/// fails updater preflight. Heal only that key.
fn heal_host_policy_compose_path(path: &Path) -> Result<()> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    let mut seen = false;
    let mut changed = false;
    let mut body = String::with_capacity(text.len().saturating_add(64));
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("MYRIAD_GUARD_ENV_FILE=") {
            if seen {
                changed = true;
                continue;
            }
            seen = true;
            if value == COMPOSE_RELATIVE_POLICY_PATH {
                body.push_str(line);
            } else {
                changed = true;
                body.push_str("MYRIAD_GUARD_ENV_FILE=");
                body.push_str(COMPOSE_RELATIVE_POLICY_PATH);
            }
            body.push('\n');
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    if !seen {
        changed = true;
        body.push_str("MYRIAD_GUARD_ENV_FILE=");
        body.push_str(COMPOSE_RELATIVE_POLICY_PATH);
        body.push('\n');
    }
    if !changed {
        return Ok(());
    }
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    warn!(
        path = %path.display(),
        "healing MYRIAD_GUARD_ENV_FILE in host Guard policy"
    );
    let tmp = parent.join(".docker-guard.env.tmp");
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)
            .with_context(|| format!("cannot create {}", tmp.display()))?;
        file.write_all(body.as_bytes())
            .with_context(|| format!("cannot write {}", tmp.display()))?;
        file.sync_all()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(&tmp, path).with_context(|| format!("cannot install {}", path.display()))?;
    Ok(())
}

pub(crate) fn validate_guard_image_ref(image: &str, allow_unpinned_dev: bool) -> Result<()> {
    if allow_unpinned_dev && image.starts_with("myriad-updater-dev:") {
        return Ok(());
    }
    let prefix = format!("{TRUSTED_GUARD_REPOSITORY}@sha256:");
    let Some(digest) = image.strip_prefix(&prefix) else {
        return Err(anyhow!(
            "DOCKER_GUARD_EXPECTED_IMAGE must be {TRUSTED_GUARD_REPOSITORY}@sha256:<64 hex>"
        ));
    };
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(anyhow!(
            "docker-guard image digest must contain exactly 64 hex characters"
        ));
    }
    Ok(())
}

/// Docker Engine `RepoDigests` often omit the `docker.io/` registry prefix even
/// when the image was pulled as `docker.io/…`. Policy files and Guard identity
/// still require the canonical `docker.io/…@sha256:<64 hex>` form.
pub(crate) fn canonicalize_trusted_digest_ref(actual: &str) -> Result<String> {
    let Some((repo, digest)) = actual.rsplit_once("@sha256:") else {
        return Err(anyhow!(
            "DOCKER_GUARD_EXPECTED_IMAGE must be {TRUSTED_GUARD_REPOSITORY}@sha256:<64 hex>"
        ));
    };
    if repo.trim_start_matches("docker.io/")
        != TRUSTED_GUARD_REPOSITORY.trim_start_matches("docker.io/")
    {
        return Err(anyhow!(
            "pulled image has no trusted updater repository digest"
        ));
    }
    let canonical = format!("{TRUSTED_GUARD_REPOSITORY}@sha256:{digest}");
    validate_guard_image_ref(&canonical, false)?;
    Ok(canonical)
}

pub(crate) fn digest_reference_matches(actual: &str, expected: &str) -> bool {
    let Some((actual_repo, actual_digest)) = actual.rsplit_once("@sha256:") else {
        return false;
    };
    let Some((expected_repo, expected_digest)) = expected.rsplit_once("@sha256:") else {
        return false;
    };
    actual_repo.trim_start_matches("docker.io/") == expected_repo.trim_start_matches("docker.io/")
        && actual_digest.eq_ignore_ascii_case(expected_digest)
}

fn validate_simple_name(label: &str, value: &str) -> Result<()> {
    validate_identifier(value).map_err(|e| anyhow!("{label}: {e}"))
}
