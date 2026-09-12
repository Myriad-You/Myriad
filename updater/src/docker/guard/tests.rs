use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, Uri};
use bytes::Bytes;
use serde_json::{json, Value};

use crate::config::SecretString;

use super::classify::{classify_request, Decision};
use super::config::{
    canonicalize_trusted_digest_ref, digest_reference_matches, ensure_host_policy_file,
    validate_guard_image_ref, validate_host_policy_path,
};
use super::forward::GenericMutationLease;
use super::self_update::{
    ensure_no_business_update, finalize_or_fail_orphaned_pending_handoff, handle_self_update,
    handoff_attempt_from_inspect, helper_exit_code_from_inspect, persist_recovery_attempt,
    prevent_release_downgrade, recovery_attempt_from_status, recovery_is_durably_exhausted,
    validate_self_update_tag, HandoffAttempt, SelfUpdateRequestBody, SELF_UPDATE_TOKEN_HEADER,
};
use super::validate::{
    allowlisted_network_name, authorize_guard_network_attachment, managed_project_service,
    validate_container_create, validate_container_rename, validate_endpoint_settings,
    validate_image_pull,
};
use super::{
    strip_api_version, GuardConfig, GuardState, SELF_UPDATE_GATE, TRUSTED_GUARD_REPOSITORY,
};

fn state() -> GuardState {
    state_with_visible_root(PathBuf::from("/host/compose"))
}

fn state_with_visible_root(visible_root: PathBuf) -> GuardState {
    GuardState {
        config: Arc::new(GuardConfig {
            listen: "127.0.0.1:2375".parse().unwrap(),
            socket_path: "/var/run/docker.sock".into(),
            project: "myriad".into(),
            compose_network: "myriad-net".into(),
            admin_network: "myriad-admin-net".into(),
            guard_network: "myriad-docker-guard-net".into(),
            compose_dir: visible_root,
            state_dir: "/host/state".into(),
            expected_guard_image: format!("{TRUSTED_GUARD_REPOSITORY}@sha256:{}", "a".repeat(64)),
            self_update_token: SecretString::new("g7N2pQ8xV4mK6rT9wY3zA5bC1dF0hJ8l"),
            allow_unpinned_dev: false,
            allowed_images: [
                "docker.io/example/backend".into(),
                "docker.io/example/frontend".into(),
                "docker.io/example/proxy".into(),
                "postgres".into(),
            ]
            .into_iter()
            .collect(),
            service_images: [
                ("backend".into(), "docker.io/example/backend".into()),
                (
                    "backend-volume-init".into(),
                    "docker.io/example/backend".into(),
                ),
                ("frontend".into(), "docker.io/example/frontend".into()),
                ("proxy".into(), "docker.io/example/proxy".into()),
                ("postgres".into(), "postgres".into()),
            ]
            .into_iter()
            .collect(),
        }),
        host_compose_root: Arc::new(PathBuf::from("/srv/myriad")),
        mutation_gate: Arc::new(AtomicUsize::new(0)),
    }
}

#[test]
fn compromised_updater_cannot_inject_mutable_or_foreign_image_identity() {
    for tag in ["latest", "preview", "../v9.9.9", "v1.2.3;id"] {
        assert!(validate_self_update_tag(tag).is_err(), "accepted {tag}");
    }
    assert!(validate_self_update_tag("v1.2.3").is_ok());
    assert!(validate_self_update_tag("dev-0123456").is_ok());
}

#[test]
fn self_update_request_rejects_repo_digest_and_command_injection_fields() {
    let injected = br#"{
        "target_tag":"v1.2.3",
        "trust_path":"dockerhub_tag",
        "repo":"docker.io/attacker/root",
        "digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "command":["sh","-c","id"]
    }"#;
    assert!(serde_json::from_slice::<SelfUpdateRequestBody>(injected).is_err());
}

#[tokio::test]
async fn self_update_requires_the_host_policy_capability_before_parsing_body() {
    let missing = Request::builder()
        .method(Method::POST)
        .uri("/_myriad/self-update")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(
        handle_self_update(state(), missing).await.status(),
        StatusCode::UNAUTHORIZED
    );

    let wrong = Request::builder()
        .method(Method::POST)
        .uri("/_myriad/self-update")
        .header(SELF_UPDATE_TOKEN_HEADER, "x".repeat(40))
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(
        handle_self_update(state(), wrong).await.status(),
        StatusCode::UNAUTHORIZED
    );

    let valid = Request::builder()
        .method(Method::POST)
        .uri("/_myriad/self-update")
        .header(SELF_UPDATE_TOKEN_HEADER, "g7N2pQ8xV4mK6rT9wY3zA5bC1dF0hJ8l")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(
        handle_self_update(state(), valid).await.status(),
        StatusCode::BAD_REQUEST
    );
}

#[test]
fn mutation_gate_is_exclusive_and_clone_safe() {
    let gate = Arc::new(AtomicUsize::new(0));
    let lease = GenericMutationLease::acquire(gate.clone()).unwrap();
    let response_lease = lease.clone();
    drop(lease);
    assert_eq!(gate.load(Ordering::SeqCst), 1);
    assert!(gate
        .compare_exchange(0, SELF_UPDATE_GATE, Ordering::SeqCst, Ordering::SeqCst)
        .is_err());
    drop(response_lease);
    assert_eq!(gate.load(Ordering::SeqCst), 0);
    gate.store(SELF_UPDATE_GATE, Ordering::SeqCst);
    assert!(GenericMutationLease::acquire(gate).is_err());
}

