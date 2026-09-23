//! Version-owned deployment compose. The updater writes the target version's
//! template directly into the host compose file and keeps a normalized baseline
//! so the next preflight can detect manual edits before overwriting them.
use crate::{
    docker::ComposeRunner,
    error::{Result, UpdaterError},
    state::atomic,
    worker::Worker,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct PreparedCompose {
    files: Vec<ComposeChange>,
    /// Where the normalized baseline lives; updated on both install and restore.
    #[serde(default)]
    baseline_path: PathBuf,
    /// Normalized JSON of the compose that `install()` writes.
    #[serde(default)]
    install_baseline: Vec<u8>,
    /// Normalized JSON of the compose that `restore()` writes back.
    #[serde(default)]
    restore_baseline: Vec<u8>,
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
            atomic::write_atomic_bytes(&file.path, if restore { &file.before } else { &file.after })?;
        }
        let baseline = if restore {
            &self.restore_baseline
        } else {
            &self.install_baseline
        };
        // Best-effort: a stale/missing baseline only makes the next preflight
        // prompt again (fail-safe), so never fail the write for it.
        if !self.baseline_path.as_os_str().is_empty() && !baseline.is_empty() {
            let _ = atomic::write_atomic_bytes(&self.baseline_path, baseline);
        }
        Ok(())
    }
}

/// Build the in-place compose write plan for the target image.
///
/// Returns the plan, a runner over the target template, and whether the current
/// compose differs from the last one the updater wrote (`true` means the user
/// edited it, or there is no baseline yet — e.g. first run / migrated from the
/// old symlink layout).
pub async fn prepare(
    worker: &Arc<Worker>,
    compose: &ComposeRunner,
    image: &str,
    target: &crate::version::DeployTag,
) -> Result<(PreparedCompose, ComposeRunner, bool)> {
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
    let target_json = candidate_runner.source_json().await?;

    // Detect manual edits: the current compose vs the baseline the updater last
    // wrote. A missing/unreadable baseline counts as changed (fail-safe: prompt).
    let baseline_path = worker.state().root().join("compose-baseline.json");
    let baseline = read_baseline(&baseline_path);
    let compose_changed = baseline.as_ref() != Some(&current);

    let backup_dir = worker.state().root().join("compose-backup");
    std::fs::create_dir_all(&backup_dir)?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut files = Vec::new();
    let mut blanked = Vec::new();
    for (index, path) in compose.files().iter().enumerate() {
        let current_bytes = std::fs::read(path)?;
        // Visible pre-upgrade backup of the original compose, next to the state.
        let backup_name = format!("{}-{}-{}.yml", target.as_str(), ts, index);
        let _ = atomic::write_atomic_bytes(&backup_dir.join(&backup_name), &current_bytes);
        let after_bytes = if index == 0 {
            template.clone()
        } else {
            blanked.push(path.display().to_string());
            b"{\"services\":{}}".to_vec()
        };
        files.push(ComposeChange {
            path: path.clone(),
            before: current_bytes,
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

    Ok((
        PreparedCompose {
            files,
            baseline_path,
            install_baseline: serde_json::to_vec(&target_json)?,
            restore_baseline: serde_json::to_vec(&current)?,
        },
        candidate_runner,
        compose_changed,
    ))
}

fn read_baseline(path: &Path) -> Option<serde_json::Value> {
    let bytes = crate::state::read_existing(path).ok()??;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

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
}
