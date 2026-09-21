//! Probe the host's docker compose configuration.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeProbe {
    /// Which command is invokable: "docker compose" (v2) or "docker-compose" (v1) or None.
    pub binary: Option<ComposeBinary>,
    pub compose_files: Vec<PathBuf>,
    /// True if compose.yaml/yml references `${MYRIAD_TAG}` (and friends) as required by spec.
    pub references_required_tag_vars: bool,
    pub project_name_pinned: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComposeBinary {
    DockerComposeV2, // `docker compose`
    DockerComposeV1, // `docker-compose`
}

pub async fn probe(compose_dir: &Path) -> ComposeProbe {
    let binary = detect_binary().await;

    let files = match collect_compose_files(compose_dir) {
        Ok(files) => files,
        Err(error) => {
            return ComposeProbe {
                binary,
                compose_files: Vec::new(),
                references_required_tag_vars: false,
                project_name_pinned: false,
                error: Some(error),
            };
        }
    };

    let mut refs_tag = false;
    for f in &files {
        let s = match std::fs::read_to_string(f) {
            Ok(s) => s,
            Err(error) => {
                let path = f.display().to_string();
                return ComposeProbe {
                    binary,
                    compose_files: files,
                    references_required_tag_vars: false,
                    project_name_pinned: false,
                    error: Some(format!("cannot read compose file {path}: {error}")),
                };
            }
        };
        if s.contains("${MYRIAD_TAG}") || s.contains("$MYRIAD_TAG") {
            refs_tag = true;
            break;
        }
    }

    let mut project_name_pinned = std::env::var("COMPOSE_PROJECT_NAME").is_ok();
    if !project_name_pinned {
        for f in &files {
            let s = match std::fs::read_to_string(f) {
                Ok(s) => s,
                Err(error) => {
                    let path = f.display().to_string();
                    return ComposeProbe {
                        binary,
                        compose_files: files,
                        references_required_tag_vars: refs_tag,
                        project_name_pinned: false,
                        error: Some(format!("cannot read compose file {path}: {error}")),
                    };
                }
            };
            if s.lines().any(|l| l.trim_start().starts_with("name:")) {
                project_name_pinned = true;
                break;
            }
        }
    }

    let error = if binary.is_none() {
        Some("neither `docker compose` (v2) nor `docker-compose` (v1) is available".into())
    } else if files.is_empty() {
        Some(format!(
            "no compose file found in {} (or one level below); expected compose.yaml / \
             docker-compose.yml. For 1Panel, mount the app directory that contains the \
             compose file as UPDATER_COMPOSE_DIR (/host/compose)",
            compose_dir.display()
        ))
    } else {
        None
    };

    ComposeProbe {
        binary,
        compose_files: files,
        references_required_tag_vars: refs_tag,
        project_name_pinned,
        error,
    }
}

async fn detect_binary() -> Option<ComposeBinary> {
    // Try v2 first.
    if let Ok(out) = Command::new("docker")
        .args(["compose", "version", "--short"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        && out.success()
    {
        return Some(ComposeBinary::DockerComposeV2);
    }
    if let Ok(out) = Command::new("docker-compose")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        && out.success()
    {
        return Some(ComposeBinary::DockerComposeV1);
    }
    None
}

pub(crate) fn collect_compose_files(
    compose_dir: &Path,
) -> std::result::Result<Vec<PathBuf>, String> {
    let names = [
        "compose.yaml",
        "compose.yml",
        "docker-compose.yaml",
        "docker-compose.yml",
    ];
    let mut files = Vec::new();
    for name in names {
        let p = compose_dir.join(name);
        if crate::probe::filesystem::path_is_present(&p).map_err(|error| error.to_string())? {
            files.push(p);
        }
    }
    if files.is_empty() {
        let rd = std::fs::read_dir(compose_dir).map_err(|error| {
            format!("cannot list compose dir {}: {error}", compose_dir.display())
        })?;
        for entry in rd {
            let entry = entry.map_err(|error| {
                format!("cannot read compose dir {}: {error}", compose_dir.display())
            })?;
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            for name in names {
                let p = entry.path().join(name);
                if crate::probe::filesystem::path_is_present(&p)
                    .map_err(|error| error.to_string())?
                {
                    files.push(p);
                }
            }
        }
    }
    Ok(files)
}

impl ComposeBinary {
    /// Build a `Command` invoking the binary with `-p <project>` and the discovered compose file(s).
    pub fn command(&self, project: &str, files: &[PathBuf]) -> tokio::process::Command {
        let mut cmd = match self {
            ComposeBinary::DockerComposeV2 => {
                let mut c = Command::new("docker");
                c.arg("compose");
                c
            }
            ComposeBinary::DockerComposeV1 => Command::new("docker-compose"),
        };
        cmd.arg("-p").arg(project);
        for f in files {
            cmd.arg("-f").arg(f);
        }
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_subdirectory_is_used_only_when_root_has_no_compose() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("myriad");
        std::fs::create_dir(&nested).unwrap();
        let child = nested.join("docker-compose.yml");
        std::fs::write(&child, "services: {}\n").unwrap();
        assert_eq!(collect_compose_files(dir.path()).unwrap(), vec![child]);
        let root = dir.path().join("compose.yaml");
        std::fs::write(&root, "services: {}\n").unwrap();
        assert_eq!(collect_compose_files(dir.path()).unwrap(), vec![root]);
    }

    #[test]
    fn collect_compose_files_does_not_treat_exists_false_as_absence() {
        let src = include_str!("compose.rs");
        let prod = src.split("#[cfg(test)]").next().expect("prod");
        assert!(!prod.contains("p.exists()"));
        assert!(prod.contains("path_is_present"));
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("compose.yaml"), "name: t\nservices: {}\n").unwrap();
        let files = collect_compose_files(dir.path()).unwrap();
        assert_eq!(files.len(), 1);
    }
}
