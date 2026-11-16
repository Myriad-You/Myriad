//! The site owner's private talk under the partner's one id (protocol 2,
//! b2). Her nodes there were made under the core's fixed name,
//! `personal:local-owner`. Once the account has its partner id
//! (`her_log::partner_id`, minted the first time an end asks), they move to
//! `personal:<partner id>`, the name her log and every other side use. The
//! move is the core's (`myriad_nodes::migrate`): the rows' envelopes and
//! bodies, `person:local-owner` keys included. Then the heads are folded
//! again under the new name, and what is kept beside the rows (placements,
//! inbox, instructions) takes the new name too. Node ids do not change: the
//! core makes them under the fixed name either way.
//!
//! A move runs in one transaction, under the locks of both names and of
//! her own audience, and running it again moves nothing. It runs at start,
//! when a link's welcome gives the partner id, and from any batch still
//! written under the old name: that batch moves first and is then written
//! under the new one ([`super::write`]). An account with no partner id yet
//! stays under the old name.
//!
//! Only the site owner's talk moves: any other account's private talk is
//! `personal:<user_id>`, and its node ids are made from that.

use std::collections::BTreeMap;

use myriad_nodes::Body;
use myriad_persona_log::partner::LEGACY;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::Value;

use super::audience::OWN;
use super::heads::{self, KeptRow};
use super::prepare::{self, RowIn};
use super::{Error, write};

/// The partner id the site owner's talk has moved to, once it has.
#[cfg(not(test))]
static MOVED: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

#[cfg(test)]
thread_local! {
    /// In tests, each test's own: they share the process, not a database.
    static MOVED: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The partner id the site owner's talk is under, if it has moved.
pub(crate) fn moved_to() -> Option<String> {
    #[cfg(not(test))]
    return MOVED.read().ok().and_then(|moved| moved.clone());
    #[cfg(test)]
    MOVED.with(|moved| moved.borrow().clone())
}

/// The talk is under `partner` from now on.
pub(crate) fn remember(partner: Option<&str>) {
    #[cfg(not(test))]
    if let Ok(mut moved) = MOVED.write() {
        *moved = partner.map(str::to_string);
    }
    #[cfg(test)]
    MOVED.with(|moved| *moved.borrow_mut() = partner.map(str::to_string));
}

/// The talk under the core's fixed name.
pub(crate) fn legacy_talk() -> String {
    format!("personal:{LEGACY}")
}

/// `user_id`'s partner id, if the site minted one (none is minted here).
async fn partner_of<C: ConnectionTrait>(db: &C, user_id: i32) -> Result<Option<String>, Error> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT partner_id FROM agent_log_partners WHERE user_id = $1",
            [user_id.into()],
        ))
        .await?;
    Ok(row.map(|row| row.try_get("", "partner_id")).transpose()?)
}

/// Moves the site owner's talk if the account has its partner id, and
/// remembers where it is. At start, and once a link gave the id.
pub(crate) async fn settle(db: &DatabaseConnection) -> Result<(), Error> {
    let Some(owner) = crate::services::principal::site_owner_id(db).await? else {
        return Ok(());
    };
    let Some(partner) = partner_of(db, owner).await? else {
        return Ok(());
    };
    if moved_to().as_deref() == Some(partner.as_str()) && !any_under_old(db, owner).await? {
        return Ok(());
    }
    let txn = db.begin().await?;
    let moved = move_talk(&txn, owner, &partner).await?;
    txn.commit().await?;
    remember(Some(&partner));
    if moved > 0 {
        tracing::info!(
            rows = moved,
            "[memory-nodes] the site owner's talk moved to the partner id"
        );
    }
    Ok(())
}

