//! Installation capability for the setup control plane.
//!
//! `CONFIG_MODE`, database reachability, and an empty administrator table are
//! state predicates, not authorization. Every setup mutation therefore needs a
//! short-lived capability stored outside the web surface. The first owner claim
//! consumes that capability; durable owner state prevents replay after restart.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::HeaderMap;
use myriad_error::AppError;
use rand::{distr::Alphanumeric, RngExt};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

pub const BOOTSTRAP_TOKEN_HEADER: &str = "x-bootstrap-token";
const TOKEN_FILE_NAME: &str = ".bootstrap-token";
const CLAIMED_MARKER_FILE_NAME: &str = ".bootstrap-claimed";
const ROTATION_MARKER_FILE_NAME: &str = ".bootstrap-rotating";
const TOKEN_FILE_VERSION: u8 = 1;
const MIN_OPERATOR_TOKEN_LEN: usize = 32;
const DEFAULT_TTL_SECS: u64 = 30 * 60;
const MIN_TTL_SECS: u64 = 60;
const MAX_TTL_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Serialize, Deserialize)]
struct PersistedCapability {
    version: u8,
    token: String,
    issued_at_unix_secs: u64,
}

#[derive(Debug)]
struct InstallationCapability {
    token: Option<String>,
    expires_at_unix_secs: u64,
    generated_token_file: Option<PathBuf>,
    token_file: PathBuf,
    claimed_marker_file: PathBuf,
    rotation_marker_file: PathBuf,
    ttl_secs: u64,
    allow_remote: bool,
}

impl InstallationCapability {
    fn authorize(
        &self,
        headers: &HeaderMap,
        peer_ip: IpAddr,
        now_unix_secs: u64,
    ) -> Result<(), AppError> {
        if !peer_ip.is_loopback() && !self.allow_remote {
            return Err(remote_bootstrap_forbidden_error());
        }
        let Some(expected) = self.token.as_deref() else {
            return Err(bootstrap_token_required_error());
        };
        if now_unix_secs >= self.expires_at_unix_secs {
            return Err(bootstrap_token_expired_error());
        }
        let provided = headers
            .get(BOOTSTRAP_TOKEN_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        if bootstrap_token_matches(expected, provided) {
            Ok(())
        } else {
            Err(bootstrap_token_required_error())
        }
    }

    fn consume(&mut self) -> Option<PathBuf> {
        self.token.take();
        self.generated_token_file.take()
    }

    fn rotate(&mut self, now_unix_secs: u64) -> io::Result<()> {
        // The durable marker is written first. If the process dies anywhere in
        // rotation, startup sees it and mints another replacement instead of
        // ever reloading the database-init token from the old file.
        persist_claimed_marker(&self.rotation_marker_file)?;
        // Invalidate the capability before touching disk. If persistence fails,
        // this process remains fail-closed and a restart can safely mint a new
        // capability for the still-unclaimed installation.
        self.token.take();
        self.generated_token_file.take();

        let rotated = PersistedCapability {
            version: TOKEN_FILE_VERSION,
            token: generate_token(),
            issued_at_unix_secs: now_unix_secs,
        };
        persist_generated_capability(&self.token_file, &rotated)?;
        remove_token_file(&self.rotation_marker_file)?;
        self.expires_at_unix_secs = now_unix_secs.saturating_add(self.ttl_secs);
        self.token = Some(rotated.token);
        self.generated_token_file = Some(self.token_file.clone());
        Ok(())
    }
}

/// Absence is intentionally fail-closed. Startup must explicitly initialize
/// this state whenever installation is unclaimed.
static INSTALLATION_CAPABILITY: OnceLock<Mutex<InstallationCapability>> = OnceLock::new();

fn unix_now() -> io::Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| io::Error::other(format!("system clock is before Unix epoch: {error}")))
}

fn token_file_path(data_dir: &Path) -> PathBuf {
    data_dir.join(TOKEN_FILE_NAME)
}

