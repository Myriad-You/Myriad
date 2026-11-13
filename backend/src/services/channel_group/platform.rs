//! Talking to the platforms: sending her reply and the typing mark, and looking up who is paired.

use super::*;

/// Deliver one chunk of her reply in the group, as a reply to `line`, with
/// her `@name`s as the platform's mentions.
pub(super) async fn send_reply(
    line: &GroupLine,
    token: &str,
    pieces: &[myriad_agent_rules::mentions::Piece],
    quoting: bool,
) -> Result<(), ConnectFailureKind> {
    use myriad_agent_rules::mentions;
    #[cfg(test)]
    if g0::on() {
        let pieces: Vec<serde_json::Value> = pieces
            .iter()
            .map(|piece| match piece {
                mentions::Piece::Text(text) => serde_json::json!({ "text": text }),
                mentions::Piece::Mention { id, name } => {
                    serde_json::json!({ "mention": id, "name": name })
                }
            })
            .collect();
        let quoted = quoting.then(|| line.message_id.clone());
        return if g0::send(
            "send",
            serde_json::json!({ "venue": line.venue(), "thread": line.thread, "pieces": pieces, "quoted": quoted }),
        ) {
            Ok(())
        } else {
            Err(ConnectFailureKind::Transient)
        };
    }
    match line.platform {
        ChannelPlatform::Telegram => {
            let Ok(chat) = line.chat.parse() else {
                return Err(ConnectFailureKind::Permanent);
            };
            let quoted = match quoting.then(|| line.message_id.parse()) {
                Some(Ok(message_id)) => Some(message_id),
                Some(Err(_)) => return Err(ConnectFailureKind::Permanent),
                None => None,
            };
            let (text, entities) = mentions::telegram_text_and_entities(pieces);
            crate::services::telegram_bot::send_group_reply(
                token,
                chat,
                &text,
                &entities,
                quoted,
                line.thread,
            )
            .await
        }
        ChannelPlatform::Discord => {
            let (content, users) = mentions::discord_content_and_users(pieces);
            crate::services::discord_bot::send_group_reply(
                token,
                &line.chat,
                &content,
                &users,
                quoting.then_some(line.message_id.as_str()),
            )
            .await
        }
        ChannelPlatform::OneBot => {
            use myriad_agent_rules::onebot::encode::{encode_group_message, encode_reply_segment};
            let mut segments = Vec::new();
            if quoting {
                // A line whose id cannot be quoted is still answered, unquoted.
                segments.extend(encode_reply_segment(&line.message_id));
            }
            segments.extend(mentions::onebot_segments(pieces));
            let Some(action) = encode_group_message(&line.chat, &segments) else {
                return Err(ConnectFailureKind::Permanent);
            };
            match crate::services::onebot_send::send_action(action).await {
                Ok(None) => Ok(()),
                Ok(Some(kind)) => Err(kind),
                Err(_) => Err(ConnectFailureKind::Transient),
            }
        }
        ChannelPlatform::Qq => {
            // QQ's group API has no segment model here; a mention stays the
            // name people know, not a platform ping (no everyone form).
            let text: String = pieces
                .iter()
                .map(|piece| match piece {
                    mentions::Piece::Text(text) => text.clone(),
                    mentions::Piece::Mention { name, .. } => format!("@{name}"),
                })
                .collect();
            if text.trim().is_empty() {
                return Ok(());
            }
            let Ok(db) = db() else {
                return Err(ConnectFailureKind::Transient);
            };
            let auth = crate::services::qq_bot::outbound_auth_header()
                .await
                .map_err(|_| ConnectFailureKind::Transient)?;
            let msg_id = (!line.message_id.is_empty()).then_some(line.message_id.as_str());
            crate::services::qq_work::send_group(&db, &auth, &line.chat, &text, msg_id).await
        }
        ChannelPlatform::Feishu => Err(ConnectFailureKind::Permanent),
    }
}

