//! The nodes of an audience as placing, going over a stretch and sorting
//! see them: the live ones (not gone into another, forgotten or taken
//! back), each with its names and a little of what is on it, most recently
//! added to first. What is on a node is shown only while its memory is: an
//! entry whose memory was replaced, faded or let go is hidden, never shown
//! as if it still held.

use std::collections::{BTreeSet, HashMap, HashSet};

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::Value;

use super::{Error, read};

/// Entries shown of a node, newest first, at most.
const HOLDS_SHOWN: usize = 3;
/// Steps followed from a node gone into another to where it went.
const LANDING_STEPS: usize = 8;

/// A live node, as shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Known {
    pub id: String,
    pub key: Option<String>,
    /// `topic`, `task` or `person`.
    pub kind: Option<String>,
    pub name: String,
    pub aliases: Vec<String>,
    /// What is on it, newest first, at most [`HOLDS_SHOWN`].
    pub holds: Vec<String>,
    /// Its newest entry or span, Unix microseconds.
    latest: i64,
}

impl Known {
    pub(crate) fn is_topic(&self) -> bool {
        self.kind.as_deref() == Some("topic")
    }

    /// A person's own node (「关于你」, a group, her): never where a stretch
    /// or a kind of thing goes.
    pub(crate) fn is_person(&self) -> bool {
        self.kind.as_deref() == Some("person")
    }

    pub(crate) fn topic(&self) -> myriad_merope::topics::Topic<'_> {
        myriad_merope::topics::Topic {
            name: &self.name,
            aliases: &self.aliases,
            holds: &self.holds,
        }
    }
}

/// Whether a node of this status is still there.
pub(crate) fn is_live(status: &str) -> bool {
    !matches!(status, "absorbed" | "forgotten" | "retracted")
}

fn memory_of(entry: &Value) -> Vec<String> {
    entry["refs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|reference| {
            reference
                .as_str()?
                .strip_prefix(myriad_nodes::body::MEMORY_REF)
        })
        .map(str::to_string)
        .collect()
}

/// The memory rows among `ids` still held: not replaced, faded or let go.
pub(crate) async fn held<C: ConnectionTrait>(
    db: &C,
    ids: Vec<String>,
) -> Result<HashSet<String>, Error> {
    if ids.is_empty() {
        return Ok(HashSet::new());
    }
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM agent_memories \
             WHERE id = ANY($1) AND invalid_at IS NULL AND superseded_by IS NULL",
            [ids.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| Ok(row.try_get::<String>("", "id")?))
        .collect()
}

/// The day she learned each of the memory rows `ids`, Unix microseconds:
/// when it was kept, or when the first of the short notes settled into it
/// was taken ([`crate::services::agent::merope::chat_remember`]), which is
/// when it was said. Not when it was put on a node: an old memory put on
/// one today was still learned then.
pub(crate) async fn learned<C: ConnectionTrait>(
    db: &C,
    ids: Vec<String>,
) -> Result<HashMap<String, i64>, Error> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT h.id, LEAST(h.created_at, min(n.created_at)) AS at \
             FROM agent_memories h \
             LEFT JOIN agent_memories n ON n.superseded_by = h.id AND n.invalid_reason = 'settled' \
             WHERE h.id = ANY($1) GROUP BY h.id, h.created_at",
            [ids.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            let at: chrono::DateTime<chrono::FixedOffset> = row.try_get("", "at")?;
            Ok((row.try_get::<String>("", "id")?, at.timestamp_micros()))
        })
        .collect()
}

/// The live nodes of `user_id`'s `audience`, most recently added to first.
pub(crate) async fn known<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
) -> Result<Vec<Known>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT node_id, node_key, kind, name, aliases, status, node FROM agent_node_heads \
             WHERE user_id = $1 AND audience = $2 ORDER BY node_id",
            [user_id.into(), audience.into()],
        ))
        .await?;
    let mut nodes = Vec::new();
    for row in &rows {
        let status: String = row.try_get("", "status")?;
        let Some(name) = row.try_get::<Option<String>>("", "name")? else {
            continue;
        };
        if !is_live(&status) {
            continue;
        }
        let node: Value = row.try_get("", "node")?;
        let aliases: Value = row.try_get("", "aliases")?;
        nodes.push((
            row.try_get::<String>("", "node_id")?,
            row.try_get::<Option<String>>("", "node_key")?,
            row.try_get::<Option<String>>("", "kind")?,
            name,
            serde_json::from_value::<Vec<String>>(aliases).unwrap_or_default(),
            node,
        ));
    }
    let named: BTreeSet<String> = nodes
        .iter()
        .flat_map(|node| node.5["entries"].as_array().into_iter().flatten())
        .flat_map(memory_of)
        .collect();
    let held = held(db, named.into_iter().collect()).await?;
    let mut out: Vec<Known> = nodes
        .into_iter()
        .map(|(id, key, kind, name, aliases, node)| {
            let mut entries: Vec<&Value> = node["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|entry| {
                    let memories = memory_of(entry);
                    memories.is_empty() || memories.iter().any(|memory| held.contains(memory))
                })
                .collect();
            entries.sort_by_key(|entry| std::cmp::Reverse(entry["at"].as_i64().unwrap_or(0)));
            let spans = node["spans"].as_array().into_iter().flatten();
            let latest = entries
                .iter()
                .copied()
                .chain(spans)
                .filter_map(|item| item["at"].as_i64())
                .chain(node["createdAt"].as_i64())
                .max()
                .unwrap_or(0);
            Known {
                id,
                key,
                kind,
                name,
                aliases,
                holds: entries
                    .iter()
                    .take(HOLDS_SHOWN)
                    .filter_map(|entry| entry["text"].as_str().map(str::to_string))
                    .collect(),
                latest,
            }
        })
        .collect();
    out.sort_by(|a, b| b.latest.cmp(&a.latest).then_with(|| a.id.cmp(&b.id)));
    Ok(out)
}

/// Where `node` of `user_id`'s `audience` is now: itself, or the node it
/// went into (the last one, when that one went on); none when it is gone.
pub(crate) async fn landing<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    node: &str,
) -> Result<Option<String>, Error> {
    let mut at = node.to_string();
    for _ in 0..LANDING_STEPS {
        let Some(head) = read::node(db, user_id, audience, &at).await? else {
            return Ok(None);
        };
        let Some(into) = head["absorbedInto"]["into"].as_str() else {
            let gone = head["forgotten"].as_bool().unwrap_or(false)
                || head["retracted"]
                    .as_array()
                    .is_some_and(|retracted| !retracted.is_empty());
            return Ok((!gone).then_some(at));
        };
        at = if into.len() == 32 && into.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            into.to_string()
        } else {
            myriad_nodes::node_id(audience, into)?
        };
    }
    Ok(None)
}
