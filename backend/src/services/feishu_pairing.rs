//! Feishu p2p pairing entry: classify inbound text, then shared pairing I/O.

use myriad_agent_rules::channel::{
    ingest_channel_text, pairing_bind_reply_for, session_key, FeishuCardCallback, InboundDecision,
    InboundFeishuText, PairingBindResult, PairingLookup, PAIRING_REQUIRED_REPLY,
};
use sea_orm::{DatabaseConnection, DbErr};
use tracing::{info, warn};

use crate::services::channel_pairing::{self, FEISHU};

pub use crate::services::channel_pairing::{IssuedPairingCode, PairingStatus};

pub async fn lookup_any(db: &DatabaseConnection, keys: &[String]) -> Result<PairingLookup, DbErr> {
    channel_pairing::lookup_any(db, FEISHU, keys).await
}

pub async fn status_for_user(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<PairingStatus, DbErr> {
    channel_pairing::status_for_user(db, FEISHU, user_id).await
}

pub async fn mint_code(db: &DatabaseConnection, user_id: i32) -> Result<IssuedPairingCode, DbErr> {
    channel_pairing::mint_code(db, FEISHU, user_id).await
}

pub async fn unpair(db: &DatabaseConnection, user_id: i32) -> Result<bool, DbErr> {
    channel_pairing::unpair(db, FEISHU, user_id).await
}

pub async fn consume_code_keys(
    db: &DatabaseConnection,
    keys: &[String],
    raw_code: &str,
) -> Result<PairingBindResult, DbErr> {
    channel_pairing::consume_code_keys(db, FEISHU, keys, raw_code).await
}

/// Worker entry: classify p2p text, pair, or start Work.
pub async fn handle_inbound(event: InboundFeishuText) {
    let Ok(db) = crate::services::tapp_registry::database().await else {
        warn!("Feishu pairing skipped: database is not connected");
        return;
    };
    let inbound = event.inbound();
    let pairing = match lookup_any(&db, &event.identity_keys).await {
        Ok(value) => value,
        Err(error) => {
            warn!(error = %error, "Feishu pairing lookup failed");
            return;
        }
    };
    let chat_id = event.chat_id_key();
    match ingest_channel_text(&inbound, pairing, false, "feishu", &chat_id) {
        InboundDecision::Duplicate { .. } => {}
        InboundDecision::PairingRequired { reply, .. } => {
            send_text(&chat_id, &reply).await;
        }
        InboundDecision::ConsumePairingCode { code, .. } => {
            let result = match consume_code_keys(&db, &event.identity_keys, &code).await {
                Ok(value) => value,
                Err(error) => {
                    warn!(error = %error, "Feishu pairing consume failed");
                    PairingBindResult::InvalidOrExpired
                }
            };
            if let PairingBindResult::Bound { user_id } = result {
                info!(user_id, "Feishu p2p paired");
            }
            send_text(&chat_id, pairing_bind_reply_for(result, "feishu")).await;
        }
        InboundDecision::StartWork {
            user_id,
            input,
            session_key,
            msg_id,
            ..
        } => {
            if let Err(error) =
                channel_pairing::ensure_aliases(&db, FEISHU, user_id, &event.identity_keys).await
            {
                warn!(error = %error, "Feishu pairing alias write failed");
            }
            crate::services::feishu_work::start_paired_work_with_images(
                &db,
                user_id,
                &event.open_id,
                &chat_id,
                &input,
                &event.images,
                &session_key,
                &msg_id,
            )
            .await;
        }
    }
}

/// Worker entry: interactive-card button. Always start Work after pairing.
pub async fn handle_callback(event: FeishuCardCallback) {
    let Ok(db) = crate::services::tapp_registry::database().await else {
        warn!("Feishu callback skipped: database is not connected");
        return;
    };
    let pairing = match lookup_any(&db, &event.identity_keys).await {
        Ok(value) => value,
        Err(error) => {
            warn!(error = %error, "Feishu callback pairing lookup failed");
            return;
        }
    };
    let PairingLookup::Paired { user_id } = pairing else {
        send_text(&event.chat_id_key(), PAIRING_REQUIRED_REPLY).await;
        return;
    };
    if let Err(error) =
        channel_pairing::ensure_aliases(&db, FEISHU, user_id, &event.identity_keys).await
    {
        warn!(error = %error, "Feishu callback alias write failed");
    }
    crate::services::feishu_work::start_paired_callback(
        &db,
        user_id,
        &event.open_id,
        &event.chat_id_key(),
        &event.data,
        &session_key("feishu", &event.chat_id_key()),
        &event.msg_id(),
    )
    .await;
}

async fn send_text(chat_id: &str, content: &str) {
    if let Err(error) = crate::services::feishu_bot_api::send_message(chat_id, content).await {
        warn!(?error, "Feishu pairing reply failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use myriad_agent_rules::channel::InboundC2cText;

    #[test]
    fn unpaired_plain_text_is_not_a_work_request() {
        let event = InboundC2cText {
            msg_id: "om1".into(),
            user_openid: "ou1".into(),
            content: "帮我查天气".into(),
            images: Vec::new(),
        };
        let decision = ingest_channel_text(&event, PairingLookup::Unpaired, false, "feishu", "oc1");
        match decision {
            InboundDecision::PairingRequired { reply, .. } => {
                assert_eq!(reply, PAIRING_REQUIRED_REPLY);
            }
            other => panic!("{other:?}"),
        }
    }
}
