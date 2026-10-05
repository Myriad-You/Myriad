//! Her writing first to someone she is paired with.

use super::*;

/// Platforms where she can write first (see
/// [`ChannelPlatform::may_send_unasked`]).
fn first_word_platforms() -> impl Iterator<Item = ChannelPlatform> {
    ChannelPlatform::ALL
        .into_iter()
        .filter(|platform| platform.may_send_unasked())
}

/// People she could write to first: paired, with a private chat she has had
/// with them on a platform where she can.
pub(crate) async fn reachable_people(db: &DatabaseConnection) -> Vec<i32> {
    let mut people = std::collections::BTreeSet::new();
    for platform in first_word_platforms() {
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
    // The private chat they used with her last, on any platform she may
    // write first on.
    for (platform, row) in recent_chats(db, user_id).await {
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
    None
}

/// Their private chats with her on platforms she may write first on, the
/// one used last first.
async fn recent_chats(
    db: &DatabaseConnection,
    user_id: i32,
) -> Vec<(ChannelPlatform, shared_registry::RegistryRow)> {
    let platforms: Vec<ChannelPlatform> = first_word_platforms().collect();
    // Namespaces are this enum's constants, never input.
    let namespaces = platforms
        .iter()
        .map(|platform| format!("'{}'", platform.session_ns()))
        .collect::<Vec<_>>()
        .join(", ");
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "SELECT namespace, record_id, runtime_id, payload FROM runtime_registry \
                 WHERE namespace IN ({namespaces}) AND subject_id = $1 \
                   AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT \
                 ORDER BY updated_at DESC"
            ),
            [user_id.into()],
        ))
        .await
        .unwrap_or_default();
    rows.into_iter()
        .filter_map(|row| {
            let namespace: String = row.try_get("", "namespace").ok()?;
            let platform = platforms
                .iter()
                .copied()
                .find(|platform| platform.session_ns() == namespace)?;
            Some((
                platform,
                shared_registry::RegistryRow {
                    record_id: row.try_get("", "record_id").ok()?,
                    runtime_id: row.try_get("", "runtime_id").ok()?,
                    payload: row.try_get("", "payload").ok()?,
                },
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writing first and answering later read one table: a platform where
    /// she may answer when she wakes is one she may write first on.
    #[test]
    fn she_writes_first_wherever_she_may_send_unasked() {
        let first: Vec<ChannelPlatform> = first_word_platforms().collect();
        assert_eq!(
            first,
            [
                ChannelPlatform::Telegram,
                ChannelPlatform::Discord,
                ChannelPlatform::OneBot
            ]
        );
        for platform in ChannelPlatform::ALL {
            assert_eq!(
                first.contains(&platform),
                super::chat_waiting::can_answer_later(platform),
                "{platform:?}"
            );
        }
    }
}
