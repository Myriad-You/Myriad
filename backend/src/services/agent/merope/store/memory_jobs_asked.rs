//! What they asked her not to keep, held to next: the lines of a stretch,
//! and what she held of them before.

use super::*;

/// The stretch `job` holds, of someone who asked her not to keep something
/// (in this very stretch, it may be): its lines are judged next, and not
/// looked back to until they are. Written with whatever came of it.
pub(super) async fn judge_lines(
    tx: &sea_orm::DatabaseTransaction,
    job: &Job,
) -> anyhow::Result<()> {
    let Some(Payload::Settle {
        present,
        from,
        last,
        ..
    }) = &job.data
    else {
        return Ok(());
    };
    if crate::services::agent::memory::unified::asked_not_to_keep(tx, job.owner)
        .await?
        .is_empty()
    {
        return Ok(());
    }
    let from = job.said_from.unwrap_or(*from);
    let id = super::super::super::memory_jobs::key(&[
        "not_kept",
        &job.owner.to_string(),
        &present.venue(),
        &from.to_rfc3339(),
        &last.to_rfc3339(),
    ]);
    let data = Payload::NotKept {
        present: present.clone(),
        from,
        last: *last,
    };
    enqueue_in(tx, &id, job.owner, data, None).await
}

/// What `owner` just asked her not to keep (`updates`) is held to what she
/// held of them before, in private and in each group they talk with her in
/// that she holds anything of, next. Written with the request.
pub(super) async fn judge_old(
    tx: &sea_orm::DatabaseTransaction,
    owner: i32,
    updates: &super::super::super::chat_remember::ChatMemoryUpdates,
) -> anyhow::Result<()> {
    use super::super::super::asked::old::IN_GROUPS;
    if updates.asked.is_empty() {
        return Ok(());
    }
    let asked = crate::clock::now().fixed_offset();
    let groups = tx
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT substr(venue, length('group:') + 1) AS venue FROM agent_memories \
             WHERE venue LIKE 'group:%' AND source = ANY($1) AND invalid_at IS NULL \
               AND substr(venue, length('group:') + 1) IN ( \
                 SELECT context->>'venue' FROM agent_sessions \
                 WHERE user_id = $2 AND context->>'mode' = 'chat')",
            [IN_GROUPS.map(str::to_string).to_vec().into(), owner.into()],
        ))
        .await?;
    let mut places = vec![None];
    for row in groups {
        places.push(Some(row.try_get::<String>("", "venue")?));
    }
    for venue in places {
        let id = super::super::super::memory_jobs::key(&[
            "asked_over",
            &owner.to_string(),
            venue.as_deref().unwrap_or("private"),
            &asked.to_rfc3339(),
        ]);
        enqueue_in(tx, &id, owner, Payload::AskedOver { asked, venue }, None).await?;
    }
    Ok(())
}