#[test]
fn self_update_rejects_nonempty_or_unsafe_job_state() {
    let state_root = tempfile::tempdir().unwrap();
    let mut state = state();
    Arc::make_mut(&mut state.config).state_dir = state_root.path().to_path_buf();
    assert!(ensure_no_business_update(&state).is_ok());
    std::fs::write(state_root.path().join("job.current"), "job-123").unwrap();
    assert!(ensure_no_business_update(&state).is_err());
}

#[test]
fn guard_identity_requires_trusted_repository_and_exact_digest() {
    let valid = format!(
        "{TRUSTED_GUARD_REPOSITORY}@sha256:{}",
        "0123456789abcdef".repeat(4)
    );
    assert!(validate_guard_image_ref(&valid, false).is_ok());
    assert!(validate_guard_image_ref("evil.example/guard@sha256:aaaaaaaa", false).is_err());
    assert!(
        validate_guard_image_ref("docker.io/somekawahitomi/myriad-updater:latest", false).is_err()
    );
    assert!(validate_guard_image_ref(
        &format!("{TRUSTED_GUARD_REPOSITORY}@sha256:{}", "g".repeat(64)),
        false
    )
    .is_err());
    assert!(validate_guard_image_ref("myriad-updater-dev:v0.0.0-dev", true).is_ok());
}

#[test]
fn trusted_digest_canonicalizes_engine_repodigests_without_registry_prefix() {
    let digest = "869973a4d9b4aba6383fdc6aba62b6a908328aebcce748f34ac1f5590193b0b9";
    let canonical = format!("{TRUSTED_GUARD_REPOSITORY}@sha256:{digest}");
    assert_eq!(
        canonicalize_trusted_digest_ref(&format!("somekawahitomi/myriad-updater@sha256:{digest}"))
            .unwrap(),
        canonical
    );
    assert_eq!(
        canonicalize_trusted_digest_ref(&canonical).unwrap(),
        canonical
    );
    assert!(canonicalize_trusted_digest_ref(&format!(
        "evil.example/myriad-updater@sha256:{digest}"
    ))
    .is_err());
    assert!(
        canonicalize_trusted_digest_ref(&format!("somekawahitomi/myriad-updater:{digest}"))
            .is_err()
    );
}

#[test]
fn host_policy_path_accepts_only_the_container_file() {
    assert!(validate_host_policy_path("/guard-policy/docker-guard.env").is_ok());
    assert!(validate_host_policy_path("guard-policy/docker-guard.env").is_err());
    assert!(validate_host_policy_path("/tmp/docker-guard.env").is_err());
    assert!(validate_host_policy_path("../etc/passwd").is_err());
}

#[test]
fn ensure_host_policy_file_writes_once_and_skips_missing_parent() {
    let missing = PathBuf::from("/no/such/myriad-policy/docker-guard.env");
    let cfg = state().config.as_ref().clone();
    assert!(ensure_host_policy_file(&cfg, &missing).is_ok());

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("docker-guard.env");
    assert!(ensure_host_policy_file(&cfg, &path).is_ok());
    let first = fs::read_to_string(&path).unwrap();
    assert!(first.contains("DOCKER_GUARD_IMAGE="));
    assert!(first.contains("GUARD_SELF_UPDATE_TOKEN="));
    assert!(first.contains("MYRIAD_GUARD_ENV_FILE=guard-policy/docker-guard.env"));
    let pinned = first.clone();
    assert!(ensure_host_policy_file(&cfg, &path).is_ok());
    assert_eq!(fs::read_to_string(&path).unwrap(), pinned);

    fs::write(
        &path,
        "DOCKER_GUARD_IMAGE=${DOCKER_GUARD_IMAGE:?Set DOCKER_GUARD_IMAGE in .env}\n",
    )
    .unwrap();
    assert!(ensure_host_policy_file(&cfg, &path).is_ok());
    let healed = fs::read_to_string(&path).unwrap();
    assert!(healed.contains(&cfg.expected_guard_image));
    assert!(!healed.contains("${DOCKER_GUARD_IMAGE:?"));
}

#[test]
fn ensure_host_policy_file_heals_legacy_compose_path() {
    let cfg = state().config.as_ref().clone();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("docker-guard.env");
    let token = "keep-this-host-token-value-32chars";
    fs::write(
        &path,
        format!(
            "DOCKER_GUARD_IMAGE={}\n\
             GUARD_SELF_UPDATE_TOKEN={token}\n\
             GUARD_COMPOSE_PROJECT_NAME=myriad\n\
             GUARD_MYRIAD_DOCKER_NETWORK=myriad-net\n\
             GUARD_MYRIAD_ADMIN_NETWORK=myriad-admin-net\n\
             GUARD_MYRIAD_DOCKER_GUARD_NETWORK=myriad-docker-guard-net\n\
             MYRIAD_GUARD_ENV_FILE=/etc/myriad/docker-guard.env\n",
            cfg.expected_guard_image
        ),
    )
    .unwrap();
    assert!(ensure_host_policy_file(&cfg, &path).is_ok());
    let healed = fs::read_to_string(&path).unwrap();
    assert!(healed.contains(&format!("DOCKER_GUARD_IMAGE={}", cfg.expected_guard_image)));
    assert!(healed.contains(&format!("GUARD_SELF_UPDATE_TOKEN={token}")));
    assert!(healed.contains("MYRIAD_GUARD_ENV_FILE=guard-policy/docker-guard.env"));
    assert!(!healed.contains("MYRIAD_GUARD_ENV_FILE=/etc/myriad/docker-guard.env"));

    let again = healed.clone();
    assert!(ensure_host_policy_file(&cfg, &path).is_ok());
    assert_eq!(fs::read_to_string(&path).unwrap(), again);
}

