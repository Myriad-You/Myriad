//! Environment probe. Run at startup. Refuses to operate on environments we don't support
//! safely (named volumes, rootless docker, podman shim, missing compose binary, etc.).
//!
//! Probes are pure functions of the host environment plus a small set of paths from the CLI.
//! Result is serialized into `state/env-probe.json` so users can attach it to bug reports.

pub mod compose;
pub mod docker;
pub mod filesystem;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::DbMode;
use crate::error::Result;

pub struct ProbeInputs {
    pub state_dir: PathBuf,
    pub compose_dir: PathBuf,
    pub env_file: PathBuf,
    pub pgdata: PathBuf,
    /// Resolved `MYRIAD_DB_MODE` (bundled default).
    pub db_mode: DbMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvProbe {
    pub schema_version: u32,
    pub compose: compose::ComposeProbe,
    pub docker: docker::DockerProbe,
    pub pgdata: filesystem::PgdataProbe,
    pub env_file: filesystem::EnvFileProbe,
    /// `bundled` | `external` — see `MYRIAD_DB_MODE`.
    #[serde(default = "default_db_mode_str")]
    pub db_mode: String,
    /// Whether update flow will snapshot/restore `pgdata` (false when external).
    #[serde(default = "default_pgdata_snapshot_enabled")]
    pub pgdata_snapshot_enabled: bool,
    #[serde(default)]
    pub fatal: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

fn default_db_mode_str() -> String {
    DbMode::Bundled.as_str().to_string()
}

fn default_pgdata_snapshot_enabled() -> bool {
    true
}

impl EnvProbe {
    pub fn fatal_error(&self) -> Option<&str> {
        self.fatal.first().map(|s| s.as_str())
    }
    pub fn warnings(&self) -> impl Iterator<Item = &str> {
        self.warnings.iter().map(|s| s.as_str())
    }
}

pub async fn run_all(inputs: &ProbeInputs) -> Result<EnvProbe> {
    let mut fatal = Vec::new();
    let mut warnings = Vec::new();
    let db_mode = inputs.db_mode;
    let pgdata_snapshot_enabled = db_mode.pgdata_snapshot_enabled();

    let compose = compose::probe(&inputs.compose_dir).await;
    if let Some(e) = &compose.error {
        fatal.push(format!("compose: {e}"));
    }
    let docker_probe = docker::probe().await;
    if let Some(e) = &docker_probe.error {
        fatal.push(format!("docker: {e}"));
    }
    if docker_probe.rootless {
        fatal.push("rootless docker is not supported in M1".into());
    }
    if docker_probe.is_podman {
        fatal.push("podman is not supported in M1".into());
    }

    let pgdata = filesystem::probe_pgdata(&inputs.pgdata, &inputs.state_dir).await;
    let (pg_fatal, pg_warn) = classify_pgdata(&pgdata, &inputs.pgdata, db_mode);
    fatal.extend(pg_fatal);
    warnings.extend(pg_warn);

    let env_file = filesystem::probe_env_file(&inputs.env_file).await;
    if !env_file.exists {
        fatal.push(format!(
            ".env file {} not mounted into updater",
            inputs.env_file.display()
        ));
    } else if env_file.duplicate_keys {
        fatal.push(".env contains duplicate keys; refusing to manage".into());
    } else if !env_file.has_required_tag_vars {
        fatal.push(
            ".env is missing MYRIAD_TAG / PROXY_TAG / UPDATER_TAG; cannot perform updates".into(),
        );
    }

    // Optional heuristic only: never auto-switch mode. Warn when URL host is clearly
    // not the compose service name `postgres` while still in bundled mode.
    if db_mode == DbMode::Bundled
        && let Some(w) = database_url_bundled_mismatch_warning(&inputs.env_file) {
            warnings.push(w);
        }

    if pgdata_snapshot_enabled && pgdata.cross_device {
        warnings.push(format!(
            "pgdata is on a different filesystem from {}/snapshots; rename rollback unavailable, falling back to copy",
            inputs.state_dir.display()
        ));
    }
    if let Some(skew) = docker_probe.daemon_time_skew_seconds
        && skew.abs() > 300 {
            warnings.push(format!(
                "docker daemon clock skew is {skew}s; TLS or rate-limit issues may appear"
            ));
        }

    Ok(EnvProbe {
        schema_version: 1,
        compose,
        docker: docker_probe,
        pgdata,
        env_file,
        db_mode: db_mode.as_str().to_string(),
        pgdata_snapshot_enabled,
        fatal,
        warnings,
    })
}

/// Classify pgdata probe outcomes into fatal vs warning strings.
///
/// Missing path is warning-only for bundled mode (updater may boot before first postgres init).
/// Named-volume remains fatal when the path exists and is flagged (bundled only).
/// External mode never requires local pgdata — skip snapshot-related fatals/warnings.
fn classify_pgdata(
    pgdata: &filesystem::PgdataProbe,
    path: &std::path::Path,
    db_mode: DbMode,
) -> (Vec<String>, Vec<String>) {
    let mut fatal = Vec::new();
    let mut warnings = Vec::new();

    if db_mode.is_external() {
        // External DB: local pgdata is unused for update/rollback. Surface probe errors
        // only as soft warnings so operators can still inspect the path if mounted.
        if let Some(e) = &pgdata.error {
            warnings.push(format!("pgdata (ignored in external mode): {e}"));
        }
        return (fatal, warnings);
    }

    if let Some(e) = &pgdata.error {
        fatal.push(format!("pgdata: {e}"));
    }
    // Named-volume remains fatal only when the path exists and is flagged.
    if pgdata.exists && pgdata.is_named_volume {
        fatal.push(
            "pgdata appears to be a docker named volume; M1 requires a bind mount. \
             See docs/updater-spec.md §19 for migration steps."
                .into(),
        );
    }
    // Missing pgdata is a warning, not fatal: updater can still boot, serve status,
    // pull images, and manage non-DB updates. Snapshot/restore require the path later.
    if !pgdata.exists {
        warnings.push(format!(
            "pgdata path {} does not exist inside the updater container; \
             snapshot and rollback restore will fail until the path is mounted \
             (normal on first boot before postgres has created data)",
            path.display()
        ));
    }
    (fatal, warnings)
}

/// If `DATABASE_URL` host is set and is not the compose service name `postgres`,
/// return a warning when running in bundled mode. Never changes mode.
fn database_url_bundled_mismatch_warning(env_file: &std::path::Path) -> Option<String> {
    let env = crate::env_file::EnvFile::load(env_file).ok()?;
    let url = env.get("DATABASE_URL")?;
    let host = database_url_host(url)?;
    if host.eq_ignore_ascii_case("postgres") {
        return None;
    }
    Some(format!(
        "DATABASE_URL host is `{host}` but MYRIAD_DB_MODE=bundled (default); \
         if Postgres is outside compose, set MYRIAD_DB_MODE=external so updater skips pgdata snapshots"
    ))
}

/// Extract hostname from a `postgres://` / `postgresql://` URL (best-effort, no secrets).
pub fn database_url_host(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Prefer url crate when scheme is present.
    if let Ok(parsed) = url::Url::parse(trimmed)
        && let Some(host) = parsed.host_str()
            && !host.is_empty() {
                return Some(host.to_string());
            }
    // Fallback: postgres://user:pass@host:port/db without full parse.
    let rest = trimmed
        .strip_prefix("postgres://")
        .or_else(|| trimmed.strip_prefix("postgresql://"))?;
    let after_at = rest.rsplit('@').next()?;
    let host = after_at.split(&[':', '/'][..]).next()?.trim();
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::filesystem::PgdataProbe;
    use std::path::Path;

    fn empty_probe(exists: bool) -> PgdataProbe {
        PgdataProbe {
            exists,
            is_named_volume: false,
            fs_type: None,
            device_id: None,
            cross_device: false,
            size_bytes: None,
            free_bytes_on_fs: None,
            error: None,
        }
    }

    #[test]
    fn missing_pgdata_is_warning_not_fatal() {
        let (fatal, warnings) = classify_pgdata(
            &empty_probe(false),
            Path::new("/host/compose/pgdata"),
            DbMode::Bundled,
        );
        assert!(
            fatal.is_empty(),
            "missing path must not be fatal: {fatal:?}"
        );
        assert!(
            warnings.iter().any(|w| w.contains("does not exist")),
            "expected warning: {warnings:?}"
        );
    }

    #[test]
    fn present_pgdata_no_fatal_from_exists() {
        let (fatal, warnings) = classify_pgdata(
            &empty_probe(true),
            Path::new("/host/compose/pgdata"),
            DbMode::Bundled,
        );
        assert!(fatal.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn named_volume_still_fatal_when_exists() {
        let mut p = empty_probe(true);
        p.is_named_volume = true;
        let (fatal, _) = classify_pgdata(&p, Path::new("/host/compose/pgdata"), DbMode::Bundled);
        assert!(
            fatal.iter().any(|f| f.contains("named volume")),
            "expected named-volume fatal: {fatal:?}"
        );
    }

    #[test]
    fn named_volume_flag_ignored_when_missing() {
        let mut p = empty_probe(false);
        p.is_named_volume = true;
        let (fatal, warnings) =
            classify_pgdata(&p, Path::new("/host/compose/pgdata"), DbMode::Bundled);
        assert!(
            fatal.is_empty(),
            "missing path should not fatal on named-volume flag alone: {fatal:?}"
        );
        assert!(!warnings.is_empty());
    }

    #[test]
    fn external_mode_skips_pgdata_requirement_warnings() {
        let (fatal, warnings) = classify_pgdata(
            &empty_probe(false),
            Path::new("/host/compose/pgdata"),
            DbMode::External,
        );
        assert!(fatal.is_empty());
        assert!(
            warnings.is_empty(),
            "external mode must not warn about missing pgdata: {warnings:?}"
        );

        let mut named = empty_probe(true);
        named.is_named_volume = true;
        let (fatal, _) =
            classify_pgdata(&named, Path::new("/host/compose/pgdata"), DbMode::External);
        assert!(
            fatal.is_empty(),
            "external mode must not fatal on named volume: {fatal:?}"
        );
    }

    #[test]
    fn database_url_host_parses() {
        assert_eq!(
            database_url_host("postgres://myriad:secret@db.example.com:5432/myriad").as_deref(),
            Some("db.example.com")
        );
        assert_eq!(
            database_url_host("postgresql://u@postgres:5432/db").as_deref(),
            Some("postgres")
        );
        assert_eq!(database_url_host("").as_deref(), None);
    }
}
