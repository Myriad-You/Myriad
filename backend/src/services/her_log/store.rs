//! Her log as the server keeps it: one append-only log per audience, in
//! `agent_log_rows`, with payloads sealed beside it and the ledger of what
//! was forgotten.
//!
//! The rules are the core's and only theirs: a batch from another side goes
//! through `myriad_nodes::wire::take` (the log's `admit` with the server as
//! the side that holds it, then each body's checks; a clock too far ahead
//! refuses it whole), then `merge` with the part of this log it touches;
//! what the merge adds is written, what it turns into stubs loses its
//! payload, and each new ledger entry deletes the keys it reaches. What a
//! side lacks, and which bodies go with those rows, is
//! `myriad_nodes::wire::send` for that side (`myriad_persona_log::holder`:
//! a partner's computer never gets a group's rows or anyone else's private
//! talk), which sends every forget first. Nothing here decides a row's
//! shape, how two logs meet, or which body may travel.
//!
//! The server keeps shared rows only (a local row never leaves its side),
//! so every row has its place in its audience: `(audience, origin, dseq)`.
//! A body it keeps is the canonical bytes the wire checked, so every side
//! that holds it holds the same bytes.

use std::collections::{BTreeMap, BTreeSet};

use myriad_nodes::wire::{self, Batch, SendInput, TakeInput, Withheld};
use myriad_persona_log::{
    Entry, Holder, Holdings, MergeInput, Row, Vector,
    kinds::Rule,
    merge,
    row::{Audience, Author, LOG_VERSION, Replicate},
    vector,
};
use myriad_persona_sync::{Rev, canonical::canonical_bytes};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, TransactionTrait,
};
use serde::Serialize;
use serde_json::Value;

use super::{Error, NODE, keys};

fn rows_of(results: Vec<QueryResult>) -> Result<Vec<Row>, Error> {
    results
        .into_iter()
        .map(|result| {
            let envelope: serde_json::Value = result.try_get("", "envelope")?;
            serde_json::from_value(envelope)
                .map_err(|_| Error::Refused("a kept row does not read as a row".into()))
        })
        .collect()
}

/// Every row of `audience`.
pub async fn rows<C: ConnectionTrait>(db: &C, audience: &str) -> Result<Vec<Row>, Error> {
    rows_of(
        db.query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT envelope FROM agent_log_rows WHERE audience = $1",
            [audience.into()],
        ))
        .await?,
    )
}

/// What the server holds of `audience`.
pub async fn holdings<C: ConnectionTrait>(db: &C, audience: &str) -> Result<Holdings, Error> {
    Ok(vector::holdings(&rows(db, audience).await?)?)
}

/// The ledger of `audience`.
pub async fn ledger<C: ConnectionTrait>(db: &C, audience: &str) -> Result<Vec<Entry>, Error> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT audience, target, at FROM agent_log_ledger WHERE audience = $1 \
         ORDER BY target",
        [audience.into()],
    ))
    .await?
    .into_iter()
    .map(|row| {
        let at: String = row.try_get("", "at")?;
        Ok(Entry {
            audience: row.try_get("", "audience")?,
            target: row.try_get("", "target")?,
            at: at
                .parse::<Rev>()
                .map_err(|_| Error::Refused("a kept ledger entry has no clock".into()))?,
        })
    })
    .collect()
}

/// The audience of each of `objects` the server holds rows of, in any
/// audience: an object never changes audience.
async fn known<C: ConnectionTrait>(
    db: &C,
    objects: &[String],
) -> Result<BTreeMap<String, String>, Error> {
    if objects.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut known = BTreeMap::new();
    for row in db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT object, audience FROM agent_log_rows WHERE object = ANY($1)",
            [objects.to_vec().into()],
        ))
        .await?
    {
        known.insert(row.try_get("", "object")?, row.try_get("", "audience")?);
    }
    Ok(known)
}

