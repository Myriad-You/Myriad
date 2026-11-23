//! Every session of a question gone over the settle way, the short notes
//! kept as rows while a talk goes on and let fade once they have.

use super::*;

/// How long a short note is still there to recall, in the settle way: past
/// this, by the question's date, it has faded. A stand-in for a fast fading
/// memory strength, not a rule production has.
const SHORT_NOTES_LAST: chrono::Duration = chrono::Duration::days(3);

/// Every session of a question, the settle way: what is kept, and what of
/// it came from the turns that answer the question.
#[allow(clippy::too_many_arguments)]
pub(in super::super) async fn go_over(
    db: &sea_orm::DatabaseConnection,
    judge: &crate::services::analyzer::AiAnalyzer,
    user_id: i32,
    sessions: &[super::super::Session<'_>],
    question_date: &str,
    answering: &mut Vec<String>,
    kept: &mut usize,
    trail: &mut super::super::note_trail::Trail,
) {
    let asked_at = date_of(question_date);
    // The short notes kept as rows, as production keeps them while a talk
    // goes on, by when their session was over.
    let mut as_rows: Vec<(chrono::DateTime<chrono::FixedOffset>, Vec<String>)> = Vec::new();
    for (session_at, turns, present) in sessions {
        let (session_at, present) = (*session_at, present);
        // A private talk is gone over from what was said, as production
        // goes over it now: no notes to keep or let fade.
        if on_nodes(present) {
            let raw = Box::pin(super::raw::session(
                db, user_id, present, session_at, turns, trail,
            ))
            .await;
            *kept += raw.written;
            answering.extend(raw.answering);
            trail.close(db, user_id, present).await;
            continue;
        }
        // Notes of a session long enough before this one have faded
        // (the bench's stand-in for fading, as when they ask).
        let (faded, fresh): (Vec<_>, Vec<_>) = std::mem::take(&mut as_rows)
            .into_iter()
            .partition(|(over, _)| session_at - *over > SHORT_NOTES_LAST);
        as_rows = fresh;
        let_fade(db, faded.into_iter().flat_map(|(_, ids)| ids).collect()).await;
        let mut row_of: Vec<Option<String>> = Vec::new();
        let mut notes: Vec<super::super::super::chat_remember::ChatMemoryUpdate> = Vec::new();
        let mut noted_at = Vec::new();
        let mut noted: Vec<(String, bool)> = Vec::new();
        let mut short: Vec<(
            chrono::DateTime<chrono::FixedOffset>,
            super::super::super::chat_remember::ChatMemoryUpdates,
            bool,
        )> = Vec::new();
        for (index, turn) in turns.iter().enumerate() {
            if turn.role != "user" {
                continue;
            }
            let reply = turns
                .get(index + 1)
                .filter(|next| next.role == "assistant")
                .map(|next| next.content.as_str())
                .unwrap_or("");
            let at = session_at + chrono::Duration::minutes(index as i64);
            let text: String = turn
                .content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let answer_here =
                turn.has_answer || turns.get(index + 1).is_some_and(|next| next.has_answer);
            if text.chars().count() > 2_000 {
                if answer_here {
                    trail.answer_line(&text, 0, true);
                }
                continue;
            }
            let Some(updates) = note_turn(db, user_id, present, &text, reply, at).await else {
                if answer_here {
                    trail.answer_line(&text, 0, false);
                }
                continue;
            };
            let_go(db, user_id, present, at, &updates).await;
            let taken: Vec<_> = updates
                .updates
                .iter()
                .filter(|update| update.fact.is_some())
                .cloned()
                .collect();
            if answer_here {
                trail.answer_line(&text, taken.len(), false);
            }
            // What she said and what they suggested she try are kept as
            // they are, as the line is gone over: kept later, a line before
            // the last one with a note would no longer be current.
            let hers = super::super::super::chat_remember::ChatMemoryUpdates {
                updates: Vec::new(),
                said: updates.said.clone(),
                put_onto: updates.put_onto.clone(),
                said_new: super::super::super::chat_remember::SaidNew::Nothing,
                taken_back_in: Default::default(),
                said_from: Default::default(),
                asked: Default::default(),
            };
            if (!hers.said.is_empty() || !hers.put_onto.is_empty())
                && apply_at(db, user_id, present, at, &hers).await.is_some()
            {
                *kept += 1;
                if answer_here {
                    answering.extend(
                        hers.said
                            .iter()
                            .map(|said| super::super::super::ingest::compact_summary(&said.said)),
                    );
                }
            }
            row_of.extend(keep_as_notes(db, user_id, present, at, &taken).await);
            noted.extend(
                taken
                    .iter()
                    .map(|note| (note.fact.clone().unwrap_or_default(), answer_here)),
            );
            notes.extend(taken);
            noted_at.resize(notes.len(), at);
            short.push((at, updates, answer_here));
        }
        let over_at = session_at + chrono::Duration::minutes(turns.len() as i64);
        let mut settled = Vec::new();
        if !notes.is_empty() {
            let answer_lines: String = turns
                .iter()
                .filter(|turn| turn.role == "user" && turn.has_answer)
                .map(|turn| {
                    turn.content
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect::<Vec<_>>()
                .join("\n");
            trail.notes(&noted);
            let (lasting, _, put_away) = settle_session(
                db, judge, user_id, present, turns, &notes, &noted_at, &row_of, over_at, trail,
            )
            .await;
            // A note put into what is kept for good is let go only for what
            // holds it now, as production does; the rest are still notes.
            let still: std::collections::HashSet<String> =
                crate::services::agent::memory::unified::latest_of(
                    db,
                    user_id,
                    crate::services::agent::memory::unified::SHORT_NOTE_SOURCE,
                    10_000,
                )
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|row| row.id)
                .collect();
            settled = put_away
                .into_iter()
                .filter(|index| {
                    row_of
                        .get(*index)
                        .cloned()
                        .flatten()
                        .is_none_or(|id| !still.contains(&id))
                })
                .collect();
            for (fact, evidence) in lasting {
                *kept += 1;
                if !answer_lines.is_empty()
                    && evidence
                        .split(" / ")
                        .any(|part| answer_lines.contains(part.trim()))
                {
                    answering.push(fact);
                }
            }
        }
        // Short notes still fresh when the question is asked, but not those
        // settled into what is kept for good: production retires them.
        if asked_at - over_at <= SHORT_NOTES_LAST {
            // Kept below as the bench keeps a fresh note; its row would take
            // it as one already there.
            let_fade(
                db,
                std::mem::take(&mut row_of).into_iter().flatten().collect(),
            )
            .await;
            let mut note = 0;
            for (at, updates, answer_here) in short {
                let facts = super::super::super::chat_remember::ChatMemoryUpdates {
                    updates: updates
                        .updates
                        .iter()
                        .filter(|update| {
                            if update.fact.is_none() {
                                return true;
                            }
                            note += 1;
                            let fresh = !settled.contains(&(note - 1));
                            if fresh {
                                trail.fresh(note - 1);
                            }
                            fresh
                        })
                        .cloned()
                        .collect(),
                    said: Vec::new(),
                    put_onto: Vec::new(),
                    said_new: updates.said_new,
                    taken_back_in: Default::default(),
                    said_from: Default::default(),
                    asked: Default::default(),
                };
                if !facts.updates.is_empty()
                    && apply_at(db, user_id, present, at, &facts).await.is_some()
                {
                    *kept += 1;
                    if answer_here {
                        answering.extend(
                            facts
                                .updates
                                .iter()
                                .filter_map(|update| update.fact.as_deref())
                                .map(super::super::super::ingest::compact_summary),
                        );
                    }
                }
            }
        }
        trail.close(db, user_id, present).await;
        let over_at = session_at + chrono::Duration::minutes(turns.len() as i64);
        as_rows.push((over_at, row_of.into_iter().flatten().collect()));
    }
    // What she has when they ask is as before: the notes kept as rows go,
    // those still fresh then were kept above.
    let_fade(db, as_rows.into_iter().flat_map(|(_, ids)| ids).collect()).await;
}

/// Short notes of one turn (`taken`) kept as rows as production keeps them
/// while the talk goes on (`Keep::Note`), so what is noted next sees them
/// as production's extraction does: the latest of them, and what it checks
/// a new fact against. Their rows, in the order of `taken` (none for one
/// not kept: taken as one already there).
async fn keep_as_notes(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    present: &Audience,
    at: chrono::DateTime<chrono::FixedOffset>,
    taken: &[super::super::super::chat_remember::ChatMemoryUpdate],
) -> Vec<Option<String>> {
    use crate::services::agent::memory::unified;
    let notes = || async {
        unified::latest_of(db, user_id, unified::SHORT_NOTE_SOURCE, 10_000)
            .await
            .unwrap_or_default()
    };
    let before: std::collections::HashSet<String> =
        notes().await.into_iter().map(|row| row.id).collect();
    let updates = super::super::super::chat_remember::ChatMemoryUpdates {
        updates: taken.to_vec(),
        ..Default::default()
    };
    if let Ok(transaction) = sea_orm::TransactionTrait::begin(db).await {
        // Dated as production dates a note: now, so it has not faded.
        let _ = super::super::super::store::apply_chat_memory_updates_on(
            &transaction,
            user_id,
            at,
            &updates,
            present,
            super::super::super::store::Keep::Note,
        )
        .await;
        let _ = transaction.commit().await;
    }
    let new: Vec<_> = notes()
        .await
        .into_iter()
        .filter(|row| !before.contains(&row.id))
        .collect();
    taken
        .iter()
        .map(|note| {
            let fact =
                super::super::super::ingest::compact_summary(note.fact.as_deref().unwrap_or(""));
            new.iter()
                .find(|row| super::super::super::ingest::compact_summary(&row.content) == fact)
                .map(|row| row.id.clone())
        })
        .collect()
}

/// Short notes kept as rows let go: faded, in the bench's stand-in.
async fn let_fade(db: &sea_orm::DatabaseConnection, ids: Vec<String>) {
    if ids.is_empty() {
        return;
    }
    let _ = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM agent_memories WHERE id = ANY($1)",
            [ids.into()],
        ))
        .await;
}
