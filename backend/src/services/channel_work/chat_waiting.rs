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

tokio::task_local! {
    /// The arriving record of the message being handled: what it is known
    /// by once it waits, so a restart takes it back once.
    pub(super) static ARRIVING: String;
}

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
    /// The arriving records already in `texts`.
    #[serde(default)]
    arrived: Vec<String>,
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
            arrived: seen.arrived.clone(),
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

type ArrivingByChat = std::collections::BTreeMap<String, Vec<(String, Arriving)>>;

/// What was arriving in each chat when the site stopped, in the order it came.
async fn arriving_by_chat(db: &DatabaseConnection) -> ArrivingByChat {
    let mut rows = shared_registry::list(db, ARRIVING_NS, None, None)
        .await
        .unwrap_or_default();
    rows.sort_by(|a, b| a.record_id.cmp(&b.record_id));
    let mut by_chat = ArrivingByChat::default();
    for row in rows {
        match serde_json::from_value::<Arriving>(row.payload) {
            Ok(kept) => by_chat
                .entry(kept.session_key.clone())
                .or_default()
                .push((row.record_id, kept)),
            Err(_) => arrived(db, &row.record_id).await,
        }
    }
    by_chat
}

/// After a restart: what was arriving in a chat comes in again, in the order
/// it came.
async fn take_back_arriving(
    db: DatabaseConnection,
    session_key: String,
    kept: Vec<(String, Arriving)>,
) {
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
                ARRIVING.scope(
                    record.clone(),
                    super::inbound::continue_text(
                        &db,
                        kept.user_id,
                        "",
                        &kept.text,
                        &images,
                        &session_key,
                        sink,
                    ),
                ),
            )
            .await;
        }
        arrived(&db, &record).await;
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
    // What was arriving goes after what waited in the same chat; what had
    // already reached the waiting record when the site stopped comes in
    // once. Known by its record, not its words: the same `?` sent again, or
    // two pictures without words, are two messages.
    let mut arriving = arriving_by_chat(db).await;
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
        let mut later = arriving.remove(&session_key).unwrap_or_default();
        for (record, _) in &later {
            if waiting.arrived.contains(record) {
                arrived(db, record).await;
            }
        }
        later.retain(|(record, _)| !waiting.arrived.contains(record));
        let texts = waiting.texts.join("\n");
        let (db, user_id, work_session_id) = (db.clone(), waiting.user_id, waiting.work_session_id);
        // She is not on the site: what was waiting is held for when she is
        // back, never started and refused (D3).
        if super::held::away() {
            super::held::hold(&db, &sink, &session_key, user_id, &texts, &images).await;
            forget_waiting(&db, &session_key).await;
            if !later.is_empty() {
                arriving.insert(session_key, later);
            }
            continue;
        }
        tracing::info!("[Channel] what she had not answered, waited on again after a restart");
        tokio::spawn(async move {
            start_chat_turn(
                db.clone(),
                user_id,
                work_session_id,
                &texts,
                &images,
                sink,
                &session_key,
            )
            .await;
            take_back_arriving(db, session_key, later).await;
        });
    }
    for (session_key, kept) in arriving {
        tokio::spawn(take_back_arriving(db.clone(), session_key, kept));
    }
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
            arrived: vec!["tg:1:0001:a".into()],
        })
        .unwrap();
        let back: Waiting = serde_json::from_value(kept.clone()).unwrap();
        assert_eq!(back.texts, ["在吗", "睡了？"]);
        assert_eq!(back.images[0].url, "tg:file");
        assert_eq!(back.arrived, ["tg:1:0001:a"]);
        // One kept before records were: none.
        let mut before = kept;
        before.as_object_mut().unwrap().remove("arrived");
        let back: Waiting = serde_json::from_value(before).unwrap();
        assert!(back.arrived.is_empty());
    }
}
