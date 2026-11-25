//! Going over a private talk from what was said (`myriad_merope::settle_raw`):
//! no short note is taken line by line there; the talk itself is what she
//! has of it for now, and going over keeps for good what lasts, straight
//! from their lines, with what she said worth keeping having said.
//!
//! The job's `from` is where the lines not yet gone over begin, and it keeps
//! the lines gone over lately ([`Read`]): a line said a moment before the
//! last one gone over but written after it was read is gone over with the
//! next stretch, never passed by, and a line already gone over is shown
//! only as what came before (context), never gone over twice. Both move on
//! only with what going over keeps, in the transaction that writes it
//! (`store::memory_jobs::finish`).

use std::collections::BTreeSet;

use myriad_merope::settle_raw::{self as raw, Who};

use super::super::ingest::compact_summary;
use super::super::memory_jobs::{Effect, Failure};
use super::{ChatMemoryUpdate, ChatMemoryUpdates, Said};
use crate::services::agent::memory::origins;

mod again;
mod lines;
pub(in crate::services::agent::merope) use lines::{ELSEWHERE, Read, SAID, Spoken, words_of};
use lines::{Lines, lines, read_of};
pub(super) use lines::{of_a_game, their_message};

/// What she knows of them for good shown beside it: the newest, then what
/// each line of theirs brings to mind (a correction of something old must
/// see the old), at most so many in all.
const KNOWN_NEWEST: usize = 30;
const KNOWN_PER_LINE: usize = 4;
const KNOWN: usize = 60;
/// Lines of a stretch read at once, at most: more is gone over in passes.
const LINES: i64 = 80;
/// What she already said of herself shown beside it, at most.
const YOURS: usize = 5;
/// Of the time a stretch has, what is left for after the last answer:
/// what it gives checked against what she was not shown, and the rest.
const KEPT_FOR_AFTER: std::time::Duration = std::time::Duration::from_secs(20);
/// Of that, up to this long is for the check.
const CHECKED_AFTER: std::time::Duration = std::time::Duration::from_secs(15);
/// Asked again about lines accounted for nowhere only with this long left.
const ASKED_AGAIN_WITHIN: std::time::Duration = std::time::Duration::from_secs(15);
/// Output a pass may take, thinking included: as much as a keeping call
/// has (`call::keeping`), whichever way it is asked to think.
const OUTPUT: u32 = 16_000;

/// How going over is asked to think: as she keeps what was said
/// (`call::Voice::Keeping`). The memory bench can ask it to think little
/// (`MEROPE_MEMORY_BENCH_SETTLE_THINKING=low`, test builds only), to see
/// what that costs in what is kept.
fn voice() -> super::super::call::Voice {
    #[cfg(test)]
    if std::env::var("MEROPE_MEMORY_BENCH_SETTLE_THINKING").as_deref() == Ok("low") {
        return super::super::call::Voice::Hers;
    }
    super::super::call::Voice::Keeping
}

/// What going over one pass gives: what to keep, and how many of their
/// lines were still accounted for nowhere after asking again about them
/// (none is lost: the line itself stays).
#[derive(Debug, Default)]
pub(in crate::services::agent::merope) struct Gone {
    pub updates: ChatMemoryUpdates,
    /// For each of `updates.updates`, the indexes in the pass's lines it
    /// comes from (a taking back: the line taking it back).
    #[cfg_attr(not(test), expect(dead_code, reason = "read by the memory bench"))]
    pub from: Vec<Vec<usize>>,
    #[cfg_attr(not(test), expect(dead_code, reason = "read by the memory bench"))]
    pub asked_again: usize,
    #[cfg_attr(not(test), expect(dead_code, reason = "read by the memory bench"))]
    pub left: usize,
    /// Their messages (`agent_messages.id`) they took back in the stretch
    /// itself, never kept of it, and those asking her not to keep something.
    pub let_go: Vec<i32>,
}

static LEFT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static ASKED_AGAIN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Lines of theirs gone over and still accounted for nowhere since the site
/// started, and how many were asked about again: counts, never words.
pub(crate) fn unaccounted() -> (u64, u64) {
    use std::sync::atomic::Ordering::Relaxed;
    (LEFT.load(Relaxed), ASKED_AGAIN.load(Relaxed))
}

