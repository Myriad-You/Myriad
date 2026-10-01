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
            let Some(action) = myriad_agent_rules::onebot::encode::encode_group_message(
                &line.chat,
                &mentions::onebot_segments(pieces),
            ) else {
                return Err(ConnectFailureKind::Permanent);
            };
            match crate::services::onebot_send::send_action(action).await {
                Ok(None) => Ok(()),
                Ok(Some(kind)) => Err(kind),
                Err(_) => Err(ConnectFailureKind::Transient),
            }
        }
        ChannelPlatform::Qq | ChannelPlatform::Feishu => Err(ConnectFailureKind::Permanent),
    }
}

pub(super) async fn send_typing(line: &GroupLine, token: &str) {
    match line.platform {
        ChannelPlatform::Telegram => {
            let _ = crate::services::telegram_bot::send_typing(token, &line.chat).await;
        }
        ChannelPlatform::Discord => {
            let _ = crate::services::discord_bot::send_typing(token, &line.chat).await;
        }
        ChannelPlatform::OneBot => {
            // Group typing is not a documented NapCat action. Private typing
            // uses `set_input_status` with a user id, which a group line has
            // no reason to poke.
        }
        ChannelPlatform::Qq | ChannelPlatform::Feishu => {}
    }
}

/// Put her reaction (one of `joining::REACTIONS`) on `line`. Whether the
/// platform took it.
pub(super) async fn react(line: &GroupLine, token: &str, emoji: &str) -> bool {
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

pub(super) fn text_limit(platform: ChannelPlatform) -> usize {
    match platform {
        ChannelPlatform::Discord => myriad_agent_rules::channel::DISCORD_TEXT_LIMIT,
        _ => myriad_agent_rules::channel::TELEGRAM_TEXT_LIMIT,
    }
}

/// Her reply without a （回复 …） mark she copied from the transcript: the
/// platform already shows what she replies to.
pub(super) fn without_reply_mark(reply: &str) -> String {
    let trimmed = reply.trim_start();
    if let Some(rest) = trimmed.strip_prefix("（回复") {
        if let Some(end) = rest
            .find('）')
            .filter(|end| rest[..*end].chars().count() <= 200)
        {
            return rest[end + '）'.len_utf8()..].trim_start().to_string();
        }
    }
    reply.to_string()
}

/// Her reply, chunk by chunk; whether any of it reached the group.
pub(super) async fn deliver(line: &GroupLine, token: &str, reply: &str, began: Instant) -> bool {
    use crate::services::agent::merope::group::timing::typing;
    use myriad_agent_rules::channel::{as_messages, split_channel_text};
    let venue = line.venue();
    let people = people(&venue);
    let room = room(&venue);
    let mut sent = false;
    // Most turns go as one message, and a few in a row when something grabs
    // her: typing each before it goes, typed the way people there type (or
    // chat apps usually do, until the group has said enough), and only the
    // first quoting the line she answers.
    let lines = as_messages(reply);
    let most = myriad_merope::talk_shape::messages_this_turn(lines.len(), rand::random::<f64>());
    for (index, message) in myriad_merope::talk_shape::goes_out_as(&lines, most, room.as_ref())
        .into_iter()
        .enumerate()
    {
        // Typing it takes as long as it takes; the first she was already
        // at while she thought.
        let left = if index == 0 {
            typing(&message).saturating_sub(began.elapsed())
        } else {
            typing(&message)
        };
        if !left.is_zero() {
            send_typing(line, token).await;
            tokio::time::sleep(left).await;
        }
        for chunk in split_channel_text(&message, text_limit(line.platform)) {
            let pieces = myriad_agent_rules::mentions::split_mentions(&chunk, &people);
            // A line with no message behind it (her saying something first)
            // quotes nothing.
            match send_reply(line, token, &pieces, !sent && !line.message_id.is_empty()).await {
                Ok(()) => {
                    // How long whoever called her waited for her first words.
                    let waited = (!sent && line.addressed)
                        .then(|| said_ago(line))
                        .flatten()
                        .map(|age| age.num_milliseconds() as f64 / 1000.0);
                    let typed = myriad_merope::talk_shape::typed_by(
                        HER,
                        chrono::Utc::now().timestamp(),
                        &chunk,
                    );
                    note_said(&venue, typed, waited, Some(&chunk));
                    sent = true;
                }
                Err(kind) => {
                    warn!(?kind, venue = %line.venue(), "[Group] reply not sent");
                    keep_ledger(&venue).await;
                    return sent;
                }
            }
        }
    }
    keep_ledger(&venue).await;
    sent
}

pub(super) async fn lookup(db: &DatabaseConnection, line: &GroupLine) -> Option<PairingLookup> {
    crate::services::channel_pairing::lookup_openid(
        db,
        PairingChannel::of(line.platform),
        &line.from,
    )
    .await
    .ok()
}