/// For each of `origins`, the highest epoch of its her rows held in
/// `audience`: a lower one arriving from it is refused (`epoch_back`).
async fn epochs<C: ConnectionTrait>(
    db: &C,
    audience: &str,
    origins: &[String],
) -> Result<BTreeMap<String, u64>, Error> {
    if origins.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut held = BTreeMap::new();
    for row in db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT origin, MAX((envelope->>'epoch')::BIGINT) AS epoch FROM agent_log_rows \
             WHERE audience = $1 AND origin = ANY($2) AND envelope->>'epoch' IS NOT NULL \
             GROUP BY origin",
            [audience.into(), origins.to_vec().into()],
        ))
        .await?
    {
        let epoch: i64 = row.try_get("", "epoch")?;
        held.insert(row.try_get("", "origin")?, epoch as u64);
    }
    Ok(held)
}

/// The part of `audience`'s log a merge of `incoming` needs: rows sharing
/// an id or a place with it, and every row a forget in it reaches.
async fn related<C: ConnectionTrait>(
    db: &C,
    audience: &str,
    incoming: &[Row],
) -> Result<Vec<Row>, Error> {
    let ids: Vec<String> = incoming.iter().map(|row| row.id.clone()).collect();
    let origins: Vec<String> = incoming.iter().map(|row| row.origin.clone()).collect();
    let dseqs: Vec<i64> = incoming
        .iter()
        .filter_map(|row| row.dseq)
        .map(|dseq| dseq as i64)
        .collect();
    let mut objects = Vec::new();
    let mut subjects = Vec::new();
    for row in incoming.iter().filter(|row| row.rule() == Rule::Forget) {
        match row
            .object
            .strip_prefix(myriad_persona_log::kinds::SUBJECT)
            .and_then(|rest| rest.strip_prefix(':'))
        {
            Some(subject) => subjects.push(subject.to_string()),
            None => objects.push(row.object.clone()),
        }
    }
    // A superset is fine: rows the batch does not touch merge as themselves.
    rows_of(
        db.query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT envelope FROM agent_log_rows WHERE id = ANY($1) \
             OR (audience = $2 AND ((origin = ANY($3) AND dseq = ANY($4)) \
                 OR object = ANY($5) OR subject = ANY($6)))",
            [
                ids.into(),
                audience.into(),
                origins.into(),
                dseqs.into(),
                objects.into(),
                subjects.into(),
            ],
        ))
        .await?,
    )
}

/// Merges of one audience happen one at a time.
async fn lock<C: ConnectionTrait>(db: &C, audience: &str) -> Result<(), Error> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended('myriad-her-log:' || $1, 0))",
        [audience.into()],
    ))
    .await?;
    Ok(())
}

/// One row not taken, and why: an admit reason (`envelope`, `local`,
/// `audience_changed`, `not_a_member`, `not_held`), `another_audience`
/// when it is not in the batch's audience, or a clash (`same_id_differs`,
/// `same_place`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotTaken {
    pub id: String,
    pub reason: String,
}

/// What a batch did to the log.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Taken {
    /// Rows that were new here.
    pub added: Vec<String>,
    /// Rows already here that are stubs now.
    pub stubbed: Vec<String>,
    pub duplicates: u64,
    pub not_taken: Vec<NotTaken>,
    /// Rows taken (or asked for again) without their body, and why
    /// (`myriad_nodes::wire::Why`).
    pub withheld: Vec<Withheld>,
    /// Of those, the rows whose body may yet come: to ask the sender for
    /// again, at most once per link session (`log.vector` `ask`).
    pub ask: Vec<String>,
}

fn reason_name(value: impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Takes `batch` from `from` (a device id, a file's origin, or [`NODE`] for
/// the server's own rows) as `here` (the server, [`super::server`]), at
/// the server's time `now`. `asked` is what the server asked `from` for
/// again this link session: those rows, when they come back, give only
/// their body.
pub async fn take(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    from: &str,
    here: &Holder,
    batch: Batch,
    asked: &BTreeSet<String>,
    now: i64,
) -> Result<Taken, Error> {
    let txn = db.begin().await?;
    let taken = take_in(&txn, root, from, here, batch, asked, now).await?;
    txn.commit().await?;
    Ok(taken)
}

/// The rows of `audience` among `ids` the server holds, as it holds them.
async fn held<C: ConnectionTrait>(
    db: &C,
    audience: &str,
    ids: Vec<String>,
) -> Result<Vec<Row>, Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    rows_of(
        db.query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT envelope FROM agent_log_rows WHERE audience = $1 AND id = ANY($2)",
            [audience.into(), ids.into()],
        ))
        .await?,
    )
}

