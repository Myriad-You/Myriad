// Agent process / recipe execution paths.

use sea_orm::DatabaseConnection;

use super::agent_footer::*;
use super::agent_header::*;
use super::motion_overlay::{round_motion_style, utterance_index_in_session};
use super::types::*;
use super::{capability, executor, planner, response_agent, types};

impl Agent {
    /// 创建新的 Agent 实例
    pub async fn new(db: DatabaseConnection) -> Self {
        Self {
            planner: planner::Planner::new().await,
            executor: executor::Executor::new(db.clone()).await,
            db,
        }
    }

    /// 处理用户请求
    ///
    /// 两层流程：
    /// 1. Planner 规划（Pro AI 单次调用）
    /// 2. 根据 PlannerOutput.status 分流
    /// 3. 执行 Recipe
    /// 4. 升级重试（如需要）
    ///
    /// 整个回合跑在一次 AI 配额预留里，见 [`AgentTurnBudget`]。
    pub async fn process(&self, request: UserRequest) -> Result<AgentResponse, String> {
        let user_id = request.user_id;
        let task_id = turn_task_id(&request);
        AgentTurnBudget::run(
            &self.db,
            user_id,
            "agent.process",
            task_id,
            Box::pin(self.process_inner(request)),
        )
        .await
    }

    async fn process_inner(&self, request: UserRequest) -> Result<AgentResponse, String> {
        let user_id = request.user_id;

        // 请求驱动的过期任务清理
        executor::maybe_cleanup_tasks().await;

        tracing::info!(
            user_id = user_id,
            input = %request.raw_input,
            "[Agent] Processing request"
        );

        crate::services::agent::consciousness::remember_live_presence(
            user_id,
            crate::services::agent::consciousness::live_presence_from_request(&request),
        );
        crate::services::agent::merope::mark_activity(&self.db, user_id, "thinking").await;
        let (mood_transition, memory_input_at) = crate::services::agent::merope::note_user_turn(
            &self.db,
            &request,
            utterance_index_in_session(&request),
        )
        .await
        .unzip();
        let mood_before = mood_transition.as_ref().map(|transition| transition.before);
        let round_motion_style = round_motion_style(&request, mood_transition.as_ref()).await;

        // Chat does not consume Pro and must not emit Recipe/tool calls.
        if request.context.as_ref().is_some_and(|context| {
            context.interaction_mode == crate::services::agent::AgentInteractionMode::Chat
        }) {
            return self
                .process_chat(
                    request,
                    mood_transition,
                    round_motion_style,
                    memory_input_at,
                )
                .await;
        }

        self.process_work(request, mood_transition, mood_before, round_motion_style)
            .await
    }

    /// 处理用户请求（带实时进度回调）
    ///
    /// 与 process 相同的两层逻辑，但会通过 channel 发送进度更新
    pub async fn process_with_progress(
        &self,
        request: UserRequest,
        progress_tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
    ) -> Result<AgentResponse, String> {
        let user_id = request.user_id;
        let task_id = turn_task_id(&request);
        AgentTurnBudget::run(
            &self.db,
            user_id,
            "agent.process_with_progress",
            task_id,
            Box::pin(self.process_with_progress_inner(request, progress_tx)),
        )
        .await
    }

