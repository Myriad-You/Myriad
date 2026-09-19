//! Network names docker-guard will accept on container create / network connect.
//!
//! Must stay aligned with `guard::is_allowlisted_network_name` (same env keys + defaults).
//! Preflight uses this to reject updates **before** maintenance/stop/snapshot when compose
//! would attach managed services to a network outside that allowlist — the failure mode that
//! also breaks rollback (`compose up` on the old tag).

use std::path::Path;

use crate::env_file::EnvFile;
use crate::error::UpdaterError;

/// Default name of the operator-owned network shared with an external PostgreSQL
/// container. Override with `MYRIAD_BACKEND_EXTRA_NETWORK`. Only
/// backend / federation-worker / persona-worker may attach to it, and it must
/// differ from the three managed networks (business/admin/guard).
pub const DEFAULT_EXTERNAL_DATABASE_NETWORK: &str = "myriad-backend-ext";

/// Built-in extra networks that the database clients may join even when the
/// operator has not declared `MYRIAD_BACKEND_EXTRA_NETWORK`. These are
/// first-party panel networks a host-side manager attaches on its own, so the
/// Guard must recognize them without an explicit selector. Same service scope
/// as the configured external network; never overrides the managed networks.
pub const FALLBACK_EXTRA_NETWORKS: &[&str] = &["1panel-network"];

/// Shared by preflight and Guard create/connect/disconnect authorization.
pub(crate) fn service_network_allowed(
    service: &str,
    network: &str,
    business: &str,
    admin: &str,
    guard: &str,
    external: &str,
) -> bool {
    let network = network.trim_start_matches('/');
    let managed = network == business || network == admin || network == guard;
    if !managed && (network == external || FALLBACK_EXTRA_NETWORKS.contains(&network)) {
        return matches!(service, "backend" | "federation-worker" | "persona-worker");
    }
    match service {
        "postgres" | "frontend" | "federation-worker" | "persona-worker" => network == business,
        "backend" | "proxy" => network == business || network == admin,
        "updater" => network == admin || network == guard,
        _ => false,
    }
}

/// Allowlisted Docker network **names** (the `Name` field / compose `networks.*.name`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkAllowlist {
    pub compose_network: String,
    pub admin_network: String,
    pub guard_network: String,
    pub external_database_network: String,
}

impl NetworkAllowlist {
    /// Resolve from process env, then optional `.env`, then docker-guard defaults.
    pub fn resolve(env_file: Option<&Path>) -> Result<Self, UpdaterError> {
        let compose_network = resolve_name("MYRIAD_DOCKER_NETWORK", "myriad-net", env_file);
        let admin_network = resolve_name("MYRIAD_ADMIN_NETWORK", "myriad-admin-net", env_file);
        let guard_network = resolve_name(
            "MYRIAD_DOCKER_GUARD_NETWORK",
            "myriad-docker-guard-net",
            env_file,
        );
        let external_database_network = resolve_name(
            "MYRIAD_BACKEND_EXTRA_NETWORK",
            DEFAULT_EXTERNAL_DATABASE_NETWORK,
            env_file,
        );
        for (key, value) in [
            ("MYRIAD_DOCKER_NETWORK", compose_network.as_str()),
            ("MYRIAD_ADMIN_NETWORK", admin_network.as_str()),
            ("MYRIAD_DOCKER_GUARD_NETWORK", guard_network.as_str()),
            (
                "MYRIAD_BACKEND_EXTRA_NETWORK",
                external_database_network.as_str(),
            ),
        ] {
            crate::docker::guard::validate_identifier(value)
                .map_err(|e| UpdaterError::Precondition(format!("{key}: {e}")))?;
        }
        if external_database_network == compose_network
            || external_database_network == admin_network
            || external_database_network == guard_network
        {
            return Err(UpdaterError::Precondition(
                "MYRIAD_BACKEND_EXTRA_NETWORK must differ from the managed networks".into(),
            ));
        }
        Ok(Self {
            compose_network,
            admin_network,
            guard_network,
            external_database_network,
        })
    }

    pub fn contains(&self, name: &str) -> bool {
        let name = name.trim_start_matches('/');
        name == self.compose_network
            || name == self.admin_network
            || name == self.guard_network
            || name == self.external_database_network
            || FALLBACK_EXTRA_NETWORKS.contains(&name)
    }

    pub fn allows_service(&self, service: &str, name: &str) -> bool {
        service_network_allowed(
            service,
            name,
            &self.compose_network,
            &self.admin_network,
            &self.guard_network,
            &self.external_database_network,
        )
    }

