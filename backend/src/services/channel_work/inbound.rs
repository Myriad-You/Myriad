//! A message or button press coming in: claimed once, then answered as chat, a command, or the reply a pending prompt waits for.

use super::*;

/// Claim one inbound message id; `false` means it was already handled (or
/// the claim could not be recorded, which is treated the same way).
pub(crate) async fn claim_inbound(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    user_id: Option<i32>,
    inbound_id: &str,
) -> bool {
    match shared_registry::put_if_absent(
        db,
        platform.inbound_ns(),
        inbound_id,
        user_id.map_or(
            RegistryIdentity {
                subject_id: None,
                owner_id: None,
                tapp_id: None,
                runtime_id: None,
            },
            identity,
        ),
        &Value::Bool(true),
        (crate::clock::now() + ChronoDuration::hours(24)).timestamp(),
    )
    .await
    {
        Ok(true) => true,
        Ok(false) => {
            info!(inbound_id, %platform, "channel duplicate inbound ignored");
            false
        }
        Err(error) => {
            warn!(%error, %platform, "channel duplicate check failed");
            false
        }
    }
}

pub async fn handle_text_with_images(
    db: &DatabaseConnection,
    user_id: i32,
    sender_id: &str,
    chat_id: &str,
    input: &str,
    images: &[ChannelImageRef],
    session_key: &str,
    transport: ChannelTransport,
) {
    // The private-text entry has already claimed this message id.
    let platform = transport.platform();
    let binding = match ChannelBinding::resolve(db, platform, user_id, sender_id).await {
        Ok(Some(binding)) => binding,
        _ => return,
    };
    let session_key = binding.session_key(session_key);
    let sink = ChannelSink {
        transport,
        binding,
        db: db.clone(),
    };
    let session_key = session_key.to_string();
    let chat_id = chat_id.to_string();
    let input = input.to_string();
    let images = images.to_vec();
    let db = db.clone();
    let lock_key = session_key.clone();
    with_chat_lock(&lock_key, async move {
        if !sink.authorized().await {
            return;
        }
        continue_text(&db, user_id, &chat_id, &input, &images, &session_key, sink).await;
    })
    .await;
}

pub async fn handle_callback(
    db: &DatabaseConnection,
    user_id: i32,
    sender_id: &str,
    chat_id: &str,
    data: &str,
    session_key: &str,
    inbound_id: &str,
    transport: ChannelTransport,
) {
    let platform = transport.platform();
    let binding = match ChannelBinding::resolve(db, platform, user_id, sender_id).await {
        Ok(Some(binding)) => binding,
        _ => return,
    };
    let session_key = binding.session_key(session_key);
    let sink = ChannelSink {
        transport,
        binding,
        db: db.clone(),
    };
    let platform = sink.platform();
    let session_key = session_key.to_string();
    let chat_id = chat_id.to_string();
    let data = data.to_string();
    let inbound_id = inbound_id.to_string();
    let db = db.clone();
    let lock_key = session_key.clone();
    with_chat_lock(&lock_key, async move {
        let claimed = format!("{session_key}:{inbound_id}");
        if !sink.authorized().await || !claim_inbound(&db, platform, Some(user_id), &claimed).await
        {
            return;
        }
        // A piece's button answers that piece's question.
        let key = myriad_agent_rules::channel::parse_telegram_callback(&data)
            .and_then(|bound| works::number_of_prompt(&bound.prompt_id))
            .map_or_else(|| session_key.clone(), |number| works::lane_key(&session_key, number));
        let Some(pending) = load_pending(&db, platform, &key).await else {
            return;
        };
        match telegram_callback_action(&pending.prompt, &data) {
            TelegramCallbackAction::RequestInput => {
                let _ = sink.send_force_reply(&pending.prompt.question).await;
            }
            TelegramCallbackAction::Resume(answer) if key != session_key => {
                let work_session = load_session(&db, platform, &key)
                    .await
                    .map(|stored| stored.session_id)
                    .unwrap_or_default();
                super::held::INBOUND
                    .scope(
                        claimed,
                        works::answer_pending(&db, user_id, &key, work_session, pending, &answer, sink),
                    )
                    .await;
            }
            TelegramCallbackAction::Resume(answer) => {
                super::held::INBOUND
                    .scope(
                        claimed,
                        continue_text(&db, user_id, &chat_id, &answer, &[], &session_key, sink),
                    )
                    .await;
            }
            TelegramCallbackAction::Stale => {
                let _ = sink.send_text(PENDING_STALE_REPLY).await;
            }
            TelegramCallbackAction::Unknown => {
                let _ = sink
                    .send_prompt(&format_pending_prompt(&pending.prompt), &pending.prompt)
                    .await;
            }
        }
    })
    .await;
}

pub(super) async fn continue_text(
    db: &DatabaseConnection,
    user_id: i32,
    _chat_id: &str,
    input: &str,
    images: &[ChannelImageRef],
    session_key: &str,
    sink: ChannelSink,
) {
    let platform = sink.platform();
    if !sink.authorized().await {
        return;
    }
    match crate::services::principal::current_roles(db, user_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            warn!(user_id, "channel user missing");
            return;
        }
        Err(error) => {
            warn!(%error, user_id, "channel user lookup failed");
            return;
        }
    }
    let session_id = match bind_session(db, platform, user_id, session_key).await {
        Ok(stored) => stored.session_id,
        Err(error) => {
            warn!(%error, "channel session bind failed");
            let _ = sink.send_text("无法保存通道会话，请稍后重试。").await;
            return;
        }
    };

    if let Some(command) = parse_channel_command(input) {
        handle_command(
            db,
            user_id,
            platform,
            session_key,
            &session_id,
            command,
            &sink,
        )
        .await;
        return;
    }

    // She is not on the site: held for when she is back, and nobody is
    // answered, not even with a refusal (D3).
    if super::held::away() {
        super::held::hold(db, &sink, session_key, user_id, input, images).await;
        return;
    }
    // Back: what was held in this chat is read with what came now.
    let taken = super::held::take(db, session_key).await;
    let (input, images) = match &taken {
        Some(held) => {
            let mut all = held.images.clone();
            all.extend_from_slice(images);
            let text = [held.text.trim(), input.trim()]
                .into_iter()
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            (text, all)
        }
        None => (input.to_string(), images.to_vec()),
    };
    let (input, images) = (input.as_str(), images.as_slice());

    if runtime::recover_session(db, session_key, Some(sink.clone())).await {
        let _ = sink.send_text("正在恢复上一件事的回复，请稍后再试。").await;
        return;
    }
    // A question from before pieces, kept on the chat itself.
    if let Some(pending) = load_pending(db, platform, session_key).await {
        works::answer_pending(db, user_id, session_key, session_id, pending, input, sink).await;
        return;
    }
    // The one piece waiting for an answer takes it; more, and she places it.
    let pieces = works::pieces(db, platform, user_id, session_key).await;
    let plain: Vec<works::Piece> = pieces.iter().map(|(_, piece)| piece.clone()).collect();
    if let works::Route::Answer(number) = works::route(&plain)
        && let Some((lane, _)) = pieces.iter().find(|(lane, _)| lane.number == number)
        && let Some(pending) = load_pending(db, platform, &lane.key).await
    {
        let work_session = lane.stored.session_id.clone();
        works::answer_pending(db, user_id, &lane.key, work_session, pending, input, sink).await;
        return;
    }

    // She answers as herself; Work is what she hands off.
    chat::start_chat_turn(
        db.clone(),
        user_id,
        session_id,
        input,
        images,
        sink,
        session_key,
    )
    .await;
}
