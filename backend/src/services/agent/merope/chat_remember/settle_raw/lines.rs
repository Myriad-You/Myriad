//! The lines of a private talk as they are kept (`agent_messages`): which
//! are gone over with a stretch, and which were already.

use myriad_merope::settle_raw as raw;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use crate::services::agent::memory::unified::Audience;
use crate::services::agent::merope::memory_jobs::Failure;

/// How far back from the last line gone over a stretch looks for lines
/// written late, in seconds.
const READ_SLACK: i64 = 120;

/// One line of the talk, as it was said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::services::agent::merope) struct Spoken {
    /// Its message (`agent_messages.id`).
    pub id: i32,
    pub theirs: bool,
    pub text: String,
    pub at: chrono::DateTime<chrono::FixedOffset>,
    /// Already gone over, or kept on another of her ends: shown as context
    /// only.
    pub gone: bool,
    /// A line of a game they were playing.
    pub in_game: bool,
}

/// Their words as a stretch holds them: however long, when there is enough
/// in them to be gone over.
pub(in crate::services::agent::merope) fn words_of(text: &str) -> Option<String> {
    let words = crate::services::agent::merope::ingest::redact_event_text(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (super::super::should_extract_chat_remember(&words) || words.chars().count() > 2_000)
        .then_some(words)
}

/// Their message in this private talk that `words` ([`words_of`]) are, noted
/// as said at `input_at`, and when it was kept: the newest by then saying
/// that, or the newest. A stretch is of its messages, each said when it was
/// kept (a moment before it is noted as said).
pub(in crate::services::agent::merope::chat_remember) async fn their_message(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    present: &Audience,
    words: &str,
    input_at: chrono::DateTime<chrono::FixedOffset>,
) -> Option<(i32, chrono::DateTime<chrono::FixedOffset>)> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id, m.content, m.created_at FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE (s.context->>'past') IS NULL AND s.user_id = $1 AND s.context->>'mode' = 'chat' \
               AND (s.context->>'venue') IS NOT DISTINCT FROM $2 \
               AND m.role = 'user' AND (m.metadata->>'piece') IS NULL AND m.origin IS NULL \
               AND m.created_at BETWEEN $3 AND $4 \
             ORDER BY m.created_at DESC, m.id DESC LIMIT 8",
            [
                user_id.into(),
                present.group_id().map(str::to_string).into(),
                (input_at - chrono::Duration::days(1)).into(),
                input_at.into(),
            ],
        ))
        .await
        .ok()?;
    let mut newest = None;
    for row in rows {
        let said = (
            row.try_get::<i32>("", "id").ok()?,
            row.try_get("", "created_at").ok()?,
        );
        let content: String = row.try_get("", "content").ok()?;
        if words_of(&content).as_deref() == Some(words) {
            return Some(said);
        }
        newest = newest.or(Some(said));
    }
    newest
}

/// Notes their message as a line of a game they were playing.
pub(in crate::services::agent::merope::chat_remember) async fn of_a_game(
    db: &sea_orm::DatabaseConnection,
    message: i32,
) {
    let _ = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_messages SET metadata = (COALESCE(metadata::jsonb, '{}'::jsonb) \
               || '{\"inGame\":true}'::jsonb)::json WHERE id = $1",
            [message.into()],
        ))
        .await;
}

/// The lines of a talk gone over lately (each when it was said, and its
/// message), those within [`READ_SLACK`] of the last: what the next
/// stretch need not go over again.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(in crate::services::agent::merope) struct Read {
    pub lines: Vec<(chrono::DateTime<chrono::FixedOffset>, i32)>,
    /// The message a pass ended within, and how many of its parts are gone
    /// over (`raw::line_parts`).
    #[serde(default)]
    pub partway: Option<(i32, usize)>,
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

    /// Where a stretch from `from` looks for lines from: a little before
    /// the last one gone over, for lines written late; a little before
    /// `from` with none gone over that is known of, for what was noted as
    /// said after its message was kept.
    pub fn looks_from(
        read: Option<&Self>,
        from: chrono::DateTime<chrono::FixedOffset>,
    ) -> chrono::DateTime<chrono::FixedOffset> {
        let slack = chrono::Duration::seconds(READ_SLACK);
        read.and_then(Self::to)
            .map_or(from - slack, |to| (to - slack).min(from))
    }
}

/// Of `lines`, read now: each is gone over once.
pub(super) fn read_of(lines: &[Spoken]) -> Vec<(chrono::DateTime<chrono::FixedOffset>, i32)> {
    let mut read: Vec<_> = lines.iter().map(|line| (line.at, line.id)).collect();
    read.dedup();
    read
}

/// Which lines of a talk: the last [`raw::BEFORE_SHOWN`] before a moment, or
/// the first so many from it on.
#[derive(Clone, Copy)]
pub(super) enum Lines {
    Before,
    From(i64),
}

/// The lines here from `from` on or ([`Lines::Before`]) before it, oldest
/// first. One kept on another of her ends (copied from there, or left for
/// her there) is context only; a draft never sent was not said.
pub(super) async fn lines(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    present: &Audience,
    from: chrono::DateTime<chrono::FixedOffset>,
    which: Lines,
) -> Result<Vec<Spoken>, Failure> {
    let (side, order, limit) = match which {
        Lines::Before => ("<", "DESC", raw::BEFORE_SHOWN as i64),
        Lines::From(at_most) => (">=", "ASC", at_most),
    };
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "SELECT m.id, m.role, m.content, m.created_at, \
                   COALESCE(m.metadata->>'inGame', '') = 'true' AS in_game, \
                   {ELSEWHERE} AS elsewhere \
                 FROM agent_messages m \
                 JOIN agent_sessions s ON s.id = m.session_id \
                 WHERE (s.context->>'past') IS NULL AND s.user_id = $1 AND s.context->>'mode' = 'chat' \
                   AND (s.context->>'venue') IS NOT DISTINCT FROM $2 \
                   AND {SAID} \
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
    let mut lines = Vec::new();
    for row in &rows {
        let read = || -> Result<Spoken, sea_orm::DbErr> {
            Ok(Spoken {
                id: row.try_get("", "id")?,
                theirs: row.try_get::<String>("", "role")? == "user",
                text: row.try_get("", "content")?,
                at: row.try_get("", "created_at")?,
                gone: row.try_get("", "elsewhere")?,
                in_game: row.try_get("", "in_game")?,
            })
        };
        lines.push(read().map_err(|_| Failure::Storage)?);
    }
    if matches!(which, Lines::Before) {
        lines.reverse();
    }
    Ok(lines)
}

/// Whether a line of a talk, `agent_messages` `m`, is none of hers to keep
/// here: kept on another of her ends (copied from there, left for her there,
/// or said from there through the site), or said with her switch off.
pub(in crate::services::agent::merope) const ELSEWHERE: &str = "(m.origin IS NOT NULL \
     OR COALESCE(m.metadata->>'leftForHer', '') = 'true' \
     OR COALESCE(m.metadata->>'withoutHer', '') = 'true' \
     OR (m.metadata->>'lentSay') IS NOT NULL)";

/// What was said in a talk, of `agent_messages` `m`: hers and theirs, no
/// piece of work coming back, no draft never sent.
pub(in crate::services::agent::merope) const SAID: &str = "m.role IN ('user', 'assistant') \
     AND (m.metadata->>'piece') IS NULL AND (m.metadata->>'cutOff') IS DISTINCT FROM 'unsent'";
