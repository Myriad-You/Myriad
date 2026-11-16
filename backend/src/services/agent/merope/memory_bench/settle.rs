//! The settle way of the memory bench (`MEROPE_MEMORY_BENCH_WAY=settle`):
//! each session's lines taken as short notes, the session gone over once it
//! is over, and only what lasts kept for good, as production does now.

use super::*;

/// What one of their lines leaves, taken by production's own extraction
/// (`chat_remember::extract`: what she already has and what they told her
/// last shown, then checked against what she has but was not shown) but not
/// kept: a short note, in the settle way.
async fn note_turn(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    present: &Audience,
    text: &str,
    reply: &str,
    at: chrono::DateTime<chrono::FixedOffset>,
) -> Option<super::super::chat_remember::ChatMemoryUpdates> {
    use super::super::memory_jobs::Effect;
    match super::super::chat_remember::extract(
        db,
        user_id,
        text,
        reply,
        at,
        present,
        &super::super::TurnContext::default(),
    )
    .await
    {
        Ok(Effect::Chat(updates)) => Some(updates),
        Ok(_) => None,
        Err(error) => {
            eprintln!("bench call failed: noting: {error:?}");
            None
        }
    }
}

/// A line that only takes something back, saying nothing new, let go at
/// once as production lets it go: what she knows for good too.
async fn let_go(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    present: &Audience,
    at: chrono::DateTime<chrono::FixedOffset>,
    updates: &super::super::chat_remember::ChatMemoryUpdates,
) {
    if !updates.updates.is_empty() && updates.updates.iter().all(|update| update.fact.is_none()) {
        let taken_back = super::super::chat_remember::ChatMemoryUpdates {
            updates: updates.updates.clone(),
            ..Default::default()
        };
        apply_at(db, user_id, present, at, &taken_back).await;
    }
}

/// A session gone over once it is over (the settle way), as production goes
/// over a stretch: in passes of at most production's number of notes, oldest
/// first (`chat_remember::settle::first_pass`, `noted_at` when each note was
/// taken), each once the one before is written, so it sees what that one
/// kept among what she knows; each shown the lines up to where its notes
/// end. A pass that fails ends it, as it ends production's job: the notes
/// after it are not gone over (production brings them along with the next
/// stretch). The lasting facts kept, with their evidence, which of them are
/// notes kept as they were, and the indexes of the notes put into what is
/// kept for good: production retires those, so they are no longer short
/// notes.
#[allow(clippy::too_many_arguments)]
async fn settle_session(
    db: &sea_orm::DatabaseConnection,
    judge: &crate::services::analyzer::AiAnalyzer,
    user_id: i32,
    present: &Audience,
    turns: &[Turn],
    notes: &[super::super::chat_remember::ChatMemoryUpdate],
    noted_at: &[chrono::DateTime<chrono::FixedOffset>],
    rows: &[Option<String>],
    at: chrono::DateTime<chrono::FixedOffset>,
    trail: &mut super::note_trail::Trail,
) -> (Vec<(String, String)>, Vec<String>, Vec<usize>) {
    let (mut lasting, mut passed_over, mut settled) = (Vec::new(), Vec::new(), Vec::new());
    let mut start = 0;
    while start < notes.len() {
        let end = start + super::super::chat_remember::settle::first_pass(&noted_at[start..]);
        let shown = noted_at
            .get(end)
            .map_or(turns.len(), |rest| said_before(turns.len(), at, *rest));
        let pass = Pass {
            turns,
            shown,
            notes: &notes[start..end],
            rows: rows.get(start..end).unwrap_or(&[]),
            first: start,
        };
        let Some((kept, passed, put_away)) =
            Box::pin(settle_pass(db, judge, user_id, present, pass, at, trail)).await
        else {
            break;
        };
        lasting.extend(kept);
        passed_over.extend(passed);
        settled.extend(put_away.into_iter().map(|index| index + start));
        start = end;
    }
    (lasting, passed_over, settled)
}

/// Of a session's `count` turns over `at`, how many were said before
/// `rest`: turn `index` was said `count - index` minutes before `at`, as
/// `over_time` and `go_over` take them (and production shows a pass the
/// lines said before the rest's first note).
fn said_before(
    count: usize,
    at: chrono::DateTime<chrono::FixedOffset>,
    rest: chrono::DateTime<chrono::FixedOffset>,
) -> usize {
    (0..count)
        .take_while(|index| at - chrono::Duration::minutes((count - index) as i64) < rest)
        .count()
}

