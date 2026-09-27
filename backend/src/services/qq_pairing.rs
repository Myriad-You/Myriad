//! QQ C2C pairing entry: classify inbound text, then shared pairing I/O.

use myriad_agent_rules::channel::InboundC2cText;
#[cfg(test)]
use myriad_agent_rules::channel::PairingLookup;
use sea_orm::{DatabaseConnection, DbErr};
use tracing::warn;

use crate::services::channel_pairing::{self, PrivateText, QQ};
use crate::services::channel_platform::ChannelPlatform;

pub use crate::services::channel_pairing::{IssuedPairingCode, PairingStatus};

#[cfg(test)]
pub fn mask_openid(openid: &str) -> String {
    channel_pairing::mask_openid(openid)
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

/// Gateway worker entry for one C2C text.
pub async fn handle_inbound_c2c(event: InboundC2cText, auth_header: &str) {
    channel_pairing::handle_private_text(QqText { event, auth_header }).await;
}

struct QqText<'a> {
    event: InboundC2cText,
    auth_header: &'a str,
}

impl PrivateText for QqText<'_> {
    const PLATFORM: ChannelPlatform = ChannelPlatform::Qq;

    fn inbound(&self) -> &InboundC2cText {
        &self.event
    }

    fn session_chat_id(&self) -> String {
        self.event.user_openid.clone()
    }

    async fn reply(&self, db: &DatabaseConnection, text: &str) {
        send_passive_text(
            db,
            self.auth_header,
            &self.event.user_openid,
            text,
            &self.event.msg_id,
        )
        .await;
    }

    async fn start_work(
        &self,
        db: &DatabaseConnection,
        user_id: i32,
        input: &str,
        session_key: &str,
    ) {
        crate::services::qq_work::start_paired_work_with_images(
            db,
            user_id,
            &self.event.user_openid,
            &self.event.user_openid,
            input,
            &self.event.images,
            session_key,
            &self.event.msg_id,
        )
        .await;
    }
}

/// Pairing replies go through the same sender as Work replies: per-message
/// `msg_seq` from the shared sequence table (a second reply to one message
/// with a fixed seq is rejected as a duplicate) and the fallback to an active
/// message once the passive reply window has closed.
async fn send_passive_text(
    db: &DatabaseConnection,
    auth_header: &str,
    openid: &str,
    content: &str,
    msg_id: &str,
) {
    if msg_id.is_empty() {
        return;
    }
    if let Err(kind) =
        crate::services::qq_work::send_c2c(db, auth_header, openid, content, Some(msg_id)).await
    {
        warn!(?kind, "QQ pairing reply failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use myriad_agent_rules::channel::{InboundDecision, PAIRING_REQUIRED_REPLY, ingest_c2c_text};

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
