//! Runtime configuration loaded from process environment variables.
//! In production compose these come from the host `.env`.

use std::path::Path;

use crate::error::UpdaterError;
use serde::{Deserialize, Serialize};

/// Minimum accepted length for `UPDATE_TOKEN`.
pub const UPDATE_TOKEN_MIN_LEN: usize = 32;

/// Tokens at or below this length trigger a boot warning (barely minimum).
/// Operators should use a longer random secret; hard minimum remains 32.
pub const UPDATE_TOKEN_WARN_BELOW_LEN: usize = 40;

/// Minimum accepted length for the host-policy self-update capability.
pub const GUARD_SELF_UPDATE_TOKEN_MIN_LEN: usize = 32;

/// How PostgreSQL is deployed relative to the compose stack.
///
/// - [`Bundled`](DbMode::Bundled) (default): postgres runs in compose with `./pgdata`;
///   updater snapshots/restores that path on update/rollback.
/// - [`External`](DbMode::External): DB lives outside compose; never require or touch
///   `UPDATER_PGDATA` / `./pgdata` during update or rollback (image tags only).
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DbMode {
    #[default]
    Bundled,
    External,
}

impl DbMode {
    pub fn as_str(self) -> &'static str {
        match self {
            DbMode::Bundled => "bundled",
            DbMode::External => "external",
        }
    }

    /// Whether the update flow should snapshot/restore `pgdata`.
    pub fn pgdata_snapshot_enabled(self) -> bool {
        matches!(self, DbMode::Bundled)
    }

    pub fn is_external(self) -> bool {
        matches!(self, DbMode::External)
    }

    pub fn parse(raw: &str) -> Result<Self, UpdaterError> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "bundled" => Ok(DbMode::Bundled),
            "external" => Ok(DbMode::External),
            other => Err(UpdaterError::Config(format!(
                "unknown MYRIAD_DB_MODE: {other} (expected bundled|external)"
            ))),
        }
    }

    /// Resolve `MYRIAD_DB_MODE`: process env first, then mounted `.env`, else `bundled`.
    ///
    /// Does **not** auto-switch from `DATABASE_URL` host — external mode must be explicit.
    pub fn resolve(env_file: Option<&Path>) -> Result<Self, UpdaterError> {
        if let Ok(raw) = std::env::var("MYRIAD_DB_MODE") {
            if !raw.trim().is_empty() {
                return Self::parse(&raw);
            }
        }
        if let Some(path) = env_file {
            if path.exists() {
                if let Ok(env) = crate::env_file::EnvFile::load(path) {
                    if let Some(raw) = env.get("MYRIAD_DB_MODE") {
                        if !raw.trim().is_empty() {
                            return Self::parse(raw);
                        }
                    }
                }
            }
        }
        Ok(DbMode::Bundled)
    }
}

impl std::fmt::Display for DbMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Required: shared secret for mutating API endpoints. Must be >= 32 chars.
    pub update_token: SecretString,

    /// Host-policy capability used only for the Guard TCB self-update endpoint.
    /// This is intentionally distinct from updater API authentication.
    pub guard_self_update_token: SecretString,

    /// Release channel to track.
    pub channel: Channel,

    /// Owner/repo on GitHub used to fetch release.json.
    pub github_repo: String,

    /// Optional bearer token for GitHub API.
    ///
    /// - **Release mode** prefers GitHub for `release.json` (digests, cosign, min_from).
    ///   When the release asset is missing/private (404/401) or the token is unset, preflight
    ///   allows Docker Hub `vX.Y.Z` only with explicit per-install tag consent.
    ///   Public image installs therefore work without a GitHub Release or token after consent.
    /// - **Commit mode** works without it: discovery uses Docker Hub common frontend/backend
    ///   tags when the token is absent (typical for private source repos that only publish
    ///   images publicly).
    pub github_token: Option<SecretString>,

    /// Optional image mirror prefix (e.g. `mirror.local`). Applied as a rewrite.
    pub registry_mirror: Option<String>,

    /// How often to poll GitHub for new releases. Set to 0 to disable polling.
    pub check_interval_secs: u64,

    /// Cosign signature verification policy. See release::cosign::CosignPolicy.
    /// COSIGN_VERIFY env: off | soft | strict (default: strict).
    ///
    /// `off` alone is refused: also set `UPDATER_ALLOW_INSECURE_COSIGN=true`
    /// (or alias `COSIGN_INSECURE_OK=true`) so disabling verification is intentional.
    pub cosign_verify: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Stable,
    Preview,
}

impl std::fmt::Display for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Channel::Stable => f.write_str("stable"),
            Channel::Preview => f.write_str("preview"),
        }
    }
}

impl std::str::FromStr for Channel {
    type Err = UpdaterError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stable" => Ok(Channel::Stable),
            "preview" => Ok(Channel::Preview),
            other => Err(UpdaterError::Config(format!(
                "unknown channel: {other} (expected stable|preview)"
            ))),
        }
    }
}

