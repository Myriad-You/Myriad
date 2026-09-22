//! Version-owned application definitions and host-owned deployment settings.
use crate::{
    docker::ComposeRunner,
    error::{Result, UpdaterError},
    state::atomic,
    worker::Worker,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

const APPLICATION: [&str; 5] = [
    "backend",
    "frontend",
    "backend-volume-init",
    "federation-worker",
    "persona-worker",
];

/// Service keys the host owns: `merge_application` copies them from the current
/// deployment instead of taking the target template's value.
const SITE_OWNED_KEYS: [&str; 9] = [
    "ports",
    "networks",
    "extra_hosts",
    "dns",
    "dns_search",
    "logging",
    "restart",
    "env_file",
    "labels",
];

#[derive(Debug, Serialize, Deserialize)]
pub struct PreparedCompose {
    files: Vec<ComposeChange>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ComposeChange {
    path: PathBuf,
    before: Vec<u8>,
    after: Vec<u8>,
}

impl PreparedCompose {
    pub fn install(&self) -> Result<()> {
        self.write(false)
    }
    pub fn restore(&self) -> Result<()> {
        self.write(true)
    }

    fn write(&self, restore: bool) -> Result<()> {
        for file in &self.files {
            // Follow the host entry point to the writable state file, so the
            // updater never needs write access to the deployment root or policy.
            let path = std::fs::canonicalize(&file.path)?;
            atomic::write_atomic_bytes(&path, if restore { &file.before } else { &file.after })?;
        }
        Ok(())
    }
}

pub async fn prepare(
    worker: &Arc<Worker>,
    compose: &ComposeRunner,
    image: &str,
    target: &crate::version::DeployTag,
) -> Result<(PreparedCompose, ComposeRunner)> {
    let inspection = worker
        .docker()
        .raw()
        .inspect_image(image)
        .await
        .map_err(|e| UpdaterError::Docker(format!("inspect deployment revision: {e}")))?;
    let labels = inspection.config.and_then(|c| c.labels).unwrap_or_default();
    let revision = labels
        .get("org.opencontainers.image.revision")
        .cloned()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| crate::release::github::deploy_tag_to_git_ref(target));
    let variant = worker.cli().db_mode.as_str();
    let template = match labels.get(&format!("io.myriad.compose.{variant}")) {
        Some(encoded) => base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| {
                UpdaterError::Precondition(format!("invalid image deployment template: {e}"))
            })?,
        // v0.5.3 has no embedded template. Remove after the first refactor release.
        None => {
            worker
                .github_client()?
                .compose_template(&revision, worker.cli().db_mode.is_external())
                .await?
        }
    };
    let candidate = worker.state().root().join("compose-candidate.json");
    atomic::write_atomic_bytes(&candidate, &template)?;
    let candidate_runner = compose.with_files(vec![candidate.clone()]);
    let current = compose.source_json().await?;
    let target = candidate_runner.source_json().await?;
    let merged = merge_application(current, target)?;
    let after = serde_json::to_vec_pretty(&merged)?;
    atomic::write_atomic_bytes(&candidate, &after)?;
    let mut files = Vec::new();
    let mut blanked = Vec::new();
    for (index, path) in compose.files().iter().enumerate() {
        // Compose already combined any panel fragments. Keep one authority, so
        // every fragment after the first is emptied. Record which ones: an
        // operator reverting to an older updater would otherwise find empty files
        // with nothing saying where the contents went.
        let after_bytes = if index == 0 {
            after.clone()
        } else {
            blanked.push(path.display().to_string());
            b"{\"services\":{}}".to_vec()
        };
        files.push(ComposeChange {
            path: path.clone(),
            before: std::fs::read(path)?,
            after: after_bytes,
        });
    }
    if !blanked.is_empty() {
        worker.state().record_operation(&format!(
            "audit: compose_fragments_blanked count={} files={}",
            blanked.len(),
            blanked.join(",")
        ));
    }
    Ok((PreparedCompose { files }, candidate_runner))
}

