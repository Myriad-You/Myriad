//! Exercise the helper process with a deterministic Docker CLI, without touching a deployment.
use serde_json::Value;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};

fn handoff(recovery: bool, fail: bool) -> (tempfile::TempDir, std::process::Output) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let docker = root.join("docker");
    fs::write(&docker, r##"#!/bin/sh
set -eu
case "$1 $2" in
  'compose -p')
    case " $* " in
      *' config '*)
        printf '{"services":{"docker-guard":{"image":"%s"},"updater":{"image":"%s"},"updater-gateway":{"image":"%s"}}}' "$MYRIAD_TCB_GUARD_IMAGE" "$MYRIAD_TCB_UPDATER_IMAGE" "$MYRIAD_TCB_GATEWAY_IMAGE" ;;
      *' up '*)
        echo up >> "$TEST_ROOT/calls"
        [ "$TEST_FAIL" = 0 ] || { echo 'injected compose failure' >&2; exit 1; }
        printf '%s\n' "$MYRIAD_TCB_GUARD_IMAGE" "$MYRIAD_TCB_UPDATER_IMAGE" "$MYRIAD_TCB_GATEWAY_IMAGE" > "$TEST_ROOT/running" ;;
      *) exit 9 ;;
    esac ;;
  'image inspect') printf '[{"Id":"%s"}]' "$3" ;;
  'container inspect')
    case "$3" in
      myriad-docker-guard) line=1 ;; myriad-updater) line=2 ;; myriad-updater-gateway) line=3 ;; *) exit 8 ;;
    esac
    id=$(sed -n "${line}p" "$TEST_ROOT/running")
    printf '[{"Image":"%s","State":{"Running":true,"Status":"running","Health":{"Status":"healthy"}}}]' "$id" ;;
  *) exit 7 ;;
esac
"##).unwrap();
    fs::set_permissions(&docker, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(root.join("compose.yaml"), "services: {}\n").unwrap();
    fs::write(root.join(".env"), "UPDATER_TAG=v9.0.0\nKEEP=unchanged\n").unwrap();
    fs::write(root.join("guard.env"), "GUARD_SELF_UPDATE_TOKEN=keep\n").unwrap();
    let image = |ch: char| {
        format!(
            "docker.io/somekawahitomi/myriad-updater@sha256:{}",
            ch.to_string().repeat(64)
        )
    };
    let previous = [image('a'), image('b'), image('c')];
    let mut command = Command::new(env!("CARGO_BIN_EXE_myriad-tcb-self-update"));
    command
        .env(
            "PATH",
            format!("{}:{}", root.display(), std::env::var("PATH").unwrap()),
        )
        .env("TEST_ROOT", root)
        .env("TEST_FAIL", if fail { "1" } else { "0" });
    for (key, value) in [
        ("PREVIOUS_IMAGE", previous[0].clone()),
        ("PREVIOUS_IMAGES", serde_json::to_string(&previous).unwrap()),
        ("TARGET_IMAGE", image('d')),
        ("PREVIOUS_TAG", "v9.0.0".into()),
        ("TARGET_TAG", "preview".into()),
        ("PROJECT", "myriad".into()),
        ("PROJECT_DIRECTORY", root.display().to_string()),
        ("HOST_COMPOSE_ROOT", root.display().to_string()),
        ("COMPOSE_DIR", root.display().to_string()),
        ("APP_ENV_FILE", root.join(".env").display().to_string()),
        (
            "GUARD_ENV_FILE",
            root.join("guard.env").display().to_string(),
        ),
        (
            "STATUS_FILE",
            root.join("self-update-last.json").display().to_string(),
        ),
        ("COMPOSE_NETWORK", "myriad-net".into()),
        ("ADMIN_NETWORK", "myriad-admin-net".into()),
        ("GUARD_NETWORK", "myriad-guard-net".into()),
        ("RECOVERY_ONLY", if recovery { "1" } else { "" }.into()),
    ] {
        command.env(format!("MYRIAD_SELF_UPDATE_{key}"), value);
    }
    let output = command.output().unwrap();
    (dir, output)
}

#[test]
fn switch_failure_stays_pending_and_does_not_start_a_second_rollback_owner() {
    let (dir, output) = handoff(false, true);
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(dir.path().join("calls")).unwrap(),
        "up\n"
    );
    let status: Value =
        serde_json::from_slice(&fs::read(dir.path().join("self-update-last.json")).unwrap())
            .unwrap();
    assert_eq!(status["status"], "pending");
    assert!(
        status["error"]
            .as_str()
            .unwrap()
            .contains("injected compose failure")
    );
}

#[test]
fn recovery_restores_each_previous_image_and_preserves_unrelated_configuration() {
    let (dir, output) = handoff(true, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("compose.yaml")).unwrap(),
        "services: {}\n"
    );
    let images = fs::read_to_string(dir.path().join("running")).unwrap();
    let images: Vec<_> = images.lines().collect();
    assert_eq!(images.len(), 3);
    assert_ne!(images[0], images[1]);
    assert_ne!(images[1], images[2]);
    let env = fs::read_to_string(dir.path().join(".env")).unwrap();
    assert!(env.contains("UPDATER_TAG=v9.0.0"));
    assert!(env.contains("KEEP=unchanged"));
    assert_eq!(
        fs::read_to_string(dir.path().join("calls")).unwrap(),
        "up\n"
    );
}

#[test]
fn branch_target_is_resolved_to_one_image_for_all_services() {
    let (dir, output) = handoff(false, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("compose.yaml")).unwrap(),
        "services: {}\n"
    );
    let images = fs::read_to_string(dir.path().join("running")).unwrap();
    let images: Vec<_> = images.lines().collect();
    assert_eq!(images.len(), 3);
    assert!(images.iter().all(|image| *image == images[0]));
    assert!(
        fs::read_to_string(dir.path().join(".env"))
            .unwrap()
            .contains("UPDATER_TAG=preview")
    );
}
