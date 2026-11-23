//! A stretch another part of her needs: gone over now, and how it fares.

use super::*;

/// Another part of her needs what was said in `id`'s stretch: it is gone
/// over now rather than once they go quiet. Where its lines not yet gone
/// over begin, when there are any of theirs (none, nothing to hurry); a
/// stretch being gone over already is waited on as it is, and one that
/// failed keeps its wait before the next try.
pub(in crate::services::agent::merope) async fn hurry(
    db: &DatabaseConnection,
    id: &str,
    owner: i32,
) -> anyhow::Result<Option<chrono::DateTime<chrono::FixedOffset>>> {
    let tx = db.begin().await?;
    lock(&tx).await?;
    let now = now(&tx).await?;
    let Some(mut job) = registry::get::<Job>(&tx, NAMESPACE, id).await? else {
        return Ok(None);
    };
    let Some(Payload::Settle { from, heard, .. }) = &job.data else {
        return Ok(None);
    };
    if job.owner != owner || heard.is_empty() {
        return Ok(None);
    }
    let from = *from;
    let in_flight = job.token.is_some() && job.lease > now;
    if !in_flight && job.attempts == 0 && job.ready > now {
        job.ready = now;
        save(&tx, id, &job, now).await?;
    }
    tx.commit().await?;
    Ok(Some(from))
}

/// Where a stretch another part of her waits on is ([`waiting_for`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::services::agent::merope) enum WaitingFor {
    Waiting,
    /// All of it gone over: the job holds nothing, or what came after.
    Over,
    /// Let go unread after its last try: it goes with the next.
    Unread,
}

/// How `id`'s stretch that began `from` fares: waiting while the job holds
/// any of it, however it was taken up (with what was brought along from
/// before it, or a pass of it left).
pub(in crate::services::agent::merope) async fn waiting_for(
    db: &DatabaseConnection,
    id: &str,
    from: chrono::DateTime<chrono::FixedOffset>,
) -> anyhow::Result<WaitingFor> {
    let Some(job) = registry::get::<Job>(db, NAMESPACE, id).await? else {
        return Ok(WaitingFor::Over);
    };
    if let Some(Payload::Settle { from: now, .. }) = &job.data
        && job.said_from.unwrap_or(*now).min(*now) <= from
    {
        return Ok(WaitingFor::Waiting);
    }
    Ok(match job.unread_from {
        Some(unread) if unread <= from => WaitingFor::Unread,
        _ => WaitingFor::Over,
    })
}
