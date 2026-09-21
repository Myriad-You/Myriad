//! Reconcile host-managed redeployments from Docker's actual image identity.
use anyhow::{Result, anyhow};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeIdentity {
    pub image: String,
    pub image_id: String,
    pub version: String,
}

fn is_official_version_tag(reference: &str) -> bool {
    reference
        .trim_start_matches("docker.io/")
        .strip_prefix("somekawahitomi/myriad-updater:")
        .is_some_and(|tag| super::self_update::validate_self_update_tag(tag).is_ok())
}

pub(crate) fn validate_startup_reference(reference: &str, allow_unpinned_dev: bool) -> Result<()> {
    if is_official_version_tag(reference) {
        return Ok(());
    }
    super::config::validate_guard_image_ref(reference, allow_unpinned_dev)
}

pub(crate) fn runtime_identity(
    container: &Value,
    image: &Value,
    project: &str,
    service: &str,
    require_healthy: bool,
) -> Result<RuntimeIdentity> {
    use super::config::{canonicalize_trusted_digest_ref, digest_reference_matches};
    if container
        .pointer("/Config/Labels/com.docker.compose.project")
        .and_then(Value::as_str)
        != Some(project)
        || container
            .pointer("/Config/Labels/com.docker.compose.service")
            .and_then(Value::as_str)
            != Some(service)
        || (require_healthy
            && (container.pointer("/State/Running").and_then(Value::as_bool) != Some(true)
                || container
                    .pointer("/State/Health/Status")
                    .and_then(Value::as_str)
                    != Some("healthy")))
    {
        return Err(anyhow!(
            "{service} is not a healthy member of the configured project"
        ));
    }
    let image_id = container
        .get("Image")
        .and_then(Value::as_str)
        .unwrap_or_default();
    validate_image_id(image_id)?;
    if image.get("Id").and_then(Value::as_str) != Some(image_id) {
        return Err(anyhow!(
            "{service} image inspection does not match its running image"
        ));
    }
    let mut digests = image
        .get("RepoDigests")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|value| canonicalize_trusted_digest_ref(value).ok())
        .collect::<Vec<_>>();
    digests.sort();
    let exact = digests
        .first()
        .cloned()
        .ok_or_else(|| anyhow!("{service} has no official repository digest"))?;
    let configured = container
        .pointer("/Config/Image")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !digests
        .iter()
        .any(|digest| digest_reference_matches(configured, digest))
        && !is_official_version_tag(configured)
    {
        return Err(anyhow!(
            "{service} was not deployed from an official version or matching digest"
        ));
    }
    // Container Env can be overridden by Compose; only the image stamp is identity.
    let version = image
        .pointer("/Config/Env")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find_map(|value| value.strip_prefix("MYRIAD_VERSION="))
        .ok_or_else(|| anyhow!("{service} image has no baked version"))?;
    super::self_update::validate_self_update_tag(version).map_err(anyhow::Error::msg)?;
    Ok(RuntimeIdentity {
        image: exact,
        image_id: image_id.into(),
        version: version.into(),
    })
}

pub(crate) fn validate_image_id(id: &str) -> Result<()> {
    let digest = id.strip_prefix("sha256:").unwrap_or_default();
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(anyhow!("running image has no valid immutable image id"));
    }
    Ok(())
}

pub(crate) async fn inspect_identity(
    config: &super::GuardConfig,
    container: &str,
    service: &str,
    healthy: bool,
) -> Result<RuntimeIdentity> {
    super::validate_identifier(container).map_err(anyhow::Error::msg)?;
    let inspect = super::forward::daemon_json(
        &config.socket_path,
        &format!("/containers/{container}/json"),
    )
    .await?;
    let id = inspect
        .get("Image")
        .and_then(Value::as_str)
        .unwrap_or_default();
    validate_image_id(id)?;
    let image =
        super::forward::daemon_json(&config.socket_path, &format!("/images/{id}/json")).await?;
    runtime_identity(&inspect, &image, &config.project, service, healthy)
}

const STARTUP_GATE: usize = super::SELF_UPDATE_GATE | 1;

