// Agent confirmation resume and task management paths.

use chrono::{Duration, Utc};
use serde_json::{json, Value};
use std::collections::HashMap;

use super::{
    capability,
    executor,
    identity,
    response_agent,
    types,
};
use super::agent_header::*;
use super::agent_footer::*;
use super::types::*;

impl Agent {

    /// 处理用户确认
    pub async fn process_confirmation(
        &self,
        confirmation: UserConfirmation,
    ) -> Result<AgentResponse, String> {
        let user_id = confirmation.user_id;
        let task_id = confirmation.confirmation_id.clone();
        AgentTurnBudget::run(
            &self.db,
            user_id,
            "agent.process_confirmation",
            task_id,
            Box::pin(self.process_confirmation_inner(confirmation)),
        )
        .await
    }

    async fn process_confirmation_inner(
        &self,
        confirmation: UserConfirmation,
    ) -> Result<AgentResponse, String> {
        // PostgreSQL provides atomic, owner-scoped consumption across replicas.
        // The local map is only a hot cache and is cleared after the shared take.
        let pending = crate::services::tapp_registry::take_for_subject::<
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
    pub(crate) async fn check_missing_required_parameters(
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
    /// - High / Critical：拒绝自动执行，返回说明性响应
    /// - Low / Medium：自动确认放行并留痕
    ///
    /// 返回 `None` = 非系统用户，走正常确认流程；
    /// `Some(Ok(()))` = 已自动确认，继续执行；
    /// `Some(Err(response))` = 被拒绝，直接返回该响应。
    pub(crate) fn system_sensitive_gate(
        user_id: i32,
        sensitive_steps: &[PendingConfirmation],
    ) -> Option<Result<(), AgentResponse>> {
        if user_id != SYSTEM_USER_ID {
            return None;
        }
        if let Some(blocked) = sensitive_steps.iter().find(|s| {
            matches!(s.risk_level, RiskLevel::High | RiskLevel::Critical)
        }) {
            let msg = format!(
                "定时任务包含敏感操作 '{}'（{}，风险 {:?}），已拒绝自动执行。请手动操作或调整任务指令。",
                blocked.capability_name, blocked.capability_id, blocked.risk_level
            );
            tracing::warn!(
                capability = %blocked.capability_id,
                risk = ?blocked.risk_level,
                "[Agent] System task blocked: High/Critical operation requires human confirmation"
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

    pub(crate) async fn check_sensitive_steps(&self, recipe: &Recipe) -> Vec<PendingConfirmation> {
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
    pub(crate) fn generate_impact_description(
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
    pub(crate) async fn request_confirmation_v2(
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
        crate::services::tapp_registry::put(
            &self.db,
            CONFIRMATION_REGISTRY_NAMESPACE,
            &confirmation_id,
            crate::services::tapp_registry::RegistryIdentity {
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
    pub(crate) fn generate_confirmation_message(
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
    pub(crate) async fn generate_response_message_v2(
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

        // 多步骤结果汇总：交给 response_agent AI 流式生成
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
    pub(crate) fn infer_data_display_v2(
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
        AgentTurnBudget::run(
            &self.db,
            user_id,
            "agent.resume_task",
            task_id.to_string(),
            Box::pin(self.resume_task_inner(task_id, answer, user_id)),
        )
        .await
    }

    async fn resume_task_inner(
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
        AgentTurnBudget::run(
            &self.db,
            user_id,
            "agent.resume_task_with_progress",
            task_id.to_string(),
            Box::pin(self.resume_task_with_progress_inner(task_id, answer, user_id, progress_tx)),
        )
        .await
    }

    async fn resume_task_with_progress_inner(
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
    pub(crate) async fn stream_chat_response(
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
    pub(crate) async fn stream_text_as_tokens(tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>, text: &str) {
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
    pub(crate) fn extract_final_result(&self, task_state: &TaskState) -> serde_json::Value {
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

        // 关键改进：收集所有步骤中的 frontendAction 和 action
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
                // 也检查 action 字段（兼容 brew.generateReadingList 等）
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

                    // 添加所有收集到的 frontendActions
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
                    // 添加所有收集到的 frontendActions
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

        // 添加所有收集到的 frontendActions
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
    pub(crate) fn extract_frontend_action(&self, result: &Value) -> Option<Value> {
        // 检查 frontendAction（单个）
        if let Some(action) = result.get("frontendAction") {
            if action.get("type").and_then(Value::as_str).is_some() {
                return Some(action.clone());
            }
            tracing::warn!(action = %action, "[Agent] frontendAction is missing type");
        }

        // 也检查 "action" 字段（兼容 brew.generateReadingList 等返回格式）
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