#[test]
fn release_self_update_rejects_semver_downgrade() {
    assert!(prevent_release_downgrade("v1.2.3", "v1.2.2").is_err());
    assert!(prevent_release_downgrade("v1.2.3", "v1.2.3").is_ok());
    assert!(prevent_release_downgrade("v1.2.3", "v1.3.0").is_ok());
}

fn create(service: &str, image: &str, host: Value) -> Bytes {
    Bytes::from(
        serde_json::to_vec(&json!({
            "Image": image,
            "Labels": {
                "com.docker.compose.project": "myriad",
                "com.docker.compose.service": service,
            },
            "HostConfig": host,
        }))
        .unwrap(),
    )
}

#[test]
fn digest_pinned_business_images_keep_the_existing_guard_boundary() {
    let digest = "a".repeat(64);
    for (service, repo) in [("backend", "backend"), ("frontend", "frontend")] {
        let body = create(
            service,
            &format!("docker.io/example/{repo}:dev-abcdef0@sha256:{digest}"),
            json!({}),
        );
        assert!(validate_container_create(&state(), &body).is_ok());
        let wrong_repo = create(
            service,
            &format!("docker.io/evil/{repo}@sha256:{digest}"),
            json!({}),
        );
        assert!(validate_container_create(&state(), &wrong_repo).is_err());
    }
    let init = backend_volume_init_create(
        "0:0",
        "MYRIAD_VOLUME_INIT_ONLY=true",
        json!({
            "AutoRemove":false, "Binds":["myriad_backend_cache:/app/cache:rw", "myriad_backend_data:/app/data:rw"],
            "NetworkMode":"none", "SecurityOpt":["no-new-privileges:true"]
        }),
    );
    let mut value: Value = serde_json::from_slice(&init).unwrap();
    value["Image"] = json!(format!(
        "docker.io/example/backend:dev-abcdef0@sha256:{digest}"
    ));
    assert!(
        validate_container_create(&state(), &Bytes::from(serde_json::to_vec(&value).unwrap()))
            .is_ok()
    );
}

#[test]
fn backend_create_allows_only_named_project_volumes() {
    let body = create(
        "backend",
        "docker.io/example/backend:v1",
        json!({"Binds": ["myriad_backend_cache:/app/cache:rw"]}),
    );
    assert!(validate_container_create(&state(), &body).is_ok());
}

fn backend_volume_init_create(user: &str, init_env: &str, host: Value) -> Bytes {
    Bytes::from(
        serde_json::to_vec(&json!({
            "Image": "docker.io/example/backend:v1",
            "User": user,
            "Env": [
                "DATABASE_URL=postgres://example",
                init_env,
            ],
            "Labels": {
                "com.docker.compose.project": "myriad",
                "com.docker.compose.service": "backend-volume-init",
                "com.docker.compose.oneoff": "True",
            },
            "HostConfig": host,
        }))
        .unwrap(),
    )
}

