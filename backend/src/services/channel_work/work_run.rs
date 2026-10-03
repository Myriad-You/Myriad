//! Their Work started from a chat, and a run picked up again once a pending prompt is answered.

use super::*;

/// Start their Work with images already cached: what they asked for, or what
/// she handed off for them.
pub(super) async fn start_work_run(
    db: DatabaseConnection,
    user_id: i32,
    session_id: String,
    input: &str,
    custom_data: Option<Value>,
    sink: ChannelSink,
    session_key: &str,
) {
    let input = if input.trim().is_empty() && custom_data.is_some() {
        "请查看这张图片。"
    } else {
        input
    };
    let run = match crate::services::agent::run::start_for_user(
        db.clone(),
        user_id,
        ProcessRequest {
            input: input.to_string(),
            context: Some(ProcessContext {
                mode: Some(AgentInteractionMode::Work),
                session_id: (!session_id.is_empty()).then_some(session_id),
                current_route: None,
                active_platforms: None,
                conversation_history: None,
                custom_data: custom_data.clone(),
                intention_id: None,
                autonomy_permission_cap: None,
                rig_state: None,
                group: None,
                channel_chat: None,
                from_channel: true,
            }),
        },
    )
    .await
    {
        Ok(run) => run,
        Err(error) => {
            let body = error.to_json();
            let message = body
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| body.get("error").and_then(Value::as_str))
                .unwrap_or("办事没能开始。")
                .to_string();
            warn!(error = %message, "channel Work start failed");
            let _ = sink.send_text(&message).await;
            return;
        }
    };
    start_delivery(
        run,
        db,
        user_id,
        session_key.to_string(),
        sink,
        input.to_string(),
    )
    .await;
}

pub(super) async fn resume_pending(
    db: DatabaseConnection,
    session_id: String,
    session_key: &str,
    user_id: i32,
    kind: PendingKind,
    answer: String,
    sink: ChannelSink,
    latest_input: &str,
    parked: StoredPending,
) {
    sink.send_typing().await;
    let sid = (!session_id.is_empty()).then_some(session_id);
    let mut next_original = latest_input.to_string();
    let run = match kind {
        PendingKind::Clarify { original_input } => {
            let (input, parked_original) = clarify_followup(&original_input, &answer);
            next_original = parked_original;
            crate::services::agent::run::start_for_user(
                db.clone(),
                user_id,
                ProcessRequest {
                    input,
                    context: Some(ProcessContext {
                        mode: Some(AgentInteractionMode::Work),
                        session_id: sid,
                        current_route: None,
                        active_platforms: None,
                        conversation_history: None,
                        custom_data: None,
                        intention_id: None,
                        autonomy_permission_cap: None,
                        rig_state: None,
                        group: None,
                        channel_chat: None,
                        from_channel: true,
                    }),
                },
            )
            .await
            .map_err(|error| error.to_json())
        }
        PendingKind::Answer {
            task_id,
            question_id,
            ..
        } => crate::services::agent::run::answer_for_user(
            db.clone(),
            user_id,
            task_id,
            question_id,
            answer,
            sid,
        )
        .await
        .map_err(|error| error.to_json()),
    };

    match run {
        Ok(run) => {
            start_delivery(
                run,
                db,
                user_id,
                session_key.to_string(),
                sink,
                next_original,
            )
            .await;
        }
        Err(body) => {
            if sink.authorized().await {
                if let Err(error) = shared_registry::put(
                    &db,
                    sink.platform().pending_ns(),
                    session_key,
                    identity(user_id),
                    &parked,
                    parked
                        .prompt
                        .expires_at_unix
                        .unwrap_or_else(|| (Utc::now() + ChronoDuration::days(2)).timestamp()),
                )
                .await
                {
                    warn!(%error, "cannot restore unaccepted channel answer");
                }
            }
            let message = body
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| body.get("error").and_then(Value::as_str))
                .unwrap_or("这一步没能继续。")
                .to_string();
            warn!(error = %message, "channel resume failed");
            let _ = sink.send_text(&message).await;
        }
    }
}