fn claimed_marker_path(data_dir: &Path) -> PathBuf {
    data_dir.join(CLAIMED_MARKER_FILE_NAME)
}

fn rotation_marker_path(data_dir: &Path) -> PathBuf {
    data_dir.join(ROTATION_MARKER_FILE_NAME)
}

fn generate_token() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect()
}

fn validate_token(token: &str) -> io::Result<()> {
    if token.len() < MIN_OPERATOR_TOKEN_LEN || token.len() > 256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("bootstrap token length must be {MIN_OPERATOR_TOKEN_LEN}..=256 bytes"),
        ));
    }
    if !token.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "bootstrap token must contain printable ASCII without whitespace",
        ));
    }
    Ok(())
}

fn ttl_from_env() -> io::Result<u64> {
    let Some(raw) = std::env::var("MYRIAD_BOOTSTRAP_TTL_SECS").ok() else {
        return Ok(DEFAULT_TTL_SECS);
    };
    let ttl = raw.parse::<u64>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "MYRIAD_BOOTSTRAP_TTL_SECS must be an integer",
        )
    })?;
    if !(MIN_TTL_SECS..=MAX_TTL_SECS).contains(&ttl) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("MYRIAD_BOOTSTRAP_TTL_SECS must be {MIN_TTL_SECS}..={MAX_TTL_SECS}"),
        ));
    }
    Ok(ttl)
}

fn allow_remote_from_env() -> io::Result<bool> {
    match std::env::var("MYRIAD_ALLOW_REMOTE_BOOTSTRAP") {
        Err(std::env::VarError::NotPresent) => Ok(false),
        Ok(value) if value.eq_ignore_ascii_case("true") => Ok(true),
        Ok(value) if value.eq_ignore_ascii_case("false") => Ok(false),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "MYRIAD_ALLOW_REMOTE_BOOTSTRAP must be exactly true or false",
        )),
        Err(error) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("cannot read MYRIAD_ALLOW_REMOTE_BOOTSTRAP: {error}"),
        )),
    }
}

fn validate_token_file(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "bootstrap capability path is not a regular file: {}",
                path.display()
            ),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("bootstrap capability must be mode 0600: {}", path.display()),
            ));
        }
        let parent_uid = path
            .parent()
            .map(fs::metadata)
            .transpose()?
            .map(|parent| parent.uid());
        if parent_uid.is_some_and(|uid| metadata.uid() != uid) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "bootstrap capability is not owned by the backend uid: {}",
                    path.display()
                ),
            ));
        }
    }
    Ok(metadata)
}

fn read_persisted_capability(path: &Path) -> io::Result<PersistedCapability> {
    let metadata = validate_token_file(path)?;
    let text = fs::read_to_string(path)?;
    if let Ok(persisted) = serde_json::from_str::<PersistedCapability>(&text) {
        if persisted.version != TOKEN_FILE_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "unsupported bootstrap capability version: {}",
                    persisted.version
                ),
            ));
        }
        validate_token(&persisted.token)?;
        return Ok(persisted);
    }

    // Compatibility with the previous plaintext token file. Its filesystem
    // modification time becomes the issuance time; malformed files never reopen setup.
    let token = text.trim().to_string();
    validate_token(&token)?;
    let issued_at_unix_secs = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(|error| io::Error::other(format!("invalid token mtime: {error}")))?
        .as_secs();
    Ok(PersistedCapability {
        version: TOKEN_FILE_VERSION,
        token,
        issued_at_unix_secs,
    })
}