#[test]
fn backend_volume_init_allows_only_narrow_root_one_off() {
    let allowed = backend_volume_init_create(
        "0:0",
        "MYRIAD_VOLUME_INIT_ONLY=true",
        json!({
            "AutoRemove": false,
            "Binds": [
                "myriad_backend_cache:/app/cache:rw",
                "myriad_backend_data:/app/data:rw"
            ],
            "NetworkMode": "none",
            "SecurityOpt": ["no-new-privileges:true"]
        }),
    );
    assert!(validate_container_create(&state(), &allowed).is_ok());

    let missing_flag = backend_volume_init_create(
        "0:0",
        "MYRIAD_VOLUME_INIT_ONLY=false",
        json!({
            "AutoRemove": true,
            "Binds": [
                "myriad_backend_cache:/app/cache:rw",
                "myriad_backend_data:/app/data:rw"
            ],
            "SecurityOpt": ["no-new-privileges:true"]
        }),
    );
    assert!(validate_container_create(&state(), &missing_flag)
        .unwrap_err()
        .contains("narrow root init mode"));

    let auto_remove_payload = backend_volume_init_create(
        "0:0",
        "MYRIAD_VOLUME_INIT_ONLY=true",
        json!({
            "AutoRemove": true,
            "Binds": [
                "myriad_backend_cache:/app/cache:rw",
                "myriad_backend_data:/app/data:rw"
            ],
            "SecurityOpt": ["no-new-privileges:true"]
        }),
    );
    assert!(validate_container_create(&state(), &auto_remove_payload)
        .unwrap_err()
        .contains("narrow root init mode"));

    let missing_data_volume = backend_volume_init_create(
        "0:0",
        "MYRIAD_VOLUME_INIT_ONLY=true",
        json!({
            "AutoRemove": false,
            "Binds": ["myriad_backend_cache:/app/cache:rw"],
            "SecurityOpt": ["no-new-privileges:true"]
        }),
    );
    assert!(validate_container_create(&state(), &missing_data_volume)
        .unwrap_err()
        .contains("narrow root init mode"));

    let arbitrary_root_backend = backend_volume_init_create(
        "root:root",
        "MYRIAD_VOLUME_INIT_ONLY=true",
        json!({
            "AutoRemove": false,
            "Binds": [
                "myriad_backend_cache:/app/cache:rw",
                "myriad_backend_data:/app/data:rw"
            ],
            "SecurityOpt": ["no-new-privileges:true"]
        }),
    );
    assert!(validate_container_create(&state(), &arbitrary_root_backend)
        .unwrap_err()
        .contains("narrow root init mode"));

    let networked_initializer = backend_volume_init_create(
        "0:0",
        "MYRIAD_VOLUME_INIT_ONLY=true",
        json!({
            "AutoRemove": false,
            "Binds": [
                "myriad_backend_cache:/app/cache:rw",
                "myriad_backend_data:/app/data:rw"
            ],
            "NetworkMode": "myriad-net",
            "SecurityOpt": ["no-new-privileges:true"]
        }),
    );
    assert!(validate_container_create(&state(), &networked_initializer)
        .unwrap_err()
        .contains("narrow root init mode"));

    let non_root_initializer = backend_volume_init_create(
        "1000:1000",
        "MYRIAD_VOLUME_INIT_ONLY=true",
        json!({
            "AutoRemove": false,
            "Binds": [
                "myriad_backend_cache:/app/cache:rw",
                "myriad_backend_data:/app/data:rw"
            ],
            "NetworkMode": "none",
            "SecurityOpt": ["no-new-privileges:true"]
        }),
    );
    assert!(validate_container_create(&state(), &non_root_initializer)
        .unwrap_err()
        .contains("narrow root init mode"));

    let regular_root_backend = Bytes::from(
        serde_json::to_vec(&json!({
            "Image": "docker.io/example/backend:v1",
            "User": "0:0",
            "Env": ["MYRIAD_VOLUME_INIT_ONLY=true"],
            "Labels": {
                "com.docker.compose.project": "myriad",
                "com.docker.compose.service": "backend",
                "com.docker.compose.oneoff": "True"
            },
            "HostConfig": {
                "AutoRemove": false,
                "Binds": [
                    "myriad_backend_cache:/app/cache:rw",
                    "myriad_backend_data:/app/data:rw"
                ],
                "NetworkMode": "none",
                "SecurityOpt": ["no-new-privileges:true"]
            }
        }))
        .unwrap(),
    );
    assert!(validate_container_create(&state(), &regular_root_backend)
        .unwrap_err()
        .contains("explicit root user"));
}

#[test]
fn backend_host_bind_is_denied() {
    let body = create(
        "backend",
        "docker.io/example/backend:v1",
        json!({"Binds": ["/:/host:rw"]}),
    );
    assert!(validate_container_create(&state(), &body)
        .unwrap_err()
        .contains("host bind"));
}

#[test]
fn privileged_create_is_denied() {
    let body = create(
        "frontend",
        "docker.io/example/frontend:v1",
        json!({"Privileged": true}),
    );
    assert!(validate_container_create(&state(), &body)
        .unwrap_err()
        .contains("Privileged"));
}

#[test]
fn host_runtime_and_daemon_file_write_overrides_are_denied() {
    for host in [
        json!({"Runtime": "custom-root-runtime"}),
        json!({"ContainerIDFile": "/etc/cron.d/escape"}),
        json!({"CgroupParent": "/system.slice"}),
        json!({"Annotations": {"run.oci.handler": "host-runtime"}}),
        json!({"LogConfig": {"Type": "syslog", "Config": {}}}),
    ] {
        let body = create("frontend", "docker.io/example/frontend:v1", host);
        assert!(validate_container_create(&state(), &body).is_err());
    }
}

#[test]
fn compose_v5_zero_log_config_is_treated_as_daemon_json_file() {
    let body = create(
        "frontend",
        "docker.io/example/frontend:v1",
        json!({"LogConfig": {"Type": "", "Config": {}}}),
    );
    assert!(validate_container_create(&state(), &body).is_ok());
}

#[test]
fn generic_api_cannot_recreate_tcb_services() {
    for service in ["updater", "updater-gateway", "docker-guard"] {
        let body = create(service, "docker.io/example/updater:v1", json!({}));
        assert!(validate_container_create(&state(), &body)
            .unwrap_err()
            .contains("generic updater API"));
    }
}

#[test]
fn generic_create_cannot_reserve_control_plane_container_names() {
    let body = create("backend", "docker.io/example/backend:v1", json!({}));
    for name in [
        "myriad-tcb-self-update",
        "myriad-tcb-self-update-recovery",
        "myriad-tcb-self-update-recovery-exhausted",
        "myriad-docker-guard",
        "myriad-updater",
        "myriad-updater-gateway",
    ] {
        let uri: Uri = format!("/v1.51/containers/create?name={name}")
            .parse()
            .unwrap();
        assert!(classify_request(&state(), &Method::POST, &uri, &body).is_err());
    }
}

