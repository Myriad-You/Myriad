//! What she held of someone before they asked her not to keep something:
//! what she would come back to with them, what still stings, what is on her
//! mind about them, what she took up herself to say or do, and what she
//! makes of them and of her days with them, which she goes on from each time
//! she makes it anew; and of each group they talk with her in, what she
//! would come back to there and what she makes of it, as far as it is said
//! of them. Once the request is written, each is judged by what they asked,
//! and what is of it is closed; what they told her to do stays theirs.
//!
//! A job for each place written with the request
//! (`memory_jobs::Payload::AskedOver`), tried until it is done: only what is
//! told to go is closed, and what is not told stays as it is, to be asked
//! about again. Until it is done, she brings up none of these to them of
//! herself ([`done_first`]), or there ([`done_first_in`]); answering what
//! they say, she may still speak from them. Someone who asks nothing new is
//! asked nothing about.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use futures::StreamExt;
use myriad_nodes::body::{IntentStatus, Origin};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};

use crate::models::entities::agent_memories;
use crate::services::agent::memory::unified::{self, Audience};
use crate::services::agent::merope::memory_jobs::{self, Failure};
use crate::services::agent::merope::memory_nodes::{
    home_intents, intent_store, take_back_check, write,
};
use crate::services::agent::merope::on_mind::OnMind;
use crate::services::agent::merope::store::memory_jobs::Own;
use crate::services::agent::merope::{bits, chat_days, on_mind, sore, store, threads};

pub(super) static CLOSED: AtomicU64 = AtomicU64::new(0);
pub(super) static HELD_BACK: AtomicU64 = AtomicU64::new(0);

/// Asked about together, at most, and so many asks at once.
const AT_ONCE: usize = 100;
const ASKS: usize = 4;
/// Of what she makes of them, looked through at most, newest first.
const MADE: u64 = 1000;

/// What she holds of a group that is held to it.
pub(in crate::services::agent::merope) const IN_GROUPS: [&str; 4] = [
    threads::SOURCE,
    bits::LANDS_SOURCE,
    bits::SOURCE,
    bits::DAY_SOURCE,
];

