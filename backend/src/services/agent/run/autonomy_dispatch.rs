//! Claim autonomy-accepted proposals and enter the existing Work path.

use super::*;
use crate::services::agent::consciousness::{
    AcceptSource, AutonomyClaim, AutonomyGrantStore, IntentRecord, IntentStatus, IntentStore,
    autonomy_claim_decision, build_autonomy_work_request,
};
use crate::services::agent::queue::LaneQueue;
use crate::services::agent::run_hub::{AgentRun, create_run};
use crate::services::agent::sessions::{
    ensure_session, persist_user_message, require_user_message_persisted,
};
use crate::services::agent::{
    Agent, AgentProgressEvent, AgentResponse, LANE_QUEUE, SYSTEM_USER_ID,
};
use serde_json::{Value, json};

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
        // A row the query still returned must not keep the head of the queue.
        defer_skipped_autonomy(db, &intent).await;
        return Ok(());
    }
    // The same gate every run starts behind: the site may have closed the
    // agent to this person since they accepted. Left Accepted, like a
    // revoked grant, so it can run if it opens again.
    if super::start::agent_access_gate(db, intent.user_id)
        .await
        .is_err()
    {
        defer_skipped_autonomy(db, &intent).await;
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
        // Keep Accepted so the user can still click the card, but leave the
        // oldest-first batch. expires_at is what expiry uses, not updated_at.
        defer_skipped_autonomy(db, &intent).await;
        return Ok(());
    };

    let store = IntentStore::new(db.clone());
    // CAS Accepted → Running first so concurrent ticks do not create empty
    // sessions, and only while the source is still autonomy: a user who
    // accepted the card in the meantime owns it and runs it with their input.
    if !store
        .claim_for_autonomy(&intent.id, intent.user_id)
        .await
        .map_err(|error| error.to_string())?
    {
        return Ok(());
    }

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
    let (progress_tx, progress_rx) = tokio::sync::mpsc::channel(256);
    tokio::spawn(forward_autonomy_progress(run.clone(), progress_rx));
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
    if let Err(error) = require_user_message_persisted(
        persist_user_message(db, &session_id, &intent.proposal.instruction).await,
    ) {
        let _ = store
            .reclaim_running_to_accepted(&intent.id, intent.user_id)
            .await;
        return Err(error);
    }
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

async fn defer_skipped_autonomy(db: &DatabaseConnection, intent: &IntentRecord) {
    if let Err(error) = IntentStore::new(db.clone())
        .defer_autonomy_skip(&intent.id, intent.user_id)
        .await
    {
        tracing::warn!(
            %error,
            intent_id = %intent.id,
            "[Autonomy] could not defer skipped proposal"
        );
    }
}

async fn forward_autonomy_progress(
    run: Arc<AgentRun>,
    mut events: tokio::sync::mpsc::Receiver<AgentProgressEvent>,
) {
    // The producer is installed immediately after create_run, before any
    // fallible work. Its final sender is also dropped if the driver is aborted.
    let mut outcome_emitted = false;
    while let Some(event) = events.recv().await {
        outcome_emitted |= matches!(
            &event,
            AgentProgressEvent::TaskCompleted { .. } | AgentProgressEvent::Error { .. }
        );
        run.publish(event).await;
    }
    // A confirmation uses a nonterminal TaskCompleted to park the run. A
    // waiting question keeps a sender in its wait loop. Preserve both, and
    // never publish an error over a run already completed by another observer.
    if !outcome_emitted && !run.snapshot().await.2 {
        run.publish(AgentProgressEvent::Error {
            task_id: None,
            message: "Autonomous task stopped before producing a result".into(),
            code: "AUTONOMY_DISPATCH_FAILED".into(),
        })
        .await;
    }
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
        .unwrap_or_default();
    let is_waiting = api_response
        .task
        .as_ref()
        .is_some_and(|task| task.status == "waiting_for_input")
        && !task_id.is_empty();
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
                loop_db,
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
    let response_value = serde_json::to_value(&api_response)
        .unwrap_or_else(|_| AppError::public_json("serialization failed"));
    let _ = progress_tx
        .send(AgentProgressEvent::TaskCompleted {
            task_id,
            success: api_response.success,
            response: Box::new(response_value),
        })
        .await;
}

pub(crate) fn work_turn_session_metadata(
    api_response: &ApiResponse,
    run_id: &str,
    task_id: &str,
) -> Value {
    let base = serde_json::to_value(api_response).unwrap_or_else(|_| json!({}));
    session_metadata_with_run_identity(Some(base), run_id, task_id)
}

