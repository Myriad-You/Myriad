//! When what she knows of someone would come to mind, for facts kept before
//! a stretch of talk gone over wrote it with each (see
//! `myriad_merope::cues`): a few at a time, at upkeep, billed to whom they
//! are about.

use std::time::Duration;

use myriad_merope::cues::{AT_ONCE, SCHEMA_NAME, input, parse, schema, system};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use super::super::call::{self, Voice};
use crate::services::agent::memory::unified;

/// What she knows for good of someone.
const SOURCES: &[&str] = &["chat"];
/// People gone over in one upkeep, and how many asks each.
const PEOPLE: i64 = 10;
const ASKS: usize = 3;
const CALL_TIMEOUT: Duration = Duration::from_secs(45);

/// Write when they would come to mind for facts still without it.
pub async fn write_missing(db: &DatabaseConnection) {
    let people: Vec<i32> = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT m.user_id FROM agent_memories m \
             WHERE m.user_id IS NOT NULL AND m.source = ANY($1) AND m.invalid_at IS NULL \
               AND NOT EXISTS (SELECT 1 FROM agent_memory_cues c WHERE c.memory_id = m.id) \
             LIMIT $2",
            [
                SOURCES
                    .iter()
                    .map(|source| source.to_string())
                    .collect::<Vec<_>>()
                    .into(),
                PEOPLE.into(),
            ],
        ))
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| row.try_get::<i32>("", "user_id").ok())
        .collect();
    for user_id in people {
        for _ in 0..ASKS {
            let rows = unified::without_cues(db, user_id, SOURCES, AT_ONCE as u64).await;
            if rows.is_empty() || write_for(db, user_id, &rows).await == 0 {
                break;
            }
        }
    }
}

/// Ask once for `rows`; how many got cues.
async fn write_for(
    db: &DatabaseConnection,
    user_id: i32,
    rows: &[crate::models::entities::agent_memories::Model],
) -> usize {
    let facts: Vec<String> = rows
        .iter()
        .map(|row| super::super::ingest::compact_summary(&row.content))
        .collect();
    let Ok(raw) = call::Ask::new(Voice::Judge, user_id, "memory_cues")
        .within(CALL_TIMEOUT)
        .json_raw(system(), &input(&facts), SCHEMA_NAME, &schema())
        .await
    else {
        return 0;
    };
    let mut kept = 0;
    for (index, cues) in parse(&raw, rows.len()).unwrap_or_default() {
        if matches!(
            unified::keep_cues(db, &rows[index].id, &cues).await,
            Ok(true)
        ) {
            kept += 1;
        }
    }
    if kept > 0 {
        tracing::info!(
            kept,
            "[Merope] wrote when what she knows would come to mind"
        );
    }
    kept
}
