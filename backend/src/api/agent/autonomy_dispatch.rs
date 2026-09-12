//! Claim autonomy-accepted proposals and enter the existing Work path.

use super::*;
use crate::services::agent::consciousness::{
    autonomy_claim_decision, build_autonomy_work_request, AcceptSource, AutonomyClaim,
    AutonomyGrantStore, IntentRecord, IntentStatus, IntentStore,
};
use crate::services::agent::queue::LaneQueue;
use crate::services::agent::run_hub::create_run;
use crate::services::agent::{
    Agent, AgentProgressEvent, AgentResponse, AgentResponseType, TaskStatus, LANE_QUEUE,
    SYSTEM_USER_ID,
};
use serde_json::{json, Value};

pub async fn tick_autonomy_work(db: DatabaseConnection) {
    let store = IntentStore::new(db.clone());
    if let Err(error) = store.expire_stale_global().await {
        tracing::warn!(%error, "[Autonomy] expire stale proposals failed");
    }
    let pending = match store.list_autonomy_accepted(4).await {
        Ok(pending) => pending,
        Err(error) => {
            tracing::warn!(%error, "[Autonomy] list accepted proposals failed");
            return;
        }
    };
    for intent in pending {
        if let Err(error) = dispatch_one(&db, intent).await {
            tracing::warn!(%error, "[Autonomy] dispatch failed");
        }
    }
}

async fn dispatch_one(db: &DatabaseConnection, intent: IntentRecord) -> Result<(), String> {
    if intent.user_id == SYSTEM_USER_ID || intent.accept_source != AcceptSource::Autonomy {
        return Ok(());
    }
    let grant = AutonomyGrantStore::new(db.clone())
        .find(intent.user_id)
        .await
        .map_err(|error| error.to_string())?;
    let granted: Vec<String> = crate::services::agent::get_user_permissions(db, intent.user_id)
        .await
        .into_iter()
        .collect();
    // Revoked or empty after re-filter: leave Accepted so the user can still
    // click the proposal card. User accept upgrades accept_source.
    let AutonomyClaim::Claim { cap } = autonomy_claim_decision(
        intent.user_id,
        intent.accept_source,
        grant.as_ref(),
        &granted,
    ) else {
        return Ok(());
    };

    let store = IntentStore::new(db.clone());
    // CAS Accepted → Running first so concurrent ticks do not create empty sessions.
    store
        .transition(
            &intent.id,
            intent.user_id,
            IntentStatus::Running,
            None,
            None,
            None,
        )
        .await
        .map_err(|error| error.to_string())?;

    let session_id = match ensure_session(
        db,
        None,
        intent.user_id,
        crate::services::agent::AgentInteractionMode::Work,
    )
    .await
    {
        Ok(session_id) => session_id,
        Err(error) => {
            let _ = store
                .reclaim_running_to_accepted(&intent.id, intent.user_id)
                .await;
            return Err(error);
        }
    };
    let run = create_run(intent.user_id, Some(session_id.clone())).await;
    let run_id = run.run_id().to_string();
    if let Err(error) = store
        .reattach_work(
            &intent.id,
            intent.user_id,
            session_id.clone(),
            run_id.clone(),
        )
        .await
    {
        tracing::warn!(%error, intent_id = %intent.id, "[Autonomy] attach session after claim failed");
    }

    let mut request = match build_autonomy_work_request(
        intent.user_id,
        &intent.proposal.instruction,
        &intent.id,
        session_id.clone(),
        cap,
    ) {
        Some(request) => request,
        None => {
            advance_intention_work(
                db,
                Some(intent.id.as_str()),
                intent.user_id,
                IntentStatus::Failed,
                Some("heartbeat identity cannot start personal Work".into()),
            )
            .await;
            return Err("heartbeat identity cannot start personal Work".into());
        }
    };
    let lane_key = LaneQueue::make_lane_key(intent.user_id, Some(&session_id));
    if let Some(ref mut ctx) = request.context {
        ctx.run_id = Some(run_id.clone());
        ctx.lane_key = Some(lane_key.clone());
    }
    let _ = persist_user_message(db, &session_id, &intent.proposal.instruction).await;
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel(256);
    let run_for_forwarder = run.clone();
    tokio::spawn(async move {
        while let Some(event) = progress_rx.recv().await {
            run_for_forwarder.publish(event).await;
        }
    });
    let _ = progress_tx
        .send(AgentProgressEvent::SessionCreated {
            session_id: session_id.clone(),
        })
        .await;
    let mut lane_guard = match LANE_QUEUE
        .acquire_timeout(
            &lane_key,
            std::time::Duration::from_secs(LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS),
        )
        .await
    {
        Ok(guard) => Some(guard),
        Err(error) => {
            advance_intention_work(
                db,
                Some(intent.id.as_str()),
                intent.user_id,
                IntentStatus::Failed,
                Some(error.clone()),
            )
            .await;
            return Err(error);
        }
    };
    let agent = Agent::new(db.clone()).await;
    match agent
        .process_with_progress(request, progress_tx.clone())
        .await
    {
        Ok(response) => {
            finish_autonomy_turn(
                db,
                &intent,
                &session_id,
                &run_id,
                response,
                progress_tx,
                &mut lane_guard,
            )
            .await;
        }
        Err(error) => {
            tracing::warn!(%error, intent_id = %intent.id, "[Autonomy] Work turn failed");
            advance_intention_work(
                db,
                Some(intent.id.as_str()),
                intent.user_id,
                IntentStatus::Failed,
                Some(error),
            )
            .await;
        }
    }
    Ok(())
}