/// Wrapper to keep secrets out of Debug/log output by accident.
#[derive(Clone, Serialize, Deserialize)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

impl Config {
    /// True when a non-empty `GITHUB_TOKEN` is configured.
    ///
    /// Without a token, private repos return 404/401; commit-mode metadata should use
    /// Docker Hub instead of spamming GitHub API failures every check interval.
    pub fn github_token_present(&self) -> bool {
        self.github_token
            .as_ref()
            .is_some_and(|t| !t.expose().trim().is_empty())
    }

    pub fn load_from_env() -> Result<Self, UpdaterError> {
        let token = std::env::var("UPDATE_TOKEN")
            .map_err(|_| UpdaterError::Config("UPDATE_TOKEN is required".into()))?;
        if token.trim().len() < UPDATE_TOKEN_MIN_LEN {
            return Err(UpdaterError::Config(format!(
                "UPDATE_TOKEN must be at least {UPDATE_TOKEN_MIN_LEN} characters"
            )));
        }
        if is_weak_token(&token) {
            return Err(UpdaterError::Config(
                "UPDATE_TOKEN appears to be a weak/common value".into(),
            ));
        }

        let guard_self_update_token =
            std::env::var("DOCKER_GUARD_SELF_UPDATE_TOKEN").map_err(|_| {
                UpdaterError::Config("DOCKER_GUARD_SELF_UPDATE_TOKEN is required".into())
            })?;
        if guard_self_update_token.len() < GUARD_SELF_UPDATE_TOKEN_MIN_LEN
            || guard_self_update_token.len() > 256
            || !guard_self_update_token
                .bytes()
                .all(|byte| byte.is_ascii_graphic())
        {
            return Err(UpdaterError::Config(format!(
                "DOCKER_GUARD_SELF_UPDATE_TOKEN must be {GUARD_SELF_UPDATE_TOKEN_MIN_LEN}..=256 printable non-whitespace ASCII characters"
            )));
        }

        let channel: Channel = std::env::var("CHANNEL")
            .unwrap_or_else(|_| "stable".into())
            .parse()?;

        let github_repo =
            std::env::var("MYRIAD_GITHUB_REPO").unwrap_or_else(|_| "Myriad-You/Myriad".into());

        let github_token = optional_secret(std::env::var("GITHUB_TOKEN").ok());

        let registry_mirror = std::env::var("REGISTRY_MIRROR")
            .ok()
            .filter(|s| !s.trim().is_empty());

        let check_interval_secs: u64 = std::env::var("CHECK_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3600);

        let cosign_verify = std::env::var("COSIGN_VERIFY").unwrap_or_else(|_| "strict".into());
        validate_cosign_off_dual_key(
            &cosign_verify,
            env_truthy("UPDATER_ALLOW_INSECURE_COSIGN"),
            env_truthy("COSIGN_INSECURE_OK"),
        )?;

        Ok(Self {
            update_token: SecretString::new(token),
            guard_self_update_token: SecretString::new(guard_self_update_token),
            channel,
            github_repo,
            github_token,
            registry_mirror,
            check_interval_secs,
            cosign_verify,
        })
    }
}

/// True when `COSIGN_VERIFY` disables verification (`off` / `false` / `0`).
pub fn cosign_verify_is_off(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "off" | "false" | "0"
    )
}

/// Fail closed when cosign is disabled without an explicit second allow key.
///
/// Accepts either `UPDATER_ALLOW_INSECURE_COSIGN` or alias `COSIGN_INSECURE_OK`.
pub fn validate_cosign_off_dual_key(
    cosign_verify: &str,
    allow_insecure: bool,
    insecure_ok_alias: bool,
) -> Result<(), UpdaterError> {
    if cosign_verify_is_off(cosign_verify) && !(allow_insecure || insecure_ok_alias) {
        return Err(UpdaterError::Config(
            "COSIGN_VERIFY=off requires UPDATER_ALLOW_INSECURE_COSIGN=true \
             (or COSIGN_INSECURE_OK=true) to acknowledge supply-chain risk. \
             Prefer COSIGN_VERIFY=strict (default)."
                .into(),
        ));
    }
    Ok(())
}

