//! What placing a stretch could not place, placed later: kept by when it
//! was said (never the words), one span of time per account, and gone over
//! at her hourly upkeep a few batches at a time, as placing a stretch goes
//! ([`super::place_in`]: a line already in a span or the inbox is not asked
//! of again, and a span's row is named by its node and first line, so a
//! line placed twice is written once).
//!
//! Kept when placing's answer failed, was cut off or could not be read;
//! when going over a stretch was shown only its latest lines; and when
//! placing was shown only the earliest of them (one call takes so many,
//! the core's `PLACE_ITEMS`). Failing twice
//! in a row, it waits a day; still not placed a week after it was first
//! kept, it is let go, counted and told in one line (the span of time and
//! how many lines, never what was said).

use chrono::{DateTime, FixedOffset};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use super::{Line, audience, place_in};
use crate::services::runtime_registry as registry;

/// Where what is still to be placed is kept, by account.
pub(crate) const NAMESPACE: &str = "merope_memory_node_unplaced";
/// Lines placed in one call, oldest first: as many as placing is shown.
const AT_ONCE: i64 = myriad_nodes::limits::PLACE_ITEMS as i64;
/// Calls an account's upkeep places in, at most: a call is given 10
/// seconds, so 40 an account at worst; 160 lines an hour, more than a day's
/// talk leaves behind (a stretch's latest past 40, or its earliest past
/// 80), so what is behind is caught up within the hour.
const BATCHES: usize = 4;
/// Failing this many times in a row, it waits ...
const WAITS_AFTER: u32 = 2;
/// ... this long.
const WAIT: i64 = 86_400;
/// Still not placed this long after it was first kept, it is let go.
const KEPT_FOR: i64 = 7 * 86_400;

/// When the lines still to be placed were said, Unix microseconds, from
/// the first to the last; since when (Unix seconds), and how its placing
/// went since.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Unplaced {
    from: i64,
    until: i64,
    since: i64,
    failures: u32,
    at: i64,
}

/// How what could not be placed went since the site started, for her
/// status: spans of time kept to place later, batches placed later,
/// batches whose placing failed again (its answer failed, was cut off or
/// could not be read), times placing later stopped on an error (asked
/// again the next hour, not counted toward waiting a day), and lines let go
/// unplaced after a week. Counts only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Failures {
    pub(crate) kept: u64,
    pub(crate) placed_later: u64,
    pub(crate) failed: u64,
    pub(crate) errors: u64,
    pub(crate) let_go: u64,
}

static KEPT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PLACED_LATER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static FAILED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static ERRORS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LET_GO: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// [`Failures`] so far.
pub(crate) fn failures() -> Failures {
    use std::sync::atomic::Ordering::Relaxed;
    Failures {
        kept: KEPT.load(Relaxed),
        placed_later: PLACED_LATER.load(Relaxed),
        failed: FAILED.load(Relaxed),
        errors: ERRORS.load(Relaxed),
        let_go: LET_GO.load(Relaxed),
    }
}

fn counted(counter: &std::sync::atomic::AtomicU64, by: u64) {
    counter.fetch_add(by, std::sync::atomic::Ordering::Relaxed);
}

fn identity(user_id: i32) -> registry::RegistryIdentity<'static> {
    registry::RegistryIdentity {
        subject_id: Some(user_id),
        owner_id: Some(user_id),
        tapp_id: None,
        runtime_id: None,
    }
}

fn now() -> i64 {
    crate::clock::now().timestamp()
}

async fn put(db: &DatabaseConnection, user_id: i32, unplaced: &Unplaced) {
    if let Err(error) = registry::put(
        db,
        NAMESPACE,
        &user_id.to_string(),
        identity(user_id),
        unplaced,
        unplaced.since + KEPT_FOR + WAIT,
    )
    .await
    {
        tracing::warn!(%error, "[memory-nodes] what could not be placed was not kept");
    }
}

async fn let_go(db: &DatabaseConnection, user_id: i32) {
    if let Err(error) = registry::delete(db, NAMESPACE, &user_id.to_string()).await {
        tracing::warn!(%error, "[memory-nodes] what was placed later not let go");
    }
}

