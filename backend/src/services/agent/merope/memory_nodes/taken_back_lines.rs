//! Their private lines that went with a memory they took back, on her
//! nodes: what cites one is a span or an item in her inbox, which name their
//! lines (`message:<id>`), not a memory. What was made of them is hidden
//! there ([`super::super::hidden`]), and shown again should it come back; it
//! is never forgotten: which lines those are is told by their words where it
//! is not known, and they say other things too.

use std::collections::HashSet;

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use super::{Error, OF_A_LINE};
use crate::services::agent::merope::chat_lines;

/// Their private lines that went with the memories `memories` they took
/// back (`chat_lines::taken_back_with`).
pub(super) async fn lines_of<C: ConnectionTrait>(
    db: &C,
    memories: &[String],
) -> Result<HashSet<i32>, Error> {
    let whose = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT user_id FROM agent_memories \
             WHERE id = ANY($1) AND user_id IS NOT NULL AND venue = 'private'",
            [memories.to_vec().into()],
        ))
        .await?;
    let mut lines = HashSet::new();
    for row in whose {
        let user_id: i32 = row.try_get("", "user_id")?;
        lines.extend(
            chat_lines::taken_back_with(db, user_id, |memory| {
                memories.iter().any(|one| one == memory)
            })
            .await?,
        );
    }
    Ok(lines)
}

/// Whether what `user_id`'s `audience` holds rests on one of their lines
/// that went with a memory they took back (a span, her inbox, a version
/// resting on one).
pub(super) async fn lines_cited(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    audience: &str,
) -> Result<bool, Error> {
    let gone = chat_lines::taken_back_with(db, user_id, |_| true).await?;
    if gone.is_empty() {
        return Ok(false);
    }
    let refs: Vec<String> = gone.iter().map(|id| format!("message:{id}")).collect();
    Ok(db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 AS one FROM ( \
               SELECT c.ref FROM agent_node_rows r \
               CROSS JOIN LATERAL jsonb_array_elements_text( \
                 COALESCE((r.body::jsonb)->(CASE WHEN r.kind = 'node.span' THEN 'refs' \
                                                 ELSE 'because' END), '[]'::jsonb)) AS c(ref) \
               WHERE r.user_id = $1 AND r.audience = $2 AND r.body <> '' AND r.kind = ANY($3) \
               UNION ALL \
               SELECT c.ref FROM agent_node_inbox i \
               CROSS JOIN LATERAL jsonb_array_elements_text( \
                 COALESCE(i.item->'refs', '[]'::jsonb)) AS c(ref) \
               WHERE i.user_id = $1 AND i.audience = $2) cited \
             WHERE cited.ref = ANY($4) LIMIT 1",
            [
                user_id.into(),
                audience.into(),
                OF_A_LINE
                    .iter()
                    .map(|kind| kind.to_string())
                    .collect::<Vec<_>>()
                    .into(),
                refs.into(),
            ],
        ))
        .await?
        .is_some())
}

/// What rests on `user_id`'s lines they took back in the stretch they said
/// them in, or that are what they asked her not to keep, hidden on her nodes
/// at once (their heads written again). If this fails, they are as hidden
/// once her nodes are next read, or those heads written.
pub(crate) async fn hide_lines_let_go(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    lines: &[i32],
) {
    use sea_orm::TransactionTrait;
    let result = async {
        let tx = db.begin().await?;
        let lines: HashSet<i32> = lines.iter().copied().collect();
        for (user_id, audience) in super::super::hidden::citing(&tx, &[], &lines).await? {
            super::super::hidden::rewrite(&tx, user_id, &audience).await?;
        }
        Ok::<(), Error>(tx.commit().await?)
    }
    .await;
    if let Err(error) = result {
        // Nothing read since tells of it (`super::before_reading`).
        super::sweep_again(user_id);
        tracing::warn!(
            user_id,
            kind = super::error_kind(&error),
            "[memory-nodes] what rests on lines they took back is hidden when next written"
        );
    }
}