fn persist_generated_capability(path: &Path, capability: &PersistedCapability) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".bootstrap-token.tmp-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let payload = serde_json::to_vec(capability)
        .map_err(|error| io::Error::other(format!("serialize bootstrap capability: {error}")))?;

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let write_result = (|| -> io::Result<()> {
        let mut file = options.open(&temporary)?;
        file.write_all(&payload)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        #[cfg(unix)]
        fs::set_permissions(
            &temporary,
            std::os::unix::fs::PermissionsExt::from_mode(0o600),
        )?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(&temporary, path)?;
        validate_token_file(path)?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result
}

fn remove_token_file(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn persist_claimed_marker(path: &Path) -> io::Result<()> {
    if path.exists() {
        validate_token_file(path)?;
        return Ok(());
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(b"claimed\n")?;
    file.sync_all()?;
    validate_token_file(path)?;
    Ok(())
}

fn recover_incomplete_rotation(
    token_file: &Path,
    rotation_marker_file: &Path,
    now_unix_secs: u64,
) -> io::Result<Option<PersistedCapability>> {
    match fs::symlink_metadata(rotation_marker_file) {
        Ok(_) => {
            validate_token_file(rotation_marker_file)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    let replacement = PersistedCapability {
        version: TOKEN_FILE_VERSION,
        token: generate_token(),
        issued_at_unix_secs: now_unix_secs,
    };
    persist_generated_capability(token_file, &replacement)?;
    remove_token_file(rotation_marker_file)?;
    Ok(Some(replacement))
}

pub(crate) fn bootstrap_required_notice(token_file: Option<&Path>, ttl_secs: u64) -> String {
    let source = match token_file {
        Some(path) => format!("read the owner-only file {}", path.display()),
        None => "use the operator-supplied MYRIAD_BOOTSTRAP_TOKEN".to_string(),
    };
    format!(
        "Installation setup is locked by a short-lived capability. {source}; send it as \
         `{BOOTSTRAP_TOKEN_HEADER}`. The secret is not echoed in logs and expires in \
         {ttl_secs} seconds. Remote peers are denied unless MYRIAD_ALLOW_REMOTE_BOOTSTRAP=true."
    )
}

pub(crate) fn notice_leaks_token(notice: &str, token: &str) -> bool {
    token.len() >= 8 && notice.contains(token)
}

pub(crate) fn bootstrap_token_required_error() -> AppError {
    AppError::unauthorized("Bootstrap token required")
        .with_message("安装操作需要服务器本地的短期引导令牌；令牌不会输出到常规日志。")
        .with_hint(format!("Send the `{BOOTSTRAP_TOKEN_HEADER}` header"))
}

fn bootstrap_token_expired_error() -> AppError {
    AppError::unauthorized("Bootstrap token expired")
        .with_message("安装引导令牌已过期；请重启未完成安装的服务以安全轮换令牌。")
}

fn remote_bootstrap_forbidden_error() -> AppError {
    AppError::forbidden("Remote bootstrap disabled").with_message(
        "安装控制面默认只接受 loopback 连接；远程安装需要运维显式设置 MYRIAD_ALLOW_REMOTE_BOOTSTRAP=true。",
    )
}

pub(crate) fn bootstrap_token_matches(expected: &str, provided: &str) -> bool {
    let provided = provided.trim();
    provided.len() == expected.len() && provided.as_bytes().ct_eq(expected.as_bytes()).into()
}

/// Initialize the unclaimed installation capability. Repeated calls in the
/// same process keep the original state.
pub fn init_for_setup(data_dir: &Path, database_verified_unclaimed: bool) -> io::Result<()> {
    if INSTALLATION_CAPABILITY.get().is_some() {
        return Ok(());
    }
    let now = unix_now()?;
    let ttl_secs = ttl_from_env()?;
    let allow_remote = allow_remote_from_env()?;
    let token_file = token_file_path(data_dir);
    let claimed_marker_file = claimed_marker_path(data_dir);
    let rotation_marker_file = rotation_marker_path(data_dir);
    match fs::symlink_metadata(&claimed_marker_file) {
        Ok(_) if database_verified_unclaimed => {
            validate_token_file(&claimed_marker_file)?;
            remove_token_file(&claimed_marker_file)?;
        }
        Ok(_) => {
            validate_token_file(&claimed_marker_file)?;
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "installation is durably marked claimed; database proof is required before reopening setup",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    let recovered_rotation = recover_incomplete_rotation(&token_file, &rotation_marker_file, now)?;
    let token_file_exists = match fs::symlink_metadata(&token_file) {
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error),
    };
    let operator_token = std::env::var("MYRIAD_BOOTSTRAP_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let (token, issued_at, generated_token_file) = if let Some(replacement) = recovered_rotation {
        (replacement.token, now, Some(token_file.clone()))
    } else if token_file_exists {
        // A persisted capability always wins over the environment. In
        // particular, a database-init rotation must survive restart and an old
        // MYRIAD_BOOTSTRAP_TOKEN must never reactivate the previous phase.
        let existing = read_persisted_capability(&token_file)?;
        if existing.issued_at_unix_secs > now.saturating_add(5) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "bootstrap capability issuance time is in the future",
            ));
        }
        if now < existing.issued_at_unix_secs.saturating_add(ttl_secs) {
            (
                existing.token,
                existing.issued_at_unix_secs,
                Some(token_file.clone()),
            )
        } else {
            let rotated = PersistedCapability {
                version: TOKEN_FILE_VERSION,
                token: generate_token(),
                issued_at_unix_secs: now,
            };
            persist_generated_capability(&token_file, &rotated)?;
            (rotated.token, now, Some(token_file.clone()))
        }
    } else {
        let generated = PersistedCapability {
            version: TOKEN_FILE_VERSION,
            token: match operator_token {
                Some(token) => {
                    validate_token(&token)?;
                    token
                }
                None => generate_token(),
            },
            issued_at_unix_secs: now,
        };
        persist_generated_capability(&token_file, &generated)?;
        (generated.token, now, Some(token_file.clone()))
    };

    let notice = bootstrap_required_notice(generated_token_file.as_deref(), ttl_secs);
    debug_assert!(!notice_leaks_token(&notice, &token));
    let state = InstallationCapability {
        token: Some(token),
        expires_at_unix_secs: issued_at.saturating_add(ttl_secs),
        generated_token_file,
        token_file,
        claimed_marker_file,
        rotation_marker_file,
        ttl_secs,
        allow_remote,
    };
    if INSTALLATION_CAPABILITY.set(Mutex::new(state)).is_ok() {
        tracing::warn!("{notice}");
    }
    Ok(())
}

/// Check the capability and the raw transport peer. Forwarded headers are
/// intentionally ignored because a public proxy must not turn itself into loopback.
pub fn require_bootstrap(headers: &HeaderMap, peer_ip: IpAddr) -> Result<(), AppError> {
    let Some(state) = INSTALLATION_CAPABILITY.get() else {
        tracing::warn!("Setup request rejected: installation capability is uninitialized");
        return Err(bootstrap_token_required_error());
    };
    let now = unix_now().map_err(|error| {
        AppError::internal("Bootstrap clock unavailable").with_message(error.to_string())
    })?;
    let state = state.lock().map_err(|_| {
        AppError::internal("Bootstrap capability state unavailable")
            .with_message("Installation capability state is poisoned; restart required.")
    })?;
    state.authorize(headers, peer_ip, now)
}

/// Durably mark the installation claimed, then consume the process capability
/// immediately before the owner transaction commits. Marker failure leaves the
/// token valid so the database transaction can be rolled back and retried.
pub fn consume_bootstrap() -> io::Result<()> {
    let Some(state) = INSTALLATION_CAPABILITY.get() else {
        return Err(io::Error::other("installation capability is uninitialized"));
    };
    let mut state = state
        .lock()
        .map_err(|_| io::Error::other("installation capability mutex is poisoned"))?;
    persist_claimed_marker(&state.claimed_marker_file)?;
    let token_file = state.consume();
    if let Some(path) = token_file {
        remove_token_file(&path)?;
    }
    Ok(())
}

/// Emergency fail-closed transition for a path that has already obtained
/// durable owner proof but could not persist cleanup metadata.
pub fn invalidate_bootstrap_in_memory() -> io::Result<()> {
    let Some(state) = INSTALLATION_CAPABILITY.get() else {
        return Err(io::Error::other("installation capability is uninitialized"));
    };
    state
        .lock()
        .map_err(|_| io::Error::other("installation capability mutex is poisoned"))?
        .consume();
    Ok(())
}

/// A successful database initialization consumes its authorizing secret and
/// advances setup to the owner-claim phase with a newly generated capability.
/// The replacement is only written to the owner-only token file; it is never
/// returned by the web API or logged.
pub fn rotate_bootstrap_after_database_initialization() -> io::Result<()> {
    let Some(state) = INSTALLATION_CAPABILITY.get() else {
        return Err(io::Error::other("installation capability is uninitialized"));
    };
    let now = unix_now()?;
    state
        .lock()
        .map_err(|_| io::Error::other("installation capability mutex is poisoned"))?
        .rotate(now)
}

/// Claimed installations do not initialize a capability on restart. Remove a
/// stale file so an old secret cannot be mistaken for an active one.
pub fn remove_stale_token_file(data_dir: &Path) -> io::Result<()> {
    persist_claimed_marker(&claimed_marker_path(data_dir))?;
    remove_token_file(&token_file_path(data_dir))
}

pub fn validate_env_value(key: &str, value: &str) -> Result<(), String> {
    if value.contains('\n') || value.contains('\r') {
        return Err(format!(
            "{key} contains a line break; that would inject additional .env entries"
        ));
    }
    if value.contains('\0') {
        return Err(format!("{key} contains a NUL byte"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(token: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(token) = token {
            headers.insert(BOOTSTRAP_TOKEN_HEADER, token.parse().unwrap());
        }
        headers
    }

    fn capability(token: &str, allow_remote: bool) -> InstallationCapability {
        InstallationCapability {
            token: Some(token.to_string()),
            expires_at_unix_secs: 1_100,
            generated_token_file: None,
            token_file: PathBuf::from(".bootstrap-token"),
            claimed_marker_file: PathBuf::from(".bootstrap-claimed"),
            rotation_marker_file: PathBuf::from(".bootstrap-rotating"),
            ttl_secs: DEFAULT_TTL_SECS,
            allow_remote,
        }
    }

    #[test]
    fn installation_capability_rejects_missing_wrong_expired_and_replay() {
        let token = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKL";
        let mut state = capability(token, false);
        let loopback: IpAddr = "127.0.0.1".parse().unwrap();
        assert!(state.authorize(&headers(None), loopback, 1_000).is_err());
        assert!(state
            .authorize(
                &headers(Some("wrong-token-value-that-is-long-enough")),
                loopback,
                1_000
            )
            .is_err());
        assert!(state
            .authorize(&headers(Some(token)), loopback, 1_100)
            .is_err());
        assert!(state
            .authorize(&headers(Some(token)), loopback, 1_000)
            .is_ok());
        state.consume();
        assert!(state
            .authorize(&headers(Some(token)), loopback, 1_000)
            .is_err());
    }

    #[test]
    fn database_initialization_rotation_invalidates_the_original_across_restart() {
        let dir = std::env::temp_dir().join(format!("myriad-bootstrap-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = token_file_path(&dir);
        let original = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKL";
        let mut state = InstallationCapability {
            token: Some(original.to_string()),
            expires_at_unix_secs: 1_100,
            generated_token_file: None,
            token_file: path.clone(),
            claimed_marker_file: dir.join(CLAIMED_MARKER_FILE_NAME),
            rotation_marker_file: dir.join(ROTATION_MARKER_FILE_NAME),
            ttl_secs: 120,
            allow_remote: false,
        };

        state.rotate(1_010).unwrap();
        let replacement = read_persisted_capability(&path).unwrap();
        assert_ne!(replacement.token, original);
        let loopback: IpAddr = "127.0.0.1".parse().unwrap();
        assert!(state
            .authorize(&headers(Some(original)), loopback, 1_011)
            .is_err());
        assert!(state
            .authorize(&headers(Some(&replacement.token)), loopback, 1_011)
            .is_ok());

        let restarted = InstallationCapability {
            token: Some(replacement.token.clone()),
            expires_at_unix_secs: replacement.issued_at_unix_secs + 120,
            generated_token_file: Some(path.clone()),
            token_file: path,
            claimed_marker_file: dir.join(CLAIMED_MARKER_FILE_NAME),
            rotation_marker_file: dir.join(ROTATION_MARKER_FILE_NAME),
            ttl_secs: 120,
            allow_remote: false,
        };
        assert!(restarted
            .authorize(&headers(Some(original)), loopback, 1_011)
            .is_err());
        assert!(restarted
            .authorize(&headers(Some(&replacement.token)), loopback, 1_011)
            .is_ok());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn incomplete_rotation_marker_never_reloads_the_old_token() {
        let dir = std::env::temp_dir().join(format!("myriad-bootstrap-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let token_file = token_file_path(&dir);
        let marker = rotation_marker_path(&dir);
        let original = PersistedCapability {
            version: TOKEN_FILE_VERSION,
            token: "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKL".into(),
            issued_at_unix_secs: 1_000,
        };
        persist_generated_capability(&token_file, &original).unwrap();
        persist_claimed_marker(&marker).unwrap();

        let recovered = recover_incomplete_rotation(&token_file, &marker, 1_010)
            .unwrap()
            .unwrap();
        assert_ne!(recovered.token, original.token);
        assert_eq!(
            read_persisted_capability(&token_file).unwrap().token,
            recovered.token
        );
        assert!(!marker.exists());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn remote_peer_requires_explicit_policy_and_forwarded_headers_do_not_matter() {
        let token = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKL";
        let remote: IpAddr = "203.0.113.9".parse().unwrap();
        let mut supplied = headers(Some(token));
        supplied.insert("x-forwarded-for", "127.0.0.1".parse().unwrap());
        assert!(capability(token, false)
            .authorize(&supplied, remote, 1_000)
            .is_err());
        assert!(capability(token, true)
            .authorize(&supplied, remote, 1_000)
            .is_ok());
    }

    #[test]
    fn persisted_capability_is_private_and_parse_errors_fail_closed() {
        let dir = std::env::temp_dir().join(format!("myriad-bootstrap-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = token_file_path(&dir);
        let persisted = PersistedCapability {
            version: TOKEN_FILE_VERSION,
            token: generate_token(),
            issued_at_unix_secs: 123,
        };
        persist_generated_capability(&path, &persisted).unwrap();
        let loaded = read_persisted_capability(&path).unwrap();
        assert_eq!(loaded.token, persisted.token);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::write(&path, "not a valid capability\n").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
        assert!(read_persisted_capability(&path).is_err());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn generated_tokens_are_long_and_unique() {
        let first = generate_token();
        let second = generate_token();
        assert_eq!(first.len(), 48);
        assert_ne!(first, second);
        assert!(first
            .chars()
            .all(|character| character.is_ascii_alphanumeric()));
    }

    #[test]
    fn notice_never_contains_the_secret() {
        let token = "SuperSecretBootstrapTokenValueABCDEF1234567890XYZ";
        let path = Path::new("/var/lib/myriad/.bootstrap-token");
        let notice = bootstrap_required_notice(Some(path), DEFAULT_TTL_SECS);
        assert!(!notice_leaks_token(&notice, token));
        assert!(notice.contains(path.to_str().unwrap()));
        assert!(notice.contains(BOOTSTRAP_TOKEN_HEADER));
    }

    #[test]
    fn token_compare_and_env_value_validation_are_strict() {
        let token = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKL";
        assert!(bootstrap_token_matches(token, token));
        assert!(bootstrap_token_matches(token, &format!("  {token}  ")));
        assert!(!bootstrap_token_matches(token, ""));
        assert!(validate_env_value("JWT_SECRET", "abc\nADMIN_OVERRIDE=1").is_err());
        assert!(validate_env_value("JWT_SECRET", "abc\0def").is_err());
        assert!(validate_env_value("DATABASE_URL", "postgres://a:b==@h/d").is_ok());
    }
}
