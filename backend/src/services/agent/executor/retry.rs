//! Shared post-DAG serial retry loop (also serial execute and resume_with_answer).
//! DAG waves classify/queue; they do not call `execute_step_with_retry`.
//! Loop: analyze → param fixes → collect prepend suggestions → backoff → retry.
//! Callers inject prepends. Pure decisions: [`crate::services::agent::retry_pure`].

use super::handlers::HandlerContext;
use super::Executor;
use crate::config::ModelTier;
use crate::services::agent::error_analyzer_pure::{analyze_error, apply_param_fixes};
use crate::services::agent::retry_pure::{
    compute_retry_delay_ms, default_max_retries as pure_default_max_retries,
    format_retry_final_error, prepend_step_id, should_retry_step, step_retry_delay_config,
};
use crate::services::agent::types::*;
use serde_json::Value;
use std::collections::HashMap;

/// 单次步骤执行+重试的完整结果
pub struct StepRetryOutcome {
    /// 是否成功
    pub success: bool,
    /// 成功时的输出
    pub output: Option<Value>,
    /// 最终错误消息（含重试历史）
    pub error: Option<String>,
    /// 本次 `execute_step` 耗时（不是跨尝试合计）
    pub duration_ms: u64,
    /// 实际重试次数（0 = 一次就成功）
    pub retry_count: u32,
    /// 最后使用的模型层级
    pub last_tier: ModelTier,
    /// 错误分析器建议注入的前置步骤
    pub prepend_steps: Vec<RecipeStep>,
}

impl StepRetryOutcome {
    /// 转换为 StepResult（用于存入 task_state.step_results）
    pub fn to_step_result(&self, step_id: &str) -> StepResult {
        StepResult {
            step_id: step_id.to_string(),
            success: self.success,
            output: self.output.clone(),
            error: self.error.clone(),
            duration_ms: self.duration_ms,
            retry_count: self.retry_count,
        }
    }
}

/// 重试配置（调用方按自身路径提供）
pub struct RetryConfig {
    /// 该步骤允许的最大尝试次数
    pub max_attempts: u32,
    /// 全局重试预算剩余（跨步骤共享，会被扣减）
    pub global_budget: u32,
}

/// 重试过程中发送 SSE 事件所需的上下文
pub struct RetryEventContext {
    /// 步骤在前端展示的序号
    pub step_display_index: u32,
    /// 进度通道
    pub progress_tx: Option<tokio::sync::mpsc::Sender<AgentProgressEvent>>,
}