    async fn process_with_progress_inner(
        &self,
        request: UserRequest,
        progress_tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
    ) -> Result<AgentResponse, String> {
        let user_id = request.user_id;

        tracing::info!(
            user_id = user_id,
            input = %request.raw_input,
            mode = request
                .context
                .as_ref()
                .map(|context| context.interaction_mode.as_str())
                .unwrap_or("work"),
            has_history = request.context.as_ref().and_then(|c| c.conversation_history.as_ref()).is_some(),
            "[Agent] Processing request with progress tracking"
        );

        crate::services::agent::consciousness::remember_live_presence(
            user_id,
            crate::services::agent::consciousness::live_presence_from_request(&request),
        );
        crate::services::agent::merope::mark_activity(&self.db, user_id, "thinking").await;
        let (mood_transition, memory_input_at) = crate::services::agent::merope::note_user_turn(
            &self.db,
            &request,
            utterance_index_in_session(&request),
        )
        .await
        .unzip();
        let mood_before = mood_transition.as_ref().map(|transition| transition.before);
        let round_motion_style = round_motion_style(&request, mood_transition.as_ref()).await;

        if let Some(mood) = mood_transition.clone() {
            let _ = progress_tx
                .send(AgentProgressEvent::MeropeStateChanged {
                    mood: mood.clone(),
                    activity: "thinking".to_string(),
                })
                .await;
        }

        // Chat 是独立的人设对话路径。在 Planner 之前分流才能保证：
        // 1. 不消耗 Pro；2. 即使输入像操作指令，也不会生成 Recipe/Tool Call。
        // Chat does not consume Pro and must not emit Recipe/tool calls.
        if request.context.as_ref().is_some_and(|context| {
            context.interaction_mode == crate::services::agent::AgentInteractionMode::Chat
        }) {
            return self
                .process_chat_with_progress(
                    request,
                    progress_tx,
                    mood_transition,
                    round_motion_style,
                    memory_input_at,
                )
                .await;
        }

        self.process_work_with_progress(
            request,
            progress_tx,
            mood_transition,
            mood_before,
            round_motion_style,
        )
        .await
    }

    /// 执行已保存的 Recipe（跳过意图分析）
    ///
    /// 用于从预设中直接执行任务，避免重复的意图解析。
    /// Work path: skip intent, never Chat.
    pub async fn execute_saved_recipe(
        &self,
        recipe: &Recipe,
        user_id: i32,
        progress_tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
    ) -> Result<AgentResponse, String> {
        AgentTurnBudget::run(
            &self.db,
            user_id,
            "agent.execute_saved_recipe",
            recipe.id.clone(),
            Box::pin(self.execute_saved_recipe_inner(recipe, user_id, progress_tx)),
        )
        .await
    }

