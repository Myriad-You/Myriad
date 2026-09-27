//! Carry the installed storage source across version-owned Compose templates.
//! No migration flags: the current Compose is the only record of the layout.
use std::path::Path;

use serde_json::{Value, json};

use crate::error::{Result, UpdaterError};

pub(super) fn preserve_storage(
    current: &Value,
    resolved: &Value,
    target: &mut Value,
    root: &Path,
) -> Result<()> {
    let fail = || {
        UpdaterError::Precondition(
            "unrecognized backend storage layout; migrate storage explicitly before updating"
                .into(),
        )
    };
    let mounts = resolved
        .pointer("/services/backend/volumes")
        .and_then(Value::as_array)
        .ok_or_else(fail)?;
    let mut legacy = None;
    for kind in ["data", "cache"] {
        let destination = format!("/app/{kind}");
        let matches: Vec<_> = mounts
            .iter()
            .filter(|m| m["target"] == destination)
            .collect();
        if matches.len() != 1 || matches[0]["read_only"] == true {
            return Err(fail());
        }
        let mount = matches[0];
        let volume = format!("backend_{kind}");
        let is_legacy = match mount["type"].as_str() {
            Some("volume")
                if mount["source"] == volume && mount.pointer("/volume/subpath").is_none() =>
            {
                true
            }
            Some("bind")
                if mount["source"].as_str().map(Path::new) == Some(root.join(kind).as_path()) =>
            {
                false
            }
            _ => return Err(fail()),
        };
        if legacy.is_some_and(|previous| previous != is_legacy) {
            return Err(fail());
        }
        legacy = Some(is_legacy);
        if is_legacy {
            let project = resolved["name"].as_str().ok_or_else(fail)?;
            if resolved["volumes"][&volume]["name"] != format!("{project}_{volume}") {
                return Err(fail());
            }
            let definition = current
                .get("volumes")
                .and_then(|v| v.get(&volume))
                .ok_or_else(fail)?;
            // Preserve external/driver_opts/name as well as the logical source:
            // these may describe either original or bind-backed named volumes.
            if !target.get("volumes").is_some_and(Value::is_object) {
                target["volumes"] = json!({});
            }
            target["volumes"][&volume] = definition.clone();
        } else if let Some(volumes) = target.get_mut("volumes").and_then(Value::as_object_mut) {
            volumes.remove(&volume);
        }
    }
    let legacy = legacy.ok_or_else(fail)?;
    for service in [
        "backend",
        "backend-volume-init",
        "persona-worker",
        "federation-worker",
    ] {
        let Some(config) = target.get_mut("services").and_then(|v| v.get_mut(service)) else {
            continue;
        };
        let pairs: &[(&str, &str, bool)] = if service == "federation-worker" {
            &[
                ("data", "/app/data", true),
                ("data/federation", "/app/data/federation", false),
                ("data/federation_media", "/app/data/federation_media", false),
                ("data/media", "/app/data/media", false),
                ("cache/images", "/tmp/cache/images", false),
            ]
        } else {
            &[("data", "/app/data", false), ("cache", "/app/cache", false)]
        };
        config["volumes"] = Value::Array(
            pairs
                .iter()
                .map(|(relative, destination, ro)| {
                    if legacy {
                        let (kind, subpath) = relative.split_once('/').unwrap_or((relative, ""));
                        let mut mount = json!({"type":"volume", "source":format!("backend_{kind}"),
                    "target":destination, "read_only":ro});
                        if !subpath.is_empty() {
                            mount["volume"] = json!({"subpath":subpath, "nocopy":true});
                        }
                        mount
                    } else {
                        json!({"type":"bind", "source":format!("./{relative}"),
                    "target":destination, "read_only":ro, "bind":{"create_host_path":false}})
                    }
                })
                .collect(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(legacy: bool) -> Value {
        let mounts: Vec<_> = ["data", "cache"]
            .iter()
            .map(|kind| {
                json!({"type":if legacy { "volume" } else { "bind" },
                "source":if legacy {format!("backend_{kind}")} else {format!("/srv/myriad/{kind}")},
                "target":format!("/app/{kind}")})
            })
            .collect();
        json!({"name":"site", "services":{"backend":{"volumes":mounts}, "backend-volume-init":{},
            "persona-worker":{},"federation-worker":{}},
            "volumes":{"backend_data":{"external":true,"name":"site_backend_data"},
                       "backend_cache":{"driver":"local","name":"site_backend_cache"}}})
    }

    #[test]
    fn template_changes_preserve_storage_in_both_directions() {
        for legacy in [true, false] {
            let installed = config(legacy);
            let mut target = config(!legacy);
            preserve_storage(
                &installed,
                &installed,
                &mut target,
                Path::new("/srv/myriad"),
            )
            .unwrap();
            let mounts = &target["services"]["federation-worker"]["volumes"];
            assert_eq!(mounts[0]["read_only"], true);
            assert_eq!(mounts[3]["read_only"], false);
            if legacy {
                assert_eq!(target["volumes"], installed["volumes"]);
                assert_eq!(mounts[3]["source"], "backend_data");
                assert_eq!(mounts[3]["volume"]["subpath"], "media");
            } else {
                assert_eq!(mounts[3]["source"], "./data/media");
                assert_eq!(mounts[3]["bind"]["create_host_path"], false);
                assert!(target["volumes"].as_object().unwrap().is_empty());
            }
        }
    }

    // Exercises the actual Compose parser and PreparedCompose persistence, not
    // just hand-written JSON. No daemon or registry is required.
    #[tokio::test]
    #[ignore = "requires Docker Compose CLI"]
    async fn storage_compose_roundtrip_preserves_sources_and_rollback() {
        use crate::docker::ComposeRunner;
        use crate::probe::compose::ComposeBinary;
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        for template in [
            "docker-compose.yml",
            "docs/deployment/examples/docker-compose.external-db.example.yml",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().canonicalize().unwrap();
            let file = root.join("compose.json");
            let env = root.join(".env");
            let policy = root.join("policy.env");
            std::fs::write(&env, "MYRIAD_TAG=v0.5.8\nUPDATER_TAG=v0.5.8\nPROXY_TAG=v0.5.7\nGUARD_SELF_UPDATE_TOKEN=fixture\nUPDATE_TOKEN=fixture\nUPDATER_GATEWAY_SECRET=fixture\nPOSTGRES_PASSWORD=fixture\nPERSONA_DB_PASSWORD=fixture\nFEDERATION_DB_PASSWORD=fixture\nJWT_SECRET=fixture\nMYRIAD_SETUP_SECRET=fixture\nDATABASE_URL=postgres://fixture:fixture@db/fixture\nPERSONA_DATABASE_URL=postgres://persona:fixture@db/fixture\nFEDERATION_DATABASE_URL=postgres://federation:fixture@db/fixture\n").unwrap();
            std::fs::write(&policy, "").unwrap();
            std::fs::copy(repo.join(template), &file).unwrap();
            let runner = ComposeRunner::new(
                ComposeBinary::DockerComposeV2,
                "storage-fixture",
                vec![file.clone()],
                env,
                policy,
                root.clone(),
                root.clone(),
            );
            let direct = runner.source_json().await.unwrap();
            let mut legacy = direct.clone();
            preserve_storage(&config(true), &config(true), &mut legacy, &root).unwrap();
            legacy["volumes"]["backend_data"]["name"] = json!("storage-fixture_backend_data");
            legacy["volumes"]["backend_cache"]["name"] = json!("storage-fixture_backend_cache");
            for (installed, candidate) in [(&legacy, &direct), (&direct, &legacy)] {
                let before = serde_json::to_vec(installed).unwrap();
                std::fs::write(&file, &before).unwrap();
                let current = runner.source_json().await.unwrap();
                let resolved = runner.config_json().await.unwrap();
                let mut target = candidate.clone();
                preserve_storage(&current, &resolved, &mut target, &root).unwrap();
                let plan = super::super::PreparedCompose {
                    files: vec![super::super::ComposeChange {
                        path: file.clone(),
                        before: before.clone(),
                        after: serde_json::to_vec(&target).unwrap(),
                    }],
                    baseline_path: root.join("baseline.json"),
                    install_baseline: serde_json::to_vec(&target).unwrap(),
                    restore_baseline: serde_json::to_vec(&current).unwrap(),
                };
                plan.install().unwrap();
                assert_eq!(runner.source_json().await.unwrap(), target);
                let updated = runner.config_json().await.unwrap();
                for service in [
                    "backend",
                    "backend-volume-init",
                    "persona-worker",
                    "federation-worker",
                ] {
                    let sorted = |value: &Value| {
                        let mut mounts = value["services"][service]["volumes"]
                            .as_array()
                            .unwrap()
                            .clone();
                        mounts.sort_by_key(|m| m["target"].as_str().unwrap().to_owned());
                        mounts
                    };
                    assert_eq!(sorted(&updated), sorted(&resolved));
                }
                assert_eq!(updated.get("volumes"), resolved.get("volumes"));
                plan.prepare_restore(&runner, root.join("baseline.json"))
                    .await
                    .unwrap()
                    .restore()
                    .unwrap();
                assert_eq!(std::fs::read(&file).unwrap(), before);

                // The operator subsequently migrated storage. A historical
                // snapshot must restore code without undoing that migration.
                let migrated = serde_json::to_vec(candidate).unwrap();
                std::fs::write(&file, &migrated).unwrap();
                let expected = runner.config_json().await.unwrap();
                let restore = plan
                    .prepare_restore(&runner, root.join("baseline.json"))
                    .await
                    .unwrap();
                assert_eq!(std::fs::read(&file).unwrap(), migrated);
                restore.restore().unwrap();
                let actual = runner.config_json().await.unwrap();
                for service in [
                    "backend",
                    "backend-volume-init",
                    "persona-worker",
                    "federation-worker",
                ] {
                    let mounts = |value: &Value| {
                        let mut mounts = value["services"][service]["volumes"].as_array().unwrap().clone();
                        mounts.sort_by_key(|m| m["target"].as_str().unwrap().to_owned());
                        mounts
                    };
                    assert_eq!(mounts(&actual), mounts(&expected));
                }
                assert_eq!(actual.get("volumes"), expected.get("volumes"));

                let mut foreign = candidate.clone();
                foreign["services"]["backend"]["volumes"] = json!([
                    {"type":"bind", "source":"/foreign/data", "target":"/app/data"},
                    {"type":"bind", "source":"/foreign/cache", "target":"/app/cache"}
                ]);
                let foreign = serde_json::to_vec(&foreign).unwrap();
                std::fs::write(&file, &foreign).unwrap();
                assert!(
                    plan.prepare_restore(&runner, root.join("baseline.json"))
                        .await
                        .is_err()
                );
                assert_eq!(std::fs::read(&file).unwrap(), foreign);
            }
        }
    }

    #[test]
    fn ambiguous_or_foreign_sources_never_silently_switch_to_empty_storage() {
        for source in ["/other/data", "/srv/myriad/data/../data", "other_volume"] {
            let mut current = config(false);
            current["services"]["backend"]["volumes"][0]["source"] = json!(source);
            assert!(
                preserve_storage(
                    &current,
                    &current,
                    &mut config(false),
                    Path::new("/srv/myriad")
                )
                .is_err()
            );
        }
        let mut foreign = config(true);
        foreign["volumes"]["backend_data"]["name"] = json!("other-project_backend_data");
        assert!(
            preserve_storage(
                &foreign,
                &foreign,
                &mut config(false),
                Path::new("/srv/myriad")
            )
            .is_err()
        );
        let mut current = config(true);
        current["services"]["backend"]["volumes"][0] =
            config(false)["services"]["backend"]["volumes"][0].clone();
        assert!(
            preserve_storage(
                &current,
                &current,
                &mut config(false),
                Path::new("/srv/myriad")
            )
            .is_err()
        );
    }
}
