//! Black-box tests: run the daemon, use its HTTP API, and fake only external services.
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
};

const TOKEN: &str = "87f1c493a572db0e69fe38214cb859a64b3c901d";
const OLD: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NEW: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const REPO: &str = "docker.io/somekawahitomi/myriad-proxy";

struct Mock {
    root: PathBuf,
    guard_mode: usize,
    guard_calls: AtomicUsize,
    pulls: AtomicUsize,
    registry_calls: AtomicUsize,
    release_registry: Semaphore,
    running_target: bool,
    old_digest_missing: bool,
}

impl Mock {
    fn save(&self, file: &str, value: &Value) {
        let path = self.root.join("state").join(file);
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, serde_json::to_vec(value).unwrap()).unwrap();
        fs::rename(temporary, path).unwrap();
    }

    async fn serve(self: Arc<Self>, mut socket: TcpStream) {
        let mut input = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let count = socket.read(&mut buf).await.unwrap_or(0);
            if count == 0 {
                return;
            }
            input.extend_from_slice(&buf[..count]);
            if let Some(end) = input.windows(4).position(|v| v == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&input[..end]).to_lowercase();
                let length = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: ")?.parse::<usize>().ok())
                    .unwrap_or(0);
                if input.len() >= end + 4 + length {
                    break;
                }
            }
        }
        let request = String::from_utf8_lossy(&input);
        let first = request.lines().next().unwrap();
        let mut code = "200 OK";
        let body = if first.starts_with("CONNECT ") {
            if first.contains("hub.docker.com") {
                self.registry_calls.fetch_add(1, Ordering::SeqCst);
                self.release_registry.acquire().await.unwrap().forget();
            }
            code = "502 Bad Gateway";
            "registry unavailable".into()
        } else if first.contains("/_myriad/self-update") {
            assert!(request.contains("\"target_tag\":\"preview\""));
            let call = self.guard_calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                if self.guard_mode == 2 {
                    self.save("self-update-last.json", &json!({"status":"pending", "target_tag":"preview", "previous_tag":"v0.5.3", "at":"guard-accepted"}));
                }
                // Request reached Guard, but its response never reached the caller.
                return;
            }
            code = "409 Conflict";
            "busy".into()
        } else if first.contains("/_ping") {
            "OK".into()
        } else if first.contains("/images/create") {
            self.pulls.fetch_add(1, Ordering::SeqCst);
            "{\"status\":\"complete\"}\n".into()
        } else if first.contains("/images/") {
            let old = first.contains("aaaa");
            let id = if old { OLD } else { NEW };
            let digests = if old && self.old_digest_missing {
                vec![]
            } else {
                vec![format!("{REPO}@{id}")]
            };
            json!({"Id":id,"RepoDigests":digests}).to_string()
        } else if first.contains("/containers/myriad-proxy/json") {
            json!({"Image":if self.running_target { NEW } else { OLD },"State":{"Running":true,"Health":{"Status":"healthy"}}}).to_string()
        } else if first.contains("/containers/") {
            json!({"Mounts":[{"Source":self.root,"Destination":self.root,"Type":"bind"}]})
                .to_string()
        } else if first.contains("backend:1103/health") {
            json!({"version":"v0.5.3","commit_sha":"1234567890123456789012345678901234567890"})
                .to_string()
        } else if first.contains("/healthz") {
            "OK".into()
        } else {
            panic!("unexpected external request: {first}");
        };
        let response = format!(
            "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;
    }
}

struct Daemon {
    _dir: tempfile::TempDir,
    mock: Arc<Mock>,
    mock_url: String,
    server: tokio::task::JoinHandle<()>,
    process: Option<Child>,
    url: String,
    client: reqwest::Client,
}

