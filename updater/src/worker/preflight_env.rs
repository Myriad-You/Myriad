//! Update-time environment checks that do **not** change compose topology or the update
//! state machine. They only inspect the host/compose view and fail closed **before**
//! maintenance / stop / snapshot.
//!
//! Complements startup `probe::run_all` (which can go stale after a panel edits the stack)
//! and the network allowlist preflight.

use std::path::Path;
use std::sync::Arc;

use tracing::info;

use crate::config::DbMode;
use crate::env_file::EnvFile;
use crate::error::{Result, UpdaterError};

use crate::worker::Worker;

/// Fixed `container_name` values assumed by production compose, network preflight, and
/// health probes. Panels that rewrite these names will break stop/up/rollback.
pub const EXPECTED_CONTAINER_NAMES: &[(&str, &str)] = &[
    ("backend", "myriad-backend"),
    ("federation-worker", "myriad-federation-worker"),
    ("persona-worker", "myriad-persona-worker"),
    ("frontend", "myriad-frontend"),
    ("postgres", "myriad-postgres"),
];

/// Compose service names the updater stop/up paths require.
pub fn required_services(db_mode: DbMode) -> Vec<&'static str> {
    let mut s = vec!["backend", "frontend"];
    if !db_mode.is_external() {
        s.push("postgres");
    }
    s
}

/// Local gates independent of `docker compose config` (tag vars, writable paths, Docker API).
pub async fn check_local_environment(worker: &Arc<Worker>) -> Result<()> {
    check_manageable_tag_vars(worker.as_ref())?;
    check_paths_writable(worker.as_ref())?;
    check_docker_api(worker.as_ref()).await?;
    info!("preflight: local environment (tags, paths, docker API) ok");
    Ok(())
}

/// Topology + project label + postgres volume shape from an already-resolved compose config.
/// Call alongside network allowlist checks (same config JSON — no orchestration change).
pub async fn check_compose_contract(
    worker: &Arc<Worker>,
    compose_config: &serde_json::Value,
    project: &str,
    workers: [bool; 2],
) -> Result<()> {
    let db_mode = worker.cli().db_mode;
    check_target_topology(compose_config, db_mode, workers)?;
    if workers[1] {
        check_persona_runtime(compose_config)?;
    }
    check_worker_routes(worker.as_ref(), workers).await?;
    check_postgres_pgdata_volume(compose_config, db_mode)?;
    check_running_compose_project(worker.as_ref(), project).await?;
    info!(
        project = %project,
        "preflight: compose contract (services, container_name, pgdata volume, project labels) ok"
    );
    Ok(())
}

/// The edge is TCB and must already understand split routing before a business
/// image removes these endpoints from web. Inspect the running image and env,
/// not merely the host file an operator may not have applied yet.
async fn check_worker_routes(worker: &Worker, required: [bool; 2]) -> Result<()> {
    if !required.into_iter().any(|enabled| enabled) {
        return Ok(());
    }
    let proxy = worker
        .docker()
        .raw()
        .inspect_container("myriad-proxy", None)
        .await
        .map_err(|error| UpdaterError::Precondition(format!("inspect proxy: {error}")))?;
    let running = proxy.state.as_ref().and_then(|state| state.running) == Some(true);
    let env = proxy.config.as_ref().and_then(|config| config.env.as_ref());
    let image = proxy
        .image
        .as_deref()
        .ok_or_else(|| UpdaterError::Precondition("proxy image identity missing".into()))?;
    let image = worker
        .docker()
        .raw()
        .inspect_image(image)
        .await
        .map_err(|error| {
            UpdaterError::Precondition(format!("inspect proxy capability: {error}"))
        })?;
    let labels = image.config.and_then(|config| config.labels);
    for (required, role, route, capability) in [
        (
            required[0],
            "federation",
            "PROXY_FEDERATION_UPSTREAM=http://federation-worker:1103",
            "io.myriad.proxy.federation-routing",
        ),
        (
            required[1],
            "persona",
            "PROXY_PERSONA_UPSTREAM=http://persona-worker:1103",
            "io.myriad.proxy.persona-routing",
        ),
    ] {
        if !required {
            continue;
        }
        let routed = env.is_some_and(|env| env.iter().any(|value| value == route));
        let capable = labels
            .as_ref()
            .and_then(|labels| labels.get(capability))
            .is_some_and(|value| value == "1");
        if !running || !routed || !capable {
            return Err(UpdaterError::Precondition(format!(
                "upgrade/recreate the proxy TCB with {role}-routing support and {route} before upgrading the backend"
            )));
        }
    }
    Ok(())
}

