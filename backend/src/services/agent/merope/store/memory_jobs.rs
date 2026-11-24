//! Durable memory work. Short transactions serialize admission, leases and
//! persona replacement; no transaction or lock is held across model I/O.
use super::super::memory_jobs::{Effect, Failure, Payload};
use crate::services::runtime_registry as registry;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};

pub(in crate::services::agent::merope) const NAMESPACE: &str = "merope_memory_jobs";
/// What is judged by what they asked her not to keep, apart from the rest:
/// a build that knows nothing of it never reads it, and takes the rest up as
/// it did.
pub(in crate::services::agent::merope) const ASKED_NAMESPACE: &str = "merope_asked_jobs";
/// Rows of the queue that could not be read since the site started, each
/// passed over: the rest go on.
static PASSED_OVER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// [`PASSED_OVER`] so far.
pub(crate) fn passed_over() -> u64 {
    PASSED_OVER.load(std::sync::atomic::Ordering::Relaxed)
}

/// Counted and told of once a row, however often the queue is looked at.
fn pass_over(id: &str) {
    static SEEN: std::sync::Mutex<std::collections::BTreeSet<String>> =
        std::sync::Mutex::new(std::collections::BTreeSet::new());
    let mut seen = SEEN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !seen.insert(id.to_string()) {
        return;
    }
    let so_far = PASSED_OVER.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    tracing::warn!(job = id, so_far, "[Merope] a memory job could not be read");
}
const EVENTS: &str = "merope_memory_events";
const RETENTION: i64 = 86_400;
const CAPACITY: i64 = 2048;
pub(in crate::services::agent::merope) const MAX_ATTEMPTS: u8 = 3;
/// How long a job that failed on its `attempts`th try waits for the next, in
/// seconds: a moment after the first, longer after that, so a connection
/// that dropped for half a minute is waited out, not tried into.
pub(in crate::services::agent::merope) fn again_in(attempts: u8) -> i64 {
    if attempts == 1 { 5 } else { 30 }
}
/// How long a claim is held: longer than any job is let run
/// (`memory_jobs::tick`), so an answer that comes in time is never let go
/// for coming late. A process that dies holding one, its job waits this long.
const LEASE: i64 = 120;
const _: () = assert!(LEASE as u64 > super::super::memory_jobs::SETTLE_LIMIT.as_secs() + 5);
/// A job tried until it is done ([`Job::until_done`]), past the tries the
/// others have: this often.
const UNTIL_DONE_EVERY: i64 = 600;
/// One read again each time it is tried, not told again: this often.
const READ_AGAIN_EVERY: i64 = 3_600;
/// How long words held as they land wait for her answer before they are gone
/// over without it.
const HELD_FOR: i64 = 120;
/// Quiet this long, and a stretch of talk is over and gone over.
const SETTLES_AFTER: i64 = myriad_merope::settle_raw::QUIET_SECS as i64;
/// Talk that never goes quiet is still gone over this long after it began.
const SETTLES_AT_MOST: i64 = myriad_merope::settle_raw::AT_MOST_SECS as i64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in crate::services::agent::merope) struct Job {
    pub owner: i32,
    /// When her persona was last written as it was taken up; only what
    /// fences a job kept before [`Job::her`] was.
    pub persona: Option<chrono::DateTime<chrono::FixedOffset>>,
    /// Which her it was taken up for. Editing her (name, personality, looks,
    /// a portrait being drawn) leaves her the same her: only another birth
    /// lets the job go.
    #[serde(default)]
    pub her: Option<Her>,
    pub epoch: i64,
    pub data: Option<Payload>,
    pub ready: i64,
    pub last_arrival: i64,
    pub lease: i64,
    pub token: Option<String>,
    pub attempts: u8,
    /// How often it was asked of what they asked not to keep, to no answer
    /// that will do (`Failure::untold`), since when, and when last.
    #[serde(default)]
    pub untold: u8,
    #[serde(default)]
    pub untold_since: Option<i64>,
    #[serde(default)]
    pub untold_at: Option<i64>,
    pub claimed_count: usize,
    pub outcome: Option<String>,
    /// What the same person said while this was being gone over: it is gone
    /// over next, never dropped and never written ahead of what came before.
    #[serde(default)]
    pub next: Option<Payload>,
    /// Where a stretch let go unread began: the next stretch here is gone
    /// over from there, so its notes are brought along (see [`settling`]).
    #[serde(default)]
    pub unread_from: Option<chrono::DateTime<chrono::FixedOffset>>,
    /// How many stretches have brought what is left unread along already.
    #[serde(default)]
    pub unread_times: u8,
    /// What the stretch now being gone over brought along.
    #[serde(default)]
    pub brought: Option<settling::Brought>,
    /// A stretch gone over in passes (more notes than are gone over at
    /// once): where its lines begin, shown with each pass. Its `from` is
    /// where the notes not gone over yet begin.
    #[serde(default)]
    pub said_from: Option<chrono::DateTime<chrono::FixedOffset>>,
    /// A private talk: the lines gone over lately, kept from one stretch to
    /// the next (`chat_remember::settle_raw::Read`).
    #[serde(default)]
    pub read: Option<super::super::chat_remember::settle_raw::Read>,
}
impl Job {
    /// Tried until it is done, and kept until then.
    fn until_done(&self) -> bool {
        self.data.as_ref().is_some_and(Payload::by_what_they_asked)
    }