    async fn execute_saved_recipe_inner(
        &self,
        recipe: &Recipe,
        user_id: i32,
        progress_tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
    ) -> Result<AgentResponse, String> {
        // 每次运行 mint 新 id，保证 TaskState.task_id 与 TaskCreated 唯一且可取消
        let mut recipe = recipe.clone();
        let template_id = recipe.id.clone();
        recipe.id = uuid::Uuid::new_v4().to_string();

        tracing::info!(
            user_id = user_id,
            recipe_id = %recipe.id,
            template_id = %template_id,
            recipe_name = %recipe.name,
            steps_count = recipe.steps.len(),
            "[Agent] Executing saved recipe directly"
        );

        Self::validate_saved_recipe(&recipe)?;
        let granted = crate::services::agent::get_user_permissions(&self.db, user_id).await;
        for step in &recipe.steps {
            crate::services::agent::planner::capability_allowed_for_grants(
                &step.capability_id,
                &step.params,
                &granted,
            )
            .await?;
        }

        if let Some(response) = self
            .mood_refuse_response(
                user_id,
                crate::services::agent::merope::maybe_refuse_new_task(&self.db, user_id).await,
                Some(&progress_tx),
            )
            .await
        {
            return Ok(response);
        }

        // Saved recipes are an execution shortcut, not a security shortcut.
        // Re-run the same sensitive-operation gate used by newly planned work.
        let sensitive_steps = self.check_sensitive_steps(&recipe).await;
        if !sensitive_steps.is_empty() {
            match Self::system_sensitive_gate(user_id, &sensitive_steps) {
                Some(Ok(())) => {}
                Some(Err(response)) => return Ok(response),
                None => {
                    let planner_output = Self::planner_output_for_saved_recipe(&recipe);
                    let session_id = recipe
                        .lane_key
                        .as_deref()
                        .and_then(session_id_from_lane_key);
                    // Saved recipes don't carry the original process run_id.
                    return self
                        .request_confirmation_v2(
                            &recipe,
                            &planner_output,
                            user_id,
                            sensitive_steps,
                            session_id,
                            None,
                            None,
                        )
                        .await;
                }
            }
        }

        // TaskCreated 在 mint 新 run id 后发送，保证与 TaskState.task_id 一致
        let step_descs: Vec<String> = recipe
            .steps
            .iter()
            .map(capability::get_step_description)
            .collect();
        let _ = progress_tx
            .send(AgentProgressEvent::TaskCreated {
                task_id: recipe.id.clone(),
                message: response_agent::executing_preset(&recipe.name),
                total_steps: recipe.steps.len() as u32,
                step_descriptions: step_descs,
            })
            .await;

        // 直接执行 recipe
        crate::services::agent::merope::mark_activity(&self.db, user_id, "working").await;
        let task_result = self
            .executor
            .execute_with_progress(&recipe, user_id, Some(progress_tx))
            .await;
        crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
        let task_state = task_result?;

        // 根据执行类型返回结果
        let mut result = self.extract_final_result(&task_state);
        let frontend_action = self.extract_frontend_action(&result);

        // 添加 recipe 到结果
        if let Some(obj) = result.as_object_mut() {
            obj.insert(
                "recipe".to_string(),
                serde_json::to_value(&recipe).unwrap_or_default(),
            );
        }

        // 为已保存的 recipe 生成消息（委托 response_agent）
        let message = match task_state.status {
            types::TaskStatus::Completed => response_agent::recipe_completed(&recipe.name),
            types::TaskStatus::Failed => response_agent::recipe_failed(
                &recipe.name,
                &task_state.error.clone().unwrap_or_default(),
            ),
            _ => response_agent::in_progress(&recipe.name),
        };

        // v3 记忆记录（saved recipe 执行也需要记录）
        {
            let ok = task_state.status == types::TaskStatus::Completed;
            record_execution_memory(MemoryRecordParams {
                user_id,
                user_input: &recipe.name,
                recipe: &recipe,
                planner_steps_len: recipe.steps.len(),
                success: ok,
                error_msg: task_state.error.as_deref(),
                log_prefix: "saved:",
                conversation_context: None,
                step_results: Some(&task_state.step_results),
            })
            .await;
        }

        Ok(AgentResponse {
            response_type: AgentResponseType::Answer,
            message,
            data: Some(result),
            data_display: None,
            suggestions: vec![],
            task: Some(task_state),
            confirmation: None,
            frontend_action,
            performance: None,
        })
    }

    pub(crate) fn planner_output_for_saved_recipe(recipe: &Recipe) -> PlannerOutput {
        PlannerOutput {
            status: PlannerStatus::Plan,
            confidence: 1.0,
            reasoning: Some("Saved recipe execution".to_string()),
            steps: recipe
                .steps
                .iter()
                .map(|step| AiRecipeStep {
                    id: step.id.clone(),
                    capability_id: step.capability_id.clone(),
                    action: step.action.clone(),
                    params: step.params.clone(),
                    depends_on: step.depends_on.clone(),
                    on_failure: match step.on_failure {
                        FailureStrategy::Skip => "skip".to_string(),
                        _ => "abort".to_string(),
                    },
                    retry: step.retry.clone(),
                    timeout_ms: step.timeout_ms,
                })
                .collect(),
            clarification: None,
            unsupported_reason: None,
            chat_reply: None,
        }
    }

