pub(super) mod curiosity;
pub(super) mod memory_jobs;

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Statement,
    TransactionTrait,
};
use serde_json::Value;
use uuid::Uuid;

use crate::models::entities::{
    agent_addressee_state, agent_diary, agent_persona, agent_proactive_messages, agent_sessions,
};
use crate::services::agent::memory::unified::Priming;

use super::state::{
    Affect, AffectBaseline, apply_music_listening, clamp, persona_affect_baseline, settle,
};

mod addressee;
mod diary;
mod generation;
mod memory;
mod persona;
mod proactive;

pub use addressee::*;
pub use diary::*;
pub use generation::*;
pub use memory::*;
pub use persona::*;
pub use proactive::*;

#[cfg(test)]
#[path = "store_memory_tests.rs"]
mod memory_tests;

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

/// Chat lines since `since`, oldest first: (when, the line, whether hers).
/// Each line of a message is one line, as it went out.
pub async fn chat_lines_since(
    db: &sea_orm::DatabaseConnection,
    since: chrono::DateTime<chrono::FixedOffset>,
    limit: i64,
) -> Result<Vec<(chrono::DateTime<chrono::FixedOffset>, String, bool)>, sea_orm::DbErr> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.created_at, m.content, m.role FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE m.role IN ('user', 'assistant') AND s.context->>'mode' = 'chat' \
               AND m.created_at >= $1 \
             ORDER BY m.created_at ASC LIMIT $2",
            [since.into(), limit.into()],
        ))
        .await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let at = row
                .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                .ok()?;
            let content = row.try_get::<String>("", "content").ok()?;
            let hers = row.try_get::<String>("", "role").ok()? == "assistant";
            Some(
                content
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(|line| (at, line.to_string(), hers))
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .collect())
}
