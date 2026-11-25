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
//! - **Asked not to keep.** A line of something they asked her not to keep
//!   from then on ([`super::asked::lines`]) is left out the same way, her
//!   answer to it too: its words stay in the chat as they were. A stretch
//!   not judged yet ([`super::asked::lines::waiting`]) is left out whole.
//! - **Replaced since.** What a memory quotes that a later fact took the
//!   place of (the old fact is no longer shown) is still what they said
//!   that day, and is found; but its day says so where she reads it
//!   ([`found_on`]), so their old words do not read as how things are
//!   beside the fact that took their place. The words themselves stay as
//!   said: what is matched, given a vector or written into a day is them.

use std::collections::{BTreeMap, HashSet};

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
    /// The conversation it is in.
    pub(crate) session: String,
    /// The memories known to come from it (`memory::origins::tell`).
    pub(crate) told: Vec<String>,
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
/// back), nothing said while she was off, nothing empty, and nothing they took back ([`withdrawn`]); each
/// without the directives riding on it.
/// Nothing when it cannot be read.
pub(crate) async fn private_lines(db: &DatabaseConnection, user_id: i32, latest: i64) -> Vec<Line> {
    let Ok(rows) = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id, m.role, m.content, m.created_at, m.session_id, \
               m.metadata->>'told' AS told, \
               (COALESCE(m.metadata->>'notKept', '') = 'true' \
                 OR COALESCE(m.metadata->>'notTold', '') = 'true') AS not_kept \
             FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE (s.context->>'past') IS NULL AND s.user_id = $1 AND (s.context->>'venue') IS NULL \
               AND COALESCE(s.context->>'mode', 'chat') = 'chat' AND NOT COALESCE(s.archived, false) \
               AND m.role IN ('user', 'assistant') AND (m.metadata->>'piece') IS NULL \
               AND COALESCE(m.metadata->>'withoutHer', '') <> 'true' \
             ORDER BY m.created_at DESC, m.id DESC LIMIT $2",
            [user_id.into(), latest.into()],
        ))
        .await
    else {
        return Vec::new();
    };
    // What of it is not judged yet, unread, could be any of it.
    let Ok(waiting) = super::asked::lines::waiting(db, Some(user_id), None).await else {
        return Vec::new();
    };
    let mut not_kept = HashSet::new();
    let mut lines: Vec<Line> = rows
        .iter()
        .filter_map(|row| {
            let line = line_of(row)?;
            if row.try_get("", "not_kept").ok()?
                || waiting
                    .iter()
                    .any(|stretch| stretch.holds(user_id, line.at))
            {
                not_kept.insert(line.id);
            }
            Some(line)
        })
        .collect();
    lines.reverse();
    // What they took back cannot be told apart from the rest unread: no
    // line is read then, rather than every one of them.
    let Some((mut gone, replaced)) = withdrawn(db, user_id, &lines).await else {
        return Vec::new();
    };
    for (index, line) in lines.iter().enumerate() {
        if not_kept.contains(&line.id) {
            gone.insert(line.id);
            gone.extend(answer_to(&lines, index));
        }
    }
    lines.retain(|line| !gone.contains(&line.id));
    for line in &mut lines {
        line.replaced = replaced.contains(&line.id);
    }
    lines
}

/// Of `lines` (oldest first), those they took back, and those of theirs
/// whose fact was replaced since.
///
/// Taken back: the lines a memory they took back (one retired as let go,
/// with nothing in its place, or deleted) is known to come from, or, when
/// none is known, those it quotes, said from a day before it was first noted
/// until it was written; and her answer right after each line of theirs. A
/// let-go memory keeps its row and its words stay in the chat (「当作没说过」 is not
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
    let quoted = quoted(db, Some(user_id), "private")
        .await
        .inspect_err(|error| {
            let failed = UNREAD.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            tracing::warn!(
                failed,
                "[chat-lines] what they took back could not be read, so no line is: {error}"
            );
        })
        .ok()?;
    Some(became(lines, quoted.iter().collect()))
}

