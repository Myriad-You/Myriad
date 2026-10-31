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
/// What came and waits on the chat (she holds it while she writes a reply),
/// by `<session key>:<when>:<id>`: kept until it is in hand, so a restart
/// meanwhile does not lose a message already claimed.
const ARRIVING_NS: &str = "channel_private_arriving";

#[derive(serde::Serialize, serde::Deserialize)]
struct Arriving {
    user_id: i32,
    session_key: String,
    text: String,
    images: Vec<KeptImage>,
}

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
    let keep_until = (crate::clock::now() + WAITING_FOR).timestamp();
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

/// Keep a message that is about to wait on the chat; the record to let go
/// of once it is in hand.
pub(super) async fn arriving(
    db: &DatabaseConnection,
    session_key: &str,
    user_id: i32,
    text: &str,
    images: &[ChannelImageRef],
) -> Option<String> {
    let record = format!(
        "{session_key}:{:020}:{}",
        crate::clock::now().timestamp_micros(),
        uuid::Uuid::new_v4().simple()
    );
    let kept = Arriving {
        user_id,
        session_key: session_key.to_string(),
        text: text.to_string(),
        images: images.iter().map(kept_image).collect(),
    };
    let put = shared_registry::put(
        db,
        ARRIVING_NS,
        &record,
        RegistryIdentity {
            subject_id: Some(user_id),
            owner_id: None,
            tapp_id: None,
            runtime_id: None,
        },
        &kept,
        (crate::clock::now() + WAITING_FOR).timestamp(),
    )
    .await;
    match put {
        Ok(()) => Some(record),
        Err(error) => {
            warn!(%error, "[Channel] could not keep a message waiting on the chat");
            None
        }
    }
}

pub(super) async fn arrived(db: &DatabaseConnection, record: &str) {
    let _ = shared_registry::delete(db, ARRIVING_NS, record).await;
}

fn kept_image(image: &ChannelImageRef) -> KeptImage {
    KeptImage {
        url: image.url.clone(),
        name: image.name.clone(),
        mime: image.mime.clone(),
        size: image.size,
    }
}

/// After a restart: what waited on a chat comes in again, in the order it
/// came, after what she had not answered there.
async fn take_back_arriving(db: &DatabaseConnection) {
    let mut rows = shared_registry::list(db, ARRIVING_NS, None, None)
        .await
        .unwrap_or_default();
    rows.sort_by(|a, b| a.record_id.cmp(&b.record_id));
    let mut by_chat: std::collections::BTreeMap<String, Vec<(String, Arriving)>> =
        Default::default();
    for row in rows {
        match serde_json::from_value::<Arriving>(row.payload) {
            Ok(kept) => by_chat
                .entry(kept.session_key.clone())
                .or_default()
                .push((row.record_id, kept)),
            Err(_) => arrived(db, &row.record_id).await,
        }
    }
    for (session_key, kept) in by_chat {
        let db = db.clone();
        tokio::spawn(async move {
            for (record, kept) in kept {
                if let Some(sink) = sink_for(&db, kept.user_id, &session_key).await {
                    let images: Vec<ChannelImageRef> = kept
                        .images
                        .into_iter()
                        .map(|image| ChannelImageRef {
                            url: image.url,
                            name: image.name,
                            mime: image.mime,
                            size: image.size,
                        })
                        .collect();
                    super::with_chat_lock(
                        &session_key,
                        super::inbound::continue_text(
                            &db,
                            kept.user_id,
                            "",
                            &kept.text,
                            &images,
                            &session_key,
                            sink,
                        ),
                    )
                    .await;
                }
                arrived(&db, &record).await;
            }
        });
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
        let Some(sink) = sink_for(db, waiting.user_id, &session_key).await else {
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
        // She is not on the site: what was waiting is held for when she is
        // back, never started and refused (D3).
        if super::held::away() {
            super::held::hold(&db, &sink, &session_key, user_id, &texts, &images).await;
            forget_waiting(&db, &session_key).await;
            continue;
        }
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
    take_back_arriving(db).await;
}

/// A sink to answer `user_id` in `session_key` with, from the session the
/// channel stored: None when the pairing is gone or the bot is not there.
pub(super) async fn sink_for(
    db: &DatabaseConnection,
    user_id: i32,
    session_key: &str,
) -> Option<ChannelSink> {
    for platform in ChannelPlatform::ALL {
        let Some(stored) = super::load_session(db, platform, session_key).await else {
            continue;
        };
        let (Some(binding), Some(address)) = (stored.binding, stored.address) else {
            continue;
        };
        if binding.user_id != user_id || !binding.is_current(db).await {
            continue;
        }
        let transport = address.connect(db).await?;
        return Some(ChannelSink {
            transport,
            binding,
            db: db.clone(),
        });
    }
    None
}

/// The chat is still paired with `user_id` on `platform`.
pub(super) async fn paired(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    user_id: i32,
    session_key: &str,
) -> bool {
    let Some(stored) = super::load_session(db, platform, session_key).await else {
        return false;
    };
    match stored.binding {
        Some(binding) => binding.user_id == user_id && binding.is_current(db).await,
        None => false,
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
