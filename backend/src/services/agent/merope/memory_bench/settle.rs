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
    // Asked as production asks it (`call::Voice::Keeping`: the model's own
    // thinking), and tried again as production tries a stretch again, an
    // unreadable answer too, as many times.
    let keeping = crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(
        super::super::chat_remember::settle::SETTLE_TIMEOUT,
    ))
    .await;
    let model = keeping.as_ref().unwrap_or(judge);
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
            &turns
                .iter()
                .map(|turn| turn.role == "user")
                .collect::<Vec<_>>(),
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
    let Some((
        raw,
        myriad_merope::chat_remember::Settled {
            lasting: kept,
            took_back,
            ..
        },
    )) = read
    else {
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
    // What they took back first, with nothing in its place, each on its
    // own, as production does.
    for one in took_back {
        from_rows.push(Vec::new());
        updates
            .updates
            .push(super::super::chat_remember::ChatMemoryUpdate {
                standing: Default::default(),
                topic: None,
                cues: Vec::new(),
                fact: None,
                supersedes: vec![one.entry],
                evidence: None,
                concepts: Vec::new(),
            });
    }
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
                // What it says has changed, as production keeps it.
                supersedes: item.replaces.clone(),
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
            said_new: super::super::chat_remember::SaidNew::Nothing,
            taken_back_in: Default::default(),
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

mod going_over;
pub(super) use going_over::go_over;

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
