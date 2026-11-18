//! A second look before what they took back is forgotten on her nodes.
//!
//! Going over a stretch, or a line that only lets go, may judge that they
//! took something back. The memory is let go at once and what rests on it
//! is hidden where she reads; both can come back. Forgetting what rests on
//! it on her nodes cannot: it waits here (`agent_takeback_check`) until the
//! core's short question (`myriad_merope::take_back_check`), asked with only
//! their line and what she kept, says they took all of it back. Anything
//! else, or no answer after [`TRIES`], only lets it go. The sweep forgets
//! only what was confirmed here ([`super::known::Named::forgets`]); what was
//! taken back before this, with no row here, stays only hidden.
//!
//! Their line is kept as its message's id, never its words; logs carry
//! counts and kinds only.

use std::time::Duration;

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};

use super::Error;
use crate::services::agent::memory::unified::Audience;

/// Waiting for the second look.
pub(crate) const PENDING: &str = "pending";
/// They took it back: what rests on it is forgotten.
pub(crate) const CONFIRMED: &str = "confirmed";
/// A change, a part taken back, or neither: only let go.
pub(crate) const CHANGED: &str = "changed";
/// No line to look at, no answer, or not let go as taken back any more
/// when it was confirmed: only let go.
pub(crate) const UNSURE: &str = "unsure";
/// Asked this many times with no answer, it is left unsure.
const TRIES: i16 = 3;
/// At most this many are asked for one person at a time, each claimed just
/// before it is asked.
const AT_ONCE: usize = 8;
/// One being asked is asked again by someone else only after this long.
const ASKED_AGAIN_AFTER_SECS: i64 = 120;
const TIMEOUT: Duration = Duration::from_secs(30);
/// No new one is asked for one person after this long, so a slow answer
/// does not hold up the memory job that asks.
const ONE_PERSON_FOR: Duration = Duration::from_secs(45);
/// Nor at her upkeep, over everyone, after this long.
const EVERYONE_FOR: Duration = Duration::from_secs(120);
/// How far around when a line was held its message is looked for.
const AROUND_MINUTES: i64 = 10;

/// The memories `memories` of `user_id`, just let go as taken back in their
/// message `said` (none when it is not known), wait for the second look;
/// with no message to look at, they stay only let go. A row left from an
/// earlier taking back of the same memory (it came back since) starts over:
/// what was confirmed then confirms nothing now. What rests on them is
/// hidden on her nodes at once ([`super::hidden`]). In the transaction that
/// lets them go.
pub(crate) async fn hold<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    user_id: i32,
    memories: &[String],
    said: Option<i32>,
) -> Result<(), Error> {
    if memories.is_empty() {
        return Ok(());
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_takeback_check (user_id, memory_id, said_ref, state, tries, at) \
         SELECT $1, m.id, $3, $4, 0, now() FROM agent_memories m \
         WHERE m.id = ANY($2) AND m.user_id = $1 \
         ON CONFLICT (user_id, memory_id) DO UPDATE SET said_ref = EXCLUDED.said_ref, \
           state = EXCLUDED.state, tries = 0, at = EXCLUDED.at, asked_at = NULL",
        [
            user_id.into(),
            memories.to_vec().into(),
            said.into(),
            if said.is_some() { PENDING } else { UNSURE }.into(),
        ],
    ))
    .await?;
    // Hidden on her nodes at once, where she and her upkeep read them.
    super::forget::hide_taken_back(db, memories).await;
    Ok(())
}

/// Their message among the lines `user_text` held for a line gone over
/// alone (`memory_jobs::Payload::Chat`), held from `from` to `to`, in their
/// talk with her at `present`: the latest whose words are one of those
/// lines. None when none is found: what it lets go of is only let go.
pub(crate) async fn their_line<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    present: &Audience,
    user_text: &str,
    from: chrono::DateTime<chrono::FixedOffset>,
    to: chrono::DateTime<chrono::FixedOffset>,
) -> Option<i32> {
    let around = chrono::Duration::minutes(AROUND_MINUTES);
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id, m.content FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' \
               AND (s.context->>'venue') IS NOT DISTINCT FROM $2 \
               AND m.role = 'user' AND m.created_at >= $3 AND m.created_at <= $4 \
             ORDER BY m.created_at DESC, m.id DESC LIMIT 40",
            [
                user_id.into(),
                present.group_id().map(str::to_string).into(),
                (from - around).into(),
                (to + around).into(),
            ],
        ))
        .await
        .ok()?;
    let held: Vec<&str> = user_text.split('\n').collect();
    rows.iter().find_map(|row| {
        let content: String = row.try_get("", "content").ok()?;
        let words = crate::services::agent::merope::chat_remember::memory_user_text(&content)?;
        held.contains(&words.as_str())
            .then(|| row.try_get::<i32>("", "id").ok())
            .flatten()
    })
}

/// What is waiting for `user_id`, asked now: after what let it go is
/// committed, and again after each of their later memory jobs for what got
/// no answer. Never fails: one not asked stays waiting.
pub(crate) async fn go_over(db: &DatabaseConnection, user_id: i32) {
    go_over_until(db, user_id, std::time::Instant::now() + ONE_PERSON_FOR).await;
}

