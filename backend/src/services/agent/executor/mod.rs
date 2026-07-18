//! 执行引擎模块
//!
//! 负责执行 Recipe 中的步骤，管理任务状态
//!
//! ## 模块结构
//!
//! - `task_store` - 任务状态存储和持久化
//! - `utils` - 工具函数（相似度计算、输出摘要等）
//! - `handlers` - 各类能力的具体执行实现
//!
//! ## 整合项目现有服务
//!
//! - `services/analyzer` - AI 分析服务
//! - `api/reports` - 报告生成系统
//! - `api/tapp` - 平台数据 API
//! - `services/brew_parser` - RSS/Atom 解析
//! - `services/tapp_api_service` - Tapp API 执行

pub mod dag;
pub mod error_analyzer;
pub mod events;
pub mod handlers;
pub mod retry;
pub mod task_store;
pub mod utils;

// 重新导出常用类型
pub use task_store::{
    cancel_task_for_user, claim_task_for_resume, clear_cancellation, enqueue_steering,
    get_task_for_user, get_user_tasks, init_task_store_db, is_cancelled, maybe_cleanup_tasks,
    persist_task_async, refresh_task_for_user, take_steering, TASK_STORE,
};
pub use utils::{extract_image_url, summarize_output, truncate_str};

use crate::config::ModelTier;
use crate::services::agent::capability::get_registry;
use crate::services::agent::tier_router::{self, TierRouter};
use crate::services::agent::types::{self, *};
use crate::services::ai::create_ai_analyzer_for_tier;
use crate::services::analyzer::AiAnalyzer;
use handlers::HandlerContext;
use sea_orm::DatabaseConnection;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// 执行引擎
pub struct Executor {
    /// 数据库连接
    pub(crate) db: DatabaseConnection,
    /// Pro 层级 AI 分析器（复杂推理、规划、创造性任务）
    pub(crate) pro_analyzer: Option<AiAnalyzer>,
    /// Standard 层级 AI 分析器（常规任务、数据处理、定式化操作）
    pub(crate) standard_analyzer: Option<AiAnalyzer>,
}

impl Executor {
    fn should_block_unconfirmed_dynamic_step(user_id: i32, risk: RiskLevel) -> bool {
        risk == RiskLevel::Critical
            || (user_id != crate::services::agent::SYSTEM_USER_ID && risk == RiskLevel::High)
    }

    /// 创建新的执行引擎
    pub async fn new(db: DatabaseConnection) -> Self {
        let pro_analyzer = create_ai_analyzer_for_tier(ModelTier::Pro).await;
        let standard_analyzer = create_ai_analyzer_for_tier(ModelTier::Standard).await;
        Self {
            db,
            pro_analyzer,
            standard_analyzer,
        }
    }

    /// 根据 ModelTier 获取对应的 AI 分析器
    fn get_analyzer_for_tier(&self, tier: ModelTier) -> Option<&AiAnalyzer> {
        match tier {
            ModelTier::Pro => self
                .pro_analyzer
                .as_ref()
                .or(self.standard_analyzer.as_ref()),
            ModelTier::Standard => self
                .standard_analyzer
                .as_ref()
                .or(self.pro_analyzer.as_ref()),
        }
    }

    /// 带熔断器的 tier 解析
    ///
    /// 优先使用熔断器感知的解析（自动降级），如果两个 tier 都熔断则 fallback 到普通解析。
    fn resolve_tier_with_breaker(
        capability_id: &str,
        explicit_tier: Option<ModelTier>,
    ) -> ModelTier {
        tier_router::resolve_with_circuit_breaker(capability_id, explicit_tier).unwrap_or_else(
            || {
                // 两个 tier 都熔断时仍然尝试（best-effort）
                tracing::warn!(
                    capability_id = capability_id,
                    "[Executor] Both tiers circuit-broken, falling back to default resolve"
                );
                TierRouter::resolve_with_override(capability_id, explicit_tier)
            },
        )
    }

    /// 记录步骤执行结果到熔断器
    fn record_step_to_breaker(tier: ModelTier, success: bool) {
        let breaker = tier_router::get_circuit_breaker(tier);
        if success {
            breaker.record_success();
        } else {
            breaker.record_failure();
        }
    }

    /// 执行方案（不带进度回调，兼容旧代码）
    pub async fn execute(&self, recipe: &Recipe, user_id: i32) -> Result<TaskState, String> {
        self.execute_with_progress(recipe, user_id, None).await
    }

