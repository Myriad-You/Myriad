use super::*;
use sea_orm::{ConnectionTrait, DatabaseBackend, Schema, Statement};

pub(super) fn checkpoint() -> Checkpoint {
    let request = UserRequest {
        raw_input: "Compare the time in Tokyo and UTC".into(),
        timestamp: chrono::Utc::now(),
        user_id: 0,
        context: None,
    };
    let mut recipe = Agent::build_recipe_from_steps(vec![], "Time comparison".into(), &request);
    recipe.engine = AgentEngine::WorkLoop;
    let mut task = TaskState::new(&recipe);
    task.status = TaskStatus::Running;
    task.execution_context = Some(ExecutionContext::default());
    Checkpoint {
        budget: Some(budget::Budget::default()),
        recipe_run: None,
        version: 1,
        revision: 0,
        lease_id: uuid::Uuid::new_v4().to_string(),
        user_id: 0,
        request,
        task,
        history: vec![ToolMessage::User {
            content: "Compare the time in Tokyo and UTC".into(),
        }],
        selected: vec![],
        pending: Default::default(),
        inflight: None,
        wait: None,
        denied: Default::default(),
        attempted_effects: Default::default(),
        call_counts: Default::default(),
        rounds: 0,
        calls: 0,
        input_chars: 0,
        active_ms: 0,
        plan: json!([]),
        final_text: String::new(),
    }
}

fn pending(id: &str, name: &str) -> PendingCall {
    PendingCall {
        call: ToolCall {
            id: id.into(),
            name: name.into(),
            arguments: "{}".into(),
        },
        capability_id: None,
        approval: None,
    }
}

fn answer(state: &Checkpoint, text: &str) -> UserAnswer {
    UserAnswer {
        task_id: state.task.task_id.clone(),
        question_id: state
            .task
            .pending_question
            .as_ref()
            .unwrap()
            .question_id
            .clone(),
        answer: text.into(),
        skipped: false,
    }
}

#[tokio::test]
async fn unattended_heartbeat_create_is_rejected_without_effects() {
    let (message, risk) = capability::capability_requires_confirmation_async("heartbeat.create")
        .await
        .expect("heartbeat.create requires confirmation");
    assert_eq!(risk, RiskLevel::Medium);
    assert!(!message.is_empty());

    let before = match crate::services::agent::heartbeat::get_heartbeat() {
        Some(manager) => Some(manager.get_tasks().await.len()),
        None => None,
    };

    let mut state = checkpoint();
    assert_eq!(state.user_id, crate::services::agent::SYSTEM_USER_ID);
    let pending = PendingCall {
        call: ToolCall {
            id: "hb-create".into(),
            name: tools::tool_name("heartbeat.create"),
            arguments:
                r#"{"name":"spread","schedule":"0 * * * *","action":"create another heartbeat"}"#
                    .into(),
        },
        capability_id: Some("heartbeat.create".into()),
        approval: None,
    };
    state.pending.push_back(pending.clone());
    assert!(
        reject_unattended_confirmation(&mut state, &pending, "heartbeat.create", risk),
        "user 0 must not auto-run heartbeat.create"
    );
    assert!(state.inflight.is_none());
    assert!(state.attempted_effects.is_empty());
    assert!(state.wait.is_none());
    assert!(state.pending.is_empty());
    let result = state
        .task
        .step_results
        .get("hb-create")
        .expect("rejection is recorded on the call");
    assert!(!result.success);
    assert_eq!(
        result.error.as_deref(),
        Some("Unattended execution cannot authorize this operation")
    );

    let after = match crate::services::agent::heartbeat::get_heartbeat() {
        Some(manager) => Some(manager.get_tasks().await.len()),
        None => None,
    };
    assert_eq!(before, after, "rejection must not write HEARTBEAT.md");

    // A Medium read such as http.fetch still auto-runs for the heartbeat.
    let fetch = PendingCall {
        call: ToolCall {
            id: "hb-fetch".into(),
            name: tools::tool_name("http.fetch"),
            arguments: r#"{"url":"https://example.com"}"#.into(),
        },
        capability_id: Some("http.fetch".into()),
        approval: None,
    };
    let (_, fetch_risk) = capability::capability_requires_confirmation_async("http.fetch")
        .await
        .expect("http.fetch requires confirmation");
    assert_eq!(fetch_risk, RiskLevel::Medium);
    let mut medium = checkpoint();
    medium.pending.push_back(fetch.clone());
    assert!(!reject_unattended_confirmation(
        &mut medium,
        &fetch,
        "http.fetch",
        fetch_risk
    ));
    assert!(medium.task.step_results.is_empty());
    assert_eq!(medium.pending.len(), 1);

    // heartbeat.* stays blocked even at Low risk.
    let mut toggle = checkpoint();
    toggle.pending.push_back(pending.clone());
    assert!(reject_unattended_confirmation(
        &mut toggle,
        &pending,
        "heartbeat.toggle",
        RiskLevel::Low
    ));
}