async fn finish_autonomy_turn(
    db: &DatabaseConnection,
    intent: &IntentRecord,
    session_id: &str,
    run_id: &str,
    response: AgentResponse,
    progress_tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
    lane_guard: &mut Option<crate::services::agent::queue::LaneGuard>,
) {
    let api_response: ApiResponse = response.into();
    let task_id = api_response
        .task
        .as_ref()
        .map(|task| task.task_id.clone())
        .filter(|id| !id.is_empty())
        .or_else(|| {
            api_response
                .confirmation
                .as_ref()
                .map(|confirmation| format!("confirmation:{}", confirmation.confirmation_id))
        })
        .unwrap_or_default();
    let is_waiting = api_response
        .task
        .as_ref()
        .is_some_and(|task| task.status == "waiting_for_input")
        && !task_id.is_empty();
    let is_confirmation = api_response.confirmation.is_some()
        || api_response.response_type == "confirmation_required";
    let metadata = work_turn_session_metadata(&api_response, run_id, &task_id);
    let _ = persist_assistant_message(
        db,
        session_id,
        if task_id.is_empty() {
            None
        } else {
            Some(task_id.as_str())
        },
        &api_response.message,
        Some(metadata),
    )
    .await;

    if is_waiting {
        drop(lane_guard.take());
        advance_intention_work(
            db,
            Some(intent.id.as_str()),
            intent.user_id,
            IntentStatus::Waiting,
            Some(api_response.message.clone()),
        )
        .await;
        let loop_db = db.clone();
        let intent_id = intent.id.clone();
        let user_id = intent.user_id;
        let session_id = session_id.to_string();
        let run_id = run_id.to_string();
        tokio::spawn(async move {
            spawn_restored_wait_loop(
                user_id,
                task_id,
                session_id,
                run_id,
                progress_tx,
                Some(loop_db),
                Some(intent_id),
            )
            .await;
        });
        return;
    }

    drop(lane_guard.take());
    let status = completed_turn_intention_status(&api_response);
    advance_intention_work(
        db,
        Some(intent.id.as_str()),
        intent.user_id,
        status,
        Some(api_response.message.clone()),
    )
    .await;
    let response_value = if is_confirmation {
        park_confirmation_run(&api_response, &task_id)
    } else {
        serde_json::to_value(&api_response)
            .unwrap_or_else(|_| AppError::public_json("serialization failed"))
    };
    let _ = progress_tx
        .send(AgentProgressEvent::TaskCompleted {
            task_id,
            success: api_response.success,
            response: Box::new(response_value),
        })
        .await;
}

pub(crate) fn park_confirmation_run(api_response: &ApiResponse, task_id: &str) -> Value {
    let mut value = serde_json::to_value(api_response)
        .unwrap_or_else(|_| AppError::public_json("serialization failed"));
    if let Some(object) = value.as_object_mut() {
        object.insert("streamTerminal".into(), json!(false));
        let mut task = object.get("task").cloned().unwrap_or_else(|| json!({}));
        if !task.is_object() {
            task = json!({});
        }
        if let Some(task_object) = task.as_object_mut() {
            if !task_id.is_empty() {
                task_object.insert("taskId".into(), json!(task_id));
            }
            task_object.insert("status".into(), json!("waiting_for_input"));
        }
        object.insert("task".into(), task);
    }
    value
}