    /// 执行方案（带实时进度回调）
    pub async fn execute_with_progress(
        &self,
        recipe: &Recipe,
        user_id: i32,
        progress_tx: Option<tokio::sync::mpsc::Sender<types::AgentProgressEvent>>,
    ) -> Result<TaskState, String> {
        // 创建任务状态
        let mut task_state = TaskState::new(recipe);
        task_state.status = TaskStatus::Running;
        task_state.lane_id = recipe.lane_key.clone();

        // 创建执行上下文（包含对话历史）
        let mut context = ExecutionContext::from_request_full(
            &recipe.original_request,
            &recipe.name,
            recipe.page_context.clone(),
            recipe.conversation_context.clone(),
        );
        context.variables.insert(
            "_task_id".to_string(),
            Value::String(task_state.task_id.clone()),
        );

        // 记录对话上下文信息
        if let Some(ref history) = context.conversation_context {
            tracing::info!(
                task_id = %task_state.task_id,
                history_len = history.len(),
                "[Executor] Conversation context loaded with {} messages",
                history.len()
            );
        }

        // 注入角色身份上下文（从 Orchestrator 分析结果）
        if let Some(role_ctx_val) = recipe.metadata.get("role_contexts") {
            if let Some(obj) = role_ctx_val.as_object() {
                for (role_key, ctx_val) in obj {
                    if let Some(ctx_str) = ctx_val.as_str() {
                        context
                            .role_contexts
                            .insert(role_key.clone(), ctx_str.to_string());
                    }
                }
                tracing::info!(
                    task_id = %task_state.task_id,
                    roles = context.role_contexts.len(),
                    "[Executor] Role identity contexts loaded"
                );
            }
        }

        // 召回记忆上下文，供各 AI 步骤的 inject_role_identity 注入 systemPrompt
        if let Some(mem) = crate::services::agent::memory::get_memory() {
            use crate::services::agent::memory::{MemoryTier, RecallQuery};
            let memories = mem
                .recall_with_params(RecallQuery {
                    query: recipe.original_request.clone(),
                    limit: 4,
                    tier_filter: Some(vec![MemoryTier::LongTerm, MemoryTier::MediumTerm]),
                    user_id: Some(user_id),
                    ..Default::default()
                })
                .await;
            if !memories.is_empty() {
                let lines: Vec<String> = memories
                    .iter()
                    .map(|m| {
                        let content: String = m.content.chars().take(200).collect();
                        if m.content.chars().count() > 200 {
                            format!("- {}...", content)
                        } else {
                            format!("- {}", content)
                        }
                    })
                    .collect();
                context.memory_context = Some(lines.join("\n"));
                tracing::info!(
                    task_id = %task_state.task_id,
                    memories = memories.len(),
                    "[Executor] Memory context loaded for AI steps"
                );
            }
        }

        // 如果有 page_context，存入 step_outputs
        if let Some(page_ctx) = context.page_context.clone() {
            let page_title = page_ctx
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            context.add_output("__page_context__", page_ctx);
            tracing::info!(
                task_id = %task_state.task_id,
                page_title = %page_title,
                "[Executor] Page context stored as __page_context__"
            );
        }

        // 存储任务
        {
            let mut store = TASK_STORE.write().await;
            store.store(user_id, task_state.clone());
        }

        tracing::info!(
            task_id = %task_state.task_id,
            recipe_id = %recipe.id,
            steps = recipe.steps.len(),
            "[Executor] Starting recipe execution"
        );

        // 对话历史已通过 ExecutionContext.conversation_context 传递到 HandlerContext
        // 不再冗余存入 step_outputs（inject_role_identity 直接从 execution_context 读取）

        // 初始化执行追踪
        let execution_start = std::time::Instant::now();
        let trace_id = format!("trace_{}", task_state.task_id);
        let mut step_traces: Vec<types::StepTrace> = Vec::new();
        let mut tier_usage: HashMap<String, u32> = HashMap::new();

        // 全局重试预算（跨所有步骤最多重试 5 次）
        let mut global_retry_budget: u32 = 5;

        // SSE 事件发送器
        let emitter = events::StepEventEmitter::new(progress_tx.clone());

        // 🛡️ 全局已执行步骤计数器（防止动态步骤导致无限执行）
        let mut total_executed_steps: usize = 0;
        const MAX_TOTAL_STEPS: usize = 15;

        // 执行步骤
        let mut step_index = 0;
        let all_steps: Vec<RecipeStep> = recipe.steps.clone();
        let total_steps = all_steps.len();

        // 前端步骤显示用：已入队的动态步骤总数（不随 pop 减少）
        let mut dynamic_steps_queued: usize = 0;
        // 前端步骤显示用：已发送 StepStarted 的次数（用作 display index）
        let mut display_step_counter: usize = 0;
        // 被隐藏的 Skill 编排步骤数量（用于修正 effective_total）
        let mut hidden_skill_steps: usize = 0;

        // 构建 DAG 调度器（检测是否有并行依赖）
        let mut dag_scheduler = dag::DagScheduler::new(&all_steps).ok();
        let mut use_dag = dag_scheduler.as_ref().is_some_and(|d| d.is_parallel_mode())
            && !all_steps
                .iter()
                .any(|step| step.capability_id == "tapp.interact");
        // 追踪被注入 DAG 的动态子步骤 ID（区分原始 recipe 步骤和 Skill 子步骤）
        let mut dag_injected_ids: HashSet<String> = HashSet::new();
        if use_dag {
            tracing::info!(
                task_id = %task_state.task_id,
                "[Executor] Parallel DAG mode detected, using DAG scheduler"
            );
        }

        while step_index < all_steps.len()
            || context.has_pending_steps()
            || dag_scheduler.as_ref().is_some_and(|d| d.has_remaining())
        {
            // 🔴 检查任务是否被取消
            if is_cancelled(&task_state.task_id).await {
                tracing::info!(
                    task_id = %task_state.task_id,
                    "[Executor] Task cancelled by user"
                );
                task_state.status = TaskStatus::Cancelled;
                task_state.completed_at = Some(chrono::Utc::now());
                task_state.error =
                    Some(crate::services::agent::response_agent::task_cancelled_by_user());

                // 清除取消标记
                clear_cancellation(&task_state.task_id).await;

                // 发送取消事件
                if let Some(ref tx) = progress_tx {
                    let _ = tx
                        .send(AgentProgressEvent::Error {
                            task_id: Some(task_state.task_id.clone()),
                            message: crate::services::agent::response_agent::task_cancelled(),
                            code: "CANCELLED".to_string(),
                        })
                        .await;
                }

                // 更新存储
                {
                    let mut store = TASK_STORE.write().await;
                    if let Some(task) = store.get_mut(&task_state.task_id) {
                        *task = task_state.clone();
                    }
                }
                persist_task_async(user_id, task_state.clone());

                return Ok(task_state);
            }

            // 🛡️ 全局步骤上限检查
            if total_executed_steps >= MAX_TOTAL_STEPS {
                tracing::warn!(
                    task_id = %task_state.task_id,
                    executed = total_executed_steps,
                    "[Executor] Global step limit reached ({}), stopping execution",
                    MAX_TOTAL_STEPS
                );
                // 清空待执行的动态步骤
                context.pending_dynamic_steps.clear();
                break;
            }
            total_executed_steps += 1;

            // 优先处理动态生成的步骤
            let (step, _is_dynamic) = if let Some(dynamic_step) = context.pop_dynamic_step() {
                tracing::info!(
                    step_id = %dynamic_step.id,
                    "[Executor] Executing dynamic step"
                );
                (Some(dynamic_step), true)
            } else if use_dag {
                // DAG 模式：获取所有就绪步骤
                let ready = dag_scheduler
                    .as_ref()
                    .map(|d| d.get_ready_steps())
                    .unwrap_or_default();
                if ready.is_empty() {
                    break;
                } else if ready.len() > 1 {
                    // ====== 流式 DAG 并行执行 ======
                    // 使用 FuturesUnordered：任何步骤完成时立即检查并启动新就绪步骤
                    // 避免 join_all 的波次阻塞（慢步骤不阻塞快步骤的后续依赖）
                    use futures::stream::{FuturesUnordered, StreamExt};
                    use std::future::Future;
                    use std::pin::Pin;

                    /// (step, result, duration_ms, new_dynamic_steps, new_variables, new_decisions)
                    type StepFuture<'a> = Pin<
                        Box<
                            dyn Future<
                                    Output = (
                                        RecipeStep,
                                        Result<Value, String>,
                                        u64,
                                        Vec<RecipeStep>,
                                        HashMap<String, Value>,
                                        Vec<types::ExecutionDecision>,
                                    ),
                                > + Send
                                + 'a,
                        >,
                    >;

                    let effective_total =
                        (total_steps + dynamic_steps_queued).saturating_sub(hidden_skill_steps);
                    let mut spawned: HashSet<String> = HashSet::new();
                    let mut in_flight: FuturesUnordered<StepFuture<'_>> = FuturesUnordered::new();
                    let mut failed_for_retry: Vec<(RecipeStep, String, u64)> = Vec::new();
                    // 暂挂的用户问题队列：检测到后停止启动新步骤，等 in-flight 自然完成
                    let mut pending_questions_from_dag: Vec<types::UserQuestion> = Vec::new();

                    // 启动所有初始就绪步骤
                    for step in &ready {
                        spawned.insert(step.id.clone());
                        total_executed_steps += 1;

                        {
                            let desc =
                                crate::services::agent::capability::get_step_description(step);
                            emitter.step_started(
                                &step.id,
                                display_step_counter as u32,
                                effective_total as u32,
                                &desc,
                                crate::services::agent::response_agent::describe_parallel_step_start(&desc),
                            ).await;
                            emitter
                                .debug_start(
                                    &step.id,
                                    &step.capability_id,
                                    if step.action.is_empty() {
                                        None
                                    } else {
                                        Some(step.action.clone())
                                    },
                                    if context.original_request.is_empty() {
                                        None
                                    } else {
                                        Some(context.original_request.clone())
                                    },
                                    Self::build_debug_params(&step.params),
                                    dag_injected_ids.contains(&step.id),
                                )
                                .await;
                        }
                        display_step_counter += 1;

                        let step_tier =
                            Self::resolve_tier_with_breaker(&step.capability_id, step.model_tier);
                        let step_analyzer = self.get_analyzer_for_tier(step_tier);
                        let ctx_snapshot = context.clone();
                        let step_clone = step.clone();
                        let executor_task_id = task_state.task_id.clone();
                        in_flight.push(Box::pin(async move {
                            let start = std::time::Instant::now();
                            let mut ctx = ctx_snapshot;
                            let handler_ctx = HandlerContext {
                                db: &self.db,
                                ai_analyzer: step_analyzer,
                                user_id,
                                task_id: Some(executor_task_id),
                                execution_context: Some(ctx.clone()),
                            };
                            let pre_dyn = ctx.pending_dynamic_steps.len();
                            let pre_decisions = ctx.decision_history.len();
                            let result =
                                self.execute_step(&step_clone, &mut ctx, &handler_ctx).await;
                            let new_dynamic = ctx.pending_dynamic_steps[pre_dyn..].to_vec();
                            // 收集并行步骤新增的 variables 和 decisions，回传给主 context
                            let new_vars = ctx.variables.clone();
                            let new_decisions = ctx.decision_history[pre_decisions..].to_vec();
                            (
                                step_clone,
                                result,
                                start.elapsed().as_millis() as u64,
                                new_dynamic,
                                new_vars,
                                new_decisions,
                            )
                        }));
                    }
                    // 外层循环已计 1，修正计数
                    total_executed_steps = total_executed_steps.saturating_sub(1);

                    tracing::info!(
                        task_id = %task_state.task_id,
                        initial = ready.len(),
                        "[Executor] Streaming DAG: launched {} initial steps",
                        ready.len()
                    );

                    // 流式处理：每完成一个步骤，立即检查并启动新就绪步骤
                    while let Some((
                        step,
                        step_result,
                        duration_ms,
                        returned_dynamic_steps,
                        returned_vars,
                        returned_decisions,
                    )) = in_flight.next().await
                    {
                        // 取消检查：在流式循环中也能及时响应取消
                        if is_cancelled(&task_state.task_id).await {
                            tracing::info!(
                                task_id = %task_state.task_id,
                                "[Executor] DAG streaming cancelled by user"
                            );
                            task_state.status = TaskStatus::Failed;
                            task_state.error = Some("用户取消了任务".to_string());
                            // 中断流式循环，外层循环的取消检查会处理状态保存
                            break;
                        }

                        let par_tier =
                            TierRouter::resolve_with_override(&step.capability_id, step.model_tier);
                        let par_requires_llm = TierRouter::requires_llm(&step.capability_id);
                        let par_tier_str = if par_requires_llm {
                            format!("{:?}", par_tier)
                        } else {
                            String::new()
                        };
                        if let Some(idx) = all_steps.iter().position(|s| s.id == step.id) {
                            step_index = idx + 1;
                        }

                        match step_result {
                            Ok(output) => {
                                Self::record_step_to_breaker(par_tier, true);
                                context.add_output(&step.id, output.clone());
                                // 合并并行步骤产生的 variables 和 decisions 到主 context
                                for (k, v) in returned_vars {
                                    context.variables.entry(k).or_insert(v);
                                }
                                context.decision_history.extend(returned_decisions);

                                let is_injected = dag_injected_ids.contains(&step.id);

                                {
                                    let output_preview = {
                                        let s = serde_json::to_string(&output).unwrap_or_default();
                                        if s.len() > 1000 {
                                            format!("{}...", &s[..1000])
                                        } else {
                                            s
                                        }
                                    };
                                    emitter
                                        .step_succeeded(
                                            &step.id,
                                            0,
                                            duration_ms,
                                            summarize_output(&output),
                                            extract_image_url(&output),
                                        )
                                        .await;
                                    emitter
                                        .debug_complete(
                                            &step.id,
                                            &step.capability_id,
                                            is_injected,
                                            duration_ms,
                                            true,
                                            Some(output_preview),
                                            None,
                                        )
                                        .await;
                                }

                                task_state.step_results.insert(
                                    step.id.clone(),
                                    StepResult {
                                        step_id: step.id.clone(),
                                        success: true,
                                        output: Some(output),
                                        error: None,
                                        duration_ms,
                                        retry_count: 0,
                                    },
                                );

                                if let Some(ref mut dag) = dag_scheduler {
                                    dag.mark_completed(&step.id);
                                }

                                if let Some(evo) =
                                    crate::services::agent::skill_evolution::get_skill_evolution()
                                {
                                    evo.on_execution_complete(&step.capability_id, true, None)
                                        .await;
                                }

                                // 合并从 execute_step 返回的动态步骤（技能子步骤等）
                                // pre_dynamic_count 在 generator 之前取值，确保 generator 和 skill 子步骤都能被 DAG 注入
                                let pre_dynamic_count = context.pending_dynamic_steps.len();

                                // 动态步骤生成器：处理 ConditionalBranch / AiGenerated
                                // 🛡️ DAG注入的动态步骤不触发生成器，防止链式爆炸
                                if !is_injected {
                                    if let Some(ref gen) = step.generator {
                                        if let Some(ref output_val) = task_state
                                            .step_results
                                            .get(&step.id)
                                            .and_then(|r| r.output.clone())
                                        {
                                            let generated = self
                                                .process_step_generator(
                                                    gen,
                                                    &step,
                                                    output_val,
                                                    &mut context,
                                                )
                                                .await;
                                            if !generated.is_empty() {
                                                tracing::info!(
                                                    step_id = %step.id,
                                                    count = generated.len(),
                                                    "[Executor] DAG generator produced {} dynamic steps",
                                                    generated.len()
                                                );
                                                context.queue_dynamic_steps(generated);
                                            }
                                        }
                                    }
                                }

                                if !returned_dynamic_steps.is_empty() {
                                    tracing::info!(
                                        step_id = %step.id,
                                        count = returned_dynamic_steps.len(),
                                        "[Executor] DAG step returned {} dynamic steps from executor",
                                        returned_dynamic_steps.len()
                                    );
                                    context.queue_dynamic_steps(returned_dynamic_steps);
                                }

                                // 动态步骤并行化：注入 DAG 调度器
                                let post_dynamic_count = context.pending_dynamic_steps.len();
                                if post_dynamic_count > pre_dynamic_count {
                                    let new_count = post_dynamic_count - pre_dynamic_count;
                                    if new_count > 1 {
                                        let new_steps: Vec<RecipeStep> = context
                                            .pending_dynamic_steps[pre_dynamic_count..]
                                            .to_vec();
                                        let injected = if let Some(ref mut dag) = dag_scheduler {
                                            dag.add_steps(&new_steps).is_ok()
                                        } else {
                                            false
                                        };
                                        if injected {
                                            for s in
                                                &context.pending_dynamic_steps[pre_dynamic_count..]
                                            {
                                                dag_injected_ids.insert(s.id.clone());
                                            }
                                            context
                                                .pending_dynamic_steps
                                                .drain(pre_dynamic_count..);
                                            tracing::info!(
                                                task_id = %task_state.task_id,
                                                count = new_count,
                                                "[Executor] DAG parallel: dynamic steps injected into DAG"
                                            );
                                        }
                                    }
                                }

                                if !par_tier_str.is_empty() {
                                    *tier_usage.entry(par_tier_str.clone()).or_insert(0) += 1;
                                }
                                step_traces.push(types::StepTrace {
                                    step_id: step.id.clone(),
                                    capability_id: step.capability_id.clone(),
                                    tier_used: par_tier_str,
                                    duration_ms,
                                    success: true,
                                    error: None,
                                    action: step.action.clone(),
                                    params: serde_json::to_value(&step.params).ok(),
                                    output_preview: None,
                                    is_dynamic: is_injected,
                                });

                                // 动态分析：检查是否需要用户输入
                                // 🛡️ DAG注入的动态步骤不触发分析，防止链式膨胀
                                if !is_injected {
                                    if let Some(ref output_val) = task_state
                                        .step_results
                                        .get(&step.id)
                                        .and_then(|r| r.output.clone())
                                    {
                                        if let Some(question) = self
                                            .analyze_and_generate_dynamic_steps(
                                                &step,
                                                output_val,
                                                &mut context,
                                                recipe,
                                            )
                                            .await
                                        {
                                            tracing::info!(
                                                step_id = %step.id,
                                                question_id = %question.question_id,
                                                queued = pending_questions_from_dag.len(),
                                                "[Executor] DAG parallel: step output requires user input, queuing question"
                                            );
                                            pending_questions_from_dag.push(question);
                                            // 不再启动新步骤，让 in-flight 自然完成
                                        }
                                    }
                                }

                                // ★ 核心改进：立即检查并启动新就绪步骤（如果没有待处理问题）
                                if pending_questions_from_dag.is_empty() {
                                    if let Some(ref dag) = dag_scheduler {
                                        for new_step in dag.get_ready_steps() {
                                            if spawned.contains(&new_step.id) {
                                                continue;
                                            }
                                            spawned.insert(new_step.id.clone());
                                            total_executed_steps += 1;

                                            {
                                                let desc = crate::services::agent::capability::get_step_description(&new_step);
                                                emitter.step_started(
                                                &new_step.id,
                                                display_step_counter as u32,
                                                effective_total as u32,
                                                &desc,
                                                crate::services::agent::response_agent::describe_parallel_step_start(&desc),
                                            ).await;
                                                emitter
                                                    .debug_start(
                                                        &new_step.id,
                                                        &new_step.capability_id,
                                                        if new_step.action.is_empty() {
                                                            None
                                                        } else {
                                                            Some(new_step.action.clone())
                                                        },
                                                        if context.original_request.is_empty() {
                                                            None
                                                        } else {
                                                            Some(context.original_request.clone())
                                                        },
                                                        Self::build_debug_params(&new_step.params),
                                                        dag_injected_ids.contains(&new_step.id),
                                                    )
                                                    .await;
                                            }
                                            display_step_counter += 1;

                                            let new_tier = Self::resolve_tier_with_breaker(
                                                &new_step.capability_id,
                                                new_step.model_tier,
                                            );
                                            let new_analyzer = self.get_analyzer_for_tier(new_tier);
                                            let ctx_snapshot = context.clone();
                                            let new_step_clone = new_step.clone();
                                            let executor_task_id = task_state.task_id.clone();
                                            in_flight.push(Box::pin(async move {
                                                let start = std::time::Instant::now();
                                                let mut ctx = ctx_snapshot;
                                                let handler_ctx = HandlerContext {
                                                    db: &self.db,
                                                    ai_analyzer: new_analyzer,
                                                    user_id,
                                                    task_id: Some(executor_task_id),
                                                    execution_context: Some(ctx.clone()),
                                                };
                                                let pre_dyn = ctx.pending_dynamic_steps.len();
                                                let pre_decisions = ctx.decision_history.len();
                                                let result = self
                                                    .execute_step(
                                                        &new_step_clone,
                                                        &mut ctx,
                                                        &handler_ctx,
                                                    )
                                                    .await;
                                                let new_dynamic =
                                                    ctx.pending_dynamic_steps[pre_dyn..].to_vec();
                                                let new_vars = ctx.variables.clone();
                                                let new_decisions =
                                                    ctx.decision_history[pre_decisions..].to_vec();
                                                (
                                                    new_step_clone,
                                                    result,
                                                    start.elapsed().as_millis() as u64,
                                                    new_dynamic,
                                                    new_vars,
                                                    new_decisions,
                                                )
                                            }));

                                            tracing::info!(
                                                step_id = %new_step.id,
                                                "[Executor] Streaming DAG: immediately spawned newly-ready step"
                                            );
                                        }
                                    }
                                } // end pending_questions_from_dag.is_empty() guard
                            }
                            Err(e) => {
                                Self::record_step_to_breaker(par_tier, false);
                                let is_injected = dag_injected_ids.contains(&step.id);

                                let analysis = error_analyzer::ErrorAnalyzer::analyze(
                                    &e,
                                    &step.capability_id,
                                    &step.params,
                                );
                                let max_retries = step
                                    .retry
                                    .as_ref()
                                    .map(|r| r.max_attempts.min(3))
                                    .unwrap_or_else(|| {
                                        if step.capability_id.starts_with("ai.")
                                            || step.capability_id.starts_with("skill:")
                                            || step.capability_id == "prompt.generate"
                                        {
                                            2
                                        } else {
                                            1
                                        }
                                    });

                                if analysis.retryable && global_retry_budget > 0 && max_retries > 1
                                {
                                    tracing::info!(
                                        step_id = %step.id,
                                        category = ?analysis.category,
                                        "[Executor] Streaming DAG step failed, queuing for retry: {}",
                                        analysis.description
                                    );
                                    failed_for_retry.push((step, e, duration_ms));
                                } else {
                                    tracing::error!(
                                        step_id = %step.id,
                                        error = %e,
                                        "[Executor] Streaming DAG step failed (not retryable)"
                                    );

                                    emitter.step_failed(&step.id, 0, duration_ms, &e).await;
                                    emitter
                                        .debug_complete(
                                            &step.id,
                                            &step.capability_id,
                                            is_injected,
                                            duration_ms,
                                            false,
                                            None,
                                            Some(e.clone()),
                                        )
                                        .await;

                                    if let Some(evo) =
                                        crate::services::agent::skill_evolution::get_skill_evolution(
                                        )
                                    {
                                        evo.on_execution_complete(
                                            &step.capability_id,
                                            false,
                                            Some(&e),
                                        )
                                        .await;
                                    }

                                    task_state.step_results.insert(
                                        step.id.clone(),
                                        StepResult {
                                            step_id: step.id.clone(),
                                            success: false,
                                            output: None,
                                            error: Some(e.clone()),
                                            duration_ms,
                                            retry_count: 0,
                                        },
                                    );

                                    if let Some(ref mut dag) = dag_scheduler {
                                        dag.mark_failed(&step.id, &step.on_failure);
                                    }

                                    if !par_tier_str.is_empty() {
                                        *tier_usage.entry(par_tier_str.clone()).or_insert(0) += 1;
                                    }
                                    step_traces.push(types::StepTrace {
                                        step_id: step.id.clone(),
                                        capability_id: step.capability_id.clone(),
                                        tier_used: par_tier_str,
                                        duration_ms,
                                        success: false,
                                        error: Some(e),
                                        action: step.action.clone(),
                                        params: serde_json::to_value(&step.params).ok(),
                                        output_preview: None,
                                        is_dynamic: is_injected,
                                    });
                                }
                            }
                        }
                    } // end streaming loop

                    // ====== 流式 DAG 后处理：暂停等待用户输入 ======
                    // 🛡️ 如果已取消，跳过 WaitingForInput 和重试
                    let dag_cancelled = task_state.status == TaskStatus::Failed
                        && task_state.error.as_deref() == Some("用户取消了任务");
                    if !dag_cancelled && !pending_questions_from_dag.is_empty() {
                        let question = pending_questions_from_dag.remove(0);
                        // 将剩余问题存入 context，resume 后继续提问
                        if !pending_questions_from_dag.is_empty() {
                            tracing::info!(
                                deferred = pending_questions_from_dag.len(),
                                "[Executor] DAG: {} additional questions stored for later",
                                pending_questions_from_dag.len()
                            );
                            context.pending_questions.extend(pending_questions_from_dag);
                        }
                        emitter
                            .waiting_for_input(&task_state.task_id, &question)
                            .await;

                        // 保存任务状态为 WaitingForInput
                        task_state.status = TaskStatus::WaitingForInput;
                        task_state.set_pending_question(question);
                        task_state.recipe = Some(recipe.clone());
                        context.retry_budget_remaining = global_retry_budget;
                        task_state.execution_context = Some(context);

                        {
                            let mut store = TASK_STORE.write().await;
                            if let Some(task) = store.get_mut(&task_state.task_id) {
                                *task = task_state.clone();
                            }
                        }
                        persist_task_async(user_id, task_state.clone());

                        return Ok(task_state);
                    } // end !dag_cancelled guard

                    // ====== 流式DAG后的串行重试（复用 retry.rs 统一逻辑）======
                    // 🛡️ 如果已取消，跳过所有重试
                    let retry_list = if dag_cancelled {
                        Vec::new()
                    } else {
                        failed_for_retry
                    };
                    for (step, _first_error, _first_duration) in retry_list {
                        let is_injected = dag_injected_ids.contains(&step.id);

                        // 复用统一重试方法：DAG 已失败一次，这里从头重新执行+重试
                        let max_retries = Self::default_max_retries(&step);
                        let mut retry_config = retry::RetryConfig {
                            max_attempts: max_retries,
                            global_budget: global_retry_budget,
                        };
                        let event_ctx = retry::RetryEventContext {
                            step_display_index: 0,
                            progress_tx: progress_tx.clone(),
                        };

                        let outcome = self
                            .execute_step_with_retry(
                                &step,
                                &mut context,
                                user_id,
                                &mut retry_config,
                                &event_ctx,
                            )
                            .await;
                        global_retry_budget = retry_config.global_budget;

                        let duration_ms = outcome.duration_ms;
                        let tier_str = if TierRouter::requires_llm(&step.capability_id) {
                            format!("{:?}", outcome.last_tier)
                        } else {
                            String::new()
                        };

                        // 注入错误分析器建议的前置步骤
                        if !outcome.prepend_steps.is_empty() {
                            context.queue_dynamic_steps(outcome.prepend_steps.clone());
                        }

                        if outcome.success {
                            emitter
                                .step_succeeded(
                                    &step.id,
                                    0,
                                    duration_ms,
                                    summarize_output(
                                        outcome.output.as_ref().unwrap_or(&json!(null)),
                                    ),
                                    extract_image_url(
                                        outcome.output.as_ref().unwrap_or(&json!(null)),
                                    ),
                                )
                                .await;

                            task_state
                                .step_results
                                .insert(step.id.clone(), outcome.to_step_result(&step.id));

                            if let Some(ref mut dag) = dag_scheduler {
                                dag.mark_completed(&step.id);
                            }

                            if let Some(evo) =
                                crate::services::agent::skill_evolution::get_skill_evolution()
                            {
                                evo.on_execution_complete(&step.capability_id, true, None)
                                    .await;
                            }
                        } else {
                            let error_msg = outcome.error.as_deref().unwrap_or("unknown error");

                            emitter
                                .step_failed(&step.id, 0, duration_ms, error_msg)
                                .await;
                            emitter
                                .debug_complete(
                                    &step.id,
                                    &step.capability_id,
                                    is_injected,
                                    duration_ms,
                                    false,
                                    None,
                                    Some(error_msg.to_string()),
                                )
                                .await;

                            if let Some(evo) =
                                crate::services::agent::skill_evolution::get_skill_evolution()
                            {
                                evo.on_execution_complete(
                                    &step.capability_id,
                                    false,
                                    Some(error_msg),
                                )
                                .await;
                            }

                            task_state
                                .step_results
                                .insert(step.id.clone(), outcome.to_step_result(&step.id));

                            if let Some(ref mut dag) = dag_scheduler {
                                dag.mark_failed(&step.id, &step.on_failure);
                            }
                        }

                        if !tier_str.is_empty() {
                            *tier_usage.entry(tier_str.clone()).or_insert(0) += 1;
                        }
                        step_traces.push(types::StepTrace {
                            step_id: step.id.clone(),
                            capability_id: step.capability_id.clone(),
                            tier_used: tier_str,
                            duration_ms,
                            success: outcome.success,
                            error: outcome.error,
                            action: step.action.clone(),
                            params: serde_json::to_value(&step.params).ok(),
                            output_preview: None,
                            is_dynamic: is_injected,
                        });
                    }

                    task_state.update_progress(effective_total);
                    continue; // 继续下一波并行步骤
                } else {
                    // 单个就绪步骤，走顺序路径
                    let next = match ready.into_iter().next() {
                        Some(s) => s,
                        None => break,
                    };
                    if let Some(idx) = all_steps.iter().position(|s| s.id == next.id) {
                        step_index = idx + 1;
                    }
                    let is_injected = dag_injected_ids.contains(&next.id);
                    (Some(next), is_injected)
                }
            } else if step_index < all_steps.len() {
                let s = all_steps[step_index].clone();
                step_index += 1;
                (Some(s), false)
            } else {
                break;
            };

            // ====== 顺序执行路径（单步） ======
            let step = match step {
                Some(s) => s,
                None => continue,
            };

            // Skill 编排步骤（skill: 前缀）：只是生成子步骤，不直接面向用户，跳过前端进度
            let is_skill_planning = step.capability_id.starts_with("skill:");

            // 计算前端显示用的总步骤数和当前序号
            let effective_total =
                (total_steps + dynamic_steps_queued).saturating_sub(hidden_skill_steps);
            task_state.current_step = step_index;
            task_state.update_progress(effective_total);

            // 发送步骤开始事件（Skill 编排步骤不发送）
            if !is_skill_planning {
                let step_description =
                    crate::services::agent::capability::get_step_description(&step);
                emitter
                    .step_started(
                        &step.id,
                        display_step_counter as u32,
                        effective_total as u32,
                        &step_description,
                        crate::services::agent::response_agent::describe_step_start(
                            &step_description,
                        ),
                    )
                    .await;
                display_step_counter += 1;
            } else {
                hidden_skill_steps += 1;
            }
            emitter
                .debug_start(
                    &step.id,
                    &step.capability_id,
                    if step.action.is_empty() {
                        None
                    } else {
                        Some(step.action.clone())
                    },
                    if context.original_request.is_empty() {
                        None
                    } else {
                        Some(context.original_request.clone())
                    },
                    Self::build_debug_params(&step.params),
                    _is_dynamic,
                )
                .await;

            // 执行步骤（带智能重试）——委托给统一的 retry 模块
            let pre_dynamic_count = context.pending_dynamic_steps.len();
            let step_display_index = display_step_counter.saturating_sub(1) as u32;
            let mut retry_config = retry::RetryConfig {
                max_attempts: Self::default_max_retries(&step),
                global_budget: global_retry_budget,
            };
            let event_ctx = retry::RetryEventContext {
                step_display_index,
                progress_tx: progress_tx.clone(),
            };

            let outcome = self
                .execute_step_with_retry(
                    &step,
                    &mut context,
                    user_id,
                    &mut retry_config,
                    &event_ctx,
                )
                .await;
            global_retry_budget = retry_config.global_budget;

            // 注入错误分析器建议的前置步骤
            if !outcome.prepend_steps.is_empty() {
                context.queue_dynamic_steps(outcome.prepend_steps.clone());
            }

            let duration_ms = outcome.duration_ms;

            if outcome.success {
                let output = outcome.output.clone().unwrap_or_default();

                // 发送步骤完成事件（Skill 编排步骤不发送前端可见的完成事件）
                if !is_skill_planning {
                    emitter
                        .step_succeeded(
                            &step.id,
                            step_display_index,
                            duration_ms,
                            summarize_output(&output),
                            extract_image_url(&output),
                        )
                        .await;
                }
                emitter
                    .debug_complete(
                        &step.id,
                        &step.capability_id,
                        _is_dynamic,
                        duration_ms,
                        true,
                        None,
                        None,
                    )
                    .await;

                // 动态步骤生成器
                if !_is_dynamic {
                    if let Some(ref gen) = step.generator {
                        let generated = self
                            .process_step_generator(gen, &step, &output, &mut context)
                            .await;
                        if !generated.is_empty() {
                            tracing::info!(
                                step_id = %step.id,
                                count = generated.len(),
                                "[Executor] Generator produced {} dynamic steps",
                                generated.len()
                            );
                            context.queue_dynamic_steps(generated);
                        }
                    }
                }

                // 更新动态步骤计数
                let post_dynamic_count = context.pending_dynamic_steps.len();
                if post_dynamic_count > pre_dynamic_count {
                    let new_count = post_dynamic_count - pre_dynamic_count;
                    dynamic_steps_queued += new_count;

                    if new_count > 1 {
                        let new_steps: Vec<RecipeStep> =
                            context.pending_dynamic_steps[pre_dynamic_count..].to_vec();
                        let injected = if let Some(ref mut dag) = dag_scheduler {
                            dag.add_steps(&new_steps).is_ok()
                        } else {
                            match dag::DagScheduler::new(&new_steps) {
                                Ok(new_dag) => {
                                    dag_scheduler = Some(new_dag);
                                    true
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "[Executor] Failed to create DAG for dynamic steps");
                                    false
                                }
                            }
                        };
                        if injected {
                            for s in &context.pending_dynamic_steps[pre_dynamic_count..] {
                                dag_injected_ids.insert(s.id.clone());
                            }
                            context.pending_dynamic_steps.drain(pre_dynamic_count..);
                            if !use_dag {
                                use_dag = true;
                                tracing::info!(task_id = %task_state.task_id, count = new_count, "[Executor] Dynamic steps injected into DAG, enabling parallel mode");
                            } else {
                                tracing::info!(task_id = %task_state.task_id, count = new_count, "[Executor] Dynamic steps injected into existing DAG");
                            }
                        }
                    }
                }

                task_state
                    .step_results
                    .insert(step.id.clone(), outcome.to_step_result(&step.id));

                if let Some(ref mut dag) = dag_scheduler {
                    dag.mark_completed(&step.id);
                }

                if let Some(evolution) =
                    crate::services::agent::skill_evolution::get_skill_evolution()
                {
                    evolution
                        .on_execution_complete(&step.capability_id, true, None)
                        .await;
                }

                if let Some(question) = tapp_interaction_wait_question(&output) {
                    emitter
                        .waiting_for_input(&task_state.task_id, &question)
                        .await;
                    task_state.status = TaskStatus::WaitingForInput;
                    task_state.set_pending_question(question);
                    task_state.recipe = Some(recipe.clone());
                    context.retry_budget_remaining = global_retry_budget;
                    task_state.execution_context = Some(context);
                    {
                        let mut store = TASK_STORE.write().await;
                        if let Some(task) = store.get_mut(&task_state.task_id) {
                            *task = task_state.clone();
                        }
                    }
                    task_store::save_task_to_db(user_id, &task_state)
                        .await
                        .map_err(|error| {
                            format!("persist Tapp interaction wait state failed: {error}")
                        })?;
                    return Ok(task_state);
                }

                // 动态分析：检查是否需要用户输入
                if !_is_dynamic {
                    if let Some(question) = self
                        .analyze_and_generate_dynamic_steps(&step, &output, &mut context, recipe)
                        .await
                    {
                        emitter
                            .waiting_for_input(&task_state.task_id, &question)
                            .await;

                        task_state.status = TaskStatus::WaitingForInput;
                        task_state.set_pending_question(question);
                        task_state.recipe = Some(recipe.clone());
                        context.retry_budget_remaining = global_retry_budget;
                        task_state.execution_context = Some(context);

                        {
                            let mut store = TASK_STORE.write().await;
                            if let Some(task) = store.get_mut(&task_state.task_id) {
                                *task = task_state.clone();
                            }
                        }
                        persist_task_async(user_id, task_state.clone());

                        return Ok(task_state);
                    }
                }
            } else {
                // 步骤失败
                let error_msg = outcome.error.as_deref().unwrap_or("unknown error");

                if !is_skill_planning {
                    emitter
                        .step_failed(&step.id, step_display_index, duration_ms, error_msg)
                        .await;
                }
                emitter
                    .debug_complete(
                        &step.id,
                        &step.capability_id,
                        _is_dynamic,
                        duration_ms,
                        false,
                        None,
                        Some(error_msg.to_string()),
                    )
                    .await;

                if let Some(evolution) =
                    crate::services::agent::skill_evolution::get_skill_evolution()
                {
                    evolution
                        .on_execution_complete(&step.capability_id, false, Some(error_msg))
                        .await;
                }

                task_state
                    .step_results
                    .insert(step.id.clone(), outcome.to_step_result(&step.id));

                if let Some(ref mut dag) = dag_scheduler {
                    dag.mark_failed(&step.id, &step.on_failure);
                }
            }

            // 记录步骤追踪
            {
                let tier_str = if TierRouter::requires_llm(&step.capability_id) {
                    format!("{:?}", outcome.last_tier)
                } else {
                    String::new()
                };
                if !tier_str.is_empty() {
                    *tier_usage.entry(tier_str.clone()).or_insert(0) += 1;
                }
                step_traces.push(types::StepTrace {
                    step_id: step.id.clone(),
                    capability_id: step.capability_id.clone(),
                    tier_used: tier_str,
                    duration_ms,
                    success: outcome.success,
                    error: outcome.error.clone(),
                    action: step.action.clone(),
                    params: serde_json::to_value(&step.params).ok(),
                    output_preview: None,
                    is_dynamic: _is_dynamic,
                });
            }
        }

