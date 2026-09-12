// Executor resume and dynamic step paths

use crate::config::ModelTier;
use crate::services::agent::tier_router::TierRouter;
use crate::services::agent::types::{self, *};
use myriad_agent_rules::extract_json_object_from_ai_response;
use serde_json::{json, Value};
use std::collections::HashMap;

use super::executor_footer::*;
use super::Executor;
use super::{
    claim_task_for_resume, clear_cancellation, is_cancelled, persist_task_async, truncate_str,
    TASK_STORE,
};
use super::{dag, events, frontend_ack, retry, task_store};

impl Executor {
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
        let mut recipe = task_state.recipe.clone().unwrap_or_else(|| recipe.clone());

        // 恢复执行上下文
        let mut context = task_state.execution_context.take().unwrap_or_default();
        if context.autonomy_permission_cap.is_none() {
            context.autonomy_permission_cap = recipe.autonomy_permission_cap.clone();
        }
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

        let should_skip = if let Some(question) = &task_state.pending_question {
            // 校验 answer.question_id 是否匹配当前待回答的问题
            if answer.question_id != question.question_id {
                // 宽容模式：如果只有唯一一个待回答问题，接受不匹配的 answer
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
                                directive: Some(
                                    "The answer expired; continuing with remaining steps"
                                        .to_string(),
                                ),
                                user_request: None,
                                params: None,
                                output_preview: None,
                                is_dynamic: false,
                                duration_ms: None,
                                success: Some(false),
                                error: Some("The answer timed out".to_string()),
                            })
                            .await;
                    }
                    context.record_decision(
                        DecisionType::SkipStep,
                        "The answer expired; skip this question",
                        "The question timed out unanswered; continue remaining steps",
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

        // 写回 task_state.recipe，供后续 resume / 执行使用
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

        // 构建 DAG；len>1 且无 tapp.interact 才走 ready-set（本路径一次一步）
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
                task_state.error = Some("The task was cancelled".to_string());
                break;
            }
            // 步骤上限保护
            total_executed += 1;
            if total_executed > MAX_RESUME_STEPS {
                tracing::warn!(task_id = %task_id, "[Executor] Resume exceeded max step limit");
                task_state.error = Some("Resume exceeded the step cap".to_string());
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
                let output = frontend_ack::publish_and_await_snapshots(
                    &emitter,
                    &task_state.task_id,
                    &step.id,
                    &step.capability_id,
                    step_display_index,
                    duration_ms,
                    outcome.output.clone().unwrap_or_default(),
                    &mut context,
                    !is_skill_planning,
                )
                .await;
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

                let mut result = outcome.to_step_result(&step.id);
                result.output = Some(output.clone());
                task_state.step_results.insert(step.id.clone(), result);
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
                            tracing::error!(%error, "persist Tapp interaction wait state failed");
                            "Failed to persist Tapp interaction wait state".to_string()
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

                // 动态步骤>1 时写入 DAG ready-set（resume 仍串行取 next）
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

        // 循环结束后若仍有 pending_questions，发下一个
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
    pub(crate) async fn process_user_answer(
        &self,
        answer: &UserAnswer,
        question: &UserQuestion,
        context: &mut ExecutionContext,
        recipe: &mut Recipe,
    ) -> bool {
        // 同时以 question_id 为键存储，避免多轮提问覆盖
        let qid_key = format!("answer_{}", answer.question_id);
        context.set_var(&qid_key, json!(answer.answer.clone()));

        // 写入 step_outputs，键为 answer_{question_id}
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
                            "The task was cancelled",
                            "The user chose not to continue",
                            None,
                        );
                        return true; // 跳过后续步骤
                    }
                    "skip" => {
                        context.record_decision(
                            DecisionType::SkipStep,
                            "The failed step was skipped",
                            "Continue without this step",
                            None,
                        );
                        // 不跳过后续步骤
                        return false;
                    }
                    "retry" => {
                        context.record_decision(
                            DecisionType::ModifyParams,
                            "The failed step will be retried",
                            "Retry the failed step",
                            None,
                        );
                        // 实际的重试逻辑由 resume_with_answer 处理（清除步骤结果与输出）
                        return false;
                    }
                    _ => {
                        context.record_decision(
                            DecisionType::ModifyParams,
                            &format!("User chose: {}", answer.answer),
                            "Adjust params from the user's choice",
                            None,
                        );
                    }
                }
            }
            QuestionType::FreeText => {
                // 必填空答时写入占位符并继续
                if question.required && answer.answer.trim().is_empty() {
                    tracing::warn!(
                        question_id = %question.question_id,
                        "[Executor] Required question received empty answer, using placeholder"
                    );
                    context.set_var("user_input", json!("(no input provided)"));
                } else {
                    context.set_var("user_input", json!(answer.answer.clone()));
                }

                context.record_decision(
                    DecisionType::ModifyParams,
                    &format!("User entered: {}", answer.answer),
                    "Use the information the user provided",
                    None,
                );
            }
            QuestionType::Confirmation => {
                if answer.answer == "yes" {
                    context.record_decision(
                        DecisionType::GenerateSteps,
                        "User confirmed; continue",
                        "The user confirmed the action",
                        None,
                    );
                } else {
                    context.record_decision(
                        DecisionType::SkipStep,
                        "User declined; skip related steps",
                        "The user chose not to run this action",
                        None,
                    );
                    return true; // 用户拒绝确认 → 跳过后续步骤
                }
            }
            QuestionType::Numeric | QuestionType::Date => {
                // Numeric/Date：原样写入 user_input（无必填空答占位）
                context.set_var("user_input", json!(answer.answer.clone()));
                context.record_decision(
                    DecisionType::ModifyParams,
                    &format!("User entered: {}", answer.answer),
                    "Use the information the user provided",
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
    pub(crate) async fn analyze_and_generate_dynamic_steps(
        &self,
        step: &RecipeStep,
        output: &Value,
        context: &mut ExecutionContext,
        recipe: &Recipe,
    ) -> Option<UserQuestion> {
        // 动态步骤队列已达 MAX_DYNAMIC_QUEUE(15) 则跳过分析
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

        // 跳过 ai.* / compare.* / prompt.generate / translate.text / code.explain。
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
                        QuestionOption::new("retry", "Retry")
                            .with_description("Run this step again"),
                        QuestionOption::new("skip", "Skip")
                            .with_description("Skip this step and continue"),
                        QuestionOption::new("cancel", "Cancel")
                            .with_description("Cancel the whole task"),
                    ],
                    true,
                ));
            }
        }

        // 获取 AI 分析器（ModelTier::Standard）
        let analyzer = self.get_analyzer_for_tier(ModelTier::Standard);
        let Some(analyzer) = analyzer else {
            tracing::debug!("[Executor] No AI analyzer available, skipping step analysis");
            return None;
        };

        // 截断输出以控制 prompt 大小
        let output_str = {
            let full = serde_json::to_string_pretty(output).unwrap_or_default();
            if full.len() > 3000 {
                // 按 UTF-8 字符边界截断到 3000 字节
                format!("{}...(truncated)", truncate_str(&full, 3000))
            } else {
                full
            }
        };

        let system_prompt = r#"You analyze a step result and decide whether it is clear enough, or the user must step in.