#[test]
fn persisted_helper_identity_recovers_only_guard_created_handoff_intent() {
    let previous = format!(
        "docker.io/somekawahitomi/myriad-updater@sha256:{}",
        "a".repeat(64)
    );
    let target = format!(
        "docker.io/somekawahitomi/myriad-updater@sha256:{}",
        "b".repeat(64)
    );
    let inspect = json!({
        "Path": "/usr/local/bin/myriad-tcb-self-update",
        "HostConfig": {"NetworkMode": "none", "ReadonlyRootfs": true},
        "Config": {
            "Image": target.clone(),
            "Env": [
                format!("{}={previous}", super::super::self_update_helper::ENV_PREVIOUS_IMAGE),
                format!("{}={target}", super::super::self_update_helper::ENV_TARGET_IMAGE),
                format!("{}=v1.2.2", super::super::self_update_helper::ENV_PREVIOUS_TAG),
                format!("{}=v1.2.3", super::super::self_update_helper::ENV_TARGET_TAG),
            ]
        }
    });
    let attempt = handoff_attempt_from_inspect(&inspect).unwrap();
    assert_eq!(attempt.previous_tag, "v1.2.2");
    assert_eq!(attempt.target_tag, "v1.2.3");
    assert!(!attempt.recovery_only);

    let mut recovery = inspect.clone();
    recovery["Config"]["Env"]
        .as_array_mut()
        .unwrap()
        .push(json!(format!(
            "{}=1",
            super::super::self_update_helper::ENV_RECOVERY_ONLY
        )));
    assert!(
        handoff_attempt_from_inspect(&recovery)
            .unwrap()
            .recovery_only
    );

    let mut forged = inspect;
    forged["Config"]["Image"] = json!(previous);
    assert!(handoff_attempt_from_inspect(&forged).is_err());
}

#[tokio::test]
async fn orphaned_pending_handoff_becomes_a_fresh_failure() {
    let root = tempfile::tempdir().unwrap();
    let mut state = state();
    Arc::make_mut(&mut state.config).state_dir = root.path().to_path_buf();
    let path = root.path().join("self-update-last.json");
    let pending = super::super::self_update_helper::SelfUpdateLastStatus::pending_before_handoff(
        "v1.2.3".into(),
        "v1.2.2".into(),
    );
    super::super::self_update_helper::write_status(&path, &pending).unwrap();

    assert!(!finalize_or_fail_orphaned_pending_handoff(&state).await);

    let status: super::super::self_update_helper::SelfUpdateLastStatus =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(matches!(
        status.status,
        super::super::self_update_helper::SelfUpdateOutcome::Failed
    ));
    assert!(status
        .error
        .unwrap()
        .contains("target TCB was not fully active"));
    assert_eq!(state.mutation_gate.load(Ordering::SeqCst), SELF_UPDATE_GATE);
}

#[test]
fn recovery_retry_budget_survives_guard_restart() {
    let root = tempfile::tempdir().unwrap();
    let mut state = state();
    Arc::make_mut(&mut state.config).state_dir = root.path().to_path_buf();
    let attempt = HandoffAttempt {
        previous_image: format!(
            "docker.io/somekawahitomi/myriad-updater@sha256:{}",
            "a".repeat(64)
        ),
        target_image: format!(
            "docker.io/somekawahitomi/myriad-updater@sha256:{}",
            "b".repeat(64)
        ),
        previous_tag: "v1.2.2".into(),
        target_tag: "v1.2.3".into(),
        recovery_only: true,
    };

    assert!(persist_recovery_attempt(&state, &attempt, 1));
    assert_eq!(recovery_attempt_from_status(&state, &attempt), 1);

    let status: super::super::self_update_helper::SelfUpdateLastStatus =
        serde_json::from_slice(&std::fs::read(root.path().join("self-update-last.json")).unwrap())
            .unwrap();
    assert!(matches!(
        status.status,
        super::super::self_update_helper::SelfUpdateOutcome::Pending
    ));
    assert_eq!(status.recovery_attempt, 1);
}

#[test]
fn third_recovery_failure_is_exhausted_even_before_final_status_write() {
    assert!(recovery_is_durably_exhausted(Some(1), 2, false));
    assert!(recovery_is_durably_exhausted(Some(1), 0, true));
    assert!(!recovery_is_durably_exhausted(Some(1), 1, false));
    assert!(!recovery_is_durably_exhausted(Some(0), 2, true));
    assert!(!recovery_is_durably_exhausted(None, 2, true));
}

#[test]
fn staged_recovery_is_not_mistaken_for_successful_exit() {
    let created = json!({
        "State": {"Status": "created", "Running": false, "ExitCode": 0}
    });
    let running = json!({
        "State": {"Status": "running", "Running": true, "ExitCode": 0}
    });
    let succeeded = json!({
        "State": {"Status": "exited", "Running": false, "ExitCode": 0}
    });
    let failed = json!({
        "State": {"Status": "exited", "Running": false, "ExitCode": 1}
    });

    assert_eq!(helper_exit_code_from_inspect(&created).unwrap(), None);
    assert_eq!(helper_exit_code_from_inspect(&running).unwrap(), None);
    assert_eq!(helper_exit_code_from_inspect(&succeeded).unwrap(), Some(0));
    assert_eq!(helper_exit_code_from_inspect(&failed).unwrap(), Some(1));
}