#[cfg(test)]
fn intention_status_from_response(response: &AgentResponse) -> IntentStatus {
    if response
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
    use crate::services::agent::consciousness::AutonomyGrantView;

    /// Oldest-first batch of 4. A skip stamps `updated_at` so the row leaves
    /// the head; a claim is a dispatch. This is the queue contract
    /// `defer_autonomy_skip` implements.
    fn advance_batch(
        rows: &mut [(String, chrono::DateTime<chrono::Utc>, AutonomyClaim, bool)],
        now: chrono::DateTime<chrono::Utc>,
    ) -> Vec<String> {
        let mut order: Vec<usize> = (0..rows.len()).filter(|index| !rows[*index].3).collect();
        order.sort_by_key(|index| rows[*index].1);
        let mut dispatched = Vec::new();
        for index in order.into_iter().take(4) {
            match &rows[index].2 {
                AutonomyClaim::Claim { .. } => {
                    rows[index].3 = true;
                    dispatched.push(rows[index].0.clone());
                }
                AutonomyClaim::Skip => {
                    rows[index].1 = now;
                }
            }
        }
        dispatched
    }

    #[test]
    fn revoked_rows_do_not_block_the_next_autonomy_tick() {
        let start = chrono::DateTime::parse_from_rfc3339("2026-09-24T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let granted = vec!["calendar:read".to_string()];
        let mut rows = Vec::new();
        for index in 0..4 {
            let grant = AutonomyGrantView {
                user_id: 10 + index,
                allowed_permissions: granted.clone(),
                revoked: true,
            };
            rows.push((
                format!("revoked-{index}"),
                start + chrono::Duration::seconds(index as i64),
                autonomy_claim_decision(10 + index, AcceptSource::Autonomy, Some(&grant), &granted),
                false,
            ));
            assert!(
                matches!(rows[index as usize].2, AutonomyClaim::Skip),
                "revoked grant must be skipped"
            );
        }
        let live = AutonomyGrantView {
            user_id: 42,
            allowed_permissions: granted.clone(),
            revoked: false,
        };
        rows.push((
            "live".into(),
            start + chrono::Duration::seconds(10),
            autonomy_claim_decision(42, AcceptSource::Autonomy, Some(&live), &granted),
            false,
        ));
        assert!(matches!(rows[4].2, AutonomyClaim::Claim { .. }));

        let first = advance_batch(&mut rows, start + chrono::Duration::minutes(1));
        assert!(
            first.is_empty(),
            "the first tick only sees the four revoked heads"
        );
        let second = advance_batch(&mut rows, start + chrono::Duration::minutes(2));
        assert_eq!(second, vec!["live".to_string()]);
        assert!(rows[4].3, "the live proposal was dispatched");
        assert!(rows[..4].iter().all(|row| !row.3));
        assert!(rows[..4].iter().all(|row| row.1 > rows[4].1));
    }

    #[test]
    fn skipped_dispatch_defers_instead_of_returning_unchanged() {
        let src = include_str!("autonomy_dispatch.rs");
        assert!(
            src.matches("defer_skipped_autonomy(db, &intent)").count() >= 2,
            "both skip returns must leave the queue head"
        );
    }

    #[tokio::test]
    async fn autonomy_producer_error_closes_running_run() {
        let run = AgentRun::new_for_test("autonomy-producer-error", 7431);
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        tx.send(AgentProgressEvent::SessionCreated {
            session_id: "session".into(),
        })
        .await
        .unwrap();
        tx.send(AgentProgressEvent::WaitingForInput {
            task_id: "task".into(),
            question_id: "question".into(),
            question_type: "free_text".into(),
            question: "provisional".into(),
            context: None,
            options: None,
            required: true,
            default_value: None,
        })
        .await
        .unwrap();
        drop(tx); // Queue/quota errors and dropped dispatch futures release the producer.
        forward_autonomy_progress(run.clone(), rx).await;
        let (events, _, completed) = run.snapshot().await;
        assert!(completed, "failed autonomy dispatch left a running orphan");
        assert!(matches!(
            events.last().map(|event| &event.event),
            Some(AgentProgressEvent::Error { .. })
        ));
    }

    #[tokio::test]
    async fn aborted_autonomy_producer_closes_run_after_sender_drop() {
        let run = AgentRun::new_for_test("autonomy-producer-abort", 7434);
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let forwarder = tokio::spawn(forward_autonomy_progress(run.clone(), rx));
        let (ready, entered) = tokio::sync::oneshot::channel();
        let producer = tokio::spawn(async move {
            let _sender = tx;
            let _ = ready.send(());
            std::future::pending::<()>().await;
        });
        entered.await.unwrap();
        assert!(
            !run.snapshot().await.2,
            "a live producer must retain its run"
        );
        producer.abort();
        let _ = producer.await;
        tokio::time::timeout(std::time::Duration::from_secs(1), forwarder)
            .await
            .unwrap()
            .unwrap();
        assert!(run.snapshot().await.2);
    }

    #[tokio::test]
    async fn autonomy_forwarder_preserves_success_and_confirmation_wait() {
        for waiting in [false, true] {
            let run = AgentRun::new_for_test(format!("autonomy-closed-{waiting}"), 7432);
            let (tx, rx) = tokio::sync::mpsc::channel(4);
            tx.send(AgentProgressEvent::TaskCompleted {
                task_id: "task".into(), success: true,
                response: Box::new(json!({"task": {"status": if waiting { "waiting_for_input" } else { "completed" }}})),
            }).await.unwrap();
            drop(tx);
            forward_autonomy_progress(run.clone(), rx).await;
            let (events, _, completed) = run.snapshot().await;
            assert_eq!(events.len(), 1, "forwarder replaced an existing outcome");
            assert_eq!(completed, !waiting);
        }
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
            frontend_action: None,
            performance: None,
            session_id: None,
        };
        let meta = work_turn_session_metadata(&response, "run_1", "t1");
        assert_eq!(meta["runId"], "run_1");
        assert_eq!(meta["taskId"], "t1");
        assert_eq!(meta["task"]["pendingQuestion"]["questionId"], "q1");

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
#[cfg(test)]
use crate::services::agent::{AgentResponseType, TaskStatus};
use myriad_error::AppError;
