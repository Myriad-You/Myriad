//! The one way rows get into her nodes on the site.
//!
//! A batch for one account and one audience, in one transaction:
//!
//! 1. every row checked by the core's rules ([`super::prepare`]), its body
//!    kept as the canonical bytes `Body::read` gives back;
//! 2. the audience locked (`pg_advisory_xact_lock` on account and audience),
//!    so two writers of one audience take turns and its count has no holes;
//! 3. a row already kept under its id is nothing new (the same row again)
//!    or refused (another row under that id, or in another audience);
//! 4. each new row gets its envelope: the server's one clock
//!    (`her_log::store::next_rev`, `agent_log_clock`, the clock her log's own
//!    rows take), the site as its side, a count in the audience for a
//!    shared row, and for her rows the lease epoch;
//! 5. the audience's rows folded again with the new ones; a new row the fold
//!    passes by (a tag that does not fit, the person's row not said by
//!    them…) refuses the batch; every head the fold changes is rewritten.
//!
//! Her rows need her pen (a [`Thought`]): while her mind is on another host
//! core the site writes no row of hers. The person's own rows do not. No
//! model is called in here; what a model decides is decided before.

use std::collections::BTreeSet;

use myriad_persona_log::{
    Row,
    lease::ALONE,
    row::{LOG_VERSION, Payload, Replicate},
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::Value;

use super::{
    Error,
    heads::{self, KeptRow},
    prepare::{Author, Prepared, RowIn, prepare_all},
};
use crate::services::her_log::{self, NODE, pen::Thought};

/// What a batch wrote: the new rows, and the nodes whose heads changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Written {
    pub rows: Vec<String>,
    pub heads: Vec<String>,
}

async fn lock<C: ConnectionTrait>(db: &C, user_id: i32, audience: &str) -> Result<(), Error> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended('myriad-memory-nodes:' || $1 || ':' || $2, 0))",
        [user_id.to_string().into(), audience.into()],
    ))
    .await?;
    Ok(())
}

/// A row kept, as `write` compares a new one with it.
struct Kept {
    audience: String,
    node: String,
    kind: String,
    body: Option<String>,
    author: String,
    row: Row,
}

/// The rows of `user_id`'s `audience`, and any other of the account's rows
/// under one of `ids`.
async fn kept<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    ids: Vec<String>,
) -> Result<Vec<Kept>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT audience, node_id, kind, body, author, envelope FROM agent_node_rows \
             WHERE user_id = $1 AND (audience = $2 OR row_id = ANY($3))",
            [user_id.into(), audience.into(), ids.into()],
        ))
        .await?;
    rows.into_iter()
        .map(|result| {
            let envelope: Value = result.try_get("", "envelope")?;
            let row: Row = serde_json::from_value(envelope)
                .map_err(|_| Error::Refused("a kept row does not read as a row".into()))?;
            let body: String = result.try_get("", "body")?;
            Ok(Kept {
                audience: result.try_get("", "audience")?,
                node: result.try_get("", "node_id")?,
                kind: result.try_get("", "kind")?,
                body: (!row.forgotten).then_some(body),
                author: result.try_get("", "author")?,
                row,
            })
        })
        .collect()
}

/// The epoch her rows are written under: the server's grant while her log
/// is on and the server holds her mind, else none ever held.
async fn epoch<C: ConnectionTrait>(db: &C) -> Result<u64, Error> {
    if !her_log::enabled() {
        return Ok(ALONE);
    }
    Ok(match her_log::lease::standing(db).await? {
        Some(grant) if grant.held_by(NODE) => grant.epoch,
        _ => ALONE,
    })
}

/// The site's last count of shared rows in the audience.
async fn last_count<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
) -> Result<u64, Error> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT COALESCE(MAX(dseq), 0) AS last FROM agent_node_rows \
             WHERE user_id = $1 AND audience = $2 AND origin = $3",
            [user_id.into(), audience.into(), NODE.into()],
        ))
        .await?;
    Ok(row
        .map(|row| row.try_get::<i64>("", "last"))
        .transpose()?
        .unwrap_or(0)
        .max(0) as u64)
}

fn envelope(
    prepared: &Prepared,
    audience: &str,
    hlc: myriad_persona_sync::Rev,
    dseq: Option<u64>,
    epoch: u64,
) -> Result<Row, Error> {
    let row = Row {
        version: LOG_VERSION,
        id: prepared.id.clone(),
        origin: NODE.to_string(),
        author: prepared.author.core(),
        epoch: (prepared.author == Author::Her).then_some(epoch),
        dseq,
        hlc,
        audience: audience.to_string(),
        subject: None,
        object: format!("node:{}", prepared.node),
        kind: prepared.kind.clone(),
        payload: Some(Payload {
            reference: format!("blob:{}", prepared.id),
            key: None,
        }),
        replicate: prepared.replicate,
        forgotten: false,
        to: prepared.to.clone(),
        takes: prepared.takes.clone(),
    };
    row.check()
        .map_err(|_| Error::Refused("a row's envelope does not check".into()))?;
    Ok(row)
}