/// One pass of [`settle_session`]: of `turns`, the first `shown`; its notes
/// and their rows, the first of them note `first` of the session.
struct Pass<'a> {
    turns: &'a [Turn],
    shown: usize,
    notes: &'a [super::super::chat_remember::ChatMemoryUpdate],
    rows: &'a [Option<String>],
    first: usize,
}

/// One pass gone over: which of its short notes last, put together, kept as
/// production keeps a fact, and a note the answer passed over kept as it
/// is. As [`settle_session`] says, its indexes the pass's own; none when
/// the call failed or could not be read.
async fn settle_pass(
    db: &sea_orm::DatabaseConnection,
    judge: &crate::services::analyzer::AiAnalyzer,
    user_id: i32,
    present: &Audience,
    pass: Pass<'_>,
    at: chrono::DateTime<chrono::FixedOffset>,
    trail: &mut super::note_trail::Trail,
) -> Option<(Vec<(String, String)>, Vec<String>, Vec<usize>)> {
    let Pass {
        turns: all,
        shown,
        notes,
        rows,
        first,
    } = pass;
    let turns = &all[..shown];
    // What she knows of them for good, newest first, as many as production
    // shows beside a stretch.
    let mut known = crate::services::agent::memory::unified::active_in(
        db,
        user_id,
        present,
        &crate::services::agent::memory::unified::MemoryKind::ABOUT_PERSON,
    )
    .await
    .unwrap_or_default();
    known.retain(|row| row.source == "chat");
    known.sort_by_key(|row| std::cmp::Reverse(row.created_at));
    let known: Vec<String> = known
        .into_iter()
        .take(SETTLE_KNOWN)
        .map(|row| super::super::ingest::compact_summary(&row.content))
        .collect();
    let theirs = turns
        .iter()
        .filter(|turn| turn.role == "user")
        .map(|turn| {
            turn.content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let lately: Vec<&str> = notes
        .iter()
        .filter_map(|note| note.fact.as_deref())
        .collect();
    // In a private talk the stretch is cut onto her nodes first and her
    // nodes are what she puts it on, as production goes over it.
    let mut offered = Vec::new();
    if on_nodes(present) {
        use super::super::memory_nodes::placing::{self, Line};
        let lines: Vec<Line> = turns
            .iter()
            .enumerate()
            .map(|(index, turn)| Line {
                id: index as i32 + 1,
                theirs: turn.role == "user",
                text: turn.content.clone(),
                at: at - chrono::Duration::minutes((all.len() - index) as i64),
            })
            .collect();
        let placed = placing::place(db, user_id, None, &lines).await;
        offered =
            super::super::memory_nodes::facts::shown_topics(db, user_id, None, &theirs, &placed)
                .await;
    }
    let input = json!({
        "conversation": turns
            .iter()
            .map(|turn| json!({
                "who": if turn.role == "user" { "them" } else { "you" },
                "text": turn.content,
            }))
            .collect::<Vec<_>>(),
        "lately": lately,
        "today": at.with_timezone(&super::super::clock::zone()).format("%Y-%m-%d (%A)").to_string(),
        "topics": super::super::chat_remember::settle::shown_topics(&offered),
    })
    .to_string();
    // Asked as production asks it (`call::Voice::Thorough`: thinking
    // hard), and tried again as production tries a stretch again, an
    // unreadable answer too, as many times.
    let thorough = crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(
        super::super::chat_remember::settle::SETTLE_TIMEOUT,
    ))
    .await
    .map(|analyzer| analyzer.with_deep_thinking());
    let model = thorough.as_ref().unwrap_or(judge);
    let mut read = None;
    let mut went = "going over failed";
    for _ in 0..super::super::store::memory_jobs::MAX_ATTEMPTS {
        let started = std::time::Instant::now();
        let (answer, reported) = crate::services::ai_cost_ledger::reporting(model.analyze_json(
            &myriad_merope::chat_remember::settle_system_prompt(
                &known,
                myriad_merope::chat_remember::Examples::for_words(&theirs),
            ),
            &input,
            myriad_merope::chat_remember::SETTLE_SCHEMA_NAME,
            Some(&myriad_merope::chat_remember::settle_schema()),
        ))
        .await;
        let Ok(raw) =
            answer.inspect_err(|error| eprintln!("bench call failed: settling: {error:#}"))
        else {
            went = "going over failed";
            continue;
        };
        let parsed = myriad_merope::chat_remember::parse_settle(
            &raw,
            &theirs,
            &known,
            notes.len(),
            offered.len(),
        );
        super::note_trail::keep_settle_call(
            &raw,
            parsed.is_some(),
            notes.len(),
            reported.as_ref(),
            started.elapsed(),
        );
        match parsed {
            Some(answer) => {
                read = Some((raw, answer));
                break;
            }
            None => went = "going over not read",
        }
    }
    let Some((raw, (kept, replaces))) = read else {
        for index in 0..notes.len() {
            trail.went(first + index, went, None);
        }
        return None;
    };
    let left = myriad_merope::chat_remember::settle_left(&raw, &kept, notes.len());
    trail_went(trail, &raw, &kept, &left, notes, first);
    let mut settled: Vec<usize> = kept.iter().flat_map(|item| item.from.clone()).collect();
    settled.extend(&left);
    let mut lasting = Vec::new();
    let mut updates = super::super::chat_remember::ChatMemoryUpdates::default();
    // The rows of the notes each update was put together from.
    let row = |index: usize| rows.get(index).cloned().flatten();
    let mut from_rows: Vec<Vec<String>> = Vec::new();
    for item in kept {
        from_rows.push(item.from.iter().filter_map(|index| row(*index)).collect());
        let fact = super::super::ingest::compact_summary(&item.fact);
        updates
            .updates
            .push(super::super::chat_remember::ChatMemoryUpdate {
                // When it happened and where it stands, as production keeps it.
                standing: item.standing.clone(),
                // When it would be worth remembering, as production keeps it.
                cues: item.when_for,
                topic: super::super::chat_remember::settle::goes_under(&item.topic, &offered),
                fact: Some(fact.clone()),
                // What it changes goes with the first: it says how it is now.
                supersedes: if updates.updates.is_empty() {
                    replaces.clone()
                } else {
                    Vec::new()
                },
                evidence: Some(item.evidence.clone()),
                concepts: crate::services::agent::memory::unified::clean_concepts(
                    serde_json::from_value(item.concepts).unwrap_or_default(),
                ),
            });
        lasting.push((fact, item.evidence));
    }
    // A note the answer passed over is kept as it is, as production does.
    let mut passed_over = Vec::new();
    for index in left {
        from_rows.push(row(index).into_iter().collect());
        let note = &notes[index];
        let fact = super::super::ingest::compact_summary(note.fact.as_deref().unwrap_or(""));
        updates
            .updates
            .push(super::super::chat_remember::ChatMemoryUpdate {
                standing: Default::default(),
                topic: None,
                cues: Vec::new(),
                fact: Some(fact.clone()),
                supersedes: Vec::new(),
                evidence: note.evidence.clone(),
                concepts: crate::services::agent::memory::unified::clean_concepts(
                    note.concepts.clone(),
                ),
            });
        lasting.push((fact.clone(), note.evidence.clone().unwrap_or_default()));
        passed_over.push(fact);
    }
    if !updates.updates.is_empty() {
        super::writing::settle_at(db, user_id, present, at, &updates, &from_rows).await;
        super::super::memory_nodes::facts::settled(db, user_id, None, present, &updates).await;
    }
    Some((lasting, passed_over, settled))
}

/// Where going over put each note (`raw` its answer), on `trail`.
fn trail_went(
    trail: &mut super::note_trail::Trail,
    raw: &str,
    kept: &[myriad_merope::chat_remember::Lasting],
    left: &[usize],
    notes: &[super::super::chat_remember::ChatMemoryUpdate],
    first: usize,
) {
    let answer: Value = serde_json::from_str(myriad_merope::chat_remember::strip_json_fence(raw))
        .unwrap_or_default();
    for went in ["moment", "known"] {
        for index in answer[went].as_array().into_iter().flatten() {
            if let Some(index) = index.as_u64() {
                trail.went(first + index as usize, went, None);
            }
        }
    }
    for item in kept {
        let fact = super::super::ingest::compact_summary(&item.fact);
        for index in &item.from {
            trail.went(first + index, "lasting", Some(&fact));
        }
    }
    for index in left {
        let fact =
            super::super::ingest::compact_summary(notes[*index].fact.as_deref().unwrap_or(""));
        trail.went(first + index, "passed over, kept as it was", Some(&fact));
    }
}

/// Whether what she goes over is put on her nodes: a private talk only, as
/// production.
pub(super) fn on_nodes(present: &Audience) -> bool {
    !present.is_group()
}

/// Once all is written, what production's upkeep does to her nodes: what
/// going over placed nowhere sorted onto them, and those due gone over.
pub(super) async fn keep_up_nodes(db: &sea_orm::DatabaseConnection, user_id: i32) {
    if !on_nodes(&Audience::private(user_id)) {
        return;
    }
    super::super::memory_nodes::sorting::sort_for(db, user_id, None, NODE_SORTS).await;
    let Some(pen) = crate::services::her_log::pen::think() else {
        return;
    };
    let audience = super::super::memory_nodes::audience::private_talk(user_id, None);
    if let Err(error) =
        super::super::memory_nodes::upkeep::run(db, user_id, &audience, NODE_UPKEEPS, &pen).await
    {
        eprintln!("bench call failed: node upkeep: {error:#}");
    }
}

/// Batches of what is on no node sorted at the end, at most.
const NODE_SORTS: usize = 8;
/// Upkeep calls at the end, at most: what another account gets in a day.
const NODE_UPKEEPS: usize = 6;

/// One session the settle way as weeks go by: each of their lines taken as
/// a short note, what she said kept as it is, and the session gone over
/// once it is over. The short notes themselves are not kept: by the next
/// session they are settled or fading. The short notes taken, the lasting
/// facts kept with their evidence, and which of those are notes the answer
/// passed over, kept as they were.
pub(super) async fn over_time(
    db: &sea_orm::DatabaseConnection,
    judge: &crate::services::analyzer::AiAnalyzer,
    user_id: i32,
    present: &Audience,
    session_at: chrono::DateTime<chrono::FixedOffset>,
    turns: &[Turn],
) -> (Vec<String>, Vec<(String, String)>, Vec<String>) {
    let mut notes: Vec<super::super::chat_remember::ChatMemoryUpdate> = Vec::new();
    let mut noted_at = Vec::new();
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
        let Some(updates) = note_turn(db, user_id, present, &text, reply, at).await else {
            continue;
        };
        let_go(db, user_id, present, at, &updates).await;
        notes.extend(
            updates
                .updates
                .iter()
                .filter(|update| update.fact.is_some())
                .cloned(),
        );
        noted_at.resize(notes.len(), at);
        let hers = super::super::chat_remember::ChatMemoryUpdates {
            updates: Vec::new(),
            said: updates.said,
            put_onto: updates.put_onto,
        };
        if !hers.said.is_empty() || !hers.put_onto.is_empty() {
            apply_at(db, user_id, present, at, &hers).await;
        }
    }
    if notes.is_empty() {
        return (Vec::new(), Vec::new(), Vec::new());
    }
    let over_at = session_at + chrono::Duration::minutes(turns.len() as i64);
    let (lasting, passed_over, _) = settle_session(
        db,
        judge,
        user_id,
        present,
        turns,
        &notes,
        &noted_at,
        &[],
        over_at,
        &mut super::note_trail::Trail::off(),
    )
    .await;
    let notes = notes.into_iter().filter_map(|note| note.fact).collect();
    (notes, lasting, passed_over)
}