impl Executor {
    /// 执行单个步骤，带智能错误分析和自动重试
    ///
    /// Shared by serial / post-DAG / resume. Loop: analyze → param fixes →
    /// collect prepend suggestions → backoff → retry. Callers inject prepends
    /// and handle SSE success/fail, DAG, TaskStore. Only `StepRetrying` is sent here.
    ///
    /// Success: `add_output`. Retry: restore in-memory `ExecutionContext` snapshot
    /// (handler I/O is not rolled back). Final failure does not restore.
    pub async fn execute_step_with_retry(
        &self,
        step: &RecipeStep,
        context: &mut ExecutionContext,
        user_id: i32,
        config: &mut RetryConfig,
        event_ctx: &RetryEventContext,
    ) -> StepRetryOutcome {
        let mut retry_count: u32 = 0;
        let mut retry_params_override: Option<HashMap<String, Value>> = None;
        let mut retry_errors: Vec<String> = Vec::new();
        let mut prepend_steps: Vec<RecipeStep> = Vec::new();

        loop {
            // 每次重试重新解析 tier（熔断器状态可能已变化）
            let tier = Self::resolve_tier_with_breaker(&step.capability_id, step.model_tier);
            let step_analyzer = self.get_analyzer_for_tier(tier);

            if retry_count == 0 {
                tracing::debug!(
                    step_id = %step.id,
                    capability = %step.capability_id,
                    tier = ?tier,
                    "[Executor] Using {:?} tier for step", tier
                );
            }

            let handler_ctx = HandlerContext {
                db: &self.db,
                ai_analyzer: step_analyzer,
                user_id,
                task_id: context
                    .variables
                    .get("_task_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                execution_context: Some(context.clone()),
                autonomy_permission_cap: context.autonomy_permission_cap.clone(),
            };

            // 快照 context 以便重试时回滚
            let ctx_snapshot = context.clone();

            // Use retry_params_override from a prior `apply_param_fixes`, else the original step.
            let effective_step = if let Some(ref override_params) = retry_params_override {
                let mut patched = step.clone();
                patched.params = override_params.clone();
                patched
            } else {
                step.clone()
            };

            let start_time = std::time::Instant::now();
            let step_result = self
                .execute_step(&effective_step, context, &handler_ctx)
                .await;
            let duration_ms = start_time.elapsed().as_millis() as u64;

            match step_result {
                Ok(output) => {
                    Self::record_step_to_breaker(tier, true);
                    context.add_output(&step.id, output.clone());

                    return StepRetryOutcome {
                        success: true,
                        output: Some(output),
                        error: None,
                        duration_ms,
                        retry_count,
                        last_tier: tier,
                        prepend_steps,
                    };
                }
                Err(e) => {
                    Self::record_step_to_breaker(tier, false);
                    retry_count += 1;
                    retry_errors.push(e.clone());

                    // 智能错误分析（纯域规则，无 I/O）
                    let effective_params = retry_params_override.as_ref().unwrap_or(&step.params);
                    let analysis = analyze_error(&e, &step.capability_id, effective_params);

                    let should_retry = should_retry_step(
                        retry_count,
                        config.max_attempts,
                        config.global_budget,
                        analysis.retryable,
                    );

                    if should_retry {
                        config.global_budget -= 1;

                        // 回滚 context 到重试前快照
                        *context = ctx_snapshot;

                        // 应用参数修复（纯域投影）
                        if !analysis.param_fixes.is_empty() {
                            let fixed = apply_param_fixes(
                                retry_params_override.as_ref().unwrap_or(&step.params),
                                &analysis.param_fixes,
                            );
                            retry_params_override = Some(fixed);
                            tracing::info!(
                                step_id = %step.id,
                                category = ?analysis.category,
                                fixes = analysis.param_fixes.len(),
                                "[Executor] Error analyzed, applying {} param fixes for retry",
                                analysis.param_fixes.len()
                            );
                        }

                        // 收集前置步骤建议
                        if let Some(ref prepend_cap) = analysis.suggested_prepend_capability {
                            tracing::info!(
                                step_id = %step.id,
                                suggested = %prepend_cap,
                                "[Executor] Error analyzer suggests prepending '{}'",
                                prepend_cap
                            );
                            prepend_steps.push(RecipeStep {
                                id: prepend_step_id(&step.id, retry_count),
                                order: step.order.saturating_sub(1),
                                capability_id: prepend_cap.clone(),
                                action: "execute".to_string(),
                                params: analysis.suggested_prepend_params.clone(),
                                depends_on: vec![],
                                on_failure: FailureStrategy::Skip,
                                retry: None,
                                timeout_ms: None,
                                generator: None,
                                model_tier: None,
                            });
                        }

                        // 发送重试事件
                        if let Some(ref tx) = event_ctx.progress_tx {
                            let _ = tx
                                .send(AgentProgressEvent::StepRetrying {
                                    step_id: step.id.clone(),
                                    step_index: event_ctx.step_display_index,
                                    retry_count,
                                    max_retries: config.max_attempts,
                                    reason: analysis.description.clone(),
                                })
                                .await;
                        }

                        // 退避延迟（纯 domain 计算）
                        let (base_delay, use_backoff) = step_retry_delay_config(step);
                        let adjusted = compute_retry_delay_ms(
                            base_delay,
                            use_backoff,
                            retry_count,
                            analysis.delay_multiplier,
                        );
                        tracing::warn!(
                            step_id = %step.id,
                            retry = retry_count,
                            delay_ms = adjusted,
                            budget = config.global_budget,
                            category = ?analysis.category,
                            error = %e,
                            "[Executor] Step failed ({}), retrying in {}ms ({}/{})",
                            analysis.description,
                            adjusted,
                            retry_count,
                            config.max_attempts
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(adjusted)).await;

                        continue; // 重试
                    }

                    // 不可重试或预算耗尽
                    tracing::error!(
                        step_id = %step.id,
                        error = %e,
                        retries = retry_count,
                        "[Executor] Step failed (no more retries)"
                    );

                    let final_error = format_retry_final_error(&retry_errors, &e);

                    return StepRetryOutcome {
                        success: false,
                        output: None,
                        error: Some(final_error),
                        duration_ms,
                        retry_count,
                        last_tier: tier,
                        prepend_steps,
                    };
                }
            }
        }
    }

    /// Default `max_attempts` (AI/skill/`prompt.generate` → 2, else 1; explicit clamped to 3).
    pub fn default_max_retries(step: &RecipeStep) -> u32 {
        pure_default_max_retries(step)
    }
}
