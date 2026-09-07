//! Telegram DM Work ingest: paired text / callback → start / resume → sendMessage.
//!
//! Mode is locked to Work. Thinking stays dropped. Clarification, confirmation,
//! and waiting_for_input stay in the same chat and get inline buttons.
//! frontendAction still fails visibly. There is no passive window.

use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use myriad_agent_rules::channel::{
    decide_pending_reply, format_pending_prompt, pending_prompt_from_model_json, plan_delivery,
    telegram_callback_action, telegram_force_reply_markup, telegram_reply_markup, ChannelEvent,
    DeliveryContext, DeliveryPlan, PendingDecision, PendingKind, PendingOption, PendingPrompt,
    TelegramCallbackAction, PANEL_REQUIRED_REPLY,
};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement, Value as SeaValue,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{info, warn};

use crate::api::agent::{ProcessContext, ProcessRequest};
use crate::middleware::auth::{mint_session_claims, Claims};
use crate::services::agent::run_hub::AgentRun;
use crate::services::agent::{AgentInteractionMode, AgentProgressEvent};
use crate::services::tapp_registry::{self as shared_registry, RegistryIdentity};

const MSG_NAMESPACE: &str = "telegram_dm_update";
const SESSION_NAMESPACE: &str = "telegram_dm_session";
const PENDING_NAMESPACE: &str = "telegram_dm_pending";
const BINDING_TTL_SECS: i64 = 30 * 24 * 60 * 60;
/// Refresh before Telegram's ~5s typing window expires.
const TYPING_REFRESH: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSession {
    session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPending {
    prompt: PendingPrompt,
}

pub async fn start_paired_work(
    db: &DatabaseConnection,
    user_id: i32,
    chat_id: &str,
    input: &str,
    session_key: &str,
    update_id: &str,
    token: &str,
) {
    if !claim_inbound(db, user_id, update_id, "Telegram DM duplicate update_id ignored").await {
        return;
    }
    continue_paired_input(db, user_id, chat_id, input, session_key, token).await;
}

pub async fn start_paired_callback(
    db: &DatabaseConnection,
    user_id: i32,
    chat_id: &str,
    data: &str,
    session_key: &str,
    update_id: &str,
    token: &str,
) {
    if !claim_inbound(db, user_id, update_id, "Telegram DM duplicate callback ignored").await {
        return;
    }
    let Some(pending) = load_pending(db, session_key).await else {
        return;
    };
    match telegram_callback_action(&pending.prompt, data) {
        TelegramCallbackAction::RequestInput => {
            send_force_reply(token, chat_id, &pending.prompt.question).await;
        }
        TelegramCallbackAction::Resume(answer) => {
            continue_paired_input(db, user_id, chat_id, &answer, session_key, token).await;
        }
        TelegramCallbackAction::Unknown => {
            send_prompt(
                token,
                chat_id,
                &format_pending_prompt(&pending.prompt),
                &pending.prompt,
            )
            .await;
        }
    }
}

async fn claim_inbound(
    db: &DatabaseConnection,
    user_id: i32,
    update_id: &str,
    duplicate_message: &str,
) -> bool {
    match shared_registry::put_if_absent(
        db,
        MSG_NAMESPACE,
        update_id,
        RegistryIdentity {
            subject_id: Some(user_id),
            owner_id: Some(user_id),
            tapp_id: None,
            runtime_id: None,
        },
        &Value::Bool(true),
        (Utc::now() + ChronoDuration::hours(24)).timestamp(),
    )
    .await
    {
        Ok(true) => true,
        Ok(false) => {
            info!(update_id, detail = duplicate_message, "Telegram DM duplicate inbound");
            false
        }
        Err(error) => {
            warn!(%error, "Telegram DM duplicate check failed");
            false
        }
    }
}

async fn continue_paired_input(
    db: &DatabaseConnection,
    user_id: i32,
    chat_id: &str,
    input: &str,
    session_key: &str,
    token: &str,
) {
    let claims = match claims_for_user(db, user_id).await {
        Ok(value) => value,
        Err(error) => {
            warn!(%error, user_id, "Telegram DM claims lookup failed");
            return;
        }
    };
    let session_id = match bind_session(db, user_id, session_key).await {
        Ok(value) => value,
        Err(error) => {
            warn!(%error, "Telegram DM session bind failed");
            String::new()
        }
    };

    if let Some(pending) = load_pending(db, session_key).await {
        match decide_pending_reply(&pending.prompt, input, Utc::now().timestamp()) {
            PendingDecision::Reask { reply } => {
                send_prompt(token, chat_id, &reply, &pending.prompt).await;
                return;
            }
            PendingDecision::Expired { reply } => {
                clear_pending(db, session_key).await;
                send_text(token, chat_id, &reply).await;
                return;
            }
            PendingDecision::Resume {
                kind,
                answer,
                confirmed,
            } => {
                clear_pending(db, session_key).await;
                resume_pending(
                    db.clone(),
                    claims,
                    session_id,
                    session_key,
                    kind,
                    answer,
                    confirmed,
                    token,
                    chat_id,
                    input,
                )
                .await;
                return;
            }
        }
    }

    start_new_work(
        db.clone(),
        claims,
        session_id,
        input,
        token,
        chat_id,
        session_key,
    )
    .await;
}

async fn start_new_work(
    db: DatabaseConnection,
    claims: Claims,
    session_id: String,
    input: &str,
    token: &str,
    chat_id: &str,
    session_key: &str,
) {
    send_typing(token, chat_id).await;
    let run = match crate::api::agent::start_process_run(
        db.clone(),
        claims,
        ProcessRequest {
            input: input.to_string(),
            context: Some(ProcessContext {
                mode: Some(AgentInteractionMode::Work),
                session_id: (!session_id.is_empty()).then_some(session_id),
                current_route: None,
                active_platforms: None,
                conversation_history: None,
                custom_data: None,
                intention_id: None,
                autonomy_permission_cap: None,
                rig_state: None,
            }),
        },
    )
    .await
    {
        Ok(run) => run,
        Err(error) => {
            let body = error.0.to_json();
            let message = body
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| body.get("error").and_then(Value::as_str))
                .unwrap_or("办事没能开始。")
                .to_string();
            warn!(error = %message, "Telegram DM Work start failed");
            send_text(token, chat_id, &message).await;
            return;
        }
    };

    tokio::spawn(deliver_run(
        run,
        db,
        session_key.to_string(),
        token.to_string(),
        chat_id.to_string(),
        input.to_string(),
    ));
}

