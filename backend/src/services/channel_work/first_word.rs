//! Her writing first to someone she is paired with.

use super::*;

/// Platforms where she can write first: a bot there may start a message.
/// QQ only answers a message it received; Feishu waits for later.
pub(super) const FIRST_WORD_PLATFORMS: [ChannelPlatform; 2] =
    [ChannelPlatform::Telegram, ChannelPlatform::Discord];

/// People she could write to first: paired, with a private chat she has had
/// with them on a platform where she can.
pub(crate) async fn reachable_people(db: &DatabaseConnection) -> Vec<i32> {
    let mut people = std::collections::BTreeSet::new();
    for platform in FIRST_WORD_PLATFORMS {
        for row in shared_registry::list(db, platform.session_ns(), None, None)
            .await
            .unwrap_or_default()
        {
            if let Ok(stored) = serde_json::from_value::<StoredSession>(row.payload) {
                if let (Some(binding), Some(_)) = (stored.binding, stored.address) {
                    people.insert(binding.user_id);
                }
            }
        }
    }
    people.into_iter().collect()
}

/// Her first line to a paired person, in the private chat they last used
/// with her: sent, and kept in her chat with them there, so their answer
/// carries on from it. Where it went, if it went.
pub(crate) async fn say_first(
    db: &DatabaseConnection,
    user_id: i32,
    text: &str,
) -> Option<ChannelPlatform> {
    for platform in FIRST_WORD_PLATFORMS {
        let rows = shared_registry::list(db, platform.session_ns(), Some(user_id), None)
            .await
            .unwrap_or_default();
        // Newest last.
        for row in rows.into_iter().rev() {
            let Ok(stored) = serde_json::from_value::<StoredSession>(row.payload) else {
                continue;
            };
            let (Some(binding), Some(address)) = (stored.binding, stored.address) else {
                continue;
            };
            if binding.user_id != user_id || !binding.is_current(db).await {
                continue;
            }
            let Some(transport) = address.connect(db).await else {
                continue;
            };
            let sink = ChannelSink {
                transport,
                binding,
                db: db.clone(),
            };
            let session_key = row.record_id;
            let sent = runtime::with_chat_lock(&session_key, async {
                for chunk in split_channel_text(text, sink.text_limit()) {
                    if sink.send_text(&chunk).await.is_err() {
                        return false;
                    }
                }
                true
            })
            .await;
            if !sent {
                return None;
            }
            if let Some(chat) = chat::chat_session(db, platform, user_id, &session_key).await {
                if let Err(error) = crate::services::agent::sessions::persist_assistant_message(
                    db,
                    &chat,
                    None,
                    text,
                    Some(serde_json::json!({ "unprompted": true })),
                )
                .await
                {
                    warn!(%error, "her first line was sent but not kept in the chat");
                }
            }
            return Some(platform);
        }
    }
    None
}