fn check_persona_runtime(config: &serde_json::Value) -> Result<()> {
    let worker = &config["services"]["persona-worker"];
    let cpu = worker
        .pointer("/deploy/resources/limits/cpus")
        .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()));
    let memory = worker
        .pointer("/deploy/resources/limits/memory")
        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()));
    let pids = worker
        .pointer("/deploy/resources/limits/pids")
        .and_then(serde_json::Value::as_i64);
    if worker["user"] != "1000:1000"
        || worker["read_only"] != true
        || worker["cap_drop"] != serde_json::json!(["ALL"])
        || cpu.is_none_or(|n| n <= 0.0 || n > 1.0)
        || memory.is_none_or(|n| n <= 0 || n > 1_073_741_824)
        || pids.is_none_or(|n| n <= 0 || n > 64)
        || worker["pids_limit"]
            .as_i64()
            .is_none_or(|n| n <= 0 || n > 64)
        || worker["tmpfs"] != serde_json::json!(["/tmp:size=32m,mode=1777"])
        || !worker["security_opt"].as_array().is_some_and(|values| {
            values.iter().any(|v| {
                matches!(
                    v.as_str(),
                    Some("no-new-privileges" | "no-new-privileges:true")
                )
            })
        })
    {
        return Err(UpdaterError::Precondition("persona worker resource or security boundary is missing; migrate Compose before upgrading".into()));
    }

    if worker["command"] != serde_json::json!(["/app/myriad-persona-worker"]) {
        return Err(UpdaterError::Precondition(
            "persona worker command is missing; migrate Compose before upgrading".into(),
        ));
    }
    for (key, expected) in [
        ("MYRIAD_PROCESS_ROLE", "persona-worker"),
        ("DATA_DIR", "/app/data"),
        ("CACHE_DIR", "/app/cache"),
        ("PERSONA_WEB_UPSTREAM", "http://backend:1103"),
    ] {
        if worker["environment"][key].as_str() != Some(expected) {
            return Err(UpdaterError::Precondition(format!(
                "persona worker requires {key}={expected}"
            )));
        }
    }
    let mounts = worker["volumes"].as_array().ok_or_else(|| {
        UpdaterError::Precondition("persona worker data/cache volumes missing".into())
    })?;
    if mounts.len() != 2
        || [
            ("backend_data", "/app/data"),
            ("backend_cache", "/app/cache"),
        ]
        .iter()
        .any(|(source, target)| {
            !mounts.iter().any(|m| {
                m["type"] == "volume"
                    && m["source"] == *source
                    && m["target"] == *target
                    && m["read_only"] != true
                    && m.pointer("/volume/subpath").is_none()
            })
        })
    {
        return Err(UpdaterError::Precondition(
            "persona worker requires exactly the writable first-party data/cache volumes".into(),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tag / path / docker API
// ---------------------------------------------------------------------------

fn check_manageable_tag_vars(worker: &Worker) -> Result<()> {
    let env = EnvFile::load(&worker.cli().env_file)?;
    let mut missing = Vec::new();
    for k in ["MYRIAD_TAG", "PROXY_TAG", "UPDATER_TAG"] {
        if env.get(k).is_none() {
            missing.push(k.to_string());
        }
    }
    if !missing.is_empty() {
        return Err(UpdaterError::Precondition(format!(
            ".env is missing {}; updater cannot safely swap tags. Fix {} before updating.",
            missing.join(", "),
            worker.cli().env_file.display()
        )));
    }

    Ok(())
}

fn check_paths_writable(worker: &Worker) -> Result<()> {
    let state = &worker.cli().state_dir;
    assert_writable_dir(state, "state dir")?;
    // Job history / audit always need state; snapshots only when bundled.
    if !worker.cli().db_mode.is_external() {
        let snaps = state.join("snapshots");
        assert_writable_dir(&snaps, "snapshot dir")?;
    }
    // Tag swap rewrites .env in place.
    assert_writable_file(&worker.cli().env_file, ".env")?;
    if !worker.cli().db_mode.is_external() {
        // Snapshot copies into state; pgdata itself must be readable (write not required until restore).
        let pg = &worker.cli().pgdata;
        if crate::probe::filesystem::path_is_present(pg)? {
            assert_readable_dir(pg, "pgdata")?;
        }
    }
    Ok(())
}

fn assert_writable_dir(path: &Path, label: &str) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|e| {
        UpdaterError::Precondition(format!(
            "{label} {} is not creatable/writable: {e}",
            path.display()
        ))
    })?;
    let probe = path.join(format!(".myriad-preflight-write.{}", std::process::id()));
    std::fs::write(&probe, b"ok").map_err(|e| {
        UpdaterError::Precondition(format!(
            "{label} {} is not writable: {e}. Check the writable state mount.",
            path.display()
        ))
    })?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

fn assert_readable_dir(path: &Path, label: &str) -> Result<()> {
    let meta = std::fs::metadata(path).map_err(|e| {
        UpdaterError::Precondition(format!("{label} {} is not readable: {e}", path.display()))
    })?;
    if !meta.is_dir() {
        return Err(UpdaterError::Precondition(format!(
            "{label} {} is not a directory",
            path.display()
        )));
    }
    // Best-effort: list one entry to catch permission denials.
    let _ = std::fs::read_dir(path).map_err(|e| {
        UpdaterError::Precondition(format!(
            "{label} {} cannot be listed (permission?): {e}",
            path.display()
        ))
    })?;
    Ok(())
}

fn assert_writable_file(path: &Path, label: &str) -> Result<()> {
    if !crate::probe::filesystem::path_is_present(path)? {
        return Err(UpdaterError::Precondition(format!(
            "{label} {} does not exist",
            path.display()
        )));
    }
    use std::fs::OpenOptions;
    OpenOptions::new().append(true).open(path).map_err(|e| {
        UpdaterError::Precondition(format!(
            "{label} {} is not writable: {e}. Tag swap requires a writable .env on the \
                 deployment-root bind.",
            path.display()
        ))
    })?;
    Ok(())
}

async fn check_docker_api(worker: &Worker) -> Result<()> {
    worker.docker().raw().ping().await.map_err(|e| {
        UpdaterError::Precondition(format!(
            "Docker API unreachable ({e}). Updater talks through docker-guard \
             (DOCKER_HOST); fix guard health / myriad-docker-guard-net before updating \
             so compose recreate is possible."
        ))
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Compose config contract (pure + running labels)
// ---------------------------------------------------------------------------

/// Fail if required services missing or `container_name` rewritten away from production names.
#[cfg(test)]
pub fn check_compose_topology(config: &serde_json::Value, db_mode: DbMode) -> Result<()> {
    check_target_topology(config, db_mode, [true, true])
}

fn check_target_topology(
    config: &serde_json::Value,
    db_mode: DbMode,
    workers: [bool; 2],
) -> Result<()> {
    let Some(services) = config.get("services").and_then(|v| v.as_object()) else {
        return Err(UpdaterError::Precondition(
            "compose config has no services map; refusing update".into(),
        ));
    };

    // Reject before maintenance/stop: a new image must never discover the
    // missing role only after the running installation has been taken offline.
    if workers.iter().any(|required| *required)
        && config
            .pointer("/services/backend/environment/MYRIAD_PROCESS_ROLE")
            .and_then(serde_json::Value::as_str)
            != Some("web")
    {
        return Err(UpdaterError::Precondition(
            "migrate Compose and updater/Guard before upgrading: backend requires explicit MYRIAD_PROCESS_ROLE=web, federation-worker and persona-worker; implicit combined execution is no longer supported".into(),
        ));
    }
    let mut required = required_services(db_mode);
    if workers[0] {
        required.push("federation-worker");
    }
    if workers[1] {
        required.push("persona-worker");
    }
    let mut missing_svc = Vec::new();
    let mut name_issues = Vec::new();

    for svc in &required {
        let Some(def) = services.get(*svc) else {
            missing_svc.push(*svc);
            continue;
        };
        let expected = EXPECTED_CONTAINER_NAMES
            .iter()
            .find(|(s, _)| *s == *svc)
            .map(|(_, n)| *n);
        let Some(expected) = expected else {
            continue;
        };
        match def.get("container_name").and_then(|v| v.as_str()) {
            Some(name) if name == expected => {}
            Some(name) => name_issues.push(format!(
                "{svc} container_name={name:?} (expected {expected:?})"
            )),
            None => name_issues.push(format!(
                "{svc} has no container_name (expected {expected:?}; \
                 preflight and health probes use fixed names)"
            )),
        }
    }

    if !missing_svc.is_empty() {
        return Err(UpdaterError::Precondition(format!(
            "compose is missing required service(s): {}. \
             Updater stop/up paths expect these names — do not rename services in a panel UI.",
            missing_svc.join(", ")
        )));
    }
    if !name_issues.is_empty() {
        return Err(UpdaterError::Precondition(format!(
            "compose container_name contract broken: {}. \
             Restore official container_name values before updating.",
            name_issues.join("; ")
        )));
    }
    Ok(())
}

/// Bundled postgres must bind-mount host pgdata (not a Docker named volume).
pub fn check_postgres_pgdata_volume(config: &serde_json::Value, db_mode: DbMode) -> Result<()> {
    if db_mode.is_external() {
        return Ok(());
    }
    let Some(pg) = config.get("services").and_then(|s| s.get("postgres")) else {
        return Ok(()); // topology check already fails if missing
    };

    let volumes = match pg.get("volumes") {
        Some(serde_json::Value::Array(a)) => a,
        _ => {
            return Err(UpdaterError::Precondition(
                "postgres service has no volumes; bundled mode requires a host bind for pgdata \
                 (./pgdata → /var/lib/postgresql). Named volumes cannot be snapshotted."
                    .into(),
            ));
        }
    };

    let mut saw_pgdata_mount = false;
    for vol in volumes {
        if let Some(s) = vol.as_str() {
            // Short syntax still appears rarely; compose config usually expands to objects.
            if short_volume_targets_pgdata(s) {
                saw_pgdata_mount = true;
                if short_volume_is_named(s) {
                    return Err(UpdaterError::Precondition(format!(
                        "postgres volume {s:?} looks like a named volume; M1 requires a bind \
                         mount (e.g. ./pgdata:/var/lib/postgresql) for snapshots/rollback"
                    )));
                }
            }
            continue;
        }
        let Some(obj) = vol.as_object() else {
            continue;
        };
        let target = obj
            .get("target")
            .or_else(|| obj.get("destination"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !is_pgdata_target(target) {
            continue;
        }
        saw_pgdata_mount = true;
        let typ = obj.get("type").and_then(|v| v.as_str()).unwrap_or("bind");
        if typ == "volume" {
            let source = obj
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("<named>");
            return Err(UpdaterError::Precondition(format!(
                "postgres pgdata mount is a Docker named volume ({source} → {target}); \
                 updater snapshots require a host bind mount under the deployment root \
                 (official: ./pgdata:/var/lib/postgresql). Fix compose before updating."
            )));
        }
    }

    if !saw_pgdata_mount {
        return Err(UpdaterError::Precondition(
            "postgres has no volume targeting /var/lib/postgresql; \
             bundled snapshots need a bind-mounted pgdata path"
                .into(),
        ));
    }
    Ok(())
}

fn is_pgdata_target(target: &str) -> bool {
    let t = target.trim_end_matches('/');
    t == "/var/lib/postgresql"
        || t == "/var/lib/postgresql/data"
        || t.starts_with("/var/lib/postgresql/")
}

fn short_volume_targets_pgdata(spec: &str) -> bool {
    // "src:dst" or "src:dst:mode"
    let parts: Vec<&str> = spec.split(':').collect();
    if parts.len() < 2 {
        return false;
    }
    // Last path-like segment before optional mode is target when 2+ parts.
    // Heuristic: if any part after first is pgdata path.
    parts.iter().skip(1).any(|p| is_pgdata_target(p))
}

fn short_volume_is_named(spec: &str) -> bool {
    let src = spec.split(':').next().unwrap_or("");
    // Bind sources are absolute or relative paths; named volumes are bare identifiers.
    !src.is_empty() && !src.starts_with('.') && !src.starts_with('/') && !src.contains('/')
}

/// Container names to inspect for project-label / network checks.
pub fn running_check_containers(db_mode: DbMode) -> Vec<&'static str> {
    EXPECTED_CONTAINER_NAMES
        .iter()
        .filter(|(svc, _)| !db_mode.is_external() || *svc != "postgres")
        .map(|(_, name)| *name)
        .collect()
}

/// Running business containers must belong to the same Compose project updater will drive.
async fn check_running_compose_project(worker: &Worker, project: &str) -> Result<()> {
    let containers = running_check_containers(worker.cli().db_mode);
    let mut bad = Vec::new();
    for name in containers {
        let info = match worker.docker().raw().inspect_container(name, None).await {
            Ok(info) => info,
            Err(_) => continue,
        };
        let labels = info.config.and_then(|c| c.labels).unwrap_or_default();
        match labels.get("com.docker.compose.project") {
            Some(p) if p == project => {}
            Some(p) => bad.push(format!(
                "{name} com.docker.compose.project={p:?} (expected {project:?})"
            )),
            None => {
                // Not compose-managed or labels stripped — still dangerous for `compose up`.
                bad.push(format!(
                    "{name} has no com.docker.compose.project label (expected {project:?})"
                ));
            }
        }
    }
    if bad.is_empty() {
        return Ok(());
    }
    Err(UpdaterError::Precondition(format!(
        "running container(s) are not in Compose project {project:?}: {}. \
         Set COMPOSE_PROJECT_NAME={project} (and avoid panel renames) so docker-guard \
         and compose target the same stack.",
        bad.join("; ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    #[ignore = "requires Docker Compose CLI; no daemon or network needed"]
    fn v053_published_compose_satisfies_business_upgrade_contract() {
        let dir = tempfile::tempdir().unwrap();
        let compose = dir.path().join("docker-compose.yml");
        std::fs::write(
            &compose,
            include_str!("../../testdata/v0.5.3/docker-compose.yml"),
        )
        .unwrap();
        let output = std::process::Command::new("docker")
            .args(["compose", "-p", "myriad", "-f"])
            .arg(&compose)
            .args(["config", "--format", "json"])
            .envs([
                ("MYRIAD_TAG", "v0.5.3"),
                ("UPDATER_TAG", "v0.5.3"),
                ("PROXY_TAG", "v0.5.3"),
                ("MYRIAD_SETUP_SECRET", "test"),
                ("GUARD_SELF_UPDATE_TOKEN", "test"),
                ("PERSONA_DB_PASSWORD", "test"),
                ("FEDERATION_DB_PASSWORD", "test"),
                ("POSTGRES_PASSWORD", "test"),
                ("UPDATE_TOKEN", "test"),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let model: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        check_target_topology(&model, DbMode::Bundled, [true, true]).unwrap();
        check_postgres_pgdata_volume(&model, DbMode::Bundled).unwrap();
        check_persona_runtime(&model).unwrap();
    }

    #[test]
    fn combined_backend_target_does_not_require_split_workers() {
        let config = json!({"services": {
            "backend": {"container_name":"myriad-backend"},
            "frontend": {"container_name":"myriad-frontend"}
        }});
        assert!(check_target_topology(&config, DbMode::External, [false, false]).is_ok());
        assert!(check_target_topology(&config, DbMode::External, [true, true]).is_err());
    }

    #[test]
    fn topology_ok_for_official_names() {
        let cfg = json!({
            "services": {
                "backend": { "container_name": "myriad-backend", "environment": {"MYRIAD_PROCESS_ROLE": "web"} },
                "federation-worker": {"container_name": "myriad-federation-worker"},
                "persona-worker": {"container_name": "myriad-persona-worker"},
                "frontend": { "container_name": "myriad-frontend" },
                "postgres": { "container_name": "myriad-postgres" }
            }
        });
        assert!(check_compose_topology(&cfg, DbMode::Bundled).is_ok());
    }

    #[test]
    fn legacy_topology_is_rejected_before_maintenance() {
        let config = json!({"services": {
            "backend": {"container_name": "myriad-backend"},
            "frontend": {"container_name": "myriad-frontend"},
            "postgres": {"container_name": "myriad-postgres"}
        }});
        let error = check_compose_topology(&config, DbMode::Bundled).unwrap_err();
        assert!(error.to_string().contains("migrate Compose"), "{error}");
    }

    #[test]
    fn split_topology_requires_both_roles_and_exact_worker_name() {
        let mut config = json!({"services": {
            "backend": {"container_name": "myriad-backend", "environment": {"MYRIAD_PROCESS_ROLE": "web"}},
            "frontend": {"container_name": "myriad-frontend"}
        }});
        assert!(check_compose_topology(&config, DbMode::External).is_err());
        config["services"]["federation-worker"] =
            json!({"container_name": "myriad-federation-worker"});
        assert!(check_compose_topology(&config, DbMode::External).is_err());
        config["services"]["persona-worker"] = json!({"container_name": "myriad-persona-worker"});
        assert!(check_compose_topology(&config, DbMode::External).is_ok());
        config["services"]["federation-worker"]["container_name"] = json!("other-worker");
        assert!(check_compose_topology(&config, DbMode::External).is_err());
        config["services"]["federation-worker"]["container_name"] =
            json!("myriad-federation-worker");
        config["services"]["backend"]["environment"]["MYRIAD_PROCESS_ROLE"] = json!("all");
        assert!(check_compose_topology(&config, DbMode::External).is_err());
    }

    #[test]
    fn topology_rejects_renamed_container() {
        let cfg = json!({
            "services": {
                "backend": { "container_name": "bt-backend", "environment": {"MYRIAD_PROCESS_ROLE": "web"} },
                "federation-worker": {"container_name": "myriad-federation-worker"},
                "persona-worker": {"container_name": "myriad-persona-worker"},
                "frontend": { "container_name": "myriad-frontend" },
                "postgres": { "container_name": "myriad-postgres" }
            }
        });
        let err = check_compose_topology(&cfg, DbMode::Bundled).unwrap_err();
        assert!(err.to_string().contains("container_name"), "{err}");
    }

    #[test]
    fn topology_external_skips_postgres_service() {
        let cfg = json!({
            "services": {
                "backend": { "container_name": "myriad-backend", "environment": {"MYRIAD_PROCESS_ROLE": "web"} },
                "federation-worker": {"container_name": "myriad-federation-worker"},
                "persona-worker": {"container_name": "myriad-persona-worker"},
                "frontend": { "container_name": "myriad-frontend" }
            }
        });
        assert!(check_compose_topology(&cfg, DbMode::External).is_ok());
        assert!(check_compose_topology(&cfg, DbMode::Bundled).is_err());
    }

    #[test]
    fn pgdata_bind_ok() {
        let cfg = json!({
            "services": {
                "postgres": {
                    "volumes": [{
                        "type": "bind",
                        "source": "/host/compose/pgdata",
                        "target": "/var/lib/postgresql"
                    }]
                }
            }
        });
        assert!(check_postgres_pgdata_volume(&cfg, DbMode::Bundled).is_ok());
    }

    #[test]
    fn pgdata_named_volume_rejected() {
        let cfg = json!({
            "services": {
                "postgres": {
                    "volumes": [{
                        "type": "volume",
                        "source": "pgdata",
                        "target": "/var/lib/postgresql"
                    }]
                }
            }
        });
        let err = check_postgres_pgdata_volume(&cfg, DbMode::Bundled).unwrap_err();
        assert!(err.to_string().contains("named volume"), "{err}");
    }

    #[test]
    fn pgdata_external_skips() {
        let cfg = json!({ "services": {} });
        assert!(check_postgres_pgdata_volume(&cfg, DbMode::External).is_ok());
    }

    #[test]
    fn short_volume_helpers() {
        assert!(short_volume_targets_pgdata("./pgdata:/var/lib/postgresql"));
        assert!(!short_volume_is_named("./pgdata:/var/lib/postgresql"));
        assert!(short_volume_is_named("pgdata:/var/lib/postgresql"));
    }

    #[test]
    fn external_running_check_skips_postgres_container() {
        let bundled = running_check_containers(DbMode::Bundled);
        assert!(bundled.contains(&"myriad-postgres"));
        let external = running_check_containers(DbMode::External);
        assert!(!external.contains(&"myriad-postgres"));
        assert!(external.contains(&"myriad-backend"));
    }

    #[test]
    fn compose_discovery_reports_io_faults_as_precondition() {
        // ENOTDIR (a file used as a directory) is not NotFound, so it must not be
        // swallowed as absence nor surfaced as an Internal/HTTP 500 error.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-dir");
        std::fs::write(&file, "x").unwrap();
        let err = crate::probe::compose::collect_compose_files(&file)
            .map_err(UpdaterError::Precondition)
            .unwrap_err();
        assert!(matches!(err, UpdaterError::Precondition(_)), "got {err}");
    }
}

#[cfg(test)]
mod persona_runtime_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn persona_runtime_contract_rejects_broken_storage_and_unbounded_resources() {
        let config = json!({"services":{"persona-worker":{
            "command":["/app/myriad-persona-worker"],"user":"1000:1000","read_only":true,"cap_drop":["ALL"],
            "security_opt":["no-new-privileges:true"],"tmpfs":["/tmp:size=32m,mode=1777"],"pids_limit":64,
            "deploy":{"resources":{"limits":{"cpus":1,"memory":"1073741824","pids":64}}},
            "environment":{"MYRIAD_PROCESS_ROLE":"persona-worker","DATA_DIR":"/app/data","CACHE_DIR":"/app/cache","PERSONA_WEB_UPSTREAM":"http://backend:1103"},
            "volumes":[{"type":"volume","source":"backend_data","target":"/app/data","read_only":false},{"type":"volume","source":"backend_cache","target":"/app/cache"}]
        }}});
        assert!(check_persona_runtime(&config).is_ok());
        for (path, value) in [
            ("/command", json!(["/app/myriad-backend"])),
            ("/environment/MYRIAD_PROCESS_ROLE", json!("all")),
            ("/volumes/0/read_only", json!(true)),
            ("/volumes/1/source", json!("other")),
            ("/deploy/resources/limits/cpus", json!(0)),
            ("/deploy/resources/limits/memory", json!(0)),
            ("/pids_limit", json!(-1)),
            ("/read_only", json!(false)),
        ] {
            let mut invalid = config.clone();
            *invalid["services"]["persona-worker"]
                .pointer_mut(path)
                .unwrap() = value;
            assert!(check_persona_runtime(&invalid).is_err(), "accepted {path}");
        }
    }
}