/// Seals `data` as the payload of `row`, unless it has one already.
async fn keep_payload<C: ConnectionTrait>(
    txn: &C,
    root: &dyn keys::Root,
    row: &Row,
    data: &[u8],
) -> Result<(), Error> {
    let sealed = keys::seal_payload(
        txn,
        root,
        &row.audience,
        &row.object,
        row.subject.as_deref(),
        &row.id,
        data,
    )
    .await?;
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_log_payloads (row_id, sealed, created_at) VALUES ($1, $2, $3) \
         ON CONFLICT (row_id) DO NOTHING",
        [
            row.id.clone().into(),
            sealed.into(),
            crate::clock::now().into(),
        ],
    ))
    .await?;
    Ok(())
}

/// [`take`] inside the caller's transaction.
async fn take_in<C: ConnectionTrait>(
    txn: &C,
    root: &dyn keys::Root,
    from: &str,
    here: &Holder,
    batch: Batch,
    asked: &BTreeSet<String>,
    now: i64,
) -> Result<Taken, Error> {
    let Batch {
        audience,
        rows,
        payloads,
        mut withheld,
    } = batch;
    let audience = audience.as_str();
    Audience::parse(audience)?;
    lock(txn, audience).await?;
    let mut taken = Taken::default();
    let (rows, elsewhere): (Vec<Row>, Vec<Row>) =
        rows.into_iter().partition(|row| row.audience == audience);
    taken
        .not_taken
        .extend(elsewhere.into_iter().map(|row| NotTaken {
            id: row.id,
            reason: "another_audience".into(),
        }));
    let ids: BTreeSet<&str> = rows.iter().map(|row| row.id.as_str()).collect();
    withheld.retain(|one| ids.contains(one.id.as_str()));
    let again: Vec<String> = ids
        .iter()
        .filter(|id| asked.contains(**id))
        .map(|id| id.to_string())
        .collect();
    let objects: Vec<String> = rows.iter().map(|row| row.object.clone()).collect();
    let origins: Vec<String> = rows
        .iter()
        .map(|row| row.origin.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let before = ledger(txn, audience).await?;
    let again = held(txn, audience, again).await?;
    let wired = wire::take(TakeInput {
        batch: Batch {
            audience: audience.to_string(),
            rows,
            payloads,
            withheld,
        },
        holder: here.clone(),
        now,
        known: known(txn, &objects).await?,
        // Who is in a group is not known here yet: not checked.
        members: BTreeMap::new(),
        ledger: before.clone(),
        epochs: BTreeMap::from([(audience.to_string(), epochs(txn, audience, &origins).await?)]),
        held: again.clone(),
    })?;
    let admitted = wired.admitted;
    if let Some(ahead) = admitted.clock_ahead {
        return Err(Error::ClockAhead {
            ahead_ms: ahead.ahead_ms,
            server_time: now,
        });
    }
    taken
        .not_taken
        .extend(admitted.refused.iter().map(|refusal| NotTaken {
            id: refusal.id.clone(),
            reason: reason_name(refusal.reason),
        }));
    let accepted: BTreeSet<String> = admitted.accepted.iter().map(|row| row.id.clone()).collect();
    let related_rows = related(txn, audience, &admitted.accepted).await?;
    let merged = merge(MergeInput {
        log: related_rows,
        incoming: admitted.accepted,
    });
    taken
        .not_taken
        .extend(merged.conflicts.iter().map(|conflict| NotTaken {
            id: conflict.id.clone(),
            reason: reason_name(conflict.clash),
        }));
    let by_id: BTreeMap<&str, &Row> = merged
        .log
        .iter()
        .map(|row| (row.id.as_str(), row))
        .collect();
    let body = |row: &Row| {
        wired
            .bodies
            .get(&row.id)
            .filter(|_| row.payload.is_some() && !row.forgotten)
            .map(canonical_bytes)
    };
    let mut newest: Option<&Rev> = None;
    for id in &merged.added {
        let row = by_id[id.as_str()];
        insert(txn, row, from).await?;
        newest = newest.max(Some(&row.hlc));
        if let Some(data) = body(row) {
            keep_payload(txn, root, row, &data).await?;
        }
    }
    // A row asked for again comes back as the server holds it: only its
    // body is new.
    for row in again.iter().filter(|row| !accepted.contains(&row.id)) {
        if let Some(data) = body(row) {
            keep_payload(txn, root, row, &data).await?;
        }
    }
    for id in &merged.stubbed {
        let row = by_id[id.as_str()];
        txn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_log_rows SET envelope = $2, forgotten = true WHERE id = $1",
            [row.id.clone().into(), row.to_value().into()],
        ))
        .await?;
        txn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM agent_log_payloads WHERE row_id = $1",
            [row.id.clone().into()],
        ))
        .await?;
    }
    let had: BTreeSet<(&str, &str, String)> = before
        .iter()
        .map(|entry| {
            (
                entry.audience.as_str(),
                entry.target.as_str(),
                entry.at.to_string(),
            )
        })
        .collect();
    for entry in &admitted.ledger {
        let at = entry.at.to_string();
        if had.contains(&(entry.audience.as_str(), entry.target.as_str(), at.clone())) {
            continue;
        }
        txn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_log_ledger (audience, target, at, recorded_at) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (audience, target) DO UPDATE SET at = EXCLUDED.at",
            [
                entry.audience.clone().into(),
                entry.target.clone().into(),
                at.into(),
                crate::clock::now().into(),
            ],
        ))
        .await?;
        keys::forget(txn, &entry.audience, &entry.target).await?;
    }
    if let Some(seen) = newest {
        observe(txn, seen).await?;
    }
    taken.added = merged.added;
    taken.stubbed = merged.stubbed;
    taken.duplicates = merged.duplicates;
    taken.withheld = wired.withheld;
    taken.ask = wired.ask;
    Ok(taken)
}

