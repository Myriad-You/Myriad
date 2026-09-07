//! Telegram DM Work ingest: paired text → start_process_run → final sendMessage.
//!
//! Mode is locked to Work. Process events are dropped. Confirmation and
//! frontendAction fail visibly. There is no passive window.

use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use myriad_agent_rules::channel::{
    plan_delivery, ChannelEvent, DeliveryContext, DeliveryPlan, PANEL_REQUIRED_REPLY,
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
const BINDING_TTL_SECS: i64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSession {
    session_id: String,
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
        Ok(true) => {}
        Ok(false) => {
            info!(update_id, "Telegram DM duplicate update_id ignored");
            return;
        }
        Err(error) => {
            warn!(%error, "Telegram DM duplicate check failed");
            return;
        }
    }

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
            warn!(user_id, error = %message, "Telegram DM Work start failed");
            send_text(token, chat_id, &message).await;
            return;
        }
    };

    tokio::spawn(deliver_run(run, token.to_string(), chat_id.to_string()));
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

fn delivery_context() -> DeliveryContext {
    DeliveryContext {
        inbound_msg_id: None,
        passive_window_open: false,
        remaining_passive_replies: 0,
    }
}

async fn deliver_run(run: Arc<AgentRun>, token: String, chat_id: String) {
    let mut envelopes = Box::pin(crate::api::agent::agent_run_envelopes(run));
    while let Some(envelope) = futures::StreamExt::next(&mut envelopes).await {
        if let Some(event) = map_progress(&envelope.event) {
            match plan_delivery(&event, &delivery_context()) {
                DeliveryPlan::Drop => {}
                DeliveryPlan::ActiveText { content }
                | DeliveryPlan::FailVisible { content, .. }
                | DeliveryPlan::PassiveText { content, .. } => {
                    send_text(&token, &chat_id, &content).await;
                    return;
                }
            }
        }
    }
}

fn map_progress(event: &AgentProgressEvent) -> Option<ChannelEvent> {
    match event {
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
        AgentProgressEvent::WaitingForInput { .. } => Some(ChannelEvent::ConfirmationRequired),
        AgentProgressEvent::Error { message, .. } => Some(ChannelEvent::Error {
            message: message.clone(),
        }),
        AgentProgressEvent::TaskCompleted { response, .. } => Some(map_completed(response)),
    }
}

fn map_completed(response: &Value) -> ChannelEvent {
    if response.get("frontendAction").is_some() {
        return ChannelEvent::FrontendAction;
    }
    let response_type = response
        .get("responseType")
        .and_then(Value::as_str)
        .unwrap_or("");
    if response_type == "confirmation_required" || response.get("confirmation").is_some() {
        return ChannelEvent::ConfirmationRequired;
    }
    let message = response
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if response.get("success").and_then(Value::as_bool) == Some(false) || response_type == "error" {
        return ChannelEvent::Error {
            message: if message.is_empty() {
                "办事失败。".into()
            } else {
                message.to_string()
            },
        };
    }
    ChannelEvent::Answer {
        message: if message.is_empty() {
            PANEL_REQUIRED_REPLY.to_string()
        } else {
            message.to_string()
        },
    }
}

async fn send_text(token: &str, chat_id: &str, content: &str) {
    if let Err(error) = crate::services::telegram_bot::send_message(token, chat_id, content).await {
        warn!(?error, "Telegram DM send failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_answer_uses_message() {
        let event = map_completed(&serde_json::json!({
            "success": true,
            "responseType": "answer",
            "message": "票已订好"
        }));
        assert_eq!(
            event,
            ChannelEvent::Answer {
                message: "票已订好".into()
            }
        );
    }

    #[test]
    fn confirmation_and_frontend_action_are_visible_failures() {
        assert_eq!(
            map_completed(&serde_json::json!({
                "success": true,
                "responseType": "confirmation_required",
                "message": "请确认",
                "confirmation": {"id": "c1"}
            })),
            ChannelEvent::ConfirmationRequired
        );
        assert_eq!(
            map_completed(&serde_json::json!({
                "success": true,
                "responseType": "answer",
                "message": "打开页面",
                "frontendAction": {"type": "navigate"}
            })),
            ChannelEvent::FrontendAction
        );
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