/// What she knows of them for good, at most, shown beside a stretch (as
/// production's `chat_remember::settle`).
const SETTLE_KNOWN: usize = 40;

/// How long a short note is still there to recall, in the settle way: past
/// this, by the question's date, it has faded. A stand-in for a fast fading
/// memory strength, not a rule production has.
const SHORT_NOTES_LAST: chrono::Duration = chrono::Duration::days(3);

/// Every session of a question, the settle way: what is kept, and what of
/// it came from the turns that answer the question.
#[allow(clippy::too_many_arguments)]
pub(super) async fn go_over(
    db: &sea_orm::DatabaseConnection,
    judge: &crate::services::analyzer::AiAnalyzer,
    user_id: i32,
    sessions: &[super::Session<'_>],
    question_date: &str,
    answering: &mut Vec<String>,
    kept: &mut usize,
    trail: &mut super::note_trail::Trail,
) {
    let asked_at = date_of(question_date);
    // The short notes kept as rows, as production keeps them while a talk
    // goes on, by when their session was over.
    let mut as_rows: Vec<(chrono::DateTime<chrono::FixedOffset>, Vec<String>)> = Vec::new();
    for (session_at, turns, present) in sessions {
        let (session_at, present) = (*session_at, present);
        // Notes of a session long enough before this one have faded
        // (the bench's stand-in for fading, as when they ask).
        let (faded, fresh): (Vec<_>, Vec<_>) = std::mem::take(&mut as_rows)
            .into_iter()
            .partition(|(over, _)| session_at - *over > SHORT_NOTES_LAST);
        as_rows = fresh;
        let_fade(db, faded.into_iter().flat_map(|(_, ids)| ids).collect()).await;
        let mut row_of: Vec<Option<String>> = Vec::new();
        let mut notes: Vec<super::super::chat_remember::ChatMemoryUpdate> = Vec::new();
        let mut noted_at = Vec::new();
        let mut noted: Vec<(String, bool)> = Vec::new();
        let mut short: Vec<(
            chrono::DateTime<chrono::FixedOffset>,
            super::super::chat_remember::ChatMemoryUpdates,
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
        // What she said and what they suggested she try are kept as they are.
        for (at, updates, answer_here) in &short {
            let hers = super::super::chat_remember::ChatMemoryUpdates {
                updates: Vec::new(),
                said: updates.said.clone(),
                put_onto: updates.put_onto.clone(),
            };
            if (!hers.said.is_empty() || !hers.put_onto.is_empty())
                && apply_at(db, user_id, present, *at, &hers).await.is_some()
            {
                *kept += 1;
                if *answer_here {
                    answering.extend(
                        hers.said
                            .iter()
                            .map(|said| super::super::ingest::compact_summary(&said.said)),
                    );
                }
            }
        }
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
                let facts = super::super::chat_remember::ChatMemoryUpdates {
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
                                .map(super::super::ingest::compact_summary),
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
    taken: &[super::super::chat_remember::ChatMemoryUpdate],
) -> Vec<Option<String>> {
    use crate::services::agent::memory::unified;
    let notes = || async {
        unified::latest_of(db, user_id, unified::SHORT_NOTE_SOURCE, 10_000)
            .await
            .unwrap_or_default()
    };
    let before: std::collections::HashSet<String> =
        notes().await.into_iter().map(|row| row.id).collect();
    let updates = super::super::chat_remember::ChatMemoryUpdates {
        updates: taken.to_vec(),
        ..Default::default()
    };
    if let Ok(transaction) = sea_orm::TransactionTrait::begin(db).await {
        // Dated as production dates a note: now, so it has not faded.
        let _ = super::super::store::apply_chat_memory_updates_on(
            &transaction,
            user_id,
            at,
            &updates,
            present,
            super::super::store::Keep::Note,
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
            let fact = super::super::ingest::compact_summary(note.fact.as_deref().unwrap_or(""));
            new.iter()
                .find(|row| super::super::ingest::compact_summary(&row.content) == fact)
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

#[cfg(test)]
mod tests {
    use super::said_before;

    #[test]
    fn a_pass_is_shown_the_turns_said_before_the_rest_begins() {
        let session_at = chrono::DateTime::from_timestamp(1_700_000_000, 0)
            .unwrap()
            .fixed_offset();
        let minute = |index: i64| session_at + chrono::Duration::minutes(index);
        // Six turns, over at minute 6; the rest's first note was taken on
        // the user turn at minute 4: turns 0..=3 were said before it.
        assert_eq!(said_before(6, minute(6), minute(4)), 4);
        assert_eq!(said_before(6, minute(6), minute(0)), 0);
        assert_eq!(said_before(6, minute(6), minute(6)), 6);
    }
}
