// Executor core: run recipe / process entry

use crate::config::ModelTier;
use crate::services::agent::tier_router::{self, TierRouter};
use crate::services::agent::types::{self, *};
use crate::services::ai::create_ai_analyzer_for_tier;
use crate::services::analyzer::AiAnalyzer;
use sea_orm::DatabaseConnection;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

use super::handlers::HandlerContext;
use super::executor_footer::*;
use super::Executor;
use super::{
    clear_cancellation, extract_image_url, is_cancelled, persist_task_async,
    summarize_output, TASK_STORE,
};
use super::{dag, error_analyzer, events, retry, task_store};

impl Executor {
    pub(crate) fn should_block_unconfirmed_dynamic_step(user_id: i32, risk: RiskLevel) -> bool {
        // Aligned with system_sensitive_gate: system Medium auto-run; High+ blocked.
        crate::services::agent::executor_resolve_pure::should_block_unconfirmed_dynamic_step(
            user_id, risk,
        )
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
    pub(crate) fn get_analyzer_for_tier(&self, tier: ModelTier) -> Option<&AiAnalyzer> {
        match tier {
            ModelTier::Pro => self
                .pro_analyzer
                .as_ref()
                .or(self.standard_analyzer.as_ref()),
            ModelTier::Standard => self
                .standard_analyzer
                .as_ref()
                .or(self.pro_analyzer.as_ref()),
            // Executor tasks do not currently route to Lite. Keep this arm so
            // explicit future Lite tasks degrade safely until a cached Lite
            // analyzer is added to the executor.
            ModelTier::Lite => self
                .standard_analyzer
                .as_ref()
                .or(self.pro_analyzer.as_ref()),
        }
    }

    /// 带熔断器的 tier 解析
    ///
    /// 优先使用熔断器感知的解析（自动降级），如果两个 tier 都熔断则 fallback 到普通解析。
    pub(crate) fn resolve_tier_with_breaker(
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
    pub(crate) fn record_step_to_breaker(tier: ModelTier, success: bool) {
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
        // Full-site AI usage: attribute every nested AiAnalyzer call (including admin).
        let attr = crate::services::ai_cost_ledger::AiLedgerAttribution {
            subject_id: user_id,
            owner_id: user_id,
            source: "agent".into(),
            operation: "agent".into(),
            tapp_id: "__agent__".into(),
            task_id: recipe.id.clone(),
        };
        crate::services::ai_cost_ledger::with_ai_ledger_attribution(attr, async {
            self.execute_with_progress_inner(recipe, user_id, progress_tx)
                .await
        })
        .await
    }

    async fn execute_with_progress_inner(
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

        // 全局已执行步骤计数器（防止动态步骤导致无限执行）
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
            // 检查任务是否被取消
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

            // 全局步骤上限检查
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
                    // 流式 DAG 并行执行
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
                                // DAG注入的动态步骤不触发生成器，防止链式爆炸
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
                                // DAG注入的动态步骤不触发分析，防止链式膨胀
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

                    // 流式 DAG 后处理：暂停等待用户输入
                    // 如果已取消，跳过 WaitingForInput 和重试
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

                    // 流式DAG后的串行重试（复用 retry.rs 统一逻辑）
                    // 如果已取消，跳过所有重试
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

            // 顺序执行路径（单步）
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
}