/// What was said in the group `venue` (as sessions keep it) that whoever
/// said it took back ([`withdrawn`]), among `lines`: each with whose, oldest
/// first.
pub(crate) async fn withdrawn_in_group(
    db: &DatabaseConnection,
    venue: &str,
    lines: &BTreeMap<i32, Vec<Line>>,
) -> Result<HashSet<i32>, sea_orm::DbErr> {
    let quoted = quoted(db, None, &format!("group:{venue}")).await?;
    let mut gone = HashSet::new();
    for (user_id, lines) in lines {
        let theirs = quoted.iter().filter(|one| one.user_id == *user_id);
        gone.extend(became(lines, theirs.collect()).0);
    }
    Ok(gone)
}

/// Their private lines ([`withdrawn`]) that each of `user_id`'s memories
/// they took back comes from, in any conversation of theirs with her: those
/// of the memories `which`. Where the lines are not known, the one line each
/// of its words is most surely of: theirs (hers, for what she told them),
/// nearest to when it was first noted, and for few words only a line of
/// nothing else, or the one line they are in. With them those they took back in the stretch they said
/// them in, kept as no memory (`which("")`), those of what they asked her
/// not to keep, and her answer to each.
pub(crate) async fn taken_back_with<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    which: impl Fn(&str) -> bool,
) -> Result<HashSet<i32>, sea_orm::DbErr> {
    let mut gone = if which("") {
        let_go_at_once(db, user_id).await?
    } else {
        HashSet::new()
    };
    if !any_taken_back(db, user_id).await? {
        return Ok(gone);
    }
    let quoted: Vec<Quoted> = quoted(db, Some(user_id), "private")
        .await?
        .into_iter()
        .filter(|one| one.taken_back && which(&one.memory))
        .collect();
    if quoted.is_empty() {
        return Ok(gone);
    }
    let from: Vec<DateTime<FixedOffset>> = quoted
        .iter()
        .map(|one| one.noted - origins::QUOTED_WITHIN)
        .collect();
    let to: Vec<DateTime<FixedOffset>> = quoted.iter().map(|one| one.written).collect();
    let memories: Vec<String> = quoted.iter().map(|one| one.memory.clone()).collect();
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id, m.role, m.content, m.created_at, m.session_id, \
               m.metadata->>'told' AS told FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND (s.context->>'venue') IS NULL \
               AND COALESCE(s.context->>'mode', 'chat') = 'chat' \
               AND m.role IN ('user', 'assistant') AND (m.metadata->>'piece') IS NULL \
               AND ((m.metadata->'told')::jsonb ?| $4 \
                 OR EXISTS (SELECT 1 FROM unnest($2::timestamptz[], $3::timestamptz[]) \
                            AS around(since, until) \
                            WHERE m.created_at BETWEEN around.since AND around.until)) \
             ORDER BY m.created_at, m.id",
            [user_id.into(), from.into(), to.into(), memories.into()],
        ))
        .await?;
    let lines: Vec<Line> = rows.iter().filter_map(line_of).collect();
    let known = known_lines(&lines, quoted.iter());
    let mut theirs: HashSet<i32> = HashSet::new();
    for one in &quoted {
        if let Some(known) = known.get(one.memory.as_str()) {
            theirs.extend(known);
            if one.own {
                continue;
            }
        }
        let Some(quote) = one.quote.as_ref() else {
            continue;
        };
        let within: Vec<&Line> = lines.iter().filter(|line| one.reaches(line)).collect();
        let said: Vec<String> = (within.iter())
            .map(|line| origins::folded(&line.content))
            .collect();
        theirs.extend(
            (quote.among(&said).into_iter())
                .map(|at| within[at])
                .min_by_key(|line| (line.at - one.noted).abs())
                .map(|line| line.id),
        );
    }
    gone.extend(answers_to(db, &theirs).await?);
    gone.extend(theirs);
    Ok(gone)
}

