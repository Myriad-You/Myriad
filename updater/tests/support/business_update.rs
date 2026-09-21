//! Business upgrades through the real daemon API. Only Docker and application health are mocked.
use super::*;
use base64::Engine;
use std::io::Write;

const TARGET: &str = "dev-bbbbbbb";

impl Mock {
    pub(super) fn business_response(&self, request: &str) -> Option<String> {
        if !self.root.join("business").exists() {
            return None;
        }
        let version = fs::read_to_string(self.root.join("app-version")).unwrap();
        let version = version.trim();
        let id = if version == TARGET { NEW } else { OLD };
        let running = self.root.join("app-running").exists();
        if request.starts_with("POST ") && (request.contains("/stop") || request.contains("/kill"))
        {
            let file = if request.contains("postgres") {
                "pg-running"
            } else {
                "app-running"
            };
            let _ = fs::remove_file(self.root.join(file));
            return Some(String::new());
        }
        let value = if request.contains("/images/") && !request.contains("/images/create") {
            if request.starts_with("POST ") {
                return Some(String::new());
            }
            let old = request.contains("v0.5.3")
                || request.contains("myriad-rollback")
                || request.contains("aaaa");
            let image = if old { OLD } else { NEW };
            let mut labels = json!({"org.opencontainers.image.revision":"bbbbbbb"});
            if self.root.join("split-worker").exists() {
                labels["io.myriad.runtime.federation-worker"] = "1".into();
                labels["io.myriad.proxy.federation-routing"] = "1".into();
            }
            if !self.root.join("legacy-template").exists() {
                for variant in ["bundled", "external"] {
                    if let Ok(bytes) = fs::read(
                        self.root
                            .join(format!("state/cache/compose-bbbbbbb-{variant}.yaml")),
                    ) {
                        labels[format!("io.myriad.compose.{variant}")] =
                            base64::engine::general_purpose::STANDARD
                                .encode(bytes)
                                .into();
                    }
                }
            }
            json!({"Id":image,"RepoDigests":[format!("docker.io/example/backend@{image}")],"Config":{"Labels":labels}})
        } else if request.contains("/containers/json") {
            if self.root.join("split-worker").exists() {
                json!([{"Id":"worker", "Names":["/myriad-federation-worker"]}])
            } else {
                json!([])
            }
        } else if request.contains("/containers/myriad-proxy/") {
            json!({"Image":OLD,"Config":{"Env":["PROXY_FEDERATION_UPSTREAM=http://federation-worker:1103"],"Labels":{"com.docker.compose.project":"myriad"}},"State":{"Running":true,"Health":{"Status":"healthy"}}})
        } else if request.contains("/containers/postgres/")
            || request.contains("/containers/myriad-postgres/")
        {
            json!({"Image":OLD,"Config":{"Labels":{"com.docker.compose.project":"myriad"}},"State":{"Running":self.root.join("pg-running").exists(),"Health":{"Status":"healthy"}}})
        } else if request.contains("/containers/myriad-")
            || request.contains("/containers/backend/")
        {
            let env = if self.root.join("split-worker").exists() {
                vec!["MYRIAD_PROCESS_ROLE=web"]
            } else {
                vec![]
            };
            json!({"Image":id,"Config":{"Image":format!("docker.io/example/backend:{version}"),"Env":env,"Labels":{"com.docker.compose.project":"myriad"}},"State":{"Running":running,"Health":{"Status":"healthy"}}})
        } else if request.contains("/networks/") {
            json!({"Name":"myriad-net"})
        } else if request.contains("backend:1103/health") {
            json!({"version":version,"commit_sha":"1234567890123456789012345678901234567890","db_connected":true,"migrations_applied":true,"routes_full":true,"storage_writable":true})
        } else {
            return None;
        };
        Some(value.to_string())
    }
}

