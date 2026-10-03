//! Durable memory work. Short transactions serialize admission, leases and
//! persona replacement; no transaction or lock is held across model I/O.
use super::super::memory_jobs::{Effect, Failure, Payload};
use crate::services::runtime_registry as registry;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

pub(super) const NAMESPACE: &str = "merope_memory_jobs";
const EVENTS: &str = "merope_memory_events";
const RETENTION: i64 = 86_400;
const CAPACITY: i64 = 2048;
pub(in crate::services::agent::merope) const MAX_ATTEMPTS: u8 = 3;
const LEASE: i64 = 60;
/// How long words held as they land wait for her answer before they are gone
/// over without it.
const HELD_FOR: i64 = 120;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in crate::services::agent::merope) struct Job {
    pub owner: i32,
    pub persona: Option<chrono::DateTime<chrono::FixedOffset>>,
    pub epoch: i64,
    pub data: Option<Payload>,
    pub ready: i64,
    pub last_arrival: i64,
    pub lease: i64,
    pub token: Option<String>,
    pub attempts: u8,
    pub claimed_count: usize,
    pub outcome: Option<String>,
    /// What the same person said while this was being gone over: it is gone
    /// over next, never dropped and never written ahead of what came before.
    #[serde(default)]
    pub next: Option<Payload>,
}
#[derive(Debug, Clone)]
pub(in crate::services::agent::merope) struct Claim {
    pub id: String,
    pub job: Job,
}

/// Also taken first by persona replacement/deletion, on their transaction.
pub(in crate::services::agent::merope) async fn lock(
    db: &impl ConnectionTrait,
) -> anyhow::Result<()> {
    db.execute_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(1296388170, 1)".to_string(),
    ))
    .await?;
    Ok(())
}
async fn now(db: &impl ConnectionTrait) -> anyhow::Result<i64> {
    Ok(db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT EXTRACT(EPOCH FROM clock_timestamp())::BIGINT AS now".to_string(),
        ))
        .await?
        .ok_or_else(|| anyhow::anyhow!("missing clock"))?
        .try_get("", "now")?)
}
async fn epoch(db: &impl ConnectionTrait, owner: i32, locked: bool) -> anyhow::Result<Option<i64>> {
    let sql = if locked {
        "SELECT token_version FROM users WHERE id = $1 FOR SHARE"
    } else {
        "SELECT token_version FROM users WHERE id = $1"
    };
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        [owner.into()],
    ))
    .await?
    .map(|row| crate::middleware::auth::row_session_epoch(&row).map_err(Into::into))
    .transpose()
}
async fn save(db: &impl ConnectionTrait, id: &str, job: &Job, now: i64) -> anyhow::Result<()> {
    registry::put(
        db,
        NAMESPACE,
        id,
        registry::RegistryIdentity {
            subject_id: Some(job.owner),
            owner_id: Some(job.owner),
            tapp_id: None,
            runtime_id: None,
        },
        job,
        now + RETENTION,
    )
    .await?;
    Ok(())
}
pub(in crate::services::agent::merope) async fn forget(
    db: &impl ConnectionTrait,
) -> anyhow::Result<()> {
    for namespace in [NAMESPACE, EVENTS] {
        registry::delete_matching(db, namespace, None, None, None, None).await?;
    }
    Ok(())
}

