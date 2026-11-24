//! Spans and entries another end forgot, until what rests on them can be
//! looked at.
//!
//! An end that only puts a forgotten mark in place of what a piece came from
//! writes it again and forgets the row: a Mac's row is `<slot>@<writer>`, the
//! slot's next version `<slot>@<writer>~<n>`, and one it writes in place of
//! another end's row has the slot `p-<that row>` or `n-<that row>`. Nothing
//! said is gone, and what rests on it stays. **A forget takes what rests on
//! the row unless the end that wrote the forget has such a version of its own
//! standing**, of the same kind, whatever its words: those of the forgotten
//! one went as its forget came, so they are not compared. Any other end's
//! forget takes it, whoever marked it meanwhile. Once no version stands any
//! more, what rests on the rows it was written in place of goes with it
//! ([`replaced`]).
//!
//! The forget goes ahead of everything else (`myriad_nodes::wire`), so the
//! version may come batches after. **It is looked at when everything the end
//! that wrote it wrote up to it is here and no more of it is on its way**
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

/// The slot of a Mac's row's id, without its writer (the core's
/// `nodes.resting_on` reads it alike). An id too long is a digest of the
/// whole, from which none is read; one ending in `@` and digits has no
/// writer.
fn slot(id: &str) -> Option<&str> {
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let (slot, written) = id.rsplit_once('@')?;
    let writer = match written.rsplit_once('~') {
        Some((writer, n)) if digits(n) => writer,
        _ => written,
    };
    (!slot.is_empty() && !writer.is_empty() && !digits(writer)).then_some(slot)
}

/// Whether a row in `at` (a slot) is a version of the piece `id`.
fn is_version(at: &str, id: &str) -> bool {
    slot(id) == Some(at)
        || ["p-", "n-"]
            .iter()
            .any(|place| at.strip_prefix(place) == Some(id))
}

/// The versions of its slot before `id`'s, the latest [`BEFORE`] and the
/// first.
fn versions_before(id: &str) -> Vec<String> {
    let Some((first, n)) = id
        .rsplit_once('~')
        .filter(|_| slot(id).is_some())
        .and_then(|(first, n)| Some((first, n.parse::<u32>().ok()?)))
    else {
        return Vec::new();
    };
    let later = (1..n).rev().take(BEFORE).map(|n| format!("{first}~{n}"));
    later.chain([first.to_string()]).collect()
}

/// How many versions before one are looked back at.
const BEFORE: usize = 64;

/// `pieces`, gone, and those they were written in place of, however far back
/// (an earlier version of the slot; a slot of `p-<row>` or `n-<row>`), here or
/// not: an intent names the one that stood as it was written. Not one that
/// still `stands`.
pub(super) fn replaced(
    mut pieces: BTreeSet<String>,
    stands: impl Fn(&str) -> bool,
) -> BTreeSet<String> {
    let mut left: Vec<String> = pieces.iter().cloned().collect();
    while let Some(piece) = left.pop() {
        let Some(at) = slot(&piece) else { continue };
        let rows = ["p-", "n-"]
            .iter()
            .filter_map(|place| at.strip_prefix(place))
            .filter(|row| !row.is_empty())
            .map(str::to_string)
            .chain(versions_before(&piece));
        for row in rows {
            if !stands(&row) && pieces.insert(row.clone()) {
                left.push(row);
            }
        }
    }
    pieces
}

/// Whether grounds name the piece `id` (its row or its slot).
fn names(id: &str) -> impl Iterator<Item = String> {
    let ids: Vec<String> = std::iter::once(id)
        .chain(slot(id))
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
    /// The pieces that stand: whose, in which slot, of which kind.
    standing: BTreeSet<(String, String, String)>,
    /// Their rows.
    whole: BTreeSet<String>,
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
        let mut whole = BTreeSet::new();
        let place = |row: &Row| {
            slot(&row.id).map(|slot| (row.origin.clone(), slot.to_string(), row.kind.clone()))
        };
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
                standing.extend(place(&row));
                whole.insert(row.id);
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
                standing.extend(place(row));
                whole.insert(row.id.clone());
            }
        }
        Ok(Self {
            audience,
            rounds,
            held,
            forgets,
            standing,
            whole,
        })
    }

    /// Whether `origin` has a version of the piece `id` standing.
    fn stands(&self, origin: &str, id: &str, row: Option<&Row>) -> bool {
        self.standing.iter().any(|(whose, at, kind)| {
            whose == origin && row.is_none_or(|row| *kind == row.kind) && is_version(at, id)
        })
    }

    /// `pieces` (forgotten), by what the forgets that name each (by its row,
    /// or the node it is on) come to: gone by one that may be looked at;
    /// still due, when another may not; only written again. One no forget in
    /// the log names is gone.
    pub(super) fn sort(&self, pieces: &BTreeSet<String>, rows: &BTreeMap<String, &Row>) -> Sorted {
        let mut sorted = Sorted::default();
        for id in pieces {
            let row = rows.get(id).copied();
            let by_row = self.forgets.get(&format!("{ROW}:{id}"));
            let by_node = row.and_then(|row| self.forgets.get(&row.object));
            let forgets: Vec<_> = by_row.into_iter().chain(by_node).flatten().collect();
            let (ready, due): (Vec<_>, Vec<_>) =
                forgets.iter().partition(|(origin, dseq, from)| {
                    self.rounds
                        .through(self.audience, &self.held, origin, *dseq, from)
                });
            let goes = |(origin, _, _): &&&(String, u64, String)| !self.stands(origin, id, row);
            if forgets.is_empty() || ready.iter().any(goes) {
                sorted.gone.insert(id.clone());
            } else if !due.is_empty() {
                sorted.due.insert(id.clone());
            } else {
                sorted.written_again.insert(id.clone());
            }
        }
        sorted.gone = replaced(sorted.gone, |id| self.whole.contains(id));
        sorted.due.retain(|id| !sorted.gone.contains(id));
        sorted.written_again.retain(|id| !sorted.gone.contains(id));
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
    digest(&format!("{piece}\n{intent}"))
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
    Ok(marks
        .into_iter()
        .filter(|(mark, (piece, _))| kept.contains(mark) && sorted.gone.contains(*piece))
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