        // A cancellation can arrive while the final long-running step is in
        // flight. Recheck before committing a terminal success so a remote
        // replica's cancelled DB state cannot be overwritten as completed.
        let cancelled = is_cancelled(&task_state.task_id).await;
        if cancelled {
            clear_cancellation(&task_state.task_id).await;
        }

        // 附加执行追踪
        task_state.execution_trace = Some(types::ExecutionTrace {
            trace_id,
            steps: step_traces,
            total_duration_ms: execution_start.elapsed().as_millis() as u64,
            tier_usage,
            planner_decision: None,
        });

        // 根据步骤结果决定最终状态
        let total_steps = task_state.step_results.len();
        let failed_steps = task_state
            .step_results
            .values()
            .filter(|r| !r.success)
            .count();
        if cancelled {
            task_state.status = TaskStatus::Cancelled;
            task_state.error =
                Some(crate::services::agent::response_agent::task_cancelled_by_user());
        } else if failed_steps > 0 && failed_steps == total_steps {
            task_state.status = TaskStatus::Failed;
            let errors: Vec<String> = task_state
                .step_results
                .values()
                .filter_map(|r| r.error.clone())
                .collect();
            task_state.error = Some(errors.join("; "));
        } else {
            task_state.status = TaskStatus::Completed;
        }
        task_state.completed_at = Some(chrono::Utc::now());
        task_state.progress = 100;

        {
            let mut store = TASK_STORE.write().await;
            if let Some(task) = store.get_mut(&task_state.task_id) {
                *task = task_state.clone();
            }
        }
        persist_task_async(user_id, task_state.clone());

