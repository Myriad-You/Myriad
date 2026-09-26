use super::acceptance_tests::*;
use super::*;
use crate::models::entities::{agent_task_presets as presets, tapp_storage};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait, QueryFilter,
    Schema, Set, Statement,
};

fn step(id: &str, cap: &str, params: Value) -> RecipeStep {
    RecipeStep {
        id: id.into(),
        order: 0,
        capability_id: cap.into(),
        action: id.into(),
        params: serde_json::from_value(params).unwrap(),
        depends_on: vec![],
        on_failure: FailureStrategy::Abort,
        retry: None,
        timeout_ms: None,
        model_tier: None,
        generator: None,
    }
}
async fn preset(db: &sea_orm::DatabaseConnection, user: i32, steps: Vec<RecipeStep>) -> i32 {
    db.execute_raw(
        DatabaseBackend::Postgres.build(
            Schema::new(DatabaseBackend::Postgres)
                .create_table_from_entity(presets::Entity)
                .if_not_exists(),
        ),
    )
    .await
    .unwrap();
    let mut recipe = tests::checkpoint().task.recipe.unwrap();
    recipe.steps = steps;
    let now = chrono::Utc::now().fixed_offset();
    presets::ActiveModel {
        user_id: Set(user),
        input: Set("fixture".into()),
        preset_type: Set("favorite".into()),
        parsed_steps: Set(Some(json!(recipe))),
        last_used_at: Set(now),
        created_at: Set(now),
        use_count: Set(0),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap()
    .id
}
fn parent(id: i32) -> PendingCall {
    PendingCall {
        call: ToolCall {
            id: "workflow".into(),
            name: "run_recipe".into(),
            arguments: json!({"preset_id":id}).to_string(),
        },
        capability_id: None,
        approval: None,
    }
}
fn enqueue(state: &mut Checkpoint, pending: PendingCall) {
    state.history.push(ToolMessage::Assistant {turn:serde_json::from_value(json!({"text":"","calls":[pending.call],"native":{"role":"assistant","content":"","tool_calls":[sse_call("workflow","run_recipe",serde_json::from_str::<Value>(&pending.call.arguments).unwrap())]},"provider":"openai","model":"test-model"})).unwrap()});
    state.pending.push_back(pending);
}
async fn final_model() -> (
    crate::services::analyzer::AiAnalyzer,
    tokio::task::JoinHandle<()>,
) {
    model(axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|axum::Json(body): axum::Json<Value>| async move {
            let messages = body["messages"].as_array().unwrap();
            let results: Vec<_> = messages.iter().filter(|m| m["role"] == "tool").collect();
            assert_eq!(
                results.len(),
                1,
                "Only the parent workflow result belongs in provider history"
            );
            assert_eq!(results[0]["tool_call_id"], "workflow");
            (
                [("content-type", "text/event-stream")],
                sse("Workflow result received.", vec![]),
            )
        }),
    ))
    .await
}

#[tokio::test]
#[ignore = "requires isolated myriad_work_loop_test"]
async fn postgres_recipe_orders_dependencies_and_resumes_without_duplicate_note() {
    let db = business_database(8581).await;
    let agent = Agent::new(db.clone()).await;
    let fact = uuid::Uuid::new_v4().to_string();
    let id = preset(
        &db,
        8581,
        vec![
            step(
                "save",
                "note.create",
                json!({"title":"Recipe result","contentFrom":"read.data[0].text"}),
            ),
            step(
                "read",
                "data.transform",
                json!({"input":{"items":[{"text":fact}]},"pipeline":[]}),
            ),
        ],
    )
    .await;
    let mut state = state_for(8581);
    let pending = parent(id);
    enqueue(&mut state, pending.clone());
    store::save(&db, &mut state).await.unwrap();
    let emitter = executor::events::StepEventEmitter::new(None);
    assert!(
        !agent
            .work_tool(&mut state, pending, &emitter)
            .await
            .unwrap()
    );
    assert!(recipes::advance(&mut state).unwrap());
    let read = state.pending.front().unwrap().clone();
    agent.work_tool(&mut state, read, &emitter).await.unwrap();
    assert!(recipes::advance(&mut state).unwrap());
    let save = state.pending.front().unwrap().clone();
    agent.work_tool(&mut state, save, &emitter).await.unwrap();
    store::save(&db, &mut state).await.unwrap();
    // Reload between effect completion and parent aggregation: do not replay it.
    state = store::load(&db, &state.task.task_id, 8581).await.unwrap();
    let (analyzer, server) = final_model().await;
    agent
        .drive_work_loop(&mut state, None, &emitter, &analyzer)
        .await
        .unwrap();
    server.abort();
    assert_eq!(state.task.status, TaskStatus::Completed);
    let output = state.task.step_results["workflow"].output.as_ref().unwrap();
    assert_eq!(output["success"], true, "{output}");
    assert_eq!(output["steps"].as_array().unwrap().len(), 2);
    let notes = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(8581))
        .all(&db)
        .await
        .unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].value["content"], fact);
}

