//! Discord DM pairing entry: classify inbound text, then shared pairing I/O.

use myriad_agent_rules::channel::{
    ingest_channel_text, pairing_bind_reply_for, session_key, DiscordPrivateComponent,
    DiscordPrivateText, InboundDecision, PairingBindResult, PairingLookup, PAIRING_REQUIRED_REPLY,
};
use sea_orm::{DatabaseConnection, DbErr};
use tracing::{info, warn};

use crate::services::channel_pairing::{self, DISCORD_DM};

pub use crate::services::channel_pairing::{IssuedPairingCode, PairingStatus};

pub async fn lookup_openid(db: &DatabaseConnection, openid: &str) -> Result<PairingLookup, DbErr> {
    channel_pairing::lookup_openid(db, DISCORD_DM, openid).await
}

pub async fn status_for_user(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<PairingStatus, DbErr> {
    channel_pairing::status_for_user(db, DISCORD_DM, user_id).await
}

pub async fn mint_code(db: &DatabaseConnection, user_id: i32) -> Result<IssuedPairingCode, DbErr> {
    channel_pairing::mint_code(db, DISCORD_DM, user_id).await
}

pub async fn unpair(db: &DatabaseConnection, user_id: i32) -> Result<bool, DbErr> {
    channel_pairing::unpair(db, DISCORD_DM, user_id).await
}

pub async fn consume_code(
    db: &DatabaseConnection,
    openid: &str,
    raw_code: &str,
) -> Result<PairingBindResult, DbErr> {
    channel_pairing::consume_code(db, DISCORD_DM, openid, raw_code).await
}

/// Worker entry: classify private text, pair, or start Work.
pub async fn handle_inbound(event: DiscordPrivateText, token: &str) {
    let Ok(db) = crate::services::tapp_registry::database().await else {
        warn!("Discord pairing skipped: database is not connected");
        return;
    };
    let inbound = event.inbound();
    let pairing = match lookup_openid(&db, &inbound.user_openid).await {
        Ok(value) => value,
        Err(error) => {
            warn!(error = %error, "Discord pairing lookup failed");
            return;
        }
    };
    match ingest_channel_text(&inbound, pairing, false, "discord", &event.author_id) {
        InboundDecision::Duplicate { .. } => {}
        InboundDecision::PairingRequired { reply, .. } => {
            send_text(token, &event.channel_id, &reply).await;
        }
        InboundDecision::ConsumePairingCode {
            user_openid, code, ..
        } => {
            let result = match consume_code(&db, &user_openid, &code).await {
                Ok(value) => value,
                Err(error) => {
                    warn!(error = %error, "Discord pairing consume failed");
                    PairingBindResult::InvalidOrExpired
                }
            };
            if let PairingBindResult::Bound { user_id } = result {
                info!(user_id, "Discord DM paired");
            }
            send_text(
                token,
                &event.channel_id,
                pairing_bind_reply_for(result, "discord"),
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
            crate::services::discord_work::start_paired_work(
                &db,
                user_id,
                &event.channel_id,
                &input,
                &session_key,
                &msg_id,
                token,
            )
            .await;
        }
    }
}

/// Worker entry: component press. ACK first (3s), then resume pending.
pub async fn handle_component(event: DiscordPrivateComponent, token: &str) {
    if let Err(error) = crate::services::discord_bot::ack_component(
        token,
        &event.interaction_id,
        &event.interaction_token,
    )
    .await
    {
        warn!(?error, "Discord interaction ack failed");
    }
    let Ok(db) = crate::services::tapp_registry::database().await else {
        warn!("Discord callback skipped: database is not connected");
        return;
    };
    let pairing = match lookup_openid(&db, &event.author_id).await {
        Ok(value) => value,
        Err(error) => {
            warn!(error = %error, "Discord callback pairing lookup failed");
            return;
        }
    };
    let PairingLookup::Paired { user_id } = pairing else {
        send_text(token, &event.channel_id, PAIRING_REQUIRED_REPLY).await;
        return;
    };
    crate::services::discord_work::start_paired_callback(
        &db,
        user_id,
        &event.channel_id,
        &event.custom_id,
        &session_key("discord", &event.author_id),
        &event.message_id,
        token,
    )
    .await;
}

async fn send_text(token: &str, channel_id: &str, content: &str) {
    if let Err(error) = crate::services::discord_bot::send_message(token, channel_id, content).await
    {
        warn!(?error, "Discord pairing reply failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use myriad_agent_rules::channel::InboundC2cText;

    #[test]
    fn unpaired_plain_text_is_not_a_work_request() {
        let event = InboundC2cText {
            msg_id: "10".into(),
            user_openid: "1001".into(),
            content: "帮我查天气".into(),
        };
        let decision =
            ingest_channel_text(&event, PairingLookup::Unpaired, false, "discord", "1001");
        match decision {
            InboundDecision::PairingRequired { reply, .. } => {
                assert_eq!(reply, PAIRING_REQUIRED_REPLY);
            }
            other => panic!("{other:?}"),
        }
    }
}
