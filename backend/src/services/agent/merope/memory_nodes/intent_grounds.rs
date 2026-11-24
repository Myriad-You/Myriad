//! Her intents resting on what is forgotten. Which of them do is the core's
//! (`myriad_nodes::resting_on`); here they are found in the rows kept, by
//! each path something is forgotten on this site, and forgotten by a forget
//! of their own each.
//!
//! - **Asked first, stubbed after, deleted last.** The words of what is
//!   forgotten say which other rows carry them, so the core is asked while
//!   the bodies are still here; a row her log made a stub of already is
//!   asked about with the body kept here ([`Round`]).
//! - **What they rest on is forgotten first** (a span's, an entry's, a whole
//!   node's forget before the intents'), so another end taking these rows in
//!   reads them in that order.
//! - **What comes late is asked about again** ([`Round::taken`]): an intent
//!   taken in, or whose body is, against every span and entry forgotten here
//!   before, and the memories and lines of theirs its intent names that were
//!   ([`crate::services::agent::memory::forgotten`]). Only the intents it is
//!   a version of, and those its words are carried on as: one here from
//!   before that nothing came of stays as it is.
//!
//! A node taken back from sync (撤回同步) is no forgetting, though its rows
//! are stubs here as if it were: its home holds them still, so what rests on
//! them stays ([`forgotten_by`]).
//!
//! One another end forgot is looked at once everything it wrote up to that
//! forget is here: a version it wrote in its place stands then, and what rests
//! on it stays ([`super::pieces_due`]).
//!
//! The site is no node's home and writes no `retract.home`
//! (`her_log::store`), so each is a plain forget of its row.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use myriad_nodes::{
    body::MEMORY_REF,
    resting_on::{RestingOnInput, resting_on},
};
use myriad_persona_log::{
    Row,
    forget::{self as ledger, Entry, Kind},
};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::Value;

use super::forget::{Forgets, forget_one, row_node};
use super::pieces_due::{self, Due};
use super::{Error, heads, logged};
use crate::services::agent::memory::forgotten;
use crate::services::her_log::{rounds::Rounds, store};

const INTENT: &str = "node.intent";
const MESSAGE_REF: &str = "message:";

/// What is being forgotten: spans and entries by their rows, memories by
/// their ids, the partner's lines by theirs (`agent_messages.id`) or as
/// grounds name them.
#[derive(Debug, Default)]
pub(super) struct Gone {
    pub pieces: BTreeSet<String>,
    pub memories: Vec<String>,
    pub lines: Vec<String>,
}

impl Gone {
    /// The spans and entries among the rows `going` of `kept`, and those
    /// forgotten before that they were written in place of.
    pub(super) fn of(kept: &[heads::KeptRow], going: &BTreeSet<String>) -> Self {
        let pieces = |of: &dyn Fn(&Row) -> bool| -> BTreeSet<String> {
            kept.iter()
                .filter(|kept| is_piece(&kept.row.kind) && of(&kept.row))
                .map(|kept| kept.row.id.clone())
                .collect()
        };
        Self {
            pieces: pieces_due::replaced(
                pieces(&|row| going.contains(&row.id)),
                &pieces(&|row| row.forgotten),
            ),
            ..Self::default()
        }
    }

    /// The memories and lines, as grounds name them.
    fn names(&self) -> BTreeSet<String> {
        let named = |prefix: &str, id: &String| match id.strip_prefix(prefix) {
            Some(_) => id.clone(),
            None => format!("{prefix}{id}"),
        };
        self.memories
            .iter()
            .map(|id| named(MEMORY_REF, id))
            .chain(self.lines.iter().map(|id| named(MESSAGE_REF, id)))
            .collect()
    }

    fn is_empty(&self) -> bool {
        self.pieces.is_empty() && self.memories.is_empty() && self.lines.is_empty()
    }
}

fn is_piece(kind: &str) -> bool {
    matches!(kind, "node.span" | "node.entry")
}

/// The intents naming any of `refs`, by the account and audience they are
/// in.
pub(super) async fn citing<C: ConnectionTrait>(
    db: &C,
    refs: &[String],
) -> Result<Vec<(i32, String)>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT r.user_id, r.audience FROM agent_node_rows r \
             CROSS JOIN LATERAL jsonb_array_elements_text( \
               CASE WHEN r.body = '' THEN '[]'::jsonb \
                    ELSE COALESCE((r.body::jsonb)->'because', '[]'::jsonb) END) AS cited(ref) \
             WHERE r.kind = $2 AND cited.ref = ANY($1)",
            [refs.to_vec().into(), INTENT.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| Ok((row.try_get("", "user_id")?, row.try_get("", "audience")?)))
        .collect()
}

