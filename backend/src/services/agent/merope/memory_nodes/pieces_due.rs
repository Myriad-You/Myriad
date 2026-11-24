//! Spans and entries another end forgot, until what rests on them can be
//! looked at.
//!
//! A Mac that only puts a forgotten mark in place of what a piece came from
//! writes it again as its slot's next version (its row is `<slot>@<writer>`,
//! the next `<slot>@<writer>~<n>`) and forgets the row: nothing said is gone,
//! and what rests on it stays. **A slot is gone when no version of it
//! stands**, whatever the words of one that does: those of the forgotten one
//! went as its forget came, so they are not compared.
//!
//! The forget goes ahead of everything else (`myriad_nodes::wire`), so the
//! next version may come batches after. **It is looked at when everything its
//! writer wrote up to it is here and no more of it is on its way**
//! (`her_log::rounds`): the version was written before the forget. (An older
//! build of the Mac wrote it right after: the round's end covers that.) The
//! row itself is a stub, its words gone, as its forget comes.
//!
//! **Until then it is kept as due** ([`Due`], `forgotten::PIECE_DUE`): by its
//! id, across links and restarts. Which forget it waits for is in her log.
//! The words of an intent forgotten meanwhile say which others carry them on
//! only while they are here: the ones they say are kept with it
//! (`forgotten::INTENT_DUE`). A row her log does not get (left out as its
//! reader may not hold it) is a gap nothing fills: what waits past it waits
//! on, and what rests on it stays (the count is on her state).

use std::collections::{BTreeMap, BTreeSet};

use myriad_persona_log::{Row, Vector, kinds::ROW};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::Value;

use super::{Error, heads};
use crate::services::agent::memory::{forgotten, meaning::digest};
use crate::services::her_log::{rounds::Rounds, store};

/// The slot and its writer, of a Mac's row's id (the core's
/// `nodes.resting_on` reads it alike). An id too long is a digest of the
/// whole, from which none is read; one ending in `@` and digits has no
/// writer.
fn slot(id: &str) -> Option<&str> {
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let (at, written) = id.rsplit_once('@')?;
    let writer = match written.rsplit_once('~') {
        Some((writer, n)) if digits(n) => writer,
        _ => written,
    };
    (!at.is_empty() && !writer.is_empty() && !digits(writer))
        .then(|| &id[..at.len() + 1 + writer.len()])
}

/// What the versions of a piece share: its slot and writer, or its id.
fn key(id: &str) -> &str {
    slot(id).unwrap_or(id)
}

/// Whether grounds name the piece `id` (its row or its slot).
fn names(id: &str) -> impl Iterator<Item = String> {
    let slot = slot(id).and_then(|slot| Some(slot.rsplit_once('@')?.0));
    let ids: Vec<String> = std::iter::once(id)
        .chain(slot)
        .map(str::to_string)
        .collect();
    ids.into_iter()
        .flat_map(|id| ["span", "entry"].map(|kind| format!("{kind}:{id}")))
}

/// The forgets in her log of an audience, and the pieces that stand there.
pub(super) struct Due<'a> {
    audience: &'a str,
    rounds: &'a Rounds,
    held: Vector,
    /// By what they name: who wrote each, its count, who sent it.
    forgets: BTreeMap<String, Vec<(String, u64, String)>>,
    /// Slots (with their writer) and kinds a version of which stands.
    standing: BTreeSet<(String, String)>,
}

impl<'a> Due<'a> {
    pub(super) async fn read<C: ConnectionTrait>(
        db: &C,
        user_id: i32,
        audience: &'a str,
        rounds: &'a Rounds,
        kept: &[heads::KeptRow],
    ) -> Result<Self, Error> {
        let held = store::holdings(db, user_id, audience).await?.vector;
        let mut forgets: BTreeMap<String, Vec<(String, u64, String)>> = BTreeMap::new();
        // One whose body has not come yet is in the log alone.
        let mut standing = BTreeSet::new();
        for found in db
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT envelope, received_from FROM agent_log_rows \
                 WHERE user_id = $1 AND audience = $2 AND NOT forgotten \
                   AND kind IN ('forget', 'node.span', 'node.entry')",
                [user_id.into(), audience.into()],
            ))
            .await?
        {
            let row: Row = serde_json::from_value(found.try_get::<Value>("", "envelope")?)
                .map_err(|_| Error::Refused("a row of her log does not read".into()))?;
            if row.kind != "forget" {
                standing.extend(slot(&row.id).map(|slot| (slot.to_string(), row.kind.clone())));
            } else if let Some(dseq) = row.dseq {
                forgets.entry(row.object.clone()).or_default().push((
                    row.origin.clone(),
                    dseq,
                    found.try_get("", "received_from")?,
                ));
            }
        }
        for kept in kept {
            let row = &kept.row;
            if !row.forgotten && matches!(row.kind.as_str(), "node.span" | "node.entry") {
                standing.extend(slot(&row.id).map(|slot| (slot.to_string(), row.kind.clone())));
            }
        }
        Ok(Self {
            audience,
            rounds,
            held,
            forgets,
            standing,
        })
    }

    /// Whether one of the forgets that name the piece `id` (by its row, or
    /// the node it is on) may be looked at; one no forget in the log names
    /// has nothing to wait for.
    fn ready(&self, id: &str, row: Option<&Row>) -> bool {
        let by_row = self.forgets.get(&format!("{ROW}:{id}"));
        let by_node = row.and_then(|row| self.forgets.get(&row.object));
        let mut forgets = by_row.into_iter().chain(by_node).flatten().peekable();
        forgets.peek().is_none()
            || forgets.any(|(origin, dseq, from)| {
                self.rounds
                    .through(self.audience, &self.held, origin, *dseq, from)
            })
    }

    /// Whether a version of the piece `id` stands.
    fn stands(&self, id: &str, row: Option<&Row>) -> bool {
        slot(id).is_some_and(|slot| {
            self.standing
                .iter()
                .any(|(it, kind)| it == slot && row.is_none_or(|row| *kind == row.kind))
        })
    }

    /// Of `pieces` (forgotten), those looked at now and gone, with the
    /// forgotten versions of them; those still due.
    pub(super) fn sort(&self, pieces: &BTreeSet<String>, rows: &BTreeMap<String, &Row>) -> Sorted {
        let mut sorted = Sorted::default();
        for id in pieces {
            let row = rows.get(id).copied();
            if !self.ready(id, row) {
                sorted.due.insert(id.clone());
            } else if !self.stands(id, row) {
                sorted.gone.insert(id.clone());
            } else {
                sorted.written_again.insert(id.clone());
            }
        }
        sorted
    }
}