/// Of `stretch` (oldest first), the lines up to `last`'s and what she said
/// after it, until their next line not gone over: what came after is the
/// next stretch's. A line too short to be one of a stretch ('对') begins
/// none: it is of this.
pub(in crate::services::agent::merope) fn up_to(
    stretch: Vec<Spoken>,
    last: chrono::DateTime<chrono::FixedOffset>,
) -> Vec<Spoken> {
    let end = stretch
        .iter()
        .position(|line| {
            line.theirs && !line.gone && line.at > last && words_of(&line.text).is_some()
        })
        .unwrap_or(stretch.len());
    let mut stretch = stretch;
    stretch.truncate(end);
    stretch
}

/// Where the next pass begins, when `lines` is more than one pass.
pub(in crate::services::agent::merope) fn pass_end(lines: &[Spoken]) -> usize {
    let at = vec![None; lines.len()];
    raw::pass_end(&shown(lines, &at))
}

/// Of `stretch`, what this pass shows: from at most
/// [`raw::BEFORE_SHOWN`] lines gone over before its first line not gone
/// over, up to where the pass ends.
pub(in crate::services::agent::merope) fn this_pass(stretch: &[Spoken]) -> std::ops::Range<usize> {
    let first = stretch
        .iter()
        .position(|line| !line.gone)
        .unwrap_or(stretch.len());
    let start = first.saturating_sub(raw::BEFORE_SHOWN);
    start..start + pass_end(&stretch[start..])
}

/// When a line was said, as she is shown it.
fn said_at(at: chrono::DateTime<chrono::FixedOffset>) -> String {
    at.with_timezone(&super::super::clock::zone())
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

fn shown<'a>(lines: &'a [Spoken], at: &'a [Option<String>]) -> Vec<raw::Line<'a>> {
    lines
        .iter()
        .zip(at)
        .map(|(line, at)| as_shown(line, at.as_deref(), &line.text))
        .collect()
}

fn as_shown<'a>(line: &'a Spoken, at: Option<&'a str>, text: &'a str) -> raw::Line<'a> {
    raw::Line {
        who: if line.theirs { Who::Them } else { Who::You },
        text,
        in_game: line.in_game,
        before: line.gone,
        at,
    }
}

/// `lines` as a pass is shown them, a long one part by part
/// ([`raw::line_parts`]): the parts, and of each its line's index. Each part
/// is accounted for.
fn in_parts<'a>(lines: &'a [Spoken], at: &'a [Option<String>]) -> (Vec<raw::Line<'a>>, Vec<usize>) {
    let mut parts = Vec::new();
    let mut of = Vec::new();
    for (index, (line, at)) in lines.iter().zip(at).enumerate() {
        for part in raw::line_parts(&line.text) {
            parts.push(as_shown(line, at.as_deref(), part));
            of.push(index);
        }
    }
    (parts, of)
}

/// `lines`, each part of a long one a line of its own of the same message,
/// so a pass may end within it; of the one gone over in part, the parts that
/// were are gone.
pub(in crate::services::agent::merope) fn by_part(
    lines: Vec<Spoken>,
    partway: Option<(i32, usize)>,
) -> Vec<Spoken> {
    let mut parts = Vec::new();
    for line in lines {
        for (nth, text) in raw::line_parts(&line.text).into_iter().enumerate() {
            let gone = partway.is_some_and(|(id, done)| line.id == id && nth < done);
            parts.push(Spoken {
                text: text.to_string(),
                gone: line.gone || gone,
                ..line.clone()
            });
        }
    }
    parts
}

/// Of `lines`, those that say what `evidence` quotes, each part of it on its
/// own: few words are all a line says, not a part of another, unless none
/// says only them.
fn saying(evidence: &str, lines: &[&Spoken]) -> Vec<i32> {
    let said: Vec<String> = (lines.iter())
        .map(|line| origins::folded(&line.text))
        .collect();
    let mut saying = BTreeSet::new();
    for quote in origins::quotes(Some(evidence)) {
        let surely = quote.strictly();
        let mut of: Vec<usize> = (0..said.len())
            .filter(|at| surely.is_in(&said[*at]))
            .collect();
        if of.is_empty() {
            of.extend((0..said.len()).filter(|at| said[*at].contains(&quote.text)));
        }
        saying.extend(of);
    }
    saying.into_iter().map(|at| lines[at].id).collect()
}

