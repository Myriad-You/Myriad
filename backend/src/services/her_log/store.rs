//! Her log as the server keeps it: one append-only log per account and
//! audience, in `agent_log_rows`, with payloads sealed beside it, the
//! audience's ledger, and the nodes known to be someone else's in hers.
//!
//! The rules are the core's and only theirs: a batch from another side goes
//! through `myriad_nodes::wire::take` (the log's `admit` with the server as
//! the side that holds it, then each body's checks; a clock too far ahead
//! refuses it whole), then `merge` with the part of this log it touches;
//! what the merge adds is written, what it turns into stubs loses its
//! payload, and when the ledger grew, `log.reach` says which other rows it
//! takes away now (a retract's list, a merge's or a split's names). What a
//! side lacks, and which bodies go with those rows, is
//! `myriad_nodes::wire::send` for that side (`myriad_persona_log::holder`:
//! a partner's computer never gets a group's rows or anyone else's private
//! talk; a core older than ABI 2 gets nothing while a row it would refuse
//! is to go), which sends every forget and retract first. Nothing here
//! decides a row's shape, how two logs meet, or which body may travel.
//!
//! Every table is keyed by the account ([`Owner::user`]): node ids are
//! unique only per account and audience, so one account's rows, ledger and
//! keys are never read for another's. The server's side of an account goes
//! by its partner id ([`Owner::partner`]), and so does every side it
//! exchanges rows with (`partner::exchange`, in the core).
//!
//! The server keeps shared rows only (a local row never leaves its side),
//! so every row has its place in its audience: `(audience, origin, dseq)`.
//! A body it keeps is the canonical bytes the wire checked, so every side
//! that holds it holds the same bytes. The server is never a home of a
//! node taken back from sync: it takes no home page and writes no
//! `retract.home`, so every retract is a forget here.

use std::collections::{BTreeMap, BTreeSet};

use myriad_nodes::wire::{self, Batch, SendInput, TakeInput, Withheld};
use myriad_persona_log::{
    Entry, Holder, Holdings, MergeInput, Row, Vector,
    forget::{self, Kind, ReachInput},
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

use super::{Error, NODE, Owner, keys};

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

/// Every row of `user`'s `audience`.
pub async fn rows<C: ConnectionTrait>(
    db: &C,
    user: i32,
    audience: &str,
) -> Result<Vec<Row>, Error> {
    rows_of(
        db.query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT envelope FROM agent_log_rows WHERE user_id = $1 AND audience = $2",
            [user.into(), audience.into()],
        ))
        .await?,
    )
}

/// Every audience `user`'s log holds a row of, sorted.
pub async fn audiences<C: ConnectionTrait>(db: &C, user: i32) -> Result<Vec<String>, Error> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT DISTINCT audience FROM agent_log_rows WHERE user_id = $1 ORDER BY audience",
        [user.into()],
    ))
    .await?
    .into_iter()
    .map(|row| Ok(row.try_get("", "audience")?))
    .collect()
}

/// What the server holds of `user`'s `audience`.
pub async fn holdings<C: ConnectionTrait>(
    db: &C,
    user: i32,
    audience: &str,
) -> Result<Holdings, Error> {
    Ok(vector::holdings(&rows(db, user, audience).await?)?)
}

/// The ledger of `user`'s `audience`, as the core gave it.
pub async fn ledger<C: ConnectionTrait>(
    db: &C,
    user: i32,
    audience: &str,
) -> Result<Vec<Entry>, Error> {
    let Some(row) = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT entries FROM agent_log_ledgers WHERE user_id = $1 AND audience = $2",
            [user.into(), audience.into()],
        ))
        .await?
    else {
        return Ok(Vec::new());
    };
    let entries: Value = row.try_get("", "entries")?;
    serde_json::from_value(entries)
        .map_err(|_| Error::Refused("a kept ledger does not read as a ledger".into()))
}

async fn store_ledger<C: ConnectionTrait>(
    db: &C,
    user: i32,
    audience: &str,
    entries: &[Entry],
) -> Result<(), Error> {
    let entries = serde_json::to_value(entries)
        .map_err(|_| Error::Refused("a ledger does not write".into()))?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_log_ledgers (user_id, audience, entries, updated_at) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (user_id, audience) DO UPDATE \
         SET entries = EXCLUDED.entries, updated_at = EXCLUDED.updated_at",
        [
            user.into(),
            audience.into(),
            entries.into(),
            crate::clock::now().into(),
        ],
    ))
    .await?;
    Ok(())
}