/// The intent rows of `user_id`'s `audience` (`kept`, every row of it) to
/// forget with `gone`, in the log's order.
pub(super) async fn resting<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    kept: &[heads::KeptRow],
    gone: &Gone,
) -> Result<Vec<Row>, Error> {
    asked(db, user_id, audience, kept, heads::bodies(kept)?, gone).await
}

async fn asked<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    kept: &[heads::KeptRow],
    bodies: BTreeMap<String, Value>,
    gone: &Gone,
) -> Result<Vec<Row>, Error> {
    if gone.is_empty()
        || !kept
            .iter()
            .any(|kept| kept.row.kind == INTENT && !kept.row.forgotten)
    {
        return Ok(Vec::new());
    }
    let mut rows: Vec<Row> = kept.iter().map(|kept| kept.row.clone()).collect();
    rows.extend(bare(db, user_id, audience).await?);
    let resting = resting_on(RestingOnInput {
        rows: rows.clone(),
        bodies,
        ledger: Vec::new(),
        gone: gone.pieces.iter().cloned().collect(),
        messages: gone.lines.clone(),
        memories: gone.memories.clone(),
        loose: Vec::new(),
    })?;
    let resting: BTreeSet<String> = resting.intents.into_iter().collect();
    rows.retain(|row| resting.contains(&row.id));
    myriad_persona_log::row::sort(&mut rows);
    Ok(rows)
}

/// The intents of `user_id`'s `audience` whose body is still to come: in her
/// log, and not among her nodes' rows until it has ([`logged::take_in`]).
async fn bare<C: ConnectionTrait>(db: &C, user_id: i32, audience: &str) -> Result<Vec<Row>, Error> {
    if logged::into_log(db, user_id, audience).await?.is_none() {
        return Ok(Vec::new());
    }
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT l.envelope FROM agent_log_rows l \
         WHERE l.user_id = $1 AND l.audience = $2 AND l.kind = $3 AND NOT l.forgotten \
           AND NOT EXISTS (SELECT 1 FROM agent_node_rows n \
                           WHERE n.user_id = l.user_id AND n.row_id = l.id)",
        [user_id.into(), audience.into(), INTENT.into()],
    ))
    .await?
    .iter()
    .map(|row| {
        serde_json::from_value(row.try_get::<Value>("", "envelope")?)
            .map_err(|_| Error::Refused("a row of her log does not read".into()))
    })
    .collect()
}

/// Forgets `intents` ([`resting`], of `kept`) by a forget of each one's row
/// and, of an intent nothing is left of, what the site kept of the
/// instruction. The rows are made stubs by the caller, with the rest of what
/// goes.
pub(super) async fn forget<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    kept: &[heads::KeptRow],
    intents: &[Row],
) -> Result<(), Error> {
    for row in intents {
        forget_one(db, user_id, audience, &row_node(row), Forgets::Row(&row.id)).await?;
    }
    let going: BTreeSet<&str> = intents.iter().map(|row| row.id.as_str()).collect();
    let named = |goes: bool| -> BTreeSet<String> {
        kept.iter()
            .filter(|kept| kept.row.kind == INTENT && going.contains(kept.row.id.as_str()) == goes)
            .filter_map(|kept| body_of(kept)?["intent"].as_str().map(str::to_string))
            .collect()
    };
    let left = named(false);
    let ids: Vec<String> = named(true).difference(&left).cloned().collect();
    if !ids.is_empty() {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM agent_node_instructions \
             WHERE user_id = $1 AND audience = $2 AND intent_id = ANY($3)",
            [user_id.into(), audience.into(), ids.into()],
        ))
        .await?;
    }
    Ok(())
}

