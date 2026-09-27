//! Telegram DM pairing entry: classify inbound text, then shared pairing I/O.

use myriad_agent_rules::channel::{
    InboundC2cText, PAIRING_REQUIRED_REPLY, PairingLookup, TelegramPrivateCallback,
    TelegramPrivateText,
};
use sea_orm::{DatabaseConnection, DbErr};
use tracing::warn;

use crate::services::channel_pairing::{self, PrivateText, TELEGRAM};
use crate::services::channel_platform::ChannelPlatform;

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

/// Worker entry for one private text.
pub async fn handle_inbound(event: TelegramPrivateText, token: &str) {
    let inbound = event.inbound();
    channel_pairing::handle_private_text(TelegramText {
        event,
        inbound,
        token,
    })
    .await;
}

struct TelegramText<'a> {
    event: TelegramPrivateText,
    inbound: InboundC2cText,
    token: &'a str,
}

impl PrivateText for TelegramText<'_> {
    const PLATFORM: ChannelPlatform = ChannelPlatform::Telegram;

    fn inbound(&self) -> &InboundC2cText {
        &self.inbound
    }

    fn session_chat_id(&self) -> String {
        self.event.chat_id_key()
    }

    async fn reply(&self, _db: &DatabaseConnection, text: &str) {
        send_text(self.token, &self.event.chat_id_key(), text).await;
    }

    async fn start_work(
        &self,
        db: &DatabaseConnection,
        user_id: i32,
        input: &str,
        session_key: &str,
    ) {
        crate::services::telegram_work::start_paired_work_with_images(
            db,
            user_id,
            &self.event.from_id.to_string(),
            &self.event.chat_id_key(),
            input,
            &self.event.images,
            session_key,
            self.token,
        )
        .await;
    }
}

/// Worker entry: inline-button press. Always ack the callback first.
pub async fn handle_callback(event: TelegramPrivateCallback, token: &str) {
    if let Err(error) =
        crate::services::telegram_bot::answer_callback_query(token, &event.callback_query_id).await
    {
        warn!(?error, "Telegram callback ack failed");
    }
    let Ok(db) = crate::services::process_db::database() else {
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
        &event.from_id.to_string(),
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
    use myriad_agent_rules::channel::{InboundDecision, ingest_channel_text};

    #[test]
    fn unpaired_plain_text_is_not_a_work_request() {
        let event = InboundC2cText {
            msg_id: "10".into(),
            user_openid: "1001".into(),
            content: "帮我查天气".into(),
            images: Vec::new(),
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