/// The nodes of `user`'s `audience` known to be a person's or a group's in
/// her own (`nodes.wire_take` `elsewhere`, kept), sorted.
pub async fn elsewhere<C: ConnectionTrait>(
    db: &C,
    user: i32,
    audience: &str,
) -> Result<Vec<String>, Error> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT node FROM agent_log_elsewhere WHERE user_id = $1 AND audience = $2 ORDER BY node",
        [user.into(), audience.into()],
    ))
    .await?
    .into_iter()
    .map(|row| Ok(row.try_get("", "node")?))
    .collect()
}

/// The audience of each of `objects` that `user`'s log holds rows of, in
/// any of its audiences: an object never changes audience within one
/// partner's audiences. Another account's rows are another partner's, and
/// their node ids say nothing about this one's.
async fn known<C: ConnectionTrait>(
    db: &C,
    user: i32,
    objects: &[String],
) -> Result<BTreeMap<String, String>, Error> {
    if objects.is_empty() {
        return Ok(BTreeMap::new());
    }
    let mut known = BTreeMap::new();
    for row in db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT object, audience FROM agent_log_rows \
             WHERE user_id = $1 AND object = ANY($2)",
            [user.into(), objects.to_vec().into()],
        ))
        .await?
    {
        known.insert(row.try_get("", "object")?, row.try_get("", "audience")?);
    }
    Ok(known)
}

/// For each of `origins`, the highest epoch of its her rows held in
/// `user`'s `audience`: a lower one arriving from it is refused
/// (`epoch_back`).
async fn epochs<C: ConnectionTrait>(
    db: &C,
    user: i32,
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
             WHERE user_id = $1 AND audience = $2 AND origin = ANY($3) \
               AND envelope->>'epoch' IS NOT NULL \
             GROUP BY origin",
            [user.into(), audience.into(), origins.to_vec().into()],
        ))
        .await?
    {
        let epoch: i64 = row.try_get("", "epoch")?;
        held.insert(row.try_get("", "origin")?, epoch as u64);
    }
    Ok(held)
}

/// The part of `user`'s `audience` a merge of `incoming` needs: rows
/// sharing an id or a place with it, and every row a forget or a retract
/// in it names. What a ledger entry reaches beyond those (a retract's
/// list, a node's other names) is taken away after, by `log.reach`.
async fn related<C: ConnectionTrait>(
    db: &C,
    user: i32,
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
    for row in incoming
        .iter()
        .filter(|row| matches!(row.rule(), Rule::Forget | Rule::Retract))
    {
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
            "SELECT envelope FROM agent_log_rows WHERE user_id = $1 AND (id = ANY($2) \
             OR (audience = $3 AND ((origin = ANY($4) AND dseq = ANY($5)) \
                 OR object = ANY($6) OR subject = ANY($7))))",
            [
                user.into(),
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

/// Merges of one account's audience happen one at a time.
async fn lock<C: ConnectionTrait>(db: &C, user: i32, audience: &str) -> Result<(), Error> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended('myriad-her-log:' || $1 || ':' || $2, 0))",
        [user.to_string().into(), audience.into()],
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
    /// Stubs here that a whole row filled (only a row moved to the
    /// partner's id does, on the server: it is never a home).
    pub filled: Vec<String>,
    pub duplicates: u64,
    pub not_taken: Vec<NotTaken>,
    /// Rows taken (or asked for again) without their body, and why
    /// (`myriad_nodes::wire::Why`).
    pub withheld: Vec<Withheld>,
    /// Of those, the rows whose body may yet come: to ask the sender for
    /// again, at most once per link session (`log.vector` `ask`).
    pub ask: Vec<String>,
    /// Nodes this batch showed to be a person's or a group's in her own
    /// audience: kept, and no body of theirs is kept or sent from now on.
    pub elsewhere: Vec<String>,
}

fn reason_name(value: impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Takes `batch` from `from` (a device id, `file`, or [`NODE`] for the
/// server's own rows) into `owner`'s log, as the server of that account
/// ([`Owner::server`]), at the server's time `now`. The other side goes by
/// the account's partner id: the site gives every device of the account
/// that id, and a device still on another refuses to sync in its own core.
/// `asked` is what the server asked `from` for again this link session:
/// those rows, when they come back, give only their body.
pub async fn take(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    owner: &Owner,
    from: &str,
    batch: Batch,
    asked: &BTreeSet<String>,
    now: i64,
) -> Result<Taken, Error> {
    let txn = db.begin().await?;
    let taken = take_in(&txn, root, owner, from, batch, asked, now).await?;
    txn.commit().await?;
    Ok(taken)
}

/// The rows of `user`'s `audience` among `ids` the server holds, as it
/// holds them.
async fn held<C: ConnectionTrait>(
    db: &C,
    user: i32,
    audience: &str,
    ids: Vec<String>,
) -> Result<Vec<Row>, Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    rows_of(
        db.query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT envelope FROM agent_log_rows \
             WHERE user_id = $1 AND audience = $2 AND id = ANY($3)",
            [user.into(), audience.into(), ids.into()],
        ))
        .await?,
    )
}