/// Whether any memory of `user_id` was taken back or deleted at all
/// (`known::taken_back_sql`): what it replaced goes with it, and cannot
/// without it. One look, where that is followed for each.
async fn any_taken_back<C: ConnectionTrait>(db: &C, user_id: i32) -> Result<bool, sea_orm::DbErr> {
    Ok(db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 AS one FROM agent_memories m WHERE m.user_id = $1 \
               AND (m.invalid_reason = 'deleted' \
                 OR (m.invalid_reason = 'superseded' AND m.superseded_by IS NULL) \
                 OR (m.superseded_by IS NOT NULL \
                   AND NOT EXISTS (SELECT 1 FROM agent_memories n \
                                   WHERE n.id = m.superseded_by AND n.user_id = m.user_id))) \
             LIMIT 1",
            [user_id.into()],
        ))
        .await?
        .is_some())
}

/// Her answer to each of their lines `ids`: what she said right after it, in
/// the same conversation.
async fn answers_to<C: ConnectionTrait>(
    db: &C,
    ids: &HashSet<i32>,
) -> Result<Vec<i32>, sea_orm::DbErr> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        format!(
            "SELECT answer.id AS answer FROM agent_messages m {ANSWER} \
             WHERE m.id = ANY($1) AND m.role = 'user' AND answer.id IS NOT NULL"
        ),
        [ids.iter().copied().collect::<Vec<i32>>().into()],
    ))
    .await?
    .iter()
    .map(|row| row.try_get("", "answer"))
    .collect()
}

/// Joined to a line `m`: what was said right after it in its conversation,
/// when that is hers.
const ANSWER: &str = "LEFT JOIN LATERAL ( \
       SELECT a.id, a.role FROM agent_messages a \
       WHERE a.session_id = m.session_id AND a.role IN ('user', 'assistant') \
         AND (a.metadata->>'piece') IS NULL \
         AND (a.created_at, a.id) > (m.created_at, m.id) \
       ORDER BY a.created_at, a.id LIMIT 1) answer ON answer.role = 'assistant'";

/// Their private lines they took back in the stretch they said them in
/// (`takenBack`), or that are what they asked her not to keep (`notKept`,
/// `asked::lines`), and her answer to each.
async fn let_go_at_once<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
) -> Result<HashSet<i32>, sea_orm::DbErr> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "SELECT m.id, CASE WHEN m.role = 'user' THEN answer.id END AS answer \
                 FROM agent_messages m \
                 JOIN agent_sessions s ON s.id = m.session_id {ANSWER} \
                 WHERE s.user_id = $1 AND (s.context->>'venue') IS NULL \
                   AND COALESCE(s.context->>'mode', 'chat') = 'chat' \
                   AND ((m.metadata->>'takenBack') IS NOT NULL \
                     OR m.metadata->>'notKept' = 'true' \
                     OR m.metadata->>'notTold' = 'true')"
            ),
            [user_id.into()],
        ))
        .await?;
    let mut lines = HashSet::new();
    for row in &rows {
        lines.insert(row.try_get("", "id")?);
        lines.extend(row.try_get::<Option<i32>>("", "answer")?);
    }
    Ok(lines)
}

/// A line as it is kept, without the directives and work riding on it, as
/// the chat in front of her has it.
pub(super) fn line_of(row: &sea_orm::QueryResult) -> Option<Line> {
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
        session: row.try_get("", "session_id").ok()?,
        told: row
            .try_get::<Option<String>>("", "told")
            .ok()?
            .and_then(|told| serde_json::from_str(&told).ok())
            .unwrap_or_default(),
        replaced: false,
    })
}

