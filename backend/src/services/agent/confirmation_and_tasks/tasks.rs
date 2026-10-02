// Work task get/cancel/resume.

use super::super::agent_footer::*;
use super::super::agent_header::*;
use super::super::types::*;
use super::super::{executor, types};

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

    /// 验证可恢复任务的所有权、状态与引擎，并预检答案。全程不调用 AI。
    ///
    /// 独立于预算作用域之外：任务不存在、不在等待输入、或属于已下线的执行器——
    /// 这些都拿不到一个模型调用，不该记一次调用额度。
    async fn check_resumable_work(
        &self,
        task_id: &str,
        answer: &UserAnswer,
        user_id: i32,
    ) -> Result<(), String> {
        let task = executor::get_task_for_user(task_id, user_id)
            .await
            .ok_or("Task not found or access denied")?;
        if task.status != TaskStatus::WaitingForInput {
            return Err("Task is not waiting for input".to_string());
        }
        if task.recipe.is_none() {
            return Err(
                "Recipe not available for resume (task may have been loaded from DB after restart)"
                    .to_string(),
            );
        }
        self.validate_work_answer(task_id, answer, user_id).await
    }

    /// 恢复 WaitingForInput 任务执行
    pub async fn resume_task(
        &self,
        task_id: &str,
        answer: UserAnswer,
        user_id: i32,
    ) -> Result<AgentResponse, String> {
        self.check_resumable_work(task_id, &answer, user_id).await?;
        AgentTurnBudget::run_continuation(
            &self.db,
            user_id,
            "agent.resume_task",
            task_id.to_string(),
            Box::pin(self.resume_work_loop(task_id, answer, user_id, None)),
        )
        .await
    }

    /// 恢复 WaitingForInput 任务执行（带 SSE 进度流）
    pub async fn resume_task_with_progress(
        &self,
        task_id: &str,
        answer: UserAnswer,
        user_id: i32,
        progress_tx: tokio::sync::mpsc::Sender<types::AgentProgressEvent>,
    ) -> Result<AgentResponse, String> {
        self.check_resumable_work(task_id, &answer, user_id).await?;
        AgentTurnBudget::run_continuation(
            &self.db,
            user_id,
            "agent.resume_task_with_progress",
            task_id.to_string(),
            Box::pin(self.resume_work_loop(task_id, answer, user_id, Some(progress_tx))),
        )
        .await
    }
}