/// Settles now, and again each minute: another process of the site may
/// have been the one a link gave the id to.
pub(crate) async fn start(db: DatabaseConnection) {
    if let Err(error) = settle(&db).await {
        tracing::warn!(%error, "[memory-nodes] the site owner's talk could not move to the partner id");
    }
    let every = crate::services::jobs::Every::new(std::time::Duration::from_secs(60))
        .after(std::time::Duration::from_secs(60))
        .jitter(std::time::Duration::from_secs(5));
    crate::services::jobs::jobs().periodic("site owner's talk under the partner id", every, move || {
        let db = db.clone();
        async move {
            if let Err(error) = settle(&db).await {
                tracing::warn!(%error, "[memory-nodes] the site owner's talk could not move to the partner id");
            }
        }
    });
}

/// Whether `user_id`'s talk is under the partner id for good: the id is
/// there and no row is left under the old name. Until then her own rows
/// may still move (a person key in them), so none goes into her log
/// (`logged`).
pub(super) async fn settled<C: ConnectionTrait>(db: &C, user_id: i32) -> Result<bool, Error> {
    Ok(partner_of(db, user_id).await?.is_some() && !any_under_old(db, user_id).await?)
}

/// Whether any row of `user_id`'s is still under the old name.
async fn any_under_old<C: ConnectionTrait>(db: &C, user_id: i32) -> Result<bool, Error> {
    Ok(db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 AS one FROM agent_node_rows WHERE user_id = $1 AND audience = $2 LIMIT 1",
            [user_id.into(), legacy_talk().into()],
        ))
        .await?
        .is_some())
}

/// A batch sent under the old name, moved: the partner id, the new name,
/// and the batch's rows as the core moves them.
pub(super) struct Redirected {
    pub partner: String,
    pub audience: String,
    pub rows: Vec<RowIn>,
}

/// Where a batch for `user_id`'s `audience` goes, in `txn`: under the old
/// name once the partner id is there, the talk is moved first and the batch
/// goes under the new name. `None` when it goes where it was sent. The
/// caller remembers the partner id once its transaction is kept.
///
/// A talk that does not move (two rows on one place) stays under the old
/// name and the batch is written there: her memory does not stop for it.
/// The next [`settle`] says so again, for someone to look at.
pub(super) async fn redirect<C: ConnectionTrait>(
    txn: &C,
    user_id: i32,
    audience: &str,
    rows: &[RowIn],
) -> Result<Option<Redirected>, Error> {
    if audience != legacy_talk() {
        return Ok(None);
    }
    let Some(partner) = partner_of(txn, user_id).await? else {
        return Ok(None);
    };
    match move_talk(txn, user_id, &partner).await {
        Ok(_) => {}
        Err(Error::Refused(why)) => {
            tracing::warn!(
                why,
                "[memory-nodes] written under the old name: the talk does not move"
            );
            return Ok(None);
        }
        Err(error) => return Err(error),
    }
    Ok(Some(Redirected {
        audience: format!("personal:{partner}"),
        rows: moved_rows(rows, &partner)?,
        partner,
    }))
}

/// `rows`, written under the old name, as the core moves them.
fn moved_rows(rows: &[RowIn], partner: &str) -> Result<Vec<RowIn>, Error> {
    let from = legacy_talk();
    let mut envelopes = Vec::with_capacity(rows.len());
    let mut bodies = BTreeMap::new();
    for (at, row) in rows.iter().enumerate() {
        let prepared = prepare::prepare(&from, row)?;
        // Where it would stand does not matter to the move: only what it is.
        let hlc = myriad_persona_sync::Rev {
            wall_ms: 1,
            counter: at as u32,
            node: crate::services::her_log::NODE.to_string(),
        };
        let dseq = (prepared.replicate == myriad_persona_log::row::Replicate::Shared)
            .then_some(at as u64 + 1);
        envelopes.push(write::envelope(
            &prepared,
            &from,
            hlc,
            dseq,
            myriad_persona_log::lease::ALONE,
        )?);
        bodies.insert(row.id.clone(), row.body.clone());
    }
    let migrated = myriad_nodes::migrate::migrate(myriad_nodes::migrate::MigrateInput {
        rows: envelopes,
        bodies,
        ledger: Vec::new(),
        from: LEGACY.to_string(),
        to: partner.to_string(),
    })?;
    if !migrated.unread.is_empty() || !migrated.clashes.is_empty() {
        return Err(Error::Refused(
            "a row written under the old name does not move".into(),
        ));
    }
    Ok(rows
        .iter()
        .map(|row| RowIn {
            body: migrated
                .bodies
                .get(&row.id)
                .cloned()
                .unwrap_or_else(|| row.body.clone()),
            ..row.clone()
        })
        .collect())
}

