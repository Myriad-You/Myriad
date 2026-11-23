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

use myriad_merope::settle_raw::{self as raw, Who};

use super::super::ingest::compact_summary;
use super::super::memory_jobs::{Effect, Failure};
use super::{ChatMemoryUpdate, ChatMemoryUpdates, Said};

/// What she knows of them for good shown beside it: the newest, then what
/// each line of theirs brings to mind (a correction of something old must
/// see the old), at most so many in all.
const KNOWN_NEWEST: usize = 30;
const KNOWN_PER_LINE: usize = 4;
const KNOWN: usize = 60;
/// How far back from the last line gone over a stretch looks for lines
/// written late, in seconds.
pub(in crate::services::agent::merope) const READ_SLACK: i64 = 120;
/// Lines of a stretch read at once, at most: more is gone over in passes.
const LINES: i64 = 80;
/// What she already said of herself shown beside it, at most.
const YOURS: usize = 5;
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

/// One line of the talk, as it was said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::services::agent::merope) struct Spoken {
    /// Its message (`agent_messages.id`).
    pub id: i32,
    pub theirs: bool,
    pub text: String,
    pub at: chrono::DateTime<chrono::FixedOffset>,
    /// Already gone over: shown as context only.
    pub gone: bool,
}

/// The lines of a talk gone over lately (each when it was said, and its
/// message), those within [`READ_SLACK`] of the last: what the next
/// stretch need not go over again.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(in crate::services::agent::merope) struct Read {
    pub lines: Vec<(chrono::DateTime<chrono::FixedOffset>, i32)>,
}

impl Read {
    /// When the last line gone over was said.
    pub fn to(&self) -> Option<chrono::DateTime<chrono::FixedOffset>> {
        self.lines.iter().map(|(at, _)| *at).max()
    }

    pub fn has(&self, id: i32) -> bool {
        self.lines.iter().any(|(_, read)| *read == id)
    }

    /// These read too: only those within [`READ_SLACK`] of the last kept.
    pub fn with(mut self, read: &[(chrono::DateTime<chrono::FixedOffset>, i32)]) -> Self {
        for line in read {
            if !self.has(line.1) {
                self.lines.push(*line);
            }
        }
        if let Some(to) = self.to() {
            self.lines
                .retain(|(at, _)| *at >= to - chrono::Duration::seconds(READ_SLACK));
        }
        self.lines.sort();
        self
    }

    /// Where a stretch from `from` looks for lines from.
    pub fn looks_from(
        read: Option<&Self>,
        from: chrono::DateTime<chrono::FixedOffset>,
    ) -> chrono::DateTime<chrono::FixedOffset> {
        read.and_then(Self::to)
            .map(|to| to - chrono::Duration::seconds(READ_SLACK))
            .map_or(from, |back| back.min(from))
    }
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
/// next stretch's.
pub(in crate::services::agent::merope) fn up_to(
    stretch: Vec<Spoken>,
    last: chrono::DateTime<chrono::FixedOffset>,
) -> Vec<Spoken> {
    let end = stretch
        .iter()
        .position(|line| line.theirs && !line.gone && line.at > last)
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
        .map(|(line, at)| raw::Line {
            who: if line.theirs { Who::Them } else { Who::You },
            text: &line.text,
            in_game: false,
            before: line.gone,
            at: at.as_deref(),
        })
        .collect()
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
    let looks_from = Read::looks_from(read, from);
    let mut stretch = lines(db, user_id, present, looks_from, true).await?;
    for line in &mut stretch {
        line.gone = true;
    }
    for mut line in lines(db, user_id, present, looks_from, false).await? {
        line.gone = read.is_some_and(|read| read.has(line.id));
        stretch.push(line);
    }
    let stretch = up_to(stretch, last);
    if !stretch.iter().any(|line| line.theirs && !line.gone) {
        return Ok(Effect::NoChange);
    }
    let pass = this_pass(&stretch);
    let rest = stretch.get(pass.end).map(|line| line.at);
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
        &this
            .iter()
            .filter(|line| !line.gone)
            .map(|line| Line {
                id: line.id,
                theirs: line.theirs,
                text: line.text.clone(),
                at: line.at,
            })
            .collect::<Vec<_>>(),
    )
    .await;
    let today = super::super::clock::local_now()
        .format("%Y-%m-%d (%A)")
        .to_string();
    let gone = go_over(db, user_id, present, this, &today, site_owner, &placed).await?;
    let read: Vec<_> = this
        .iter()
        .filter(|line| !line.gone)
        .map(|line| (line.at, line.id))
        .collect();
    let updates = gone.updates;
    Ok(Effect::SettledFromLines {
        settled: vec![Vec::new(); updates.updates.len()],
        updates,
        rest,
        read,
    })
}

