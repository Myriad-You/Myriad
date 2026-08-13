//! Probe the docker daemon: version, rootless flag, podman shim detection, clock skew.

use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerProbe {
    pub available: bool,
    pub server_version: Option<String>,
    pub api_version: Option<String>,
    pub rootless: bool,
    pub is_podman: bool,
    pub daemon_time_skew_seconds: Option<i64>,
    pub error: Option<String>,
}

pub async fn probe() -> DockerProbe {
    let mut command = Command::new("docker");
    crate::docker::compose::harden_docker_command(&mut command);
    let out = command
        .args(["info", "--format", "{{json .}}"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await;

    let (available, json) = match out {
        Ok(o) if o.status.success() => (true, String::from_utf8_lossy(&o.stdout).into_owned()),
        Ok(o) => {
            return DockerProbe {
                available: false,
                server_version: None,
                api_version: None,
                rootless: false,
                is_podman: false,
                daemon_time_skew_seconds: None,
                error: Some(format!(
                    "`docker info` failed: {}",
                    String::from_utf8_lossy(&o.stderr).trim()
                )),
            }
        }
        Err(e) => {
            return DockerProbe {
                available: false,
                server_version: None,
                api_version: None,
                rootless: false,
                is_podman: false,
                daemon_time_skew_seconds: None,
                error: Some(format!("cannot exec docker: {e}")),
            }
        }
    };

    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);

    let server_version = parsed
        .get("ServerVersion")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Podman's docker compatibility shim sometimes reports `ServerVersion` like "4.x.x" plus
    // a `Host.Security.Options` entry referencing "rootless". Most reliable: check name/Driver
    // strings.
    let is_podman = json.to_lowercase().contains("podman");
    let rootless = parsed
        .pointer("/SecurityOptions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .any(|s| s.as_str().is_some_and(|s| s.contains("rootless")))
        })
        .unwrap_or(false)
        || json.to_lowercase().contains("rootless: true");

    // Daemon time via `Date` header through `docker version` won't help; skip skew detection in M1.
    DockerProbe {
        available,
        server_version,
        api_version: parsed
            .pointer("/ServerVersion")
            .and_then(|v| v.as_str())
            .map(String::from),
        rootless,
        is_podman,
        daemon_time_skew_seconds: None,
        error: None,
    }
}