/// Keeps that `user_id`'s private talk said from `from` until `until` is
/// still to be placed, with whatever was kept before.
pub(crate) async fn keep(
    db: &DatabaseConnection,
    user_id: i32,
    from: DateTime<FixedOffset>,
    until: DateTime<FixedOffset>,
) {
    let (from, until) = (from.timestamp_micros(), until.timestamp_micros());
    let unplaced = match read(db, user_id).await {
        Some(kept) => Unplaced {
            from: kept.from.min(from),
            until: kept.until.max(until),
            ..kept
        },
        None => Unplaced {
            from,
            until,
            since: now(),
            failures: 0,
            at: 0,
        },
    };
    put(db, user_id, &unplaced).await;
    counted(&KEPT, 1);
    tracing::info!(
        from = %micros(unplaced.from),
        until = %micros(unplaced.until),
        "[memory-nodes] lines not placed: placed later"
    );
}

fn micros(at: i64) -> String {
    DateTime::from_timestamp_micros(at)
        .unwrap_or_default()
        .to_rfc3339()
}

/// `user_id`'s private talk said from `from` until `until` (Unix
/// microseconds), oldest first, at most `limit`.
async fn lines(
    db: &DatabaseConnection,
    user_id: i32,
    from: i64,
    until: i64,
    limit: Option<i64>,
) -> Result<Vec<Line>, sea_orm::DbErr> {
    let at = |micros: i64| {
        DateTime::from_timestamp_micros(micros)
            .unwrap_or_default()
            .fixed_offset()
    };
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id, m.role, m.content, m.created_at FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' \
               AND (s.context->>'venue') IS NULL \
               AND m.role IN ('user', 'assistant') AND (m.metadata->>'piece') IS NULL \
               AND m.created_at >= $2 AND m.created_at <= $3 \
             ORDER BY m.created_at, m.id LIMIT $4",
            [
                user_id.into(),
                at(from).into(),
                at(until).into(),
                limit.unwrap_or(i64::MAX).into(),
            ],
        ))
        .await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            Some(Line {
                id: row.try_get("", "id").ok()?,
                theirs: row.try_get::<String>("", "role").ok()? == "user",
                text: row.try_get("", "content").ok()?,
                at: row.try_get("", "created_at").ok()?,
            })
        })
        .collect())
}

/// What is still to be placed, a batch an account, at her upkeep: the site
/// owner's however it goes, every other account's within its day's calls
/// of upkeep.
pub(crate) async fn place_later(db: &DatabaseConnection) {
    let kept = match registry::list(db, NAMESPACE, None, None).await {
        Ok(kept) => kept,
        Err(error) => {
            tracing::warn!(%error, "[memory-nodes] what is still to be placed could not be read");
            return;
        }
    };
    let site_owner = crate::services::principal::site_owner_id(db)
        .await
        .ok()
        .flatten();
    for row in kept {
        let Ok(user_id) = row.record_id.parse::<i32>() else {
            continue;
        };
        let Ok(unplaced) = serde_json::from_value::<Unplaced>(row.payload.clone()) else {
            let_go(db, user_id).await;
            continue;
        };
        if let Err(error) = place_one(db, user_id, site_owner, unplaced).await {
            counted(&ERRORS, 1);
            tracing::warn!(%error, "[memory-nodes] what is still to be placed not placed");
        }
    }
}