#[test]
fn persisted_spend_blocks_even_pending_tools_after_resume() {
    let mut value = serde_json::to_value(checkpoint()).unwrap();
    value["budget"] = json!({"spent_tokens":100,"reserved_tokens":0,"limit_tokens":100});
    let mut state: Checkpoint = serde_json::from_value(value).unwrap();
    state.pending.push_back(pending("write", "write"));
    assert!(
        state.budget_error().is_some(),
        "spent budget must stop pending effects, not only the next model call"
    );
    let restored: Checkpoint =
        serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
    assert!(restored.budget_error().is_some());
}

#[test]
fn confirmation_checks_identity_expiry_and_explicit_choice() {
    let mut state = checkpoint();
    state.pending.push_back(pending("write", "write"));
    state.wait = Some(Wait::Approval {
        fingerprint: "bound-arguments".into(),
    });
    state
        .task
        .set_pending_question(UserQuestion::confirmation("Write?", "Exact arguments"));
    let mut input = answer(&state, "yes");
    input.task_id = "another-task".into();
    assert!(apply_answer(&mut state.clone(), &input).is_err());
    input = answer(&state, "yes");
    input.question_id = "previous-question".into();
    assert!(apply_answer(&mut state.clone(), &input).is_err());
    assert!(apply_answer(&mut state.clone(), &answer(&state, "maybe")).is_err());
    let mut expired = state.clone();
    expired.task.pending_question.as_mut().unwrap().expires_at =
        Some(chrono::Utc::now() - chrono::Duration::seconds(1));
    assert!(apply_answer(&mut expired, &answer(&state, "yes")).is_err());
    input = answer(&state, "yes");
    apply_answer(&mut state, &input).unwrap();
    assert_eq!(
        state.pending.front().unwrap().approval.as_deref(),
        Some("bound-arguments")
    );
    assert!(apply_answer(&mut state, &input).is_err());
}

#[test]
fn declined_and_interrupted_actions_are_not_replayed() {
    let mut state = checkpoint();
    state.pending.push_back(pending("write", "write"));
    state.wait = Some(Wait::Approval {
        fingerprint: "bound-arguments".into(),
    });
    state
        .task
        .set_pending_question(UserQuestion::confirmation("Write?", "Exact arguments"));
    let input = answer(&state, "no");
    apply_answer(&mut state, &input).unwrap();
    assert!(state.pending.is_empty());
    assert!(state.denied.contains("bound-arguments"));

    state.pending.push_back(pending("uncertain", "write"));
    state.pending.push_back(pending("unstarted", "write"));
    state.inflight = Some("uncertain".into());
    state.attempted_effects.insert("effect-fingerprint".into());
    state.wait = Some(Wait::Recovery);
    state
        .task
        .set_pending_question(UserQuestion::free_text("Resume?", "Check outcome", true));
    let input = answer(&state, "Check what was saved");
    apply_answer(&mut state, &input).unwrap();
    assert!(state.pending.is_empty());
    assert!(state.inflight.is_none());
    assert!(state.attempted_effects.contains("effect-fingerprint"));
    let tail = &state.history[state.history.len() - 3..];
    assert!(matches!(&tail[0],ToolMessage::Tool {call,..} if call.id == "uncertain"));
    assert!(matches!(&tail[1],ToolMessage::Tool {call,..} if call.id == "unstarted"));
    assert!(matches!(&tail[2], ToolMessage::User { .. }));
}

#[test]
fn work_prompt_receives_live_route_and_request_preferences() {
    let mut state = checkpoint();
    state.request.context = Some(RequestContext {
        current_route: Some("/journal".into()),
        preferences: Some(json!({"language":"zh"})),
        active_platforms: vec!["steam".into()],
        ..Default::default()
    });
    let evidence = request_evidence(
        &state.request,
        state.task.recipe.as_ref().unwrap(),
        &state.task,
    );
    assert_eq!(evidence["route"], "/journal");
    assert_eq!(evidence["preferences"]["language"], "zh");
    assert_eq!(evidence["platforms"][0], "steam");
}

