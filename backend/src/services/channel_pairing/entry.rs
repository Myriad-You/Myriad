//! One entry for every platform's inbound private text: drop redeliveries,
//! then prompt for pairing, bind a pairing code, or start Work.

use std::future::Future;

use myriad_agent_rules::channel::{
    InboundC2cText, InboundDecision, PairingBindResult, PairingLookup, ingest_channel_text,
    pairing_bind_reply_for, session_key,
};
use sea_orm::{DatabaseConnection, DbErr};
use tracing::{info, warn};

use super::{PairingChannel, consume_code, lookup_openid};
use crate::services::channel_platform::ChannelPlatform;

/// One inbound private text, and how its platform answers and hands it on.
pub(crate) trait PrivateText: Send + Sync {
    const PLATFORM: ChannelPlatform;

    /// The message as the pairing rules read it.
    fn inbound(&self) -> &InboundC2cText;

    /// The chat the Work session key is built from.
    fn session_chat_id(&self) -> String;

    fn lookup(
        &self,
        db: &DatabaseConnection,
    ) -> impl Future<Output = Result<PairingLookup, DbErr>> + Send {
        lookup_openid(
            db,
            PairingChannel::of(Self::PLATFORM),
            &self.inbound().user_openid,
        )
    }

    fn consume(
        &self,
        db: &DatabaseConnection,
        code: &str,
    ) -> impl Future<Output = Result<PairingBindResult, DbErr>> + Send {
        consume_code(
            db,
            PairingChannel::of(Self::PLATFORM),
            &self.inbound().user_openid,
            code,
        )
    }

    fn reply(&self, db: &DatabaseConnection, text: &str) -> impl Future<Output = ()> + Send;

    fn start_work(
        &self,
        db: &DatabaseConnection,
        user_id: i32,
        input: &str,
        session_key: &str,
    ) -> impl Future<Output = ()> + Send;
}

pub(crate) async fn handle_private_text<T: PrivateText>(text: T) {
    let platform = T::PLATFORM;
    let Ok(db) = crate::services::process_db::database() else {
        warn!(%platform, "channel inbound skipped: database is not connected");
        return;
    };
    let inbound = text.inbound();
    let chat_id = text.session_chat_id();
    // Gateways replay messages after a reconnect. Claim each one before
    // pairing state is read: once the first delivery of a pairing code has
    // bound the sender, a replay would otherwise start Work with the code.
    let already_seen = !inbound.msg_id.is_empty()
        && !crate::services::channel_work::claim_inbound(
            &db,
            platform,
            None,
            &format!(
                "{}:{}",
                session_key(platform.slug(), &chat_id),
                inbound.msg_id
            ),
        )
        .await;
    let pairing = if already_seen {
        // Not consulted: a duplicate is dropped before pairing matters.
        PairingLookup::Unpaired
    } else {
        match text.lookup(&db).await {
            Ok(pairing) => pairing,
            Err(error) => {
                warn!(%error, %platform, "channel pairing lookup failed");
                return;
            }
        }
    };
    match ingest_channel_text(inbound, pairing, already_seen, platform.slug(), &chat_id) {
        InboundDecision::Duplicate { .. } => {}
        InboundDecision::PairingRequired { reply, .. } => text.reply(&db, &reply).await,
        InboundDecision::ConsumePairingCode { code, .. } => {
            let result = text.consume(&db, &code).await.unwrap_or_else(|error| {
                warn!(%error, %platform, "channel pairing consume failed");
                PairingBindResult::InvalidOrExpired
            });
            if let PairingBindResult::Bound { user_id } = result {
                info!(user_id, %platform, "channel paired");
            }
            text.reply(&db, pairing_bind_reply_for(result, platform.slug()))
                .await;
        }
        InboundDecision::StartWork {
            user_id,
            input,
            session_key,
            ..
        } => text.start_work(&db, user_id, &input, &session_key).await,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_private_text_is_claimed_before_pairing_is_read() {
        let entry = include_str!("entry.rs");
        let handle = entry
            .split("pub(crate) async fn handle_private_text")
            .nth(1)
            .expect("entry");
        let claim = handle.find("claim_inbound(").expect("claim");
        let lookup = handle.find("text.lookup(").expect("lookup");
        assert!(claim < lookup, "the claim must precede the pairing lookup");

        for (name, source) in [
            ("qq_pairing", include_str!("../qq_pairing.rs")),
            ("discord_pairing", include_str!("../discord_pairing.rs")),
            ("telegram_pairing", include_str!("../telegram_pairing.rs")),
            ("feishu_pairing", include_str!("../feishu_pairing.rs")),
            ("onebot_pairing", include_str!("../onebot_pairing.rs")),
        ] {
            let body = source.split("#[cfg(test)]\nmod tests").next().unwrap();
            assert!(
                body.contains("channel_pairing::handle_private_text("),
                "{name} must enter through handle_private_text"
            );
            assert!(
                !body.contains("ingest_channel_text(") && !body.contains("ingest_c2c_text("),
                "{name} classifies inbound text on its own"
            );
        }
    }
}
