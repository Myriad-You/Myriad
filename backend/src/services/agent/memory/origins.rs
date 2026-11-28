//! What a memory came from, forgotten with it: the words that would teach
//! her the same thing again. No trace of what was forgotten is kept to
//! guard against relearning it; what could teach it again goes instead.
//!
//! Only a person's memories have words of theirs behind them, and only
//! where she keeps those words, in that person's own place (private, or the
//! one group it was learned in); nothing crosses into another person's or
//! place's. By where a memory came from:
//!
//! - what they said, noted line by line or kept once a stretch was gone
//!   over (`chat_note`, `chat`), and what she told them (`said`): the
//!   evidence is their words (or hers) as said. The lines that say it in
//!   that place, within [`QUOTED_WITHIN`] before the memory was written, go
//!   with her answer to each, as do the same words in her chat diary and any
//!   job still queued to go over them. The short notes a stretch was kept
//!   from are earlier versions of what it kept, so they go too. What a line
//!   of hers taken along taught her (what she said in her answer, kept as
//!   `said`) is looked for before it goes ([`Gone::taught`]) and forgotten
//!   after, as any memory is.
//! - a day with them (`day_with`): written from that whole day of private
//!   talk, and written again for any day without one, so the day's lines and
//!   chat diary go.
//!
//! What cannot go, and why:
//!
//! - an event (`event`): the memory keeps only its summary; the notice it
//!   came from is gone within a day or is someone else's (another server's,
//!   a room's). A like event later is something new that happened;
//! - what she saw them play (`presence`) is the platform's; a game she
//!   played with them (`game`) was gone when the game was put away; what she
//!   looked up (`lookup`) was on the web;
//! - what she went over at night (`bit`, `us`, `lands`, `thread`, `sore`,
//!   `about_me`) and what she took from her work with them (`work`) name
//!   no line, and a night only goes over the day before;
//! - a group's (`stranger`, `jargon`, `group_day`, and what she heard
//!   there): the group's messages are the platform's, and she keeps them
//!   for hours only;
//! - her own (her days, views, what she did) are not anyone's words.
//!
//! Copies on the owner's devices and in her backups (persona sync) are not
//! reached from here: a device can bring a forgotten memory or line back
//! with its next merge until her log (sync S6) carries the forget there.

use chrono::{DateTime, Duration, FixedOffset, NaiveDate};
use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, Statement};

use crate::models::entities::agent_memories;
use crate::services::agent::merope::{
    chat_days,
    store::{DIARY_SOURCE_CHAT, SAID_SOURCE, memory_jobs},
};

use super::unified::{ASKED_SOURCE, SHORT_NOTE_SOURCE};

mod quoted;
pub(crate) use quoted::{Quote, folded, quote, quotes};
use quoted::{SHORTEST_QUOTE, few};

/// How long before a memory was written the words it quotes were said:
/// a line is gone over within a day (queued work is kept that long).
pub const QUOTED_WITHIN: Duration = Duration::days(1);

/// A line deleted with what it taught her: for what else cites it to go
/// too, here (her nodes) and on the owner's other ends (her log).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gone {
    /// Its `agent_messages.id`.
    pub id: i32,
    pub session_id: String,
    pub role: String,
    /// The row of her log it is, when it is one (`agent_messages.log_row`).
    pub log_row: Option<String>,
    /// A line of hers taken along (her answer to a line that went, a line a
    /// memory of what she said came from, a day's): the memories it taught
    /// her ([`quoting`]), looked for before it went, for them to go after
    /// it as any memory forgotten goes (`manage::forget_taught`). One known
    /// to come from several lines goes when any of them does; one that only
    /// says the same as a line still here does not.
    pub taught: Vec<String>,
}

impl Gone {
    pub fn of(line: &crate::models::entities::agent_messages::Model) -> Self {
        Self {
            id: line.id,
            session_id: line.session_id.clone(),
            role: line.role.clone(),
            log_row: line.log_row.clone(),
            taught: Vec::new(),
        }
    }

    /// What the lines `gone` taught her, each once.
    pub fn taught_by(gone: &[Self]) -> Vec<String> {
        let mut taught: Vec<String> = gone.iter().flat_map(|line| line.taught.clone()).collect();
        taught.sort();
        taught.dedup();
        taught
    }
}

