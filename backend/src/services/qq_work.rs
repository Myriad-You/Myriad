//! QQ C2C adapter: pairing already resolved → shared private-chat Work.

use chrono::{Duration as ChronoDuration, Utc};
use myriad_agent_rules::channel::next_passive_seq;
use myriad_error::redact_secrets;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::warn;

use crate::services::channel_work::{self, ChannelSink};
use crate::services::http_client;
use crate::services::tapp_registry::{self as shared_registry, RegistryIdentity};
use crate::GLOBAL_DYNAMIC_CONFIG;

const API_BASE: &str = "https://api.bot.qq.com";
const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const SEQ_NAMESPACE: &str = "qq_c2c_seq";

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
    channel_work::handle_text(
        db,
        user_id,
        openid,
        input,
        session_key,
        msg_id,
        ChannelSink::Qq {
            db: db.clone(),
            auth_header: auth_header.to_string(),
            openid: openid.to_string(),
            inbound_msg_id: (!msg_id.is_empty()).then(|| msg_id.to_string()),
        },
    )
    .await;
}

pub async fn send_c2c(
    db: &DatabaseConnection,
    auth_header: &str,
    openid: &str,
    content: &str,
    inbound_msg_id: Option<&str>,
) -> Result<(), String> {
    if content.is_empty() || openid.is_empty() {
        return Ok(());
    }
    let enabled = {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        config.qq_bot_enabled
    };
    if !enabled {
        return Ok(());
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
        Ok(resp) if resp.status().is_success() => Ok(()),
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if inbound_msg_id.is_some() && should_fallback_active(status.as_u16(), &text) {
                warn!(
                    status = status.as_u16(),
                    "QQ C2C passive send failed; falling back to active"
                );
                return Box::pin(send_c2c(db, auth_header, openid, content, None)).await;
            }
            warn!(
                status = status.as_u16(),
                body = %redact_secrets(&text),
                "QQ C2C send failed"
            );
            Err(format!("qq send {status}"))
        }
        Err(error) => {
            warn!(
                error = %redact_secrets(&error.to_string()),
                "QQ C2C send request failed"
            );
            Err(redact_secrets(&error.to_string()))
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
