//! A stretch that could not be gone over is not let go: its notes stay
//! notes, and the next stretch in the same place is gone over from where
//! the one left unread began, so they are brought along. Brought along
//! [`BROUGHT_AT_MOST`] times and still unread, they are kept for good as
//! they are, as going over keeps a note it passes over: none is lost, the
//! stretches after are gone over on their own, and what is shown does not
//! keep growing. Kept so, they are not put together with what she already
//! knows and may say it twice, or say what is no longer so; a stretch that
//! comes to it later replaces it as it would any fact.
//!
//! What a stretch brought along is dropped with it when the job goes stale
//! (`Effect::Stale`, or the job rebuilt on enqueue). Its notes stay notes and
//! fade; nothing of theirs is kept for good. Staling is destroying, so it is
//! only for what the job was taken up under being gone (AGENTS.md: 暂存不是
//! 销毁). Her switch off is putting by; clearing or retiring her, the login
//! changing and the person gone are destroying. Each, as the queue has it:
//! - Her switch turned off, or she is not on the site (moving, living on
//!   another end, moved out): the queue is not taken up meanwhile, and a job
//!   being gone over as she goes off is put back as it was
//!   (`Effect::Paused`). Putting by: it goes on when she is back, while it
//!   is kept (a day, or [`WAITS`] while it holds notes left unread).
//! - Her persona cleared or retired (`store::persona`, `clear_on` and
//!   `retire_her_on`): those run `memory_jobs::forget` and drop every job
//!   outright; and a her born again after, or brought back from another
//!   end's backup, is another lineage, which no job taken up before goes on
//!   for (`memory_jobs::Her`). Destroying.
//! - `users.token_version` changed (signed out everywhere, password changed),
//!   or the person is gone: the login it was taken up under is gone.
//!   Destroying.
//! - Editing her (name, personality, zone, looks) and portrait or avatar
//!   generation noting how it goes leave her the same her: nothing is let
//!   go for them.
//! - A device's her merged into hers on the site (`persona_sync::merge`):
//!   she is still the her the site has, with more to her, and the lineage
//!   stays. Nothing she was told on the site before is let go: while the
//!   merge lands she is frozen, which is her being off (putting by), and
//!   what was waiting is gone over after, into the merged her. Letting the
//!   queue go there would leave those notes with no stretch to keep them.

use sea_orm::ConnectionTrait;

use super::{Job, Payload};

/// How long a stretch left unread waits for the next: its notes are still
/// notes until then, however long they go unmentioned.
pub(super) const WAITS: i64 = 90 * 86_400;

/// How many stretches after it a stretch left unread is brought along by
/// before its notes are kept as they are.
const BROUGHT_AT_MOST: u8 = 2;

/// Notes brought along into the stretch now being gone over: from `since`,
/// up to where the stretch itself began (`own`), brought `times` so far.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub(in crate::services::agent::merope) struct Brought {
    since: chrono::DateTime<chrono::FixedOffset>,
    own: chrono::DateTime<chrono::FixedOffset>,
    times: u8,
}

/// How going over stretches went wrong since the site started, for its
/// status: answers that could not be read, stretches tried again (for any
/// failure), stretches left unread after their last try (their notes wait
/// for the next), stretches that brought such notes along, and notes kept
/// for good as they were once brought along as often as they are. Counts
/// only, never what was said.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Failures {
    pub(crate) unreadable: u64,
    pub(crate) asked_again: u64,
    pub(crate) left_unread: u64,
    pub(crate) brought_along: u64,
    pub(crate) kept_unsettled: u64,
}

static UNREADABLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static ASKED_AGAIN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LEFT_UNREAD: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static BROUGHT_ALONG: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static KEPT_UNSETTLED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// [`Failures`] so far.
pub(crate) fn failures() -> Failures {
    use std::sync::atomic::Ordering::Relaxed;
    Failures {
        unreadable: UNREADABLE.load(Relaxed),
        asked_again: ASKED_AGAIN.load(Relaxed),
        left_unread: LEFT_UNREAD.load(Relaxed),
        brought_along: BROUGHT_ALONG.load(Relaxed),
        kept_unsettled: KEPT_UNSETTLED.load(Relaxed),
    }
}