/// Deletes what `rows` (a memory and its earlier versions, about to be
/// deleted on the same transaction) came from; the lines deleted, for what
/// else cites them to go too. The caller holds the memory-work lock
/// ([`memory_jobs::lock`]) from before it read them, so no queued job writes
/// in between.
pub async fn forget<C: ConnectionTrait>(
    db: &C,
    rows: &[agent_memories::Model],
) -> Result<Vec<Gone>, DbErr> {
    // Found for all of them first, then deleted: a line gone for one is
    // still there to tell which line another quotes.
    let mut came_from = Vec::new();
    for row in rows {
        let Some(user_id) = row.user_id.filter(|id| *id > 0) else {
            continue;
        };
        let Some(place) = place(&row.venue) else {
            continue;
        };
        match row.source.as_str() {
            source
                if source == SHORT_NOTE_SOURCE
                    || source == "chat"
                    || source == ASKED_SOURCE
                    || source == SAID_SOURCE =>
            {
                came_from.push((user_id, came_from_lines(db, user_id, place, row).await?));
            }
            source if source == chat_days::SOURCE && place.is_none() => {
                if let Some((start, end)) = day(row.evidence.as_deref()) {
                    came_from.push((user_id, CameFrom::Day(start, end)));
                }
            }
            _ => {}
        }
    }
    let mut lines = Vec::new();
    for (user_id, from) in came_from {
        match from {
            CameFrom::Lines { ids, whole, quotes } => {
                lines.extend(delete_lines(db, user_id, &ids).await?);
                for (said, at) in &whole {
                    forget_line_traces(db, user_id, said, *at).await?;
                }
                for (quote, from, to) in &quotes {
                    delete_diary(db, user_id, Some(quote), *from, *to).await?;
                    memory_jobs::forget_holding(db, user_id, quote).await?;
                }
            }
            CameFrom::Day(start, end) => {
                lines.extend(delete_day(db, user_id, start, end).await?);
                delete_diary(db, user_id, None, start, end).await?;
            }
        }
    }
    Ok(lines)
}

/// What a memory came from, to go with it.
enum CameFrom {
    Lines {
        ids: Vec<i32>,
        /// What is said in some of them and when: her diary and what waits
        /// hold each of those too.
        whole: Vec<(String, DateTime<FixedOffset>)>,
        /// What it quotes, in her diary from when until when and in what
        /// waits.
        quotes: Vec<(Quote, DateTime<FixedOffset>, DateTime<FixedOffset>)>,
    },
    /// A day's talk, from its first midnight to the next.
    Day(DateTime<FixedOffset>, DateTime<FixedOffset>),
}

/// [`CameFrom`], of a memory that quotes lines.
async fn came_from_lines<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    place: Option<&str>,
    row: &agent_memories::Model,
) -> Result<CameFrom, DbErr> {
    let from = row.valid_from.min(row.created_at) - QUOTED_WITHIN;
    let mut told = told_by(db, user_id, place, &row.id).await?;
    if row.source == ASKED_SOURCE && told.ids.is_empty() {
        // Kept as of no one place, wherever they asked it.
        told = told_anywhere(db, user_id, &row.id).await?;
    }
    let known = told.known_before(row.created_at);
    let (mut ids, mut whole) = (told.ids, told.said);
    let mut quoted = Vec::new();
    for quote in quotes(row.evidence.as_deref()) {
        let few = quote.strictly().whole;
        // The lines it is known to come from are the ones it quotes: none is
        // looked for by its words, nor anything else by a few of them.
        if !known {
            let hers = row.source == SAID_SOURCE;
            let of = lines_holding(db, user_id, place, &quote, hers, from, row.created_at).await?;
            let said: Vec<String> = of.iter().map(|line| folded(&line.1)).collect();
            for at in quote.among(&said) {
                let (id, said, at) = &of[at];
                ids.push(*id);
                if few {
                    whole.push((said.clone(), *at));
                }
            }
        } else if few {
            continue;
        }
        quoted.push((quote.strictly(), from, row.created_at));
    }
    Ok(CameFrom::Lines {
        ids,
        whole,
        quotes: quoted,
    })
}

