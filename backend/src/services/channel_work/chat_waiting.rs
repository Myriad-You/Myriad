//! What came in a private chat that she has not answered yet, kept across a
//! restart where she can answer later, and waited on again after one.

use myriad_agent_rules::channel::ChannelImageRef;
use sea_orm::DatabaseConnection;
use tracing::warn;

use crate::services::channel_platform::ChannelPlatform;
use crate::services::runtime_registry::{self as shared_registry, RegistryIdentity};

use super::ChannelSink;
use super::chat::{UNSEEN, start_chat_turn};

/// Runtime-registry namespace of what came in a private chat while she was
/// asleep, by session key: kept until she wakes and reads it.
const WAITING_NS: &str = "channel_private_waiting";
/// Kept this long at most, whatever happens.
const WAITING_FOR: chrono::Duration = chrono::Duration::hours(24);

/// Platforms where she can answer when she wakes; elsewhere a message wakes
/// her (see [`ChannelPlatform::may_send_unasked`]).
pub(super) fn can_answer_later(platform: ChannelPlatform) -> bool {
    platform.may_send_unasked()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct KeptImage {
    url: String,
    name: String,
    mime: String,
    size: u64,
}

/// What came in a private chat that she has not answered yet.
#[derive(serde::Serialize, serde::Deserialize)]
struct Waiting {
    user_id: i32,
    work_session_id: String,
    texts: Vec<String>,
    images: Vec<KeptImage>,
}

/// Keep what she has not answered in this chat, so a restart before she
/// does (asleep, about to read it, or mid-reply) does not lose it.
pub(super) async fn keep_waiting(
    db: &DatabaseConnection,
    session_key: &str,
    user_id: i32,
    work_session_id: &str,
) {
    let Some(waiting) = UNSEEN.lock().ok().and_then(|unseen| {
        unseen.get(session_key).map(|seen| Waiting {
            user_id,
            work_session_id: work_session_id.to_string(),
            texts: seen.texts.clone(),
            images: seen
                .images
                .iter()
                .map(|image| KeptImage {
                    url: image.url.clone(),
                    name: image.name.clone(),
                    mime: image.mime.clone(),
                    size: image.size,
                })
                .collect(),
        })
    }) else {
        return;
    };
    let keep_until = (chrono::Utc::now() + WAITING_FOR).timestamp();
    if let Err(error) = shared_registry::put(
        db,
        WAITING_NS,
        session_key,
        RegistryIdentity {
            subject_id: Some(user_id),
            owner_id: None,
            tapp_id: None,
            runtime_id: None,
        },
        &waiting,
        keep_until,
    )
    .await
    {
        warn!(%error, "[Channel] could not keep a message she has not answered");
    }
}

pub(super) async fn forget_waiting(db: &DatabaseConnection, session_key: &str) {
    let _ = shared_registry::delete(db, WAITING_NS, session_key).await;
}

/// After a restart: what came in private chats that she had not answered,
/// waited on again (until she wakes, if she is asleep), each in the chat it
/// came in. A chat no longer paired, or a platform turned off, lets it go.
pub(crate) async fn take_back_waiting(db: &DatabaseConnection) {
    let rows = shared_registry::list(db, WAITING_NS, None, None)
        .await
        .unwrap_or_default();
    for row in rows {
        let session_key = row.record_id;
        let Ok(waiting) = serde_json::from_value::<Waiting>(row.payload) else {
            forget_waiting(db, &session_key).await;
            continue;
        };
        let mut sink = None;
        for platform in ChannelPlatform::ALL {
            let Some(stored) = super::load_session(db, platform, &session_key).await else {
                continue;
            };
            let (Some(binding), Some(address)) = (stored.binding, stored.address) else {
                continue;
            };
            if binding.user_id != waiting.user_id || !binding.is_current(db).await {
                continue;
            }
            if let Some(transport) = address.connect(db).await {
                sink = Some(ChannelSink {
                    transport,
                    binding,
                    db: db.clone(),
                });
            }
            break;
        }
        let Some(sink) = sink else {
            forget_waiting(db, &session_key).await;
            continue;
        };
        let images: Vec<ChannelImageRef> = waiting
            .images
            .into_iter()
            .map(|image| ChannelImageRef {
                url: image.url,
                name: image.name,
                mime: image.mime,
                size: image.size,
            })
            .collect();
        let texts = waiting.texts.join("\n");
        let (db, user_id, work_session_id) = (db.clone(), waiting.user_id, waiting.work_session_id);
        tracing::info!("[Channel] what she had not answered, waited on again after a restart");
        tokio::spawn(async move {
            start_chat_turn(
                db,
                user_id,
                work_session_id,
                &texts,
                &images,
                sink,
                &session_key,
            )
            .await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn she_sleeps_on_a_message_only_where_she_can_answer_it_later() {
        assert!(can_answer_later(ChannelPlatform::Telegram));
        assert!(can_answer_later(ChannelPlatform::Discord));
        assert!(can_answer_later(ChannelPlatform::OneBot));
        // The official QQ bot answers only a fresh message: it wakes her.
        assert!(!can_answer_later(ChannelPlatform::Qq));
        assert!(!can_answer_later(ChannelPlatform::Feishu));
        let kept = serde_json::to_value(Waiting {
            user_id: 7,
            work_session_id: "w".into(),
            texts: vec!["在吗".into(), "睡了？".into()],
            images: vec![KeptImage {
                url: "tg:file".into(),
                name: "a.jpg".into(),
                mime: "image/jpeg".into(),
                size: 3,
            }],
        })
        .unwrap();
        let back: Waiting = serde_json::from_value(kept).unwrap();
        assert_eq!(back.texts, ["在吗", "睡了？"]);
        assert_eq!(back.images[0].url, "tg:file");
    }
}