pub(crate) fn schedule_reconciliation(state: super::GuardState) {
    use std::sync::atomic::Ordering;
    if state.config.allow_unpinned_dev {
        return;
    }
    if state
        .mutation_gate
        .compare_exchange(0, STARTUP_GATE, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    tokio::spawn(async move {
        let result = async {
            // Never adopt over a running/recoverable upgrade transaction.
            for name in [super::SELF_UPDATE_HELPER_NAME, super::SELF_UPDATE_RECOVERY_NAME] {
                if super::self_update::helper_container_exists(&state.config.socket_path, name).await? {
                    return Err(anyhow!("trusted handoff still exists; startup reconciliation deferred"));
                }
            }
            // Resume an interrupted metadata sync before taking a fresh snapshot.
            // A renamed exhausted recovery helper can still be executing too.
            for name in [super::STARTUP_RECONCILE_NAME, super::SELF_UPDATE_EXHAUSTED_NAME] {
                if super::self_update::helper_container_exists(&state.config.socket_path, name).await? {
                    super::self_update::wait_for_stopped_helper(&state, name).await?;
                }
            }
            // Metadata follows installed images. An unhealthy old stack still needs repair.
            let identity = inspect_stack(&state.config, false).await?;
            if !policy_matches(&state, &identity)? || !crate::deployment::compose_is_managed(&state.config.compose_dir)? {
                super::self_update::reconcile_runtime_policy(&state, &identity).await?;
            }
            tracing::info!(version = %identity[1].version, image = %identity[1].image, guard_image = %identity[0].image, gateway_image = %identity[2].image, "reconciled host deployment identity");
            Ok::<(), anyhow::Error>(())
        }.await;
        if let Err(error) = result {
            tracing::warn!(%error, "startup identity reconciliation deferred; existing policy retained");
        }
        // A temporary Docker outage must not leave a permanent mutation gate.
        while super::self_update::helper_container_exists(
            &state.config.socket_path,
            super::STARTUP_RECONCILE_NAME,
        )
        .await
        .unwrap_or(true)
        {
            match super::self_update::wait_for_stopped_helper(&state, super::STARTUP_RECONCILE_NAME)
                .await
            {
                Ok(()) => break,
                Err(error) => tracing::warn!(%error, "waiting for reconciliation helper to stop"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        let _ = super::self_update::finalize_or_fail_orphaned_pending_handoff(&state).await;
        // All helper execution has stopped; failed health does not forbid repair.
        let _ = state.mutation_gate.compare_exchange(
            STARTUP_GATE,
            0,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    });
}

pub(crate) async fn healthy_stack(config: &super::GuardConfig) -> Result<[RuntimeIdentity; 3]> {
    inspect_stack(config, true).await
}

pub(crate) async fn inspect_stack(
    config: &super::GuardConfig,
    healthy: bool,
) -> Result<[RuntimeIdentity; 3]> {
    let guard = inspect_identity(config, "myriad-docker-guard", "docker-guard", healthy).await?;
    if guard.image != config.expected_guard_image {
        return Err(anyhow!("Guard was replaced during startup reconciliation"));
    }
    let updater = inspect_identity(config, "myriad-updater", "updater", healthy).await?;
    let gateway =
        inspect_identity(config, "myriad-updater-gateway", "updater-gateway", healthy).await?;
    Ok([guard, updater, gateway])
}

fn policy_matches(state: &super::GuardState, stack: &[RuntimeIdentity; 3]) -> Result<bool> {
    let app = crate::env_file::EnvFile::load(&state.config.compose_dir.join(".env"))?;
    let guard = crate::env_file::EnvFile::load(std::path::Path::new(super::POLICY_CONTAINER_FILE))?;
    if let Some(tag) = app.get("UPDATER_TAG")
        && !crate::version::DeployTag::parse(tag)
            .is_ok_and(|target| target.matches_runtime_version(&stack[1].version))
    {
        tracing::warn!(requested_tag = %tag, running_version = %stack[1].version,
            "deployment tag differs from running updater; reconciliation preserves the requested tag and does not replace images");
    }
    Ok(
        app.get("UPDATER_IMAGE_REF") == Some(stack[1].image.as_str())
            && app
                .get("UPDATER_GATEWAY_IMAGE_REF")
                .filter(|value| !value.is_empty())
                .or_else(|| app.get("UPDATER_IMAGE_REF"))
                == Some(stack[2].image.as_str())
            && app.get("DOCKER_GUARD_IMAGE") == Some(stack[0].image.as_str())
            && guard.get("DOCKER_GUARD_IMAGE") == Some(stack[0].image.as_str()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> (Value, Value) {
        let id = format!("sha256:{}", "b".repeat(64));
        (
            json!({"Image": id, "Config": {
            "Image": "docker.io/somekawahitomi/myriad-updater:v0.4.13",
            "Env": ["MYRIAD_VERSION=v0.4.6"],
            "Labels": {"com.docker.compose.project": "myriad", "com.docker.compose.service": "updater"}
        }, "State": {"Running": true, "Health": {"Status": "healthy"}}}),
            json!({"Id": id, "RepoDigests": [format!("somekawahitomi/myriad-updater@sha256:{}", "c".repeat(64))],
            "Config": {"Env": ["MYRIAD_VERSION=v0.4.13"]}}),
        )
    }

    #[test]
    fn startup_accepts_official_tags_but_not_foreign_images() {
        assert!(
            validate_startup_reference("docker.io/somekawahitomi/myriad-updater:v0.4.13", false)
                .is_ok()
        );
        assert!(validate_startup_reference("evil.example/myriad-updater:v0.4.13", false).is_err());
    }

    #[test]
    fn unhealthy_existing_service_can_be_selected_for_repair() {
        let (mut container, image) = fixture();
        container["State"]["Running"] = serde_json::json!(false);
        container["State"]["Health"]["Status"] = serde_json::json!("unhealthy");
        assert!(runtime_identity(&container, &image, "myriad", "updater", false).is_ok());
        assert!(runtime_identity(&container, &image, "myriad", "updater", true).is_err());
    }

    #[test]
    fn adopts_healthy_manual_tag_deployment_using_image_baked_version() {
        let (container, image) = fixture();
        let actual = runtime_identity(&container, &image, "myriad", "updater", true).unwrap();
        assert_eq!(actual.version, "v0.4.13");
        assert_eq!(
            actual.image,
            format!(
                "docker.io/somekawahitomi/myriad-updater@sha256:{}",
                "c".repeat(64)
            )
        );
    }

    #[test]
    fn changing_container_tag_does_not_upgrade_a_pinned_old_image() {
        let (mut container, mut image) = fixture();
        container["Config"]["Image"] = image["RepoDigests"][0].clone();
        container["Config"]["Env"] = json!(["UPDATER_TAG=v0.4.13", "MYRIAD_VERSION=v0.4.13"]);
        image["Config"]["Env"] = json!(["MYRIAD_VERSION=v0.4.6"]);

        let actual = runtime_identity(&container, &image, "myriad", "updater", true).unwrap();
        assert_eq!(actual.version, "v0.4.6");
        assert_eq!(actual.image_id, container["Image"].as_str().unwrap());
    }

    #[test]
    fn unhealthy_foreign_or_malformed_runtime_is_not_adopted() {
        let (container, image) = fixture();
        for (pointer, value) in [
            ("/State/Health/Status", json!("starting")),
            ("/State/Running", json!(false)),
            (
                "/Config/Labels/com.docker.compose.project",
                json!("another"),
            ),
            (
                "/Config/Labels/com.docker.compose.service",
                json!("backend"),
            ),
            ("/Config/Image", json!("evil.example/updater:v0.4.13")),
            ("/Image", json!("sha256:invalid")),
        ] {
            let mut changed = container.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(
                runtime_identity(&changed, &image, "myriad", "updater", true).is_err(),
                "{pointer}"
            );
        }
        for (pointer, value) in [
            (
                "/RepoDigests",
                json!([format!("evil.example/updater@sha256:{}", "c".repeat(64))]),
            ),
            ("/Config/Env", json!(["MYRIAD_VERSION=bad/tag"])),
            ("/Id", json!(format!("sha256:{}", "d".repeat(64)))),
        ] {
            let mut changed = image.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(
                runtime_identity(&container, &changed, "myriad", "updater", true).is_err(),
                "{pointer}"
            );
        }
    }

    #[test]
    fn metadata_identity_does_not_require_health_acceptance() {
        let (mut container, image) = fixture();
        container["State"]["Health"]["Status"] = json!("starting");
        assert!(runtime_identity(&container, &image, "myriad", "updater", false).is_ok());
        assert!(runtime_identity(&container, &image, "myriad", "updater", true).is_err());
    }
}
