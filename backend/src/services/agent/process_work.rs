// Work path: Planner → Recipe → escalation. Not Chat.

use serde_json::{json, Value};

use super::agent_footer::*;
use super::agent_header::*;
use super::motion_overlay::{
    attach_motion_to_result, motion_context, publish_local_motion, spawn_motion_refinement,
};
use super::types::*;
use super::{capability, escalation, orchestrator, recipe, response_agent, skill_evolution, types};

impl Agent {
    pub(super) async fn process_work(
        &self,
        request: UserRequest,
        mood_transition: Option<crate::services::agent::merope::MoodTransition>,
        mood_before: Option<f64>,
        round_motion_style: String,
    ) -> Result<AgentResponse, String> {
        let user_id = request.user_id;
        // 1. Planner 规划（索引按授予权限过滤，避免规划到执行层必拒的能力）
        let granted = crate::services::agent::get_user_permissions(&self.db, user_id).await;
        let planner_output = match self.planner.plan_for(&request, &granted).await {
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
        crate::services::agent::merope::note_chat_diary(&self.db, user_id, &request.raw_input)
            .await;

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
                    &request,
                    mood_transition.clone(),
                    &round_motion_style,
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
                    &request,
                    mood_transition.clone(),
                    &round_motion_style,
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
                    &request,
                    mood_transition.clone(),
                    &round_motion_style,
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
                        &request,
                        mood_transition.clone(),
                        &round_motion_style,
                    )
                    .await;
                }
                // 低置信度：本路径只打 warn；带进度路径会降级为澄清。
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

