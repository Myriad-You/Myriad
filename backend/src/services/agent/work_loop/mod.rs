//! Work is an observation-driven tool loop. Saved recipes run as frames inside it.
//! Model messages are server-only; public task state is a projection, not the
//! continuation. Waiting, effects and model boundaries are durable checkpoints.
#[cfg(test)]
mod acceptance_tests;
mod budget;
mod output;
#[cfg(test)]
mod recipe_tests;
mod recipes;
#[cfg(test)]
mod recovery_tests;
mod state;
mod store;
#[cfg(test)]
mod tests;
mod tool_schema;
mod tools;

use super::{Agent, capability, executor, types::*};
use crate::services::agent::capability::CapabilityRef;
use crate::services::analyzer::tool_calling::{ToolCall, ToolDefinition, ToolMessage};
use serde_json::{Value, json};
pub(crate) use state::is_work_recipe;
use state::*;
use std::collections::HashMap;
pub(crate) use store::{expire_question, recover};
use tokio::sync::mpsc::Sender;

impl Agent {
    /// Read-only preflight for transports. Claiming still happens atomically in
    /// resume_work_loop, so this does not authorize or consume an answer.
    pub(crate) async fn validate_work_answer(
        &self,
        task_id: &str,
        answer: &UserAnswer,
        user_id: i32,
    ) -> Result<(), String> {
        let state = store::load(&self.db, task_id, user_id).await?;
        state.validate_answer(answer)?;
        if matches!(state.wait, Some(Wait::Interaction { .. })) {
            crate::services::tapp_agent_interaction::verify_task_answer(&self.db, user_id, answer)
                .await?;
        }
        Ok(())
    }

    /// Work task record for a request. Work steps are appended as tools run.
    pub(crate) fn build_recipe_from_steps(
        steps: Vec<RecipeStep>,
        name: String,
        request: &UserRequest,
    ) -> Recipe {
        let estimated_duration_ms: u64 = steps.iter().map(|s| s.timeout_ms.unwrap_or(15000)).sum();

        let page_context = request
            .context
            .as_ref()
            .and_then(|c| c.custom_data.as_ref())
            .and_then(|d| d.get("pageContent").cloned());

        let conversation_context = request
            .context
            .as_ref()
            .and_then(|c| c.conversation_history.clone());

        let lane_key = request.context.as_ref().and_then(|c| c.lane_key.clone());
        let autonomy_permission_cap = request
            .context
            .as_ref()
            .and_then(|c| c.autonomy_permission_cap.clone());

        let mut metadata = HashMap::new();
        if let Some(route) = request
            .context
            .as_ref()
            .and_then(|c| c.current_route.clone())
            .filter(|route| !route.is_empty())
        {
            metadata.insert("current_route".to_string(), json!(route));
        }
        if let Some(custom) = request
            .context
            .as_ref()
            .and_then(|c| c.custom_data.as_ref())
        {
            if let Some(music) = custom.get("musicStatus").cloned() {
                metadata.insert("music_status".to_string(), music);
            }
            if let Some(windows) = custom.get("windowState").cloned() {
                metadata.insert("window_state".to_string(), windows);
            }
        }

        Recipe {
            id: format!("recipe_{}", uuid::Uuid::new_v4()),
            name,
            original_request: request.raw_input.clone(),
            execution_type: ExecutionType::Instant,
            steps,
            expected_output: OutputFormat::Json,
            estimated_duration_ms,
            created_at: chrono::Utc::now(),
            metadata,
            page_context,
            conversation_context,
            lane_key,
            autonomy_permission_cap,
            engine: AgentEngine::WorkLoop,
        }
    }

    pub(crate) async fn start_work_loop(
        &self,
        request: UserRequest,
        tx: Option<Sender<AgentProgressEvent>>,
    ) -> Result<AgentResponse, String> {
        let state = self.new_work_checkpoint(&request).await;
        self.launch_work_loop(state, tx).await
    }

    /// Run a saved preset as a Work task. Its steps run first, each through
    /// `work_tool` (grants, confirmation, checkpoints); the model then answers
    /// from the aggregate folded into the user turn.
    pub(crate) async fn start_preset_work_loop(
        &self,
        request: UserRequest,
        preset_id: i32,
        tx: Option<Sender<AgentProgressEvent>>,
    ) -> Result<AgentResponse, String> {
        // A saved preset is an execution shortcut, not a way around her mood.
        let refusal = super::merope::maybe_refuse_new_task(&self.db, request.user_id).await;
        if let Some(response) = self
            .mood_refuse_response(request.user_id, refusal, tx.as_ref())
            .await
        {
            return Ok(response);
        }
        let mut state = self.new_work_checkpoint(&request).await;
        let call = ToolCall {
            id: format!("preset_{}", uuid::Uuid::new_v4().simple()),
            name: "run_recipe".into(),
            arguments: json!({"preset_id": preset_id}).to_string(),
        };
        recipes::start(&self.db, &mut state, call, preset_id).await?;
        if let Some(frame) = state.recipe_run.as_mut() {
            frame.direct = true;
        }
        self.launch_work_loop(state, tx).await
    }