## Principle
Prefer finishing autonomously. Ask only on a real ambiguity. Do not re-ask what was already answered.

## Must ask (return ask)
- Irreversible change/delete that the user has not confirmed
- Search returned several very different entities and you cannot tell which they want
- A required param is missing and cannot be reasonably inferred

## Should continue (return continue)
- The result reasonably matches, even if not 100% exact
- Several hits, but the best one is obvious
- Pure information lookup
- They already answered a similar question (see answered record below)
- Intent can be inferred from context

Write question/context/label in the addressee's language.

## Output
No question: {"action":"continue"}
Need a question:
{
  "action": "ask",
  "questionType": "single_choice" | "free_text" | "confirmation",
  "question": "a short question",
  "context": "why they need to answer",
  "options": [{"value": "v1", "label": "shown text", "description": "optional note"}],
  "required": true
}

JSON only."#;

        // 构建已有 Q&A 历史，让 AI 知道用户已回答过什么
        let qa_history = if context.answered_questions.is_empty() {
            String::new()
        } else {
            let mut history = String::from("\n\nAlready answered (do not ask these again):\n");
            for (qid, ans) in &context.answered_questions {
                history.push_str(&format!("- Question {}: they answered \"{}\"\n", qid, ans));
            }
            history
        };

        // 构建用户选择/输入变量上下文
        let var_context = {
            let mut parts = Vec::new();
            if let Some(choice) = context.variables.get("user_choice") {
                parts.push(format!("Already chose: {}", choice));
            }
            if let Some(input) = context.variables.get("user_input") {
                parts.push(format!("Already entered: {}", input));
            }
            if parts.is_empty() {
                String::new()
            } else {
                format!("\nAlready provided: {}", parts.join("; "))
            }
        };

        let user_prompt = format!(
            "Intent: {}\nStep: {} (capability: {})\nResult:\n{}{}{}",
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
                // 提取 JSON（含 markdown 围栏的情况）
                let extracted = extract_json_object_from_ai_response(response);
                let json_str = extracted.as_deref().unwrap_or(response);

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
                            .unwrap_or("Please provide more information");
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
                            &format!("AI asked the user to step in: {question_text}"),
                            &format!("Step {} output needs clarification", step.id),
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
    pub(crate) fn check_dependencies(
        &self,
        step: &RecipeStep,
        outputs: &HashMap<String, Value>,
    ) -> bool {
        for dep_id in &step.depends_on {
            if !outputs.contains_key(dep_id) {
                return false;
            }
        }
        true
    }

    // 动态步骤生成器

    /// 处理步骤上的 StepGenerator，返回生成的动态步骤
    pub(crate) async fn process_step_generator(
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
    pub(crate) fn evaluate_condition(
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
        // 5. "output.field != null" → 非 null

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
}