/// Whisper `text` to `user` on `platform` (in private, where only they see
/// it); whether it reached them. A Discord member is written to in the DM
/// channel opened with them; a Telegram user who never started a chat with
/// her, a Discord member who keeps server DMs closed, a QQ user who is not
/// her friend, cannot be reached (the platform refuses).
pub(super) async fn whisper(
    token: &str,
    platform: &str,
    user: &str,
    text: &str,
    choices: &[String],
    wait: u64,
) -> bool {
    #[cfg(test)]
    let who = format!("{platform}:{user}");
    let reached = match platform_of(platform) {
        Some(ChannelPlatform::Telegram) => {
            let markup = telegram_buttons(choices, wait);
            #[cfg(test)]
            if g0::on() {
                return g0::outbound(
                    "telegram.sendMessage",
                    serde_json::json!({ "chat_id": user, "text": text, "reply_markup": markup }),
                    &who,
                );
            }
            crate::services::telegram_bot::send_outbound(token, user, text, markup)
                .await
                .is_ok()
        }
        Some(ChannelPlatform::Discord) => {
            #[cfg(test)]
            let opened = if g0::on() {
                g0::note(
                    "discord.openDm",
                    serde_json::json!({ "recipient_id": user }),
                );
                Ok(format!("dm-{user}"))
            } else {
                crate::services::discord_bot::open_dm(token, user).await
            };
            #[cfg(not(test))]
            let opened = crate::services::discord_bot::open_dm(token, user).await;
            match opened {
                Ok(channel) => {
                    let components = discord_buttons(choices, wait);
                    #[cfg(test)]
                    if g0::on() {
                        return g0::outbound(
                            "discord.send",
                            serde_json::json!({ "channel": channel, "content": text, "components": components }),
                            &who,
                        );
                    }
                    crate::services::discord_bot::send_outbound(token, &channel, text, components)
                        .await
                        .is_ok()
                }
                Err(_) => false,
            }
        }
        Some(ChannelPlatform::OneBot) => {
            use myriad_agent_rules::onebot::encode;
            match encode::encode_private_message(user, &[encode::encode_text_segment(text)]) {
                Some(action) => {
                    #[cfg(test)]
                    if g0::on() {
                        return g0::outbound("onebot.action", action, &who);
                    }
                    matches!(
                        crate::services::onebot_send::send_action(action).await,
                        Ok(None)
                    )
                }
                None => false,
            }
        }
        _ => false,
    };
    if !reached {
        warn!(platform, "[Group] a whisper did not reach them");
    }
    reached
}

/// What a pressed button of a game's sends back (`seat:<wait>:<number>`):
/// the wait it was for, and the number of the choice; else none. A button
/// sent before buttons carried their wait (`seat:<number>`) has none.
pub(super) fn seat_button(data: &str) -> Option<(Option<u64>, String)> {
    let rest = data.strip_prefix(SEAT_BUTTON)?;
    let (wait, number) = match rest.split_once(':') {
        Some((wait, number)) => (Some(wait.parse().ok()?), number),
        None => (None, rest),
    };
    number.parse::<usize>().ok()?;
    Some((wait, number.to_string()))
}

const SEAT_BUTTON: &str = "seat:";

/// A game's choices as Telegram inline buttons, one a row; none without.
pub(super) fn telegram_buttons(choices: &[String], wait: u64) -> Option<serde_json::Value> {
    (!choices.is_empty()).then(|| {
        let rows: Vec<serde_json::Value> = choices
            .iter()
            .enumerate()
            .map(|(index, choice)| {
                serde_json::json!([{
                    "text": choice.chars().take(60).collect::<String>(),
                    "callback_data": format!("{SEAT_BUTTON}{wait}:{index}"),
                }])
            })
            .collect();
        serde_json::json!({ "inline_keyboard": rows })
    })
}