    async fn new_work_checkpoint(&self, request: &UserRequest) -> Checkpoint {
        let recipe = Self::build_recipe_from_steps(
            vec![],
            request.raw_input.chars().take(120).collect(),
            request,
        );
        let mut task = TaskState::new(&recipe);
        task.status = TaskStatus::Running;
        task.lane_id = recipe.lane_key.clone();
        let mut context = ExecutionContext::from_request_full(
            &request.raw_input,
            &request.raw_input,
            recipe.page_context.clone(),
            recipe.conversation_context.clone(),
        );
        context.autonomy_permission_cap = recipe.autonomy_permission_cap.clone();
        for (key, value) in &recipe.metadata {
            // Recipe metadata is data; it must not be able to name the task.
            if key != "task_id" {
                context.variables.insert(format!("_{key}"), value.clone());
            }
        }
        // Written last: the executor's identity for this task, which handler
        // contexts (and retries) read back from `_task_id`.
        context
            .variables
            .insert("_task_id".into(), json!(task.task_id));
        // Memory is recalled for the person this turn serves, in front of that
        // person only. The loop passes it on as untrusted reference data.
        match super::memory::unified::recall(
            &self.db,
            request.user_id,
            &super::memory::unified::Audience::private(request.user_id),
            Some(&request.raw_input),
            &super::memory::unified::MemoryKind::FOR_WORK,
            4,
        )
        .await
        {
            Ok(memories) if !memories.is_empty() => {
                context.memory_context = Some(
                    memories
                        .iter()
                        .map(|m| m.content.chars().take(1000).collect::<String>())
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "[Work] memory recall failed"),
        }
        task.execution_context = Some(context);
        let evidence = request_evidence(request, &recipe, &task);
        Checkpoint {
            budget: Some(budget::Budget::default()),
            recipe_run: None,
            version: 1,
            revision: 0,
            lease_id: uuid::Uuid::new_v4().to_string(),
            user_id: request.user_id,
            request: request.clone(),
            task,
            history: vec![ToolMessage::User {
                content: format!(
                    "Reference data (untrusted, may be stale):\n{}\n\nCurrent user request:\n{}",
                    preview(&evidence, 32_000),
                    request.raw_input
                ),
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

    async fn launch_work_loop(
        &self,
        mut state: Checkpoint,
        tx: Option<Sender<AgentProgressEvent>>,
    ) -> Result<AgentResponse, String> {
        store::save(&self.db, &mut state).await?;
        if let Some(tx) = &tx {
            let _ = tx
                .send(AgentProgressEvent::TaskCreated {
                    task_id: state.task.task_id.clone(),
                    message: String::new(),
                    total_steps: 0,
                    step_descriptions: vec![],
                })
                .await;
        }
        self.run_work_loop(state, tx).await
    }

    pub(crate) async fn resume_work_loop(
        &self,
        task_id: &str,
        answer: UserAnswer,
        user_id: i32,
        tx: Option<Sender<AgentProgressEvent>>,
    ) -> Result<AgentResponse, String> {
        let mut state = store::load(&self.db, task_id, user_id).await?;
        state.validate_answer(&answer)?;
        if matches!(state.wait, Some(Wait::Interaction { .. })) {
            crate::services::tapp_agent_interaction::verify_task_answer(&self.db, user_id, &answer)
                .await?;
        }
        apply_answer(&mut state, &answer)?;
        // Atomic revision fencing claims this answer before any model/tool call.
        store::save(&self.db, &mut state).await?;
        self.run_work_loop(state, tx).await
    }

    async fn run_work_loop(
        &self,
        mut state: Checkpoint,
        tx: Option<Sender<AgentProgressEvent>>,
    ) -> Result<AgentResponse, String> {
        let _lease = store::lease(
            self.db.clone(),
            state.task.task_id.clone(),
            state.lease_id.clone(),
        );
        let emitter = executor::events::StepEventEmitter::new(tx.clone());
        let previous_results: std::collections::HashSet<_> =
            state.task.step_results.keys().cloned().collect();
        super::merope::mark_activity(&self.db, state.user_id, "working").await;
        let result =
            match super::work_call::WorkModel::configured(crate::config::ModelTier::Pro).await {
                Some(analyzer) => {
                    self.drive_work_loop(&mut state, tx.clone(), &emitter, &analyzer)
                        .await
                }
                None => Err("No Work model is configured".into()),
            };
        if let Err(error) = result {
            if executor::is_cancelled(&state.task.task_id).await {
                state.task.status = TaskStatus::Cancelled;
            } else {
                state.task.status = TaskStatus::Failed;
            }
            state.task.error = Some(error.clone());
            state.final_text = error;
            state.task.completed_at = Some(chrono::Utc::now());
            // A stale worker must not overwrite a newer continuation.
            store::save(&self.db, &mut state).await?;
        }
        super::merope::mark_activity(&self.db, state.user_id, "idle").await;
        if state.task.status == TaskStatus::WaitingForInput {
            if let Some(question) = &state.task.pending_question {
                emitter
                    .waiting_for_input(&state.task.task_id, question)
                    .await;
            }
        } else if let Some(tx) = &tx {
            let _ = tx
                .send(AgentProgressEvent::SummaryToken {
                    token: state.final_text.clone(),
                    done: false,
                })
                .await;
            let _ = tx
                .send(AgentProgressEvent::SummaryToken {
                    token: String::new(),
                    done: true,
                })
                .await;
        }
        if matches!(
            state.task.status,
            TaskStatus::Completed | TaskStatus::Failed
        ) {
            if let Some(recipe) = &state.task.recipe {
                super::agent_footer::record_execution_memory(
                    super::agent_footer::MemoryRecordParams {
                        db: &self.db,
                        user_id: state.user_id,
                        user_input: &state.request.raw_input,
                        recipe,
                        success: state.task.status == TaskStatus::Completed,
                        step_results: Some(&state.task.step_results),
                    },
                )
                .await;
            }
        }
        let actions = if tx.is_none() {
            Some(output::new_frontend_actions(&state.task, &previous_results))
        } else {
            None
        };
        let mut response = checkpoint_response(state);
        if let Some(actions) = actions {
            let data = response.data.as_mut().unwrap();
            data["frontendActions"] = json!(actions);
            response.frontend_action = self.extract_frontend_action(data);
        }
        Ok(response)
    }

    async fn drive_work_loop(
        &self,
        state: &mut Checkpoint,
        tx: Option<Sender<AgentProgressEvent>>,
        emitter: &executor::events::StepEventEmitter,
        analyzer: &super::work_call::WorkModel,
    ) -> Result<(), String> {
        // Legacy checkpoints keep their already-consumed context estimate.
        let budget = state.budget.get_or_insert_with(|| budget::Budget {
            spent_tokens: (state.input_chars.div_ceil(4) as u64).saturating_add(
                state
                    .history
                    .iter()
                    .filter_map(|message| {
                        if let ToolMessage::Assistant { turn } = message {
                            Some(turn.native.to_string().len().div_ceil(4) as u64)
                        } else {
                            None
                        }
                    })
                    .sum::<u64>(),
            ),
            ..Default::default()
        });
        // A reservation left by a killed request is charged once, never reset.
        budget.settle(None);
        store::save(&self.db, state).await?;
        crate::services::ai_cost_ledger::with_work_usage_meter(
            crate::services::ai_cost_ledger::AiUsageMeter::new(),
            self.drive_metered_work_loop(state, tx, emitter, analyzer),
        )
        .await
    }

    async fn drive_metered_work_loop(
        &self,
        state: &mut Checkpoint,
        tx: Option<Sender<AgentProgressEvent>>,
        emitter: &executor::events::StepEventEmitter,
        analyzer: &super::work_call::WorkModel,
    ) -> Result<(), String> {
        loop {
            if executor::is_cancelled(&state.task.task_id).await {
                return Err("Task cancelled".into());
            }
            if let Some(error) = state.budget_error() {
                return Err(error.into());
            }
            let steering = executor::take_steering(&self.db, &state.task.task_id).await;
            if !steering.is_empty() {
                // Resolve the previous assistant batch before injecting user messages.
                while let Some(pending) = state.pending.pop_front() {
                    state.tool_result(pending.call,&json!({"cancelled":"Superseded by a new user instruction before execution"}));
                }
                recipes::abort(state, "Saved recipe superseded by a new user instruction");
                let instruction = steering.join("\n");
                state.history.push(ToolMessage::User {
                    content: instruction.clone(),
                });
                if let Some(context) = state.task.execution_context.as_mut() {
                    context
                        .user_intent
                        .push_str(&format!("\nUser update: {instruction}"));
                }
                store::save(&self.db, state).await?;
            }
            if recipes::advance(state)? {
                store::save(&self.db, state).await?;
                continue;
            }
            if let Some(pending) = state.pending.front().cloned() {
                let previous_plan = state.plan.clone();
                let started = std::time::Instant::now();
                let paused = self.work_tool(state, pending, emitter).await?;
                state.active_ms = state
                    .active_ms
                    .saturating_add(started.elapsed().as_millis() as u64);
                store::save(&self.db, state).await?;
                if state.plan != previous_plan {
                    if let Some(tx) = &tx {
                        let _ = tx
                            .send(AgentProgressEvent::WorkPlanUpdated {
                                task_id: state.task.task_id.clone(),
                                steps: serde_json::from_value(state.plan.clone())
                                    .unwrap_or_default(),
                            })
                            .await;
                    }
                }
                if paused {
                    return Ok(());
                }
                continue;
            }
            let granted = tools::granted_for(state, &self.db).await;
            state.prune_results();
            let definitions = tools::definitions(state, &granted).await;
            let system = tools::system_prompt(state, &granted).await;
            let input_chars = system.chars().count()
                + serde_json::to_string(&state.history)
                    .unwrap()
                    .chars()
                    .count()
                + serde_json::to_string(&definitions).unwrap().chars().count();
            if state.input_chars.saturating_add(input_chars) > MAX_INPUT_CHARS {
                return Err("The task reached its context budget".into());
            }
            // UTF-8 bytes provide a conservative preflight estimate; provider usage
            // settles the reservation. No tokenizer or dollar price is invented.
            let input_estimate = (system.len()
                + serde_json::to_vec(&state.history).unwrap().len()
                + serde_json::to_vec(&definitions).unwrap().len())
                as u64;
            let max_output = state
                .budget
                .as_mut()
                .unwrap()
                .reserve_model(input_estimate)?;
            let before_tokens = crate::services::ai_cost_ledger::work_tokens();
            state.input_chars += input_chars;
            state.rounds += 1;
            store::save(&self.db, state).await?;
            let started = std::time::Instant::now();
            let turn = {
                let mut visible = String::new();
                let mut last_emit = std::time::Instant::now();
                let progress = state.task.progress;
                let completed_steps = state.calls as u32;
                let inference =
                    analyzer.tool_turn(&system, &state.history, &definitions, max_output, |text| {
                        visible.push_str(&text);
                        let snapshot = visible.clone();
                        let tx = tx.clone();
                        let emit = last_emit.elapsed().as_millis() >= 150;
                        if emit {
                            last_emit = std::time::Instant::now();
                        }
                        async move {
                            if emit {
                                if let Some(tx) = tx {
                                    let _ = tx
                                        .send(AgentProgressEvent::Progress {
                                            progress,
                                            completed_steps,
                                            total_steps: 0,
                                            message: snapshot,
                                        })
                                        .await;
                                }
                            }
                        }
                    });
                tokio::pin!(inference);
                let remaining = MAX_ACTIVE_MS.saturating_sub(state.active_ms);
                let deadline = tokio::time::sleep(std::time::Duration::from_millis(remaining));
                tokio::pin!(deadline);
                let mut tick = tokio::time::interval(std::time::Duration::from_millis(500));
                loop {
                    tokio::select! {
                        result = &mut inference => break result,
                        _ = &mut deadline => return Err("The task reached its active-time limit".into()),
                        _ = tick.tick() => if executor::is_cancelled(&state.task.task_id).await { return Err("Task cancelled".into()); },
                    }
                }
            };
            state.active_ms = state
                .active_ms
                .saturating_add(started.elapsed().as_millis() as u64);
            let charged =
                crate::services::ai_cost_ledger::work_tokens().saturating_sub(before_tokens);
            state
                .budget
                .as_mut()
                .unwrap()
                .settle((turn.is_ok() && charged > 0).then_some(charged));
            let turn = match turn {
                Ok(turn) => turn,
                Err(error) => {
                    // A model transport failure did not execute a tool. Keep
                    // completed actions and the exact provider continuation.
                    state.wait = Some(Wait::Recovery);
                    state.task.set_pending_question(UserQuestion::free_text(
                        "The model request could not finish. Reply to retry from the saved progress.",
                        &error.to_string(), true,
                    ));
                    store::save(&self.db, state).await?;
                    return Ok(());
                }
            };
            if turn.calls.is_empty() {
                state.final_text = turn.text.clone();
                state.history.push(ToolMessage::Assistant { turn });
                state.task.status = TaskStatus::Completed;
                state.task.progress = 100;
                state.task.completed_at = Some(chrono::Utc::now());
                store::save(&self.db, state).await?;
                return Ok(());
            }
            if !turn.text.is_empty() {
                if let Some(tx) = &tx {
                    let _ = tx
                        .send(AgentProgressEvent::Progress {
                            progress: state.task.progress,
                            completed_steps: state.calls as u32,
                            total_steps: 0,
                            message: turn.text.clone(),
                        })
                        .await;
                }
            }
            for call in &turn.calls {
                if state.history.iter().any(|message| matches!(message, ToolMessage::Assistant {turn} if turn.calls.iter().any(|previous| previous.id == call.id))) {
                    return Err("Model reused an executed call id".into());
                }
                let capability_id = state
                    .selected
                    .iter()
                    .find(|id| tools::tool_name(id) == call.name)
                    .cloned();
                state.pending.push_back(PendingCall {
                    call: call.clone(),
                    capability_id,
                    approval: None,
                });
            }
            state.history.push(ToolMessage::Assistant { turn });
            store::save(&self.db, state).await?;
        }
    }

    async fn work_tool(
        &self,
        state: &mut Checkpoint,
        pending: PendingCall,
        emitter: &executor::events::StepEventEmitter,
    ) -> Result<bool, String> {
        let call = &pending.call;
        let params: Value = match serde_json::from_str(&call.arguments) {
            Ok(Value::Object(params)) => Value::Object(params),
            _ => {
                finish_call(
                    state,
                    &pending,
                    Err("Tool arguments must be a JSON object".into()),
                    0,
                );
                return Ok(false);
            }
        };
        let count_key = operation_key(&call.name, &params);
        if state.call_counts.get(&count_key).copied().unwrap_or(0) >= 3 {
            finish_call(state,&pending,Err("Repeated identical calls made no progress. Change the approach or explain the blocker.".into()),0);
            return Ok(false);
        }
        let granted = tools::granted_for(state, &self.db).await;
        let Some(id) = &pending.capability_id else {
            let output = match tools::validate_local(&call.name, &params) {
                Err(error) => Err(error),
                Ok(()) if call.name == "list_recipes" => {
                    recipes::list(&self.db, state.user_id).await
                }
                Ok(()) if call.name == "run_recipe" => {
                    match recipes::start(
                        &self.db,
                        state,
                        call.clone(),
                        params["preset_id"].as_i64().unwrap() as i32,
                    )
                    .await
                    {
                        Ok(()) => return Ok(false),
                        Err(error) => Err(error),
                    }
                }
                Ok(()) => tools::local_call(state, call, &params, &granted).await,
            };
            if call.name == "ask_user" && output.is_ok() {
                let question = UserQuestion::free_text(
                    params["question"].as_str().unwrap(),
                    params["context"].as_str().unwrap_or(""),
                    true,
                );
                state.pending.pop_front();
                state.calls += 1;
                state.wait = Some(Wait::Answer { call: call.clone() });
                state.task.set_pending_question(question);
                return Ok(true);
            }
            finish_call(state, &pending, output, 0);
            return Ok(false);
        };
        let capability = capability::get_capability_by_id(id).await;
        let Some(capability) = capability else {
            finish_call(
                state,
                &pending,
                Err("Tool is no longer available".into()),
                0,
            );
            return Ok(false);
        };
        let mut params_map: std::collections::HashMap<String, Value> =
            serde_json::from_value(params.clone()).unwrap();
        let permission =
            super::tool_permissions::capability_allowed_for_grants(id, &params_map, &granted).await;
        let validation = tool_schema::prepare(&capability.input_schema)
            .and_then(|schema| schema.validate(&params));
        if let Err(error) = permission.and(validation) {
            finish_call(state, &pending, Err(error), 0);
            return Ok(false);
        }
        let mcp_output_schema =
            if CapabilityRef::parse(&id).is_mcp() && capability.output_schema != json!({}) {
                match tool_schema::prepare(&capability.output_schema) {
                    Ok(schema) => Some(schema),
                    Err(error) => {
                        finish_call(state, &pending, Err(error), 0);
                        return Ok(false);
                    }
                }
            } else {
                None
            };
        let mut context = state.context();
        if context.autonomy_permission_cap.is_some() {
            // Re-read right before the effect: a revocation since the tool
            // list was built must stop this call.
            if let Err(error) = super::consciousness::authorize_capability(
                &self.db,
                state.user_id,
                context.autonomy_permission_cap.as_deref(),
                id,
                &capability.required_permissions,
            )
            .await
            {
                finish_call(state, &pending, Err(error), 0);
                return Ok(false);
            }
        }
        executor::params::inject_request_context_params(
            id,
            &mut params_map,
            context.variables.get("_current_route"),
            context.page_context.as_ref(),
            context.variables.get("_music_status"),
            context.variables.get("_window_state"),
        );
        let fingerprint = fingerprint(&capability, &json!(params_map));
        let effect_key = operation_key(id, &json!(params_map));
        if state.denied.contains(&fingerprint) || state.attempted_effects.contains(&effect_key) {
            finish_call(state,&pending,Err("This operation was declined or already attempted. Inspect the existing result instead of repeating it.".into()),0);
            return Ok(false);
        }
        if let Some((message, risk)) = capability::capability_requires_confirmation_async(id).await
        {
            if reject_unattended_confirmation(state, &pending, id, risk) {
                return Ok(false);
            } else if state.user_id != super::SYSTEM_USER_ID
                && pending.approval.as_deref() != Some(&fingerprint)
            {
                let mut question = UserQuestion::confirmation(
                    &message,
                    &format!("{}\n{}", capability.name, preview(&json!(params_map), 6000)),
                );
                question.expires_at = Some(
                    chrono::Utc::now()
                        + chrono::Duration::minutes(
                            if matches!(risk, RiskLevel::High | RiskLevel::Critical) {
                                5
                            } else {
                                15
                            },
                        ),
                );
                state.wait = Some(Wait::Approval { fingerprint });
                state.task.set_pending_question(question);
                return Ok(true);
            }
        }
        let template = state.recipe_run.as_ref().and_then(|frame| {
            (frame.active_call.as_ref() == Some(&call.id))
                .then(|| frame.steps[frame.cursor].clone())
        });
        let step = RecipeStep {
            id: call.id.clone(),
            order: state.calls as u32,
            capability_id: id.clone(),
            action: template
                .as_ref()
                .map(|step| step.action.clone())
                .unwrap_or_else(|| context.user_intent.clone()),
            params: params_map.clone(),
            depends_on: vec![],
            on_failure: FailureStrategy::Abort,
            retry: None,
            timeout_ms: template.as_ref().and_then(|step| step.timeout_ms),
            model_tier: template.as_ref().and_then(|step| step.model_tier),
            generator: None,
        };
        // Conservative: only declared DataRead operations are replay-safe. Unknown
        // MCP/network/write outcomes need reconciliation, never automatic retries.
        // Some read/UI tools optionally call AI despite requires_ai=false.
        // Reserve for every handler; unused allowance is returned on completion.
        let budget = state.budget.get_or_insert_with(Default::default);
        let allowance = budget.remaining().min(32768);
        if allowance == 0 {
            return Err("The task reached its token budget".into());
        }
        budget.reserved_tokens = allowance;
        let request_budget =
            crate::services::analyzer::request_budget::RequestBudget::new(allowance);
        let effectful = capability.category != CapabilityCategory::DataRead;
        if effectful {
            state.attempted_effects.insert(effect_key);
        }
        let before_tokens = crate::services::ai_cost_ledger::work_tokens();
        state.inflight = Some(call.id.clone());
        if let Some(recipe) = state.task.recipe.as_mut() {
            recipe.steps.push(step.clone());
        }
        store::save(&self.db, state).await?;
        emitter
            .step_started(
                &call.id,
                state.calls as u32,
                (state.calls + state.pending.len()) as u32,
                &capability.name,
                capability.description.clone(),
            )
            .await;
        let started = std::time::Instant::now();
        let tier = super::tier_router::resolve_tier_with_breaker(id, step.model_tier);
        let analyzer = super::work_call::WorkModel::configured(tier).await;
        let handler = executor::handlers::HandlerContext {
            db: &self.db,
            ai_analyzer: analyzer.as_ref(),
            user_id: state.user_id,
            task_id: Some(state.task.task_id.clone()),
            step_id: Some(call.id.clone()),
            execution_context: Some(context.clone()),
            autonomy_permission_cap: context.autonomy_permission_cap.clone(),
        };
        let timeout = super::executor_utils_pure::step_timeout_secs(
            step.timeout_ms,
            capability.estimated_duration_ms,
            super::executor_utils_pure::category_timeout_fallback_secs(&capability.category),
            capability.requires_ai,
        )
        .min(MAX_ACTIVE_MS.saturating_sub(state.active_ms).div_ceil(1000));
        let output = request_budget
            .scope(
                executor::executor_footer::execute_capability_with_timeout_and_cancel(
                    id,
                    &step.action,
                    &capability.category,
                    &params_map,
                    &handler,
                    timeout,
                    Some(&state.task.task_id),
                ),
            )
            .await;
        let charged = crate::services::ai_cost_ledger::work_tokens().saturating_sub(before_tokens);
        if let Some(budget) = state.budget.as_mut() {
            if charged > 0 || budget.reserved_tokens > 0 {
                budget.settle(Some(charged.max(request_budget.charged())));
            }
        }
        let duration = started.elapsed().as_millis() as u64;
        let output = match output {
            Ok(output) => {
                output::publish(
                    emitter,
                    &state.task.task_id,
                    &step,
                    &capability,
                    mcp_output_schema.as_deref(),
                    duration,
                    output,
                    &mut context,
                )
                .await
            }
            // Any failed effectful call goes to recovery below, whatever its
            // outcome, so the Work loop needs only the message.
            Err(error) => Err(error.message),
        };
        state.task.execution_context = Some(context);
        super::tier_router::record_step_to_breaker(tier, output.is_ok());
        if let Ok(output) = &output {
            state
                .task
                .execution_context
                .as_mut()
                .unwrap()
                .add_output(&call.id, output.clone());
            if let Some(question) = output::interaction_question(id, output) {
                state.task.step_results.insert(
                    call.id.clone(),
                    StepResult {
                        step_id: call.id.clone(),
                        success: true,
                        output: Some(output.clone()),
                        error: None,
                        duration_ms: duration,
                        retry_count: 0,
                    },
                );
                state.wait = Some(Wait::Interaction {
                    call: call.clone(),
                    step_id: call.id.clone(),
                });
                state.pending.pop_front();
                state.calls += 1;
                state.inflight = None;
                state.task.set_pending_question(question);
                return Ok(true);
            }
        }
        if let Err(error) = &output {
            emitter
                .step_failed(&call.id, state.calls as u32, duration, error)
                .await;
        }
        if effectful && output.is_err() && !executor::is_cancelled(&state.task.task_id).await {
            // Preserve the pending call and its in-flight identity for reconciliation.
            state.wait = Some(Wait::Recovery);
            state.task.set_pending_question(UserQuestion::free_text("The operation did not return a verified result. Reply to continue and check its outcome before taking further action.",&capability.name,true));
            return Ok(true);
        }
        state.inflight = None;
        finish_call(state, &pending, output, duration);
        Ok(false)
    }
}

fn request_evidence(request: &UserRequest, recipe: &Recipe, task: &TaskState) -> Value {
    let context = request.context.as_ref();
    json!({"page":recipe.page_context,"history":recipe.conversation_context,
        "memory":task.execution_context.as_ref().and_then(|c| c.memory_context.as_ref()),
        "route":context.and_then(|c|c.current_route.as_ref()),
        "platforms":context.map(|c| &c.active_platforms),
        "preferences":context.and_then(|c|c.preferences.as_ref()),
        "music":recipe.metadata.get("music_status"),"windows":recipe.metadata.get("window_state")})
}

/// Heartbeat cannot authorize High and above, nor capabilities that create or
/// trigger further automatic runs (`unattended_may_auto_run`). The call is
/// recorded as a tool error and the handler is not entered, so there is no
/// effect. Other Low / Medium calls return false and auto-run. Interactive
/// users are unchanged.
pub(super) fn reject_unattended_confirmation(
    state: &mut Checkpoint,
    pending: &PendingCall,
    capability_id: &str,
    risk: RiskLevel,
) -> bool {
    if state.user_id != super::SYSTEM_USER_ID {
        return false;
    }
    if super::executor_resolve_pure::unattended_may_auto_run(capability_id, risk) {
        return false;
    }
    finish_call(
        state,
        pending,
        Err("Unattended execution cannot authorize this operation".into()),
        0,
    );
    true
}

fn finish_call(
    state: &mut Checkpoint,
    pending: &PendingCall,
    result: Result<Value, String>,
    duration_ms: u64,
) {
    if let Ok(params) = serde_json::from_str::<Value>(&pending.call.arguments) {
        *state
            .call_counts
            .entry(operation_key(&pending.call.name, &params))
            .or_default() += 1;
    }
    let (output, error) = match result {
        Ok(output) => (Some(output), None),
        Err(error) => (None, Some(error)),
    };
    let response = output.clone().unwrap_or_else(|| json!({"error":error}));
    state.task.step_results.insert(
        pending.call.id.clone(),
        StepResult {
            step_id: pending.call.id.clone(),
            success: error.is_none(),
            output,
            error,
            duration_ms,
            retry_count: 0,
        },
    );
    state.tool_result(pending.call.clone(), &response);
    state.pending.pop_front();
    state.calls += 1;
    state.task.current_step = state.calls;
}

fn apply_answer(state: &mut Checkpoint, answer: &UserAnswer) -> Result<(), String> {
    state.validate_answer(answer)?;
    let wait = state.wait.take().ok_or("Missing Work continuation")?;
    match wait {
        Wait::Approval { fingerprint } => {
            if !answer.skipped && answer.answer == "yes" {
                state
                    .pending
                    .front_mut()
                    .ok_or("Missing pending tool")?
                    .approval = Some(fingerprint);
            } else {
                state.denied.insert(fingerprint);
                let pending = state.pending.pop_front().ok_or("Missing pending tool")?;
                state.calls += 1;
                state.tool_result(
                    pending.call,
                    &json!({"error":"The user declined this operation. Do not repeat it."}),
                );
                recipes::abort(
                    state,
                    "Saved recipe stopped because the user declined an operation",
                );
            }
        }
        Wait::Answer { call } => state.tool_result(
            call,
            &json!({"answer":answer.answer,"skipped":answer.skipped}),
        ),
        Wait::Interaction { call, step_id } => {
            let result: Value = serde_json::from_str(&answer.answer)
                .map_err(|_| "Expected a structured Tapp result")?;
            if let Some(step) = state.task.step_results.get_mut(&step_id) {
                step.output = Some(result.clone());
                step.success = !answer.skipped;
            }
            state
                .task
                .execution_context
                .as_mut()
                .unwrap()
                .add_output(&step_id, result.clone());
            state.tool_result(call, &result);
        }
        Wait::Recovery => {
            if let Some(id) = state.inflight.take() {
                if let Some(pending) = state.pending.pop_front_if(|p| p.call.id == id) {
                    state.calls += 1;
                    state.tool_result(pending.call,&json!({"error":"Tool outcome unknown after interruption. Do not repeat the action; inspect its target first.","user_observation":answer.answer}));
                }
            }
            // Resolve the rest of the old batch before adding a user turn.
            while let Some(pending) = state.pending.pop_front() {
                state.tool_result(
                    pending.call,
                    &json!({"cancelled":"Re-evaluate after recovery before executing this action"}),
                );
            }
            recipes::abort(
                state,
                "Saved recipe interrupted; inspect completed effects before starting another workflow",
            );
            state.history.push(ToolMessage::User {
                content: format!(
                    "Continue from the saved progress. User update: {}",
                    answer.answer
                ),
            });
        }
    }
    state.task.status = TaskStatus::Running;
    state.task.pending_question = None;
    state.lease_id = uuid::Uuid::new_v4().to_string();

    Ok(())
}

/// Public projection only; raw provider history never leaves the checkpoint.
fn checkpoint_response(state: Checkpoint) -> AgentResponse {
    // Preserve business execution order; provider call ids are not sortable.
    let outputs: Vec<_> = state
        .task
        .recipe
        .as_ref()
        .into_iter()
        .flat_map(|r| &r.steps)
        .filter_map(|step| {
            let result = state.task.step_results.get(&step.id)?;
            result
                .success
                .then_some(result.output.as_ref()?)
                .map(|output| (step, output))
        })
        .collect();
    let mut data = outputs
        .last()
        .map(|(step, output)| {
            if CapabilityRef::parse(&step.capability_id).is_mcp() {
                json!({"result":output})
            } else {
                (*output).clone()
            }
        })
        .unwrap_or(json!({}));
    if !data.is_object() {
        data = json!({"result":data});
    }
    data["workPlan"] = state.plan.clone();
    // Actions were already emitted on the stream; don't replay them at completion.
    for key in ["frontendAction", "frontendActions", "action"] {
        data.as_object_mut().unwrap().remove(key);
    }
    AgentResponse {
        response_type: if matches!(
            state.task.status,
            TaskStatus::Failed | TaskStatus::Cancelled
        ) {
            AgentResponseType::Error
        } else if state.task.status == TaskStatus::WaitingForInput {
            AgentResponseType::TaskCompleted
        } else {
            AgentResponseType::Answer
        },
        message: state.final_text,
        data: Some(data),
        data_display: None,
        suggestions: vec![],
        task: Some(state.task),
        frontend_action: None,
        performance: None,
    }
}

pub(crate) async fn saved_response(
    db: &sea_orm::DatabaseConnection,
    task_id: &str,
    user_id: i32,
) -> Result<AgentResponse, String> {
    Ok(checkpoint_response(
        store::load(db, task_id, user_id).await?,
    ))
}