impl Daemon {
    async fn new(guard_mode: usize, running_target: bool, old_digest_missing: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir(root.join("state")).unwrap();
        fs::write(root.join(".env"), "MYRIAD_DB_MODE=external\nMYRIAD_TAG=v0.5.3\nPROXY_TAG=preview\nUPDATER_TAG=v0.5.3\nKEEP=unchanged\n").unwrap();
        fs::write(
            root.join("compose.yaml"),
            "name: myriad\nservices:\n  backend:\n    image: example:${MYRIAD_TAG}\n",
        )
        .unwrap();
        fs::write(root.join("guard.env"), format!("DOCKER_GUARD_IMAGE=docker.io/somekawahitomi/myriad-updater@{OLD}\nGUARD_COMPOSE_PROJECT_NAME=myriad\nGUARD_MYRIAD_DOCKER_NETWORK=myriad-net\nGUARD_MYRIAD_ADMIN_NETWORK=myriad-admin-net\nGUARD_MYRIAD_DOCKER_GUARD_NETWORK=myriad-guard-net\nMYRIAD_GUARD_ENV_FILE=guard-policy/docker-guard.env\n")).unwrap();
        let docker = root.join("docker");
        fs::write(
            &docker,
            r##"#!/bin/sh
set -eu
case "$1" in
  info) echo '{"ServerVersion":"28.0.0","SecurityOptions":[]}' ;;
  compose)
    case " $* " in
      *' version '*) echo '2.39.0' ;;
      *' up '*)
        printf '%s\n' "$PROXY_TAG" >> "$TEST_ROOT/compose-calls"
        case "$PROXY_TAG" in *bbbb*) echo 'injected replacement failure' >&2; exit 1 ;; esac ;;
      *) exit 8 ;;
    esac ;;
  *) exit 9 ;;
