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
static ONE_BY_ONE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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

/// A private talk gone over from what was said, since the site started:
/// their lines asked about again for being accounted for nowhere, those
/// still accounted for nowhere after (they stay as said), lines sent back
/// to be written one by one ([`leave_unread`]), talks gone over
/// because another part of her needed them, and of those, how many were
/// not in time. Counts only, never what was said.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FromLines {
    pub(crate) asked_again: u64,
    pub(crate) left: u64,
    /// Their lines left unread after being brought along twice, sent back
    /// to be written down one by one.
    pub(crate) one_by_one: u64,
    pub(crate) hurried: u64,
    pub(crate) hurried_late: u64,
}

/// [`FromLines`] so far.
pub(crate) fn from_lines() -> FromLines {
    let (left, asked_again) = super::super::super::chat_remember::settle_raw::unaccounted();
    let (hurried, hurried_late) = super::super::super::memory_jobs::hurried();
    FromLines {
        asked_again,
        left,
        one_by_one: ONE_BY_ONE.load(std::sync::atomic::Ordering::Relaxed),
        hurried,
        hurried_late,
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
        ..
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
            // A private talk is not noted line by line: what it brought
            // along goes back to be written down one by one, the old way,
            // and kept for good, on this same transaction (notes from
            // before it was not are kept as they are, as before).
            let inner = sea_orm::TransactionTrait::begin(db).await?;
            let read = job.read.clone();
            let kept_now = async {
                let facts = keep_as_they_are(
                    &inner,
                    job.owner,
                    &present,
                    last,
                    brought.since,
                    brought.own,
                )
                .await?;
                let lines = if present.is_group() {
                    0
                } else {
                    one_by_one(&inner, job, brought.since, brought.own).await?
                };
                anyhow::Ok((facts, lines))
            }
            .await;
            match kept_now {
                Ok((facts, lines)) => {
                    inner.commit().await?;
                    counted(&ONE_BY_ONE, lines as u64);
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
                    job.read = read;
                    tracing::warn!(
                        owner = job.owner,
                        "[Merope] notes or lines left unread could not be kept: they wait"
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

/// Whether every message of `read`, and the one it ended within, is still
/// kept.
pub(super) async fn all_there(
    db: &sea_orm::DatabaseTransaction,
    read: &[(chrono::DateTime<chrono::FixedOffset>, i32)],
    partway: Option<(i32, usize)>,
) -> anyhow::Result<bool> {
    use sea_orm::{DatabaseBackend, Statement};
    let ids: std::collections::BTreeSet<i32> = read
        .iter()
        .map(|(_, id)| *id)
        .chain(partway.map(|(id, _)| id))
        .collect();
    let there = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT count(*) AS there FROM agent_messages WHERE id = ANY($1)",
            [ids.iter().copied().collect::<Vec<i32>>().into()],
        ))
        .await?
        .map(|row| row.try_get::<i64>("", "there"))
        .transpose()?
        .unwrap_or_default();
    Ok(there as usize == ids.len())
}

/// What a line written down on its own and kept for good (`update`) rests
/// on: their messages said at `lines`, in private.
pub(super) async fn of_lines(
    db: &sea_orm::DatabaseTransaction,
    owner: i32,
    present: &crate::services::agent::memory::unified::Audience,
    update: &mut super::super::super::chat_remember::ChatMemoryUpdates,
    lines: &[chrono::DateTime<chrono::FixedOffset>],
) -> anyhow::Result<()> {
    use sea_orm::{DatabaseBackend, Statement};
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' \
               AND (s.context->>'venue') IS NOT DISTINCT FROM $2 \
               AND m.role = 'user' AND m.created_at = ANY($3)",
            [
                owner.into(),
                present.group_id().map(str::to_string).into(),
                lines.to_vec().into(),
            ],
        ))
        .await?;
    let ids: Vec<Option<i32>> = if rows.is_empty() {
        vec![None]
    } else {
        rows.iter()
            .map(|row| row.try_get("", "id").map(Some))
            .collect::<Result<_, _>>()?
    };
    let kept: Vec<String> = update
        .updates
        .iter()
        .filter(|one| one.fact.is_some())
        .filter_map(|one| one.evidence.clone())
        .collect();
    for evidence in kept {
        for id in &ids {
            for at in lines {
                update.rests_on(&evidence, *id, *at);
            }
        }
    }
    // What she said back then, found from when it was said.
    let hers: Vec<String> = update
        .said
        .iter()
        .map(|said| said.evidence.clone())
        .collect();
    for evidence in hers {
        for at in lines {
            update.rests_on(&evidence, None, *at);
        }
    }
    Ok(())
}

/// Their messages `lines`, taken back in the stretch they were said in: no
/// more brought up from what was said than one they asked her not to keep
/// (`asked::lines`), and hidden on her nodes.
pub(super) async fn not_brought_up(
    db: &sea_orm::DatabaseTransaction,
    lines: &[i32],
) -> anyhow::Result<()> {
    use sea_orm::{DatabaseBackend, Statement};
    if lines.is_empty() {
        return Ok(());
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_messages SET metadata = (CASE WHEN jsonb_typeof(metadata::jsonb) = 'object' \
             THEN metadata::jsonb ELSE '{}'::jsonb END \
           || '{\"notKept\":true,\"takenBack\":true}'::jsonb)::json WHERE id = ANY($1)",
        [lines.to_vec().into()],
    ))
    .await?;
    Ok(())
}