/// Whether `ledger` forgets `row`. A row a node taken back from sync holds is
/// forgotten only by a forget that names it or its node: any other leaves it
/// on its home.
fn forgotten_by(ledger: &[Entry], row: &Row) -> bool {
    let of_this = |entry: &Entry| entry.audience == row.audience;
    let taken_back = ledger.iter().any(|entry| {
        of_this(entry)
            && entry.is_retract()
            && (entry.target == row.object || entry.takes.contains(&row.id))
    });
    if !taken_back {
        return ledger::forgets(ledger, row);
    }
    let by_row = format!("{}:{}", myriad_persona_log::kinds::ROW, row.id);
    ledger.iter().any(|entry| {
        of_this(entry)
            && entry.kind == Kind::Forget
            && (entry.target == row.object || entry.target == by_row)
    })
}

/// Her intents resting on the spans and entries among `going` (rows of
/// `kept`, forgotten already by forgets of their own or of their node) and
/// not among them forgotten too, and put among them, to be made stubs.
pub(super) async fn forget_with<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    kept: &[heads::KeptRow],
    going: &mut Vec<Row>,
) -> Result<(), Error> {
    let ids: BTreeSet<String> = going.iter().map(|row| row.id.clone()).collect();
    let mut intents = resting(db, user_id, audience, kept, &Gone::of(kept, &ids)).await?;
    intents.retain(|row| !ids.contains(&row.id));
    forget(db, user_id, audience, kept, &intents).await?;
    going.extend(intents);
    Ok(())
}

/// The intents of `kept` the fold has forgotten already (with a node) that
/// still say words here and name any of `gone`: to be made stubs.
pub(super) fn forgotten_naming(
    kept: &[heads::KeptRow],
    folded: &myriad_nodes::Folded,
    gone: &Gone,
) -> Vec<Row> {
    let names = gone.names();
    kept.iter()
        .filter(|kept| kept.row.kind == INTENT && folded.stubbed.contains(&kept.row.id))
        .filter(|kept| {
            body_of(kept).is_some_and(|body| {
                strings(&body["because"])
                    .iter()
                    .any(|named| names.contains(named))
            })
        })
        .map(|kept| kept.row.clone())
        .collect()
}

/// What the site kept of the instructions given in `refs` (lines) goes.
pub(super) async fn instructions_of<C: ConnectionTrait>(
    db: &C,
    refs: &[String],
) -> Result<(), Error> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM agent_node_instructions WHERE message_ref = ANY($1)",
        [refs.to_vec().into()],
    ))
    .await?;
    Ok(())
}

fn body_of(kept: &heads::KeptRow) -> Option<Value> {
    serde_json::from_str(kept.body.as_deref()?).ok()
}

/// Of `named` (references), the partner's lines `user_id` was made to
/// forget.
pub(super) async fn lines_gone<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    named: &[String],
) -> Result<Vec<String>, Error> {
    let ids: Vec<String> = named
        .iter()
        .filter_map(|reference| reference.strip_prefix(MESSAGE_REF).map(str::to_string))
        .collect();
    Ok(forgotten::among(db, user_id, forgotten::MESSAGE, &ids)
        .await?
        .into_iter()
        .map(|id| format!("{MESSAGE_REF}{id}"))
        .collect())
}

/// One taking in of her log's rows of an audience ([`logged::take_in`]).
pub(super) struct Round {
    /// What the log made stubs of and is whole here: kind and body, by row.
    before: HashMap<String, (String, Value)>,
}

