// Agent process / recipe execution paths.

use sea_orm::DatabaseConnection;
use serde_json::{json, Value};

use super::agent_footer::*;
use super::agent_header::*;
use super::types::*;
use super::{
    capability, escalation, executor, orchestrator, planner, recipe, response_agent,
    skill_evolution, types,
};

fn utterance_index_in_session(request: &UserRequest) -> u32 {
    request
        .context
        .as_ref()
        .and_then(|ctx| ctx.conversation_history.as_ref())
        .map(|history| {
            history
                .iter()
                .filter(|message| message.role == "user")
                .count()
                .saturating_sub(1) as u32
        })
        .unwrap_or(0)
}

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

        crate::services::agent::merope::mark_activity(&self.db, user_id, "talking").await;
        let mood_transition = crate::services::agent::merope::note_user_turn(
            &self.db,
            user_id,
            &request.raw_input,
            utterance_index_in_session(&request),
        )
        .await;
        let mood_before = mood_transition.as_ref().map(|transition| transition.before);
        if let Some(mood) = mood_transition.clone() {
            spawn_motion_directive(
                crate::services::agent::merope::MotionContext {
                    user_id,
                    phase: crate::services::agent::merope::MotionPhase::Reaction,
                    mood,
                    activity: "talking".to_string(),
                    user_text: request.raw_input.clone(),
                    response_text: None,
                    task_success: None,
                },
                None,
            );
        }

        // 1. Planner 规划
        let planner_output = match self.planner.plan(&request).await {
            Ok(output) => output,
            Err(error) => {
                crate::services::agent::merope::note_chat_diary(
                    &self.db,
                    user_id,
                    &request.raw_input,
                )
                .await;
                crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                return Err(error);
            }
        };
        crate::services::agent::merope::note_chat_diary(&self.db, user_id, &request.raw_input).await;

        tracing::debug!(
            status = ?planner_output.status,
            confidence = planner_output.confidence,
            steps = planner_output.steps.len(),
            "[Agent] Planner output"
        );

        // 2. 根据状态分流
        match planner_output.status {
            PlannerStatus::Chat => {
                crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                let response = AgentResponse {
                    response_type: AgentResponseType::Answer,
                    message: planner_output
                        .chat_reply
                        .unwrap_or_else(response_agent::greeting),
                    data: Some(json!({ "type": "chat" })),
                    data_display: None,
                    suggestions: vec![],
                    task: None,
                    confirmation: None,
                    frontend_action: None,
                    performance: None,
                };
                return attach_motion_to_result(
                    Ok(response),
                    user_id,
                    &request.raw_input,
                    mood_transition.clone(),
                    None,
                )
                .await;
            }
            PlannerStatus::Clarify => {
                let clarification = planner_output
                    .clarification
                    .unwrap_or(PlannerClarification {
                        message: response_agent::need_clarification(),
                        options: vec![],
                    });
                crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                let response = AgentResponse {
                    response_type: AgentResponseType::Clarification,
                    message: clarification.message.clone(),
                    data: Some(json!({
                        "confidence": planner_output.confidence,
                        "clarification": {
                            "message": clarification.message,
                            "options": clarification.options
                        }
                    })),
                    data_display: None,
                    suggestions: clarification.options,
                    task: None,
                    confirmation: None,
                    frontend_action: None,
                    performance: None,
                };
                return attach_motion_to_result(
                    Ok(response),
                    user_id,
                    &request.raw_input,
                    mood_transition.clone(),
                    None,
                )
                .await;
            }
            PlannerStatus::Unsupported => {
                let reason = planner_output
                    .unsupported_reason
                    .unwrap_or_else(response_agent::unsupported_operation);

                // 记录能力缺口
                if let Some(evo) = skill_evolution::get_skill_evolution() {
                    evo.detect_capability_gap(&request.raw_input, &reason).await;
                }

                crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                let response = AgentResponse {
                    response_type: AgentResponseType::Answer,
                    message: reason.clone(),
                    data: Some(json!({
                        "unsupported": true,
                        "reason": reason,
                    })),
                    data_display: None,
                    suggestions: response_agent::default_suggestions(),
                    task: None,
                    confirmation: None,
                    frontend_action: None,
                    performance: None,
                };
                return attach_motion_to_result(
                    Ok(response),
                    user_id,
                    &request.raw_input,
                    mood_transition.clone(),
                    None,
                )
                .await;
            }
            PlannerStatus::Plan => {
                if let Some(response) = self
                    .mood_refuse_response(
                        user_id,
                        crate::services::agent::merope::refuse_new_task_message(mood_before),
                        None,
                    )
                    .await
                {
                    return attach_motion_to_result(
                        Ok(response),
                        user_id,
                        &request.raw_input,
                        mood_transition.clone(),
                        None,
                    )
                    .await;
                }
                // 低置信度时在 process() 中也记录警告
                if planner_output.confidence < 0.3 && planner_output.confidence > 0.0 {
                    tracing::warn!(
                        confidence = planner_output.confidence,
                        "[Agent] Low planner confidence in process()"
                    );
                }
            }
        }

        // 3. 转换步骤为 Recipe
        let cap_ids: Vec<String> = planner_output
            .steps
            .iter()
            .map(|s| s.capability_id.clone())
            .collect();
        let cap_schemas = capability::get_capabilities_by_ids(&cap_ids).await;
        let recipe_steps = match recipe::validate_and_convert_steps(
            planner_output.steps.clone(),
            planner_output.reasoning.clone(),
            &cap_schemas,
        ) {
            Ok(steps) => steps,
            Err(error) => {
                crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                return Err(error);
            }
        };

        let recipe = Self::build_recipe_from_steps(
            recipe_steps,
            planner_output
                .reasoning
                .clone()
                .unwrap_or_else(|| request.raw_input.clone()),
            &request,
        );

        // 4. 检查敏感操作（系统任务自动确认，Critical 除外）
        let sensitive_steps = self.check_sensitive_steps(&recipe).await;
        if !sensitive_steps.is_empty() {
            match Self::system_sensitive_gate(user_id, &sensitive_steps) {
                Some(Ok(())) => {} // 系统任务已自动确认，继续执行
                Some(Err(blocked)) => {
                    crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                    return attach_motion_to_result(
                        Ok(blocked),
                        user_id,
                        &request.raw_input,
                        mood_transition.clone(),
                        None,
                    )
                    .await;
                }
                None => {
                    let session_id = request.context.as_ref().and_then(|c| c.session_id.clone());
                    let run_id = request.context.as_ref().and_then(|c| c.run_id.clone());
                    crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                    let response = self
                        .request_confirmation_v2(
                            &recipe,
                            &planner_output,
                            user_id,
                            sensitive_steps,
                            session_id,
                            run_id,
                        )
                        .await;
                    return attach_motion_to_result(
                        response,
                        user_id,
                        &request.raw_input,
                        mood_transition.clone(),
                        None,
                    )
                    .await;
                }
            }
        }

        // 4.5 检查必需参数缺失
        match self
            .check_missing_required_parameters(&recipe, &planner_output, user_id, None)
            .await
        {
            Ok(Some(missing_response)) => {
                crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                return attach_motion_to_result(
                    Ok(missing_response),
                    user_id,
                    &request.raw_input,
                    mood_transition.clone(),
                    None,
                )
                .await;
            }
            Ok(None) => {}
            Err(error) => {
                crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                return Err(error);
            }
        }

        // 5. 执行方案
        crate::services::agent::merope::mark_activity(&self.db, user_id, "working").await;
        let task_result = self.executor.execute(&recipe, user_id).await;
        crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
        let task_state = task_result?;
        let result = self.extract_final_result(&task_state);

        // 6. v3 记忆提取 + 日志
        let ok = task_state.status == TaskStatus::Completed;
        record_execution_memory(MemoryRecordParams {
            user_id,
            user_input: &request.raw_input,
            recipe: &recipe,
            planner_steps_len: planner_output.steps.len(),
            success: ok,
            error_msg: task_state.error.as_deref(),
            log_prefix: "",
            conversation_context: request
                .context
                .as_ref()
                .and_then(|c| c.conversation_history.as_deref()),
            step_results: Some(&task_state.step_results),
        })
        .await;

        // Skill 自动创建（成功的多步骤 Recipe → 泛化 Skill）
        if ok && recipe.steps.len() >= 2 {
            if let Some(evolution) = skill_evolution::get_skill_evolution() {
                let evo = evolution.clone();
                let request_text = request.raw_input.clone();
                let step_caps: Vec<String> = recipe
                    .steps
                    .iter()
                    .map(|s| s.capability_id.clone())
                    .collect();
                let step_descriptions: String = recipe
                    .steps
                    .iter()
                    .enumerate()
                    .map(|(i, s)| format!("{}. {} ({})", i + 1, s.action, s.capability_id))
                    .collect::<Vec<_>>()
                    .join("\n");
                crate::services::ai_cost_ledger::spawn_with_current_ai_attribution(
                    move || async move {
                        match evo
                            .auto_create_skill_abstracted(
                                &request_text,
                                &step_descriptions,
                                &step_caps,
                            )
                            .await
                        {
                            Ok(skill) => {
                                tracing::info!(skill_id = %skill.id, "[Agent] Auto-created skill from recipe")
                            }
                            Err(e) => {
                                tracing::debug!(error = %e, "[Agent] Skill auto-creation skipped")
                            }
                        }
                    },
                );
            }
        }

        // 7. 构建响应
        let frontend_action = self.extract_frontend_action(&result);
        let data_display = self.infer_data_display_v2(&result, &planner_output);

        let response = AgentResponse {
            response_type: AgentResponseType::Answer,
            message: self
                .generate_response_message_v2(&planner_output, &task_state, user_id, None)
                .await,
            data: Some(result),
            data_display,
            suggestions: vec![],
            task: Some(task_state),
            confirmation: None,
            frontend_action,
            performance: None,
        };
        attach_motion_to_result(
            Ok(response),
            user_id,
            &request.raw_input,
            mood_transition,
            None,
        )
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
            has_history = request.context.as_ref().and_then(|c| c.conversation_history.as_ref()).is_some(),
            "[Agent] Processing request with progress tracking"
        );

        crate::services::agent::merope::mark_activity(&self.db, user_id, "talking").await;
        let mood_transition = crate::services::agent::merope::note_user_turn(
            &self.db,
            user_id,
            &request.raw_input,
            utterance_index_in_session(&request),
        )
        .await;
        let mood_before = mood_transition.as_ref().map(|transition| transition.before);

        if let Some(mood) = mood_transition.clone() {
            let _ = progress_tx
                .send(AgentProgressEvent::MeropeStateChanged {
                    mood: mood.clone(),
                    activity: "talking".to_string(),
                })
                .await;
            spawn_motion_directive(
                crate::services::agent::merope::MotionContext {
                    user_id,
                    phase: crate::services::agent::merope::MotionPhase::Reaction,
                    mood,
                    activity: "talking".to_string(),
                    user_text: request.raw_input.clone(),
                    response_text: None,
                    task_success: None,
                },
                Some(progress_tx.clone()),
            );
        }

        // 1. Planner 规划（Pro AI 单次调用）
        let _ = progress_tx
            .send(AgentProgressEvent::Progress {
                progress: 5,
                completed_steps: 0,
                total_steps: 0,
                message: response_agent::understanding_request(),
            })
            .await;

        let planner_output = match self.planner.plan(&request).await {
            Ok(output) => output,
            Err(error) => {
                crate::services::agent::merope::note_chat_diary(
                    &self.db,
                    user_id,
                    &request.raw_input,
                )
                .await;
                crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                return Err(error);
            }
        };
        crate::services::agent::merope::note_chat_diary(&self.db, user_id, &request.raw_input).await;

        tracing::debug!(
            status = ?planner_output.status,
            confidence = planner_output.confidence,
            steps = planner_output.steps.len(),
            "[Agent] Planner output"
        );

        // 发送 Planner 决策调试事件
        let planner_step_summaries: Vec<types::PlannerStepSummary> = planner_output
            .steps
            .iter()
            .map(|s| types::PlannerStepSummary {
                id: s.id.clone(),
                capability_id: s.capability_id.clone(),
                action: s.action.clone(),
                params: serde_json::to_value(&s.params).ok(),
            })
            .collect();
        let _planner_decision_info = types::PlannerDecisionInfo {
            status: format!("{:?}", planner_output.status),
            reasoning: planner_output.reasoning.clone(),
            confidence: planner_output.confidence,
            planned_steps: planner_step_summaries.clone(),
        };
        let _ = progress_tx
            .send(AgentProgressEvent::PlannerDecision {
                status: format!("{:?}", planner_output.status),
                reasoning: planner_output.reasoning.clone(),
                confidence: planner_output.confidence,
                steps: planner_step_summaries,
                user_request: request.raw_input.clone(),
            })
            .await;

        // Planner 决策完成 → 并行 spawn 子任务生成会话标题
        // 仅在有 session_id 且 session 尚无标题时触发
        if let Some(ref ctx) = request.context {
            if let Some(ref session_id) = ctx.session_id {
                let title_session_id = session_id.clone();
                let title_db = self.executor.db.clone();
                let title_tx = progress_tx.clone();
                let title_input = request.raw_input.clone();
                let title_reasoning = planner_output.reasoning.clone();
                tokio::spawn(async move {
                    use crate::models::entities::agent_sessions;
                    use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait};

                    // 检查 session 是否已有标题（续对话不需要重新生成）
                    if let Ok(Some(session)) = agent_sessions::Entity::find_by_id(&title_session_id)
                        .one(&title_db)
                        .await
                    {
                        if session.title.is_some() {
                            return; // 已有标题，跳过
                        }
                    }

                    let title =
                        generate_session_title_ai(&title_input, title_reasoning.as_deref()).await;

                    // 持久化到数据库
                    if let Ok(Some(session)) = agent_sessions::Entity::find_by_id(&title_session_id)
                        .one(&title_db)
                        .await
                    {
                        let mut active: agent_sessions::ActiveModel = session.into();
                        active.title = ActiveValue::Set(Some(title.clone()));
                        let _ = active.update(&title_db).await;
                    }

                    let _ = title_tx
                        .send(AgentProgressEvent::SessionTitleUpdated { title })
                        .await;
                });
            }
        }

        // 2. 根据状态分流
        match planner_output.status {
            PlannerStatus::Chat => {
                let planner_reply = planner_output
                    .chat_reply
                    .unwrap_or_else(response_agent::greeting);

                let delivery = mood_transition.clone().map(|mood| {
                    spawn_motion_directive(
                        crate::services::agent::merope::MotionContext {
                            user_id,
                            phase: crate::services::agent::merope::MotionPhase::Delivery,
                            mood,
                            activity: "talking".to_string(),
                            user_text: request.raw_input.clone(),
                            response_text: Some(planner_reply.clone()),
                            task_success: None,
                        },
                        Some(progress_tx.clone()),
                    )
                });

                // 尝试真正的流式 AI 回复（token-by-token from model）
                let reply = self
                    .stream_chat_response(&request, &planner_reply, &progress_tx)
                    .await;
                let performance = match delivery {
                    Some(handle) => handle.await.ok().flatten(),
                    None => None,
                };
                crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;

                let _ = progress_tx
                    .send(AgentProgressEvent::Progress {
                        progress: 100,
                        completed_steps: 1,
                        total_steps: 1,
                        message: response_agent::done_status(),
                    })
                    .await;
                return Ok(AgentResponse {
                    response_type: AgentResponseType::Answer,
                    message: reply.clone(),
                    data: Some(json!({ "reply": reply, "type": "chat" })),
                    data_display: None,
                    suggestions: vec![],
                    task: None,
                    confirmation: None,
                    frontend_action: None,
                    performance,
                });
            }
            PlannerStatus::Clarify => {
                let clarification = planner_output
                    .clarification
                    .unwrap_or(PlannerClarification {
                        message: response_agent::need_clarification(),
                        options: vec![],
                    });

                // 流式推送澄清消息
                Self::stream_text_as_tokens(&progress_tx, &clarification.message).await;
                crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                let response = AgentResponse {
                    response_type: AgentResponseType::Clarification,
                    message: clarification.message.clone(),
                    data: Some(json!({
                        "confidence": planner_output.confidence,
                        "clarification": {
                            "message": clarification.message,
                            "options": clarification.options
                        }
                    })),
                    data_display: None,
                    suggestions: clarification.options,
                    task: None,
                    confirmation: None,
                    frontend_action: None,
                    performance: None,
                };
                return attach_motion_to_result(
                    Ok(response),
                    user_id,
                    &request.raw_input,
                    mood_transition.clone(),
                    Some(progress_tx.clone()),
                )
                .await;
            }
            PlannerStatus::Unsupported => {
                let reason = planner_output
                    .unsupported_reason
                    .unwrap_or_else(response_agent::unsupported_operation);

                // 记录能力缺口
                if let Some(evo) = skill_evolution::get_skill_evolution() {
                    evo.detect_capability_gap(&request.raw_input, &reason).await;
                }

                crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                let response = AgentResponse {
                    response_type: AgentResponseType::Answer,
                    message: reason.clone(),
                    data: Some(json!({
                        "unsupported": true,
                        "reason": reason,
                    })),
                    data_display: None,
                    suggestions: response_agent::default_suggestions(),
                    task: None,
                    confirmation: None,
                    frontend_action: None,
                    performance: None,
                };
                return attach_motion_to_result(
                    Ok(response),
                    user_id,
                    &request.raw_input,
                    mood_transition.clone(),
                    Some(progress_tx.clone()),
                )
                .await;
            }
            PlannerStatus::Plan => {
                if let Some(response) = self
                    .mood_refuse_response(
                        user_id,
                        crate::services::agent::merope::refuse_new_task_message(mood_before),
                        Some(&progress_tx),
                    )
                    .await
                {
                    return attach_motion_to_result(
                        Ok(response),
                        user_id,
                        &request.raw_input,
                        mood_transition.clone(),
                        Some(progress_tx.clone()),
                    )
                    .await;
                }
                // 低置信度：降级为澄清请求，避免盲目执行
                if planner_output.confidence < 0.3 && planner_output.confidence > 0.0 {
                    tracing::warn!(
                        confidence = planner_output.confidence,
                        "[Agent] Very low planner confidence, requesting clarification"
                    );
                    let msg = format!(
                        "我对这个请求的理解置信度较低（{:.0}%），可能会误解你的意图。{}能再详细描述一下你想要做什么吗？",
                        planner_output.confidence * 100.0,
                        planner_output.reasoning.as_deref().map(|r| format!("我的理解是：{}。", r)).unwrap_or_default()
                    );
                    Self::stream_text_as_tokens(&progress_tx, &msg).await;
                    crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                    let response = AgentResponse {
                        response_type: AgentResponseType::Clarification,
                        message: msg.clone(),
                        data: Some(json!({
                            "confidence": planner_output.confidence,
                            "clarification": { "message": msg, "options": [] }
                        })),
                        data_display: None,
                        suggestions: vec![],
                        task: None,
                        confirmation: None,
                        frontend_action: None,
                        performance: None,
                    };
                    return attach_motion_to_result(
                        Ok(response),
                        user_id,
                        &request.raw_input,
                        mood_transition.clone(),
                        Some(progress_tx.clone()),
                    )
                    .await;
                }
            }
        }

        // 3. 转换步骤为 Recipe
        let _ = progress_tx
            .send(AgentProgressEvent::Progress {
                progress: 15,
                completed_steps: 0,
                total_steps: 0,
                message: response_agent::planning_steps(),
            })
            .await;

        let cap_ids: Vec<String> = planner_output
            .steps
            .iter()
            .map(|s| s.capability_id.clone())
            .collect();
        let cap_schemas = capability::get_capabilities_by_ids(&cap_ids).await;
        let recipe_steps = match recipe::validate_and_convert_steps(
            planner_output.steps.clone(),
            planner_output.reasoning.clone(),
            &cap_schemas,
        ) {
            Ok(steps) => steps,
            Err(error) => {
                crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                return Err(error);
            }
        };

        let mut recipe = Self::build_recipe_from_steps(
            recipe_steps,
            planner_output
                .reasoning
                .clone()
                .unwrap_or_else(|| request.raw_input.clone()),
            &request,
        );

        // 3.5 多 Agent 协作分析（Orchestrator）
        let (assignment, role_group_count, can_parallelize) =
            orchestrator::Orchestrator::analyze_recipe(&recipe);

        // 获取角色身份上下文并注入 Recipe metadata
        let role_contexts = orchestrator::Orchestrator::get_role_contexts(&recipe).await;
        if !role_contexts.is_empty() {
            let ctx_map: serde_json::Map<String, Value> = role_contexts
                .iter()
                .map(|(role, ctx)| (format!("{:?}", role), Value::String(ctx.clone())))
                .collect();
            recipe
                .metadata
                .insert("role_contexts".to_string(), Value::Object(ctx_map));
        }

        if assignment.is_multi_agent {
            tracing::info!(
                agents = assignment.total_agents,
                tier_mix = %assignment.tier_mix,
                parallel = can_parallelize,
                "[Agent] Multi-agent collaboration: {} agents, {} role groups",
                assignment.total_agents,
                role_group_count
            );
            let _ = progress_tx
                .send(AgentProgressEvent::TaskAssigned {
                    task_id: recipe.id.clone(),
                    assignment: Box::new(assignment.clone()),
                })
                .await;
        }

        // 4. 检查敏感操作（单步/多步共用：依赖 capability 元数据 requires_confirmation/risk，
        // 不能只靠 capability_id 字符串启发式，否则 tapp.interact / page.interact / MCP 会直通）
        // gates run before fast path so sensitive/missing-param never bypass.
        // carry session_id so confirm resume stays on the same conversation.
        let sensitive_steps = self.check_sensitive_steps(&recipe).await;
        if !sensitive_steps.is_empty() {
            match Self::system_sensitive_gate(user_id, &sensitive_steps) {
                Some(Ok(())) => {} // 系统任务已自动确认，继续执行
                Some(Err(blocked)) => {
                    crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                    return attach_motion_to_result(
                        Ok(blocked),
                        user_id,
                        &request.raw_input,
                        mood_transition.clone(),
                        Some(progress_tx.clone()),
                    )
                    .await;
                }
                None => {
                    let session_id = request.context.as_ref().and_then(|c| c.session_id.clone());
                    let run_id = request.context.as_ref().and_then(|c| c.run_id.clone());
                    crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                    let response = self
                        .request_confirmation_v2(
                            &recipe,
                            &planner_output,
                            user_id,
                            sensitive_steps,
                            session_id,
                            run_id,
                        )
                        .await;
                    return attach_motion_to_result(
                        response,
                        user_id,
                        &request.raw_input,
                        mood_transition.clone(),
                        Some(progress_tx.clone()),
                    )
                    .await;
                }
            }
        }

        // 4.5 检查必需参数缺失 — 执行前收集用户信息（单步/多步共用）
        match self
            .check_missing_required_parameters(
                &recipe,
                &planner_output,
                user_id,
                Some(&progress_tx),
            )
            .await
        {
            Ok(Some(missing_response)) => {
                crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                return attach_motion_to_result(
                    Ok(missing_response),
                    user_id,
                    &request.raw_input,
                    mood_transition.clone(),
                    Some(progress_tx.clone()),
                )
                .await;
            }
            Ok(None) => {}
            Err(error) => {
                crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                return Err(error);
            }
        }

        // 快速路径：仅安全的单步且参数齐全时走
        if recipe.steps.len() == 1 {
            tracing::debug!(
                recipe_id = %recipe.id,
                "[Agent] Using fast path for simple query"
            );
            let result = self
                .execute_simple_query_v2(
                    &recipe,
                    &planner_output,
                    user_id,
                    progress_tx.clone(),
                    &request.raw_input,
                    request
                        .context
                        .as_ref()
                        .and_then(|c| c.conversation_history.as_deref()),
                )
                .await;
            return attach_motion_to_result(
                result,
                user_id,
                &request.raw_input,
                mood_transition.clone(),
                Some(progress_tx.clone()),
            )
            .await;
        }
        // 快速路径结束

        // 发送任务创建事件（多步骤任务，附带步骤描述供前端展示执行计划）
        // task_id 必须等于 TaskState.task_id（= recipe.id），前端用此 id 做 cancel/steer
        let step_descs: Vec<String> = recipe
            .steps
            .iter()
            .map(capability::get_step_description)
            .collect();
        let _ = progress_tx
            .send(AgentProgressEvent::TaskCreated {
                task_id: recipe.id.clone(),
                message: String::new(),
                total_steps: recipe.steps.len() as u32,
                step_descriptions: step_descs.clone(),
            })
            .await;

        // 副 Agent 生成计划说明（AI 流式推送，告诉用户即将做什么）
        let _plan_msg =
            response_agent::announce_plan(&request.raw_input, &step_descs, user_id, &progress_tx)
                .await;

        // 5. 执行方案（带进度回调和升级）
        let result = self
            .execute_recipe_with_progress_v2(
                &recipe,
                &planner_output,
                &request,
                user_id,
                progress_tx.clone(),
            )
            .await;

        // 5.7 Skill 自动创建（AI 抽象化版：成功的多步骤 Recipe → 泛化 Skill）
        if result.is_ok() && recipe.steps.len() >= 2 {
            if let Some(evolution) = skill_evolution::get_skill_evolution() {
                let evo = evolution.clone();
                let request_text = request.raw_input.clone();
                let step_caps: Vec<String> = recipe
                    .steps
                    .iter()
                    .map(|s| s.capability_id.clone())
                    .collect();
                let step_descriptions: String = recipe
                    .steps
                    .iter()
                    .enumerate()
                    .map(|(i, s)| format!("{}. {} ({})", i + 1, s.action, s.capability_id))
                    .collect::<Vec<_>>()
                    .join("\n");

                crate::services::ai_cost_ledger::spawn_with_current_ai_attribution(
                    move || async move {
                        match evo
                            .auto_create_skill_abstracted(
                                &request_text,
                                &step_descriptions,
                                &step_caps,
                            )
                            .await
                        {
                            Ok(skill) => {
                                tracing::info!(skill_id = %skill.id, "[Agent] AI-abstracted skill created from recipe")
                            }
                            Err(e) => {
                                tracing::debug!(error = %e, "[Agent] Skill auto-creation skipped")
                            }
                        }
                    },
                );
            }
        }

        // 6. AI 驱动的记忆提取 + 会话记忆归档
        {
            let ok = result.is_ok();
            let step_results_ref = result
                .as_ref()
                .ok()
                .and_then(|r| r.task.as_ref())
                .map(|t| &t.step_results);
            record_execution_memory(MemoryRecordParams {
                user_id,
                user_input: &request.raw_input,
                recipe: &recipe,
                planner_steps_len: planner_output.steps.len(),
                success: ok,
                error_msg: result.as_ref().err().map(|e| e.as_str()),
                log_prefix: "",
                conversation_context: request
                    .context
                    .as_ref()
                    .and_then(|c| c.conversation_history.as_deref()),
                step_results: step_results_ref,
            })
            .await;
        }

        attach_motion_to_result(
            result,
            user_id,
            &request.raw_input,
            mood_transition,
            Some(progress_tx),
        )
        .await
    }

    /// 执行配方（带进度回调和升级）— 使用 Planner
    pub(crate) async fn execute_recipe_with_progress_v2(
        &self,
        recipe: &Recipe,
        planner_output: &PlannerOutput,
        original_request: &UserRequest,
        user_id: i32,
        progress_tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
    ) -> Result<AgentResponse, String> {
        // 执行 Recipe
        crate::services::agent::merope::mark_activity(&self.db, user_id, "working").await;
        let task_result = self
            .executor
            .execute_with_progress(recipe, user_id, Some(progress_tx.clone()))
            .await;
        crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
        let mut task_state = task_result?;

        // 注入 Planner 决策到 ExecutionTrace
        if let Some(ref mut trace) = task_state.execution_trace {
            trace.planner_decision = Some(types::PlannerDecisionInfo {
                status: format!("{:?}", planner_output.status),
                reasoning: planner_output.reasoning.clone(),
                confidence: planner_output.confidence,
                planned_steps: planner_output
                    .steps
                    .iter()
                    .map(|s| types::PlannerStepSummary {
                        id: s.id.clone(),
                        capability_id: s.capability_id.clone(),
                        action: s.action.clone(),
                        params: serde_json::to_value(&s.params).ok(),
                    })
                    .collect(),
            });
        }

        // 提取结果
        let mut result = self.extract_final_result(&task_state);

        // WaitingForInput 时直接返回，不进行升级评估（结果不完整是正常的）
        if task_state.status == TaskStatus::WaitingForInput {
            if let Some(obj) = result.as_object_mut() {
                obj.insert(
                    "recipe".to_string(),
                    serde_json::to_value(recipe).unwrap_or_default(),
                );
            }
            let frontend_action = self.extract_frontend_action(&result);
            return Ok(AgentResponse {
                response_type: AgentResponseType::Answer,
                message: response_agent::need_more_info(),
                data: Some(result),
                data_display: None,
                suggestions: vec![],
                task: Some(task_state),
                confirmation: None,
                frontend_action,
                performance: None,
            });
        }

        // 评估结果是否需要升级（简化版：检查空结果）
        if self.should_escalate(&task_state, &result) {
            tracing::info!("[Agent] Result unsatisfactory, attempting replan");

            let hint = self.build_escalation_hint(&task_state, &result);
            let _ = progress_tx
                .send(AgentProgressEvent::Progress {
                    progress: 50,
                    completed_steps: 0,
                    total_steps: 0,
                    message: response_agent::escalation_status(&hint),
                })
                .await;

            // 使用 Planner.replan
            match self.planner.replan(original_request, &hint).await {
                Ok(replan_output)
                    if replan_output.status == PlannerStatus::Plan
                        && !replan_output.steps.is_empty() =>
                {
                    let cap_ids: Vec<String> = replan_output
                        .steps
                        .iter()
                        .map(|s| s.capability_id.clone())
                        .collect();
                    let cap_schemas = capability::get_capabilities_by_ids(&cap_ids).await;

                    if let Ok(new_steps) = recipe::validate_and_convert_steps(
                        replan_output.steps.clone(),
                        replan_output.reasoning.clone(),
                        &cap_schemas,
                    ) {
                        let new_recipe = Self::build_recipe_from_steps(
                            new_steps,
                            replan_output
                                .reasoning
                                .clone()
                                .unwrap_or_else(response_agent::escalation_retry),
                            original_request,
                        );

                        // 执行升级后的 Recipe
                        let progress_tx_for_summary = progress_tx.clone();
                        crate::services::agent::merope::mark_activity(&self.db, user_id, "working")
                            .await;
                        let new_task_result = self
                            .executor
                            .execute_with_progress(&new_recipe, user_id, Some(progress_tx))
                            .await;
                        crate::services::agent::merope::mark_activity(&self.db, user_id, "idle")
                            .await;
                        let new_task_state = new_task_result?;
                        result = self.extract_final_result(&new_task_state);

                        let frontend_action = self.extract_frontend_action(&result);
                        let data_display = self.infer_data_display_v2(&result, &replan_output);

                        if let Some(obj) = result.as_object_mut() {
                            obj.insert(
                                "recipe".to_string(),
                                serde_json::to_value(&new_recipe).unwrap_or_default(),
                            );
                        }

                        return Ok(AgentResponse {
                            response_type: if new_task_state.status == TaskStatus::Failed {
                                AgentResponseType::Error
                            } else {
                                AgentResponseType::Answer
                            },
                            message: self
                                .generate_response_message_v2(
                                    &replan_output,
                                    &new_task_state,
                                    user_id,
                                    Some(&progress_tx_for_summary),
                                )
                                .await,
                            data: Some(result),
                            data_display,
                            suggestions: vec![],
                            task: Some(new_task_state),
                            confirmation: None,
                            frontend_action,
                            performance: None,
                        });
                    }
                }
                _ => {
                    tracing::info!(
                        "[Agent] Replan failed or returned non-plan, using original result"
                    );
                }
            }
        }

        // 返回原始结果
        let frontend_action = self.extract_frontend_action(&result);
        let data_display = self.infer_data_display_v2(&result, planner_output);

        if let Some(obj) = result.as_object_mut() {
            obj.insert(
                "recipe".to_string(),
                serde_json::to_value(recipe).unwrap_or_default(),
            );
        }

        let is_failed = task_state.status == TaskStatus::Failed;
        Ok(AgentResponse {
            response_type: if is_failed {
                AgentResponseType::Error
            } else {
                AgentResponseType::Answer
            },
            message: self
                .generate_response_message_v2(
                    planner_output,
                    &task_state,
                    user_id,
                    Some(&progress_tx),
                )
                .await,
            data: Some(result),
            data_display,
            suggestions: vec![],
            task: Some(task_state),
            confirmation: None,
            frontend_action,
            performance: None,
        })
    }

    /// 从 TaskState 提取能力 ID 列表（用于升级门控）
    pub(crate) fn capability_ids_from_task(task_state: &TaskState) -> Vec<String> {
        if let Some(recipe) = &task_state.recipe {
            let ids: Vec<String> = recipe
                .steps
                .iter()
                .map(|s| s.capability_id.clone())
                .collect();
            if !ids.is_empty() {
                return ids;
            }
        }
        if let Some(trace) = &task_state.execution_trace {
            let ids: Vec<String> = trace
                .steps
                .iter()
                .map(|s| s.capability_id.clone())
                .collect();
            if !ids.is_empty() {
                return ids;
            }
        }
        Vec::new()
    }

    /// 是否允许联网搜索升级（白名单：generateReadingList + 显式 flag，或纯外部调研链）
    pub(crate) fn allow_web_search_escalation(
        task_state: &TaskState,
        capability_ids: &[String],
    ) -> bool {
        // brew.generateReadingList 仅在步骤参数显式开启时允许 web
        if let Some(recipe) = &task_state.recipe {
            for step in &recipe.steps {
                if step.capability_id == "brew.generateReadingList" {
                    let flag = step
                        .params
                        .get("allowWebSearch")
                        .or_else(|| step.params.get("useWebSearch"))
                        .or_else(|| step.params.get("allow_web_search"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if flag {
                        return true;
                    }
                }
            }
        }
        // 已包含 webSearch 的计划不算「从本地升级到 web」；本地域默认禁止
        let has_local = capability_ids
            .iter()
            .any(|id| escalation::ResultEvaluator::is_local_data_capability(id));
        if has_local {
            return false;
        }
        // 非本地域空结果可继续建议 web
        true
    }

    /// 构建评估上下文
    pub(crate) fn evaluation_context_for_task(
        task_state: &TaskState,
    ) -> escalation::EvaluationContext {
        let capability_ids = Self::capability_ids_from_task(task_state);
        let allow_web_search = Self::allow_web_search_escalation(task_state, &capability_ids);
        escalation::EvaluationContext {
            capability_ids,
            allow_web_search,
        }
    }

    /// 判断是否需要升级
    pub(crate) fn should_escalate(&self, task_state: &TaskState, result: &Value) -> bool {
        if task_state.status == TaskStatus::Failed {
            // 配置类错误（API Key 未配置）不应触发 replan 烧预算/再次选 webSearch
            if let Some(err) = &task_state.error {
                let err_lower = err.to_lowercase();
                if err.contains("API Key 未配置")
                    || err.contains("未配置")
                    || err_lower.contains("not configured")
                    || err_lower.contains("api key")
                {
                    tracing::info!(
                        error = %err,
                        "[Agent] Configuration error — skip escalation/replan"
                    );
                    return false;
                }
            }
            return true;
        }
        // 使用 ResultEvaluator 进行深度评估（携带能力上下文以门控 webSearch）
        let evaluator = escalation::ResultEvaluator::new();
        let ctx = Self::evaluation_context_for_task(task_state);
        let eval = evaluator.evaluate_with_context(result, &ctx);
        if !eval.is_satisfied {
            tracing::info!(
                score = eval.satisfaction_score,
                reason = ?eval.reason,
                patterns = ?eval.failure_patterns,
                suggests_web = eval.suggests_web_search,
                suggests_local = eval.suggests_local_alternatives,
                caps = ?ctx.capability_ids,
                "[Agent] ResultEvaluator: escalation recommended"
            );
        }
        !eval.is_satisfied
    }

    /// 构建升级提示（使用 ResultEvaluator 的失败模式分析）
    pub(crate) fn build_escalation_hint(&self, task_state: &TaskState, result: &Value) -> String {
        if task_state.status == TaskStatus::Failed {
            let err = task_state.error.as_deref().unwrap_or("Processing failed");
            let err_lower = err.to_lowercase();
            if err.contains("API Key 未配置")
                || err.contains("未配置")
                || err_lower.contains("not configured")
            {
                return format!(
                    "前次执行因配置缺失失败：{}。请勿重试同一能力或改用 ai.webSearch；改为本地能力或提示用户配置密钥。",
                    err
                );
            }
            return format!("前次执行失败：{}。请尝试替代方案。", err);
        }

        let evaluator = escalation::ResultEvaluator::new();
        let ctx = Self::evaluation_context_for_task(task_state);
        let eval = evaluator.evaluate_with_context(result, &ctx);

        let mut hints = Vec::new();
        if let Some(reason) = &eval.reason {
            hints.push(format!("失败原因：{}", reason));
        }
        // notFound 建议值：replan 最高优先 — 用建议值重试 brew，禁止 webSearch
        if !eval.suggested_retry_values.is_empty() {
            let joined = eval.suggested_retry_values.join(" / ");
            let brew_cap = ctx
                .capability_ids
                .iter()
                .find(|id| id.starts_with("brew."))
                .map(|s| s.as_str())
                .unwrap_or("brew.items");
            hints.push(format!(
                "【最高优先】用 {} 重试，将 sourceName/name/query/author 设为建议值之一：{}。不要使用 ai.webSearch",
                brew_cap, joined
            ));
        }
        for hint in &eval.improvement_hints {
            hints.push(hint.clone());
        }
        if eval.suggests_web_search {
            hints.push("请尝试联网搜索能力（ai.webSearch 或 ai.groundingSearch）".to_string());
        } else if eval.suggests_local_alternatives {
            // 本地 brew miss：强制 replan 走 brew.page / search.fuzzy / brew.items
            let already_forbids = eval.improvement_hints.iter().any(|h| {
                h.contains("禁止使用 ai.webSearch") || h.contains("禁止改用 ai.webSearch")
            });
            if !already_forbids {
                hints.push(
                    "禁止使用 ai.webSearch / ai.groundingSearch；优先 brew.page、search.fuzzy 或 brew.items（放宽参数）"
                        .to_string(),
                );
            }
        }
        if hints.is_empty() {
            if eval.suggests_local_alternatives {
                "前次本地数据结果为空，请用 brew.page / search.fuzzy / brew.items 放宽查询或向用户澄清，不要联网搜索。"
                    .to_string()
            } else {
                "前次执行结果为空或不满足目标，请尝试其他能力或联网搜索。".to_string()
            }
        } else {
            hints.join("。")
        }
    }

    async fn mood_refuse_response(
        &self,
        user_id: i32,
        message: Option<String>,
        progress_tx: Option<&tokio::sync::mpsc::Sender<AgentProgressEvent>>,
    ) -> Option<AgentResponse> {
        let message = message?;
        if let Some(tx) = progress_tx {
            Self::stream_text_as_tokens(tx, &message).await;
        }
        crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
        Some(AgentResponse {
            response_type: AgentResponseType::Answer,
            message,
            data: Some(json!({ "type": "mood_refuse" })),
            data_display: None,
            suggestions: vec![],
            task: None,
            confirmation: None,
            frontend_action: None,
            performance: None,
        })
    }

    /// 执行已保存的 Recipe（跳过意图分析）
    ///
    /// 用于从预设中直接执行任务，避免重复的意图解析
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

    /// 快速路径：执行简单的单步查询（Planner 版）
    pub(crate) async fn execute_simple_query_v2(
        &self,
        recipe: &Recipe,
        planner_output: &PlannerOutput,
        user_id: i32,
        progress_tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
        user_input: &str,
        conversation_context: Option<&[ConversationMessage]>,
    ) -> Result<AgentResponse, String> {
        let step = &recipe.steps[0];
        let step_description = capability::get_step_description(step);

        // 发送 TaskCreated（前端思考面板依赖此事件初始化）
        let _ = progress_tx
            .send(AgentProgressEvent::TaskCreated {
                task_id: recipe.id.clone(),
                message: String::new(),
                total_steps: 1,
                step_descriptions: vec![step_description.clone()],
            })
            .await;

        // 发送开始执行进度
        let _ = progress_tx
            .send(AgentProgressEvent::Progress {
                progress: 20,
                completed_steps: 0,
                total_steps: 1,
                message: response_agent::describe_step_start(&step_description),
            })
            .await;

        // 带进度执行（Skill 可能展开为多个动态子步骤，需要把 progress_tx 传下去）
        crate::services::agent::merope::mark_activity(&self.db, user_id, "working").await;
        let task_result = self
            .executor
            .execute_with_progress(recipe, user_id, Some(progress_tx.clone()))
            .await;
        crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
        let task_state = task_result?;

        // 获取执行结果
        let step_result = task_state.step_results.get(&step.id);
        let success = step_result.map(|r| r.success).unwrap_or(false);

        // 发送完成进度
        let _ = progress_tx
            .send(AgentProgressEvent::Progress {
                progress: 100,
                completed_steps: 1,
                total_steps: 1,
                message: response_agent::done_status(),
            })
            .await;

        // 构建响应
        let mut result = self.extract_final_result(&task_state);
        let frontend_action = self.extract_frontend_action(&result);
        let data_display = self.infer_data_display_v2(&result, planner_output);

        if let Some(obj) = result.as_object_mut() {
            obj.insert(
                "recipe".to_string(),
                serde_json::to_value(recipe).unwrap_or_default(),
            );
        }

        // v3 记忆记录（单步查询也需要记录）
        {
            record_execution_memory(MemoryRecordParams {
                user_id,
                user_input,
                recipe,
                planner_steps_len: 1,
                success,
                error_msg: task_state.error.as_deref(),
                log_prefix: "",
                conversation_context,
                step_results: Some(&task_state.step_results),
            })
            .await;
        }

        let is_failed = task_state.status == TaskStatus::Failed;
        Ok(AgentResponse {
            response_type: if is_failed {
                AgentResponseType::Error
            } else {
                AgentResponseType::Answer
            },
            message: self
                .generate_response_message_v2(
                    planner_output,
                    &task_state,
                    user_id,
                    Some(&progress_tx),
                )
                .await,
            data: Some(result),
            data_display,
            suggestions: vec![],
            task: Some(task_state),
            confirmation: None,
            frontend_action,
            performance: None,
        })
    }

    /// 处理用户确认
    pub async fn confirmation_lane_key(
        &self,
        confirmation_id: &str,
        user_id: i32,
    ) -> Result<Option<String>, String> {
        Ok(self
            .confirmation_resume_context(confirmation_id, user_id)
            .await?
            .and_then(|ctx| ctx.lane_key))
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
        .map_err(|error| format!("Failed to load confirmation: {error}"))?;
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

fn spawn_motion_directive(
    context: crate::services::agent::merope::MotionContext,
    progress_tx: Option<tokio::sync::mpsc::Sender<AgentProgressEvent>>,
) -> tokio::task::JoinHandle<Option<crate::services::agent::merope::PerformanceDirective>> {
    tokio::spawn(async move {
        let performance = crate::services::agent::merope::direct_motion(context).await;
        if let (Some(tx), Some(performance)) = (progress_tx, performance.as_ref()) {
            let _ = tx
                .send(AgentProgressEvent::PerformancePlan {
                    performance: performance.clone(),
                })
                .await;
        }
        performance
    })
}

async fn attach_motion_to_result(
    result: Result<AgentResponse, String>,
    user_id: i32,
    user_text: &str,
    mood: Option<crate::services::agent::merope::MoodTransition>,
    progress_tx: Option<tokio::sync::mpsc::Sender<AgentProgressEvent>>,
) -> Result<AgentResponse, String> {
    let mut response = result?;
    let Some(mood) = mood else {
        return Ok(response);
    };
    let task_success = response.task.as_ref().and_then(|task| match task.status {
        TaskStatus::Completed => Some(true),
        TaskStatus::Failed | TaskStatus::Cancelled => Some(false),
        _ => None,
    });
    let phase = if task_success.is_some() {
        crate::services::agent::merope::MotionPhase::Outcome
    } else {
        crate::services::agent::merope::MotionPhase::Delivery
    };
    let handle = spawn_motion_directive(
        crate::services::agent::merope::MotionContext {
            user_id,
            phase,
            mood,
            activity: "talking".to_string(),
            user_text: user_text.to_string(),
            response_text: Some(response.message.clone()),
            task_success,
        },
        progress_tx,
    );
    response.performance = handle.await.ok().flatten();
    Ok(response)
}