async fn resume_pending(
    db: DatabaseConnection,
    claims: Claims,
    session_id: String,
    session_key: &str,
    kind: PendingKind,
    answer: String,
    confirmed: Option<bool>,
    token: &str,
    chat_id: &str,
    latest_input: &str,
) {
    send_typing(token, chat_id).await;
    let sid = (!session_id.is_empty()).then_some(session_id);
    let mut next_original = latest_input.to_string();
    let run = match kind {
        PendingKind::Clarify { original_input } => {
            let combined = format!("{original_input}\n补充说明：{answer}");
            next_original = combined.clone();
            crate::api::agent::start_process_run(
                db.clone(),
                claims,
                ProcessRequest {
                    input: combined.clone(),
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
                    }),
                },
            )
            .await
            .map_err(|error| error.0.to_json())
        }
        PendingKind::Confirm { confirmation_id } => crate::api::agent::start_confirm_run(
            db.clone(),
            claims,
            confirmation_id,
            confirmed.unwrap_or(false),
            None,
        )
        .await
        .map_err(|error| error.0.to_json()),
        PendingKind::Answer {
            task_id,
            question_id,
            ..
        } => crate::api::agent::start_answer_run(
            db.clone(),
            claims,
            task_id,
            question_id,
            answer,
            sid,
        )
        .await
        .map_err(|error| error.0.to_json()),
    };

    match run {
        Ok(run) => {
            tokio::spawn(deliver_run(
                run,
                db,
                session_key.to_string(),
                token.to_string(),
                chat_id.to_string(),
                next_original,
            ));
        }
        Err(body) => {
            let message = body
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| body.get("error").and_then(Value::as_str))
                .unwrap_or("这一步没能继续。")
                .to_string();
            warn!(error = %message, "Telegram DM resume failed");
            send_text(token, chat_id, &message).await;
        }
    }
}

