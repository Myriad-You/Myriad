//! QQ C2C pairing entry: classify inbound text, then shared pairing I/O.

use myriad_agent_rules::channel::{
    ingest_c2c_text, pairing_bind_reply, InboundC2cText, InboundDecision, PairingBindResult,
    PairingLookup, PAIRING_REQUIRED_REPLY,
};
use sea_orm::{DatabaseConnection, DbErr};
use tracing::{info, warn};

use crate::services::channel_pairing::{self, QQ};
use crate::services::http_client;
use crate::GLOBAL_DYNAMIC_CONFIG;

pub use crate::services::channel_pairing::{IssuedPairingCode, PairingStatus};

const API_BASE: &str = "https://api.bot.qq.com";
const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

pub fn mask_openid(openid: &str) -> String {
    channel_pairing::mask_openid(openid)
}

pub async fn lookup_openid(db: &DatabaseConnection, openid: &str) -> Result<PairingLookup, DbErr> {
    channel_pairing::lookup_openid(db, QQ, openid).await
}

pub async fn status_for_user(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<PairingStatus, DbErr> {
    channel_pairing::status_for_user(db, QQ, user_id).await
}

pub async fn mint_code(db: &DatabaseConnection, user_id: i32) -> Result<IssuedPairingCode, DbErr> {
    channel_pairing::mint_code(db, QQ, user_id).await
}

pub async fn unpair(db: &DatabaseConnection, user_id: i32) -> Result<bool, DbErr> {
    channel_pairing::unpair(db, QQ, user_id).await
}

pub async fn consume_code(
    db: &DatabaseConnection,
    openid: &str,
    raw_code: &str,
) -> Result<PairingBindResult, DbErr> {
    channel_pairing::consume_code(db, QQ, openid, raw_code).await
}

/// Gateway worker entry: classify the C2C text, bind pairing codes, or start Work.
pub async fn handle_inbound_c2c(event: InboundC2cText, auth_header: &str) {
    let Ok(db) = crate::services::tapp_registry::database().await else {
        warn!("QQ pairing skipped: database is not connected");
        return;
    };
    let pairing = match lookup_openid(&db, &event.user_openid).await {
        Ok(value) => value,
        Err(error) => {
            warn!(error = %error, "QQ pairing lookup failed");
            return;
        }
    };
    match ingest_c2c_text(&event, pairing, false) {
        InboundDecision::Duplicate { .. } => {}
        InboundDecision::PairingRequired { reply, msg_id, .. } => {
            send_passive_text(auth_header, &event.user_openid, &reply, &msg_id).await;
        }
        InboundDecision::ConsumePairingCode {
            user_openid,
            code,
            msg_id,
        } => {
            let result = match consume_code(&db, &user_openid, &code).await {
                Ok(value) => value,
                Err(error) => {
                    warn!(error = %error, "QQ pairing consume failed");
                    PairingBindResult::InvalidOrExpired
                }
            };
            if let PairingBindResult::Bound { user_id } = result {
                info!(user_id, "QQ C2C paired");
            }
            send_passive_text(
                auth_header,
                &event.user_openid,
                pairing_bind_reply(result),
                &msg_id,
            )
            .await;
        }
        InboundDecision::StartWork {
            user_id,
            input,
            session_key,
            msg_id,
            ..
        } => {
            crate::services::qq_work::start_paired_work_with_images(
                &db,
                user_id,
                &event.user_openid,
                &input,
                &event.images,
                &session_key,
                &msg_id,
                auth_header,
            )
            .await;
        }
    }
}

async fn send_passive_text(auth_header: &str, openid: &str, content: &str, msg_id: &str) {
    if content.is_empty() || openid.is_empty() || msg_id.is_empty() {
        return;
    }
    let enabled = {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        config.qq_bot_enabled
    };
    if !enabled {
        return;
    }
    let client = http_client::get_global_client().await;
    let url = format!("{API_BASE}/v2/users/{openid}/messages");
    let body = serde_json::json!({
        "content": content,
        "msg_type": 0,
        "msg_id": msg_id,
        "msg_seq": 1,
    });
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
            warn!(
                status = status.as_u16(),
                body = %myriad_error::redact_secrets(&text),
                "QQ pairing reply failed"
            );
        }
        Err(error) => {
            warn!(
                error = %myriad_error::redact_secrets(&error.to_string()),
                "QQ pairing reply request failed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_openid_without_returning_the_full_id() {
        assert_eq!(mask_openid("abcdefg"), "****defg");
        assert_eq!(mask_openid("ab"), "****");
        assert_eq!(mask_openid(""), "");
        assert!(!mask_openid("openid-secret-value").contains("openid-secret"));
    }

    #[test]
    fn unpaired_plain_text_is_not_a_work_request() {
        let event = InboundC2cText {
            msg_id: "m1".into(),
            user_openid: "oid".into(),
            content: "帮我查天气".into(),
            images: Vec::new(),
        };
        let decision = ingest_c2c_text(&event, PairingLookup::Unpaired, false);
        match decision {
            InboundDecision::PairingRequired { reply, .. } => {
                assert_eq!(reply, PAIRING_REQUIRED_REPLY);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn pairing_channel_is_qq() {
        assert_eq!(QQ.provider, "qq");
    }
}