#[test]
fn repo_digest_match_accepts_docker_hub_canonicalization_only() {
    let expected = format!(
        "docker.io/somekawahitomi/myriad-updater@sha256:{}",
        "a".repeat(64)
    );
    let canonical = format!("somekawahitomi/myriad-updater@sha256:{}", "a".repeat(64));
    assert!(digest_reference_matches(&canonical, &expected));
    assert!(!digest_reference_matches(
        &format!("attacker/myriad-updater@sha256:{}", "a".repeat(64)),
        &expected
    ));
    assert!(!digest_reference_matches(
        &format!("somekawahitomi/myriad-updater@sha256:{}", "b".repeat(64)),
        &expected
    ));
}

#[test]
fn postgres_symlink_bind_is_denied() {
    let visible = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink("/etc", visible.path().join("pgdata")).unwrap();
    let state = state_with_visible_root(visible.path().to_path_buf());
    let body = create(
        "postgres",
        "postgres:18-alpine",
        json!({"Binds": ["/srv/myriad/pgdata:/var/lib/postgresql:rw"]}),
    );
    assert!(validate_container_create(&state, &body)
        .unwrap_err()
        .contains("symbolic links"));
}

#[test]
fn service_repository_mapping_rejects_cross_service_images() {
    let body = create("frontend", "docker.io/example/backend:v1", json!({}));
    assert!(validate_container_create(&state(), &body)
        .unwrap_err()
        .contains("fixed service repository"));
}

#[test]
fn bind_mount_propagation_is_denied() {
    let visible = tempfile::tempdir().unwrap();
    std::fs::create_dir(visible.path().join("pgdata")).unwrap();
    let state = state_with_visible_root(visible.path().to_path_buf());
    let string_bind = create(
        "postgres",
        "postgres:18-alpine",
        json!({"Binds": ["/srv/myriad/pgdata:/var/lib/postgresql:rw,rshared"]}),
    );
    assert!(validate_container_create(&state, &string_bind)
        .unwrap_err()
        .contains("propagation"));

    let structured_mount = create(
        "postgres",
        "postgres:18-alpine",
        json!({"Mounts": [{
            "Type": "bind",
            "Source": "/srv/myriad/pgdata",
            "Target": "/var/lib/postgresql",
            "BindOptions": {"Propagation": "rshared"}
        }]}),
    );
    assert!(validate_container_create(&state, &structured_mount)
        .unwrap_err()
        .contains("propagation"));
}

#[test]
fn exec_and_unknown_mutations_are_denied() {
    let request = Uri::from_static("/v1.51/containers/myriad-backend/exec");
    assert!(classify_request(&state(), &Method::POST, &request, &Bytes::new()).is_err());
}

#[test]
fn initializer_logs_remain_project_scoped_read_only_access() {
    let request = Uri::from_static("/v1.51/containers/init-container-id/logs?stdout=1&stderr=1");
    assert_eq!(
        classify_request(&state(), &Method::GET, &request, &Bytes::new()).unwrap(),
        Decision::ProjectContainer("init-container-id".into())
    );
    assert!(classify_request(&state(), &Method::POST, &request, &Bytes::new()).is_err());
}

#[test]
fn compose_recreate_rename_is_narrowly_allowed() {
    for name in [
        "myriad-backend",
        "myriad-backend-1",
        "myriad_backend_1",
        "0fea459923c4_myriad-backend-1",
    ] {
        let uri: Uri = format!("/v1.51/containers/abc123/rename?name={name}")
            .parse()
            .unwrap();
        assert!(validate_container_rename(&state(), &uri).is_ok(), "{name}");
    }

    for name in ["docker-guard", "other-backend-1", "abc_myriad-backend-1"] {
        let uri: Uri = format!("/v1.51/containers/abc123/rename?name={name}")
            .parse()
            .unwrap();
        assert!(validate_container_rename(&state(), &uri).is_err(), "{name}");
    }
}

#[test]
fn image_pull_is_repository_allowlisted() {
    let allowed =
        Uri::from_static("/v1.51/images/create?fromImage=docker.io%2Fexample%2Fbackend&tag=v1");
    assert!(validate_image_pull(&state(), &allowed, &Bytes::new()).is_ok());
    let empty_platform = Uri::from_static(
        "/v1.51/images/create?fromImage=docker.io%2Fexample%2Fbackend&tag=v1&platform=",
    );
    assert!(validate_image_pull(&state(), &empty_platform, &Bytes::new()).is_ok());
    let pinned_platform = Uri::from_static(
        "/v1.51/images/create?fromImage=docker.io%2Fexample%2Fbackend&tag=v1&platform=linux%2Farm64",
    );
    assert!(validate_image_pull(&state(), &pinned_platform, &Bytes::new()).is_ok());
    let denied = Uri::from_static("/v1.51/images/create?fromImage=evil%2Fpayload&tag=latest");
    assert!(validate_image_pull(&state(), &denied, &Bytes::new()).is_err());

    let imported = Uri::from_static(
        "/v1.51/images/create?fromImage=docker.io%2Fexample%2Fbackend&tag=v1&fromSrc=https%3A%2F%2Fevil.invalid%2Fimage.tar",
    );
    assert!(validate_image_pull(&state(), &imported, &Bytes::new()).is_err());
    assert!(validate_image_pull(&state(), &allowed, &Bytes::from_static(b"tar payload")).is_err());
}

