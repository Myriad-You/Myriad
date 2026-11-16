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
            })
        })
        .collect();
    lines.reverse();
    let gone = withdrawn(db, user_id, &lines).await;
    lines.retain(|line| !gone.contains(&line.id));
    lines
}

/// Of `lines` (oldest first), those they took back: what a memory they took
/// back quotes (one retired as let go, with nothing in its place, or
/// deleted), said within a day before it was written, and her answer right
/// after each line of theirs. A let-go memory keeps its row and its words
/// stay in the chat (「当作没说过」 is not deleting): only she does not bring
/// them up again. Read when looked for, so a taking back that came from
/// another end counts too.
async fn withdrawn(db: &DatabaseConnection, user_id: i32, lines: &[Line]) -> HashSet<i32> {
    let Ok(rows) = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT evidence, created_at FROM agent_memories \
             WHERE user_id = $1 AND venue = 'private' AND source = ANY($2) \
               AND (invalid_reason = 'deleted' \
                    OR (invalid_reason = 'superseded' AND superseded_by IS NULL))",
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
    else {
        return HashSet::new();
    };
    let taken_back: Vec<(String, DateTime<FixedOffset>)> = rows
        .iter()
        .filter_map(|row| {
            let evidence: Option<String> = row.try_get("", "evidence").ok()?;
            Some((
                origins::quote(evidence.as_deref())?,
                row.try_get("", "created_at").ok()?,
            ))
        })
        .collect();
    let mut gone = HashSet::new();
    for (index, line) in lines.iter().enumerate() {
        let said = line
            .content
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let quoted = taken_back.iter().any(|(quote, written)| {
            line.at <= *written
                && line.at >= *written - origins::QUOTED_WITHIN
                && said.contains(quote)
        });
        if !quoted {
            continue;
        }
        gone.insert(line.id);
        if !line.hers
            && let Some(answer) = lines.get(index + 1).filter(|next| next.hers)
        {
            gone.insert(answer.id);
        }
    }
    gone
}

#[cfg(test)]
#[path = "chat_lines_tests.rs"]
pub(in crate::services::agent::merope) mod tests;