async fn claims_for_user(db: &DatabaseConnection, user_id: i32) -> Result<Claims, DbErr> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT username, COALESCE(is_admin, false) AS is_admin, \
                    COALESCE(is_owner, false) AS is_owner, \
                    COALESCE(token_version, 0) AS token_version \
             FROM users WHERE id = $1 LIMIT 1",
            vec![SeaValue::Int(Some(user_id))],
        ))
        .await?
        .ok_or_else(|| DbErr::RecordNotFound("paired user missing".into()))?;
    let username: String = row.try_get("", "username").unwrap_or_default();
    let is_admin: bool = row.try_get("", "is_admin").unwrap_or(false);
    let is_owner: bool = row.try_get("", "is_owner").unwrap_or(false);
    let token_version: i64 = row
        .try_get::<i32>("", "token_version")
        .ok()
        .map(i64::from)
        .or_else(|| row.try_get::<i64>("", "token_version").ok())
        .unwrap_or(0);
    Ok(mint_session_claims(
        user_id,
        username,
        is_admin,
        is_owner,
        token_version,
    ))
}

async fn bind_session(
    db: &DatabaseConnection,
    user_id: i32,
    session_key: &str,
) -> Result<String, DbErr> {
    if let Some(stored) =
        shared_registry::get::<StoredSession>(db, SESSION_NAMESPACE, session_key).await?
    {
        if !stored.session_id.is_empty() {
            return Ok(stored.session_id);
        }
    }
    let session_id =
        crate::api::agent::ensure_session(db, None, user_id, AgentInteractionMode::Work)
            .await
            .map_err(DbErr::Custom)?;
    shared_registry::put(
        db,
        SESSION_NAMESPACE,
        session_key,
        RegistryIdentity {
            subject_id: Some(user_id),
            owner_id: Some(user_id),
            tapp_id: None,
            runtime_id: None,
        },
        &StoredSession {
            session_id: session_id.clone(),
        },
        (Utc::now() + ChronoDuration::seconds(BINDING_TTL_SECS)).timestamp(),
    )
    .await?;
    Ok(session_id)
}

async fn load_pending(db: &DatabaseConnection, session_key: &str) -> Option<StoredPending> {
    if session_key.is_empty() {
        return None;
    }
    match shared_registry::get::<StoredPending>(db, PENDING_NAMESPACE, session_key).await {
        Ok(value) => value,
        Err(error) => {
            warn!(%error, "Telegram DM pending load failed");
            None
        }
    }
}

async fn save_pending(db: &DatabaseConnection, session_key: &str, prompt: PendingPrompt) {
    if session_key.is_empty() {
        return;
    }
    if let Err(error) = shared_registry::put(
        db,
        PENDING_NAMESPACE,
        session_key,
        RegistryIdentity {
            subject_id: None,
            owner_id: None,
            tapp_id: None,
            runtime_id: None,
        },
        &StoredPending { prompt },
        (Utc::now() + ChronoDuration::days(2)).timestamp(),
    )
    .await
    {
        warn!(%error, "Telegram DM pending save failed");
    }
}

async fn clear_pending(db: &DatabaseConnection, session_key: &str) {
    if session_key.is_empty() {
        return;
    }
    if let Err(error) =
        shared_registry::take::<StoredPending>(db, PENDING_NAMESPACE, session_key).await
    {
        warn!(%error, "Telegram DM pending clear failed");
    }
}

fn delivery_context() -> DeliveryContext {
    DeliveryContext {
        inbound_msg_id: None,
        passive_window_open: false,
        remaining_passive_replies: 0,
    }
}