impl Round {
    /// Read before they are made stubs here.
    pub(super) async fn begin<C: ConnectionTrait>(
        db: &C,
        user_id: i32,
        audience: &str,
    ) -> Result<Self, Error> {
        let mut before = HashMap::new();
        for row in db
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT n.row_id, n.kind, n.body FROM agent_node_rows n \
                 JOIN agent_log_rows l ON l.user_id = n.user_id AND l.id = n.row_id \
                 WHERE n.user_id = $1 AND n.audience = $2 AND l.forgotten AND n.body <> '' \
                   AND n.kind IN ('node.intent', 'node.span', 'node.entry')",
                [user_id.into(), audience.into()],
            ))
            .await?
        {
            let body: String = row.try_get("", "body")?;
            let body = serde_json::from_str(&body)
                .map_err(|_| Error::Refused("a kept body does not read".into()))?;
            before.insert(row.try_get("", "row_id")?, (row.try_get("", "kind")?, body));
        }
        Ok(Self { before })
    }

    /// The rows her log's forgets leave to go here too, once `came` (row
    /// ids) are kept: what rests on a span or an entry forgotten elsewhere,
    /// and what came of an intent resting on anything forgotten before. Each
    /// forgotten; the rows, to be made stubs.
    pub(super) async fn taken<C: ConnectionTrait>(
        self,
        db: &C,
        user_id: i32,
        audience: &str,
        came: &BTreeSet<String>,
        rounds: &Rounds,
    ) -> Result<Vec<Row>, Error> {
        let kept = heads::rows_of(db, user_id, audience).await?;
        let rows: Vec<Row> = kept.iter().map(|kept| kept.row.clone()).collect();
        let ledger = ledger::joined(
            &store::ledger(db, user_id, audience).await?,
            &ledger::ledger(&rows),
        );
        let gone = |row: &Row| row.forgotten && is_piece(&row.kind) && forgotten_by(&ledger, row);
        let by_id: BTreeMap<String, &Row> = rows.iter().map(|row| (row.id.clone(), row)).collect();
        // Each span and entry forgotten, the rows a forget names that never
        // came among them.
        let by_row = format!("{}:", myriad_persona_log::kinds::ROW);
        let mut all: BTreeSet<String> = rows
            .iter()
            .filter(|row| gone(row))
            .map(|row| row.id.clone())
            .collect();
        let mut looking: BTreeSet<String> = all
            .iter()
            .filter(|id| self.before.contains_key(*id) || came.contains(*id))
            .cloned()
            .collect();
        all.extend(
            ledger
                .iter()
                .filter(|entry| entry.kind == Kind::Forget && entry.audience == audience)
                .filter_map(|entry| entry.target.strip_prefix(&by_row))
                .filter(|id| !by_id.contains_key(*id))
                .map(str::to_string),
        );
        looking.extend(pieces_due::among(db, user_id, &all).await?);
        let looking = pieces_due::replaced(looking, &all);
        let late = Late::of(&kept, came);
        if looking.is_empty() && late.named.is_empty() {
            return Ok(Vec::new());
        }
        let due = Due::read(db, user_id, audience, rounds, &kept).await?;
        let sorted = due.sort(&looking, &by_id);
        let bodies_here = heads::bodies(&kept)?;
        let mut bodies = bodies_here.clone();
        for (id, (kind, body)) in &self.before {
            if kind == INTENT {
                bodies.insert(id.clone(), body.clone());
            }
        }
        let of = |pieces: BTreeSet<String>| Gone {
            pieces,
            ..Gone::default()
        };
        // What the words of one forgotten now say of those still due: they
        // are not here to say it when those are looked at.
        if !sorted.due.is_empty() {
            pieces_due::keep(db, user_id, &sorted.due).await?;
            let mut by_piece = Vec::new();
            for pieces in std::iter::once(sorted.due.clone())
                .chain(sorted.due.iter().map(|id| BTreeSet::from([id.clone()])))
            {
                let gone = of(pieces);
                let here = asked(db, user_id, audience, &kept, bodies_here.clone(), &gone).await?;
                let here: BTreeSet<String> = here.into_iter().map(|row| row.id).collect();
                let said = asked(db, user_id, audience, &kept, bodies.clone(), &gone).await?;
                let said: BTreeSet<String> = said
                    .into_iter()
                    .map(|row| row.id)
                    .filter(|id| !here.contains(id))
                    .collect();
                // All of them first: mostly none of them says any.
                if by_piece.is_empty() && said.is_empty() {
                    break;
                }
                by_piece.push((gone.pieces, said));
            }
            for (pieces, intents) in by_piece.into_iter().skip(1) {
                for piece in pieces {
                    pieces_due::keep_resting(db, user_id, &piece, &intents).await?;
                }
            }
        }
        let intents: Vec<&Row> = rows.iter().filter(|row| row.kind == INTENT).collect();
        let resting = pieces_due::looked_at(db, user_id, &sorted, &intents).await?;
        let mut going = asked(
            db,
            user_id,
            audience,
            &kept,
            bodies.clone(),
            &of(sorted.gone.clone()),
        )
        .await?;
        for row in intents {
            if !row.forgotten
                && resting.contains(&row.id)
                && !going.iter().any(|it| it.id == row.id)
            {
                going.push(row.clone());
            }
        }
        myriad_persona_log::row::sort(&mut going);
        // Late: the intents something came of.
        if !late.named.is_empty() {
            let before = due.sort(&all, &by_id);
            let named = late
                .rows(&kept)
                .flat_map(|(_, body)| strings(&body["because"]))
                .collect();
            pieces_due::keep(db, user_id, &before.due_named(&named)).await?;
            let before = before.gone;
            let gone = late.gone(db, user_id, &kept, before).await?;
            // Asked with what came and without: the words of an intent that
            // came may be carried on elsewhere, and only they say so.
            let before: Vec<heads::KeptRow> = kept
                .iter()
                .filter(|kept| kept.row.kind != INTENT || !came.contains(&kept.row.id))
                .cloned()
                .collect();
            let bodies_before = bodies
                .iter()
                .filter(|(id, _)| !came.contains(*id))
                .map(|(id, body)| (id.clone(), body.clone()))
                .collect();
            let as_before = asked(db, user_id, audience, &before, bodies_before, &gone)
                .await?
                .into_iter()
                .map(|row| row.id)
                .collect();
            let more = asked(db, user_id, audience, &kept, bodies, &gone).await?;
            let seen: BTreeSet<String> = going.iter().map(|row| row.id.clone()).collect();
            going.extend(
                late.only(&kept, more, &as_before)
                    .into_iter()
                    .filter(|row| !seen.contains(&row.id)),
            );
        }
        forget(db, user_id, audience, &kept, &going).await?;
        Ok(going)
    }
}