/// Moves `user_id`'s talk under the old name to `partner`'s, with what is
/// kept beside it; the rows moved (none when nothing is left there).
pub(super) async fn move_talk<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    partner: &str,
) -> Result<usize, Error> {
    let from = legacy_talk();
    let to = format!("personal:{partner}");
    // Always in this order, as every writer locks one audience at a time.
    for audience in [OWN, from.as_str(), to.as_str()] {
        write::lock(db, user_id, audience).await?;
    }
    let old = heads::rows_of(db, user_id, &from).await?;
    if old.is_empty() {
        return Ok(0);
    }
    let mut kept = old;
    kept.extend(heads::rows_of(db, user_id, OWN).await?);
    kept.extend(heads::rows_of(db, user_id, &to).await?);
    let before: BTreeMap<String, KeptRow> = kept
        .iter()
        .map(|kept| (kept.row.id.clone(), kept.clone()))
        .collect();
    let migrated = myriad_nodes::migrate::migrate(myriad_nodes::migrate::MigrateInput {
        rows: kept.iter().map(|kept| kept.row.clone()).collect(),
        bodies: heads::bodies(&kept)?,
        ledger: Vec::new(),
        from: LEGACY.to_string(),
        to: partner.to_string(),
    })?;
    if !migrated.clashes.is_empty() {
        // Two rows on one place: nothing is moved, nothing lost.
        return Err(Error::Refused(
            "the site owner's talk does not move: two rows stand on one place".into(),
        ));
    }
    if !migrated.unread.is_empty() {
        tracing::warn!(
            rows = migrated.unread.len(),
            "[memory-nodes] rows whose body the core does not read moved as they are"
        );
    }
    let moved: BTreeMap<&str, &myriad_persona_log::Row> = migrated
        .rows
        .iter()
        .filter(|row| migrated.moved.contains(&row.id))
        .map(|row| (row.id.as_str(), row))
        .collect();
    for (id, row) in &moved {
        let body = match (migrated.bodies.get(*id), before.get(*id)) {
            (Some(value), Some(kept)) if kept.body.is_some() && !row.forgotten => {
                canonical(&row.kind, value)?
            }
            (_, Some(kept)) => kept.body.clone().unwrap_or_default(),
            (_, None) => continue,
        };
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_node_rows SET audience = $3, body = $4, envelope = $5 \
             WHERE user_id = $1 AND row_id = $2",
            [
                user_id.into(),
                (*id).into(),
                row.audience.clone().into(),
                body.into(),
                row.to_value().into(),
            ],
        ))
        .await?;
    }
    for table in [
        "agent_node_placements",
        "agent_node_inbox",
        "agent_node_instructions",
    ] {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &format!("UPDATE {table} SET audience = $3 WHERE user_id = $1 AND audience = $2"),
            [user_id.into(), from.clone().into(), to.clone().into()],
        ))
        .await?;
    }
    // The heads, folded again where the rows now are.
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM agent_node_heads WHERE user_id = $1 AND audience = $2",
        [user_id.into(), from.into()],
    ))
    .await?;
    for audience in [to.as_str(), OWN] {
        let rows = heads::rows_of(db, user_id, audience).await?;
        heads::refresh(db, user_id, audience, &heads::fold_rows(&rows)?).await?;
    }
    Ok(moved.len())
}

/// A moved body in the one byte form the site keeps.
fn canonical(kind: &str, value: &Value) -> Result<String, Error> {
    String::from_utf8(Body::read(kind, value)?.canonical_bytes())
        .map_err(|_| Error::Refused("a body's canonical bytes are not text".into()))
}