async fn place_one(
    db: &DatabaseConnection,
    user_id: i32,
    site_owner: Option<i32>,
    unplaced: Unplaced,
) -> Result<(), super::Error> {
    let audience = audience::private_talk(user_id, site_owner);
    let now = now();
    if now - unplaced.since > KEPT_FOR {
        let said = lines(db, user_id, unplaced.from, unplaced.until, None).await?;
        let refs: Vec<String> = said.iter().map(Line::reference).collect();
        let placed = super::already(db, user_id, &audience, &refs).await?;
        let left = refs
            .iter()
            .filter(|line| !placed.contains_key(*line))
            .count();
        counted(&LET_GO, left as u64);
        tracing::info!(
            from = %micros(unplaced.from),
            until = %micros(unplaced.until),
            lines = left,
            "[memory-nodes] lines never placed in a week: let go"
        );
        let_go(db, user_id).await;
        return Ok(());
    }
    if unplaced.failures >= WAITS_AFTER && now < unplaced.at + WAIT {
        tracing::info!(
            from = %micros(unplaced.from),
            until = %micros(unplaced.until),
            failures = unplaced.failures,
            again_at = %DateTime::from_timestamp(unplaced.at + WAIT, 0).unwrap_or_default().to_rfc3339(),
            "[memory-nodes] lines still to be placed wait: placing failed before"
        );
        return Ok(());
    }
    let owners = audience == audience::owners_talk();
    let mut start = unplaced;
    for _ in 0..BATCHES {
        match place_batch(db, user_id, &audience, owners, start, now).await? {
            Some(rest) => start = rest,
            None => break,
        }
    }
    Ok(())
}

/// Places the earliest batch of what is kept (`start`), and keeps the rest:
/// what is still to be placed after it, if any and placing went on.
async fn place_batch(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    owners: bool,
    start: Unplaced,
    now: i64,
) -> Result<Option<Unplaced>, super::Error> {
    // Each batch: her mind gone elsewhere meanwhile, placing would ask
    // nothing and this batch would pass for placed.
    if crate::services::her_log::pen::think().is_none() {
        return Ok(None);
    }
    if !owners
        && super::super::upkeep::calls_today(db, user_id, audience).await?
            >= super::super::upkeep::PER_DAY
    {
        return Ok(None);
    }
    let batch = lines(db, user_id, start.from, start.until, Some(AT_ONCE)).await?;
    let Some(last) = batch.last().map(|line| line.at.timestamp_micros()) else {
        if read(db, user_id).await.as_ref() == Some(&start) {
            let_go(db, user_id).await;
        }
        return Ok(None);
    };
    // Placed already, every one: nothing to ask, and no call of her day.
    let refs: Vec<String> = batch.iter().map(Line::reference).collect();
    let placed = super::already(db, user_id, audience, &refs).await?;
    let asked = refs.iter().any(|line| !placed.contains_key(line));
    let placed = if asked {
        let placed = place_in(db, user_id, audience, &batch).await?;
        if !owners {
            super::super::upkeep::count_calls(db, user_id, audience, 1).await?;
        }
        placed
    } else {
        super::Placed::default()
    };
    // Read again before it is written: what was kept while this was asked
    // is kept with it, not written over.
    let current = read(db, user_id).await;
    if placed.missed.is_some() {
        let current = current.unwrap_or_else(|| start.clone());
        let failed = Unplaced {
            failures: start.failures + 1,
            at: now,
            ..current
        };
        put(db, user_id, &failed).await;
        counted(&FAILED, 1);
        return Ok(None);
    }
    if asked {
        counted(&PLACED_LATER, 1);
    }
    // Not all of the batch shown: on from the first not shown.
    let next = placed
        .left
        .map_or(last, |(first, _)| first.timestamp_micros());
    let done = placed.left.is_none() && ((batch.len() as i64) < AT_ONCE || last >= start.until);
    match current {
        // Let go meanwhile: nothing left to keep.
        None => Ok(None),
        Some(current) if current == start && done => {
            let_go(db, user_id).await;
            Ok(None)
        }
        Some(current) => {
            // From the last one's time again: one said at the same moment
            // past the batch is not passed by (those placed are not asked
            // of). Kept earlier meanwhile, from there.
            let from = if current.from < start.from {
                current.from
            } else {
                next
            };
            let rest = Unplaced {
                from,
                failures: 0,
                ..current
            };
            put(db, user_id, &rest).await;
            Ok(Some(rest))
        }
    }
}

/// What is kept still to be placed for `user_id`, if any (none when it
/// cannot be read).
async fn read(db: &DatabaseConnection, user_id: i32) -> Option<Unplaced> {
    registry::get(db, NAMESPACE, &user_id.to_string())
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "[memory-nodes] what was still to be placed could not be read");
            None
        })
}

#[cfg(test)]
#[path = "placing_backlog_tests.rs"]
mod tests;
