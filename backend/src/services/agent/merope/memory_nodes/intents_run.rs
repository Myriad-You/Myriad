//! The partner's instruction run (decision N3): one Work run of that one
//! thing, as the person who gave it, with nobody there to answer. It enters
//! the site's Work path the way an accepted proposal of hers does
//! (`run::autonomy_dispatch`): behind the gate every run starts behind, in
//! a Work conversation of its own, on the person's lane, with its progress
//! on a run of its own.
//!
//! Unattended (`RequestContext::unattended`): a confirmation it needs is
//! given only where unattended Work may run on its own (low and medium
//! risk, nothing that sets off further runs); anything higher is refused,
//! and a run that stops to ask ends there instead of waiting. Its grants
//! are those given with the instruction at most (`instruction_cap`), and
//! only as far as they are still granted at each call.

use serde_json::json;

use crate::services::agent::{
    Agent, AgentInteractionMode, AgentProgressEvent, LANE_QUEUE, RequestContext, UserRequest,
    queue::LaneQueue,
    run::{ApiResponse, agent_access_gate, forward_autonomy_progress, work_turn_session_metadata},
    run_hub::create_run,
    sessions::{ensure_session, persist_assistant_message, persist_user_message},
};

/// What its result line holds, at most.
const LINE_CHARS: usize = 400;

/// What came of running it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Ran {
    /// It ran: whether it went through, its answer, the conversation.
    Done {
        succeeded: bool,
        line: String,
        session: Option<String>,
    },
    /// The site has closed the agent to them since: nothing ran.
    Closed,
}

fn cut(text: &str) -> String {
    text.trim().chars().take(LINE_CHARS).collect()
}

fn failed(line: &str, session: Option<String>) -> Ran {
    Ran::Done {
        succeeded: false,
        line: cut(line),
        session,
    }
}

/// What Work is given: only that one thing, in their words, and a report
/// back. Their words are data: the instruction is theirs, what is around
/// it hers.
pub(crate) fn work_input(text: &str, quote: &str) -> String {
    format!(
        "The person you are with asked you earlier to do one thing at this time. Do only that \
         one thing, then say plainly what you did and how it went.\nWhat you took it as: \
         {text}\nTheir words then: «{quote}»"
    )
}

/// Runs `input` as `user_id`, unattended, under `cap` at most.
pub(crate) async fn run(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    input: &str,
    cap: Vec<String>,
) -> Ran {
    #[cfg(test)]
    if crate::services::channel_group::g0::on() {
        crate::services::channel_group::g0::note(
            "intent_work",
            json!({ "user": user_id, "input": input, "cap": cap }),
        );
        return Ran::Done {
            succeeded: true,
            line: "ran it".into(),
            session: None,
        };
    }
    if user_id <= 0 || agent_access_gate(db, user_id).await.is_err() {
        return Ran::Closed;
    }
    let session_id = match ensure_session(db, None, user_id, AgentInteractionMode::Work).await {
        Ok(session_id) => session_id,
        Err(error) => return failed(&error, None),
    };
    let run = create_run(user_id, Some(session_id.clone())).await;
    let (progress_tx, progress_rx) = tokio::sync::mpsc::channel(256);
    // Her managed background work: with every slot taken, the run goes on
    // without anyone watching its progress (what it sends is dropped).
    if !crate::services::agent::merope::background::spawn(
        "intent work progress",
        forward_autonomy_progress(run.clone(), progress_rx),
    ) {
        tracing::info!("[memory-nodes] an instruction's run goes on unwatched: no slot free");
    }
    let run_id = run.run_id().to_string();
    let lane_key = LaneQueue::make_lane_key(user_id, Some(&session_id));
    let request = UserRequest {
        raw_input: input.to_string(),
        timestamp: crate::clock::now(),
        user_id,
        context: Some(RequestContext {
            interaction_mode: AgentInteractionMode::Work,
            session_id: Some(session_id.clone()),
            run_id: Some(run_id.clone()),
            lane_key: Some(lane_key.clone()),
            unattended: true,
            instruction_cap: Some(cap),
            ..Default::default()
        }),
    };
    if let Err(error) = persist_user_message(db, &session_id, input).await {
        return failed(&error, Some(session_id));
    }
    let _ = progress_tx
        .send(AgentProgressEvent::SessionCreated {
            session_id: session_id.clone(),
        })
        .await;
    let lane = LANE_QUEUE
        .acquire_timeout(
            &lane_key,
            std::time::Duration::from_secs(LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS),
        )
        .await;
    let _lane = match lane {
        Ok(guard) => guard,
        Err(error) => return failed(&error, Some(session_id)),
    };
    let agent = Agent::new(db.clone()).await;
    match agent
        .process_with_progress(request, progress_tx.clone())
        .await
    {
        Ok(response) => {
            let succeeded = response.is_successful_outcome();
            let api: ApiResponse = response.into();
            let task_id = api
                .task
                .as_ref()
                .map(|task| task.task_id.clone())
                .unwrap_or_default();
            let metadata = work_turn_session_metadata(&api, &run_id, &task_id);
            let _ = persist_assistant_message(
                db,
                &session_id,
                (!task_id.is_empty()).then_some(task_id.as_str()),
                &api.message,
                Some(metadata),
            )
            .await;
            let line = cut(&api.message);
            let response = serde_json::to_value(&api).unwrap_or_else(|_| json!({}));
            let _ = progress_tx
                .send(AgentProgressEvent::TaskCompleted {
                    task_id,
                    success: api.success,
                    response: Box::new(response),
                })
                .await;
            Ran::Done {
                succeeded: succeeded && api.success,
                line,
                session: Some(session_id),
            }
        }
        Err(error) => failed(&error, Some(session_id)),
    }
}