/// What the retired memories learned at `venue` quote ([`withdrawn`]): of
/// `user_id`, or with none of everyone there.
async fn quoted<C: ConnectionTrait>(
    db: &C,
    user_id: Option<i32>,
    venue: &str,
) -> Result<Vec<Quoted>, sea_orm::DbErr> {
    let whose = |row: &str| match user_id {
        Some(_) => format!("{row}.user_id = $1"),
        None => format!("$1::int IS NULL AND {row}.user_id IS NOT NULL"),
    };
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            // The notes each was gone over from are joined once, among this
            // person's rows, not looked up row by row; and each of those
            // notes quotes words of its own, which went the way of the fact
            // it was gone over into.
            format!(
                "WITH RECURSIVE {}, retired AS ( \
               SELECT m.id, m.user_id, m.source, m.evidence, m.created_at, \
                 LEAST(m.created_at, m.valid_from, min(s.created_at)) AS noted, \
                 m.id IN (SELECT id FROM taken_back) AS taken_back, \
                 EXISTS (SELECT 1 FROM agent_memories n \
                         WHERE n.id = m.superseded_by AND n.user_id = m.user_id \
                           AND n.invalid_reason IS DISTINCT FROM 'deleted' \
                           AND NOT (n.invalid_reason IS NOT DISTINCT FROM 'superseded' \
                                    AND n.superseded_by IS NULL)) AS replaced \
               FROM agent_memories m \
               LEFT JOIN agent_memories s ON s.user_id = m.user_id AND {} \
                 AND s.superseded_by = m.id AND s.invalid_reason = 'settled' \
               WHERE {} AND m.venue = $3 AND m.source = ANY($2) \
                 AND m.invalid_reason IN ('deleted', 'superseded') \
               GROUP BY m.id) \
             SELECT id, user_id, source, evidence, created_at, noted, taken_back, replaced, \
               true AS own \
             FROM retired \
             UNION ALL \
             SELECT r.id, r.user_id, s.source, s.evidence, r.created_at, s.created_at, \
               r.taken_back, r.replaced, false \
             FROM retired r JOIN agent_memories s ON s.user_id = r.user_id \
               AND s.superseded_by = r.id AND s.invalid_reason = 'settled'",
                super::memory_nodes::known::taken_back_sql(&format!(
                    "{} AND s.venue = $3 AND s.source = ANY($2) \
                     AND s.invalid_reason IN ('deleted', 'superseded')",
                    whose("s")
                )),
                whose("s"),
                whose("m"),
            ),
            [
                user_id.into(),
                vec![
                    crate::services::agent::memory::unified::SHORT_NOTE_SOURCE.to_string(),
                    "chat".to_string(),
                    super::store::SAID_SOURCE.to_string(),
                ]
                .into(),
                venue.into(),
            ],
        ))
        .await?;
    let mut quoted = Vec::new();
    for row in &rows {
        let evidence: Option<String> = row.try_get("", "evidence")?;
        let mut quotes: Vec<_> = origins::quotes(evidence.as_deref())
            .into_iter()
            .map(Some)
            .collect();
        if quotes.is_empty() {
            quotes.push(None);
        }
        for quote in quotes {
            quoted.push(Quoted {
                memory: row.try_get("", "id")?,
                user_id: row.try_get("", "user_id")?,
                quote,
                written: row.try_get("", "created_at")?,
                noted: row.try_get("", "noted")?,
                taken_back: row.try_get("", "taken_back")?,
                replaced: row.try_get("", "replaced")?,
                hers: row.try_get::<String>("", "source")? == super::store::SAID_SOURCE,
                own: row.try_get("", "own")?,
            });
        }
    }
    Ok(quoted)
}

/// Her answer right after the line of theirs at `index`, in the same
/// conversation.
pub(super) fn answer_to(lines: &[Line], index: usize) -> Option<i32> {
    let line = lines.get(index).filter(|line| !line.hers)?;
    let next = lines[index + 1..]
        .iter()
        .find(|next| next.session == line.session)?;
    next.hers.then_some(next.id)
}

/// How many times what they took back could not be read since she
/// started: each time, a look back that read nothing.
static UNREAD: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// What one retired memory quotes, whose, when it was written and when the
/// first note it was gone over from was taken (`noted`; when it was written,
/// for one gone over from none), and what became of it.
struct Quoted {
    memory: String,
    user_id: i32,
    /// None when it quotes nothing a line is found by: it is known by the
    /// lines it was told in alone.
    quote: Option<origins::Quote>,
    written: DateTime<FixedOffset>,
    noted: DateTime<FixedOffset>,
    taken_back: bool,
    replaced: bool,
    /// What she told them: in lines of hers.
    hers: bool,
    /// Its own words, not those of a note it was gone over from.
    own: bool,
}

