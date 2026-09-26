//! Real child processes exercise crash recovery without touching a running site.
use super::*;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use std::{path::Path, time::Duration};

const FIXTURE: &str = "services::agent::work_loop::recovery_tests::work_process_fixture";

async fn signal(path: &Path) {
    tokio::time::timeout(Duration::from_secs(20), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("fixture process must reach its checkpoint");
}

fn spawn(dir: &Path, task_id: &str, role: &str) -> tokio::process::Child {
    let log = std::fs::File::create(dir.join(format!("{role}.log"))).unwrap();
    tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", FIXTURE, "--nocapture"])
        .env("MYRIAD_WORK_PROCESS_ROLE", role)
        .env("MYRIAD_WORK_PROCESS_TASK", task_id)
        .env("MYRIAD_WORK_PROCESS_DIR", dir)
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}

async fn successful(child: &mut tokio::process::Child, dir: &Path, role: &str) {
    let status = tokio::time::timeout(Duration::from_secs(30), child.wait())
        .await
        .expect("fixture must exit")
        .unwrap();
    assert!(
        status.success(),
        "{}",
        std::fs::read_to_string(dir.join(format!("{role}.log"))).unwrap()
    );
}

struct FixtureDir(std::path::PathBuf);
impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
#[ignore = "requires MYRIAD_WORK_TEST_DATABASE_URL pointing to myriad_work_loop_test"]
async fn postgres_work_process_recovery_and_competing_resumes() {
    let db = tests::test_database().await;
    db.execute_raw(Statement::from_string(DatabaseBackend::Postgres,
        "CREATE TABLE IF NOT EXISTS work_test_effects (task_id TEXT NOT NULL, operation TEXT NOT NULL)"))
        .await.unwrap();
    db.execute_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "INSERT INTO users (id, is_admin, is_owner) VALUES (8282, true, false) \
         ON CONFLICT(id) DO UPDATE SET is_admin=true",
    ))
    .await
    .unwrap();
    let task_id = uuid::Uuid::new_v4().to_string();
    let dir = FixtureDir(std::env::temp_dir().join(format!("myriad-work-{task_id}")));
    std::fs::create_dir(&dir.0).unwrap();

    // The scripted provider deliberately asks to repeat the ambiguous effect.
    // The actual Work dispatcher must refuse it, then execute a real read tool.
    let effect_task = task_id.clone();
    let app = axum::Router::new().route("/v1/chat/completions", axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
        let effect_task = effect_task.clone();
        async move {
            let results: Vec<_> = body["messages"].as_array().unwrap().iter().filter(|m| m["role"] == "tool").collect();
            let (delta, reason) = match results.len() {
                1 => (json!({"tool_calls":[{"index":0,"id":"repeat-effect","function":{"name":tools::tool_name("scheduler.trigger"),"arguments":json!({"taskId":effect_task}).to_string()}}]}), "tool_calls"),
                2 => {
                    assert!(results[1]["content"].as_str().unwrap().contains("already attempted"));
                    (json!({"tool_calls":[{"index":0,"id":"read-time","function":{"name":tools::tool_name("time.info"),"arguments":"{\"timezone\":\"UTC\"}"}}]}), "tool_calls")
                },
                3 => {
                    let read: Value = serde_json::from_str(results[2]["content"].as_str().unwrap()).unwrap();
                    assert!(read.get("error").is_none(), "real read handler must succeed");
                    (json!({"content":"Recovered without repeating the effect"}), "stop")
                },
                _ => panic!("unexpected provider turn"),
            };
            ([("content-type","text/event-stream")], format!("data: {}\n\ndata: [DONE]\n\n", json!({"choices":[{"delta":delta,"finish_reason":reason}]})))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    std::fs::write(
        dir.0.join("model-url"),
        format!("http://{}/v1", listener.local_addr().unwrap()),
    )
    .unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let mut original = spawn(&dir.0, &task_id, "interrupted");
    signal(&dir.0.join("interrupted.ready")).await;
    // A live process must not lose its lease to a recovery scan.
    let mut live_scan = spawn(&dir.0, &task_id, "recover-live");
    successful(&mut live_scan, &dir.0, "recover-live").await;
    assert_eq!(
        store::load(&db, &task_id, 8282).await.unwrap().task.status,
        TaskStatus::Running
    );

    original.kill().await.unwrap();
    original.wait().await.unwrap();
    // Advance lease age in the isolated DB instead of waiting 90 wall seconds.
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_tasks SET updated_at=NOW()-INTERVAL '2 minutes' WHERE id=$1",
        [task_id.clone().into()],
    ))
    .await
    .unwrap();
    let mut recovery = spawn(&dir.0, &task_id, "recover-expired");
    successful(&mut recovery, &dir.0, "recover-expired").await;
    let recovered = store::load(&db, &task_id, 8282).await.unwrap();
    assert_eq!(recovered.task.status, TaskStatus::WaitingForInput);
    assert!(matches!(recovered.wait, Some(Wait::Recovery)));
    assert_eq!(recovered.inflight.as_deref(), Some("effect"));

    let mut stale = spawn(&dir.0, &task_id, "stale");
    successful(&mut stale, &dir.0, "stale").await;
    let mut first = spawn(&dir.0, &task_id, "resume-a");
    let mut second = spawn(&dir.0, &task_id, "resume-b");
    signal(&dir.0.join("resume-a.ready")).await;
    signal(&dir.0.join("resume-b.ready")).await;
    std::fs::write(dir.0.join("resume.go"), []).unwrap();
    successful(&mut first, &dir.0, "resume-a").await;
    successful(&mut second, &dir.0, "resume-b").await;
    let winners = ["resume-a", "resume-b"]
        .into_iter()
        .filter(|role| {
            std::fs::read_to_string(dir.0.join(format!("{role}.result"))).unwrap() == "claimed"
        })
        .count();
    assert_eq!(winners, 1, "only one OS process can claim an answer");
    let completed = store::load(&db, &task_id, 8282).await.unwrap();
    assert_eq!(completed.task.status, TaskStatus::Completed);
    assert!(completed.pending.is_empty());
    assert!(completed.inflight.is_none());
    assert!(completed.attempted_effects.contains(&operation_key(
        "scheduler.trigger",
        &json!({"taskId":task_id})
    )));
    assert!(!completed.task.step_results["repeat-effect"].success);
    assert!(completed.task.step_results["read-time"].success);
    let count = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) AS count FROM work_test_effects WHERE task_id=$1",
            [task_id.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "count")
        .unwrap();
    assert_eq!(
        count, 1,
        "the ambiguous pre-crash effect must not be replayed"
    );
    let response = saved_response(&db, &task_id, 8282).await.unwrap();
    assert_eq!(response.message, "Recovered without repeating the effect");
    assert!(response.frontend_action.is_none());
    assert!(response.data.unwrap().get("frontendActions").is_none());

    // Cross-process cancellation is authoritative even when the checkpoint
    // still contains an outstanding question and no final model answer.
    let mut waiting = tests::checkpoint();
    waiting.wait = Some(Wait::Recovery);
    waiting
        .task
        .set_pending_question(UserQuestion::free_text("Continue?", "", true));
    store::save(&db, &mut waiting).await.unwrap();
    let mut cancel = spawn(&dir.0, &waiting.task.task_id, "cancel");
    successful(&mut cancel, &dir.0, "cancel").await;
    let response = saved_response(&db, &waiting.task.task_id, 0).await.unwrap();
    assert_eq!(response.message, "Cancelled on another process");
    let task = response.task.unwrap();
    assert_eq!(task.status, TaskStatus::Cancelled);
    assert!(task.pending_question.is_none());
    assert!(task.completed_at.is_some());
    assert!(store::save(&db, &mut waiting).await.is_err());
    server.abort();
}

