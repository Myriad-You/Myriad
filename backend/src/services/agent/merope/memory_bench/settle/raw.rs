//! A private session the settle way as production goes over a private talk
//! now (`chat_remember::settle_raw`): nothing is noted line by line; the
//! session is gone over from what was said, with production's own
//! function, whenever production would: each time its box is full
//! (`myriad_merope::settle_raw::box_is_full`, counted as production counts
//! it: each line of theirs, and her answer to it) and once it is over (its
//! turns a minute apart, it never goes quiet before). Each time in passes,
//! as production's job goes on (`settle_raw::pass_end`), and tried again as
//! production tries a stretch again.

use super::*;
use crate::services::agent::merope::chat_remember::settle_raw::{self, Spoken};

/// Lines longer than this are left out of what is gone over (and marked so
/// on the trail), as the bench has always left them.
const TOO_LONG: usize = 2_000;

/// What a session gone over kept: each lasting fact with its evidence, and
/// how many updates were written in all.
#[derive(Default)]
pub(in super::super) struct Kept {
    pub lasting: Vec<(String, String)>,
    pub written: usize,
    /// Facts (and what she said) that came from a line answering the
    /// question.
    pub answering: Vec<String>,
}

/// Where the session's lines are cut to be gone over: after each line at
/// which production's box fills, and at the end.
pub(in super::super) fn stretches(lines: &[Spoken]) -> Vec<std::ops::Range<usize>> {
    let mut cuts = Vec::new();
    let (mut start, mut theirs, mut answered) = (0, 0, 0);
    for (index, line) in lines.iter().enumerate() {
        if line.theirs {
            theirs += 1;
        } else if index > start && lines[index - 1].theirs {
            answered += 1;
        }
        if myriad_merope::settle_raw::box_is_full(theirs + answered) {
            cuts.push(start..index + 1);
            (start, theirs, answered) = (index + 1, 0, 0);
        }
    }
    if start < lines.len() {
        cuts.push(start..lines.len());
    }
    cuts
}

pub(in super::super) async fn session(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    present: &Audience,
    session_at: chrono::DateTime<chrono::FixedOffset>,
    turns: &[Turn],
    trail: &mut super::super::note_trail::Trail,
) -> Kept {
    let mut lines: Vec<Spoken> = Vec::new();
    // Which of `lines` answer the question.
    let mut answer_here: Vec<bool> = Vec::new();
    for (index, turn) in turns.iter().enumerate() {
        let text: String = turn
            .content
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let theirs = turn.role == "user";
        let answers =
            theirs && (turn.has_answer || turns.get(index + 1).is_some_and(|next| next.has_answer));
        if text.chars().count() > TOO_LONG {
            if answers {
                trail.answer_line(&text, 0, true);
            }
            continue;
        }
        lines.push(Spoken {
            id: index as i32 + 1,
            theirs,
            text,
            at: session_at + chrono::Duration::minutes(index as i64),
            gone: false,
        });
        answer_here.push(answers);
    }
    let mut kept = Kept::default();
    // How many facts each line gave, for the trail.
    let mut gave = vec![0usize; lines.len()];
    for stretch in stretches(&lines) {
        let mut start = stretch.start;
        while start < stretch.end {
            let end = start + settle_raw::pass_end(&lines[start..stretch.end]);
            let before =
                &lines[start.saturating_sub(myriad_merope::settle_raw::BEFORE_SHOWN)..start];
            let pass = &lines[start..end];
            let over_at = pass.last().map_or(session_at, |line| line.at);
            let Some(gone) = Box::pin(one_pass(db, user_id, present, before, pass, over_at)).await
            else {
                // Out of tries: production leaves it to the next stretch;
                // the bench, as before, goes on without it.
                break;
            };
            for (update, from) in gone.updates.updates.iter().zip(&gone.from) {
                let Some(fact) = update.fact.as_deref() else {
                    continue;
                };
                let fact = super::super::super::ingest::compact_summary(fact);
                for index in from {
                    gave[start + index] += 1;
                }
                if from.iter().any(|index| answer_here[start + index]) {
                    kept.answering.push(fact.clone());
                }
                kept.lasting
                    .push((fact, update.evidence.clone().unwrap_or_default()));
            }
            if pass
                .iter()
                .zip(&answer_here[start..end])
                .any(|(_, here)| *here)
            {
                kept.answering.extend(
                    gone.updates
                        .said
                        .iter()
                        .map(|said| super::super::super::ingest::compact_summary(&said.said)),
                );
            }
            let updates = &gone.updates;
            if !(updates.updates.is_empty()
                && updates.said.is_empty()
                && updates.put_onto.is_empty())
            {
                let settled = vec![Vec::new(); updates.updates.len()];
                if super::super::writing::settle_at(
                    db, user_id, present, over_at, updates, &settled,
                )
                .await
                .is_some()
                {
                    kept.written +=
                        updates.updates.len() + updates.said.len() + updates.put_onto.len();
                }
                super::super::super::memory_nodes::facts::settled(
                    db, user_id, None, present, updates,
                )
                .await;
            }
            start = end;
        }
    }
    for (index, line) in lines.iter().enumerate() {
        if answer_here[index] {
            trail.answer_line(&line.text, gave[index], false);
        }
    }
    kept
}