impl Daemon {
    async fn business(cut: &str) -> Self {
        let daemon = Self::new(0, false, false).await;
        let root = &daemon.mock.root;
        fs::write(root.join("business"), "").unwrap();
        fs::write(root.join("cut-point"), cut).unwrap();
        fs::write(root.join("app-version"), "v0.5.3").unwrap();
        fs::write(root.join("app-running"), "").unwrap();
        let mut env = fs::OpenOptions::new()
            .append(true)
            .open(root.join(".env"))
            .unwrap();
        writeln!(env, "JWT_SECRET={TOKEN}\nBACKEND_IMAGE=docker.io/example/backend\nFRONTEND_IMAGE=docker.io/example/frontend\nDATABASE_URL=postgres://fixture/db").unwrap();
        let model = json!({"name":"myriad", "services": {
            "backend":{"container_name":"myriad-backend","image":"docker.io/example/backend:${MYRIAD_TAG}","networks":{"default":{}}},
            "frontend":{"container_name":"myriad-frontend","image":"docker.io/example/frontend:${MYRIAD_TAG}","networks":{"default":{}}},
            "backend-volume-init":{"image":"docker.io/example/backend:${MYRIAD_TAG}"}
        }, "networks":{"default":{"name":"myriad-net"}}});
        fs::create_dir_all(root.join("state/cache")).unwrap();
        fs::create_dir_all(root.join("state/compose")).unwrap();
        fs::write(root.join("state/compose/compose.yaml"), model.to_string()).unwrap();
        fs::remove_file(root.join("compose.yaml")).unwrap();
        std::os::unix::fs::symlink("state/compose/compose.yaml", root.join("compose.yaml"))
            .unwrap();
        fs::write(
            root.join("state/cache/compose-bbbbbbb-external.yaml"),
            model.to_string(),
        )
        .unwrap();
        let real_docker = Command::new("sh")
            .args(["-c", "command -v docker"])
            .output()
            .unwrap();
        assert!(real_docker.status.success());
        fs::write(root.join("real-docker"), real_docker.stdout).unwrap();
        // A barrier is inside the mocked Docker operation, not inside production code.
        // It records the real update reaching that operation, then waits to be killed.
        fs::write(
            root.join("docker"),
            r##"#!/bin/sh
set -eu
barrier() {
  if [ "$(cat "$TEST_ROOT/cut-point")" = "$1" ]; then
    echo "$1" > "$TEST_ROOT/reached"
    while :; do sleep 1; done
  fi
}
case "$1" in
  info) echo '{"ServerVersion":"28.0.0","SecurityOptions":[]}' ;;
  ps) echo 'volume-init' ;;
  wait) echo 0 ;;
  logs) echo 'storage ready' ;;
  compose)
    tag=$(sed -n 's/^MYRIAD_TAG=//p' "$TEST_ROOT/.env")
    case " $* " in
      *' version '*) echo '2.39.0' ;;
      *' config '*) exec "$(cat "$TEST_ROOT/real-docker")" "$@" ;;
      *' stop '*)
        case " $* " in *' postgres '*) rm -f "$TEST_ROOT/pg-running"; exit 0 ;; esac
        rm -f "$TEST_ROOT/app-running"
        barrier stopped ;;
      *' up '*|*' start '*)
        case " $* " in
          *' postgres '*) touch "$TEST_ROOT/pg-running" ;;
          *' backend-volume-init '*)
            barrier swapped
            if [ -f "$TEST_ROOT/fail-new-init" ]; then
              [ ! -d "$TEST_ROOT/pgdata" ] || echo migrated > "$TEST_ROOT/pgdata/record"
              echo 'injected initializer failure' >&2
              exit 1
            fi ;;
          *)
            printf '%s\n' "$*" >> "$TEST_ROOT/app-commands"
            echo "$tag" > "$TEST_ROOT/app-version"
            touch "$TEST_ROOT/app-running"
            echo "$tag" >> "$TEST_ROOT/app-starts"
            barrier started ;;
        esac ;;
      *) echo "unexpected compose call: $*" >&2; exit 8 ;;
    esac ;;
  *) echo "unexpected docker call: $*" >&2; exit 9 ;;
