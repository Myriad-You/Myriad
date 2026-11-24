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
        let data = Payload::AskedOver {
            asked,
            venue,
            stays: Vec::new(),
        };
        enqueue_in(tx, &id, owner, data, None).await?;
    }
    Ok(())
}

/// A job as it was taken up, under which what comes of it is written: only
/// while it is its own still.
#[derive(Clone, Copy)]
pub(crate) struct Own<'a>(&'a Claim);

impl<'a> Own<'a> {
    pub(in crate::services::agent::merope) fn of(claim: &'a Claim) -> Self {
        Self(claim)
    }

    /// The job as it is now, while it is its own. On a transaction that
    /// holds [`lock`].
    async fn job(&self, tx: &sea_orm::DatabaseTransaction) -> anyhow::Result<Option<Job>> {
        let now = now(tx).await?;
        Ok(registry::get::<Job>(tx, self.0.job.namespace(), &self.0.id)
            .await?
            .filter(|job| job.token.is_some() && job.token == self.0.job.token && job.lease > now))
    }

    /// Whether it is its own still, and may be written: not once she was
    /// cleared, the login it was taken up under went, or what it is of was
    /// forgotten. On a transaction, held from here on till it ends.
    pub(in crate::services::agent::merope) async fn still(
        &self,
        tx: &sea_orm::DatabaseTransaction,
    ) -> anyhow::Result<bool> {
        lock(tx).await?;
        match self.job(tx).await? {
            Some(job) => current(tx, &job, true).await,
            None => Ok(false),
        }
    }

    pub(in crate::services::agent::merope) fn owner(&self) -> i32 {
        self.0.job.owner
    }

    pub(in crate::services::agent::merope) fn gives_up(&self) -> bool {
        self.0.gives_up
    }

    /// Of what it holds to what they asked, `told` (each as it is known
    /// there) stays: not asked about again on another try.
    pub(in crate::services::agent::merope) async fn stays(
        &self,
        db: &DatabaseConnection,
        told: Vec<String>,
    ) -> anyhow::Result<()> {
        if told.is_empty() {
            return Ok(());
        }
        let tx = db.begin().await?;
        lock(&tx).await?;
        let Some(mut job) = self.job(&tx).await? else {
            return Ok(());
        };
        let Some(Payload::AskedOver { stays, .. }) = &mut job.data else {
            return Ok(());
        };
        stays.extend(told);
        stays.sort_unstable();
        stays.dedup();
        let now = now(&tx).await?;
        save(&tx, &self.0.id, &job, now).await?;
        tx.commit().await?;
        Ok(())
    }

    /// What was told so on the tries before.
    pub(in crate::services::agent::merope) fn stayed(&self) -> &'a [String] {
        match &self.0.job.data {
            Some(Payload::AskedOver { stays, .. }) => stays,
            _ => &[],
        }
    }
}