async fn deliver_run(
    run: Arc<AgentRun>,
    db: DatabaseConnection,
    session_key: String,
    token: String,
    chat_id: String,
    original_input: String,
) {
    send_typing(&token, &chat_id).await;
    let mut envelopes = Box::pin(crate::api::agent::agent_run_envelopes(run));
    let mut refresh = tokio::time::interval(TYPING_REFRESH);
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    refresh.tick().await;
    loop {
        tokio::select! {
            envelope = futures::StreamExt::next(&mut envelopes) => {
                let Some(envelope) = envelope else {
                    return;
                };
                if let Some((event, parked)) = map_progress(&envelope.event, &original_input) {
                    if let Some(prompt) = parked.as_ref() {
                        save_pending(&db, &session_key, prompt.clone()).await;
                    } else {
                        clear_pending(&db, &session_key).await;
                    }
                    match plan_delivery(&event, &delivery_context()) {
                        DeliveryPlan::Drop => {}
                        DeliveryPlan::ActiveText { content }
                        | DeliveryPlan::FailVisible { content, .. }
                        | DeliveryPlan::PassiveText { content, .. } => {
                            if let Some(prompt) = parked {
                                send_prompt(&token, &chat_id, &content, &prompt).await;
                            } else {
                                send_text(&token, &chat_id, &content).await;
                            }
                            return;
                        }
                    }
                }
            }
            _ = refresh.tick() => {
                send_typing(&token, &chat_id).await;
            }
        }
    }
}