/// Only invoked by the parent above. Each invocation has independent memory,
/// cancellation flags, connection pool, task cache and process lifetime.
#[tokio::test]
#[ignore = "child fixture; run postgres_work_process_recovery_and_competing_resumes"]
async fn work_process_fixture() {
    let role = std::env::var("MYRIAD_WORK_PROCESS_ROLE").expect("parent fixture required");
    let task_id = std::env::var("MYRIAD_WORK_PROCESS_TASK").unwrap();
    let dir = std::path::PathBuf::from(std::env::var_os("MYRIAD_WORK_PROCESS_DIR").unwrap());
    let db = tests::test_database().await;
    match role.as_str() {
        "interrupted" => {
            let mut state = tests::checkpoint();
            state.task.task_id = task_id.clone();
            state.user_id = 8282;
            state.request.user_id = 8282;
            state.selected = vec!["scheduler.trigger".into(), "time.info".into()];
            state.inflight = Some("effect".into());
            state.attempted_effects.insert(operation_key(
                "scheduler.trigger",
                &json!({"taskId":task_id}),
            ));
            state.pending.push_back(PendingCall {
                call: ToolCall {
                    id: "effect".into(),
                    name: tools::tool_name("scheduler.trigger"),
                    arguments: json!({"taskId":task_id}).to_string(),
                },
                capability_id: Some("scheduler.trigger".into()),
                approval: None,
            });
            let call = state.pending.front().unwrap().call.clone();
            state.history.push(ToolMessage::Assistant { turn: crate::services::analyzer::tool_calling::ToolTurn {
                text: String::new(), calls: vec![call.clone()],
                native: json!({"role":"assistant","content":null,"tool_calls":[{"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments}}]}),
                provider: crate::services::analyzer::AiProvider::OpenAI.as_str().into(), model: Some("fixture-model".into()), usage: None,
            }});
            store::save(&db, &mut state).await.unwrap();
            let _lease = store::lease(db.clone(), task_id.clone(), state.lease_id.clone());
            std::fs::write(dir.join("stale.json"), serde_json::to_vec(&state).unwrap()).unwrap();
            // Stand-in for a committed external write whose acknowledgement
            // never reaches the checkpoint. No production application tool runs.
            db.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO work_test_effects VALUES ($1, 'external-write')",
                [task_id.into()],
            ))
            .await
            .unwrap();
            std::fs::write(dir.join("interrupted.ready"), []).unwrap();
            std::future::pending::<()>().await;
        }
        "recover-live" | "recover-expired" => store::recover(&db).await.unwrap(),
        "stale" => {
            let mut old: Checkpoint =
                serde_json::from_slice(&std::fs::read(dir.join("stale.json")).unwrap()).unwrap();
            assert!(store::save(&db, &mut old).await.is_err());
        }
        "resume-a" | "resume-b" => {
            let mut state = store::load(&db, &task_id, 8282).await.unwrap();
            let answer = UserAnswer {
                task_id: task_id.clone(),
                question_id: state
                    .task
                    .pending_question
                    .as_ref()
                    .unwrap()
                    .question_id
                    .clone(),
                answer: "Inspect the saved result and continue".into(),
                skipped: false,
            };
            apply_answer(&mut state, &answer).unwrap();
            std::fs::write(dir.join(format!("{role}.ready")), []).unwrap();
            signal(&dir.join("resume.go")).await;
            if store::save(&db, &mut state).await.is_ok() {
                assert!(state.history.iter().any(|m| matches!(m, ToolMessage::Tool { content, .. } if content.contains("outcome unknown"))));
                let analyzer = crate::services::analyzer::AiAnalyzer::new(
                    crate::services::analyzer::AiProvider::OpenAI,
                    "fixture-only".into(),
                    "fixture-model".into(),
                    Some(std::fs::read_to_string(dir.join("model-url")).unwrap()),
                )
                .await;
                let agent = Agent::new(db.clone()).await;
                let emitter = executor::events::StepEventEmitter::new(None);
                agent
                    .drive_work_loop(&mut state, None, &emitter, &analyzer)
                    .await
                    .unwrap();
                assert_eq!(state.task.status, TaskStatus::Completed);
                std::fs::write(dir.join(format!("{role}.result")), "claimed").unwrap();
            } else {
                std::fs::write(dir.join(format!("{role}.result")), "fenced").unwrap();
            }
        }
        "cancel" => {
            db.execute_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
                "UPDATE agent_tasks SET status='cancelled', error='Cancelled on another process', completed_at=NOW(), updated_at=NOW() WHERE id=$1 AND user_id=0", [task_id.into()]))
                .await.unwrap();
        }
        _ => panic!("unknown fixture role"),
    }
}
