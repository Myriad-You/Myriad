//! Answering a line: the reply, stickers, and not answering nonstop.

use super::*;

/// Before answering `message`, she reads the talk: what is going on, what
/// it means, what people are telling her about herself (kept, see
/// `merope::making_sense`). With what the group told her before, as the
/// sections her reply starts from.
pub(super) async fn make_sense(db: &DatabaseConnection, message: &GroupLine) -> Option<String> {
    use crate::services::agent::merope::group::making_sense;
    let venue = message.venue();
    let stored = unified_venue(&venue);
    let owner = crate::services::site_owner::site_owner_user_id(db)
        .await
        .ok()?;
    let lines = transcript(&venue, Some(&message.message_id));
    let said = said_now(message);
    let mut conversation: Vec<String> = lines
        .iter()
        .skip(lines.len().saturating_sub(CONVERSATION_LINES))
        .map(|line| {
            if line.role == "assistant" {
                format!("you：{}", line.content)
            } else {
                line.content.clone()
            }
        })
        .collect();
    conversation.push(format!("{}：{said}", message.display_name));
    let sense = making_sense::read(owner, &conversation).await;
    if let Some(told) = sense.as_ref().and_then(|sense| sense.about_you.as_deref()) {
        making_sense::remember_told(db, &stored, told, &said).await;
    }
    let sections: Vec<String> = [
        making_sense::told_section(db, &stored).await,
        sense.as_ref().map(myriad_merope::making_sense::section),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!sections.is_empty()).then(|| sections.join("\n\n"))
}

/// The session someone last talked to her in, in this group.
pub(super) async fn last_group_session(
    db: &DatabaseConnection,
    user_id: i32,
    venue: &str,
) -> Option<String> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT id FROM agent_sessions WHERE user_id = $1 AND context->>'venue' = $2 \
           AND context->>'mode' = 'chat' ORDER BY last_active_at DESC LIMIT 1",
        [user_id.into(), venue.into()],
    ))
    .await
    .ok()
    .flatten()?
    .try_get::<String>("", "id")
    .ok()
}

/// Whether she stopped answering whoever wrote this line (see `LOOP_ROUNDS`).
pub(super) fn stopped_answering(message: &GroupLine) -> bool {
    with_group(&message.venue(), |group| {
        group.paused.retain(|_, since| since.elapsed() < LOOP_PAUSE);
        group.paused.contains_key(&message.from)
    })
    .unwrap_or(false)
}

/// She answered whoever wrote this line; after too many rounds with them
/// too fast, she stops answering them for a while.
pub(super) fn answered(message: &GroupLine) {
    let venue = message.venue();
    with_group(&venue, |group| {
        group
            .answered
            .push_back((message.from.clone(), Instant::now()));
        while group.answered.len() > LOOP_ROUNDS {
            group.answered.pop_front();
        }
        let looping = group.answered.len() == LOOP_ROUNDS
            && group.answered.iter().all(|(from, _)| *from == message.from)
            && group
                .answered
                .front()
                .is_some_and(|(_, at)| at.elapsed() < LOOP_WINDOW);
        if looping {
            warn!(%venue, "[Group] answering one sender nonstop; she stops for a while");
            group.paused.insert(message.from.clone(), Instant::now());
            group.answered.clear();
        }
    });
}

pub(super) async fn answer(message: &GroupLine, token: &str, chime: Option<String>) -> bool {
    let Ok(db) = crate::services::process_db::database() else {
        return false;
    };
    if stopped_answering(message) {
        return false;
    }
    if is_muted(&message.venue()) {
        info!(venue = %message.venue(), "[Group] muted there; she says nothing");
        return false;
    }
    let inbound_id = format!("group:{}:{}", message.chat, message.message_id);
    if !crate::services::channel_work::claim_inbound(&db, message.platform, None, &inbound_id).await
    {
        return false;
    }
    see(&message.venue(), token).await;
    let user_id = match lookup(&db, message).await {
        Some(PairingLookup::Paired { user_id }) => user_id,
        // Someone from outside the community: answered lightly.
        Some(_) => return answer_stranger(&db, message, token, chime.as_deref()).await,
        _ => return false,
    };
    let Some(binding) = current_binding(&db, message, user_id).await else {
        return false;
    };
    let began = Instant::now();
    let Some((reply, sticker)) = run_turn(&db, message, user_id, token, chime, false).await else {
        return false;
    };
    // Unpaired or switched off while she was thinking: say nothing.
    if !binding.is_current(&db).await {
        return false;
    }
    let reply = without_reply_mark(&reply);
    let sent = say_and_send(message, token, &reply, sticker, began).await;
    if sent {
        answered(message);
    }
    sent
}

/// Her words, if any, and the sticker she chose, if any: the sticker after
/// the words; one she is making, once it is made. Whether anything went.
pub(super) async fn say_and_send(
    message: &GroupLine,
    token: &str,
    reply: &str,
    sticker: Option<serde_json::Value>,
    began: Instant,
) -> bool {
    let venue = message.venue();
    let mut sent = false;
    if !reply.trim().is_empty() {
        sent = deliver(message, token, reply, began).await;
        if sent {
            record_hers(&venue, reply).await;
        }
    }
    if let Some(chosen) = sticker {
        let (message, token) = (message.clone(), token.to_string());
        tokio::spawn(async move {
            send_sticker(&message, &token, &chosen).await;
        });
        sent = true;
    }
    sent
}

/// Send the sticker she chose into the group, making it first if it is new.
pub(super) async fn send_sticker(message: &GroupLine, token: &str, chosen: &serde_json::Value) {
    use crate::services::agent::merope::group::stickers;
    let Ok(db) = crate::services::process_db::database() else {
        return;
    };
    if chosen.get("make").is_some() {
        send_typing(message, token).await;
    }
    let Some(sticker) = stickers::resolve(&db, chosen, None).await else {
        return;
    };
    let Some((png, _)) = stickers::picture(&sticker).await else {
        return;
    };
    let Some(prepared) = crate::services::sticker_send::prepare(png, message.platform).await else {
        return;
    };
    let sent = match message.platform {
        ChannelPlatform::Telegram => crate::services::telegram_bot::send_sticker(
            token,
            &message.chat,
            &prepared.bytes,
            message.thread,
        )
        .await
        .is_ok(),
        ChannelPlatform::Discord => crate::services::discord_bot::send_photo(
            token,
            &message.chat,
            &prepared.bytes,
            prepared.mime,
            None,
        )
        .await
        .is_ok(),
        ChannelPlatform::OneBot => {
            use base64::Engine as _;
            let inline = format!(
                "base64://{}",
                base64::engine::general_purpose::STANDARD.encode(&prepared.bytes)
            );
            match myriad_agent_rules::onebot::encode::encode_group_message(
                &message.chat,
                &[myriad_agent_rules::onebot::encode::encode_image_segment(
                    &inline,
                )],
            ) {
                Some(action) => matches!(
                    crate::services::onebot_send::send_action(action).await,
                    Ok(None)
                ),
                None => false,
            }
        }
        ChannelPlatform::Qq | ChannelPlatform::Feishu => false,
    };
    if sent {
        stickers::sent(&db, &sticker).await;
        // The group sees she sent it, and so does she.
        record_hers(
            &message.venue(),
            &format!("（表情包：{}）", sticker.meaning),
        )
        .await;
    } else {
        warn!(venue = %message.venue(), "[Group] sticker not sent");
    }
}