        // 4. 检查敏感操作（系统任务：Low/Medium 自动确认；High/Critical 拒绝）
        let sensitive_steps = self.check_sensitive_steps(&recipe).await;
        if !sensitive_steps.is_empty() {
            match Self::system_sensitive_gate(user_id, &sensitive_steps) {
                Some(Ok(())) => {} // 系统任务已自动确认，继续执行
                Some(Err(blocked)) => {
                    crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                    return attach_motion_to_result(
                        Ok(blocked),
                        &request,
                        mood_transition.clone(),
                        &round_motion_style,
                    )
                    .await;
                }
                None => {
                    let session_id = request.context.as_ref().and_then(|c| c.session_id.clone());
                    let run_id = request.context.as_ref().and_then(|c| c.run_id.clone());
                    let source_intent_id = request
                        .context
                        .as_ref()
                        .and_then(|c| c.source_intent_id.clone());
                    crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                    let response = self
                        .request_confirmation_v2(
                            &recipe,
                            &planner_output,
                            user_id,
                            sensitive_steps,
                            session_id,
                            run_id,
                            source_intent_id,
                        )
                        .await;
                    return attach_motion_to_result(
                        response,
                        &request,
                        mood_transition.clone(),
                        &round_motion_style,
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
                    &request,
                    mood_transition.clone(),
                    &round_motion_style,
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

        // 记忆提取 + 日志；history ≥ 4 时会话摘要归档。
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
        attach_motion_to_result(Ok(response), &request, mood_transition, &round_motion_style).await
    }

    pub(super) async fn process_work_with_progress(
        &self,
        request: UserRequest,
        progress_tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
        mood_transition: Option<crate::services::agent::merope::MoodTransition>,
        mood_before: Option<f64>,
        round_motion_style: String,
    ) -> Result<AgentResponse, String> {
        let user_id = request.user_id;

        // Refinements are scoped to this turn. Dropping the guards aborts any
        // Lite call that has outlived the meaning of its reaction.
        let mut motion_refinements = Vec::new();

        if let Some(mood) = mood_transition.clone() {
            let reaction_context = motion_context(
                &request,
                user_id,
                crate::services::agent::merope::MotionPhase::Reaction,
                mood,
                round_motion_style.clone(),
                None,
                None,
            );
            publish_local_motion(&reaction_context, &progress_tx).await;
            motion_refinements.push(spawn_motion_refinement(
                reaction_context,
                progress_tx.clone(),
            ));
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

        let granted = crate::services::agent::get_user_permissions(&self.db, user_id).await;
        let planner_output = match self
            .planner
            .plan_with_progress_for(&request, &progress_tx, &granted)
            .await
        {
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
        crate::services::agent::merope::note_chat_diary(&self.db, user_id, &request.raw_input)
            .await;

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
                // Planner time was the useful window for the reaction. Never
                // let an unfinished reaction cross into the spoken reply.
                motion_refinements.clear();
                let planner_reply = planner_output
                    .chat_reply
                    .unwrap_or_else(response_agent::greeting);

                // 尝试真正的流式 AI 回复（token-by-token from model）
                let reply = self
                    .stream_chat_response(&request, &planner_reply, &progress_tx)
                    .await;
                if let Some(mood) = mood_transition.clone() {
                    let delivery_context = motion_context(
                        &request,
                        user_id,
                        crate::services::agent::merope::MotionPhase::Delivery,
                        mood,
                        round_motion_style.clone(),
                        Some(reply.clone()),
                        None,
                    );
                    publish_local_motion(&delivery_context, &progress_tx).await;
                }
                response_agent::finish_stream(&progress_tx).await;
                let performance = None;
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
                    &request,
                    mood_transition.clone(),
                    &round_motion_style,
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
                    &request,
                    mood_transition.clone(),
                    &round_motion_style,
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
                        &request,
                        mood_transition.clone(),
                        &round_motion_style,
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
                        "I'm not very confident I understood that ({:.0}%). {}Could you describe what you want in more detail?",
                        planner_output.confidence * 100.0,
                        planner_output
                            .reasoning
                            .as_deref()
                            .map(|r| format!("My reading is: {r}. "))
                            .unwrap_or_default()
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
                        &request,
                        mood_transition.clone(),
                        &round_motion_style,
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
                        &request,
                        mood_transition.clone(),
                        &round_motion_style,
                    )
                    .await;
                }
                None => {
                    let session_id = request.context.as_ref().and_then(|c| c.session_id.clone());
                    let run_id = request.context.as_ref().and_then(|c| c.run_id.clone());
                    let source_intent_id = request
                        .context
                        .as_ref()
                        .and_then(|c| c.source_intent_id.clone());
                    crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                    let response = self
                        .request_confirmation_v2(
                            &recipe,
                            &planner_output,
                            user_id,
                            sensitive_steps,
                            session_id,
                            run_id,
                            source_intent_id,
                        )
                        .await;
                    return attach_motion_to_result(
                        response,
                        &request,
                        mood_transition.clone(),
                        &round_motion_style,
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
                    &request,
                    mood_transition.clone(),
                    &round_motion_style,
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
                &request,
                mood_transition.clone(),
                &round_motion_style,
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

        // announce_plan：流式计划说明（说话模型，失败则模板）。
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

        attach_motion_to_result(result, &request, mood_transition, &round_motion_style).await
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

            let granted = crate::services::agent::get_user_permissions(&self.db, user_id).await;
            match self
                .planner
                .replan_with_progress_for(original_request, &hint, &progress_tx, &granted)
                .await
            {
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
        // 本地域（brew / platform / search.fuzzy / config.get / library）默认禁止 web 升级。
        let has_local = capability_ids
            .iter()
            .any(|id| escalation::is_local_data_capability(id));
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
        // Evaluate structured results with the task capability policy.
        let ctx = Self::evaluation_context_for_task(task_state);
        let eval = escalation::evaluate_with_context(result, &ctx);
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
                    "Previous run failed because configuration is missing: {}. Do not retry the same capability or switch to ai.webSearch; use a local capability or ask the user to configure a key.",
                    err
                );
            }
            return format!("Previous run failed: {}. Try an alternative.", err);
        }

        let ctx = Self::evaluation_context_for_task(task_state);
        let eval = escalation::evaluate_with_context(result, &ctx);

        let mut hints = Vec::new();
        if let Some(reason) = &eval.reason {
            hints.push(format!("Failure reason: {reason}"));
        }
        // notFound suggestions: replan first — retry brew with a suggested value, never webSearch
        if !eval.suggested_retry_values.is_empty() {
            let joined = eval.suggested_retry_values.join(" / ");
            let brew_cap = ctx
                .capability_ids
                .iter()
                .find(|id| id.starts_with("brew."))
                .map(|s| s.as_str())
                .unwrap_or("brew.items");
            hints.push(format!(
                "[Highest priority] Retry with {brew_cap}, setting sourceName/name/query/author to one of: {joined}. Do not use ai.webSearch"
            ));
        }
        for hint in &eval.improvement_hints {
            hints.push(hint.clone());
        }
        if eval.suggests_web_search {
            hints.push(
                "Try a web search capability (ai.webSearch or ai.groundingSearch)".to_string(),
            );
        } else if eval.suggests_local_alternatives {
            // Local brew miss: force replan onto brew.page / search.fuzzy / brew.items
            let already_forbids = eval.improvement_hints.iter().any(|h| {
                h.contains("禁止使用 ai.webSearch")
                    || h.contains("禁止改用 ai.webSearch")
                    || h.contains("Do not use ai.webSearch")
            });
            if !already_forbids {
                hints.push(
                    "Do not use ai.webSearch / ai.groundingSearch; prefer brew.page, search.fuzzy, or brew.items (relax parameters)"
                        .to_string(),
                );
            }
        }
        if hints.is_empty() {
            if eval.suggests_local_alternatives {
                "Previous local data result was empty. Use brew.page / search.fuzzy / brew.items with a broader query, or ask the user. Do not search the web."
                    .to_string()
            } else {
                "Previous result was empty or did not meet the goal. Try another capability or a web search.".to_string()
            }
        } else {
            hints.join(". ")
        }
    }

    pub(super) async fn mood_refuse_response(
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

        // 单步查询同样走 record_execution_memory。
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
}