/// Seals `data` as the payload of `user`'s `row`, unless it has one already.
async fn keep_payload<C: ConnectionTrait>(
    txn: &C,
    root: &dyn keys::Root,
    user: i32,
    row: &Row,
    data: &[u8],
) -> Result<(), Error> {
    let sealed = keys::seal_payload(
        txn,
        root,
        user,
        &row.audience,
        &row.object,
        row.subject.as_deref(),
        &row.id,
        data,
    )
    .await?;
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_log_payloads (user_id, row_id, sealed, created_at) \
         VALUES ($1, $2, $3, $4) ON CONFLICT (user_id, row_id) DO NOTHING",
        [
            user.into(),
            row.id.clone().into(),
            sealed.into(),
            crate::clock::now().into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Makes `row` (as it stands now: a stub, or whole again) what `user`'s
/// log holds of it, its lookup columns too: a stub filled by a row moved to
/// the partner's id names that id where it named `local-owner`. A stub
/// loses its payload.
async fn restate<C: ConnectionTrait>(txn: &C, user: i32, row: &Row) -> Result<(), Error> {
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_log_rows \
         SET envelope = $3, forgotten = $4, audience = $5, object = $6, subject = $7, kind = $8 \
         WHERE user_id = $1 AND id = $2",
        [
            user.into(),
            row.id.clone().into(),
            row.to_value().into(),
            row.forgotten.into(),
            row.audience.clone().into(),
            row.object.clone().into(),
            row.subject.clone().into(),
            row.kind.clone().into(),
        ],
    ))
    .await?;
    if row.forgotten {
        txn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM agent_log_payloads WHERE user_id = $1 AND row_id = $2",
            [user.into(), row.id.clone().into()],
        ))
        .await?;
    }
    Ok(())
}

/// [`take`] inside the caller's transaction.
async fn take_in<C: ConnectionTrait>(
    txn: &C,
    root: &dyn keys::Root,
    owner: &Owner,
    from: &str,
    batch: Batch,
    asked: &BTreeSet<String>,
    now: i64,
) -> Result<Taken, Error> {
    let user = owner.user;
    let Batch {
        audience,
        rows: incoming,
        payloads,
        mut withheld,
    } = batch;
    let audience = audience.as_str();
    Audience::parse(audience)?;
    lock(txn, user, audience).await?;
    let mut taken = Taken::default();
    let (incoming, astray): (Vec<Row>, Vec<Row>) = incoming
        .into_iter()
        .partition(|row| row.audience == audience);
    taken
        .not_taken
        .extend(astray.into_iter().map(|row| NotTaken {
            id: row.id,
            reason: "another_audience".into(),
        }));
    let ids: BTreeSet<&str> = incoming.iter().map(|row| row.id.as_str()).collect();
    withheld.retain(|one| ids.contains(one.id.as_str()));
    let again: Vec<String> = ids
        .iter()
        .filter(|id| asked.contains(**id))
        .map(|id| id.to_string())
        .collect();
    let objects: Vec<String> = incoming.iter().map(|row| row.object.clone()).collect();
    let origins: Vec<String> = incoming
        .iter()
        .map(|row| row.origin.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let before = ledger(txn, user, audience).await?;
    let again = held(txn, user, audience, again).await?;
    let wired = wire::take(TakeInput {
        batch: Batch {
            audience: audience.to_string(),
            rows: incoming,
            payloads,
            withheld,
        },
        holder: owner.server(),
        peer: owner.partner.clone(),
        now,
        known: known(txn, user, &objects).await?,
        // Who is in a group is not known here yet: not checked.
        members: BTreeMap::new(),
        ledger: Some(before.clone()),
        epochs: BTreeMap::from([(
            audience.to_string(),
            epochs(txn, user, audience, &origins).await?,
        )]),
        held: again.clone(),
        elsewhere: elsewhere(txn, user, audience).await?,
        // The person and group keys the server knows are not listed yet:
        // a sender's word that it kept a node's body back as a person's or
        // a group's holds only for a key in the batch's bodies or the
        // partner, so no ordinary node's body is taken away by it.
        people: Vec::new(),
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
    let related_rows = related(txn, user, audience, &admitted.accepted).await?;
    let merged = merge(MergeInput {
        log: related_rows,
        incoming: admitted.accepted,
        ledger: admitted.ledger.clone(),
        holder: Some(owner.server()),
    })?;
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
        insert(txn, user, row, from).await?;
        newest = newest.max(Some(&row.hlc));
        if let Some(data) = body(row) {
            keep_payload(txn, root, user, row, &data).await?;
        }
    }
    // A row asked for again comes back as the server holds it: only its
    // body is new.
    for row in again.iter().filter(|row| !accepted.contains(&row.id)) {
        if let Some(data) = body(row) {
            keep_payload(txn, root, user, row, &data).await?;
        }
    }
    for id in &merged.filled {
        let row = by_id[id.as_str()];
        restate(txn, user, row).await?;
        if let Some(data) = body(row) {
            keep_payload(txn, root, user, row, &data).await?;
        }
    }
    let mut stubbed: BTreeSet<String> = BTreeSet::new();
    for id in &merged.stubbed {
        restate(txn, user, by_id[id.as_str()]).await?;
        stubbed.insert(id.clone());
    }
    stubbed.extend(apply_ledger(txn, user, audience, &before, &admitted.ledger).await?);
    let mut learnt = wired.elsewhere.clone();
    learnt.sort();
    learnt.dedup();
    if !learnt.is_empty() {
        for node in &learnt {
            txn.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO agent_log_elsewhere (user_id, audience, node, recorded_at) \
                 VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
                [
                    user.into(),
                    audience.into(),
                    node.clone().into(),
                    crate::clock::now().into(),
                ],
            ))
            .await?;
        }
        // Any body of theirs kept before goes.
        txn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM agent_log_payloads p USING agent_log_rows r \
             WHERE p.user_id = r.user_id AND p.row_id = r.id \
               AND r.user_id = $1 AND r.audience = $2 AND r.object = ANY($3)",
            [user.into(), audience.into(), learnt.clone().into()],
        ))
        .await?;
    }
    if let Some(seen) = newest {
        observe(txn, seen).await?;
    }
    taken.added = merged.added;
    taken.stubbed = stubbed.into_iter().collect();
    taken.filled = merged.filled;
    taken.duplicates = merged.duplicates;
    taken.withheld = wired.withheld;
    taken.ask = wired.ask;
    taken.elsewhere = learnt;
    Ok(taken)
}