/// [`go_over`], starting none after `until`. Whether the model could be
/// asked: when it cannot be reached, nothing more is asked now.
async fn go_over_until(db: &DatabaseConnection, user_id: i32, until: std::time::Instant) -> bool {
    for _ in 0..AT_ONCE {
        if std::time::Instant::now() >= until {
            return true;
        }
        let claimed = match claim(db, user_id).await {
            Ok(Some(claimed)) => claimed,
            Ok(None) => return true,
            Err(error) => {
                tracing::warn!(
                    kind = super::forget::error_kind(&error),
                    "[memory-nodes] what waits for a second look could not be read"
                );
                return true;
            }
        };
        match ask(db, user_id, &claimed).await {
            Ok(Asked::Answered) => {}
            Ok(Asked::Unreachable) => return false,
            Err(error) => tracing::warn!(
                kind = super::forget::error_kind(&error),
                "[memory-nodes] a second look at what they took back stopped"
            ),
        }
    }
    true
}

/// Everyone's waiting ones, asked at her upkeep: one who says nothing more
/// after taking something back still has it looked at.
pub(crate) async fn go_over_waiting(db: &DatabaseConnection) {
    let users = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT user_id FROM agent_takeback_check \
             WHERE state = $1 \
               AND (asked_at IS NULL OR asked_at < now() - $2 * interval '1 second') \
             ORDER BY user_id LIMIT 50",
            [PENDING.into(), ASKED_AGAIN_AFTER_SECS.into()],
        ))
        .await;
    let Ok(users) = users else {
        tracing::warn!("[memory-nodes] who waits for a second look could not be read");
        return;
    };
    let until = std::time::Instant::now() + EVERYONE_FOR;
    for row in users {
        if std::time::Instant::now() >= until {
            return;
        }
        if let Ok(user_id) = row.try_get::<i32>("", "user_id")
            && !go_over_until(db, user_id, until).await
        {
            return;
        }
    }
}

/// One claimed to be asked: which memory, their line, and when it was held,
/// so what is written back lands only on that holding, not on a later one
/// of the same memory.
struct Claimed {
    memory: String,
    said: Option<i32>,
    at: chrono::DateTime<chrono::FixedOffset>,
}

/// How asking went.
enum Asked {
    /// Settled, or tried once more.
    Answered,
    /// The model could not be reached: left waiting as it was, no try spent.
    Unreachable,
}

/// One of `user_id`'s waiting ones nobody is asking now, marked as being
/// asked.
async fn claim(db: &DatabaseConnection, user_id: i32) -> Result<Option<Claimed>, Error> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_takeback_check c SET asked_at = now() \
             FROM (SELECT memory_id FROM agent_takeback_check \
                   WHERE user_id = $1 AND state = $2 \
                     AND (asked_at IS NULL OR asked_at < now() - $3 * interval '1 second') \
                   ORDER BY at, memory_id LIMIT 1 FOR UPDATE SKIP LOCKED) w \
             WHERE c.user_id = $1 AND c.memory_id = w.memory_id \
             RETURNING c.memory_id, c.said_ref, c.at",
            [
                user_id.into(),
                PENDING.into(),
                ASKED_AGAIN_AFTER_SECS.into(),
            ],
        ))
        .await?;
    row.map(|row| {
        Ok(Claimed {
            memory: row.try_get("", "memory_id")?,
            said: row.try_get("", "said_ref")?,
            at: row.try_get("", "at")?,
        })
    })
    .transpose()
}

