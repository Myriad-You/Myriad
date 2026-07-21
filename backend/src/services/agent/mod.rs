//! Agent 模块
//!
//! AI 驱动的自然语言任务编排系统
//!
//! ## 架构（两层：Planner → Executor）
//!
//! ```text
//! 用户输入 ──▶ Planner (Pro AI) ──▶ PlannerOutput
//!                                        │
//!                          ┌──────────────┼──────────────┐
//!                          │              │              │
//!                       Chat          Clarify         Plan
//!                          │              │              │
//!                          ▼              ▼              ▼
//!                    直接回复        请求澄清      Recipe Steps
//!                                                       │
//!                                                       ▼
//!                                              ConfirmationCheck
//!                                                       │
//!                                                       ▼
//!                                                  Executor ──▶ Result
//!                                                       │
//!                                                       ▼
//!                                              EscalationManager
//!                                                (Planner.replan)
//! ```
//!
//! ## 模块
//!
//! - `types`: 核心类型定义
//! - `capability`: 能力注册表
//! - `planner`: 规划器（Pro AI 单次调用完成意图理解+执行规划）
//! - `recipe`: 方案验证与转换
//! - `executor`: 执行引擎
//! - `escalation`: 结果评估与智能升级
//!
//! ## 使用示例
//!
//! ```rust,ignore
//! use crate::services::agent::{Agent, UserRequest};
//!
//! let agent = Agent::new(db).await;
//! let request = UserRequest {
//!     raw_input: "总结一下最近一周B站的更新".to_string(),
//!     timestamp: chrono::Utc::now(),
//!     user_id: 1,
//!     context: None,
//! };
//!
//! let response = agent.process(request).await?;
//! ```

pub mod capability;
pub mod escalation;
pub mod executor;
pub mod heartbeat;
pub mod identity;
pub mod intent;
pub mod mcp;
pub mod memory;
pub mod notification_preferences;
pub mod notification_producers;
pub mod notifications;
pub mod orchestrator;
pub mod planner;
pub mod queue;
pub mod recipe;
pub mod response_agent;
pub mod routing;
pub mod run_hub;
pub mod skill;
pub mod skill_evolution;
pub mod tier_router;
pub mod types;

// 重新导出核心类型
pub use types::*;

use chrono::{Duration, Utc};
use once_cell::sync::Lazy;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 全局 Lane Queue（控制并发和用户级串行）
pub static LANE_QUEUE: Lazy<Arc<queue::LaneQueue>> =
    Lazy::new(|| Arc::new(queue::LaneQueue::new(4)));

/// 系统用户 ID（Heartbeat 定时任务等无人值守场景）
pub const SYSTEM_USER_ID: i32 = 0;

/// 待确认配方存储
static PENDING_CONFIRMATIONS: Lazy<Arc<RwLock<HashMap<String, PendingRecipeConfirmation>>>> =
    Lazy::new(|| Arc::new(RwLock::new(HashMap::new())));
const CONFIRMATION_REGISTRY_NAMESPACE: &str = "agent_recipe_confirmation";

/// Peek-only context for attaching a confirmation resume to its original session.
#[derive(Debug, Clone)]
pub struct ConfirmationResumeContext {
    pub lane_key: Option<String>,
    pub session_id: Option<String>,
    /// Original process run id when confirmation was requested (may be reused on confirm/stream).
    pub run_id: Option<String>,
}

/// Extract session id from a lane key of the form `user:{id}:session:{session_id}`.
fn session_id_from_lane_key(lane_key: &str) -> Option<String> {
    lane_key
        .split_once(":session:")
        .map(|(_, session_id)| session_id.to_string())
        .filter(|session_id| !session_id.is_empty())
}

/// 待确认的配方信息
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingRecipeConfirmation {
    /// 确认请求
    pub request: ConfirmationRequest,
    /// 原始配方
    pub recipe: Recipe,
    /// 用户 ID
    pub user_id: i32,
    /// 原始 PlannerOutput（用于升级重规划）
    pub planner_output: PlannerOutput,
    /// 发起确认时的会话 ID（确认续跑需写回同一 session 历史）
    #[serde(default)]
    pub session_id: Option<String>,
    /// 发起确认时的 run id（确认续跑复用同一 run hub / 通知）
    #[serde(default)]
    pub run_id: Option<String>,
}