/// Keeps `after`, `user`'s ledger of `audience` once it grew from `before`,
/// and does what it takes away now: `log.reach` over the audience's whole
/// rows (beyond what a merge saw: a retract's list, a node's other names)
/// makes those stubs, and a forgotten object's key goes, and so does a
/// node's taken back from sync (the server is never its home). The rows it
/// made stubs, sorted.
async fn apply_ledger<C: ConnectionTrait>(
    txn: &C,
    user: i32,
    audience: &str,
    before: &[Entry],
    after: &[Entry],
) -> Result<Vec<String>, Error> {
    let grew: Vec<&Entry> = after
        .iter()
        .filter(|entry| !before.contains(entry))
        .collect();
    if grew.is_empty() && after.len() == before.len() {
        return Ok(Vec::new());
    }
    store_ledger(txn, user, audience, after).await?;
    let whole: Vec<Row> = rows(txn, user, audience)
        .await?
        .into_iter()
        .filter(|row| !row.forgotten)
        .collect();
    let reached = forget::reach(ReachInput {
        rows: whole.clone(),
        ledger: after.to_vec(),
    })?;
    let reached: BTreeSet<String> = reached.stubbed.into_iter().collect();
    let mut stubbed = Vec::new();
    for row in whole.iter().filter(|row| reached.contains(&row.id)) {
        restate(txn, user, &row.stub()).await?;
        stubbed.push(row.id.clone());
    }
    for entry in grew
        .iter()
        .filter(|entry| matches!(entry.kind, Kind::Forget | Kind::Retract))
    {
        keys::forget(txn, user, &entry.audience, &entry.target).await?;
    }
    stubbed.sort();
    Ok(stubbed)
}