async fn insert<C: ConnectionTrait>(db: &C, row: &Row, from: &str) -> Result<(), Error> {
    let dseq = row
        .dseq
        .filter(|_| row.replicate == Replicate::Shared)
        .ok_or_else(|| Error::Refused("the server keeps shared rows only".into()))?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_log_rows (id, audience, origin, dseq, hlc, object, subject, kind, \
         forgotten, envelope, received_from, received_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        [
            row.id.clone().into(),
            row.audience.clone().into(),
            row.origin.clone().into(),
            (dseq as i64).into(),
            row.hlc.to_string().into(),
            row.object.clone().into(),
            row.subject.clone().into(),
            row.kind.clone().into(),
            row.forgotten.into(),
            row.to_value().into(),
            from.into(),
            crate::clock::now().into(),
        ],
    ))
    .await?;
    Ok(())
}

/// What a side that is `to`, holding `theirs` of `audience`, lacks, forgets
/// first, in `log.rows` batches (`myriad_nodes::wire::send`): nothing when
/// it may not hold the audience (`not_held`); then the rows it asks again
/// for (`ask`) with their bodies. A body goes only when the wire's checks
/// pass; the others go as rows, each batch saying why.
pub async fn send<C: ConnectionTrait>(
    db: &C,
    root: &dyn keys::Root,
    audience: &str,
    theirs: &Vector,
    ask: Vec<String>,
    to: &Holder,
) -> Result<wire::Sent, Error> {
    let rows = if to.holds(audience) {
        rows(db, audience).await?
    } else {
        Vec::new()
    };
    // The bodies the batches may carry: what it lacks, what it asks for
    // again, and every node's created row (it says whose the node is, so a
    // node that is someone else's in hers stays off the wire).
    let lacking: BTreeSet<String> = vector::answer(&rows, theirs)?
        .into_iter()
        .map(|row| row.id)
        .collect();
    let asked: BTreeSet<&str> = ask.iter().map(String::as_str).collect();
    let wanted: Vec<&Row> = rows
        .iter()
        .filter(|row| row.payload.is_some() && !row.forgotten)
        .filter(|row| {
            lacking.contains(&row.id)
                || asked.contains(row.id.as_str())
                || row.kind == "node.created"
        })
        .collect();
    let bodies = bodies(db, root, audience, &wanted).await?;
    Ok(wire::send(SendInput {
        audience: audience.to_string(),
        rows,
        bodies,
        theirs: theirs.clone(),
        ask,
        holder: to.clone(),
    })?)
}

