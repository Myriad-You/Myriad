//! An audience's heads: its rows folded (`myriad_nodes::fold`), one head
//! per node, kept beside the rows and rewritten in the same transaction.
//! A head holds the folded node as the core gives it, and a few of its
//! fields as columns to look nodes up by.

use std::collections::BTreeMap;

use myriad_nodes::{FoldInput, Folded, Node, fold};
use myriad_persona_log::Row;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::Value;

use super::Error;

/// A row as kept: its envelope, and its body unless it is a stub.
#[derive(Debug, Clone)]
pub(crate) struct KeptRow {
    pub row: Row,
    pub body: Option<String>,
}

/// The bodies of `rows` by row id, as the core takes them: none for a stub.
pub(crate) fn bodies(rows: &[KeptRow]) -> Result<BTreeMap<String, Value>, Error> {
    let mut bodies = BTreeMap::new();
    for kept in rows {
        // A row with no payload (a forget) has no body.
        if kept.row.payload.is_none() {
            continue;
        }
        if let (false, Some(body)) = (kept.row.forgotten, &kept.body) {
            let value: Value = serde_json::from_str(body)
                .map_err(|_| Error::Refused("a kept body does not read".into()))?;
            bodies.insert(kept.row.id.clone(), value);
        }
    }
    Ok(bodies)
}

/// The fold of `rows`. The site keeps no ledger of forgets for these rows
/// yet, so the ledger is empty; the forgets among the rows still count.
pub(crate) fn fold_rows(rows: &[KeptRow]) -> Result<Folded, Error> {
    Ok(fold(FoldInput {
        rows: rows.iter().map(|kept| kept.row.clone()).collect(),
        bodies: bodies(rows)?,
        ledger: Vec::new(),
    })?)
}

/// Every row of `user_id`'s `audience`, as kept (a stub without its body).
pub(crate) async fn rows_of<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
) -> Result<Vec<KeptRow>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT body, envelope FROM agent_node_rows WHERE user_id = $1 AND audience = $2",
            [user_id.into(), audience.into()],
        ))
        .await?;
    rows.into_iter()
        .map(|result| {
            let envelope: Value = result.try_get("", "envelope")?;
            let row: Row = serde_json::from_value(envelope)
                .map_err(|_| Error::Refused("a kept row does not read as a row".into()))?;
            let body: String = result.try_get("", "body")?;
            Ok(KeptRow {
                body: (!row.forgotten).then_some(body),
                row,
            })
        })
        .collect()
}

fn word<T: serde::Serialize>(value: &T) -> Option<String> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
}

/// Where a node stands, in one word: gone (`forgotten`, `retracted`,
/// `absorbed`), or its status (absent reads as `active`).
pub(crate) fn status_of(node: &Node) -> String {
    if node.forgotten {
        return "forgotten".into();
    }
    if !node.retracted.is_empty() {
        return "retracted".into();
    }
    if node.absorbed_into.is_some() {
        return "absorbed".into();
    }
    node.status
        .as_ref()
        .and_then(|status| word(&status.current.value))
        .unwrap_or_else(|| "active".into())
}

/// Rewrites every head of `user_id`'s `audience` that `folded` changes, and
/// takes away any it no longer has. The node ids of those rewritten.
pub(crate) async fn refresh<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    folded: &Folded,
) -> Result<Vec<String>, Error> {
    let kept = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT node_id, node FROM agent_node_heads WHERE user_id = $1 AND audience = $2",
            [user_id.into(), audience.into()],
        ))
        .await?;
    let mut before: BTreeMap<String, Value> = BTreeMap::new();
    for row in kept {
        before.insert(row.try_get("", "node_id")?, row.try_get("", "node")?);
    }
    let now = crate::clock::now().fixed_offset();
    let mut changed = Vec::new();
    for node in folded.nodes.iter().filter(|node| node.audience == audience) {
        let value = serde_json::to_value(node)
            .map_err(|_| Error::Refused("a folded node does not serialize".into()))?;
        if before.remove(&node.id).as_ref() == Some(&value) {
            continue;
        }
        let aliases = node
            .aliases
            .as_ref()
            .map(|aliases| aliases.current.value.clone())
            .unwrap_or_default();
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_node_heads \
             (user_id, audience, node_id, node_key, kind, name, aliases, status, node, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             ON CONFLICT (user_id, audience, node_id) DO UPDATE SET \
             node_key = EXCLUDED.node_key, kind = EXCLUDED.kind, name = EXCLUDED.name, \
             aliases = EXCLUDED.aliases, status = EXCLUDED.status, node = EXCLUDED.node, \
             updated_at = EXCLUDED.updated_at",
            [
                user_id.into(),
                audience.into(),
                node.id.clone().into(),
                node.key.clone().into(),
                node.kind
                    .as_ref()
                    .and_then(|kind| word(&kind.current.value.kind))
                    .into(),
                node.name
                    .as_ref()
                    .map(|name| name.current.value.clone())
                    .into(),
                Value::from(aliases).into(),
                status_of(node).into(),
                value.into(),
                now.into(),
            ],
        ))
        .await?;
        changed.push(node.id.clone());
    }
    // A node the fold no longer has (none, while nothing here forgets).
    for gone in before.into_keys() {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM agent_node_heads WHERE user_id = $1 AND audience = $2 AND node_id = $3",
            [user_id.into(), audience.into(), gone.clone().into()],
        ))
        .await?;
        changed.push(gone);
    }
    changed.sort();
    Ok(changed)
}