esac
"##,
        )
        .unwrap();
        daemon
    }

    async fn business_update(&self) -> String {
        let response = self
            .post("/update", json!({"target_commit":TARGET,"mode":"commit"}))
            .await;
        let code = response.status();
        let body: Value = response.json().await.unwrap();
        assert!(code.is_success(), "{code}: {body}");
        body["job_id"].as_str().unwrap().into()
    }

    async fn job(&self, id: &str) -> Value {
        self.client
            .get(format!("{}/jobs/{id}", self.url))
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

    async fn wait_at_cut(&self, job: &str) {
        for _ in 0..400 {
            if self.mock.root.join("reached").exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("did not reach cut point: {}", self.job(job).await);
    }
}

#[tokio::test]
async fn business_upgrade_completes_from_http_request_and_stays_completed_after_restart() {
    let mut daemon = Daemon::business("").await;
    daemon.start().await;
    let id = daemon.business_update().await;
    for _ in 0..400 {
        let status = daemon.status().await;
        if daemon.job(&id).await["status"] == "succeeded"
            && status["maintenance_active"] == false
            && status["job_in_flight"].is_null()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        daemon.job(&id).await["status"],
        "succeeded",
        "{}",
        daemon.job(&id).await
    );
    assert_eq!(daemon.status().await["maintenance_active"], false);
    assert!(daemon.status().await["job_in_flight"].is_null());
    assert_eq!(
        fs::read_to_string(daemon.mock.root.join("app-version"))
            .unwrap()
            .trim(),
        TARGET
    );
    let starts = fs::read_to_string(daemon.mock.root.join("app-starts")).unwrap();
    daemon.power_cut();
    daemon.start().await;
    assert_eq!(daemon.status().await["current_version"], TARGET);
    assert_eq!(daemon.status().await["maintenance_active"], false);
    assert_eq!(
        fs::read_to_string(daemon.mock.root.join("app-starts")).unwrap(),
        starts
    );
}

#[tokio::test]
async fn business_power_cut_after_stop_restores_old_services_without_user_input() {
    let mut daemon = Daemon::business("stopped").await;
    daemon.start().await;
    let id = daemon.business_update().await;
    daemon.wait_at_cut(&id).await;
    assert_eq!(daemon.status().await["maintenance_active"], true);
    assert!(!daemon.mock.root.join("app-running").exists());
    daemon.power_cut();
    fs::remove_file(daemon.mock.root.join("reached")).unwrap();
    fs::write(daemon.mock.root.join("cut-point"), "started").unwrap();
    daemon.start().await;
    daemon.wait_at_cut(&id).await;
    assert_eq!(daemon.status().await["maintenance_active"], true);
    daemon.power_cut();
    fs::write(daemon.mock.root.join("cut-point"), "").unwrap();
    daemon.start().await;
    daemon.completed_job(&id, "failed").await;
    assert!(daemon.mock.root.join("app-running").exists());
    assert_eq!(
        fs::read_to_string(daemon.mock.root.join("app-version"))
            .unwrap()
            .trim(),
        "v0.5.3"
    );
    assert_eq!(daemon.status().await["maintenance_active"], false);
    assert!(daemon.status().await["job_in_flight"].is_null());
}

#[tokio::test]
async fn business_power_cut_after_swap_completes_without_user_input() {
    for cut in ["swapped", "started"] {
        let mut daemon = Daemon::business(cut).await;
        daemon.start().await;
        let id = daemon.business_update().await;
        daemon.wait_at_cut(&id).await;
        assert!(
            fs::read_to_string(daemon.mock.root.join(".env"))
                .unwrap()
                .contains(&format!("MYRIAD_TAG={TARGET}\n"))
        );
        if cut == "started" {
            assert!(daemon.mock.root.join("app-running").exists());
            assert_eq!(
                fs::read_to_string(daemon.mock.root.join("app-version"))
                    .unwrap()
                    .trim(),
                TARGET
            );
        }
        daemon.power_cut();
        fs::write(daemon.mock.root.join("cut-point"), "").unwrap();
        daemon.start().await;
        daemon.completed_job(&id, "succeeded").await;
        assert_eq!(daemon.job(&id).await["status"], "succeeded", "cut={cut}");
        assert_eq!(
            daemon.status().await["maintenance_active"],
            false,
            "cut={cut}"
        );
        assert_eq!(daemon.status().await["current_version"], TARGET);
        assert!(daemon.status().await["job_in_flight"].is_null());
    }
}

impl Daemon {
    fn bundled_database(&self) {
        let root = &self.mock.root;
        let env = fs::read_to_string(root.join(".env"))
            .unwrap()
            .replace("MYRIAD_DB_MODE=external", "MYRIAD_DB_MODE=bundled");
        fs::write(
            root.join(".env"),
            format!("{env}POSTGRES_PASSWORD=fixture\n"),
        )
        .unwrap();
        fs::create_dir(root.join("pgdata")).unwrap();
        fs::write(root.join("pgdata/record"), "original").unwrap();
        fs::write(root.join("pg-running"), "").unwrap();
        let mut model: Value =
            serde_json::from_slice(&fs::read(root.join("compose.yaml")).unwrap()).unwrap();
        model["services"]["postgres"] = json!({"image":"postgres:18", "container_name":"myriad-postgres", "volumes":[{"type":"bind","source":"./pgdata","target":"/var/lib/postgresql"}]});
        fs::write(root.join("compose.yaml"), model.to_string()).unwrap();
        fs::write(
            root.join("state/cache/compose-bbbbbbb-bundled.yaml"),
            model.to_string(),
        )
        .unwrap();
        let copy = root.join("cp");
        fs::write(
            &copy,
            r##"#!/bin/sh
set -eu
/bin/cp "$@"
for destination; do :; done
cut=$(cat "$TEST_ROOT/cut-point")
case "$cut:$destination" in
  snapshot-copied:*.tmp|restore-copied:*/pgdata)
    touch "$TEST_ROOT/reached"
    while :; do sleep 1; done ;;
esac
"##,
        )
        .unwrap();
        fs::set_permissions(copy, fs::Permissions::from_mode(0o755)).unwrap();
    }

    async fn completed_job(&self, id: &str, expected: &str) {
        for _ in 0..400 {
            let status = self.status().await;
            if self.job(id).await["status"] == expected
                && status["maintenance_active"] == false
                && status["job_in_flight"].is_null()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!(
            "expected {expected} without maintenance: {}",
            self.job(id).await
        );
    }
}

#[tokio::test]
async fn bundled_upgrade_and_snapshot_power_cut_preserve_database() {
    for cut in ["", "snapshot-copied", "swapped", "started"] {
        let mut daemon = Daemon::business(cut).await;
        daemon.bundled_database();
        daemon.start().await;
        let id = daemon.business_update().await;
        if cut.is_empty() {
            daemon.completed_job(&id, "succeeded").await;
        } else {
            daemon.wait_at_cut(&id).await;
            daemon.power_cut();
            fs::write(daemon.mock.root.join("cut-point"), "").unwrap();
            daemon.start().await;
        }
        daemon
            .completed_job(
                &id,
                if cut == "snapshot-copied" {
                    "failed"
                } else {
                    "succeeded"
                },
            )
            .await;
        assert_eq!(
            fs::read_to_string(daemon.mock.root.join("pgdata/record")).unwrap(),
            "original",
            "cut={cut}"
        );
        assert_eq!(daemon.status().await["maintenance_active"], false);
        assert!(daemon.status().await["job_in_flight"].is_null());
        assert_eq!(
            daemon.status().await["current_version"],
            if cut == "snapshot-copied" {
                "v0.5.3"
            } else {
                TARGET
            }
        );
    }
}

#[tokio::test]
async fn failed_upgrade_and_power_cut_during_restore_recover_old_compose_and_database() {
    for cut in ["", "restore-copied"] {
        let mut daemon = Daemon::business(cut).await;
        daemon.bundled_database();
        let root = daemon.mock.root.clone();
        let before = fs::read(root.join("compose.yaml")).unwrap();
        // The new version changes its definition, then its initializer fails.
        let mut target: Value = serde_json::from_slice(&before).unwrap();
        target["services"]["backend"]["environment"] = json!({"NEW_SETTING":"1"});
        fs::write(
            root.join("state/cache/compose-bbbbbbb-bundled.yaml"),
            target.to_string(),
        )
        .unwrap();
        fs::write(root.join("fail-new-init"), "").unwrap();
        daemon.start().await;
        let id = daemon.business_update().await;
        if !cut.is_empty() {
            daemon.wait_at_cut(&id).await;
            daemon.power_cut();
            fs::write(root.join("cut-point"), "").unwrap();
            daemon.start().await;
        }
        daemon.completed_job(&id, "failed").await;
        assert_eq!(fs::read(root.join("compose.yaml")).unwrap(), before);
        assert_eq!(
            fs::read_to_string(root.join("pgdata/record")).unwrap(),
            "original"
        );
        assert_eq!(daemon.status().await["current_version"], "v0.5.3");
        assert!(root.join("app-running").exists());
        assert!(root.join("pg-running").exists());
    }
}

#[tokio::test]
async fn upgrade_adds_media_mount_and_keeps_deployment_settings() {
    let mut daemon = Daemon::business("").await;
    let root = &daemon.mock.root;
    fs::write(root.join("split-worker"), "").unwrap();
    let mut current: Value =
        serde_json::from_slice(&fs::read(root.join("compose.yaml")).unwrap()).unwrap();
    current["services"]["federation-worker"] = json!({"image":"docker.io/example/backend:${MYRIAD_TAG}","container_name":"myriad-federation-worker", "volumes":[{"type":"volume","source":"backend_data","target":"/app/data","read_only":true}]});
    for subpath in ["federation", "federation_media"] {
        current["services"]["federation-worker"]["volumes"].as_array_mut().unwrap().push(json!({"type":"volume","source":"backend_data","target":format!("/app/data/{subpath}"),"volume":{"subpath":subpath,"nocopy":true}}));
    }
    current["services"]["federation-worker"]["volumes"].as_array_mut().unwrap().push(json!({"type":"volume","source":"backend_cache","target":"/tmp/cache/images","volume":{"subpath":"images","nocopy":true}}));
    current["volumes"] = json!({"backend_data":{"name":"existing-data"}, "backend_cache":{}});
    current["services"]["backend"]["extra_hosts"] = json!(["db.example:192.0.2.1"]);
    current["services"]["backend"]["environment"] = json!({"DATABASE_URL":"postgres://custom/database","CUSTOM_SETTING":"keep", "MYRIAD_PROCESS_ROLE":"web"});
    current["services"]["frontend"]["ports"] = json!(["8088:80"]);
    current["services"]["proxy"] = json!({"image":"example/proxy:keep"});
    fs::write(root.join("compose.yaml"), current.to_string()).unwrap();
    let mut target = current.clone();
    // A panel may have resolved old image tags to literals. The target template
    // supplies its own image expression; old text is not an upgrade requirement.
    for name in [
        "backend",
        "frontend",
        "backend-volume-init",
        "federation-worker",
    ] {
        current["services"][name]["image"] = current["services"][name]["image"]
            .as_str()
            .unwrap()
            .replace("${MYRIAD_TAG}", "v0.5.3")
            .into();
    }
    fs::write(root.join("compose.yaml"), current.to_string()).unwrap();
    target["services"]["federation-worker"]["volumes"].as_array_mut().unwrap().push(json!({"type":"volume","source":"backend_data","target":"/app/data/media","volume":{"subpath":"media","nocopy":true}}));
    target["services"]["frontend"]["ports"] = json!(["80:80"]);
    target["services"]["backend"]["environment"] =
        json!({"DATABASE_URL":"${DATABASE_URL}","NEW_SETTING":"1", "MYRIAD_PROCESS_ROLE":"web"});
    target["services"]["proxy"]["image"] = json!("example/proxy:do-not-upgrade");
    target["volumes"]["backend_data"]["name"] = json!("would-lose-data");
    fs::write(
        root.join("state/cache/compose-bbbbbbb-external.yaml"),
        target.to_string(),
    )
    .unwrap();
    daemon.start().await;
    let id = daemon.business_update().await;
    daemon.completed_job(&id, "succeeded").await;
    assert!(
        fs::read_to_string(daemon.mock.root.join("app-commands"))
            .unwrap()
            .contains("federation-worker")
    );
    let after: Value =
        serde_json::from_slice(&fs::read(daemon.mock.root.join("compose.yaml")).unwrap()).unwrap();
    assert!(
        after["services"]["federation-worker"]["volumes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|mount| mount["target"] == "/app/data/media")
    );
    assert_eq!(
        after["services"]["backend"]["environment"]["DATABASE_URL"],
        "postgres://custom/database"
    );
    assert_eq!(
        after["services"]["backend"]["environment"]["CUSTOM_SETTING"],
        "keep"
    );
    assert_eq!(
        after["services"]["backend"]["environment"]["NEW_SETTING"],
        "1"
    );
    assert_eq!(
        after["services"]["frontend"]["ports"][0]["published"],
        "8088"
    );
    assert_eq!(after["services"]["frontend"]["ports"][0]["target"], 80);
    assert!(
        after["services"]["backend"]["extra_hosts"]
            .to_string()
            .contains("192.0.2.1")
    );
    assert_eq!(after["services"]["proxy"], current["services"]["proxy"]);
    assert_eq!(
        after["volumes"]["backend_data"],
        current["volumes"]["backend_data"]
    );
    assert!(
        !after["services"]
            .as_object()
            .unwrap()
            .contains_key("postgres")
    );
}

#[tokio::test]
async fn legacy_target_without_embedded_compose_uses_source_cache() {
    let mut daemon = Daemon::business("").await;
    fs::write(daemon.mock.root.join("legacy-template"), "").unwrap();
    daemon.start().await;
    let id = daemon.business_update().await;
    daemon.completed_job(&id, "succeeded").await;
}

#[tokio::test]
async fn status_remains_available_when_recovery_is_interrupted_again() {
    let mut daemon = Daemon::business("swapped").await;
    daemon.start().await;
    let id = daemon.business_update().await;
    daemon.wait_at_cut(&id).await;
    daemon.power_cut();
    fs::remove_file(daemon.mock.root.join("reached")).unwrap();
    fs::write(daemon.mock.root.join("cut-point"), "started").unwrap();
    daemon.start().await;
    daemon.wait_at_cut(&id).await;
    assert_eq!(daemon.status().await["maintenance_active"], true);
    assert_eq!(daemon.status().await["current_version"], "v0.5.3");
    let starts = fs::read_to_string(daemon.mock.root.join("app-starts")).unwrap();
    daemon.power_cut();
    fs::write(daemon.mock.root.join("cut-point"), "").unwrap();
    daemon.start().await;
    daemon.completed_job(&id, "succeeded").await;
    assert_eq!(
        fs::read_to_string(daemon.mock.root.join("app-starts")).unwrap(),
        starts
    );
}