    /// Whether what it is of is not asked about any more, should it get no
    /// answer that will do again: asked so often, or so long, to none, while
    /// other judgments got theirs.
    fn gives_up(&self, now: i64) -> bool {
        use super::super::memory_jobs::{GIVES_UP_AFTER, GIVES_UP_AFTER_SECS};
        self.untold_since(now).is_some_and(|since| {
            (self.untold >= GIVES_UP_AFTER || now - since >= GIVES_UP_AFTER_SECS)
                && super::super::call::judged_since(since)
        })
    }

    /// Since when it has been asked to no answer that will do; not over a
    /// day with no such try in it (away, or the account refused).
    fn untold_since(&self, now: i64) -> Option<i64> {
        self.untold_since
            .filter(|_| self.untold_at.is_some_and(|last| now - last <= RETENTION))
    }

    /// Asked again, to no answer that will do.
    fn untold(&mut self, failure: &Failure, now: i64) {
        if self.untold_since(now).is_none() {
            self.untold = 0;
            self.untold_since = Some(now);
        }
        self.untold_at = Some(now);
        // Refused, it is only a while till it is given up on.
        self.untold = self.untold.saturating_add(u8::from(!failure.refused()));
    }

    fn told(&mut self) {
        self.untold = 0;
        self.untold_since = None;
        self.untold_at = None;
    }

    /// Not told by what they asked, when last tried: what comes meanwhile
    /// must not shorten the wait.
    fn not_told(&self) -> bool {
        (self.outcome.as_deref()).is_some_and(|outcome| outcome.starts_with("not_told_"))
    }

    /// Where it waits.
    fn namespace(&self) -> &'static str {
        if self.until_done() {
            ASKED_NAMESPACE
        } else {
            NAMESPACE
        }
    }
}

/// Which her a job was taken up for: the site's copy's lineage
/// (`persona_sync::lease::lineage`), none while there is no copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::services::agent::merope) struct Her {
    lineage: Option<String>,
}

impl Her {
    async fn now(db: &impl ConnectionTrait) -> anyhow::Result<Self> {
        Ok(Self {
            lineage: crate::services::persona_sync::lease::lineage(db).await?,
        })
    }
}

/// Whether `job` was taken up for the her there is now. Taken up before
/// there was a copy of her (or before jobs noted which her), it goes by
/// when her persona was last written.
fn same_her(job: &Job, her: &Her, persona: Option<chrono::DateTime<chrono::FixedOffset>>) -> bool {
    match &job.her {
        Some(Her {
            lineage: Some(taken),
        }) => her.lineage.as_ref() == Some(taken),
        _ => job.persona == persona,
    }
}

#[derive(Debug, Clone)]
pub(in crate::services::agent::merope) struct Claim {
    pub id: String,
    pub job: Job,
    /// As it was taken up ([`Job::gives_up`]): the same to its end.
    pub gives_up: bool,
}