/// A private talk's lines not gone over from `since` until `until`, theirs
/// and her answers, back in the queue to be written down the old way, line
/// by line (`memory_jobs::Payload::Chat`), each on its own and kept for
/// good: put there on `db`, with what lets the stretch go. How many of their
/// lines; `job` has them as gone over, none to be gone over again.
async fn one_by_one(
    db: &sea_orm::DatabaseTransaction,
    job: &mut Job,
    since: chrono::DateTime<chrono::FixedOffset>,
    until: chrono::DateTime<chrono::FixedOffset>,
) -> anyhow::Result<usize> {
    use super::super::super::chat_remember::settle_raw::{ELSEWHERE, Read, SAID};
    use sea_orm::{DatabaseBackend, Statement};
    let owner = job.owner;
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "SELECT m.id, m.role, m.content, m.created_at, \
                   {ELSEWHERE} AS elsewhere, \
                   COALESCE(m.metadata->>'inGame', '') = 'true' AS in_game \
                 FROM agent_messages m \
                 JOIN agent_sessions s ON s.id = m.session_id \
                 WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' \
                   AND (s.context->>'venue') IS NULL AND {SAID} \
                   AND m.created_at >= $2 AND m.created_at < $3 \
                 ORDER BY m.created_at, m.id"
            ),
            [
                owner.into(),
                Read::looks_from(job.read.as_ref(), since).into(),
                until.into(),
            ],
        ))
        .await?;
    let present = crate::services::agent::memory::unified::Audience::private(owner);
    let mut theirs = 0;
    let mut read = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let id: i32 = row.try_get("", "id")?;
        let at: chrono::DateTime<chrono::FixedOffset> = row.try_get("", "created_at")?;
        let before = job.read.as_ref().is_some_and(|read| read.has(id));
        // Gone over in part already: only the rest of it.
        let done = job
            .read
            .as_ref()
            .and_then(|read| read.partway)
            .filter(|(partway, _)| *partway == id)
            .map_or(0, |(_, done)| done);
        read.push((at, id));
        if before
            || row.try_get::<bool>("", "elsewhere")?
            || row.try_get::<String>("", "role")? != "user"
        {
            continue;
        }
        let text: String = row.try_get("", "content")?;
        // Her answer to it, when the next line is hers.
        let reply = match rows.get(index + 1) {
            Some(next) if next.try_get::<String>("", "role")? == "assistant" => {
                super::super::super::ingest::compact_summary(
                    &next.try_get::<String>("", "content")?,
                )
            }
            _ => String::new(),
        };
        let turn = super::super::super::TurnContext {
            in_game: row.try_get("", "in_game")?,
            ..Default::default()
        };
        // A job each (a long line's part each), so each is read by the day
        // it was said on, and none folds into the next.
        let parts = myriad_merope::settle_raw::line_parts(&text);
        for (part, text) in parts.into_iter().enumerate().skip(done) {
            let Some(text) = super::super::super::chat_remember::memory_user_text(text) else {
                continue;
            };
            let moment = at.timestamp_micros().to_string();
            let whole = ["chat", &owner.to_string(), &present.venue(), &moment];
            let nth = part.to_string();
            let key = if part == 0 {
                super::super::super::memory_jobs::key(&whole)
            } else {
                super::super::super::memory_jobs::key(&[&whole[..], &[&nth]].concat())
            };
            super::enqueue_in(
                db,
                &key,
                owner,
                Payload::Chat {
                    user_text: text,
                    reply: reply.clone(),
                    input_at: at,
                    present: present.clone(),
                    turn: turn.clone(),
                    lines: vec![at],
                    for_good: true,
                },
                None,
            )
            .await?;
        }
        theirs += 1;
    }
    let mut read = job.read.take().unwrap_or_default().with(&read);
    read.partway = None;
    job.read = Some(read);
    Ok(theirs)
}