#[tokio::test]
#[ignore = "requires isolated myriad_work_loop_test"]
async fn postgres_recipe_confirmation_denial_revocation_and_owner_isolation() {
    let db = business_database(8582).await;
    let agent = Agent::new(db.clone()).await;
    let emitter = executor::events::StepEventEmitter::new(None);
    let mut trigger = step(
        "trigger",
        "scheduler.trigger",
        json!({"taskId":"never-run"}),
    );
    trigger.on_failure = FailureStrategy::Skip;
    let id = preset(
        &db,
        8582,
        vec![
            trigger,
            step(
                "save",
                "note.create",
                json!({"title":"Forbidden","content":"Must not be saved after denial"}),
            ),
        ],
    )
    .await;
    let mut foreign = state_for(8583);
    let pending = parent(id);
    enqueue(&mut foreign, pending.clone());
    agent
        .work_tool(&mut foreign, pending, &emitter)
        .await
        .unwrap();
    assert!(!foreign.task.step_results["workflow"].success);
    assert!(foreign.recipe_run.is_none());
    for revoke in [false, true] {
        let mut state = state_for(8582);
        let pending = parent(id);
        enqueue(&mut state, pending.clone());
        store::save(&db, &mut state).await.unwrap();
        agent
            .work_tool(&mut state, pending, &emitter)
            .await
            .unwrap();
        recipes::advance(&mut state).unwrap();
        let pending = state.pending.front().unwrap().clone();
        assert!(
            agent
                .work_tool(&mut state, pending.clone(), &emitter)
                .await
                .unwrap()
        );
        store::save(&db, &mut state).await.unwrap();
        state = store::load(&db, &state.task.task_id, 8582).await.unwrap();
        if revoke {
            db.execute_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "UPDATE users SET is_admin=false WHERE id=8582",
            ))
            .await
            .unwrap();
            crate::middleware::auth::invalidate_auth_cache_local(8582);
            crate::services::principal::invalidate_site_owner_cache();
        }
        let answer = UserAnswer {
            task_id: state.task.task_id.clone(),
            question_id: state
                .task
                .pending_question
                .as_ref()
                .unwrap()
                .question_id
                .clone(),
            answer: if revoke { "yes" } else { "no" }.into(),
            skipped: false,
        };
        apply_answer(&mut state, &answer).unwrap();
        if !revoke {
            assert!(
                state.recipe_run.is_none(),
                "Denial must stop even a skip-on-error recipe"
            );
        }
        let (analyzer, server) = final_model().await;
        agent
            .drive_work_loop(&mut state, None, &emitter, &analyzer)
            .await
            .unwrap();
        server.abort();
        assert!(!state.task.step_results[&pending.call.id].success);
        assert!(!state.attempted_effects.contains(&operation_key(
            "scheduler.trigger",
            &json!({"taskId":"never-run"})
        )));
    }
    assert!(
        tapp_storage::Entity::find()
            .filter(tapp_storage::Column::UserId.eq(8582))
            .all(&db)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
#[ignore = "requires isolated myriad_work_loop_test"]
async fn postgres_budget_survives_reload_and_blocks_pending_write() {
    let db = business_database(8584).await;
    let agent = Agent::new(db.clone()).await;
    let mut state = state_for(8584);
    state.budget = Some(budget::Budget {
        spent_tokens: 90,
        reserved_tokens: 10,
        limit_tokens: 100,
    });
    state.pending.push_back(call(
        "blocked",
        "note.create",
        json!({"title":"No","content":"No"}),
    ));
    store::save(&db, &mut state).await.unwrap();
    state = store::load(&db, &state.task.task_id, 8584).await.unwrap();
    let (analyzer, server) = final_model().await;
    let error = agent
        .drive_work_loop(
            &mut state,
            None,
            &executor::events::StepEventEmitter::new(None),
            &analyzer,
        )
        .await
        .unwrap_err();
    server.abort();
    assert!(error.contains("token budget"));
    assert_eq!(state.budget.as_ref().unwrap().spent_tokens, 100);
    let restored = store::load(&db, &state.task.task_id, 8584).await.unwrap();
    assert_eq!(restored.budget.unwrap().spent_tokens, 100);
    assert!(
        tapp_storage::Entity::find()
            .filter(tapp_storage::Column::UserId.eq(8584))
            .all(&db)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
#[ignore = "requires isolated myriad_work_loop_test"]
async fn postgres_interrupted_model_keeps_reserved_spend_after_retry() {
    let db = business_database(8585).await;
    let agent = Agent::new(db.clone()).await;
    let mut state = state_for(8585);
    let (analyzer, server) = model(axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|| async {
            (
                [("content-type", "text/event-stream")],
                "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
            )
        }),
    ))
    .await;
    let emitter = executor::events::StepEventEmitter::new(None);
    agent
        .drive_work_loop(&mut state, None, &emitter, &analyzer)
        .await
        .unwrap();
    assert!(matches!(state.wait, Some(Wait::Recovery)));
    let charged = state.budget.as_ref().unwrap().spent_tokens;
    assert!(
        charged >= 8192,
        "An unknown provider outcome cannot refund its output reservation"
    );
    state = store::load(&db, &state.task.task_id, 8585).await.unwrap();
    let answer = UserAnswer {
        task_id: state.task.task_id.clone(),
        question_id: state
            .task
            .pending_question
            .as_ref()
            .unwrap()
            .question_id
            .clone(),
        answer: "retry".into(),
        skipped: false,
    };
    apply_answer(&mut state, &answer).unwrap();
    agent
        .drive_work_loop(&mut state, None, &emitter, &analyzer)
        .await
        .unwrap();
    server.abort();
    assert!(state.budget.as_ref().unwrap().spent_tokens >= charged + 8192);
}