/// One thing she held of them.
enum Old {
    /// A memory of hers that goes.
    Row(Box<agent_memories::Model>),
    /// What that one says she made of them before.
    Before(Box<agent_memories::Model>, Vec<&'static str>),
    OnMind(OnMind),
    /// Her intent, by its id.
    Intent(String),
}

impl Old {
    /// What it is known by, saying `text` ([`mark`]).
    fn mark(&self, text: &str) -> String {
        match self {
            Self::Row(row) => mark(&["", &row.source, text]),
            Self::Before(row, keys) => mark(&["", &row.source, &keys.join(" "), text]),
            Self::OnMind(_) => mark(&["", "on_mind", text]),
            Self::Intent(_) => mark(&["", "intent", text]),
        }
    }
}

/// What one thing she held is known by on its job: what it says, where it was
/// held, and as whom it is judged; none of it in words.
fn mark(of: &[&str]) -> String {
    let mut mark = memory_jobs::key(of);
    mark.truncate(16);
    mark
}

/// `held` but for what was told to stay on a try before, and what each of
/// the rest is known by.
fn not_told_to_stay<T>(
    own: Option<Own<'_>>,
    held: &mut Vec<T>,
    mark: impl Fn(&T) -> String,
) -> Vec<String> {
    let stayed = own.map_or(&[][..], |own| own.stayed());
    let mut marks = Vec::new();
    held.retain(|one| {
        let mark = mark(one);
        let asks = !stayed.contains(&mark);
        if asks {
            marks.push(mark);
        }
        asks
    });
    marks
}

/// Of `marks`, those told to stay are noted, before any of the others is
/// closed.
async fn stay(
    db: &DatabaseConnection,
    own: Option<Own<'_>>,
    marks: Vec<String>,
    told: &super::Told,
) -> Result<(), Failure> {
    let Some(own) = own else {
        return Ok(());
    };
    let stays = marks
        .into_iter()
        .enumerate()
        .filter(|(index, _)| !told.out.contains(index) && !told.untold.contains(index))
        .map(|(_, mark)| mark)
        .collect();
    own.stays(db, stays).await.map_err(|_| Failure::Storage)
}

/// What she keeps of `user_id` in private from `source`, newest first.
async fn of_theirs(
    db: &DatabaseConnection,
    user_id: i32,
    source: &str,
) -> Result<Vec<agent_memories::Model>, Failure> {
    let alone = Audience::private(user_id).venue();
    unified::venue_source_rows(db, Some(user_id), &alone, source, MADE)
        .await
        .map_err(|_| Failure::Storage)
}

/// What `user_id` asked her not to keep, held to what she already held of
/// them in private: what is of it closed. An `Err`: not all of it could be
/// told or closed now, to be tried again; what was closed stays closed. Told
/// by `until`, or not; with `gives_up`, what is not told is let be. On its
/// `nth` try, the `nth` part is asked about first: what is not reached in
/// the time of one try is in another's, and what was told to stay is not
/// asked about again (noted on `own`, the job it is of).
pub(in crate::services::agent::merope) async fn let_go(
    db: &DatabaseConnection,
    user_id: i32,
    until: Instant,
    gives_up: bool,
    nth: usize,
    own: Option<Own<'_>>,
) -> Result<(), Failure> {
    let storage = |_| Failure::Storage;
    let mut held: Vec<(Old, String)> = Vec::new();
    let mut rows = of_theirs(db, user_id, threads::SOURCE).await?;
    rows.reverse();
    held.extend(rows.into_iter().map(row));
    // Wherever they were born.
    let sores = unified::latest_of(db, user_id, sore::SOURCE, MADE)
        .await
        .map_err(storage)?;
    held.extend(sores.into_iter().map(row));
    let mind = on_mind::kept(db).await.map_err(|_| Failure::Storage)?;
    if let Some(mind) = mind.filter(|mind| mind.about == Some(user_id)) {
        let text = format!("{} {}", mind.what, mind.so_far);
        held.push((Old::OnMind(mind), text));
    }
    for (_, intent) in waiting(db, user_id).await? {
        // A thread's goes with its thread; what they told her to do is theirs.
        if intent.origin != Some(Origin::Partner) && !intent.id.starts_with(home_intents::THREAD) {
            held.push((Old::Intent(intent.id), intent.text));
        }
    }
    for source in [
        bits::US_SOURCE,
        bits::LANDS_SOURCE,
        bits::SOURCE,
        chat_days::SOURCE,
    ] {
        let rows = of_theirs(db, user_id, source).await?;
        for row in &rows {
            for (keys, before) in bits::us_before(row) {
                held.push((Old::Before(Box::new(row.clone()), keys), before));
            }
        }
        held.extend(rows.into_iter().map(self::row));
    }
    let marks = not_told_to_stay(own, &mut held, |(old, text)| old.mark(text));
    let texts: Vec<String> = held.iter().map(|(_, text)| text.clone()).collect();
    let told = judged(db, user_id, None, &texts, until, nth).await;
    stay(db, own, marks, &told).await?;
    let of_it = &told.out;
    CLOSED.fetch_add(of_it.len() as u64, Ordering::Relaxed);
    let mut closed = Vec::new();
    let mut doomed = BTreeSet::new();
    let mut off_mind = None;
    let mut before = Vec::new();
    for (_, (old, _)) in held
        .into_iter()
        .enumerate()
        .filter(|(index, _)| of_it.contains(index))
    {
        match old {
            Old::Row(row) => {
                doomed.insert(format!("{}{}", home_intents::THREAD, row.id));
                closed.push(*row);
            }
            Old::Before(row, keys) => before.push((*row, keys)),
            Old::OnMind(mind) => off_mind = Some(mind),
            Old::Intent(id) => {
                doomed.insert(id);
            }
        }
    }
    for (row, keys) in before {
        if !closed
            .iter()
            .any(|one: &agent_memories::Model| one.id == row.id)
        {
            (bits::withhold_us_before(db, &row, &keys).await).map_err(storage)?;
        }
    }
    if !closed.is_empty() {
        close(db, &closed).await.map_err(storage)?;
        home_intents::in_step(db, user_id).await;
    }
    if let Some(mind) = off_mind {
        // What came onto it since was not judged.
        if on_mind::kept(db)
            .await
            .map_err(|_| Failure::Storage)?
            .as_ref()
            == Some(&mind)
        {
            on_mind::let_go(db).await;
        }
    }
    // Her own (a thread's that did not step with it, too): closed with her
    // pen, or, with none now, later.
    for (talk, intent) in waiting(db, user_id).await? {
        if !doomed.contains(&intent.id) {
            continue;
        }
        let pen = crate::services::her_log::pen::think().ok_or(Failure::Storage)?;
        let row = intent_store::settle_row(&intent, IntentStatus::Dropped);
        write::write(db, user_id, &talk, &[row], Some(&pen))
            .await
            .map_err(|_| Failure::Storage)?;
    }
    told.let_be(gives_up)
}

/// [`let_go`], of what she holds of the group `venue` (as sessions keep it):
/// what is said there of `user_id`, by the name they go by lately (as she
/// knows them, when no one was lately). Nothing of a group they do not talk
/// with her in, nor of someone without a name: whose it is cannot be told.
pub(in crate::services::agent::merope) async fn let_go_in(
    db: &DatabaseConnection,
    user_id: i32,
    venue: &str,
    until: Instant,
    gives_up: bool,
    nth: usize,
    own: Option<Own<'_>>,
) -> Result<(), Failure> {
    if !talks_in(db, user_id, venue)
        .await
        .map_err(|_| Failure::Storage)?
    {
        return Ok(());
    }
    let here = crate::services::channel_group::names_here(db, venue)
        .await
        .remove(&user_id);
    let name = match here {
        Some(name) => name,
        None => name_of(db, user_id).await.map_err(|_| Failure::Storage)?,
    };
    let name = name.trim();
    if name.is_empty() {
        tracing::warn!(
            user_id,
            "[Merope] a group's old talk is left as it is: no name to judge by"
        );
        return Ok(());
    }
    let stored = Audience::group(venue, 0).venue();
    let mut held: Vec<(agent_memories::Model, String)> = Vec::new();
    for source in IN_GROUPS {
        let rows = unified::venue_source_rows(db, None, &stored, source, MADE)
            .await
            .map_err(|_| Failure::Storage)?;
        held.extend(rows.into_iter().map(|row| {
            let text = says(&row);
            (row, text)
        }));
    }
    let marks = not_told_to_stay(own, &mut held, |(row, text)| {
        mark(&[name, &row.source, text])
    });
    let texts: Vec<String> = held.iter().map(|(_, text)| text.clone()).collect();
    let told = judged(db, user_id, Some(name), &texts, until, nth).await;
    stay(db, own, marks, &told).await?;
    let of_it = &told.out;
    CLOSED.fetch_add(of_it.len() as u64, Ordering::Relaxed);
    let rows: Vec<agent_memories::Model> = held
        .into_iter()
        .enumerate()
        .filter(|(index, _)| of_it.contains(index))
        .map(|(_, (row, _))| row)
        .collect();
    close(db, &rows).await.map_err(|_| Failure::Storage)?;
    told.let_be(gives_up)
}

/// What `user_id` goes by on the site.
async fn name_of(db: &DatabaseConnection, user_id: i32) -> Result<String, sea_orm::DbErr> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT COALESCE(NULLIF(btrim(display_name), ''), username, '') AS name \
             FROM users WHERE id = $1",
            [user_id.into()],
        ))
        .await?;
    row.map_or(Ok(String::new()), |row| row.try_get("", "name"))
}

