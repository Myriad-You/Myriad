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
//!   from are earlier versions of what it kept, so they go too.
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

use super::unified::SHORT_NOTE_SOURCE;

/// How long before a memory was written the words it quotes were said:
/// a line is gone over within a day (queued work is kept that long).
pub const QUOTED_WITHIN: Duration = Duration::days(1);

/// Fewer characters than this say too little to be found by: a quote this
/// short is not looked for.
const SHORTEST_QUOTE: usize = 2;

/// Deletes what `rows` (a memory and its earlier versions, about to be
/// deleted on the same transaction) came from. The caller holds the
/// memory-work lock ([`memory_jobs::lock`]) from before it read them, so no
/// queued job writes in between.
pub async fn forget<C: ConnectionTrait>(
    db: &C,
    rows: &[agent_memories::Model],
) -> Result<(), DbErr> {
    for row in rows {
        let Some(user_id) = row.user_id.filter(|id| *id > 0) else {
            continue;
        };
        let Some(place) = place(&row.venue) else {
            continue;
        };
        match row.source.as_str() {
            source if source == SHORT_NOTE_SOURCE || source == "chat" || source == SAID_SOURCE => {
                let Some(quote) = quote(row.evidence.as_deref()) else {
                    continue;
                };
                let from = row.created_at - QUOTED_WITHIN;
                delete_quoted(db, user_id, place, &quote, from, row.created_at).await?;
                delete_diary(db, user_id, Some(&quote), from, row.created_at).await?;
                memory_jobs::forget_holding(db, user_id, &quote).await?;
            }
            source if source == chat_days::SOURCE && place.is_none() => {
                let Some((start, end)) = day(row.evidence.as_deref()) else {
                    continue;
                };
                delete_day(db, user_id, start, end).await?;
                delete_diary(db, user_id, None, start, end).await?;
            }
            _ => {}
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

/// The words as said, with runs of space as one: a quote is plain text (a
/// record's evidence is JSON and quotes nothing).
fn quote(evidence: Option<&str>) -> Option<String> {
    let evidence = evidence?.trim();
    if evidence.starts_with('{') || evidence.starts_with('[') {
        return None;
    }
    let quote = evidence.split_whitespace().collect::<Vec<_>>().join(" ");
    (quote.chars().count() >= SHORTEST_QUOTE).then_some(quote)
}

/// The day a day's line is about, from its first midnight to the next.
fn day(evidence: Option<&str>) -> Option<(DateTime<FixedOffset>, DateTime<FixedOffset>)> {
    let value: serde_json::Value = serde_json::from_str(evidence?).ok()?;
    let day: NaiveDate = value["day"].as_str()?.parse().ok()?;
    let clock = crate::services::agent::merope::clock::day_start;
    Some((clock(day)?, clock(day.succ_opt()?)?))
}

/// The person's lines in `place` saying `quote`, said in `from..=to`, and
/// her answer to each line of theirs.
async fn delete_quoted<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    place: Option<&str>,
    quote: &str,
    from: DateTime<FixedOffset>,
    to: DateTime<FixedOffset>,
) -> Result<(), DbErr> {
    delete_messages(
        db,
        "WITH quoted AS ( \
           SELECT m.id, m.role, m.session_id, m.created_at FROM agent_messages m \
           JOIN agent_sessions s ON s.id = m.session_id \
           WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' \
             AND (s.context->>'venue') IS NOT DISTINCT FROM $2 \
             AND m.role IN ('user', 'assistant') AND m.created_at BETWEEN $4 AND $5 \
             AND position($3 in regexp_replace(m.content, '\\s+', ' ', 'g')) > 0 \
         ), answers AS ( \
           SELECT (SELECT r.id FROM agent_messages r \
                   WHERE r.session_id = q.session_id AND r.role = 'assistant' \
                     AND r.created_at > q.created_at \
                   ORDER BY r.created_at LIMIT 1) AS id \
           FROM quoted q WHERE q.role = 'user' \
         ) \
         DELETE FROM agent_messages WHERE id IN ( \
           SELECT id FROM quoted UNION SELECT id FROM answers WHERE id IS NOT NULL \
         ) RETURNING session_id",
        vec![
            user_id.into(),
            place.map(str::to_string).into(),
            quote.into(),
            from.into(),
            to.into(),
        ],
    )
    .await
}

/// Every line of a private day with the person.
async fn delete_day<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
) -> Result<(), DbErr> {
    delete_messages(
        db,
        "DELETE FROM agent_messages m USING agent_sessions s \
         WHERE s.id = m.session_id AND s.user_id = $1 AND s.context->>'mode' = 'chat' \
           AND (s.context->>'venue') IS NULL \
           AND m.created_at >= $2 AND m.created_at < $3 \
         RETURNING m.session_id",
        vec![user_id.into(), start.into(), end.into()],
    )
    .await
}

/// Runs a delete of lines returning their conversations, and has each
/// conversation count what is left in it.
async fn delete_messages<C: ConnectionTrait>(
    db: &C,
    delete: &str,
    values: Vec<sea_orm::Value>,
) -> Result<(), DbErr> {
    let sessions: Vec<String> = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            delete,
            values,
        ))
        .await?
        .iter()
        .filter_map(|row| row.try_get::<String>("", "session_id").ok())
        .collect();
    if sessions.is_empty() {
        return Ok(());
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_sessions s SET message_count = \
           (SELECT COUNT(*) FROM agent_messages m WHERE m.session_id = s.id) \
         WHERE s.id = ANY($1)",
        [sessions.into()],
    ))
    .await?;
    Ok(())
}

/// Her chat diary of the person in `from..=to`: the entries holding
/// `quote`, or all of them.
async fn delete_diary<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    quote: Option<&str>,
    from: DateTime<FixedOffset>,
    to: DateTime<FixedOffset>,
) -> Result<(), DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM agent_diary WHERE user_id = $1 AND source = $2 \
           AND created_at BETWEEN $3 AND $4 \
           AND ($5::text IS NULL \
                OR position($5 in regexp_replace(content, '\\s+', ' ', 'g')) > 0)",
        [
            user_id.into(),
            DIARY_SOURCE_CHAT.into(),
            from.into(),
            to.into(),
            quote.map(str::to_string).into(),
        ],
    ))
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_words_long_enough_are_quotes() {
        assert_eq!(quote(Some("  养了一只\n猫  ")), Some("养了一只 猫".into()));
        assert_eq!(quote(Some(r#"{"day":"2026-10-01"}"#)), None);
        assert_eq!(quote(Some("嗯")), None);
        assert_eq!(quote(None), None);
        assert_eq!(place("private"), Some(None));
        assert_eq!(place("group:telegram:-1"), Some(Some("telegram:-1")));
        assert_eq!(place("own"), None);
    }
}
