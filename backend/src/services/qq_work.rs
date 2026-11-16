//! QQ C2C adapter: pairing already resolved → shared private-chat Work.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::Duration as ChronoDuration;
use myriad_agent_rules::channel::{
    ConnectFailure, ConnectFailureKind, classify_connect_failure, next_passive_seq,
    parse_qq_file_info,
};
use myriad_error::redact_secrets;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::warn;

use crate::GLOBAL_DYNAMIC_CONFIG;
use crate::services::channel_work::{self, ChannelTransport};
use crate::services::http_client;
use crate::services::runtime_registry::{self as shared_registry, RegistryIdentity};

const API_BASE: &str = "https://api.bot.qq.com";
const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const SEQ_NAMESPACE: &str = "qq_c2c_seq";
const GROUP_SEQ_NAMESPACE: &str = "qq_group_seq";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSeq {
    seq: u32,
}

pub async fn start_paired_work_with_images(
    db: &DatabaseConnection,
    user_id: i32,
    sender_id: &str,
    openid: &str,
    input: &str,
    images: &[myriad_agent_rules::channel::ChannelImageRef],
    session_key: &str,
    msg_id: &str,
) {
    channel_work::handle_text_with_images(
        db,
        user_id,
        sender_id,
        openid,
        input,
        images,
        session_key,
        ChannelTransport::Qq {
            db: db.clone(),
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
) -> Result<(), ConnectFailureKind> {
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
            Err(classify_connect_failure(&ConnectFailure::HttpStatus {
                status: status.as_u16(),
                body: &text,
            }))
        }
        Err(error) => {
            warn!(
                error = %redact_secrets(&error.to_string()),
                "QQ C2C send request failed"
            );
            Err(ConnectFailureKind::Transient)
        }
    }
}

/// Send plain text to an allowlisted group. Turning groups off or taking the
/// group off the allowlist stops delivery. The group's passive reply window
/// is short, so a line always carries the inbound `msg_id` when there is one;
/// without one QQ treats it as an active message and the daily quota applies.
/// There is no active fallback after a rejected passive send: the group mind
/// does not retry a failed line.
pub async fn send_group(
    db: &DatabaseConnection,
    auth_header: &str,
    group_openid: &str,
    content: &str,
    inbound_msg_id: Option<&str>,
) -> Result<(), ConnectFailureKind> {
    if content.is_empty() || group_openid.is_empty() {
        return Ok(());
    }
    let allowed = {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        config.qq_bot_enabled
            && config.qq_bot_groups_enabled
            && crate::services::qq_bot::group_allowed(&config.qq_bot_group_ids, group_openid)
    };
    if !allowed {
        return Ok(());
    }
    let msg_id = inbound_msg_id.filter(|id| !id.is_empty());
    let seq = match msg_id {
        Some(msg_id) => Some(next_seq_in(db, GROUP_SEQ_NAMESPACE, msg_id).await),
        None => None,
    };
    let body = group_body(content, msg_id, seq);
    let client = http_client::get_global_client().await;
    let url = group_messages_url(group_openid);
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
            warn!(
                status = status.as_u16(),
                body = %redact_secrets(&text),
                "QQ group send failed"
            );
            Err(classify_connect_failure(&ConnectFailure::HttpStatus {
                status: status.as_u16(),
                body: &text,
            }))
        }
        Err(error) => {
            warn!(
                error = %redact_secrets(&error.to_string()),
                "QQ group send request failed"
            );
            Err(ConnectFailureKind::Transient)
        }
    }
}

fn group_messages_url(group_openid: &str) -> String {
    // Allowlisted ids are ASCII letters, digits, `_` and `-` (see
    // `qq_bot::normalize_group_ids`), so no path escaping is needed.
    format!("{API_BASE}/v2/groups/{group_openid}/messages")
}

fn group_body(content: &str, msg_id: Option<&str>, seq: Option<u32>) -> Value {
    let mut body = serde_json::json!({
        "content": content,
        "msg_type": 0,
    });
    if let (Some(msg_id), Some(seq)) = (msg_id, seq) {
        body["msg_id"] = Value::String(msg_id.to_string());
        body["msg_seq"] = Value::from(seq);
    }
    body
}