/// The payloads of `rows`, opened and read as JSON, by row id. One whose key
/// is gone, that does not open or that does not read, is left out: its row
/// goes without it (`no_body`).
async fn bodies<C: ConnectionTrait>(
    db: &C,
    root: &dyn keys::Root,
    audience: &str,
    rows: &[&Row],
) -> Result<BTreeMap<String, Value>, Error> {
    let mut out = BTreeMap::new();
    if rows.is_empty() {
        return Ok(out);
    }
    let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    let mut sealed: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for result in db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT row_id, sealed FROM agent_log_payloads WHERE row_id = ANY($1)",
            [ids.into()],
        ))
        .await?
    {
        sealed.insert(result.try_get("", "row_id")?, result.try_get("", "sealed")?);
    }
    for row in rows {
        let Some(sealed) = sealed.get(&row.id) else {
            continue;
        };
        let opened = match keys::open_payload(db, root, audience, &row.object, &row.id, sealed)
            .await
        {
            Ok(opened) => opened,
            // One body that does not open goes without it (`no_body`); the
            // rest still go.
            Err(Error::Key) => {
                tracing::warn!(row = %row.id, "[her-log] a kept body does not open; its row goes without it");
                None
            }
            Err(error) => return Err(error),
        };
        if let Some(body) = opened.and_then(|data| serde_json::from_slice::<Value>(&data).ok()) {
            out.insert(row.id.clone(), body);
        }
    }
    Ok(out)
}

/// The server's clock after seeing `seen` (so its own rows sort after
/// what it took).
async fn observe<C: ConnectionTrait>(db: &C, seen: &Rev) -> Result<(), Error> {
    let now = super::now();
    let last = last_clock(db).await?;
    let next = myriad_persona_log::clock::observe(last.as_ref(), seen, now, 0, NODE)?;
    store_clock(db, &next).await
}

async fn last_clock<C: ConnectionTrait>(db: &C) -> Result<Option<Rev>, Error> {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT last FROM agent_log_clock WHERE node = $1 FOR UPDATE",
        [NODE.into()],
    ))
    .await?
    .map(|row| {
        let last: String = row.try_get("", "last")?;
        last.parse::<Rev>()
            .map_err(|_| Error::Refused("the server's kept clock does not read".into()))
    })
    .transpose()
}

async fn store_clock<C: ConnectionTrait>(db: &C, rev: &Rev) -> Result<(), Error> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_log_clock (node, last) VALUES ($1, $2) \
         ON CONFLICT (node) DO UPDATE SET last = EXCLUDED.last",
        [NODE.into(), rev.to_string().into()],
    ))
    .await?;
    Ok(())
}

/// The server's next revision: for a row it writes, or a grant it issues.
pub async fn next_rev<C: ConnectionTrait>(db: &C) -> Result<Rev, Error> {
    let now = super::now();
    let last = last_clock(db).await?;
    let next = myriad_persona_log::clock::next(last.as_ref(), now, 0, NODE)?;
    store_clock(db, &next).await?;
    Ok(next)
}