/// Whether `user_id` talks with her in the group `venue`.
async fn talks_in(
    db: &DatabaseConnection,
    user_id: i32,
    venue: &str,
) -> Result<bool, sea_orm::DbErr> {
    Ok(db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 AS one FROM agent_sessions WHERE user_id = $1 \
             AND context->>'mode' = 'chat' AND context->>'venue' = $2 LIMIT 1",
            [user_id.into(), venue.into()],
        ))
        .await?
        .is_some())
}

/// What a memory of hers says, as it is judged: with what it is about.
fn says(row: &agent_memories::Model) -> String {
    if let Some(thread) = threads::text_of(row) {
        return thread;
    }
    match bits::bit_of(row) {
        Some((handle, how)) => format!("{handle}: {how}"),
        None => row.content.clone(),
    }
}

fn row(row: agent_memories::Model) -> (Old, String) {
    let text = says(&row);
    (Old::Row(Box::new(row)), text)
}

/// What is told of `items`: which are of what `user_id` asked (said of
/// `who` in them, where others are too).
async fn judged(
    db: &DatabaseConnection,
    user_id: i32,
    who: Option<&str>,
    items: &[String],
    until: Instant,
    nth: usize,
) -> super::Told {
    let parts = items
        .chunks(AT_ONCE)
        .enumerate()
        .map(|(nth, part)| judged_part(db, user_id, who, nth, part, until));
    let parts = from_nth(parts.collect(), nth);
    let told: Vec<(usize, super::Told)> =
        futures::stream::iter(parts).buffered(ASKS).collect().await;
    let mut all = super::Told::default();
    for (first, told) in told {
        all.out
            .extend(told.out.into_iter().map(|index| first + index));
        all.untold
            .extend(told.untold.into_iter().map(|index| first + index));
        // One that could not be asked is asked again, whatever the rest.
        if all.failure.as_ref().is_none_or(Failure::untold) {
            all.failure = told.failure.or(all.failure);
        }
    }
    all
}