#[test]
fn image_tag_requires_allowlisted_source_and_target() {
    let allowed = Uri::from_static("/v1.51/images/docker.io%2Fexample%2Fbackend:v1/tag?repo=docker.io%2Fexample%2Fbackend&tag=myriad-rollback");
    assert!(classify_request(&state(), &Method::POST, &allowed, &Bytes::new()).is_ok());

    let denied_source = Uri::from_static("/v1.51/images/evil%2Fpayload:v1/tag?repo=docker.io%2Fexample%2Fbackend&tag=myriad-rollback");
    assert!(classify_request(&state(), &Method::POST, &denied_source, &Bytes::new()).is_err());

    let cross_repository = Uri::from_static("/v1.51/images/docker.io%2Fexample%2Fbackend:v1/tag?repo=docker.io%2Fexample%2Ffrontend&tag=v1");
    assert!(classify_request(&state(), &Method::POST, &cross_repository, &Bytes::new()).is_err());

    let unescaped_slashes = Uri::from_static("/v1.51/images/docker.io/example/backend:v1/tag?repo=docker.io%2Fexample%2Fbackend&tag=myriad-rollback");
    assert!(classify_request(&state(), &Method::POST, &unescaped_slashes, &Bytes::new()).is_ok());

    let restore_version_ref = Uri::from_static("/v1.51/images/docker.io%2Fexample%2Fbackend:myriad-rollback/tag?repo=docker.io%2Fexample%2Fbackend&tag=v1");
    assert!(classify_request(&state(), &Method::POST, &restore_version_ref, &Bytes::new()).is_ok());
}

#[test]
fn api_version_prefix_is_normalized() {
    assert_eq!(
        strip_api_version("/v1.51/containers/json"),
        "/containers/json"
    );
    assert_eq!(strip_api_version("/containers/json"), "/containers/json");
}

fn create_with_networking(service: &str, image: &str, host: Value, endpoints: Value) -> Bytes {
    Bytes::from(
        serde_json::to_vec(&json!({
            "Image": image,
            "Labels": {
                "com.docker.compose.project": "myriad",
                "com.docker.compose.service": service,
            },
            "HostConfig": host,
            "NetworkingConfig": {
                "EndpointsConfig": endpoints,
            },
        }))
        .unwrap(),
    )
}

#[test]
fn generic_api_cannot_create_any_guard_network_client() {
    let s = state();
    let backend = create_with_networking(
        "backend",
        "docker.io/example/backend:v1",
        json!({}),
        json!({"myriad-docker-guard-net": {}}),
    );
    assert!(
        validate_container_create(&s, &backend)
            .unwrap_err()
            .contains("only the updater service may attach to the docker-guard network"),
        "backend must not join guard-net at create time"
    );

    let postgres = create_with_networking(
        "postgres",
        "postgres:18-alpine",
        json!({}),
        json!({"myriad-docker-guard-net": {}}),
    );
    assert!(validate_container_create(&s, &postgres).is_err());

    let updater = create_with_networking(
        "updater",
        "docker.io/example/updater:v1",
        json!({}),
        json!({
            "myriad-admin-net": {},
            "myriad-docker-guard-net": {},
        }),
    );
    assert!(validate_container_create(&s, &updater).is_err());
}

#[test]
fn admin_network_is_allowlisted_but_guard_stays_updater_only() {
    let s = state();
    let backend = create_with_networking(
        "backend",
        "docker.io/example/backend:v1",
        json!({}),
        json!({
            "myriad-net": {},
            "myriad-admin-net": {},
        }),
    );
    assert!(
        validate_container_create(&s, &backend).is_ok(),
        "backend may dual-home business + admin nets"
    );

    let backend_guard = create_with_networking(
        "backend",
        "docker.io/example/backend:v1",
        json!({}),
        json!({
            "myriad-net": {},
            "myriad-admin-net": {},
            "myriad-docker-guard-net": {},
        }),
    );
    assert!(
        validate_container_create(&s, &backend_guard)
            .unwrap_err()
            .contains("only the updater service may attach to the docker-guard network"),
        "backend must still be denied on guard-net"
    );

    let mode_admin = create(
        "backend",
        "docker.io/example/backend:v1",
        json!({"NetworkMode": "myriad-admin-net"}),
    );
    assert!(validate_container_create(&s, &mode_admin).is_ok());

    let foreign = create_with_networking(
        "backend",
        "docker.io/example/backend:v1",
        json!({}),
        json!({"bridge": {}}),
    );
    assert!(validate_container_create(&s, &foreign)
        .unwrap_err()
        .contains("outside the Myriad allowlist"));
}

#[test]
fn generic_api_rejects_guard_network_mode_for_every_service() {
    let s = state();
    let backend = create(
        "backend",
        "docker.io/example/backend:v1",
        json!({"NetworkMode": "myriad-docker-guard-net"}),
    );
    assert!(validate_container_create(&s, &backend)
        .unwrap_err()
        .contains("only the updater service may attach to the docker-guard network"));

    let updater = create(
        "updater",
        "docker.io/example/updater:v1",
        json!({"NetworkMode": "myriad-docker-guard-net"}),
    );
    assert!(validate_container_create(&s, &updater).is_err());
}

