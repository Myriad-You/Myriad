//! Telegram DM pairing entry: classify inbound text, then shared pairing I/O.

use myriad_agent_rules::channel::{
    ingest_channel_text, pairing_bind_reply_for, InboundDecision, PairingBindResult, PairingLookup,
    TelegramPrivateCallback, TelegramPrivateText, PAIRING_REQUIRED_REPLY,
};
use sea_orm::{DatabaseConnection, DbErr};
use tracing::{info, warn};

use crate::services::channel_pairing::{self, TELEGRAM};

pub use crate::services::channel_pairing::{IssuedPairingCode, PairingStatus};

pub async fn lookup_openid(db: &DatabaseConnection, openid: &str) -> Result<PairingLookup, DbErr> {
    channel_pairing::lookup_openid(db, TELEGRAM, openid).await
}

pub async fn status_for_user(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<PairingStatus, DbErr> {
    channel_pairing::status_for_user(db, TELEGRAM, user_id).await
}

pub async fn mint_code(db: &DatabaseConnection, user_id: i32) -> Result<IssuedPairingCode, DbErr> {
    channel_pairing::mint_code(db, TELEGRAM, user_id).await
}

pub async fn unpair(db: &DatabaseConnection, user_id: i32) -> Result<bool, DbErr> {
    channel_pairing::unpair(db, TELEGRAM, user_id).await
}

pub async fn consume_code(
    db: &DatabaseConnection,
    openid: &str,
    raw_code: &str,
) -> Result<PairingBindResult, DbErr> {
    channel_pairing::consume_code(db, TELEGRAM, openid, raw_code).await
}

/// Worker entry: classify private text, pair, or start Work.
pub async fn handle_inbound(event: TelegramPrivateText, token: &str) {
    let Ok(db) = crate::services::tapp_registry::database().await else {
        warn!("Telegram pairing skipped: database is not connected");
        return;
    };
    let inbound = event.inbound();
    let pairing = match lookup_openid(&db, &inbound.user_openid).await {
        Ok(value) => value,
        Err(error) => {
            warn!(error = %error, "Telegram pairing lookup failed");
            return;
        }
    };
    let chat_id = event.chat_id_key();
    match ingest_channel_text(&inbound, pairing, false, "telegram", &chat_id) {
        InboundDecision::Duplicate { .. } => {}
        InboundDecision::PairingRequired { reply, .. } => {
            send_text(token, &chat_id, &reply).await;
        }
        InboundDecision::ConsumePairingCode {
            user_openid, code, ..
        } => {
            let result = match consume_code(&db, &user_openid, &code).await {
                Ok(value) => value,
                Err(error) => {
                    warn!(error = %error, "Telegram pairing consume failed");
                    PairingBindResult::InvalidOrExpired
                }
            };
            if let PairingBindResult::Bound { user_id } = result {
                info!(user_id, "Telegram DM paired");
            }
            send_text(token, &chat_id, pairing_bind_reply_for(result, "telegram")).await;
        }
        InboundDecision::StartWork {
            user_id,
            input,
            session_key,
            msg_id,
            ..
        } => {
            crate::services::telegram_work::start_paired_work(
                &db,
                user_id,
                &chat_id,
                &input,
                &session_key,
                &msg_id,
                token,
            )
            .await;
        }
    }
}

/// Worker entry: inline-button press. Always ack the callback first.
pub async fn handle_callback(event: TelegramPrivateCallback, token: &str) {
    if let Err(error) =
        crate::services::telegram_bot::answer_callback_query(token, &event.callback_query_id).await
    {
        warn!(?error, "Telegram callback ack failed");
    }
    let Ok(db) = crate::services::tapp_registry::database().await else {
        warn!("Telegram callback skipped: database is not connected");
        return;
    };
    let pairing = match lookup_openid(&db, &event.from_id.to_string()).await {
        Ok(value) => value,
        Err(error) => {
            warn!(error = %error, "Telegram callback pairing lookup failed");
            return;
        }
    };
    let PairingLookup::Paired { user_id } = pairing else {
        send_text(token, &event.chat_id_key(), PAIRING_REQUIRED_REPLY).await;
        return;
    };
    crate::services::telegram_work::start_paired_callback(
        &db,
        user_id,
        &event.chat_id_key(),
        &event.data,
        &myriad_agent_rules::channel::session_key("telegram", &event.chat_id_key()),
        &event.msg_id(),
        token,
    )
    .await;
}

async fn send_text(token: &str, chat_id: &str, content: &str) {
    if let Err(error) = crate::services::telegram_bot::send_message(token, chat_id, content).await {
        warn!(?error, "Telegram pairing reply failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use myriad_agent_rules::channel::{ingest_channel_text, InboundC2cText};

    #[test]
    fn unpaired_plain_text_is_not_a_work_request() {
        let event = InboundC2cText {
            msg_id: "10".into(),
            user_openid: "1001".into(),
            content: "帮我查天气".into(),
        };
        let decision =
            ingest_channel_text(&event, PairingLookup::Unpaired, false, "telegram", "1001");
        match decision {
            InboundDecision::PairingRequired { reply, .. } => {
                assert_eq!(reply, PAIRING_REQUIRED_REPLY);
            }
            other => panic!("{other:?}"),
        }
    }
}
