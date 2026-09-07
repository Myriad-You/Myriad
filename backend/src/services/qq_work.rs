//! QQ C2C Work ingest: paired text → start_process_run → final C2C reply.
//!
//! Mode is locked to Work. Thinking/step events are dropped. Confirmation and
//! frontendAction fail visibly. Passive window first; otherwise active send.

use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use myriad_agent_rules::channel::{
    next_passive_seq, plan_delivery, ChannelEvent, DeliveryContext, DeliveryPlan,
    PANEL_REQUIRED_REPLY,
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
use crate::services::http_client;
use crate::services::tapp_registry::{self as shared_registry, RegistryIdentity};
use crate::GLOBAL_DYNAMIC_CONFIG;
use myriad_error::redact_secrets;

const API_BASE: &str = "https://api.bot.qq.com";
const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const MSG_NAMESPACE: &str = "qq_c2c_msg";
const SESSION_NAMESPACE: &str = "qq_c2c_session";
const SEQ_NAMESPACE: &str = "qq_c2c_seq";
const BINDING_TTL_SECS: i64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSession {
    session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSeq {
    seq: u32,
}

pub async fn start_paired_work(
    db: &DatabaseConnection,
    user_id: i32,
    openid: &str,
    input: &str,
    session_key: &str,
    msg_id: &str,
    auth_header: &str,
) {
    match shared_registry::put_if_absent(
        db,
        MSG_NAMESPACE,
        msg_id,
        RegistryIdentity {
            subject_id: Some(user_id),
            owner_id: Some(user_id),
            tapp_id: None,
            runtime_id: None,
        },
        &json_unit(),
        (Utc::now() + ChronoDuration::hours(24)).timestamp(),
    )
    .await
    {
        Ok(true) => {}
        Ok(false) => {
            info!(msg_id, "QQ C2C duplicate msg_id ignored");
            return;
        }
        Err(error) => {
            warn!(%error, "QQ C2C duplicate check failed");
            return;
        }
    }

    let claims = match claims_for_user(db, user_id).await {
        Ok(value) => value,
        Err(error) => {
            warn!(%error, user_id, "QQ C2C claims lookup failed");
            return;
        }
    };
    let session_id = match bind_session(db, user_id, session_key).await {
        Ok(value) => value,
        Err(error) => {
            warn!(%error, "QQ C2C session bind failed");
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
            warn!(user_id, error = %message, "QQ C2C Work start failed");
            deliver_text(db, auth_header, openid, msg_id, &message).await;
            return;
        }
    };

    tokio::spawn(deliver_run(
        db.clone(),
        run,
        auth_header.to_string(),
        openid.to_string(),
        msg_id.to_string(),
    ));
}

fn json_unit() -> Value {
    Value::Bool(true)
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
            .map_err(|error| DbErr::Custom(error))?;
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

async fn deliver_run(
    db: DatabaseConnection,
    run: Arc<AgentRun>,
    auth_header: String,
    openid: String,
    msg_id: String,
) {
    let mut envelopes = Box::pin(crate::api::agent::agent_run_envelopes(run));
    while let Some(envelope) = futures::StreamExt::next(&mut envelopes).await {
        if let Some(event) = map_progress(&envelope.event) {
            let ctx = DeliveryContext {
                inbound_msg_id: Some(msg_id.clone()),
                passive_window_open: true,
                remaining_passive_replies: 4,
            };
            match plan_delivery(&event, &ctx) {
                DeliveryPlan::Drop => {}
                DeliveryPlan::PassiveText { content, msg_id } => {
                    send_c2c(&db, &auth_header, &openid, &content, Some(&msg_id)).await;
                    return;
                }
                DeliveryPlan::ActiveText { content }
                | DeliveryPlan::FailVisible {
                    content,
                    passive: false,
                    ..
                } => {
                    send_c2c(&db, &auth_header, &openid, &content, None).await;
                    return;
                }
                DeliveryPlan::FailVisible {
                    content,
                    msg_id: Some(id),
                    passive: true,
                } => {
                    send_c2c(&db, &auth_header, &openid, &content, Some(&id)).await;
                    return;
                }
                DeliveryPlan::FailVisible { content, .. } => {
                    send_c2c(&db, &auth_header, &openid, &content, None).await;
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

async fn deliver_text(
    db: &DatabaseConnection,
    auth_header: &str,
    openid: &str,
    msg_id: &str,
    content: &str,
) {
    send_c2c(db, auth_header, openid, content, Some(msg_id)).await;
}

async fn send_c2c(
    db: &DatabaseConnection,
    auth_header: &str,
    openid: &str,
    content: &str,
    inbound_msg_id: Option<&str>,
) {
    if content.is_empty() || openid.is_empty() {
        return;
    }
    let enabled = {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        config.qq_bot_enabled
    };
    if !enabled {
        return;
    }
    let mut body = serde_json::json!({
        "content": content,
        "msg_type": 0,
    });
    if let Some(msg_id) = inbound_msg_id.filter(|id| !id.is_empty()) {
        let seq = next_seq(db, msg_id).await;
        body["msg_id"] = Value::String(msg_id.to_string());
        body["msg_seq"] = Value::from(seq);
    }
    let client = http_client::get_global_client().await;
    let url = format!("{API_BASE}/v2/users/{openid}/messages");
    match client
        .post(&url)
        .timeout(HTTP_TIMEOUT)
        .header("Authorization", auth_header)
        .json(&body)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {}
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if inbound_msg_id.is_some() && should_fallback_active(status.as_u16(), &text) {
                warn!(
                    status = status.as_u16(),
                    "QQ C2C passive send failed; falling back to active"
                );
                Box::pin(send_c2c(db, auth_header, openid, content, None)).await;
                return;
            }
            warn!(
                status = status.as_u16(),
                body = %redact_secrets(&text),
                "QQ C2C send failed"
            );
        }
        Err(error) => {
            warn!(
                error = %redact_secrets(&error.to_string()),
                "QQ C2C send request failed"
            );
        }
    }
}

fn should_fallback_active(status: u16, body: &str) -> bool {
    status >= 400
        && (body.contains("304023") || (body.contains("msg_id") && body.contains("invalid")))
}

async fn next_seq(db: &DatabaseConnection, msg_id: &str) -> u32 {
    let last = shared_registry::get::<StoredSeq>(db, SEQ_NAMESPACE, msg_id)
        .await
        .ok()
        .flatten()
        .map(|stored| stored.seq);
    let seq = next_passive_seq(last);
    let _ = shared_registry::put(
        db,
        SEQ_NAMESPACE,
        msg_id,
        RegistryIdentity {
            subject_id: None,
            owner_id: None,
            tapp_id: None,
            runtime_id: None,
        },
        &StoredSeq { seq },
        (Utc::now() + ChronoDuration::hours(2)).timestamp(),
    )
    .await;
    seq
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
    fn process_events_are_not_mapped() {
        assert!(map_progress(&AgentProgressEvent::ThinkingToken {
            token: "…".into(),
            done: false
        })
        .is_none());
        assert!(map_progress(&AgentProgressEvent::Progress {
            progress: 10,
            completed_steps: 1,
            total_steps: 3,
            message: "进行中".into()
        })
        .is_none());
    }
}