/// A chat id names one person in one place: their lines fold into one job
/// until it is gone over (into `next` while it is). A line held as it lands
/// waits a while for her reply; the reply makes it ready. Stranger ids name
/// a batch; each delivered message has its own receipt so replay cannot
/// inflate the exchange count.
pub(in crate::services::agent::merope) async fn enqueue(
    db: &DatabaseConnection,
    id: &str,
    owner: i32,
    data: Payload,
    event: Option<&str>,
) -> anyhow::Result<()> {
    let tx = db.begin().await?;
    lock(&tx).await?;
    let now = now(&tx).await?;
    let Some(epoch) = epoch(&tx, owner, false).await? else {
        return Ok(());
    };
    let persona = super::get_persona_on(&tx).await?.map(|row| row.updated_at);
    if let Some(event) = event {
        if registry::get::<bool>(&tx, EVENTS, event).await?.is_some() {
            return Ok(());
        }
    }
    let mut existing = registry::get::<Job>(&tx, NAMESPACE, id).await?;
    if existing
        .as_ref()
        .is_some_and(|job| job.persona != persona || job.epoch != epoch || job.owner != owner)
    {
        existing = None;
    }
    if existing.as_ref().is_none_or(|job| job.data.is_none()) {
        let count: i64 = tx.query_one_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
            "SELECT COUNT(*) AS count FROM runtime_registry WHERE namespace = $1 AND expires_at > $2 AND payload->'data' <> 'null'::jsonb",
            [NAMESPACE.into(), now.into()])).await?.unwrap().try_get("", "count")?;
        anyhow::ensure!(count < CAPACITY, "memory queue full");
    }
    let in_flight = existing
        .as_ref()
        .is_some_and(|job| job.token.is_some() && job.lease > now);
    let mut job = existing
        .filter(|job| job.data.is_some() || job.next.is_some())
        .unwrap_or(Job {
        owner,
        persona,
        epoch,
        data: None,
        ready: now,
        last_arrival: now,
        lease: 0,
        token: None,
        attempts: 0,
        claimed_count: 0,
        outcome: None,
        next: None,
    });
    job.last_arrival = now;
    if let Payload::Chat { .. } = data {
        let held = data.awaits_reply();
        let slot = if in_flight { &mut job.next } else { &mut job.data };
        match slot {
            Some(earlier) => earlier.fold(data),
            None => *slot = Some(data),
        }
        if !in_flight {
            // Held as it lands: wait for her answer, but not forever. Her
            // answer makes it ready; a failed attempt keeps its backoff.
            job.ready = if held {
                job.ready.max(now + HELD_FOR)
            } else if job.attempts == 0 {
                now
            } else {
                job.ready.max(now)
            };
        }
        save(&tx, id, &job, now).await?;
        tx.commit().await?;
        return Ok(());
    }
    match (&mut job.data, data) {
        (
            Some(Payload::Stranger {
                exchanges,
                stranger,
                count,
                venue,
            }),
            Payload::Stranger {
                exchanges: added,
                stranger: latest,
                ..
            },
        ) => {
            anyhow::ensure!(exchanges.len() < 64, "stranger memory batch full");
            exchanges.extend(added);
            *stranger = latest;
            *count = count_exchange(
                &tx,
                &myriad_merope::strangers::talks_key(venue, &stranger.who),
                now,
            )
            .await?;
            // New arrivals must not shorten the backoff of a failed attempt.
            job.ready = job
                .ready
                .max(if exchanges.len() >= 8 { now } else { now + 180 });
            if job.attempts == 0 && exchanges.len() >= 8 {
                job.ready = now;
            }
        }
        (slot, mut data) => {
            if let Payload::Stranger {
                count,
                venue,
                stranger,
                ..
            } = &mut data
            {
                *count = count_exchange(
                    &tx,
                    &myriad_merope::strangers::talks_key(venue, &stranger.who),
                    now,
                )
                .await?;
                job.ready = now + 180;
            }
            *slot = Some(data);
        }
    }
    save(&tx, id, &job, now).await?;
    if let Some(event) = event {
        registry::put(
            &tx,
            EVENTS,
            event,
            registry::RegistryIdentity {
                subject_id: Some(owner),
                owner_id: Some(owner),
                tapp_id: None,
                runtime_id: None,
            },
            &true,
            now + RETENTION,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
async fn count_exchange(db: &impl ConnectionTrait, id: &str, now: i64) -> anyhow::Result<i64> {
    Ok(registry::increment(
        db,
        super::super::strangers::TALKS_NAMESPACE,
        id,
        now + 60 * 86_400,
    )
    .await?)
}

pub(in crate::services::agent::merope) async fn claim(
    db: &DatabaseConnection,
) -> anyhow::Result<Option<Claim>> {
    let tx = db.begin().await?;
    lock(&tx).await?;
    let now = now(&tx).await?;
    // The shared advisory lock makes claim atomic across processes; expired
    // leases are eligible without a separate recovery pass.
    let rows = tx.query_all_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
        "SELECT record_id, payload FROM runtime_registry WHERE namespace = $1 AND expires_at > $2 AND payload->'data' <> 'null'::jsonb AND (payload->>'ready')::BIGINT <= $2 AND (payload->>'lease')::BIGINT <= $2 ORDER BY updated_at LIMIT 16",
        [NAMESPACE.into(), now.into()])).await?;
    for row in rows {
        let id: String = row.try_get("", "record_id")?;
        let mut job: Job = serde_json::from_value(row.try_get("", "payload")?)?;
        if job.attempts >= MAX_ATTEMPTS {
            // A process may die on its final attempt. Retire just its first
            // batch; exchanges appended meanwhile still deserve a turn.
            let count = job.claimed_count;
            consume(&mut job, count, now, "attempts_exhausted");
            promote_next(&mut job, now);
            save(&tx, &id, &job, now).await?;
            continue;
        }
        job.claimed_count = match &job.data {
            Some(Payload::Stranger { exchanges, .. }) => exchanges.len().min(8),
            _ => 0,
        };
        job.attempts += 1;
        job.lease = now + LEASE;
        job.token = Some(uuid::Uuid::new_v4().to_string());
        save(&tx, &id, &job, now).await?;
        if let Some(Payload::Stranger { exchanges, .. }) = &mut job.data {
            exchanges.truncate(8);
        }
        tx.commit().await?;
        return Ok(Some(Claim { id, job }));
    }
    tx.commit().await?;
    Ok(None)
}

