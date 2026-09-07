// Chat-only Agent path. Strict Lite; no Planner, Recipe, or Pro.

use serde_json::{json, Value};

use super::agent_header::Agent;
use super::motion_overlay::{
    attach_motion_to_result, motion_context, spawn_chat_motion_refinement, try_publish_motion,
};
use super::response_agent;
use super::types::*;

impl Agent {
    pub(super) async fn process_chat(
        &self,
        request: UserRequest,
        mood_transition: Option<crate::services::agent::merope::MoodTransition>,
        round_motion_style: String,
        memory_input_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    ) -> Result<AgentResponse, String> {
        let user_id = request.user_id;
        crate::services::agent::merope::note_chat_diary(&self.db, user_id, &request.raw_input)
            .await;
        let reply = match self.strict_lite_chat_response(&request).await {
            Ok(reply) => reply,
            Err(error) => {
                crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                return Err(error);
            }
        };
        crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
        let (reply, music) = publish_model_outfit_overlay(&self.db, &request, &reply, None).await;
        crate::services::agent::merope::spawn_chat_remember(
            user_id,
            request.raw_input.clone(),
            reply.clone(),
            memory_input_at,
        );
        return attach_motion_to_result(
            Ok(AgentResponse {
                response_type: AgentResponseType::Answer,
                message: reply.clone(),
                data: Some(chat_reply_with_overlay(&request, &reply)),
                data_display: None,
                suggestions: vec![],
                task: None,
                confirmation: None,
                frontend_action: music.map(chat_music_frontend_action),
                performance: None,
            }),
            &request,
            mood_transition.clone(),
            &round_motion_style,
        )
        .await;
    }

    pub(super) async fn process_chat_with_progress(
        &self,
        request: UserRequest,
        progress_tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
        mood_transition: Option<crate::services::agent::merope::MoodTransition>,
        round_motion_style: String,
        memory_input_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    ) -> Result<AgentResponse, String> {
        let user_id = request.user_id;

        let _ = progress_tx
            .send(AgentProgressEvent::Progress {
                progress: 5,
                completed_steps: 0,
                total_steps: 1,
                message: response_agent::understanding_request(),
            })
            .await;

        // React without waiting on the model or the action queue. The director
        // observes emitted prose on its own task, never on the token callback.
        let mut speech_delivery = None;
        let mut motion_refinement = None;
        let live_direction = if let Some(run_id) = request
            .context
            .as_ref()
            .and_then(|ctx| ctx.run_id.as_deref())
        {
            super::run_hub::get_live_run_for_user(run_id, user_id)
                .await
                .map(|run| run.playback_direction.clone())
        } else {
            None
        };
        if let Some(mood) = mood_transition.clone() {
            let reaction_context = motion_context(
                &request,
                user_id,
                crate::services::agent::merope::MotionPhase::Reaction,
                mood,
                round_motion_style.clone(),
                None,
                None,
            );
            if let Some(performance) =
                crate::services::agent::merope::local_directive(&reaction_context)
            {
                try_publish_motion(&progress_tx, performance);
            }
            let (delivery, guard) = spawn_chat_motion_refinement(
                reaction_context,
                progress_tx.clone(),
                live_direction.clone(),
            );
            speech_delivery = Some(std::sync::Arc::new(std::sync::Mutex::new(delivery)));
            motion_refinement = Some(guard);
        }

        // Persistence still precedes chat context construction, but must
        // not stand in front of the character's immediate reaction.
        crate::services::agent::merope::note_chat_diary(&self.db, user_id, &request.raw_input)
            .await;

        let reply = match self
            .stream_strict_lite_chat_response(&request, &progress_tx, speech_delivery)
            .await
        {
            Ok(reply) => {
                publish_model_outfit_overlay(&self.db, &request, &reply, Some(&progress_tx))
                    .await
                    .0
            }
            Err(error) => {
                if let Some(live) = &live_direction {
                    live.close();
                }
                crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
                return Err(error);
            }
        };

        // Close text without waiting for the director. Already-published plans
        // remain owned by the client's playback clock, not this generation task.
        response_agent::finish_stream(&progress_tx).await;
        crate::services::agent::merope::spawn_chat_remember(
            user_id,
            request.raw_input.clone(),
            reply.clone(),
            memory_input_at,
        );
        let performance = None;

        crate::services::agent::merope::mark_activity(&self.db, user_id, "idle").await;
        let _ = progress_tx
            .send(AgentProgressEvent::Progress {
                progress: 100,
                completed_steps: 1,
                total_steps: 1,
                message: response_agent::done_status(),
            })
            .await;

        // Transient results have a separate playback lifetime. No post-terminal
        // event is appended to the durable run, and no lane waits on the model.
        let refined = if let Some(guard) = motion_refinement {
            if let Some(live) = live_direction {
                guard.finish_for_playback(live);
                true // The immediate floor is already installed; don't overwrite it.
            } else {
                guard.stop().await == super::motion_overlay::MotionPublication::Refined
            }
        } else {
            if let Some(live) = live_direction {
                live.close();
            }
            false
        };
        // The abort barrier above makes this choice race-free: a local landing
        // cannot overwrite a semantic baseline published concurrently. No cues
        // are replayed, and none of this stands in front of text completion.
        if !refined {
            if let Some(mood) = mood_transition {
                let delivery_context = motion_context(
                    &request,
                    user_id,
                    crate::services::agent::merope::MotionPhase::Delivery,
                    mood,
                    round_motion_style,
                    Some(reply.clone()),
                    None,
                );
                if let Some(mut performance) =
                    crate::services::agent::merope::local_directive(&delivery_context)
                {
                    performance.plan.cues.clear();
                    try_publish_motion(&progress_tx, performance);
                }
            }
        }

        return Ok(AgentResponse {
            response_type: AgentResponseType::Answer,
            message: reply.clone(),
            data: Some(chat_reply_with_overlay(&request, &reply)),
            data_display: None,
            suggestions: vec![],
            task: None,
            confirmation: None,
            frontend_action: None,
            performance,
        });
    }
}

