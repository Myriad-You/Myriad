//! What the site's own Work did for the owner while she lived on another
//! end (H3e-9): Work runs on the site then as the plain assistant, without
//! her, and how each run ended is kept here for her (`agent_kept_for_her`),
//! in the order it ended. She takes it in once she is back on the site, as
//! she would have then, told it happened while she was away; with H4 her
//! host takes it over the link. Only Work outcomes are kept: everything else
//! that would have reached her meanwhile was for someone present.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

/// The Work outcomes she is told of.
const WORK_OUTCOMES: [&str; 3] = [
    "agent.task_completed",
    "agent.task_failed",
    "agent.task_cancelled",
];

fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values)
}

/// Kept while she lives on another end, or her mind is on one: a Work
/// outcome (not while she is moving: no Work runs then).
pub(crate) fn keeps(event_key: &str) -> bool {
    WORK_OUTCOMES.contains(&event_key)
        && matches!(
            crate::services::persona_sync::lease::place(),
            crate::services::persona_sync::lease::Where::Elsewhere(_)
                | crate::services::persona_sync::lease::Where::Vacant
                | crate::services::persona_sync::lease::Where::MindOn(_)
        )
}

pub(crate) async fn keep(db: &DatabaseConnection, user_id: i32, event_key: &str, summary: &str) {
    let summary: String = summary.chars().take(2000).collect();
    if let Err(error) = db
        .execute_raw(statement(
            "INSERT INTO agent_kept_for_her (user_id, event_key, summary) VALUES ($1, $2, $3)",
            vec![user_id.into(), event_key.into(), summary.into()],
        ))
        .await
    {
        tracing::warn!(%error, event_key, "[Merope] could not keep a Work outcome for her");
    } else {
        tracing::info!(
            event_key,
            "[Merope] a Work outcome is kept for her until she is back"
        );
    }
}

/// She is back on the site: what was kept for her reaches her in the order
/// it happened, each told as happening while she was away. A row goes once
/// she took it in (or it is not for anyone she knows any more).
pub(crate) async fn replay(db: DatabaseConnection) {
    let rows = match db
        .query_all_raw(statement(
            "SELECT id, user_id, event_key, summary, happened_at FROM agent_kept_for_her ORDER BY id",
            vec![],
        ))
        .await
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(%error, "[Merope] could not read what was kept for her");
            return;
        }
    };
    for row in rows {
        let (Ok(id), Ok(user_id), Ok(event_key), Ok(summary), Ok(at)) = (
            row.try_get::<i64>("", "id"),
            row.try_get::<i32>("", "user_id"),
            row.try_get::<String>("", "event_key"),
            row.try_get::<String>("", "summary"),
            row.try_get::<chrono::DateTime<chrono::FixedOffset>>("", "happened_at"),
        ) else {
            continue;
        };
        let told = format!(
            "While you were away from this site ({}): {summary}",
            at.with_timezone(&chrono::Utc).format("%Y-%m-%d %H:%M UTC")
        );
        if let Err(error) = super::ingest::ingest(&db, user_id, &event_key, &told).await {
            tracing::warn!(%error, event_key, "[Merope] a Work outcome kept for her did not reach her");
            // She is away again, or it failed: the rest waits for next time.
            if !crate::services::persona_sync::lease::place().here() {
                return;
            }
        }
        let _ = db
            .execute_raw(statement(
                "DELETE FROM agent_kept_for_her WHERE id = $1",
                vec![id.into()],
            ))
            .await;
    }
}
