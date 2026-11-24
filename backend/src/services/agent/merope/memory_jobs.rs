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
        /// What it keeps is kept for good, not as short notes: what was said
        /// to her at work, and a private talk's lines that going over from
        /// what was said could not read
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
    /// A stretch gone over of someone who asked her not to keep something:
    /// its lines are to be judged (`asked::lines`), and none of it is looked
    /// back to until they are. Written with what the stretch kept, so never
    /// missed; tried until it is done.
    NotKept {
        present: crate::services::agent::memory::unified::Audience,
        from: chrono::DateTime<chrono::FixedOffset>,
        last: chrono::DateTime<chrono::FixedOffset>,
    },
    /// They asked her not to keep something: what she already held of them
    /// (`asked::old`), in private or in the group `venue`, is to be held to
    /// it. Written with the request, so never missed; tried until it is done.
    AskedOver {
        asked: chrono::DateTime<chrono::FixedOffset>,
        #[serde(default)]
        venue: Option<String>,
        /// What was told to stay on a try before, each known by what it says
        /// and where it was held (hashed); asked about again by another
        /// request, or once it says something else.
        #[serde(default)]
        stays: Vec<String>,
    },
    /// Who they are in a group is known now, and they asked her not to keep
    /// something: what she noted of them there (by `marker`) becomes theirs,
    /// without what is of it (`strangers::adopt_held`). Tried until it is
    /// done; not theirs till then.
    Adopt { marker: String },
    /// Short notes of someone who asked her not to keep something, from
    /// `since` until `until`, left unread time after time: kept for good as
    /// they are (`store::memory_jobs::settling`), without what is of it
    /// (`asked::unread`). Tried until it is done; notes till then.
    LeftUnread {
        present: crate::services::agent::memory::unified::Audience,
        last: chrono::DateTime<chrono::FixedOffset>,
        since: chrono::DateTime<chrono::FixedOffset>,
        until: chrono::DateTime<chrono::FixedOffset>,
    },
    /// A stretch of a group's talk as it was read, and what of it those in it
    /// asked her not to keep not told yet: nothing of it kept till it is. Let
    /// go with the words themselves (`channel_group::heard_again`).
    GroupHeard {
        venue: String,
        stretch: Vec<(chrono::DateTime<chrono::FixedOffset>, String, String, bool)>,
        speakers: Vec<(String, String)>,
        read: super::bits::Read,
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
            // More than is gone over at once: the oldest of it is let go,
            // and counted.
            let mut away = 0;
            while user_text.chars().count() > FOLDED_CHARS {
                match user_text.split_once('\n') {
                    Some((_, rest)) => *user_text = rest.to_string(),
                    None => break,
                }
                away += 1;
            }
            if away > 0 {
                FOLDED_AWAY.fetch_add(away, std::sync::atomic::Ordering::Relaxed);
                tracing::warn!(
                    lines = away,
                    "[Merope] lines waiting to be gone over let go: more than is gone over at once"
                );
            }
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

    /// Judged by what they asked her not to keep.
    pub(super) fn by_what_they_asked(&self) -> bool {
        self.only_holds_to_it()
            || matches!(
                self,
                Payload::LeftUnread { .. } | Payload::GroupHeard { .. }
            )
    }

    /// Judged so and writing nothing new: the lines, and what she held, are
    /// there whoever she is by then.
    pub(super) fn only_holds_to_it(&self) -> bool {
        matches!(
            self,
            Payload::NotKept { .. } | Payload::AskedOver { .. } | Payload::Adopt { .. }
        )
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
        /// The message the pass ended within, and how many of its parts it
        /// or one before went over.
        partway: Option<(i32, usize)>,
        /// Their messages they took back in the stretch itself, or asked her
        /// in not to keep something: not brought up again
        /// (`store::memory_jobs::finish`).
        let_go: Vec<i32>,
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
    /// What they asked her not to keep could not be told of it (`asked`):
    /// nothing of it is written till it is, and it is tried again for that
    /// alone, however it failed ([`GIVES_UP_AFTER`]).
    NotTold(call::Failure),
    /// Not all of it could be asked about now (`last`), and of what was, one
    /// got no answer that will do (`shown`): tried again as the first, and
    /// counted as the second ([`Self::counted`]).
    Unreached {
        last: call::Failure,
        shown: call::Failure,
    },
    Storage,
}
impl Failure {
    pub(super) fn retryable(&self) -> bool {
        match self {
            Self::Model(failure) | Self::Unreached { last: failure, .. } => failure.retryable(),
            Self::NotTold(_) => false,
            Self::Storage => true,
        }
    }

    /// The model was asked and gave no answer that will do, for what it was
    /// shown ([`call::Failure::of_what_was_shown`]): given up on in the end.
    pub(super) fn untold(&self) -> bool {
        matches!(
            self,
            Self::Model(failure) | Self::NotTold(failure) if failure.of_what_was_shown()
        )
    }

    /// What it counts as towards giving up, if at all: of what was shown,
    /// whatever else could not be asked about.
    pub(super) fn counted(&self) -> Option<call::Failure> {
        match self {
            Self::Model(failure) | Self::NotTold(failure) if failure.of_what_was_shown() => {
                Some(*failure)
            }
            Self::Unreached { shown, .. } => Some(*shown),
            _ => None,
        }
    }

    /// Refused for what it was shown: so until that is another.
    pub(super) fn refused(&self) -> bool {
        matches!(
            self,
            Self::Model(call::Failure::Refused) | Self::NotTold(call::Failure::Refused)
        )
    }

    /// [`Self::NotTold`].
    pub(super) fn not_told(&self) -> bool {
        matches!(self, Self::NotTold(_))
    }

    /// Of judging by what they asked not to keep: to be tried till it is told.
    pub(super) fn of_judging(self) -> Self {
        match self {
            Self::Model(failure) => Self::NotTold(failure),
            failure => failure,
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
            Self::Model(failure) | Self::Unreached { last: failure, .. } => failure.fmt(f),
            Self::NotTold(failure) => write!(f, "not_told_{failure}"),
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
            // A line asking her not to keep something is judged too.
            Some(
                Payload::Settle { .. }
                | Payload::Chat { .. }
                | Payload::NotKept { .. }
                | Payload::AskedOver { .. }
                | Payload::Adopt { .. }
                | Payload::LeftUnread { .. }
                | Payload::GroupHeard { .. },
            ) => SETTLE_LIMIT,
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
        match queue::waiting_for(db, &id, from).await {
            Ok(queue::WaitingFor::Over) => return Hurried::Done,
            // Let go unread: not in time, whatever comes of it.
            Ok(queue::WaitingFor::Unread) => {
                HURRIED_LATE.fetch_add(1, Relaxed);
                return Hurried::Late;
            }
            Ok(queue::WaitingFor::Waiting) | Err(_) => {}
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
    let nth = usize::from(claim.job.attempts);
    let own = queue::Own::of(claim);
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
                if claim.gives_up {
                    chat_remember::Tried::GaveUp
                } else if claim.job.attempts < queue::MAX_ATTEMPTS {
                    chat_remember::Tried::Again
                } else {
                    chat_remember::Tried::Last
                },
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
        Payload::NotKept {
            present,
            from,
            last,
        } => super::asked::lines::mark(
            db,
            claim.job.owner,
            present,
            *from,
            *last,
            judged_by(),
            claim.gives_up,
        )
        .await
        .map(|()| Effect::NoChange),
        Payload::AskedOver { venue, .. } => match venue {
            None => {
                super::asked::old::let_go(
                    db,
                    claim.job.owner,
                    judged_by(),
                    claim.gives_up,
                    nth,
                    Some(own),
                )
                .await
            }
            Some(venue) => {
                super::asked::old::let_go_in(
                    db,
                    claim.job.owner,
                    venue,
                    judged_by(),
                    claim.gives_up,
                    nth,
                    Some(own),
                )
                .await
            }
        }
        .map(|()| Effect::NoChange),
        Payload::Adopt { marker } => {
            strangers::adopt_held(db, claim.job.owner, marker, judged_by(), claim.gives_up)
                .await
                .map(|()| Effect::NoChange)
        }
        Payload::LeftUnread {
            present,
            last,
            since,
            until,
        } => super::asked::unread::keep(db, own, present, *last, (*since, *until), judged_by())
            .await
            .map(|()| Effect::NoChange),
        Payload::GroupHeard {
            venue,
            stretch,
            speakers,
            read,
        } => crate::services::channel_group::heard_again(
            db,
            claim.job.owner,
            venue,
            stretch,
            speakers,
            super::bits::Again { read, own },
            claim.gives_up,
        )
        .await
        .map(|()| Effect::NoChange)
        .map_err(|not_told| not_told.failure),
    }
}

/// When what a job asks of the judge is to be told by: within what the job
/// has.
fn judged_by() -> std::time::Instant {
    std::time::Instant::now() + SETTLE_LIMIT - Duration::from_secs(8)
}

/// A stretch of `venue`'s talk, read, to be kept later
/// ([`Payload::GroupHeard`]).
pub(crate) async fn heard_later(
    db: &DatabaseConnection,
    owner: i32,
    venue: &str,
    stretch: Vec<(chrono::DateTime<chrono::FixedOffset>, String, String, bool)>,
    speakers: Vec<(String, String)>,
    read: super::bits::Read,
) -> anyhow::Result<()> {
    let id = key(&["group_heard", venue, &read.at.to_rfc3339()]);
    let data = Payload::GroupHeard {
        venue: venue.to_string(),
        stretch,
        speakers,
        read,
    };
    queue::enqueue(db, &id, owner, data, None).await
}

/// About half a day of tries ([`queue`]'s, for what is tried until done).
pub(super) const GIVES_UP_AFTER: u8 = 60;
/// What is refused, or not told, for this long is given up on however seldom
/// it was asked.
pub(super) const GIVES_UP_AFTER_SECS: i64 = 3 * 86_400;
/// How long a stretch of a group's talk is tried: as long as its lines are at
/// hand.
pub(crate) const HEARD_FOR: chrono::Duration = chrono::Duration::hours(6);
/// Lines waiting to be gone over together, at most.
const FOLDED_CHARS: usize = 12_000;

static FOLDED_AWAY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Lines let go for want of room where they waited ([`FOLDED_CHARS`]), never
/// gone over, since the site started.
pub(super) fn folded_away() -> u64 {
    FOLDED_AWAY.load(std::sync::atomic::Ordering::Relaxed)
}
