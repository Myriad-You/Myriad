// Work entry points. Saved Recipes use their separate executor path.
use super::agent_header::*;
use super::motion_overlay::attach_motion_to_result;
use super::types::*;

impl Agent {
    pub(super) async fn process_work(
        &self,
        request: UserRequest,
        mood_transition: Option<crate::services::agent::merope::MoodTransition>,
        round_motion_style: String,
    ) -> Result<AgentResponse, String> {
        super::merope::note_chat_diary(&self.db, request.user_id, &request.raw_input).await;
        let result = self.start_work_loop(request.clone(), None).await;
        attach_motion_to_result(result, &request, mood_transition, &round_motion_style).await
    }

    pub(super) async fn process_work_with_progress(
        &self,
        request: UserRequest,
        progress_tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
        mood_transition: Option<crate::services::agent::merope::MoodTransition>,
        round_motion_style: String,
    ) -> Result<AgentResponse, String> {
        super::merope::note_chat_diary(&self.db, request.user_id, &request.raw_input).await;
        let result = self
            .start_work_loop(request.clone(), Some(progress_tx))
            .await;
        attach_motion_to_result(result, &request, mood_transition, &round_motion_style).await
    }
}