/// `parts`, all of them, from the `nth` on and round.
fn from_nth<T>(mut parts: Vec<T>, nth: usize) -> Vec<T> {
    let first = nth % parts.len().max(1);
    parts.rotate_left(first);
    parts
}

/// The `nth` part of what is [`judged`], and where it begins.
async fn judged_part(
    db: &DatabaseConnection,
    user_id: i32,
    who: Option<&str>,
    nth: usize,
    part: &[String],
    until: Instant,
) -> (usize, super::Told) {
    (
        nth * AT_ONCE,
        super::told_of(db, user_id, who, part, until).await,
    )
}

/// The intents waiting in `user_id`'s private talk, each with its audience.
async fn waiting(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Vec<(String, intent_store::Held)>, Failure> {
    let mut waiting = Vec::new();
    for (whose, talk) in intent_store::waiting_audiences(db)
        .await
        .map_err(|_| Failure::Storage)?
    {
        if whose != user_id {
            continue;
        }
        let held = intent_store::held(db, user_id, &talk)
            .await
            .map_err(|_| Failure::Storage)?;
        waiting.extend(
            held.into_iter()
                .filter(|held| held.waiting())
                .map(|held| (talk.clone(), held)),
        );
    }
    Ok(waiting)
}

/// Closes `rows`: taken back, with nothing in their place, so what rests on
/// them on her nodes is not read either.
async fn close(
    db: &DatabaseConnection,
    rows: &[agent_memories::Model],
) -> Result<(), sea_orm::DbErr> {
    if rows.is_empty() {
        return Ok(());
    }
    let tx = db.begin().await?;
    for row in rows {
        match row.user_id {
            Some(user_id) => {
                let ids = [row.id.clone()];
                unified::retire(&tx, user_id, &ids, "superseded").await?;
                take_back_check::hold(&tx, user_id, &ids, None)
                    .await
                    .map_err(|error| sea_orm::DbErr::Custom(error.to_string()))?;
            }
            None => {
                unified::retire_unowned(&tx, &row.venue, &row.id, "superseded").await?;
            }
        }
    }
    tx.commit().await
}

/// Before something she held of `user_id` is said to them: whether what they
/// asked lately has been held to it. While it has not, nothing of it is to
/// be said yet.
pub(in crate::services::agent::merope) async fn done_first(
    db: &DatabaseConnection,
    user_id: i32,
) -> bool {
    let done = matches!(waits(db, Some(user_id), None).await, Ok(false));
    if !done {
        HELD_BACK.fetch_add(1, Ordering::Relaxed);
        tracing::info!(
            user_id,
            "[Merope] not brought up yet: what they asked not to keep is not held to it"
        );
    }
    done
}

/// [`done_first`], of what is said in the group `venue`, whoever of it asked.
pub(in crate::services::agent::merope) async fn done_first_in(
    db: &DatabaseConnection,
    venue: &str,
) -> bool {
    let done = matches!(waits(db, None, Some(venue)).await, Ok(false));
    if !done {
        HELD_BACK.fetch_add(1, Ordering::Relaxed);
    }
    done
}

/// Whether something asked (by `user_id`, or with none by anyone) is still
/// to be held to what she held at `venue` (none: in private).
async fn waits(
    db: &DatabaseConnection,
    user_id: Option<i32>,
    venue: Option<&str>,
) -> Result<bool, sea_orm::DbErr> {
    Ok(db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 AS one FROM runtime_registry WHERE namespace = $1 \
             AND payload->'data'->'AskedOver' IS NOT NULL \
             AND ($2::int IS NULL OR (payload->>'owner')::int = $2) \
             AND (payload->'data'->'AskedOver'->>'venue') IS NOT DISTINCT FROM $3 LIMIT 1",
            [
                store::memory_jobs::ASKED_NAMESPACE.into(),
                user_id.into(),
                venue.map(str::to_string).into(),
            ],
        ))
        .await?
        .is_some())
}

#[cfg(test)]
mod tests {
    /// What is not reached in the time of one try is first in another's, and
    /// none is missed in any.
    #[test]
    fn each_try_begins_at_another_part() {
        let tries: Vec<Vec<u8>> = (0..4)
            .map(|nth| super::from_nth(vec![0, 1, 2], nth))
            .collect();
        assert_eq!(tries, [[0, 1, 2], [1, 2, 0], [2, 0, 1], [0, 1, 2]]);
        assert!(super::from_nth(Vec::<u8>::new(), 3).is_empty());
    }
}