fn env_truthy(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .as_deref()
        .map(is_truthy)
        .unwrap_or(false)
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn is_weak_token(s: &str) -> bool {
    const WEAK: &[&str] = &[
        "admin",
        "password",
        "secret",
        "changeme",
        "test",
        "00000000000000000000000000000000",
        "11111111111111111111111111111111",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ];
    let lower = s.to_ascii_lowercase();
    WEAK.iter().any(|w| lower.contains(w))
}

fn optional_secret(value: Option<String>) -> Option<SecretString> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(SecretString::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_parse() {
        assert_eq!("stable".parse::<Channel>().unwrap(), Channel::Stable);
        assert!("foo".parse::<Channel>().is_err());
    }

    #[test]
    fn weak_token_detection() {
        assert!(is_weak_token("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
        assert!(is_weak_token("MyPassword12345678901234567890123"));
        assert!(!is_weak_token("9xQ3vN8mP2rT5wY7zA1bC4dF6hJ8kL0n"));
    }

    #[test]
    fn optional_secret_ignores_empty_values() {
        assert!(optional_secret(None).is_none());
        assert!(optional_secret(Some("".into())).is_none());
        assert!(optional_secret(Some("   ".into())).is_none());
        assert_eq!(
            optional_secret(Some(" token-value ".into()))
                .expect("token")
                .expose(),
            "token-value"
        );
    }

    #[test]
    fn github_token_present_requires_nonempty() {
        let mut cfg = Config {
            update_token: SecretString::new("9xQ3vN8mP2rT5wY7zA1bC4dF6hJ8kL0n"),
            guard_self_update_token: SecretString::new("g7N2pQ8xV4mK6rT9wY3zA5bC1dF0hJ8l"),
            channel: Channel::Stable,
            github_repo: "Myriad-You/Myriad".into(),
            github_token: None,
            registry_mirror: None,
            check_interval_secs: 3600,
            cosign_verify: "off".into(),
        };
        assert!(!cfg.github_token_present());
        cfg.github_token = Some(SecretString::new("   "));
        assert!(!cfg.github_token_present());
        cfg.github_token = Some(SecretString::new("ghp_example"));
        assert!(cfg.github_token_present());
    }

    #[test]
    fn cosign_off_requires_dual_key() {
        assert!(cosign_verify_is_off("off"));
        assert!(cosign_verify_is_off("OFF"));
        assert!(cosign_verify_is_off("false"));
        assert!(cosign_verify_is_off("0"));
        assert!(!cosign_verify_is_off("strict"));
        assert!(!cosign_verify_is_off("soft"));

        assert!(validate_cosign_off_dual_key("off", false, false).is_err());
        assert!(validate_cosign_off_dual_key("off", true, false).is_ok());
        assert!(validate_cosign_off_dual_key("off", false, true).is_ok());
        assert!(validate_cosign_off_dual_key("strict", false, false).is_ok());
        assert!(validate_cosign_off_dual_key("soft", false, false).is_ok());
    }

    #[test]
    fn is_truthy_values() {
        assert!(is_truthy("true"));
        assert!(is_truthy("YES"));
        assert!(is_truthy("1"));
        assert!(is_truthy("on"));
        assert!(!is_truthy("false"));
        assert!(!is_truthy(""));
        assert!(!is_truthy("maybe"));
    }

    #[test]
    fn db_mode_parse() {
        assert_eq!(DbMode::parse("bundled").unwrap(), DbMode::Bundled);
        assert_eq!(DbMode::parse("BUNDLED").unwrap(), DbMode::Bundled);
        assert_eq!(DbMode::parse("external").unwrap(), DbMode::External);
        assert_eq!(DbMode::parse("").unwrap(), DbMode::Bundled);
        assert!(DbMode::parse("managed").is_err());
        assert!(DbMode::Bundled.pgdata_snapshot_enabled());
        assert!(!DbMode::External.pgdata_snapshot_enabled());
        assert!(DbMode::External.is_external());
        assert!(!DbMode::Bundled.is_external());
    }

    /// `MYRIAD_DB_MODE` is process-global, so the tests that read or write it
    /// must not run concurrently. Without this they interleave: one clears the
    /// var while another has just set it, and `db_mode_resolve_from_env_file`
    /// intermittently sees `bundled` instead of the file's `external`.
    fn db_mode_env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        // A poisoned lock only means some other test panicked; the env state is
        // reset by each test anyway, so recover rather than cascade failures.
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn db_mode_resolve_defaults_bundled() {
        let _guard = db_mode_env_lock();
        // Unset process env + missing file → bundled.
        std::env::remove_var("MYRIAD_DB_MODE");
        let missing = std::path::Path::new("/tmp/myriad-db-mode-missing-env-xyz");
        assert_eq!(DbMode::resolve(Some(missing)).unwrap(), DbMode::Bundled);
        assert_eq!(DbMode::resolve(None).unwrap(), DbMode::Bundled);
    }

    #[test]
    fn db_mode_resolve_from_env_file() {
        let _guard = db_mode_env_lock();
        std::env::remove_var("MYRIAD_DB_MODE");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "MYRIAD_DB_MODE=external\nMYRIAD_TAG=v1.0.0\n").unwrap();
        assert_eq!(DbMode::resolve(Some(&path)).unwrap(), DbMode::External);
    }

    #[test]
    fn db_mode_process_env_overrides_file() {
        let _guard = db_mode_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "MYRIAD_DB_MODE=external\n").unwrap();
        std::env::set_var("MYRIAD_DB_MODE", "bundled");
        let got = DbMode::resolve(Some(&path));
        std::env::remove_var("MYRIAD_DB_MODE");
        assert_eq!(got.unwrap(), DbMode::Bundled);
    }
}