fn counted(counter: &std::sync::atomic::AtomicU64, by: u64) {
    counter.fetch_add(by, std::sync::atomic::Ordering::Relaxed);
}

pub(super) fn unreadable() {
    counted(&UNREADABLE, 1);
}

pub(super) fn asked_again() {
    counted(&ASKED_AGAIN, 1);
}

/// The stretch `job` holds is let go unread after its last try. Where it
/// began is kept for the next to begin there too; notes it brought along
/// as often as they are brought are kept for good as they are, and only
/// its own wait. What was kept so, and where: to be put on her nodes once
/// this is written ([`super::super::super::memory_nodes::facts::kept_as_they_were`]).
pub(super) async fn leave_unread(
    db: &sea_orm::DatabaseTransaction,
    job: &mut Job,
) -> anyhow::Result<Option<Kept>> {
    let Some(Payload::Settle {
        from,
        last,
        present,
    }) = &job.data
    else {
        return Ok(None);
    };
    let (from, last, present) = (*from, *last, present.clone());
    let mut kept = 0;
    let mut kept_as = None;
    match job.brought.take() {
        Some(brought) if brought.times >= BROUGHT_AT_MOST => {
            // On its own: what cannot be kept so is left unread as before,
            // never holding up the rest of the queue.
            let inner = sea_orm::TransactionTrait::begin(db).await?;
            match keep_as_they_are(
                &inner,
                job.owner,
                &present,
                last,
                brought.since,
                brought.own,
            )
            .await
            {
                Ok(facts) => {
                    inner.commit().await?;
                    kept = facts.len();
                    kept_as = Some(Kept {
                        owner: job.owner,
                        present: present.clone(),
                        facts,
                    });
                    counted(&KEPT_UNSETTLED, kept as u64);
                    job.unread_from = Some(brought.own);
                    job.unread_times = 0;
                }
                Err(_) => {
                    inner.rollback().await?;
                    tracing::warn!(
                        owner = job.owner,
                        "[Merope] notes left unread could not be kept as they were: they wait"
                    );
                    job.unread_from = Some(brought.since);
                    job.unread_times = brought.times;
                }
            }
        }
        Some(brought) => {
            job.unread_from = Some(brought.since);
            job.unread_times = brought.times;
        }
        None => {
            job.unread_from = Some(job.unread_from.map_or(from, |earlier| earlier.min(from)));
        }
    }
    counted(&LEFT_UNREAD, 1);
    tracing::warn!(
        owner = job.owner,
        kept_as_they_were = kept,
        failures = %serde_json::to_value(failures()).unwrap_or_default(),
        "[Merope] a stretch was left unread: its notes go with the next"
    );
    Ok(kept_as)
}

/// Notes kept for good as they were ([`leave_unread`]): whose, where, and
/// each as it is kept now.
pub(in crate::services::agent::merope) struct Kept {
    pub(in crate::services::agent::merope) owner: i32,
    pub(in crate::services::agent::merope) present:
        crate::services::agent::memory::unified::Audience,
    pub(in crate::services::agent::merope) facts: Vec<String>,
}

impl Kept {
    /// Put on her nodes, once what kept them is written.
    pub(in crate::services::agent::merope) async fn on_nodes(
        self,
        db: &sea_orm::DatabaseConnection,
    ) {
        let site_owner = crate::services::principal::site_owner_id(db)
            .await
            .ok()
            .flatten();
        // Boxed, as going over's own: the chain down to the model call
        // would otherwise be laid out in the caller's frame.
        Box::pin(super::super::super::memory_nodes::facts::kept_as_they_were(
            db,
            self.owner,
            site_owner,
            &self.present,
            &self.facts,
        ))
        .await;
    }
}