/// Upload bytes as C2C image (`file_type` 1), then send `msg_type` 7 with `file_info`.
/// Empty `content` is omitted so QQ does not insert a blank line.
pub async fn send_c2c_image(
    db: &DatabaseConnection,
    auth_header: &str,
    openid: &str,
    bytes: &[u8],
    inbound_msg_id: Option<&str>,
) -> Result<(), ConnectFailureKind> {
    if bytes.is_empty() || openid.is_empty() {
        return Ok(());
    }
    let enabled = {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        config.qq_bot_enabled
    };
    if !enabled {
        return Ok(());
    }
    let file_info = upload_c2c_image(auth_header, openid, bytes)
        .await
        .map_err(|_| ConnectFailureKind::Transient)?;
    let mut body = serde_json::json!({
        "msg_type": 7,
        "media": { "file_info": file_info },
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
                    "QQ C2C passive image failed; falling back to active"
                );
                return Box::pin(send_c2c_image(db, auth_header, openid, bytes, None)).await;
            }
            warn!(
                status = status.as_u16(),
                body = %redact_secrets(&text),
                "QQ C2C image send failed"
            );
            Err(classify_connect_failure(&ConnectFailure::HttpStatus {
                status: status.as_u16(),
                body: &text,
            }))
        }
        Err(error) => {
            warn!(
                error = %redact_secrets(&error.to_string()),
                "QQ C2C image send request failed"
            );
            Err(ConnectFailureKind::Transient)
        }
    }
}

async fn upload_c2c_image(auth_header: &str, openid: &str, bytes: &[u8]) -> Result<String, String> {
    let client = http_client::get_global_client().await;
    let url = format!("{API_BASE}/v2/users/{openid}/files");
    let body = serde_json::json!({
        "file_type": 1,
        "file_data": BASE64.encode(bytes),
        "srv_send_msg": false,
    });
    match client
        .post(&url)
        .timeout(HTTP_TIMEOUT)
        .header("Authorization", auth_header)
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            parse_qq_file_info(status, &text).map_err(|kind| {
                warn!(
                    status,
                    body = %redact_secrets(&text),
                    "QQ C2C image upload failed"
                );
                format!("qq image upload {status} {kind:?}")
            })
        }
        Err(error) => {
            warn!(
                error = %redact_secrets(&error.to_string()),
                "QQ C2C image upload request failed"
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
    next_seq_in(db, SEQ_NAMESPACE, msg_id).await
}

/// Passive reply counters are per message and per scene: a group's `msg_seq`
/// is kept apart from C2C's.
async fn next_seq_in(db: &DatabaseConnection, namespace: &str, msg_id: &str) -> u32 {
    let last = shared_registry::get::<StoredSeq>(db, namespace, msg_id)
        .await
        .ok()
        .flatten()
        .map(|stored| stored.seq);
    let seq = next_passive_seq(last);
    let _ = shared_registry::put(
        db,
        namespace,
        msg_id,
        RegistryIdentity {
            subject_id: None,
            owner_id: None,
            tapp_id: None,
            runtime_id: None,
        },
        &StoredSeq { seq },
        (crate::clock::now() + ChronoDuration::hours(2)).timestamp(),
    )
    .await;
    seq
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_messages_go_to_the_group_endpoint() {
        assert_eq!(
            group_messages_url("group-a"),
            "https://api.bot.qq.com/v2/groups/group-a/messages"
        );
        assert_ne!(GROUP_SEQ_NAMESPACE, SEQ_NAMESPACE);
    }

    #[test]
    fn passive_group_body_carries_message_id_and_sequence() {
        let body = group_body("你好", Some("m1"), Some(2));
        assert_eq!(body["content"], "你好");
        assert_eq!(body["msg_type"], 0);
        assert_eq!(body["msg_id"], "m1");
        assert_eq!(body["msg_seq"], 2);

        let active = group_body("你好", None, None);
        assert!(active.get("msg_id").is_none());
        assert!(active.get("msg_seq").is_none());
    }
}
