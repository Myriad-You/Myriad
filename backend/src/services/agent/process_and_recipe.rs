// Agent process / recipe execution paths.

use sea_orm::DatabaseConnection;

use super::agent_footer::*;
use super::agent_header::*;
use super::motion_overlay::{round_motion_style, utterance_index_in_session};
use super::types::*;

impl Agent {
    /// 创建新的 Agent 实例
    pub async fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// 处理用户请求
    ///
    /// Chat vs Work (`process_inner`). Chat does not consume Pro and must not
    /// emit Recipe/tool calls. Work uses the native tool loop in `work_loop`
    /// Work requests Pro; configuration may fall back to Standard.
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

        tracing::info!(
            user_id = user_id,
            input = %request.raw_input,
            "[Agent] Processing request"
        );

        crate::services::agent::consciousness::remember_live_presence(
            user_id,
            crate::services::agent::consciousness::live_presence_from_request(&request),
        );
        crate::services::agent::merope::mark_activity(&self.db, user_id, "thinking").await;
        let (mood_transition, memory_input_at) = crate::services::agent::merope::note_user_turn(
            &self.db,
            &request,
            utterance_index_in_session(&request),
        )
        .await
        .unzip();
        let round_motion_style = round_motion_style(&request, mood_transition.as_ref()).await;

        // Chat does not consume Pro and must not emit Recipe/tool calls.
        if request.context.as_ref().is_some_and(|context| {
            context.interaction_mode == crate::services::agent::AgentInteractionMode::Chat
        }) {
            return self
                .process_chat(
                    request,
                    mood_transition,
                    round_motion_style,
                    memory_input_at,
                )
                .await;
        }

        self.process_work(request, mood_transition, round_motion_style)
            .await
    }

    /// Same Chat/Work split as `process`, plus `progress_tx`.
    /// Work-with-progress may escalate after Recipe.
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
            mode = request
                .context
                .as_ref()
                .map(|context| context.interaction_mode.as_str())
                .unwrap_or("work"),
            has_history = request.context.as_ref().and_then(|c| c.conversation_history.as_ref()).is_some(),
            "[Agent] Processing request with progress tracking"
        );

        crate::services::agent::consciousness::remember_live_presence(
            user_id,
            crate::services::agent::consciousness::live_presence_from_request(&request),
        );
        crate::services::agent::merope::mark_activity(&self.db, user_id, "thinking").await;
        let (mood_transition, memory_input_at) = crate::services::agent::merope::note_user_turn(
            &self.db,
            &request,
            utterance_index_in_session(&request),
        )
        .await
        .unzip();
        let round_motion_style = round_motion_style(&request, mood_transition.as_ref()).await;

        if let Some(mood) = mood_transition.clone() {
            let _ = progress_tx
                .send(AgentProgressEvent::MeropeStateChanged {
                    mood: mood.clone(),
                    activity: "thinking".to_string(),
                })
                .await;
        }

        // Chat 是独立的人设对话路径。在 Planner 之前分流才能保证：
        // 1. 不消耗 Pro；2. 即使输入像操作指令，也不会生成 Recipe/Tool Call。
        // Chat does not consume Pro and must not emit Recipe/tool calls.
        if request.context.as_ref().is_some_and(|context| {
            context.interaction_mode == crate::services::agent::AgentInteractionMode::Chat
        }) {
            return self
                .process_chat_with_progress(
                    request,
                    progress_tx,
                    mood_transition,
                    round_motion_style,
                    memory_input_at,
                )
                .await;
        }

        self.process_work_with_progress(request, progress_tx, mood_transition, round_motion_style)
            .await
    }

    /// 运行已保存的预设（`POST /presets/{id}/execute`）。
    ///
    /// 与普通回合一样跑在一次 AI 配额预留里；步骤由工具循环逐步执行。
    pub async fn execute_preset(
        &self,
        request: UserRequest,
        preset_id: i32,
        progress_tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
    ) -> Result<AgentResponse, String> {
        let user_id = request.user_id;
        AgentTurnBudget::run(
            &self.db,
            user_id,
            "agent.execute_preset",
            format!(
                "preset_{preset_id}_{}",
                request.timestamp.timestamp_millis()
            ),
            Box::pin(self.start_preset_work_loop(request, preset_id, Some(progress_tx))),
        )
        .await
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

#[cfg(test)]
mod split_path_contract_tests {
    /// Chat must still branch before Work/Planner on the shipped dispatcher.
    #[test]
    fn chat_mode_still_branches_before_process_work() {
        let src = include_str!("process_and_recipe.rs");
        let inner = src
            .split("async fn process_inner")
            .nth(1)
            .expect("process_inner");
        let chat = inner
            .find("AgentInteractionMode::Chat")
            .expect("Chat branch");
        let work = inner.find("process_work(").expect("Work call");
        assert!(chat < work, "Chat must run before process_work");
        assert!(
            !inner[..work].contains("planner"),
            "Planner must not run before the Chat/Work split"
        );
    }
}
