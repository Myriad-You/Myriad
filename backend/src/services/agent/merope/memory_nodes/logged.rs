//! Her nodes in her log (protocol 2, b2). A node row is already a row of
//! her log: the core's envelope, the server's one clock, the site as its
//! side. Rows of the site owner's talk (`personal:<partner id>`) and of
//! her own (`own:`) also go into the log itself, in the transaction that
//! writes them, while the log is on and the owner syncs it. Then every
//! other side of hers gets them. No other account's talk and no group ever
//! goes in.
//!
//! **A row's count is the log's.** A count says where a row stands (its
//! audience, its side, the count), not which row it is: the id does that,
//! made without the count (`myriad_nodes::entries::row_id_from`). A row of
//! these audiences takes the log's next count (`her_log::store::own_count`)
//! when it goes into the log. Until it does, its count is the site's own,
//! one past the last node row (`write::last_count`). Nothing outside this
//! site has seen such a row: no node row went anywhere before it was in
//! the log. So before a new row goes in, the rows of its audience that are
//! not in yet go first, in the order they were written, each under its
//! new count ([`catch_up`]). That way the log's counts follow the rows'
//! order, and nothing written while the log was off is lost.
//!
//! [`catch_up`] also runs on its own: wherever her records are compared
//! with her log (`her_log::state::sync`, after her lines, each minute,
//! before she leaves for another core). So everything is in the log before
//! a handover's `through` is read.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::Value;

use super::audience::OWN;
use super::{Error, write};
use crate::services::her_log::{self, NODE, Owner, keys};

/// Whether `audience` is one her log keeps of the site owner's: her own,
/// or the talk under the partner id.
fn kept_in_log(audience: &str) -> bool {
    audience == OWN
        || audience
            .strip_prefix("personal:")
            .is_some_and(|member| myriad_persona_log::partner::check(member).is_ok())
}

/// `user_id`'s account as a side of her log, when rows of `audience` go
/// into it now: an audience it keeps, the log on, and the owner syncing it.
pub(super) async fn into_log<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
) -> Result<Option<Owner>, Error> {
    if !kept_in_log(audience)
        || !her_log::pen::in_play(db, &her_log::current_settings(), Some(user_id)).await?
    {
        return Ok(None);
    }
    // Her own rows wait until the talk has moved: a move rewrites them,
    // and a row in the log is never rewritten.
    if audience == OWN && !super::partner::settled(db, user_id).await? {
        return Ok(None);
    }
    Ok(Some(her_log::owner(db, user_id).await?))
}

/// Puts the site's rows of `audience` that are not in the log yet into it,
/// in the order they were written, each under the log's next count; under
/// the audience's lock, in the caller's transaction. How many went in.
pub(super) async fn catch_up<C: ConnectionTrait>(
    db: &C,
    root: &dyn keys::Root,
    owner: &Owner,
    audience: &str,
) -> Result<usize, Error> {
    let user_id = owner.user;
    write::lock(db, user_id, audience).await?;
    let found = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT r.row_id, r.body, r.envelope FROM agent_node_rows r \
             WHERE r.user_id = $1 AND r.audience = $2 AND r.origin = $3 AND r.dseq IS NOT NULL \
               AND NOT EXISTS (SELECT 1 FROM agent_log_rows l \
                               WHERE l.user_id = r.user_id AND l.id = r.row_id)",
            [user_id.into(), audience.into(), NODE.into()],
        ))
        .await?;
    let mut rows = Vec::with_capacity(found.len());
    for result in found {
        let envelope: Value = result.try_get("", "envelope")?;
        let row: myriad_persona_log::Row = serde_json::from_value(envelope)
            .map_err(|_| Error::Refused("a kept row does not read as a row".into()))?;
        let body: String = result.try_get("", "body")?;
        rows.push((row, body));
    }
    rows.sort_by(|(a, _), (b, _)| a.log_order(b));
    for (mut row, body) in rows.iter().cloned() {
        row.dseq = Some(her_log::store::own_count(db, user_id, audience).await?);
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_node_rows SET dseq = $3, envelope = $4 WHERE user_id = $1 AND row_id = $2",
            [
                user_id.into(),
                row.id.clone().into(),
                row.dseq.map(|dseq| dseq as i64).into(),
                row.to_value().into(),
            ],
        ))
        .await?;
        let body = (!body.is_empty()).then_some(body.as_bytes());
        her_log::store::keep_own(db, root, owner, &row, body).await?;
    }
    Ok(rows.len())
}

/// [`catch_up`] for each audience of the site owner's her log keeps, each in
/// its own transaction: wherever her records are compared with her log.
pub(crate) async fn catch_up_all(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    owner: &Owner,
) -> Result<usize, Error> {
    let mut put = 0;
    for audience in [owner.personal(), OWN.to_string()] {
        let txn = db.begin().await?;
        if into_log(&txn, owner.user, &audience).await?.is_none() {
            continue;
        }
        // A count is no part of a folded node: the heads stay as they are.
        let went = catch_up(&txn, root, owner, &audience).await?;
        txn.commit().await?;
        put += went;
    }
    Ok(put)
}