/// A stretch taken up to be gone over begins where one left unread began.
/// Only as it is taken up: while lines still come, when it is due goes by
/// where this stretch itself began.
pub(super) fn bring_along(job: &mut Job) {
    let Some(Payload::Settle { from, .. }) = &mut job.data else {
        return;
    };
    if let Some(unread) = job.unread_from.take() {
        job.brought = Some(Brought {
            since: unread,
            own: *from,
            times: job.unread_times + 1,
        });
        job.unread_times = 0;
        *from = (*from).min(unread);
    }
}

/// The stretch `job` holds is over (gone over, let go, or out of tries):
/// counted once when it brought notes along. Not as it is taken up, so a
/// stretch put back and taken up again is not counted twice.
pub(super) fn over(job: &Job) {
    if job.brought.is_some() {
        counted(&BROUGHT_ALONG, 1);
    }
}

/// A stretch gone over in part: what is left of it is gone over next, from
/// `rest`, shown its lines from where they began. What it brought along and
/// has gone over now is over; what it brought along and has not stays
/// brought along, as many times as it was (a stretch's passes are one
/// bringing along): left unread, it waits for the next stretch, or is kept
/// as it is once brought along [`BROUGHT_AT_MOST`] times.
pub(super) fn go_on(job: &mut Job, rest: chrono::DateTime<chrono::FixedOffset>) {
    let Some(Payload::Settle { from, .. }) = &mut job.data else {
        return;
    };
    job.said_from = Some(job.said_from.unwrap_or(*from));
    *from = rest;
    match &mut job.brought {
        Some(brought) if rest < brought.own => brought.since = rest,
        Some(_) => {
            over(job);
            job.brought = None;
        }
        None => {}
    }
}

/// A stretch put back untouched (she went off while it was gone over):
/// what it brought along is left unread again, as before it was taken up.
pub(super) fn put_back(job: &mut Job) {
    let Some(brought) = job.brought.take() else {
        return;
    };
    if let Some(Payload::Settle { from, .. }) = &mut job.data {
        *from = brought.own;
    }
    job.unread_from = Some(brought.since);
    job.unread_times = brought.times.saturating_sub(1);
}

/// The notes of `owner` here from `since` until `until`, kept for good as
/// they are: each as going over keeps a note it passes over. Each as it is
/// kept.
async fn keep_as_they_are<C: ConnectionTrait + sea_orm::TransactionTrait>(
    db: &C,
    owner: i32,
    present: &crate::services::agent::memory::unified::Audience,
    last: chrono::DateTime<chrono::FixedOffset>,
    since: chrono::DateTime<chrono::FixedOffset>,
    until: chrono::DateTime<chrono::FixedOffset>,
) -> anyhow::Result<Vec<String>> {
    use crate::services::agent::memory::unified;
    let rows = unified::active_in(db, owner, present, &unified::MemoryKind::ABOUT_PERSON).await?;
    let mut updates = super::super::super::chat_remember::ChatMemoryUpdates::default();
    let mut settled = Vec::new();
    for note in rows.iter().filter(|row| {
        row.source == unified::SHORT_NOTE_SOURCE
            && row.created_at >= since
            && row.created_at < until
    }) {
        settled.push(vec![note.id.clone()]);
        updates
            .updates
            .push(super::super::super::chat_remember::ChatMemoryUpdate {
                standing: Default::default(),
                fact: Some(super::super::super::ingest::compact_summary(&note.content)),
                supersedes: Vec::new(),
                evidence: note.evidence.clone(),
                concepts: unified::clean_concepts(note.concepts.clone()),
                topic: None,
                cues: Vec::new(),
            });
    }
    super::super::settle_chat_memory_on(db, owner, last, &updates, &settled, present).await?;
    Ok(updates
        .updates
        .into_iter()
        .filter_map(|update| update.fact)
        .collect())
}