    pub fn describe(&self) -> String {
        format!(
            "{}, {}, {}, {} (backend/federation-worker/persona-worker only), {} (built-in extra network, same services)",
            self.compose_network,
            self.admin_network,
            self.guard_network,
            self.external_database_network,
            FALLBACK_EXTRA_NETWORKS.join(", ")
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
    if let Some(path) = env_file
        && let Ok(env) = EnvFile::load(path)
        && let Some(raw) = env.get(key)
    {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    default.to_string()
}

/// Services recreated during update / rollback `compose up` paths.
pub const UPDATE_RECREATE_SERVICES: &[&str] = &[
    "backend",
    "federation-worker",
    "persona-worker",
    "frontend",
    "postgres",
];

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
        .filter(|(service, name)| !allow.allows_service(service, name))
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
            external_database_network: DEFAULT_EXTERNAL_DATABASE_NETWORK.into(),
        }
    }

    fn allow_with_external(external: &str) -> NetworkAllowlist {
        NetworkAllowlist {
            external_database_network: external.into(),
            ..allow()
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
    fn external_database_network_accepts_all_three_database_clients() {
        let cfg = json!({
            "services": {
                "backend": {"networks": ["business", "db"]},
                "federation-worker": {"networks": ["business", "db"]},
                "persona-worker": {"networks": ["business", "db"]}
            },
            "networks": {
                "business": {"name": "myriad-net"},
                "db": {"name": "myriad-backend-ext", "external": true}
            }
        });
        let attachments = networks_for_services(&cfg, "myriad", UPDATE_RECREATE_SERVICES);
        assert!(find_disallowed_attachments(&allow(), &attachments).is_empty());
        assert!(allow().contains("myriad-backend-ext"));
    }

    #[test]
    fn external_database_network_rejects_non_database_services_and_other_networks() {
        let entries: Vec<_> = [
            "frontend",
            "proxy",
            "postgres",
            "updater",
            "updater-gateway",
            "docker-guard",
            "backend-volume-init",
            "unknown",
        ]
        .into_iter()
        .map(|service| (service.to_string(), "myriad-backend-ext".to_string()))
        .chain(
            ["backend", "federation-worker", "persona-worker"]
                .into_iter()
                .map(|service| (service.to_string(), "foreign-db-net".to_string())),
        )
        .collect();
        assert_eq!(find_disallowed_attachments(&allow(), &entries), entries);
    }

    #[test]
    fn custom_external_database_network_is_honored() {
        let custom = allow_with_external("my-custom-db-net");
        for service in ["backend", "federation-worker", "persona-worker"] {
            assert!(custom.allows_service(service, "my-custom-db-net"));
            assert!(!custom.allows_service(service, "myriad-backend-ext"));
        }
        assert!(custom.contains("my-custom-db-net"));
        assert!(!custom.contains("myriad-backend-ext"));
        assert!(!custom.allows_service("frontend", "my-custom-db-net"));
    }

    #[test]
    fn builtin_fallback_network_allows_only_database_clients() {
        let a = allow();
        for service in ["backend", "federation-worker", "persona-worker"] {
            assert!(a.allows_service(service, "1panel-network"), "{service}");
        }
        for service in [
            "frontend",
            "proxy",
            "postgres",
            "updater",
            "updater-gateway",
            "docker-guard",
            "backend-volume-init",
        ] {
            assert!(!a.allows_service(service, "1panel-network"), "{service}");
        }
        assert!(a.contains("1panel-network"));
        assert!(a.contains("/1panel-network"));
        // Managed networks keep their normal service rules; the fallback branch
        // only adds extra networks.
        assert!(a.allows_service("backend", "myriad-net"));
        assert!(a.allows_service("frontend", "myriad-net"));
        assert!(!a.allows_service("frontend", "myriad-admin-net"));
    }

    #[test]
    fn external_network_must_not_alias_a_managed_network() {
        // Even if misconfigured, the external branch must not widen worker
        // access to the admin/guard networks.
        for managed in ["myriad-admin-net", "myriad-docker-guard-net"] {
            let cfg = allow_with_external(managed);
            for worker in ["federation-worker", "persona-worker"] {
                assert!(!cfg.allows_service(worker, managed));
            }
        }
        assert!(
            !allow_with_external("myriad-docker-guard-net")
                .allows_service("backend", "myriad-docker-guard-net")
        );
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
    #[test]
    fn worker_networks_cannot_inherit_backend_admin_access() {
        let entries = vec![
            ("backend".into(), "myriad-admin-net".into()),
            ("persona-worker".into(), "myriad-admin-net".into()),
            ("federation-worker".into(), "myriad-admin-net".into()),
        ];
        assert_eq!(
            find_disallowed_attachments(&allow(), &entries),
            entries[1..]
        );
    }
}
