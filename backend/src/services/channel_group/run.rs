//! Running a turn: a member's turn under their account, or a light answer to anyone else.

use super::*;

/// Answer someone from outside the community, with little context, on the
/// site owner's budget. `why` is why she speaks when they did not call her.
pub(super) async fn answer_stranger(
    db: &DatabaseConnection,
    message: &GroupLine,
    token: &str,
    why: Option<&str>,
) -> bool {
    let venue = message.venue();
    let within = with_group(&venue, |group| {
        count_today(&mut group.stranger_replies, STRANGER_REPLIES_PER_DAY)
    })
    .unwrap_or(false);
    if !within {
        info!(%venue, "[Group] enough replies to outsiders today");
        return false;
    }
    let Ok(owner) = crate::services::site_owner::site_owner_user_id(db).await else {
        return false;
    };
    let stranger = crate::services::agent::merope::strangers::Stranger {
        who: format!("{}:{}", message.platform.slug(), message.from),
        name: message.display_name.chars().take(40).collect(),
    };
    let reading: Vec<String> = [
        room(&venue).map(|room| myriad_merope::talk_shape::describe(&room, "How people type here")),
        how_she_differs(&venue).await,
        make_sense(db, message).await,
    ]
    .into_iter()
    .flatten()
    .collect();
    send_typing(message, token).await;
    let began = Instant::now();
    let transcript = transcript(&venue, Some(&message.message_id));
    let Ok(Some((reply, sticker))) = tokio::time::timeout(
        TURN_DEADLINE,
        crate::services::agent::merope::strangers::reply(
            db,
            owner,
            &venue,
            &stranger,
            &transcript,
            &said_now(message),
            why,
            &reading,
        ),
    )
    .await
    else {
        return false;
    };
    let reply = without_reply_mark(&reply);
    let sent = say_and_send(message, token, &reply, sticker, began).await;
    if sent {
        answered(message);
        crate::services::agent::merope::strangers::enqueue_after(
            db,
            owner,
            venue,
            stranger,
            said_now(message),
            reply,
            &message.message_id,
        )
        .await;
    }
    sent
}

pub(super) async fn current_binding(
    db: &DatabaseConnection,
    message: &GroupLine,
    user_id: i32,
) -> Option<ChannelBinding> {
    let binding = ChannelBinding::resolve(db, message.platform, user_id, &message.from)
        .await
        .ok()
        .flatten()?;
    binding.is_current(db).await.then_some(binding)
}

pub(super) async fn run_turn(
    db: &DatabaseConnection,
    message: &GroupLine,
    user_id: i32,
    token: &str,
    chime: Option<String>,
    first: bool,
) -> Option<(String, Option<serde_json::Value>)> {
    crate::services::principal::current_roles(db, user_id)
        .await
        .ok()??;
    let venue = message.venue();
    let key = (venue.clone(), user_id);
    let mut known = SESSIONS
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(&key).cloned());
    // After a restart: the one they talked to her in here before, not a
    // new one each time.
    if known.is_none() {
        known = last_group_session(db, user_id, &venue).await;
    }
    // Made as the group's from the start: never read back as a private one.
    let session_id = crate::services::agent::sessions::ensure_session_in(
        db,
        known.as_deref(),
        user_id,
        AgentInteractionMode::Chat,
        Some(&venue),
    )
    .await
    .ok()?;
    if let Ok(mut sessions) = SESSIONS.lock() {
        sessions.insert(key, session_id.clone());
    }
    let run = crate::services::agent::run::start_for_user(
        db.clone(),
        user_id,
        crate::services::agent::run::ProcessRequest {
            input: said_now(message),
            context: Some(crate::services::agent::run::ProcessContext {
                mode: Some(AgentInteractionMode::Chat),
                session_id: Some(session_id),
                group: Some(crate::services::agent::run::GroupTurn {
                    transcript: transcript(&venue, (!first).then_some(message.message_id.as_str())),
                    room: room(&venue),
                    differs: how_she_differs(&venue).await,
                    // Saying something first, there is no line of theirs
                    // to make sense of or to be late for.
                    making_sense: if first {
                        None
                    } else {
                        make_sense(db, message).await
                    },
                    late: if first { None } else { seen_late(message) },
                    venue,
                    chime,
                    speaker: message.display_name.clone(),
                }),
                ..Default::default()
            }),
        },
    )
    .await
    .ok()?;
    let mut events = Box::pin(crate::services::agent::run::agent_run_envelopes(run));
    let mut typing = tokio::time::interval(TYPING_EVERY);
    let deadline = tokio::time::sleep(TURN_DEADLINE);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => {
                warn!(venue = %message.venue(), "[Group] no reply in time; she says nothing");
                return None;
            }
            _ = typing.tick() => send_typing(message, token).await,
            envelope = events.next() => {
                match envelope?.event {
                    AgentProgressEvent::TaskCompleted { success, response, .. } => {
                        // A superseded or failed turn says nothing in the group.
                        if !success {
                            info!(venue = %message.venue(), "[Group] turn superseded or failed; she says nothing");
                            return None;
                        }
                        let text = response
                            .get("message")
                            .and_then(|value| value.as_str())
                            .map(str::trim)
                            .unwrap_or_default()
                            .to_string();
                        let sticker = response.pointer("/data/sticker").cloned();
                        return (!text.is_empty() || sticker.is_some()).then_some((text, sticker));
                    }
                    AgentProgressEvent::Error { .. } => {
                        warn!(venue = %message.venue(), "[Group] her turn failed; she says nothing");
                        return None;
                    }
                    _ => {}
                }
            }
        }
    }
}