/// Of the messages `lines` are of, those said whole in `let_go` (indexes):
/// one only a part of which they took back is not let go.
fn whole(lines: &[Spoken], parts: &[raw::Line], of: &[usize], let_go: &[usize]) -> Vec<i32> {
    let mut ids: Vec<i32> = Vec::new();
    for index in let_go.iter().filter_map(|part| of.get(*part)) {
        let id = lines[*index].id;
        let all = (0..parts.len())
            .filter(|part| lines[of[*part]].id == id)
            .all(|part| let_go.contains(&part) || (!parts[part].before && !parts[part].to_place()));
        if all && !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// A private talk's stretch, gone over (`memory_jobs::Payload::Settle`):
/// its lines from `from` (and any written late since those `read`), up to
/// `last`'s and her answer, placed on her nodes and gone over, a pass at a
/// time. What this pass read comes back with what it keeps.
pub(in crate::services::agent::merope) async fn settle_raw(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    present: &crate::services::agent::memory::unified::Audience,
    from: chrono::DateTime<chrono::FixedOffset>,
    last: chrono::DateTime<chrono::FixedOffset>,
    read: Option<&Read>,
) -> Result<Effect, Failure> {
    let started = std::time::Instant::now();
    let looks_from = Read::looks_from(read, from);
    let mut stretch = lines(db, user_id, present, looks_from, Lines::Before).await?;
    for line in &mut stretch {
        line.gone = true;
    }
    // Those read already do not crowd out the ones that were not.
    let at_most = LINES + read.map_or(0, |read| read.lines.len() as i64);
    let found = lines(db, user_id, present, looks_from, Lines::From(at_most)).await?;
    let more = found.len() as i64 >= at_most;
    let found_to = found.last().map(|line| line.at);
    for mut line in found {
        line.gone |= read.is_some_and(|read| read.has(line.id));
        stretch.push(line);
    }
    let here = stretch.len();
    let mut stretch = up_to(stretch, last);
    // As many as are read at once, and the stretch does not end among them:
    // the rest of it is gone over next.
    let next = (more && stretch.len() == here)
        .then_some(found_to)
        .flatten()
        .map(|at| at + chrono::Duration::microseconds(1));
    if !stretch.iter().any(|line| line.theirs && !line.gone) {
        return Ok(match next {
            Some(_) => Effect::SettledFromLines {
                updates: ChatMemoryUpdates::default(),
                settled: Vec::new(),
                rest: next,
                read: read_of(&stretch),
                partway: None,
                let_go: Vec::new(),
            },
            None => Effect::NoChange,
        });
    }
    let was_partway = read.and_then(|read| read.partway);
    stretch = by_part(stretch, was_partway);
    let pass = this_pass(&stretch);
    let rest = stretch.get(pass.end).map(|line| line.at).or(next);
    // Ending within a long message: its parts up to there are gone over.
    let partway = match (
        pass.end.checked_sub(1).map(|last| stretch[last].id),
        stretch.get(pass.end),
    ) {
        (Some(last), Some(next)) if last == next.id => Some((
            last,
            stretch[..pass.end]
                .iter()
                .filter(|line| line.id == last)
                .count(),
        )),
        _ => None,
    };
    let this = &stretch[pass];
    let site_owner = crate::services::principal::site_owner_id(db)
        .await
        .ok()
        .flatten();
    use super::super::memory_nodes::placing::{self, Line};
    let placed = placing::place(
        db,
        user_id,
        site_owner,
        &messages(this, &stretch, was_partway.map(|(id, _)| id))
            .into_iter()
            .map(|(line, text)| Line {
                id: line.id,
                theirs: line.theirs,
                text,
                at: line.at,
            })
            .collect::<Vec<_>>(),
    )
    .await;
    let today = super::super::clock::local_now()
        .format("%Y-%m-%d (%A)")
        .to_string();
    // What is asked after the first answer has what is left of the time.
    let until = started + super::super::memory_jobs::SETTLE_LIMIT - KEPT_FOR_AFTER;
    let gone = go_over(
        db,
        user_id,
        present,
        this,
        &today,
        site_owner,
        &placed,
        Some(until),
    )
    .await?;
    let mut read = read_of(this);
    read.retain(|(_, id)| partway.is_none_or(|(partway, _)| *id != partway));
    let updates = gone.updates;
    // Of a message another pass goes over the rest of, only a part is told.
    let mut let_go = gone.let_go;
    let_go.retain(|id| {
        ![partway, was_partway]
            .into_iter()
            .flatten()
            .any(|(partway, _)| partway == *id)
    });
    Ok(Effect::SettledFromLines {
        settled: vec![Vec::new(); updates.updates.len()],
        updates,
        rest,
        read,
        partway,
        let_go,
    })
}

/// The messages this pass begins to go over, each with the whole of what it
/// says in `stretch`: what is gone over now or was before in a pass that
/// ended within it (`begun`) is not given again.
fn messages<'a>(
    this: &'a [Spoken],
    stretch: &[Spoken],
    begun: Option<i32>,
) -> Vec<(&'a Spoken, String)> {
    let mut messages: Vec<(&Spoken, String)> = Vec::new();
    for line in this.iter().filter(|line| !line.gone) {
        if Some(line.id) == begun || messages.iter().any(|(one, _)| one.id == line.id) {
            continue;
        }
        let whole = stretch
            .iter()
            .filter(|part| part.id == line.id)
            .map(|part| part.text.as_str())
            .collect();
        messages.push((line, whole));
    }
    messages
}

/// What she knows of them for good, as a pass is shown it.
#[derive(Debug, Default, PartialEq, Eq)]
pub(in crate::services::agent::merope) struct Known {
    /// The newest, then what each line of theirs brings to mind.
    pub known: Vec<String>,
    /// What they asked her not to keep, all of it, beside that: never
    /// crowded out of it, and holding wherever they asked it.
    pub asked: Vec<String>,
    /// Of those, what they may have let her keep again, not looked at yet.
    pub pending: Vec<String>,
}

pub(in crate::services::agent::merope) async fn what_she_knows(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    present: &crate::services::agent::memory::unified::Audience,
    lines: &[Spoken],
) -> Result<Known, Failure> {
    use crate::services::agent::memory::unified;
    let held = unified::asked_not_to_keep(db, user_id)
        .await
        .map_err(|_| Failure::Storage)?;
    let ids: Vec<String> = held.iter().map(|row| row.id.clone()).collect();
    let waiting = super::super::memory_nodes::take_back_check::waiting_of(db, user_id, &ids)
        .await
        .map_err(|_| Failure::Storage)?;
    let says = |row: &unified::MemoryRecord| compact_summary(&row.content);
    let pending = (held
        .iter()
        .filter(|row| waiting.contains(&row.id))
        .map(says))
    .collect();
    let asked: Vec<String> = held.iter().map(says).collect();
    let rows = unified::active_in(db, user_id, present, &unified::MemoryKind::ABOUT_PERSON)
        .await
        .map_err(|_| Failure::Storage)?;
    // What she kept for good from talks here alone can be changed by this:
    // what another end kept, her events and short notes are not among it.
    let mut known: Vec<&unified::MemoryRecord> =
        rows.iter().filter(|row| row.source == "chat").collect();
    let hers: std::collections::HashSet<String> = known
        .iter()
        .map(|row| compact_summary(&row.content))
        .collect();
    known.sort_by_key(|row| std::cmp::Reverse(row.created_at));
    let mut known: Vec<String> = known
        .into_iter()
        .take(KNOWN_NEWEST)
        .map(|row| compact_summary(&row.content))
        .collect();
    for line in lines.iter().filter(|line| !line.gone && line.theirs) {
        let (recalled, _) = unified::quietly(super::super::store::recall_remembered_primed(
            db,
            user_id,
            present,
            Some(&line.text),
            KNOWN_PER_LINE,
            &unified::Priming::default(),
            1.0,
        ))
        .await
        .map_err(|_| Failure::Storage)?;
        for fact in recalled {
            if known.len() < KNOWN
                && !fact.is_empty()
                && !known.contains(&fact)
                && hers.contains(&fact)
            {
                known.push(fact);
            }
        }
    }
    Ok(Known {
        known,
        asked,
        pending,
    })
}

/// One pass gone over: `lines` (those `gone` shown as context only, each
/// with when it was said), what she knows of them (the newest, and what
/// each of their lines brings to mind), what they asked her not to keep, the topics on her nodes it most
/// likely touches (`placed` it there first) and what she already said of
/// herself.
/// Their lines it accounts for nowhere are asked about once more, alone,
/// in the time left before `until`.
/// The bench goes over its sessions with this too.
#[allow(clippy::too_many_arguments)]
pub(in crate::services::agent::merope) async fn go_over(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    present: &crate::services::agent::memory::unified::Audience,
    lines: &[Spoken],
    today: &str,
    site_owner: Option<i32>,
    placed: &[String],
    until: Option<std::time::Instant>,
) -> Result<Gone, Failure> {
    use crate::services::agent::memory::unified;
    let Known {
        known,
        asked,
        pending,
    } = what_she_knows(db, user_id, present, lines).await?;
    let new = || lines.iter().filter(|line| !line.gone);
    let at: Vec<Option<String>> = lines.iter().map(|line| Some(said_at(line.at))).collect();
    let (shown_lines, of) = in_parts(lines, &at);
    let theirs = new()
        .filter(|line| line.theirs)
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if theirs.trim().is_empty() {
        return Ok(Gone::default());
    }
    let offered = if present.is_group() {
        Vec::new()
    } else {
        super::super::memory_nodes::facts::shown_topics(db, user_id, site_owner, &theirs, placed)
            .await
    };
    let talk = new()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let yours: Vec<String> = super::super::store::said_of_herself(db, present, &talk, YOURS)
        .await
        .into_iter()
        .map(|said| said.said)
        .collect();
    let topics = super::settle::shown_topics(&offered);
    let examples = myriad_merope::chat_remember::Examples::for_words(&theirs);
    let system = raw::raw_settle_system_prompt(&known, &asked, examples);
    let ask = |only: Option<Vec<usize>>,
               within: std::time::Duration,
               system: &str,
               asked: &[String]| {
        let input = raw::raw_settle_input(&shown_lines, today, &topics, &yours, only.as_deref());
        let (system, asked) = (system.to_string(), asked.to_vec());
        let (shown_lines, known, pending) = (&shown_lines, &known, &pending);
        let offered = offered.len();
        async move {
            let answer = super::super::call::Ask::new(voice(), user_id, "chat_settle")
                .cached(super::super::call::SETTLE_CACHE)
                .within(within)
                .output_at_most(OUTPUT)
                .json_raw(
                    &system,
                    &input,
                    raw::RAW_SETTLE_SCHEMA_NAME,
                    &raw::raw_settle_schema(),
                )
                .await?;
            let read = |asked: &[String]| {
                let only = only.as_deref();
                raw::parse_raw_settle(&answer, shown_lines, known, asked, offered, only)
            };
            let mut got =
                read(&asked).ok_or(Failure::Model(super::super::call::Failure::InvalidOutput))?;
            // Asked for over again while it waits for that second look: it
            // is held to, whatever they let her before (`held_to`).
            if !pending.is_empty() {
                let others: Vec<String> =
                    (asked.iter().filter(|one| !pending.contains(one)).cloned()).collect();
                let again = read(&others).map(|got| got.asked).unwrap_or_default();
                (got.asked).extend(again.into_iter().filter(|one| pending.contains(&one.fact)));
            }
            Ok::<_, Failure>(got)
        }
    };
    let time_left = |longest: std::time::Duration, after: std::time::Duration| {
        until.map_or(longest, |until| {
            (until + after)
                .saturating_duration_since(std::time::Instant::now())
                .min(longest)
        })
    };
    let settle_within = || time_left(super::settle::SETTLE_TIMEOUT, std::time::Duration::ZERO);
    let got = ask(None, settle_within(), &system, &asked).await?;
    let check_within = || {
        time_left(
            super::super::memory_nodes::take_back_check::TIMEOUT,
            CHECKED_AFTER,
        )
    };
    let again_prompt = again::prompt_for_again(&known, &asked, &got.asked, examples);
    let mut asked_again = 0;
    let within = settle_within();
    let ask_again = async {
        if got.left.is_empty() || within < ASKED_AGAIN_WITHIN {
            return None;
        }
        asked_again = got.left.len();
        ASKED_AGAIN.fetch_add(asked_again as u64, std::sync::atomic::Ordering::Relaxed);
        let (again_asked, again_system) = again::prompt_parts(&again_prompt, &asked, &system);
        // Asked again only about those (with pass-1 `notToKeep` in view): what
        // it gives is added; failing, they stay accounted for nowhere, as they were.
        ask(Some(got.left.clone()), within, again_system, again_asked)
            .await
            .ok()
    };
    let (told, again) = tokio::join!(
        again::doubts(user_id, &got, &shown_lines, check_within()),
        ask_again
    );
    let check_until = until.map(|until| until + CHECKED_AFTER);
    let got = again::with_second_pass(
        user_id,
        got,
        again,
        &told,
        &shown_lines,
        check_until,
    )
    .await;
    if !got.left.is_empty() {
        LEFT.fetch_add(got.left.len() as u64, std::sync::atomic::Ordering::Relaxed);
        tracing::info!(
            user_id,
            lines = got.left.len(),
            "[chat-remember] lines of theirs gone over and accounted for nowhere: they stay as said"
        );
    }
    let mut updates = ChatMemoryUpdates::default();
    let mut from: Vec<Vec<usize>> = Vec::new();
    let local = |part: usize| of.get(part).copied();
    let index_of = |part: usize| local(part).and_then(|index| lines.get(index));
    // What they took back first, with nothing in its place, each on its own.
    for one in got.settled.took_back {
        if let Some(line) = one.said.and_then(index_of) {
            updates.taken_back_in.insert(updates.updates.len(), line.id);
        }
        from.push(one.said.and_then(local).into_iter().collect());
        updates.updates.push(ChatMemoryUpdate {
            standing: Default::default(),
            fact: None,
            supersedes: vec![one.entry],
            evidence: None,
            concepts: Vec::new(),
            topic: None,
            cues: Vec::new(),
        });
    }
    // A line that only asks her not to keep something goes with what they
    // took back, unless something is kept of it; any other is judged by what
    // is asked, as every line is (`memory_jobs::Payload::NotKept`).
    let kept_from: BTreeSet<usize> = (got.settled.lasting.iter())
        .flat_map(|fact| fact.from.iter().copied())
        .collect();
    let mut going = got.let_go.clone();
    going.extend((got.asked.iter()).filter_map(|one| {
        let part = one.said.filter(|part| !kept_from.contains(part))?;
        let said = origins::folded(shown_lines.get(part)?.text);
        let mut quotes = origins::quotes(Some(&one.evidence)).into_iter();
        quotes
            .any(|quote| {
                origins::Quote {
                    whole: true,
                    ..quote
                }
                .is_in(&said)
            })
            .then_some(part)
    }));
    let let_go = whole(lines, &shown_lines, &of, &going);
    for item in got.settled.lasting {
        let mut rests_on: Vec<usize> = item.from.iter().filter_map(|part| local(*part)).collect();
        rests_on.dedup();
        let of: Vec<&Spoken> = rests_on.iter().map(|index| &lines[*index]).collect();
        let quoting = saying(&item.evidence, &of);
        for line in &of {
            let told = quoting.is_empty() || quoting.contains(&line.id);
            updates.rests_on(&item.evidence, told.then_some(line.id), line.at);
        }
        from.push(rests_on);
        updates.updates.push(ChatMemoryUpdate {
            standing: item.standing.clone(),
            fact: Some(compact_summary(&item.fact)),
            supersedes: item.replaces.clone(),
            evidence: Some(item.evidence),
            concepts: unified::clean_concepts(
                serde_json::from_value(item.concepts).unwrap_or_default(),
            ),
            cues: item.when_for,
            topic: super::settle::goes_under(&item.topic, &offered),
        });
    }
    for said in got.said {
        let of: Vec<&Spoken> = new()
            .filter(|line| !line.theirs && origins::folded(&line.text).contains(&said.evidence))
            .collect();
        let mut quoting = saying(&said.evidence, &of);
        if quoting.is_empty() {
            quoting.extend(of.first().map(|line| line.id));
        }
        for line in of {
            let told = quoting.contains(&line.id).then_some(line.id);
            updates.rests_on(&said.evidence, told, line.at);
        }
        updates.said.push(Said {
            said: compact_summary(&said.said),
            evidence: said.evidence,
            hers: said.hers,
            concepts: serde_json::from_value(said.concepts)
                .map(unified::clean_concepts)
                .unwrap_or_default(),
        });
    }
    updates.put_onto = got.put_onto;
    // What she has but was not shown: the same thing said another way is
    // not kept twice.
    let within = time_left(super::already::TIMEOUT, CHECKED_AFTER);
    super::already::check_for_good(db, user_id, present, &known, &mut updates, within).await;
    // Checking may drop or merge facts: where each came from is kept only
    // while they still line up.
    if from.len() != updates.updates.len() {
        from = vec![Vec::new(); updates.updates.len()];
    }
    // What they asked her not to keep, each in the line asking it; after
    // what is checked, for each is known by its place.
    for one in got.asked {
        from.push(one.said.and_then(local).into_iter().collect());
        if let Some(line) = one.said.and_then(index_of) {
            updates.rests_on(&one.evidence, Some(line.id), line.at);
        }
        updates.ask_not_to_keep(one);
    }
    Ok(Gone {
        updates,
        from,
        asked_again,
        left: got.left.len(),
        let_go,
    })
}

#[cfg(test)]
#[path = "settle_raw_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "settle_raw_db_tests.rs"]
mod db_tests;