/// [`forget`] for one memory of `user_id`'s (or her own, kept under
/// `own_venue` with no person) that another end forgot, taken in here:
/// the memory-work lock first, then the row, then the words it came from,
/// on the caller's transaction. The lines deleted; none when there is no
/// such row here or it is a group's.
pub async fn forget_taken_in<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    own_venue: &str,
    id: &str,
) -> Result<Vec<Gone>, DbErr> {
    use sea_orm::EntityTrait;
    memory_jobs::lock(db)
        .await
        .map_err(|error| DbErr::Custom(error.to_string()))?;
    let row = agent_memories::Entity::find_by_id(id.to_string())
        .one(db)
        .await?
        .filter(|row| {
            !row.venue.starts_with("group:")
                && (row.user_id == Some(user_id)
                    || (row.user_id.is_none() && row.venue == own_venue))
        });
    forget(db, row.as_slice()).await
}

/// The way back from one line: the person's (`user_id`'s) memories in
/// `place` (`None` private, a group's id) that `line` taught, from what they
/// said or what she told them (the sources [`forget`] follows to lines): the
/// ones known to come from it ([`tell`]), and those not known to come from
/// any line that quote it and were written within [`QUOTED_WITHIN`] after it
/// (after the first line it quotes, for a talk gone over later than that).
/// For forgetting the line with what it taught her: each goes by
/// [`super::manage::forget_any`].
pub async fn quoting<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    place: Option<&str>,
    line: &Quotable<'_>,
) -> Result<Vec<String>, DbErr> {
    let said = folded(line.content);
    // Too short to be found by: only what is known to come from it.
    let short = said.chars().count() < SHORTEST_QUOTE;
    let hers = line.role == "assistant";
    let sources = if hers {
        vec![SAID_SOURCE.to_string()]
    } else {
        vec![SHORT_NOTE_SOURCE.to_string(), "chat".to_string()]
    };
    let venue = place.map_or_else(|| "private".to_string(), |group| format!("group:{group}"));
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, evidence, created_at, valid_from FROM agent_memories \
             WHERE user_id = $1 AND venue = $2 AND source = ANY($3) AND invalid_at IS NULL \
               AND ((created_at >= $4 AND LEAST(valid_from, created_at) <= $5) \
                 OR id IN (SELECT jsonb_array_elements_text((m.metadata->'told')::jsonb) \
                           FROM agent_messages m WHERE m.id = $6)) \
             ORDER BY created_at, id",
            vec![
                user_id.into(),
                venue.into(),
                sources.into(),
                line.at.into(),
                (line.at + QUOTED_WITHIN).into(),
                line.id.into(),
            ],
        ))
        .await?;
    let mut out = Vec::new();
    for row in rows {
        let id: String = row.try_get("", "id")?;
        let told = told_by(db, user_id, place, &id).await?;
        let written: DateTime<FixedOffset> = row.try_get("", "created_at")?;
        let mut taught = told.ids.contains(&line.id);
        if !taught && !short && !told.known_before(written) {
            let evidence: Option<String> = row.try_get("", "evidence")?;
            let from = written.min(row.try_get("", "valid_from")?) - QUOTED_WITHIN;
            for quote in quotes(evidence.as_deref()) {
                if !quote.is_in(&said) {
                    continue;
                }
                if !quote.strictly().is_in(&said) {
                    // A few words, not all it says: only with no other line
                    // they could be of.
                    let mut of =
                        lines_holding(db, user_id, place, &quote, hers, from, written).await?;
                    of.retain(|(id, ..)| *id != line.id);
                    let mut said: Vec<String> = of.iter().map(|line| folded(&line.1)).collect();
                    said.push(folded(line.content));
                    if !quote.among(&said).contains(&of.len()) {
                        continue;
                    }
                }
                taught = true;
                break;
            }
        }
        if taught {
            out.push(id);
        }
    }
    Ok(out)
}

/// One line of a talk, for what it taught ([`quoting`]).
pub struct Quotable<'a> {
    /// Its `agent_messages.id`.
    pub id: i32,
    pub role: &'a str,
    pub content: &'a str,
    pub at: DateTime<FixedOffset>,
}

/// The lines a memory is known to come from ([`tell`]).
#[derive(Debug, Default)]
struct Told {
    ids: Vec<i32>,
    /// What each says, and when it was said.
    said: Vec<(String, DateTime<FixedOffset>)>,
    first: Option<DateTime<FixedOffset>>,
}

impl Told {
    /// Whether the lines a memory written at `written` was made from are
    /// among them: lines that only said again what she already had, later,
    /// are not those.
    fn known_before(&self, written: DateTime<FixedOffset>) -> bool {
        self.first.is_some_and(|first| first <= written)
    }
}