/// The notes of `owner` here from `since` until `until`.
pub(in crate::services::agent::merope) async fn notes_left<C: ConnectionTrait>(
    db: &C,
    owner: i32,
    present: &crate::services::agent::memory::unified::Audience,
    since: chrono::DateTime<chrono::FixedOffset>,
    until: chrono::DateTime<chrono::FixedOffset>,
) -> Result<Vec<crate::services::agent::memory::unified::MemoryRecord>, sea_orm::DbErr> {
    use crate::services::agent::memory::unified;
    Ok(
        unified::active_in(db, owner, present, &unified::MemoryKind::ABOUT_PERSON)
            .await?
            .into_iter()
            .filter(|row| {
                row.source == unified::SHORT_NOTE_SOURCE
                    && row.created_at >= since
                    && row.created_at < until
            })
            .collect(),
    )
}

/// The notes of `owner` here from `since` until `until`, kept for good as
/// they are. Of someone who asked her not to keep something, once they are
/// held to it (`asked::unread`), by a job written here.
async fn keep_as_they_are(
    db: &sea_orm::DatabaseTransaction,
    owner: i32,
    present: &crate::services::agent::memory::unified::Audience,
    last: chrono::DateTime<chrono::FixedOffset>,
    since: chrono::DateTime<chrono::FixedOffset>,
    until: chrono::DateTime<chrono::FixedOffset>,
) -> anyhow::Result<Vec<String>> {
    use crate::services::agent::memory::unified;
    let notes = notes_left(db, owner, present, since, until).await?;
    if notes.is_empty() || unified::asked_not_to_keep(db, owner).await?.is_empty() {
        return keep_notes(db, owner, present, last, notes).await;
    }
    let id = super::super::super::memory_jobs::key(&[
        "left_unread",
        &owner.to_string(),
        &present.venue(),
        &since.to_rfc3339(),
        &until.to_rfc3339(),
    ]);
    let data = Payload::LeftUnread {
        present: present.clone(),
        last,
        since,
        until,
    };
    super::enqueue_in(db, &id, owner, data, None).await?;
    Ok(Vec::new())
}

/// `notes` kept for good as they are: each as going over keeps a note it
/// passes over. Each as it is kept.
pub(in crate::services::agent::merope) async fn keep_notes<
    C: ConnectionTrait + sea_orm::TransactionTrait,
>(
    db: &C,
    owner: i32,
    present: &crate::services::agent::memory::unified::Audience,
    last: chrono::DateTime<chrono::FixedOffset>,
    notes: Vec<crate::services::agent::memory::unified::MemoryRecord>,
) -> anyhow::Result<Vec<String>> {
    use crate::services::agent::memory::unified;
    let mut updates = super::super::super::chat_remember::ChatMemoryUpdates::default();
    let mut settled = Vec::new();
    for note in notes {
        settled.push(vec![note.id]);
        updates
            .updates
            .push(super::super::super::chat_remember::ChatMemoryUpdate {
                standing: Default::default(),
                fact: Some(super::super::super::ingest::compact_summary(&note.content)),
                supersedes: Vec::new(),
                evidence: note.evidence,
                concepts: unified::clean_concepts(note.concepts),
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
