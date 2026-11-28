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
        lines.push(Spoken {
            id: index as i32 + 1,
            theirs,
            text,
            at: session_at + chrono::Duration::minutes(index as i64),
            gone: false,
            in_game: false,
        });
        answer_here.push(answers);
    }
    let (parts, part_of, turn_parts) = split_parts(&lines);
    let mut kept = Kept::default();
    // How many facts each line gave, for the trail.
    let mut gave = vec![0usize; lines.len()];
    for stretch in stretches(&lines) {
        let s_start = turn_parts[stretch.start].0;
        let s_end = turn_parts[stretch.end - 1].1;
        let mut start = s_start;
        while start < s_end {
            let end = start + settle_raw::pass_end(&parts[start..s_end]);
            let before =
                &parts[start.saturating_sub(myriad_merope::settle_raw::BEFORE_SHOWN)..start];
            let pass = &parts[start..end];
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
                let mut orig: Vec<usize> = from.iter().map(|index| part_of[start + index]).collect();
                orig.sort_unstable();
                orig.dedup();
                for index in &orig {
                    gave[*index] += 1;
                }
                if orig.iter().any(|index| answer_here[*index]) {
                    kept.answering.push(fact.clone());
                }
                kept.lasting
                    .push((fact, update.evidence.clone().unwrap_or_default()));
            }
            if (start..end).any(|index| answer_here[part_of[index]]) {
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
                && updates.asked.is_empty()
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
                if !super::super::ablation::current().no_nodes {
                    super::super::super::memory_nodes::facts::settled(
                        db, user_id, None, present, updates,
                    )
                    .await;
                }
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

/// Split turns into <= `PART_CHARS` parts (matching production `by_part`),
/// returning `(parts, part_of_turn, turn_part_ranges)`.
fn split_parts(lines: &[Spoken]) -> (Vec<Spoken>, Vec<usize>, Vec<(usize, usize)>) {
    let mut parts = Vec::new();
    let mut part_of = Vec::new();
    let mut turn_parts = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        let start = parts.len();
        for text in myriad_merope::settle_raw::line_parts(&line.text) {
            parts.push(Spoken {
                text: text.to_string(),
                ..line.clone()
            });
            part_of.push(index);
        }
        turn_parts.push((start, parts.len()));
    }
    (parts, part_of, turn_parts)
}

/// Join multi-part messages sharing a turn id into whole lines for `placing`.
fn whole_messages(pass: &[Spoken]) -> Vec<super::super::super::memory_nodes::placing::Line> {
    use super::super::super::memory_nodes::placing::Line;
    let mut merged: Vec<Line> = Vec::new();
    for part in pass {
        if let Some(last) = merged.last_mut().filter(|line| line.id == part.id) {
            last.text.push_str(&part.text);
        } else {
            merged.push(Line {
                id: part.id,
                theirs: part.theirs,
                text: part.text.clone(),
                at: part.at,
            });
        }
    }
    merged
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
    if on_nodes(present) && !super::super::ablation::current().no_nodes {
        let lines = whole_messages(pass);
        placed = super::super::super::memory_nodes::placing::place(db, user_id, None, &lines).await;
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
                db, user_id, present, shown, today, None, placed, None,
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
        gone.as_ref().ok().map(|gone| &gone.dropped),
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
            in_game: false,
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

    #[test]
    fn long_user_and_assistant_turns_are_split_into_parts_not_dropped() {
        let long_user = "我喜欢喝手冲咖啡。".repeat(300);
        let long_assistant = "那我推荐试试浅烘的耶加雪菲，".repeat(300);
        assert!(long_user.chars().count() > 2_000);
        assert!(long_assistant.chars().count() > 2_000);
        let turns = [
            Spoken {
                text: long_user.clone(),
                ..line(0, true)
            },
            Spoken {
                text: long_assistant.clone(),
                ..line(1, false)
            },
        ];
        assert_eq!(stretches(&turns), std::slice::from_ref(&(0..2)));
        let (parts, part_of, turn_parts) = split_parts(&turns);
        assert!(parts.len() > 2);
        assert!(
            parts
                .iter()
                .all(|part| part.text.chars().count() <= myriad_merope::settle_raw::PART_CHARS)
        );
        assert_eq!(turn_parts.len(), 2);
        assert_eq!(turn_parts[0].0, 0);
        assert_eq!(turn_parts[1].1, parts.len());
        assert!(part_of[turn_parts[0].0..turn_parts[0].1].iter().all(|&i| i == 0));
        assert!(part_of[turn_parts[1].0..turn_parts[1].1].iter().all(|&i| i == 1));
        let whole = whole_messages(&parts);
        assert_eq!(whole.len(), 2);
        assert_eq!(whole[0].text, long_user);
        assert_eq!(whole[1].text, long_assistant);
    }
}