/// The lines in `place` the memory `id` is known to come from.
async fn told_by<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    place: Option<&str>,
    id: &str,
) -> Result<Told, DbErr> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id, m.content, m.created_at FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND (s.context->>'venue') IS NOT DISTINCT FROM $2 \
               AND (m.metadata->'told')::jsonb ? $3 \
             ORDER BY m.created_at, m.id",
            [user_id.into(), place.map(str::to_string).into(), id.into()],
        ))
        .await?;
    told_in(&rows)
}

/// The lines, wherever `user_id` said them, the memory `id` is known to come
/// from.
async fn told_anywhere<C: ConnectionTrait>(db: &C, user_id: i32, id: &str) -> Result<Told, DbErr> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id, m.content, m.created_at FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND (m.metadata->'told')::jsonb ? $2 \
             ORDER BY m.created_at, m.id",
            [user_id.into(), id.into()],
        ))
        .await?;
    told_in(&rows)
}

fn told_in(rows: &[sea_orm::QueryResult]) -> Result<Told, DbErr> {
    let mut told = Told::default();
    for row in rows {
        let at = row.try_get("", "created_at")?;
        told.ids.push(row.try_get("", "id")?);
        told.said.push((row.try_get("", "content")?, at));
        told.first.get_or_insert(at);
    }
    Ok(told)
}

/// Notes on the lines `lines` (`agent_messages.id`) that the memory `id`
/// comes from them (`metadata.told`): what she kept of a line is then found
/// by it, and the line by what she kept, whatever words either has.
pub(crate) async fn tell<C: ConnectionTrait>(db: &C, lines: &[i32], id: &str) -> Result<(), DbErr> {
    if lines.is_empty() {
        return Ok(());
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        // On the row itself, so what is noted on it meanwhile stays.
        "UPDATE agent_messages m SET metadata = ( \
           CASE WHEN jsonb_typeof(m.metadata::jsonb) = 'object' \
             THEN m.metadata::jsonb ELSE '{}'::jsonb END \
           || jsonb_build_object('told', \
             COALESCE(m.metadata::jsonb->'told', '[]'::jsonb) || to_jsonb($2::text)))::json \
         WHERE m.id = ANY($1) AND NOT COALESCE(m.metadata::jsonb->'told' ? $2, false)",
        [lines.to_vec().into(), id.into()],
    ))
    .await?;
    Ok(())
}

/// Deletes the rest of what one line, `content` said at `at` and now gone,
/// left of itself: the same words in the person's chat diary from then until
/// [`QUOTED_WITHIN`] after, and any queued job still holding them. The caller
/// holds the memory-work lock ([`memory_jobs::lock`]).
pub async fn forget_line_traces<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    content: &str,
    at: DateTime<FixedOffset>,
) -> Result<(), DbErr> {
    let Some(said) = quote(Some(content)) else {
        return Ok(());
    };
    let whole = Quote {
        whole: few(&said),
        text: said,
    };
    delete_diary(db, user_id, Some(&whole), at, at + QUOTED_WITHIN).await?;
    // Queued a part at a time when it is long.
    for part in myriad_merope::settle_raw::line_parts(content) {
        if let Some(said) = quote(Some(part)) {
            let part = Quote {
                whole: few(&said),
                text: said,
            };
            memory_jobs::forget_holding(db, user_id, &part).await?;
        }
    }
    Ok(())
}

/// The place's id as sessions know it: `None` in private, the group's id in
/// a group. Her own (`own`) is no one's place.
fn place(venue: &str) -> Option<Option<&str>> {
    match venue {
        "private" => Some(None),
        venue => venue.strip_prefix("group:").map(Some),
    }
}

/// The day a day's line is about, from its first midnight to the next.
fn day(evidence: Option<&str>) -> Option<(DateTime<FixedOffset>, DateTime<FixedOffset>)> {
    let value: serde_json::Value = serde_json::from_str(evidence?).ok()?;
    let day: NaiveDate = value["day"].as_str()?.parse().ok()?;
    let clock = crate::services::agent::merope::clock::day_start;
    Some((clock(day)?, clock(day.succ_opt()?)?))
}

