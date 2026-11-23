//! Recoverable memory extraction, owned by the Persona driver lifecycle.
use super::{call, chat_remember, store::memory_jobs as queue, strangers};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) enum Payload {
    /// What one person said to her in one place since it was last gone over:
    /// every line, in order, with her replies so far. A line said while she
    /// was still answering the one before is not lost; it joins the next.
    Chat {
        user_text: String,
        reply: String,
        /// The newest line's.
        input_at: chrono::DateTime<chrono::FixedOffset>,
        present: crate::services::agent::memory::unified::Audience,
        turn: super::TurnContext,
        /// When each line folded in here was said; one held twice (when it
        /// lands, and again with her reply) is not repeated.
        #[serde(default)]
        lines: Vec<chrono::DateTime<chrono::FixedOffset>>,
        /// What it keeps is kept for good, not as short notes: a private
        /// talk's lines that going over from what was said could not read
        /// (`store::memory_jobs::settling::leave_unread`), written down one
        /// by one instead.
        #[serde(default)]
        for_good: bool,
    },
    /// One person's stretch of talk in one place, gone over once it is over
    /// (see `chat_remember::settle`): from its first line to its latest.
    Settle {
        present: crate::services::agent::memory::unified::Audience,
        from: chrono::DateTime<chrono::FixedOffset>,
        last: chrono::DateTime<chrono::FixedOffset>,
        /// Their lines waiting, each with whether she has answered it: each
        /// is one line waiting, and her answer another
        /// ([`Payload::waiting`]). Once.
        #[serde(default)]
        heard: Vec<(chrono::DateTime<chrono::FixedOffset>, bool)>,
    },
    Stranger {
        venue: String,
        stranger: strangers::Stranger,
        exchanges: Vec<myriad_merope::strangers::Exchange>,
        count: i64,
    },
}
impl Payload {
    /// Fold a later arrival for the same person and place into this one.
    /// Their lines join in order; her reply, when there is one, is added.
    pub(super) fn fold(&mut self, later: Payload) {
        if let (
            Payload::Settle {
                from, last, heard, ..
            },
            Payload::Settle {
                from: added_from,
                last: added_last,
                heard: added_heard,
                ..
            },
        ) = (&mut *self, &later)
        {
            *from = (*from).min(*added_from);
            *last = (*last).max(*added_last);
            for one in added_heard {
                if !heard.contains(one) {
                    heard.push(*one);
                }
            }
            return;
        }
        let (
            Payload::Chat {
                user_text,
                reply,
                input_at,
                present,
                turn,
                lines,
                for_good,
            },
            Payload::Chat {
                user_text: added_text,
                reply: added_reply,
                input_at: added_at,
                turn: added_turn,
                lines: added_lines,
                for_good: added_for_good,
                ..
            },
        ) = (self, later)
        else {
            return;
        };
        let _ = present;
        *for_good |= added_for_good;
        if lines.is_empty() {
            lines.push(*input_at);
        }
        let added_lines = if added_lines.is_empty() {
            vec![added_at]
        } else {
            added_lines
        };
        if !added_lines.iter().all(|at| lines.contains(at)) {
            if !added_text.trim().is_empty() {
                if !user_text.is_empty() {
                    user_text.push('\n');
                }
                user_text.push_str(&added_text);
            }
            for at in added_lines {
                if !lines.contains(&at) {
                    lines.push(at);
                }
            }
            // What came just before their first line is still what came
            // before; where they are now is the latest.
            let before = turn.before.take();
            *turn = added_turn;
            turn.before = before.or(turn.before.take());
        }
        if !added_reply.trim().is_empty() && !reply.contains(added_reply.trim()) {
            if !reply.is_empty() {
                reply.push('\n');
            }
            reply.push_str(&added_reply);
        }
        *input_at = (*input_at).max(added_at);
    }