#[test]
fn managed_services_may_still_attach_compose_network() {
    let s = state();
    for service_image in [
        ("backend", "docker.io/example/backend:v1"),
        ("frontend", "docker.io/example/frontend:v1"),
    ] {
        let body = create_with_networking(
            service_image.0,
            service_image.1,
            json!({}),
            json!({"myriad-net": {}}),
        );
        assert!(
            validate_container_create(&s, &body).is_ok(),
            "{} must still attach to compose network",
            service_image.0
        );

        let mode_body = create(
            service_image.0,
            service_image.1,
            json!({"NetworkMode": "myriad-net"}),
        );
        assert!(
            validate_container_create(&s, &mode_body).is_ok(),
            "{} NetworkMode=compose network must remain allowed",
            service_image.0
        );
    }

    assert!(authorize_guard_network_attachment("backend", "myriad-net", &s.config).is_ok());
    assert!(authorize_guard_network_attachment("postgres", "myriad-net", &s.config).is_ok());

    let updater = create_with_networking(
        "updater",
        "docker.io/example/updater:v1",
        json!({}),
        json!({"myriad-net": {}}),
    );
    assert!(validate_container_create(&s, &updater).is_err());
    assert!(authorize_guard_network_attachment("updater", "myriad-net", &s.config).is_err());
}

#[test]
fn endpoint_identity_overrides_are_denied() {
    let s = state();
    assert!(validate_endpoint_settings(
        "backend",
        &json!({"Aliases": ["backend", "myriad-backend"]}),
        &s.config,
    )
    .is_ok());
    for endpoint in [
        json!({"Aliases": ["postgres"]}),
        json!({"IPAMConfig": {"IPv4Address": "172.28.0.2"}}),
        json!({"MacAddress": "02:42:ac:1c:00:02"}),
        json!({"DriverOpts": {"com.example.host": "true"}}),
        json!({"DNSNames": ["postgres"]}),
    ] {
        assert!(validate_endpoint_settings("backend", &endpoint, &s.config).is_err());
    }
}

#[test]
fn guard_network_attachment_policy_is_updater_only() {
    let s = state();
    assert!(
        authorize_guard_network_attachment("backend", "myriad-docker-guard-net", &s.config)
            .is_err()
    );
    assert!(
        authorize_guard_network_attachment("frontend", "myriad-docker-guard-net", &s.config)
            .is_err()
    );
    assert!(
        authorize_guard_network_attachment("postgres", "myriad-docker-guard-net", &s.config)
            .is_err()
    );
    assert!(
        authorize_guard_network_attachment("updater", "myriad-docker-guard-net", &s.config).is_ok()
    );
}

#[test]
fn network_connect_classify_requires_container_field() {
    let uri = Uri::from_static("/v1.51/networks/myriad-docker-guard-net/connect");
    let missing = classify_request(&state(), &Method::POST, &uri, &Bytes::from_static(b"{}"));
    assert!(missing.unwrap_err().contains("Container"));

    let body = Bytes::from(serde_json::to_vec(&json!({"Container": "myriad-backend-1"})).unwrap());
    let decision = classify_request(&state(), &Method::POST, &uri, &body).unwrap();
    assert_eq!(
        decision,
        Decision::ProjectNetworkMutation {
            network: "myriad-docker-guard-net".into(),
            container: "myriad-backend-1".into(),
            endpoint: None,
        }
    );
}

#[test]
fn managed_project_service_and_network_helpers() {
    let s = state();
    let backend = json!({
        "Config": {
            "Labels": {
                "com.docker.compose.project": "myriad",
                "com.docker.compose.service": "backend",
            }
        }
    });
    assert_eq!(
        managed_project_service(&backend, &s.config).as_deref(),
        Some("backend")
    );

    let foreign = json!({
        "Config": {
            "Labels": {
                "com.docker.compose.project": "other",
                "com.docker.compose.service": "backend",
            }
        }
    });
    assert!(managed_project_service(&foreign, &s.config).is_none());

    let guard_net = json!({"Name": "myriad-docker-guard-net"});
    assert_eq!(
        allowlisted_network_name(&guard_net, &s.config).as_deref(),
        Some("myriad-docker-guard-net")
    );
    let other_net = json!({"Name": "bridge"});
    assert!(allowlisted_network_name(&other_net, &s.config).is_none());

    // Connect path: non-updater + guard-net is denied once labels/name are resolved.
    let service = managed_project_service(&backend, &s.config).unwrap();
    let network = allowlisted_network_name(&guard_net, &s.config).unwrap();
    assert!(
        authorize_guard_network_attachment(&service, &network, &s.config)
            .unwrap_err()
            .contains("only the updater service may attach")
    );

    let updater = json!({
        "Config": {
            "Labels": {
                "com.docker.compose.project": "myriad",
                "com.docker.compose.service": "updater",
            }
        }
    });
    let service = managed_project_service(&updater, &s.config).unwrap();
    assert!(authorize_guard_network_attachment(&service, &network, &s.config).is_ok());

    // Compose business network stays open to managed services.
    let compose_net = json!({"Name": "myriad-net"});
    let network = allowlisted_network_name(&compose_net, &s.config).unwrap();
    let service = managed_project_service(&backend, &s.config).unwrap();
    assert!(authorize_guard_network_attachment(&service, &network, &s.config).is_ok());
}
