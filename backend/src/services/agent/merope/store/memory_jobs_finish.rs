//! Ending a claim: a stale worker fenced off, and what it found committed
//! with its receipt in one transaction.

use super::*;

/// Fence stale workers and atomically commit both the memory and its receipt.
pub(in crate::services::agent::merope) async fn finish(
    db: &DatabaseConnection,
    claim: &Claim,
    result: Result<Effect, Failure>,
) -> anyhow::Result<()> {
    // What a line that only lets go took back is looked at again with that
    // line (`take_back_check`): its message, found before the transaction
    // takes a connection of its own.
    let said_alone = match (&result, &claim.job.data) {
        (
            Ok(Effect::Chat(update)),
            Some(Payload::Chat {
                input_at,
                present,
                user_text,
                lines,
                ..
            }),
        ) if update.updates.iter().any(|one| one.fact.is_none()) => {
            let from = lines.iter().min().copied().unwrap_or(*input_at);
            super::super::super::memory_nodes::take_back_check::their_line(
                db,
                claim.job.owner,
                present,
                user_text,
                from,
                *input_at,
            )
            .await
        }
        _ => None,
    };
    let tx = db.begin().await?;
    lock(&tx).await?;
    let now = now(&tx).await?;
    let Some(mut job) = registry::get::<Job>(&tx, NAMESPACE, &claim.id).await? else {
        return Ok(());
    };
    if job.token != claim.job.token || job.token.is_none() || job.lease <= now {
        return Ok(());
    }
    let result = if current(&tx, &job, true).await? {
        result
    } else {
        Ok(Effect::Stale)
    };
    let outcome;
    // What a stretch kept, put on her nodes once it is written.
    let mut on_nodes = None;
    // What a stretch never read kept as it was, put on them the same way.
    let mut kept_as_they_were = None;
    // A stretch is tried again when its answer could not be read too: what
    // it would keep for good is not let go on one unreadable answer.
    let stretch = matches!(job.data, Some(Payload::Settle { .. }));
    if stretch && matches!(&result, Err(failure) if failure.unreadable()) {
        settling::unreadable();
    }
    match result {
        Err(failure)
            if (failure.retryable() || (stretch && failure.unreadable()))
                && job.attempts < MAX_ATTEMPTS =>
        {
            if stretch {
                settling::asked_again();
            }
            outcome = failure.to_string();
            if let (Some(data), Some(next)) = (&mut job.data, job.next.take()) {
                data.fold(next);
            }
            job.ready = now + again_in(job.attempts);
            job.lease = 0;
            job.token = None;
            job.outcome = Some(outcome.clone());
        }
        Ok(Effect::Paused) => {
            // Put back as it was, to wait with the rest while she is off:
            // what it brought along unread again, what came meanwhile with
            // it, and this try not counted.
            outcome = "paused".to_string();
            settling::put_back(&mut job);
            if let (Some(data), Some(next)) = (&mut job.data, job.next.take()) {
                data.fold(next);
            }
            job.attempts = job.attempts.saturating_sub(1);
            job.lease = 0;
            job.token = None;
            job.outcome = Some(outcome.clone());
        }
        result => {
            // More notes than are gone over at once: the rest goes on.
            let goes_on = match &result {
                Ok(Effect::Settle { rest, .. }) => *rest,
                _ => None,
            };
            if goes_on.is_none() {
                settling::over(&job);
            }
            outcome = match result {
                Ok(Effect::Chat(mut update)) => {
                    let Some(Payload::Chat {
                        input_at, present, ..
                    }) = &job.data
                    else {
                        anyhow::bail!("wrong memory job kind");
                    };
                    if let Some(said) = said_alone {
                        update.taken_back_in = (0..update.updates.len())
                            .map(|index| (index, said))
                            .collect();
                    }
                    if super::super::apply_chat_memory_updates_on(
                        &tx,
                        job.owner,
                        *input_at,
                        &update,
                        present,
                        super::super::Keep::Note,
                    )
                    .await?
                    {
                        "applied".into()
                    } else {
                        "stale_or_duplicate".into()
                    }
                }
                Ok(Effect::Settle {
                    updates, settled, ..
                }) => {
                    let Some(Payload::Settle { present, last, .. }) = &job.data else {
                        anyhow::bail!("wrong memory job kind");
                    };
                    // What was put together is kept for good now, each
                    // note pointing at what it was kept as: forgetting that
                    // takes the notes with it. The rest stays a short note
                    // and fades.
                    let kept = super::super::settle_chat_memory_on(
                        &tx, job.owner, *last, &updates, &settled, present,
                    )
                    .await?;
                    on_nodes = Some((present.clone(), updates.clone()));
                    if kept { "applied" } else { "no_change" }.into()
                }
                Ok(Effect::Stranger { previous, note }) => {
                    let Some(Payload::Stranger {
                        venue, stranger, ..
                    }) = &claim.job.data
                    else {
                        anyhow::bail!("wrong memory job kind");
                    };
                    super::super::super::strangers::apply_note(
                        &tx,
                        venue,
                        stranger,
                        previous.as_deref(),
                        note.as_ref(),
                    )
                    .await?;
                    "applied".into()
                }
                Ok(Effect::Stale) => {
                    job.data = None;
                    "stale".into()
                }
                Ok(Effect::NoChange) => "no_change".into(),
                Ok(Effect::Paused) => unreachable!("put back above"),
                Err(failure) => {
                    // Its notes stay notes and go with the next stretch.
                    kept_as_they_were = settling::leave_unread(&tx, &mut job).await?;
                    failure.to_string()
                }
            };
            if let Some(rest) = goes_on {
                // Gone over next from where the rest begins: at once, or a
                // while later when they said more meanwhile (as lines put a
                // stretch off, never past a while after where the rest
                // begins); with tries of its own.
                let came = job.next.is_some();
                if let (Some(data), Some(next)) = (&mut job.data, job.next.take()) {
                    data.fold(next);
                }
                settling::go_on(&mut job, rest);
                job.attempts = 0;
                job.token = None;
                job.lease = 0;
                job.ready = if came { now + SETTLES_AFTER } else { now };
                job.outcome = Some(outcome.clone());
            } else {
                let count = match &claim.job.data {
                    Some(Payload::Stranger { exchanges, .. }) => exchanges.len(),
                    _ => 0,
                };
                consume(&mut job, count, now, &outcome);
                job.brought = None;
                promote_next(&mut job, now);
            }
        }
    }
    save(&tx, &claim.id, &job, now).await?;
    tx.commit().await?;
    // What they took back here (or before, with no answer yet) is looked at
    // again, now that letting it go is committed.
    if outcome != "paused"
        && matches!(
            claim.job.data,
            Some(Payload::Chat { .. } | Payload::Settle { .. })
        )
    {
        super::super::super::memory_nodes::take_back_check::go_over(db, job.owner).await;
    }
    tracing::info!(job = claim.id, attempt = claim.job.attempts, %outcome, "[Merope] memory job");
    if matches!(claim.job.data, Some(Payload::Settle { .. })) && outcome != "paused" {
        crate::services::agent::alive::settled(&outcome);
    }
    if let Some((present, updates)) = on_nodes {
        let owner = crate::services::principal::site_owner_id(db)
            .await
            .ok()
            .flatten();
        // Boxed: in the debug build the site runs, the whole chain down to the
        // model call would otherwise be laid out in this frame's stack.
        Box::pin(super::super::super::memory_nodes::facts::settled(
            db, job.owner, owner, &present, &updates,
        ))
        .await;
    }
    if let Some(kept) = kept_as_they_were {
        kept.on_nodes(db).await;
    }
    Ok(())
}