async fn insert<C: ConnectionTrait>(db: &C, user: i32, row: &Row, from: &str) -> Result<(), Error> {
    let dseq = row
        .dseq
        .filter(|_| row.replicate == Replicate::Shared)
        .ok_or_else(|| Error::Refused("the server keeps shared rows only".into()))?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_log_rows (user_id, id, audience, origin, dseq, hlc, object, subject, \
         kind, forgotten, envelope, received_from, received_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        [
            user.into(),
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

/// What a side that is `to` (one of `owner`'s), holding `theirs` of
/// `audience`, lacks, forgets and retracts first, in `log.rows` batches
/// (`myriad_nodes::wire::send`): nothing when it may not hold the audience
/// (`not_held`), nothing when its core is older than `peer_abi` 2 and a row
/// it would refuse is to go (`too_old`); then the rows it asks again for
/// (`ask`) with their bodies. A body goes only when the wire's checks pass;
/// the others go as rows, each batch saying why. `peer_abi` is what its
/// link said (2 when its `hello` listed `log2`, [`super::relay::peer_abi`]).
#[allow(clippy::too_many_arguments)]
pub async fn send<C: ConnectionTrait>(
    db: &C,
    root: &dyn keys::Root,
    owner: &Owner,
    audience: &str,
    theirs: &Vector,
    ask: Vec<String>,
    to: &Holder,
    peer_abi: u32,
) -> Result<wire::Sent, Error> {
    let user = owner.user;
    let rows = if to.holds(audience) {
        rows(db, user, audience).await?
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
    let bodies = bodies(db, root, user, audience, &wanted).await?;
    Ok(wire::send(SendInput {
        audience: audience.to_string(),
        rows,
        bodies,
        theirs: theirs.clone(),
        ask,
        holder: to.clone(),
        here: owner.partner.clone(),
        peer_abi,
        ledger: ledger(db, user, audience).await?,
        elsewhere: elsewhere(db, user, audience).await?,
    })?)
}

/// The payloads of `rows`, opened and read as JSON, by row id. One whose key
/// is gone, that does not open or that does not read, is left out: its row
/// goes without it (`no_body`).
async fn bodies<C: ConnectionTrait>(
    db: &C,
    root: &dyn keys::Root,
    user: i32,
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
            "SELECT row_id, sealed FROM agent_log_payloads WHERE user_id = $1 AND row_id = ANY($2)",
            [user.into(), ids.into()],
        ))
        .await?
    {
        sealed.insert(result.try_get("", "row_id")?, result.try_get("", "sealed")?);
    }
    for row in rows {
        let Some(sealed) = sealed.get(&row.id) else {
            continue;
        };
        let opened = match keys::open_payload(
            db,
            root,
            user,
            audience,
            &row.object,
            &row.id,
            sealed,
        )
        .await
        {
            Ok(opened) => opened,
            // One body that does not open goes without it (`no_body`);
            // the rest still go.
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

/// The server's next count in `user`'s `audience`, past every row of its
/// own there.
async fn next_dseq<C: ConnectionTrait>(db: &C, user: i32, audience: &str) -> Result<u64, Error> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_log_counters (user_id, audience, dseq) \
             VALUES ($1, $2, COALESCE((SELECT MAX(dseq) FROM agent_log_rows \
                                       WHERE user_id = $1 AND audience = $2 AND origin = $3), 0) + 1) \
             ON CONFLICT (user_id, audience) DO UPDATE SET dseq = agent_log_counters.dseq + 1 \
             RETURNING dseq",
            [user.into(), audience.into(), NODE.into()],
        ))
        .await?
        .ok_or_else(|| Error::Refused("no count".into()))?;
    Ok(row.try_get::<i64>("", "dseq")?.max(1) as u64)
}

/// The owner, on the server, has her forget `target` (an object, or
/// `subject:<id>`) in `audience` of `owner`'s log: an owner's `forget` row
/// of the server's own. Like every row a side writes, it goes on the ledger
/// by `log.ledger` (with the server as the side, so a forget about the
/// partner under another id is refused), not through the check of rows
/// another side sends; then what the ledger takes away now goes. Every side
/// gets it before anything else it lacks.
pub async fn forget(
    db: &DatabaseConnection,
    owner: &Owner,
    audience: &str,
    target: &str,
) -> Result<Row, Error> {
    Audience::parse(audience)?;
    let user = owner.user;
    // The count, the clock, the row and the ledger in one transaction: a
    // forget that is not kept leaves no hole in the server's count.
    let txn = db.begin().await?;
    lock(&txn, user, audience).await?;
    let hlc = next_rev(&txn).await?;
    let dseq = next_dseq(&txn, user, audience).await?;
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
        to: None,
        takes: Vec::new(),
    };
    row.check()?;
    let before = ledger(&txn, user, audience).await?;
    let recorded = forget::record(forget::RecordInput {
        rows: vec![row.clone()],
        ledger: before.clone(),
        holder: Some(owner.server()),
    })?;
    insert(&txn, user, &row, NODE).await?;
    apply_ledger(&txn, user, audience, &before, &recorded.ledger).await?;
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