pub(crate) fn work_turn_session_metadata(
    api_response: &ApiResponse,
    run_id: &str,
    task_id: &str,
) -> Value {
    let mut base = serde_json::to_value(api_response).unwrap_or_else(|_| json!({}));
    if let Some(confirmation) = &api_response.confirmation {
        if let Some(obj) = base.as_object_mut() {
            let details = confirmation
                .pending_steps
                .iter()
                .map(|step| {
                    let impact = if step.impact.is_empty() {
                        String::new()
                    } else {
                        format!("\n{}", step.impact.join("\n"))
                    };
                    format!("{}: {}{impact}", step.capability_name, step.message)
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            obj.insert(
                "pendingQuestion".into(),
                json!({
                    "questionId": format!("confirmation:{}", confirmation.confirmation_id),
                    "confirmationId": confirmation.confirmation_id,
                    "questionType": "confirmation",
                    "question": api_response.message,
                    "context": details,
                    "required": true,
                    "riskLevel": confirmation.risk_level,
                    "expiresInSeconds": confirmation.expires_in_seconds,
                    "pendingSteps": confirmation.pending_steps,
                }),
            );
        }
    }
    session_metadata_with_run_identity(Some(base), run_id, task_id)
}

#[cfg(test)]
fn intention_status_from_response(response: &AgentResponse) -> IntentStatus {
    if matches!(
        response.response_type,
        AgentResponseType::ConfirmationRequired
    ) || response.confirmation.is_some()
        || response
            .task
            .as_ref()
            .is_some_and(|task| task.status == TaskStatus::WaitingForInput)
    {
        return IntentStatus::Waiting;
    }
    if matches!(
        response.response_type,
        AgentResponseType::Answer | AgentResponseType::TaskCompleted
    ) && response.is_successful_outcome()
    {
        return IntentStatus::Completed;
    }
    IntentStatus::Failed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_stays_waiting() {
        let response = AgentResponse {
            response_type: AgentResponseType::ConfirmationRequired,
            message: "confirm".into(),
            data: None,
            data_display: None,
            suggestions: vec![],
            task: None,
            confirmation: None,
            frontend_action: None,
            performance: None,
        };
        assert_eq!(
            intention_status_from_response(&response),
            IntentStatus::Waiting
        );
    }

    #[test]
    fn waiting_for_input_stays_waiting() {
        let response = AgentResponse {
            response_type: AgentResponseType::Answer,
            message: "need a date".into(),
            data: None,
            data_display: None,
            suggestions: vec![],
            task: Some(crate::services::agent::TaskState {
                task_id: "t1".into(),
                recipe_id: "r1".into(),
                status: TaskStatus::WaitingForInput,
                current_step: 0,
                step_results: Default::default(),
                started_at: chrono::Utc::now(),
                completed_at: None,
                error: None,
                progress: 0,
                pending_question: None,
                execution_context: None,
                lane_id: None,
                execution_trace: None,
                recipe: None,
            }),
            confirmation: None,
            frontend_action: None,
            performance: None,
        };
        assert_eq!(
            intention_status_from_response(&response),
            IntentStatus::Waiting
        );
    }

    #[test]
    fn waiting_turn_metadata_keeps_pending_question() {
        let mut response = ApiResponse {
            success: true,
            response_type: "answer".into(),
            message: "Which day?".into(),
            data: None,
            data_display: None,
            suggestions: vec![],
            task: Some(TaskInfo {
                task_id: "t1".into(),
                status: "waiting_for_input".into(),
                progress: 40,
                error: None,
                current_step: None,
                completed_steps: 0,
                total_steps: 1,
                dynamic_steps_added: 0,
                pending_question: Some(QuestionSummary {
                    question_id: "q1".into(),
                    question_type: "free_text".into(),
                    question: "Which day?".into(),
                    context: None,
                    options: None,
                    required: Some(true),
                    default_value: None,
                }),
                step_history: vec![],
                execution_trace: None,
            }),
            confirmation: None,
            frontend_action: None,
            performance: None,
            session_id: None,
        };
        let meta = work_turn_session_metadata(&response, "run_1", "t1");
        assert_eq!(meta["runId"], "run_1");
        assert_eq!(meta["taskId"], "t1");
        assert_eq!(meta["task"]["pendingQuestion"]["questionId"], "q1");

        response.task = None;
        response.response_type = "confirmation_required".into();
        response.confirmation = Some(ConfirmationInfo {
            confirmation_id: "c1".into(),
            risk_level: "high".into(),
            expires_in_seconds: 300,
            pending_steps: vec![PendingStepInfo {
                step_id: "s1".into(),
                capability_name: "mail.send".into(),
                message: "Send the note".into(),
                impact: vec!["Writes mail".into()],
            }],
        });
        let meta = work_turn_session_metadata(&response, "run_2", "confirmation:c1");
        assert_eq!(meta["pendingQuestion"]["confirmationId"], "c1");
        assert_eq!(meta["pendingQuestion"]["questionType"], "confirmation");
        assert_eq!(meta["taskId"], "confirmation:c1");
        assert!(meta["pendingQuestion"]["context"]
            .as_str()
            .unwrap()
            .contains("mail.send"));
        let parked = park_confirmation_run(&response, "confirmation:c1");
        assert_eq!(parked["task"]["status"], "waiting_for_input");
        assert_eq!(parked["streamTerminal"], false);

        response.task = Some(TaskInfo {
            task_id: "t2".into(),
            status: "waiting_for_input".into(),
            progress: 40,
            error: None,
            current_step: None,
            completed_steps: 0,
            total_steps: 1,
            dynamic_steps_added: 0,
            pending_question: Some(QuestionSummary {
                question_id: "q2".into(),
                question_type: "free_text".into(),
                question: "Which day?".into(),
                context: None,
                options: None,
                required: Some(true),
                default_value: None,
            }),
            step_history: vec![],
            execution_trace: None,
        });
        response.confirmation = None;
        response.response_type = "answer".into();
        let mut resume = work_turn_session_metadata(&response, "run_3", "t2");
        resume
            .as_object_mut()
            .expect("object")
            .insert("confirmationResume".into(), json!(true));
        assert_eq!(resume["confirmationResume"], true);
        assert_eq!(resume["runId"], "run_3");
        assert_eq!(resume["task"]["pendingQuestion"]["questionId"], "q2");
    }
}
use myriad_error::AppError;