    /// Lines of a stretch waiting to be gone over, theirs and her answers
    /// together (`myriad_merope::settle_raw::box_is_full`).
    pub(super) fn waiting(&self) -> usize {
        match self {
            Payload::Settle { heard, .. } => {
                let theirs: std::collections::BTreeSet<_> =
                    heard.iter().map(|(at, _)| at).collect();
                theirs.len() + heard.iter().filter(|(_, answered)| *answered).count()
            }
            _ => 0,
        }
    }

    /// Held as it landed, before she has answered.
    pub(super) fn awaits_reply(&self) -> bool {
        matches!(self, Payload::Chat { reply, .. } if reply.trim().is_empty())
    }
}

pub(super) enum Effect {
    Chat(chat_remember::ChatMemoryUpdates),
    /// What a stretch gives to keep for good, and for each of `updates`,
    /// the short notes it was put together from (taken back in its favor).
    /// `rest`: it had more notes than are gone over at once, and those from
    /// then on are gone over next (see `chat_remember::settle`).
    Settle {
        updates: chat_remember::ChatMemoryUpdates,
        settled: Vec<Vec<String>>,
        rest: Option<chrono::DateTime<chrono::FixedOffset>>,
    },
    /// A private talk's pass gone over from what was said
    /// (`chat_remember::settle_raw`): as [`Effect::Settle`], with the lines
    /// it read, kept on the job once it is written so the next stretch does
    /// not go over them again.
    SettledFromLines {
        updates: chat_remember::ChatMemoryUpdates,
        settled: Vec<Vec<String>>,
        rest: Option<chrono::DateTime<chrono::FixedOffset>>,
        read: Vec<(chrono::DateTime<chrono::FixedOffset>, i32)>,
    },
    Stranger {
        previous: Option<String>,
        note: Option<myriad_merope::strangers::Noted>,
    },
    NoChange,
    Stale,
    /// Her switch went off while it was being gone over: put back to wait,
    /// as the queue waits while she is off, never let go (only clearing or
    /// retiring her, or the login it was taken up under going, lets it go).
    Paused,
}
#[derive(Debug)]
pub(super) enum Failure {
    Model(call::Failure),
    Storage,
}
impl Failure {
    pub(super) fn retryable(&self) -> bool {
        match self {
            Self::Model(failure) => failure.retryable(),
            Self::Storage => true,
        }
    }

    /// The model answered, but not in a way that could be read.
    pub(super) fn unreadable(&self) -> bool {
        matches!(self, Self::Model(call::Failure::InvalidOutput))
    }
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Model(failure) => failure.fmt(f),
            Self::Storage => f.write_str("storage_failed"),
        }
    }
}
impl From<call::Failure> for Failure {
    fn from(value: call::Failure) -> Self {
        Self::Model(value)
    }
}

pub(super) fn key(parts: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    hex::encode(digest.finalize())
}

/// How long a stretch is gone over, at most: placed on her nodes first,
/// then gone over (`chat_remember::settle`), with a little more for what it
/// reads around the call. A claim is held longer than this
/// (`store::memory_jobs`), so its answer is not let go as late.
pub(super) const SETTLE_LIMIT: Duration = chat_remember::settle::SETTLE_TIMEOUT
    .saturating_add(super::memory_nodes::placing::TIMEOUT)
    .saturating_add(Duration::from_secs(10));