/// [`Due::sort`].
#[derive(Debug, Default)]
pub(super) struct Sorted {
    pub gone: BTreeSet<String>,
    pub written_again: BTreeSet<String>,
    pub due: BTreeSet<String>,
}

impl Sorted {
    /// Those due that any of `named` (references) names.
    pub(super) fn due_named(&self, named: &BTreeSet<String>) -> BTreeSet<String> {
        self.due
            .iter()
            .filter(|id| names(id).any(|name| named.contains(&name)))
            .cloned()
            .collect()
    }
}

/// Of `pieces`, those kept as due.
pub(super) async fn among<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    pieces: &BTreeSet<String>,
) -> Result<BTreeSet<String>, Error> {
    let marks: Vec<String> = pieces.iter().map(|id| digest(id)).collect();
    let due = forgotten::among(db, user_id, forgotten::PIECE_DUE, &marks).await?;
    Ok(pieces
        .iter()
        .filter(|id| due.contains(&digest(id)))
        .cloned()
        .collect())
}

/// Keeps `pieces` as due.
pub(super) async fn keep<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    pieces: &BTreeSet<String>,
) -> Result<(), Error> {
    let marks: Vec<String> = pieces.iter().map(|id| digest(id)).collect();
    Ok(forgotten::keep(db, user_id, forgotten::PIECE_DUE, &marks).await?)
}

fn intent_mark(piece: &str, intent: &str) -> String {
    digest(&format!("{}\n{intent}", key(piece)))
}

/// Keeps `intents` (rows) as resting on the piece `piece`, due.
pub(super) async fn keep_resting<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    piece: &str,
    intents: &BTreeSet<String>,
) -> Result<(), Error> {
    let marks: Vec<String> = intents
        .iter()
        .map(|intent| intent_mark(piece, intent))
        .collect();
    Ok(forgotten::keep(db, user_id, forgotten::INTENT_DUE, &marks).await?)
}

/// `sorted`'s are looked at: of `intents` (rows), those kept as resting on
/// one that is gone. Nothing of what was looked at is kept as due after.
pub(super) async fn looked_at<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    sorted: &Sorted,
    intents: &[&Row],
) -> Result<BTreeSet<String>, Error> {
    let done: Vec<&String> = sorted.gone.iter().chain(&sorted.written_again).collect();
    let pieces: Vec<String> = done.iter().map(|id| digest(id)).collect();
    if forgotten::among(db, user_id, forgotten::PIECE_DUE, &pieces)
        .await?
        .is_empty()
    {
        return Ok(BTreeSet::new());
    }
    let marks: BTreeMap<String, (&str, &str)> = done
        .iter()
        .flat_map(|id| {
            intents
                .iter()
                .map(|row| (intent_mark(id, &row.id), (id.as_str(), row.id.as_str())))
        })
        .collect();
    let all: Vec<String> = marks.keys().cloned().collect();
    let kept = forgotten::among(db, user_id, forgotten::INTENT_DUE, &all).await?;
    forgotten::let_go(db, user_id, forgotten::PIECE_DUE, &pieces).await?;
    forgotten::let_go(db, user_id, forgotten::INTENT_DUE, &all).await?;
    let gone: BTreeSet<&str> = sorted.gone.iter().map(|id| key(id)).collect();
    Ok(marks
        .into_iter()
        .filter(|(mark, (piece, _))| kept.contains(mark) && gone.contains(key(piece)))
        .map(|(_, (_, intent))| intent.to_string())
        .collect())
}

/// How many are due, over everyone; null when it could not be read.
pub(crate) async fn count(db: &sea_orm::DatabaseConnection) -> Value {
    let found = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) AS n FROM agent_forgotten WHERE what = $1",
            [forgotten::PIECE_DUE.into()],
        ))
        .await;
    match found {
        Ok(Some(row)) => row.try_get::<i64>("", "n").map_or(Value::Null, Value::from),
        _ => Value::Null,
    }
}

/// Whether anything of `user_id`'s is due.
pub(super) async fn any<C: ConnectionTrait>(db: &C, user_id: i32) -> Result<bool, Error> {
    Ok(db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 AS one FROM agent_forgotten WHERE user_id = $1 AND what = $2 LIMIT 1",
            [user_id.into(), forgotten::PIECE_DUE.into()],
        ))
        .await?
        .is_some())
}
