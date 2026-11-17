//! The lines of her private chat with someone that a chat turn may look
//! back through: their own talk with her, as the chat in front of her has
//! it, and nothing they took back. Scrolling back by words
//! ([`super::remembering::look_back`]), turning back to a day
//! ([`super::chat_days`]) and finding by meaning ([`super::chat_near`]) all
//! read these.
//!
//! - **Whose and where.** Their private chat only: never a group's (no
//!   venue), never a work session (a session with no mode written is chat),
//!   never an archived conversation, never a piece of work coming back.
//! - **Taken back.** What a memory they took back quotes is not brought up
//!   again, nor her answer to it ([`withdrawn`]). 「当我没说」 keeps the
//!   words in the chat (CONTEXT: it means not bringing them up, not
//!   deleting): only looking back leaves them out.
//! - **Replaced since.** What a memory quotes that a later fact took the
//!   place of (the old fact is no longer shown) is still what they said
//!   that day, and is found; but its day says so where she reads it
//!   ([`found_on`]), so their old words do not read as how things are
//!   beside the fact that took their place. The words themselves stay as
//!   said: what is matched, given a vector or written into a day is them.

use std::collections::HashSet;

use chrono::{DateTime, FixedOffset};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use crate::services::agent::memory::origins;

/// One line of their private chat.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Line {
    pub(crate) id: i32,
    pub(crate) at: DateTime<FixedOffset>,
    pub(crate) hers: bool,
    pub(crate) content: String,
    /// A fact kept from this line of theirs has since been replaced by a
    /// later one she still keeps.
    pub(crate) replaced: bool,
}

/// Beside the day of a line of theirs she looked back to, when what she kept
/// from it was replaced by a later fact.
pub(crate) const REPLACED_SINCE: &str = "what you kept from this was later replaced";

/// The `day` of a line as she reads it when found: with that said beside
/// it when what it told her was `replaced` since. Beside the day, not in
/// their words: what they said stays as said, and is matched, merged and
/// cut as any other line.
pub(crate) fn found_on(day: String, replaced: bool) -> String {
    if replaced {
        format!("{day}, {REPLACED_SINCE}")
    } else {
        day
    }
}

/// The day a line was said, on her clock, as every other line she reads
/// is dated: the database gives the moment at no offset, and its day there
/// is the day before for what was said in her small hours.
pub(crate) fn day(at: &DateTime<FixedOffset>) -> String {
    super::clock::date_of(at).to_string()
}

/// The day alone of a line as found: what lines are put in order by, so
/// one whose day says more stays where it was said among that day's.
pub(crate) fn day_of(found_on: &str) -> &str {
    found_on
        .strip_suffix(REPLACED_SINCE)
        .and_then(|day| day.strip_suffix(", "))
        .unwrap_or(found_on)
}

/// The newest `latest` lines of `user_id`'s private chat with her that a
/// chat turn may look through, oldest first: their own talk only (no
/// venue: never a group's; a session with no mode written is chat), not an
/// archived conversation, the two of them only (no piece of work coming
/// back), nothing empty, and nothing they took back ([`withdrawn`]); each
/// without the directives riding on it.
/// Nothing when it cannot be read.
pub(crate) async fn private_lines(db: &DatabaseConnection, user_id: i32, latest: i64) -> Vec<Line> {
    let Ok(rows) = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id, m.role, m.content, m.created_at FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND (s.context->>'venue') IS NULL \
               AND COALESCE(s.context->>'mode', 'chat') = 'chat' AND NOT COALESCE(s.archived, false) \
               AND m.role IN ('user', 'assistant') AND (m.metadata->>'piece') IS NULL \
             ORDER BY m.created_at DESC, m.id DESC LIMIT $2",
            [user_id.into(), latest.into()],
        ))
        .await
    else {
        return Vec::new();
    };
    let mut lines: Vec<Line> = rows
        .iter()
        .filter_map(|row| {
            // Without the directives and work riding on a stored line, as
            // the chat in front of her has it.
            let content = crate::services::agent::chat_prompt::chat_safe_content(
                &row.try_get::<String>("", "content").ok()?,
            );
            if content.trim().is_empty() {
                return None;
            }
            Some(Line {
                id: row.try_get("", "id").ok()?,
                at: row.try_get("", "created_at").ok()?,
                hers: row.try_get::<String>("", "role").ok()? == "assistant",
                content,
                replaced: false,
            })
        })
        .collect();
    lines.reverse();
    // What they took back cannot be told apart from the rest unread: no
    // line is read then, rather than every one of them.
    let Some((gone, replaced)) = withdrawn(db, user_id, &lines).await else {
        return Vec::new();
    };
    lines.retain(|line| !gone.contains(&line.id));
    for line in &mut lines {
        line.replaced = replaced.contains(&line.id);
    }
    lines
}

