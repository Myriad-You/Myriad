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

/// A node a rule made that holds nothing and has no part standing: a
/// repository whose sessions' grants were all dropped. Its name stays for
/// the sessions of a later grant (a node forgotten whole never comes back
/// under its key), and it is shown nowhere: not recalled, not in what she
/// is told, not on the page.
fn empty_by_rule(node: &Node) -> bool {
    node.created_by == Some(myriad_persona_log::row::Author::Rule)
        && node.entries.is_empty()
        && node.spans.is_empty()
        && node.views.is_empty()
        && node.open.is_empty()
        && node.standing.is_empty()
        && node.intents.is_empty()
        && node.parts.is_none_or(|parts| parts.total == 0)
}

/// A node the rules made of the owner's coding-agent sessions before
/// sessions went to her inbox: a session as a task (`minted:s…`), a folder
/// (`minted:l…`) or a repository (`repo:`). A session is not a thing of
/// hers, so such a node is set aside: kept as it is, row for row, and
/// shown nowhere (not recalled, not offered to her upkeep, not on the
/// page) unless what to do with what is on it is decided another way. One
/// that holds a span stays: what was said there was placed on it, and is
/// not hidden with it.
fn set_aside(node: &Node) -> bool {
    node.created_by == Some(myriad_persona_log::row::Author::Rule)
        && node.spans.is_empty()
        && node.key.as_deref().is_some_and(|key| {
            minted_by_rule(key, "minted:s")
                || minted_by_rule(key, "minted:l")
                || key.starts_with("repo:")
        })
}

/// Whether `key` is `mark` and the 24 hex a rule mints after it (a
/// session's task `minted:s…`, a folder `minted:l…`), not a name of hers
/// that only starts the same (`minted:summer-trip`).
pub(crate) fn minted_by_rule(key: &str, mark: &str) -> bool {
    key.strip_prefix(mark).is_some_and(|token| {
        token.len() == 24
            && token
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

fn word<T: serde::Serialize>(value: &T) -> Option<String> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
}

/// Where a node stands, in one word: gone (`forgotten`, `retracted`,
/// `absorbed`), a bare name a rule made that holds nothing now (`dormant`),
/// one the rules made of a session, set aside (`set_aside`), or its status
/// (absent reads as `active`).
pub(crate) fn status_of(node: &Node) -> String {
    if node.forgotten {
        return "forgotten".into();
    }
    if empty_by_rule(node) {
        return "dormant".into();
    }
    if set_aside(node) {
        return "set_aside".into();
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
/// takes away any it no longer has. The node ids of those rewritten. What
/// rests on a memory they took back is written hidden
/// ([`super::hidden::folded`]): the heads are what she reads.
pub(crate) async fn refresh<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    folded: &Folded,
) -> Result<Vec<String>, Error> {
    let hidden = super::hidden::folded(db, user_id, audience, folded, true).await?;
    let folded = hidden.as_ref().unwrap_or(folded);
    let kept = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT node_id, node, status FROM agent_node_heads WHERE user_id = $1 AND audience = $2",
            [user_id.into(), audience.into()],
        ))
        .await?;
    // A head whose node is as it was and whose word for where it stands
    // is too stays: one whose word changed (how a word is said changed
    // here) is written again.
    let mut before: BTreeMap<String, (Value, String)> = BTreeMap::new();
    for row in kept {
        before.insert(
            row.try_get("", "node_id")?,
            (row.try_get("", "node")?, row.try_get("", "status")?),
        );
    }
    let now = crate::clock::now().fixed_offset();
    let mut changed = Vec::new();
    for node in folded.nodes.iter().filter(|node| node.audience == audience) {
        let value = serde_json::to_value(node)
            .map_err(|_| Error::Refused("a folded node does not serialize".into()))?;
        if before
            .remove(&node.id)
            .is_some_and(|(was, status)| was == value && status == status_of(node))
        {
            continue;
        }
        let aliases = node
            .aliases
            .as_ref()
            .map(|aliases| aliases.current.value.clone())
            .unwrap_or_default();
        let status = status_of(node);
        // Forgotten, taken back, folded into another or left empty: what
        // upkeep kept of it built to nothing goes with it.
        if matches!(
            status.as_str(),
            "forgotten" | "retracted" | "absorbed" | "dormant" | "set_aside"
        ) {
            super::unbuilt::went(db, user_id, audience, &node.id).await?;
        }
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
                status.into(),
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
        super::unbuilt::went(db, user_id, audience, &gone).await?;
        changed.push(gone);
    }
    changed.sort();
    Ok(changed)
}