/// A game's choices as Discord buttons, five a row (at most five rows);
/// none without.
pub(super) fn discord_buttons(choices: &[String], wait: u64) -> Option<serde_json::Value> {
    (!choices.is_empty()).then(|| {
        let buttons: Vec<serde_json::Value> = choices
            .iter()
            .take(25)
            .enumerate()
            .map(|(index, choice)| {
                serde_json::json!({
                    "type": 2,
                    "style": 1,
                    "label": choice.chars().take(80).collect::<String>(),
                    "custom_id": format!("{SEAT_BUTTON}{wait}:{index}"),
                })
            })
            .collect();
        let rows: Vec<serde_json::Value> = buttons
            .chunks(5)
            .map(|row| serde_json::json!({ "type": 1, "components": row }))
            .collect();
        serde_json::Value::Array(rows)
    })
}

pub(super) async fn send_typing(line: &GroupLine, token: &str) {
    #[cfg(test)]
    if g0::on() {
        g0::send("typing", serde_json::json!({ "venue": line.venue() }));
        return;
    }
    match line.platform {
        ChannelPlatform::Telegram => {
            let _ = crate::services::telegram_bot::send_typing(token, &line.chat).await;
        }
        ChannelPlatform::Discord => {
            let _ = crate::services::discord_bot::send_typing(token, &line.chat).await;
        }
        ChannelPlatform::OneBot => {
            // SnowLuma's `set_input_status` covers friend chats only (it
            // takes a user id and sends chat type 0); a group has no typing
            // action to call.
        }
        ChannelPlatform::Qq | ChannelPlatform::Feishu => {}
    }
}

/// Put her reaction (one of `joining::REACTIONS`) on `line`. Whether the
/// platform took it.
pub(super) async fn react(line: &GroupLine, token: &str, emoji: &str) -> bool {
    #[cfg(test)]
    if g0::on() {
        return g0::send(
            "react",
            serde_json::json!({ "venue": line.venue(), "messageId": line.message_id, "emoji": emoji }),
        );
    }
    let done = match line.platform {
        ChannelPlatform::Telegram => match (line.chat.parse(), line.message_id.parse()) {
            (Ok(chat), Ok(message_id)) => {
                crate::services::telegram_bot::set_reaction(token, chat, message_id, emoji).await
            }
            _ => Err(ConnectFailureKind::Permanent),
        },
        ChannelPlatform::Discord => {
            crate::services::discord_bot::add_reaction(token, &line.chat, &line.message_id, emoji)
                .await
        }
        ChannelPlatform::OneBot => {
            match myriad_agent_rules::onebot::encode::encode_set_msg_emoji_like(
                &line.message_id,
                emoji,
            ) {
                Some(action) => match crate::services::onebot_send::send_action(action).await {
                    Ok(None) => Ok(()),
                    Ok(Some(kind)) => Err(kind),
                    Err(_) => Err(ConnectFailureKind::Transient),
                },
                None => Err(ConnectFailureKind::Permanent),
            }
        }
        ChannelPlatform::Qq | ChannelPlatform::Feishu => Err(ConnectFailureKind::Permanent),
    };
    if let Err(kind) = &done {
        warn!(venue = %line.venue(), ?kind, "[Group] her reaction did not go");
    }
    done.is_ok()
}

/// Send the sticker she chose into the group, making it first if it is new;
/// what it means, once it went.
pub(super) async fn send_sticker(
    message: &GroupLine,
    token: &str,
    chosen: &serde_json::Value,
) -> Option<String> {
    use crate::services::agent::merope::group::stickers;
    #[cfg(test)]
    if g0::on() {
        if chosen.get("make").is_some() {
            send_typing(message, token).await;
        }
        if g0::send(
            "sticker",
            serde_json::json!({ "venue": message.venue(), "chosen": chosen }),
        ) {
            let meaning = chosen
                .get("meaning")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("贴纸");
            return Some(meaning.to_string());
        }
        return None;
    }
    let db = db().ok()?;
    if chosen.get("make").is_some() {
        send_typing(message, token).await;
    }
    let sticker = stickers::resolve(&db, chosen, None).await?;
    let (png, _) = stickers::picture(&sticker).await?;
    let prepared = crate::services::sticker_send::prepare(png, message.platform).await?;
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
        Some(sticker.meaning)
    } else {
        warn!(venue = %message.venue(), "[Group] sticker not sent");
        None
    }
}