/// The lines here from `from` on, oldest first, or ([`raw::BEFORE_SHOWN`])
/// the last ones before it.
async fn lines(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    present: &crate::services::agent::memory::unified::Audience,
    from: chrono::DateTime<chrono::FixedOffset>,
    before: bool,
) -> Result<Vec<Spoken>, Failure> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let (side, order, limit) = if before {
        ("<", "DESC", raw::BEFORE_SHOWN as i64)
    } else {
        (">=", "ASC", LINES)
    };
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "SELECT m.id, m.role, m.content, m.created_at FROM agent_messages m \
                 JOIN agent_sessions s ON s.id = m.session_id \
                 WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' \
                   AND (s.context->>'venue') IS NOT DISTINCT FROM $2 \
                   AND m.role IN ('user', 'assistant') AND (m.metadata->>'piece') IS NULL \
                   AND m.created_at {side} $3 \
                 ORDER BY m.created_at {order}, m.id {order} LIMIT $4"
            ),
            [
                user_id.into(),
                present.group_id().map(str::to_string).into(),
                from.into(),
                limit.into(),
            ],
        ))
        .await
        .map_err(|_| Failure::Storage)?;
    let mut lines: Vec<Spoken> = rows
        .iter()
        .filter_map(|row| {
            Some(Spoken {
                id: row.try_get("", "id").ok()?,
                theirs: row.try_get::<String>("", "role").ok()? == "user",
                text: row.try_get("", "content").ok()?,
                at: row.try_get("", "created_at").ok()?,
                gone: false,
            })
        })
        .collect();
    if before {
        lines.reverse();
    }
    Ok(lines)
}

/// One pass gone over: `lines` (those `gone` shown as context only, each
/// with when it was said), what she knows of them (the newest, and what
/// each of their lines brings to mind), the topics on her nodes it most
/// likely touches (`placed` it there first) and what she already said of
/// herself.
/// Their lines it accounts for nowhere are asked about once more, alone.
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
) -> Result<Gone, Failure> {
    use crate::services::agent::memory::unified;
    let rows = unified::active_in(db, user_id, present, &unified::MemoryKind::ABOUT_PERSON)
        .await
        .map_err(|_| Failure::Storage)?;
    let mut known: Vec<&unified::MemoryRecord> =
        rows.iter().filter(|row| row.source == "chat").collect();
    known.sort_by_key(|row| std::cmp::Reverse(row.created_at));
    let mut known: Vec<String> = known
        .into_iter()
        .take(KNOWN_NEWEST)
        .map(|row| compact_summary(&row.content))
        .collect();
    let new = || lines.iter().filter(|line| !line.gone);
    for line in new().filter(|line| line.theirs) {
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
            if known.len() < KNOWN && !fact.is_empty() && !known.contains(&fact) {
                known.push(fact);
            }
        }
    }
    let at: Vec<Option<String>> = lines.iter().map(|line| Some(said_at(line.at))).collect();
    let shown_lines = shown(lines, &at);
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
    let system = raw::raw_settle_system_prompt(
        &known,
        &[],
        myriad_merope::chat_remember::Examples::for_words(&theirs),
    );
    let ask = |only: Option<Vec<usize>>| {
        let input = raw::raw_settle_input(&shown_lines, today, &topics, &yours, only.as_deref());
        let (system, shown_lines, known) = (&system, &shown_lines, &known);
        let offered = offered.len();
        async move {
            let answer = super::super::call::Ask::new(voice(), user_id, "chat_settle")
                .cached(super::super::call::SETTLE_CACHE)
                .within(super::settle::SETTLE_TIMEOUT)
                .output_at_most(OUTPUT)
                .json_raw(
                    system,
                    &input,
                    raw::RAW_SETTLE_SCHEMA_NAME,
                    &raw::raw_settle_schema(),
                )
                .await?;
            raw::parse_raw_settle(&answer, shown_lines, known, &[], offered, only.as_deref())
                .ok_or(Failure::Model(super::super::call::Failure::InvalidOutput))
        }
    };
    let mut got = ask(None).await?;
    let mut asked_again = 0;
    if !got.left.is_empty() {
        asked_again = got.left.len();
        ASKED_AGAIN.fetch_add(asked_again as u64, std::sync::atomic::Ordering::Relaxed);
        // Asked again only about those: what it gives is added; failing,
        // they stay accounted for nowhere, as they were.
        if let Ok(again) = ask(Some(got.left.clone())).await {
            got.settled.lasting.extend(again.settled.lasting);
            got.left = again.left;
        }
    }
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
    let local = |index: usize| (index < lines.len()).then_some(index);
    let index_of = |index: usize| lines.get(index);
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
    for item in got.settled.lasting {
        from.push(item.from.iter().filter_map(|index| local(*index)).collect());
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
    super::already::check_against_unshown(db, user_id, present, &known, &mut updates).await;
    // Checking may drop or merge facts: where each came from is kept only
    // while they still line up.
    if from.len() != updates.updates.len() {
        from = vec![Vec::new(); updates.updates.len()];
    }
    Ok(Gone {
        updates,
        from,
        asked_again,
        left: got.left.len(),
    })
}

#[cfg(test)]
#[path = "settle_raw_tests.rs"]
mod tests;
