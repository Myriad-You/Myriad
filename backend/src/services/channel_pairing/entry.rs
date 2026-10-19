//! One entry for every platform's inbound private text: drop redeliveries,
//! then prompt for pairing, bind a pairing code, or start Work.

use std::future::Future;

use myriad_agent_rules::channel::{
    ChannelFileRef, ChannelVoiceRef, InboundC2cText, InboundDecision, PairingBindResult,
    PairingLookup, ingest_channel_text, pairing_bind_reply_for, session_key,
};
use sea_orm::{DatabaseConnection, DbErr};
use tracing::{info, warn};

use super::{PairingChannel, consume_code, lookup_openid};
use crate::services::channel_platform::ChannelPlatform;
use crate::services::channel_work::attachments::{self, VoiceAudio};

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

    /// The audio behind one of its voices. A public http(s) URL is fetched by
    /// default; a platform overrides this for its own handles, with its
    /// credentials kept on the backend. `Ok(None)`: no audio to be had.
    fn voice_audio(
        &self,
        voice: &ChannelVoiceRef,
    ) -> impl Future<Output = Result<Option<VoiceAudio>, String>> + Send {
        attachments::fetch_public_voice(voice)
    }

    /// The platform's own transcript of a voice, asked when the site's speech
    /// service heard nothing. None by default.
    fn platform_voice_text(
        &self,
        _voice: &ChannelVoiceRef,
    ) -> impl Future<Output = Option<String>> + Send {
        async { None }
    }

    /// The bytes of one of its files (only small text files are asked for).
    /// A public http(s) URL is fetched by default, like a voice.
    fn file_bytes(
        &self,
        file: &ChannelFileRef,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, String>> + Send {
        attachments::fetch_public_file(file)
    }

    fn start_work(
        &self,
        db: &DatabaseConnection,
        user_id: i32,
        input: &str,
        session_key: &str,
    ) -> impl Future<Output = ()> + Send;
}

/// A paired sender's message with nothing in it she can read.
const NOTHING_READABLE_REPLY: &str = "这条消息里没有能读的内容：目前只认文字、图片、语音和文件。";

pub(crate) async fn handle_private_text<T: PrivateText>(text: T) {
    let platform = T::PLATFORM;
    let Ok(db) = crate::services::process_db::database() else {
        warn!(%platform, "channel inbound skipped: database is not connected");
        return;
    };
    let inbound = text.inbound();
    let from = inbound.user_openid.clone();
    let chat_id = text.session_chat_id();
    // Gateways replay messages after a reconnect. Claim each one before
    // pairing state is read: once the first delivery of a pairing code has
    // bound the sender, a replay would otherwise start Work with the code.
    let claimed = format!(
        "{}:{}",
        session_key(platform.slug(), &chat_id),
        inbound.msg_id
    );
    let has_id = !inbound.msg_id.is_empty();
    let already_seen = has_id
        && !crate::services::channel_work::claim_inbound(&db, platform, None, &claimed).await;
    // A move of theirs in a game they sit at in a group, paired or not, is
    // the game's (D2c-13): it never reaches pairing. Anything else they say
    // goes on as it would.
    if !already_seen
        && crate::services::channel_group::whispered(
            platform,
            &inbound.user_openid,
            &inbound.content,
        )
        .await
    {
        return;
    }
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
        InboundDecision::PairingRequired { reply, .. } => {
            text.reply(&db, &unpaired_reply(platform, &from, reply))
                .await
        }
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
        } => {
            // Voices and files are read only now, once the sender is paired:
            // pairing never fetches or transcribes for a stranger.
            let input = attachments::compose_input(&text, &input).await;
            if input.trim().is_empty() && text.inbound().images.is_empty() {
                text.reply(&db, NOTHING_READABLE_REPLY).await;
                return;
            }
            // Held by this id while she is not on the site (D3).
            let held_as = if has_id { claimed } else { String::new() };
            crate::services::channel_work::held::INBOUND
                .scope(held_as, text.start_work(&db, user_id, &input, &session_key))
                .await
        }
    }
}

/// What one not paired is told for a private line that is not a game's
/// move: asked to pair, unless they sit at a game in a group now (they
/// play it unpaired, D2c-13; the private chat takes only the game till it
/// is over).
pub(crate) fn unpaired_reply(platform: ChannelPlatform, from: &str, reply: String) -> String {
    if crate::services::channel_group::seated(platform, from) {
        myriad_merope::hosting::SEATED_UNPAIRED_LINE.to_string()
    } else {
        reply
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
        let game = handle.find("channel_group::whispered(").expect("game");
        assert!(
            claim < game && game < lookup,
            "a game's move is taken after the claim and before pairing"
        );
        let decide = handle.find("ingest_channel_text(").expect("decide");
        let start = handle.find("InboundDecision::StartWork").expect("start");
        let read = handle.find("attachments::compose_input(").expect("attachments");
        assert!(
            decide < start && start < read,
            "voices and files are fetched only for a paired sender"
        );

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