/// The server's next count in `audience`, past every row of its own there.
async fn next_dseq<C: ConnectionTrait>(db: &C, audience: &str) -> Result<u64, Error> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_log_counters (audience, dseq) \
             VALUES ($1, COALESCE((SELECT MAX(dseq) FROM agent_log_rows \
                                   WHERE audience = $1 AND origin = $2), 0) + 1) \
             ON CONFLICT (audience) DO UPDATE SET dseq = agent_log_counters.dseq + 1 \
             RETURNING dseq",
            [audience.into(), NODE.into()],
        ))
        .await?
        .ok_or_else(|| Error::Refused("no count".into()))?;
    Ok(row.try_get::<i64>("", "dseq")?.max(1) as u64)
}

/// The owner, on the server, has her forget `target` (an object, or
/// `subject:<id>`) in `audience`: an owner's `forget` row of the server's,
/// taken like any other. Every side gets it before anything else it lacks.
pub async fn forget(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    here: &Holder,
    audience: &str,
    target: &str,
) -> Result<Row, Error> {
    // The count, the clock and the row in one transaction: a forget that is
    // not kept leaves no hole in the server's count.
    let txn = db.begin().await?;
    lock(&txn, audience).await?;
    let hlc = next_rev(&txn).await?;
    let dseq = next_dseq(&txn, audience).await?;
    let row = Row {
        version: LOG_VERSION,
        id: format!("site_{}", uuid::Uuid::new_v4().simple()),
        origin: NODE.to_string(),
        author: Author::Owner,
        epoch: None,
        dseq: Some(dseq),
        hlc,
        audience: audience.to_string(),
        subject: None,
        object: target.to_string(),
        kind: "forget".to_string(),
        payload: None,
        replicate: Replicate::Shared,
        forgotten: false,
    };
    row.check()?;
    let now = super::now();
    let taken = take_in(
        &txn,
        root,
        NODE,
        here,
        Batch {
            audience: audience.to_string(),
            rows: vec![row.clone()],
            payloads: Vec::new(),
            withheld: Vec::new(),
        },
        &BTreeSet::new(),
        now,
    )
    .await?;
    if !taken.added.contains(&row.id) {
        return Err(Error::Refused("the forget was not taken".into()));
    }
    txn.commit().await?;
    Ok(row)
}

/// The owner's account for her log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub user_id: i32,
    /// Where it is kept; empty until the region is decided (problem 13).
    pub region: Option<String>,
    pub started_at: chrono::DateTime<chrono::FixedOffset>,
    pub stopped_at: Option<chrono::DateTime<chrono::FixedOffset>>,
}

impl Account {
    pub fn on(&self) -> bool {
        self.stopped_at.is_none()
    }
}

pub async fn account<C: ConnectionTrait>(db: &C, owner: i32) -> Result<Option<Account>, Error> {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT user_id, region, started_at, stopped_at FROM agent_log_accounts \
         WHERE user_id = $1",
        [owner.into()],
    ))
    .await?
    .map(|row| {
        Ok(Account {
            user_id: row.try_get("", "user_id")?,
            region: row.try_get("", "region")?,
            started_at: row.try_get("", "started_at")?,
            stopped_at: row.try_get("", "stopped_at")?,
        })
    })
    .transpose()
}

/// The owner turned syncing her log on: the account is made (its region
/// empty) or started again.
pub async fn start<C: ConnectionTrait>(db: &C, owner: i32) -> Result<Account, Error> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_log_accounts (user_id, region, started_at) VALUES ($1, NULL, $2) \
         ON CONFLICT (user_id) DO UPDATE SET stopped_at = NULL",
        [owner.into(), crate::clock::now().into()],
    ))
    .await?;
    account(db, owner)
        .await?
        .ok_or_else(|| Error::Refused("no account".into()))
}

/// The owner turned it off: the retention period counts from now.
pub async fn stop<C: ConnectionTrait>(db: &C, owner: i32) -> Result<(), Error> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_log_accounts SET stopped_at = $2 \
         WHERE user_id = $1 AND stopped_at IS NULL",
        [owner.into(), crate::clock::now().into()],
    ))
    .await?;
    Ok(())
}