fn chat_music_frontend_action(
    action: crate::services::agent::chat_music::ChatMusicAction,
) -> Value {
    json!({
        "type": "music_control",
        "action": action.as_str(),
        "timestamp": chrono::Utc::now().timestamp_millis(),
    })
}

fn chat_reply_with_overlay(request: &UserRequest, reply: &str) -> Value {
    let mut data = crate::services::agent::chat_prompt::chat_reply_data(reply, &request.raw_input);
    let session_id = request
        .context
        .as_ref()
        .and_then(|context| context.session_id.as_deref())
        .unwrap_or("");
    if let Some(object) = data.as_object_mut() {
        object.insert(
            "outfitId".into(),
            match crate::services::agent::merope::overlay_outfit_id(request.user_id, session_id) {
                Some(id) => Value::String(id),
                None => Value::Null,
            },
        );
    }
    data
}

async fn publish_model_outfit_overlay(
    db: &sea_orm::DatabaseConnection,
    request: &UserRequest,
    reply: &str,
    progress_tx: Option<&tokio::sync::mpsc::Sender<AgentProgressEvent>>,
) -> (
    String,
    Option<crate::services::agent::chat_music::ChatMusicAction>,
) {
    let (spoken, directive, music) =
        crate::services::agent::chat_music::peel_chat_live_reply(reply);
    let Some(directive) =
        myriad_merope::wear_directive_after_reply(&request.raw_input, &spoken, directive)
    else {
        return (spoken, music);
    };
    let Some(session_id) = request
        .context
        .as_ref()
        .and_then(|context| context.session_id.as_deref())
    else {
        return (spoken, music);
    };
    let Some(outfit_id) = crate::services::agent::merope::apply_model_wear_directive(
        db,
        request.user_id,
        session_id,
        &directive,
    )
    .await
    else {
        return (spoken, music);
    };
    if let Some(progress_tx) = progress_tx {
        let _ = progress_tx
            .send(AgentProgressEvent::OutfitOverlay { outfit_id })
            .await;
    }
    (spoken, music)
}