/// The intents something came of, by node and id, and every row of theirs.
struct Late {
    named: BTreeSet<(String, String)>,
}

impl Late {
    fn of(kept: &[heads::KeptRow], came: &BTreeSet<String>) -> Self {
        Self {
            named: kept
                .iter()
                .filter(|kept| came.contains(&kept.row.id))
                .filter_map(|kept| Self::name(kept, &body_of(kept)?))
                .collect(),
        }
    }

    fn name(kept: &heads::KeptRow, body: &Value) -> Option<(String, String)> {
        (kept.row.kind == INTENT && !kept.row.forgotten).then_some(())?;
        Some((
            kept.row.object.clone(),
            body["intent"].as_str()?.to_string(),
        ))
    }

    /// Each whole row of theirs, with its body.
    fn rows<'a>(
        &'a self,
        kept: &'a [heads::KeptRow],
    ) -> impl Iterator<Item = (&'a Row, Value)> + 'a {
        kept.iter().filter_map(move |kept| {
            let body = body_of(kept)?;
            let name = Self::name(kept, &body)?;
            self.named.contains(&name).then_some((&kept.row, body))
        })
    }

    /// Everything forgotten before that they may rest on: `pieces`, and the
    /// memories and lines they name that were forgotten.
    async fn gone<C: ConnectionTrait>(
        &self,
        db: &C,
        user_id: i32,
        kept: &[heads::KeptRow],
        pieces: BTreeSet<String>,
    ) -> Result<Gone, Error> {
        let named: Vec<String> = self
            .rows(kept)
            .flat_map(|(_, body)| strings(&body["because"]))
            .collect();
        let memories = named
            .iter()
            .filter_map(|reference| reference.strip_prefix(MEMORY_REF).map(str::to_string))
            .collect();
        let memories = super::known::named(db, memories).await?;
        Ok(Gone {
            pieces,
            memories: memories
                .into_iter()
                .filter(|(_, named)| named.taken_back && named.forgets)
                .map(|(id, _)| id)
                .collect(),
            lines: lines_gone(db, user_id, &named).await?,
        })
    }

    /// Of `rows` ([`resting`]), theirs: their whole rows, and those still to
    /// come that one of these was written on; and any that is not
    /// `as_before`, resting on it whether anything came or not.
    fn only(
        &self,
        kept: &[heads::KeptRow],
        rows: Vec<Row>,
        as_before: &BTreeSet<String>,
    ) -> Vec<Row> {
        let going: BTreeSet<String> = rows.iter().map(|row| row.id.clone()).collect();
        let mut theirs: BTreeSet<String> = BTreeSet::new();
        let mut under: BTreeSet<String> = BTreeSet::new();
        for (row, body) in self.rows(kept) {
            if going.contains(&row.id) {
                under.extend(strings(&body["base"]));
                theirs.insert(row.id.clone());
            }
        }
        let here: BTreeSet<&str> = kept.iter().map(|kept| kept.row.id.as_str()).collect();
        rows.into_iter()
            .filter(|row| {
                theirs.contains(&row.id)
                    || !as_before.contains(&row.id)
                    || (under.contains(&row.id) && !here.contains(row.id.as_str()))
            })
            .collect()
    }
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect()
}