    pub(crate) fn validate_saved_recipe(recipe: &Recipe) -> Result<(), String> {
        use std::collections::{HashMap, HashSet, VecDeque};

        if recipe.steps.is_empty() {
            return Err("Saved recipe contains no steps".to_string());
        }
        if recipe.steps.len() > 32 {
            return Err("Saved recipe exceeds the 32-step limit".to_string());
        }

        let ids: HashSet<&str> = recipe.steps.iter().map(|step| step.id.as_str()).collect();
        if ids.len() != recipe.steps.len() || ids.contains("") {
            return Err("Saved recipe contains empty or duplicate step IDs".to_string());
        }

        let mut indegree: HashMap<&str, usize> = ids.iter().map(|id| (*id, 0)).collect();
        let mut dependants: HashMap<&str, Vec<&str>> = HashMap::new();
        for step in &recipe.steps {
            for dependency in &step.depends_on {
                if !ids.contains(dependency.as_str()) {
                    return Err(format!(
                        "Saved recipe step '{}' references unknown dependency '{}'",
                        step.id, dependency
                    ));
                }
                if dependency == &step.id {
                    return Err(format!("Saved recipe step '{}' depends on itself", step.id));
                }
                *indegree.entry(step.id.as_str()).or_default() += 1;
                dependants
                    .entry(dependency.as_str())
                    .or_default()
                    .push(step.id.as_str());
            }
        }

        let mut queue: VecDeque<&str> = indegree
            .iter()
            .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
            .collect();
        let mut visited = 0;
        while let Some(id) = queue.pop_front() {
            visited += 1;
            for dependant in dependants.get(id).into_iter().flatten() {
                let degree = indegree
                    .get_mut(dependant)
                    .expect("validated dependant must exist");
                *degree -= 1;
                if *degree == 0 {
                    queue.push_back(dependant);
                }
            }
        }
        if visited != recipe.steps.len() {
            return Err("Saved recipe contains a dependency cycle".to_string());
        }

        Ok(())
    }

    /// Peek confirmation resume context without consuming the pending entry.
    pub async fn confirmation_resume_context(
        &self,
        confirmation_id: &str,
        user_id: i32,
    ) -> Result<Option<ConfirmationResumeContext>, String> {
        let pending = crate::services::tapp_registry::get::<PendingRecipeConfirmation>(
            &self.db,
            CONFIRMATION_REGISTRY_NAMESPACE,
            confirmation_id,
        )
        .await
        .map_err(|error| {
            tracing::error!(%error, "Failed to load confirmation");
            "Failed to load confirmation".to_string()
        })?;
        Ok(pending
            .filter(|pending| pending.user_id == user_id)
            .map(|pending| ConfirmationResumeContext {
                lane_key: pending.recipe.lane_key.clone(),
                session_id: pending.session_id.clone().or_else(|| {
                    // Older confirmations may only have session embedded in lane_key.
                    pending
                        .recipe
                        .lane_key
                        .as_deref()
                        .and_then(session_id_from_lane_key)
                }),
                run_id: pending.run_id.clone(),
                source_intent_id: pending.source_intent_id.clone(),
            }))
    }
}

/// 本回合在配额 / 成本账里的 task id。
///
/// 优先用客户端的 run 或 session id，让账目能 join 回具体对话；两者都缺时退回
/// 请求时间戳。
fn turn_task_id(request: &UserRequest) -> String {
    request
        .context
        .as_ref()
        .and_then(|c| c.run_id.clone().or_else(|| c.session_id.clone()))
        .unwrap_or_else(|| format!("turn_{}", request.timestamp.timestamp_millis()))
}

#[cfg(test)]
mod split_path_contract_tests {
    /// Chat must still branch before Work/Planner on the shipped dispatcher.
    #[test]
    fn chat_mode_still_branches_before_process_work() {
        let src = include_str!("process_and_recipe.rs");
        let inner = src
            .split("async fn process_inner")
            .nth(1)
            .expect("process_inner");
        let chat = inner
            .find("AgentInteractionMode::Chat")
            .expect("Chat branch");
        let work = inner.find("process_work(").expect("Work call");
        assert!(chat < work, "Chat must run before process_work");
        assert!(
            !inner[..work].contains("planner"),
            "Planner must not run before the Chat/Work split"
        );
    }

    #[test]
    fn execute_saved_recipe_stays_on_work_path() {
        let src = include_str!("process_and_recipe.rs");
        let inner = src
            .split("async fn execute_saved_recipe_inner")
            .nth(1)
            .and_then(|rest| rest.split("fn turn_task_id").next())
            .expect("execute_saved_recipe_inner body");
        assert!(
            !inner.contains("process_chat"),
            "saved recipe must not enter Chat"
        );
        assert!(
            !inner.contains("AgentInteractionMode::Chat"),
            "saved recipe must not branch on Chat mode"
        );
    }
}