/// The second look at one: their line (theirs, in a talk of theirs) and
/// what she kept, asked; then settled as the answer says.
async fn ask(db: &DatabaseConnection, user_id: i32, claimed: &Claimed) -> Result<Asked, Error> {
    let memory = claimed.memory.as_str();
    let line = match claimed.said {
        Some(said) => db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT m.content FROM agent_messages m \
                 JOIN agent_sessions s ON s.id = m.session_id \
                 WHERE m.id = $1 AND s.user_id = $2 AND m.role = 'user'",
                [said.into(), user_id.into()],
            ))
            .await?
            .map(|row| row.try_get::<String>("", "content"))
            .transpose()?,
        None => None,
    };
    let kept = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT content FROM agent_memories WHERE id = $1 AND user_id = $2",
            [memory.into(), user_id.into()],
        ))
        .await?
        .map(|row| row.try_get::<String>("", "content"))
        .transpose()?;
    let (Some(line), Some(kept)) = (line, kept) else {
        // Their line is gone (or never was theirs): nothing to look at.
        settle(db, user_id, claimed, UNSURE).await?;
        return Ok(Asked::Answered);
    };
    use myriad_merope::take_back_check as check;
    let answer =
        super::super::call::Ask::new(super::super::call::Voice::Judge, user_id, "take_back_check")
            .within(TIMEOUT)
            .json_raw(
                &check::system(),
                &check::input(&line, &kept),
                check::SCHEMA_NAME,
                &check::schema(),
            )
            .await;
    use super::super::call::Failure;
    let answer = match answer {
        // Not reached (her thoughts elsewhere, the model away or slow): no
        // answer was given, so no try is spent.
        Err(
            Failure::Unavailable
            | Failure::Timeout
            | Failure::Transport
            | Failure::Upstream
            | Failure::RateLimited
            | Failure::Quota
            | Failure::StreamInterrupted,
        ) => {
            tracing::info!(
                says = "unreached",
                "[memory-nodes] a second look at what they took back"
            );
            return Ok(Asked::Unreachable);
        }
        Err(_) => None,
        Ok(raw) => check::read(&raw),
    };
    tracing::info!(
        says = answer.map_or("none", check::Says::as_str),
        "[memory-nodes] a second look at what they took back"
    );
    match answer {
        Some(check::Says::TakesBack) => confirm(db, user_id, claimed).await?,
        Some(_) => settle(db, user_id, claimed, CHANGED).await?,
        None => {
            db.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE agent_takeback_check SET tries = tries + 1, \
                   state = CASE WHEN tries + 1 >= $4 THEN $5 ELSE state END \
                 WHERE user_id = $1 AND memory_id = $2 AND state = $3 AND at = $6",
                [
                    user_id.into(),
                    memory.into(),
                    PENDING.into(),
                    TRIES.into(),
                    UNSURE.into(),
                    claimed.at.into(),
                ],
            ))
            .await?;
        }
    }
    Ok(Asked::Answered)
}

/// One still waiting as it was claimed, settled as `state` without
/// forgetting anything.
async fn settle(
    db: &DatabaseConnection,
    user_id: i32,
    claimed: &Claimed,
    state: &str,
) -> Result<(), Error> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_takeback_check SET state = $4, at = now() \
         WHERE user_id = $1 AND memory_id = $2 AND state = $3 AND at = $5",
        [
            user_id.into(),
            claimed.memory.as_str().into(),
            PENDING.into(),
            state.into(),
            claimed.at.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// They took it back: confirmed, and what rests on it (and on what it
/// replaced) forgotten on her nodes, in one transaction. Only while the
/// memory is still let go as taken back (another end may have brought it
/// back meanwhile): otherwise it is left unsure and nothing is forgotten.
/// Forgetting that fails is left to the sweep, which forgets what was
/// confirmed.
async fn confirm(db: &DatabaseConnection, user_id: i32, claimed: &Claimed) -> Result<(), Error> {
    let memory = claimed.memory.as_str();
    let txn = db.begin().await?;
    let still = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 AS one FROM agent_memories WHERE id = $1 AND user_id = $2 \
               AND invalid_reason = 'superseded' AND superseded_by IS NULL FOR UPDATE",
            [memory.into(), user_id.into()],
        ))
        .await?
        .is_some();
    let state = if still { CONFIRMED } else { UNSURE };
    let confirmed = txn
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_takeback_check SET state = $4, at = now() \
             WHERE user_id = $1 AND memory_id = $2 AND state = $3 AND at = $5",
            [
                user_id.into(),
                memory.into(),
                PENDING.into(),
                state.into(),
                claimed.at.into(),
            ],
        ))
        .await?
        .rows_affected();
    if still && confirmed > 0 {
        super::forget::forget_taken_back(&txn, &[memory.to_string()]).await;
    }
    txn.commit().await?;
    Ok(())
}

/// How many wait, were only let go as a change or neither, and were left
/// unsure, over everyone: for the status page. Counts only; null when
/// they could not be read.
pub(crate) async fn counts(db: &DatabaseConnection) -> serde_json::Value {
    let Ok(rows) = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT state, count(*) AS n FROM agent_takeback_check \
             WHERE state = ANY($1) GROUP BY state",
            [vec![PENDING.to_string(), CHANGED.to_string(), UNSURE.to_string()].into()],
        ))
        .await
    else {
        return serde_json::Value::Null;
    };
    let mut counts = serde_json::json!({ PENDING: 0, CHANGED: 0, UNSURE: 0 });
    for row in rows {
        if let (Ok(state), Ok(n)) = (
            row.try_get::<String>("", "state"),
            row.try_get::<i64>("", "n"),
        ) {
            counts[state] = n.into();
        }
    }
    counts
}

/// Each of `user_id`'s taken back and not confirmed yet, confirmed now as
/// if the second look agreed: for tests of what follows a taking back.
#[cfg(test)]
pub(crate) async fn confirmed_now(db: &DatabaseConnection, user_id: i32) {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_takeback_check SET state = $2 \
             WHERE user_id = $1 AND state <> $3 RETURNING memory_id, said_ref, at",
            [user_id.into(), PENDING.into(), CONFIRMED.into()],
        ))
        .await
        .unwrap();
    for row in rows {
        let claimed = Claimed {
            memory: row.try_get("", "memory_id").unwrap(),
            said: row.try_get("", "said_ref").unwrap(),
            at: row.try_get("", "at").unwrap(),
        };
        confirm(db, user_id, &claimed).await.unwrap();
    }
}