esac
"##,
        )
        .unwrap();
        fs::set_permissions(docker, fs::Permissions::from_mode(0o755)).unwrap();
        let mock = Arc::new(Mock {
            root: root.into(),
            guard_mode,
            guard_calls: AtomicUsize::new(0),
            pulls: AtomicUsize::new(0),
            registry_calls: AtomicUsize::new(0),
            release_registry: Semaphore::new(0),
            running_target,
            old_digest_missing,
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mock_url = format!("http://{}", listener.local_addr().unwrap());
        let handler = mock.clone();
        let server = tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let handler = handler.clone();
                tokio::spawn(handler.serve(socket));
            }
        });
        let listen = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listen.local_addr().unwrap());
        drop(listen);
        Self {
            _dir: dir,
            mock,
            mock_url,
            server,
            process: None,
            url,
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap(),
        }
    }

    async fn start(&mut self) {
        let root = &self.mock.root;
        let mut command = Command::new(env!("CARGO_BIN_EXE_myriad-updater"));
        command
            .env(
                "PATH",
                format!("{}:{}", root.display(), std::env::var("PATH").unwrap()),
            )
            .env("TEST_ROOT", root)
            .env("UPDATE_TOKEN", TOKEN)
            .env("DOCKER_GUARD_SELF_UPDATE_TOKEN", TOKEN)
            .env("UPDATER_STATE_DIR", root.join("state"))
            .env("UPDATER_COMPOSE_DIR", root)
            .env("UPDATER_ENV_FILE", root.join(".env"))
            .env("UPDATER_PGDATA", root.join("pgdata"))
            .env("UPDATER_LISTEN", self.url.trim_start_matches("http://"))
            .env("UPDATER_GUARD_ENV_FILE", root.join("guard.env"))
            .env("MYRIAD_DB_MODE", "external")
            .env("CHECK_INTERVAL_SECS", "0")
            .env_remove("GITHUB_TOKEN")
            .env_remove("DOCKER_TLS_VERIFY")
            .env_remove("DOCKER_CERT_PATH")
            .env("DOCKER_HOST", self.mock_url.replace("http://", "tcp://"))
            .env(
                "UPDATER_DEBUG_GUARDED_DOCKER_HOST",
                self.mock_url.replace("http://", "tcp://"),
            )
            .env(
                "DOCKER_GUARD_SELF_UPDATE_URL",
                format!("{}/_myriad/self-update", self.mock_url),
            )
            .env("HTTP_PROXY", &self.mock_url)
            .env("HTTPS_PROXY", &self.mock_url)
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env_remove("ALL_PROXY")
            .env_remove("http_proxy")
            .env_remove("https_proxy")
            .env_remove("all_proxy")
            .env_remove("no_proxy")
            .stdout(Stdio::from(
                fs::File::create(root.join("daemon.log")).unwrap(),
            ))
            .stderr(Stdio::from(
                fs::File::create(root.join("daemon.err")).unwrap(),
            ));
        self.process = Some(command.spawn().unwrap());
        for _ in 0..200 {
            if let Some(exit) = self.process.as_mut().unwrap().try_wait().unwrap() {
                panic!(
                    "daemon exited {exit}: {} {}",
                    fs::read_to_string(root.join("daemon.log")).unwrap(),
                    fs::read_to_string(root.join("daemon.err")).unwrap()
                );
            }
            if self
                .client
                .get(format!("{}/healthz", self.url))
                .send()
                .await
                .is_ok()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("daemon did not start");
    }

    async fn post(&self, route: &str, body: Value) -> reqwest::Response {
        self.client
            .post(format!("{}{route}", self.url))
            .header("X-Update-Token", TOKEN)
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    async fn status(&self) -> Value {
        self.client
            .get(format!("{}/status", self.url))
            .header("X-Update-Token", TOKEN)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    async fn outcome(&self, field: &str, expected: &str) -> Value {
        for _ in 0..200 {
            let status = self.status().await;
            if status[field]["status"] == expected {
                return status[field].clone();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("missing {field}={expected}: {}", self.status().await);
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if let Some(child) = &mut self.process {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.server.abort();
    }
}

async fn wait_calls(calls: &AtomicUsize, count: usize) {
    for _ in 0..120 {
        if calls.load(Ordering::SeqCst) >= count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "expected {count} calls, got {}",
        calls.load(Ordering::SeqCst)
    );
}

fn queued_self() -> Value {
    json!({"status":"pending","target_tag":"preview","previous_tag":"v0.5.3","at":"queued","queued":true})
}

#[tokio::test]
async fn admission_is_immediate_discovery_failure_is_retryable_and_status_stays_available() {
    let mut daemon = Daemon::new(0, false, false).await;
    daemon.start().await;
    let response = daemon.post("/admin/self-update", json!({})).await;
    assert!(response.status().is_success());
    assert_eq!(response.json::<Value>().await.unwrap()["scheduled"], true);
    wait_calls(&daemon.mock.registry_calls, 1).await;
    assert_eq!(
        daemon.status().await["self_update_last"]["status"],
        "pending"
    );
    assert_eq!(
        daemon.post("/admin/self-update", json!({})).await.status(),
        409
    );
    assert_eq!(
        daemon
            .post("/self-update/last/dismiss", json!({}))
            .await
            .status(),
        409
    );
    daemon.mock.release_registry.add_permits(1);
    daemon.outcome("self_update_last", "failed").await;
    assert!(
        daemon
            .post("/admin/self-update", json!({}))
            .await
            .status()
            .is_success()
    );
    wait_calls(&daemon.mock.registry_calls, 2).await;
    daemon.mock.release_registry.add_permits(1);
    daemon.outcome("self_update_last", "failed").await;
    fs::write(
        daemon.mock.root.join("state/self-update-last.json"),
        "broken old history",
    )
    .unwrap();
    assert!(daemon.status().await["self_update_last"].is_null());
}

#[tokio::test]
async fn restart_resumes_request_and_v053_lost_response_does_not_publish_false_failure() {
    let mut daemon = Daemon::new(1, false, false).await;
    daemon.mock.save("self-update-last.json", &queued_self());
    daemon.start().await;
    wait_calls(&daemon.mock.guard_calls, 2).await;
    assert_eq!(
        daemon.status().await["self_update_last"]["status"],
        "pending"
    );
    assert_eq!(
        daemon.post("/admin/self-update", json!({})).await.status(),
        409
    );
    // The old Guard eventually writes its legacy record, with no new protocol fields.
    daemon.mock.save("self-update-last.json", &json!({"status":"pending","target_tag":"preview","previous_tag":"v0.5.3","at":"legacy-accepted"}));
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(
        daemon.status().await["self_update_last"]["status"],
        "pending"
    );
    daemon.mock.save("self-update-last.json", &json!({"status":"failed","target_tag":"preview","previous_tag":"v0.5.3","at":"legacy-finished","error":"restored previous stack"}));
    daemon.outcome("self_update_last", "failed").await;
    assert!(
        daemon
            .post("/self-update/last/dismiss", json!({}))
            .await
            .status()
            .is_success()
    );
}

#[tokio::test]
async fn persisted_guard_acceptance_survives_a_lost_http_response() {
    let mut daemon = Daemon::new(2, false, false).await;
    daemon.mock.save("self-update-last.json", &queued_self());
    daemon.start().await;
    wait_calls(&daemon.mock.guard_calls, 1).await;
    tokio::time::sleep(Duration::from_millis(2200)).await;
    assert_eq!(
        daemon.status().await["self_update_last"]["at"],
        "guard-accepted"
    );
    assert_eq!(daemon.mock.guard_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        daemon
            .post("/admin/proxy-update", json!({"target_version":"preview"}))
            .await
            .status(),
        409
    );
}

#[tokio::test]
async fn mutable_proxy_update_failure_automatically_restores_the_previous_image() {
    let mut daemon = Daemon::new(0, false, false).await;
    daemon.start().await;
    assert!(
        daemon
            .post("/admin/proxy-update", json!({"target_version":"preview"}))
            .await
            .status()
            .is_success()
    );
    let result = daemon.outcome("proxy_update_last", "failed").await;
    assert_eq!(result["rolled_back"], true, "{result}");
    assert_eq!(
        fs::read_to_string(daemon.mock.root.join("compose-calls")).unwrap(),
        format!("preview@{NEW}\npreview@{OLD}\n")
    );
    let env = fs::read_to_string(daemon.mock.root.join(".env")).unwrap();
    assert!(env.contains("PROXY_TAG=preview"));
    assert!(env.contains("KEEP=unchanged"));
    assert!(
        daemon
            .post("/proxy-update/last/dismiss", json!({}))
            .await
            .status()
            .is_success()
    );
}

fn queued_proxy() -> Value {
    json!({"status":"pending","target_tag":"preview","target_image":format!("{REPO}@{NEW}"),"previous_tag":"preview","previous_image_id":OLD,"previous_image":OLD,"at":"before-restart"})
}

#[tokio::test]
async fn proxy_restart_keeps_recovery_reference_after_mutable_tag_moves() {
    let mut daemon = Daemon::new(0, false, true).await;
    daemon.mock.save("proxy-update-last.json", &queued_proxy());
    daemon.start().await;
    let result = daemon.outcome("proxy_update_last", "failed").await;
    assert_eq!(result["rolled_back"], true, "{result}");
    assert_eq!(daemon.mock.pulls.load(Ordering::SeqCst), 0);
    assert_eq!(
        fs::read_to_string(daemon.mock.root.join("compose-calls")).unwrap(),
        format!("preview@{NEW}\npreview@{OLD}\n")
    );
}

#[tokio::test]
async fn proxy_restart_finishes_an_already_healthy_target_without_recreating_it() {
    let mut daemon = Daemon::new(0, true, true).await;
    daemon.mock.save("proxy-update-last.json", &queued_proxy());
    daemon.start().await;
    daemon.outcome("proxy_update_last", "succeeded").await;
    assert_eq!(daemon.mock.pulls.load(Ordering::SeqCst), 0);
    assert!(!daemon.mock.root.join("compose-calls").exists());
}