/// Also taken first by persona replacement/deletion, and by forgetting a
/// memory, on their transaction.
pub(in crate::services::agent) async fn lock(db: &impl ConnectionTrait) -> anyhow::Result<()> {
    db.execute_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(1296388170, 1)".to_string(),
    ))
    .await?;
    Ok(())
}
/// Now in seconds, by the site's clock (it was the database's).
async fn now(_db: &impl ConnectionTrait) -> anyhow::Result<i64> {
    Ok(crate::clock::now().timestamp())
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
        job.namespace(),
        id,
        registry::RegistryIdentity {
            subject_id: Some(job.owner),
            owner_id: Some(job.owner),
            tapp_id: None,
            runtime_id: None,
        },
        job,
        match &job.data {
            // As long as a group's lines are at hand.
            Some(Payload::GroupHeard { read, .. }) => {
                (read.at + super::super::memory_jobs::HEARD_FOR).timestamp()
            }
            // Its lines wait for it.
            _ if job.until_done() => now + 100 * 365 * RETENTION,
            _ if job.unread_from.is_some() => now + settling::WAITS,
            _ => now + RETENTION,
        },
    )
    .await?;
    Ok(())
}
pub(in crate::services::agent::merope) async fn forget(
    db: &impl ConnectionTrait,
) -> anyhow::Result<()> {
    // Not what is still to be held to what they asked her not to keep
    // ([`Payload::only_holds_to_it`]).
    registry::delete_matching(db, EVENTS, None, None, None, None).await?;
    registry::delete_matching(db, NAMESPACE, None, None, None, None).await?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM runtime_registry WHERE namespace = $1 \
           AND (payload->'data'->'LeftUnread' IS NOT NULL \
             OR payload->'data'->'GroupHeard' IS NOT NULL)",
        [ASKED_NAMESPACE.into()],
    ))
    .await?;
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
    enqueue_in(&tx, id, owner, data, event).await?;
    tx.commit().await?;
    Ok(())
}