/// Agent 主入口
///
/// 两层架构：Planner (Pro AI) → Executor
pub struct Agent {
    /// 规划器（Pro AI 单次调用）
    planner: planner::Planner,
    /// 执行引擎
    executor: executor::Executor,
    /// Shared persistence used by confirmation hand-offs across backend replicas.
    db: DatabaseConnection,
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
    pub async fn process(&self, request: UserRequest) -> Result<AgentResponse, String> {
        let user_id = request.user_id;

        // 请求驱动的过期任务清理
        executor::maybe_cleanup_tasks().await;

        tracing::info!(
            user_id = user_id,
            input = %request.raw_input,
            "[Agent] Processing request"
        );

        // 1. Planner 规划
        let planner_output = self.planner.plan(&request).await?;

        tracing::debug!(
            status = ?planner_output.status,
            confidence = planner_output.confidence,
            steps = planner_output.steps.len(),
            "[Agent] Planner output"
        );

        // 2. 根据状态分流
        match planner_output.status {
            PlannerStatus::Chat => {
                return Ok(AgentResponse {
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
                });
            }
            PlannerStatus::Clarify => {
                let clarification = planner_output
                    .clarification
                    .unwrap_or(PlannerClarification {
                        message: response_agent::need_clarification(),
                        options: vec![],
                    });
                return Ok(AgentResponse {
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
                });
            }
            PlannerStatus::Unsupported => {
                let reason = planner_output
                    .unsupported_reason
                    .unwrap_or_else(response_agent::unsupported_operation);

                // 记录能力缺口
                if let Some(evo) = skill_evolution::get_skill_evolution() {
                    evo.detect_capability_gap(&request.raw_input, &reason).await;
                }

                return Ok(AgentResponse {
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
                });
            }
            PlannerStatus::Plan => {
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
        let recipe_steps = recipe::validate_and_convert_steps(
            planner_output.steps.clone(),
            planner_output.reasoning.clone(),
            &cap_schemas,
        )?;

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
                Some(Err(blocked)) => return Ok(blocked),
                None => {
                    let session_id = request
                        .context
                        .as_ref()
                        .and_then(|c| c.session_id.clone());
                    let run_id = request.context.as_ref().and_then(|c| c.run_id.clone());
                    return self
                        .request_confirmation_v2(
                            &recipe,
                            &planner_output,
                            user_id,
                            sensitive_steps,
                            session_id,
                            run_id,
                        )
                        .await;
                }
            }
        }

        // 4.5 检查必需参数缺失
        if let Some(missing_response) = self
            .check_missing_required_parameters(&recipe, &planner_output, user_id, None)
            .await?
        {
            return Ok(missing_response);
        }

        // 5. 执行方案
        let task_state = self.executor.execute(&recipe, user_id).await?;
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
                tokio::spawn(async move {
                    match evo
                        .auto_create_skill_abstracted(&request_text, &step_descriptions, &step_caps)
                        .await
                    {
                        Ok(skill) => {
                            tracing::info!(skill_id = %skill.id, "[Agent] Auto-created skill from recipe")
                        }
                        Err(e) => {
                            tracing::debug!(error = %e, "[Agent] Skill auto-creation skipped")
                        }
                    }
                });
            }
        }

        // 7. 构建响应
        let frontend_action = self.extract_frontend_action(&result);
        let data_display = self.infer_data_display_v2(&result, &planner_output);

        Ok(AgentResponse {
            response_type: AgentResponseType::Answer,
            message: self
                .generate_response_message_v2(&planner_output, &task_state, None)
                .await,
            data: Some(result),
            data_display,
            suggestions: vec![],
            task: Some(task_state),
            confirmation: None,
            frontend_action,
        })
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

        tracing::info!(
            user_id = user_id,
            input = %request.raw_input,
            has_history = request.context.as_ref().and_then(|c| c.conversation_history.as_ref()).is_some(),
            "[Agent] Processing request with progress tracking"
        );

        // 1. Planner 规划（Pro AI 单次调用）
        let _ = progress_tx
            .send(AgentProgressEvent::Progress {
                progress: 5,
                completed_steps: 0,
                total_steps: 0,
                message: response_agent::understanding_request(),
            })
            .await;

        let planner_output = self.planner.plan(&request).await?;

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

                // 尝试真正的流式 AI 回复（token-by-token from model）
                let reply = self
                    .stream_chat_response(&request, &planner_reply, &progress_tx)
                    .await;

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
                return Ok(AgentResponse {
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
                });
            }
            PlannerStatus::Unsupported => {
                let reason = planner_output
                    .unsupported_reason
                    .unwrap_or_else(response_agent::unsupported_operation);

                // 记录能力缺口
                if let Some(evo) = skill_evolution::get_skill_evolution() {
                    evo.detect_capability_gap(&request.raw_input, &reason).await;
                }

                return Ok(AgentResponse {
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
                });
            }
            PlannerStatus::Plan => {
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
                    return Ok(AgentResponse {
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
                    });
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
        let recipe_steps = recipe::validate_and_convert_steps(
            planner_output.steps.clone(),
            planner_output.reasoning.clone(),
            &cap_schemas,
        )?;

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
        //    不能只靠 capability_id 字符串启发式，否则 tapp.interact / page.interact / MCP 会直通）
        //    P1: gates run before fast path so sensitive/missing-param never bypass.
        //    P2: carry session_id so confirm resume stays on the same conversation.
        let sensitive_steps = self.check_sensitive_steps(&recipe).await;
        if !sensitive_steps.is_empty() {
            match Self::system_sensitive_gate(user_id, &sensitive_steps) {
                Some(Ok(())) => {} // 系统任务已自动确认，继续执行
                Some(Err(blocked)) => return Ok(blocked),
                None => {
                    let session_id = request
                        .context
                        .as_ref()
                        .and_then(|c| c.session_id.clone());
                    let run_id = request.context.as_ref().and_then(|c| c.run_id.clone());
                    return self
                        .request_confirmation_v2(
                            &recipe,
                            &planner_output,
                            user_id,
                            sensitive_steps,
                            session_id,
                            run_id,
                        )
                        .await;
                }
            }
        }

        // 4.5 检查必需参数缺失 — 执行前收集用户信息（单步/多步共用）
        if let Some(missing_response) = self
            .check_missing_required_parameters(
                &recipe,
                &planner_output,
                user_id,
                Some(&progress_tx),
            )
            .await?
        {
            return Ok(missing_response);
        }

        // ========== 快速路径：仅安全的单步且参数齐全时走 ==========
        if recipe.steps.len() == 1 {
            tracing::debug!(
                recipe_id = %recipe.id,
                "[Agent] Using fast path for simple query"
            );
            return self
                .execute_simple_query_v2(
                    &recipe,
                    &planner_output,
                    user_id,
                    progress_tx,
                    &request.raw_input,
                    request
                        .context
                        .as_ref()
                        .and_then(|c| c.conversation_history.as_deref()),
                )
                .await;
        }
        // ========== 快速路径结束 ==========

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
            response_agent::announce_plan(&request.raw_input, &step_descs, &progress_tx).await;

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

                tokio::spawn(async move {
                    match evo
                        .auto_create_skill_abstracted(&request_text, &step_descriptions, &step_caps)
                        .await
                    {
                        Ok(skill) => {
                            tracing::info!(skill_id = %skill.id, "[Agent] AI-abstracted skill created from recipe")
                        }
                        Err(e) => {
                            tracing::debug!(error = %e, "[Agent] Skill auto-creation skipped")
                        }
                    }
                });
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

        result
    }

    /// 执行配方（带进度回调和升级）— 使用 Planner
    async fn execute_recipe_with_progress_v2(
        &self,
        recipe: &Recipe,
        planner_output: &PlannerOutput,
        original_request: &UserRequest,
        user_id: i32,
        progress_tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
    ) -> Result<AgentResponse, String> {
        // 执行 Recipe
        let mut task_state = self
            .executor
            .execute_with_progress(recipe, user_id, Some(progress_tx.clone()))
            .await?;

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
                        let new_task_state = self
                            .executor
                            .execute_with_progress(&new_recipe, user_id, Some(progress_tx))
                            .await?;
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
                                    Some(&progress_tx_for_summary),
                                )
                                .await,
                            data: Some(result),
                            data_display,
                            suggestions: vec![],
                            task: Some(new_task_state),
                            confirmation: None,
                            frontend_action,
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
                .generate_response_message_v2(planner_output, &task_state, Some(&progress_tx))
                .await,
            data: Some(result),
            data_display,
            suggestions: vec![],
            task: Some(task_state),
            confirmation: None,
            frontend_action,
        })
    }

    /// 从 TaskState 提取能力 ID 列表（用于升级门控）
    fn capability_ids_from_task(task_state: &TaskState) -> Vec<String> {
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
    fn allow_web_search_escalation(task_state: &TaskState, capability_ids: &[String]) -> bool {
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
    fn evaluation_context_for_task(
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
    fn should_escalate(&self, task_state: &TaskState, result: &Value) -> bool {
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
    fn build_escalation_hint(&self, task_state: &TaskState, result: &Value) -> String {
        if task_state.status == TaskStatus::Failed {
            let err = task_state.error.as_deref().unwrap_or("未知错误");
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
            let already_forbids = eval
                .improvement_hints
                .iter()
                .any(|h| h.contains("禁止使用 ai.webSearch") || h.contains("禁止改用 ai.webSearch"));
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

    /// 执行已保存的 Recipe（跳过意图分析）
    ///
    /// 用于从预设中直接执行任务，避免重复的意图解析
    pub async fn execute_saved_recipe(
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
        let task_state = self
            .executor
            .execute_with_progress(&recipe, user_id, Some(progress_tx))
            .await?;

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
        })
    }

    fn planner_output_for_saved_recipe(recipe: &Recipe) -> PlannerOutput {
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

    fn validate_saved_recipe(recipe: &Recipe) -> Result<(), String> {
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
    async fn execute_simple_query_v2(
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
        let task_state = self
            .executor
            .execute_with_progress(recipe, user_id, Some(progress_tx.clone()))
            .await?;

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
                .generate_response_message_v2(planner_output, &task_state, Some(&progress_tx))
                .await,
            data: Some(result),
            data_display,
            suggestions: vec![],
            task: Some(task_state),
            confirmation: None,
            frontend_action,
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
        let pending = crate::api::tapp_runtime::shared_registry::get::<PendingRecipeConfirmation>(
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

    /// 处理用户确认
    pub async fn process_confirmation(
        &self,
        confirmation: UserConfirmation,
    ) -> Result<AgentResponse, String> {
        // PostgreSQL provides atomic, owner-scoped consumption across replicas.
        // The local map is only a hot cache and is cleared after the shared take.
        let pending = crate::api::tapp_runtime::shared_registry::take_for_subject::<
            PendingRecipeConfirmation,
        >(
            &self.db,
            CONFIRMATION_REGISTRY_NAMESPACE,
            &confirmation.confirmation_id,
            confirmation.user_id,
        )
        .await
        .map_err(|error| format!("Failed to consume confirmation: {error}"))?;
        PENDING_CONFIRMATIONS
            .write()
            .await
            .remove(&confirmation.confirmation_id);

        match pending {
            Some(pending_confirmation) => {
                // 二次校验（防御性）
                if pending_confirmation.user_id != confirmation.user_id {
                    return Ok(AgentResponse {
                        response_type: AgentResponseType::Error,
                        message: response_agent::confirmation_not_found(),
                        data: None,
                        data_display: None,
                        suggestions: response_agent::retry_operation_suggestions(),
                        task: None,
                        confirmation: None,
                        frontend_action: None,
                    });
                }

                if !confirmation.confirmed {
                    return Ok(AgentResponse {
                        response_type: AgentResponseType::Answer,
                        message: response_agent::operation_cancelled(),
                        data: Some(json!({
                            "cancelled": true,
                            "confirmation_id": confirmation.confirmation_id
                        })),
                        data_display: None,
                        suggestions: response_agent::cancel_suggestions(),
                        task: None,
                        confirmation: None,
                        frontend_action: None,
                    });
                }

                if Utc::now() > pending_confirmation.request.expires_at {
                    tracing::info!(
                        confirmation_id = %confirmation.confirmation_id,
                        "[Agent] Confirmation expired, rejecting"
                    );
                    return Ok(AgentResponse {
                        response_type: AgentResponseType::Error,
                        message: response_agent::confirmation_expired(),
                        data: None,
                        data_display: None,
                        suggestions: response_agent::retry_suggestions(),
                        task: None,
                        confirmation: None,
                        frontend_action: None,
                    });
                }

                tracing::info!(
                    confirmation_id = %confirmation.confirmation_id,
                    user_id = pending_confirmation.user_id,
                    "[Agent] User confirmed sensitive operation"
                );

                // Sensitive gating runs before required-parameter prompting in
                // the initial request. After confirmation, ask for any missing
                // values instead of executing a partially specified recipe.
                if let Some(missing_response) = self
                    .check_missing_required_parameters(
                        &pending_confirmation.recipe,
                        &pending_confirmation.planner_output,
                        pending_confirmation.user_id,
                        None,
                    )
                    .await?
                {
                    return Ok(missing_response);
                }

                // 始终以 pending 所有者身份执行（已与 caller 对齐）
                let task_state = self
                    .executor
                    .execute(&pending_confirmation.recipe, pending_confirmation.user_id)
                    .await?;

                let result = self.extract_final_result(&task_state);
                let frontend_action = self.extract_frontend_action(&result);

                // v3 记忆记录（确认后的敏感操作也需要记录）
                {
                    let ok = task_state.status == TaskStatus::Completed;
                    record_execution_memory(MemoryRecordParams {
                        user_id: pending_confirmation.user_id,
                        user_input: &pending_confirmation.recipe.name,
                        recipe: &pending_confirmation.recipe,
                        planner_steps_len: pending_confirmation.recipe.steps.len(),
                        success: ok,
                        error_msg: task_state.error.as_deref(),
                        log_prefix: "confirmed:",
                        conversation_context: None,
                        step_results: Some(&task_state.step_results),
                    })
                    .await;
                }

                Ok(AgentResponse {
                    response_type: AgentResponseType::Answer,
                    message: self
                        .generate_response_message_v2(
                            &pending_confirmation.planner_output,
                            &task_state,
                            None,
                        )
                        .await,
                    data: Some(result),
                    data_display: None,
                    suggestions: vec![],
                    task: Some(task_state),
                    confirmation: None,
                    frontend_action,
                })
            }
            None => Ok(AgentResponse {
                response_type: AgentResponseType::Error,
                message: response_agent::confirmation_not_found(),
                data: None,
                data_display: None,
                suggestions: response_agent::retry_operation_suggestions(),
                task: None,
                confirmation: None,
                frontend_action: None,
            }),
        }
    }

    /// 检查配方步骤中是否有必需参数缺失
    /// 如果有缺失参数，创建任务并发送 WaitingForInput 事件让用户补充信息
    async fn check_missing_required_parameters(
        &self,
        recipe: &Recipe,
        _planner_output: &PlannerOutput,
        user_id: i32,
        progress_tx: Option<&tokio::sync::mpsc::Sender<AgentProgressEvent>>,
    ) -> Result<Option<AgentResponse>, String> {
        let missing = collect_missing_required_params(recipe).await;

        if missing.is_empty() {
            return Ok(None);
        }

        tracing::info!(
            missing_count = missing.len(),
            params = ?missing.iter().map(|m| format!("{}:{}", m.step_id, m.param_name)).collect::<Vec<_>>(),
            "[Agent] Missing required parameters, asking user before execution"
        );

        // 创建一个任务来持有 WaitingForInput 状态
        let mut task_state = types::TaskState::new(recipe);
        task_state.status = types::TaskStatus::WaitingForInput;

        // 按参数逐个提问（结构化 question_id = pre_param:{step_id}:{param_name}），
        // 其余进入 pending_questions，resume 时写回 Recipe 后再问下一个
        let question_expires = Some(chrono::Utc::now() + chrono::Duration::minutes(30));
        let mut questions: Vec<types::UserQuestion> = missing
            .iter()
            .map(|m| types::UserQuestion {
                question_id: pre_param_question_id(&m.step_id, &m.param_name),
                question_type: types::QuestionType::FreeText,
                question: response_agent::ask_single_param(&m.description),
                context: format!("step={} param={}", m.step_id, m.param_name),
                options: None,
                required: true,
                default_value: None,
                created_at: chrono::Utc::now(),
                expires_at: question_expires,
            })
            .collect();

        let question = questions.remove(0);
        let mut exec_ctx = types::ExecutionContext::from_request_full(
            &recipe.original_request,
            &recipe.name,
            recipe.page_context.clone(),
            recipe.conversation_context.clone(),
        );
        exec_ctx.pending_questions = questions;

        task_state.set_pending_question(question.clone());
        task_state.execution_context = Some(exec_ctx);
        // 保证 resume 时有可变 recipe 可写回参数
        task_state.recipe = Some(recipe.clone());

        // 存储任务等待用户回答
        {
            let mut store = executor::TASK_STORE.write().await;
            store.store(user_id, task_state.clone());
        }
        executor::persist_task_async(user_id, task_state.clone());

        // 发送 SSE 事件（仅 streaming 路径有 progress_tx）
        if let Some(tx) = progress_tx {
            let _ = tx
                .send(AgentProgressEvent::TaskCreated {
                    task_id: task_state.task_id.clone(),
                    message: String::new(),
                    total_steps: recipe.steps.len() as u32,
                    step_descriptions: Vec::new(),
                })
                .await;

            let _ = tx
                .send(AgentProgressEvent::WaitingForInput {
                    task_id: task_state.task_id.clone(),
                    question_id: question.question_id.clone(),
                    question_type: serde_json::to_value(&question.question_type)
                        .ok()
                        .and_then(|v| v.as_str().map(String::from))
                        .unwrap_or_else(|| "free_text".to_string()),
                    question: question.question.clone(),
                    context: None,
                    options: None,
                    required: question.required,
                    default_value: None,
                })
                .await;
        }

        // 构建响应 — task 就是 TaskState，前端通过 SSE 得到 WaitingForInput
        Ok(Some(AgentResponse {
            response_type: AgentResponseType::TaskCompleted,
            message: String::new(),
            data: None,
            data_display: None,
            suggestions: vec![],
            task: Some(task_state),
            confirmation: None,
            frontend_action: None,
        }))
    }

    /// 检查配方中的敏感步骤
    /// 系统任务对敏感步骤的自动确认门控
    ///
    /// 无人值守场景（Heartbeat 定时任务）等待人工确认只会让任务静默空跑，因此：
    /// - Critical（系统级敏感操作）：拒绝执行，返回说明性响应
    /// - 其余等级：自动确认放行并留痕
    ///
    /// 返回 `None` = 非系统用户，走正常确认流程；
    /// `Some(Ok(()))` = 已自动确认，继续执行；
    /// `Some(Err(response))` = 被拒绝，直接返回该响应。
    fn system_sensitive_gate(
        user_id: i32,
        sensitive_steps: &[PendingConfirmation],
    ) -> Option<Result<(), AgentResponse>> {
        if user_id != SYSTEM_USER_ID {
            return None;
        }
        if let Some(critical) = sensitive_steps
            .iter()
            .find(|s| s.risk_level == RiskLevel::Critical)
        {
            let msg = format!(
                "定时任务包含系统级敏感操作 '{}'（{}），已拒绝自动执行。请手动操作或调整任务指令。",
                critical.capability_name, critical.capability_id
            );
            tracing::warn!(
                capability = %critical.capability_id,
                "[Agent] System task blocked: critical operation requires human confirmation"
            );
            return Some(Err(AgentResponse {
                response_type: AgentResponseType::Answer,
                message: msg.clone(),
                data: Some(json!({ "blocked": true, "reason": msg })),
                data_display: None,
                suggestions: vec![],
                task: None,
                confirmation: None,
                frontend_action: None,
            }));
        }
        tracing::info!(
            steps = %sensitive_steps
                .iter()
                .map(|s| s.capability_id.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            "[Agent] System task auto-confirmed sensitive steps"
        );
        Some(Ok(()))
    }

    async fn check_sensitive_steps(&self, recipe: &Recipe) -> Vec<PendingConfirmation> {
        let mut sensitive = Vec::new();

        for step in &recipe.steps {
            // 使用异步版本，可以从 Capability 结构体或静态配置获取
            if let Some((message, risk_level)) =
                capability::capability_requires_confirmation_async(&step.capability_id).await
            {
                let definition = capability::get_capability_by_id(&step.capability_id).await;
                let capability_name = definition
                    .as_ref()
                    .map(|capability| capability.name.clone())
                    .unwrap_or_else(|| step.capability_id.clone());

                let description = definition
                    .map(|capability| capability.description)
                    .unwrap_or_default();

                // 生成影响说明
                let impact = self.generate_impact_description(step, &risk_level);

                sensitive.push(PendingConfirmation {
                    step_id: step.id.clone(),
                    capability_id: step.capability_id.clone(),
                    capability_name,
                    description,
                    risk_level,
                    confirmation_message: message,
                    impact,
                });
            }
        }

        sensitive
    }

    /// 生成操作影响说明
    fn generate_impact_description(
        &self,
        step: &RecipeStep,
        risk_level: &RiskLevel,
    ) -> Vec<String> {
        let level = match risk_level {
            RiskLevel::Critical => "critical",
            RiskLevel::High => "high",
            RiskLevel::Medium => "medium",
            RiskLevel::Low => "low",
            RiskLevel::None => "none",
        };
        let mut impact: Vec<String> = response_agent::risk_impact(level);

        // 添加具体参数信息
        if let Some(platform) = step.params.get("platform") {
            impact.push(response_agent::target_platform(&platform.to_string()));
        }
        if let Some(url) = step.params.get("url") {
            impact.push(response_agent::target_url(&url.to_string()));
        }

        impact
    }

    /// 请求用户确认（使用 PlannerOutput）
    async fn request_confirmation_v2(
        &self,
        recipe: &Recipe,
        planner_output: &PlannerOutput,
        user_id: i32,
        sensitive_steps: Vec<PendingConfirmation>,
        session_id: Option<String>,
        run_id: Option<String>,
    ) -> Result<AgentResponse, String> {
        let confirmation_id = uuid::Uuid::new_v4().to_string();

        // 确定最高风险等级
        let max_risk = sensitive_steps
            .iter()
            .map(|s| &s.risk_level)
            .max_by_key(|r| match r {
                RiskLevel::Critical => 4,
                RiskLevel::High => 3,
                RiskLevel::Medium => 2,
                RiskLevel::Low => 1,
                RiskLevel::None => 0,
            })
            .cloned()
            .unwrap_or(RiskLevel::None);

        // 过期时间：高风险 5 分钟，其他 15 分钟
        let expires_in = match max_risk {
            RiskLevel::Critical | RiskLevel::High => Duration::minutes(5),
            _ => Duration::minutes(15),
        };

        let confirmation_request = ConfirmationRequest {
            confirmation_id: confirmation_id.clone(),
            recipe_id: recipe.id.clone(),
            pending_steps: sensitive_steps,
            expires_at: Utc::now() + expires_in,
        };

        let pending = PendingRecipeConfirmation {
            request: confirmation_request.clone(),
            recipe: recipe.clone(),
            user_id,
            planner_output: planner_output.clone(),
            session_id: session_id.filter(|s| !s.is_empty()),
            run_id: run_id.filter(|s| !s.is_empty()),
        };
        crate::api::tapp_runtime::shared_registry::put(
            &self.db,
            CONFIRMATION_REGISTRY_NAMESPACE,
            &confirmation_id,
            crate::api::tapp_runtime::shared_registry::RegistryIdentity {
                subject_id: Some(user_id),
                owner_id: Some(user_id),
                tapp_id: None,
                runtime_id: None,
            },
            &pending,
            confirmation_request.expires_at.timestamp(),
        )
        .await
        .map_err(|error| format!("Failed to persist confirmation: {error}"))?;
        PENDING_CONFIRMATIONS
            .write()
            .await
            .insert(confirmation_id.clone(), pending);

        // 生成确认消息
        let message = self.generate_confirmation_message(&confirmation_request, &max_risk);

        tracing::info!(
            confirmation_id = %confirmation_id,
            risk_level = ?max_risk,
            steps = confirmation_request.pending_steps.len(),
            "[Agent] Requesting user confirmation for sensitive operation"
        );

        Ok(AgentResponse {
            response_type: AgentResponseType::ConfirmationRequired,
            message,
            data: None,
            data_display: None,
            suggestions: response_agent::confirmation_suggestions(),
            task: None,
            confirmation: Some(confirmation_request),
            frontend_action: None,
        })
    }

    /// 生成确认提示消息
    fn generate_confirmation_message(
        &self,
        request: &ConfirmationRequest,
        risk_level: &RiskLevel,
    ) -> String {
        let prefix = response_agent::risk_prefix(match risk_level {
            RiskLevel::Critical => "critical",
            RiskLevel::High => "high",
            RiskLevel::Medium => "medium",
            RiskLevel::Low => "low",
            RiskLevel::None => "none",
        });

        let step_names: Vec<_> = request
            .pending_steps
            .iter()
            .map(|s| s.capability_name.as_str())
            .collect();

        let impact_text = request
            .pending_steps
            .iter()
            .map(|s| format!("• {}: {}", s.capability_name, s.confirmation_message))
            .collect::<Vec<_>>()
            .join("\n");

        response_agent::confirmation_dialog(prefix, &step_names.join("、"), &impact_text)
    }

    // NOTE: Old execute_recipe / execute_with_escalation / build_response_from_result
    // removed — escalation is now handled by Planner.replan() in execute_recipe_with_progress_v2

    /// 从步骤构建 Recipe
    fn build_recipe_from_steps(
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

        Recipe {
            id: format!("recipe_{}", uuid::Uuid::new_v4()),
            name,
            original_request: request.raw_input.clone(),
            execution_type: ExecutionType::Instant,
            steps,
            expected_output: OutputFormat::Json,
            estimated_duration_ms,
            created_at: chrono::Utc::now(),
            metadata: HashMap::new(),
            page_context,
            conversation_context,
            lane_key,
        }
    }

    /// 生成响应消息（Planner 版）— 委托给 response_agent
    async fn generate_response_message_v2(
        &self,
        _planner_output: &PlannerOutput,
        task_state: &TaskState,
        progress_tx: Option<&tokio::sync::mpsc::Sender<AgentProgressEvent>>,
    ) -> String {
        if task_state.status == TaskStatus::Failed {
            let err = task_state.error.as_deref().unwrap_or("未知错误");
            return response_agent::error_message(err);
        }

        let result = self.extract_final_result(task_state);

        // 检查 extract_final_result 返回的错误信息
        if let Some(error) = result.get("error").and_then(|v| v.as_str()) {
            if !error.is_empty() {
                return response_agent::execution_error(error);
            }
        }

        // ⭐ 多步骤结果汇总：交给 response_agent AI 流式生成
        let successful_results: Vec<_> = {
            let mut r: Vec<_> = task_state
                .step_results
                .values()
                .filter(|r| r.success)
                .collect();
            r.sort_by_key(|r| &r.step_id);
            r
        };
        if successful_results.len() > 1 {
            let step_outputs: Vec<response_agent::StepOutput<'_>> = successful_results
                .iter()
                .filter_map(|r| {
                    r.output.as_ref().map(|o| response_agent::StepOutput {
                        step_id: &r.step_id,
                        output: o,
                    })
                })
                .collect();

            if !step_outputs.is_empty() {
                let user_request = task_state
                    .recipe
                    .as_ref()
                    .map(|r| r.original_request.as_str())
                    .unwrap_or("");
                let ctx = response_agent::ResponseContext {
                    user_request,
                    step_outputs,
                    progress_tx,
                };
                return response_agent::generate_final_response(ctx).await;
            }
        }

        // 单步骤：委托 response_agent 提取有意义的回复
        if let Some(msg) = response_agent::generate_single_step_response(&result) {
            return msg;
        }

        // 检查是否有部分步骤失败
        let total_steps = task_state.step_results.len();
        let failed_steps: Vec<_> = task_state
            .step_results
            .values()
            .filter(|r| !r.success)
            .collect();
        if !failed_steps.is_empty() && failed_steps.len() < total_steps {
            let success_count = total_steps - failed_steps.len();
            let fail_info: Vec<String> = failed_steps
                .iter()
                .filter_map(|r| r.error.clone())
                .collect();
            return response_agent::partial_completion(success_count, total_steps, &fail_info);
        }

        response_agent::completion_message()
    }

    /// 智能推断数据展示类型（Planner 版）
    fn infer_data_display_v2(
        &self,
        data: &Value,
        _planner_output: &PlannerOutput,
    ) -> Option<DataDisplayHint> {
        // 复用现有的数据结构推断逻辑，但不依赖 ParsedIntent
        match data {
            Value::Array(arr) if !arr.is_empty() => {
                if let Some(Value::Object(obj)) = arr.first() {
                    let fields: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();

                    // 时间线数据
                    if fields
                        .iter()
                        .any(|f| f.contains("time") || f.contains("date") || f.contains("created"))
                        && fields.iter().any(|f| {
                            f.contains("title") || f.contains("content") || f.contains("message")
                        })
                    {
                        let time_field = fields
                            .iter()
                            .find(|f| {
                                f.contains("time") || f.contains("date") || f.contains("created")
                            })
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "time".to_string());
                        let content_field = fields
                            .iter()
                            .find(|f| {
                                f.contains("title") || f.contains("content") || f.contains("name")
                            })
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "content".to_string());
                        return Some(DataDisplayHint::Timeline {
                            time_field,
                            content_field,
                        });
                    }

                    // 卡片列表
                    if fields
                        .iter()
                        .any(|f| f.contains("title") || f.contains("name"))
                    {
                        let title_field = fields
                            .iter()
                            .find(|f| f.contains("title") || f.contains("name"))
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "title".to_string());
                        let description_field = fields
                            .iter()
                            .find(|f| {
                                f.contains("desc") || f.contains("summary") || f.contains("content")
                            })
                            .map(|s| s.to_string());
                        let image_field = fields
                            .iter()
                            .find(|f| {
                                f.contains("image")
                                    || f.contains("cover")
                                    || f.contains("thumbnail")
                            })
                            .map(|s| s.to_string());
                        return Some(DataDisplayHint::CardList {
                            title_field,
                            description_field,
                            image_field,
                        });
                    }

                    // 默认表格
                    let columns: Vec<ColumnDef> = fields
                        .iter()
                        .take(6)
                        .map(|f| ColumnDef {
                            field: f.to_string(),
                            title: humanize_field_name(f),
                            width: None,
                            sortable: true,
                        })
                        .collect();
                    return Some(DataDisplayHint::Table {
                        columns,
                        data_path: None,
                    });
                }
            }
            Value::Object(obj) => {
                if obj.contains_key("aiSummary")
                    || obj.contains_key("analysis")
                    || obj.contains_key("summary")
                {
                    return Some(DataDisplayHint::Markdown);
                }
                if obj.contains_key("source") && obj.contains_key("results") {
                    if let Some(Value::Array(results)) = obj.get("results") {
                        if !results.is_empty() && results.len() > 1 {
                            return Some(DataDisplayHint::CardList {
                                title_field: "name".to_string(),
                                description_field: Some("description".to_string()),
                                image_field: None,
                            });
                        }
                    }
                }
                // 内嵌数组
                for (key, value) in obj.iter() {
                    if let Value::Array(arr) = value {
                        if !arr.is_empty() {
                            if let Some(Value::Object(inner)) = arr.first() {
                                let inner_fields: Vec<&str> =
                                    inner.keys().map(|k| k.as_str()).collect();
                                let columns: Vec<ColumnDef> = inner_fields
                                    .iter()
                                    .take(6)
                                    .map(|f| ColumnDef {
                                        field: f.to_string(),
                                        title: humanize_field_name(f),
                                        width: None,
                                        sortable: true,
                                    })
                                    .collect();
                                return Some(DataDisplayHint::Table {
                                    columns,
                                    data_path: Some(key.clone()),
                                });
                            }
                        }
                    }
                }
                if obj.contains_key("markdown") || obj.contains_key("content") {
                    if let Some(Value::String(s)) =
                        obj.get("markdown").or_else(|| obj.get("content"))
                    {
                        if s.contains('#') || s.contains('*') || s.contains('`') {
                            return Some(DataDisplayHint::Markdown);
                        }
                    }
                }
                if obj.contains_key("chartData") || obj.contains_key("series") {
                    return Some(DataDisplayHint::Chart {
                        chart_type: ChartType::Line,
                        x_field: "x".to_string(),
                        y_field: "y".to_string(),
                    });
                }
                if obj.len() <= 10 {
                    return Some(DataDisplayHint::KeyValue);
                }
            }
            Value::String(s)
                if s.contains('#') || s.contains('*') || s.contains('`') || s.contains('\n') =>
            {
                return Some(DataDisplayHint::Markdown);
            }
            _ => {}
        }
        None
    }

    /// 获取任务状态（带所有权校验，防止 IDOR）
    pub async fn get_task_for_user(&self, task_id: &str, user_id: i32) -> Option<TaskState> {
        executor::get_task_for_user(task_id, user_id).await
    }

    /// 取消任务（带所有权校验）
    pub async fn cancel_task_for_user(&self, task_id: &str, user_id: i32) -> bool {
        executor::cancel_task_for_user(task_id, user_id).await
    }

    /// 获取用户的所有任务
    pub async fn get_user_tasks(&self, user_id: i32) -> Vec<TaskState> {
        executor::get_user_tasks(user_id).await
    }

    /// 恢复 WaitingForInput 任务执行
    pub async fn resume_task(
        &self,
        task_id: &str,
        answer: UserAnswer,
        user_id: i32,
    ) -> Result<AgentResponse, String> {
        // 获取任务并验证所有权
        let task = executor::get_task_for_user(task_id, user_id)
            .await
            .ok_or("Task not found or access denied")?;

        if task.status != TaskStatus::WaitingForInput {
            return Err("Task is not waiting for input".to_string());
        }

        // 从 task_state 中取出保存的 recipe
        let recipe = task.recipe.as_ref().ok_or(
            "Recipe not available for resume (task may have been loaded from DB after restart)",
        )?;

        let task_state = self
            .executor
            .resume_with_answer(task_id, answer, recipe, user_id, None)
            .await?;

        // 记忆记录（仅在任务达到终态时）
        if task_state.status == TaskStatus::Completed || task_state.status == TaskStatus::Failed {
            record_execution_memory(MemoryRecordParams {
                user_id,
                user_input: &recipe.original_request,
                recipe,
                planner_steps_len: task_state.step_results.len(),
                success: task_state.status == TaskStatus::Completed,
                error_msg: task_state.error.as_deref(),
                log_prefix: "resume:",
                conversation_context: None,
                step_results: Some(&task_state.step_results),
            })
            .await;
        }

        // 提取结果
        let result = self.extract_final_result(&task_state);
        let frontend_action = self.extract_frontend_action(&result);

        Ok(AgentResponse {
            response_type: if task_state.status == TaskStatus::Failed {
                AgentResponseType::Error
            } else {
                AgentResponseType::Answer
            },
            message: if task_state.status == TaskStatus::Failed {
                response_agent::error_message(task_state.error.as_deref().unwrap_or("未知错误"))
            } else {
                response_agent::completion_message()
            },
            data: Some(result),
            data_display: None,
            suggestions: vec![],
            task: Some(task_state),
            confirmation: None,
            frontend_action,
        })
    }

    /// 恢复 WaitingForInput 任务执行（带 SSE 进度流）
    pub async fn resume_task_with_progress(
        &self,
        task_id: &str,
        answer: UserAnswer,
        user_id: i32,
        progress_tx: tokio::sync::mpsc::Sender<types::AgentProgressEvent>,
    ) -> Result<AgentResponse, String> {
        let task = executor::get_task_for_user(task_id, user_id)
            .await
            .ok_or("Task not found or access denied")?;

        if task.status != TaskStatus::WaitingForInput {
            return Err("Task is not waiting for input".to_string());
        }

        let recipe = task
            .recipe
            .as_ref()
            .ok_or("Recipe not available for resume")?;

        let task_state = self
            .executor
            .resume_with_answer(task_id, answer, recipe, user_id, Some(progress_tx))
            .await?;

        // 记忆记录（仅在任务达到终态时）
        if task_state.status == TaskStatus::Completed || task_state.status == TaskStatus::Failed {
            record_execution_memory(MemoryRecordParams {
                user_id,
                user_input: &recipe.original_request,
                recipe,
                planner_steps_len: task_state.step_results.len(),
                success: task_state.status == TaskStatus::Completed,
                error_msg: task_state.error.as_deref(),
                log_prefix: "resume:",
                conversation_context: None,
                step_results: Some(&task_state.step_results),
            })
            .await;
        }

        let result = self.extract_final_result(&task_state);
        let frontend_action = self.extract_frontend_action(&result);

        Ok(AgentResponse {
            response_type: if task_state.status == TaskStatus::Failed {
                AgentResponseType::Error
            } else {
                AgentResponseType::Answer
            },
            message: if task_state.status == TaskStatus::Failed {
                response_agent::error_message(task_state.error.as_deref().unwrap_or("未知错误"))
            } else if task_state.status == TaskStatus::WaitingForInput {
                response_agent::need_more_info()
            } else {
                response_agent::completion_message()
            },
            data: Some(result),
            data_display: None,
            suggestions: vec![],
            task: Some(task_state),
            confirmation: None,
            frontend_action,
        })
    }

    /// 真正的流式 Chat 回复：使用 analyze_stream 从 AI 模型逐 token 输出
    ///
    /// 构建包含人格 + 对话历史的 prompt，调用流式 AI 接口，
    /// 每个 token 实时推送给前端。AI 不可用时回退到模拟流式。
    async fn stream_chat_response(
        &self,
        request: &UserRequest,
        planner_reply: &str,
        progress_tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>,
    ) -> String {
        use crate::config::ModelTier;
        use crate::services::ai::create_ai_analyzer_for_tier;

        let analyzer = match create_ai_analyzer_for_tier(ModelTier::Standard).await {
            Some(a) => a,
            None => {
                // AI 不可用，回退到模拟流式
                Self::stream_text_as_tokens(progress_tx, planner_reply).await;
                return planner_reply.to_string();
            }
        };

        // 加载 Agent 人格
        let soul = identity::get_identity()
            .await
            .and_then(|id| id.soul)
            .unwrap_or_default();
        let soul: String = soul.chars().take(2000).collect();

        // 构建对话历史
        let history_text = request
            .context
            .as_ref()
            .and_then(|c| c.conversation_history.as_ref())
            .map(|history| {
                let recent: Vec<_> = history
                    .iter()
                    .rev()
                    .take(10)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                recent
                    .iter()
                    .map(|msg| format!("{}：{}", msg.role, msg.content))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();

        let prompt = if history_text.is_empty() {
            format!(
                "{soul}\n\n用户对你说：{input}\n\n\
                 请以你的角色自然地回复用户。使用用户的语言。保持简短、温暖、自然。\
                 不要输出任何 JSON 或格式标记，只输出纯文本回复。",
                soul = soul,
                input = request.raw_input,
            )
        } else {
            format!(
                "{soul}\n\n以下是对话历史：\n{history}\n\n\
                 用户最新消息：{input}\n\n\
                 请以你的角色自然地回复用户。使用用户的语言。保持简短、温暖、自然。\
                 不要输出任何 JSON 或格式标记，只输出纯文本回复。",
                soul = soul,
                history = history_text,
                input = request.raw_input,
            )
        };

        let tx = progress_tx.clone();
        match analyzer
            .analyze_stream(&prompt, |token| {
                let _ = tx.try_send(AgentProgressEvent::SummaryToken {
                    token: token.to_string(),
                    done: false,
                });
                true
            })
            .await
        {
            Ok(full_text) if !full_text.trim().is_empty() => {
                let _ = tx.try_send(AgentProgressEvent::SummaryToken {
                    token: String::new(),
                    done: true,
                });
                full_text.trim().to_string()
            }
            _ => {
                // 流式失败，回退到 planner 的回复 + 模拟流式
                tracing::warn!(
                    "[Agent] Streaming chat response failed, falling back to planner reply"
                );
                Self::stream_text_as_tokens(progress_tx, planner_reply).await;
                planner_reply.to_string()
            }
        }
    }

    /// 将已有文本分块推送为 SummaryToken 事件，模拟流式输出
    ///
    /// 将文本按句/标点拆分为自然片段，逐个发送给前端，
    /// 让用户看到"AI 在打字"的效果而非一次性出现全部内容。
    async fn stream_text_as_tokens(tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>, text: &str) {
        // 按自然断点切分（标点、换行）
        let mut chunks = Vec::new();
        let mut current = String::new();
        for ch in text.chars() {
            current.push(ch);
            // 在句号、逗号、换行、感叹号、问号等处断开
            if matches!(
                ch,
                '。' | '，' | '！' | '？' | '\n' | '；' | '：' | '.' | ',' | '!' | '?' | ';' | ':'
            ) || current.len() > 40
            {
                chunks.push(std::mem::take(&mut current));
            }
        }
        if !current.is_empty() {
            chunks.push(current);
        }

        for chunk in &chunks {
            let _ = tx
                .send(AgentProgressEvent::SummaryToken {
                    token: chunk.clone(),
                    done: false,
                })
                .await;
            // 极短延迟让前端有时间渲染，避免所有 token 在同一帧到达
            tokio::time::sleep(tokio::time::Duration::from_millis(15)).await;
        }
        // 发送完成标记
        let _ = tx
            .send(AgentProgressEvent::SummaryToken {
                token: String::new(),
                done: true,
            })
            .await;
    }

    /// 提取任务最终结果
    /// 改进：对于多步骤任务，合并所有相关结果
    fn extract_final_result(&self, task_state: &TaskState) -> serde_json::Value {
        // 找到所有成功的步骤结果
        let mut results: Vec<_> = task_state
            .step_results
            .values()
            .filter(|r| r.success)
            .collect();

        results.sort_by_key(|r| &r.step_id);

        // 如果没有成功的步骤，返回失败信息
        if results.is_empty() {
            let errors: Vec<String> = task_state
                .step_results
                .values()
                .filter_map(|r| r.error.clone())
                .collect();
            let error_msg = if errors.is_empty() {
                response_agent::not_executed()
            } else {
                errors.join("; ")
            };
            return json!({
                "status": format!("{:?}", task_state.status),
                "error": error_msg
            });
        }

        // 如果只有一个结果，直接返回
        if results.len() <= 1 {
            return results
                .last()
                .and_then(|r| r.output.clone())
                .unwrap_or(json!({
                    "status": format!("{:?}", task_state.status),
                    "progress": task_state.progress
                }));
        }

        // ⭐ 关键改进：收集所有步骤中的 frontendAction 和 action
        let mut all_frontend_actions: Vec<Value> = Vec::new();
        for result in &results {
            if let Some(output) = &result.output {
                // 检查 frontendAction
                if let Some(action) = output.get("frontendAction") {
                    all_frontend_actions.push(action.clone());
                    tracing::info!(
                        step_id = %result.step_id,
                        action_type = ?action.get("type"),
                        "[Agent] Collected frontendAction from step"
                    );
                }
                // 🔴 也检查 action 字段（兼容 brew.generateReadingList 等）
                if let Some(action) = output.get("action") {
                    all_frontend_actions.push(action.clone());
                    tracing::info!(
                        step_id = %result.step_id,
                        action_type = ?action.get("type"),
                        "[Agent] Collected action from step"
                    );
                }
            }
        }

        // 多步骤结果：检查是否有分析/总结类型的最终结果
        let last_result = results.last().and_then(|r| r.output.as_ref());

        // 如果最后一步是分析/总结，检查是否有实际内容
        if let Some(last) = last_result {
            // 检查是否是 AI 分析结果
            if let Some(analysis) = last.get("analysis").and_then(|a| a.as_str()) {
                if !analysis.is_empty() {
                    // 合并搜索结果和分析结果
                    let mut combined = json!({
                        "analysis": analysis,
                        "type": last.get("type").and_then(|t| t.as_str()).unwrap_or("general")
                    });

                    // 收集所有搜索步骤的来源信息
                    let mut sources = Vec::new();
                    for result in &results {
                        if let Some(output) = &result.output {
                            // 检查是否是联网搜索结果
                            if output.get("source").is_some() {
                                if let Some(query) = output.get("query").and_then(|q| q.as_str()) {
                                    sources.push(json!({
                                        "query": query,
                                        "source": output.get("source")
                                    }));
                                }
                            }
                            // 检查是否有 aiSummary
                            if let Some(summary) = output.get("aiSummary").and_then(|s| s.as_str())
                            {
                                if !summary.is_empty() && combined.get("searchSummary").is_none() {
                                    combined["searchSummary"] = json!(summary);
                                }
                            }
                        }
                    }

                    if !sources.is_empty() {
                        combined["sources"] = json!(sources);
                    }

                    // ⭐ 添加所有收集到的 frontendActions
                    if !all_frontend_actions.is_empty() {
                        combined["frontendActions"] = json!(all_frontend_actions);
                    }

                    return combined;
                }
            }

            // 检查是否是 AI 总结结果
            if let Some(summary) = last.get("summary").and_then(|s| s.as_str()) {
                if !summary.is_empty() {
                    let mut result = last.clone();
                    // ⭐ 添加所有收集到的 frontendActions
                    if !all_frontend_actions.is_empty() {
                        result["frontendActions"] = json!(all_frontend_actions);
                        tracing::info!(
                            count = all_frontend_actions.len(),
                            "[Agent] Merged {} frontendActions into summary result",
                            all_frontend_actions.len()
                        );
                    }
                    return result;
                }
            }
        }

        // 默认返回最后一个结果
        let mut final_result = results
            .last()
            .and_then(|r| r.output.clone())
            .unwrap_or(json!({
                "status": format!("{:?}", task_state.status),
                "progress": task_state.progress
            }));

        // ⭐ 添加所有收集到的 frontendActions
        if !all_frontend_actions.is_empty() {
            final_result["frontendActions"] = json!(all_frontend_actions);
            tracing::info!(
                count = all_frontend_actions.len(),
                "[Agent] Merged {} frontendActions into default final result",
                all_frontend_actions.len()
            );
        }

        final_result
    }

    /// 从执行结果中提取前端动作
    fn extract_frontend_action(&self, result: &Value) -> Option<Value> {
        // 检查 frontendAction（单个）
        if let Some(action) = result.get("frontendAction") {
            if action.get("type").and_then(Value::as_str).is_some() {
                return Some(action.clone());
            }
            tracing::warn!(action = %action, "[Agent] frontendAction is missing type");
        }

        // 🔴 也检查 "action" 字段（兼容 brew.generateReadingList 等返回格式）
        if let Some(action) = result.get("action") {
            tracing::debug!(
                action = %action,
                "[Agent] Found action in result, attempting to deserialize"
            );
            if action.get("type").and_then(Value::as_str).is_some() {
                let mut final_action = action.clone();
                if final_action.get("criteria").is_none() {
                    if let Some(criteria) = result.get("criteria") {
                        final_action["criteria"] = criteria.clone();
                    }
                }
                return Some(final_action);
            }
        }

        // 检查 frontendActions（数组）- 返回第一个
        if let Some(actions) = result.get("frontendActions").and_then(|v| v.as_array()) {
            if let Some(first_action) = actions.first() {
                if first_action.get("type").and_then(Value::as_str).is_some() {
                    return Some(first_action.clone());
                }
            }
        }

        // 检查 plan.steps（AI 分析生成的步骤）- 如果 autoExecute 或需要自动执行
        if let Some(plan) = result.get("plan") {
            if let (Some(true), Some(steps)) = (
                plan.get("canFulfill").and_then(|v| v.as_bool()),
                plan.get("steps").and_then(|v| v.as_array()),
            ) {
                if let Some(first_step) = steps.first() {
                    let action_type = first_step
                        .get("actionType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("click");

                    let timestamp = chrono::Utc::now().timestamp_millis();

                    return match action_type {
                        "navigate" => {
                            let path = first_step
                                .get("path")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());

                            Some(json!({
                                "type": "navigate",
                                "path": path,
                                "timestamp": timestamp,
                            }))
                        }
                        _ => Some(json!({
                            "type": "page_interact",
                            "target": first_step.get("target").cloned(),
                            "action": action_type,
                            "timestamp": timestamp,
                        })),
                    };
                }
            }
        }

        None
    }
}

/// 记录执行记忆的通用参数
struct MemoryRecordParams<'a> {
    user_id: i32,
    user_input: &'a str,
    recipe: &'a Recipe,
    planner_steps_len: usize,
    success: bool,
    error_msg: Option<&'a str>,
    /// 日志前缀（区分来源: "", "saved:", "confirmed:"）
    log_prefix: &'a str,
    /// 是否记录会话归档（仅完整 process 流程需要）
    conversation_context: Option<&'a [ConversationMessage]>,
    /// 实际步骤执行结果（用于丰富记忆提取的上下文）
    step_results: Option<&'a std::collections::HashMap<String, types::StepResult>>,
}

/// 统一的执行后记忆记录
///
/// 提取自 process / process_with_progress / execute_saved_recipe /
/// execute_simple_query_v2 / process_confirmation 中的重复逻辑。
async fn record_execution_memory(params: MemoryRecordParams<'_>) {
    let Some(mem) = memory::get_memory() else {
        return;
    };

    let ok = params.success;
    let step_caps: Vec<String> = params
        .recipe
        .steps
        .iter()
        .map(|s| s.capability_id.clone())
        .collect();

    // 1. AI 提取多维度记忆
    let exec_results: std::collections::HashMap<String, serde_json::Value> = params
        .recipe
        .steps
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let step_key = format!("step_{}", i + 1);
            // 从实际 step_results 中查找对应输出摘要
            let output_summary = params
                .step_results
                .and_then(|sr| sr.get(&s.id))
                .and_then(|r| r.output.as_ref())
                .map(memory::summarize_value_for_memory);
            let step_success = params
                .step_results
                .and_then(|sr| sr.get(&s.id))
                .map(|r| r.success)
                .unwrap_or(ok);
            let mut val = serde_json::json!({
                "action": s.action,
                "capability": s.capability_id,
                "success": step_success,
            });
            if let Some(summary) = output_summary {
                val["output_summary"] = serde_json::Value::String(summary);
            }
            if let Some(err) = params
                .step_results
                .and_then(|sr| sr.get(&s.id))
                .and_then(|r| r.error.as_ref())
            {
                val["error"] = serde_json::Value::String(err.clone());
            }
            (step_key, val)
        })
        .collect();
    // 将会话上下文转换为 Value 格式供记忆提取 AI 使用
    let conversation_values: Option<Vec<serde_json::Value>> =
        params.conversation_context.map(|msgs| {
            msgs.iter()
                .filter_map(|m| serde_json::to_value(m).ok())
                .collect()
        });
    mem.extract_memories_from_execution(
        params.user_input,
        conversation_values.as_deref(),
        &exec_results,
        ok,
        &step_caps,
        params.user_id,
    )
    .await;

    // 2. 失败教训
    if !ok {
        let error = params.error_msg.unwrap_or("unknown");
        let lesson = format!(
            "{}执行失败教训：{} → 步骤 [{}] 失败: {}",
            params.log_prefix,
            params.user_input.chars().take(40).collect::<String>(),
            step_caps.join(", "),
            error
        );
        mem.remember_full(
            &lesson,
            memory::MemoryType::ExecutionLesson,
            memory::MemoryTier::MediumTerm,
            0.8,
            Vec::new(),
            step_caps.clone(),
            params.user_id,
        )
        .await;
    }

    // 3. 日志
    let first_cap = params
        .recipe
        .steps
        .first()
        .map(|s| s.capability_id.as_str())
        .unwrap_or("?");
    let summary = format!(
        "{}{} | {} 步 ({}, ...) → {}",
        params.log_prefix,
        params.user_input.chars().take(30).collect::<String>(),
        params.planner_steps_len,
        first_cap,
        if ok { "✓" } else { "✗" }
    );
    mem.log_daily(params.user_id, &summary).await;

    // 4. 会话摘要归档
    if let Some(history) = params.conversation_context {
        if history.len() >= 4 {
            let session_summary = format!(
                "会话主题：{} | 执行了 {} 步骤 | 结果：{}",
                params.user_input.chars().take(50).collect::<String>(),
                params.planner_steps_len,
                if ok { "成功" } else { "失败" }
            );
            mem.consolidate_session(&session_summary, params.user_id)
                .await;
        }
    }

    // 5. 提升 + 清理
    mem.promote_memories().await;
    mem.cleanup_short_term().await;
}

/// 获取系统能力摘要
pub async fn get_capabilities_summary() -> serde_json::Value {
    capability::get_capability_summary().await
}

/// Query the current database role. Agent recipes can execute long after a
/// token was issued, so a hard-coded "first user is admin" rule is unsafe.
pub(crate) async fn user_is_current_admin(db: &sea_orm::DatabaseConnection, user_id: i32) -> bool {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    if user_id == 0 {
        return true;
    }

    match db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT is_admin FROM users WHERE id = $1 LIMIT 1",
            [user_id.into()],
        ))
        .await
    {
        Ok(Some(row)) => row.try_get::<bool>("", "is_admin").unwrap_or(false),
        Ok(None) => false,
        Err(error) => {
            tracing::warn!(
                user_id,
                %error,
                "[Agent] Failed to refresh current user role; using least privilege"
            );
            false
        }
    }
}

/// Agent 能力预设（与 Tapp 权限页「开关模板」对应；运行时以 Tapp 开关为准）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // 预设档位名 / 单测映射
pub enum AgentUsageMode {
    /// 禁用（Agent 相关 elevated 全关）
    None,
    /// 仅 AI 对话/分析
    Chat,
    /// 标准（平台/共享 Brew 只读 + AI；无出站）
    Standard,
    /// 扩展（标准 + 出站抓取 + 调度 + 个人 Tapp 写）
    Elevated,
}

impl AgentUsageMode {
    #[allow(dead_code)]
    pub fn parse(s: &str) -> Self {
        match s {
            "chat" => Self::Chat,
            "standard" => Self::Standard,
            "elevated" => Self::Elevated,
            _ => Self::None,
        }
    }
}

/// 某预设对应的 Agent 权限串（不含 brew:write / report:write）
fn permissions_for_usage_mode(mode: AgentUsageMode) -> std::collections::HashSet<String> {
    use std::collections::HashSet;
    let mut perms = HashSet::new();
    match mode {
        AgentUsageMode::None => {}
        AgentUsageMode::Chat => {
            for p in &["ai:chat", "ai:analyze", "system:read"] {
                perms.insert((*p).to_string());
            }
        }
        // 共享订阅库：普通用户永不授予 brew:write（加/改/删源仅管理员）
        AgentUsageMode::Standard | AgentUsageMode::Elevated => {
            for p in &[
                "platform:read",
                "steam:read",
                "bilibili:read",
                "bangumi:read",
                "github:read",
                "netease:read",
                "ai:analyze",
                "ai:chat",
                "ai:search",
                "ai:image",
                "brew:read", // 读共享库 + 个人已读/收藏（brew.mark）
                "report:read",
                "tapp:read",
                "system:read",
            ] {
                perms.insert((*p).to_string());
            }
            if mode == AgentUsageMode::Elevated {
                // 扩展：出站 + 调度 + 仅自己的 Tapp；报告生成仅管理员
                for p in &[
                    "http:fetch",
                    "web:scrape",
                    "tapp:write",
                    "weather:read",
                    "metadata:read",
                    "proxy:read",
                    "scheduler:read",
                    "scheduler:write",
                ] {
                    perms.insert((*p).to_string());
                }
            }
        }
    }
    perms
}

/// 非管理员 Agent 能力候选全集（再经 Tapp 开关过滤）
fn max_user_agent_permissions() -> std::collections::HashSet<String> {
    permissions_for_usage_mode(AgentUsageMode::Elevated)
}

/// 校验当前用户是否允许使用 Agent（页面可见性 + Tapp `ai:chat`）
///
/// 返回 Ok(is_admin)；禁用时返回 403 语义错误字符串
pub async fn ensure_agent_usage_allowed(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> Result<bool, String> {
    use crate::services::permission_service::{TappPermission, TappPermissionService, UserRole};

    let is_admin = user_is_current_admin(db, user_id).await;
    if is_admin || user_id == SYSTEM_USER_ID {
        return Ok(true);
    }

    let prefs = crate::api::config::load_module_visibility_preferences_for_agent(db).await;
    let visibility = prefs.agent_visibility();
    if visibility == "admin" {
        return Err("Agent 仅管理员可用".to_string());
    }

    // 能力真相源：Tapp 权限（设置页预设模板会批量开关这些项）
    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
    if !TappPermissionService::check(&config, UserRole::User, TappPermission::AiChat) {
        return Err(
            "Agent 未对普通用户开放 AI 对话（请在「Tapp 权限管理」中下放 ai:chat 或选用助手预设）"
                .to_string(),
        );
    }
    Ok(false)
}

/// Agent 权限串 → Tapp 权限（强制对齐；未映射的权限非管理员一律拒绝，仅 `system:read` 例外）
///
/// 非管理员能力 = 候选全集 ∩ Tapp 下放开关（与权限页预设模板同一真相源）
fn agent_perm_to_tapp(perm: &str) -> Option<crate::services::permission_service::TappPermission> {
    use crate::services::permission_service::TappPermission;
    match perm {
        // AI（elevated，须在 Tapp 权限管理中下放）
        "ai:chat" => Some(TappPermission::AiChat),
        "ai:analyze" => Some(TappPermission::AiAnalyze),
        "ai:image" => Some(TappPermission::AiImage),
        "ai:search" | "ai:generate" => Some(TappPermission::AiGenerate),
        // 读（basic，默认全员）
        "brew:read" => Some(TappPermission::BrewRead),
        "report:read" => Some(TappPermission::ReportRead),
        "platform:read" | "steam:read" | "bilibili:read" | "bangumi:read" | "github:read"
        | "netease:read" | "weather:read" | "metadata:read" => Some(TappPermission::PlatformRead),
        "tapp:read" => Some(TappPermission::TappListRead),
        // 写 / 出站（elevated 或 privileged）
        "brew:write" => Some(TappPermission::BrewWrite),
        "report:write" => Some(TappPermission::ReportWrite),
        "http:fetch" | "web:scrape" | "proxy:read" => Some(TappPermission::NetworkFetch),
        "scheduler:read" | "scheduler:write" => Some(TappPermission::SchedulerRegister),
        // 个人 Tapp 写：用 storage（basic）表达「可持久化自己的内容」，非 manage 全站
        "tapp:write" => Some(TappPermission::Storage),
        // system:read 无 Tapp 对应，见 retain 特例
        _ => None,
    }
}

/// 获取用户在 Agent 系统中的权限集
///
/// - 管理员 / 系统用户：全部能力权限
/// - 其他：候选全集 ∩ Tapp 权限检查（强制对齐；无独立 agentUsage 天花板）
pub async fn get_user_permissions(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> std::collections::HashSet<String> {
    use crate::services::permission_service::{TappPermissionService, UserRole};
    use std::collections::HashSet;

    // 系统用户或管理员：全部权限
    if user_is_current_admin(db, user_id).await {
        let registry = capability::get_registry().await;
        let mut permissions: HashSet<String> = registry
            .get_all()
            .iter()
            .flat_map(|cap| cap.required_permissions.iter().cloned())
            .collect();
        permissions.insert("mcp:execute".to_string());
        return permissions;
    }

    let prefs = crate::api::config::load_module_visibility_preferences_for_agent(db).await;
    // 可见性 admin-only 时，非管理员无任何 agent 能力
    if prefs.agent_visibility() == "admin" {
        return HashSet::new();
    }

    let mut perms = max_user_agent_permissions();

    // 强制与 Tapp 对齐
    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
    perms.retain(|p| {
        if p == "system:read" {
            return true; // Agent 内部只读元信息，无 Tapp 对应
        }
        match agent_perm_to_tapp(p) {
            Some(tp) => TappPermissionService::check(&config, UserRole::User, tp),
            // 未映射权限：非管理员拒绝（避免旁路）
            None => false,
        }
    });

    perms
}

/// 初始化任务存储的数据库连接
///
/// 应在应用启动时调用，以支持任务持久化和恢复
pub async fn init_task_store(db: DatabaseConnection) {
    executor::init_task_store_db(db).await;
}

/// 清理过期的确认请求
pub async fn cleanup_expired_confirmations() {
    let now = Utc::now();
    let mut store = PENDING_CONFIRMATIONS.write().await;

    let expired: Vec<String> = store
        .iter()
        .filter(|(_, v)| v.request.expires_at < now)
        .map(|(k, _)| k.clone())
        .collect();

    for id in expired {
        tracing::debug!(confirmation_id = %id, "[Agent] Cleaning up expired confirmation");
        store.remove(&id);
    }
}

/// 将字段名转换为用户友好的标题
/// 子任务：AI 生成会话标题（在 tokio::spawn 中调用，与 executor 并行）
async fn generate_session_title_ai(user_input: &str, reasoning: Option<&str>) -> String {
    use crate::config::ModelTier;
    use crate::services::ai::create_ai_analyzer_for_tier;

    let truncated: String = user_input.chars().take(300).collect();

    if let Some(analyzer) = create_ai_analyzer_for_tier(ModelTier::Standard).await {
        let context = if let Some(r) = reasoning {
            format!(
                "User message: {}\nAgent understanding: {}",
                truncated,
                r.chars().take(200).collect::<String>()
            )
        } else {
            truncated.clone()
        };
        let prompt = format!(
            "Based on the following conversation context, generate a concise session title (5-15 characters, in the same language as the user). \
             Return ONLY the title text, no quotes, no explanation.\n\n{}",
            context
        );
        match analyzer.analyze(&prompt).await {
            Ok(raw) => {
                let cleaned = raw
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\u{300c}')
                    .trim_matches('\u{300d}')
                    .trim();
                if cleaned.is_empty() || cleaned.len() > 100 {
                    fallback_title_text(&truncated)
                } else {
                    cleaned.to_string()
                }
            }
            Err(e) => {
                tracing::warn!("[Agent] AI title generation failed: {}", e);
                fallback_title_text(&truncated)
            }
        }
    } else {
        fallback_title_text(&truncated)
    }
}

/// 标题降级：截取用户输入前 50 字符
fn fallback_title_text(input: &str) -> String {
    let count = input.chars().count();
    if count > 50 {
        format!("{}...", input.chars().take(47).collect::<String>())
    } else if count > 0 {
        input.to_string()
    } else {
        "New conversation".to_string()
    }
}

fn humanize_field_name(field: &str) -> String {
    // 常见字段名映射
    let mappings: &[(&str, &str)] = &[
        ("id", "ID"),
        ("title", "标题"),
        ("name", "名称"),
        ("content", "内容"),
        ("description", "描述"),
        ("desc", "描述"),
        ("summary", "摘要"),
        ("time", "时间"),
        ("date", "日期"),
        ("created_at", "创建时间"),
        ("updated_at", "更新时间"),
        ("author", "作者"),
        ("platform", "平台"),
        ("status", "状态"),
        ("progress", "进度"),
        ("count", "数量"),
        ("price", "价格"),
        ("url", "链接"),
        ("image", "图片"),
        ("cover", "封面"),
        ("thumbnail", "缩略图"),
        ("views", "浏览量"),
        ("likes", "点赞数"),
        ("comments", "评论数"),
        ("duration", "时长"),
        ("category", "分类"),
        ("tags", "标签"),
    ];

    // 查找映射
    for (key, label) in mappings {
        if field.to_lowercase() == *key || field.to_lowercase().ends_with(&format!("_{}", key)) {
            return label.to_string();
        }
    }

    // 默认处理：将 snake_case 或 camelCase 转换为空格分隔
    let mut result = String::new();
    for (i, c) in field.chars().enumerate() {
        if c == '_' {
            result.push(' ');
        } else if c.is_uppercase() && i > 0 {
            result.push(' ');
            result.push(c);
        } else if i == 0 {
            result.push(c.to_ascii_uppercase());
        } else {
            result.push(c);
        }
    }
    result
}

// ============ 预执行参数收集 / 写回 ============

/// 缺失的必需参数
#[derive(Debug, Clone, PartialEq, Eq)]
struct MissingRequiredParam {
    step_id: String,
    param_name: String,
    description: String,
}

/// 结构化 question_id，resume 时据此写回 Recipe.step.params
fn pre_param_question_id(step_id: &str, param_name: &str) -> String {
    format!("pre_param:{}:{}", step_id, param_name)
}

/// 解析 `pre_param:{step_id}:{param_name}`；兼容旧格式 `pre_param_{param_name}`
pub(crate) fn parse_pre_param_question_id(question_id: &str) -> Option<(String, String)> {
    if let Some(rest) = question_id.strip_prefix("pre_param:") {
        let mut parts = rest.splitn(2, ':');
        let step_id = parts.next()?.to_string();
        let param_name = parts.next()?.to_string();
        if step_id.is_empty() || param_name.is_empty() {
            return None;
        }
        return Some((step_id, param_name));
    }
    // 兼容旧版 pre_param_{name}（无 step 映射，调用方用第一个匹配步骤）
    if let Some(param_name) = question_id.strip_prefix("pre_param_") {
        if !param_name.is_empty() && param_name != "s" {
            return Some((String::new(), param_name.to_string()));
        }
    }
    None
}

/// 将用户回答写回 Recipe 对应步骤参数
pub(crate) fn apply_pre_param_answer_to_recipe(
    recipe: &mut Recipe,
    question_id: &str,
    answer: &str,
) -> bool {
    let Some((step_id, param_name)) = parse_pre_param_question_id(question_id) else {
        return false;
    };
    let value = serde_json::Value::String(answer.trim().to_string());
    if step_id.is_empty() {
        // 旧格式：写入第一个缺少该参数的步骤，否则第一个步骤
        let idx = recipe
            .steps
            .iter()
            .position(|s| !s.params.contains_key(&param_name))
            .or(if recipe.steps.is_empty() {
                None
            } else {
                Some(0)
            });
        if let Some(i) = idx {
            recipe.steps[i].params.insert(param_name, value);
            return true;
        }
        return false;
    }
    if let Some(step) = recipe.steps.iter_mut().find(|s| s.id == step_id) {
        step.params.insert(param_name, value);
        return true;
    }
    false
}

fn step_has_param_value(step: &types::RecipeStep, param_name: &str) -> bool {
    let has_value = step
        .params
        .get(param_name)
        .map(|v| !v.is_null() && v.as_str().is_none_or(|s| !s.is_empty()))
        .unwrap_or(false);
    let has_from = step.params.contains_key(&format!("{}From", param_name));
    has_value || has_from
}

fn push_missing_from_schema(
    missing: &mut Vec<MissingRequiredParam>,
    step: &types::RecipeStep,
    schema: &Value,
) {
    let Some(required) = schema.get("required").and_then(|v| v.as_array()) else {
        return;
    };
    let properties = schema.get("properties");
    for req_val in required {
        let Some(param_name) = req_val.as_str() else {
            continue;
        };
        if step_has_param_value(step, param_name) {
            continue;
        }
        let description = properties
            .and_then(|p| p.get(param_name))
            .and_then(|p| p.get("description"))
            .and_then(|d| d.as_str())
            .unwrap_or(param_name)
            .to_string();
        missing.push(MissingRequiredParam {
            step_id: step.id.clone(),
            param_name: param_name.to_string(),
            description,
        });
    }
}

/// 收集 Recipe 中缺失的必需参数（静态 registry + 动态 MCP schema + Skill）
async fn collect_missing_required_params(recipe: &Recipe) -> Vec<MissingRequiredParam> {
    let registry = capability::get_registry().await;
    let skill_registry = skill::get_skill_registry();
    let mcp_schemas = load_mcp_tool_schemas().await;
    let mut missing = Vec::new();

    for step in &recipe.steps {
        if let Some(skill_id) = step.capability_id.strip_prefix("skill:") {
            if let Some(skill_reg) = skill_registry {
                if let Some(sk) = skill_reg.get(skill_id).await {
                    for param_name in &sk.parameters {
                        if !step_has_param_value(step, param_name) {
                            missing.push(MissingRequiredParam {
                                step_id: step.id.clone(),
                                param_name: param_name.to_string(),
                                description: param_name.replace('_', " "),
                            });
                        }
                    }
                }
            }
            continue;
        }

        if let Some(cap) = registry.get(&step.capability_id) {
            push_missing_from_schema(&mut missing, step, &cap.input_schema);
            continue;
        }

        // 动态 MCP 工具：不在静态 registry 中，从 MCP manager 读 schema
        if step.capability_id.starts_with("mcp.") {
            if let Some(schema) = mcp_schemas.get(&step.capability_id) {
                push_missing_from_schema(&mut missing, step, schema);
            }
        }
    }

    missing
}

/// capability_id (`mcp.{server}.{tool}`) → input_schema
async fn load_mcp_tool_schemas() -> HashMap<String, Value> {
    let mut map = HashMap::new();
    let Some(manager) = mcp::get_mcp_manager() else {
        return map;
    };
    for (server_id, tool) in manager.list_tools().await {
        let cap_id = format!("mcp.{}.{}", server_id, tool.name);
        map.insert(cap_id, tool.input_schema);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(cap: &str, risk: RiskLevel) -> PendingConfirmation {
        PendingConfirmation {
            step_id: "s1".to_string(),
            capability_id: cap.to_string(),
            capability_name: cap.to_string(),
            description: String::new(),
            risk_level: risk,
            confirmation_message: String::new(),
            impact: Vec::new(),
        }
    }

    #[test]
    fn session_id_is_parsed_from_lane_key() {
        assert_eq!(
            session_id_from_lane_key("user:42:session:ses_abc"),
            Some("ses_abc".to_string())
        );
        assert_eq!(session_id_from_lane_key("user:42"), None);
        assert_eq!(session_id_from_lane_key("user:42:session:"), None);
    }

    #[test]
    fn test_system_gate_normal_user_needs_confirmation() {
        let steps = vec![pending("cache.clear", RiskLevel::High)];
        assert!(
            Agent::system_sensitive_gate(1, &steps).is_none(),
            "普通用户应走正常确认流程"
        );
    }

    #[test]
    fn test_system_gate_auto_confirms_up_to_high() {
        for risk in [RiskLevel::Low, RiskLevel::Medium, RiskLevel::High] {
            let steps = vec![pending("storage.set", risk)];
            match Agent::system_sensitive_gate(SYSTEM_USER_ID, &steps) {
                Some(Ok(())) => {}
                other => panic!(
                    "系统任务应自动确认 {risk:?}，got {:?}",
                    other.map(|r| r.is_ok())
                ),
            }
        }
    }

    #[test]
    fn test_system_gate_blocks_critical() {
        let steps = vec![
            pending("storage.set", RiskLevel::Low),
            pending("system.shutdown", RiskLevel::Critical),
        ];
        match Agent::system_sensitive_gate(SYSTEM_USER_ID, &steps) {
            Some(Err(resp)) => {
                assert!(resp.message.contains("system.shutdown"), "{}", resp.message);
            }
            other => panic!("Critical 应被拒绝，got {:?}", other.map(|r| r.is_ok())),
        }
    }

    #[test]
    fn agent_usage_mode_parse_and_permission_sets() {
        assert_eq!(AgentUsageMode::parse("none"), AgentUsageMode::None);
        assert_eq!(AgentUsageMode::parse("chat"), AgentUsageMode::Chat);
        assert_eq!(AgentUsageMode::parse("standard"), AgentUsageMode::Standard);
        assert_eq!(AgentUsageMode::parse("elevated"), AgentUsageMode::Elevated);
        assert_eq!(AgentUsageMode::parse("bogus"), AgentUsageMode::None);

        let none = permissions_for_usage_mode(AgentUsageMode::None);
        assert!(none.is_empty());

        let chat = permissions_for_usage_mode(AgentUsageMode::Chat);
        assert!(chat.contains("ai:chat"));
        assert!(!chat.contains("brew:write"));
        assert!(!chat.contains("http:fetch"));

        let standard = permissions_for_usage_mode(AgentUsageMode::Standard);
        assert!(standard.contains("brew:read"));
        assert!(!standard.contains("brew:write"));
        assert!(standard.contains("ai:chat"));
        assert!(!standard.contains("http:fetch"));

        let elevated = permissions_for_usage_mode(AgentUsageMode::Elevated);
        assert!(elevated.contains("http:fetch"));
        assert!(elevated.contains("web:scrape"));
        assert!(elevated.contains("tapp:write"));
        assert!(elevated.contains("scheduler:read"));
        assert!(!elevated.contains("brew:write"));
        assert!(!elevated.contains("report:write"));

        let max = max_user_agent_permissions();
        assert_eq!(max, elevated);
    }

    #[test]
    fn agent_perm_to_tapp_force_alignment_map() {
        use crate::services::permission_service::TappPermission;
        assert_eq!(agent_perm_to_tapp("ai:chat"), Some(TappPermission::AiChat));
        assert_eq!(
            agent_perm_to_tapp("ai:search"),
            Some(TappPermission::AiGenerate)
        );
        assert_eq!(
            agent_perm_to_tapp("http:fetch"),
            Some(TappPermission::NetworkFetch)
        );
        assert_eq!(
            agent_perm_to_tapp("brew:read"),
            Some(TappPermission::BrewRead)
        );
        assert_eq!(
            agent_perm_to_tapp("report:write"),
            Some(TappPermission::ReportWrite)
        );
        assert_eq!(agent_perm_to_tapp("system:read"), None); // 特例：不经 Tapp
        assert_eq!(agent_perm_to_tapp("unknown:perm"), None);
    }

    #[test]
    fn saved_recipe_validation_rejects_dependency_cycles() {
        let mut recipe = Recipe::new("cycle", "cycle", ExecutionType::Instant);
        recipe.steps = vec![
            AiRecipeStep {
                id: "a".to_string(),
                capability_id: "ai.summarize".to_string(),
                action: "a".to_string(),
                params: HashMap::new(),
                depends_on: vec!["b".to_string()],
                on_failure: "abort".to_string(),
                retry: None,
                timeout_ms: None,
            }
            .into_recipe_step(0, None),
            AiRecipeStep {
                id: "b".to_string(),
                capability_id: "ai.summarize".to_string(),
                action: "b".to_string(),
                params: HashMap::new(),
                depends_on: vec!["a".to_string()],
                on_failure: "abort".to_string(),
                retry: None,
                timeout_ms: None,
            }
            .into_recipe_step(1, None),
        ];

        let error = Agent::validate_saved_recipe(&recipe).expect_err("cycle must be rejected");
        assert!(error.contains("dependency cycle"));
    }

    #[test]
    fn heartbeat_outcome_rejects_blocked_and_interactive_responses() {
        let response = |response_type, data| AgentResponse {
            response_type,
            message: "result".to_string(),
            data,
            data_display: None,
            suggestions: vec![],
            task: None,
            confirmation: None,
            frontend_action: None,
        };

        assert!(response(AgentResponseType::Answer, None).is_successful_outcome());
        assert!(
            !response(AgentResponseType::Answer, Some(json!({"blocked": true})))
                .is_successful_outcome()
        );
        assert!(!response(AgentResponseType::ConfirmationRequired, None).is_successful_outcome());
        assert!(!response(AgentResponseType::Clarification, None).is_successful_outcome());

        for status in [
            TaskStatus::Pending,
            TaskStatus::Running,
            TaskStatus::WaitingForInput,
            TaskStatus::Paused,
            TaskStatus::Failed,
            TaskStatus::Cancelled,
        ] {
            let recipe = Recipe::new("heartbeat", "heartbeat", ExecutionType::Instant);
            let mut task = TaskState::new(&recipe);
            task.status = status;
            let mut non_terminal = response(AgentResponseType::TaskCompleted, None);
            non_terminal.task = Some(task);
            assert!(!non_terminal.is_successful_outcome());
        }

        let recipe = Recipe::new("heartbeat", "heartbeat", ExecutionType::Instant);
        let mut task = TaskState::new(&recipe);
        task.status = TaskStatus::Completed;
        let mut completed = response(AgentResponseType::TaskCompleted, None);
        completed.task = Some(task.clone());
        assert!(completed.is_successful_outcome());

        let mut completed_with_failed_step = response(AgentResponseType::TaskCompleted, None);
        let mut failed_step_task = task;
        failed_step_task.step_results.insert(
            "blocked_dynamic_step".to_string(),
            StepResult {
                step_id: "blocked_dynamic_step".to_string(),
                success: false,
                output: None,
                error: Some("blocked".to_string()),
                duration_ms: 0,
                retry_count: 0,
            },
        );
        completed_with_failed_step.task = Some(failed_step_task);
        assert!(!completed_with_failed_step.is_successful_outcome());
    }

    #[test]
    fn task_state_task_id_matches_recipe_id_for_sse_cancel() {
        let recipe = Recipe::new("t", "do something", ExecutionType::Instant);
        let task = TaskState::new(&recipe);
        assert_eq!(
            task.task_id, recipe.id,
            "TaskCreated SSE uses recipe.id; cancel/steer must hit the same id"
        );
        assert_eq!(task.recipe_id, recipe.id);
    }

    #[test]
    fn pre_param_question_id_roundtrip() {
        let qid = pre_param_question_id("step_1", "tappId");
        assert_eq!(qid, "pre_param:step_1:tappId");
        assert_eq!(
            parse_pre_param_question_id(&qid),
            Some(("step_1".into(), "tappId".into()))
        );
        assert_eq!(
            parse_pre_param_question_id("pre_param_url"),
            Some(("".into(), "url".into()))
        );
        assert!(parse_pre_param_question_id("other").is_none());
    }

    fn sample_step(id: &str, cap: &str) -> types::RecipeStep {
        types::RecipeStep {
            id: id.into(),
            order: 0,
            capability_id: cap.into(),
            action: "act".into(),
            params: HashMap::new(),
            depends_on: vec![],
            on_failure: types::FailureStrategy::Abort,
            retry: None,
            timeout_ms: None,
            model_tier: None,
            generator: None,
        }
    }

    #[test]
    fn apply_pre_param_writes_back_to_recipe_step() {
        let mut recipe = Recipe::new("t", "open tapp", ExecutionType::Instant);
        recipe
            .steps
            .push(sample_step("step_1", "tapp.interact"));

        assert!(apply_pre_param_answer_to_recipe(
            &mut recipe,
            "pre_param:step_1:tappId",
            " my-tapp "
        ));
        assert_eq!(
            recipe.steps[0].params.get("tappId").and_then(|v| v.as_str()),
            Some("my-tapp")
        );
    }

    #[test]
    fn step_has_param_respects_from_refs() {
        let mut step = sample_step("s", "ai.summarize");
        assert!(!step_has_param_value(&step, "content"));
        step.params
            .insert("contentFrom".into(), json!("step_0"));
        assert!(step_has_param_value(&step, "content"));
    }
}
