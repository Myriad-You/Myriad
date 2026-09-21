//! Business upgrades through the real daemon API. Only Docker and application health are mocked.
use super::*;
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
        let value = if request.contains("/images/") && !request.contains("/images/create") {
            if request.starts_with("POST ") {
                return Some(String::new());
            }
            let old = request.contains("v0.5.3")
                || request.contains("myriad-rollback")
                || request.contains("aaaa");
            let image = if old { OLD } else { NEW };
            json!({"Id":image,"RepoDigests":[format!("docker.io/example/backend@{image}")],"Config":{"Labels":{}}})
        } else if request.contains("/containers/json") {
            json!([])
        } else if request.contains("/containers/myriad-") {
            json!({"Image":id,"Config":{"Image":format!("docker.io/example/backend:{version}"),"Labels":{"com.docker.compose.project":"myriad"}},"State":{"Running":running,"Health":{"Status":"healthy"}}})
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
            "backend":{"container_name":"myriad-backend","image":"docker.io/example/backend:TAG","networks":{"default":{}}},
            "frontend":{"container_name":"myriad-frontend","image":"docker.io/example/frontend:TAG","networks":{"default":{}}},
            "backend-volume-init":{"image":"docker.io/example/backend:TAG"}
        }, "networks":{"default":{"name":"myriad-net"}}});
        fs::write(root.join("compose-model.json"), model.to_string()).unwrap();
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
      *' config '*) sed "s/TAG/$tag/g" "$TEST_ROOT/compose-model.json" ;;
      *' stop '*)
        rm -f "$TEST_ROOT/app-running"
        barrier stopped ;;
      *' up '*|*' start '*)
        case " $* " in
          *' backend-volume-init '*) barrier swapped ;;
          *)
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
        for _ in 0..200 {
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
    for _ in 0..200 {
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
    fs::write(daemon.mock.root.join("cut-point"), "").unwrap();
    daemon.start().await;
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

// Characterization of an outstanding acceptance failure, not the desired recovery contract.
// Replace this assertion when post-swap recovery no longer needs operator intervention.
#[tokio::test]
async fn business_power_cut_after_swap_exposes_the_manual_recovery_gap() {
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
        assert_eq!(daemon.job(&id).await["status"], "needs_manual", "cut={cut}");
        assert_eq!(
            daemon.status().await["maintenance_active"],
            true,
            "cut={cut}"
        );
        eprintln!("ACCEPTANCE GAP: cut={cut}, restart requires manual recovery");
    }
}
