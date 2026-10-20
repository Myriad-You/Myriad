//! Doing what her group mind says, in order: those that answer are each
//! their own task and step the group again when done.

use futures::future::BoxFuture;
use myriad_merope::group::{self as core, Answer, Effect, Event, Pieces};

use super::*;

/// Do what a step said, in order: those that answer are each their own
/// task and step the group again when done.
pub(super) fn execute(venue: String, effects: Vec<Effect>) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        for effect in effects {
            perform(&venue, effect).await;
        }
    })
}

/// Step the group with what came back, and do what that says.
pub(super) fn answered(venue: String, id: String, answer: Answer) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        let out = step(&venue, Event::Done { id, answer });
        execute(venue, out.effects).await;
    })
}

pub(super) fn kept_for(seconds: i64) -> i64 {
    (now() + chrono::Duration::seconds(seconds)).timestamp()
}

pub(super) async fn perform(venue: &str, effect: Effect) {
    let venue = venue.to_string();
    match effect {
        Effect::Wake { key, after } => {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs_f64(after.max(0.0))).await;
                let out = step(&venue, Event::Woke { key });
                execute(venue, out.effects).await;
            });
        }
        Effect::KeepLines { lines } => {
            if let Ok(db) = db()
                && let Err(error) = keep(
                    &db,
                    LINES_NAMESPACE,
                    &venue,
                    &StoredLines { lines },
                    kept_for(core::TRANSCRIPT_FOR_SECONDS),
                )
                .await
            {
                warn!(%error, %venue, "[Group] could not keep the group's lines");
            }
        }
        Effect::KeepReach { thread, seen_at } => {
            let Ok(db) = db() else {
                return;
            };
            let reach = StoredReach {
                thread,
                seen_at: chrono::DateTime::from_timestamp_micros(seen_at).unwrap_or_default(),
            };
            if let Err(error) = keep(
                &db,
                REACH_NAMESPACE,
                &venue,
                &reach,
                kept_for(core::SEEN_WITHIN_SECONDS),
            )
            .await
            {
                warn!(%error, %venue, "[Group] could not keep that she is in the group");
            }
        }
        Effect::Ledger {
            messages,
            waits,
            theirs,
            hers,
        } => {
            let Ok(db) = db() else {
                return;
            };
            let stored = kept::<core::Ledger>(&db, LEDGER_NAMESPACE, &venue)
                .await
                .ok()
                .flatten()
                .unwrap_or_default();
            let ledger = core::merge_ledger(stored, messages, waits, &theirs, &hers);
            if let Err(error) = keep(
                &db,
                LEDGER_NAMESPACE,
                &venue,
                &ledger,
                kept_for(LEDGER_DAYS * 86_400),
            )
            .await
            {
                warn!(%error, %venue, "[Group] could not keep the talk ledger");
            }
        }
        Effect::Jargon { text } => jargon_heard(&venue, &text),
        Effect::TakeIn { stretch } => {
            tokio::spawn(async move {
                let Ok(db) = db() else {
                    return;
                };
                let Some(owner) = owner(&db).await else {
                    return;
                };
                let stretch = stretch
                    .into_iter()
                    .filter_map(|line| {
                        Some((
                            chrono::DateTime::parse_from_rfc3339(&line.at).ok()?,
                            line.name,
                            line.text,
                            line.hers,
                        ))
                    })
                    .collect();
                take_in_heard(&db, owner, &venue, stretch).await;
            });
        }
        Effect::PictureAgain { key, seen } => {
            if let Ok(db) = db()
                && let Some(owner) = owner(&db).await
            {
                picture_again(&db, owner, &venue, &key, &seen).await;
            }
        }
        Effect::SpokeUp { line, taken } => {
            if let Ok(db) = db() {
                spoke_up(&db, &venue, &line, taken).await;
            }
        }
        Effect::StrangerAfter {
            owner,
            stranger,
            said,
            reply,
            message_id,
        } => {
            #[cfg(test)]
            if g0::on() {
                g0::note(
                    "strangerAfter",
                    serde_json::json!({ "owner": owner, "venue": venue, "who": stranger.who, "said": said, "reply": reply, "messageId": message_id }),
                );
                // A scene with a database of its own keeps the exchange for
                // her note on them, as the site would.
                if g0::database().is_none() {
                    return;
                }
            }
            let Ok(db) = db() else {
                return;
            };
            crate::services::agent::merope::group::strangers::enqueue_after(
                &db,
                owner,
                venue,
                crate::services::agent::merope::group::strangers::Stranger {
                    who: stranger.who,
                    name: stranger.name,
                },
                said,
                reply,
                &message_id,
            )
            .await;
        }
        Effect::Sticker { to, chosen } => {
            let Some(line) = line_to(&to) else {
                return;
            };
            let token = token(&venue);
            tokio::spawn(async move {
                if let Some(meaning) = send_sticker(&line, &token, &chosen).await {
                    let restore = kept_before(&venue).await;
                    let out = step(&venue, Event::StickerSent { meaning, restore });
                    execute(venue, out.effects).await;
                }
            });
        }
        Effect::Settled { waiter, value } => {
            if let Some(sender) = WAITERS.lock().ok().and_then(|mut all| all.remove(&waiter)) {
                let _ = sender.send(value);
            }
        }
        Effect::Claim { id, inbound_id } => {
            tokio::spawn(async move {
                let ok = match (db(), platform_of(venue.split(':').next().unwrap_or(""))) {
                    (Ok(db), Some(platform)) => claim(&db, platform, &inbound_id).await,
                    _ => false,
                };
                answered(venue, id, Answer::Claimed { ok }).await;
            });
        }
        Effect::Look { id, pictures } => {
            tokio::spawn(async move {
                let seen = look(&venue, pictures).await;
                answered(venue, id, Answer::Looked { seen }).await;
            });
        }
        Effect::Identify { id, from } => {
            tokio::spawn(async move {
                let answer = identify(&venue, &from).await;
                answered(venue, id, answer).await;
            });
        }
        Effect::Prepare { id, user_id } => {
            tokio::spawn(async move {
                let ok = prepare(&venue, user_id).await;
                answered(venue, id, Answer::Prepared { ok }).await;
            });
        }
        Effect::Read { id, what, .. } => {
            tokio::spawn(async move {
                let answer = read(&venue, what).await;
                answered(venue, id, answer).await;
            });
        }
        Effect::Embed { id, text } => {
            tokio::spawn(async move {
                let vector = crate::services::agent::memory::meaning::embed_asked(&text).await;
                answered(venue, id, Answer::Embedded { vector }).await;
            });
        }
        Effect::Write { what, .. } => write(what).await,
        Effect::MayLookUp { id, term } => {
            tokio::spawn(async move {
                let may = may_look_up(&term).await;
                answered(venue, id, Answer::MayLookUp { may }).await;
            });
        }
        Effect::Define { id, term } => {
            tokio::spawn(async move {
                let entry = crate::services::agent::merope::group::senses::define(&term)
                    .await
                    .ok()
                    .map(|taken| core::Entry {
                        title: taken.title,
                        text: taken.text,
                        url: taken.url,
                    });
                answered(venue, id, Answer::Defined { entry }).await;
            });
        }
        Effect::Search { id, query } => {
            tokio::spawn(async move {
                let found =
                    crate::services::agent::merope::group::senses::look_up(&query, false).await;
                answered(venue, id, Answer::Found { found }).await;
            });
        }
        Effect::ReadKept { id } => {
            tokio::spawn(async move {
                let restore = kept_before(&venue).await;
                answered(venue, id, Answer::Kept { restore }).await;
            });
        }
        Effect::ReadLedger { id } => {
            tokio::spawn(async move {
                let stored = match db() {
                    Ok(db) => kept::<core::Ledger>(&db, LEDGER_NAMESPACE, &venue)
                        .await
                        .ok()
                        .flatten()
                        .map(|ledger| Pieces {
                            theirs: ledger.theirs,
                            hers: ledger.hers,
                        }),
                    Err(_) => None,
                };
                answered(venue, id, Answer::LedgerRead { stored }).await;
            });
        }
        Effect::TurnBegin {
            id,
            user_id,
            input,
            transcript,
            speaker,
            chime,
            first,
        } => {
            tokio::spawn(async move {
                let answer =
                    turn_begin(&venue, user_id, input, transcript, speaker, chime, first).await;
                answered(venue, id, answer).await;
            });
        }
        Effect::TurnEnd {
            id,
            turn,
            user_id,
            said,
            sticker,
            first,
        } => {
            tokio::spawn(async move {
                let reply = crate::services::agent::group_turn::end(&turn, said, sticker).await;
                // Unpaired or switched off while she was thinking.
                let bound = first || still_hers(&venue, user_id).await;
                answered(venue, id, Answer::TurnEnded { reply, bound }).await;
            });
        }
        Effect::TurnKept {
            turn,
            went_out,
            sticker,
        } => {
            tokio::spawn(async move {
                crate::services::agent::group_turn::kept(&turn, &went_out, sticker).await;
            });
        }
        Effect::React { id, to, emoji } => {
            tokio::spawn(async move {
                let ok = match line_to(&to) {
                    Some(line) => react(&line, &token(&venue), &emoji).await,
                    None => false,
                };
                if ok {
                    info!(%venue, "[Group] she put a reaction on a line");
                }
                answered(venue, id, Answer::Reacted { ok }).await;
            });
        }
        Effect::Whisper {
            id,
            platform,
            user,
            text,
            choices,
            wait,
            ..
        } => {
            tokio::spawn(async move {
                let wait = wait.unwrap_or_default();
                let ok = whisper(&token(&venue), &platform, &user, &text, &choices, wait).await;
                answered(venue, id, Answer::Whispered { ok }).await;
            });
        }
        Effect::KeepTable { table } => {
            let Ok(db) = db() else {
                return;
            };
            match table {
                Some(table) => {
                    if let Err(error) = keep(
                        &db,
                        TABLE_NAMESPACE,
                        &venue,
                        &table,
                        kept_for(TABLE_FOR_SECONDS),
                    )
                    .await
                    {
                        warn!(%error, %venue, "[Group] could not keep the game at the table");
                    }
                }
                None => forget_kept(&db, TABLE_NAMESPACE, &venue).await,
            }
        }
        Effect::Seat { who } => seat(&venue, who),
        Effect::Unseat { who } => unseat(&venue, &who),
        Effect::Typing { id, to } => {
            tokio::spawn(async move {
                if let Some(line) = line_to(&to) {
                    send_typing(&line, &token(&venue)).await;
                }
                answered(venue, id, Answer::Typed).await;
            });
        }
        Effect::Ask {
            id,
            how,
            voice,
            owner,
            operation,
            timeout_ms,
            system,
            input,
            schema_name,
            schema,
            turn,
        } => {
            tokio::spawn(async move {
                let within = Duration::from_millis(timeout_ms);
                let asked = match turn {
                    // Within a member's turn: as their Chat turn asks her.
                    Some(turn) if how == core::AskKind::Say => tokio::time::timeout(
                        within,
                        crate::services::agent::group_turn::say(&turn, &input),
                    )
                    .await
                    .unwrap_or(Err("timeout")),
                    // A judgment within a member's turn is billed as part of it.
                    Some(turn) => {
                        crate::services::agent::group_turn::within(
                            Some(&turn),
                            crate::services::agent::merope::group::asking::judge(
                                how,
                                &voice,
                                owner,
                                &operation,
                                within,
                                &system,
                                &input,
                                &schema_name,
                                &schema,
                            ),
                        )
                        .await
                    }
                    None => {
                        crate::services::agent::merope::group::asking::judge(
                            how,
                            &voice,
                            owner,
                            &operation,
                            within,
                            &system,
                            &input,
                            &schema_name,
                            &schema,
                        )
                        .await
                    }
                };
                let (raw, failure) = match asked {
                    Ok(raw) => (Some(raw), None),
                    Err(failure) => (None, Some(failure.to_string())),
                };
                answered(venue, id, Answer::Asked { raw, failure }).await;
            });
        }
        Effect::Send {
            id,
            to,
            pieces,
            quote,
        } => {
            tokio::spawn(async move {
                use myriad_agent_rules::mentions::Piece;
                let pieces: Vec<Piece> = pieces
                    .into_iter()
                    .map(|piece| match piece {
                        core::Piece::Text { text } => Piece::Text(text),
                        core::Piece::Mention { id, name } => Piece::Mention { id, name },
                    })
                    .collect();
                let ok = match line_to(&to) {
                    Some(line) => match send_reply(&line, &token(&venue), &pieces, quote).await {
                        Ok(()) => true,
                        Err(kind) => {
                            warn!(?kind, %venue, "[Group] reply not sent");
                            false
                        }
                    },
                    None => false,
                };
                answered(venue, id, Answer::Sent { ok }).await;
            });
        }
    }
}