/// [`enqueue`] on a transaction that already holds [`lock`]: written with
/// it, or not at all.
pub(super) async fn enqueue_in(
    tx: &sea_orm::DatabaseTransaction,
    id: &str,
    owner: i32,
    data: Payload,
    event: Option<&str>,
) -> anyhow::Result<()> {
    let now = now(tx).await?;
    let Some(epoch) = epoch(tx, owner, false).await? else {
        return Ok(());
    };
    let persona = super::get_persona_on(tx).await?.map(|row| row.updated_at);
    let her = Her::now(tx).await?;
    if let Some(event) = event {
        if registry::get::<bool>(tx, EVENTS, event).await?.is_some() {
            return Ok(());
        }
    }
    let by_asked = data.by_what_they_asked();
    let space = if by_asked { ASKED_NAMESPACE } else { NAMESPACE };
    let mut existing = registry::get::<Job>(tx, space, id).await?;
    if existing.as_ref().is_some_and(|job| {
        !same_her(job, &her, persona) || job.epoch != epoch || job.owner != owner
    }) {
        existing = None;
    }
    // What they asked holds however much waits.
    if !by_asked && existing.as_ref().is_none_or(|job| job.data.is_none()) {
        let count: i64 = tx.query_one_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
            "SELECT COUNT(*) AS count FROM runtime_registry WHERE namespace = $1 AND expires_at > $2 AND payload->'data' <> 'null'::jsonb",
            [NAMESPACE.into(), now.into()])).await?.unwrap().try_get("", "count")?;
        anyhow::ensure!(count < CAPACITY, "memory queue full");
    }
    let in_flight = existing
        .as_ref()
        .is_some_and(|job| job.token.is_some() && job.lease > now);
    let unread = existing
        .as_ref()
        .and_then(|job| Some((job.unread_from?, job.unread_times)));
    let read = existing.as_ref().and_then(|job| job.read.clone());
    let mut job = existing
        .filter(|job| job.data.is_some() || job.next.is_some())
        .unwrap_or(Job {
            owner,
            persona,
            her: None,
            epoch,
            data: None,
            ready: now,
            last_arrival: now,
            lease: 0,
            token: None,
            attempts: 0,
            untold: 0,
            untold_since: None,
            untold_at: None,
            claimed_count: 0,
            outcome: None,
            next: None,
            unread_from: None,
            unread_times: 0,
            brought: None,
            said_from: None,
            read: None,
        });
    if job.read.is_none() {
        job.read = read;
    }
    // The same her: noted as she is now, should she have been born since.
    // Not while there is no persona: a copy may be on its way in then.
    if persona.is_some() {
        job.her = Some(her);
    }
    if let (None, Some((from, times))) = (job.unread_from, unread) {
        job.unread_from = Some(from);
        job.unread_times = times;
    }
    job.last_arrival = now;
    if let Payload::Settle { from, .. } = &data {
        // Each line puts it off until they go quiet, but never past a while
        // after the stretch began.
        let began = match &job.data {
            Some(Payload::Settle { from, .. }) if !in_flight => from.timestamp(),
            _ => from.timestamp(),
        };
        let slot = if in_flight {
            &mut job.next
        } else {
            &mut job.data
        };
        match slot {
            Some(earlier) => earlier.fold(data),
            None => *slot = Some(data),
        }
        if !in_flight {
            let due = settle_due(job.data.as_ref(), now, began);
            job.ready = if job.attempts == 0 && !job.not_told() {
                due
            } else {
                job.ready.max(due)
            };
        }
        save(tx, id, &job, now).await?;
        return Ok(());
    }
    if let Payload::Chat { .. } = data {
        let held = data.awaits_reply();
        let slot = if in_flight {
            &mut job.next
        } else {
            &mut job.data
        };
        match slot {
            Some(earlier) => earlier.fold(data),
            None => *slot = Some(data),
        }
        if !in_flight {
            // Held as it lands: wait for her answer, but not forever. Her
            // answer makes it ready; a failed attempt keeps its backoff.
            job.ready = if held {
                job.ready.max(now + HELD_FOR)
            } else if job.attempts == 0 && !job.not_told() {
                now
            } else {
                job.ready.max(now)
            };
        }
        save(tx, id, &job, now).await?;
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
                tx,
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
                    tx,
                    &myriad_merope::strangers::talks_key(venue, &stranger.who),
                    now,
                )
                .await?;
                job.ready = now + 180;
            }
            *slot = Some(data);
        }
    }
    save(tx, id, &job, now).await?;
    if let Some(event) = event {
        registry::put(
            tx,
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
    // What stretches out of tries kept as they were, put on her nodes once
    // this is written.
    let mut kept = Vec::new();
    // The shared advisory lock makes claim atomic across processes; expired
    // leases are eligible without a separate recovery pass.
    let mut unread: Vec<String> = Vec::new();
    while let Some(row) = tx.query_one_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
        "SELECT record_id, payload FROM runtime_registry WHERE namespace = ANY($1) AND expires_at > $2 AND payload->'data' <> 'null'::jsonb AND (payload->>'ready')::BIGINT <= $2 AND (payload->>'lease')::BIGINT <= $2 AND record_id <> ALL($3) ORDER BY updated_at LIMIT 1",
        [vec![NAMESPACE.to_string(), ASKED_NAMESPACE.to_string()].into(), now.into(), unread.clone().into()])).await? {
        let id: String = row.try_get("", "record_id")?;
        let Ok(mut job) = serde_json::from_value::<Job>(row.try_get("", "payload")?) else {
            // Another build's, or broken: left as it is, the rest go on.
            pass_over(&id);
            unread.push(id);
            continue;
        };
        if job.attempts >= MAX_ATTEMPTS && !job.until_done() {
            // A process may die on its final attempt. Retire just its first
            // batch; exchanges appended meanwhile still deserve a turn.
            let count = job.claimed_count;
            settling::over(&job);
            // Only while what it was taken up under still holds.
            if matches!(job.data, Some(Payload::Settle { .. })) && current(&tx, &job, true).await? {
                kept.extend(settling::leave_unread(&tx, &mut job).await?);
            }
            judge_lines(&tx, &job).await?;
            consume(&mut job, count, now, "attempts_exhausted");
            job.brought = None;
            promote_next(&mut job, now);
            save(&tx, &id, &job, now).await?;
            continue;
        }
        job.claimed_count = match &job.data {
            Some(Payload::Stranger { exchanges, .. }) => exchanges.len().min(8),
            _ => 0,
        };
        job.attempts = job.attempts.saturating_add(1);
        job.lease = now + LEASE;
        job.token = Some(uuid::Uuid::new_v4().to_string());
        settling::bring_along(&mut job);
        save(&tx, &id, &job, now).await?;
        if let Some(Payload::Stranger { exchanges, .. }) = &mut job.data {
            exchanges.truncate(8);
        }
        tx.commit().await?;
        on_nodes_meanwhile(db, kept);
        let gives_up = job.gives_up(now);
        return Ok(Some(Claim { id, job, gives_up }));
    }
    tx.commit().await?;
    on_nodes_meanwhile(db, kept);
    Ok(None)
}