fn map_progress(
    event: &AgentProgressEvent,
    original_input: &str,
) -> Option<(ChannelEvent, Option<PendingPrompt>)> {
    match event {
        AgentProgressEvent::WaitingForInput {
            task_id,
            question_id,
            question_type,
            question,
            options,
            ..
        } => {
            let prompt = PendingPrompt {
                kind: PendingKind::Answer {
                    task_id: task_id.clone(),
                    question_id: question_id.clone(),
                    question_type: question_type.clone(),
                },
                question: question.clone(),
                options: options
                    .as_ref()
                    .map(|rows| {
                        rows.iter()
                            .map(|row| PendingOption {
                                value: row.value.clone(),
                                label: if row.label.trim().is_empty() {
                                    row.value.clone()
                                } else {
                                    row.label.clone()
                                },
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                expires_at_unix: None,
            };
            Some((
                ChannelEvent::Answer {
                    message: format_pending_prompt(&prompt),
                },
                Some(prompt),
            ))
        }
        AgentProgressEvent::Error { message, .. } => Some((
            ChannelEvent::Error {
                message: message.clone(),
            },
            None,
        )),
        AgentProgressEvent::TaskCompleted { response, .. } => {
            Some(map_completed(response, original_input))
        }
        AgentProgressEvent::ThinkingToken { .. }
        | AgentProgressEvent::StepStarted { .. }
        | AgentProgressEvent::StepCompleted { .. }
        | AgentProgressEvent::Progress { .. }
        | AgentProgressEvent::StepRetrying { .. }
        | AgentProgressEvent::RunStarted { .. }
        | AgentProgressEvent::SessionCreated { .. }
        | AgentProgressEvent::SessionTitleUpdated { .. }
        | AgentProgressEvent::SummaryToken { .. }
        | AgentProgressEvent::TaskCreated { .. }
        | AgentProgressEvent::TaskAssigned { .. }
        | AgentProgressEvent::PlannerDecision { .. }
        | AgentProgressEvent::StepDebug { .. }
        | AgentProgressEvent::PerformancePlan { .. }
        | AgentProgressEvent::MeropeStateChanged { .. }
        | AgentProgressEvent::OutfitOverlay { .. }
        | AgentProgressEvent::MusicControl { .. } => None,
    }
}

fn map_completed(response: &Value, original_input: &str) -> (ChannelEvent, Option<PendingPrompt>) {
    if response.get("frontendAction").is_some() {
        return (ChannelEvent::FrontendAction, None);
    }
    let response_type = response
        .get("responseType")
        .and_then(Value::as_str)
        .unwrap_or("");
    if response_type == "confirmation_required" || response.get("confirmation").is_some() {
        if let Some(prompt) = confirmation_prompt(response) {
            return (
                ChannelEvent::Answer {
                    message: format_pending_prompt(&prompt),
                },
                Some(prompt),
            );
        }
        return (ChannelEvent::ConfirmationRequired, None);
    }
    if response_type == "clarification" {
        let prompt = clarification_prompt(response, original_input);
        return (
            ChannelEvent::Answer {
                message: format_pending_prompt(&prompt),
            },
            Some(prompt),
        );
    }
    let message = response
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if let Some(prompt) = pending_prompt_from_model_json(message, original_input) {
        return (
            ChannelEvent::Answer {
                message: format_pending_prompt(&prompt),
            },
            Some(prompt),
        );
    }
    if response.get("success").and_then(Value::as_bool) == Some(false) || response_type == "error" {
        return (
            ChannelEvent::Error {
                message: if message.is_empty() {
                    "办事失败。".into()
                } else {
                    message.to_string()
                },
            },
            None,
        );
    }
    (
        ChannelEvent::Answer {
            message: if message.is_empty() {
                PANEL_REQUIRED_REPLY.to_string()
            } else {
                message.to_string()
            },
        },
        None,
    )
}

fn confirmation_prompt(response: &Value) -> Option<PendingPrompt> {
    let confirmation = response.get("confirmation")?;
    let confirmation_id = confirmation
        .get("confirmationId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())?;
    let question = response
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let step = confirmation
        .get("pendingSteps")
        .and_then(Value::as_array)
        .and_then(|steps| steps.first())
        .and_then(|step| step.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let question = if question.is_empty() {
        step.to_string()
    } else if step.is_empty() || question.contains(step) {
        question.to_string()
    } else {
        format!("{question}\n{step}")
    };
    let expires_in = confirmation
        .get("expiresInSeconds")
        .and_then(Value::as_i64)
        .filter(|secs| *secs > 0);
    Some(PendingPrompt {
        kind: PendingKind::Confirm {
            confirmation_id: confirmation_id.to_string(),
        },
        question,
        options: Vec::new(),
        expires_at_unix: expires_in.map(|secs| Utc::now().timestamp().saturating_add(secs)),
    })
}

fn clarification_prompt(response: &Value, original_input: &str) -> PendingPrompt {
    let question = response
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let mut options = json_string_options(response.get("suggestions"));
    if options.is_empty() {
        options = json_string_options(
            response
                .get("clarification")
                .and_then(|value| value.get("options")),
        );
    }
    let prompt = PendingPrompt {
        kind: PendingKind::Clarify {
            original_input: original_input.to_string(),
        },
        question: question.clone(),
        options,
        expires_at_unix: None,
    };
    if prompt.options.is_empty() {
        if let Some(from_json) = pending_prompt_from_model_json(&question, original_input) {
            return from_json;
        }
    }
    prompt
}

fn json_string_options(value: Option<&Value>) -> Vec<PendingOption> {
    value
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let text = row.as_str().or_else(|| {
                        row.get("label")
                            .and_then(Value::as_str)
                            .or_else(|| row.get("value").and_then(Value::as_str))
                    })?;
                    let trimmed = text.trim();
                    (!trimmed.is_empty()).then(|| PendingOption {
                        value: trimmed.to_string(),
                        label: trimmed.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn send_text(token: &str, chat_id: &str, content: &str) {
    send_outbound(token, chat_id, content, None).await;
}

async fn send_prompt(token: &str, chat_id: &str, content: &str, prompt: &PendingPrompt) {
    send_outbound(token, chat_id, content, telegram_reply_markup(prompt)).await;
}

async fn send_force_reply(token: &str, chat_id: &str, placeholder: &str) {
    send_outbound(
        token,
        chat_id,
        "请在这里输入。",
        Some(telegram_force_reply_markup(placeholder)),
    )
    .await;
}

async fn send_outbound(
    token: &str,
    chat_id: &str,
    content: &str,
    reply_markup: Option<Value>,
) {
    if let Err(error) =
        crate::services::telegram_bot::send_outbound(token, chat_id, content, reply_markup).await
    {
        warn!(?error, "Telegram DM send failed");
    }
}

async fn send_typing(token: &str, chat_id: &str) {
    if let Err(error) = crate::services::telegram_bot::send_typing(token, chat_id).await {
        warn!(?error, "Telegram DM typing failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_answer_uses_message() {
        let (event, parked) = map_completed(
            &serde_json::json!({
                "success": true,
                "responseType": "answer",
                "message": "票已订好"
            }),
            "订票",
        );
        assert_eq!(
            event,
            ChannelEvent::Answer {
                message: "票已订好".into()
            }
        );
        assert!(parked.is_none());
    }

    #[test]
    fn model_json_answer_parks_numbered_options() {
        let (event, parked) = map_completed(
            &serde_json::json!({
                "success": true,
                "responseType": "answer",
                "message": "{\n  \"intent\": \"clarification\",\n  \"message\": \"【步骤 1 · clarification】\\n测试用，选一个城市\",\n  \"clarifications_needed\": [\n    { \"question\": \"测试用，选一个城市\", \"suggestions\": [\"东京\", \"京都\"] }\n  ]\n}"
            }),
            "测试脚本",
        );
        let ChannelEvent::Answer { message } = event else {
            panic!("{event:?}");
        };
        assert!(message.contains("1. 东京"));
        assert!(message.contains("2. 京都"));
        assert!(!message.contains("clarifications_needed"));
        let prompt = parked.expect("json parks");
        assert!(matches!(prompt.kind, PendingKind::Clarify { .. }));
        assert_eq!(prompt.options.len(), 2);
    }

    #[test]
    fn clarification_stays_in_chat_with_numbered_options() {
        let (event, parked) = map_completed(
            &serde_json::json!({
                "success": true,
                "responseType": "clarification",
                "message": "去哪一站？",
                "suggestions": ["上海", "杭州"]
            }),
            "订票",
        );
        let ChannelEvent::Answer { message } = event else {
            panic!("{event:?}");
        };
        assert!(message.contains("1. 上海"));
        assert!(message.contains("2. 杭州"));
        let prompt = parked.expect("clarification parks");
        assert!(matches!(prompt.kind, PendingKind::Clarify { .. }));
    }

    #[test]
    fn confirmation_asks_yes_or_no_instead_of_sending_to_panel() {
        let (event, parked) = map_completed(
            &serde_json::json!({
                "success": true,
                "responseType": "confirmation_required",
                "message": "要删掉这篇文章吗？",
                "confirmation": {
                    "confirmationId": "c1",
                    "expiresInSeconds": 60
                }
            }),
            "删除文章",
        );
        let ChannelEvent::Answer { message } = event else {
            panic!("{event:?}");
        };
        assert!(message.contains("要删掉这篇文章吗？"));
        assert!(message.contains("是"));
        let prompt = parked.expect("confirmation parks");
        assert!(matches!(
            prompt.kind,
            PendingKind::Confirm { confirmation_id } if confirmation_id == "c1"
        ));
    }

    #[test]
    fn frontend_action_is_still_a_visible_failure() {
        let (event, parked) = map_completed(
            &serde_json::json!({
                "success": true,
                "responseType": "answer",
                "message": "打开页面",
                "frontendAction": {"type": "navigate"}
            }),
            "打开",
        );
        assert_eq!(event, ChannelEvent::FrontendAction);
        assert!(parked.is_none());
    }

    #[test]
    fn waiting_for_input_parks_free_text() {
        let event = AgentProgressEvent::WaitingForInput {
            task_id: "t1".into(),
            question_id: "q1".into(),
            question_type: "free_text".into(),
            question: "标题写什么？".into(),
            context: None,
            options: None,
            required: true,
            default_value: None,
        };
        let (mapped, parked) = map_progress(&event, "写文").expect("mapped");
        let ChannelEvent::Answer { message } = mapped else {
            panic!("{mapped:?}");
        };
        assert!(message.contains("标题写什么？"));
        let prompt = parked.expect("parked");
        assert!(matches!(
            prompt.kind,
            PendingKind::Answer { question_type, .. } if question_type == "free_text"
        ));
    }

    #[test]
    fn telegram_delivery_is_always_active() {
        let plan = plan_delivery(
            &ChannelEvent::Answer {
                message: "做好了".into(),
            },
            &delivery_context(),
        );
        assert_eq!(
            plan,
            DeliveryPlan::ActiveText {
                content: "做好了".into(),
            }
        );
    }
}