#[tokio::test]
async fn failed_discovery_does_not_partially_expand_the_tool_catalog() {
    let mut state = checkpoint();
    let call = pending("discover", "discover_tools").call;
    let result = tools::local_call(
        &mut state,
        &call,
        &json!({"ids":["time.info","missing.capability"]}),
        &Default::default(),
    )
    .await;
    assert!(result.is_err());
    assert!(state.selected.is_empty());
}

/// Shared, explicit test database; never falls back to the application's DB.
pub(super) async fn test_database() -> sea_orm::DatabaseConnection {
    let url =
        std::env::var("MYRIAD_WORK_TEST_DATABASE_URL").expect("explicit disposable database URL");
    let db = sea_orm::Database::connect(url).await.unwrap();
    let name = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT current_database() AS name",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "name")
        .unwrap();
    assert_eq!(
        name, "myriad_work_loop_test",
        "This test creates tables only in its dedicated database"
    );
    let schema = Schema::new(DatabaseBackend::Postgres);
    db.execute_raw(
        DatabaseBackend::Postgres.build(
            schema
                .create_table_from_entity(crate::models::entities::agent_tasks::Entity)
                .if_not_exists(),
        ),
    )
    .await
    .unwrap();
    for sql in [
        "CREATE TABLE IF NOT EXISTS runtime_registry (namespace TEXT NOT NULL, record_id TEXT NOT NULL, subject_id INTEGER, owner_id INTEGER, tapp_id TEXT, runtime_id TEXT, payload JSONB NOT NULL, expires_at BIGINT NOT NULL, updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), PRIMARY KEY(namespace,record_id))",
        "CREATE TABLE IF NOT EXISTS runtime_mailbox (message_id BIGSERIAL PRIMARY KEY, channel TEXT NOT NULL, runtime_id TEXT NOT NULL, payload JSONB NOT NULL, expires_at BIGINT NOT NULL)",
        // principal/auth snapshot reads is_owner + token_version.
        "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, is_admin BOOLEAN NOT NULL, is_owner BOOLEAN NOT NULL DEFAULT false, token_version INTEGER NOT NULL DEFAULT 0)",
    ] {
        db.execute_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
            .await
            .unwrap();
    }
    db
}