        Ok(task_state)
    }

    /// 执行单个步骤
    async fn execute_step(
        &self,
        step: &RecipeStep,
        context: &mut ExecutionContext,
        handler_ctx: &HandlerContext<'_>,
    ) -> Result<Value, String> {
        if let Some(task_id) = handler_ctx.task_id.as_deref() {
            let steering = take_steering(handler_ctx.db, task_id).await;
            if !steering.is_empty() {
                let combined = steering.join("\n");
                context.variables.insert(
                    "_steering_instruction".to_string(),
                    Value::String(combined.clone()),
                );
                context.user_intent = if context.user_intent.is_empty() {
                    combined.clone()
                } else {
                    format!("{}\nSteering: {}", context.user_intent, combined)
                };
                tracing::info!(
                    task_id = %task_id,
                    step_id = %step.id,
                    "[Executor] Applied steering instruction at step boundary"
                );
            }
        }

        // Skill 执行：以 "skill:" 开头的 capability_id 由 Skill 系统处理
        if let Some(skill_id) = step.capability_id.strip_prefix("skill:") {
            return self
                .execute_skill_step(skill_id, step, context, handler_ctx)
                .await;
        }

        // 获取能力定义
        let capability =
            crate::services::agent::capability::get_capability_by_id(&step.capability_id)
                .await
                .ok_or_else(|| format!("Unknown capability: {}", step.capability_id))?;

        // 权限校验：检查 capability 声明的 required_permissions
        if !capability.required_permissions.is_empty() {
            let user_perms =
                crate::services::agent::get_user_permissions(handler_ctx.db, handler_ctx.user_id)
                    .await;
            for perm in &capability.required_permissions {
                if !user_perms.contains(perm) {
                    return Err(format!(
                        "权限不足：执行 '{}' 需要 '{}' 权限",
                        step.capability_id, perm
                    ));
                }
            }
        }

        // 敏感操作补检：动态子步骤（技能展开/动态分析生成）绕过了 Planner 层的
        // check_sensitive_steps 确认流程。高风险/不可逆操作不允许在无确认的情况下
        // 由动态步骤自动执行（系统任务除外，其确认策略在 Agent::process 统一处理）
        if context.is_dynamic_step(&step.id) {
            if let Some((_, risk)) =
                crate::services::agent::capability::capability_requires_confirmation_async(
                    &step.capability_id,
                )
                .await
            {
                let blocked =
                    Self::should_block_unconfirmed_dynamic_step(handler_ctx.user_id, risk);
                if blocked {
                    tracing::warn!(
                        step_id = %step.id,
                        capability = %step.capability_id,
                        risk = ?risk,
                        "[Executor] Blocked unconfirmed high-risk dynamic step"
                    );
                    return Err(format!(
                        "步骤 '{}' 涉及未经确认的高风险操作（{}），动态生成的子步骤不允许自动执行",
                        step.id, step.capability_id
                    ));
                }
            }
        }

        // 解析参数
        let (mut resolved_params, unresolved) =
            self.resolve_params(&step.params, &context.step_outputs);

        // 🔑 注入主 Agent 的具体指令：step.action 是 Planner 对这个子步骤的直接命令
        // 让 AI handler 知道「主 Agent 要求我做什么」，而不是自行发挥
        if !step.action.is_empty() && !resolved_params.contains_key("__directive") {
            resolved_params.insert("__directive".to_string(), json!(step.action));
        }
        // 注入用户原始请求，让 handler 知道最终用户的意图
        if !context.original_request.is_empty() && !resolved_params.contains_key("__user_request") {
            resolved_params.insert(
                "__user_request".to_string(),
                json!(context.original_request),
            );
        }
        if let Some(steering) = context.variables.get("_steering_instruction") {
            resolved_params
                .entry("__steering".to_string())
                .or_insert_with(|| steering.clone());
        }

        if !unresolved.is_empty() {
            tracing::warn!(
                step_id = %step.id,
                unresolved = ?unresolved,
                "[Executor] Some parameters could not be resolved"
            );
        }

        // 应用能力特定的参数回退逻辑
        self.apply_capability_param_fallbacks(
            &step.capability_id,
            &mut resolved_params,
            &context.step_outputs,
        );

        // 根据能力类别和预估时长确定超时（秒），预估时长取3倍作为缓冲
        // 优先使用 RecipeStep 指定的 timeout_ms，否则用能力声明推断
        let timeout_secs = step
            .timeout_ms
            .map(|ms| (ms / 1000).clamp(10, 300))
            .unwrap_or_else(|| {
                capability
                    .estimated_duration_ms
                    .map(|ms| (ms * 3 / 1000).clamp(10, 300))
                    .unwrap_or_else(|| match &capability.category {
                        CapabilityCategory::AiProcess | CapabilityCategory::ResourceCreate => 120,
                        CapabilityCategory::ExternalIntegration => 30,
                        _ => 30,
                    })
            });
        // AI 类能力最少给 60 秒（Pro 模型处理复杂输入+长文生成经常需要 40-50s）
        let timeout_secs = if capability.requires_ai && timeout_secs < 60 {
            60
        } else {
            timeout_secs
        };
        let capability_category = capability.category.clone();

        tracing::debug!(
            step_id = %step.id,
            capability = %step.capability_id,
            timeout_secs = timeout_secs,
            params = ?resolved_params,
            "[Executor] Executing step"
        );

        // 分发到具体 handler（带超时保护）
        tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            handlers::execute_capability(
                &step.capability_id,
                &step.action,
                &capability_category,
                &resolved_params,
                handler_ctx,
            ),
        )
        .await
        .map_err(|_| {
            tracing::error!(
                step_id = %step.id,
                capability = %step.capability_id,
                timeout_secs = timeout_secs,
                "[Executor] Step timed out"
            );
            crate::services::agent::response_agent::step_timeout(&step.capability_id, timeout_secs)
        })?
    }

    /// 执行 Skill 步骤
    ///
    /// Skill 的 full_instructions 包含执行策略（自然语言描述的步骤编排），
    /// 通过 AI 将其转化为具体的能力调用序列并动态注入执行上下文。
    async fn execute_skill_step(
        &self,
        skill_id: &str,
        step: &RecipeStep,
        context: &mut ExecutionContext,
        handler_ctx: &HandlerContext<'_>,
    ) -> Result<Value, String> {
        // 从注册表加载 Skill
        let registry = crate::services::agent::skill::get_skill_registry()
            .ok_or("Skill registry not initialized")?;
        let skill = registry
            .get(skill_id)
            .await
            .ok_or_else(|| format!("Unknown skill: {}", skill_id))?;

        tracing::info!(
            skill_id = skill_id,
            skill_name = %skill.name,
            "[Executor] Executing skill"
        );

        // 检查 gating（前置条件）
        if !skill.gating.capabilities.is_empty() {
            let cap_registry = get_registry().await;
            for required_cap in &skill.gating.capabilities {
                if cap_registry.get(required_cap).is_none() {
                    return Err(format!(
                        "Skill '{}' requires capability '{}' which is not available",
                        skill.name, required_cap
                    ));
                }
            }
        }

        // 用 AI 将 Skill instructions 转化为执行计划
        let analyzer = handler_ctx
            .ai_analyzer
            .ok_or("AI analyzer not available for skill execution")?;

        // 构建可用能力列表（仅 Skill gating 中声明的 + 通用 AI 能力）
        let available_caps = {
            let cap_registry = get_registry().await;
            let mut caps_desc = String::new();
            let gating_caps = &skill.gating.capabilities;
            let all_caps = cap_registry.get_all();
            for cap in &all_caps {
                if gating_caps.contains(&cap.id) || cap.id.starts_with("ai.") {
                    // 提取 required params
                    let params_hint = cap
                        .input_schema
                        .get("required")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    caps_desc.push_str(&format!(
                        "- `{}`: {} (参数: {})\n",
                        cap.id,
                        cap.description,
                        if params_hint.is_empty() {
                            "无必需"
                        } else {
                            &params_hint
                        }
                    ));
                }
            }
            drop(cap_registry);
            caps_desc
        };

        // 构建用户上下文（原始请求 + 对话历史摘要 + 中途转向指令）
        let user_context = {
            let mut ctx_parts = Vec::new();
            ctx_parts.push(format!("用户原始请求: {}", context.original_request));
            // Steering taken at the skill planning step boundary must reshape the plan.
            if let Some(steering) = context
                .variables
                .get("_steering_instruction")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                ctx_parts.push(format!(
                    "用户最新转向指令（优先遵循，调整后续步骤参数与目标）：{}",
                    steering
                ));
            } else if !context.user_intent.is_empty()
                && context.user_intent != context.original_request
            {
                // user_intent may already include "Steering: ..." from the step boundary.
                if context.user_intent.contains("Steering:") {
                    ctx_parts.push(format!("更新后的用户意图: {}", context.user_intent));
                }
            }
            if let Some(conv) = &context.conversation_context {
                let recent: Vec<String> = conv
                    .iter()
                    .rev()
                    .take(3)
                    .rev()
                    .map(|m| {
                        format!(
                            "[{}]: {}",
                            m.role,
                            m.content.chars().take(200).collect::<String>()
                        )
                    })
                    .collect();
                if !recent.is_empty() {
                    ctx_parts.push(format!("最近对话:\n{}", recent.join("\n")));
                }
            }
            ctx_parts.join("\n\n")
        };

        // 将 ${param} 槽位替换为 step.params 中的实际值
        let resolved_instructions = {
            let mut text = skill.full_instructions.clone();
            for (key, value) in &step.params {
                let placeholder = format!("${{{}}}", key);
                let replacement = match value {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                text = text.replace(&placeholder, &replacement);
            }
            text
        };

        tracing::info!(
            skill_id = skill_id,
            step_params = %serde_json::to_string(&step.params).unwrap_or_default(),
            has_context = !user_context.is_empty(),
            params_resolved = resolved_instructions != skill.full_instructions,
            "[Executor] Skill planning with params and context"
        );

        // 收集已有的搜索/分析输出（避免重复 webSearch，并让 Skill AI 知道上游数据）
        let prior_knowledge = {
            let mut knowledge_parts = Vec::new();
            for (out_id, out_val) in context.get_all_outputs() {
                // 收集各类有意义的输出（搜索摘要、分析结果等）
                let text = if let Some(summary) = out_val.get("aiSummary").and_then(|v| v.as_str())
                {
                    Some(summary)
                } else if let Some(analysis) = out_val.get("analysis").and_then(|v| v.as_str()) {
                    Some(analysis)
                } else {
                    out_val.get("reply").and_then(|v| v.as_str())
                };
                if let Some(text) = text {
                    let truncated: String = text.chars().take(1500).collect();
                    knowledge_parts.push(format!("[{}] {}", out_id, truncated));
                }
            }
            if knowledge_parts.is_empty() {
                String::new()
            } else {
                format!(
                    "\n## 已有上游数据（直接复用，不要重复搜索或分析相同内容）\n{}\n",
                    knowledge_parts.join("\n\n")
                )
            }
        };

        // 处理 count/variations 参数 → 注入到 Skill prompt
        let count_hint = {
            let count = step
                .params
                .get("count")
                .and_then(|v| v.as_u64())
                .unwrap_or(1);
            let variations = step.params.get("variations").and_then(|v| v.as_array());
            if count > 1 || variations.is_some() {
                let mut hint = format!("\n## 数量与变体要求\n用户需要 {} 组不同的结果。", count);
                if let Some(vars) = variations {
                    let descs: Vec<String> = vars
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    hint.push_str(&format!(
                        "\n变体描述：\n{}",
                        descs
                            .iter()
                            .enumerate()
                            .map(|(i, d)| format!("{}. {}", i + 1, d))
                            .collect::<Vec<_>>()
                            .join("\n")
                    ));
                }
                hint.push_str(
                    "\n\n**重要**：搜索/调研步骤只执行一次，结果被所有变体共享。\
                为每个变体分别生成独立的 prompt.generate + ai.image 步骤对。\n",
                );
                hint
            } else {
                String::new()
            }
        };

        let prompt = format!(
            "你是 Myriad 的 DAG 执行计划编排器。\n\
             你的任务：根据 Skill 策略和用户需求，输出一个 JSON 执行计划（步骤的有向无环图）。\n\
             引擎会根据 `depends_on` 自动调度：依赖已满足的步骤**立即并行启动**，无需你操心并行逻辑。\n\
             你只需把依赖关系写对。\n\n\
             ## Skill: {name}\n{desc}\n\n\
             ## 用户上下文\n{context}\n\n\
             ## 可用能力（只能使用这些 capability_id）\n{caps}\n\
             ## 执行策略\n{instructions}\n\n\
             ## 用户参数\n{params}\n\
             {prior_knowledge}\
             {count_hint}\n\
             ---\n\n\
             # 规则\n\n\
             ## 一、结构\n\
             1. `capability_id` 只能从上面的列表选择\n\
             2. 每个 step 必须有唯一 `id`（简短标识，如 `search_info`, `gen_prompt_1`）\n\
             3. 每个 step 必须有 `depends_on` 数组（无依赖写 `[]`）\n\
             4. `action` 字段写该步骤的具体目标（展示给用户看）\n\
             5. 最多 8 个步骤。只返回纯 JSON，不要 markdown 包裹\n\n\
             ## 二、搜索优先\n\
             6. **情报优先**：当任务涉及你不完全确定的外部知识（角色外貌、事件细节、专业信息等），\
             **必须先 ai.webSearch 获取情报**，所有后续步骤都依赖它。搜索是为了让后续生成更准确。\n\
             7. 搜索步骤全计划最多 1 个，多变体共享搜索结果。\
             如「已有搜索结果」已包含所需信息，则不再搜索。\n\n\
             ## 三、依赖 = 执行顺序\n\
             8. `depends_on: []` → 立即执行（与其他无依赖步骤并行）\n\
             9. `depends_on: [\"X\"]` → 等待 X 完成后执行（多个步骤 depends_on 同一个 X → 它们同时并行）\n\
             10. **黄金法则**：如果 step B 需要用到 step A 的输出/情报 → B 必须 `depends_on: [\"A\"]`\n\n\
             ## 四、数据流（xxxFrom）\n\
             11. 后续步骤使用前序输出：在 params 中写 `\"<字段名>From\": \"<step_id>\"`\n\
             12. 引擎自动把 `promptFrom: \"gen_prompt_1\"` 解析为：取 gen_prompt_1 的输出注入到 `prompt` 参数\n\
             13. **严禁** $$variable$$ 语法或模板占位符。params 值要么是具体文本，要么用 xxxFrom 引用\n\n\
             ---\n\n\
             # 示例（3 张角色图，需要搜索角色信息）\n\n\
             ```json\n\
             {{\n\
               \"steps\": [\n\
                 {{\"id\": \"search\",      \"capability_id\": \"ai.webSearch\",   \"action\": \"搜索角色外貌特征\",      \"params\": {{\"query\": \"...\"}},             \"depends_on\": []}},\n\
                 {{\"id\": \"prompt_1\",    \"capability_id\": \"prompt.generate\", \"action\": \"生成变体1提示词\",       \"params\": {{\"description\": \"...\"}},       \"depends_on\": [\"search\"]}},\n\
                 {{\"id\": \"prompt_2\",    \"capability_id\": \"prompt.generate\", \"action\": \"生成变体2提示词\",       \"params\": {{\"description\": \"...\"}},       \"depends_on\": [\"search\"]}},\n\
                 {{\"id\": \"prompt_3\",    \"capability_id\": \"prompt.generate\", \"action\": \"生成变体3提示词\",       \"params\": {{\"description\": \"...\"}},       \"depends_on\": [\"search\"]}},\n\
                 {{\"id\": \"img_1\",       \"capability_id\": \"ai.image\",        \"action\": \"生成变体1图片\",         \"params\": {{\"promptFrom\": \"prompt_1\"}},   \"depends_on\": [\"prompt_1\"]}},\n\
                 {{\"id\": \"img_2\",       \"capability_id\": \"ai.image\",        \"action\": \"生成变体2图片\",         \"params\": {{\"promptFrom\": \"prompt_2\"}},   \"depends_on\": [\"prompt_2\"]}},\n\
                 {{\"id\": \"img_3\",       \"capability_id\": \"ai.image\",        \"action\": \"生成变体3图片\",         \"params\": {{\"promptFrom\": \"prompt_3\"}},   \"depends_on\": [\"prompt_3\"]}}\n\
               ]\n\
             }}\n\
             ```\n\
             执行流：search(独占) → prompt_1+prompt_2+prompt_3(并行) → 各自的 img 在 prompt 完成后立即启动\n\n\
             只返回 JSON，不要任何解释文字。",
            name = skill.name,
            desc = skill.description,
            context = user_context,
            caps = available_caps,
            instructions = resolved_instructions,
            params = serde_json::to_string_pretty(&step.params).unwrap_or_default(),
            prior_knowledge = prior_knowledge,
            count_hint = count_hint,
        );

        let ai_result = analyzer
            .analyze(&prompt)
            .await
            .map_err(|e| format!("Skill AI planning failed: {}", e))?;

        tracing::debug!(
            skill_id = skill_id,
            ai_plan_len = ai_result.len(),
            "[Executor] Skill AI planning result: {}",
            ai_result.chars().take(500).collect::<String>()
        );

        // 解析 AI 返回的步骤计划（从文本中提取 JSON）
        let plan: Value = {
            // 尝试找到 JSON 块
            let text = ai_result.trim();
            let json_str = if let Some(start) = text.find('{') {
                if let Some(end) = text.rfind('}') {
                    &text[start..=end]
                } else {
                    text
                }
            } else {
                text
            };
            match serde_json::from_str(json_str) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(
                        skill_id = skill_id,
                        error = %e,
                        response_preview = truncate_str(json_str, 200),
                        "[Executor] Skill AI JSON parse failed"
                    );
                    return Err(format!(
                        "Skill '{}' AI planning returned unparseable response: {}",
                        skill.name, e
                    ));
                }
            }
        };

        let planned_steps = plan
            .get("steps")
            .and_then(|s| s.as_array())
            .cloned()
            .unwrap_or_default();

        if planned_steps.is_empty() {
            return Err(format!(
                "Skill '{}' AI planning returned no executable steps",
                skill.name
            ));
        }

        // 步骤上限
        let planned_steps: Vec<&Value> = planned_steps.iter().take(8).collect();

        // 验证并构建动态步骤（两遍扫描：第一遍建立 id 映射，第二遍解析引用）
        let cap_registry = get_registry().await;
        // AI step id → 实际 step id 映射（用于 depends_on 和 xxxFrom 引用重写）
        let mut id_map: HashMap<String, String> = HashMap::new();

        // 预注入父级步骤 ID → 自身映射，让 skill 子步骤的 xxxFrom 能引用父级输出
        // 父级步骤已完成，不需要在 DAG 中等待，但 xxxFrom 参数解析需要它们
        let parent_step_ids: std::collections::HashSet<String> =
            context.get_all_outputs().keys().cloned().collect();
        for parent_id in &parent_step_ids {
            id_map.insert(parent_id.clone(), parent_id.clone());
        }
        // 记录哪些步骤被跳过（gating/无效），用于依赖断裂检测
        let mut skipped_indices: std::collections::HashSet<usize> =
            std::collections::HashSet::new();
        // 被跳过的 AI step id 集合（用于 pass 2 检测依赖断裂）
        let mut skipped_ai_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        // ===== 第一遍：验证 capability_id + 建立完整 id_map =====
        for (i, planned) in planned_steps.iter().enumerate() {
            let cap_id = planned
                .get("capability_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // 验证 capability_id
            if cap_id.is_empty()
                || (!cap_id.starts_with("skill:")
                    && !cap_id.starts_with("mcp.")
                    && cap_registry.get(cap_id).is_none())
            {
                tracing::warn!(
                    skill_id = skill_id,
                    step_index = i,
                    invalid_cap = cap_id,
                    "[Executor] Skill step generated invalid capability_id, skipping"
                );
                if i == 0 {
                    return Err(format!(
                        "Skill '{}' generated invalid first step capability: '{}'",
                        skill_id, cap_id
                    ));
                }
                skipped_indices.insert(i);
                // 记录被跳过步骤的 AI id，用于依赖断裂检测
                if let Some(ai_id) = planned.get("id").and_then(|v| v.as_str()) {
                    if !ai_id.is_empty() {
                        skipped_ai_ids.insert(ai_id.to_string());
                    }
                }
                continue;
            }

            // Gating 校验
            if !skill.gating.capabilities.is_empty()
                && !cap_id.starts_with("ai.")
                && !cap_id.starts_with("skill:")
                && !cap_id.starts_with("mcp.")
                && !skill.gating.capabilities.contains(&cap_id.to_string())
            {
                tracing::warn!(
                    skill_id = skill_id,
                    cap_id = cap_id,
                    "[Executor] Skill step uses capability outside gating scope, skipping"
                );
                skipped_indices.insert(i);
                if let Some(ai_id) = planned.get("id").and_then(|v| v.as_str()) {
                    if !ai_id.is_empty() {
                        skipped_ai_ids.insert(ai_id.to_string());
                    }
                }
                continue;
            }

            // 预注册 AI step id → 实际 step id 映射（确保前向引用可解析）
            let ai_step_id = planned.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let step_id = format!("{}_{}_step_{}", step.id, skill_id, i);
            if !ai_step_id.is_empty() {
                id_map.insert(ai_step_id.to_string(), step_id);
            }
        }

        // ===== 第二遍：解析引用 + 构建动态步骤 =====
        let mut dynamic_steps = Vec::with_capacity(planned_steps.len());

        for (i, planned) in planned_steps.iter().enumerate() {
            if skipped_indices.contains(&i) {
                continue;
            }

            let cap_id = planned
                .get("capability_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let action = planned
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("execute");
            let params: HashMap<String, Value> = planned
                .get("params")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let ai_step_id = planned.get("id").and_then(|v| v.as_str()).unwrap_or("");

            // depends_on: 重写 AI step id + 检测依赖断裂
            let mut has_broken_dep = false;
            let depends_on = planned
                .get("depends_on")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| {
                    let dep = v.as_str()?;
                    // 检查是否引用了被跳过的步骤
                    if skipped_ai_ids.contains(dep) {
                        tracing::warn!(
                            dependency = %dep,
                            step_id = ai_step_id,
                            "[Executor] Skill sub-step depends on skipped step '{}', data flow broken",
                            dep
                        );
                        has_broken_dep = true;
                        return None;
                    }
                    // 父级步骤已完成，无需在 DAG 中等待（数据通过 xxxFrom 参数解析获取）
                    if parent_step_ids.contains(dep) {
                        return None;
                    }
                    if let Some(actual) = id_map.get(dep) {
                        Some(actual.clone())
                    } else {
                        tracing::warn!(
                            dependency = %dep,
                            "[Executor] Skill sub-step depends_on references unknown step id '{}', dropping dependency",
                            dep
                        );
                        None
                    }
                }).collect::<Vec<_>>())
                .unwrap_or_default();

            if has_broken_dep {
                tracing::warn!(
                    skill_id = skill_id,
                    step_id = ai_step_id,
                    "[Executor] Skill step depends on a skipped step, data flow may be incomplete"
                );
            }

            // 重写 params 中的 xxxFrom 引用
            let resolved_params: HashMap<String, Value> = params.into_iter().map(|(k, v)| {
                if k.ends_with("From") {
                    if let Some(ref_str) = v.as_str() {
                        let parts: Vec<&str> = ref_str.splitn(2, '.').collect();
                        if let Some(actual_id) = id_map.get(parts[0]) {
                            let new_ref = if parts.len() > 1 {
                                format!("{}.{}", actual_id, parts[1])
                            } else {
                                actual_id.clone()
                            };
                            return (k, Value::String(new_ref));
                        }
                        tracing::warn!(
                            param = %k,
                            reference = %ref_str,
                            "[Executor] xxxFrom references unknown step id '{}', passing literal value",
                            parts[0]
                        );
                    }
                }
                (k, v)
            }).collect();

            let on_failure = if i == 0 && !cap_id.starts_with("ai.") {
                types::FailureStrategy::Abort
            } else {
                types::FailureStrategy::Skip
            };

            let step_id = format!("{}_{}_step_{}", step.id, skill_id, i);

            dynamic_steps.push(RecipeStep {
                id: step_id,
                order: (step.order * 100) + (i as u32),
                capability_id: cap_id.to_string(),
                action: action.to_string(),
                params: resolved_params,
                depends_on,
                on_failure,
                retry: None,
                timeout_ms: Some(60000),
                model_tier: skill.tier_hint.as_ref().map(|h| h.to_model_tier()),
                generator: None,
            });
        }
        drop(cap_registry);

        if dynamic_steps.is_empty() {
            return Err(format!(
                "Skill '{}' all generated steps had invalid capability_ids",
                skill.name
            ));
        }

        let steps_count = dynamic_steps.len();
        // 收集子步骤 ID，供下游 resolve_path_reference 聚合真实输出
        let substep_ids: Vec<String> = dynamic_steps.iter().map(|s| s.id.clone()).collect();
        context.queue_dynamic_steps(dynamic_steps);

        tracing::info!(
            skill_id = skill_id,
            dynamic_steps = steps_count,
            "[Executor] Skill generated {} dynamic steps",
            steps_count
        );

        let last_output = json!({
            "skill": skill_id,
            "status": "planned",
            "dynamic_steps_generated": steps_count,
            "__substep_ids": substep_ids,
        });

        Ok(last_output)
    }

    /// 解析参数中的引用
    /// 构建 StepDebug 事件用的参数预览（截断超长字符串）
    fn build_debug_params(params: &HashMap<String, Value>) -> Option<Value> {
        let mut p = params.clone();
        for v in p.values_mut() {
            if let Some(s) = v.as_str() {
                if s.len() > 500 {
                    // truncate_str 保证不在多字节字符中间截断
                    *v = json!(format!(
                        "{}...({}chars)",
                        utils::truncate_str(s, 500),
                        s.len()
                    ));
                }
            }
        }
        serde_json::to_value(&p).ok()
    }

    /// 能力特定的参数回退：当 resolve_params 无法填充某个必要参数时，
    /// 从前置步骤输出中尝试语义搜索。每个能力的回退逻辑集中在此处，
    /// 避免污染主执行路径。
    fn apply_capability_param_fallbacks(
        &self,
        capability_id: &str,
        params: &mut HashMap<String, Value>,
        previous_outputs: &HashMap<String, Value>,
    ) {
        if capability_id == "music.playlist" {
            let has_playlist_id = params
                .get("playlistId")
                .map(|v| {
                    v.as_str().map(|s| !s.is_empty()).unwrap_or(false)
                        || v.as_i64().is_some()
                        || v.as_u64().is_some()
                })
                .unwrap_or(false);

            if !has_playlist_id {
                tracing::info!(
                    "[Executor] music.playlist missing playlistId, searching previous outputs"
                );
                for output in previous_outputs.values() {
                    if let Some(pid) = output
                        .get("recommendedPlaylistId")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        tracing::info!(playlist_id = %pid, "[Executor] Found recommendedPlaylistId");
                        params.insert("playlistId".to_string(), json!(pid));
                        break;
                    }
                    if let Some(first) = output
                        .get("playlists")
                        .and_then(|p| p.as_array())
                        .and_then(|arr| arr.first())
                    {
                        let id_opt = first.get("id").and_then(|id| {
                            id.as_i64()
                                .map(|n| n.to_string())
                                .or_else(|| id.as_str().map(String::from))
                        });
                        if let Some(id_str) = id_opt {
                            tracing::info!(playlist_id = %id_str, "[Executor] Found playlistId from playlists[0]");
                            params.insert("playlistId".to_string(), json!(id_str));
                            break;
                        }
                    }
                }
            }
        }
    }

    fn resolve_params(
        &self,
        params: &HashMap<String, Value>,
        previous_outputs: &HashMap<String, Value>,
    ) -> (HashMap<String, Value>, Vec<String>) {
        let mut resolved = HashMap::new();
        let mut unresolved = Vec::new();

        for (key, value) in params {
            if key.ends_with("From") {
                if let Some(ref_str) = value.as_str() {
                    let resolved_value = self.resolve_path_reference(ref_str, previous_outputs);
                    if let Some(output) = resolved_value {
                        let new_key = key.trim_end_matches("From").to_string();

                        // 数据型参数（data, content, input, context）保留完整对象，
                        // 因为下游 handler（如 ai.analyze）有自己的 extract_semantic_text 逻辑
                        // 能处理结构化数据（如 results 数组、嵌套字段等）。
                        //
                        // 文本型参数（title, description, summary, prompt 等）提取语义文本，
                        // 因为下游期望的是纯字符串。
                        let is_data_param = matches!(
                            new_key.as_str(),
                            "data" | "content" | "input" | "context" | "items"
                        );

                        // ID 型参数（playlistId、songId、id 等）：引用结构化输出时
                        // 必须提取真实 ID，绝不能走语义文本提取——后者会把
                        // message 提示文案当 ID 用（歌单播放曾因此拿到
                        // "找到 10 个…" 字符串，任务"成功"但前端加载必败）
                        let is_id_param = new_key == "id"
                            || new_key.ends_with("Id")
                            || new_key.ends_with("ID")
                            || new_key.ends_with("_id");

                        let final_value = if output.is_string() || is_data_param {
                            Some(output)
                        } else if is_id_param && (output.is_object() || output.is_array()) {
                            // 提取失败按未解析处理：让步骤显式报错，
                            // 而不是塞进错误的值静默错下去
                            Self::extract_id_from_output(&output, &new_key)
                        } else if output.is_object() {
                            let text = Self::extract_text_from_output(&output);
                            if text.is_empty() {
                                Some(output)
                            } else {
                                Some(Value::String(text))
                            }
                        } else {
                            Some(output)
                        };

                        match final_value {
                            Some(v) => {
                                resolved.insert(new_key, v);
                                continue;
                            }
                            None => unresolved.push(format!("{}: {}", key, ref_str)),
                        }
                    } else {
                        unresolved.push(format!("{}: {}", key, ref_str));
                    }
                }
            }

            resolved.insert(key.clone(), value.clone());
        }

        (resolved, unresolved)
    }

    /// 从结构化步骤输出中提取 ID 型参数值。
    /// 优先级：同名字段 → 顶层 id → 常见列表字段第一项的 id / 同名字段。
    /// 数组输入取第一个元素递归提取
    fn extract_id_from_output(output: &Value, param_key: &str) -> Option<Value> {
        fn id_like(v: &Value) -> bool {
            v.is_string() || v.is_number()
        }

        match output {
            Value::Array(arr) => arr
                .first()
                .and_then(|first| Self::extract_id_from_output(first, param_key)),
            Value::Object(obj) => {
                // 1. 同名字段（如 playlistId）
                if let Some(v) = obj.get(param_key).filter(|v| id_like(v)) {
                    return Some(v.clone());
                }
                // 2. 顶层 id
                if let Some(v) = obj.get("id").filter(|v| id_like(v)) {
                    return Some(v.clone());
                }
                // 3. 常见列表字段的第一项（搜索类输出：playlists/results/…）
                for list_key in ["playlists", "results", "items", "list", "songs", "data"] {
                    if let Some(first) = obj
                        .get(list_key)
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.first())
                    {
                        if let Some(v) = first.get(param_key).filter(|v| id_like(v)) {
                            return Some(v.clone());
                        }
                        if let Some(v) = first.get("id").filter(|v| id_like(v)) {
                            return Some(v.clone());
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// 从步骤输出的 JSON 对象中提取主要文本内容
    fn extract_text_from_output(output: &Value) -> String {
        let text_keys = [
            "analysis",
            "reply",
            "aiSummary",
            "summary",
            "description",
            "message",
            "content",
            "prompt",
        ];
        if let Some(obj) = output.as_object() {
            for key in &text_keys {
                if let Some(text) = obj.get(*key).and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        return text.to_string();
                    }
                }
            }
        }
        String::new()
    }

    /// 解析路径引用
    fn resolve_path_reference(
        &self,
        ref_str: &str,
        previous_outputs: &HashMap<String, Value>,
    ) -> Option<Value> {
        // 支持 "step_id" 或 "step_id.path.to.value" 或 "step_id.array[0].field"
        let parts: Vec<&str> = ref_str.splitn(2, '.').collect();
        let step_id = parts[0];

        let output = previous_outputs.get(step_id)?;

        // Skill 步骤特殊处理：输出中 __substep_ids 表示这是技能占位符，
        // 用最后一个已完成子步骤的真实输出替代无意义的 "planned" 元数据
        let effective_output =
            if let Some(substep_ids) = output.get("__substep_ids").and_then(|v| v.as_array()) {
                let mut last: Option<&Value> = None;
                for id_val in substep_ids {
                    if let Some(id) = id_val.as_str() {
                        if let Some(sub_output) = previous_outputs.get(id) {
                            last = Some(sub_output);
                        }
                    }
                }
                match last {
                    Some(sub_out) => std::borrow::Cow::Borrowed(sub_out),
                    None => std::borrow::Cow::Borrowed(output),
                }
            } else {
                std::borrow::Cow::Borrowed(output)
            };

        if parts.len() == 1 {
            return Some(effective_output.into_owned());
        }

        // 解析路径
        let path = parts[1];
        self.get_value_by_path(&effective_output, path)
    }

    /// 通过路径获取值
    fn get_value_by_path(&self, value: &Value, path: &str) -> Option<Value> {
        let mut current = value;

        for segment in path.split('.') {
            // 检查是否有数组索引（如 field[0]）
            if let Some(bracket_pos) = segment.find('[') {
                let field_name = &segment[..bracket_pos];
                // 确保有闭合的 ] 且索引部分非空
                if !segment.ends_with(']') || bracket_pos + 1 >= segment.len() - 1 {
                    return None;
                }
                let index_str = &segment[bracket_pos + 1..segment.len() - 1];

                if !field_name.is_empty() {
                    current = current.get(field_name)?;
                }

                let index: usize = index_str.parse().ok()?;
                current = current.get(index)?;
            } else {
                current = current.get(segment)?;
            }
        }

        Some(current.clone())
    }

    // ========================================================================
    // 动态步骤生成系统
    // ========================================================================

    /// 用户回答后恢复执行
    pub async fn resume_with_answer(
        &self,
        task_id: &str,
        answer: UserAnswer,
        recipe: &Recipe,
        user_id: i32,
        progress_tx: Option<tokio::sync::mpsc::Sender<types::AgentProgressEvent>>,
    ) -> Result<TaskState, String> {
        // 先复制持久化恢复态；答案校验和 context 变换都只作用于这份副本。
        // 真正开始执行前再通过数据库 CAS 争抢跨副本 resume 权。
        let mut task_state = {
            let store = TASK_STORE.read().await;
            let task = store.get(task_id).ok_or("Task not found")?;
            if task.status != TaskStatus::WaitingForInput {
                return Err("Task is not waiting for input".to_string());
            }
            task.clone()
        };

        // 优先使用 task 上保存的 recipe（可能已写入先前 pre_param 答案），否则用调用方传入的
        let mut recipe = task_state
            .recipe
            .clone()
            .unwrap_or_else(|| recipe.clone());

        // 恢复执行上下文
        let mut context = task_state.execution_context.take().unwrap_or_default();
        context.variables.insert(
            "_task_id".to_string(),
            Value::String(task_state.task_id.clone()),
        );

        // 清理已过期的排队问题，避免 resume 后发送过期问题
        let now = chrono::Utc::now();
        let before_len = context.pending_questions.len();
        context
            .pending_questions
            .retain(|q| q.expires_at.is_none_or(|exp| now <= exp));
        if context.pending_questions.len() < before_len {
            tracing::info!(
                task_id = %task_id,
                removed = before_len - context.pending_questions.len(),
                "[Executor] Cleaned {} expired pending questions on resume",
                before_len - context.pending_questions.len()
            );
        }

        // 根据回答类型处理，返回是否应跳过后续步骤
        let should_skip = if let Some(question) = &task_state.pending_question {
            // 校验 answer.question_id 是否匹配当前待回答的问题
            if answer.question_id != question.question_id {
                // 宽容模式：如果只有唯一一个待回答问题，接受不匹配的 answer
                // （前端可能缓存了旧的 question_id 格式）
                if context.pending_questions.is_empty() {
                    tracing::warn!(
                        task_id = %task_id,
                        expected = %question.question_id,
                        actual = %answer.question_id,
                        "[Executor] Answer question_id mismatch but only one pending question, accepting anyway"
                    );
                } else {
                    tracing::warn!(
                        task_id = %task_id,
                        expected = %question.question_id,
                        actual = %answer.question_id,
                        "[Executor] Answer question_id mismatch with multiple pending questions, rejecting"
                    );
                    return Err(format!(
                        "Question ID mismatch: expected {}, got {}",
                        question.question_id, answer.question_id
                    ));
                }
            }
            // 检查问题是否已过期（过期答案不记录，避免污染 answered_questions）
            if let Some(expires_at) = question.expires_at {
                if chrono::Utc::now() > expires_at {
                    tracing::warn!(
                        task_id = %task_id,
                        question_id = %question.question_id,
                        "[Executor] Question expired, skipping answer processing and continuing execution"
                    );
                    // 通知前端答案已过期
                    if let Some(ref tx) = progress_tx {
                        let _ = tx
                            .send(types::AgentProgressEvent::StepDebug {
                                step_id: format!("question_{}", question.question_id),
                                phase: "expired".to_string(),
                                capability_id: "system.question".to_string(),
                                directive: Some("用户回答已过期，继续执行剩余步骤".to_string()),
                                user_request: None,
                                params: None,
                                output_preview: None,
                                is_dynamic: false,
                                duration_ms: None,
                                success: Some(false),
                                error: Some("回答超时".to_string()),
                            })
                            .await;
                    }
                    context.record_decision(
                        DecisionType::SkipStep,
                        "用户回答已过期，跳过该问题",
                        "问题超时未回答，继续执行剩余步骤",
                        None,
                    );
                    false // 不跳过剩余步骤，继续执行
                } else {
                    // 验证通过后才记录答案
                    context.record_answer(&answer.question_id, &answer.answer);
                    self.process_user_answer(&answer, question, &mut context, &mut recipe)
                        .await
                }
            } else {
                // 无过期时间，直接记录并处理
                context.record_answer(&answer.question_id, &answer.answer);
                self.process_user_answer(&answer, question, &mut context, &mut recipe)
                    .await
            }
        } else {
            tracing::warn!(
                task_id = %task_id,
                "[Executor] resume_with_answer called but pending_question is None; answer may not be processed correctly"
            );
            false
        };

        // 持久化写回参数后的 recipe，供后续 resume / 执行使用
        task_state.recipe = Some(recipe.clone());

        // 用户选择 retry：仅在确实存在错误步骤时触发重试逻辑
        if answer.answer == "retry" {
            if let Some(error_step_id) = context
                .variables
                .get("_error_step_id")
                .and_then(|v| v.as_str())
            {
                let step_id = error_step_id.to_string();
                task_state.step_results.remove(&step_id);
                // 同时清除该步骤的输出，避免旧错误输出影响后续依赖
                context.step_outputs.remove(&step_id);
                // 在 DAG 中重置该步骤状态（如果有 DAG）
                tracing::info!(
                    task_id = %task_id,
                    step_id = %step_id,
                    "[Executor] User chose retry: removed step result for re-execution"
                );
            } else {
                tracing::debug!(
                    task_id = %task_id,
                    "[Executor] User answered 'retry' but no _error_step_id found, treating as normal answer"
                );
            }
            // 清理临时变量
            context.variables.remove("_error_step_id");
        }

        if !claim_task_for_resume(task_id, user_id).await? {
            return Err("Task answer was already claimed by another executor".to_string());
        }
        task_state.status = TaskStatus::Running;
        {
            let mut store = TASK_STORE.write().await;
            if let Some(task) = store.get_mut(task_id) {
                *task = task_state.clone();
            }
        }

        // 清除待回答问题
        task_state.clear_pending_question();

        // 用户选择取消或拒绝确认 → 直接完成任务
        if should_skip {
            tracing::info!(
                task_id = %task_id,
                "[Executor] User declined/cancelled, completing task"
            );
            task_state.status = TaskStatus::Completed;
            task_state.execution_context = Some(context);
            {
                let mut store = TASK_STORE.write().await;
                if let Some(task) = store.get_mut(&task_state.task_id) {
                    *task = task_state.clone();
                }
            }
            persist_task_async(user_id, task_state.clone());
            return Ok(task_state);
        }

        // 仍有 pre_param 排队：先继续问下一个，避免带着缺参 recipe 开跑
        if context
            .pending_questions
            .first()
            .is_some_and(|q| q.question_id.starts_with("pre_param"))
        {
            let question = context.pending_questions.remove(0);
            tracing::info!(
                task_id = %task_id,
                question_id = %question.question_id,
                remaining = context.pending_questions.len(),
                "[Executor] Next pre_param question after answer"
            );
            if let Some(ref tx) = progress_tx {
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
            task_state.set_pending_question(question);
            task_state.recipe = Some(recipe.clone());
            task_state.execution_context = Some(context);
            {
                let mut store = TASK_STORE.write().await;
                if let Some(task) = store.get_mut(&task_state.task_id) {
                    *task = task_state.clone();
                }
            }
            persist_task_async(user_id, task_state.clone());
            return Ok(task_state);
        }

        tracing::info!(
            task_id = %task_id,
            "[Executor] Resuming execution after user answer"
        );

        // 继续执行剩余步骤（使用已写回 pre_param 的 recipe）
        let all_steps: Vec<RecipeStep> = recipe.steps.clone();
        let mut step_index = task_state.current_step;

        // 构建 DAG 调度器（检测是否有并行依赖）
        let mut dag_scheduler = dag::DagScheduler::new(&all_steps).ok();
        let mut use_dag = dag_scheduler.as_ref().is_some_and(|d| d.is_parallel_mode())
            && !all_steps
                .iter()
                .any(|step| step.capability_id == "tapp.interact");

        // 已完成的步骤需要在 DAG 中标记
        if let Some(ref mut dag) = dag_scheduler {
            for step_id in task_state.step_results.keys() {
                dag.mark_completed(step_id);
            }
        }

        // ExecutionTrace 初始化
        let execution_start = std::time::Instant::now();
        let trace_id = format!("trace_{}_resume", task_state.task_id);
        let mut step_traces: Vec<types::StepTrace> = Vec::new();
        let mut tier_usage: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();

        let mut total_executed: usize = 0;
        const MAX_RESUME_STEPS: usize = 50;
        let mut global_retry_budget: u32 = context.retry_budget_remaining;

        // SSE 事件发送器
        let emitter = events::StepEventEmitter::new(progress_tx.clone());

        while step_index < all_steps.len()
            || context.has_pending_steps()
            || dag_scheduler.as_ref().is_some_and(|d| d.has_remaining())
        {
            // 取消检查
            if is_cancelled(&task_state.task_id).await {
                tracing::info!(task_id = %task_id, "[Executor] Resume cancelled by user");
                task_state.status = TaskStatus::Failed;
                task_state.error = Some("用户取消了任务".to_string());
                break;
            }
            // 步骤上限保护
            total_executed += 1;
            if total_executed > MAX_RESUME_STEPS {
                tracing::warn!(task_id = %task_id, "[Executor] Resume exceeded max step limit");
                task_state.error = Some("恢复执行超出步骤上限".to_string());
                break;
            }

            // 判断是否来自动态步骤
            let mut _is_dynamic = false;
            let step = if let Some(dynamic_step) = context.pop_dynamic_step() {
                _is_dynamic = true;
                dynamic_step
            } else if use_dag {
                let ready = dag_scheduler
                    .as_ref()
                    .map(|d| d.get_ready_steps())
                    .unwrap_or_default();
                if let Some(next) = ready.into_iter().next() {
                    if let Some(idx) = all_steps.iter().position(|s| s.id == next.id) {
                        step_index = idx + 1;
                    }
                    next
                } else {
                    break;
                }
            } else if step_index < all_steps.len() {
                let s = all_steps[step_index].clone();
                step_index += 1;
                s
            } else {
                break;
            };

            // 跳过已完成的步骤
            if task_state.step_results.contains_key(&step.id) {
                continue;
            }

            let is_skill_planning = step.capability_id.starts_with("skill:");

            task_state.current_step = step_index;
            let total_steps = all_steps.len() + context.pending_dynamic_steps.len();
            task_state.update_progress(total_steps);

            // 检查依赖
            if !self.check_dependencies(&step, &context.step_outputs) {
                continue;
            }

            // 发送步骤开始事件（Skill 编排步骤不发送）
            if !is_skill_planning {
                let step_description =
                    crate::services::agent::capability::get_step_description(&step);
                emitter
                    .step_started(
                        &step.id,
                        (step_index.saturating_sub(1)) as u32,
                        total_steps as u32,
                        &step_description,
                        crate::services::agent::response_agent::describe_step_start(
                            &step_description,
                        ),
                    )
                    .await;
            }
            emitter
                .debug_start(
                    &step.id,
                    &step.capability_id,
                    if step.action.is_empty() {
                        None
                    } else {
                        Some(step.action.clone())
                    },
                    if context.original_request.is_empty() {
                        None
                    } else {
                        Some(context.original_request.clone())
                    },
                    Self::build_debug_params(&step.params),
                    _is_dynamic,
                )
                .await;

            // 执行步骤（带智能重试）——委托给统一的 retry 模块
            let pre_dynamic_count = context.pending_dynamic_steps.len();
            let step_display_index = (step_index.saturating_sub(1)) as u32;
            let mut retry_config = retry::RetryConfig {
                max_attempts: Self::default_max_retries(&step),
                global_budget: global_retry_budget,
            };
            let event_ctx = retry::RetryEventContext {
                step_display_index,
                progress_tx: progress_tx.clone(),
            };

            let outcome = self
                .execute_step_with_retry(
                    &step,
                    &mut context,
                    user_id,
                    &mut retry_config,
                    &event_ctx,
                )
                .await;
            global_retry_budget = retry_config.global_budget;

            // 注入错误分析器建议的前置步骤
            if !outcome.prepend_steps.is_empty() {
                context.queue_dynamic_steps(outcome.prepend_steps.clone());
            }

            let duration_ms = outcome.duration_ms;

            if outcome.success {
                let output = outcome.output.clone().unwrap_or_default();

                // 发送步骤完成事件（Skill 编排步骤不发送）
                if !is_skill_planning {
                    emitter
                        .step_succeeded(
                            &step.id,
                            step_display_index,
                            duration_ms,
                            summarize_output(&output),
                            extract_image_url(&output),
                        )
                        .await;
                }
                emitter
                    .debug_complete(
                        &step.id,
                        &step.capability_id,
                        _is_dynamic,
                        duration_ms,
                        true,
                        None,
                        None,
                    )
                    .await;

                task_state
                    .step_results
                    .insert(step.id.clone(), outcome.to_step_result(&step.id));
                if let Some(ref mut dag) = dag_scheduler {
                    dag.mark_completed(&step.id);
                }
                if let Some(question) = tapp_interaction_wait_question(&output) {
                    emitter
                        .waiting_for_input(&task_state.task_id, &question)
                        .await;
                    task_state.status = TaskStatus::WaitingForInput;
                    task_state.set_pending_question(question);
                    task_state.recipe = Some(recipe.clone());
                    context.retry_budget_remaining = global_retry_budget;
                    task_state.execution_context = Some(context);
                    {
                        let mut store = TASK_STORE.write().await;
                        if let Some(task) = store.get_mut(&task_state.task_id) {
                            *task = task_state.clone();
                        }
                    }
                    task_store::save_task_to_db(user_id, &task_state)
                        .await
                        .map_err(|error| {
                            format!("persist Tapp interaction wait state failed: {error}")
                        })?;
                    return Ok(task_state);
                }

                // 动态步骤生成器
                if !_is_dynamic {
                    if let Some(ref gen) = step.generator {
                        let generated = self
                            .process_step_generator(gen, &step, &output, &mut context)
                            .await;
                        if !generated.is_empty() {
                            tracing::info!(
                                step_id = %step.id,
                                count = generated.len(),
                                "[Executor] Resume generator produced {} dynamic steps",
                                generated.len()
                            );
                            context.queue_dynamic_steps(generated);
                        }
                    }
                }

                // 动态分析：检查是否需要用户输入
                if !_is_dynamic {
                    if let Some(question) = self
                        .analyze_and_generate_dynamic_steps(&step, &output, &mut context, &recipe)
                        .await
                    {
                        emitter
                            .waiting_for_input(&task_state.task_id, &question)
                            .await;

                        task_state.set_pending_question(question);
                        context.retry_budget_remaining = global_retry_budget;
                        task_state.execution_context = Some(context);

                        let mut store = TASK_STORE.write().await;
                        if let Some(task) = store.get_mut(&task_state.task_id) {
                            *task = task_state.clone();
                        }
                        drop(store);
                        persist_task_async(user_id, task_state.clone());

                        return Ok(task_state);
                    }
                }

                // 动态步骤并行化：注入 DAG 调度器
                let post_dynamic_count = context.pending_dynamic_steps.len();
                if post_dynamic_count > pre_dynamic_count {
                    let new_count = post_dynamic_count - pre_dynamic_count;
                    if new_count > 1 {
                        let new_steps: Vec<RecipeStep> =
                            context.pending_dynamic_steps[pre_dynamic_count..].to_vec();
                        let injected = if let Some(ref mut dag) = dag_scheduler {
                            dag.add_steps(&new_steps).is_ok()
                        } else {
                            match dag::DagScheduler::new(&new_steps) {
                                Ok(new_dag) => {
                                    dag_scheduler = Some(new_dag);
                                    true
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        error = %e,
                                        "[Executor] Resume: failed to create DAG for dynamic steps"
                                    );
                                    false
                                }
                            }
                        };
                        if injected {
                            context.pending_dynamic_steps.drain(pre_dynamic_count..);
                            if !use_dag {
                                use_dag = true;
                                tracing::info!(
                                    task_id = %task_state.task_id,
                                    count = new_count,
                                    "[Executor] Resume: dynamic steps injected into DAG, enabling parallel mode"
                                );
                            } else {
                                tracing::info!(
                                    task_id = %task_state.task_id,
                                    count = new_count,
                                    "[Executor] Resume: dynamic steps injected into existing DAG"
                                );
                            }
                        }
                    }
                }

                task_state
                    .step_results
                    .insert(step.id.clone(), outcome.to_step_result(&step.id));

                if let Some(ref mut dag) = dag_scheduler {
                    dag.mark_completed(&step.id);
                }

                if let Some(evolution) =
                    crate::services::agent::skill_evolution::get_skill_evolution()
                {
                    evolution
                        .on_execution_complete(&step.capability_id, true, None)
                        .await;
                }
            } else {
                // 步骤失败
                let error_msg = outcome.error.as_deref().unwrap_or("unknown error");

                if !is_skill_planning {
                    emitter
                        .step_failed(&step.id, step_display_index, duration_ms, error_msg)
                        .await;
                }
                emitter
                    .debug_complete(
                        &step.id,
                        &step.capability_id,
                        _is_dynamic,
                        duration_ms,
                        false,
                        None,
                        Some(error_msg.to_string()),
                    )
                    .await;

                if let Some(evolution) =
                    crate::services::agent::skill_evolution::get_skill_evolution()
                {
                    evolution
                        .on_execution_complete(&step.capability_id, false, Some(error_msg))
                        .await;
                }

                task_state
                    .step_results
                    .insert(step.id.clone(), outcome.to_step_result(&step.id));

                if let Some(ref mut dag) = dag_scheduler {
                    dag.mark_failed(&step.id, &step.on_failure);
                }
            }

            // 记录步骤追踪
            {
                let tier_str = if TierRouter::requires_llm(&step.capability_id) {
                    format!("{:?}", outcome.last_tier)
                } else {
                    String::new()
                };
                if !tier_str.is_empty() {
                    *tier_usage.entry(tier_str.clone()).or_insert(0) += 1;
                }
                step_traces.push(types::StepTrace {
                    step_id: step.id.clone(),
                    capability_id: step.capability_id.clone(),
                    tier_used: tier_str,
                    duration_ms,
                    success: outcome.success,
                    error: outcome.error.clone(),
                    action: step.action.clone(),
                    params: serde_json::to_value(&step.params).ok(),
                    output_preview: None,
                    is_dynamic: _is_dynamic,
                });
            }
        }

        // 检查是否有排队的待提问（从 DAG 执行阶段延迟的问题）
        // 跳过 resume 执行期间已过期的问题
        let now = chrono::Utc::now();
        context
            .pending_questions
            .retain(|q| q.expires_at.is_none_or(|exp| now <= exp));
        if !context.pending_questions.is_empty() {
            let question = context.pending_questions.remove(0);
            tracing::info!(
                task_id = %task_id,
                question_id = %question.question_id,
                remaining = context.pending_questions.len(),
                "[Executor] Sending next deferred question after resume"
            );

            emitter
                .waiting_for_input(&task_state.task_id, &question)
                .await;

            task_state.status = TaskStatus::WaitingForInput;
            task_state.set_pending_question(question);
            task_state.recipe = Some(recipe.clone());
            context.retry_budget_remaining = global_retry_budget;
            task_state.execution_context = Some(context);

            {
                let mut store = TASK_STORE.write().await;
                if let Some(task) = store.get_mut(&task_state.task_id) {
                    *task = task_state.clone();
                }
            }
            persist_task_async(user_id, task_state.clone());

            return Ok(task_state);
        }

        // The answer/resume path has the same last-step cancellation window as
        // initial execution, including cancellations issued on another replica.
        let cancelled = is_cancelled(&task_state.task_id).await;
        if cancelled {
            clear_cancellation(&task_state.task_id).await;
        }

        // 附加执行追踪
        task_state.execution_trace = Some(types::ExecutionTrace {
            trace_id,
            steps: step_traces,
            total_duration_ms: execution_start.elapsed().as_millis() as u64,
            tier_usage,
            planner_decision: None,
        });

        // 根据步骤结果决定最终状态
        let total_steps = task_state.step_results.len();
        let failed_steps = task_state
            .step_results
            .values()
            .filter(|r| !r.success)
            .count();
        if cancelled {
            task_state.status = TaskStatus::Cancelled;
            task_state.error =
                Some(crate::services::agent::response_agent::task_cancelled_by_user());
        } else if failed_steps > 0 && failed_steps == total_steps {
            task_state.status = TaskStatus::Failed;
            let errors: Vec<String> = task_state
                .step_results
                .values()
                .filter_map(|r| r.error.clone())
                .collect();
            task_state.error = Some(errors.join("; "));
        } else {
            task_state.status = TaskStatus::Completed;
        }
        task_state.completed_at = Some(chrono::Utc::now());
        task_state.progress = 100;

        {
            let mut store = TASK_STORE.write().await;
            if let Some(task) = store.get_mut(&task_state.task_id) {
                *task = task_state.clone();
            }
        }
        persist_task_async(user_id, task_state.clone());

        Ok(task_state)
    }

    /// 处理用户回答，返回是否应跳过后续步骤
    async fn process_user_answer(
        &self,
        answer: &UserAnswer,
        question: &UserQuestion,
        context: &mut ExecutionContext,
        recipe: &mut Recipe,
    ) -> bool {
        // 同时以 question_id 为键存储，避免多轮提问覆盖
        let qid_key = format!("answer_{}", answer.question_id);
        context.set_var(&qid_key, json!(answer.answer.clone()));

        // 同步存入 step_outputs，供下游步骤通过 xxxFrom 引用用户回答
        context.add_output(
            &qid_key,
            json!({
                "answer": answer.answer.clone(),
                "question": question.question.clone(),
            }),
        );

        // pre_param:* 答案写回 Recipe 具体步骤参数（否则 resume 仍用缺参的原 recipe）
        if crate::services::agent::parse_pre_param_question_id(&answer.question_id).is_some()
            || crate::services::agent::parse_pre_param_question_id(&question.question_id).is_some()
        {
            let qid = if crate::services::agent::parse_pre_param_question_id(&answer.question_id)
                .is_some()
            {
                answer.question_id.as_str()
            } else {
                question.question_id.as_str()
            };
            let applied = crate::services::agent::apply_pre_param_answer_to_recipe(
                recipe,
                qid,
                &answer.answer,
            );
            tracing::info!(
                question_id = %qid,
                applied = applied,
                "[Executor] Applied pre_param answer to recipe"
            );
        }

        match question.question_type {
            QuestionType::SingleChoice | QuestionType::MultipleChoice => {
                context.set_var("user_choice", json!(answer.answer.clone()));

                // 验证答案是否在合法选项中
                if let Some(options) = &question.options {
                    if !options.is_empty() {
                        let valid = options.iter().any(|o| o.value == answer.answer);
                        if valid {
                            if let Some(option) = options.iter().find(|o| o.value == answer.answer)
                            {
                                context
                                    .set_var("selected_option_label", json!(option.label.clone()));
                            }
                        } else {
                            // 不匹配但可能是 cancel/skip/retry 等控制指令
                            if !matches!(answer.answer.as_str(), "cancel" | "skip" | "retry") {
                                tracing::warn!(
                                    answer = %answer.answer,
                                    options = ?options.iter().map(|o| &o.value).collect::<Vec<_>>(),
                                    "[Executor] User answer does not match any valid option"
                                );
                            }
                        }
                    }
                }

                // 错误处理选项：用户选择 retry/skip/cancel
                match answer.answer.as_str() {
                    "cancel" => {
                        context.record_decision(
                            DecisionType::SkipStep,
                            "用户选择取消任务",
                            "用户选择不执行该操作",
                            None,
                        );
                        return true; // 跳过后续步骤
                    }
                    "skip" => {
                        context.record_decision(
                            DecisionType::SkipStep,
                            "用户选择跳过错误步骤",
                            "跳过当前步骤继续执行",
                            None,
                        );
                        // 不跳过后续所有步骤，只跳过当前出错的
                        return false;
                    }
                    "retry" => {
                        context.record_decision(
                            DecisionType::ModifyParams,
                            "用户选择重试失败步骤",
                            "重新执行出错的步骤",
                            None,
                        );
                        // 实际的重试逻辑由 resume_with_answer 处理（清除步骤结果、重置 DAG 状态）
                        return false;
                    }
                    _ => {
                        context.record_decision(
                            DecisionType::ModifyParams,
                            &format!("用户选择了: {}", answer.answer),
                            "根据用户选择调整执行参数",
                            None,
                        );
                    }
                }
            }
            QuestionType::FreeText => {
                // 验证必填问题不能为空
                if question.required && answer.answer.trim().is_empty() {
                    tracing::warn!(
                        question_id = %question.question_id,
                        "[Executor] Required question received empty answer, using placeholder"
                    );
                    context.set_var("user_input", json!("（用户未提供输入）"));
                } else {
                    context.set_var("user_input", json!(answer.answer.clone()));
                }

                context.record_decision(
                    DecisionType::ModifyParams,
                    &format!("用户输入了: {}", answer.answer),
                    "使用用户提供的信息",
                    None,
                );
            }
            QuestionType::Confirmation => {
                if answer.answer == "yes" {
                    context.record_decision(
                        DecisionType::GenerateSteps,
                        "用户确认继续执行",
                        "用户确认了操作",
                        None,
                    );
                } else {
                    context.record_decision(
                        DecisionType::SkipStep,
                        "用户拒绝，跳过相关操作",
                        "用户选择不执行该操作",
                        None,
                    );
                    return true; // 用户拒绝确认 → 跳过后续步骤
                }
            }
            QuestionType::Numeric | QuestionType::Date => {
                // Numeric/Date 类型与 FreeText 同样处理：存储用户原始输入
                context.set_var("user_input", json!(answer.answer.clone()));
                context.record_decision(
                    DecisionType::ModifyParams,
                    &format!("用户输入了: {}", answer.answer),
                    "使用用户提供的信息",
                    None,
                );
            }
        }
        false // 不跳过
    }

    /// 分析步骤结果，交由 AI 判断是否需要向用户提问
    ///
    /// AI 基于步骤输出 + 用户原始意图，判断：
    /// 1. 执行结果是否足够明确，可以继续？
    /// 2. 是否存在歧义/多选/缺失信息，需要用户介入？
    /// 3. 如果需要提问，生成结构化的问题（含选项）
    async fn analyze_and_generate_dynamic_steps(
        &self,
        step: &RecipeStep,
        output: &Value,
        context: &mut ExecutionContext,
        recipe: &Recipe,
    ) -> Option<UserQuestion> {
        // 限制动态分析次数，防止无限循环
        const MAX_DYNAMIC_STEPS: usize = 15;
        if context.dynamic_steps_generated >= MAX_DYNAMIC_STEPS {
            tracing::warn!(
                "[Executor] Max dynamic steps reached ({}), skipping analysis",
                MAX_DYNAMIC_STEPS
            );
            return None;
        }

        // 限制提问次数，防止无限提问循环
        const MAX_QUESTIONS: usize = 3;
        let questions_asked = context.answered_questions.len();
        if questions_asked >= MAX_QUESTIONS {
            tracing::info!(
                questions_asked = questions_asked,
                "[Executor] Max questions reached ({}), skipping further questions",
                MAX_QUESTIONS
            );
            return None;
        }

        // AI 处理类步骤（ai.analyze、ai.chat、ai.summarize 等）的输出已经是
        // AI 经过推理后的结果，不需要另一个 AI 来二次审查是否有歧义。
        // 仅对数据获取类步骤（搜索、平台读取）做动态分析。
        if step.capability_id.starts_with("ai.")
            || step.capability_id.starts_with("compare.")
            || step.capability_id == "prompt.generate"
            || step.capability_id == "translate.text"
            || step.capability_id == "code.explain"
        {
            tracing::debug!(
                step_id = %step.id,
                capability = %step.capability_id,
                "[Executor] Skipping dynamic analysis for AI processing step"
            );
            return None;
        }

        // 如果下游已经有 AI 处理步骤依赖当前步骤的输出，跳过动态分析。
        // 典型场景：webSearch → ai.analyze 链中，ai.analyze 会处理搜索结果的
        // 歧义和选择，无需在 webSearch 输出时向用户提问。
        let has_downstream_ai = recipe.steps.iter().any(|s| {
            s.depends_on.contains(&step.id)
                && (s.capability_id.starts_with("ai.") || s.capability_id == "prompt.generate")
        });
        if has_downstream_ai {
            tracing::debug!(
                step_id = %step.id,
                capability = %step.capability_id,
                "[Executor] Skipping dynamic analysis: downstream AI step will process this output"
            );
            return None;
        }

        // 检查输出是否包含错误——错误直接生成选择题让用户决定
        if let Some(error) = output.get("error").and_then(|e| e.as_str()) {
            if !error.is_empty() {
                // 记录出错步骤 ID，便于用户选择 retry 时重新执行
                context.set_var("_error_step_id", json!(step.id.clone()));
                return Some(UserQuestion::single_choice(
                    &crate::services::agent::response_agent::step_error_question(error),
                    &crate::services::agent::response_agent::step_error_title(),
                    vec![
                        QuestionOption::new("retry", "重试").with_description("重新执行这个步骤"),
                        QuestionOption::new("skip", "跳过")
                            .with_description("跳过这个步骤继续执行"),
                        QuestionOption::new("cancel", "取消").with_description("取消整个任务"),
                    ],
                    true,
                ));
            }
        }

        // 获取 AI 分析器（使用 Standard 层级，节省 token 开销）
        let analyzer = self.get_analyzer_for_tier(ModelTier::Standard);
        let Some(analyzer) = analyzer else {
            tracing::debug!("[Executor] No AI analyzer available, skipping step analysis");
            return None;
        };

        // 截断输出以控制 prompt 大小
        let output_str = {
            let full = serde_json::to_string_pretty(output).unwrap_or_default();
            if full.len() > 3000 {
                // 在 char boundary 安全截断
                let truncate_at = full.floor_char_boundary(3000);
                format!("{}...(truncated)", &full[..truncate_at])
            } else {
                full
            }
        };

        let system_prompt = r#"你是一个任务执行分析器。你的职责是判断步骤执行结果是否足够明确，还是需要用户介入。

## 核心原则
优先自主完成任务，仅在关键歧义时才提问。不要反复对已经回答过的内容再次提问。

## 必须提问的场景（返回 ask）
- 操作涉及不可逆的修改、删除等，用户尚未确认
- 搜索返回多个完全不同的实体，无法判断用户想要哪个（仅当差异很大时）
- 用户提供的关键参数缺失且无法合理推断

## 应该继续的场景（返回 continue）
- 结果合理匹配用户需求，即使不是100%精确
- 搜索返回多条结果但最相关的那条足够明显
- 纯信息查询
- 用户之前已经回答过类似问题（见下方已有回答记录）
- 可以根据上下文合理推断用户意图

## 输出格式
不需要提问：{"action":"continue"}
需要提问：
{
  "action": "ask",
  "questionType": "single_choice" | "free_text" | "confirmation",
  "question": "简洁明了的问题",
  "context": "补充说明，帮助用户理解为什么需要回答",
  "options": [{"value": "v1", "label": "显示文本", "description": "可选说明"}],
  "required": true
}

只返回 JSON，不要其他内容。"#;

        // 构建已有 Q&A 历史，让 AI 知道用户已回答过什么
        let qa_history = if context.answered_questions.is_empty() {
            String::new()
        } else {
            let mut history = String::from("\n\n已有的用户回答记录（不要重复提问这些内容）：\n");
            for (qid, ans) in &context.answered_questions {
                history.push_str(&format!("- 问题 {}: 用户回答了 \"{}\"\n", qid, ans));
            }
            history
        };

        // 构建用户选择/输入变量上下文
        let var_context = {
            let mut parts = Vec::new();
            if let Some(choice) = context.variables.get("user_choice") {
                parts.push(format!("用户已选择: {}", choice));
            }
            if let Some(input) = context.variables.get("user_input") {
                parts.push(format!("用户已输入: {}", input));
            }
            if parts.is_empty() {
                String::new()
            } else {
                format!("\n用户已提供的信息：{}", parts.join("，"))
            }
        };

        let user_prompt = format!(
            "用户意图：{}\n步骤：{} (capability: {})\n执行结果：\n{}{}{}",
            context.user_intent,
            step.action,
            step.capability_id,
            output_str,
            qa_history,
            var_context,
        );

        match analyzer
            .analyze_with_system(system_prompt, &user_prompt)
            .await
        {
            Ok(response) => {
                let response = response.trim();
                // 尝试提取 JSON（处理 markdown code block 包裹的情况）
                let json_str = if let Some(start) = response.find('{') {
                    if let Some(end) = response.rfind('}') {
                        &response[start..=end]
                    } else {
                        response
                    }
                } else {
                    response
                };

                match serde_json::from_str::<Value>(json_str) {
                    Ok(parsed) => {
                        let action = parsed
                            .get("action")
                            .and_then(|a| a.as_str())
                            .unwrap_or("continue");
                        if action != "ask" {
                            return None;
                        }

                        let question_type = parsed
                            .get("questionType")
                            .and_then(|q| q.as_str())
                            .unwrap_or("free_text");
                        let question_text = parsed
                            .get("question")
                            .and_then(|q| q.as_str())
                            .unwrap_or("请提供更多信息");
                        let ctx = parsed.get("context").and_then(|c| c.as_str()).unwrap_or("");
                        let required = parsed
                            .get("required")
                            .and_then(|r| r.as_bool())
                            .unwrap_or(true);

                        let question = match question_type {
                            "single_choice" | "multiple_choice" => {
                                let options: Vec<QuestionOption> = parsed
                                    .get("options")
                                    .and_then(|o| o.as_array())
                                    .map(|arr| {
                                        arr.iter()
                                            .filter_map(|item| {
                                                let value =
                                                    item.get("value").and_then(|v| v.as_str())?;
                                                let label = item
                                                    .get("label")
                                                    .and_then(|l| l.as_str())
                                                    .unwrap_or(value);
                                                let desc = item
                                                    .get("description")
                                                    .and_then(|d| d.as_str());
                                                let mut opt = QuestionOption::new(value, label);
                                                if let Some(d) = desc {
                                                    opt = opt.with_description(d);
                                                }
                                                Some(opt)
                                            })
                                            .collect()
                                    })
                                    .unwrap_or_default();

                                if options.is_empty() {
                                    tracing::warn!(
                                        "[Executor] AI requested single_choice but provided no options, falling back to free_text"
                                    );
                                    UserQuestion::free_text(question_text, ctx, required)
                                } else {
                                    UserQuestion::single_choice(
                                        question_text,
                                        ctx,
                                        options,
                                        required,
                                    )
                                }
                            }
                            "confirmation" => UserQuestion::confirmation(question_text, ctx),
                            _ => UserQuestion::free_text(question_text, ctx, required),
                        };

                        context.record_decision(
                            DecisionType::AskUser,
                            &format!("AI 判断需要用户介入: {}", question_text),
                            &format!("步骤 {} 的输出需要用户澄清", step.id),
                            Some(&step.id),
                        );

                        tracing::info!(
                            step_id = %step.id,
                            question_type = question_type,
                            "[Executor] AI decided to ask user"
                        );

                        Some(question)
                    }
                    Err(e) => {
                        tracing::debug!(
                            step_id = %step.id,
                            error = %e,
                            raw = %response,
                            "[Executor] AI response not valid JSON, treating as continue"
                        );
                        None
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    step_id = %step.id,
                    error = %e,
                    "[Executor] AI analysis failed, continuing without question"
                );
                None
            }
        }
    }

    /// 检查步骤依赖
    fn check_dependencies(&self, step: &RecipeStep, outputs: &HashMap<String, Value>) -> bool {
        for dep_id in &step.depends_on {
            if !outputs.contains_key(dep_id) {
                return false;
            }
        }
        true
    }

    // ======== 动态步骤生成器 ========

    /// 处理步骤上的 StepGenerator，返回生成的动态步骤
    async fn process_step_generator(
        &self,
        generator: &StepGenerator,
        step: &RecipeStep,
        output: &Value,
        context: &mut ExecutionContext,
    ) -> Vec<RecipeStep> {
        match generator {
            StepGenerator::ConditionalBranch {
                condition,
                if_true,
                if_false,
            } => {
                let result = self.evaluate_condition(condition, &step.id, output, context);
                let branch = if result { if_true } else { if_false };
                tracing::info!(
                    step_id = %step.id,
                    condition = %condition,
                    result = result,
                    branch_steps = branch.len(),
                    "[Generator] ConditionalBranch evaluated"
                );
                branch.clone()
            }
            StepGenerator::AiGenerated {
                context_prompt,
                capability_scope,
            } => {
                self.ai_generate_steps(context_prompt, capability_scope.as_deref(), step, context)
                    .await
            }
            StepGenerator::UiInteractionFromAnalysis {
                source_step,
                operation_intent,
            } => {
                // 从源步骤输出中提取 UI 元素，生成交互步骤
                let source_output = context.step_outputs.get(source_step.as_str());
                if let Some(output) = source_output {
                    if let Some(elements) = output.get("elements").and_then(|e| e.as_array()) {
                        elements
                            .iter()
                            .take(5)
                            .enumerate()
                            .map(|(i, el)| {
                                let mut params = HashMap::new();
                                params.insert("element".to_string(), el.clone());
                                params.insert(
                                    "intent".to_string(),
                                    Value::String(operation_intent.clone()),
                                );
                                RecipeStep {
                                    id: format!("{}_ui_{}", step.id, i),
                                    order: (step.order + 1 + i as u32),
                                    capability_id: "tapp.interact".to_string(),
                                    action: "interact".to_string(),
                                    params,
                                    depends_on: vec![step.id.clone()],
                                    on_failure: FailureStrategy::Skip,
                                    retry: None,
                                    timeout_ms: Some(15_000),
                                    model_tier: None,
                                    generator: None,
                                }
                            })
                            .collect()
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                }
            }
            StepGenerator::IterateFromList {
                source_step,
                item_capability,
            } => {
                let source_output = context.step_outputs.get(source_step.as_str());
                if let Some(output) = source_output {
                    // 尝试从输出中找到列表数据
                    let items = output
                        .get("items")
                        .or_else(|| output.get("data"))
                        .or_else(|| output.get("list"))
                        .and_then(|v| v.as_array());
                    if let Some(items) = items {
                        items
                            .iter()
                            .take(10) // 最多迭代 10 项
                            .enumerate()
                            .map(|(i, item)| {
                                let mut params = HashMap::new();
                                params.insert("item".to_string(), item.clone());
                                params.insert(
                                    "index".to_string(),
                                    Value::Number(serde_json::Number::from(i)),
                                );
                                RecipeStep {
                                    id: format!("{}_iter_{}", step.id, i),
                                    order: (step.order + 1 + i as u32),
                                    capability_id: item_capability.clone(),
                                    action: "process".to_string(),
                                    params,
                                    depends_on: vec![step.id.clone()],
                                    on_failure: FailureStrategy::Skip,
                                    retry: None,
                                    timeout_ms: Some(30_000),
                                    model_tier: None,
                                    generator: None,
                                }
                            })
                            .collect()
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                }
            }
        }
    }

    /// 评估条件表达式（支持简单的 dot-path 真值检查和比较运算）
    fn evaluate_condition(
        &self,
        condition: &str,
        current_step_id: &str,
        current_output: &Value,
        context: &ExecutionContext,
    ) -> bool {
        // 支持的格式：
        // 1. "output.field" → 检查当前步骤输出中 field 的真值
        // 2. "step_id.field" → 检查指定步骤输出中 field 的真值
        // 3. "output.field == value" → 相等比较
        // 4. "output.field > 0" → 数值比较
        // 5. "output.field != null" → 非空检查

        let condition = condition.trim();

        // 解析比较运算符
        let (path, op, expected) = if let Some(pos) = condition.find("!=") {
            let (p, v) = condition.split_at(pos);
            (p.trim(), "!=", v[2..].trim())
        } else if let Some(pos) = condition.find("==") {
            let (p, v) = condition.split_at(pos);
            (p.trim(), "==", v[2..].trim())
        } else if let Some(pos) = condition.find(">=") {
            let (p, v) = condition.split_at(pos);
            (p.trim(), ">=", v[2..].trim())
        } else if let Some(pos) = condition.find("<=") {
            let (p, v) = condition.split_at(pos);
            (p.trim(), "<=", v[2..].trim())
        } else if let Some(pos) = condition.find('>') {
            let (p, v) = condition.split_at(pos);
            (p.trim(), ">", v[1..].trim())
        } else if let Some(pos) = condition.find('<') {
            let (p, v) = condition.split_at(pos);
            (p.trim(), "<", v[1..].trim())
        } else {
            // 纯路径：检查真值
            (condition, "truthy", "")
        };

        // 解析 dot-path 取值
        let value = self.resolve_dot_path(path, current_step_id, current_output, context);

        match op {
            "truthy" => Self::is_truthy(&value),
            "==" => {
                if expected == "null" || expected == "nil" {
                    value.is_null()
                } else if let Ok(expected_num) = expected.parse::<f64>() {
                    value
                        .as_f64()
                        .is_some_and(|v| (v - expected_num).abs() < f64::EPSILON)
                } else {
                    let expected_str = expected.trim_matches('"').trim_matches('\'');
                    value.as_str() == Some(expected_str)
                }
            }
            "!=" => {
                if expected == "null" || expected == "nil" {
                    !value.is_null()
                } else if let Ok(expected_num) = expected.parse::<f64>() {
                    value
                        .as_f64()
                        .is_none_or(|v| (v - expected_num).abs() >= f64::EPSILON)
                } else {
                    let expected_str = expected.trim_matches('"').trim_matches('\'');
                    value.as_str() != Some(expected_str)
                }
            }
            ">" | ">=" | "<" | "<=" => {
                let actual = value.as_f64().unwrap_or(0.0);
                let expected_num = expected.parse::<f64>().unwrap_or(0.0);
                match op {
                    ">" => actual > expected_num,
                    ">=" => actual >= expected_num,
                    "<" => actual < expected_num,
                    "<=" => actual <= expected_num,
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// 解析 dot-path 从步骤输出中取值
    fn resolve_dot_path(
        &self,
        path: &str,
        _current_step_id: &str,
        current_output: &Value,
        context: &ExecutionContext,
    ) -> Value {
        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() {
            return Value::Null;
        }

        // 确定起始值
        let (root, field_start) = if parts[0] == "output" {
            // "output.xxx" → 当前步骤输出
            (current_output, 1)
        } else if context.step_outputs.contains_key(parts[0]) {
            // "step_id.xxx" → 指定步骤输出
            match context.step_outputs.get(parts[0]) {
                Some(v) => (v, 1),
                None => return Value::Null,
            }
        } else {
            // 没有前缀，尝试从当前步骤输出解析
            (current_output, 0)
        };

        // 逐层取值
        let mut current = root.clone();
        for part in &parts[field_start..] {
            current = if let Ok(idx) = part.parse::<usize>() {
                current
                    .as_array()
                    .and_then(|arr| arr.get(idx).cloned())
                    .unwrap_or(Value::Null)
            } else {
                current.get(part).cloned().unwrap_or(Value::Null)
            };
        }
        current
    }

    /// 判断 JSON 值的真值
    fn is_truthy(value: &Value) -> bool {
        match value {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => n.as_f64().is_some_and(|v| v != 0.0),
            Value::String(s) => !s.is_empty(),
            Value::Array(arr) => !arr.is_empty(),
            Value::Object(obj) => !obj.is_empty(),
        }
    }

    /// AI 动态生成步骤
    async fn ai_generate_steps(
        &self,
        context_prompt: &str,
        capability_scope: Option<&[String]>,
        parent_step: &RecipeStep,
        context: &ExecutionContext,
    ) -> Vec<RecipeStep> {
        let analyzer = match self.get_analyzer_for_tier(ModelTier::Standard) {
            Some(a) => a,
            None => {
                tracing::warn!("[Generator] No AI analyzer available for AiGenerated");
                return vec![];
            }
        };

        // 构建能力列表
        let capabilities = if let Some(scope) = capability_scope {
            scope.join(", ")
        } else {
            let registry = get_registry().await;
            registry
                .get_all()
                .iter()
                .take(30)
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };

        // 构建上下文摘要（按 step id 排序，保证 AI 每次看到一致的上下文顺序）
        let outputs_summary: String = {
            let mut pairs: Vec<_> = context.step_outputs.iter().collect();
            pairs.sort_by_key(|(id, _)| *id);
            pairs
                .iter()
                .take(5)
                .map(|(id, val)| format!("- {}: {}", id, summarize_output(val).unwrap_or_default()))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let prompt = format!(
            r#"根据以下上下文，生成接下来需要执行的步骤。

## 用户原始请求（最高优先级，所有步骤都必须服务于此目标）
{user_request}

## 上下文提示
{ctx}

## 已完成步骤的输出
{outputs}

## 可用能力
{caps}

## 重要规则
1. 每个步骤的 action 字段必须明确写出该步骤要做什么，且必须与用户原始请求直接相关
2. 不要生成与用户请求无关的步骤
3. 最多生成 3 个步骤
4. 只输出 JSON 数组，不要其他文字

## 输出格式
```json
[{{"id": "gen_1", "capability_id": "...", "action": "具体说明这步做什么", "params": {{}}}}]
```"#,
            user_request = context.original_request,
            ctx = context_prompt,
            outputs = outputs_summary,
            caps = capabilities
        );

        let result = analyzer.analyze(&prompt).await;
        match result {
            Ok(response) => {
                let text = response.trim();
                // 提取 JSON 数组
                let json_str = if let Some(start) = text.find('[') {
                    if let Some(end) = text.rfind(']') {
                        &text[start..=end]
                    } else {
                        text
                    }
                } else {
                    text
                };

                match serde_json::from_str::<Vec<Value>>(json_str) {
                    Ok(items) => items
                        .into_iter()
                        .take(3)
                        .enumerate()
                        .filter_map(|(i, item)| {
                            let id = item
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let cap_id = item
                                .get("capability_id")
                                .and_then(|v| v.as_str())?
                                .to_string();
                            let action = item
                                .get("action")
                                .and_then(|v| v.as_str())
                                .unwrap_or("process")
                                .to_string();
                            let params: HashMap<String, Value> = item
                                .get("params")
                                .and_then(|v| serde_json::from_value(v.clone()).ok())
                                .unwrap_or_default();

                            Some(RecipeStep {
                                id: if id.is_empty() {
                                    format!("{}_ai_{}", parent_step.id, i)
                                } else {
                                    id
                                },
                                order: parent_step.order + 1 + i as u32,
                                capability_id: cap_id,
                                action,
                                params,
                                depends_on: vec![parent_step.id.clone()],
                                on_failure: FailureStrategy::Skip,
                                retry: None,
                                timeout_ms: Some(30_000),
                                model_tier: None,
                                generator: None,
                            })
                        })
                        .collect(),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "[Generator] Failed to parse AI-generated steps"
                        );
                        vec![]
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "[Generator] AI call failed");
                vec![]
            }
        }
    }
}

fn tapp_interaction_wait_question(output: &Value) -> Option<UserQuestion> {
    let interaction = output.get("interaction")?;
    let interaction_id = interaction
        .get("interactionId")
        .or_else(|| interaction.get("interaction_id"))?
        .as_str()?;
    let expires_at = interaction
        .get("deadline")
        .and_then(Value::as_str)
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&chrono::Utc));
    Some(UserQuestion {
        question_id: format!("tapp_interaction:{interaction_id}"),
        question_type: QuestionType::FreeText,
        question: "等待 Tapp 完成交互".to_string(),
        context: format!(
            "Tapp Agent Interaction {interaction_id} 将在提交结构化结果后自动恢复此任务"
        ),
        options: None,
        required: true,
        default_value: None,
        created_at: chrono::Utc::now(),
        expires_at,
    })
}

#[cfg(test)]
mod resolve_id_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dynamic_risk_gate_blocks_system_critical_but_allows_system_high() {
        assert!(Executor::should_block_unconfirmed_dynamic_step(
            crate::services::agent::SYSTEM_USER_ID,
            RiskLevel::Critical
        ));
        assert!(!Executor::should_block_unconfirmed_dynamic_step(
            crate::services::agent::SYSTEM_USER_ID,
            RiskLevel::High
        ));
        assert!(Executor::should_block_unconfirmed_dynamic_step(
            7,
            RiskLevel::High
        ));
    }

    /// 复现歌单播放链路：搜索步骤输出被整对象引用为 playlistIdFrom 时，
    /// 必须取到 playlists[0].id，而不是 message 文案
    #[test]
    fn id_param_extracts_from_search_output() {
        let output = json!({
            "success": true,
            "message": "找到 10 个「凉宫春日」相关歌单",
            "keyword": "凉宫春日",
            "playlists": [
                { "id": 12597740641u64, "name": "悲情篇章" },
                { "id": 12764048642u64, "name": "アニサマ" }
            ]
        });
        let got = Executor::extract_id_from_output(&output, "playlistId");
        assert_eq!(got, Some(json!(12597740641u64)));
    }

    #[test]
    fn id_param_prefers_same_name_field() {
        let output = json!({ "playlistId": "abc123", "id": "other", "message": "文案" });
        let got = Executor::extract_id_from_output(&output, "playlistId");
        assert_eq!(got, Some(json!("abc123")));
    }

    #[test]
    fn id_param_falls_back_to_top_level_id() {
        let output = json!({ "id": 42, "message": "文案" });
        assert_eq!(
            Executor::extract_id_from_output(&output, "songId"),
            Some(json!(42))
        );
    }

    #[test]
    fn id_param_array_input_takes_first_element() {
        let output = json!([{ "id": "first" }, { "id": "second" }]);
        assert_eq!(
            Executor::extract_id_from_output(&output, "itemId"),
            Some(json!("first"))
        );
    }

    /// 提取不到 ID 必须返回 None（上层按未解析处理并让步骤报错），
    /// 绝不能兜底成 message 文案
    #[test]
    fn id_param_without_id_yields_none() {
        let output = json!({ "message": "找到 10 个歌单", "success": true });
        assert_eq!(
            Executor::extract_id_from_output(&output, "playlistId"),
            None
        );
    }
}