fn merge_application(mut current: Value, target: Value) -> Result<Value> {
    let target_services = target["services"]
        .as_object()
        .ok_or_else(|| UpdaterError::Precondition("deployment template has no services".into()))?;
    let services = current["services"]
        .as_object_mut()
        .ok_or_else(|| UpdaterError::Precondition("deployment has no services".into()))?;
    let backend_networks = services
        .get("backend")
        .and_then(|s| s.get("networks"))
        .cloned();
    for name in APPLICATION {
        let Some(next) = target_services.get(name) else {
            services.remove(name);
            continue;
        };
        let mut next = next.clone();
        if let Some(previous) = services.get(name) {
            for key in SITE_OWNED_KEYS {
                if let Some(value) = previous.get(key) {
                    next[key] = value.clone();
                }
            }
            if let (Some(old_env), Some(new_env)) = (
                previous["environment"].as_object(),
                next.get_mut("environment").and_then(Value::as_object_mut),
            ) {
                for (key, value) in old_env {
                    // The target template defers site-owned values to `${...}`
                    // placeholders, so the site value wins whenever the target does
                    // not supply a literal. A site value that is itself an
                    // interpolation (`${MY_DB_URL}`) must survive too.
                    let target_defers = new_env
                        .get(key)
                        .is_none_or(|new| new.as_str().is_some_and(|s| s.contains("${")));
                    if target_defers {
                        new_env.insert(key.clone(), value.clone());
                    }
                }
            }
        } else if name.ends_with("-worker")
            && let Some(networks) = &backend_networks
        {
            next["networks"] = networks.clone();
        }
        services.insert(name.into(), next);
    }
    // Preserve volume identities and local network definitions; add only new names.
    for section in ["volumes", "networks", "configs", "secrets"] {
        if let Some(additions) = target[section].as_object() {
            if current.get(section).is_none() {
                current[section] = serde_json::json!({});
            }
            if let Some(existing) = current[section].as_object_mut() {
                for (key, value) in additions {
                    existing.entry(key.clone()).or_insert_with(|| value.clone());
                }
            }
        }
    }
    Ok(current)
}

/// Called by the existing trusted helper, including when launched by v0.5.3.
/// Keep the host Compose filename, but move its contents under the already
/// writable state mount. The root and Guard policy remain read-only to updater.
pub(crate) fn manage_compose_files(read_root: &Path, write_root: &Path) -> Result<()> {
    let files = crate::probe::compose::collect_compose_files(read_root)
        .map_err(UpdaterError::Precondition)?;
    for path in files {
        let relative = path
            .strip_prefix(read_root)
            .map_err(|e| UpdaterError::Config(e.to_string()))?;
        let managed_relative = PathBuf::from("state/compose").join(relative);
        let host_file = write_root.join(relative);
        let mut link = PathBuf::new();
        for _ in relative.parent().unwrap().components() {
            link.push("..");
        }
        link.push(&managed_relative);
        if std::fs::read_link(&host_file).ok().as_ref() == Some(&link) {
            continue;
        }
        let managed = write_root.join(&managed_relative);
        std::fs::create_dir_all(managed.parent().unwrap())?;
        atomic::write_atomic_bytes(&managed, &std::fs::read(&host_file)?)?;
        let temporary = tempfile::Builder::new()
            .prefix(".compose-link-")
            .tempdir_in(host_file.parent().unwrap())?;
        let new_link = temporary.path().join("link");
        std::os::unix::fs::symlink(link, &new_link)?;
        std::fs::rename(new_link, &host_file)?;
        std::fs::File::open(host_file.parent().unwrap())?.sync_all()?;
    }
    Ok(())
}