/// The person's lines in `place` with what `quote` says in them, theirs or
/// (`hers`) her own, said in `from..=to`: each with what it says, and when.
async fn lines_holding<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    place: Option<&str>,
    quote: &Quote,
    hers: bool,
    from: DateTime<FixedOffset>,
    to: DateTime<FixedOffset>,
) -> Result<Vec<(i32, String, DateTime<FixedOffset>)>, DbErr> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT m.id, m.content, m.created_at FROM agent_messages m \
         JOIN agent_sessions s ON s.id = m.session_id \
         WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' \
           AND (s.context->>'venue') IS NOT DISTINCT FROM $2 \
           AND m.role = $6 AND m.created_at BETWEEN $4 AND $5 \
           AND position($3 in regexp_replace(m.content, '\\s+', ' ', 'g')) > 0 \
         ORDER BY m.created_at, m.id",
        vec![
            user_id.into(),
            place.map(str::to_string).into(),
            quote.text.as_str().into(),
            from.into(),
            to.into(),
            if hers { "assistant" } else { "user" }.into(),
        ],
    ))
    .await?
    .iter()
    .map(|row| {
        Ok((
            row.try_get("", "id")?,
            row.try_get("", "content")?,
            row.try_get("", "created_at")?,
        ))
    })
    .collect()
}

/// The person's lines `ids` (`agent_messages.id`), and her answer to each
/// line of theirs: what was said right after it, when that is hers.
pub(super) async fn delete_lines<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    ids: &[i32],
) -> Result<Vec<Gone>, DbErr> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    delete_messages(
        db,
        user_id,
        "WITH quoted AS ( \
           SELECT m.id, m.role, m.session_id, m.created_at FROM agent_messages m \
           JOIN agent_sessions s ON s.id = m.session_id \
           WHERE s.user_id = $1 AND m.id = ANY($2) AND m.role IN ('user', 'assistant') \
         ), answers AS ( \
           SELECT r.id FROM quoted q \
           JOIN LATERAL ( \
             SELECT a.id, a.role FROM agent_messages a \
             WHERE a.session_id = q.session_id AND a.role IN ('user', 'assistant') \
               AND (a.metadata->>'piece') IS NULL \
               AND (a.created_at, a.id) > (q.created_at, q.id) \
             ORDER BY a.created_at, a.id LIMIT 1) r ON r.role = 'assistant' \
           WHERE q.role = 'user' \
         ) \
         SELECT id FROM quoted UNION SELECT id FROM answers WHERE id IS NOT NULL",
        vec![user_id.into(), ids.to_vec().into()],
    )
    .await
}

/// Every line of a private day with the person.
async fn delete_day<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
) -> Result<Vec<Gone>, DbErr> {
    delete_messages(
        db,
        user_id,
        "SELECT m.id FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
         WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' \
           AND (s.context->>'venue') IS NULL \
           AND m.created_at >= $2 AND m.created_at < $3",
        vec![user_id.into(), start.into(), end.into()],
    )
    .await
}

/// Deletes `user_id`'s lines that `select` finds (their ids), keeps each as
/// forgotten ([`super::forgotten`]), and has each conversation count what is
/// left in it. The lines deleted, each of hers with what it taught her,
/// looked for before it went ([`Gone::taught`]).
async fn delete_messages<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    select: &str,
    values: Vec<sea_orm::Value>,
) -> Result<Vec<Gone>, DbErr> {
    let ids: Vec<i32> = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            select,
            values,
        ))
        .await?
        .iter()
        .map(|row| row.try_get("", "id"))
        .collect::<Result<_, DbErr>>()?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    // Hers, while they are here: what a memory is known to come from is
    // noted on the line (`tell`).
    let hers = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id, m.content, m.created_at, s.context->>'venue' AS venue \
             FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
             WHERE m.id = ANY($1) AND m.role = 'assistant' AND s.user_id = $2",
            [ids.clone().into(), user_id.into()],
        ))
        .await?;
    let mut taught: std::collections::BTreeMap<i32, Vec<String>> = Default::default();
    for line in &hers {
        let id: i32 = line.try_get("", "id")?;
        let content: String = line.try_get("", "content")?;
        let place: Option<String> = line.try_get("", "venue")?;
        let quotable = Quotable {
            id,
            role: "assistant",
            content: &content,
            at: line.try_get("", "created_at")?,
        };
        let of = quoting(db, user_id, place.as_deref(), &quotable).await?;
        if !of.is_empty() {
            taught.insert(id, of);
        }
    }
    let deleted = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM agent_messages WHERE id = ANY($1) \
             RETURNING session_id, id, role, log_row, content, created_at",
            [ids.into()],
        ))
        .await?;
    let marks = deleted
        .iter()
        .map(|row| {
            let at: DateTime<FixedOffset> = row.try_get("", "created_at")?;
            Ok(super::forgotten::line_mark(
                &row.try_get::<String>("", "role")?,
                at.timestamp_micros(),
                &row.try_get::<String>("", "content")?,
            ))
        })
        .collect::<Result<Vec<_>, DbErr>>()?;
    super::forgotten::keep(db, user_id, super::forgotten::LINE, &marks).await?;
    let lines = deleted
        .iter()
        .map(|row| {
            let id = row.try_get("", "id")?;
            Ok(Gone {
                id,
                session_id: row.try_get("", "session_id")?,
                role: row.try_get("", "role")?,
                log_row: row.try_get("", "log_row")?,
                taught: taught.remove(&id).unwrap_or_default(),
            })
        })
        .collect::<Result<Vec<_>, DbErr>>()?;
    let ids: Vec<String> = lines.iter().map(|line| line.id.to_string()).collect();
    super::forgotten::keep(db, user_id, super::forgotten::MESSAGE, &ids).await?;
    let mut sessions: Vec<String> = lines.iter().map(|line| line.session_id.clone()).collect();
    sessions.sort();
    sessions.dedup();
    if sessions.is_empty() {
        return Ok(lines);
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_sessions s SET message_count = \
           (SELECT COUNT(*) FROM agent_messages m WHERE m.session_id = s.id) \
         WHERE s.id = ANY($1)",
        [sessions.into()],
    ))
    .await?;
    Ok(lines)
}

