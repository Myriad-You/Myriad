// Work task get/cancel/resume.

use super::super::agent_footer::*;
use super::super::agent_header::*;
use super::super::types::*;
use super::super::{executor, response_agent, types};

impl Agent {
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

    /// 取出可恢复任务的 recipe，并验证所有权与状态。全程不调用 AI。
    ///
    /// 独立于预算作用域之外：任务不存在、不在等待输入、重启后丢了 recipe——这三种
    /// 都拿不到一个模型调用，不该记一次调用额度。
    async fn resolve_resumable_recipe(
        &self,
        task_id: &str,
        user_id: i32,
    ) -> Result<Recipe, String> {
        // 获取任务并验证所有权
        let task = executor::get_task_for_user(task_id, user_id)
            .await
            .ok_or("Task not found or access denied")?;

        if task.status != TaskStatus::WaitingForInput {
            return Err("Task is not waiting for input".to_string());
        }

        // 从 task_state 中取出保存的 recipe
        task.recipe.ok_or_else(|| {
            "Recipe not available for resume (task may have been loaded from DB after restart)"
                .to_string()
        })
    }

    /// 恢复 WaitingForInput 任务执行
    pub async fn resume_task(
        &self,
        task_id: &str,
        answer: UserAnswer,
        user_id: i32,
    ) -> Result<AgentResponse, String> {
        let recipe = self.resolve_resumable_recipe(task_id, user_id).await?;
        AgentTurnBudget::run_continuation(
            &self.db,
            user_id,
            "agent.resume_task",
            task_id.to_string(),
            Box::pin(self.resume_task_inner(task_id, answer, user_id, &recipe)),
        )
        .await
    }

    async fn resume_task_inner(
        &self,
        task_id: &str,
        answer: UserAnswer,
        user_id: i32,
        recipe: &Recipe,
    ) -> Result<AgentResponse, String> {
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
                response_agent::error_message(
                    task_state.error.as_deref().unwrap_or("Processing failed"),
                )
            } else {
                response_agent::completion_message()
            },
            data: Some(result),
            data_display: None,
            suggestions: vec![],
            task: Some(task_state),
            confirmation: None,
            frontend_action,
            performance: None,
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
        let recipe = self.resolve_resumable_recipe(task_id, user_id).await?;
        AgentTurnBudget::run_continuation(
            &self.db,
            user_id,
            "agent.resume_task_with_progress",
            task_id.to_string(),
            Box::pin(self.resume_task_with_progress_inner(
                task_id,
                answer,
                user_id,
                progress_tx,
                &recipe,
            )),
        )
        .await
    }

    async fn resume_task_with_progress_inner(
        &self,
        task_id: &str,
        answer: UserAnswer,
        user_id: i32,
        progress_tx: tokio::sync::mpsc::Sender<types::AgentProgressEvent>,
        recipe: &Recipe,
    ) -> Result<AgentResponse, String> {
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
                response_agent::error_message(
                    task_state.error.as_deref().unwrap_or("Processing failed"),
                )
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
            performance: None,
        })
    }
}
