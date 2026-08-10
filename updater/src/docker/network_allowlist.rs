//! Network names docker-guard will accept on container create / network connect.
//!
//! Must stay aligned with `guard::is_allowlisted_network_name` (same env keys + defaults).
//! Preflight uses this to reject updates **before** maintenance/stop/snapshot when compose
//! would attach managed services to a network outside that allowlist — the failure mode that
//! also breaks rollback (`compose up` on the old tag).

use std::path::Path;

use crate::env_file::EnvFile;

/// Allowlisted Docker network **names** (the `Name` field / compose `networks.*.name`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkAllowlist {
    pub compose_network: String,
    pub admin_network: String,
    pub guard_network: String,
}

impl NetworkAllowlist {
    /// Resolve from process env, then optional `.env`, then docker-guard defaults.
    pub fn resolve(env_file: Option<&Path>) -> Self {
        Self {
            compose_network: resolve_name("MYRIAD_DOCKER_NETWORK", "myriad-net", env_file),
            admin_network: resolve_name("MYRIAD_ADMIN_NETWORK", "myriad-admin-net", env_file),
            guard_network: resolve_name(
                "MYRIAD_DOCKER_GUARD_NETWORK",
                "myriad-docker-guard-net",
                env_file,
            ),
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        let name = name.trim_start_matches('/');
        name == self.compose_network || name == self.admin_network || name == self.guard_network
    }

    pub fn describe(&self) -> String {
        format!(
            "{}, {}, {}",
            self.compose_network, self.admin_network, self.guard_network
        )
    }
}

fn resolve_name(key: &str, default: &str, env_file: Option<&Path>) -> String {
    if let Ok(raw) = std::env::var(key) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(path) = env_file {
        if let Ok(env) = EnvFile::load(path) {
            if let Some(raw) = env.get(key) {
                let trimmed = raw.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
    }
    default.to_string()
}

/// Services recreated during update / rollback `compose up` paths.
pub const UPDATE_RECREATE_SERVICES: &[&str] = &["backend", "frontend", "postgres"];

/// Collect Docker network names that compose will attach for the given services.
///
/// Uses `docker compose config` JSON: service network keys → top-level `networks.<key>.name`,
/// falling back to `{project}_{key}` when `name` is omitted (Compose default).
pub fn networks_for_services(
    compose_config: &serde_json::Value,
    project: &str,
    services: &[&str],
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(svc_map) = compose_config.get("services").and_then(|v| v.as_object()) else {
        return out;
    };
    for service in services {
        let Some(svc) = svc_map.get(*service) else {
            continue;
        };
        for key in service_network_keys(svc) {
            let name = resolved_network_name(compose_config, project, &key);
            out.push(((*service).to_string(), name));
        }
    }
    out
}

fn service_network_keys(service: &serde_json::Value) -> Vec<String> {
    match service.get("networks") {
        Some(serde_json::Value::Object(map)) => map.keys().cloned().collect(),
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .or_else(|| v.as_object().and_then(|o| o.keys().next().cloned()))
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn resolved_network_name(compose_config: &serde_json::Value, project: &str, key: &str) -> String {
    compose_config
        .get("networks")
        .and_then(|nets| nets.get(key))
        .and_then(|net| net.get("name"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{project}_{key}"))
}

/// Pure check used by preflight + unit tests.
pub fn find_disallowed_attachments(
    allow: &NetworkAllowlist,
    attachments: &[(String, String)],
) -> Vec<(String, String)> {
    attachments
        .iter()
        .filter(|(_, name)| !allow.contains(name))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn allow() -> NetworkAllowlist {
        NetworkAllowlist {
            compose_network: "myriad-net".into(),
            admin_network: "myriad-admin-net".into(),
            guard_network: "myriad-docker-guard-net".into(),
        }
    }

    #[test]
    fn contains_trims_leading_slash() {
        let a = allow();
        assert!(a.contains("myriad-net"));
        assert!(a.contains("/myriad-net"));
        assert!(!a.contains("other"));
    }

    #[test]
    fn collects_named_networks_from_compose_config() {
        let cfg = json!({
            "services": {
                "backend": {
                    "networks": {
                        "myriad-net": null,
                        "myriad-admin-net": {}
                    }
                },
                "frontend": {
                    "networks": ["myriad-net"]
                },
                "postgres": {
                    "networks": ["myriad-net"]
                }
            },
            "networks": {
                "myriad-net": { "name": "myriad-net" },
                "myriad-admin-net": { "name": "myriad-admin-net" }
            }
        });
        let attachments = networks_for_services(&cfg, "myriad", UPDATE_RECREATE_SERVICES);
        assert!(attachments.contains(&("backend".into(), "myriad-net".into())));
        assert!(attachments.contains(&("backend".into(), "myriad-admin-net".into())));
        assert!(attachments.contains(&("frontend".into(), "myriad-net".into())));
        assert!(find_disallowed_attachments(&allow(), &attachments).is_empty());
    }

    #[test]
    fn detects_foreign_network_name() {
        let cfg = json!({
            "services": {
                "backend": { "networks": ["myriad-net"] }
            },
            "networks": {
                "myriad-net": { "name": "legacy-bridge" }
            }
        });
        let attachments = networks_for_services(&cfg, "myriad", &["backend"]);
        let bad = find_disallowed_attachments(&allow(), &attachments);
        assert_eq!(bad, vec![("backend".into(), "legacy-bridge".into())]);
    }

    #[test]
    fn falls_back_to_project_prefixed_name() {
        let cfg = json!({
            "services": {
                "backend": { "networks": ["default"] }
            },
            "networks": {
                "default": { "driver": "bridge" }
            }
        });
        let attachments = networks_for_services(&cfg, "myriad", &["backend"]);
        assert_eq!(
            attachments,
            vec![("backend".into(), "myriad_default".into())]
        );
        assert!(!find_disallowed_attachments(&allow(), &attachments).is_empty());
    }
}
