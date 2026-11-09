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
//! **The other way** ([`take_in`]): rows of her nodes another end wrote,
//! once her log took them, are kept here as they came. Envelope and body
//! are as the core has them; a stub has no body; a forget is kept as one.
//! Then the audience is folded again. A row whose body has not come yet
//! waits for it. One the log has made a stub since (a forget, from here or
//! there) loses its body here too. Under the node lock first, then the
//! log's, as every writer here takes them.
//!
//! [`catch_up`] also runs on its own: wherever her records are compared
//! with her log (`her_log::state::sync`, after her lines, each minute,
//! before she leaves for another core). So everything is in the log before
//! a handover's `through` is read.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::Value;

use super::audience::OWN;
use super::{Error, heads, write};
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

/// The node a row of another end's is filed under: its object's, or for a
/// forget of one row, that row's node.
fn node_of(
    row: &myriad_persona_log::Row,
    nodes: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    if let Some(node) = row.object.strip_prefix("node:") {
        return Some(node.to_string());
    }
    let of = row
        .object
        .strip_prefix(myriad_persona_log::kinds::ROW)
        .and_then(|rest| rest.strip_prefix(':'))?;
    nodes.get(of).cloned()
}

/// Rows of her nodes another end wrote, in her log, kept here ([module
/// docs](self)): for each audience her log keeps of the site owner's, in
/// its own transaction. How many rows came.
pub(crate) async fn take_in(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    owner: &Owner,
) -> Result<usize, Error> {
    let mut came = 0;
    for audience in [owner.personal(), OWN.to_string()] {
        let txn = db.begin().await?;
        write::lock(&txn, owner.user, &audience).await?;
        // What the log made stubs of goes here too.
        let stubbed = txn
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE agent_node_rows n SET body = '', envelope = l.envelope \
                 FROM agent_log_rows l \
                 WHERE n.user_id = $1 AND n.audience = $2 AND l.user_id = n.user_id \
                   AND l.id = n.row_id AND l.forgotten AND n.body <> ''",
                [owner.user.into(), audience.clone().into()],
            ))
            .await?
            .rows_affected();
        let found = txn
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT l.envelope FROM agent_log_rows l \
                 WHERE l.user_id = $1 AND l.audience = $2 AND l.origin <> $3 \
                   AND (l.kind LIKE 'node.%' OR (l.kind = 'forget' \
                        AND (l.object LIKE 'node:%' OR l.object LIKE 'row:%'))) \
                   AND NOT EXISTS (SELECT 1 FROM agent_node_rows n \
                                   WHERE n.user_id = l.user_id AND n.row_id = l.id)",
                [owner.user.into(), audience.clone().into(), NODE.into()],
            ))
            .await?;
        let mut rows = Vec::with_capacity(found.len());
        for result in found {
            let envelope: Value = result.try_get("", "envelope")?;
            rows.push(
                serde_json::from_value::<myriad_persona_log::Row>(envelope)
                    .map_err(|_| Error::Refused("a row of her log does not read".into()))?,
            );
        }
        if rows.is_empty() && stubbed == 0 {
            continue;
        }
        myriad_persona_log::row::sort(&mut rows);
        let whole: Vec<&myriad_persona_log::Row> =
            rows.iter().filter(|row| !row.forgotten).collect();
        let bodies = her_log::store::bodies(&txn, root, owner.user, &audience, &whole).await?;
        let mut nodes: std::collections::BTreeMap<String, String> = txn
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT row_id, node_id FROM agent_node_rows WHERE user_id = $1 AND audience = $2",
                [owner.user.into(), audience.clone().into()],
            ))
            .await?
            .into_iter()
            .filter_map(|row| {
                Some((
                    row.try_get("", "row_id").ok()?,
                    row.try_get("", "node_id").ok()?,
                ))
            })
            .collect();
        for row in &rows {
            let body = match (&row.payload, row.forgotten, bodies.get(&row.id)) {
                (Some(_), false, Some(value)) => Some(value),
                // Its body has not come: kept once it has.
                (Some(_), false, None) => continue,
                _ => None,
            };
            let Some(node) = node_of(row, &nodes) else {
                continue;
            };
            let (text, said_by) = match body {
                Some(value) => {
                    // A body the core does not read here is left in the log.
                    let Some(text) = myriad_nodes::Body::read(&row.kind, value)
                        .ok()
                        .and_then(|body| String::from_utf8(body.canonical_bytes()).ok())
                    else {
                        continue;
                    };
                    let said_by = value
                        .get("saidBy")
                        .cloned()
                        .unwrap_or_else(|| Value::from(Vec::<Value>::new()));
                    (text, said_by)
                }
                None => (String::new(), Value::from(Vec::<Value>::new())),
            };
            let at = chrono::DateTime::from_timestamp_millis(row.hlc.wall_ms as i64)
                .ok_or_else(|| Error::Refused("a row's clock is out of range".into()))?
                .fixed_offset();
            let author = match row.author {
                myriad_persona_log::row::Author::Her => "her",
                myriad_persona_log::row::Author::Owner => "owner",
            };
            txn.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO agent_node_rows (user_id, audience, node_id, row_id, kind, body, \
                 said_by, author, at, hlc, origin, dseq, epoch, envelope) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) \
                 ON CONFLICT (user_id, row_id) DO NOTHING",
                [
                    owner.user.into(),
                    audience.clone().into(),
                    node.clone().into(),
                    row.id.clone().into(),
                    row.kind.clone().into(),
                    text.into(),
                    said_by.into(),
                    author.into(),
                    at.into(),
                    row.hlc.to_string().into(),
                    row.origin.clone().into(),
                    row.dseq.map(|dseq| dseq as i64).into(),
                    row.epoch.map(|epoch| epoch as i64).into(),
                    row.to_value().into(),
                ],
            ))
            .await?;
            nodes.insert(row.id.clone(), node);
            came += 1;
        }
        let kept = heads::rows_of(&txn, owner.user, &audience).await?;
        heads::refresh(&txn, owner.user, &audience, &heads::fold_rows(&kept)?).await?;
        txn.commit().await?;
    }
    Ok(came)
}