/// Her chat diary of the person in `from..=to`: the entries holding
/// `quote`, or all of them.
async fn delete_diary<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    quote: Option<&Quote>,
    from: DateTime<FixedOffset>,
    to: DateTime<FixedOffset>,
) -> Result<(), DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM agent_diary WHERE user_id = $1 AND source = $2 \
           AND created_at BETWEEN $3 AND $4 \
           AND ($5::text IS NULL \
                OR CASE WHEN $6 THEN btrim(regexp_replace(content, '\\s+', ' ', 'g')) = $5 \
                   ELSE position($5 in regexp_replace(content, '\\s+', ' ', 'g')) > 0 END)",
        [
            user_id.into(),
            DIARY_SOURCE_CHAT.into(),
            from.into(),
            to.into(),
            quote.map(|quote| quote.text.clone()).into(),
            quote.is_some_and(|quote| quote.whole).into(),
        ],
    ))
    .await?;
    Ok(())
}

#[cfg(test)]
#[path = "origins_db_tests.rs"]
mod db_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_words_long_enough_are_quotes() {
        assert_eq!(quote(Some("  养了一只\n猫  ")), Some("养了一只 猫".into()));
        assert_eq!(quote(Some(r#"{"day":"2026-10-01"}"#)), None);
        assert_eq!(quote(Some("嗯")), None);
        assert_eq!(quote(None), None);
        let texts = |evidence| -> Vec<(String, bool)> {
            quotes(Some(evidence))
                .into_iter()
                .map(|quote| (quote.text, quote.whole))
                .collect()
        };
        assert_eq!(
            texts("上周养了一只猫 / 叫年糕 / 嗯 / 叫年糕"),
            [
                ("上周养了一只猫 / 叫年糕 / 嗯 / 叫年糕".into(), false),
                ("上周养了一只猫".into(), false),
                ("叫年糕".into(), true)
            ]
        );
        assert_eq!(texts("养了 猫"), [("养了 猫".into(), false)]);
        // One of several, few words: a line of only that, not one with it in.
        let parts = quotes(Some("我周五有考试 / 不是"));
        assert!(parts.iter().any(|quote| quote.is_in("不是。")));
        assert!(!parts.iter().any(|quote| quote.is_in("这不是我的")));
        assert!(parts.iter().any(|quote| quote.is_in("对了我周五有考试呢")));
        // A line with it in its own words is found by all of them.
        let score = quotes(Some("打 9 / 10 分"));
        assert!(
            score
                .iter()
                .any(|quote| quote.is_in("我给它打 9 / 10 分吧"))
        );
        assert!(!score.iter().any(|quote| quote.is_in("还差 10 分")));
        assert!(quotes(Some(r#"{"day":"2026-10-01"}"#)).is_empty());
        assert_eq!(place("private"), Some(None));
        assert_eq!(place("group:telegram:-1"), Some(Some("telegram:-1")));
        assert_eq!(place("own"), None);
    }
}