/// What was kept as it was, put on her nodes while the job just claimed is
/// done: sorting it takes as long as a model's answer, and the lease runs.
/// On her managed runner: as the process stops it is let finish within the
/// drain, then cut off. What it did not put on a node then, or when the
/// runner had no room for it, stays on none: her upkeep sorts what is on no
/// node onto one later (`memory_nodes::sorting`), but what those facts do
/// to how their life stands (「现在」) is not written after the fact.
fn on_nodes_meanwhile(db: &DatabaseConnection, kept: Vec<settling::Kept>) {
    if kept.is_empty() {
        return;
    }
    let db = db.clone();
    let facts: usize = kept.iter().map(|kept| kept.facts.len()).sum();
    if !super::super::background::spawn("kept as they were, onto her nodes", async move {
        for kept in kept {
            kept.on_nodes(&db).await;
        }
    }) {
        tracing::info!(
            facts,
            "[memory-jobs] kept as they were, not put on her nodes now: left to her upkeep"
        );
    }
}

pub(in crate::services::agent::merope) async fn current(
    db: &impl ConnectionTrait,
    job: &Job,
    locked: bool,
) -> anyhow::Result<bool> {
    if (job.data.as_ref()).is_some_and(Payload::only_holds_to_it) {
        return Ok(true);
    }
    if locked {
        // Fence direct row updates as well as the coordinated reset path.
        db.query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id FROM agent_persona WHERE id = 'site' FOR SHARE".to_string(),
        ))
        .await?;
    }
    let persona = super::get_persona_on(db).await?.map(|row| row.updated_at);
    if !same_her(job, &Her::now(db).await?, persona)
        || epoch(db, job.owner, locked).await? != Some(job.epoch)
    {
        return Ok(false);
    }
    // Lines of a stretch that could not be read are kept for good, as a
    // stretch is: not fenced by lines gone over since.
    if let Some(Payload::Chat {
        input_at,
        present,
        for_good: false,
        ..
    }) = &job.data
    {
        return Ok(super::chat_memory_input_is_current(db, job.owner, *input_at, present).await?);
    }
    Ok(true)
}
/// When a stretch that `began` then is gone over: once they go quiet, but
/// never past a while after it began; in a private talk at once when its
/// box is full (`myriad_merope::settle_raw::box_is_full`).
fn settle_due(data: Option<&Payload>, now: i64, began: i64) -> i64 {
    let full = matches!(data, Some(Payload::Settle { present, .. }) if !present.is_group())
        && data.is_some_and(|data| myriad_merope::settle_raw::box_is_full(data.waiting()));
    if full {
        return now;
    }
    (now + SETTLES_AFTER).min(began + SETTLES_AT_MOST).max(now)
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
            } else if let Payload::Settle { from, .. } = &next {
                settle_due(Some(&next), now, from.timestamp())
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
    job.said_from = None;
    job.attempts = 0;
    job.told();
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

#[path = "memory_jobs_finish.rs"]
mod finishing;
pub(in crate::services::agent::merope) use finishing::finish;

#[path = "memory_jobs_forgetting.rs"]
mod forgetting;
pub(in crate::services::agent) use forgetting::forget_holding;

#[path = "memory_jobs_asked.rs"]
mod asked;
pub(crate) use asked::Own;
use asked::{judge_lines, judge_old};

#[path = "memory_jobs_hurry.rs"]
mod hurrying;
pub(in crate::services::agent::merope) use hurrying::{WaitingFor, hurry, waiting_for};

#[path = "memory_jobs_settling.rs"]
pub(in crate::services::agent) mod settling;

#[cfg(test)]
#[path = "memory_jobs_tests.rs"]
mod tests;