impl Quoted {
    /// Whether `line` is one it could quote: whose it is, and said from a
    /// day before its first note was taken until it was written (a fact
    /// gone over days after the words still reaches them).
    fn reaches(&self, line: &Line) -> bool {
        line.hers == self.hers
            && line.at >= self.noted - origins::QUOTED_WITHIN
            && line.at <= self.written
    }
}

/// Of `lines`, those each of `quoted`'s memories is known to come from, for
/// the memories whose lines are known: told before it was written (a line
/// that only said again what she already had is not where it came from).
fn known_lines<'a>(
    lines: &[Line],
    quoted: impl Iterator<Item = &'a Quoted>,
) -> BTreeMap<&'a str, Vec<i32>> {
    let mut known: BTreeMap<&str, (Vec<i32>, bool)> = BTreeMap::new();
    let mut told: BTreeMap<&str, Vec<&Line>> = BTreeMap::new();
    for line in lines {
        for memory in &line.told {
            told.entry(memory.as_str()).or_default().push(line);
        }
    }
    for one in quoted {
        let Some(lines) = told.get(one.memory.as_str()) else {
            continue;
        };
        known.entry(one.memory.as_str()).or_insert_with(|| {
            (
                lines.iter().map(|line| line.id).collect(),
                lines.iter().any(|line| line.at <= one.written),
            )
        });
    }
    known
        .into_iter()
        .filter(|(_, (_, known))| *known)
        .map(|(memory, (lines, _))| (memory, lines))
        .collect()
}

/// [`withdrawn`], once read: each line against the memories known to come
/// from it, and, for those whose lines are not known, the ones written after
/// it only, found by when they were written.
fn became(lines: &[Line], mut quoted: Vec<&Quoted>) -> (HashSet<i32>, HashSet<i32>) {
    let known = known_lines(lines, quoted.iter().copied());
    let (mut told_gone, mut told_replaced) = (HashSet::<i32>::new(), HashSet::<i32>::new());
    for one in &quoted {
        if let Some(lines) = known.get(one.memory.as_str()) {
            if one.taken_back {
                told_gone.extend(lines);
            } else if one.replaced {
                told_replaced.extend(lines);
            }
        }
    }
    quoted.retain(|one| !(one.own && known.contains_key(one.memory.as_str())));
    quoted.sort_by_key(|quoted| quoted.written);
    // The lines a few words alone, taken back, are of (`Quote::among`).
    let all_said = std::cell::LazyCell::new(|| -> Vec<String> {
        (lines.iter().map(|line| origins::folded(&line.content))).collect()
    });
    let of_few: Vec<Option<HashSet<i32>>> = quoted
        .iter()
        .map(|one| {
            let quote = (one.quote.as_ref()).filter(|quote| one.taken_back && quote.few())?;
            let within: Vec<usize> = (0..lines.len())
                .filter(|index| one.reaches(&lines[*index]))
                .collect();
            let said: Vec<String> = within
                .iter()
                .map(|index| all_said[*index].clone())
                .collect();
            let of = quote.among(&said);
            Some(of.into_iter().map(|at| lines[within[at]].id).collect())
        })
        .collect();
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
        let mut taken_back = told_gone.contains(&line.id);
        let mut since = !line.hers && told_replaced.contains(&line.id);
        for (quoted, of_few) in quoted[after..]
            .iter()
            .zip(&of_few[after..])
            .take_while(|(quoted, _)| quoted.written <= line.at + longest)
        {
            if !quoted.reaches(line) || !(quoted.taken_back || (quoted.replaced && !line.hers)) {
                continue;
            }
            let said = said.get_or_insert_with(|| origins::folded(&line.content));
            let quotes = match of_few {
                Some(of) => of.contains(&line.id),
                None => (quoted.quote.as_ref()).is_some_and(|quote| quote.is_in(said)),
            };
            if quotes {
                taken_back |= quoted.taken_back;
                since |= !quoted.taken_back;
            }
        }
        if taken_back {
            gone.insert(line.id);
            gone.extend(answer_to(lines, index));
        } else if since {
            replaced.insert(line.id);
        }
    }
    (gone, replaced)
}

#[cfg(test)]
#[path = "chat_lines_tests.rs"]
pub(in crate::services::agent) mod tests;