pub(in crate::services::agent::merope) async fn current(
    db: &impl ConnectionTrait,
    job: &Job,
    locked: bool,
) -> anyhow::Result<bool> {
    if locked {
        // Fence direct row updates as well as the coordinated reset path.
        db.query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id FROM agent_persona WHERE id = 'site' FOR SHARE".to_string(),
        ))
        .await?;
    }
    if super::get_persona_on(db).await?.map(|row| row.updated_at) != job.persona
        || epoch(db, job.owner, locked).await? != Some(job.epoch)
    {
        return Ok(false);
    }
    if let Some(Payload::Chat {
        input_at, present, ..
    }) = &job.data
    {
        return Ok(super::chat_memory_input_is_current(db, job.owner, *input_at, present).await?);
    }
    Ok(true)
}
/// Once what was gone over is done, what arrived meanwhile is next.
fn promote_next(job: &mut Job, now: i64) {
    let Some(next) = job.next.take() else {
        return;
    };
    match &mut job.data {
        Some(data) => data.fold(next),
        None => {
            job.ready = if next.awaits_reply() {
                now + HELD_FOR
            } else {
                now
            };
            job.data = Some(next);
        }
    }
}

fn consume(job: &mut Job, count: usize, now: i64, outcome: &str) {
    if let Some(Payload::Stranger { exchanges, .. }) = &mut job.data {
        exchanges.drain(..count.min(exchanges.len()));
        if exchanges.is_empty() {
            job.data = None;
        }
    } else {
        job.data = None;
    }
    job.attempts = 0;
    job.token = None;
    job.lease = 0;
    job.ready = match &job.data {
        Some(Payload::Stranger { exchanges, .. }) if exchanges.len() < 8 => {
            (job.last_arrival + 180).max(now)
        }
        _ => now,
    };
    job.outcome = Some(outcome.into());
}

/// Fence stale workers and atomically commit both the memory and its receipt.
pub(in crate::services::agent::merope) async fn finish(
    db: &DatabaseConnection,
    claim: &Claim,
    result: Result<Effect, Failure>,
) -> anyhow::Result<()> {
    let tx = db.begin().await?;
    lock(&tx).await?;
    let now = now(&tx).await?;
    let Some(mut job) = registry::get::<Job>(&tx, NAMESPACE, &claim.id).await? else {
        return Ok(());
    };
    if job.token != claim.job.token || job.token.is_none() || job.lease <= now {
        return Ok(());
    }
    let result = if current(&tx, &job, true).await? {
        result
    } else {
        Ok(Effect::Stale)
    };
    let outcome;
    match result {
        Err(failure) if failure.retryable() && job.attempts < MAX_ATTEMPTS => {
            outcome = failure.to_string();
            if let (Some(data), Some(next)) = (&mut job.data, job.next.take()) {
                data.fold(next);
            }
            job.ready = now + if job.attempts == 1 { 5 } else { 30 };
            job.lease = 0;
            job.token = None;
            job.outcome = Some(outcome.clone());
        }
        result => {
            outcome = match result {
                Ok(Effect::Chat(update)) => {
                    let Some(Payload::Chat {
                        input_at, present, ..
                    }) = &job.data
                    else {
                        anyhow::bail!("wrong memory job kind");
                    };
                    if super::apply_chat_memory_updates_on(
                        &tx, job.owner, *input_at, &update, present,
                    )
                    .await?
                    {
                        "applied".into()
                    } else {
                        "stale_or_duplicate".into()
                    }
                }
                Ok(Effect::Stranger { previous, note }) => {
                    let Some(Payload::Stranger {
                        venue, stranger, ..
                    }) = &claim.job.data
                    else {
                        anyhow::bail!("wrong memory job kind");
                    };
                    super::super::strangers::apply_note(
                        &tx,
                        venue,
                        stranger,
                        previous.as_deref(),
                        note.as_deref(),
                    )
                    .await?;
                    "applied".into()
                }
                Ok(Effect::Stale) => {
                    job.data = None;
                    "stale".into()
                }
                Ok(Effect::NoChange) => "no_change".into(),
                Err(failure) => failure.to_string(),
            };
            let count = match &claim.job.data {
                Some(Payload::Stranger { exchanges, .. }) => exchanges.len(),
                _ => 0,
            };
            consume(&mut job, count, now, &outcome);
            promote_next(&mut job, now);
        }
    }
    save(&tx, &claim.id, &job, now).await?;
    tx.commit().await?;
    tracing::info!(job = claim.id, attempt = claim.job.attempts, %outcome, "[Merope] memory job");
    Ok(())
}

#[cfg(test)]
#[path = "memory_jobs_tests.rs"]
mod tests;
