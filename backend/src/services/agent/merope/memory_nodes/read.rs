//! Reading her nodes: an audience's heads, one node as the core folded it,
//! the entries on a node and the memory rows they name. Every read is of
//! one account and one audience: two never mix.

use sea_orm::{ConnectionTrait, DatabaseBackend, QueryResult, Statement};
use serde_json::Value;

use super::Error;

/// One node's head, as looked up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Head {
    pub node_id: String,
    pub node_key: Option<String>,
    pub kind: Option<String>,
    pub name: Option<String>,
    pub aliases: Vec<String>,
    /// `active`, `ongoing`, `resting`, `done`, `dropped`, or gone:
    /// `absorbed`, `forgotten`, `retracted`.
    pub status: String,
}

fn head_of(row: &QueryResult) -> Result<Head, Error> {
    let aliases: Value = row.try_get("", "aliases")?;
    Ok(Head {
        node_id: row.try_get("", "node_id")?,
        node_key: row.try_get("", "node_key")?,
        kind: row.try_get("", "kind")?,
        name: row.try_get("", "name")?,
        aliases: serde_json::from_value(aliases).unwrap_or_default(),
        status: row.try_get("", "status")?,
    })
}

/// The heads of `user_id`'s `audience`, oldest node first; only those of
/// `status` when given.
pub(crate) async fn heads<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    status: Option<&str>,
) -> Result<Vec<Head>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT node_id, node_key, kind, name, aliases, status FROM agent_node_heads \
             WHERE user_id = $1 AND audience = $2 AND ($3::text IS NULL OR status = $3) \
             ORDER BY (node->>'createdAt')::bigint NULLS LAST, node_id",
            [
                user_id.into(),
                audience.into(),
                status.map(str::to_string).into(),
            ],
        ))
        .await?;
    rows.iter().map(head_of).collect()
}

/// One node of `user_id`'s `audience` as the core folded it.
pub(crate) async fn node<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    node_id: &str,
) -> Result<Option<Value>, Error> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT node FROM agent_node_heads \
             WHERE user_id = $1 AND audience = $2 AND node_id = $3",
            [user_id.into(), audience.into(), node_id.into()],
        ))
        .await?;
    Ok(row.map(|row| row.try_get("", "node")).transpose()?)
}

/// One entry on a node: its row, its text (the memory's), who said it,
/// where, and the memory rows it is the place of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NodeEntry {
    pub row: String,
    pub text: String,
    pub said_by: Vec<String>,
    pub audience: String,
    /// Every reference, sorted: `memory:<id>` and the originals.
    pub refs: Vec<String>,
    /// The memory rows it names, without `memory:`.
    pub memories: Vec<String>,
    pub quote_only: bool,
    /// The row's clock, Unix microseconds.
    pub at: i64,
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The entries of a folded node, in log order (those a merge brought in
/// too).
pub(crate) fn entries_of(node: &Value) -> Vec<NodeEntry> {
    let Some(entries) = node.get("entries").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .map(|entry| {
            let refs = strings(&entry["refs"]);
            NodeEntry {
                row: entry["row"].as_str().unwrap_or_default().to_string(),
                text: entry["text"].as_str().unwrap_or_default().to_string(),
                said_by: strings(&entry["saidBy"]),
                audience: entry["audience"].as_str().unwrap_or_default().to_string(),
                memories: refs
                    .iter()
                    .filter_map(|reference| {
                        reference
                            .strip_prefix(myriad_nodes::body::MEMORY_REF)
                            .map(str::to_string)
                    })
                    .collect(),
                refs,
                quote_only: entry["quoteOnly"].as_bool().unwrap_or(false),
                at: entry["at"].as_i64().unwrap_or_default(),
            }
        })
        .collect()
}

/// The entries on one node of `user_id`'s `audience`.
pub(crate) async fn entries<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    node_id: &str,
) -> Result<Vec<NodeEntry>, Error> {
    Ok(node(db, user_id, audience, node_id)
        .await?
        .map(|node| entries_of(&node))
        .unwrap_or_default())
}