async fn insert<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    prepared: &Prepared,
    row: &Row,
) -> Result<(), Error> {
    let at = chrono::DateTime::from_timestamp_millis(row.hlc.wall_ms as i64)
        .ok_or_else(|| Error::Refused("a row's clock is out of range".into()))?
        .fixed_offset();
    let said_by = serde_json::to_value(&prepared.said_by)
        .map_err(|_| Error::Refused("saidBy does not serialize".into()))?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_node_rows (user_id, audience, node_id, row_id, kind, body, said_by, \
         author, at, hlc, origin, dseq, epoch, envelope) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) \
         ON CONFLICT (user_id, row_id) DO NOTHING",
        [
            user_id.into(),
            row.audience.clone().into(),
            prepared.node.clone().into(),
            row.id.clone().into(),
            row.kind.clone().into(),
            prepared.body.clone().into(),
            said_by.into(),
            prepared.author.as_str().into(),
            at.into(),
            row.hlc.to_string().into(),
            row.origin.clone().into(),
            row.dseq.map(|dseq| dseq as i64).into(),
            row.epoch.map(|epoch| epoch as i64).into(),
            row.to_value().into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Writes `rows` into `user_id`'s `audience` ([module docs](self)). Her
/// rows need `pen`.
pub(crate) async fn write(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    rows: &[RowIn],
    pen: Option<&Thought>,
) -> Result<Written, Error> {
    if rows.is_empty() {
        return Ok(Written::default());
    }
    // An audience the core reads, where her own node may be.
    myriad_nodes::node_id(audience, "her")?;
    if pen.is_none() && rows.iter().any(|row| row.author == Author::Her) {
        return Err(Error::NoPen);
    }
    let batch = prepare_all(audience, rows)?;
    let txn = db.begin().await?;
    lock(&txn, user_id, audience).await?;
    let ids: Vec<String> = batch.iter().map(|row| row.id.clone()).collect();
    let kept = kept(&txn, user_id, audience, ids).await?;
    let mut fresh: Vec<&Prepared> = Vec::new();
    for prepared in &batch {
        match kept.iter().find(|kept| kept.row.id == prepared.id) {
            None => fresh.push(prepared),
            Some(kept) if kept.audience != audience => {
                return Err(Error::Refused(
                    "a row id is kept in another audience".into(),
                ));
            }
            Some(kept)
                if kept.body.as_deref().is_some_and(|body| {
                    prepared.same(&kept.node, &kept.kind, body, &kept.author)
                }) => {}
            Some(_) => return Err(Error::Refused("a row id already holds another row".into())),
        }
    }
    if fresh.is_empty() {
        txn.commit().await?;
        return Ok(Written::default());
    }
    let made: BTreeSet<&str> = kept
        .iter()
        .filter(|kept| kept.audience == audience && kept.kind == "node.created")
        .map(|kept| kept.node.as_str())
        .chain(
            fresh
                .iter()
                .filter(|row| row.created)
                .map(|row| row.node.as_str()),
        )
        .collect();
    if fresh.iter().any(|row| !made.contains(row.node.as_str())) {
        return Err(Error::Refused(
            "a row on a node no created row makes".into(),
        ));
    }
    let epoch = if fresh.iter().any(|row| row.author == Author::Her) {
        epoch(&txn).await?
    } else {
        ALONE
    };
    let mut count = last_count(&txn, user_id, audience).await?;
    let mut rows: Vec<KeptRow> = kept
        .into_iter()
        .filter(|kept| kept.audience == audience)
        .map(|kept| KeptRow {
            row: kept.row,
            body: kept.body,
        })
        .collect();
    let mut new = Vec::with_capacity(fresh.len());
    for prepared in &fresh {
        let hlc = her_log::store::next_rev(&txn).await?;
        let dseq = (prepared.replicate == Replicate::Shared).then(|| {
            count += 1;
            count
        });
        let row = envelope(prepared, audience, hlc, dseq, epoch)?;
        rows.push(KeptRow {
            row: row.clone(),
            body: Some(prepared.body.clone()),
        });
        new.push((*prepared, row));
    }
    let folded = heads::fold_rows(&rows)?;
    if let Some(skipped) = folded
        .skipped
        .iter()
        .find(|skipped| new.iter().any(|(_, row)| row.id == skipped.id))
    {
        let why = serde_json::to_value(skipped.why)
            .ok()
            .and_then(|why| why.as_str().map(str::to_string))
            .unwrap_or_default();
        return Err(Error::Refused(format!("the fold passes a row by: {why}")));
    }
    for (prepared, row) in &new {
        insert(&txn, user_id, prepared, row).await?;
    }
    let changed = heads::refresh(&txn, user_id, audience, &folded).await?;
    txn.commit().await?;
    Ok(Written {
        rows: new.iter().map(|(_, row)| row.id.clone()).collect(),
        heads: changed,
    })
}
