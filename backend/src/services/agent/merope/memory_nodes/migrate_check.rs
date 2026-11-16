//! The fold-back check of moving her memory (「折回核对」,
//! [`super::migrate`]): every audience an old topic went to is folded again
//! from its rows, and each old placement under a topic, of a memory still
//! held, is looked for as an entry on the node its topic went to. What is
//! not there, or is on another node, is reported with its memory's id;
//! nothing is mended here. The heads kept are compared with the fold too.

use std::collections::{BTreeMap, BTreeSet};

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde::Serialize;

use super::{Error, heads, migrate_topics::Moved, read};

/// Memory ids listed of each kind of mismatch, at most.
const LISTED: usize = 20;

/// What the check found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Checked {
    /// Old placements under a moved topic, of memories still held.
    pub expected: usize,
    /// On the node their topic went to.
    pub matched: usize,
    /// On another node of the audience only (put there before, by her
    /// nodes' own paths): how many, and the first of them.
    pub elsewhere: usize,
    pub elsewhere_ids: Vec<String>,
    /// On no node of the audience.
    pub missing: usize,
    pub missing_ids: Vec<String>,
    /// Entries on a moved topic's node not from that old topic (put there
    /// by her nodes' own paths).
    pub extra: usize,
    /// Heads that are not what their audience's rows fold into.
    pub heads_differ: usize,
}

impl Checked {
    /// Whether every old placement is on its node and every head is its
    /// fold.
    pub(crate) fn whole(&self) -> bool {
        self.matched == self.expected && self.heads_differ == 0
    }
}

/// Of an audience folded afresh: by memory id, the nodes it is an entry
/// on; and how many heads kept differ from the fold.
async fn folded<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
) -> Result<(BTreeMap<String, BTreeSet<String>>, usize), Error> {
    let folded = heads::fold_rows(&heads::rows_of(db, user_id, audience).await?)?;
    let mut on: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut differ = 0;
    for node in folded.nodes.iter().filter(|node| node.audience == audience) {
        let value = serde_json::to_value(node)
            .map_err(|_| Error::Refused("a folded node does not serialize".into()))?;
        if read::node(db, user_id, audience, &node.id).await?.as_ref() != Some(&value) {
            differ += 1;
        }
        for entry in read::entries_of(&value) {
            for memory in entry.memories {
                on.entry(memory).or_default().insert(node.id.clone());
            }
        }
    }
    Ok((on, differ))
}

/// The memories still held that `user_id` put under the old topic `topic`.
async fn under<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    topic: &str,
) -> Result<Vec<String>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id FROM agent_memory_topics p JOIN agent_memories m ON m.id = p.memory_id \
             WHERE p.user_id = $1 AND p.topic_id = $2 \
               AND m.invalid_at IS NULL AND m.superseded_by IS NULL ORDER BY m.id",
            [user_id.into(), topic.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| Ok(row.try_get::<String>("", "id")?))
        .collect()
}

/// Checks what `moved` says went where ([module docs](self)).
pub(crate) async fn check<C: ConnectionTrait>(db: &C, moved: &Moved) -> Result<Checked, Error> {
    let mut checked = Checked::default();
    let audiences: BTreeSet<(i32, String)> = moved
        .iter()
        .map(|((user_id, _), (audience, _))| (*user_id, audience.clone()))
        .collect();
    let mut found: BTreeMap<(i32, String), BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
    for (user_id, audience) in audiences {
        let (on, differ) = folded(db, user_id, &audience).await?;
        checked.heads_differ += differ;
        found.insert((user_id, audience), on);
    }
    let mut expected_on: BTreeMap<(i32, String, String), BTreeSet<String>> = BTreeMap::new();
    for ((user_id, topic), (audience, node)) in moved {
        let on = &found[&(*user_id, audience.clone())];
        for memory in under(db, *user_id, topic).await? {
            checked.expected += 1;
            expected_on
                .entry((*user_id, audience.clone(), node.clone()))
                .or_default()
                .insert(memory.clone());
            match on.get(&memory) {
                Some(nodes) if nodes.contains(node) => checked.matched += 1,
                Some(_) => {
                    checked.elsewhere += 1;
                    if checked.elsewhere_ids.len() < LISTED {
                        checked.elsewhere_ids.push(memory);
                    }
                }
                None => {
                    checked.missing += 1;
                    if checked.missing_ids.len() < LISTED {
                        checked.missing_ids.push(memory);
                    }
                }
            }
        }
    }
    for ((user_id, audience, node), memories) in &expected_on {
        let on = &found[&(*user_id, audience.clone())];
        checked.extra += on
            .iter()
            .filter(|(memory, nodes)| nodes.contains(node) && !memories.contains(*memory))
            .count();
    }
    if !checked.whole() {
        tracing::warn!(
            expected = checked.expected,
            matched = checked.matched,
            elsewhere = checked.elsewhere,
            missing = checked.missing,
            heads_differ = checked.heads_differ,
            "[memory-nodes] moving her memory: the moved rows do not fold back into the old placements"
        );
    }
    Ok(checked)
}