/// Four bounded attempts at a time. Shutdown cancels this driver; unacknowledged
/// leases become eligible again, including when the process dies mid-request.
pub(crate) async fn tick(db: DatabaseConnection) {
    if !super::is_enabled().await {
        return;
    }
    let mut claims = Vec::new();
    for _ in 0..4 {
        match queue::claim(&db).await {
            Ok(Some(claim)) => claims.push(claim),
            Ok(None) => break,
            Err(_) => {
                tracing::warn!(outcome = "claim_failed", "[Merope] memory queue");
                break;
            }
        }
    }
    let db = &db;
    futures::future::join_all(claims.into_iter().map(|claim| async move {
        // A stretch placed on her nodes first takes that long more.
        let limit = match claim.job.data {
            Some(Payload::Settle { .. }) => SETTLE_LIMIT,
            _ => Duration::from_secs(45),
        };
        let result = match tokio::time::timeout(limit, prepare(db, &claim)).await {
            Ok(result) => result,
            Err(_) => Err(Failure::Model(call::Failure::Timeout)),
        };
        let result = if super::is_enabled().await {
            result
        } else {
            Ok(Effect::Paused)
        };
        if queue::finish(db, &claim, result).await.is_err() {
            // Do not acknowledge an uncertain write. Transaction rollback or
            // a lost response is resolved by the receipt on the next claim.
            tracing::warn!(
                job = claim.id,
                outcome = "commit_failed",
                "[Merope] memory queue"
            );
        }
    }))
    .await;
}
/// How another part of her that needed a private talk gone over fared
/// ([`hurry`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Hurried {
    /// Nothing of theirs was waiting.
    Nothing,
    /// Gone over in time.
    Done,
    /// Not in time ([`myriad_merope::settle_raw::HURRY_WAIT_SECS`]): it goes
    /// on without it, and the stretch is gone over all the same.
    Late,
}

static HURRIED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static HURRIED_LATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Talks gone over because another part of her needed them, and how many
/// of those were not in time, since the site started.
pub(crate) fn hurried() -> (u64, u64) {
    use std::sync::atomic::Ordering::Relaxed;
    (HURRIED.load(Relaxed), HURRIED_LATE.load(Relaxed))
}

/// Another part of her needs what they said in this private talk (reaching
/// out to them, her own day, another end opening it): what is waiting is
/// gone over now, and waited on a while. The caller does not go over it
/// itself; late, it goes on without it.
pub(super) async fn hurry(
    db: &DatabaseConnection,
    owner: i32,
    present: &crate::services::agent::memory::unified::Audience,
) -> Hurried {
    use std::sync::atomic::Ordering::Relaxed;
    if present.is_group() {
        return Hurried::Nothing;
    }
    let id = key(&["settle", &owner.to_string(), &present.venue()]);
    let Ok(Some(from)) = queue::hurry(db, &id, owner).await else {
        return Hurried::Nothing;
    };
    HURRIED.fetch_add(1, Relaxed);
    // Taken up now rather than at the next tick.
    let ticking = db.clone();
    super::background::spawn("a talk hurried, gone over", async move {
        tick(ticking).await;
    });
    let until = tokio::time::Instant::now()
        + Duration::from_secs(myriad_merope::settle_raw::HURRY_WAIT_SECS);
    loop {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if !queue::still_waiting(db, &id, from).await.unwrap_or(true) {
            return Hurried::Done;
        }
        if tokio::time::Instant::now() >= until {
            HURRIED_LATE.fetch_add(1, Relaxed);
            return Hurried::Late;
        }
    }
}

async fn prepare(db: &DatabaseConnection, claim: &queue::Claim) -> Result<Effect, Failure> {
    if !super::is_enabled().await {
        return Ok(Effect::Paused);
    }
    if !queue::current(db, &claim.job, false)
        .await
        .map_err(|_| Failure::Storage)?
    {
        return Ok(Effect::Stale);
    }
    match claim.job.data.as_ref().expect("claimed jobs carry work") {
        Payload::Chat {
            user_text,
            reply,
            input_at,
            present,
            turn,
            ..
        } => {
            chat_remember::extract(
                db,
                claim.job.owner,
                user_text,
                reply,
                *input_at,
                present,
                turn,
            )
            .await
        }
        // A private talk is gone over from what was said; elsewhere, from
        // the short notes taken line by line.
        Payload::Settle {
            present,
            from,
            last,
            ..
        } if !present.is_group() => {
            chat_remember::settle_raw::settle_raw(
                db,
                claim.job.owner,
                present,
                *from,
                *last,
                claim.job.read.as_ref(),
            )
            .await
        }
        Payload::Settle { present, from, .. } => {
            chat_remember::settle(db, claim.job.owner, present, *from, claim.job.said_from).await
        }
        Payload::Stranger {
            venue,
            stranger,
            exchanges,
            count,
        } => strangers::prepare_note(db, claim.job.owner, venue, stranger, exchanges, *count).await,
    }
}