#[tokio::test]
#[ignore = "requires MYRIAD_WORK_TEST_DATABASE_URL pointing to myriad_work_loop_test"]
async fn postgres_recipe_discovery_is_owner_scoped() {
    use crate::models::entities::agent_task_presets;
    use sea_orm::{ActiveModelTrait, Set};
    let db = test_database().await;
    let schema = Schema::new(DatabaseBackend::Postgres);
    db.execute_raw(
        DatabaseBackend::Postgres.build(
            schema
                .create_table_from_entity(agent_task_presets::Entity)
                .if_not_exists(),
        ),
    )
    .await
    .unwrap();
    let mut state = checkpoint();
    state.user_id = 8383;
    state.request.user_id = 8383;
    let now = chrono::Utc::now().fixed_offset();
    let mut own_id = 0;
    for owner in [8383, 8384] {
        let row = agent_task_presets::ActiveModel {
            user_id: Set(owner),
            input: Set("Time preset".into()),
            preset_type: Set("favorite".into()),
            parsed_steps: Set(Some(json!(state.task.recipe))),
            last_used_at: Set(now),
            use_count: Set(0),
            created_at: Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        if owner == 8383 {
            own_id = row.id;
        }
    }
    let agent = Agent::new(db.clone()).await;
    let emitter = executor::events::StepEventEmitter::new(None);
    let call = pending("presets", "list_recipes");
    state.pending.push_back(call.clone());
    agent.work_tool(&mut state, call, &emitter).await.unwrap();
    let result = &state.task.step_results["presets"];
    assert!(
        result.success,
        "saved Recipe discovery must be available: {:?}",
        result.error
    );
    let found = result.output.as_ref().unwrap()["recipes"]
        .as_array()
        .unwrap();
    assert!(found.iter().any(|item| item["id"] == own_id));
    assert!(found.iter().all(|item| item["id"] != own_id + 1));
}

/// Uses an isolated PostgreSQL database; never falls back to the application's DB.
#[tokio::test]
#[ignore = "requires MYRIAD_WORK_TEST_DATABASE_URL pointing to myriad_work_loop_test"]
async fn postgres_fencing_recovery_and_observation_driven_execution() {
    let db = test_database().await;
    let mut state = checkpoint();
    store::save(&db, &mut state).await.unwrap();
    assert!(
        executor::task_store::save_task_to_db(0, &state.task)
            .await
            .unwrap_err()
            .contains("revision-checked")
    );
    assert!(store::load(&db, &state.task.task_id, 999).await.is_err());
    let mut other = state.clone();
    let (first, second) = tokio::join!(store::save(&db, &mut state), store::save(&db, &mut other));
    assert_ne!(
        first.is_ok(),
        second.is_ok(),
        "only one continuation claims a revision"
    );
    let mut current = store::load(&db, &state.task.task_id, 0).await.unwrap();
    store::recover(&db).await.unwrap();
    assert_eq!(
        store::load(&db, &state.task.task_id, 0)
            .await
            .unwrap()
            .task
            .status,
        TaskStatus::Running
    );
    current.pending.push_back(pending("uncertain", "write"));
    current.inflight = Some("uncertain".into());
    current.attempted_effects.insert("effect".into());
    store::save(&db, &mut current).await.unwrap();
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_tasks SET updated_at=NOW()-INTERVAL '2 minutes' WHERE id=$1",
        [current.task.task_id.clone().into()],
    ))
    .await
    .unwrap();
    store::recover(&db).await.unwrap();
    let recovered = store::load(&db, &current.task.task_id, 0).await.unwrap();
    assert_eq!(recovered.task.status, TaskStatus::WaitingForInput);
    assert_eq!(recovered.inflight.as_deref(), Some("uncertain"));
    assert!(recovered.attempted_effects.contains("effect"));
    assert!(
        store::save(&db, &mut current).await.is_err(),
        "expired worker is fenced out"
    );
    let mut cancelled = recovered;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_tasks SET status='cancelled' WHERE id=$1",
        [cancelled.task.task_id.clone().into()],
    ))
    .await
    .unwrap();
    assert!(
        store::save(&db, &mut cancelled).await.is_err(),
        "cancellation cannot be overwritten"
    );

    // A stale boot/timeout observer cannot kill a resumed task or a new question.
    let mut expired = checkpoint();
    expired.wait = Some(Wait::Answer {
        call: pending("ask", "ask_user").call,
    });
    expired
        .task
        .set_pending_question(UserQuestion::free_text("Old question", "", true));
    expired.task.pending_question.as_mut().unwrap().expires_at =
        Some(chrono::Utc::now() - chrono::Duration::seconds(1));
    let old_question = expired
        .task
        .pending_question
        .as_ref()
        .unwrap()
        .question_id
        .clone();
    store::save(&db, &mut expired).await.unwrap();
    assert!(
        !store::expire_question(&db, &expired.task.task_id, 0, "wrong-question")
            .await
            .unwrap()
    );
    assert!(
        store::expire_question(&db, &expired.task.task_id, 0, &old_question)
            .await
            .unwrap()
    );
    assert!(store::save(&db, &mut expired).await.is_err());
    assert_eq!(
        store::load(&db, &expired.task.task_id, 0)
            .await
            .unwrap()
            .task
            .status,
        TaskStatus::Failed
    );
    let mut renewed = checkpoint();
    renewed
        .task
        .set_pending_question(UserQuestion::free_text("New question", "", true));
    store::save(&db, &mut renewed).await.unwrap();
    assert!(
        !store::expire_question(&db, &renewed.task.task_id, 0, &old_question)
            .await
            .unwrap()
    );
    assert_eq!(
        store::load(&db, &renewed.task.task_id, 0)
            .await
            .unwrap()
            .task
            .status,
        TaskStatus::WaitingForInput
    );

    // A terminal Tapp result is eligible only once the matching task question
    // commits, using the real camelCase interaction serialization.
    use crate::services::agent_interaction::{
        AgentInteractionSnapshot, InteractionSource, InteractionState,
    };
    let mut interaction_task = checkpoint();
    store::save(&db, &mut interaction_task).await.unwrap();
    let interaction_id = uuid::Uuid::new_v4().to_string();
    let snapshot = AgentInteractionSnapshot {
        version: 2,
        interaction_id: interaction_id.clone(),
        interaction_type: "confirm".into(),
        tapp_id: "test.app".into(),
        state: InteractionState::Completed,
        input: json!({}),
        input_schema: None,
        result_schema: None,
        deadline: chrono::Utc::now().to_rfc3339(),
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
        result: Some(json!({"ok":true})),
        rejection_reason: None,
        source: InteractionSource {
            agent_id: "myriad.agent".into(),
            task_id: Some(interaction_task.task.task_id.clone()),
        },
    };
    db.execute_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
        "INSERT INTO runtime_registry(namespace,record_id,payload,expires_at) VALUES ('agent_interaction',$1,$2,EXTRACT(EPOCH FROM NOW())::bigint+900)",
        [interaction_id.clone().into(), json!({"subject_id":0,"snapshot":snapshot}).into()])).await.unwrap();
    let matches_result = |rows: Vec<Value>| {
        rows.iter()
            .any(|r| r["snapshot"]["interactionId"] == interaction_id)
    };
    assert!(!matches_result(
        crate::services::tapp_agent_interaction::waiting_work_result_payloads(&db)
            .await
            .unwrap()
    ));
    let mut question = UserQuestion::free_text("Interaction", "", false);
    question.question_id = format!("tapp_interaction:{interaction_id}");
    interaction_task.task.set_pending_question(question);
    store::save(&db, &mut interaction_task).await.unwrap();
    assert!(matches_result(
        crate::services::tapp_agent_interaction::waiting_work_result_payloads(&db)
            .await
            .unwrap()
    ));
    interaction_task.user_id = 777;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_tasks SET user_id=777 WHERE id=$1",
        [interaction_task.task.task_id.clone().into()],
    ))
    .await
    .unwrap();
    assert!(!matches_result(
        crate::services::tapp_agent_interaction::waiting_work_result_payloads(&db)
            .await
            .unwrap()
    ));

    // Confirmation is bound to concrete arguments and current grants, not a
    // boolean that can authorize a different action after a pause.
    db.execute_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "INSERT INTO users (id, is_admin, is_owner) VALUES (8181, true, false) \
         ON CONFLICT(id) DO UPDATE SET is_admin=true",
    ))
    .await
    .unwrap();
    let agent = Agent::new(db.clone()).await;
    let emitter = executor::events::StepEventEmitter::new(None);
    let mut approval = checkpoint();
    approval.user_id = 8181;
    approval.request.user_id = 8181;
    let mut call = pending("trigger", &tools::tool_name("scheduler.trigger"));
    call.capability_id = Some("scheduler.trigger".into());
    call.call.arguments = json!({"taskId":"original"}).to_string();
    approval.pending.push_back(call.clone());
    assert!(
        agent
            .work_tool(&mut approval, call.clone(), &emitter)
            .await
            .unwrap()
    );
    assert!(approval.task.recipe.as_ref().unwrap().steps.is_empty());
    let input = answer(&approval, "yes");
    apply_answer(&mut approval, &input).unwrap();
    approval.pending.front_mut().unwrap().call.arguments = json!({"taskId":"changed"}).to_string();
    let changed = approval.pending.front().unwrap().clone();
    assert!(
        agent
            .work_tool(&mut approval, changed, &emitter)
            .await
            .unwrap(),
        "changed arguments require a new confirmation"
    );
    let input = answer(&approval, "yes");
    apply_answer(&mut approval, &input).unwrap();
    db.execute_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "UPDATE users SET is_admin=false WHERE id=8181",
    ))
    .await
    .unwrap();
    // Raw SQL role writes bypass the service layer; drop the auth snapshot cache.
    crate::middleware::auth::invalidate_auth_cache_local(8181);
    crate::services::principal::invalidate_site_owner_cache();
    let approved = approval.pending.front().unwrap().clone();
    assert!(
        !agent
            .work_tool(&mut approval, approved, &emitter)
            .await
            .unwrap()
    );
    assert!(
        !approval.task.step_results["trigger"].success,
        "revoked grants block an already approved call"
    );
    assert!(
        approval.task.recipe.as_ref().unwrap().steps.is_empty(),
        "the handler never ran"
    );

    // The fake provider chooses its next action from actual preceding results.
    // It exercises native HTTP/SSE, discovery, dispatch, output validation and persistence.
    let fail_once = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let app = axum::Router::new().route("/v1/chat/completions", axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
        let fail_once = fail_once.clone();
        async move {
        let messages = body["messages"].as_array().unwrap();
        let results: Vec<_> = messages.iter().filter(|m|m["role"]=="tool").collect();
        if results.len() == 2 && fail_once.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return ([ ("content-type","text/event-stream") ], "data: [DONE]\n\n".to_string());
        }
        let (delta,reason) = match results.len() {
            0 => (json!({"tool_calls":[{"index":0,"id":"discover","function":{"name":"discover_tools","arguments":"{\"ids\":[\"time.info\"]}"}}]}), "tool_calls"),
            1 => {
                let result:Value=serde_json::from_str(results[0]["content"].as_str().unwrap()).unwrap();
                let name=result["loaded"][0]["tool"].as_str().unwrap();
                assert!(body["tools"].as_array().unwrap().iter().any(|t|t["function"]["name"]==name));
                (json!({"tool_calls":[{"index":0,"id":"tokyo","function":{"name":name,"arguments":"{\"timezone\":\"Asia/Tokyo\"}"}}]}),"tool_calls")
            },
            2 => {
                let result:Value=serde_json::from_str(results[1]["content"].as_str().unwrap()).unwrap();
                assert!(result.get("error").is_none(),"actual handler must succeed: {result}");
                (json!({"tool_calls":[{"index":0,"id":"utc","function":{"name":tools::tool_name("time.info"),"arguments":"{\"timezone\":\"UTC\"}"}}]}),"tool_calls")
            },
            3 => (json!({"content":"東京と UTC の時刻を確認しました。"}),"stop"),
            _ => panic!("unexpected extra model call"),
        };
        ([ ("content-type","text/event-stream") ],format!("data: {}\n\ndata: [DONE]\n\n",json!({"choices":[{"delta":delta,"finish_reason":reason}]})))
    }}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let analyzer = crate::services::analyzer::AiAnalyzer::new(
        crate::services::analyzer::AiProvider::OpenAI,
        "test-only".into(),
        "test-model".into(),
        Some(format!("http://{address}/v1")),
    )
    .await;
    let agent = Agent::new(db.clone()).await;
    let mut state = checkpoint();
    store::save(&db, &mut state).await.unwrap();
    let emitter = executor::events::StepEventEmitter::new(None);
    agent
        .drive_work_loop(&mut state, None, &emitter, &analyzer)
        .await
        .unwrap();
    assert_eq!(state.task.status, TaskStatus::WaitingForInput);
    assert!(matches!(state.wait, Some(Wait::Recovery)));
    assert_eq!(state.task.recipe.as_ref().unwrap().steps.len(), 1);
    state = store::load(&db, &state.task.task_id, 0).await.unwrap();
    let input = answer(&state, "Continue");
    apply_answer(&mut state, &input).unwrap();
    store::save(&db, &mut state).await.unwrap();
    agent
        .drive_work_loop(&mut state, None, &emitter, &analyzer)
        .await
        .unwrap();
    assert_eq!(state.task.status, TaskStatus::Completed);
    assert_eq!(state.rounds, 5);
    assert_eq!(state.calls, 3);
    assert_eq!(state.task.recipe.as_ref().unwrap().steps.len(), 2);
    assert!(state.task.step_results["tokyo"].success);
    assert!(state.task.step_results["utc"].success);
    assert_eq!(
        store::load(&db, &state.task.task_id, 0)
            .await
            .unwrap()
            .final_text,
        "東京と UTC の時刻を確認しました。"
    );
    state.plan = json!([{"step":"Inspect time","status":"completed"}]);
    store::save(&db, &mut state).await.unwrap();
    let response = saved_response(&db, &state.task.task_id, 0).await.unwrap();
    assert_eq!(response.message, state.final_text);
    assert_eq!(response.data.as_ref().unwrap()["workPlan"], state.plan);
    assert!(response.frontend_action.is_none());
    let public = serde_json::to_value(response).unwrap().to_string();
    assert!(
        !public.contains("test-model"),
        "provider continuation stays server-only"
    );
    assert!(!public.contains("lease_id"));
    server.abort();
}

#[tokio::test]
async fn stopping_work_lease_reclaims_cancellation_marker() {
    let task_id = format!("work-cancel-{}", uuid::Uuid::new_v4());
    let lease = store::lease(
        sea_orm::DatabaseConnection::default(),
        task_id.clone(),
        "lease".into(),
    );
    executor::task_store::CANCELLATION_TOKENS
        .lock()
        .unwrap()
        .insert(task_id.clone());
    drop(lease);
    assert!(
        !executor::task_store::CANCELLATION_TOKENS
            .lock()
            .unwrap()
            .contains(&task_id)
    );
}