/// One pass, placed on her nodes first as production places it, gone over
/// with production's function and tried again as production tries it.
async fn one_pass(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    present: &Audience,
    before: &[Spoken],
    pass: &[Spoken],
    over_at: chrono::DateTime<chrono::FixedOffset>,
) -> Option<settle_raw::Gone> {
    let mut placed = Vec::new();
    if on_nodes(present) {
        use super::super::super::memory_nodes::placing::{self, Line};
        let lines: Vec<Line> = pass
            .iter()
            .map(|line| Line {
                id: line.id,
                theirs: line.theirs,
                text: line.text.clone(),
                at: line.at,
            })
            .collect();
        placed = placing::place(db, user_id, None, &lines).await;
    }
    let today = over_at
        .with_timezone(&super::super::super::clock::zone())
        .format("%Y-%m-%d (%A)")
        .to_string();
    // What came before shown ahead of the pass, as already gone over.
    let shown: Vec<Spoken> = before
        .iter()
        .cloned()
        .map(|line| Spoken { gone: true, ..line })
        .chain(pass.iter().cloned())
        .collect();
    let (today, placed, shown) = (&today, &placed, &shown);
    let started = std::time::Instant::now();
    let gone = crate::services::ai_cost_ledger::spent::tried_as_production_tries(
        super::super::super::store::memory_jobs::MAX_ATTEMPTS,
        |tried| {
            std::time::Duration::from_secs(
                super::super::super::store::memory_jobs::again_in(tried).unsigned_abs(),
            )
        },
        || {
            Box::pin(settle_raw::go_over(
                db, user_id, present, shown, today, None, placed,
            ))
        },
        // Production tries a stretch again on an unreadable answer too.
        |gone| {
            gone.as_ref()
                .is_err_and(|failure| failure.retryable() || failure.unreadable())
        },
    )
    .await;
    super::super::note_trail::keep_raw_settle_call(
        started.elapsed(),
        gone.as_ref().ok().map(|gone| (gone.asked_again, gone.left)),
    );
    match gone {
        // Where each came from, as indexes in the pass.
        Ok(mut gone) => {
            for from in &mut gone.from {
                *from = from
                    .iter()
                    .filter_map(|index| index.checked_sub(before.len()))
                    .collect();
            }
            Some(gone)
        }
        Err(error) => {
            eprintln!("bench call failed: settling: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(index: usize, theirs: bool) -> Spoken {
        Spoken {
            id: index as i32 + 1,
            theirs,
            text: "说了点什么".to_string(),
            at: chrono::DateTime::from_timestamp(1_700_000_000 + index as i64 * 60, 0)
                .unwrap()
                .fixed_offset(),
            gone: false,
        }
    }

    /// Their line and her answer are two waiting; the box is full at
    /// twelve, after her sixth answer, as production counts.
    #[test]
    fn a_session_is_gone_over_each_time_the_box_fills_and_at_its_end() {
        let lines: Vec<Spoken> = (0..30).map(|index| line(index, index % 2 == 0)).collect();
        assert_eq!(stretches(&lines), [0..12, 12..24, 24..30]);
        // Lines of theirs with no answer between count once each.
        let alone: Vec<Spoken> = (0..13).map(|index| line(index, true)).collect();
        assert_eq!(stretches(&alone), [0..12, 12..13]);
        assert!(stretches(&[]).is_empty());
    }
}