pub(crate) fn compose_is_managed(root: &Path) -> Result<bool> {
    let files =
        crate::probe::compose::collect_compose_files(root).map_err(UpdaterError::Precondition)?;
    Ok(!files.is_empty()
        && files.iter().all(|file| {
            std::fs::canonicalize(file)
                .is_ok_and(|path| path.starts_with(root.join("state/compose")))
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn trusted_helper_adopts_existing_entry_points_without_changing_contents() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("panel")).unwrap();
        let file = root.join("panel/docker-compose.yml");
        let original = b"services: {backend: {image: 'example:${MYRIAD_TAG}'}}\n";
        std::fs::write(&file, original).unwrap();
        assert!(!compose_is_managed(root).unwrap());
        manage_compose_files(root, root).unwrap();
        manage_compose_files(root, root).unwrap();
        assert!(compose_is_managed(root).unwrap());
        assert_eq!(std::fs::read(&file).unwrap(), original);
        let prepared = PreparedCompose {
            files: vec![ComposeChange {
                path: file.clone(),
                before: original.to_vec(),
                after: b"services: {}\n".to_vec(),
            }],
        };
        prepared.install().unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"services: {}\n");
        prepared.restore().unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), original);
        assert!(file.is_symlink());
    }

    #[test]
    fn official_bundled_and_external_templates_merge_and_render() {
        for target in [
            include_str!("../../docker-compose.yml"),
            include_str!("../../docs/deployment/examples/docker-compose.external-db.example.yml"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("compose.yaml");
            let command = |source: bool| {
                let mut cmd = Command::new("docker");
                cmd.args(["compose", "-p", "myriad", "-f"])
                    .arg(&path)
                    .args(["config", "--format", "json"]);
                if source {
                    cmd.args([
                        "--no-interpolate",
                        "--no-normalize",
                        "--no-path-resolution",
                        "--no-env-resolution",
                    ]);
                }
                cmd.envs([
                    ("MYRIAD_TAG", "v0.5.3"),
                    ("UPDATER_TAG", "v0.5.3"),
                    ("PROXY_TAG", "v0.5.3"),
                    ("MYRIAD_SETUP_SECRET", "fixture"),
                    ("GUARD_SELF_UPDATE_TOKEN", "fixture"),
                    ("PERSONA_DB_PASSWORD", "fixture"),
                    ("FEDERATION_DB_PASSWORD", "fixture"),
                    ("POSTGRES_PASSWORD", "fixture"),
                    ("DATABASE_URL", "postgres://fixture/db"),
                    ("FEDERATION_DATABASE_URL", "postgres://fixture/federation"),
                    ("PERSONA_DATABASE_URL", "postgres://fixture/persona"),
                ]);
                let out = cmd.output().unwrap();
                assert!(
                    out.status.success(),
                    "{}",
                    String::from_utf8_lossy(&out.stderr)
                );
                serde_json::from_slice::<Value>(&out.stdout).unwrap()
            };
            std::fs::write(&path, target).unwrap();
            let template = command(true);
            let mut current = template.clone();
            let volumes = current["services"]["federation-worker"]["volumes"]
                .as_array_mut()
                .unwrap();
            volumes.retain(|mount| mount["target"] != "/app/data/media");
            current["services"]["backend"]["environment"]["JWT_SECRET"] = "site-secret".into();
            let merged = merge_application(current, template).unwrap();
            std::fs::write(&path, serde_json::to_vec(&merged).unwrap()).unwrap();
            let rendered = command(false);
            assert_eq!(
                rendered["services"]["backend"]["environment"]["JWT_SECRET"],
                "site-secret"
            );
            assert!(
                rendered["services"]["federation-worker"]["volumes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|mount| mount["target"] == "/app/data/media")
            );
        }
    }

    #[test]
    fn published_labels_contain_the_source_templates() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let out = Command::new("bash")
            .arg("scripts/extra/compose-image-labels.sh")
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let labels: Vec<_> = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        for (line, (variant, source)) in labels.iter().zip([
            ("bundled", "docker-compose.yml"),
            (
                "external",
                "docs/deployment/examples/docker-compose.external-db.example.yml",
            ),
        ]) {
            let (name, encoded) = line.split_once('=').unwrap();
            assert_eq!(name, format!("io.myriad.compose.{variant}"));
            assert_eq!(
                base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .unwrap(),
                std::fs::read(repo.join(source)).unwrap()
            );
        }
        assert_eq!(labels.len(), 2);
    }

    /// The version owns the mount list. Keeping a site mount was not an option
    /// either: the Guard rejects every site-added mount on these services (only
    /// their own `*_backend_data`/`*_backend_cache` volumes at fixed targets are
    /// allowed), so a stack that carried one could not start after the swap.
    #[test]
    fn merge_takes_the_target_mount_list() {
        let current = serde_json::json!({
            "services": {
                "backend": {
                    "image": "example/backend:${MYRIAD_TAG}",
                    "environment": {"DATABASE_URL": "${MY_DB_URL}", "CUSTOM": "keep"},
                    "volumes": [
                        "backend_data:/app/data",
                        "/host/media:/app/data/media",
                    ],
                    "ports": ["8080:80"],
                }
            },
            "volumes": {"backend_data": {"name": "existing"}},
        });
        let target = serde_json::json!({
            "services": {
                "backend": {
                    "image": "example/backend:${MYRIAD_TAG}",
                    "environment": {"DATABASE_URL": "${DATABASE_URL}"},
                    "volumes": [
                        {"type": "volume", "source": "backend_data", "target": "/app/data"},
                        {"type": "volume", "source": "backend_cache", "target": "/app/cache"},
                    ],
                }
            },
            "volumes": {"backend_data": {"name": "would-lose-data"}, "backend_cache": {}},
        });

        let merged = merge_application(current, target).unwrap();

        let volumes = merged["services"]["backend"]["volumes"].as_array().unwrap();
        assert_eq!(
            volumes.len(),
            2,
            "the target mount list replaces the host's: {volumes:?}"
        );
        assert_eq!(
            merged["services"]["backend"]["environment"]["DATABASE_URL"],
            "${MY_DB_URL}",
            "a site value that is itself an interpolation must survive"
        );
        assert_eq!(merged["services"]["backend"]["environment"]["CUSTOM"], "keep");
        assert_eq!(merged["services"]["backend"]["ports"][0], "8080:80");
        assert_eq!(
            merged["volumes"]["backend_data"]["name"], "existing",
            "volume identity stays the host's"
        );
    }
}