/// Of `lines` (oldest first), those they took back, and those of theirs
/// whose fact was replaced since.
///
/// Taken back: what a memory they took back quotes (one retired as let go,
/// with nothing in its place, or deleted), said from a day before it was
/// first noted until it was written, and her answer right after each line
/// of theirs. A let-go memory
/// keeps its row and its words stay in the chat (「当作没说过」 is not
/// deleting): only she does not bring them up again. Read when looked for,
/// so a taking back that came from another end counts too.
///
/// Either way, what the short notes it was gone over from quote went with
/// it: a note's own words are not the fact's, and are theirs all the same.
///
/// None when it cannot be read: the caller then reads no line at all, for
/// which of them were taken back is not known.
///
/// Replaced since: what a memory quotes that a later fact took the place of
/// (retired as superseded, pointing at one that was not itself taken back),
/// their line only. A fact gone over from short notes was written when she
/// went over them, which may be days after the words: its line is looked
/// for from a day before the first of those notes was taken.
async fn withdrawn(
    db: &DatabaseConnection,
    user_id: i32,
    lines: &[Line],
) -> Option<(HashSet<i32>, HashSet<i32>)> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            // The notes each was gone over from are joined once, among this
            // person's rows, not looked up row by row; and each of those
            // notes quotes words of its own, which went the way of the fact
            // it was gone over into.
            "WITH retired AS ( \
               SELECT m.id, m.user_id, m.evidence, m.created_at, \
                 LEAST(m.created_at, min(s.created_at)) AS noted, \
                 (m.invalid_reason = 'deleted' OR m.superseded_by IS NULL) AS taken_back, \
                 EXISTS (SELECT 1 FROM agent_memories n \
                         WHERE n.id = m.superseded_by AND n.user_id = m.user_id \
                           AND n.invalid_reason IS DISTINCT FROM 'deleted' \
                           AND NOT (n.invalid_reason IS NOT DISTINCT FROM 'superseded' \
                                    AND n.superseded_by IS NULL)) AS replaced \
               FROM agent_memories m \
               LEFT JOIN agent_memories s ON s.user_id = m.user_id \
                 AND s.superseded_by = m.id AND s.invalid_reason = 'settled' \
               WHERE m.user_id = $1 AND m.venue = 'private' AND m.source = ANY($2) \
                 AND m.invalid_reason IN ('deleted', 'superseded') \
               GROUP BY m.id) \
             SELECT evidence, created_at, noted, taken_back, replaced FROM retired \
             UNION ALL \
             SELECT s.evidence, r.created_at, s.created_at, r.taken_back, r.replaced \
             FROM retired r JOIN agent_memories s ON s.user_id = r.user_id \
               AND s.superseded_by = r.id AND s.invalid_reason = 'settled'",
            [
                user_id.into(),
                vec![
                    crate::services::agent::memory::unified::SHORT_NOTE_SOURCE.to_string(),
                    "chat".to_string(),
                    super::store::SAID_SOURCE.to_string(),
                ]
                .into(),
            ],
        ))
        .await
        .inspect_err(|error| {
            let failed = UNREAD.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            tracing::warn!(
                failed,
                "[chat-lines] what they took back could not be read, so no line is: {error}"
            );
        })
        .ok()?;
    let quoted: Vec<Quoted> = rows
        .iter()
        .filter_map(|row| {
            let evidence: Option<String> = row.try_get("", "evidence").ok()?;
            Some(Quoted {
                quote: origins::quote(evidence.as_deref())?,
                written: row.try_get("", "created_at").ok()?,
                noted: row.try_get("", "noted").ok()?,
                taken_back: row.try_get("", "taken_back").ok()?,
                replaced: row.try_get("", "replaced").ok()?,
            })
        })
        .collect();
    Some(became(lines, quoted))
}

/// How many times what they took back could not be read since she
/// started: each time, a look back that read nothing.
static UNREAD: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// What one retired memory quotes, when it was written and when the first
/// note it was gone over from was taken (`noted`; when it was written, for
/// one gone over from none), and what became of it.
struct Quoted {
    quote: String,
    written: DateTime<FixedOffset>,
    noted: DateTime<FixedOffset>,
    taken_back: bool,
    replaced: bool,
}

/// [`withdrawn`], once read: each line against the memories written after
/// it only, found by when they were written.
fn became(lines: &[Line], mut quoted: Vec<Quoted>) -> (HashSet<i32>, HashSet<i32>) {
    quoted.sort_by_key(|quoted| quoted.written);
    // No memory written later than this after a line can quote it.
    let longest = quoted
        .iter()
        .map(|quoted| quoted.written - quoted.noted)
        .max()
        .unwrap_or_default()
        + origins::QUOTED_WITHIN;
    let (mut gone, mut replaced) = (HashSet::new(), HashSet::new());
    for (index, line) in lines.iter().enumerate() {
        let after = quoted.partition_point(|quoted| quoted.written < line.at);
        let mut said = None;
        let (mut taken_back, mut since) = (false, false);
        for quoted in quoted[after..]
            .iter()
            .take_while(|quoted| quoted.written <= line.at + longest)
        {
            // Either way from a day before its first note was taken: a
            // fact gone over days after the words still reaches them.
            if line.at < quoted.noted - origins::QUOTED_WITHIN
                || !(quoted.taken_back || (quoted.replaced && !line.hers))
            {
                continue;
            }
            let said = said.get_or_insert_with(|| {
                line.content
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            });
            if said.contains(quoted.quote.as_str()) {
                taken_back |= quoted.taken_back;
                since |= !quoted.taken_back;
            }
        }
        if taken_back {
            gone.insert(line.id);
            if !line.hers
                && let Some(answer) = lines.get(index + 1).filter(|next| next.hers)
            {
                gone.insert(answer.id);
            }
        } else if since {
            replaced.insert(line.id);
        }
    }
    (gone, replaced)
}

#[cfg(test)]
#[path = "chat_lines_tests.rs"]
pub(in crate::services::agent::merope) mod tests;
